//! A battle's opening: monsters and actions built from the ROM's records, onto the kernel's
//! [`Fighter`] and [`Action`].
//!
//! This is the adapter the kernel's docs name (`docs/re/battle.md`, "Setup"): the readers of
//! `dq9-formats` give records, this module maps their fields onto the kernel's inputs and keeps
//! the few numbers the battle's flow needs and the kernel does not (the ways, the way rule, the
//! targeting handlers, the turn class).
//!
//! A monster's HP is drawn here, **from the world's generator**, in the order the monsters stand
//! (group by group, member by member: **[hypothesis]** for the order, the draw itself is
//! `func_02089630`), unless the battle is a set one (the table's HP, no draw).

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::error::Error;
use std::fmt;

use dq9_formats::battle::action::{Action as ActionRecord, Actions, Range, Ranges};
use dq9_formats::battle::monster::{BattleData, BattleMonster, Names};
use nds_rom::Error as FormatError;

use super::model::{
    Action, ActionRange, Amount, Fighter, Reach, ScaleStat, Side, Unsupported, grade_rate,
};
use super::monster::init_monster_hp;
use super::random::Random;

/// The most groups a battle holds (`battle + 0x81b1` bits 4–5).
pub const MAX_GROUPS: usize = 3;
/// The most monsters in one group this model carries (the group record lists eight slots).
pub const MAX_PER_GROUP: usize = 8;

/// What went wrong building a battle from records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupError {
    /// A monster number has no battle record.
    UnknownMonster(u16),
    /// An evasion or block grade past the five-entry table.
    Grade(u16, u8),
    /// More than [`MAX_GROUPS`] groups, or a group of more than [`MAX_PER_GROUP`], or none.
    Shape,
    /// An action number named by a way is in neither action table.
    UnknownAction(u16),
    /// A record failed to parse.
    Format(String),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMonster(n) => write!(f, "monster {n} has no battle record"),
            Self::Grade(n, g) => write!(f, "monster {n}: grade {g} is past the rate table"),
            Self::Shape => write!(
                f,
                "a battle holds one to three groups of one to eight monsters"
            ),
            Self::UnknownAction(n) => write!(f, "action {n} is in neither action table"),
            Self::Format(e) => write!(f, "record: {e}"),
        }
    }
}

impl Error for SetupError {}

impl From<FormatError> for SetupError {
    fn from(e: FormatError) -> Self {
        Self::Format(e.to_string())
    }
}

/// An action as the battle's flow reads it: the kernel's numbers plus what targeting and turn
/// order read (`docs/re/battle.md`, "Round").
#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    /// The action's number.
    pub number: u16,
    /// The kernel's [`Action`], or why it cannot be built (a reach outside 1 to 5).
    pub action: Result<Action, Unsupported>,
    /// `+0x08` bits 8–9: the side it aims at, 1 the opponents, 2 its own side
    /// (`func_ov000_02154a04`, `0x02154a04`; the plain Attack carries 1).
    pub aims: u8,
    /// `+0x14` bits 28–31: whom it reaches, as a nibble (the kernel's [`Reach`] when it is 1 to
    /// 5).
    pub reach: u8,
    /// `+0x0C` and `+0x0E` (u16 each): the targeting handler an AI mode 1 and an AI mode 2
    /// monster use (`func_ov024_021f66cc`); 0 is the default pick.
    pub handlers: [u16; 2],
    /// `+0x2C` bits 14–19: the turn class (`func_ov000_0215f1ac` moves classes 1, 5 and 10 to the
    /// front of the round and class 32 to the back).
    pub turn_class: u8,
    /// `+0x10` bit 19: acts first for a party member whose weapon grants it.
    pub acts_first: bool,
    /// `+0x18` bits 5–11: the kind.
    pub kind: u8,
    /// `+0x1C` bits 14–18: the hit code (`func_ov000_0215fbe0`).
    pub hit_code: u8,
}

impl Skill {
    /// A skill from the kernel's action, for a caller that builds one by hand (a test): aimed at
    /// the opponents unless it heals, default targeting, no turn class.
    pub fn from_action(action: Action) -> Self {
        let reach = match action.reach {
            Reach::Actor => 1,
            Reach::One => 2,
            Reach::All => 3,
            Reach::Group => 4,
            Reach::Attack => 5,
        };
        Self {
            number: action.number,
            aims: if action.kind == 2 { 2 } else { 1 },
            reach,
            handlers: [0, 0],
            turn_class: 0,
            acts_first: false,
            kind: action.kind,
            hit_code: action.shared_critical_bits,
            action: Ok(action),
        }
    }
}

/// Both halves of the action tables with their range tables (`actdt_a`/`actdt_b`,
/// `actdamage_a`/`actdamage_b`).
#[derive(Clone, Debug)]
pub struct ActionTables<'a> {
    a: Actions<'a>,
    b: Actions<'a>,
    ranges_a: Ranges,
    ranges_b: Ranges,
}

impl<'a> ActionTables<'a> {
    /// Parses the four files (`actdt_a_<lang>.nat`, `actdt_b_<lang>.nat`, `actdamage_a.nat`,
    /// `actdamage_b.nat`).
    ///
    /// # Errors
    /// [`SetupError::Format`] when a file is malformed.
    pub fn parse(
        a: &'a [u8],
        b: &'a [u8],
        ranges_a: &[u8],
        ranges_b: &[u8],
    ) -> Result<Self, SetupError> {
        Ok(Self {
            a: Actions::parse(a)?,
            b: Actions::parse(b)?,
            ranges_a: Ranges::parse(ranges_a)?,
            ranges_b: Ranges::parse(ranges_b)?,
        })
    }

    /// The skill numbered `number`, or `None` when neither table has it. A range index is looked
    /// up in the action's own half (the index space is shared, `docs/re/battle-data.md`).
    pub fn skill(&self, number: u16) -> Option<Skill> {
        if let Some(record) = self.a.find(number) {
            return Some(skill(record, &self.ranges_a));
        }
        self.b
            .find(number)
            .map(|record| skill(record, &self.ranges_b))
    }
}

/// The kernel's action for one record, and the extra numbers the flow reads.
fn skill(r: &ActionRecord<'_>, ranges: &Ranges) -> Skill {
    let handlers = [
        u16::from_le_bytes([r.raw[0x0c], r.raw[0x0d]]),
        u16::from_le_bytes([r.raw[0x0e], r.raw[0x0f]]),
    ];
    let hit_code = ((r.word_1c >> 14) & 0x1f) as u8;
    let action = (|| {
        let reach = Reach::from_nibble(r.reach)?;
        let amount = if r.range == 0 {
            Amount::Blow { multiplier: 1.0 }
        } else {
            match ranges.find(r.range) {
                Some(range) => Amount::Range(action_range(r, range)),
                // A range index the half does not list: nothing to draw from.
                None => return Err(Unsupported::Range),
            }
        };
        let mut a = Action::new(r.number, r.kind, amount);
        a.mp_cost = r.mp_cost;
        a.reach = reach;
        a.element = r.element;
        a.landing_element = r.resist_element;
        a.always_critical = r.always_critical;
        a.dodgeable = r.dodgeable;
        a.blockable = r.blockable;
        a.wakes_target = r.wakes;
        a.critical_percent = r.critical_multiplier;
        a.shared_critical_bits = hit_code;
        a.max_damage = r.max_amount;
        a.chains_combo = r.combo;
        a.rider = r.rider;
        a.damage_handler = u16::from(r.damage_handler);
        a.scales_accuracy = r.scales == 1;
        a.accuracy_monster = r.monster_chance;
        a.spoilt = r.spoilt;
        a.halved_by_defending = r.halved_by_defending;
        a.stat_step = r.stat_change;
        a.multi_target_falloff = r.weakens;
        a.weapon_element = r.weapon_element;
        a.trait_accuracy_die = r.party_trait;
        Ok(a)
    })();
    Skill {
        number: r.number,
        action,
        aims: ((r.word_08 >> 8) & 3) as u8,
        reach: r.reach,
        handlers,
        turn_class: ((r.word_2c >> 14) & 0x3f) as u8,
        acts_first: r.word_10 >> 19 & 1 != 0,
        kind: r.kind,
        hit_code,
    }
}

/// An action's range record with the action's scaling fields.
fn action_range(r: &ActionRecord<'_>, range: &Range) -> ActionRange {
    let scaling = match (r.scales, r.scales_magical_might, r.scales_magical_mending) {
        (2, true, _) => Some(ScaleStat::Might),
        (2, false, true) => Some(ScaleStat::Mending),
        _ => None,
    };
    ActionRange {
        spread: range.spread,
        monster_amount: range.monster,
        party_min: range.party_least,
        party_max: range.party_most,
        lo: r.scaled_low,
        hi: r.scaled_high,
        scaling,
    }
}

/// The ROM tables a battle is built from.
#[derive(Clone, Debug)]
pub struct Tables<'a> {
    /// `mon_btldata.nat`.
    pub monsters: BattleData<'a>,
    /// `mon_data_<lang>.nat` (the metal flag).
    pub names: Names<'a>,
    /// The action tables.
    pub actions: ActionTables<'a>,
}

/// A monster standing in a battle.
#[derive(Clone, Debug, PartialEq)]
pub struct Monster {
    /// The number of the monster's record, so that a call for backup can add another of its
    /// kind.
    pub number: u16,
    /// The record's table HP (`mon_btldata`), for a call for backup's own HP draw.
    pub table_hp: u16,
    /// The group it stands in (0 to 2): monsters of a group fight and are targeted as one kind.
    pub group: usize,
    /// The kernel's numbers: HP drawn, MP, attack, defence, agility, evasion and block rates,
    /// resistances, metal body.
    pub fighter: Fighter,
    /// The six ways: action numbers (`+0x18`).
    pub ways: [u16; 6],
    /// `+0x10` bits 5–7: the way rule (`func_0208a91c`).
    pub selector: u8,
    /// `+0x10` bits 3–4: the AI mode (`func_ov000_02159d94`, `0x02159d94`).
    pub ai_mode: u8,
    /// `+0x10` bits 8–10: the extra-actions class (`func_ov000_0215f57c`).
    pub extra_class: u8,
    /// `+0x10` bits 20–25: the ways usable once a battle (`func_0208a03c`).
    pub once_mask: u8,
    /// `+0x10` bit 26: it remembers who struck it last (`func_ov000_02154f30`).
    pub remembers: bool,
    /// Experience when beaten (`+0x08`).
    pub experience: u32,
    /// Gold when beaten (`+0x0C`).
    pub gold: u16,
    /// The ordinary then the rare drop (`+0x04`, `+0x06`).
    pub drops: [u16; 2],
    /// Their chance steps (`+0x02`, `+0x03`).
    pub drop_steps: [u8; 2],
}

/// A battle's monsters and the actions their ways name.
#[derive(Clone, Debug, PartialEq)]
pub struct BattleSetup {
    /// Every monster, group by group.
    pub monsters: Vec<Monster>,
    /// The skill of each way any monster holds.
    pub skills: BTreeMap<u16, Skill>,
    /// A set battle: the monsters keep the table's HP and the party cannot flee
    /// (`docs/re/battle.md`, "Flight").
    pub set_battle: bool,
}

impl BattleSetup {
    /// An encounter: `groups` lists `(monster number, count)`, each entry one group. The HP of
    /// every monster is drawn from `world`.
    ///
    /// # Errors
    /// [`SetupError`] when a record is missing or the shape is out of range.
    pub fn encounter(
        tables: &Tables<'_>,
        groups: &[(u16, u8)],
        world: &mut Random,
    ) -> Result<Self, SetupError> {
        Self::build(tables, groups, Some(world))
    }

    /// A set battle of `groups` (an `eventbattle.bin` record's monster slots): the table's HP, no
    /// draw, no flight.
    ///
    /// # Errors
    /// [`SetupError`] when a record is missing or the shape is out of range.
    pub fn set_battle(tables: &Tables<'_>, groups: &[(u16, u8)]) -> Result<Self, SetupError> {
        Self::build(tables, groups, None)
    }

    fn build(
        tables: &Tables<'_>,
        groups: &[(u16, u8)],
        mut world: Option<&mut Random>,
    ) -> Result<Self, SetupError> {
        let shape_ok = (1..=MAX_GROUPS).contains(&groups.len())
            && groups
                .iter()
                .all(|&(_, n)| (1..=MAX_PER_GROUP).contains(&usize::from(n)));
        if !shape_ok {
            return Err(SetupError::Shape);
        }
        let set_battle = world.is_none();
        let mut monsters = Vec::new();
        let mut skills = BTreeMap::new();
        for (group, &(number, count)) in groups.iter().enumerate() {
            let record = tables
                .monsters
                .find(number)
                .ok_or(SetupError::UnknownMonster(number))?;
            let metal = tables.names.find(number).is_some_and(|n| n.metal);
            // Action 2, the fall-back every way rule ends on, is always needed.
            for &way in record.ways.iter().chain(&[2]) {
                if let Entry::Vacant(slot) = skills.entry(way) {
                    slot.insert(
                        tables
                            .actions
                            .skill(way)
                            .ok_or(SetupError::UnknownAction(way))?,
                    );
                }
            }
            for _ in 0..count {
                let hp = match world.as_deref_mut() {
                    Some(world) => init_monster_hp(world, record.hp, false),
                    None => i32::from(record.hp),
                };
                monsters.push(monster(record, group, metal, hp)?);
            }
        }
        Ok(Self {
            monsters,
            skills,
            set_battle,
        })
    }
}

/// The monster standing for `record` with the HP it comes with.
fn monster(
    record: &BattleMonster<'_>,
    group: usize,
    metal: bool,
    hp: i32,
) -> Result<Monster, SetupError> {
    let rate = |grade: u8| grade_rate(grade).ok_or(SetupError::Grade(record.number, grade));
    let mut fighter = Fighter::new(Side::Monster);
    fighter.hp = hp;
    fighter.max_hp = hp;
    fighter.mp = i32::from(record.mp);
    fighter.attack = i32::from(record.attack);
    fighter.defence = i32::from(record.defence);
    fighter.agility = i32::from(record.agility);
    fighter.evasion_rate = rate(record.evasion_grade())?;
    fighter.block_rate = rate(record.block_grade())?;
    fighter.resistances = record.resistances;
    fighter.metal_body = metal;
    let word = record.word_10;
    Ok(Monster {
        number: record.number,
        table_hp: record.hp,
        group,
        fighter,
        ways: record.ways,
        selector: ((word >> 5) & 7) as u8,
        ai_mode: ((word >> 3) & 3) as u8,
        extra_class: ((word >> 8) & 7) as u8,
        once_mask: ((word >> 20) & 0x3f) as u8,
        remembers: word >> 26 & 1 != 0,
        experience: record.experience,
        gold: record.gold,
        drops: record.drops,
        drop_steps: record.drop_steps,
    })
}
