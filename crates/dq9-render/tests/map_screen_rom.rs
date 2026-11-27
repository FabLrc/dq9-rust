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
    let mut backdrops = 0;
    let mut fields = 0;
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
        // The backdrop the layout names is in the same archive: `minimapbg2`-`4`, 8×8 cells
        // (the one-cell `minimapbg4`, 8×8 pixels); the fields name none.
        match &content.backdrop {
            Some(backdrop) => {
                assert!(backdrop.width > 0 && backdrop.height > 0, "{code}");
                assert_eq!(
                    backdrop.rgba.len(),
                    (backdrop.width * backdrop.height * 4) as usize
                );
                backdrops += 1;
            }
            None => fields += 1,
        }
        // The four strips and the dots have drawn pixels in their sheet cells.
        for slot in 0..4 {
            let src = map_screen::strip_src(&content, slot);
            assert!(has_pixels(&content.sprites, src), "{code}: strip {slot}");
            let src = map_screen::dot_src(&content, slot);
            assert!(has_pixels(&content.sprites, src), "{code}: dot {slot}");
        }
        layouts += 1;
    }
    println!("{layouts} layouts resolve their picture, backdrop and sprite sheet");
    assert_eq!(layouts, 283);
    assert_eq!(backdrops, 221);
    assert_eq!(fields, 62);
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

/// The original's mode-0 window on the doc's two layouts (`docs/re/minimap.md` § 4.2): the
/// leader's picture pixel starts on the screen's (128, 96) and the picture only follows past the
/// dead zone, so the dot moves alone by up to 30 pixels horizontally.
#[test]
fn the_scroll_anchors_the_leader_on_two_layouts() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = read(&rom, map_screen::ARCHIVE);
    let (ncer, ncgr, nclr) = (
        read(&rom, map_screen::SPRITE_CELLS),
        read(&rom, map_screen::SPRITE_CHARS),
        read(&rom, map_screen::SPRITE_PALETTE),
    );
    for (code, leader, pixel) in [
        ("M01", (-7.16, 16.31), (121.0, 148.0)),
        ("S01M01", (-18.52, 41.69), (43.0, 187.0)),
    ] {
        let content = map_screen::content(archive, ncer, ncgr, nclr, code)
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        let at = map_screen::project(leader, content.scale, content.corner);
        assert_eq!(at, pixel, "{code}");
        // At map load the scroll is the leader's own pixel: the picture's top-left is the
        // anchor minus it and the leader's dot is on the anchor.
        let scroll = at;
        let world = |pixel: (f32, f32)| {
            (
                (pixel.0 + content.corner.0 * 8.0) / content.scale,
                (pixel.1 + content.corner.1 * 8.0) / content.scale,
            )
        };
        let frame = map_screen::Frame::new(&content, &[leader], Some(scroll));
        assert_eq!(frame.picture_offset, map_screen::picture_offset(at));
        assert_eq!(frame.sprites[1].dst, [124.0, 92.0, 8.0, 8.0], "{code}");
        // A 20-pixel move is inside the 30-pixel dead zone: the picture holds, the dot moves.
        let moved = (at.0 + 20.0, at.1);
        assert_eq!(map_screen::follow(scroll, moved), scroll, "{code}");
        let frame = map_screen::Frame::new(&content, &[world(moved)], Some(scroll));
        assert_eq!(frame.sprites[1].dst, [144.0, 92.0, 8.0, 8.0], "{code}");
        // A 40-pixel move carries the picture by the 10 pixels past the dead zone.
        let moved = (at.0 + 40.0, at.1);
        assert_eq!(
            map_screen::follow(scroll, moved),
            (at.0 + 10.0, at.1),
            "{code}"
        );
        // The layout's backdrop tiles the 256×192 screen: 64×64 cells, four columns, three rows.
        let backdrop = content.backdrop.as_ref().expect(code);
        assert_eq!(backdrop.size(), (64, 64), "{code}");
        let tiles = map_screen::backdrop_tiles(backdrop.size(), [0.0, 0.0, 256.0, 192.0]);
        assert_eq!(tiles.len(), 12, "{code}");
    }
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
