//! Unit tests on synthetic data: the draws and the geometry.

use super::graph::RoamGraph;
use super::pick::{self, rand_below};
use super::tables::{BattleRoamer, BattleZone, ZoneDef};
use crate::battle::{CRand, Random};

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
