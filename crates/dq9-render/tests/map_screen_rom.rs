//! The field's map screen data on the user's ROM (skipped without it): every layout the archive
//! holds resolves its picture, and every map of the map list either gets its map screen or a
//! named reason for the placeholder.

use dq9_formats::Gpc2;
use dq9_formats::Table;
use dq9_formats::maplist::{self, MapList};
use dq9_render::map_screen::{self, Error};
use nds_rom::Rom;

fn read<'a>(rom: &Rom<'a>, path: &str) -> &'a [u8] {
    rom.file_by_path(path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn every_layout_names_a_picture_the_archive_holds() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = Gpc2::parse(read(&rom, map_screen::ARCHIVE)).unwrap();
    let (ncer, ncgr, nclr) = (
        read(&rom, map_screen::SPRITE_CELLS),
        read(&rom, map_screen::SPRITE_CHARS),
        read(&rom, map_screen::SPRITE_PALETTE),
    );
    let layout_bytes = read(&rom, map_screen::ARCHIVE);
    let mut layouts = 0;
    for entry in archive.entries.iter().filter(|e| e.name.ends_with(".bmmp")) {
        let code = entry.name.trim_end_matches(".bmmp");
        let content = map_screen::content(layout_bytes, ncer, ncgr, nclr, code)
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert!(
            content.picture.width > 0 && content.picture.height > 0,
            "{code}"
        );
        assert_eq!(content.strip, (64, 16), "{code}");
        assert_eq!(content.sprites.size(), (256, 24), "{code}");
        assert!(content.scale > 0.0, "{code}");
        // The four strips and the dots have drawn pixels in their sheet cells.
        for slot in 0..4 {
            let src = map_screen::strip_src(&content, slot);
            assert!(has_pixels(&content.sprites, src), "{code}: strip {slot}");
            let src = map_screen::dot_src(&content, slot);
            assert!(has_pixels(&content.sprites, src), "{code}: dot {slot}");
        }
        layouts += 1;
    }
    println!("{layouts} layouts resolve their picture and sprite sheet");
    assert!(layouts > 200, "only {layouts} layouts");
}

#[test]
fn every_map_resolves_or_names_why() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = read(&rom, map_screen::ARCHIVE);
    let (ncer, ncgr, nclr) = (
        read(&rom, map_screen::SPRITE_CELLS),
        read(&rom, map_screen::SPRITE_CHARS),
        read(&rom, map_screen::SPRITE_PALETTE),
    );
    let table = Table::parse(read(&rom, maplist::PATH)).unwrap();
    let list = MapList::parse(&table).unwrap();
    let (mut drawn, mut missing) = (0, Vec::new());
    for record in &list.records {
        let Ok(code) = core::str::from_utf8(record.code) else {
            continue;
        };
        match map_screen::content(archive, ncer, ncgr, nclr, code) {
            Ok(content) => {
                assert!(content.strip == (64, 16), "{code}");
                drawn += 1;
            }
            Err(e) => missing.push((code.to_owned(), e)),
        }
    }
    println!(
        "{} maps: {drawn} draw a map screen, {} fall back",
        list.records.len(),
        missing.len()
    );
    for (code, error) in &missing {
        println!("  {code}: {error}");
    }
    // A map without a layout is the only named reason: a layout that exists always resolves.
    for (code, error) in &missing {
        assert!(
            matches!(error, Error::NoLayout(_)),
            "{code}: {error} (a layout that exists must resolve)"
        );
    }
    // Dungeons and interiors without an area plan have no `.bmmp`; every map that has one
    // resolves (the other test covers all 283 layouts).
    assert!(drawn > 400, "only {drawn} maps resolve");
}

/// Whether a sheet rectangle holds a pixel with alpha.
fn has_pixels(sheet: &map_screen::Image, [x, y, w, h]: [u32; 4]) -> bool {
    (y..y + h).any(|py| {
        (x..x + w).any(|px| {
            let offset = ((py * sheet.width + px) * 4 + 3) as usize;
            sheet.rgba.get(offset).is_some_and(|&a| a != 0)
        })
    })
}
