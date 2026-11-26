//! Unit tests on synthetic data: the draws, the geometry and the two spawn/movement rules the
//! field runs corrected (`docs/re/encounter.md`, "Claims checked").

use std::sync::Arc;

use super::graph::{Node, RoamGraph};
use super::pick::{self, rand_below};
use super::roamer::{Roamer, state};
use super::tables::{
    BattleRoamer, BattleZone, EncounterTables, MapZones, MonsterField, RoamerDef, ZoneDef,
};
use super::{Roamers, StepInput, Surroundings};
use crate::battle::{CRand, Random};
use crate::collision::CollisionWorld;

#[test]
fn rand_below_truncates_and_never_reaches_n() {
    // rand() = 0 gives 0 (the −1/32767 truncates to 0); the largest draw stays below n.
    let mut seen = [0u32; 10];
    let mut rng = CRand::new(1);
    for _ in 0..20_000 {
        let v = rand_below(&mut rng, 10);
        assert!((0..10).contains(&v));
        seen[v as usize] += 1;
    }
    assert!(seen.iter().all(|&n| n > 1500), "{seen:?}");
}

/// The surprise roll's odds by seed: face to face the party wins on `2 + deftness / 20` in a
/// hundred and loses on 2; a turned monster gives the party 12 + and no loss; a party caught
/// from behind loses on 12 and cannot win.
#[test]
fn surprise_roll_odds_follow_the_facing() {
    let rates = |facing: u8, deftness: u32| {
        let (mut party, mut monsters) = (0u32, 0u32);
        let mut rng = CRand::new(7);
        let n = 100_000;
        for _ in 0..n {
            match pick::surprise_roll(facing, deftness, &mut rng) {
                1 => party += 1,
                2 => monsters += 1,
                _ => {}
            }
        }
        (
            f64::from(party) / f64::from(n) * 100.0,
            f64::from(monsters) / f64::from(n) * 100.0,
        )
    };
    let (p, m) = rates(0, 100);
    assert!((6.2..7.8).contains(&p), "{p}");
    assert!((1.8..2.2).contains(&m), "{m}");
    let (p, m) = rates(1, 100);
    assert!((16.0..18.0).contains(&p), "{p}");
    assert_eq!(m, 0.0);
    let (p, m) = rates(2, 100);
    assert_eq!(p, 0.0);
    assert!((11.5..12.5).contains(&m), "{m}");
}

#[test]
fn surprise_roll_draws_are_the_partys_first() {
    // Facing 1 draws once, facing 2 draws once, facing 0 draws twice when the first misses.
    let mut a = CRand::new(3);
    pick::surprise_roll(1, 0, &mut a);
    let mut b = CRand::new(3);
    b.rand();
    assert_eq!(a, b);
    let mut a = CRand::new(3);
    pick::surprise_roll(2, 0, &mut a);
    assert_eq!(a, b);
    // Face to face: the party's `rand_below(100)` first, the monsters' only when it missed, so
    // the two draws are exactly what a manual two-draw sequence produces on the same seed.
    for seed in 1..=64u32 {
        let mut got = CRand::new(seed);
        let rolled = pick::surprise_roll(0, 0, &mut got);
        let mut want = CRand::new(seed);
        let first = rand_below(&mut want, 100);
        let expected = if first < 2 {
            1
        } else if rand_below(&mut want, 100) < 2 {
            2
        } else {
            0
        };
        assert_eq!(rolled, expected, "seed {seed}, first draw {first}");
        assert_eq!(got, want, "seed {seed}: the draws diverged");
    }
}

/// The contact's three facing cases (`ov_017::02196430`): the monster's own bearing is tested
/// first, against `0x460cbe66` = 9007.6 `fx32` angle units, so a monster looking away is case
/// 1 whatever the leader does; then the leader's, 2; else 0. A monster at the origin and the
/// party at +z: heading 0 faces the party, `0x3244` (π) faces away.
#[test]
fn the_facing_case_tests_the_monster_then_the_party() {
    use super::contact_facing;
    let monster = [0, 0, 0];
    let hero = [0, 0, 0x1000];
    let at = |roamer_angle: i32, hero_angle: i32| {
        contact_facing(monster, roamer_angle, hero, hero_angle)
    };
    assert_eq!(at(0, 0x3244), 0, "face to face");
    assert_eq!(at(0x3244, 0x3244), 1, "the monster's back turned");
    // The same monster heading with the leader looking away: the monster's test wins.
    assert_eq!(at(0x3244, 0), 1);
    assert_eq!(at(0, 0), 2, "the party caught from behind");
    // The threshold is 0.7 π, not the wiki's 49.48°: 9008 units is past it, 9007 is not.
    assert_eq!(at(9008, 0x3244), 1);
    assert_eq!(at(9007, 0x3244), 0);
    assert_eq!(at(-9008, 0x3244), 1, "the test is the absolute value");
    // The party's own threshold, the same 9007.6: its bearing to the monster is π, so a
    // heading of 3861 is 9007 off it and 3860 is 9008 off.
    assert_eq!(at(0, 3860), 2);
    assert_eq!(at(0, 3861), 0);
}

fn zone(id: u16, word: u32, weights: &[(u16, u32)]) -> ZoneDef {
    ZoneDef {
        id,
        word,
        roamers: weights
            .iter()
            .map(|&(m, w)| super::tables::RoamerDef {
                word: u32::from(m) | (w << 12),
                scale: 0x1000,
            })
            .collect(),
    }
}

#[test]
fn spawn_zone_fits_the_hour_and_prefers_the_mask() {
    // Kind 0 by day, kind 1 by night, kind 2 always; masks at bits 13–20.
    let zones = [
        zone(1, 4 << 13, &[]),
        zone(2, 1 | (1 << 13), &[]),
        zone(3, 2 | (2 << 13), &[]),
    ];
    let mut rng = CRand::new(5);
    for _ in 0..200 {
        assert_eq!(pick::spawn_zone(&zones, false, 2, &mut rng).unwrap().id, 3);
        // No mask matches 8: any eligible zone, never the night one by day.
        let id = pick::spawn_zone(&zones, false, 8, &mut rng).unwrap().id;
        assert!(id == 1 || id == 3, "{id}");
        // By night zone 1 is out, zone 2 alone meets mask 1.
        assert_eq!(pick::spawn_zone(&zones, true, 1, &mut rng).unwrap().id, 2);
    }
    // A zone with mask 0 meets any mask.
    let open = [zone(9, 0, &[])];
    assert_eq!(
        pick::spawn_zone(&open, false, 0x80, &mut rng).unwrap().id,
        9
    );
    // Nothing eligible: none.
    let night_only = [zone(2, 1, &[])];
    assert!(pick::spawn_zone(&night_only, false, 1, &mut rng).is_none());
}

#[test]
fn roamers_are_weighted() {
    let z = zone(1, 0, &[(10, 1), (11, 3)]);
    let mut rng = CRand::new(9);
    let mut counts = [0u32; 2];
    for _ in 0..40_000 {
        let r = pick::roamer_of(&z, &mut rng).unwrap();
        counts[usize::from(r.monster() - 10)] += 1;
    }
    let ratio = f64::from(counts[1]) / f64::from(counts[0]);
    assert!((2.8..3.2).contains(&ratio), "{counts:?}");
}

fn battle_zone() -> BattleZone {
    // A roamer 5 (1 to 2 of it; one kind weight 1, two kinds 1, three kinds 1) and a company of
    // 6 (weight 2, 1–2) and 7 (weight 1, 1–1).
    let roamer = 5 | (1 << 12) | (2 << 15) | (1 << 18) | (1 << 21) | (1 << 24);
    BattleZone {
        id: 1,
        roamers: vec![BattleRoamer {
            word: roamer,
            chance: 0,
            preset: 0,
        }],
        company: vec![
            6 | (2 << 12) | (1 << 15) | (2 << 18),
            7 | (1 << 12) | (1 << 15) | (1 << 18),
        ],
    }
}

/// The company draw stays inside the bounds its words set, never exceeds the limit and always
/// holds the touched monster.
#[test]
fn company_stays_within_its_bounds() {
    let zone = battle_zone();
    let mut world = Random::new(0x1234_5678_9abc_def0);
    let mut kinds = [0u32; 4];
    for _ in 0..5_000 {
        let c = pick::draw_company(5, Some(&zone), Some(&zone), 5, &mut world);
        let present: Vec<_> = c.iter().filter(|s| s.1 > 0).collect();
        assert!(c.iter().any(|&(m, n)| m == 5 && n >= 1));
        let total: i16 = c.iter().map(|s| s.1).sum();
        assert!((1..=5).contains(&total), "{c:?}");
        for &&(m, n) in &present {
            match m {
                5 => assert!((1..=2).contains(&n), "{c:?}"),
                6 => assert!((1..=2).contains(&n), "{c:?}"),
                7 => assert_eq!(n, 1),
                _ => panic!("{c:?}"),
            }
        }
        kinds[present.len()] += 1;
    }
    assert!(kinds[1] > 0 && kinds[2] > 0 && kinds[3] > 0, "{kinds:?}");
}

#[test]
fn a_company_over_the_limit_is_shrunk() {
    let zone = battle_zone();
    let mut world = Random::new(1);
    for _ in 0..2_000 {
        let c = pick::draw_company(5, Some(&zone), Some(&zone), 2, &mut world);
        let total: i16 = c.iter().map(|s| s.1).sum();
        assert!(total <= 3, "{c:?}");
    }
}

#[test]
fn monsters_38_to_40_come_alone() {
    let zone = battle_zone();
    let mut world = Random::new(2);
    let c = pick::draw_company(0x27, Some(&zone), Some(&zone), 5, &mut world);
    assert_eq!(c, [(0x27, 1), (0, 0), (0, 0)]);
}

#[test]
fn nearest_of_an_empty_graph_is_none() {
    assert_eq!(RoamGraph::default().nearest([0; 3]), None);
}

/// A map list that names no stage.
struct NoList;

impl Surroundings for NoList {
    fn stage_class(&self, _stage: u16) -> Option<u8> {
        None
    }
}

fn empty_world() -> CollisionWorld {
    CollisionWorld::from_pieces(Vec::new(), Vec::new())
}

/// A map with one zone: kind 2 (any hour), mask 0, wait 0 (`(7 − 7) × 1000`), monster 3 alone.
fn one_zone() -> Arc<EncounterTables> {
    let zone = ZoneDef {
        id: 5,
        word: 2 | (7 << 21),
        roamers: vec![RoamerDef {
            word: 3 | (1 << 12),
            scale: 0x1000,
        }],
    };
    Arc::new(EncounterTables {
        fields: vec![MapZones {
            map: 1,
            zones: vec![zone],
        }],
        battle: Vec::new(),
        monsters: vec![MonsterField {
            number: 3,
            level: 1,
            margin: 0,
            wander: 1,
            walk: 1,
            vision: 0,
            flee: 1,
            attack: 1,
            size: 0x1000,
            attack_power: 1,
            defence_power: 1,
            radius: 0x100,
            height: 0x100,
        }],
    })
}

/// Two nodes ten units apart, each linked to the other.
fn two_nodes() -> RoamGraph {
    RoamGraph {
        nodes: vec![
            Node {
                id: 0,
                mask: 1,
                flags: 0,
                position: [0, 0, 0],
                links: vec![1],
            },
            Node {
                id: 1,
                mask: 1,
                flags: 0,
                position: [0, 0, 10],
                links: vec![0],
            },
        ],
    }
}

/// A fresh arrival spawns nothing while the party stands — the slot's timer keeps running — and
/// its first step lets the spawn tick through (`docs/re/encounter.md`, "Claims checked", (b)).
#[test]
fn a_fresh_arrival_spawns_only_after_the_first_step() {
    let world = empty_world();
    let mut roamers = Roamers::new(
        1,
        one_zone(),
        Some(two_nodes()),
        CRand::new(1),
        Random::new(1),
    );
    let mut input = StepInput::at([0; 3], 0);
    for _ in 0..200 {
        roamers.step(&input, &world, &NoList);
    }
    assert_eq!(roamers.roamers().count(), 0);
    assert!(roamers.timer >= 6_000, "the timer waits: {}", roamers.timer);
    input.moving = true;
    roamers.step(&input, &world, &NoList);
    let (_, r) = roamers.roamers().next().expect("a spawn after the step");
    assert_eq!(r.number, 3);
    assert_eq!(r.position, [0, 0, 10 << 12]);
}

/// The first roamer a spawn tick of a map entered a second ago gives, the leader standing at
/// the origin and facing the far node (10.0 away), with an optional second member.
fn spawn_with_follower(follower: Option<[i32; 3]>) -> Option<(u16, [i32; 3])> {
    let world = empty_world();
    let mut roamers = Roamers::new(
        1,
        one_zone(),
        Some(two_nodes()),
        CRand::new(1),
        Random::new(1),
    );
    let mut input = StepInput::at([0; 3], 0);
    for _ in 0..40 {
        roamers.step(&input, &world, &NoList);
    }
    input.moving = true;
    input.party = [follower, None, None];
    for _ in 0..5 {
        roamers.step(&input, &world, &NoList);
    }
    roamers
        .roamers()
        .next()
        .map(|(_, r)| (r.number, r.position))
}

/// `func_02073dfc` (`0x02073e0c`) tests the spawn point against every party slot: a second
/// member within 3.5 blocks a spawn the leader alone allows (10.0 from the point).
#[test]
fn any_party_member_blocks_the_spawn_point() {
    let point = [0, 0, 10 << 12];
    // The leader alone: the far node spawns, as before.
    assert_eq!(spawn_with_follower(None), Some((3, point)));
    // The second member 1.0 from the point: refused, though the leader is 10.0 away.
    assert_eq!(spawn_with_follower(Some([0, 0, 9 << 12])), None);
    // The original's strict `< 0x3800`: 3.5 exactly is outside the block.
    assert_eq!(
        spawn_with_follower(Some([0, 0, point[2] - 0x3800])),
        Some((3, point))
    );
    assert_eq!(spawn_with_follower(Some([0, 0, point[2] - 0x37ff])), None);
    // A member elsewhere on the map does not block it.
    assert_eq!(
        spawn_with_follower(Some([10 << 12, 0, 0])),
        Some((3, point))
    );
}

/// A fleeing roamer leaves its state when its ten-unit run is done (within 1.5 of the point),
/// not after a fixed time: at twice the speed it takes half the updates. The field runs showed
/// 6→1 from 0 ms up to 1452 ms and beyond (`docs/re/encounter.md`, (c)).
#[test]
fn a_flee_ends_on_its_run_not_on_a_fixed_time() {
    let world = empty_world();
    let field = one_zone().monster(3).copied().unwrap();
    let a_flee = |ticks: i32| {
        let mut roamers = Roamers::new(
            1,
            one_zone(),
            Some(two_nodes()),
            CRand::new(1),
            Random::new(1),
        );
        let mut r = Roamer::spawn(3, 1, 5, field, 0x1000, [0x1000, 0, 0], 0, 0, 0);
        assert!(r.enter(state::FLEE, [0; 3]));
        roamers.slots[0] = Some(r);
        let mut input = StepInput::at([0; 3], 0);
        input.ticks = ticks;
        let mut updates = 0;
        while roamers.roamer(0).unwrap().state == state::FLEE {
            roamers.step(&input, &world, &NoList);
            updates += 1;
            assert!(updates < 200);
            let r = roamers.roamer(0).unwrap();
            assert!(
                r.state != state::FLEE || r.flat_distance(r.target) >= 0x1800,
                "left the flee before the run was done"
            );
        }
        let r = roamers.roamer(0).unwrap();
        assert_eq!(r.state, state::IDLE);
        assert!(r.flat_distance(r.target) < 0x1800);
        updates
    };
    // (10.0 − 1.5) units at 0x1c2 per tick, two then four ticks an update.
    assert_eq!(a_flee(2), 39);
    assert_eq!(a_flee(4), 20);
}

/// The recent list is `+0x44b8`'s ring: four ids written at a cursor (`func_ov017_021972f8`),
/// no reordering, and the fifth insertion overwrites the first. An id already there is not
/// written again, so the cursor does not move (both call sites test `0219735c` first), and the
/// battle's end zeroes the entry that names its id (`02197324`).
#[test]
fn the_recent_list_is_a_four_entry_ring() {
    let mut roamers = Roamers::new(
        1,
        one_zone(),
        Some(two_nodes()),
        CRand::new(1),
        Random::new(1),
    );
    assert_eq!(roamers.recent, [0; 4]);
    for id in 1..=4u16 {
        roamers.remember_recent(id);
    }
    assert_eq!(roamers.recent, [1, 2, 3, 4]);
    assert_eq!(roamers.recent_cursor, 0);
    // An id already present: no write, no cursor move.
    roamers.remember_recent(2);
    assert_eq!(roamers.recent, [1, 2, 3, 4]);
    assert_eq!(roamers.recent_cursor, 0);
    // The fifth overwrites the first (the oldest), not a shift.
    roamers.remember_recent(5);
    assert_eq!(roamers.recent, [5, 2, 3, 4]);
    assert_eq!(roamers.recent_cursor, 1);
    assert!(!roamers.is_recent(1) && roamers.is_recent(5));
    // Forgetting an id zeroes its entry and leaves the cursor alone; the next insertion takes
    // the cursor's slot (2 here), so two zeroed entries can coexist.
    roamers.forget_recent(3);
    assert_eq!(roamers.recent, [5, 2, 0, 4]);
    roamers.remember_recent(6);
    assert_eq!(roamers.recent, [5, 6, 0, 4]);
    assert_eq!(roamers.recent_cursor, 2);
    roamers.forget_recent(9);
    assert_eq!(roamers.recent, [5, 6, 0, 4], "an absent id changes nothing");
}

/// A roamer whose id is recent is not touched, and the list is what says so: the same two
/// objects give a contact once the entry is gone. The battle's end is the game's own remover.
#[test]
fn a_recent_id_is_not_touched_until_the_list_forgets_it() {
    let world = empty_world();
    let field = one_zone().monster(3).copied().unwrap();
    let mut roamers = Roamers::new(
        1,
        one_zone(),
        Some(two_nodes()),
        CRand::new(1),
        Random::new(1),
    );
    let mut r = Roamer::spawn(3, 7, 5, field, 0x1000, [0x800, 0, 0], 0, 0, 0);
    assert!(r.enter(state::IDLE, [0; 3]));
    roamers.slots[0] = Some(r);
    let input = StepInput::at([0; 3], 0);
    roamers.remember_recent(7);
    assert!(
        roamers.step(&input, &world, &NoList).is_none(),
        "a recent roamer is skipped"
    );
    roamers.forget_recent(7);
    assert!(
        roamers.step(&input, &world, &NoList).is_some(),
        "the id is out of the list: the roamer is touched"
    );
}

/// A won fight ends with the roamer **gone**: `ov_000::021688dc` resets its object
/// (`0x02076a8c`) and hides it at the battle's exit, so no state 11 or 12 runs — the field log
/// of `build/oracle/field-F02` has no state change after any of its 19 contacts — and
/// `ov_017::021c8c44` drops its id from `+0x44b8`.
#[test]
fn a_won_fight_removes_the_roamer_and_its_recent_id() {
    let world = empty_world();
    let field = one_zone().monster(3).copied().unwrap();
    let mut roamers = Roamers::new(
        1,
        one_zone(),
        Some(two_nodes()),
        CRand::new(1),
        Random::new(1),
    );
    let mut r = Roamer::spawn(3, 9, 5, field, 0x1000, [0x800, 0, 0], 0, 0, 0);
    assert!(r.enter(state::IDLE, [0; 3]));
    roamers.slots[0] = Some(r);
    let input = StepInput::at([0; 3], 0);
    let contact = roamers.step(&input, &world, &NoList).expect("a touch");
    assert!(roamers.is_recent(9));
    roamers.settle(contact.roamer, true, input.position);
    assert!(
        roamers.roamer(0).is_none(),
        "the object leaves the field at the battle's exit, not after a fade"
    );
    assert!(!roamers.is_recent(9), "message 83 forgets the battle's id");
    assert!(roamers.roamers().next().is_none());
}

/// A flight leaves the monster's object untouched (`ov_000::021688dc` only resets the object it
/// is done with): it stands where it was, in state 9 (`FOUGHT`), which the contact check skips,
/// and its id leaves the recent list so a rebuilt field may touch it again. `STARTLED` is not a
/// battle's end: the notice decision's flee type 0 enters it (`0x020789a8`).
#[test]
fn a_flight_leaves_the_roamer_standing_and_forgets_its_id() {
    let world = empty_world();
    let field = one_zone().monster(3).copied().unwrap();
    let mut roamers = Roamers::new(
        1,
        one_zone(),
        Some(two_nodes()),
        CRand::new(1),
        Random::new(1),
    );
    let mut r = Roamer::spawn(3, 7, 5, field, 0x1000, [0x800, 0, 0], 0, 0, 0);
    assert!(r.enter(state::IDLE, [0; 3]));
    roamers.slots[0] = Some(r);
    let mut input = StepInput::at([0; 3], 0);
    let contact = roamers.step(&input, &world, &NoList).expect("a touch");
    let at = roamers.roamer(0).unwrap().position;
    roamers.settle(contact.roamer, false, input.position);
    let roamer = roamers.roamer(0).expect("a flight keeps the roamer");
    assert_eq!(roamer.state, state::FOUGHT);
    assert_eq!(roamer.id, 7);
    assert_eq!(roamer.position, at, "it stays where it stood");
    assert!(!roamers.is_recent(7), "the id leaves the list all the same");
    // State 9 is skipped by the contact check, so the party cannot touch it again in place.
    for _ in 0..5 {
        input.moving = true;
        assert!(roamers.step(&input, &world, &NoList).is_none());
    }
}
