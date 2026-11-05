//! `dq9re chr` and `dq9re cast`: what a character archive holds, and who stands where on a
//! map for a story point (`dq9_formats::chr`, `dq9_formats::cast`,
//! `docs/re/characters.md`), optionally compared with the NPC table of a melonDS state.

use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use dq9_formats::cast::{self, Appearance, Cast, Npc, Story};
use dq9_formats::chr::{self, Chr, Role};
use dq9_formats::maplist::{self, MapList, MapRecord};
use dq9_formats::melonds::{NpcBody, NpcEntry, State};
use dq9_formats::{Gpc2, Table, map};
use nds_rom::{Narc, Rom, compress};

#[derive(clap::Args)]
pub struct ChrArgs {
    /// Archive inside the ROM, e.g. `data/chara_sub/s017.chr`.
    path: String,
}

#[derive(clap::Args)]
pub struct CastArgs {
    /// Map code (`M01`, `C01M09`) or id (`1100`); defaults to the current map of `--state`.
    map: Option<String>,
    /// Story point (track progress): chapter, step, substep. Default: the prologue, 1.1.1.
    #[arg(long, default_value_t = 1)]
    chapter: u8,
    #[arg(long, default_value_t = 1)]
    step: u8,
    #[arg(long, default_value_t = 1)]
    substep: u8,
    /// Night (`Save_IsDaytime` false); day by default.
    #[arg(long)]
    night: bool,
    /// Global flag set (repeatable).
    #[arg(long)]
    flag: Vec<i32>,
    /// Quest nibble, `QUEST=NIBBLE` (state 1 offered, 2 taken, 3 done; repeatable).
    #[arg(long, value_parser = parse_quest)]
    quest: Vec<(i32, u8)>,
    /// melonDS state: its story point, flags, quests and map replace the options, and its
    /// NPC table is compared with the cast.
    #[arg(long)]
    state: Option<PathBuf>,
}

fn parse_quest(s: &str) -> Result<(i32, u8)> {
    let (q, n) = s.split_once('=').context("expected QUEST=NIBBLE")?;
    Ok((q.parse()?, n.parse()?))
}

fn fx(v: i32) -> String {
    format!("{:.3}", f64::from(v) / 4096.0)
}

fn text(s: &[u8]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

pub fn run_chr(rom: &Rom, args: &ChrArgs) -> Result<()> {
    let data = map::rom_file(rom, &args.path).with_context(|| format!("no {}", args.path))?;
    let chr = Chr::parse(data).context("parsing character archive")?;
    println!("{}: {} members", args.path, chr.narc.len());
    for i in 0..u16::try_from(chr.narc.len())? {
        let name = chr.narc.name(i).unwrap_or("?");
        let role = match chr::role(name) {
            Role::Model if chr.model == Some(i) => "model".to_string(),
            Role::Model => "model (unused: not the first)".to_string(),
            Role::MotionTable if chr.motion_table.as_ref().is_some_and(|(m, _)| *m == i) => {
                "motion table".to_string()
            }
            Role::MotionTable => "motion table (unused: not the first)".to_string(),
            Role::Animation { motion, kind } => {
                format!("{} of motion {motion:?}", kind.extension())
            }
            Role::Other => "other".to_string(),
        };
        println!("  {i:3} {:7} {name:24} {role}", chr.narc.file(i)?.len());
    }
    if let Some(model) = chr.model_data()? {
        match nds_rom::g3d::G3dFile::parse(model) {
            Ok(g3d) => {
                for m in &g3d.models {
                    println!("model {}", m.name);
                }
                if let Some(tex) = &g3d.tex {
                    println!("embedded textures: {}", tex.textures.len());
                }
            }
            Err(e) => println!("model: not decoded ({e})"),
        }
    }
    if let Some((_, table)) = &chr.motion_table {
        println!(
            "motions (root bone {}):",
            table.root_bone.map(text).unwrap_or_default()
        );
        for (k, m) in chr.motions().iter().enumerate() {
            let files: Vec<&str> = m.files.iter().map(|(kind, _)| kind.extension()).collect();
            println!(
                "  {k:2} {:12} frames {}..{} speed {} {}",
                m.motion.name_str(),
                fx(m.motion.start),
                fx(m.motion.end),
                fx(m.motion.speed),
                if files.is_empty() {
                    "(no animation file)".to_string()
                } else {
                    files.join(" ")
                }
            );
            for c in &m.motion.cues {
                println!("       cue {} at {}", c.sound, fx(i32::from(c.time)));
            }
            for c in &m.motion.marks {
                println!("       mark {} at {}", c.value, fx(i32::from(c.time)));
            }
        }
    }
    let slots: Vec<String> = chr
        .slots()
        .iter()
        .map(|s| s.map_or("-".to_string(), |i| i.to_string()))
        .collect();
    println!(
        "type-0 package slots (nsbca nsbma nsbtp nsbta): {}",
        slots.join(" ")
    );
    Ok(())
}

fn find<'a>(list: &'a MapList<'a>, key: &str) -> Result<&'a MapRecord<'a>> {
    let found = match key.parse::<u16>() {
        Ok(id) => list.find(id),
        Err(_) => list
            .records
            .iter()
            .find(|r| r.code.eq_ignore_ascii_case(key.as_bytes())),
    };
    found.with_context(|| format!("no map {key:?} in the map list"))
}

/// The character archive of a monster NPC: `.cchr` of `<name>.mon` in `enemy.gp2`.
fn monster_chr(rom: &Rom, member: &str) -> Result<Vec<u8>> {
    let gp2 = Gpc2::parse(rom.file_by_path("data/pack_lv5/enemy.gp2")?)?;
    let entry = gp2.find(member).with_context(|| format!("no {member}"))?;
    let mon = gp2.member(entry)?;
    let narc = Narc::parse(&mon)?;
    for i in 0..u16::try_from(narc.len())? {
        if narc.name(i).is_some_and(|n| n.contains(".cchr")) {
            return Ok(compress::decompress(narc.file(i)?)?);
        }
    }
    bail!("{member} has no .cchr")
}

/// The archive an NPC is drawn with, and its initial motion's name.
fn describe(rom: &Rom, npc: &Npc) -> Result<(String, String)> {
    let exists = |p: &str| map::rom_file(rom, p).is_some();
    let appearance = npc.appearance(exists);
    let data: Option<Cow<[u8]>> = match &appearance {
        Appearance::Model { chr } => map::rom_file(rom, chr).map(Cow::Borrowed),
        Appearance::Monster { member } => Some(Cow::Owned(monster_chr(rom, member)?)),
        _ => None,
    };
    let what = match &appearance {
        Appearance::None => "-".to_string(),
        Appearance::Sprite { sheet } => sheet.clone(),
        Appearance::Model { chr } => chr.clone(),
        Appearance::Monster { member } => format!("enemy.gp2:{member}"),
    };
    let what = if !matches!(appearance, Appearance::None)
        && data.is_none()
        && !matches!(appearance, Appearance::Sprite { .. })
    {
        format!("{what} (missing)")
    } else {
        what
    };
    let motion = match (&appearance, data) {
        (Appearance::Sprite { .. }, _) => "stand_* (by camera)".to_string(),
        (_, Some(data)) => {
            let own = Chr::parse(&data)?;
            let extra_data = npc
                .placement
                .chr
                .and_then(|p| map::rom_file(rom, &format!("data/{}", text(p))));
            let extra = extra_data.map(Chr::parse).transpose()?;
            match cast::initial_motion(&npc.placement, &own, extra.as_ref()) {
                Some((k, i)) => {
                    let chr = if k == 0 {
                        &own
                    } else {
                        extra.as_ref().unwrap_or(&own)
                    };
                    let table = &chr.motion_table.as_ref().context("motion table")?.1;
                    let m = &table.motions[i];
                    format!("{}{}", if k == 1 { "+" } else { "" }, m.name_str())
                }
                None => "-".to_string(),
            }
        }
        _ => "-".to_string(),
    };
    Ok((what, motion))
}

fn print_cast(rom: &Rom, record: &MapRecord, story: &Story, cast: &Cast) -> Result<()> {
    println!(
        "{} (map {}), story {}.{}.{}, {}: {} NPCs",
        text(record.code),
        record.id,
        story.chapter,
        story.step,
        story.substep,
        if story.daytime { "day" } else { "night" },
        cast.npcs.len()
    );
    if cast.read_unset {
        println!("note: a record read a field no earlier record had set (garbage in the game)");
    }
    println!(
        "  {:>3} {:8} {:>4} {:32} {:>8} {:>8} {:>8} {:>7} motion",
        "id", "name", "kind", "drawn with", "x", "y", "z", "angle"
    );
    for npc in &cast.npcs {
        let (what, motion) = describe(rom, npc)?;
        let p = &npc.placement;
        let mut extra = Vec::new();
        if p.flags != 0 {
            extra.push(format!("flags {:#x}", p.flags));
        }
        if !p.zones.is_empty() {
            let labels: Vec<String> = p.zones.iter().map(|z| z.label.to_string()).collect();
            extra.push(format!("talk zones {}", labels.join(",")));
        }
        if p.mode != 0 {
            extra.push(format!("move mode {}", p.mode));
        }
        if p.quest >= 0 {
            extra.push(format!("quest {}", p.quest));
        }
        if let Some(chr) = p.chr {
            extra.push(format!("+ data/{}", text(chr)));
        }
        println!(
            "  {:>3} {:8} {:>4} {:32} {:>8} {:>8} {:>8} {:>7.1} {motion} {}",
            npc.member.id,
            npc.member.name_str(),
            npc.member.kind,
            what,
            fx(p.position[0]),
            fx(p.position[1]),
            fx(p.position[2]),
            f64::from(p.angle) / 4096.0 * 180.0 / std::f64::consts::PI,
            extra.join("; ")
        );
    }
    Ok(())
}

/// Differences between the predicted cast and the RAM NPC table, entry by entry.
fn compare(cast: &Cast, ram: &[NpcEntry]) -> Vec<String> {
    let mut out = Vec::new();
    if cast.npcs.len() != ram.len() {
        out.push(format!(
            "count: {} predicted, {} in RAM",
            cast.npcs.len(),
            ram.len()
        ));
    }
    for (i, (npc, entry)) in cast.npcs.iter().zip(ram).enumerate() {
        let m = &npc.member;
        let p = &npc.placement;
        let mut diff = Vec::new();
        if (m.id, m.kind, m.byte3) != (entry.id, entry.kind, entry.byte3) {
            diff.push(format!(
                "record ({}, {}, {}) vs ({}, {}, {})",
                m.id, m.kind, m.byte3, entry.id, entry.kind, entry.byte3
            ));
        }
        if m.name.unwrap_or_default() != entry.name.as_slice() {
            diff.push(format!("name {} vs {}", m.name_str(), text(&entry.name)));
        }
        match &entry.placement {
            None => diff.push("no placement in RAM".into()),
            Some(r) => {
                let ours = (
                    p.npc, p.map, p.position, p.angle, p.motion, p.order, p.quest,
                );
                let theirs = (
                    r.npc, r.map, r.position, r.angle, r.motion, r.order, r.quest,
                );
                if ours != theirs {
                    diff.push(format!("placement {ours:?} vs {theirs:?}"));
                }
                if p.flags != r.flags {
                    diff.push(format!("placement flags {:#x} vs {:#x}", p.flags, r.flags));
                }
            }
        }
        if let (Some(pos), Some(angle)) = (entry.position, entry.angle)
            && ((pos[0], pos[2]) != (p.position[0], p.position[2]) || angle != i32::from(p.angle))
        {
            diff.push(format!(
                "body at ({}, {}) angle {angle} vs ({}, {}) {}",
                pos[0], pos[2], p.position[0], p.position[2], p.angle
            ));
        }
        let body = match entry.body {
            Some(NpcBody::Sprite(_)) => "sprite",
            Some(NpcBody::Model(_)) => "model",
            Some(NpcBody::Object(_)) => "object",
            None => "nothing",
        };
        let ground = entry
            .position
            .map(|pos| {
                format!(
                    ", y {} placed, {} on the ground",
                    fx(p.position[1]),
                    fx(pos[1])
                )
            })
            .unwrap_or_default();
        if diff.is_empty() {
            println!(
                "  entry {i}: NPC {} {} ({body}) identical{ground}",
                m.id,
                m.name_str()
            );
        } else {
            out.push(format!("entry {i} (NPC {}): {}", m.id, diff.join("; ")));
        }
    }
    out
}

pub fn run_cast(rom: &Rom, args: &CastArgs) -> Result<()> {
    let table = Table::parse(rom.file_by_path(maplist::PATH)?).context("parsing map list")?;
    let list = MapList::parse(&table)?;
    let state_bytes = match &args.state {
        Some(path) => Some(fs::read(path).with_context(|| format!("reading {}", path.display()))?),
        None => None,
    };
    let state = state_bytes.as_deref().map(State::parse).transpose()?;
    let record = match (&args.map, &state) {
        (Some(key), _) => find(&list, key)?,
        (None, Some(state)) => {
            let (id, _) = state.current_map()?;
            list.find(id)
                .with_context(|| format!("current map {id} not in the list"))?
        }
        (None, None) => bail!("give a map or --state"),
    };
    let story = match &state {
        Some(state) => state.story()?,
        None => Story {
            chapter: args.chapter,
            step: args.step,
            substep: args.substep,
            daytime: !args.night,
            flags: args.flag.iter().copied().collect(),
            quests: args.quest.iter().copied().collect(),
        },
    };
    let path = cast::archive_path(record.code);
    let cast = match map::rom_file(rom, &path) {
        Some(archive) => cast::load(archive, record.id, &story)?,
        None => {
            println!("{}: no {path}, no cast", text(record.code));
            Cast::default()
        }
    };
    print_cast(rom, record, &story, &cast)?;
    if let Some(state) = &state {
        let ram = state.npcs()?;
        let (id, _) = state.current_map()?;
        if id != record.id {
            println!("\nRAM: the state is on map {id}, not compared");
            return Ok(());
        }
        println!("\nRAM NPC table ({} entries):", ram.len());
        let diffs = compare(&cast, &ram);
        if diffs.is_empty() {
            println!("RAM: the NPC table matches the cast");
        } else {
            println!("RAM: {} differences", diffs.len());
            for d in diffs {
                println!("  {d}");
            }
        }
    }
    Ok(())
}
