//! `dq9re state`: inspect a melonDS savestate (`docs/re/melonds-state.md`). When the ROM is
//! found, also compares resident code with it: checks the section offsets, lists overlays.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use dq9_formats::melonds::State;
use nds_rom::Rom;

/// Share of identical bytes above which an overlay is reported as loaded. Overlays sharing an
/// address range agree on far fewer bytes; `.data` writes keep a loaded one below 100 %.
const LOADED: f64 = 0.9;

pub fn run(path: &Path, peek: &[String], rom: Option<&Path>) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let state = State::parse(&bytes).context("parsing savestate")?;
    if peek.is_empty() {
        for s in state.sections() {
            println!(
                "{:<4} at {:08x}, {:08x} bytes",
                String::from_utf8_lossy(&s.name),
                s.offset,
                s.len
            );
        }
        println!("SDK VBlank count: {}", state.vblank_count()?);
        let (chapter, step, substep) = state.scenario()?;
        println!("scenario: chapter {chapter} step {step} substep {substep}");
        let rom_path = rom
            .map(Path::to_path_buf)
            .or_else(|| crate::find_rom().ok());
        match rom_path {
            Some(rom_path) => {
                let rom_bytes = fs::read(&rom_path)
                    .with_context(|| format!("reading {}", rom_path.display()))?;
                compare_with_rom(&state, &Rom::parse(&rom_bytes).context("parsing ROM")?)?;
            }
            None => println!("no ROM found: resident code not compared"),
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
        let bytes = state.read(addr, len)?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        println!("{addr:08x}  {hex}");
    }
    Ok(())
}

/// Share of bytes of `expected` found at `addr` in the state; `None` if the range is unmapped.
fn match_ratio(state: &State, addr: u32, expected: &[u8]) -> Option<f64> {
    let actual = state.read(addr, expected.len()).ok()?;
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
