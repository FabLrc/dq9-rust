//! The battle item consumer's category rule, read on the EU item tables: tools clear a held
//! slot, equipment does not. `docs/re/battle.md`, "Using a worn item". Skipped without a ROM.

use dq9_core::battle::item::clears_held_slot;
use dq9_formats::Gpc2;
use nds_rom::Rom;

/// One English member of an `itemdt_<category>.gp2` archive, decompressed.
fn item_member(rom: &Rom, category: &str) -> Vec<u8> {
    let path = format!("data/prm/itemdt_{category}.gp2");
    let archive = Gpc2::parse(rom.file_by_path(&path).unwrap()).unwrap();
    let name = format!("itemdt_{category}_en.nat");
    archive
        .member(archive.find(&name).unwrap())
        .unwrap()
        .into_owned()
}

/// Every record of the nine tables: the equipment classes (0–7) fall on the consumer's "leave
/// the equipment alone" side, the tools (8–9, with 10 in the code) on its "clear the held slot"
/// side. The prayer ring is the only equipment record carrying the used-up bit the consumer
/// first tests, which is why it can still break (`func_02083738`, case 7).
#[test]
fn tools_clear_a_held_slot_and_equipment_never_does() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut equipment, mut tools) = (0, 0);
    let mut equipment_used_up = Vec::new();
    for category in ["a", "b", "d", "h", "l", "s", "t", "u", "w"] {
        let table = item_member(&rom, category);
        let count = usize::from(u16::from_le_bytes([table[0], table[1]]));
        for index in 0..count {
            let at = 12 + index * 0x20;
            let id = i16::from_le_bytes([table[at + 0x18], table[at + 0x19]]);
            let word = u32::from_le_bytes([
                table[at + 8],
                table[at + 9],
                table[at + 0xa],
                table[at + 0xb],
            ]);
            let class = (word & 0xf) as u8;
            match class {
                0..=7 => {
                    assert!(!clears_held_slot(class), "item {id}: class {class}");
                    equipment += 1;
                    if (word >> 19) & 1 == 1 {
                        equipment_used_up.push(id);
                    }
                }
                8..=10 => {
                    assert!(clears_held_slot(class), "item {id}: class {class}");
                    tools += 1;
                }
                other => panic!("item {id}: category {other} is outside the consumer's classes"),
            }
        }
    }
    assert_eq!((equipment, tools), (944, 234), "the nine tables' records");
    assert_eq!(
        equipment_used_up,
        vec![18023],
        "the prayer ring is the one worn item the consumer can break"
    );
}
