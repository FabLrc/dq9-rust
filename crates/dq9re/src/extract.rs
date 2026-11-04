//! `dq9re extract`: dump code and data plus a machine-readable memory layout.
//!
//! `layout.json` is consumed by `re/ghidra_scripts/NdsLayout.java` to build the Ghidra
//! program (main RAM, autoload blocks, overlays as overlay spaces).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use nds_rom::Rom;
use serde::Serialize;

#[derive(Serialize)]
struct Layout {
    title: String,
    game_code: String,
    arm9: Arm9Layout,
    autoloads: Vec<AutoloadLayout>,
    overlays: Vec<OverlayLayout>,
    arm7: Arm7Layout,
}

#[derive(Serialize)]
struct Arm9Layout {
    /// Decompressed binary as placed at `load_addr` by the cartridge loader.
    file: &'static str,
    load_addr: u32,
    entry: u32,
    /// End of the code that stays resident at `load_addr` (start of autoload data).
    static_end: u32,
    bss_start: u32,
    bss_end: u32,
    sdk_version: u32,
}

#[derive(Serialize)]
struct AutoloadLayout {
    /// `itcm`, `dtcm` or `main` (inferred from the destination address).
    name: String,
    ram_addr: u32,
    size: u32,
    bss_size: u32,
    /// Offset of the block inside `arm9.file`.
    file_offset: usize,
}

#[derive(Serialize)]
struct OverlayLayout {
    id: u32,
    file: String,
    ram_addr: u32,
    ram_size: u32,
    bss_size: u32,
    sinit_start: u32,
    sinit_end: u32,
}

#[derive(Serialize)]
struct Arm7Layout {
    file: &'static str,
    load_addr: u32,
    entry: u32,
}

fn region_name(addr: u32) -> &'static str {
    match addr {
        ..0x0200_0000 => "itcm",
        0x0200_0000..0x0240_0000 => "main",
        _ => "dtcm",
    }
}

fn write(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

pub fn run(rom: &Rom, out: &Path, code_only: bool) -> Result<()> {
    let arm9 = rom.arm9()?;
    write(&out.join("arm9.bin"), &arm9.data)?;
    write(&out.join("arm7.bin"), rom.arm7()?)?;

    let autoloads = arm9
        .autoloads()?
        .into_iter()
        .map(|a| AutoloadLayout {
            name: region_name(a.ram_addr).to_owned(),
            ram_addr: a.ram_addr,
            size: a.size,
            bss_size: a.bss_size,
            file_offset: a.data_offset,
        })
        .collect();

    let mut overlays = Vec::new();
    for ov in rom.overlays()? {
        let file = format!("overlays/ov_{:03}.bin", ov.id);
        let data = rom
            .overlay_data(&ov)
            .with_context(|| format!("overlay {}", ov.id))?;
        write(&out.join(&file), &data)?;
        overlays.push(OverlayLayout {
            id: ov.id,
            file,
            ram_addr: ov.ram_addr,
            ram_size: ov.ram_size,
            bss_size: ov.bss_size,
            sinit_start: ov.sinit_start,
            sinit_end: ov.sinit_end,
        });
    }

    let h = &rom.header;
    let layout = Layout {
        title: h.title.clone(),
        game_code: h.game_code.clone(),
        arm9: Arm9Layout {
            file: "arm9.bin",
            load_addr: arm9.load_addr,
            entry: arm9.entry,
            static_end: arm9.params.autoload_start,
            bss_start: arm9.params.static_bss_start,
            bss_end: arm9.params.static_bss_end,
            sdk_version: arm9.params.sdk_version,
        },
        autoloads,
        overlays,
        arm7: Arm7Layout {
            file: "arm7.bin",
            load_addr: h.arm7.load_addr,
            entry: h.arm7.entry,
        },
    };
    write(
        &out.join("layout.json"),
        serde_json::to_string_pretty(&layout)?.as_bytes(),
    )?;
    println!(
        "wrote binaries, {} overlays and layout.json",
        layout.overlays.len()
    );

    if !code_only {
        for f in &rom.files.files {
            write(&out.join("files").join(&f.path), rom.file(f.id)?)?;
        }
        println!(
            "wrote {} files to {}",
            rom.files.files.len(),
            out.join("files").display()
        );
    }
    Ok(())
}
