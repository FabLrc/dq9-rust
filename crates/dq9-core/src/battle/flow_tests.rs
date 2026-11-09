//! The battle's flow on synthetic data: initiative, surprise, flight, turn classes, rollback,
//! the end of a battle. The ROM-backed battles are `tests/battle_rom.rs`.

use std::collections::BTreeMap;

use super::*;

fn plain_attack(number: u16) -> Action {
    let mut a = Action::new(number, 1, Amount::Blow { multiplier: 1.0 });
    a.reach = Reach::Attack;
    a.element = 8;
    a.landing_element = 8;
    a.dodgeable = true;
    a.blockable = true;
    a.wakes_target = true;
    a.critical_percent = 100;
    a.chains_combo = true;
    a.multi_target_falloff = true;
    a.weapon_element = true;
    a.trait_accuracy_die = true;
    a
}

fn hero(hp: i32, agility: i32) -> PartyMember {
    let mut f = Fighter::new(Side::Party);
    f.hp = hp;
    f.max_hp = hp;
    f.mp = 10;
    f.attack = 100;
    f.defence = 0;
    f.agility = agility;
    f.evasion_rate = 2.0;
    PartyMember::new(f)
}

fn slime(group: usize, hp: i32, agility: i32) -> Monster {
    let mut f = Fighter::new(Side::Monster);
    f.hp = hp;
    f.max_hp = hp;
    f.attack = 10;
    f.defence = 0;
    f.agility = agility;
    Monster {
        number: 1,
        group,
        fighter: f,
        ways: [1; 6],
        selector: 0,
        ai_mode: 1,
        extra_class: 0,
        once_mask: 0,
        remembers: false,
        experience: 2,
        gold: 4,
        drops: [22000, 22087],
        drop_steps: [0, 7],
    }
}

fn setup(monsters: Vec<Monster>, set_battle: bool) -> BattleSetup {
    let mut skills = BTreeMap::new();
    for number in [1, 2] {
        skills.insert(number, Skill::from_action(plain_attack(number)));
    }
    BattleSetup {
        monsters,
        skills,
        set_battle,
    }
}

fn attack(target: usize) -> Command {
    Command::Act {
        skill: Skill::from_action(plain_attack(1)),
        target: Target::Monster(target),
    }
}

fn battle(monsters: Vec<Monster>, party: Vec<PartyMember>, surprise: Surprise) -> Battle {
    Battle::new(
        setup(monsters, false),
        party,
        surprise,
        0x1234_5678_9abc_def0,
    )
    .unwrap()
}

/// The order of a round is `agility × NextRandomFloatBetween(0.51, 1.0)`, party first in the
/// draws, highest first.
#[test]
fn initiative_scores_agility_times_a_draw_in_party_then_monster_order() {
    let mut b = battle(
        vec![slime(0, 50, 30), slime(0, 50, 5)],
        vec![hero(100, 10)],
        Surprise::None,
    );
    let mut replay = Random::new(0x1234_5678_9abc_def0);
    let mut expected = vec![
        (Who::Party(0), 10.0 * replay.next_float_between(0.51, 1.0)),
        (Who::Monster(0), 30.0 * replay.next_float_between(0.51, 1.0)),
        (Who::Monster(1), 5.0 * replay.next_float_between(0.51, 1.0)),
    ];
    expected.sort_by(|a, b| b.1.total_cmp(&a.1));
    let report = b.round(&[attack(0)], &mut Random::new(1)).unwrap();
    assert_eq!(report.events[0], Event::Initiative(expected));
}

/// A surprise round leaves combatants out without a score: the party when the monsters have the
/// jump (the first monster always acts, each later one rolls `NextRandomMax(100)` and sits out
/// above 66), the monsters when the party has it.
#[test]
fn a_surprise_round_leaves_the_surprised_side_out_with_the_original_draws() {
    let mut b = battle(
        vec![slime(0, 50, 30), slime(0, 50, 20)],
        vec![hero(1000, 10)],
        Surprise::Monsters,
    );
    let mut replay = Random::new(0x1234_5678_9abc_def0);
    let mut expected = vec![(Who::Monster(0), 30.0 * replay.next_float_between(0.51, 1.0))];
    if replay.next_max(100) <= 66 {
        expected.push((Who::Monster(1), 20.0 * replay.next_float_between(0.51, 1.0)));
    }
    expected.sort_by(|a, b| b.1.total_cmp(&a.1));
    let report = b.round(&[attack(0)], &mut Random::new(1)).unwrap();
    assert_eq!(report.events[0], Event::Initiative(expected));

    let mut b = battle(
        vec![slime(0, 50, 30)],
        vec![hero(1000, 10)],
        Surprise::Party,
    );
    let report = b.round(&[attack(0)], &mut Random::new(1)).unwrap();
    let Event::Initiative(order) = &report.events[0] else {
        panic!("{:?}", report.events[0])
    };
    assert_eq!(
        order.iter().map(|o| o.0).collect::<Vec<_>>(),
        vec![Who::Party(0)]
    );
}

/// Flight: refused from a set battle before any draw; free in the surprise round; free when the
/// party is three times as strong; otherwise a percentage (deftness × 0.05 + 10, floored by the
/// attempt: 25, 50, 75, 100) against the **world's** generator.
#[test]
fn flight_follows_the_original_order_of_tests() {
    // Set battle.
    let mut b = Battle::new(
        setup(vec![slime(0, 50, 30)], true),
        vec![hero(100, 10)],
        Surprise::None,
        7,
    )
    .unwrap();
    let before = b.generator_state();
    assert_eq!(
        b.round(&[Command::Flee], &mut Random::new(1)).unwrap_err(),
        BattleError::FleeForbidden
    );
    assert_eq!(b.generator_state(), before);

    // The surprise round: away with no draw from the world.
    let mut b = battle(vec![slime(0, 50, 30)], vec![hero(100, 10)], Surprise::Party);
    let mut world = Random::new(42);
    let report = b.round(&[Command::Flee], &mut world).unwrap();
    assert_eq!(
        report.events[0],
        Event::Flee {
            success: true,
            percent: None
        }
    );
    assert_eq!(b.result(), Some(BattleResult::Fled));
    assert_eq!(world, Random::new(42));

    // Three times as strong: attack 100 + defence 0 against 10 + 0.
    let mut b = battle(vec![slime(0, 50, 30)], vec![hero(100, 10)], Surprise::None);
    let report = b.round(&[Command::Flee], &mut Random::new(42)).unwrap();
    assert_eq!(
        report.events[0],
        Event::Flee {
            success: true,
            percent: None
        }
    );

    // A percentage: a strong monster, deftness 100 gives 15 and the first floor 25.
    let strong = |hp| {
        let mut m = slime(0, hp, 1);
        m.fighter.attack = 100;
        m
    };
    let mut member = hero(100, 10);
    member.fighter.attack = 10;
    member.fighter.deftness = 100;
    let mut b = battle(vec![strong(50)], vec![member], Surprise::None);
    let mut world = Random::new(99);
    let mut replay = world.clone();
    let report = b.round(&[Command::Flee], &mut world).unwrap();
    let got_away = replay.next_max(100) < 25;
    assert_eq!(
        report.events[0],
        Event::Flee {
            success: got_away,
            percent: Some(25)
        }
    );
    assert_eq!(world, replay, "one draw on the world's generator");
    if !got_away {
        // The second attempt is held to 50.
        let report = b.round(&[Command::Flee], &mut world).unwrap();
        let Event::Flee { percent, .. } = report.events[0] else {
            panic!()
        };
        assert_eq!(percent, Some(50));
    }
}

/// A class 1 action goes before a faster monster's class 0 one; a rolled-back round leaves the
/// battle as it was.
#[test]
fn turn_classes_reorder_and_an_unsupported_round_changes_nothing() {
    let mut priority = plain_attack(1);
    priority.number = 1;
    let mut skill = Skill::from_action(priority);
    skill.turn_class = 1;
    let command = Command::Act {
        skill,
        target: Target::Monster(0),
    };
    let mut b = battle(
        vec![slime(0, 500, 900)],
        vec![hero(1000, 1)],
        Surprise::None,
    );
    let report = b.round(&[command], &mut Random::new(1)).unwrap();
    let actors: Vec<Who> = report
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Acted { who, .. } = e {
                Some(*who)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(actors, vec![Who::Party(0), Who::Monster(0)]);

    // A monster whose way needs a handler that is not modelled: nothing changes.
    let mut odd = Skill::from_action(plain_attack(1));
    odd.number = 66;
    odd.handlers = [66, 66];
    let mut monsters = slime(0, 50, 5);
    monsters.ways = [66; 6];
    let mut s = setup(vec![monsters], false);
    s.skills.insert(66, odd);
    let mut b = Battle::new(s, vec![hero(1000, 10)], Surprise::None, 5).unwrap();
    let state = b.generator_state();
    let hp = b.monsters()[0].fighter.hp;
    let err = b.round(&[attack(0)], &mut Random::new(1)).unwrap_err();
    assert_eq!(
        err,
        BattleError::Unsupported(Unsupported::TargetHandler(66))
    );
    assert_eq!(b.generator_state(), state);
    assert_eq!(b.monsters()[0].fighter.hp, hp);
    assert_eq!(b.round_number(), 0);
}

/// A way marked once a battle is not tried again by its group; the others go on.
#[test]
fn a_once_a_battle_way_is_spent_for_the_whole_group() {
    let mut m = slime(0, 5000, 5);
    m.ways = [1, 2, 2, 2, 2, 2];
    m.once_mask = 0b11_1111;
    let mut b = battle(vec![m.clone(), m], vec![hero(100_000, 10)], Surprise::None);
    b.round(&[attack(0)], &mut Random::new(1)).unwrap();
    // Every slot of the group is spent after two monsters took one each.
    assert_ne!(b.once_used[0], 0);
    let spent = b.once_used[0].count_ones();
    assert!((1..=2).contains(&spent), "{spent}");
}

/// Victory: the sums of the beaten, the drops once per kind (rare first, ordinary only if the
/// rare did not land, `one in 1` always landing but still drawing), none on a defeat.
#[test]
fn a_won_battle_pays_the_sums_and_rolls_one_drop_per_kind() {
    let mut b = battle(
        vec![slime(0, 1, 5), slime(0, 1, 5)],
        vec![hero(100, 99)],
        Surprise::None,
    );
    let mut world = Random::new(3);
    while b.result().is_none() {
        let target = b.monsters().iter().position(|m| m.fighter.hp > 0).unwrap();
        b.round(&[attack(target)], &mut world).unwrap();
    }
    assert_eq!(b.result(), Some(BattleResult::Won));
    let mut rand = CRand::new(1);
    let end = b.finish(&mut rand).unwrap();
    assert_eq!((end.experience, end.gold), (4, 8));
    // Two slimes of one group are one kind: the ordinary drop (step 0, always) once; the rare
    // one is "never" and draws nothing.
    assert_eq!(end.drops, vec![22000]);
    let mut replay = CRand::new(1);
    replay.rand();
    assert_eq!(rand, replay, "exactly one draw on rand");
}

/// A party that falls loses, and takes nothing.
#[test]
fn a_wiped_out_party_loses_and_takes_nothing() {
    let mut killer = slime(0, 10_000, 5);
    killer.fighter.attack = 5000;
    let mut b = battle(vec![killer], vec![hero(1, 1)], Surprise::None);
    let mut world = Random::new(3);
    for _ in 0..50 {
        if b.result().is_some() {
            break;
        }
        b.round(&[attack(0)], &mut world).unwrap();
    }
    assert_eq!(b.result(), Some(BattleResult::Lost));
    let end = b.finish(&mut CRand::new(1)).unwrap();
    assert_eq!((end.experience, end.gold, end.drops.len()), (0, 0, 0));
    assert_eq!(
        b.round(&[Command::Skip], &mut world).unwrap_err(),
        BattleError::Over
    );
}

/// The drop roll is `(int)(oneIn × ((rand() − 1) / 32767.0)) == 0`: always for 1, never when the
/// draw is below `32767 / oneIn`.
#[test]
fn the_drop_roll_matches_the_formula_and_the_cap_is_eight() {
    let mut rand = CRand::new(7);
    let mut replay = rand.clone();
    for one_in in [1, 8, 16, 256] {
        let draw = (f64::from(one_in) * (f64::from(replay.rand() as i32 - 1) / 32767.0)) as i32;
        assert_eq!(drop_roll(&mut rand, one_in), draw == 0);
    }
    let mut b = battle(vec![slime(0, 1, 5)], vec![hero(100, 9)], Surprise::None);
    b.result = Some(BattleResult::Won);
    b.kills = (0..12)
        .map(|n| end::Kind {
            number: n,
            group: 0,
            drops: [100 + n, 200 + n],
            steps: [0, 0],
        })
        .collect();
    let end = b.finish(&mut CRand::new(1)).unwrap();
    assert_eq!(
        end.drops,
        (0..8).map(|n| 200 + n).collect::<Vec<u16>>(),
        "rare first, eight kept"
    );
}
