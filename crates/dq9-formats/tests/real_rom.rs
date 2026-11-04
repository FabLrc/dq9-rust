//! Format invariants on the real game data; skipped when the ROM is absent.

use std::collections::HashSet;

use dq9_formats::{Gpc2, Table, Value, level5};
use nds_rom::Rom;

/// Calls `f(archive path, member name, decompressed bytes)` for every GPC2 member.
fn for_each_member(rom: &Rom, mut f: impl FnMut(&str, &str, &[u8])) {
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let archive = Gpc2::parse(rom.file(file.id).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", file.path));
        for entry in &archive.entries {
            let content = archive
                .member(entry)
                .unwrap_or_else(|e| panic!("{}/{}: {e}", file.path, entry.name));
            f(&file.path, &entry.name, &content);
        }
    }
}

/// Every `.gp2` is a GPC2 archive whose index hashes match its names (checked by `parse`) and
/// whose members all decompress to their declared size.
#[test]
fn all_gp2_archives_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut archives = 0;
    let mut members = 0;
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let archive = Gpc2::parse(rom.file(file.id).unwrap()).unwrap();
        for entry in &archive.entries {
            let content = archive.member(entry).unwrap();
            if let (Ok((_, size)), std::borrow::Cow::Owned(_)) =
                (level5::header(archive.stored(entry)), &content)
            {
                assert_eq!(content.len(), size, "{}/{}", file.path, entry.name);
            }
            members += 1;
        }
        archives += 1;
    }
    assert_eq!(archives, 1671);
    assert_eq!(members, 53_639);
}

/// Record tables: every string offset starts a string (checked by `parse`), the header's
/// string count equals the number of distinct strings referenced, and the known population
/// parses.
#[test]
fn record_tables_parse() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut tables = 0;
    for_each_member(&rom, |archive, name, data| {
        if !(name.ends_with(".bin") || name.ends_with(".bmmp") || name.ends_with(".bcfg")) {
            return;
        }
        let Ok(table) = Table::parse(data) else {
            return;
        };
        let strings: HashSet<*const u8> = table
            .records
            .iter()
            .flat_map(|r| &r.values)
            .filter_map(|v| match v {
                Value::Str(Some(s)) => Some(s.as_ptr()),
                _ => None,
            })
            .collect();
        assert_eq!(
            strings.len(),
            table.string_count as usize,
            "{archive}/{name}"
        );
        tables += 1;
    });
    // 47 075 `.bin` + 283 `.bmmp` + 28 `.bcfg`; the other 152 `.bin` use other formats.
    assert_eq!(tables, 47_386);
}
