//! The item spend rule read on the EU ROM: an item is spent only when its use changed
//! something. `docs/re/battle.md`, "Which uses spend the item".
//!
//! The fixtures are synthetic outcomes. The original's kind handlers raise the applied counter
//! of `ResolveAction` only for a heal that restored HP, a blow of at least 1, a status actually
//! cured or a fallen target revived; `effect_applied` reads the same states from the outcome
//! (HP or MP changed). The cured-status case has no total in the outcome, but no used-up item
//! carries a status-only kind, so the predicate is exact for the item path.

use dq9_core::battle::{
    Action, ActionRange, Amount, Fighter, Random, Side, TargetOutcome, item::effect_applied,
    resolve_action,
};

/// One landing target with `(HP before, HP after, MP taken)`.
fn targets(rows: &[(i32, i32, i32)]) -> Vec<TargetOutcome> {
    rows.iter()
        .map(|&(before, after, mp_damage)| TargetOutcome {
            landed: true,
            dodged: false,
            blocked: false,
            critical: false,
            amount: (after - before).abs(),
            mp_damage,
            fled: false,
            hp_before: before,
            hp_after: after,
        })
        .collect()
}

#[test]
fn a_heal_at_full_hp_does_nothing_and_keeps_the_item() {
    assert!(!effect_applied(&targets(&[(30, 30, 0)])));
}

#[test]
fn a_heal_on_a_wounded_ally_spends_the_item() {
    assert!(effect_applied(&targets(&[(5, 30, 0)])));
}

#[test]
fn a_revive_on_a_living_ally_does_nothing_and_keeps_the_item() {
    assert!(!effect_applied(&targets(&[(20, 20, 0)])));
}

#[test]
fn a_revive_on_a_fallen_ally_spends_the_item() {
    assert!(effect_applied(&targets(&[(0, 20, 0)])));
}

#[test]
fn a_cure_with_nothing_to_cure_does_nothing_and_keeps_the_item() {
    assert!(!effect_applied(&targets(&[(40, 40, 0)])));
}

#[test]
fn a_use_spends_when_one_of_several_targets_changed() {
    assert!(effect_applied(&targets(&[(30, 30, 0), (5, 30, 0)])));
    assert!(!effect_applied(&targets(&[(30, 30, 0), (40, 40, 0)])));
}

#[test]
fn a_blow_that_deals_nothing_does_not_count_and_a_blow_that_lands_does() {
    assert!(!effect_applied(&targets(&[(10, 10, 0)])));
    assert!(effect_applied(&targets(&[(10, 4, 0)])));
}

#[test]
fn mp_damage_counts_as_a_change() {
    assert!(effect_applied(&targets(&[(10, 10, 3)])));
}

#[test]
fn a_resolved_heal_at_full_hp_changes_no_hp_and_is_not_applied() {
    let action = Action::new(
        255,
        2,
        Amount::Range(ActionRange {
            spread: 0,
            monster_amount: 35,
            party_min: 35,
            party_max: 35,
            lo: 0,
            hi: 0,
            scaling: None,
        }),
    );
    let mut target = Fighter::new(Side::Party);
    target.hp = 30;
    target.max_hp = 30;
    let mut rng = Random::new(7);
    let outcome = resolve_action(
        &mut rng,
        &Fighter::new(Side::Party),
        &action,
        std::slice::from_ref(&target),
        0,
    )
    .unwrap();
    assert_eq!(outcome.targets[0].hp_after, 30);
    assert!(!effect_applied(&outcome.targets));
}
