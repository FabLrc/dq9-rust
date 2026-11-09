//! `dq9re state`: inspect a melonDS savestate (`.ml1`) or a raw main-RAM image, with the
//! same readers (`docs/re/melonds-state.md`). When the ROM is found and the input is a
//! savestate, also compares resident code with it: checks the section offsets, lists
//! overlays. With `--save`, compares the RAM with the save the game was loaded from.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use dq9_formats::melonds::{MAIN_RAM_BASE, MAIN_RAM_SIZE, Memory, State};
use dq9_formats::save::Save;
use nds_rom::Rom;

/// Share of identical bytes above which an overlay is reported as loaded. Overlays sharing an
/// address range agree on far fewer bytes; `.data` writes keep a loaded one below 100 %.
const LOADED: f64 = 0.9;

pub fn run(
    path: Option<&Path>,
    ram: Option<&Path>,
    peek: &[String],
    save: Option<&Path>,
    rom: Option<&Path>,
    json: bool,
) -> Result<()> {
    let file = match (path, ram) {
        (Some(path), None) => path,
        (None, Some(ram)) => ram,
        (Some(_), Some(_)) => bail!("pass a savestate or --ram, not both"),
        (None, None) => bail!("pass a savestate path or --ram FILE"),
    };
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    // A positional path may be either container; `--ram` insists on the raw image.
    let is_ram = ram.is_some() || (!bytes.starts_with(b"MELN") && bytes.len() == MAIN_RAM_SIZE);
    let (memory, state) = if is_ram {
        let memory = Memory::from_main_ram(&bytes).context("reading a main-RAM image")?;
        (memory, None)
    } else {
        let state = State::parse(&bytes).context("parsing savestate")?;
        (state.memory(), Some(state))
    };
    if json {
        println!("{}", checkpoint_json(&memory)?);
        return Ok(());
    }
    if peek.is_empty() {
        match &state {
            Some(state) => {
                for s in state.sections() {
                    println!(
                        "{:<4} at {:08x}, {:08x} bytes",
                        String::from_utf8_lossy(&s.name),
                        s.offset,
                        s.len
                    );
                }
            }
            None => println!(
                "main RAM image at {:#010x}, {} bytes (no ITCM/DTCM, no overlays)",
                MAIN_RAM_BASE,
                memory.main_ram().len()
            ),
        }
        println!("SDK VBlank count: {}", memory.vblank_count()?);
        let checkpoint = memory
            .checkpoint()
            .context("reading the story checkpoint")?;
        let (chapter, step, substep) = checkpoint
            .scenario()
            .context("current scenario track out of range")?;
        println!(
            "scenario: chapter {chapter} step {step} substep {substep} (track {})",
            checkpoint.current_track
        );
        println!(
            "map {} area {}; {} global flags, {} quests",
            checkpoint.map,
            checkpoint.area,
            checkpoint.flags.len(),
            checkpoint.quests.len()
        );
        for (i, t) in checkpoint.tracks.iter().enumerate() {
            println!(
                "track {i}: {}.{}.{} chapter flags {:?} step flags {:?}",
                t.chapter, t.step, t.substep, t.chapter_flags, t.step_flags
            );
        }
        println!("global flags: {:?}", checkpoint.flags);
        println!("quests: {:?}", checkpoint.quests);
        let hero = memory.protagonist_index()?;
        for index in 0..4 {
            if let Some(at) = memory.object(index)? {
                let p = memory.object_position(at)?.map(|v| f64::from(v) / 4096.0);
                let who = if index == hero { " (protagonist)" } else { "" };
                println!(
                    "object {index} at {at:#010x}: position ({:.3}, {:.3}, {:.3}){who}",
                    p[0], p[1], p[2]
                );
            }
        }
        match &state {
            Some(state) => {
                let rom_path = rom
                    .map(Path::to_path_buf)
                    .or_else(|| crate::find_rom().ok());
                match rom_path {
                    Some(rom_path) => {
                        let rom_bytes = fs::read(&rom_path)
                            .with_context(|| format!("reading {}", rom_path.display()))?;
                        compare_with_rom(state, &Rom::parse(&rom_bytes).context("parsing ROM")?)?;
                    }
                    None => println!("no ROM found: resident code not compared"),
                }
            }
            None => println!("no savestate: resident code, overlays and TCM not compared"),
        }
        if let Some(save) = save {
            compare_with_save(&memory, save)?;
        }
        return Ok(());
    }
    for spec in peek {
        let (addr, len) = spec.split_once(':').unwrap_or((spec, "10"));
        let addr = u32::from_str_radix(addr.trim_start_matches("0x"), 16)
            .with_context(|| format!("bad address {addr:?}"))?;
        let len = usize::from_str_radix(len.trim_start_matches("0x"), 16)
            .with_context(|| format!("bad length {len:?}"))?;
        if len > 0x1_0000 {
            bail!("refusing to print more than 0x10000 bytes at once");
        }
        let bytes = memory.read(addr, len)?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        println!("{addr:08x}  {hex}");
    }
    Ok(())
}

fn checkpoint_json(memory: &Memory) -> Result<String> {
    let c = memory.checkpoint()?;
    let tracks: Vec<serde_json::Value> = c
        .tracks
        .iter()
        .map(|t| {
            serde_json::json!({
                "chapter": t.chapter,
                "step": t.step,
                "substep": t.substep,
                "chapter_flags": t.chapter_flags,
                "step_flags": t.step_flags,
            })
        })
        .collect();
    let quests: serde_json::Map<String, serde_json::Value> = c
        .quests
        .iter()
        .map(|&(q, n)| (q.to_string(), serde_json::json!(n)))
        .collect();
    let value = serde_json::json!({
        "vblank": c.vblank,
        "map": c.map,
        "area": c.area,
        "track": c.current_track,
        "daytime": c.daytime,
        "tracks": tracks,
        "flags": c.flags,
        "quests": quests,
        "hero": hero_json(memory)?,
    });
    Ok(serde_json::to_string(&value)?)
}

/// `dq9-app --checkpoints` reads the hero in world units and prints three decimals; the
/// oracle stores fx32, so convert the same way (`1 / 4096`, yaw included).
fn hero_json(memory: &Memory) -> Result<serde_json::Value> {
    let index = memory.protagonist_index()?;
    let Some(at) = memory.object(index)? else {
        return Ok(serde_json::Value::Null);
    };
    let p = memory.object_position(at)?;
    let yaw = memory.object_heading(at)?;
    Ok(serde_json::json!({
        "x": round3(f64::from(p[0]) / 4096.0),
        "y": round3(f64::from(p[1]) / 4096.0),
        "z": round3(f64::from(p[2]) / 4096.0),
        "yaw": round3(f64::from(yaw) / 4096.0),
    }))
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn compare_with_save(memory: &Memory, path: &Path) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let save = Save::parse(&bytes).context("parsing save")?;
    let index = save.current_slot().context("no valid slot in the save")?;
    let slot = save.slot(index).context("slot")?;
    println!("save slot {index} copied into RAM by a normal load:");
    let mut unexpected = 0;
    for (c, diffs) in memory.compare_save(&slot)? {
        let verdict = match diffs.first() {
            None => "identical".to_owned(),
            Some(first) => {
                unexpected += usize::from(!c.runtime);
                format!(
                    "{}/{} bytes differ, first at +{first:#x}{}",
                    diffs.len(),
                    c.len,
                    if c.runtime {
                        " (changed by the game)"
                    } else {
                        ""
                    }
                )
            }
        };
        println!(
            "  {:<26} slot+{:<5} -> {:#010x} ({:#x} bytes): {verdict}",
            c.name, c.slot_offset, c.ram, c.len
        );
    }
    println!("party members (character record -> member object):");
    for m in memory.compare_party(&slot)? {
        let verdict = match (m.member, m.differing.is_empty()) {
            (None, _) => "no member object has its id".to_owned(),
            (Some(_), true) => "all compared fields identical".to_owned(),
            (Some(_), false) => format!("differ: {}", m.differing.join(", ")),
        };
        unexpected += usize::from(m.member.is_none() || !m.differing.is_empty());
        let at = m.member.map_or("-".to_owned(), |a| format!("{a:#010x}"));
        println!(
            "  #{} character {:<2} -> {at}: {verdict}",
            m.party_index, m.character
        );
    }
    if unexpected > 0 {
        println!("{unexpected} copies or members differ: was the game loaded from this save?");
    }
    Ok(())
}

/// Share of bytes of `expected` found at `addr` in the state; `None` if the range is unmapped.
fn match_ratio(memory: &Memory, addr: u32, expected: &[u8]) -> Option<f64> {
    let actual = memory.read(addr, expected.len()).ok()?;
    let same = actual.iter().zip(expected).filter(|(a, b)| a == b).count();
    Some(same as f64 / expected.len().max(1) as f64)
}

fn print_ratio(label: &str, ratio: Option<f64>) {
    match ratio {
        Some(r) => println!("{label}: {:.1} % identical to the ROM", r * 100.0),
        None => println!("{label}: not in the saved memories"),
    }
}

fn compare_with_rom(state: &State, rom: &Rom) -> Result<()> {
    let arm9 = rom.arm9()?;
    print_ratio(
        &format!("ARM9 static module {:#010x}", arm9.load_addr),
        match_ratio(state, arm9.load_addr, arm9.static_module()?),
    );
    for a in arm9.autoloads()? {
        let data = &arm9.data[a.data_offset..a.data_offset + a.size as usize];
        print_ratio(
            &format!("autoload {:#010x}", a.ram_addr),
            match_ratio(state, a.ram_addr, data),
        );
    }
    let mut loaded = Vec::new();
    for ov in rom.overlays()? {
        let data = rom.overlay_data(&ov)?;
        if let Some(r) = match_ratio(state, ov.ram_addr, &data)
            && r >= LOADED
        {
            loaded.push(format!("ov_{:03} ({:.1} %)", ov.id, r * 100.0));
        }
    }
    println!("overlays in memory: {}", loaded.join(", "));
    Ok(())
}
