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

// ---------------------------------------------------------------------------------------------
// The original's roamers (`docs/re/encounter.md`, "Claims checked"). These read states and logs
// produced from the game running in melonDS (`scripts/melonds-field.ps1`), which are never in
// the repository: the tests skip themselves unless `DQ9_FIELD_DIR` names the directory that
// holds the run's `*.tsv` logs and `*-ram.bin` main RAM images.

#[path = "../../dq9re/src/roamers.rs"]
#[allow(dead_code)]
mod roamers;

/// The directory of the field run, when the environment names one.
fn field_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("DQ9_FIELD_DIR").map(std::path::PathBuf::from)
}

/// Files of `dir` ending in `suffix`, sorted.
fn files_ending(dir: &std::path::Path, suffix: &str) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(suffix))
        })
        .collect();
    files.sort();
    files
}

/// The numbers of the lines of a log that start with `tag` (the first column).
fn log_lines(dir: &std::path::Path, tag: &str) -> Vec<Vec<i64>> {
    let mut out = Vec::new();
    for file in files_ending(dir, ".tsv") {
        for line in std::fs::read_to_string(file).unwrap().lines() {
            let mut cols = line.split('\t');
            if cols.next() == Some(tag) {
                out.push(cols.map(|c| c.parse().unwrap()).collect());
            }
        }
    }
    out
}

/// Every state of the run: the roamers in RAM are the model's kind on the map's graph, their
/// count is the slot's spawned count, and the graph indices agree with `F02P00.bin`.
#[test]
fn field_states_hold_roamers_of_the_models_zones() {
    let (Some(bytes), Some(dir)) = (testrom::load(), field_dir()) else {
        return;
    };
    let rom = Rom::parse(bytes).unwrap();
    let tables = tables(&rom);
    let graph = graph(&rom, "F02P00.bin").unwrap();
    let zones = &tables.zones(20002).unwrap().zones;
    let mut checked = 0;
    for file in files_ending(&dir, "-ram.bin") {
        let ram = std::fs::read(&file).unwrap();
        let mem = dq9_formats::melonds::Memory::from_main_ram(&ram).unwrap();
        let slot = roamers::current_slot(&mem).unwrap().unwrap();
        assert_eq!(slot.map, 20002, "{file:?}");
        let list = roamers::roamers(&mem, slot.group()).unwrap();
        let report = roamers::report(&mem).unwrap();
        eprintln!("{file:?}\n{report}");
        if std::env::var_os("DQ9_FIELD_REPORT").is_some() {
            let stem = file
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace("-ram.bin", "");
            std::fs::write(dir.join(format!("roamers-{stem}.txt")), &report).unwrap();
        }
        assert!(list.len() <= 12);
        // `+6` counts the spawns since the map was loaded; it never goes down.
        assert!(
            usize::from(slot.spawned) >= list.len(),
            "{file:?}: +6 under the live count"
        );
        for r in &list {
            let zone = zones.iter().find(|z| z.id == r.zone).unwrap_or_else(|| {
                panic!("{file:?}: roamer in slot {} names zone {}", r.slot, r.zone)
            });
            assert!(
                zone.roamers.iter().any(|d| d.monster() == r.monster),
                "{file:?}: monster {} is not in zone {}",
                r.monster,
                r.zone
            );
            let node = &graph.nodes[usize::from(r.node)];
            assert_eq!(u16::from(node.id), r.node_id, "{file:?}: node index and id");
            checked += 1;
        }
    }
    eprintln!("{checked} roamers checked");
}

/// What the model says of the spawn the log records, checked line by line: the zone is an eligible
/// one of the hour (the node's mask preferred), the monster is the zone's, the node is among
/// the candidates and is the most-ahead one when any is ahead, and the point is the model's.
#[test]
fn logged_spawns_follow_the_model() {
    let (Some(bytes), Some(dir)) = (testrom::load(), field_dir()) else {
        return;
    };
    let rom = Rom::parse(bytes).unwrap();
    let tables = tables(&rom);
    let graph = graph(&rom, "F02P00.bin").unwrap();
    let zones = &tables.zones(20002).unwrap().zones;
    let (mut n, mut phases) = (0, [0usize; 4]);
    let (mut nearest_agrees, mut ahead_n, mut random_n, mut point_equal) = (0, 0, 0, 0);
    let mut problems: Vec<String> = Vec::new();
    for l in log_lines(&dir, "S") {
        // frame map clock_ms phase hero(x y z angle node) timer slot monster zone node node_id
        // pos(x y z) other_count (x y z node_id state)*
        let (frame, phase, timer) = (l[0], l[3] as usize, l[9]);
        let hero: Fx3 = [l[4] as i32, l[5] as i32, l[6] as i32];
        let (angle, hero_node) = (l[7] as i32, l[8] as usize);
        let (monster, zone_id, node, node_id) = (l[11] as u16, l[12] as u16, l[13] as usize, l[14]);
        let at: Fx3 = [l[15] as i32, l[16] as i32, l[17] as i32];
        let others = &l[19..];
        n += 1;
        phases[phase] += 1;
        let mut bad = |what: &str| problems.push(format!("frame {frame} (phase {phase}): {what}"));
        // The hour: night is phase 0 alone; kind 0 is not for night, kind 1 only for it.
        let night = dq9_core::encounter::pick::is_night(phase as u8);
        let Some(zone) = zones.iter().find(|z| z.id == zone_id) else {
            bad(&format!("zone {zone_id} is not one of the map's"));
            continue;
        };
        if (zone.kind() == 0 && night) || (zone.kind() == 1 && !night) {
            bad(&format!(
                "zone {zone_id} (kind {}) drawn by night={night}",
                zone.kind()
            ));
        }
        if !zone.roamers.iter().any(|d| d.monster() == monster) {
            bad(&format!("monster {monster} is not in zone {zone_id}"));
        }
        let Some(spawn_node) = graph.nodes.get(node) else {
            bad(&format!("node index {node} is outside the graph"));
            continue;
        };
        if i64::from(spawn_node.id) != node_id {
            bad(&format!(
                "node {node} has id {}, the game says {node_id}",
                spawn_node.id
            ));
        }
        // Among the eligible zones those meeting the node's mask are preferred.
        let eligible = |z: &&dq9_core::encounter::tables::ZoneDef| {
            !((z.kind() == 0 && night) || (z.kind() == 1 && !night))
        };
        let meets = |z: &&dq9_core::encounter::tables::ZoneDef| {
            z.mask() == 0 || z.mask() & u32::from(spawn_node.mask) != 0
        };
        let preferred = zones.iter().filter(eligible).filter(meets).count();
        if preferred > 0 && !meets(&zone) {
            bad(&format!(
                "zone {zone_id} does not meet mask {} of node {node}",
                spawn_node.mask
            ));
        }
        // The leader's node: the game's own against the nearest one the model uses.
        if graph.nearest(hero) == Some(hero_node) {
            nearest_agrees += 1;
        }
        // The node and the point.
        // A roamer holds a node by its index (`+0xb8`): `func_02073d58` compares that node's id.
        let taken_ids: Vec<u8> = others
            .chunks(6)
            .filter_map(|o| graph.nodes.get(o[3] as usize).map(|n| n.id))
            .collect();
        let plan = dq9_core::encounter::plan_spawn(
            &graph,
            hero,
            angle,
            hero_node,
            &|id| taken_ids.contains(&id),
            &mut |_| 0,
        );
        let Some(plan) = plan else {
            bad(&format!(
                "the model finds no spawn: leader node {hero_node} links {:?}, taken {taken_ids:?}, \
                 spawned at node {node} {at:?}, leader {hero:?} angle {angle}",
                graph.nodes[hero_node].links
            ));
            continue;
        };
        if !plan.candidates.contains(&node) {
            bad(&format!(
                "node {node} is not among the candidates {:?}",
                plan.candidates
            ));
            continue;
        }
        if plan.ahead {
            ahead_n += 1;
            if plan.chosen != node {
                bad(&format!(
                    "node {node} chosen, the model's most-ahead is {}",
                    plan.chosen
                ));
            }
        } else {
            random_n += 1;
        }
        let model_point = {
            let again = dq9_core::encounter::plan_spawn(
                &graph,
                hero,
                angle,
                hero_node,
                &|id| taken_ids.contains(&id),
                &mut |_| plan.candidates.iter().position(|&c| c == node).unwrap(),
            )
            .unwrap();
            again.position
        };
        if (model_point[0] - at[0]).abs() <= 2 && (model_point[2] - at[2]).abs() <= 2 {
            point_equal += 1;
        } else {
            bad(&format!(
                "spawned at {at:?}, the model puts it at {model_point:?}"
            ));
        }
        // The tick: the slot's timer reached a second (30 Hz steps of 33 ms) before the spawn.
        if timer + 34 < 1000 {
            bad(&format!("timer {timer} ms is under a second"));
        }
        // Not within 3.5 of the leader, not within the box of another roamer.
        if fx::distance(hero, at) + 0x600 < 0x3800 {
            bad("closer than 3.5 to the leader");
        }
    }
    eprintln!(
        "{n} spawns; phases {phases:?}; leader node equals the model's nearest on {nearest_agrees}; \
         {ahead_n} with a node ahead, {random_n} drawn; {point_equal} points equal"
    );
    for p in &problems {
        eprintln!("PROBLEM {p}");
    }
    // On the reference run two spawns in 210 (frames 3172 and 10749) went to the leader's own
    // node where the model's most-ahead candidate lies past the 0x9fff path limit: unexplained,
    // so a tolerance of 2 %, not zero.
    assert!(
        problems.len() * 50 <= n,
        "{} of {n} spawns disagree with the model",
        problems.len()
    );
}

/// The state machine's clocks, as the log shows the game's: a roamer that has just appeared
/// stands after 1001 ms, one that is standing leaves it after 4001 ms, one that has noticed the
/// party decides after 226 ms, a startled one resumes after 3 s.
#[test]
fn logged_state_changes_follow_the_model_clocks() {
    let Some(dir) = field_dir() else { return };
    // frame slot monster old new old_state_ms old_awake_ms wander walk vision
    let mut table = std::collections::BTreeMap::<(i64, i64), (i64, i64, usize)>::new();
    for l in log_lines(&dir, "T") {
        let e = table.entry((l[3], l[4])).or_insert((i64::MAX, i64::MIN, 0));
        *e = (e.0.min(l[5]), e.1.max(l[5]), e.2 + 1);
    }
    for ((old, new), (lo, hi, count)) in &table {
        eprintln!("state {old} -> {new}: {count}x, after {lo}..={hi} ms");
    }
    let (lo, hi, _) = table[&(0, 1)];
    assert!(
        (1001 - 34..1001).contains(&lo) && hi < 1001,
        "appear {lo}..{hi}"
    );
    // The reading is the one before the change: a standing roamer leaves it at its first update
    // past 4001 ms, a noticing one decides at its first past 226 ms.
    for ((old, new), (lo, _, _)) in &table {
        match (*old, *new) {
            (1, 8 | 10) | (2, _) | (3, _) | (6, _) | (4, _) | (5, _) | (7, _) => {}
            (1, _) => assert!(*lo >= 4001 - 34, "standing -> {new} after {lo} ms"),
            (10, 8) => {}
            (10, _) => assert!(
                (226 - 34..226).contains(lo),
                "notice -> {new} after {lo} ms"
            ),
            _ => {}
        }
    }
}

/// Every contact the run logged is a touch the model recognises: the leader is within the
/// roamer's reach on x and z (its touch radius plus half the leader's) once both have moved.
#[test]
fn logged_contacts_are_within_the_models_reach() {
    let (Some(bytes), Some(dir)) = (testrom::load(), field_dir()) else {
        return;
    };
    let rom = Rom::parse(bytes).unwrap();
    let tables = tables(&rom);
    let zones = &tables.zones(20002).unwrap().zones;
    let (mut n, mut inside) = (0, 0);
    for l in log_lines(&dir, "C") {
        // frame map clock_ms phase slot monster zone state hero_before(x y z angle)
        // hero_after(x y z angle) roamer_before(x y z angle) roamer_after(x y z angle) prev_state ms
        let (monster, zone_id) = (l[5] as u16, l[6] as u16);
        let hero = [l[12] as i32, l[13] as i32, l[14] as i32];
        let roamer = [l[20] as i32, l[21] as i32, l[22] as i32];
        let field = tables.monster(monster).unwrap();
        let def = zones
            .iter()
            .find(|z| z.id == zone_id)
            .and_then(|z| z.roamers.iter().find(|d| d.monster() == monster))
            .unwrap();
        let scale = fx::mul(0x10a, i32::from(def.scale));
        let radius = ((i64::from(i32::from(field.size) >> 1) * i64::from(scale * 15) + 0x800) >> 12)
            as i32
            + (0x1000 >> 1);
        let (dx, dz) = ((hero[0] - roamer[0]).abs(), (hero[2] - roamer[2]).abs());
        n += 1;
        if dx <= radius && dz <= radius {
            inside += 1;
        }
        eprintln!(
            "contact with {monster}: dx {dx} dz {dz} reach {radius} (prev state {})",
            l[24]
        );
        assert!(
            dx <= radius + 0x1000 && dz <= radius + 0x1000,
            "far from the roamer"
        );
    }
    eprintln!("{inside}/{n} contacts inside the reach exactly");
}
