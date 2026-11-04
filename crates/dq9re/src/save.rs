//! `dq9re save`: summary of a DS save file (`docs/re/save.md`). Needs no ROM.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use dq9_formats::FontId;
use dq9_formats::save::{EquipSlot, Save, Slot};
use nds_rom::Rom;

use crate::font;

pub fn run(path: &Path, verbose: bool, rom: Option<&Path>) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let save = Save::parse(&bytes).context("parsing save")?;
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
