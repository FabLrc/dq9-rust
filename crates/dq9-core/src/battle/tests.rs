//! Unit tests of the battle kernel: generators, the float edges the wiki quotes, the order of
//! draws for several targets (which BattleEmulator, one target at a time, cannot check), the
//! refusals and the rules that no oracle vector exercises.

use super::damage::{calculate_crit_rate, calculate_physical_damage, critical_threshold};
use super::*;

fn party() -> Fighter {
    let mut f = Fighter::new(Side::Party);
    f.attack = 100;
    f.defence = 50;
    f.hp = 100;
    f.max_hp = 100;
    f.evasion_rate = 2.0;
    f
}

fn monster() -> Fighter {
    let mut f = Fighter::new(Side::Monster);
    f.attack = 100;
    f.defence = 40;
    f.hp = 100;
    f.max_hp = 100;
    f
}

fn blow() -> Action {
    let mut a = Action::new(1, 1, Amount::Blow { multiplier: 1.0 });
    a.element = 8;
    a.dodgeable = true;
    a.blockable = true;
    a.critical_percent = 100;
    a
}

/// A range action whose amount is exactly `amount`: no spread, so one draw that changes nothing.
fn flat(amount: u16) -> Action {
    let range = ActionRange {
        spread: 0,
        monster_amount: amount,
        party_min: amount,
        party_max: amount,
        lo: 0,
        hi: 1,
        scaling: None,
    };
    Action::new(0x300, 1, Amount::Range(range))
}

#[test]
fn generator_steps_then_draws_the_top_word() {
    let mut rng = Random::new(1);
    assert_eq!(rng.next_u32(), 0x5d58_8b65);
    assert_eq!(rng.next_u32(), 0xe3b5_43e4);
    let mut zero = Random::new(0);
    assert_eq!(zero.next_u32(), 0);
    assert_eq!(zero.next_u32(), 0x7188_d00c);
}

/// The state that makes the next draw `top`, by inverting one step of the generator.
fn state_before_draw(top: u32) -> u64 {
    const MULTIPLIER: u64 = 0x5D58_8B65_6C07_8965;
    const INCREMENT: u64 = 0x26_9EC3;
    let mut inverse = MULTIPLIER;
    for _ in 0..6 {
        inverse = inverse.wrapping_mul(2u64.wrapping_sub(MULTIPLIER.wrapping_mul(inverse)));
    }
    ((u64::from(top) << 32).wrapping_sub(INCREMENT)).wrapping_mul(inverse)
}

#[test]
fn largest_draw_is_exactly_one_and_next_max_holds_below_the_maximum() {
    let mut rng = Random::new(state_before_draw(u32::MAX));
    assert_eq!(rng.clone().next_u32(), u32::MAX);
    assert_eq!(rng.clone().next_float01(), 1.0);
    assert_eq!(rng.next_max(100), 99);
    let mut rng = Random::new(state_before_draw(0));
    assert_eq!(rng.next_max(100), 0);
}

#[test]
fn next_max_of_nothing_draws_nothing() {
    let mut rng = Random::new(77);
    assert_eq!(rng.next_max(0), 0);
    assert_eq!(rng.next_max(-5), 0);
    assert_eq!(rng, Random::new(77));
}

#[test]
fn c_library_rand_matches_the_published_sequence() {
    // The ANSI C sample generator the game's `rand` is: seed 1 gives 16838, 5758, 10113.
    let mut rand = CRand::new(1);
    let drawn: Vec<u32> = (0..5).map(|_| rand.rand()).collect();
    assert_eq!(drawn, [16838, 5758, 10113, 17515, 31051]);
}

#[test]
fn critical_threshold_follows_the_float_rounding_of_the_game() {
    // Deftness 159: 2.0 + 0.09 comes to 208, one under `200 + (159 - 150)`.
    let rate = calculate_crit_rate(159, 0.0, 0.0, 1.0, 1);
    assert_eq!(critical_threshold(rate), 208);
    // Deftness counts only past 150.
    assert_eq!(
        critical_threshold(calculate_crit_rate(40, 0.0, 0.0, 1.0, 1)),
        200
    );
    assert_eq!(
        critical_threshold(calculate_crit_rate(150, 0.0, 0.0, 1.0, 1)),
        200
    );
    // The wiki: computed in doubles or whole numbers, the threshold is off by one at 151 of the
    // 850 deftness values from 150 to 999.
    let off = (150..=999)
        .filter(|&d| {
            critical_threshold(calculate_crit_rate(d, 0.0, 0.0, 1.0, 1)) != 200 + (d - 150)
        })
        .count();
    assert_eq!(off, 151);
    // A multi-hit weapon divides, a spell halves, a monster never criticals.
    assert_eq!(
        critical_threshold(calculate_crit_rate(150, 0.0, 0.0, 1.0, 2)),
        100
    );
    assert_eq!(
        critical_threshold(calculate_crit_rate(150, 0.0, 0.0, 0.5, 1)),
        100
    );
    assert_eq!(
        critical_threshold(damage::critical_rate(&monster(), &blow())),
        0
    );
}

#[test]
fn monster_hp_comes_from_the_worlds_generator_below_the_table() {
    let mut seen = [false; 3];
    let mut world = Random::new(0x1234_5678);
    for _ in 0..2000 {
        let hp = init_monster_hp(&mut world, 8, false);
        assert!((6..=8).contains(&hp), "slime HP {hp}");
        seen[(hp - 6) as usize] = true;
    }
    assert_eq!(seen, [true; 3]);
    let mut untouched = Random::new(5);
    assert_eq!(init_monster_hp(&mut untouched, 8, true), 8);
    assert_eq!(untouched, Random::new(5), "a set battle draws nothing");
}

/// What `resolve_action` must draw for one party blow at one monster, replayed by hand.
#[test]
fn a_blow_draws_die_critical_dodge_block_accuracy_then_the_amount() {
    let mut actor = party();
    actor.deftness = 400;
    let mut target = monster();
    target.evasion_rate = 4.0;
    target.block_rate = 1.5;
    for seed in 1..200u64 {
        let mut by_hand = Random::new(seed);
        let mut rng = Random::new(seed);
        let _die = by_hand.next_max(100);
        let crit = by_hand.next_max(10_000)
            < critical_threshold(calculate_crit_rate(400, 0.0, 0.0, 1.0, 1));
        let dodged = by_hand.next_max(100) < 4;
        let blocked = !dodged && (by_hand.next_max(100) as f32) < 1.5;
        let _accuracy = by_hand.next_max(100);
        let base = calculate_physical_damage(100, 40, &mut by_hand) as i32;
        let outcome =
            resolve_action(&mut rng, &actor, &blow(), std::slice::from_ref(&target), 0).unwrap();
        let got = &outcome.targets[0];
        assert_eq!(
            (got.critical, got.dodged, got.blocked),
            (crit, dodged, blocked),
            "seed {seed}"
        );
        if !crit && !dodged && !blocked {
            assert_eq!(got.amount, base, "seed {seed}");
            assert_eq!(got.hp_after, target.hp - base);
        }
        if !crit {
            assert_eq!(rng, by_hand, "seed {seed}: same draws, same state");
            assert_eq!(outcome.generator_state, by_hand.state());
        }
    }
}

#[test]
fn a_group_spell_rolls_one_critical_then_a_die_an_accuracy_and_an_amount_per_target() {
    let mut action = flat(30);
    action.reach = Reach::Group;
    action.critical_percent = 50;
    let actor = monster();
    let targets = [monster(), monster(), monster()];
    let mut rng = Random::new(99);
    let mut by_hand = rng.clone();
    let outcome = resolve_action(&mut rng, &actor, &action, &targets, 0).unwrap();
    let _critical_once = by_hand.next_max(10_000);
    for _ in 0..3 {
        by_hand.next_max(100); // die
        by_hand.next_max(100); // accuracy
        by_hand.next_float_between(-0.0, 0.0); // the spread's draw
    }
    assert_eq!(rng, by_hand);
    assert_eq!(outcome.draws, 1 + 3 * 3);
    assert!(
        outcome
            .targets
            .iter()
            .all(|t| t.amount == 30 && !t.critical)
    );
}

#[test]
fn a_single_target_spell_rolls_its_critical_after_the_die() {
    let mut action = flat(30);
    action.critical_percent = 50;
    let mut rng = Random::new(5);
    let mut by_hand = rng.clone();
    resolve_action(&mut rng, &monster(), &action, &[party()], 0).unwrap();
    by_hand.next_max(100);
    by_hand.next_max(10_000);
    by_hand.next_max(100);
    by_hand.next_float_between(-0.0, 0.0);
    assert_eq!(rng, by_hand);
}

#[test]
fn a_dodged_blow_skips_the_block_roll_but_still_draws_its_amount() {
    let mut target = monster();
    target.evasion_rate = 100.0; // every draw is under it
    target.block_rate = 100.0;
    let actor = party();
    let mut rng = Random::new(11);
    let mut by_hand = rng.clone();
    let outcome = resolve_action(&mut rng, &actor, &blow(), &[target], 0).unwrap();
    by_hand.next_max(100); // die
    by_hand.next_max(10_000);
    by_hand.next_max(100); // evasion: dodged
    by_hand.next_max(100); // accuracy
    let crit = outcome.targets[0].critical;
    calculate_physical_damage(100, 40, &mut by_hand);
    if !crit {
        assert_eq!(rng, by_hand);
    }
    let got = &outcome.targets[0];
    assert!(got.dodged && !got.blocked);
    assert_eq!(got.amount, 0);
    assert_eq!(got.hp_after, 100);
}

#[test]
fn a_blocked_blow_deals_nothing_and_a_block_rate_is_not_truncated() {
    let mut target = monster();
    target.block_rate = 100.0;
    let outcome =
        resolve_action(&mut Random::new(3), &party(), &blow(), &[target.clone()], 0).unwrap();
    assert!(outcome.targets[0].blocked);
    assert_eq!(outcome.targets[0].amount, 0);
    // 0.5 blocks on a draw of 0 only: of 3000 seeds about 1 in 200, none with a rate of 0.
    let mut half = monster();
    half.block_rate = 0.5;
    let blocked = (0..3000u64)
        .filter(|&s| {
            resolve_action(&mut Random::new(s), &party(), &blow(), &[half.clone()], 0)
                .unwrap()
                .targets[0]
                .blocked
        })
        .count();
    assert!(
        (5..=40).contains(&blocked),
        "{blocked} blocks of 3000 at 0.5"
    );
    let none = (0..3000u64).any(|s| {
        resolve_action(&mut Random::new(s), &party(), &blow(), &[monster()], 0)
            .unwrap()
            .targets[0]
            .blocked
    });
    assert!(!none);
}

#[test]
fn an_always_critical_action_draws_no_critical_roll_and_a_monster_never_criticals() {
    let mut action = flat(30);
    action.always_critical = true;
    let mut rng = Random::new(8);
    let outcome = resolve_action(&mut rng, &monster(), &action, &[party()], 0).unwrap();
    // die, accuracy, amount, the critical's own draw: no roll of 10000.
    assert_eq!(outcome.draws, 4);
    assert!(outcome.targets[0].critical);
    // A monster spends the critical draw all the same, and never succeeds.
    let mut spell = flat(30);
    spell.critical_percent = 100;
    for seed in 0..500u64 {
        let mut rng = Random::new(seed);
        let outcome = resolve_action(&mut rng, &monster(), &spell, &[party()], 0).unwrap();
        assert!(!outcome.targets[0].critical);
        assert_eq!(outcome.draws, 4, "die, critical, accuracy, amount");
    }
}

#[test]
fn a_plain_attack_critical_is_the_attack_power_not_the_damage_times_up_to_two() {
    // Attack 100 against defence 40: about 40 a blow, a critical about 100 (95 to 105).
    let actor = party();
    let mut target = monster();
    target.evasion_rate = 0.0;
    let mut seen = 0;
    for seed in 0..4000u64 {
        let mut crit_blow = blow();
        crit_blow.always_critical = true;
        let got = &resolve_action(
            &mut Random::new(seed),
            &actor,
            &crit_blow,
            &[target.clone()],
            0,
        )
        .unwrap()
        .targets[0];
        assert!(
            (95..=105).contains(&got.amount),
            "seed {seed}: {}",
            got.amount
        );
        seen += 1;
    }
    assert_eq!(seen, 4000);
}

#[test]
fn a_resistance_stays_a_float_until_the_end_and_the_coin_needs_a_resistance() {
    // A blow of 1 against a resistance of a half is 0.5: above 0, so no coin, truncated to 0.
    let action = flat(1);
    let mut target = monster();
    target.resistances[0] = 50;
    let mut action_fire = action.clone();
    action_fire.element = 1;
    let mut rng = Random::new(21);
    let outcome = resolve_action(&mut rng, &monster(), &action_fire, &[target.clone()], 0).unwrap();
    assert_eq!(outcome.targets[0].amount, 0);
    assert_eq!(outcome.draws, 4, "die, critical, accuracy, amount: no coin");
    // Immune: 0.0 is not above 0 for the coin's gate either, so no coin and nothing.
    target.resistances[0] = 0;
    let mut rng = Random::new(21);
    let outcome = resolve_action(&mut rng, &monster(), &action_fire, &[target], 0).unwrap();
    assert_eq!(outcome.targets[0].amount, 0);
    assert_eq!(outcome.draws, 4);
    // Whole resistance and a blow of 0: a coin of 0 or 1.
    let zero = flat(0);
    let mut seen = [false; 2];
    for seed in 0..100u64 {
        let mut rng = Random::new(seed);
        let outcome = resolve_action(&mut rng, &monster(), &zero, &[party()], 0).unwrap();
        assert_eq!(outcome.draws, 5, "die, critical, accuracy, amount, coin");
        seen[outcome.targets[0].amount as usize] = true;
    }
    assert_eq!(seen, [true; 2]);
}

#[test]
fn the_combo_counter_multiplies_a_chaining_action_and_is_held_to_three() {
    let run = |chains: bool, combo: u8| {
        let mut action = flat(10);
        action.chains_combo = chains;
        resolve_action(&mut Random::new(4), &party(), &action, &[monster()], combo)
            .unwrap()
            .targets[0]
            .amount
    };
    assert_eq!(
        [
            run(true, 0),
            run(true, 1),
            run(true, 2),
            run(true, 3),
            run(true, 9)
        ],
        [10, 12, 15, 20, 20]
    );
    assert_eq!(run(false, 3), 10);
}

#[test]
fn the_cap_holds_damage_and_a_heal_is_held_to_the_maximum() {
    let mut action = flat(500);
    action.max_damage = 999;
    let outcome = resolve_action(&mut Random::new(1), &party(), &action, &[monster()], 0).unwrap();
    assert_eq!(outcome.targets[0].amount, 500);
    assert_eq!(outcome.targets[0].hp_after, 0, "damage floors at 0");
    action.max_damage = 300;
    let outcome = resolve_action(&mut Random::new(1), &party(), &action, &[monster()], 0).unwrap();
    assert_eq!(outcome.targets[0].amount, 300);
    let mut heal = flat(80);
    heal.kind = 2;
    let mut hurt = party();
    hurt.hp = 60;
    let outcome = resolve_action(&mut Random::new(1), &party(), &heal, &[hurt], 0).unwrap();
    assert_eq!(outcome.targets[0].amount, 80);
    assert_eq!(outcome.targets[0].hp_after, 100, "held to the maximum");
}

#[test]
fn a_party_range_without_a_scaling_number_draws_twice() {
    // The medicinal herb: 35 to 35 then the spread's 5.
    let range = ActionRange {
        spread: 5,
        monster_amount: 35,
        party_min: 35,
        party_max: 35,
        lo: 0,
        hi: 1,
        scaling: None,
    };
    let herb = Action::new(255, 2, Amount::Range(range));
    let mut rng = Random::new(6);
    let outcome = resolve_action(&mut rng, &party(), &herb, &[party()], 0).unwrap();
    assert_eq!(
        outcome.draws, 5,
        "die, critical, accuracy and the amount's two"
    );
    assert!((30..=40).contains(&outcome.targets[0].amount));
}

#[test]
fn unsupported_rules_are_refused_before_any_draw() {
    let refuse = |actor: &Fighter, action: &Action, target: &Fighter| {
        let mut rng = Random::new(9);
        let result = resolve_action(&mut rng, actor, action, std::slice::from_ref(target), 0);
        assert_eq!(rng, Random::new(9), "no draw for a refused action");
        result.unwrap_err()
    };
    let mut handler = blow();
    handler.damage_handler = 1;
    assert_eq!(
        refuse(&party(), &handler, &monster()),
        Unsupported::DamageHandler(1)
    );
    let mut special = blow();
    special.number = 0x48;
    assert_eq!(
        refuse(&party(), &special, &monster()),
        Unsupported::ActionNumber(0x48)
    );
    let mut status = blow();
    status.kind = 6;
    assert_eq!(refuse(&party(), &status, &monster()), Unsupported::Kind(6));
    let mut weapon = blow();
    weapon.weapon_element = true;
    let mut fire_sword = party();
    fire_sword.weapon_element = 1;
    assert_eq!(
        refuse(&fire_sword, &weapon, &monster()),
        Unsupported::WeaponElement
    );
    // An action that carries the bit but a weapon with no element is resolved.
    resolve_action(&mut Random::new(1), &party(), &weapon, &[monster()], 0).unwrap();
    let mut falloff = blow();
    falloff.multi_target_falloff = true;
    resolve_action(&mut Random::new(1), &party(), &falloff, &[monster()], 0).unwrap();
    assert_eq!(
        resolve_action(
            &mut Random::new(1),
            &party(),
            &falloff,
            &[monster(), monster()],
            0
        )
        .unwrap_err(),
        Unsupported::MultiTargetFalloff
    );
    let mut die = blow();
    die.trait_accuracy_die = true;
    let mut trained = party();
    trained.accuracy_die_trait = true;
    assert_eq!(
        refuse(&trained, &die, &monster()),
        Unsupported::AccuracyTrait
    );
    let mut claws = party();
    claws.weapon_strikes_several = true;
    assert_eq!(
        refuse(&claws, &blow(), &monster()),
        Unsupported::MultiHitWeapon
    );
    let mut metal = monster();
    metal.metal_body = true;
    assert_eq!(refuse(&party(), &blow(), &metal), Unsupported::MetalBody);
    let mut tense = party();
    tense.tension_level = 2;
    assert_eq!(refuse(&tense, &blow(), &monster()), Unsupported::Tension);
    let mut guarding = monster();
    guarding.guard_level = 1;
    assert_eq!(refuse(&party(), &blow(), &guarding), Unsupported::Guard);
    let mut all_mp = flat(10);
    all_mp.mp_cost = 255;
    assert_eq!(
        refuse(&party(), &all_mp, &monster()),
        Unsupported::MpCost(255)
    );
    assert_eq!(
        resolve_action(&mut Random::new(1), &party(), &blow(), &[], 0).unwrap_err(),
        Unsupported::NoTargets
    );
    assert_eq!(Reach::from_nibble(7), Err(Unsupported::Reach(7)));
}

#[test]
fn grades_map_to_the_rate_table() {
    assert_eq!(grade_rate(0), Some(0.0));
    assert_eq!(grade_rate(4), Some(25.0));
    assert_eq!(grade_rate(5), None);
}
