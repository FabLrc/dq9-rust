//! Invariants checked against the real game image. The ROM is never committed, so these tests
//! are skipped (with a notice) when it is absent; CI runs without it.
//! Location: `DQ9_ROM`, else the only `*.nds` in `<repo>/rom/` or `<repo>/`.

use std::fs;
use std::path::PathBuf;

use nds_rom::{Narc, Rom};

fn rom_bytes() -> Option<Vec<u8>> {
    let path = std::env::var_os("DQ9_ROM").map(PathBuf::from).or_else(|| {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        [repo.join("rom"), repo].into_iter().find_map(|dir| {
            fs::read_dir(dir)
                .ok()?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nds")))
        })
    });
    match path {
        Some(path) => Some(fs::read(path).expect("reading ROM")),
        None => {
            eprintln!("skipped: no ROM (set DQ9_ROM)");
            None
        }
    }
}

#[test]
fn real_rom_invariants() {
    let Some(bytes) = rom_bytes() else { return };
    let rom = Rom::parse(&bytes).unwrap();
    assert!(rom.header.crc_ok());

    // BLZ correctness: the autoload list sits right after the autoload payload, which only
    // lines up if the ARM9 decompressed to exactly the right bytes.
    let arm9 = rom.arm9().unwrap();
    let autoloads = arm9.autoloads().unwrap();
    assert!(!autoloads.is_empty());
    let payload_end: u32 =
        arm9.params.autoload_start + autoloads.iter().map(|a| a.size).sum::<u32>();
    assert_eq!(payload_end, arm9.params.autoload_list_start);

    // Every overlay decompresses to exactly its declared RAM size.
    for ov in rom.overlays().unwrap() {
        let data = rom.overlay_data(&ov).unwrap();
        assert_eq!(data.len(), ov.ram_size as usize, "overlay {}", ov.id);
    }

    // Every named file is in bounds, and every NARC parses with in-bounds members.
    for f in &rom.files.files {
        let data = rom.file(f.id).unwrap();
        if Narc::is_narc(data) {
            let narc = Narc::parse(data).unwrap_or_else(|e| panic!("{}: {e}", f.path));
            for i in 0..narc.len() as u16 {
                narc.file(i)
                    .unwrap_or_else(|e| panic!("{} member {i}: {e}", f.path));
            }
        }
    }
}
