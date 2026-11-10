//! Field regions on the real game data; skipped when the ROM is absent.

use std::collections::HashMap;

use dq9_core::field::{self, Event, Field, Leader, warp_contains, zone_contains};
use dq9_core::{CollisionWorld, Hero, Input};
use dq9_formats::Table;
use dq9_formats::map::{self, Context, Map, Regions, ZONE_DOORWAY};
use dq9_formats::maplist::{self, MapList};
use glam::Vec3;
use nds_rom::Rom;

/// Every distinct map archive set's regions, by map id.
fn all_regions(rom: &Rom<'_>) -> HashMap<u16, Regions> {
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    list.records
        .iter()
        .filter(|r| r.id != 0 && !r.code.is_empty())
        .filter_map(|r| {
            let map = Map::assemble(r, &table, &list, |p| map::rom_file(rom, p), &ctx).ok()?;
            Some((r.id, map.regions))
        })
        .collect()
}

/// The arrival of a warp or a doorway lies in front of the way back, not inside it: a hero
/// who arrives in a region would leave at once. This holds for the box test as read from the
/// code (`x' = x cos a - z sin a`) and fails for the opposite turn, which also pins the
/// direction of the rotation.
#[test]
fn arrivals_do_not_land_inside_a_return_region() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let all = all_regions(&rom);
    // The box test with the opposite turn, on floats.
    let opposite = |warp: &map::Warp, p: Vec3| {
        let v = &warp.volume;
        let to_f = |a: [i32; 3]| Vec3::from_array(a.map(|c| c as f32 / 4096.0));
        let d = p - to_f(v.center);
        let a = f32::from(v.angle) / 4096.0;
        let (s, c) = a.sin_cos();
        let local = Vec3::new(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
        let (hi, lo) = (to_f(v.high) - to_f(v.center), to_f(v.low) - to_f(v.center));
        local.cmple(hi).all() && local.cmpge(lo).all()
    };
    let (mut arrivals, mut inside, mut inside_opposite) = (0, 0, 0);
    let mut landed = Vec::new();
    for (id, regions) in &all {
        let doorway_arrivals = regions
            .zones_of(ZONE_DOORWAY)
            .filter_map(|z| z.warp.map(|w| (w.destination, w.arrival)));
        let warp_arrivals = regions.warps.iter().map(|w| (w.destination, w.arrival));
        for (destination, arrival) in doorway_arrivals.chain(warp_arrivals) {
            let Some(there) = all.get(&destination) else {
                continue;
            };
            let p = Vec3::from_array(arrival.map(|c| c as f32 / 4096.0));
            arrivals += 1;
            let hit = there.warps.iter().any(|w| warp_contains(w, p));
            inside += usize::from(hit);
            inside_opposite += usize::from(there.warps.iter().any(|w| opposite(w, p)));
            if hit {
                landed.push((*id, destination));
            }
        }
    }
    eprintln!(
        "{arrivals} arrivals, {inside} inside a warp ({inside_opposite} with the opposite turn): {landed:?}"
    );
    assert!(arrivals > 1300);
    assert!(inside <= 4, "{landed:?}");
    assert!(inside_opposite > inside);
}

/// An arrival inside a doorway of the destination faces out of it: the hero does not walk
/// into it (a doorway reacts to a hero who walks or looks against its facing).
#[test]
fn arrivals_inside_a_doorway_face_out_of_it() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let all = all_regions(&rom);
    let (mut inside, mut facing_out) = (0, 0);
    for regions in all.values() {
        let doorways = regions
            .zones_of(ZONE_DOORWAY)
            .filter_map(|z| z.warp.map(|w| (w.destination, w.arrival, w.arrival_angle)));
        for (destination, arrival, angle) in doorways {
            let Some(there) = all.get(&destination) else {
                continue;
            };
            let p = Vec3::from_array(arrival.map(|c| c as f32 / 4096.0));
            let heading = f32::from(angle) / 4096.0;
            for zone in there.zones_of(ZONE_DOORWAY).filter(|z| zone_contains(z, p)) {
                let door = f32::from(zone.volume.facing) / 4096.0;
                inside += 1;
                // dot((sin door, cos door), (sin heading, cos heading)) = cos(door - heading)
                facing_out += usize::from((door - heading).cos() > 0.0);
            }
        }
    }
    eprintln!("{facing_out} of {inside} arrivals inside a doorway face out of it");
    assert!(inside > 50);
    assert!(facing_out * 10 >= inside * 9);
}

/// The trigger file of every map loads into its field.
#[test]
fn every_map_builds_its_field() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let mut with_file = 0;
    for r in list
        .records
        .iter()
        .filter(|r| r.id != 0 && !r.code.is_empty())
    {
        let code = String::from_utf8_lossy(r.code).into_owned();
        let file = field::trigger_path(&code)
            .and_then(|p| map::rom_file(&rom, &p))
            .map(<[u8]>::to_vec);
        with_file += usize::from(file.is_some());
        Field::new(r.id, &Regions::default(), file).unwrap_or_else(|e| panic!("{code}: {e}"));
    }
    assert!(with_file > 400, "{with_file}");
}

/// The story gate at M01 2.2, on the real trigger file: the end of event 2210 attaches the
/// guest (action `0xcd` → `Guest_Start`), and only then does the mayor's line carry the
/// context `0xC0` whose end starts event 2230. Without the guest, condition 0x56 answers
/// "true", the line is `0xC1`, event 1 matches nothing and 2230 never starts — the exact
/// place a continuous run used to stall (`docs/re/trigger.md`, "The tag of `+0x56b`").
#[test]
fn the_guest_of_2210_opens_the_mayors_scene() {
    use dq9_core::trigger::Request;
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let file = field::trigger_path("M01M05")
        .and_then(|path| map::rom_file(&rom, &path))
        .map(<[u8]>::to_vec);
    assert!(file.is_some(), "M01 has a trigger file");
    let story = dq9_formats::cast::Story {
        chapter: 2,
        step: 2,
        substep: 1,
        ..Default::default()
    };
    let context = |trigger: field::TalkTrigger| -> Option<u16> {
        match trigger {
            field::TalkTrigger::Fired(events) => events.iter().find_map(|event| match event {
                Event::Request(Request::Talk { npc: 16, context }) => Some(*context),
                _ => None,
            }),
            other => panic!("the talk trigger did not fire: {other:?}"),
        }
    };
    let starts_2230 = |events: Vec<Event>| {
        events
            .iter()
            .any(|event| matches!(event, Event::Request(Request::Event(2230))))
    };

    // Without the guest: the 0x56(0) rule speaks the 0xC1 line and 2230 is not reached.
    let mut alone = field::initial_state(&story);
    let mut house = Field::new(1105, &Regions::default(), file.clone()).unwrap();
    let line = context(house.talk(16, 0, &mut alone)).expect("a talk rule fired");
    assert_eq!(line, 0xC1, "no guest, the fallback line");
    assert!(!starts_2230(house.talk_ended(16, line, None, &mut alone)));

    // The end of 2210 attaches the guest in room 1100 of the same area.
    let mut travelled = field::initial_state(&story);
    let mut town = Field::new(1100, &Regions::default(), file.clone()).unwrap();
    town.event_ended(2210, &mut travelled);
    assert!(
        travelled.guest_present(),
        "the 2210 rule runs 0xcd(1) and attaches the guest"
    );
    let line = context(house.talk(16, 0, &mut travelled)).expect("a talk rule fired");
    assert_eq!(line, 0xC0, "the guest makes condition 0x56 answer true");
    assert!(
        starts_2230(house.talk_ended(16, line, None, &mut travelled)),
        "the 0xC0 line's end starts event 2230"
    );
}

/// The end of event 2530 swaps the Hexagon's gate pair on the real `triggerD01.bin`: action
/// `0xa9(0, …)` is `func_020ae7d4`, which shows the block-0 object numbered 13 and hides the
/// one numbered 15 (`assemble.rs`: placement `+2` bit 2 is the hidden flag), and their
/// collision children follow. The trigger file holds `0xa9` once, in this rule.
#[test]
fn the_end_of_2530_swaps_the_hexagon_gate_pair() {
    use dq9_core::trigger::Request;
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let file = field::trigger_path("D01M01")
        .and_then(|path| map::rom_file(&rom, &path))
        .map(<[u8]>::to_vec);
    assert!(file.is_some(), "D01 has a trigger file");
    let story = dq9_formats::cast::Story {
        chapter: 2,
        step: 4,
        substep: 1,
        ..Default::default()
    };
    let mut state = field::initial_state(&story);
    let mut hexagon = Field::new(7101, &Regions::default(), file).unwrap();
    let events = hexagon.event_ended(2530, &mut state);
    let placements: Vec<(u16, bool)> = events
        .iter()
        .filter_map(|event| match event {
            Event::Request(Request::SetPlacementHidden {
                block: 0,
                placement,
                hidden,
            }) => Some((*placement, *hidden)),
            _ => None,
        })
        .collect();
    assert_eq!(
        placements,
        [(13, false), (15, true)],
        "object 13 is shown and object 15 hidden, in that order"
    );
}

struct World<'r> {
    rom: &'r Rom<'r>,
    collision: CollisionWorld,
    field: Field,
    hero: Hero,
    state: dq9_core::trigger::State,
}

impl<'r> World<'r> {
    fn load(rom: &'r Rom<'r>, id: u16, spawn: Option<(Vec3, f32)>) -> Self {
        let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
        let list = MapList::parse(&table).unwrap();
        let record = list.find(id).unwrap();
        let ctx = Context {
            time_of_day: 0,
            grotto_environ: 1,
        };
        let map = Map::assemble(record, &table, &list, |p| map::rom_file(rom, p), &ctx).unwrap();
        let collision = CollisionWorld::build(&map, |p| map::rom_file(rom, p)).unwrap();
        let code = String::from_utf8_lossy(record.code).into_owned();
        let file = field::trigger_path(&code)
            .and_then(|p| map::rom_file(rom, &p))
            .map(<[u8]>::to_vec);
        let field = Field::new(id, &map.regions, file).unwrap();
        let (position, yaw) =
            spawn.unwrap_or_else(|| (collision.spawn().unwrap_or(Vec3::ZERO), 0.0));
        let hero = Hero::at(position, yaw);
        let story = dq9_formats::cast::Story {
            chapter: 1,
            step: 1,
            substep: 1,
            ..Default::default()
        };
        Self {
            rom,
            collision,
            field,
            hero,
            state: field::initial_state(&story),
        }
    }

    /// Steps in `direction` until the field changes the map (at most `limit` frames).
    fn walk(&mut self, direction: Vec3, limit: u32) -> Option<field::MapChange> {
        for _ in 0..limit {
            self.hero
                .step(&self.collision, Input::from_direction(direction), 0, 1);
            let leader = Leader {
                position: self.hero.position(),
                yaw: self.hero.yaw(),
                input: direction,
            };
            for event in self.field.step(&leader, &mut self.state) {
                if let Event::MapChange(change) = event {
                    return Some(change);
                }
            }
        }
        None
    }

    /// The world after `change`, with the trigger state carried over.
    fn arrive(self, change: &field::MapChange) -> Self {
        let spawn = (
            change.position.unwrap(),
            field::yaw_of_angle(change.angle.unwrap_or(0.0)),
        );
        let mut next = Self::load(self.rom, change.destination, Some(spawn));
        next.state = self.state;
        next
    }
}

/// Angel Falls: walking north into the door of the house `M01M02` enters it, walking south
/// out of it comes back to the village in front of the door (`docs/re/map-archive.md`).
#[test]
fn angel_falls_houses_open_and_close() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut village = World::load(&rom, 1100, None);
    let ground = village.collision.ground_height(-19.5, -9.0, 3.0).unwrap();
    village.hero = Hero::at(Vec3::new(-19.5, ground, -9.0), 0.0);

    let entered = village.walk(Vec3::NEG_Z, 300).expect("the door opens");
    assert_eq!(entered.destination, 1102, "{entered:?}");
    let p = entered.position.unwrap();
    assert!((p - Vec3::new(0.4, 0.16, 6.1)).length() < 0.01, "{p}");
    // The hero was in the doorway (-19.5, -13) when it opened, not far from it.
    assert!(
        (village.hero.position().z + 13.0).abs() < 1.5,
        "{}",
        village.hero.position()
    );

    let mut house = village.arrive(&entered);
    let left = house.walk(Vec3::Z, 300).expect("the way out");
    assert_eq!(left.destination, 1100, "{left:?}");
    let p = left.position.unwrap();
    assert!((p - Vec3::new(-19.57, -0.1, -11.8)).length() < 0.01, "{p}");
    // Back in the village the hero stands outside the doorway: nothing reopens.
    let mut village = house.arrive(&left);
    assert_eq!(village.walk(Vec3::ZERO, 120), None);
}

/// Stornway at night, progress 19.2 (a melonDS state of the game at that point): the load
/// event marks the doorways 22 and 24 used (`mark_object`), exactly the two whose flags the
/// game's RAM shows with bit 2 set (`dq9re doors --state`: 12 and 14 against 8 and 10).
#[test]
fn stornway_at_night_marks_its_two_locked_doorways() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let story = dq9_formats::cast::Story {
        chapter: 19,
        step: 2,
        substep: 1,
        daytime: false,
        ..Default::default()
    };
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    let map = Map::assemble(
        list.find(100).unwrap(),
        &table,
        &list,
        |p| map::rom_file(&rom, p),
        &ctx,
    )
    .unwrap();
    let file = map::rom_file(&rom, "data/scenario/triggerC01.bin").map(<[u8]>::to_vec);
    let mut field = Field::new(100, &map.regions, file).unwrap();
    let mut state = field::initial_state(&story);
    field.enter(&mut state, true);
    let used: Vec<u8> = field
        .doorway_flags()
        .into_iter()
        .filter(|&(_, _, flags)| flags & 4 != 0)
        .map(|(_, number, _)| number)
        .collect();
    assert_eq!(used, [24, 22]);
}

/// X01M07's inner red door (type 2, class 0 number 2, `0x10` side flag, no destination):
/// native 584 (`0x02018b44`) turns its placement 90° about its origin, and the collision
/// child must turn with it — the way to the library must open for the hero.
#[test]
fn x01m07_inner_door_opens_the_way() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut world = World::load(&rom, 4107, None);
    let ground = world
        .collision
        .ground_height(-8.6, -6.0, 3.0)
        .expect("floor in front of the door");
    world.hero = Hero::at(Vec3::new(-8.6, ground, -6.0), 0.0);
    world.walk(Vec3::NEG_Z, 240);
    let blocked = world.hero.position();
    assert!(
        (blocked.z + 6.45).abs() < 0.4,
        "the closed door stops the hero: {blocked:?}"
    );
    // The double door is two placements of two zones (numbers 2 and 3): walking into the
    // doorway touches both, so both leaves turn.
    assert!(world.collision.set_door_angle(0, 2, 0x1922));
    assert!(world.collision.set_door_angle(0, 3, 0x1922));
    world.hero = Hero::at(Vec3::new(-8.6, ground, -6.0), 0.0);
    world.walk(Vec3::NEG_Z, 240);
    let through = world.hero.position();
    assert!(
        through.z < -8.0,
        "the open door lets the hero through: {through:?}"
    );
}
