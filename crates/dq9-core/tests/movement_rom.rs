//! Movement and collision on the real game data; skipped when the ROM is absent.

use dq9_core::fx;
use dq9_core::{CollisionWorld, Hero, Input};
use dq9_formats::Table;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList};
use glam::Vec3;
use nds_rom::Rom;

fn word(data: &[u8], base: u32, address: u32) -> i32 {
    let at = (address - base) as usize;
    i32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

/// The arctangent table and the constants the port copies are the ROM's.
#[test]
fn tables_and_constants_match_the_rom() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let arm9 = rom.arm9().unwrap();
    // 0x020ed460: 129 entries of `round(4096 atan(i / 128))`.
    for i in 0..129usize {
        let at = (0x020e_d460 - arm9.load_addr) as usize + 2 * i;
        let entry = i16::from_le_bytes([arm9.data[at], arm9.data[at + 1]]);
        assert_eq!(entry, fx::atan_entry(i), "entry {i}");
    }
    let main = |address: u32| word(&arm9.data, arm9.load_addr, address);
    // Top speed, corner-trap thresholds, camera turn limits.
    assert_eq!(main(0x0203_2fc4), dq9_core::movement::WALK_SPEED);
    assert_eq!(main(0x0203_19e0), 0xfd7);
    assert_eq!(main(0x0203_19e4), 0x4cc);
    assert_eq!(main(0x020a_2718), 0xc91);
    assert_eq!(main(0x020a_271c), 0x57f7);
    assert_eq!(main(0x020a_2714) as u32, 0.0015f32.to_bits());
    // The d-pad headings (`0x020e90b8`): up, down, left, right, then the diagonals.
    let headings: Vec<i32> = (0..8).map(|i| main(0x020e_90b8 + 4 * i)).collect();
    assert_eq!(
        headings,
        [0x3244, 0, 0x4b66, 0x1922, 0x3ed5, 0x25b3, 0x57f7, 0xc91]
    );
    // Overlay 17: the move limit and the slide floats, the probe offsets of the ledge test.
    let overlay = rom
        .overlays()
        .unwrap()
        .into_iter()
        .find(|o| o.id == 17)
        .expect("overlay 17");
    let data = rom.overlay_data(&overlay).unwrap();
    let field = |address: u32| word(&data, overlay.ram_addr, address);
    assert_eq!(field(0x0218_e660), 0x333);
    assert_eq!(field(0x0218_e664) as u32, 10294.4f32.to_bits());
    assert_eq!(field(0x0218_e668), 0x1922);
    assert_eq!(field(0x0218_e66c) as u32, 0x45d3_a50e);
    assert_eq!(field(0x0218_e670) as u32, 0x4420_d99a);
    assert_eq!(field(0x0218_f56c), dq9_core::movement::WALK_SPEED);
    let probe = |at: u32| -> Vec<i32> { (0..8).map(|i| field(at + 4 * i)).collect() };
    let table = |address: u32| word(&data, overlay.ram_addr, address) as u32;
    let (x, z) = (probe(table(0x0219_3288)), probe(table(0x0219_328c)));
    assert_eq!(x, [-0x400, 0, 0x400, 0, -0x2e1, 0x2e1, -0x2e1, 0x2e1]);
    assert_eq!(z, [0, -0x400, 0, 0x400, -0x2e1, -0x2e1, 0x2e1, 0x2e1]);
}

fn load(rom: &Rom<'_>, id: u16) -> CollisionWorld {
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let record = list.find(id).unwrap();
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    let map = Map::assemble(record, &table, &list, |p| map::rom_file(rom, p), &ctx).unwrap();
    CollisionWorld::build(&map, |p| map::rom_file(rom, p)).unwrap()
}

/// The hero of Angel Falls walks in every direction from the village square without leaving
/// the ground: its height stays within a couple of units and it makes progress.
#[test]
fn the_hero_walks_the_village_without_falling() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let world = load(&rom, 1100);
    let ground = world.ground_height(-19.5, -9.0, 3.0).unwrap();
    let start = Vec3::new(-19.5, ground, -9.0);
    let keys = [
        Input::from_direction(Vec3::NEG_Z),
        Input::from_direction(Vec3::Z),
        Input::from_direction(Vec3::X),
        Input::from_direction(-Vec3::X),
        Input::from_direction(Vec3::new(1.0, 0.0, 1.0)),
    ];
    for input in keys {
        let mut hero = Hero::at(start, 0.0);
        let mut lowest = f32::MAX;
        for _ in 0..240 {
            hero.step(&world, input, 0, 1);
            lowest = lowest.min(hero.position().y);
        }
        assert!(
            (hero.position().y - start.y).abs() < 2.0 && lowest > start.y - 2.0,
            "{input:?}: {:?}",
            hero.position()
        );
        assert!(hero.position().distance(start) > 0.5, "{input:?} stuck");
    }
}
