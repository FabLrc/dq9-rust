//! The mini-map data on the real game image, and every Nitro 2D file the cartridge holds.
//! Skipped when the ROM is absent (`DQ9_ROM`).

use std::collections::BTreeSet;

use dq9_formats::Gpc2;
use dq9_formats::minimap::{self, Layout, Pac, Picture};
use nds_rom::nitro2d::{self, Kind, Ncer};
use nds_rom::{Narc, Rom, compress};

const ARCHIVE: &str = "data/pack_lv5/minimap.gp2";
const SPRITE_CELLS: &str = "data/ani/obj_minimap.NCER";
const SPRITE_PACK: &str = "data/ani/obj_mm.pac";

#[derive(Default)]
struct Counts {
    palette: usize,
    character: usize,
    cell: usize,
}

impl Counts {
    fn walk(&mut self, data: &[u8], depth: usize) {
        match Kind::of(data) {
            Some(Kind::Palette) => {
                nitro2d::Nclr::parse(data).unwrap();
                self.palette += 1;
            }
            Some(Kind::Character) => {
                nitro2d::Ncgr::parse(data).unwrap();
                self.character += 1;
            }
            Some(Kind::Cell) => {
                nitro2d::Ncer::parse(data).unwrap();
                self.cell += 1;
            }
            None => self.container(data, depth),
        }
    }

    fn container(&mut self, data: &[u8], depth: usize) {
        if depth > 3 {
            return;
        }
        if Gpc2::is_gpc2(data) {
            let archive = Gpc2::parse(data).unwrap();
            for entry in &archive.entries {
                if let Ok(member) = archive.member(entry) {
                    self.walk(&member, depth + 1);
                }
            }
        } else if Narc::is_narc(data) {
            let narc = Narc::parse(data).unwrap();
            for index in 0..narc.len() as u16 {
                self.walk(narc.file(index).unwrap(), depth + 1);
            }
        } else if minimap::pac::is_pac(data) {
            if let Ok(pack) = Pac::parse(data) {
                for member in &pack.members {
                    self.walk(member.data, depth + 1);
                }
            }
        } else if depth == 0 {
            // Compressed containers (`.chr`, `.ambl`) are NARCs behind a BIOS header. A table
            // whose first word is 16 is not one: its "stream" declares nothing.
            if let Ok(plain) = compress::decompress(data)
                && plain.len() > 4
            {
                self.walk(&plain, depth + 1);
            }
        }
    }
}

/// Every Nitro 2D file reachable from the filesystem, in GPC2 and NARC archives and in packs
/// parses; the counts are the cartridge's (139 palettes, 224 characters, 138 cells).
#[test]
fn every_nitro_2d_file_parses() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut counts = Counts::default();
    for file in &rom.files.files {
        counts.walk(rom.file(file.id).unwrap(), 0);
    }
    assert_eq!(
        (counts.palette, counts.character, counts.cell),
        (139, 224, 138),
        "2D files found"
    );
}

#[test]
fn layouts_and_pictures_of_the_mini_map() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = Gpc2::parse(rom.file_by_path(ARCHIVE).unwrap()).unwrap();
    let names = |suffix: &str| -> Vec<String> {
        archive
            .entries
            .iter()
            .filter(|e| e.name.ends_with(suffix))
            .map(|e| e.name.clone())
            .collect()
    };
    let layouts = names(".bmmp");
    let pictures = names(".obg");
    assert_eq!(layouts.len(), 283);
    assert_eq!(pictures.len(), 268);
    assert_eq!(archive.entries.len(), 568);

    // Every layout parses through the tagged table reader and names an archive picture.
    let mut named = BTreeSet::new();
    for name in &layouts {
        let data = archive.member(archive.find(name).unwrap()).unwrap();
        let parsed = Layout::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        let picture = parsed
            .picture()
            .unwrap_or_else(|| panic!("{name}: no picture"));
        assert!(
            pictures
                .iter()
                .any(|p| p.strip_suffix(".obg") == Some(picture)),
            "{name}: picture {picture} absent"
        );
        assert!(parsed.scale().is_some(), "{name}: no scale");
        assert!(parsed.corner().is_some(), "{name}: no corner");
        named.insert(picture.to_owned());
    }
    assert_eq!(named.len(), 244);

    // Every picture obeys the size formula; every cell names a tile below the count and sets
    // no bit above bit 9 (both enforced by the parser).
    let mut cells = 0;
    let mut tiles = 0;
    for name in &pictures {
        let data = archive.member(archive.find(name).unwrap()).unwrap();
        let parsed = Picture::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        cells += parsed.cells.len();
        tiles += usize::from(parsed.tile_count);
        assert_eq!(parsed.unknown_02, 0, "{name}: unknown_02");
        assert_eq!(parsed.unknown_06, 0, "{name}: unknown_06");
    }
    assert_eq!((tiles, cells), (97_930, 135_168));
}

/// `pixel = position × scale − corner × 8` on the examples the wiki states: the village's road
/// mark on row 148 of 152, the pass's two marks on rows 4 and 187 of 200.
#[test]
fn the_position_formula_lands_where_the_wiki_says() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = Gpc2::parse(rom.file_by_path(ARCHIVE).unwrap()).unwrap();

    let data = archive.member(archive.find("M01.bmmp").unwrap()).unwrap();
    let village = Layout::parse(&data).unwrap();
    assert_eq!(village.scale(), Some(3.2));
    assert_eq!(village.corner(), Some((-18.0, -12.0)));
    let road = village
        .marks()
        .into_iter()
        .find(|m| m.maps.contains(&20001))
        .unwrap();
    let (column, row) = village.pixel(road.x, road.z).unwrap();
    assert!((column - 121.088).abs() < 0.01, "column {column}");
    assert_eq!(row.round() as i32, 148);
    let picture = {
        let data = archive
            .member(archive.find("M01M0001.obg").unwrap())
            .unwrap();
        Picture::parse(&data).unwrap().image()
    };
    assert_eq!((picture.width, picture.height), (280, 152));

    let data = archive
        .member(archive.find("S01M01.bmmp").unwrap())
        .unwrap();
    let pass = Layout::parse(&data).unwrap();
    assert_eq!(pass.scale(), Some(2.0));
    // File order: the mark at the pass's far end (row 187), then its first (row 4).
    let pixels: Vec<(f32, f32)> = pass
        .marks()
        .into_iter()
        .map(|m| pass.pixel(m.x, m.z).unwrap())
        .collect();
    assert_eq!(pixels.len(), 2);
    for ((x, z), (ex, ez)) in pixels.iter().zip([(42.96, 187.38), (108.66, 3.62)]) {
        assert!((x - ex).abs() < 0.01 && (z - ez).abs() < 0.01, "({x}, {z})");
    }

    let data = archive.member(archive.find("F01.bmmp").unwrap()).unwrap();
    let field = Layout::parse(&data).unwrap();
    assert_eq!(field.scale(), Some(1.0));
    let data = archive
        .member(archive.find("F01M0001.obg").unwrap())
        .unwrap();
    let picture = Picture::parse(&data).unwrap();
    assert_eq!((picture.width, picture.height), (32, 28));
    let (width, height) = (picture.image().width, picture.image().height);
    for mark in field.marks() {
        let (x, z) = field.pixel(mark.x, mark.z).unwrap();
        assert!((-64.0..width as f32 + 64.0).contains(&x));
        assert!((-64.0..height as f32 + 64.0).contains(&z));
    }
}

/// The party panel is cell 2 of `obj_minimap.NCER`: 64×64, whose top 16 rows are the strip.
#[test]
fn the_party_strip_and_the_dots() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let ncer = Ncer::parse(rom.file_by_path(SPRITE_CELLS).unwrap()).unwrap();
    assert_eq!(ncer.cells.len(), 44);
    assert_eq!(ncer.mapping, 0);
    let panel = ncer.cells[minimap::PARTY_PANEL_CELL].bounds().unwrap();
    assert_eq!(
        (panel.x, panel.y, panel.width, panel.height),
        (0, 0, 64, 64)
    );
    let strip = minimap::party_strip(&ncer.cells).unwrap();
    assert_eq!(
        (strip.x, strip.y, strip.width, strip.height),
        (0, 0, 64, 16)
    );

    // The five dots: one 8×8 part each, in palette slot 4, tiles 0x48 to 0x4c.
    for (index, cell) in ncer.cells.iter().enumerate().take(8).skip(3) {
        assert_eq!(cell.parts.len(), 1, "cell {index}");
        let part = &cell.parts[0];
        let box_ = cell.bounds().unwrap();
        assert_eq!((box_.width, box_.height), (8, 8));
        assert_eq!(part.palette, 4);
        assert_eq!(part.tile, 0x48 + (index as u16 - 3));
    }

    // The sprite pack holds the same three files.
    let pack = Pac::parse(rom.file_by_path(SPRITE_PACK).unwrap()).unwrap();
    assert_eq!(
        pack.names().collect::<Vec<_>>(),
        ["obj_mm.NCER", "obj_mm.NCGR", "obj_mm.NCLR"]
    );
    let ncer = Ncer::parse(pack.find("obj_mm.NCER").unwrap().data).unwrap();
    assert_eq!(ncer.cells.len(), 28);
}

/// A `.obg` picture decodes to its declared pixel size with colour 0 transparent.
#[test]
fn a_picture_decodes_to_rgba() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = Gpc2::parse(rom.file_by_path(ARCHIVE).unwrap()).unwrap();
    let data = archive
        .member(archive.find("M01M0001.obg").unwrap())
        .unwrap();
    let picture = Picture::parse(&data).unwrap();
    let image = picture.image();
    assert_eq!((image.width, image.height), (280, 152));
    assert_eq!(image.rgba.len(), 280 * 152);
    // The village's paper edge: 5,735 clear pixels, the rest opaque.
    let clear = image.rgba.iter().filter(|p| p[3] == 0).count();
    assert_eq!(clear, 5_735);
    assert!(image.rgba.iter().any(|p| p[3] == 255));
    assert_eq!(picture.tile(0).unwrap().len(), 32);
}

/// Every `.pac` of the cartridge parses but the five `tdata_<lang>.pac` tables, which are
/// another format. The packs are 501 files: 126 on the filesystem, 375 members of GPC2
/// archives (the wiki counted 468, the packs of its sample).
#[test]
fn every_pac_parses_except_the_tdata_files() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut packs: Vec<(String, Vec<u8>)> = Vec::new();
    for file in &rom.files.files {
        if file.path.ends_with(".pac") {
            packs.push((file.path.clone(), rom.file(file.id).unwrap().to_vec()));
        }
    }
    for file in &rom.files.files {
        let data = rom.file(file.id).unwrap();
        if !Gpc2::is_gpc2(data) {
            continue;
        }
        let archive = Gpc2::parse(data).unwrap();
        for entry in &archive.entries {
            if entry.name.ends_with(".pac")
                && let Ok(member) = archive.member(entry)
            {
                packs.push((entry.name.clone(), member.into_owned()));
            }
        }
    }
    let mut narcs = 0;
    let mut parsed = 0;
    let mut rejected = Vec::new();
    for (name, data) in &packs {
        if Narc::is_narc(data) {
            narcs += 1;
            continue;
        }
        match Pac::parse(data) {
            Ok(_) => parsed += 1,
            Err(e) => rejected.push(format!("{name}: {e}")),
        }
    }
    assert_eq!(packs.len(), 501);
    assert_eq!(narcs, 33);
    assert_eq!(parsed, 463, "{rejected:#?}");
    assert_eq!(rejected.len(), 5);
    for name in &rejected {
        assert!(name.starts_with("tdata_"), "{name} should have parsed");
    }
}

/// A `.bmmp` mark names maps, never a marker: its integers are all map-list ids
/// (`data/map/maplist9.bin`), and no layout's picture (`0x66`) or backdrop (`0x6a`) names one
/// of the archive's `marker0`-`marker4` tiles. 273 records over 91 layouts, 889 ids.
#[test]
fn a_mark_names_maps_and_no_layout_names_a_marker() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = Gpc2::parse(rom.file_by_path(ARCHIVE).unwrap()).unwrap();
    for index in 0..5 {
        assert!(
            archive.find(&format!("marker{index}.obg")).is_some(),
            "marker{index} in {ARCHIVE}"
        );
    }
    let table =
        dq9_formats::Table::parse(rom.file_by_path(dq9_formats::maplist::PATH).unwrap()).unwrap();
    let maps = dq9_formats::maplist::MapList::parse(&table).unwrap();
    assert_eq!(maps.records.len(), 1010);

    let mut layouts = Vec::new();
    for entry in &archive.entries {
        if entry.name.ends_with(".bmmp") {
            layouts.push(entry.name.clone());
        }
    }
    let (mut with_marks, mut marks, mut ids) = (0, 0, 0);
    for name in &layouts {
        let data = archive.member(archive.find(name).unwrap()).unwrap();
        let layout = Layout::parse(&data).unwrap();
        // Every string of the picture (0x66, a list) and backdrop (0x6a) tags: the only
        // `.obg` names the field screen loads.
        for record in layout.records(0x66).chain(layout.records(0x6a)) {
            for value in &record.values {
                if let dq9_formats::Value::Str(Some(bytes)) = value {
                    let field = String::from_utf8_lossy(bytes);
                    assert!(
                        !field.to_ascii_lowercase().starts_with("marker"),
                        "{name}: a marker picture {field}"
                    );
                }
            }
        }
        let found = layout.marks();
        with_marks += usize::from(!found.is_empty());
        marks += found.len();
        for mark in &found {
            assert!(!mark.maps.is_empty(), "{name}: a mark without a map");
            for &id in &mark.maps {
                ids += 1;
                assert!(
                    maps.find(id as u16).is_some(),
                    "{name}: mark {id} is no map"
                );
            }
        }
    }
    assert_eq!((with_marks, marks, ids), (91, 273, 889));

    // The village's nine marks: a house per room, one house standing for two floors, and the
    // field's mark on the road out of town.
    let data = archive.member(archive.find("M01.bmmp").unwrap()).unwrap();
    let village = Layout::parse(&data).unwrap();
    let found: Vec<Vec<u32>> = village.marks().into_iter().map(|m| m.maps).collect();
    assert_eq!(
        found,
        [
            vec![1101],
            vec![1102],
            vec![1103],
            vec![1104],
            vec![1105, 1109],
            vec![1106],
            vec![1107, 1110],
            vec![1108],
            vec![20001],
        ]
    );
}
