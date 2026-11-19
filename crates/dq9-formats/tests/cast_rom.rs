//! The seated NPCs of the ROM: the placements the sprite code draws with a `<name>_s.spr`
//! sheet, tilted and rolled by the camera (`docs/app/rendering.md` § NPCs). Skipped without a ROM.

use dq9_formats::cast::{self, Story};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::{Table, map};
use nds_rom::Rom;

/// Every seated NPC (flag `0x800`, record 15) that has its `_s.spr` sheet, over every map and
/// the start of every chapter by day and by night: `(map id, story chapter, daytime, NPC id,
/// sheet, angle, seat angle)`.
fn seated(rom: &Rom) -> Vec<(u16, u8, bool, u16, String, i32, i32)> {
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let exists = |p: &str| map::rom_file(rom, p).is_some();
    let mut found = Vec::new();
    for record in &list.records {
        let Some(archive) = map::rom_file(rom, &cast::archive_path(record.code)) else {
            continue;
        };
        for chapter in 1..=19 {
            for daytime in [true, false] {
                let story = Story {
                    chapter,
                    step: 1,
                    substep: 1,
                    daytime,
                    ..Story::default()
                };
                let c = cast::load(archive, record.id, &story).unwrap();
                for npc in &c.npcs {
                    if npc.placement.flags & 0x800 == 0 {
                        continue;
                    }
                    if let cast::Appearance::Sprite { sheet } = npc.appearance(exists)
                        && sheet.ends_with("_s.spr")
                    {
                        found.push((
                            record.id,
                            chapter,
                            daytime,
                            npc.member.id,
                            sheet,
                            i32::from(npc.placement.angle),
                            npc.placement.seat_angle,
                        ));
                    }
                }
            }
        }
    }
    found
}

/// 742 seated placements have their `_s.spr` sheet; the sleeper of Stornway Castle's cell
/// (`C01M15`) is the one `animation/seated_rom.rs` draws, so its angle is pinned here.
#[test]
fn seated_npcs_have_their_sheets() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let found = seated(&rom);
    assert_eq!(found.len(), 742);
    let cell = (115, 1, true, 41, "data/ani/n019a_s.spr".to_owned(), 3850, 0);
    assert!(found.contains(&cell), "the cell's sleeper");
    // Some carry a type 15 angle (`+0x3c`), which the sprite code adds to the NPC's own.
    assert!(found.iter().any(|f| f.6 != 0), "a seat angle");
}
