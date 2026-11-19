//! The hero on a real staircase: the `.col2` ramp of `X01M05` (4105) climbed and descended at
//! the game's top speed, against the same walk on the flat floor beside it
//! (`docs/re/movement.md` § "Stairs and the run cycle"). Real-ROM test: skips without a ROM.
//!
//! The per-update table (`--nocapture`) is the evidence of batch `j4-stair-run`: on the stairs
//! the hero keeps moving at the top speed along the slope (3D), but its horizontal displacement,
//! which the application turns into the gait, shrinks by the cosine of the slope.

use dq9_core::{CollisionWorld, Hero, Input};
use dq9_formats::Table;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList};
use glam::Vec3;
use nds_rom::Rom;

/// The two ticks of one 30 Hz game update in seconds (`Hero::step(…, 2)` at 60 ticks/s).
const UPDATE: f32 = 2.0 / 60.0;
/// Top speed in world units per second: `0x189` per tick (`movement.md`).
const TOP_SPEED: f32 = 0x189 as f32 / 4096.0 * 60.0;

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

/// One game update of a walk, as the application measures it.
#[derive(Debug, Clone, Copy)]
struct Row {
    position: Vec3,
    fx: [i32; 3],
    /// Hero state 1 (keys held, still on the floor): the game plays `run` for it.
    walking: bool,
    speed: i32,
    /// Displacement over the update in units per second, horizontal and along the slope.
    horizontal: f32,
    slope: f32,
}

/// Walks `updates` game updates from `(x, z)` standing on the floor, holding `direction`.
fn walk(world: &CollisionWorld, x: f32, z: f32, direction: Vec3, updates: usize) -> Vec<Row> {
    let ground = world.ground_height(x, z, 5.0).unwrap();
    let mut hero = Hero::at(Vec3::new(x, ground, z), 0.0);
    let input = Input::from_direction(direction);
    let mut rows = Vec::new();
    for _ in 0..updates {
        let before = hero.position();
        hero.step(world, input, 0, 2);
        let moved = hero.position() - before;
        rows.push(Row {
            position: hero.position(),
            fx: hero.position_fx(),
            walking: hero.is_walking(),
            speed: hero.speed(),
            horizontal: Vec3::new(moved.x, 0.0, moved.z).length() / UPDATE,
            slope: moved.length() / UPDATE,
        });
    }
    rows
}

fn print_table(title: &str, rows: &[Row]) {
    println!("== {title}");
    println!("upd  x      y      z       speed walking horiz u/s  slope u/s");
    for (i, r) in rows.iter().enumerate() {
        println!(
            "{i:3}  {:6.3} {:6.3} {:7.3} {:#5x} {:7} {:9.2} {:9.2}",
            r.position.x, r.position.y, r.position.z, r.speed, r.walking, r.horizontal, r.slope
        );
    }
}

/// Rows that moved the hero along the floor at the top speed (the ramp-up rows move it slower:
/// a tick's displacement uses the previous tick's speed).
fn at_full_speed(rows: &[Row]) -> impl Iterator<Item = &Row> {
    rows.iter()
        .filter(|r| r.speed == 0x189 && (r.slope - TOP_SPEED).abs() < 0.05)
}

/// The 36° ramp of `X01M05` (`ny = 0xd05`, triangles from `(±1.45, −0.35, −5.5)` up to
/// `(±1.45, 2.15, −9)`): the hero climbs it north and comes back down south. The state stays
/// walking and the speed along the slope is the top speed on the flat and on the stairs; only
/// the horizontal displacement drops on the slope, to the top speed times the slope's cosine.
#[test]
fn the_hero_runs_the_staircase_at_the_flat_speed() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let world = load(&rom, 4105);

    let up = walk(&world, 0.0, -2.0, Vec3::NEG_Z, 40);
    let down = walk(&world, 0.0, -8.8, Vec3::Z, 40);
    print_table("climb: flat floor then the stairs, north", &up);
    print_table("descent: stairs then the flat floor, south", &down);

    // Same state and same speed along the floor everywhere: no falling, no cancelled motion.
    for r in up.iter().chain(&down) {
        assert!(r.walking, "{r:?}");
    }
    // The flat floor south of the staircase (`z > -5.4`) and the staircase itself
    // (`z < -5.7`: the ramp starts at `z = -5.5`).
    let flat: Vec<&Row> = at_full_speed(&up).filter(|r| r.position.z > -5.4).collect();
    let stairs_up: Vec<&Row> = at_full_speed(&up).filter(|r| r.position.z < -5.7).collect();
    let stairs_down: Vec<&Row> = at_full_speed(&down)
        .filter(|r| r.position.z < -5.7)
        .collect();
    assert!(flat.len() > 8 && stairs_up.len() > 8 && stairs_down.len() > 8);
    for r in &flat {
        assert!((r.horizontal - TOP_SPEED).abs() < 0.05, "{r:?}");
    }
    // The ramp's cosine: normal y `0xd05 / 0x1000` = 0.813.
    for r in stairs_up.iter().chain(&stairs_down) {
        assert!(r.position.y > -0.3 && r.position.y < 2.2, "{r:?}");
        assert!(
            (r.horizontal - TOP_SPEED * 0xd05 as f32 / 4096.0).abs() < 0.1,
            "{r:?}"
        );
    }
    // The climb rises and the descent falls monotonically (no snapping between steps).
    assert!(
        stairs_up
            .windows(2)
            .all(|w| w[1].position.y > w[0].position.y)
    );
    assert!(
        stairs_down
            .windows(2)
            .all(|w| w[1].position.y < w[0].position.y)
    );

    // Physics pin: the end of both walks, bit-exact (`j4-stair-run` changes no movement code).
    assert_eq!(up.last().unwrap().fx, [0, 7119, -33928]);
    assert_eq!(down.last().unwrap().fx, [0, -981, -14741]);
}
