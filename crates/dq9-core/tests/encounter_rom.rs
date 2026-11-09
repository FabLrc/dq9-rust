//! The roaming monsters on the real game data; skipped when the ROM is absent
//! (`docs/re/encounter.md`).

use std::sync::Arc;

use dq9_core::CollisionWorld;
use dq9_core::battle::{CRand, Random};
use dq9_core::encounter::graph::RoamGraph;
use dq9_core::encounter::tables::EncounterTables;
use dq9_core::encounter::{FALLBACK_STAGE, Opening, Roamers, StepInput, Surroundings, stage_under};
use dq9_core::fx::{self, Fx3};
use dq9_formats::battle::monster::Names;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::{Gpc2, Table};
use nds_rom::{Narc, Rom};

fn tables(rom: &Rom<'_>) -> Arc<EncounterTables> {
    let file = |path: &str| rom.file_by_path(path).unwrap();
    let names = Gpc2::parse(file("data/prm/mon_data.gp2")).unwrap();
    let entry = names.find("mon_data_en.nat").unwrap();
    let names = names.member(entry).unwrap();
    Arc::new(
        EncounterTables::parse(
            file("data/prm/encfld.bin"),
            file("data/prm/encbtl.bin"),
            file("data/prm/fld_mondata.bin"),
            &Names::parse(&names).unwrap(),
        )
        .unwrap(),
    )
}

/// The spawn graph of a map: the `<code>P<nn>.bin` member of `path.gp2`, a NARC holding the
/// table.
fn graph(rom: &Rom<'_>, name: &str) -> Option<RoamGraph> {
    let archive = Gpc2::parse(rom.file_by_path("data/pack_lv5/path.gp2").unwrap()).unwrap();
    let entry = archive.find(name)?;
    let member = archive.member(entry).unwrap();
    let narc = Narc::parse(&member).unwrap();
    Some(RoamGraph::parse(narc.file(0).unwrap()).unwrap())
}

fn world(rom: &Rom<'_>, id: u16) -> CollisionWorld {
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

/// Stages the map list knows: the field and forest stages of Angel Falls Region and the
/// Hexagon's, all of class 0.
struct Listed;

impl Surroundings for Listed {
    fn stage_class(&self, stage: u16) -> Option<u8> {
        matches!(stage, 30116 | 30117 | 30214 | 30215).then_some(0)
    }
}

/// A leader standing at node `index` of `graph`, on the ground.
fn standing_at(graph: &RoamGraph, world: &CollisionWorld, index: usize) -> Fx3 {
    let mut position = graph.nodes[index].fx();
    if let Some(height) = world.ground(position) {
        position[1] = height;
    }
    position
}

/// Runs `steps` updates (30 Hz) with the leader standing still and returns every roamer that
/// ever stood on the map as `(zone, monster, position)`.
fn run(
    roamers: &mut Roamers,
    world: &CollisionWorld,
    input: &StepInput,
    steps: usize,
) -> Vec<(u16, u16, Fx3)> {
    let mut seen: Vec<(u16, u16, u16, Fx3)> = Vec::new();
    for _ in 0..steps {
        roamers.step(input, world, &Listed);
        for (_, r) in roamers.roamers() {
            if !seen.iter().any(|s| s.0 == r.id) {
                seen.push((r.id, r.zone, r.number, r.position));
            }
        }
    }
    seen.into_iter().map(|(_, z, m, p)| (z, m, p)).collect()
}

/// The path files and the zones agree with the wiki and the notes: 79 nodes on `F01`, every node
/// linked, `F01`'s zones 12 (by day), 14 (by night) and 15 (always).
#[test]
fn angel_falls_region_has_its_graph_and_zones() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let graph = graph(&rom, "F01P00.bin").unwrap();
    assert_eq!(graph.nodes.len(), 79);
    assert!(graph.nodes.iter().all(|n| !n.links.is_empty()));
    let tables = tables(&rom);
    let zones = &tables.zones(20001).unwrap().zones;
    assert_eq!(
        zones.iter().map(|z| (z.id, z.kind())).collect::<Vec<_>>(),
        [(12, 0), (14, 1), (15, 2)]
    );
}

/// On `F01` a seeded run spawns roamers of zones 12 (by day) or 14 (by night) and 15, never the
/// other hour's, each a monster of its zone, all on walkable ground.
#[test]
fn f01_spawns_from_its_zones_on_walkable_ground() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = tables(&rom);
    let graph = graph(&rom, "F01P00.bin").unwrap();
    let world = world(&rom, 20001);
    let zones = tables.zones(20001).unwrap().zones.clone();
    for (time_of_day, forbidden) in [(2u8, 14u16), (0, 12)] {
        let mut total = 0;
        for seed in 1..=6u32 {
            let mut roamers = Roamers::new(
                20001,
                tables.clone(),
                Some(graph.clone()),
                CRand::new(seed),
                Random::new(u64::from(seed)),
            );
            let start = (seed as usize * 13) % graph.nodes.len();
            let position = standing_at(&graph, &world, start);
            let mut input = StepInput::at(position, 0);
            input.time_of_day = time_of_day;
            let seen = run(&mut roamers, &world, &input, 1800);
            assert!(roamers.roamers().count() <= 12);
            for (zone, monster, at) in seen {
                total += 1;
                assert!([12, 14, 15].contains(&zone), "zone {zone}");
                assert_ne!(zone, forbidden, "time {time_of_day}");
                let def = zones.iter().find(|z| z.id == zone).unwrap();
                assert!(def.roamers.iter().any(|r| r.monster() == monster));
                assert!(world.ground(at).is_some(), "{at:?} is not on ground");
            }
        }
        eprintln!("F01 time {time_of_day}: {total} roamers over 6 seeds");
        assert!(
            total > 10,
            "only {total} roamers spawned at time {time_of_day}"
        );
    }
}

/// The Hexagon's floors 7102–7104 spawn their zone's monsters; maps with no `encfld` group spawn
/// none.
#[test]
fn the_hexagon_floors_spawn_their_zone_and_other_maps_nothing() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = tables(&rom);
    for (map, name, zone) in [
        (7102u16, "D01P02.bin", 32u16),
        (7103, "D01P03.bin", 33),
        (7104, "D01P04.bin", 34),
    ] {
        let graph = graph(&rom, name).unwrap();
        let world = world(&rom, map);
        let expected: Vec<u16> = tables.zones(map).unwrap().zones[0]
            .roamers
            .iter()
            .map(|r| r.monster())
            .collect();
        let mut total = 0;
        for seed in 1..=4u32 {
            let mut roamers = Roamers::new(
                map,
                tables.clone(),
                Some(graph.clone()),
                CRand::new(seed),
                Random::new(u64::from(seed)),
            );
            let start = (seed as usize * 5) % graph.nodes.len();
            let input = StepInput::at(standing_at(&graph, &world, start), 0);
            for (z, monster, _) in run(&mut roamers, &world, &input, 1800) {
                total += 1;
                assert_eq!(z, zone, "map {map}");
                assert!(expected.contains(&monster), "map {map}: {monster}");
            }
        }
        eprintln!("map {map}: {total} roamers of zone {zone} over 4 seeds");
        assert!(total > 0, "map {map} spawned nothing");
    }
    // `S01M01`, `D01M01` and `D01M05` have no group: whatever graph they are given, nothing
    // spawns.
    let graph = graph(&rom, "F01P00.bin").unwrap();
    let world = world(&rom, 20001);
    for map in [5101u16, 7101, 7105] {
        assert!(tables.zones(map).is_none(), "map {map}");
        let mut roamers = Roamers::new(
            map,
            tables.clone(),
            Some(graph.clone()),
            CRand::new(1),
            Random::new(1),
        );
        let input = StepInput::at(standing_at(&graph, &world, 3), 0);
        for _ in 0..1800 {
            roamers.step(&input, &world, &Listed);
        }
        assert_eq!(roamers.roamers().count(), 0, "map {map}");
    }
}

/// A touch on `F01`'s field ground names stage 30116 and in the forest 30117; a stage the list
/// does not have falls back to 30116.
#[test]
fn the_ground_names_the_stage() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let world = world(&rom, 20001);
    let mut found = std::collections::BTreeMap::<u16, usize>::new();
    for piece in world.pieces() {
        let mesh = &world.meshes()[piece.mesh];
        let shift = mesh.shift;
        for triangle in mesh.triangles.iter().filter(|t| t.normal[1] > 0x800) {
            let Some(record) = mesh.record_of(triangle) else {
                continue;
            };
            let v = triangle.vertices;
            let centre = [0, 1, 2].map(|a| {
                ((i32::from(v[0][a]) + i32::from(v[1][a]) + i32::from(v[2][a])) / 3) << shift
            });
            let at = [
                centre[0] + piece.position[0],
                centre[1] + piece.position[1],
                centre[2] + piece.position[2],
            ];
            if stage_under(&world, at) == record.stage() {
                *found.entry(record.stage()).or_default() += 1;
            }
        }
    }
    eprintln!("F01 floor triangles by stage: {found:?}");
    assert!(found.get(&30116).copied().unwrap_or(0) > 100, "{found:?}");
    assert!(found.get(&30117).copied().unwrap_or(0) > 20, "{found:?}");
    assert!(
        found.keys().all(|s| matches!(s, 30116 | 30117)),
        "{found:?}"
    );
}

/// Walking into a roamer opens a fight: the touched monster is in its company, the stage is the
/// ground's, the opening follows the facing.
#[test]
fn touching_a_roamer_opens_a_fight_on_the_ground_stage() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = tables(&rom);
    let graph = graph(&rom, "F01P00.bin").unwrap();
    let world = world(&rom, 20001);
    let mut fights = 0;
    for seed in 1..=8u32 {
        let mut roamers = Roamers::new(
            20001,
            tables.clone(),
            Some(graph.clone()),
            CRand::new(seed),
            Random::new(u64::from(seed)),
        );
        let mut input = StepInput::at(standing_at(&graph, &world, 20), 0);
        // Wait for one, then step onto it.
        let mut target = None;
        for _ in 0..3000 {
            if roamers.step(&input, &world, &Listed).is_some() {
                break;
            }
            if let Some((slot, r)) = roamers
                .roamers()
                .find(|(_, r)| r.state == dq9_core::encounter::roamer::state::IDLE)
            {
                target = Some((slot, r.position, r.number));
                break;
            }
        }
        let Some((_, position, number)) = target else {
            continue;
        };
        input.position = position;
        input.position[0] += 0x400;
        let contact = roamers.step(&input, &world, &Listed).expect("touching");
        assert_eq!(contact.monster, number);
        assert!(contact.company.iter().any(|&(m, n)| m == number && n >= 1));
        assert!(contact.company.iter().map(|c| u32::from(c.1)).sum::<u32>() <= 5);
        eprintln!("seed {seed}: {contact:?}");
        let ground = stage_under(&world, input.position);
        assert_eq!(
            contact.stage,
            if matches!(ground, 30116 | 30117) {
                ground
            } else {
                FALLBACK_STAGE
            }
        );
        assert!(matches!(
            contact.opening,
            Opening::PartyFirst | Opening::MonstersFirst | Opening::Even
        ));
        fights += 1;
    }
    eprintln!("{fights} fights opened over 8 seeds");
    assert!(fights >= 4, "{fights} fights");
    let _ = fx::PI;
}
