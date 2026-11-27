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
//!    the amount (`GetAttackBaseDamage`; even for a blow already dodged or blocked), the
//!    several-target falloff of the rank (no draw), the final damage
//!    (`CalculateFinalDamage`, `ov_024::021e6a90`: a critical's draw, a coin) and, for a
//!    blow that deals damage and carries the wake bit, the wake roll; then the kind-15 handler
//!    (`ov_024::021dc93c`) if the action is one, whose step to the maximum draws a die of two.
//!
//! What follows a blow in the game and is **not** modelled: a monster's provocation draws
//! (`func_ov024_021eb08c`), the rider, the counters and everything after the wake roll and the
//! kind-15 rise (`docs/re/battle.md`, "Open points"). The generator state returned by
//! [`resolve_action`] is therefore exact up to those last draws of the final target, not beyond.
//!
//! Not listed as ported: `ResolveAction` and `CalculateFinalDamage` are translated for the plain
//! Attack, the damage and healing spells and the non-damage kinds only, with no status. The two
//! tension helpers they call are, though.
//!
//! port: 0x02074774, 0x02074748

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

/// The tension multipliers by level and dealer kind, read on the EU ARM9 at `0x020e8908`
/// (`GetTensionMultiplier`, `0x02074748`; the USA table is `0x020e88f8`): party
/// `1.0 1.5 2.5 4.0 6.0`, monster `1.0 1.3 2.0 3.0 4.5`. `func_02074738` returns 1.0 for an
/// index past the four levels (`_fls`'s carry test).
const TENSION_MULTIPLIERS: [[f32; 5]; 2] = [[1.0, 1.5, 2.5, 4.0, 6.0], [1.0, 1.3, 2.0, 3.0, 4.5]];

/// The multipliers of the guard level (`0x020e88c0` in USA, `0x020e88d0` in EU; read from the EU
/// ARM9): the party's Defend sets 1, the round's end clears it. Applied to an action carrying
/// `+0x10` bit 4, before the coin.
const GUARD_MULTIPLIERS: [f32; 4] = [1.0, 0.5, 0.1, 0.0];

/// The accuracy a non-scaling action stands at: it lands on every draw.
const STANDING_ACCURACY: i32 = 100;

/// The several-target falloff (`0x020e88e0`, read from the ROM's ARM9): the multiplier of the
/// action's targets by rank, the rank being the target's index in the resolution loop. The index
/// is held to the table's five entries (`func_02074948`, EU `0x02074958`: `cmp`/`movlt 0`/
/// `cmp r0, #4`/`movgt 4`/`ldr r0, [r1, r0, lsl #2]`), so a sixth target and the rest keep 0.2.
const FALLOFF_TABLE: [f32; 5] = [1.0, 0.8, 0.6, 0.4, 0.2];

/// The number of targets whose rank has its own entry: past it the last multiplier stays.
const FALLOFF_RANKS: usize = FALLOFF_TABLE.len();

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
    /// Whether the action spends the actor's tension: it carries `+0x10` bit 13 and the actor
    /// held a level. The original clears the level and both status bits once, after the action's
    /// targets, whatever it came to — a hit, a miss, a dodge, a block, 0 damage, even for an
    /// actor that fell (`ResolveAction`'s tail, `0x021ed48c`–`0x021ed564`, `func_020881ac` and
    /// `func_02088234`). The caller does the clearing: the kernel reads its fighters.
    pub tension_spent: bool,
    /// One entry per target, in the order given: the tension level it holds after a **landed**
    /// kind-15 action (`ov_024::021dc93c`), capped at 4 and gated by a die of two on the step to
    /// the maximum (`0x0208768c`); `None` for every other kind and for a miss, which raises
    /// nothing. The caller writes it to its status: the kernel reads its fighters.
    pub tension_rises: Vec<Option<u8>>,
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

/// `func_02074948` (EU `0x02074958`, table `0x020e88e0`): the rank's multiplier, the rank held
/// to the table's five entries.
fn falloff_factor(rank: usize) -> f32 {
    FALLOFF_TABLE[rank.min(FALLOFF_RANKS - 1)]
}

/// Whether `ResolveAction` applies the rank factor for `actor` and `action`
/// (`0x021ec644`–`0x021ec730`): always for an action carrying `+0x10` bit 17, except the two plain
/// Attack records (1 and 2), whose factor only a party member's weapon arms — the weapon record's
/// `+0x00`, the factor kept when bit 1 is set and otherwise only when bit 0 is set, and never for
/// a monster. The same block multiplies action `0x79` by a further 0.8, which this resolver never
/// reaches (it refuses `0x79`).
fn falloff_applies(actor: &Fighter, action: &Action) -> bool {
    if !action.multi_target_falloff {
        return false;
    }
    if matches!(action.number, 1 | 2) {
        return actor.side == Side::Party && actor.weapon_strikes_several;
    }
    true
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

/// `func_02074738` (`0x02074748`, the table `0x020e8908`): the tension multiplier of `level`
/// for `side`, 1.0 past the four levels (`_fls`'s carry test).
fn tension_multiplier(side: Side, level: u8) -> f32 {
    let arm = if side == Side::Monster { 1 } else { 0 };
    TENSION_MULTIPLIERS[arm]
        .get(usize::from(level))
        .copied()
        .unwrap_or(1.0)
}

/// `CalculateTensionBonus` (`0x02074774`): `(1 + level / 10) * tension`, the division a **whole
/// number's** — dealer levels 10 to 19 all give 2. The level is the dealer's, the tension its
/// `[status + 0x24]` byte. Skipped against a metal body (`0x021e6cf8`).
fn tension_flat_bonus(level: i32, tension: u8) -> f32 {
    (1.0f32 + (level / 10) as f32) * f32::from(tension)
}

/// The maximum tension level (`[status + 0x24]` holds 0 to 4; `func_02088208`, `0x0208768c`,
/// returns −1 from 4 on).
const TENSION_MAX: u8 = 4;

/// One level up, `func_02088208` (EU `0x02088218`): `[status + 0x14] |= 0x800000` then
/// `0x0208768c`, which refuses from the maximum, and from 3 draws `NextRandomMax(2)` and refuses
/// unless it is 0; otherwise it adds one to `[status + 0x24]` and, on reaching 4, sets the
/// maximum's bit through `func_02088150` (`0x02088160`). Returns the level after (unchanged when
/// refused). The step to 4 is gated by `func_0208810c` (EU `0x0208811c`) on status bits the port
/// does not carry (bit 0, `0x10`, `0x80000`, bit 3), so only the die and the cap are modelled.
fn raise_one_level(rng: &mut Counted<'_>, level: u8) -> u8 {
    if level >= TENSION_MAX {
        return level;
    }
    // The step to the maximum (`0x020876a4`–`0x020876d0`): a die of two, and only a 0 rises.
    if level == TENSION_MAX - 1 && rng.next_max(2) >= 1 {
        return level;
    }
    level + 1
}

/// `ov_024::021dc93c` (kind 15), the rise: 0x151 and 0x152 set the level its `+0x30` short names
/// (capped at 4) when that is above the current one, without a draw; every other kind-15 action
/// adds `+0x30` levels, one `raise_one_level` at a time, bounded by `4 − level` (the handler
/// subtracts the overflow: `0x021dcc84`–`0x021dcc90`). A refused step still spends its loop turn.
fn raise_tension(rng: &mut Counted<'_>, action: &Action, level: u8) -> u8 {
    if action.tension_is_absolute() {
        let wanted = action.stat_step.clamp(0, i16::from(TENSION_MAX)) as u8;
        return level.max(wanted);
    }
    let mut level = level;
    let steps = action
        .stat_step
        .max(0)
        .min(i16::from(TENSION_MAX - level.min(TENSION_MAX)));
    for _ in 0..steps {
        level = raise_one_level(rng, level);
    }
    level
}

/// `CalculateFinalDamage` (`ov_024::021e6a90`) for the steps modelled, in the game's order:
/// tension, critical, resistance, the weapon's family bonus, block / dodge zero, the poison
/// needle, the metal body's zeroing, the coin, Metal Slash's `1 + random(2)`, the `+1` weapon,
/// tension again on a metal body, the maximum's half, the combo, truncation, the cap.
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
    // The head of `CalculateFinalDamage` (`0x021e6b9c`–`0x021e6d18`): the action carries
    // `+0x10` bit 13 and the dealer holds tension, so the amount is multiplied by the level's
    // table entry and a flat bonus is added — the bonus skipped against a metal body
    // (`0x021e6cf8`). `pre_tension` keeps the amount as it came for the critical's floor.
    let pre_tension = damage;
    let tensioned = action.tension_applies && actor.tension_level > 0;
    if tensioned {
        damage *= tension_multiplier(actor.side, actor.tension_level);
        if !target.metal_body {
            damage += tension_flat_bonus(actor.level, actor.tension_level);
        }
    }
    // A critical blow that works on a metal body has the head's tension **undone** before the
    // critical and the metal arm's own multiply (`0x021e6d1c`): it takes the multiplier once,
    // not twice. `bVar20` there is the critical flag.
    if critical && target.metal_body && action.works_on_metal {
        damage = pre_tension;
    }
    if critical {
        // The greatest of 1.2 × damage and the critical's own damage; for the plain Attack the
        // damage as it came is a floor too.
        let boosted = 1.2f32 * damage;
        if action.critical_uses_attack_power() {
            let power = actor.attack as f32 * rng.float_between(0.95, 1.05);
            damage = boosted.max(power).max(pre_tension);
        } else {
            // Only the 1.2× carries tension: the drawn factor multiplies the amount as it came.
            let doubled = pre_tension * rng.float_between(1.5, 2.0);
            damage = boosted.max(doubled);
        }
    }
    // The resistance stays a float: a blow of 1 against a half is 0.5, which is not "nothing"
    // for the coin but truncates to 0.
    damage *= target.resistance(action.element);
    // The weapon's killer bonus by the target's family (`0x021e7200`–`0x021e7494` of
    // `final-damage.c`): for a **party** attacker on an action carrying `+0x10` bit 18, each of
    // the twelve families the target answers (`func_0x02156068(battle, target, n, 0)`) multiplies
    // the damage by the worn weapon's signed six-bit field over `10.0f`
    // (`func_02085968` … `func_02085888`, all dividing by the same `0x41200000`). A monster
    // attacker never enters the branch, and a party index never answers a family.
    if actor.side == Side::Party && action.weapon_element && (1..=12).contains(&target.family) {
        damage *= f32::from(actor.family_bonus[usize::from(target.family) - 1]) / 10.0;
    }
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
    // A metal body takes the multiplier a **second** time after its 1-or-2, with no flat bonus
    // (`0x021e79d0`, inside the metal-body arm of `CalculateFinalDamage`).
    if target.metal_body && tensioned {
        damage *= tension_multiplier(actor.side, actor.tension_level);
    }
    // A target at the maximum (level 4, status bit `0x1000000`) has a landed kind-1 amount
    // halved (`0x021e7a58`, `func_ov024_021dd260` on the target), before the combo.
    if target.tension_level == 4 && action.kind == 1 {
        damage *= 0.5;
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
    resolve_action_with_attack(rng, actor, action, targets, combo_count, None)
}

/// [`resolve_action`] with the attack `GetAttackBaseDamage` (`ov_024::021e7bc0`) reads for the
/// blow: `amount_attack` is the buffed word `+0x8` when the action carries `+0x10` bit 9, and
/// `None` reads the actor's own word (the base attack). The critical's own term keeps
/// `actor.attack`: the original reads the base word `+0x134` `+0x34` there whatever the buff
/// (`0x021e6e2c`'s `sVar2 = *(short *)(… + 0x134) + 0x34`).
pub(crate) fn resolve_action_with_attack(
    rng: &mut Random,
    actor: &Fighter,
    action: &Action,
    targets: &[Fighter],
    combo_count: u8,
    amount_attack: Option<i32>,
) -> Result<ActionOutcome, Unsupported> {
    validate(actor, action, targets)?;
    // The spend is decided before any draw: the action carries `+0x10` bit 13 and the actor
    // holds a level. It happens whatever the action comes to (`0x021ed48c`).
    let tension_spent = action.tension_applies && actor.tension_level > 0;
    // One copy per action, never per target; the plain path shares the actor itself.
    let amount_actor = amount_attack
        .filter(|&attack| attack != actor.attack)
        .map(|attack| {
            let mut fighter = actor.clone();
            fighter.attack = attack;
            fighter
        });
    let mut rng = Counted { rng, draws: 0 };
    let shared = action
        .critical_is_shared()
        .then(|| roll_critical(&mut rng, actor, action));
    let mut outcomes = Vec::with_capacity(targets.len());
    let mut tension_rises = Vec::with_capacity(targets.len());
    for (rank, target) in targets.iter().enumerate() {
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
            let base = base_amount(
                amount_actor.as_ref().unwrap_or(actor),
                target,
                action,
                &mut rng,
            );
            // The several-target falloff (`0x021ec6ec`): the base amount as `GetAttackBaseDamage`
            // and the damage handler left it, `_fflt`/`_fmul`/`_ffix`-ed — truncated — before
            // `CalculateFinalDamage`, so before the critical, the resistance, the family bonus,
            // the guard, the coin, the combo and the cap.
            let base = if falloff_applies(actor, action) {
                (base as f32 * falloff_factor(rank)) as i32
            } else {
                base
            };
            amount = final_amount(&mut rng, &strike, base, critical, dodged || blocked, needle);
            if action.wakes_target && action.kind == 1 && amount > 0 {
                // The wake roll (`func_ov000_02157288`, `0x02157340`): drawn even when nothing
                // is asleep.
                rng.next_max(100);
            }
        }
        // The kind-15 handler (`ov_024::021dc93c`) runs once per target, after the amount: a
        // landed action raises the target's level, a miss raises nothing. It is the last draw
        // this resolver models (the die of two on the step to 4).
        let tension_after = (action.kind == 15 && landed)
            .then(|| raise_tension(&mut rng, action, target.tension_level));
        tension_rises.push(tension_after);
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
            // Kinds 0, 3, 4, 5, 12, 15 and 19 change no HP or MP here (kind 15's rise is above).
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
        tension_spent,
        tension_rises,
        draws: rng.draws,
        generator_state: rng.rng.state(),
    })
}
