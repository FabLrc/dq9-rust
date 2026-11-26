//! `dq9re save`: summary of a DS save file, and its write path (`docs/re/save.md`). Needs no
//! ROM; the ROM only adds character names to the summary.

use core::ops::Range;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use dq9_formats::FontId;
use dq9_formats::save::{EquipSlot, SLOT_SIZE, Save, Slot, SlotMut, Warp};
use nds_rom::Rom;

use crate::font;

/// The three stored checksums inside one slot: every other changed byte is data.
const CHECKSUMS: [(usize, usize); 3] = [(16, 20), (36, 40), (132, 136)];

pub fn run(
    path: &Path,
    verbose: bool,
    rom: Option<&Path>,
    sets: &[String],
    out: Option<&Path>,
) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut save = Save::parse(&bytes).context("parsing save")?;
    let rom_path = rom
        .map(Path::to_path_buf)
        .or_else(|| crate::find_rom().ok());
    let rom_bytes = rom_path.and_then(|p| fs::read(p).ok());
    let rom = rom_bytes.as_deref().and_then(|b| Rom::parse(b).ok());
    let font = rom
        .as_ref()
        .and_then(|r| font::load(r, FontId::Message).ok());
    println!("magic: {}", save.has_magic());
    println!("current slot: {:?}", save.current_slot());
    for index in 0..dq9_formats::save::SLOT_COUNT {
        let slot = save.slot(index).expect("index below SLOT_COUNT");
        println!("\nslot {index}");
        println!(
            "  status: {:?}  version: {}  exists: {}  intermission: {}",
            slot.status(),
            String::from_utf8_lossy(slot.version()),
            slot.exists(),
            slot.is_intermission()
        );
        for c in slot.checksums() {
            println!(
                "  checksum {:?}: stored {:08x} computed {:08x} {}",
                c.which,
                c.stored,
                c.computed,
                if c.is_valid() { "ok" } else { "MISMATCH" }
            );
        }
        if slot.exists() {
            summary(&slot, verbose, font.as_ref());
        }
    }
    if sets.is_empty() && out.is_none() {
        return Ok(());
    }
    let Some(out) = out else {
        bail!("`--set` needs `--out`: the input file is never rewritten in place");
    };
    if sets.is_empty() {
        fs::write(out, save.as_bytes()).with_context(|| format!("writing {}", out.display()))?;
        println!(
            "\nno field set: wrote a byte-identical copy ({} bytes) to {}",
            save.as_bytes().len(),
            out.display()
        );
        return Ok(());
    }
    let slot_index = save
        .current_slot()
        .context("no slot with a valid header and checksums to edit")?;
    let mut edits = Vec::with_capacity(sets.len());
    {
        let mut slot = save
            .slot_mut(slot_index)
            .with_context(|| format!("slot {slot_index} out of range"))?;
        for spec in sets {
            edits.push(set_field(&mut slot, spec)?);
        }
        // The setters leave the checksums stale (`SlotMut::seal`'s contract): the game
        // compares them on load, so a written image is always sealed.
        slot.seal();
    }
    // `Save_WriteStep` state 2 writes the normal save twice: slot 0, then the same image at
    // `0x8010`. The first 16 bytes of slot 1 hold nothing and stay as they are.
    let mirrored = slot_index == 0 && save.mirror_normal_slot();
    let (data, checksums) = count_diff(&bytes, save.as_bytes());
    println!("\nedits on slot {slot_index}:");
    for edit in &edits {
        println!("  {edit}");
    }
    println!("  checksums recomputed (seal)");
    if mirrored {
        println!("  slot 1 rewritten as the mirror copy (0x6fe4 bytes at 0x8010)");
    }
    println!(
        "  {} bytes differ from the input: {data} data, {checksums} checksum",
        data + checksums
    );
    fs::write(out, save.as_bytes()).with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(())
}

fn summary(slot: &Slot<'_>, verbose: bool, font: Option<&dq9_formats::Font<'_>>) {
    let (h, m, s) = slot.play_time();
    println!(
        "  chapter {} step {} substep {}  play time: {h}:{m:02}:{s:02}  area: {}",
        slot.chapter(),
        slot.scenario_step(),
        slot.scenario_substep(),
        slot.area()
    );
    let ev = slot.event_flags();
    let set = ev
        .global_flags()
        .iter()
        .map(|b| b.count_ones())
        .sum::<u32>();
    println!(
        "  event: track {}  global flags set: {set}",
        ev.current_track()
    );
    println!(
        "  gold: {} (bank {})  mini medals: {}",
        slot.gold_on_hand(),
        slot.gold_in_bank(),
        slot.mini_medals()
    );
    println!(
        "  standby: {}  party: {} order {:?}",
        slot.standby_count(),
        slot.party_count(),
        slot.party_order()
    );
    println!(
        "  visited {:#010x}  guests {:#010x}  tricks {:#010x}  vocations {:#06x}",
        slot.visited_locations(),
        slot.special_guests(),
        slot.party_tricks(),
        slot.unlocked_vocations()
    );
    let used = usize::from(slot.standby_count()) + usize::from(slot.party_count());
    for i in 0..used.min(dq9_formats::save::CHARACTER_COUNT) {
        let Some(c) = slot.character(i) else { break };
        let v = usize::from(c.vocation());
        let name = font.map_or_else(|| format!("{:02x?}", c.name_raw()), |f| c.name(f));
        println!(
            "  character {i}: vocation {v} level {} exp {} name {name}",
            c.level(v).unwrap_or(0),
            c.experience(v).unwrap_or(0),
        );
        if verbose {
            let levels: Vec<u8> = (0..13).filter_map(|v| c.level(v)).collect();
            println!("    levels {levels:?}");
            let gear: Vec<String> = EquipSlot::ALL
                .iter()
                .map(|&s| format!("{s:?}={}", c.equipment(s)))
                .collect();
            println!("    {}", gear.join(" "));
            println!(
                "    face {} hair {} colors gender {} skin {} eyes {} hair {} body {:?}",
                c.face(),
                c.hair_style(),
                c.gender(),
                c.skin_color(),
                c.eye_color(),
                c.hair_color(),
                c.body()
            );
        }
    }
}

/// Applies one `name=value` (or bare `seal`) and describes it.
fn set_field(slot: &mut SlotMut<'_>, spec: &str) -> Result<String> {
    let (name, value) = match spec.split_once('=') {
        Some((name, value)) => (name.trim(), value.trim()),
        None => (spec.trim(), ""),
    };
    if name == "seal" && value.is_empty() {
        slot.seal();
        return Ok("seal (checksums recomputed)".to_string());
    }
    let range = match name {
        "gold" | "gold_on_hand" => slot.set_gold_on_hand(unsigned(name, value)?),
        "gold_in_bank" | "bank" => slot.set_gold_in_bank(unsigned(name, value)?),
        "mini_medals" | "medals" => slot.set_mini_medals(unsigned(name, value)?),
        "day_timer" => slot.set_day_timer(
            value
                .parse::<f32>()
                .with_context(|| format!("`{name}` = `{value}` is not a number"))?,
        ),
        "play_time" => {
            let (hours, minutes, seconds) = three(name, value)?;
            slot.set_play_time(hours, minutes, seconds)
        }
        "area" => slot.set_area(unsigned(name, value)?),
        "exists" => slot.set_exists(boolean(name, value)?),
        "intermission" => slot.set_intermission(boolean(name, value)?),
        "warp" => slot.set_warp(warp(name, value)?),
        "chapter" => slot.set_chapter(unsigned(name, value)?),
        "scenario_step" => slot.set_scenario_step(unsigned(name, value)?),
        "scenario_substep" => slot.set_scenario_substep(unsigned(name, value)?),
        "standby_count" => slot.set_standby_count(unsigned(name, value)?),
        "party_count" => slot.set_party_count(unsigned(name, value)?),
        "party_order" => slot.set_party_order(&bytes(name, value)?),
        "visited_locations" => slot.set_visited_locations(unsigned(name, value)?),
        "special_guests" => slot.set_special_guests(unsigned(name, value)?),
        "party_tricks" => slot.set_party_tricks(unsigned(name, value)?),
        "unlocked_vocations" => slot.set_unlocked_vocations(unsigned(name, value)?),
        "current_track" => slot
            .event_flags_mut()
            .set_current_track(unsigned(name, value)?),
        _ => {
            if let Some(index) = name
                .strip_prefix("global_flag.")
                .or_else(|| name.strip_prefix("story_flag."))
            {
                let story = name.starts_with("story_");
                slot.event_flags_mut()
                    .set_global_flag(unsigned(name, index)?, boolean(name, value)?, story)
                    .ok_or_else(|| anyhow!("`{name}`: flag index {index} is out of the array"))?
            } else if let Some(rest) = name.strip_prefix("held_items.") {
                let member: usize = unsigned("member", rest)?;
                let items: Vec<u16> = bytes_of(name, value)?;
                let items: [u16; 8] = items
                    .try_into()
                    .map_err(|_| anyhow!("`{name}`: 8 item words expected"))?;
                slot.set_held_items(member, &items)
                    .ok_or_else(|| anyhow!("`{name}`: party member {member} is past 3"))?
            } else if let Some(rest) = name.strip_prefix("track.") {
                edit_track(slot, name, rest, value)?
            } else if let Some(rest) = name.strip_prefix("character.") {
                edit_character(slot, name, rest, value)?
            } else {
                bail!("unknown field `{name}`: `docs/re/save.md` § Writing lists them");
            }
        }
    };
    Ok(format!(
        "{name} = {value}: bytes {}..{}",
        range.start, range.end
    ))
}

fn edit_track(slot: &mut SlotMut<'_>, name: &str, rest: &str, value: &str) -> Result<Range<usize>> {
    let (index, field) = rest
        .split_once('.')
        .with_context(|| format!("`{name}`: write `track.<index>.<field>`"))?;
    let index: usize = unsigned("track index", index)?;
    let mut events = slot.event_flags_mut();
    let mut track = events
        .track_mut(index)
        .ok_or_else(|| anyhow!("`{name}`: track {index} is past 4"))?;
    Ok(match field {
        "chapter" => track.set_chapter(unsigned(name, value)?),
        "step" => track.set_step(unsigned(name, value)?),
        "substep" => track.set_substep(unsigned(name, value)?),
        "chapter_flags" => track.set_chapter_flags(&bytes(name, value)?),
        "step_flags" => track.set_step_flags(&bytes(name, value)?),
        _ => bail!("`{name}`: unknown track field `{field}`"),
    })
}

fn edit_character(
    slot: &mut SlotMut<'_>,
    name: &str,
    rest: &str,
    value: &str,
) -> Result<Range<usize>> {
    let mut parts = rest.split('.');
    let index: usize = unsigned("character index", parts.next().unwrap_or(""))?;
    let field = parts
        .next()
        .with_context(|| format!("`{name}`: write `character.<index>.<field>`"))?;
    let argument = parts.next();
    let mut c = slot
        .character_mut(index)
        .ok_or_else(|| anyhow!("`{name}`: character {index} is past 12"))?;
    let vocation = || -> Result<usize> {
        let text = argument.with_context(|| format!("`{name}`: a vocation index is needed"))?;
        unsigned("vocation", text)
    };
    Ok(match field {
        "name" => c.set_name_raw(&bytes(name, value)?),
        "vocation" => c.set_vocation(unsigned(name, value)?),
        "level" => c
            .set_level(vocation()?, unsigned(name, value)?)
            .context("vocation out of range")?,
        "revocations" => c
            .set_revocations(vocation()?, unsigned(name, value)?)
            .context("vocation out of range")?,
        "experience" => c
            .set_experience(vocation()?, unsigned(name, value)?)
            .context("vocation out of range")?,
        "seed" => {
            let seed = parts
                .next()
                .with_context(|| format!("`{name}`: a seed index is needed"))?;
            c.set_seed(vocation()?, unsigned("seed", seed)?, unsigned(name, value)?)
                .context("vocation or seed out of range")?
        }
        "skill_points" => c.set_unallocated_skill_points(unsigned(name, value)?),
        "proficiency_flags" => c.set_proficiency_flags(&bytes(name, value)?),
        "equipment" => {
            let slot = argument
                .with_context(|| format!("`{name}`: an equipment slot is needed"))?
                .to_ascii_lowercase();
            let slot = EquipSlot::ALL
                .into_iter()
                .find(|candidate| format!("{candidate:?}").to_ascii_lowercase() == slot)
                .ok_or_else(|| anyhow!("`{name}`: unknown equipment slot"))?;
            c.set_equipment(slot, unsigned(name, value)?)
        }
        "face" => c.set_face(unsigned(name, value)?),
        "face_item" => c.set_face_item(unsigned(name, value)?),
        "hair_style" => c.set_hair_style(unsigned(name, value)?),
        "hair_item" => c.set_hair_item(unsigned(name, value)?),
        "gender" => c.set_gender(unsigned(name, value)?),
        "skin_color" => c.set_skin_color(unsigned(name, value)?),
        "eye_color" => c.set_eye_color(unsigned(name, value)?),
        "hair_color" => c.set_hair_color(unsigned(name, value)?),
        "body" => {
            let body = bytes_of(name, value)?;
            let [width, height] = body
                .as_slice()
                .try_into()
                .map_err(|_| anyhow!("`{name}`: `width,height` expected"))?;
            c.set_body(width, height)
        }
        _ => bail!("`{name}`: unknown character field `{field}`"),
    })
}

/// Bytes that differ between the input and the written image, data and checksums apart.
fn count_diff(before: &[u8], after: &[u8]) -> (usize, usize) {
    let (mut data, mut checksums) = (0, 0);
    for (at, (&a, &b)) in before.iter().zip(after).enumerate() {
        if a == b {
            continue;
        }
        let inside = at % SLOT_SIZE;
        if CHECKSUMS
            .iter()
            .any(|&(start, end)| (start..end).contains(&inside))
        {
            checksums += 1;
        } else {
            data += 1;
        }
    }
    (data, checksums)
}

/// Decimal (with sign) or `0x` hex.
fn parse_int(name: &str, value: &str) -> Result<i64> {
    let text = value.trim();
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16),
        None => text.parse::<i64>(),
    };
    parsed.with_context(|| format!("`{name}`: `{value}` is not a number"))
}

fn unsigned<T: TryFrom<i64>>(name: &str, value: &str) -> Result<T> {
    let parsed = parse_int(name, value)?;
    T::try_from(parsed).map_err(|_| anyhow!("`{name}` = `{value}` is out of range"))
}

fn boolean(name: &str, value: &str) -> Result<bool> {
    match value {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => bail!("`{name}` = `{value}`: 0 or 1 expected"),
    }
}

/// Comma-separated byte glyph/item values.
fn bytes(name: &str, value: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        out.push(unsigned(name, part)?);
    }
    Ok(out)
}

/// Comma-separated 16-bit values.
fn bytes_of(name: &str, value: &str) -> Result<Vec<u16>> {
    let mut out = Vec::new();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        out.push(unsigned(name, part)?);
    }
    Ok(out)
}

/// `h:m:s`, as `Slot::play_time` reads it.
fn three(name: &str, value: &str) -> Result<(u16, u8, u8)> {
    let (h, m, s) = value
        .split_once(':')
        .and_then(|(h, rest)| rest.split_once(':').map(|(m, s)| (h, m, s)))
        .with_context(|| format!("`{name}`: `h:m:s` expected"))?;
    Ok((unsigned(name, h)?, unsigned(name, m)?, unsigned(name, s)?))
}

/// `map:angle:x:y:z`.
fn warp(name: &str, value: &str) -> Result<Warp> {
    let parts: Vec<&str> = value.split(':').map(str::trim).collect();
    let [map, angle, x, y, z]: [&str; 5] = parts
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("`{name}`: `map:angle:x:y:z` expected"))?;
    Ok(Warp {
        map: unsigned(name, map)?,
        angle: unsigned(name, angle)?,
        position: [unsigned(name, x)?, unsigned(name, y)?, unsigned(name, z)?],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::save::{FILE_SIZE, MAGIC};

    /// A sealed synthetic image: magic, slot 0 present, every field zero.
    fn image() -> Vec<u8> {
        let mut data = vec![0u8; FILE_SIZE];
        data[..MAGIC.len()].copy_from_slice(MAGIC);
        let mut save = Save::parse(&data).unwrap();
        {
            let mut slot = save.slot_mut(0).unwrap();
            slot.set_exists(true);
            slot.seal();
        }
        save.as_bytes().to_vec()
    }

    /// Every accepted name, with the byte range it must write: the offsets of
    /// `docs/re/save.md` and `crates/dq9-formats/src/save.rs`, stated independently here.
    const CASES: &[(&str, usize, usize)] = &[
        ("gold=1", 11448, 11452),
        ("gold_on_hand=1", 11448, 11452),
        ("gold_in_bank=1", 11452, 11456),
        ("bank=1", 11452, 11456),
        ("mini_medals=1", 11460, 11464),
        ("medals=1", 11460, 11464),
        ("play_time=1:2:3", 16024, 16028),
        ("day_timer=1.5", 32, 36),
        ("area=109", 11476, 11478),
        ("exists=1", 21, 22),
        ("intermission=0", 20, 21),
        ("warp=1:2:3:4:5", 27104, 27120),
        ("chapter=1", 11568, 11572),
        ("scenario_step=1", 11572, 11576),
        ("scenario_substep=1", 11576, 11580),
        ("standby_count=1", 7572, 7573),
        ("party_count=1", 7577, 7578),
        ("party_order=1,2", 7573, 7575),
        ("visited_locations=1", 11788, 11792),
        ("special_guests=1", 11528, 11532),
        ("party_tricks=1", 12108, 12112),
        ("unlocked_vocations=1", 12276, 12278),
        ("current_track=1", 12402, 12403),
        ("global_flag.9=1", 11725, 11726),
        ("story_flag.0x400=1", 12075, 12076),
        ("track.2.chapter=1", 11640, 11641),
        ("track.2.step=1", 11641, 11642),
        ("track.2.substep=1", 11642, 11643),
        ("track.2.chapter_flags=1,2", 11643, 11645),
        ("track.2.step_flags=1,2", 11656, 11658),
        ("held_items.0=1,2,3,4,5,6,7,8", 7578, 7594),
        ("character.0.name=1,2", 456, 466),
        ("character.0.vocation=1", 216, 217),
        ("character.0.level.5=1", 143, 144),
        ("character.0.revocations.5=1", 156, 157),
        ("character.0.experience.5=1", 184, 188),
        ("character.0.seed.5.4=1", 288, 292),
        ("character.0.skill_points=1", 380, 382),
        ("character.0.proficiency_flags=1,2", 418, 420),
        ("character.0.equipment.head=1", 500, 502),
        ("character.0.equipment.torso=1", 488, 490),
        ("character.0.face=1", 492, 493),
        ("character.0.face_item=9020", 492, 494),
        ("character.0.hair_style=1", 494, 495),
        ("character.0.hair_item=9000", 494, 496),
        ("character.0.gender=1", 508, 509),
        ("character.0.skin_color=1", 508, 509),
        ("character.0.eye_color=1", 508, 509),
        ("character.0.hair_color=1", 509, 510),
        ("character.0.body=1,2", 512, 516),
        (
            "character.12.name=1",
            136 + 12 * 572 + 320,
            136 + 12 * 572 + 330,
        ),
        ("party_order=1,2,3,4", 7573, 7577),
        ("held_items.3=0,0,0,0,0,0,0,0", 7632, 7648),
    ];

    #[test]
    fn every_name_writes_its_documented_range() {
        let mut save = Save::parse(&image()).unwrap();
        for (spec, start, end) in CASES {
            let mut slot = save.slot_mut(0).unwrap();
            let line = set_field(&mut slot, spec).unwrap();
            assert!(
                line.ends_with(&format!("bytes {start}..{end}")),
                "`{spec}`: {line}, expected {start}..{end}"
            );
        }
    }

    #[test]
    fn unknown_names_and_bad_values_are_refused() {
        let mut save = Save::parse(&image()).unwrap();
        let mut slot = save.slot_mut(0).unwrap();
        for spec in [
            "bogus=1",
            "character.13.name=1",
            "character.0.nope=1",
            "character.0.level.13=1",
            "character.0.seed.0.9=1",
            "character.0.equipment.hat=1",
            "track.5.chapter=1",
            "track.0.chapter=1.5",
            "held_items.4=0,0,0,0,0,0,0,0",
            "held_items.0=1,2",
            "global_flag.9999=1",
            "party_count=300",
            "gold=0xzz",
        ] {
            assert!(set_field(&mut slot, spec).is_err(), "`{spec}` accepted");
        }
    }

    #[test]
    fn seal_alone_changes_nothing() {
        let bytes = image();
        let mut save = Save::parse(&bytes).unwrap();
        assert!(set_field(&mut save.slot_mut(0).unwrap(), "seal").is_ok());
        assert_eq!(save.as_bytes(), &bytes[..]);
    }

    #[test]
    fn a_set_leaves_the_checksums_stale_until_the_caller_seals() {
        let mut save = Save::parse(&image()).unwrap();
        set_field(&mut save.slot_mut(0).unwrap(), "gold=1234").unwrap();
        assert!(
            !save.slot(0).unwrap().checksums()[2].is_valid(),
            "B is stale"
        );
        save.slot_mut(0).unwrap().seal();
        assert!(
            save.slot(0)
                .unwrap()
                .checksums()
                .iter()
                .all(|c| c.is_valid())
        );
    }
}
