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
    let mut tense = party();
    tense.tension_level = 2;
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
