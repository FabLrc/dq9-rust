//! `dq9re state`: inspect a melonDS savestate (`docs/re/melonds-state.md`). Needs no ROM.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use dq9_formats::melonds::State;

pub fn run(path: &Path, peek: &[String]) -> Result<()> {
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
