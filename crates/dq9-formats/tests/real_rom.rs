//! Format invariants on the real game data; skipped when the ROM is absent.

use dq9_formats::{Gpc2, level5};
use nds_rom::Rom;

/// Every `.gp2` is a GPC2 archive whose index hashes match its names (checked by `parse`) and
/// whose members all decompress to their declared size.
#[test]
fn all_gp2_archives_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut archives = 0;
    let mut members = 0;
    for f in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let data = rom.file(f.id).unwrap();
        let archive = Gpc2::parse(data).unwrap_or_else(|e| panic!("{}: {e}", f.path));
        for entry in &archive.entries {
            let content = archive
                .member(entry)
                .unwrap_or_else(|e| panic!("{}/{}: {e}", f.path, entry.name));
            if let Ok((_, size)) = level5::header(archive.stored(entry)) {
                // Compressed members must yield exactly their declared size.
                if !matches!(content, std::borrow::Cow::Borrowed(_)) {
                    assert_eq!(content.len(), size, "{}/{}", f.path, entry.name);
                }
            }
            members += 1;
        }
        archives += 1;
    }
    assert_eq!(archives, 1671);
    assert_eq!(members, 53_639);
}
