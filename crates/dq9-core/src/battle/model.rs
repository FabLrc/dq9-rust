//! Plain inputs of the resolver: a fighter's battle numbers and an action's numbers.
//!
//! These structs belong to this module on purpose: the readers of `dq9-formats` (monster battle
//! data, action tables, equipment) are written elsewhere and map **onto** these fields. Each
//! field names the field of the game's data it mirrors (wiki pages `Monsters`, `Actions`,
//! `Battle-Resolution`; `docs/re/battle.md`). Offsets `+0xNN` of an action are those of a record of
//! `actdt_a_<lang>.nat` / `actdt_b_<lang>.nat`, bits counted from 0 at the least significant.
//!
//! Everything the resolver does not model is refused up front with [`Unsupported`], never
//! guessed: see [`validate`].

use std::error::Error;
use std::fmt;

/// The side a fighter fights on. It picks the party's or the monster's arm of every formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// A member of the player's party.
    Party,
    /// A monster.
    Monster,
}

/// Bytes of a fighter's resistance array (`ResetBattleStatus`, `0x020891cc`): 22, elements 1 to
/// 21 being reachable (`GetResistance`, `ov_000::02156b38`).
pub const ELEMENTS: usize = 22;

/// The evasion / block grades of a monster record through the table `0, 2, 4, 8, 25`
/// (USA `0x020e88e4`): the rate a grade stands for.
pub const GRADE_RATES: [f32; 5] = [0.0, 2.0, 4.0, 8.0, 25.0];

/// The rate of a monster's evasion or block grade, `None` for a grade past the table.
pub fn grade_rate(grade: u8) -> Option<f32> {
    GRADE_RATES.get(usize::from(grade)).copied()
}

/// A fighter's battle numbers, as the resolver reads them. All of them are the numbers **at the
/// time of the action**: buffs already applied and `RoundUp`-ed (`docs/re/battle.md`), no
/// status modelled (a status refuses the action upstream or is not represented here).
#[derive(Clone, Debug, PartialEq)]
pub struct Fighter {
    /// Party or monster.
    pub side: Side,
    /// Current HP (the combatant's status).
    pub hp: i32,
    /// Maximum HP. A monster's is its drawn HP ([`init_monster_hp`](super::init_monster_hp)).
    pub max_hp: i32,
    /// Current MP (not read by the resolver; the caller checks the cost).
    pub mp: i32,
    /// Attack power: a monster's `mon_btldata` `+0x60`; a party member's from level, vocation,
    /// equipment.
    pub attack: i32,
    /// Defence: a monster's `mon_btldata` `+0x62`.
    pub defence: i32,
    /// Agility: a monster's `mon_btldata` `+0x64`. Only initiative reads it; the resolver does
    /// not, it is carried so that the next wave maps one record onto one struct.
    pub agility: i32,
    /// Deftness: a party member's character record `[obj + 0x150] + 0x04` bits 0–9. Only the
    /// party's critical rate reads it.
    pub deftness: i32,
    /// Magical might, the number a damage spell scales by (action `+0x10` bit 14).
    pub magical_might: i32,
    /// Magical mending, the number a healing spell scales by (action `+0x10` bit 15).
    pub magical_mending: i32,
    /// What `GetEvasionRate` returns (`ov_000::02156270`): a party member's `2.0` plus what they
    /// wear; a monster's grade through [`GRADE_RATES`]. The roll truncates it.
    pub evasion_rate: f32,
    /// What `GetBlockRate` returns (`ov_000::02156118`): a party member's shield sum and trait
    /// bonus (0 with no shield); a monster's second grade, record bits 16–18 of the word the
    /// grades share, through [`GRADE_RATES`]. The roll does **not** truncate it.
    pub block_rate: f32,
    /// Accessory's addition to the critical rate (`CalculateCritRate`'s `accessoryBonus`).
    pub crit_accessory_bonus: f32,
    /// Book's addition to the critical rate (`bookBonus`).
    pub crit_book_bonus: f32,
    /// Set for a character with trait `0x11d`, whose critical rate may double under a condition
    /// not established: the resolver refuses it ([`Unsupported::CriticalTrait`]).
    pub crit_trait_11d: bool,
    /// What the fighter takes of each element, in hundredths: a monster's record `+0x6C`, a party
    /// member's sum over what they wear (status `+0x3E + element − 1`). Element 8 is the plain
    /// Attack's.
    pub resistances: [u8; ELEMENTS],
    /// Metal body (monster data `+0x0A` bit 12): refused ([`Unsupported::MetalBody`]).
    pub metal_body: bool,
    /// Guard level, 0 to 3 (`[status + 0x138] + 0x21`): refused above 0
    /// ([`Unsupported::Guard`]).
    pub guard_level: u8,
    /// Tension level, 0 to 4 (`[status + 0x24]`): refused above 0 ([`Unsupported::Tension`]).
    pub tension_level: u8,
    /// The element of the weapon a party member wields (0: none; looked up from the weapon's
    /// kind by `CalculateFinalDamage`, `ov_024::021e6a90`). An elemental weapon (1 to 7) on an
    /// action with [`Action::weapon_element`] multiplies the damage again, which is not modelled
    /// ([`Unsupported::WeaponElement`]).
    pub weapon_element: u8,
    /// Set for a character with the trait behind `+0x10` bit 16 (a die of four in the accuracy
    /// roll, which is **[unknown]**): the resolver refuses an action that carries the bit
    /// ([`Unsupported::AccuracyTrait`]).
    pub accuracy_die_trait: bool,
    /// A party member's weapon strikes more than one (two bits at `+0x2F4` of the worn
    /// equipment, USA reading). The plain Attack then rolls its critical once for the whole
    /// action and divides its rate by the hits: refused ([`Unsupported::MultiHitWeapon`]).
    pub weapon_strikes_several: bool,
}

impl Fighter {
    /// A fighter with no stat, no evasion, whole resistances and no status.
    pub fn new(side: Side) -> Self {
        Self {
            side,
            hp: 1,
            max_hp: 1,
            mp: 0,
            attack: 0,
            defence: 0,
            agility: 0,
            deftness: 0,
            magical_might: 0,
            magical_mending: 0,
            evasion_rate: 0.0,
            block_rate: 0.0,
            crit_accessory_bonus: 0.0,
            crit_book_bonus: 0.0,
            crit_trait_11d: false,
            resistances: [100; ELEMENTS],
            metal_body: false,
            guard_level: 0,
            tension_level: 0,
            weapon_element: 0,
            accuracy_die_trait: false,
            weapon_strikes_several: false,
        }
    }

    /// `GetResistance` (`ov_000::02156b38`) with no ward and no status modifier: the byte over
    /// `100.0f`, 1.0 for an element outside 1 to 21.
    pub fn resistance(&self, element: u8) -> f32 {
        match usize::from(element) {
            e @ 1..=21 => f32::from(self.resistances[e - 1]) / 100.0,
            _ => 1.0,
        }
    }
}

/// Whom an action reaches: `actdt` `+0x17` high nibble, equal to `+0x14` bits 28–31.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// 1: the actor (Defend, Psyche Up).
    Actor,
    /// 2: one target (Heal, Frizz, Crack, Zam).
    One,
    /// 3: everyone (Multiheal, Bang, the breaths). The critical is rolled once for the action.
    All,
    /// 4: a group (Crackle, Woosh, Snooze). The critical is rolled once for the action.
    Group,
    /// 5: "the attack's alone" (wiki, not established). Resolved like one target.
    Attack,
}

impl Reach {
    /// The reach of a nibble; 0, 6, 7, 8 and above are not established or not battle reaches.
    pub fn from_nibble(nibble: u8) -> Result<Self, Unsupported> {
        match nibble {
            1 => Ok(Self::Actor),
            2 => Ok(Self::One),
            3 => Ok(Self::All),
            4 => Ok(Self::Group),
            5 => Ok(Self::Attack),
            other => Err(Unsupported::Reach(other)),
        }
    }
}

/// The number an amount scales by (action `+0x10` bits 14 and 15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleStat {
    /// Bit 14: magical might (attacking spells).
    Might,
    /// Bit 15: magical mending (heals).
    Mending,
}

/// An entry of the range table (`actdamage_a.nat` / `actdamage_b.nat`, 8-byte records) with the
/// action's scaling fields: where the amount of a spell, a heal or an item comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionRange {
    /// Range `+0x00` bits 8–17: the spread, a draw between ∓ this is added last.
    pub spread: u16,
    /// Range `+0x04` bits 0–9: a monster's amount.
    pub monster_amount: u16,
    /// Range `+0x04` bits 10–19: a party member's least amount.
    pub party_min: u16,
    /// Range `+0x04` bits 20–29: a party member's most amount.
    pub party_max: u16,
    /// Action `+0x04` bits 12–21: the user's number at which the amount leaves its least.
    pub lo: u16,
    /// Action `+0x04` bits 22–31: the user's number at which the amount reaches its most.
    pub hi: u16,
    /// `Some` when the action scales its amount (`+0x18` bits 16–17 at 2) **by a number it names**
    /// (`+0x10` bit 14 or 15). `None`: a party member's amount is *drawn* between the least and
    /// the most, one more draw.
    pub scaling: Option<ScaleStat>,
}

/// Where an action's damage or healing comes from (`GetAttackBaseDamage`, `0x021e7bc0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Amount {
    /// No range (action `+0x08` bits 14–21 at 0): a blow through `CalculatePhysicalDamage`.
    Blow {
        /// What the attacker's attack power is multiplied by before `RoundUp`: 1.0 on the plain
        /// Attack. **[unknown]** which field of the action holds it; no chapter-2 action but the
        /// plain Attack is a blow.
        multiplier: f32,
    },
    /// A range-table entry: spells, heals, items, monsters' moves.
    Range(ActionRange),
}

/// An action's numbers, as `ResolveAction` and its helpers read them.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// `+0x04` bits 0–11: the action's number (1 is the plain Attack). Actions the code singles
    /// out by number and that are not modelled are refused ([`Unsupported::ActionNumber`]).
    pub number: u16,
    /// `+0x08` low byte: MP cost. 255 (all the caster has) and 128 are refused.
    pub mp_cost: u8,
    /// `+0x17` high nibble.
    pub reach: Reach,
    /// `+0x18` bits 5–11: the kind. 1 deals damage, 2 heals; the others are refused.
    pub kind: u8,
    /// `+0x08` bits 22–26: the element of what it deals; the target's resistance multiplies it.
    pub element: u8,
    /// `+0x18` bits 27–31: the element its landing is resisted by.
    pub landing_element: u8,
    /// `+0x08` bit 29: always a critical (no draw).
    pub always_critical: bool,
    /// `+0x10` bit 5: can be dodged.
    pub dodgeable: bool,
    /// `+0x10` bit 6: can be blocked.
    pub blockable: bool,
    /// `+0x10` bit 11 (`0x800`): a blow that lands and deals damage draws the wake roll after it.
    pub wakes_target: bool,
    /// `+0x14` bits 21–27: the critical chance's multiplier in hundredths (100 on the plain
    /// Attack, 50 on the spells, 0 on items).
    pub critical_percent: u8,
    /// `+0x1C` bits 14–18: where 0 and the reach is All or Group, the critical is rolled once
    /// for the whole action.
    pub shared_critical_bits: u8,
    /// `+0x1C` bits 0–13: the most it can deal; 0 is no limit.
    pub max_damage: u16,
    /// `+0x2C` bit 27: its blows chain into a combo (the counter multiplies damage of at least 1).
    pub chains_combo: bool,
    /// Where the amount comes from.
    pub amount: Amount,
    /// `+0x18` bits 0–4: what rides on its blow. Refused above 0.
    pub rider: u8,
    /// `+0x18` bits 18–26: its damage handler. Refused above 0.
    pub damage_handler: u16,
    /// `+0x18` bits 16–17 at 1: its accuracy scales. Refused (the roll is only modelled at 100).
    pub scales_accuracy: bool,
    /// `+0x10` bit 17: strikes several, weakening as it goes (1.0, 0.8, 0.6, …). The plain Attack
    /// carries it. Against one target the factor is the table's first, 1.0 (**[hypothesis]**);
    /// against several it is refused.
    pub multi_target_falloff: bool,
    /// `+0x10` bit 18: an elemental weapon of a party attacker multiplies the damage by the
    /// target's resistance to its element (the plain Attack carries it). See
    /// [`Fighter::weapon_element`].
    pub weapon_element: bool,
    /// `+0x10` bit 16: a party member with a certain trait throws a die of four in the accuracy
    /// roll (the plain Attack carries it). See [`Fighter::accuracy_die_trait`].
    pub trait_accuracy_die: bool,
}

impl Action {
    /// An action that deals nothing: kind 1, reach one, no flag. Tests and readers start from it.
    pub fn new(number: u16, kind: u8, amount: Amount) -> Self {
        Self {
            number,
            mp_cost: 0,
            reach: Reach::One,
            kind,
            element: 0,
            landing_element: 0,
            always_critical: false,
            dodgeable: false,
            blockable: false,
            wakes_target: false,
            critical_percent: 0,
            shared_critical_bits: 0,
            max_damage: 0,
            chains_combo: false,
            amount,
            rider: 0,
            damage_handler: 0,
            scales_accuracy: false,
            multi_target_falloff: false,
            weapon_element: false,
            trait_accuracy_die: false,
        }
    }

    /// Whether the critical is rolled once for the whole action (reach All or Group with
    /// `+0x1C` bits 14–18 at 0; `ov_024::021ea4d0`) rather than once a target.
    pub fn critical_is_shared(&self) -> bool {
        matches!(self.reach, Reach::All | Reach::Group) && self.shared_critical_bits == 0
    }

    /// Whether a critical takes the attacker's attack power × `between(0.95, 1.05)` rather than
    /// the damage × `between(1.5, 2.0)`: the plain Attack (1) and actions `0xDB`, `0x1F9`.
    pub fn critical_uses_attack_power(&self) -> bool {
        matches!(self.number, 1 | 0xDB | 0x1F9)
    }
}

/// What the resolver refuses to answer: part of the game's behaviour that is not modelled (or
/// not established) yet. The action is **not** resolved and no draw is spent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// No target was given.
    NoTargets,
    /// An action the code singles out by number and whose special rules are not modelled.
    ActionNumber(u16),
    /// A kind other than 1 (damage) and 2 (heal).
    Kind(u8),
    /// A reach that is not 1 to 5.
    Reach(u8),
    /// A damage handler (slots 1 to 66).
    DamageHandler(u16),
    /// A rider effect.
    Rider(u8),
    /// An accuracy that scales.
    AccuracyScaling,
    /// An action that strikes several with the falloff table, at several targets.
    MultiTargetFalloff,
    /// A party attacker's elemental weapon.
    WeaponElement,
    /// The trait behind the accuracy die of four.
    AccuracyTrait,
    /// A party weapon that strikes more than one.
    MultiHitWeapon,
    /// An MP cost of 255 or 128.
    MpCost(u8),
    /// A critical trait whose condition is not established.
    CriticalTrait,
    /// A metal-bodied target.
    MetalBody,
    /// A fighter with tension.
    Tension,
    /// A fighter with a guard level.
    Guard,
    /// A range with a scaling number but `lo >= hi`, or `party_max < party_min`.
    Range,
    /// An AI mode (record `+0x10` bits 3–4) other than 0 and 1: mode 2 chooses at the turn and
    /// spends MP, 3 is not established.
    AiMode(u8),
    /// A way selector (record `+0x10` bits 5–7) whose rule is not modelled (7, the group's
    /// count).
    WaySelector(u8),
    /// A targeting handler (`func_ov024_021f66cc`'s table) that is not modelled.
    TargetHandler(u16),
    /// A hit code (`+0x1C` bits 14–18) that makes an action strike several times or at random
    /// targets.
    HitCode(u8),
    /// A monster that acts more than once a round.
    ExtraActions(u8),
    /// The Defend command (the guard level).
    Defend,
    /// A reach the party's targeting does not cover.
    PartyReach(u8),
    /// A monster that remembers who struck it, against several targets.
    TargetMemory,
    /// A chaining action against several targets (one combo counter per target).
    ComboMultiTarget,
    /// A party action that goes before the others (record `+0x10` bit 19 with a weapon that
    /// grants it).
    TurnOrder,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTargets => write!(f, "no target"),
            Self::ActionNumber(n) => write!(f, "action {n:#x} has rules that are not modelled"),
            Self::Kind(k) => write!(f, "action kind {k} is not modelled"),
            Self::Reach(r) => write!(f, "reach {r} is not a battle reach"),
            Self::DamageHandler(h) => write!(f, "damage handler {h} is not modelled"),
            Self::Rider(r) => write!(f, "rider effect {r} is not modelled"),
            Self::AccuracyScaling => write!(f, "a scaling accuracy is not modelled"),
            Self::MultiTargetFalloff => write!(f, "the multi-target falloff is not modelled"),
            Self::WeaponElement => write!(f, "a weapon's element is not modelled"),
            Self::AccuracyTrait => write!(f, "the accuracy die of four is not modelled"),
            Self::MultiHitWeapon => {
                write!(f, "a weapon that strikes more than one is not modelled")
            }
            Self::MpCost(c) => write!(f, "an MP cost of {c} is not modelled"),
            Self::CriticalTrait => write!(f, "trait 0x11d's critical rate is not established"),
            Self::MetalBody => write!(f, "a metal body is not modelled"),
            Self::Tension => write!(f, "tension is not modelled"),
            Self::Guard => write!(f, "a guard level is not modelled"),
            Self::Range => write!(f, "inconsistent amount range"),
            Self::AiMode(m) => write!(f, "AI mode {m} is not modelled"),
            Self::WaySelector(s) => write!(f, "way selector {s} is not modelled"),
            Self::TargetHandler(h) => write!(f, "targeting handler {h} is not modelled"),
            Self::HitCode(h) => write!(f, "hit code {h} (several hits) is not modelled"),
            Self::ExtraActions(n) => write!(f, "a monster acting {n} extra times is not modelled"),
            Self::Defend => write!(f, "Defend (the guard level) is not modelled"),
            Self::PartyReach(r) => write!(f, "party reach {r} is not modelled"),
            Self::TargetMemory => write!(f, "a monster's memory of its attackers is not modelled"),
            Self::ComboMultiTarget => write!(f, "a combo against several targets is not modelled"),
            Self::TurnOrder => write!(f, "an action that acts first is not modelled"),
        }
    }
}

impl Error for Unsupported {}

/// Actions the code singles out by number whose rules are not modelled: Kamikazee `0x1B`, Metal
/// Slash `0x40`, Thunder Thrust `0x48`, Hatchet Man `0x70`, Metalicker `0x7E`, Double-Edged Slash
/// `0xAF`, `0x79` (four fifths), Flee `0xE1` and `0x395`, the four that draw before any target
/// (`0x1FF`, `0x200`, `0x20B`, `0x20C`) and `0x82`, `0x205` (metal bodies).
const SPECIAL_ACTIONS: [u16; 15] = [
    0x1B, 0x40, 0x48, 0x70, 0x7E, 0xAF, 0x79, 0xE1, 0x395, 0x1FF, 0x200, 0x20B, 0x20C, 0x82, 0x205,
];

/// Checks everything the resolver cannot answer, before any draw.
pub fn validate(actor: &Fighter, action: &Action, targets: &[Fighter]) -> Result<(), Unsupported> {
    if targets.is_empty() {
        return Err(Unsupported::NoTargets);
    }
    if SPECIAL_ACTIONS.contains(&action.number) {
        return Err(Unsupported::ActionNumber(action.number));
    }
    if !matches!(action.kind, 1 | 2) {
        return Err(Unsupported::Kind(action.kind));
    }
    if action.damage_handler != 0 {
        return Err(Unsupported::DamageHandler(action.damage_handler));
    }
    if action.rider != 0 {
        return Err(Unsupported::Rider(action.rider));
    }
    if action.scales_accuracy {
        return Err(Unsupported::AccuracyScaling);
    }
    if action.multi_target_falloff && targets.len() > 1 {
        return Err(Unsupported::MultiTargetFalloff);
    }
    if action.weapon_element && actor.side == Side::Party && (1..=7).contains(&actor.weapon_element)
    {
        return Err(Unsupported::WeaponElement);
    }
    if action.trait_accuracy_die && actor.side == Side::Party && actor.accuracy_die_trait {
        return Err(Unsupported::AccuracyTrait);
    }
    if action.number == 1 && actor.side == Side::Party && actor.weapon_strikes_several {
        return Err(Unsupported::MultiHitWeapon);
    }
    if matches!(action.mp_cost, 128 | 255) {
        return Err(Unsupported::MpCost(action.mp_cost));
    }
    if let Amount::Range(range) = action.amount {
        let inverted =
            range.party_max < range.party_min || (range.scaling.is_some() && range.hi <= range.lo);
        if inverted {
            return Err(Unsupported::Range);
        }
    }
    if actor.crit_trait_11d {
        return Err(Unsupported::CriticalTrait);
    }
    for fighter in std::iter::once(actor).chain(targets) {
        if fighter.tension_level > 0 {
            return Err(Unsupported::Tension);
        }
        if fighter.guard_level > 0 {
            return Err(Unsupported::Guard);
        }
    }
    if targets.iter().any(|t| t.metal_body) {
        return Err(Unsupported::MetalBody);
    }
    Ok(())
}
