//! The arithmetic of a blow, a spell and a heal, in the game's single precision.
//!
//! Translated from the DQIX decompilation (`src/Combat/Main/BasicAttackCalculation.cpp`,
//! `CritRateCalculation.cpp`, Unlicense) and from the reading of overlay 24 in the wiki
//! `Battle-Resolution` page (facts only); `docs/re/battle.md` lists the addresses. Every
//! operation is an `f32` operation in the order the game performs it: the rounding is part of
//! the answer.
//!
//! port: 0x020744b8, 0x020744d0, 0x020747b4

use super::model::{Action, ActionRange, Amount, Fighter, ScaleStat, Side};
use super::random::FloatDraw;

/// `RoundUp` (`0x020744b8`): `(int)(0.5f + x)`, round half up for positive numbers.
pub fn round_up(x: f32) -> i32 {
    (0.5f32 + x) as i32
}

/// `CalculatePhysicalDamage` (`0x020744d0`): `(attack − defence / 2) / 2` (nothing if not above
/// 0); above `attack / 16` a draw between ∓ a sixteenth of it and a draw between −1 and 1 are
/// added (two draws), else the whole is one draw between 0 and `attack / 16`.
pub fn calculate_physical_damage(attack: i32, defence: i32, rng: &mut impl FloatDraw) -> f32 {
    let mut damage = attack as f32;
    damage -= defence as f32 / 2.0;
    damage /= 2.0;
    if damage <= 0.0 {
        damage = 0.0;
    } else {
        let minimum = attack as f32 / 16.0;
        if damage <= minimum {
            damage = rng.float_between(0.0, attack as f32 / 16.0);
        } else {
            let spread = damage / 16.0;
            let percentage = rng.float_between(0.0 - spread, damage / 16.0);
            let flat = rng.float_between(-1.0, 1.0);
            damage += percentage;
            damage += flat;
        }
    }
    if damage < 0.0 { 0.0 } else { damage }
}

/// `CalculateCritRate` (`0x020747b4`): `(1 / hits) × skill × (2.0 + 0.01 × max(0, deftness − 150)
/// + accessory + book)`. `deftness − 150` goes through a `short` like the game's.
pub fn calculate_crit_rate(
    deftness: i32,
    accessory_bonus: f32,
    book_bonus: f32,
    skill_bonus: f32,
    hit_count: u8,
) -> f32 {
    let minimum_chance = 2.0f32;
    let mut base_chance = deftness.wrapping_sub(150) as i16;
    if base_chance < 0 {
        base_chance = 0;
    }
    let mut base = 0.01f32 * f32::from(base_chance);
    let hits = 1.0f32 / f32::from(hit_count);
    base += minimum_chance;
    let mut chance = accessory_bonus + base;
    chance += book_bonus;
    chance *= skill_bonus;
    hits * chance
}

/// The threshold `RollCritical` (`ov_000::02156cc4`) compares a draw of `NextRandomMax(10000)`
/// against: `(int)(100.0f × rate)`.
pub fn critical_threshold(rate: f32) -> i32 {
    (100.0f32 * rate) as i32
}

/// The critical rate of `actor` for `action`: `CalculateCritRate` for a party member with the
/// action's multiplier over `100.0f`; a monster's is the literal zero of
/// `CalculateMonsterCritRate` (`0x020748f8`).
pub fn critical_rate(actor: &Fighter, action: &Action) -> f32 {
    match actor.side {
        Side::Party => calculate_crit_rate(
            actor.deftness,
            actor.crit_accessory_bonus,
            actor.crit_book_bonus,
            f32::from(action.critical_percent) / 100.0,
            1,
        ),
        Side::Monster => 0.0,
    }
}

/// The base amount of an action against one target (`GetAttackBaseDamage`, `ov_024::021e7bc0`),
/// truncated. Draws: a blow's one or two; a range's one or two.
pub fn base_amount(
    actor: &Fighter,
    target: &Fighter,
    action: &Action,
    rng: &mut impl FloatDraw,
) -> i32 {
    match action.amount {
        Amount::Blow { multiplier } => {
            let attack = round_up(actor.attack as f32 * multiplier);
            calculate_physical_damage(attack, target.defence, rng) as i32
        }
        Amount::Range(range) => range_amount(actor, &range, rng),
    }
}

/// The amount of a range-table action. The spread's draw is added last, and "every arm returns
/// through a truncation" (wiki).
fn range_amount(actor: &Fighter, range: &ActionRange, rng: &mut impl FloatDraw) -> i32 {
    let spread = f32::from(range.spread);
    match (actor.side, range.scaling) {
        (Side::Monster, _) => {
            let base = f32::from(range.monster_amount);
            (base + rng.float_between(-spread, spread)) as i32
        }
        (Side::Party, Some(stat)) => {
            let number = match stat {
                ScaleStat::Might => actor.magical_might,
                ScaleStat::Mending => actor.magical_mending,
            };
            let base = scaled_amount(number, range);
            (base as f32 + rng.float_between(-spread, spread)) as i32
        }
        (Side::Party, None) => {
            // No number to scale by: the amount itself is drawn, then the spread's draw. The
            // truncation point between the two is **[hypothesis]**: one, at the end.
            let drawn = rng.float_between(f32::from(range.party_min), f32::from(range.party_max));
            (drawn + rng.float_between(-spread, spread)) as i32
        }
    }
}

/// `(int)((stat − lo) × ((max − min) / (hi − lo))) + min` between `lo` and `hi`, the least at or
/// under `lo`, the most at or over `hi`.
fn scaled_amount(number: i32, range: &ActionRange) -> i32 {
    let (lo, hi) = (i32::from(range.lo), i32::from(range.hi));
    let (min, max) = (i32::from(range.party_min), i32::from(range.party_max));
    if number <= lo {
        min
    } else if number >= hi {
        max
    } else {
        let slope = (max - min) as f32 / (hi - lo) as f32;
        ((number - lo) as f32 * slope) as i32 + min
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_amount_reaches_both_ends_and_interpolates() {
        // Midheal: 85 at mending 100 and under, 300 from 999.
        let range = ActionRange {
            spread: 0,
            monster_amount: 85,
            party_min: 85,
            party_max: 300,
            lo: 100,
            hi: 999,
            scaling: Some(ScaleStat::Mending),
        };
        assert_eq!(scaled_amount(0, &range), 85);
        assert_eq!(scaled_amount(100, &range), 85);
        assert_eq!(scaled_amount(999, &range), 300);
        assert_eq!(scaled_amount(2000, &range), 300);
        // 85 + 400 × (215 / 899) = 85 + 95.66 → 85 + 95.
        assert_eq!(scaled_amount(500, &range), 180);
        // One under the top: 215 × 898 / 899 = 214.76 → 85 + 214.
        assert_eq!(scaled_amount(998, &range), 299);
    }
}
