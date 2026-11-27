//! Unit tests of the battle kernel: generators, the float edges the wiki quotes, the order of
//! draws for several targets (which BattleEmulator, one target at a time, cannot check), the
//! refusals and the rules that no oracle vector exercises.

use std::collections::BTreeMap;

use super::damage::{calculate_crit_rate, calculate_physical_damage, critical_threshold};
use super::resolve::resolve_action_with_attack;
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

/// A metal body: a monster record's `+0x0A` bit 12 (`func_ov000_02156068`).
fn metal_monster() -> Fighter {
    let mut f = monster();
    f.metal_body = true;
    f
}

/// The plain Attack as the kernel reads it against a metal body: `+0x10` bit 24 and an aim at
/// the opponents; no critical, so that only the metal rule decides.
fn metal_blow() -> Action {
    let mut a = blow();
    a.critical_percent = 0;
    a.works_on_metal = true;
    a.aims_at_opponents = true;
    a
}

/// `RollAccuracy`'s metal miss (`ov_000::02156648`): an action without `+0x10` bit 24 lands
/// nothing and spends no accuracy die.
#[test]
fn a_blow_that_does_not_work_on_metal_misses_the_metal_body_before_the_accuracy_die() {
    let mut plain = blow();
    plain.critical_percent = 0;
    let mut rng = Random::new(11);
    let outcome = resolve_action(&mut rng, &party(), &plain, &[metal_monster()], 0).unwrap();
    assert!(!outcome.targets[0].landed);
    assert_eq!(outcome.targets[0].amount, 0);
    assert_eq!(
        outcome.draws, 4,
        "die, critical, evasion, block: no accuracy die, no amount"
    );
    // The same blow on a body that is not metal lands and deals its damage.
    let outcome = resolve_action(&mut Random::new(11), &party(), &plain, &[monster()], 0).unwrap();
    assert!(outcome.targets[0].landed);
    assert!(outcome.targets[0].amount >= 30, "{outcome:?}");
    // Action 0x150, whose `!= 0x150` guard covers only the two status early misses of
    // `RollAccuracy` and not the metal branch: it carries no bit 24, so it misses too.
    let mut unnamed = plain.clone();
    unnamed.number = 0x150;
    let outcome = resolve_action(
        &mut Random::new(11),
        &party(),
        &unnamed,
        &[metal_monster()],
        0,
    )
    .unwrap();
    assert!(!outcome.targets[0].landed);
    assert_eq!(outcome.draws, 4);
}

/// `CalculateFinalDamage`'s metal zeroing (`0x021e77a4`): an action that works on metal still has
/// its damage set to 0, and the coin leaves 0 or 1 — the site's "the others deal 0 or 1".
#[test]
fn a_blow_that_works_on_metal_is_zeroed_and_the_coin_leaves_zero_or_one() {
    let action = metal_blow();
    let mut seen = [false; 2];
    for seed in 0..100u64 {
        let metal = resolve_action(
            &mut Random::new(seed),
            &party(),
            &action,
            &[metal_monster()],
            0,
        )
        .unwrap();
        let got = &metal.targets[0];
        assert!(got.landed && !got.critical, "seed {seed}");
        assert!((0..=1).contains(&got.amount), "seed {seed}: {}", got.amount);
        seen[got.amount as usize] = true;
        // The same action on a body that is not metal takes the blow: the coin is the only draw
        // the metal body adds.
        let plain =
            resolve_action(&mut Random::new(seed), &party(), &action, &[monster()], 0).unwrap();
        assert!(plain.targets[0].amount >= 30, "seed {seed}");
        assert_eq!(metal.draws, plain.draws + 1, "seed {seed}: the coin");
    }
    assert_eq!(seen, [true; 2], "0 and 1 both come out");
}

/// A critical is not zeroed (`0x021e6d1c`): the blow is the attack power's, on a metal body or
/// not.
#[test]
fn a_critical_is_not_zeroed_against_a_metal_body() {
    let mut action = metal_blow();
    action.always_critical = true;
    let metal = resolve_action(
        &mut Random::new(4),
        &party(),
        &action,
        &[metal_monster()],
        0,
    )
    .unwrap();
    let plain = resolve_action(&mut Random::new(4), &party(), &action, &[monster()], 0).unwrap();
    assert!(metal.targets[0].critical);
    assert_eq!(metal.targets[0].amount, plain.targets[0].amount);
    assert!((95..=105).contains(&metal.targets[0].amount));
}

/// The two actions the zeroing spares — Quadraslash `0x205` and Needle Shot `0x82` — and the two
/// that deal `1 + random(2)`, Metal Slash `0x40` and Metalicker `0x7E`.
#[test]
fn quadraslash_and_needle_shot_are_spared_and_metal_slash_deals_one_or_two() {
    for number in [0x205, 0x82] {
        let mut action = metal_blow();
        action.number = number;
        let outcome = resolve_action(
            &mut Random::new(13),
            &party(),
            &action,
            &[metal_monster()],
            0,
        )
        .unwrap();
        assert!(
            outcome.targets[0].amount >= 30,
            "{number:#x}: {:?}",
            outcome.targets[0]
        );
    }
    for number in [0x40, 0x7E] {
        let mut action = metal_blow();
        action.number = number;
        let mut seen = [false; 3];
        for seed in 0..100u64 {
            let outcome = resolve_action(
                &mut Random::new(seed),
                &party(),
                &action,
                &[metal_monster()],
                0,
            )
            .unwrap();
            let amount = outcome.targets[0].amount;
            assert!(
                (1..=2).contains(&amount),
                "{number:#x} seed {seed}: {amount}"
            );
            seen[amount as usize] = true;
        }
        assert_eq!(seen, [false, true, true], "{number:#x}");
    }
}

/// The zeroing is for an action that aims at the opponents (`+0x08` bits 8–9 at 1): an action
/// aimed at its own side leaves a metal body's damage alone.
#[test]
fn the_metal_zeroing_needs_an_action_that_aims_at_the_opponents() {
    let mut action = metal_blow();
    action.aims_at_opponents = false;
    let outcome = resolve_action(
        &mut Random::new(13),
        &party(),
        &action,
        &[metal_monster()],
        0,
    )
    .unwrap();
    assert!(outcome.targets[0].amount >= 30, "{outcome:?}");
}

/// The two weapon flags: the "+1 against metal" weapon (`func_02085128`) and the poison needle
/// (`func_020851d8`), both read on a party blow carrying `+0x10` bit 18.
#[test]
fn the_plus_one_weapon_makes_one_and_the_poison_needle_deals_exactly_one() {
    let mut action = metal_blow();
    action.weapon_element = true;
    let mut armed = party();
    armed.metal_bonus = true;
    // The coin still leaves 0 or 1 (`0x021e77a4`), then the weapon adds 1.0: the metal family's
    // weapons deal 1 or 2, never the blow.
    let mut seen = [false; 3];
    for seed in 0..40u64 {
        let outcome = resolve_action(
            &mut Random::new(seed),
            &armed,
            &action,
            &[metal_monster()],
            0,
        )
        .unwrap();
        let amount = outcome.targets[0].amount;
        assert!((1..=2).contains(&amount), "seed {seed}: {amount}");
        seen[amount as usize] = true;
    }
    assert_eq!(seen, [false, true, true]);
    // The bonus is the metal body's: a body that is not metal takes the same blow as before.
    let with = resolve_action(&mut Random::new(5), &armed, &action, &[monster()], 0).unwrap();
    let without = resolve_action(&mut Random::new(5), &party(), &action, &[monster()], 0).unwrap();
    assert_eq!(with.targets[0].amount, without.targets[0].amount);

    // The needle: exactly 1, and the dodge and the block it caused are cleared.
    let mut needle = party();
    needle.poison_needle = true;
    let mut dodging = metal_monster();
    dodging.evasion_rate = 100.0;
    let outcome = resolve_action(&mut Random::new(5), &needle, &action, &[dodging], 0).unwrap();
    assert!(!outcome.targets[0].dodged, "the needle is not dodged");
    assert_eq!(outcome.targets[0].amount, 1);
    let outcome = resolve_action(&mut Random::new(5), &needle, &action, &[monster()], 0).unwrap();
    assert_eq!(
        outcome.targets[0].amount, 1,
        "and on a body that is not metal"
    );
    // Without the weapon's `+0x10` bit 18 the flag is not read.
    let mut unarmed = action.clone();
    unarmed.weapon_element = false;
    let outcome = resolve_action(
        &mut Random::new(5),
        &needle,
        &unarmed,
        &[metal_monster()],
        0,
    )
    .unwrap();
    assert!((0..=1).contains(&outcome.targets[0].amount));
}

/// The weapon's killer bonus by the target's family (`CalculateFinalDamage`,
/// `0x021e7200`–`0x021e7494`): a party blow carrying `+0x10` bit 18 multiplies by the worn
/// weapon's row for the target's own family, in tenths over ten, in the game's single precision.
#[test]
fn the_weapon_family_bonus_multiplies_a_party_blow_by_its_tenths() {
    let mut action = flat(10);
    action.weapon_element = true;
    let amount = |actor: &Fighter, target: &Fighter| {
        let outcome = resolve_action(
            &mut Random::new(1),
            actor,
            &action,
            std::slice::from_ref(target),
            0,
        )
        .unwrap();
        (outcome.targets[0].amount, outcome.draws)
    };
    let mut actor = party();
    let mut target = monster();
    target.family = 2;
    let baseline = amount(&actor, &target);
    assert_eq!(baseline.0, 10, "ten tenths, no bonus");

    actor.family_bonus[1] = 11;
    let boosted = amount(&actor, &target);
    assert_eq!(boosted.0, 11, "eleven tenths");
    assert_eq!(boosted.1, baseline.1, "the bonus draws nothing");
    actor.family_bonus[1] = 12;
    assert_eq!(amount(&actor, &target).0, 12, "twelve tenths");
    actor.family_bonus[1] = 13;
    assert_eq!(amount(&actor, &target).0, 13, "thirteen tenths");
    actor.family_bonus[1] = 1;
    assert_eq!(amount(&actor, &target).0, 1, "one tenth: a tenth of ten");
    // Only the target's own family is read: the other eleven rows are ten.
    target.family = 7;
    assert_eq!(amount(&actor, &target).0, 10);
}

/// The family multiplication's gate: a party attacker, `+0x10` bit 18, and a target that answers
/// a family. A monster attacker never enters the branch and a party index never answers one.
#[test]
fn the_weapon_family_bonus_needs_the_party_the_bit_and_the_target_family() {
    let mut action = flat(10);
    action.weapon_element = true;
    let mut actor = party();
    actor.family_bonus[1] = 12;
    let mut target = monster();
    target.family = 2;
    let amount = |actor: &Fighter, action: &Action, target: &Fighter| {
        resolve_action(
            &mut Random::new(1),
            actor,
            action,
            std::slice::from_ref(target),
            0,
        )
        .unwrap()
        .targets[0]
            .amount
    };
    assert_eq!(amount(&actor, &action, &target), 12);

    // The action's `+0x10` bit 18 is the gate.
    let mut without = action.clone();
    without.weapon_element = false;
    assert_eq!(amount(&actor, &without, &target), 10);
    // A party target answers no family (`func_ov000_02156068` with a party index): 0, never a row.
    assert_eq!(amount(&actor, &action, &party()), 10);
    // A monster attacker never reaches the weapon record.
    let mut beast = monster();
    beast.family_bonus[1] = 12;
    assert_eq!(amount(&beast, &action, &target), 10);
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
    // An action with the falloff bit resolves against several targets (`docs/re/battle.md`,
    // "The several-target falloff"; the ranks are pinned by the dedicated tests below).
    let mut falloff = blow();
    falloff.multi_target_falloff = true;
    resolve_action(&mut Random::new(1), &party(), &falloff, &[monster()], 0).unwrap();
    resolve_action(
        &mut Random::new(1),
        &party(),
        &falloff,
        &[monster(), monster()],
        0,
    )
    .unwrap();
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
    // A level the code does not hold is refused; 1 to 4 are modelled (the tests below).
    let mut tense = party();
    tense.tension_level = 5;
    assert_eq!(refuse(&tense, &blow(), &monster()), Unsupported::Tension);
    let mut guarding = monster();
    guarding.guard_level = 4;
    assert_eq!(refuse(&party(), &blow(), &guarding), Unsupported::Guard);
    // A defended target halves the blow; the draws are the same.
    let mut defended = monster();
    defended.guard_level = 1;
    let mut halving = blow();
    halving.halved_by_defending = true;
    let plain = resolve_action(&mut Random::new(7), &party(), &blow(), &[monster()], 0).unwrap();
    let held = resolve_action(&mut Random::new(7), &party(), &halving, &[defended], 0).unwrap();
    assert_eq!(held.targets[0].amount, plain.targets[0].amount / 2);
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

#[test]
fn a_dazzled_striker_throws_the_die_of_eight_and_misses_under_five() {
    let mut action = flat(5);
    action.spoilt = true;
    let mut dazzled = party();
    dazzled.dazzled = true;
    let mut hits_clear = 0;
    let mut hits_dazzled = 0;
    for seed in 0..200u64 {
        let clear =
            resolve_action(&mut Random::new(seed), &party(), &action, &[monster()], 0).unwrap();
        let dim =
            resolve_action(&mut Random::new(seed), &dazzled, &action, &[monster()], 0).unwrap();
        hits_clear += i32::from(clear.targets[0].landed);
        hits_dazzled += i32::from(dim.targets[0].landed);
        if clear.targets[0].landed {
            // The die is drawn only while dazzled: one more draw then.
            assert!(
                dim.draws == clear.draws + 1 || !dim.targets[0].landed,
                "seed {seed}: {} vs {}",
                clear.draws,
                dim.draws
            );
        }
    }
    assert_eq!(hits_clear, 200, "an unscaled action always lands");
    assert!(
        (40..=160).contains(&hits_dazzled),
        "five faces of eight miss: {hits_dazzled} of 200"
    );
}

#[test]
fn the_poison_needle_auto_hits_after_the_die_of_one_hundred_before_the_die_of_eight() {
    let mut action = flat(5);
    action.spoilt = true;
    action.weapon_element = true;
    let mut needle = party();
    needle.dazzled = true;
    needle.poison_needle = true;
    // The same dazzled attacker without the needle, and the needle's bearer on an action without
    // `+0x10` bit 18: both keep the die of eight.
    let mut bare = party();
    bare.dazzled = true;
    let mut unflagged = action.clone();
    unflagged.weapon_element = false;
    let (mut misses_bare, mut misses_unflagged) = (0, 0);
    for seed in 0..64u64 {
        let hit =
            resolve_action(&mut Random::new(seed), &needle, &action, &[monster()], 0).unwrap();
        assert!(
            hit.targets[0].landed,
            "seed {seed}: the needle auto-hits through the dazzle"
        );
        assert_eq!(hit.targets[0].amount, 1, "seed {seed}: the needle's 1.0");
        // The die at `[battle + 0x8e6e]`, the critical, the die of 100, the range's own and its
        // spread: five draws, no die of eight.
        assert_eq!(hit.draws, 5, "seed {seed}");
        let mut by_hand = Random::new(seed);
        by_hand.next_max(100);
        by_hand.next_max(10_000);
        by_hand.next_max(100);
        by_hand.next_float_between(5.0, 5.0);
        by_hand.next_float_between(0.0, 0.0);
        assert_eq!(
            hit.generator_state,
            by_hand.state(),
            "seed {seed}: the die of 100 is still drawn"
        );

        let shrugged =
            resolve_action(&mut Random::new(seed), &bare, &action, &[monster()], 0).unwrap();
        let no_bit =
            resolve_action(&mut Random::new(seed), &needle, &unflagged, &[monster()], 0).unwrap();
        assert_eq!(
            shrugged.targets[0].landed, no_bit.targets[0].landed,
            "seed {seed}: the gate needs both flags"
        );
        // The die of eight is drawn either way; the amount only after landing.
        let spoiled = if shrugged.targets[0].landed { 6 } else { 4 };
        assert_eq!(shrugged.draws, spoiled, "seed {seed}: the die of eight");
        assert_eq!(no_bit.draws, spoiled, "seed {seed}: the die of eight");
        misses_bare += i32::from(!shrugged.targets[0].landed);
        misses_unflagged += i32::from(!no_bit.targets[0].landed);
    }
    assert!(
        (16..=48).contains(&misses_bare),
        "five faces of eight miss: {misses_bare} of 64"
    );
    assert_eq!(misses_bare, misses_unflagged);
}

#[test]
fn a_scaling_accuracy_is_the_chance_times_the_resistance() {
    let mut action = flat(5);
    action.scales_accuracy = true;
    action.accuracy_monster = 50;
    action.landing_element = 2;
    let mut target = party();
    target.resistances[1] = 50;
    // 50 × 0.5 + 0.5 = 25.5, truncated: it lands under 25.
    let mut hits = 0;
    for seed in 0..400u64 {
        let outcome = resolve_action(
            &mut Random::new(seed),
            &monster(),
            &action,
            &[target.clone()],
            0,
        )
        .unwrap();
        hits += i32::from(outcome.targets[0].landed);
    }
    assert!((60..=160).contains(&hits), "{hits} of 400 at 25 in 100");
}

#[test]
fn weird_dance_takes_mp_and_the_flight_action_removes_its_monster() {
    let mut dance = flat(5);
    dance.kind = 57;
    let mut target = monster();
    target.mp = 3;
    let outcome =
        resolve_action(&mut Random::new(1), &party(), &dance, &[target.clone()], 0).unwrap();
    assert_eq!(
        outcome.targets[0].mp_damage, 3,
        "held under the MP there is"
    );
    assert_eq!(outcome.targets[0].hp_after, target.hp);

    let mut flee = Action::new(0xE1, 0, Amount::Blow { multiplier: 1.0 });
    flee.reach = Reach::Actor;
    let actor = monster();
    let outcome = resolve_action(
        &mut Random::new(1),
        &actor,
        &flee,
        std::slice::from_ref(&actor),
        0,
    )
    .unwrap();
    assert!(outcome.targets[0].fled, "the monster left");
    assert_eq!(outcome.targets[0].hp_after, 0);
}

/// The plain Attack as the ROM's action 1 carries it: `+0x10` bit 9, the attack buff's gate.
fn buffed_blow() -> Action {
    let mut a = Action::new(1, 1, Amount::Blow { multiplier: 1.0 });
    a.reach = Reach::Attack;
    a.element = 8;
    a.landing_element = 8;
    a.attack_buff_applies = true;
    a
}

/// A self-buff of the attack level: Oomph's kind 3 with the step the action's `+0x30` holds.
fn attack_buff(step: i16) -> Skill {
    let mut a = Action::new(49, 3, Amount::Blow { multiplier: 1.0 });
    a.reach = Reach::One;
    a.stat_step = step;
    let mut skill = Skill::from_action(a);
    skill.aims = 2;
    skill
}

fn attack_buff_monster() -> Monster {
    let mut f = Fighter::new(Side::Monster);
    f.hp = 5000;
    f.max_hp = 5000;
    f.attack = 10;
    f.defence = 40;
    f.agility = 5;
    Monster {
        number: 1,
        table_hp: 5000,
        group: 0,
        fighter: f,
        ways: [1; 6],
        selector: 0,
        ai_mode: 1,
        extra_class: 0,
        once_mask: 0,
        remembers: false,
        experience: 2,
        gold: 4,
        drops: [0, 0],
        drop_steps: [0, 0],
    }
}

/// A battle whose hero has `attack`, with the plain Attack in the monster's ways.
fn attack_buff_battle(attack: i32) -> Battle {
    let mut f = Fighter::new(Side::Party);
    f.hp = 1000;
    f.max_hp = 1000;
    f.mp = 10;
    f.attack = attack;
    f.defence = 0;
    f.agility = 10;
    f.evasion_rate = 2.0;
    let mut skills = BTreeMap::new();
    skills.insert(1, Skill::from_action(buffed_blow()));
    let setup = BattleSetup {
        monsters: vec![attack_buff_monster()],
        skills,
        set_battle: false,
    };
    Battle::new(
        setup,
        vec![PartyMember::new(f)],
        Surprise::None,
        0x1234_5678_9abc_def0,
    )
    .unwrap()
}

/// The hero's blow amount in one round of a battle at `attack` with `level` already applied.
fn blow_amount(attack: i32, level: i8) -> i32 {
    let mut b = attack_buff_battle(attack);
    b.party_status[0].attack_level = level;
    let command = Command::Act {
        skill: Skill::from_action(buffed_blow()),
        target: Target::Monster(0),
    };
    let report = b.round(&[command], &mut Random::new(7)).unwrap();
    report
        .events
        .iter()
        .find_map(|e| match e {
            Event::Acted { who, outcomes, .. } if *who == Who::Party(0) => {
                Some(outcomes[0].1.amount)
            }
            _ => None,
        })
        .expect("the hero acted")
}

/// `CalculateAttackBuffMultiplier` (`0x020745fc`) is `1 + 0.25 × level`, and
/// `UpdateCombatantAttack` (`0x02155a50`) truncates its product (`_ffixu`) and holds a party
/// member under 999. A level therefore scales a physical blow by 1.25 and 1.5 up, 0.75 and 0.5
/// down — 128.75 is 128, 154.5 is 154, and 700 × 1.5 is the cap.
#[test]
fn the_attack_level_multiplies_a_physical_blow_and_truncates() {
    for (level, base, effective) in [
        (0i8, 103i32, 103i32),
        (1, 103, 128),
        (2, 103, 154),
        (1, 800, 999),
        (2, 700, 999),
        (-1, 103, 77),
        (-2, 103, 51),
    ] {
        assert_eq!(
            blow_amount(base, level),
            blow_amount(effective, 0),
            "level {level} on an attack of {base} is {effective}"
        );
    }
}

/// A kind 3 action writes the attack level (`func_0208776c` / `func_020877c0`): the step's sign
/// chooses up or down, a second application moves it again, the level is held to ±2 and every
/// write restarts a count of **five** at `+0x6e` (`mov r1, #5`, not defence's six).
#[test]
fn a_kind_three_action_moves_the_attack_level_by_its_step() {
    let mut b = attack_buff_battle(100);
    let up = Command::Act {
        skill: attack_buff(1),
        target: Target::Party(0),
    };
    b.round(std::slice::from_ref(&up), &mut Random::new(1))
        .unwrap();
    let status = b.status(Who::Party(0));
    // The write sets five and the same action's pass takes one off (`func_ov000_021599f4`).
    assert_eq!(
        (
            status.attack_level,
            status.attack_count,
            status.attack_second
        ),
        (1, 4, 0)
    );
    b.round(std::slice::from_ref(&up), &mut Random::new(2))
        .unwrap();
    let status = b.status(Who::Party(0));
    assert_eq!((status.attack_level, status.attack_count), (2, 4));
    b.round(&[up], &mut Random::new(3)).unwrap();
    let status = b.status(Who::Party(0));
    assert_eq!(
        (status.attack_level, status.attack_count),
        (2, 3),
        "held to two; the refused write leaves the count running"
    );

    // Blunt's step is the same kind with a negative `+0x30`: −2 from 0 reaches −2.
    let mut b = attack_buff_battle(100);
    let down = Command::Act {
        skill: attack_buff(-2),
        target: Target::Party(0),
    };
    b.round(&[down], &mut Random::new(1)).unwrap();
    let status = b.status(Who::Party(0));
    assert_eq!((status.attack_level, status.attack_count), (-2, 4));
}

/// A generator whose next `NextRandomMax(100)` is exactly `draw`.
fn generator_drawing(draw: i32) -> Random {
    (0u64..)
        .map(Random::new)
        .find(|r| {
            let mut r = r.clone();
            r.next_max(100) == draw
        })
        .expect("a seed for every draw")
}

fn tick_attack(status: Status, draw: i32) -> Status {
    let mut b = attack_buff_battle(100);
    b.party_status[0] = status;
    b.rng = generator_drawing(draw);
    b.tick_status(Who::Party(0));
    *b.status(Who::Party(0))
}

/// The attack level wears off on the same table and rolls as defence and agility
/// (`0x02182ad4` through `0x02158d68`, `func_ov000_0215858c`'s `+0x91` block): the first of the
/// four rolls reads index 3 and clears on 62.5 %, the last always.
#[test]
fn the_attack_level_wears_off_on_its_twins_rolls() {
    let holding = |second: u8| Status {
        attack_level: 1,
        attack_count: 5,
        attack_second: second,
        ..Status::default()
    };
    let cleared = tick_attack(holding(4), 62);
    assert_eq!(
        (
            cleared.attack_level,
            cleared.attack_second,
            cleared.attack_count
        ),
        (0, 0, 0)
    );
    let held = tick_attack(holding(4), 63);
    assert_eq!((held.attack_level, held.attack_second), (1, 3));
    assert_eq!(tick_attack(holding(3), 74).attack_level, 0);
    assert_eq!(tick_attack(holding(3), 75).attack_second, 2);
    assert_eq!(tick_attack(holding(2), 87).attack_level, 0);
    assert_eq!(tick_attack(holding(2), 88).attack_second, 1);
    assert_eq!(tick_attack(holding(1), 99).attack_level, 0);
}

/// The count of the attack write runs down over the holder's passes and hands over to the
/// four wear-off rolls at 0 (`func_ov000_021599f4`, the pair table `0x02182efc` at `+0x12`)
/// without touching defence or agility.
#[test]
fn the_attack_count_runs_down_before_its_second_count() {
    let mut b = attack_buff_battle(100);
    b.party_status[0].attack_level = 1;
    b.party_status[0].attack_count = 1;
    b.party_status[0].attack_second = 0;
    b.decrement_status(Who::Party(0));
    let status = b.status(Who::Party(0));
    assert_eq!((status.attack_count, status.attack_second), (0, 4));
    assert_eq!(
        (status.defence_count, status.agility_count),
        (0, 0),
        "the twins keep their own counts"
    );
}

/// The critical's own term is the **base** attack: the original reads `+0x134` `+0x34` there
/// (`0x021e6e2c`) whatever the attack level, while the buffed word feeds the blow. Two different
/// buffed attacks therefore leave a critical's amount alone, but not an ordinary blow's.
#[test]
fn the_attack_level_does_not_feed_the_criticals_own_attack_term() {
    let mut critical = blow();
    critical.always_critical = true;
    let actor = party();
    let target = monster();
    let amount = |action: &Action, attack: i32| {
        resolve_action_with_attack(
            &mut Random::new(9),
            &actor,
            action,
            std::slice::from_ref(&target),
            0,
            Some(attack),
        )
        .unwrap()
        .targets[0]
            .amount
    };
    assert_eq!(
        amount(&critical, 125),
        amount(&critical, 150),
        "the critical reads the base attack, not the buffed one"
    );
    let mut plain = critical.clone();
    plain.always_critical = false;
    plain.critical_percent = 0;
    assert_ne!(
        amount(&plain, 125),
        amount(&plain, 150),
        "an ordinary blow does read the buffed attack"
    );
}

/// A group action of the twelve records carrying `+0x10` bit 17 (`94`, Party Pooper, EU tables)
/// whose amount is `amount` for every target: no spread, no critical, no dodge or block,
/// element 0. The falloff tests vary only the target list and the target's own factors.
fn weakening(amount: u16) -> Action {
    let mut a = flat(amount);
    a.number = 94;
    a.reach = Reach::Group;
    a.multi_target_falloff = true;
    a
}

/// The amounts of one action's targets, in the order they were given.
fn amounts(actor: &Fighter, action: &Action, targets: &[Fighter]) -> Vec<i32> {
    resolve_action(&mut Random::new(3), actor, action, targets, 0)
        .unwrap()
        .targets
        .iter()
        .map(|target| target.amount)
        .collect()
}

/// `ResolveAction` (`0x021ec6ec`, table `0x020e88e0`): each target is multiplied by the rank's
/// entry, the rank being its **index in the resolution loop**, not its stats or its distance.
/// The target with the highest guard comes first, so a factor read from anything but the list's
/// order would not produce this sequence.
#[test]
fn the_falloff_follows_the_target_lists_order() {
    let mut action = weakening(100);
    action.halved_by_defending = true;
    let guarded = |level: u8| {
        let mut f = monster();
        f.guard_level = level;
        f
    };
    let targets = [guarded(2), guarded(0), guarded(1)];
    // Rank 0 keeps 1.0 (100 × 0.1), rank 1 takes 0.8 (100 × 0.5), rank 2 0.6 (100 × 0.1).
    assert_eq!(amounts(&party(), &action, &targets), [10, 80, 30]);
}

/// The table's entries by rank: 1.0, 0.8 and 0.6 for three targets; 1.0 alone for one, so a
/// single target is **not** weakened (the table starts at 1.0: the open point is settled).
#[test]
fn two_and_three_targets_take_one_four_fifths_and_three_fifths() {
    let action = weakening(100);
    assert_eq!(amounts(&party(), &action, &[monster()]), [100]);
    assert_eq!(
        amounts(&party(), &action, &[monster(), monster()]),
        [100, 80]
    );
    assert_eq!(
        amounts(&party(), &action, &[monster(), monster(), monster()]),
        [100, 80, 60]
    );
}

/// The rank is held to the table's five entries (`func_02074948` clamps its index to 4): a sixth
/// target and every one after it keep ×0.2.
#[test]
fn the_rank_holds_at_the_tables_fifth_entry() {
    let targets = vec![monster(); 7];
    assert_eq!(
        amounts(&party(), &weakening(100), &targets),
        [100, 80, 60, 40, 20, 20, 20]
    );
}

/// The factor is applied to the base amount and **truncated there** (`_fflt`/`_fmul`/`_ffix` at
/// `0x021ec6ec`), before `CalculateFinalDamage`: 21 × 0.8 is 16.8, read as 16; 27 × 0.8 is 21.6,
/// read as 21, and the resistance's 0.75 then takes it to 15.75 → 15 — where a factor applied
/// after the resistance would give 27 × 0.75 = 20.25 → 20, × 0.8 = 16.
#[test]
fn the_rank_factor_truncates_the_base_before_the_final_damage() {
    assert_eq!(
        amounts(&party(), &weakening(21), &[monster(), monster()]),
        [21, 16]
    );
    let mut halved = Vec::new();
    let mut action = weakening(27);
    action.element = 8;
    let mut weak = monster();
    weak.resistances[7] = 75;
    assert_eq!(amounts(&party(), &action, &[weak.clone(), weak]), {
        halved.extend([20, 15]);
        halved
    });
}

/// The two plain Attack records (1 and 2) are armed by the attacker's weapon, not by the action
/// alone: a party member whose weapon is no group weapon keeps every target's full amount, a
/// monster never weakens, and the Attack variants 223 and 224 always do — they are not in the
/// gate's `1 | 2` test.
#[test]
fn the_plain_attack_takes_its_falloff_from_the_weapon() {
    let mut party_attack = weakening(100);
    party_attack.number = 1;
    assert_eq!(
        amounts(&party(), &party_attack, &[monster(), monster()]),
        [100, 100]
    );

    // The Attack that is not number 1 (2) reaches the gate; with the group weapon bit it weakens
    // (a weapon with bit 1 set keeps the factor too, which the kernel does not model).
    let mut weapon_attack = weakening(100);
    weapon_attack.number = 2;
    let mut armed = party();
    armed.weapon_strikes_several = true;
    assert_eq!(
        amounts(&armed, &weapon_attack, &[monster(), monster()]),
        [100, 80]
    );

    let mut monster_actor = Fighter::new(Side::Monster);
    monster_actor.attack = 100;
    assert_eq!(
        amounts(&monster_actor, &party_attack, &[monster(), monster()]),
        [100, 100]
    );

    let mut variant = weakening(100);
    variant.number = 223;
    assert_eq!(
        amounts(&party(), &variant, &[monster(), monster()]),
        [100, 80]
    );
}

/// A kind-1 action whose base amount is exactly `amount` and that tension works on: the range
/// record's least and most are equal and the spread is 0, so the base is `amount` with no
/// randomness, and the element is 0, i.e. resistance 1.0.
fn tensioned_action(amount: u16) -> Action {
    let mut a = flat(amount);
    a.number = 0x300;
    a.tension_applies = true;
    a
}

/// The dealer's flat bonus, `CalculateTensionBonus` (`0x02074774`): `(1 + level / 10) × level`,
/// the division a whole number's. A dealer of level 25 gives `3 × level`.
fn flat_bonus(tension: u8) -> f32 {
    3.0 * f32::from(tension)
}

/// Tension multiplies the damage and adds a flat bonus by dealer kind (`0x020e8908`): party
/// `1.0 1.5 2.5 4.0 6.0`, monster `1.0 1.3 2.0 3.0 4.5`, the level 0 vector unchanged and no
/// extra draw.
#[test]
fn tension_multiplies_the_damage_and_adds_a_flat_bonus_by_side() {
    let action = tensioned_action(100);
    let party_mults = [1.0f32, 1.5, 2.5, 4.0, 6.0];
    let monster_mults = [1.0f32, 1.3, 2.0, 3.0, 4.5];
    let plain = resolve_action(&mut Random::new(3), &party(), &action, &[monster()], 0).unwrap();
    assert_eq!(plain.targets[0].amount, 100);
    assert!(!plain.tension_spent, "level 0 spends nothing");

    for (level, mult) in party_mults.iter().enumerate().skip(1) {
        let mut actor = party();
        actor.tension_level = level as u8;
        actor.level = 25;
        let outcome =
            resolve_action(&mut Random::new(3), &actor, &action, &[monster()], 0).unwrap();
        let expected = (100.0f32 * mult + flat_bonus(level as u8)) as i32;
        assert_eq!(outcome.targets[0].amount, expected, "party level {level}");
        assert_eq!(outcome.draws, plain.draws, "tension adds no draw");
        assert!(outcome.tension_spent);
    }
    for (level, mult) in monster_mults.iter().enumerate().skip(1) {
        let mut actor = monster();
        actor.attack = 100;
        actor.tension_level = level as u8;
        actor.level = 25;
        let outcome = resolve_action(&mut Random::new(3), &actor, &action, &[party()], 0).unwrap();
        let expected = (100.0f32 * mult + flat_bonus(level as u8)) as i32;
        assert_eq!(outcome.targets[0].amount, expected, "monster level {level}");
    }
}

/// The spend: an action carrying `+0x10` bit 13 reports it, whatever its outcome, and once the
/// caller has cleared the level the next action is not multiplied (`0x021ed48c`). A hit, a miss
/// and a dodge all spend it.
#[test]
fn the_tension_is_spent_by_the_action_that_uses_it() {
    let action = tensioned_action(100);
    let mut actor = party();
    actor.tension_level = 2;
    actor.level = 25;
    let first = resolve_action(&mut Random::new(5), &actor, &action, &[monster()], 0).unwrap();
    assert!(first.tension_spent);
    assert_eq!(first.targets[0].amount, 256);
    // The caller clears the level; the second action is the plain one.
    actor.tension_level = 0;
    let second = resolve_action(&mut Random::new(5), &actor, &action, &[monster()], 0).unwrap();
    assert!(!second.tension_spent);
    assert_eq!(second.targets[0].amount, 100);

    // A miss still spends it (`0x021ec868`): the accuracy die of 8 under the spoilt roll.
    let mut spoilt = tensioned_action(100);
    spoilt.spoilt = true;
    let mut dazzled = party();
    dazzled.tension_level = 1;
    dazzled.dazzled = true;
    dazzled.level = 25;
    let mut spent_on_miss = false;
    for seed in 0..64 {
        let outcome =
            resolve_action(&mut Random::new(seed), &dazzled, &spoilt, &[monster()], 0).unwrap();
        assert!(outcome.tension_spent);
        if !outcome.targets[0].landed {
            spent_on_miss = true;
        }
    }
    assert!(spent_on_miss, "some seed misses and still spends");
}

/// A metal body takes the multiplier a second time after its 1-or-2 and no flat bonus
/// (`0x021e79d0`): the 0-or-1 coin is multiplied again, so a level 4 party blow deals 0 or 6.
#[test]
fn a_metal_body_takes_the_multiplier_twice_and_no_flat_bonus() {
    let mut metal = monster();
    metal.metal_body = true;
    let mut action = tensioned_action(100);
    action.works_on_metal = true;
    action.aims_at_opponents = true;
    let plain = resolve_action(&mut Random::new(11), &party(), &action, &[metal.clone()], 0)
        .unwrap()
        .targets[0]
        .amount;
    assert!((0..=1).contains(&plain), "the coin: {plain}");
    let mut actor = party();
    actor.tension_level = 4;
    actor.level = 25;
    let tense = resolve_action(&mut Random::new(11), &actor, &action, &[metal], 0)
        .unwrap()
        .targets[0]
        .amount;
    assert_eq!(tense, plain * 6);
}

/// A **critical** blow that works on a metal body has the head's tension undone
/// (`0x021e6d1c`) and then the metal arm's single multiply (`0x021e79d0`): one multiplier, not
/// two — `m ×` the untensioned amount, not `m²`.
#[test]
fn a_critical_metal_blow_undoes_the_head_multiply() {
    let mut metal = monster();
    metal.metal_body = true;
    let mut action = tensioned_action(100);
    action.works_on_metal = true;
    action.aims_at_opponents = true;
    action.always_critical = true;
    let plain = resolve_action(&mut Random::new(13), &party(), &action, &[metal.clone()], 0)
        .unwrap()
        .targets[0]
        .amount;
    let mut actor = party();
    actor.tension_level = 4;
    actor.level = 25;
    let tense = resolve_action(&mut Random::new(13), &actor, &action, &[metal], 0)
        .unwrap()
        .targets[0]
        .amount;
    // The critical's own draw multiplies the untensioned 100 by 1.5 to 2; the metal arm then
    // multiplies once by 6. Without the undo the head's 6 would square to 36.
    assert!(
        (plain * 6).abs_diff(tense) <= 6,
        "one multiplier: plain {plain}, tensed {tense}"
    );
}

/// A target at the maximum (level 4) has a landed kind-1 amount halved (`0x021e7a58`), and a
/// heal (kind 2) is not.
#[test]
fn a_target_at_the_maximum_takes_half_a_kind_one_amount() {
    let mut full = monster();
    full.tension_level = 4;
    let plain = resolve_action(
        &mut Random::new(3),
        &party(),
        &tensioned_action(101),
        &[monster()],
        0,
    )
    .unwrap()
    .targets[0]
        .amount;
    let halved = resolve_action(
        &mut Random::new(3),
        &party(),
        &tensioned_action(101),
        &[full.clone()],
        0,
    )
    .unwrap()
    .targets[0]
        .amount;
    assert_eq!(plain, 101);
    assert_eq!(halved, 50);

    let mut heal = tensioned_action(101);
    heal.kind = 2;
    let held = resolve_action(&mut Random::new(3), &party(), &heal, &[full], 0)
        .unwrap()
        .targets[0]
        .amount;
    assert_eq!(held, 101);
}

/// A battle with one hero and one monster, the hero's action a tensioned kind-1 amount of 100.
fn tension_battle(seed: u64) -> Battle {
    let mut f = Fighter::new(Side::Party);
    f.hp = 1000;
    f.max_hp = 1000;
    f.mp = 10;
    f.attack = 100;
    f.defence = 0;
    f.agility = 10;
    f.evasion_rate = 2.0;
    f.level = 25;
    let mut skills = BTreeMap::new();
    skills.insert(1, Skill::from_action(tensioned_action(100)));
    let setup = BattleSetup {
        monsters: vec![attack_buff_monster()],
        skills,
        set_battle: false,
    };
    Battle::new(setup, vec![PartyMember::new(f)], Surprise::None, seed).unwrap()
}

/// The hero's amount in one round of `battle`.
fn hero_amount(battle: &mut Battle) -> i32 {
    let command = Command::Act {
        skill: Skill::from_action(tensioned_action(100)),
        target: Target::Monster(0),
    };
    let report = battle.round(&[command], &mut Random::new(7)).unwrap();
    report
        .events
        .iter()
        .find_map(|e| match e {
            Event::Acted { who, outcomes, .. } if *who == Who::Party(0) => {
                Some(outcomes[0].1.amount)
            }
            _ => None,
        })
        .expect("the hero acted")
}

/// The whole battle's spend: a tensioned status multiplies the round's blow, the status goes to
/// 0 whatever happened, and the second round's amount is the untensioned one (`0x021ed48c`).
#[test]
fn a_battle_spends_the_hero_s_tension_once() {
    let mut tense = tension_battle(0x1234);
    let mut control = tension_battle(0x1234);
    tense.party_status[0].tension_level = 2;
    let first = hero_amount(&mut tense);
    let plain_first = hero_amount(&mut control);
    assert_eq!(first, (plain_first as f32 * 2.5 + 6.0) as i32);
    assert_eq!(tense.status(Who::Party(0)).tension_level, 0);
    let second = hero_amount(&mut tense);
    let plain_second = hero_amount(&mut control);
    assert_eq!(second, plain_second);
}

/// Psyche Up's shape: kind 15, reaching the actor, `+0x30` as the rise. The amount is the
/// record's no-range blow, which the handler ignores (`ov_024::021dc93c`).
fn psyche(step: i16) -> Action {
    let mut a = Action::new(161, 15, Amount::Blow { multiplier: 1.0 });
    a.reach = Reach::Actor;
    a.stat_step = step;
    a
}

/// A landed kind-15 action raises the target's level through its `+0x30` (`ov_024::021dc93c`,
/// the handler's `iVar5 = *(short *)(action + 0x30)` loop), changes no HP and no MP, and spends
/// no tension of its own (the record carries no `+0x10` bit 13).
#[test]
fn a_kind_fifteen_action_raises_the_level_by_its_step() {
    for (before, after) in [(0u8, 1u8), (1, 2), (2, 3)] {
        let mut target = monster();
        target.tension_level = before;
        let outcome = resolve_action(
            &mut Random::new(9),
            &party(),
            &psyche(1),
            &[target.clone()],
            0,
        )
        .unwrap();
        assert_eq!(outcome.tension_rises, [Some(after)], "level {before}");
        assert_eq!(outcome.targets[0].hp_after, target.hp, "no damage");
        assert_eq!(outcome.targets[0].mp_damage, 0, "no MP taken");
        assert!(!outcome.tension_spent, "kind 15 carries no spend bit");
    }
    let mut target = monster();
    target.tension_level = 0;
    let two = resolve_action(&mut Random::new(9), &party(), &psyche(2), &[target], 0).unwrap();
    assert_eq!(two.tension_rises, [Some(2)], "330's +2 adds two levels");
}

/// The cap: the port holds 0 to 4 (`[status + 0x24]`). A rise at 4 stays, and the step **to** 4
/// draws a die of two (`0x020876a4`–`0x020876d0`, its `NextRandomMax(2)` at `0x020876c4`), only a
/// 0 rising; below 3 the rise draws nothing, so its draw count is the one the action would take
/// without a rise.
#[test]
fn the_tension_rise_caps_at_four_and_only_the_step_to_it_draws() {
    let base = {
        // The same action with a zero rise: the count of the action's own draws.
        let mut target = monster();
        target.tension_level = 3;
        resolve_action(&mut Random::new(11), &party(), &psyche(0), &[target], 0)
            .unwrap()
            .draws
    };
    for level in 0..3u8 {
        let mut target = monster();
        target.tension_level = level;
        let outcome =
            resolve_action(&mut Random::new(11), &party(), &psyche(1), &[target], 0).unwrap();
        assert_eq!(outcome.draws, base, "0 to 3 weakens nothing but the level");
    }
    // The die is the next draw of the pass: read it from the same action without a rise, then
    // assert which way it sent each seed — a 0 rises, anything else stays at 3.
    let mut counted = [0usize; 2];
    for seed in 0..64u64 {
        let mut target = monster();
        target.tension_level = 3;
        let control = resolve_action(
            &mut Random::new(seed),
            &party(),
            &psyche(0),
            &[target.clone()],
            0,
        )
        .unwrap();
        let die = Random::new(control.generator_state).next_max(2);
        let outcome =
            resolve_action(&mut Random::new(seed), &party(), &psyche(1), &[target], 0).unwrap();
        assert_eq!(
            outcome.draws,
            control.draws + 1,
            "the die of two is the next draw"
        );
        let expected = if die == 0 { 4 } else { 3 };
        assert_eq!(
            outcome.tension_rises[0],
            Some(expected),
            "seed {seed}, die {die}"
        );
        counted[usize::from(die != 0)] += 1;
    }
    assert_eq!(counted.iter().sum::<usize>(), 64);
    assert!(counted[0] > 0 && counted[1] > 0, "{counted:?}");

    // At 4 nothing moves, and a second kind-15 after the cap stays at the maximum.
    let mut full = monster();
    full.tension_level = 4;
    let held = resolve_action(&mut Random::new(5), &party(), &psyche(1), &[full], 0).unwrap();
    assert_eq!(held.tension_rises, [Some(4)]);
    assert_eq!(held.draws, base, "no die from the maximum");
}

/// 0x151 and 0x152 set the level their `+0x30` names instead of adding it (`0x021dcec8`,
/// `0x021dcecc`): 3 and 4 on the EU records, above the current level only, with no draw.
#[test]
fn the_two_absolute_kind_fifteen_actions_set_the_level() {
    let mut to_three = psyche(3);
    to_three.number = 0x151;
    let mut to_four = psyche(4);
    to_four.number = 0x152;
    for (action, before, after) in [
        (&to_three, 0u8, 3u8),
        (&to_three, 2, 3),
        (&to_three, 3, 3),
        (&to_three, 4, 4),
        (&to_four, 0, 4),
        (&to_four, 3, 4),
        (&to_four, 4, 4),
    ] {
        let mut target = monster();
        target.tension_level = before;
        let outcome = resolve_action(&mut Random::new(3), &party(), action, &[target], 0).unwrap();
        assert_eq!(
            outcome.tension_rises,
            [Some(after)],
            "{} from {before}",
            action.number
        );
    }
    // The absolute branch draws nothing: the same action whose short cannot move spends the
    // same draws as a zero rise.
    for action in [&to_three, &to_four] {
        let mut target = monster();
        target.tension_level = 1;
        let risen = resolve_action(
            &mut Random::new(3),
            &party(),
            action,
            std::slice::from_ref(&target),
            0,
        )
        .unwrap();
        let control =
            resolve_action(&mut Random::new(3), &party(), &psyche(0), &[target], 0).unwrap();
        assert_eq!(
            risen.draws, control.draws,
            "no draw from the absolute branch"
        );
    }
    assert!(to_three.tension_is_absolute() && to_four.tension_is_absolute());
    assert!(!psyche(1).tension_is_absolute());
}

/// The handler runs on a landing: a missed kind-15 action raises nothing. The rise is the last
/// draw this resolver models, so the die of two is only taken when the action landed.
#[test]
fn a_missed_kind_fifteen_action_raises_nothing() {
    let mut action = psyche(1);
    action.works_on_metal = false;
    let mut metal = monster();
    metal.metal_body = true;
    metal.tension_level = 0;
    let outcome = resolve_action(&mut Random::new(2), &party(), &action, &[metal], 0).unwrap();
    assert!(!outcome.targets[0].landed);
    assert_eq!(outcome.tension_rises, [None]);
    assert_eq!(
        outcome.draws, 2,
        "the kept die and the critical roll; the metal miss costs no accuracy die"
    );
}

/// A refused action never reaches the handler: nothing is drawn and no level moves. MP 255 (all
/// the caster has) and an empty target list are the resolver's own refusals.
#[test]
fn a_rejected_kind_fifteen_action_raises_nothing() {
    let mut costly = psyche(1);
    costly.mp_cost = 255;
    let mut rng = Random::new(1);
    let before = rng.state();
    assert_eq!(
        resolve_action(&mut rng, &party(), &costly, &[monster()], 0),
        Err(Unsupported::MpCost(255))
    );
    assert_eq!(rng.state(), before, "nothing drawn");
    assert_eq!(
        resolve_action(&mut rng, &party(), &psyche(1), &[], 0),
        Err(Unsupported::NoTargets)
    );
    assert_eq!(rng.state(), before);
}

/// The risen level is the one the next tensioned action multiplies by and spends: level 1 gives
/// party ×1.5 plus the flat bonus, and the outcome marks the spend (`0x021e6b9c`, `0x021ed48c`).
#[test]
fn the_risen_level_is_used_and_spent_by_the_next_blow() {
    let target = monster();
    let rise = resolve_action(
        &mut Random::new(4),
        &party(),
        &psyche(1),
        std::slice::from_ref(&target),
        0,
    )
    .unwrap();
    let level = rise.tension_rises[0].expect("a landed rise");
    assert_eq!(level, 1);

    let mut actor = party();
    actor.tension_level = level;
    actor.level = 25;
    let blow = resolve_action(
        &mut Random::new(7),
        &actor,
        &tensioned_action(100),
        std::slice::from_ref(&target),
        0,
    )
    .unwrap();
    assert_eq!(
        blow.targets[0].amount,
        (100.0f32 * 1.5 + flat_bonus(1)) as i32
    );
    assert!(blow.tension_spent);

    // Spent: the same actor at 0 deals the plain amount.
    actor.tension_level = 0;
    let plain = resolve_action(
        &mut Random::new(7),
        &actor,
        &tensioned_action(100),
        std::slice::from_ref(&target),
        0,
    )
    .unwrap();
    assert_eq!(plain.targets[0].amount, 100);
    assert!(!plain.tension_spent);
}

/// A battle with one hero (level 25) and one monster, the hero able to use Psyche Up's rise.
fn psyche_battle(seed: u64) -> Battle {
    let mut f = Fighter::new(Side::Party);
    f.hp = 1000;
    f.max_hp = 1000;
    f.mp = 10;
    f.attack = 100;
    f.defence = 0;
    f.agility = 10;
    f.evasion_rate = 2.0;
    f.level = 25;
    let mut skills = BTreeMap::new();
    skills.insert(1, Skill::from_action(tensioned_action(100)));
    skills.insert(161, Skill::from_action(psyche(1)));
    let setup = BattleSetup {
        monsters: vec![attack_buff_monster()],
        skills,
        set_battle: false,
    };
    Battle::new(setup, vec![PartyMember::new(f)], Surprise::None, seed).unwrap()
}

/// The whole battle path: a round's kind-15 action writes `Status::tension_level` (the kernel's
/// `[status + 0x24]`), the next round's blow multiplies by it and clears it — the same spend the
/// `v4-tension` batch pinned for a hand-set level (`0x021ed48c`).
#[test]
fn a_battle_raises_then_spends_the_hero_s_tension() {
    let mut battle = psyche_battle(0x2468);
    let rise = Command::Act {
        skill: Skill::from_action(psyche(1)),
        target: Target::Party(0),
    };
    battle
        .round(std::slice::from_ref(&rise), &mut Random::new(3))
        .unwrap();
    assert_eq!(
        battle.status(Who::Party(0)).tension_level,
        1,
        "a landed Psyche Up raises by its +1"
    );

    let blow = Command::Act {
        skill: Skill::from_action(tensioned_action(100)),
        target: Target::Monster(0),
    };
    let report = battle.round(&[blow], &mut Random::new(7)).unwrap();
    let amount = report
        .events
        .iter()
        .find_map(|e| match e {
            Event::Acted { who, outcomes, .. } if *who == Who::Party(0) => {
                Some(outcomes[0].1.amount)
            }
            _ => None,
        })
        .expect("the hero acted");
    assert_eq!(amount, (100.0f32 * 1.5 + flat_bonus(1)) as i32);
    assert_eq!(battle.status(Who::Party(0)).tension_level, 0);
}
