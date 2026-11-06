//! The Observatory tower's Lv 2 -> Lv 3 door on the real game data; skipped without the ROM.
//!
//! `X01M01` (Lv 1-Lv 2 / Exterior 1) joins `X01M03` (Lv 3 / Exterior 3) through a plain
//! `0x72` warp of type 0, not a door task and not a type-1 zone (docs/re/map-archive.md,
//! § « Zones de type 1 et passage de hauteur de l'Observatoire »). The arrival is the
//! destination map's own start point, which is what proves the transition is a normal
//! landing.

use dq9_formats::Table;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList};
use nds_rom::Rom;

fn assemble<'a>(rom: &Rom<'a>, table: &'a Table<'a>, list: &'a MapList<'a>, code: &str) -> Map {
    let record = list
        .records
        .iter()
        .find(|r| r.code == code.as_bytes())
        .unwrap_or_else(|| panic!("map {code} in the list"));
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    Map::assemble(record, table, list, |p| map::rom_file(rom, p), &ctx).unwrap()
}

#[test]
fn the_tower_door_is_a_plain_warp_onto_the_next_floor() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let source = assemble(&rom, &table, &list, "X01M01");
    let target = assemble(&rom, &table, &list, "X01M03");

    // Warp 9 of X01M01: 4 x 4 x 3 box on the upper terrace.
    let warp = source
        .regions
        .warps
        .iter()
        .find(|w| target_id(&list, "X01M03") == Some(w.destination))
        .expect("a warp from X01M01 to X01M03");
    assert_eq!(warp.kind, 0, "not the animated (task 8) kind");
    assert_close(
        warp.volume.center.map(|c| c as f32 / 4096.0),
        [-8.226, 15.5, 16.379],
    );
    let high = warp.volume.high.map(|c| c as f32 / 4096.0);
    let low = warp.volume.low.map(|c| c as f32 / 4096.0);
    assert_close(high, [-6.226, 17.5, 17.879]);
    assert_close(low, [-10.226, 13.5, 14.879]);
    assert_close(warp.arrival.map(|c| c as f32 / 4096.0), [-9.75, 4.2, 17.62]);
    let angle = f32::from(warp.arrival_angle) / 4096.0;
    assert!((angle + 2.303_835).abs() < 1e-3, "arrival angle {angle}");

    // The arrival lands on X01M03's own start point (it has its own origin in y).
    let start = target.block_list.start;
    let landing: Vec<f32> = start.position.iter().map(|c| *c as f32 / 4096.0).collect();
    assert!((landing[0] - -9.75).abs() < 1.4, "start x {landing:?}");
    assert!((landing[1] - 4.2).abs() < 0.5, "start y {landing:?}");
    assert!((landing[2] - 17.62).abs() < 1.4, "start z {landing:?}");

    // The three type-1 zones of X01M01 survive parsing but carry no destination and no
    // runtime state: Field_CheckRegions has no list for type 1 (0x0219755c).
    let type1: Vec<_> = source.regions.zones_of(1).collect();
    assert_eq!(type1.len(), 3);
    for zone in &type1 {
        assert_eq!(zone.class, 0);
        assert!(zone.warp.is_none());
        assert_close(zone.volume.size.map(|c| c as f32 / 4096.0), [1.0, 1.0, 1.0]);
    }
    assert!(
        source.regions.warps.iter().all(|w| w.kind != 1),
        "X01M01 has no type-1 warp"
    );
}

fn assert_close(actual: [f32; 3], expected: [f32; 3]) {
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() < 0.01, "{actual:?} != {expected:?}");
    }
}

fn target_id(list: &MapList<'_>, code: &str) -> Option<u16> {
    list.records
        .iter()
        .find(|r| r.code == code.as_bytes())
        .map(|r| r.id)
}
