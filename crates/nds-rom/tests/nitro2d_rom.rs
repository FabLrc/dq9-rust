//! The loose Nitro 2D files of the game image; skipped when the ROM is absent (`DQ9_ROM`).

use nds_rom::Rom;
use nds_rom::nitro2d::{Kind, Ncer, Ncgr, Nclr, is_nitro2d};

/// The nine loose 2D files parse: `obj_minimap`, `tmap_field_obj` and `tmap_ufix_obj`, three
/// files each. The other 492 are archive members (packs, `GPC2`, NARC), counted by
/// `dq9-formats`' `minimap_rom` test.
#[test]
fn every_loose_2d_file_parses() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut counts = [0usize; 3];
    for file in &rom.files.files {
        let data = rom.file(file.id).unwrap();
        let Some(kind) = Kind::of(data) else {
            assert!(!is_nitro2d(data), "{}: sniffed but unreadable", file.path);
            continue;
        };
        match kind {
            Kind::Palette => {
                Nclr::parse(data).unwrap_or_else(|e| panic!("{}: {e}", file.path));
                counts[0] += 1;
            }
            Kind::Character => {
                Ncgr::parse(data).unwrap_or_else(|e| panic!("{}: {e}", file.path));
                counts[1] += 1;
            }
            Kind::Cell => {
                Ncer::parse(data).unwrap_or_else(|e| panic!("{}: {e}", file.path));
                counts[2] += 1;
            }
        }
    }
    assert_eq!(counts, [3, 3, 3]);
}

/// The mini-map's cells: 44 cells, the party panel at cell 2 as one 64×64 part, the mapping
/// one tile a number at 32 bytes.
#[test]
fn the_mini_map_cells() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = rom.file_by_path("data/ani/obj_minimap.NCER").unwrap();
    let ncer = Ncer::parse(data).unwrap();
    assert_eq!(ncer.cells.len(), 44);
    assert_eq!(ncer.tile_bytes().unwrap(), 32);
    assert_eq!(
        ncer.cells[2].bounds().unwrap(),
        nds_rom::nitro2d::Rect {
            x: 0,
            y: 0,
            width: 64,
            height: 64
        }
    );
    // The characters hold 209 tiles; the panel's 64 tiles start at tile 8, the dots at 0x48.
    let data = rom.file_by_path("data/ani/obj_minimap.NCGR").unwrap();
    let ncgr = Ncgr::parse(data).unwrap();
    assert_eq!(ncgr.tile_count(), 209);
    assert_eq!(ncer.cells[2].parts[0].tile, 8);
    // One palette file with eight stored palettes in slots 0 to 7.
    let data = rom.file_by_path("data/ani/obj_minimap.NCLR").unwrap();
    let nclr = Nclr::parse(data).unwrap();
    assert_eq!(nclr.colours_per_palette, 16);
    assert_eq!(nclr.slots, (0..8).collect::<Vec<_>>());
}
