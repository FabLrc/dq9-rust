//! Resolution of one action against its targets, in the original order of draws.
//!
//! `ResolveAction` (`ov_024::021eb5d0`) loops over the targets itself. Its draws, in this order
//! (wiki `Battle-Resolution`, "The order of draws for one action"; checked against
//! BattleEmulator, `docs/re/battle.md`):
//!
//! 1. once for the whole action, **before any target**: the critical roll, if the action rolls it
//!    once (reach All or Group, [`Action::critical_is_shared`]);
//! 2. per target: `NextRandomMax(100)` (the die, never read here), the critical roll if it was
//!    not rolled once, the evasion roll if the action can be dodged, the block roll if it can be
//!    blocked **and the blow was not dodged**, the accuracy roll (always), then, if it landed,
//!    the amount (`GetAttackBaseDamage`; even for a blow already dodged or blocked), the final
//!    damage (`CalculateFinalDamage`, `ov_024::021e6a90`: a critical's draw, a coin) and, for a
//!    blow that deals damage and carries the wake bit, the wake roll.
//!
//! What follows a blow in the game and is **not** modelled: a monster's provocation draws
//! (`func_ov024_021eb08c`), the rider, the counters and everything after the wake roll
//! (`docs/re/battle.md`, "Open points"). The generator state returned by [`resolve_action`] is
//! therefore exact up to the wake roll of the last target, not beyond.
//!
//! Not listed as ported: `ResolveAction` and `CalculateFinalDamage` are translated for the plain
//! Attack and the damage and healing spells only, with no status.

use super::damage::{base_amount, critical_rate, critical_threshold};
use super::model::{Action, Fighter, Side, Unsupported, validate};
use super::random::{FloatDraw, Random};

/// Metal Slash `0x40` and Metalicker `0x7E`: `1 + random(2)` against a metal body when the blow
/// is not a critical (`ov_024::021e6a90`).
const METAL_SLASHERS: [u16; 2] = [0x40, 0x7E];

/// The actions whose damage a metal body does not zero: Quadraslash `0x205` and Needle Shot
/// `0x82` (`ov_024::021e6a90`). A critical is exempt too.
const METAL_ZEROING_EXEMPT: [u16; 2] = [0x205, 0x82];

/// The multipliers of the combo table (`0x021fe778`), indexed by the counter held to 3.
const COMBO_TABLE: [f32; 4] = [1.0, 1.2, 1.5, 2.0];

/// The multipliers of the guard level (`0x020e88c0`): the party's Defend sets 1, the round's end
/// clears it. Applied to an action carrying `+0x10` bit 4, before the coin.
const GUARD_MULTIPLIERS: [f32; 4] = [1.0, 0.5, 0.1, 0.0];

/// The accuracy a non-scaling action stands at: it lands on every draw.
const STANDING_ACCURACY: i32 = 100;

/// What happened to one target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetOutcome {
    /// The accuracy roll succeeded.
    pub landed: bool,
    /// The target dodged (evasion roll).
    pub dodged: bool,
    /// The target blocked (block roll).
    pub blocked: bool,
    /// The blow or spell was a critical.
    pub critical: bool,
    /// Damage dealt (kind 1), HP restored (kind 2) or MP taken (kind 57), after the cap. 0 for a
    /// dodged or blocked blow, and for every kind that changes no number.
    pub amount: i32,
    /// MP taken (kind 57), held under what the target had.
    pub mp_damage: i32,
    /// The monster left the battle (action `0xE1`, `func_ov024_021da670`'s removal).
    pub fled: bool,
    /// The target's HP before.
    pub hp_before: i32,
    /// The target's HP after: damage floored at 0, healing held to the maximum.
    pub hp_after: i32,
}

/// The result of one action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionOutcome {
    /// One entry per target, in the order given.
    pub targets: Vec<TargetOutcome>,
    /// MP the action costs its actor.
    pub mp_spent: i32,
    /// Draws taken from the generator by this call.
    pub draws: u32,
    /// The generator's state after, for chaining the next action.
    pub generator_state: u64,
}

/// A generator that counts the draws it hands out.
struct Counted<'a> {
    rng: &'a mut Random,
    draws: u32,
}

impl Counted<'_> {
    fn next_max(&mut self, max: i32) -> i32 {
        self.draws += 1;
        self.rng.next_max(max)
    }
}

impl FloatDraw for Counted<'_> {
    fn float_between(&mut self, min: f32, max: f32) -> f32 {
        self.draws += 1;
        self.rng.next_float_between(min, max)
    }
}

/// Who acts on whom, and the battle's combo counter at that moment.
struct Strike<'a> {
    actor: &'a Fighter,
    target: &'a Fighter,
    action: &'a Action,
    combo_count: u8,
}

/// `RollCritical` (`ov_000::02156cc4`): always-critical actions return 1 without a draw, others
/// `NextRandomMax(10000) < (int)(100.0f × rate)`; a monster's rate is zero but the draw is spent.
fn roll_critical(rng: &mut Counted<'_>, actor: &Fighter, action: &Action) -> bool {
    if action.always_critical {
        return true;
    }
    let threshold = critical_threshold(critical_rate(actor, action));
    rng.next_max(10_000) < threshold
}

/// `RollEvasion` (`ov_000::02156f98`): `NextRandomMax(100) < (int)rate`, the rate truncated.
fn roll_evasion(rng: &mut Counted<'_>, target: &Fighter) -> bool {
    rng.next_max(100) < target.evasion_rate as i32
}

/// `RollBlock` (`ov_000::02156e30`): `(float)NextRandomMax(100) < rate`, the rate not truncated.
fn roll_block(rng: &mut Counted<'_>, target: &Fighter) -> bool {
    (rng.next_max(100) as f32) < target.block_rate
}

/// `RollAccuracy` (`ov_000::02156648`): a metal body is missed outright by an action that does not
/// work on one (`+0x10` bit 24) — before the die, so the miss costs the action's own accuracy
/// draw; then `NextRandomMax(100)` whatever the accuracy; then, for a needle blow (`+0x10` bit 18
/// on an action carrying it, the attacker on the party, his weapon's `func_020851d8` bit),
/// an unconditional hit **after that die but before the scaling accuracy and the dazzle's die of
/// eight** (`0x021567fc`–`0x02156830`); then a scaling accuracy (a monster's `+0x14` bits 0–6
/// chance × the target's resistance to the action's landing element + 0.5, truncated); then, for
/// an action spoilt by a status (`+0x10` bit 3) on a dazzled attacker, a die of eight that misses
/// on 0 to 4; it lands under the accuracy.
fn roll_accuracy(
    rng: &mut Counted<'_>,
    actor: &Fighter,
    action: &Action,
    target: &Fighter,
) -> bool {
    if target.metal_body && !action.works_on_metal {
        return false;
    }
    let die = rng.next_max(100);
    // The poison needle's auto-hit (`0x02156814`–`0x02156830`): `+0x10` bit 18 tested on the
    // action, `func_020851d8` on the attacker's weapon record, then `return 1` — the die of 100
    // was spent, the scaling accuracy and the spoilt die of eight are skipped.
    if action.weapon_element && actor.side == Side::Party && actor.poison_needle {
        return true;
    }
    let accuracy = if action.scales_accuracy {
        let scaled = f32::from(action.accuracy_monster) * target.resistance(action.landing_element);
        (0.5f32 + scaled) as i32
    } else {
        STANDING_ACCURACY
    };
    if action.spoilt && actor.dazzled && rng.next_max(8) < 5 {
        return false;
    }
    die < accuracy
}

/// `CalculateFinalDamage` (`ov_024::021e6a90`) for the steps modelled, in the game's order:
/// critical, resistance, block / dodge zero, the poison needle, the metal body's zeroing, the
/// coin, Metal Slash's `1 + random(2)`, the `+1` weapon, the combo, truncation, the cap.
fn final_amount(
    rng: &mut Counted<'_>,
    strike: &Strike<'_>,
    base: i32,
    critical: bool,
    nullified: bool,
    needle: bool,
) -> i32 {
    let Strike {
        actor,
        target,
        action,
        combo_count,
    } = *strike;
    let mut damage = base as f32;
    if critical {
        // The greatest of 1.2 × damage and the critical's own damage; for the plain Attack the
        // damage as it came is a floor too.
        let boosted = 1.2f32 * damage;
        if action.critical_uses_attack_power() {
            let power = actor.attack as f32 * rng.float_between(0.95, 1.05);
            damage = boosted.max(power).max(damage);
        } else {
            let doubled = damage * rng.float_between(1.5, 2.0);
            damage = boosted.max(doubled);
        }
    }
    // The resistance stays a float: a blow of 1 against a half is 0.5, which is not "nothing"
    // for the coin but truncates to 0.
    damage *= target.resistance(action.element);
    if nullified {
        damage = 0.0;
    }
    // The guard level (`0x021e77a4`'s neighbour, table `0x020e88c0`): an exact half for Defend,
    // before the coin, whoever struck.
    if action.halved_by_defending && target.guard_level > 0 {
        damage *= GUARD_MULTIPLIERS[usize::from(target.guard_level.min(3))];
    }
    // The poison needle (`func_020851d8`, the worn weapon's `+0x2F4` bits 10–11): a party blow
    // carrying `+0x10` bit 18 deals exactly 1; the dodge and the block were cleared before the
    // roll's outcome, so this step only overwrites the amount.
    if needle {
        damage = 1.0;
    }
    // The metal body (`0x021e77a4`, read through `func_ov000_02156068(battle, target, 0, 1)`): an
    // action that aims at the opponents and works on metal has its damage set to exactly 0 unless
    // the blow is a critical or the action is `0x205` / `0x82`. The coin below then makes it 0 or
    // 1 — the site's "the others deal 0 or 1".
    if target.metal_body
        && action.aims_at_opponents
        && action.works_on_metal
        && !critical
        && !needle
        && !METAL_ZEROING_EXEMPT.contains(&action.number)
    {
        damage = 0.0;
    }
    let takes_both =
        target.resistance(action.element) > 0.0 && target.resistance(action.landing_element) > 0.0;
    if damage <= 0.0 && !nullified && takes_both {
        damage = rng.next_max(2) as f32;
    }
    // Metal Slash and Metalicker on a metal body, not critical: `1.0f + NextRandomMax(2)`, after
    // the coin, so they draw again on top of it.
    if target.metal_body && !critical && METAL_SLASHERS.contains(&action.number) {
        damage = 1.0 + rng.next_max(2) as f32;
    }
    // The "+1 against metal" weapon (`func_02085128`) adds 1.0 to a metal body's damage, on a
    // party blow carrying `+0x10` bit 18.
    if target.metal_body && actor.side == Side::Party && action.weapon_element && actor.metal_bonus
    {
        damage += 1.0;
    }
    if action.chains_combo && damage >= 1.0 {
        damage *= COMBO_TABLE[usize::from(combo_count.min(3))];
    }
    let mut amount = damage as i32;
    if action.max_damage != 0 {
        amount = amount.min(i32::from(action.max_damage));
    }
    amount
}

/// Resolves one `action` of `actor` against `targets`, drawing from the battle's generator
/// `rng` in the original order. `combo_count` is the battle's combo counter byte
/// (`[battle + 0x8e83]`, 0 to 3) at the time of the action; stepping it is the caller's job.
///
/// The actor and the targets are read, not changed: the HP each would end with is in the
/// outcome.
///
/// # Errors
///
/// [`Unsupported`] when the action or a fighter uses a rule that is not modelled; nothing is
/// drawn then.
pub fn resolve_action(
    rng: &mut Random,
    actor: &Fighter,
    action: &Action,
    targets: &[Fighter],
    combo_count: u8,
) -> Result<ActionOutcome, Unsupported> {
    validate(actor, action, targets)?;
    let mut rng = Counted { rng, draws: 0 };
    let shared = action
        .critical_is_shared()
        .then(|| roll_critical(&mut rng, actor, action));
    let mut outcomes = Vec::with_capacity(targets.len());
    for target in targets {
        // The die kept at `[battle + 0x8e6e]`; only a status not modelled reads it.
        rng.next_max(100);
        let critical = match shared {
            Some(critical) => critical,
            None => roll_critical(&mut rng, actor, action),
        };
        let mut dodged = action.dodgeable && roll_evasion(&mut rng, target);
        let mut blocked = action.blockable && !dodged && roll_block(&mut rng, target);
        // The poison needle's blow cannot be dodged or blocked: `CalculateFinalDamage` clears the
        // staging flags (`0x021e6e40`), so the hit is reported as a normal one.
        let needle = actor.side == Side::Party && action.weapon_element && actor.poison_needle;
        if needle {
            dodged = false;
            blocked = false;
        }
        let landed = roll_accuracy(&mut rng, actor, action, target);
        let mut amount = 0;
        if landed {
            let strike = Strike {
                actor,
                target,
                action,
                combo_count,
            };
            // Every kind computes its amount in the game: the kind handler is handed it and
            // ignores it where the kind changes no number (`docs/re/battle.md`).
            let base = base_amount(actor, target, action, &mut rng);
            amount = final_amount(&mut rng, &strike, base, critical, dodged || blocked, needle);
            if action.wakes_target && action.kind == 1 && amount > 0 {
                // The wake roll (`func_ov000_02157288`, `0x02157340`): drawn even when nothing
                // is asleep.
                rng.next_max(100);
            }
        }
        let fell = landed && action.number == 0xE1;
        let (hp_after, mp_damage) = match action.kind {
            1 => ((target.hp - amount).max(0), 0),
            2 => {
                // The heal handler (`ov_024::021db3a4`) replaces the incoming amount for
                // Fullheal and Omniheal after the normal base/final calculations and draws.
                if matches!(action.number, 0x21 | 0x310) {
                    amount = target.max_hp;
                }
                // `ov_000::0215a16c` applies the amount under the target's maximum HP.
                (target.hp.max((target.hp + amount).min(target.max_hp)), 0)
            }
            // Weird Dance (`func_ov024_021e0bf4`): the amount is taken from MP, held under what
            // the target has.
            57 => (target.hp, amount.max(0).min(target.mp.max(0))),
            // Kinds 0, 4, 5, 12 and 19 change no HP or MP here.
            _ => (target.hp, 0),
        };
        outcomes.push(TargetOutcome {
            landed,
            dodged,
            blocked,
            critical,
            amount,
            mp_damage,
            fled: fell,
            hp_before: target.hp,
            hp_after: if fell { 0 } else { hp_after },
        });
    }
    Ok(ActionOutcome {
        targets: outcomes,
        mp_spent: i32::from(action.mp_cost),
        draws: rng.draws,
        generator_state: rng.rng.state(),
    })
}
