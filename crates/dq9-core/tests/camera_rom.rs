//! The field camera's map lock (`ov_017::0219e234`) against `maplist9.bin`; skipped when the
//! ROM is absent.
//!
//! The rule the original applies on every map change is
//! `kind != 0 && (+0xc bit 7)`. It is checked on the record of each map named in the batch,
//! including the two melonDS observations and the maps that falsify a rule based on the code
//! letter (`F07M02` is a kind-2 interior; `F99M01` is a kind-1 map without bit 7), then as an
//! invariant over the whole list.

use dq9_core::camera::rotation_allowed;
use dq9_formats::Table;
use dq9_formats::maplist::{self, MapList};
use nds_rom::Rom;

/// `(id, kind, +0xc) of every map of the list that has an id and a code.
fn records(bytes: &[u8]) -> Vec<(u16, u8, u8)> {
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    list.records
        .iter()
        .filter(|r| r.id != 0 && !r.code.is_empty())
        .map(|r| (r.id, r.kind(), r.byte_c))
        .collect()
}

fn record(records: &[(u16, u8, u8)], id: u16) -> (u16, u8, u8) {
    *records
        .iter()
        .find(|r| r.0 == id)
        .unwrap_or_else(|| panic!("map {id} in the list"))
}

#[test]
fn the_rule_matches_the_maps_the_original_was_observed_on() {
    let Some(bytes) = testrom::load() else { return };
    let maps = records(bytes);
    // melonDS `build/oracle/field-F02/states/f02-enter-state.bin`: map 20002, camera
    // `+0x244` = 0x10, `L`/`R` do nothing (build/evidence/j4-vegetation-billboards).
    let (_, kind, byte_c) = record(&maps, 20002);
    assert_eq!((kind, byte_c & 0x80), (0, 0));
    assert!(!rotation_allowed(byte_c));
    // Same states, `church-state.bin` (109) and `town-state.bin` (100): `+0x244` = 0x00 and
    // `L`/`R` turn the camera.
    for id in [109, 100] {
        assert!(rotation_allowed(record(&maps, id).2), "map {id}");
    }
    // Other states in `build/`: the prologue maps allow the turn (0x00), `F01M01` does not
    // (0x10).
    for id in [4101, 4103, 4105, 4107, 4109] {
        assert!(rotation_allowed(record(&maps, id).2), "map {id}");
    }
    assert!(!rotation_allowed(record(&maps, 20101).2));
}

#[test]
fn the_rule_is_not_the_code_letter() {
    let Some(bytes) = testrom::load() else { return };
    let maps = records(bytes);
    // `F07M02` and `F34M01` are named `F` but are kind-2 interiors with bit 7: allowed.
    for id in [20207, 20341] {
        let (_, kind, byte_c) = record(&maps, id);
        assert_eq!(kind, 2, "map {id}");
        assert!(rotation_allowed(byte_c), "map {id}");
    }
    // This one is locked although its code starts with `F`: kind 1, bit 7 clear.
    let (_, kind, byte_c) = record(&maps, 19998);
    assert_eq!(kind, 1);
    assert!(!rotation_allowed(byte_c));
    // K03 is kind 0 with bit 7 set: still locked.
    let (_, kind, byte_c) = record(&maps, 3);
    assert_eq!((kind, byte_c & 0x80), (0, 0x80));
    assert!(!rotation_allowed(byte_c));
}

#[test]
fn the_whole_list_splits_82_locked_789_turning() {
    let Some(bytes) = testrom::load() else { return };
    let maps = records(bytes);
    assert_eq!(maps.len(), 871);
    let locked = maps.iter().filter(|r| !rotation_allowed(r.2)).count();
    assert_eq!(locked, 82);
    assert_eq!(maps.len() - locked, 789);
    // Every locked map has kind 0 or no bit 7; every turning map has another kind and bit 7.
    for (id, kind, byte_c) in &maps {
        let rule = *kind != 0 && byte_c & 0x80 != 0;
        assert_eq!(rotation_allowed(*byte_c), rule, "map {id}");
    }
    // The 65 terrain regions (`F` maps and `K03`) are exactly the kind-0 maps.
    assert_eq!(maps.iter().filter(|r| r.1 == 0).count(), 65);
}
