//! The party: ordered members, gold and the bag, as a save restores them and as a new game
//! creates the hero. Pure data; the level tables that fill the computed values are supplied
//! by the caller ([`Party::apply_level_tables`]). Field origins are in `docs/re/party.md`.
//!
//! A member is the persistent character record (`save::Character`, 572 bytes) plus the
//! values the game keeps beside it: the eight held items of its party slot and the maximum
//! HP/MP the record's stat block carries. The record stores, per character:
//!
//! - name at `+0x140` (10 glyph indices, zero-terminated) and sex at `+0x174` bit 0;
//! - current vocation at `+0x50`, levels `+0x02` and experience `+0x1C`, one per vocation;
//! - the skill-point pool at `+0xF4`;
//! - equipment around `+0x160` (ten `s16` slots: torso, legwear, face, hair, arms, footwear,
//!   headgear, weapon, shield, accessory);
//! - fourteen 10-bit stat fields at `+0x14C`: fields 0 to 6 are the base attributes
//!   (strength, resilience, agility, deftness, charm, magical mending, magical might),
//!   fields 7 and 8 the maximum HP and MP, the remaining five are not established. Fields 7
//!   and 8 are what `Save_UnpackMain` (`0x020a95b4`) and `func_02082d7c` write into the
//!   party member's HP and MP (`melonds::copies::member_fields`, `+0x1c`/`+0x1e`): the game
//!   restores a character at full HP and MP.
//!
//! A new game's hero is the record character creation writes (`ov_009` `0x0218636c` and the
//! `u16` list at `0x0218ab78`): the Minstrel's vocation (6), level 1 in every vocation, no
//! experience, the copper sword and the opening clothes (items 20004, 13007, 16215 and
//! 17120). The values the level table computes are left empty until
//! [`Party::apply_level_tables`] runs.

use dq9_formats::battle::equipment::BattleParams;
use dq9_formats::hero::Sex;
use dq9_formats::item::ItemStats;
use dq9_formats::level::{LevelStat, LevelTables};
use dq9_formats::save::{
    Block, Character as SaveCharacter, EquipSlot, NAME_LEN, NO_ITEM, Slot, VOCATION_COUNT,
};

use crate::battle::model::{Fighter, Side};

/// Characters a party holds (`cmp r0, #4`, `ov_003::021621a4` and its neighbours).
pub const PARTY_MAX: usize = 4;
/// Held items a party member carries (`7578 + 18 i`, 8 × u16).
pub const HELD_ITEMS: usize = 8;
/// Bag slots the game keeps (`GameState_Party+0`, capacity 152 in the oracle states).
pub const BAG_CAPACITY: usize = 152;
/// Gold the party may hold (`func_0207d2c4`'s literal `0x98967f`, and
/// `func_ov023_021f5150`'s cap when a battle's gold is added).
pub const GOLD_MAX: u32 = 9_999_999;
/// Skill points a character may hold (`iRam021f1224` at `0xA28`, the cap
/// `func_ov023_021f0a5c` applies when a level grants more than the room left).
pub const SKILL_POINTS_MAX: u16 = 2600;
/// Items in one bag slot (`func_0207c378` clamps a stack to `0x63`).
pub const BAG_STACK: u8 = 99;
/// The `k` bands of `expadj.nat`, as `func_ov023_021f5578` reads them: the most experience a
/// band holds and its `k`. The file is not in the batch's ownership; these three bands are
/// the ones recorded in `docs/re/party.md`.
const EXP_BANDS: [(u32, u32); 3] = [(10_000, 4), (20_000, 3), (u32::MAX, 2)];
/// The elements the twenty resistance bytes of `itembtlprm.nat` belong to, in their order
/// (`func_02083e28`; 8 and 22 are absent).
const EQUIPMENT_ELEMENTS: [u8; 20] = [
    1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21,
];

/// The bag inside the `Main` block: 12 header bytes, then the ids (`u16`), then the
/// quantities (`u8`). The two leading words are runtime pointers to the two arrays
/// (`bag+0xc` and `bag+0x13c` in the oracle states); the `u16` at `+8` is the capacity.
const BAG_OFFSET: usize = 0x1d5c;
const BAG_BLOCK_SIZE: usize = 0x1d4;
const BAG_HEADER: usize = 12;

/// Vocation of a new game's hero: the Minstrel (level table `level6`, confirmed by the guide;
/// `str_tm` 2106).
pub const NOVICE_VOCATION: u8 = 6;

/// Clothes character creation equips (`ov_009` `0x0218636c`, list `0x0218ab78`; the oracle
/// states' hero wears them, `pro1.ml1`/`pro11.ml1`).
pub const NEW_GAME_ARMOUR: u16 = 13007;
pub const NEW_GAME_LEGWEAR: u16 = 16215;
pub const NEW_GAME_FOOTWEAR: u16 = 17120;

/// Base attributes of a character at its current vocation and level: the level table's
/// columns 1 to 7, before seeds and equipment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BaseStats {
    pub strength: u16,
    pub resilience: u16,
    pub agility: u16,
    pub deftness: u16,
    pub charm: u16,
    pub magical_might: u16,
    pub magical_mending: u16,
}

impl BaseStats {
    /// The attributes a level table record gives.
    pub fn from_level(level: &LevelStat) -> Self {
        Self {
            strength: level.strength,
            resilience: level.resilience,
            agility: level.agility,
            deftness: level.deftness,
            charm: level.charm,
            magical_might: level.magical_might,
            magical_mending: level.magical_mending,
        }
    }
}

/// What a character wears, by item id; `None` is an empty slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Equipment {
    pub torso: Option<u16>,
    pub legs: Option<u16>,
    pub arms: Option<u16>,
    pub feet: Option<u16>,
    pub head: Option<u16>,
    pub weapon: Option<u16>,
    pub shield: Option<u16>,
    pub accessory: Option<u16>,
}

impl Equipment {
    /// The item in `slot`.
    pub fn get(&self, slot: EquipSlot) -> Option<u16> {
        match slot {
            EquipSlot::Torso => self.torso,
            EquipSlot::Legs => self.legs,
            EquipSlot::Arms => self.arms,
            EquipSlot::Feet => self.feet,
            EquipSlot::Head => self.head,
            EquipSlot::Weapon => self.weapon,
            EquipSlot::Shield => self.shield,
            EquipSlot::Accessory => self.accessory,
        }
    }

    pub fn set(&mut self, slot: EquipSlot, item: Option<u16>) {
        let field = match slot {
            EquipSlot::Torso => &mut self.torso,
            EquipSlot::Legs => &mut self.legs,
            EquipSlot::Arms => &mut self.arms,
            EquipSlot::Feet => &mut self.feet,
            EquipSlot::Head => &mut self.head,
            EquipSlot::Weapon => &mut self.weapon,
            EquipSlot::Shield => &mut self.shield,
            EquipSlot::Accessory => &mut self.accessory,
        };
        *field = item;
    }
}

/// One item word of the bag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemStack {
    /// Item id; `None` for an empty slot.
    pub item: Option<u16>,
    pub quantity: u8,
}

/// The bag: `GameState_Party+0`, `Main+0x1d5c` in the save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bag {
    slots: Vec<ItemStack>,
}

impl Bag {
    /// `capacity` empty slots.
    pub fn empty(capacity: usize) -> Self {
        Self {
            slots: vec![ItemStack::default(); capacity],
        }
    }

    /// Parses the bag from the `Main` block (`Slot::block(Block::Main)`).
    fn from_main(main: &[u8]) -> Result<Self, PartyError> {
        let block = main
            .get(BAG_OFFSET..BAG_OFFSET + BAG_BLOCK_SIZE)
            .ok_or(PartyError::BadBag)?;
        let capacity = usize::from(u16::from_le_bytes([block[8], block[9]]));
        if BAG_HEADER + 3 * capacity > BAG_BLOCK_SIZE {
            return Err(PartyError::BadBag);
        }
        let ids = BAG_HEADER;
        let quantities = BAG_HEADER + 2 * capacity;
        let slots = (0..capacity)
            .map(|i| {
                let at = ids + 2 * i;
                let id = u16::from_le_bytes([block[at], block[at + 1]]);
                ItemStack {
                    item: worn(id),
                    quantity: block[quantities + i],
                }
            })
            .collect();
        Ok(Self { slots })
    }

    pub fn slots(&self) -> &[ItemStack] {
        &self.slots
    }

    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Whether the bag holds nothing.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.item.is_none())
    }

    /// Copies of `item` in the bag (`Bag_Count`, `0x0207c7a0`).
    pub fn count(&self, item: u16) -> u32 {
        self.slots
            .iter()
            .filter(|s| s.item == Some(item))
            .map(|s| u32::from(s.quantity))
            .sum()
    }

    /// Adds one copy of `item` (`func_0207c378`): the first stack of it, else the first empty
    /// slot. A stack is held to 99. `false` when the bag is full, the copy then being lost.
    pub fn add(&mut self, item: u16) -> bool {
        self.add_copies(item, 1)
    }

    /// Adds `copies` of `item`; same rule as [`Bag::add`]. The game adds them to one stack
    /// and clamps that stack to 99 rather than starting a second one.
    pub fn add_copies(&mut self, item: u16, copies: u8) -> bool {
        if worn(item).is_none() || copies == 0 {
            return false;
        }
        if let Some(stack) = self.slots.iter_mut().find(|s| s.item == Some(item)) {
            stack.quantity = stack.quantity.saturating_add(copies).min(BAG_STACK);
            return true;
        }
        if let Some(stack) = self.slots.iter_mut().find(|s| s.item.is_none()) {
            stack.item = Some(item);
            stack.quantity = copies.min(BAG_STACK);
            return true;
        }
        false
    }
}

/// One party member: the character record plus the party slot's held items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    /// Glyph indices of the name (`+0x140`), zero-terminated.
    pub name: [u8; NAME_LEN],
    pub sex: Sex,
    /// Current vocation (`+0x50`): 0 Guardian, 1 Warrior … 6 Minstrel … 12 Ranger.
    pub vocation: u8,
    /// Level in every vocation (`+0x02`).
    pub levels: [u8; VOCATION_COUNT],
    /// Experience in every vocation (`+0x1C`).
    pub experience: [u32; VOCATION_COUNT],
    /// Current HP and MP, restored at their maximum (see the module note).
    pub hp: u16,
    pub max_hp: u16,
    pub mp: u16,
    pub max_mp: u16,
    /// Base attributes at the current vocation and level, before seeds and equipment;
    /// [`Character::apply_level_tables`] fills them.
    pub stats: BaseStats,
    pub equipment: Equipment,
    /// Skill points not yet spent (`+0xF4`).
    pub skill_points: u16,
    /// The eight held items of the party slot (`7578 + 18 i`), [`NO_ITEM`] or 0 empty.
    pub held_items: [Option<u16>; HELD_ITEMS],
}

impl Character {
    /// Converts a save record and the held items of its party slot.
    pub fn from_record(record: &SaveCharacter<'_>, held: [u16; HELD_ITEMS]) -> Self {
        let bytes = record.bytes();
        let max_hp = stat_field(bytes, 7);
        let max_mp = stat_field(bytes, 8);
        let mut equipment = Equipment::default();
        for slot in EquipSlot::ALL {
            equipment.set(slot, worn(record.equipment(slot)));
        }
        Self {
            name: core::array::from_fn(|i| record.name_raw()[i]),
            sex: if record.gender() & 1 == 1 {
                Sex::Woman
            } else {
                Sex::Man
            },
            vocation: record.vocation(),
            levels: core::array::from_fn(|v| record.level(v).unwrap_or(0)),
            experience: core::array::from_fn(|v| record.experience(v).unwrap_or(0)),
            hp: max_hp,
            max_hp,
            mp: max_mp,
            max_mp,
            stats: BaseStats::default(),
            equipment,
            skill_points: record.unallocated_skill_points(),
            held_items: held.map(worn),
        }
    }

    /// The hero character creation makes: Minstrel, level 1 everywhere, no experience, the
    /// copper sword and the opening clothes. The vocation is the one `pro1.ml1`'s member
    /// object holds (`+0x950`, and `level6`'s level-1 row matches its stats); the thirteen
    /// levels at 1 and experiences at 0 are the creation initialiser's loop (`0x02086450`);
    /// the items are `ov_009` `0x0218636c` / `0x0218ab78`. The name is empty and the sex a
    /// man: **[hypothesis]**, the name is typed on the creation screen and its default look
    /// was not read. Base attributes and HP/MP are left empty: the player types the name, and
    /// the values come from the level table ([`Character::apply_level_tables`]).
    pub fn hero() -> Self {
        let equipment = Equipment {
            torso: Some(NEW_GAME_ARMOUR),
            legs: Some(NEW_GAME_LEGWEAR),
            feet: Some(NEW_GAME_FOOTWEAR),
            weapon: Some(crate::state::NEW_GAME_WEAPON),
            ..Equipment::default()
        };
        Self {
            name: [0; NAME_LEN],
            sex: Sex::Man,
            vocation: NOVICE_VOCATION,
            levels: [1; VOCATION_COUNT],
            experience: [0; VOCATION_COUNT],
            hp: 0,
            max_hp: 0,
            mp: 0,
            max_mp: 0,
            stats: BaseStats::default(),
            equipment,
            skill_points: 0,
            held_items: [None; HELD_ITEMS],
        }
    }

    /// The name up to its first zero byte.
    pub fn name(&self) -> &[u8] {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        &self.name[..end]
    }

    /// Level in `vocation` (0 outside the thirteen).
    pub fn level_in(&self, vocation: u8) -> u8 {
        self.levels.get(usize::from(vocation)).copied().unwrap_or(0)
    }

    /// Level in the current vocation.
    pub fn level(&self) -> u8 {
        self.level_in(self.vocation)
    }

    /// Experience in `vocation`.
    pub fn experience_in(&self, vocation: u8) -> u32 {
        self.experience
            .get(usize::from(vocation))
            .copied()
            .unwrap_or(0)
    }

    /// Experience in the current vocation.
    pub fn current_experience(&self) -> u32 {
        self.experience_in(self.vocation)
    }

    /// Fills the values the game computes from the level table: the base attributes at the
    /// current vocation and level, and, for a character that has none yet (a new game's
    /// hero), the maximum and current HP and MP. A character restored from a save keeps the
    /// record's maxima, which include seeds and equipment, as the load itself writes them.
    pub fn apply_level_tables(&mut self, tables: &LevelTables) {
        let Some(stat) = tables.stat(self.vocation, self.level()) else {
            return;
        };
        self.stats = BaseStats::from_level(stat);
        if self.max_hp == 0 {
            self.max_hp = stat.max_hp;
        }
        if self.max_mp == 0 {
            self.max_mp = stat.max_mp;
        }
        if self.hp == 0 {
            self.hp = self.max_hp;
        }
        if self.mp == 0 {
            self.mp = self.max_mp;
        }
    }

    /// Sets the current HP the battle ends with, held to the maximum.
    pub fn set_hp(&mut self, hp: u16) {
        self.hp = hp.min(self.max_hp);
    }

    /// Sets the current MP the battle ends with, held to the maximum.
    pub fn set_mp(&mut self, mp: u16) {
        self.mp = mp.min(self.max_mp);
    }

    /// The battle numbers of this member: the level table's attributes plus what is worn,
    /// through [`ItemStats`] and `itembtlprm.nat` ([`BattleParams`]). The caller has applied
    /// [`Character::apply_level_tables`] (a restored save leaves `stats` empty otherwise).
    ///
    /// The sums are the ones `func_02083e28` makes over the eleven worn places: item `+0x08`
    /// bits 0–9/10–19 add to attack/defence, `+0x10` bits 0–9/10–19/20–29 to
    /// deftness/agility/magical might. Evasion is `GetEvasionRate`'s `2.0` plus the item
    /// `+0x0c` bits 10–19 over `10.0` (`func_02084f68`); block the `+0x0c` bits 0–9 over
    /// `10.0` (`func_02084ef8`, nothing with no shield); the critical bonus the `+0x0c` bits
    /// 20–29 over `10.0` (`func_02084e78`). Resistances start at 100 and add the twenty
    /// signed bytes of each worn `itembtlprm` record on elements 1–7 and 9–21, held at
    /// nothing below (`func_02083e28`). Fields the game keeps in skill panels or traits but
    /// this model does not hold stay at the kernel's neutral value.
    pub fn fighter(&self, items: &ItemStats, equipment: &BattleParams) -> Fighter {
        let mut fighter = Fighter::new(Side::Party);
        fighter.hp = i32::from(self.hp);
        fighter.max_hp = i32::from(self.max_hp);
        fighter.mp = i32::from(self.mp);
        let mut attack = i32::from(self.stats.strength);
        let mut defence = i32::from(self.stats.resilience);
        let mut agility = i32::from(self.stats.agility);
        let mut deftness = i32::from(self.stats.deftness);
        let mut magical_might = i32::from(self.stats.magical_might);
        let (mut block, mut evasion, mut critical) = (0u32, 0u32, 0u32);
        for slot in EquipSlot::ALL {
            let Some(id) = self.equipment.get(slot) else {
                continue;
            };
            if let Some(stats) = items.get(id) {
                attack += i32::from(stats.attack);
                defence += i32::from(stats.defence);
                deftness += i32::from(stats.deftness);
                agility += i32::from(stats.agility);
                magical_might += i32::from(stats.magical_might);
                block += u32::from(stats.block);
                evasion += u32::from(stats.evasion);
                critical += u32::from(stats.critical);
            }
            if let Some(param) = equipment.find(id as i16) {
                for (i, &element) in EQUIPMENT_ELEMENTS.iter().enumerate() {
                    let byte = &mut fighter.resistances[usize::from(element) - 1];
                    let value = i16::from(*byte) + i16::from(param.resistances[i]);
                    *byte = value.clamp(0, 255) as u8;
                }
                if slot == EquipSlot::Weapon {
                    // 8 is the plain Attack's element, i.e. no elemental weapon.
                    let element = param.weapon_element();
                    fighter.weapon_element = if element == 8 { 0 } else { element };
                }
            }
        }
        fighter.attack = attack;
        fighter.defence = defence;
        fighter.agility = agility;
        fighter.deftness = deftness;
        fighter.magical_might = magical_might;
        // Magical mending has no established equipment field in an item stats entry: the
        // level table's value is the whole of it here.
        fighter.magical_mending = i32::from(self.stats.magical_mending);
        fighter.evasion_rate = 2.0 + evasion as f32 / 10.0;
        // `GetBlockRate` returns zero without a shield before summing anything.
        fighter.block_rate = if self.equipment.shield.is_some() {
            block as f32 / 10.0
        } else {
            0.0
        };
        fighter.crit_accessory_bonus = critical as f32 / 10.0;
        // The book's critical bonus comes from skill panels, not from the equipment records
        // read here; the trait fields (0x11d, the accuracy die, a multi-strike weapon) are
        // not established.
        fighter.crit_book_bonus = 0.0;
        fighter.crit_trait_11d = false;
        fighter.accuracy_die_trait = false;
        fighter.weapon_strikes_several = false;
        fighter
    }

    /// Adds `gain` experience to the current vocation and takes every level its column 0
    /// reaches (`func_02082064` accepts a row whose `experience` is not above the total).
    /// `None` when no level was gained; the experience is added either way.
    fn gain_experience(&mut self, gain: u32, tables: &LevelTables) -> Option<LevelUp> {
        let vocation = usize::from(self.vocation);
        let table = tables.table(self.vocation)?;
        let experience = self.experience.get_mut(vocation)?;
        *experience = experience.saturating_add(gain);
        let total = *experience;
        let old_level = self.level();
        let old_row = table.level(old_level)?;
        let mut new_level = old_level;
        while new_level < 99 {
            let next = table.level(new_level + 1)?;
            if total < next.experience {
                break;
            }
            new_level += 1;
        }
        if new_level == old_level {
            return None;
        }
        let new_row = table.level(new_level)?;
        let skill_delta = new_row.skill_points.saturating_sub(old_row.skill_points);
        let skill_gain = skill_delta.min(SKILL_POINTS_MAX.saturating_sub(self.skill_points));
        self.skill_points += skill_gain;
        self.stats = BaseStats::from_level(new_row);
        self.max_hp = self
            .max_hp
            .saturating_add(new_row.max_hp.saturating_sub(old_row.max_hp));
        self.max_mp = self
            .max_mp
            .saturating_add(new_row.max_mp.saturating_sub(old_row.max_mp));
        self.levels[vocation] = new_level;
        Some(LevelUp {
            member: 0,
            level: new_level,
            strength: new_row.strength.saturating_sub(old_row.strength),
            resilience: new_row.resilience.saturating_sub(old_row.resilience),
            agility: new_row.agility.saturating_sub(old_row.agility),
            deftness: new_row.deftness.saturating_sub(old_row.deftness),
            charm: new_row.charm.saturating_sub(old_row.charm),
            magical_might: new_row.magical_might.saturating_sub(old_row.magical_might),
            magical_mending: new_row
                .magical_mending
                .saturating_sub(old_row.magical_mending),
            max_hp: new_row.max_hp.saturating_sub(old_row.max_hp),
            max_mp: new_row.max_mp.saturating_sub(old_row.max_mp),
            skill_points: skill_gain,
        })
    }
}

/// One member's level-up, as the battle's result window shows it: the new level and the
/// increases it prints (`func_ov023_021f0a5c` applies the same values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUp {
    /// Index in [`Party::members`].
    pub member: usize,
    /// Level reached in the current vocation.
    pub level: u8,
    pub strength: u16,
    pub resilience: u16,
    pub agility: u16,
    pub deftness: u16,
    pub charm: u16,
    pub magical_might: u16,
    pub magical_mending: u16,
    pub max_hp: u16,
    pub max_mp: u16,
    /// Skill points the levels handed out, after the pool's cap.
    pub skill_points: u16,
}

/// A stat field of the record's `+0x14C` block: three 10-bit fields per word.
fn stat_field(bytes: &[u8], field: usize) -> u16 {
    let at = 0x14c + 4 * (field / 3);
    let word = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    ((word >> (10 * (field % 3))) & 0x3ff) as u16
}

/// An equipment or held-item word: [`NO_ITEM`] and 0 are empty slots.
fn worn(id: u16) -> Option<u16> {
    (id != NO_ITEM && id != 0).then_some(id)
}

/// The party a save restores: members in order (the leader first), gold and bag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Party {
    pub members: Vec<Character>,
    /// Gold on hand (`Main+0x2c30`, `GameState+0x3970`).
    pub gold: u32,
    pub bag: Bag,
}

impl Party {
    /// A new game: the hero alone, no gold, an empty bag.
    pub fn new_game() -> Self {
        Self {
            members: vec![Character::hero()],
            gold: 0,
            bag: Bag::empty(BAG_CAPACITY),
        }
    }

    /// What `Save_UnpackMain` (`0x020a95b4`) restores: party slot `i` is record
    /// `standby_count + i` (the hero first) with held items `i`.
    pub fn from_slot(slot: &Slot<'_>) -> Result<Self, PartyError> {
        let count = usize::from(slot.party_count());
        if count > PARTY_MAX {
            return Err(PartyError::TooManyMembers);
        }
        let standby = usize::from(slot.standby_count());
        let mut members = Vec::with_capacity(count);
        for i in 0..count {
            let index = standby.checked_add(i).ok_or(PartyError::MissingCharacter)?;
            let record = slot.character(index).ok_or(PartyError::MissingCharacter)?;
            let held = slot.held_items(i).ok_or(PartyError::MissingCharacter)?;
            members.push(Character::from_record(&record, held));
        }
        Ok(Self {
            members,
            gold: slot.gold_on_hand(),
            bag: Bag::from_main(slot.block(Block::Main))?,
        })
    }

    /// The leader: the first member, who the world talks to (`0x0200fddc`).
    pub fn leader(&self) -> Option<&Character> {
        self.members.first()
    }

    /// Applies the level tables to every member.
    pub fn apply_level_tables(&mut self, tables: &LevelTables) {
        for member in &mut self.members {
            member.apply_level_tables(tables);
        }
    }

    /// Applies a won battle's experience, gold and drops, and levels the members up.
    ///
    /// Experience is shared the way `func_ov023_021f4098` does it: a member who is down
    /// (`func_02061bd8`; this model's `hp == 0`) receives and weighs nothing; for the rest
    ///
    /// ```text
    /// share = ⌈ total × (k + level) ÷ Σ (k + level) ⌉
    /// ```
    ///
    /// with `k` the band of `expadj.nat` the total falls in (4 up to 10,000, 3 up to 20,000,
    /// 2 beyond; `func_ov023_021f5534`). The battle's `rounds` factor cancels in the share,
    /// so it is not needed. The game's `× 1.05` for an experience-bonus item is not modelled:
    /// the award carries no equipment table. The share is added to the current vocation's
    /// experience (`func_ov023_021f5150`), never to the other vocations.
    ///
    /// A level is reached when the row's column 0 (`experience`) is not above the new total
    /// (`func_ov023_021f0a5c`, `func_02082064`): every level gained at once is taken. The
    /// attributes become the new row's, the maxima grow by the row's difference (seeds and
    /// equipment then still count), HP and MP are **not** healed (the game's level-up writes
    /// the maxima, not the current values), and the skill-point pool receives column 10's
    /// difference, held to 2,600 (`iRam021f1224`).
    ///
    /// Gold is added whole and held to [`GOLD_MAX`] (`func_ov023_021f5150`); each drop takes
    /// [`Bag::add`]'s rule. [`Character::apply_level_tables`] must have run before a level-up
    /// is awarded.
    pub fn award(
        &mut self,
        experience: u32,
        gold: u32,
        drops: &[u16],
        tables: &LevelTables,
    ) -> Vec<LevelUp> {
        self.gold = self.gold.saturating_add(gold).min(GOLD_MAX);
        for &item in drops {
            self.bag.add(item);
        }
        let k = EXP_BANDS
            .iter()
            .find(|(bound, _)| experience <= *bound)
            .map_or(2, |(_, k)| u64::from(*k));
        let recipients: Vec<usize> = self
            .members
            .iter()
            .enumerate()
            .filter(|(_, member)| member.hp > 0)
            .map(|(i, _)| i)
            .collect();
        let weights: Vec<u64> = recipients
            .iter()
            .map(|&i| k + u64::from(self.members[i].level()))
            .collect();
        let sum: u64 = weights.iter().sum::<u64>().max(1);
        let mut level_ups = Vec::new();
        for (&index, &weight) in recipients.iter().zip(&weights) {
            let share = (u64::from(experience) * weight).div_ceil(sum) as u32;
            if let Some(level_up) = self.members[index].gain_experience(share, tables) {
                level_ups.push(LevelUp {
                    member: index,
                    level: self.members[index].level(),
                    ..level_up
                });
            }
        }
        level_ups
    }
}

/// Why a party could not be read from a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartyError {
    /// The party count says more than four members.
    TooManyMembers,
    /// A member's record (or its held items) is past the block.
    MissingCharacter,
    /// The bag's capacity does not fit its block.
    BadBag,
}

impl core::fmt::Display for PartyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::TooManyMembers => "the save holds more than four party members",
            Self::MissingCharacter => "the save's party points past its character records",
            Self::BadBag => "the save's bag does not fit its block",
        })
    }
}

impl std::error::Error for PartyError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::save::{FILE_SIZE, Save};

    fn blank_save() -> Vec<u8> {
        vec![0; FILE_SIZE]
    }

    /// A slot with two party members (records 1 and 2), held items, gold and a small bag.
    fn party_save() -> Save {
        fn record(bytes: &mut [u8], n: usize, name: [u8; NAME_LEN], vocation: u8) {
            let at = 136 + n * 572;
            bytes[at + 0x140..at + 0x140 + NAME_LEN].copy_from_slice(&name);
            bytes[at + 2 + 6] = 7; // level 7 in the Minstrel
            bytes[at + 28 + 4 * 6..at + 32 + 4 * 6].copy_from_slice(&42u32.to_le_bytes());
            bytes[at + 80] = vocation;
            bytes[at + 244..at + 246].copy_from_slice(&9u16.to_le_bytes());
            bytes[at + 352..at + 354].copy_from_slice(&13007u16.to_le_bytes());
            bytes[at + 366..at + 368].copy_from_slice(&20004u16.to_le_bytes());
            bytes[at + 372] = 1; // a woman
            // +0x14C: word 2 fields 7 and 8 are the maximum HP and MP.
            let word = (30u32 << 10) | (12u32 << 20);
            bytes[at + 0x154..at + 0x158].copy_from_slice(&word.to_le_bytes());
        }

        let mut bytes = blank_save();
        bytes[7572] = 1; // standby
        bytes[7577] = 2; // party count
        for (i, id) in [22159u16, NO_ITEM].iter().enumerate() {
            let at = 7578 + 2 * i;
            bytes[at..at + 2].copy_from_slice(&id.to_le_bytes());
        }
        bytes[11448..11452].copy_from_slice(&1234u32.to_le_bytes());
        record(&mut bytes, 1, [0x12, 0x2f, 0, 0, 0, 0, 0, 0, 0, 0], 6);
        record(&mut bytes, 2, [0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0], 1);

        let bag = 136 + BAG_OFFSET;
        bytes[bag + 8..bag + 10].copy_from_slice(&3u16.to_le_bytes());
        for (i, id) in [22159u16, 22000, NO_ITEM].iter().enumerate() {
            let at = bag + BAG_HEADER + 2 * i;
            bytes[at..at + 2].copy_from_slice(&id.to_le_bytes());
        }
        for (i, n) in [2u8, 1, 0].iter().enumerate() {
            bytes[bag + BAG_HEADER + 6 + i] = *n;
        }
        Save::parse(&bytes).unwrap()
    }

    #[test]
    fn new_game_is_the_hero_alone_with_the_creation_equipment() {
        let party = Party::new_game();
        assert_eq!(party.members.len(), 1);
        let hero = party.leader().unwrap();
        assert_eq!(hero.vocation, NOVICE_VOCATION);
        assert_eq!(hero.levels, [1; VOCATION_COUNT]);
        assert_eq!(hero.experience, [0; VOCATION_COUNT]);
        assert_eq!(hero.sex, Sex::Man);
        assert_eq!(hero.name(), []);
        assert_eq!(hero.equipment.weapon, Some(crate::state::NEW_GAME_WEAPON));
        assert_eq!(hero.equipment.torso, Some(NEW_GAME_ARMOUR));
        assert_eq!(hero.equipment.legs, Some(NEW_GAME_LEGWEAR));
        assert_eq!(hero.equipment.feet, Some(NEW_GAME_FOOTWEAR));
        assert_eq!(hero.equipment.head, None);
        assert_eq!(party.gold, 0);
        assert_eq!(party.bag.capacity(), BAG_CAPACITY);
        assert!(party.bag.is_empty());
    }

    #[test]
    fn a_slot_restores_its_members_in_order_with_held_items_and_bag() {
        let save = party_save();
        let party = Party::from_slot(&save.slot(0).unwrap()).unwrap();
        assert_eq!(party.members.len(), 2);
        let hero = &party.members[0];
        assert_eq!(hero.name(), [0x12, 0x2f]);
        assert_eq!(hero.vocation, 6);
        assert_eq!(hero.levels[6], 7);
        assert_eq!(hero.experience_in(6), 42);
        assert_eq!(hero.skill_points, 9);
        assert_eq!(hero.sex, Sex::Woman);
        assert_eq!(hero.equipment.torso, Some(13007));
        assert_eq!(hero.equipment.weapon, Some(20004));
        assert_eq!(hero.equipment.head, None);
        assert_eq!(hero.held_items[0], Some(22159));
        assert_eq!(hero.held_items[1], None);
        assert_eq!(hero.max_hp, 30);
        assert_eq!(hero.max_mp, 12);
        assert_eq!((hero.hp, hero.mp), (30, 12));
        assert_eq!(party.members[1].vocation, 1);
        assert_eq!(party.gold, 1234);
        assert_eq!(party.bag.capacity(), 3);
        assert_eq!(party.bag.count(22159), 2);
        assert_eq!(party.bag.count(22000), 1);
        assert_eq!(party.bag.count(20004), 0);
        assert_eq!(party.bag.slots()[2], ItemStack::default());
    }

    #[test]
    fn more_than_four_members_is_rejected() {
        let mut bytes = blank_save();
        bytes[7577] = 5;
        let save = Save::parse(&bytes).unwrap();
        assert_eq!(
            Party::from_slot(&save.slot(0).unwrap()),
            Err(PartyError::TooManyMembers)
        );
    }

    #[test]
    fn a_member_past_the_character_records_is_rejected() {
        let mut bytes = blank_save();
        bytes[7572] = 13;
        bytes[7577] = 1;
        let save = Save::parse(&bytes).unwrap();
        assert_eq!(
            Party::from_slot(&save.slot(0).unwrap()),
            Err(PartyError::MissingCharacter)
        );
    }

    #[test]
    fn a_bag_capacity_larger_than_its_block_is_rejected() {
        let mut bytes = blank_save();
        bytes[136 + BAG_OFFSET + 8..136 + BAG_OFFSET + 10].copy_from_slice(&u16::MAX.to_le_bytes());
        let save = Save::parse(&bytes).unwrap();
        assert_eq!(
            Party::from_slot(&save.slot(0).unwrap()),
            Err(PartyError::BadBag)
        );
    }

    #[test]
    fn level_tables_fill_the_hero_attributes_and_health() {
        let files: [Vec<u8>; VOCATION_COUNT] = core::array::from_fn(|_| level_file());
        let files: [&[u8]; VOCATION_COUNT] = files.each_ref().map(Vec::as_slice);
        let tables = LevelTables::parse(files).unwrap();
        let mut party = Party::new_game();
        party.apply_level_tables(&tables);
        let hero = &party.members[0];
        assert_eq!(
            hero.stats,
            BaseStats {
                strength: 40,
                resilience: 41,
                agility: 42,
                deftness: 43,
                charm: 44,
                magical_might: 45,
                magical_mending: 46,
            }
        );
        assert_eq!((hero.max_hp, hero.max_mp), (47, 48));
        assert_eq!((hero.hp, hero.mp), (47, 48));
    }

    /// A synthetic equipment member: `(id, stats index, attack, rates, attributes)`.
    fn equipment_member(entries: &[(u16, u16, u32, u32, u32)]) -> Vec<u8> {
        const RECORD: usize = 0x20;
        let count = entries.len();
        let records = 12 + count * RECORD;
        let mut data = vec![0u8; records + count * RECORD];
        data[..2].copy_from_slice(&(count as u16).to_le_bytes());
        for (i, &(id, stats_index, attack, rates, attributes)) in entries.iter().enumerate() {
            let at = 12 + i * RECORD;
            data[at..at + 4].copy_from_slice(&u32::from(stats_index).to_le_bytes());
            data[at + 0x18..at + 0x1a].copy_from_slice(&id.to_le_bytes());
            let stats_at = records + usize::from(stats_index) * RECORD;
            data[stats_at + 0x08..stats_at + 0x0c].copy_from_slice(&attack.to_le_bytes());
            data[stats_at + 0x0c..stats_at + 0x10].copy_from_slice(&rates.to_le_bytes());
            data[stats_at + 0x10..stats_at + 0x14].copy_from_slice(&attributes.to_le_bytes());
        }
        data
    }

    /// `itembtlprm.nat` with the opening equipment: copper sword, celestial costume and a
    /// shield whose first resistance byte is −20 (element 1).
    fn battle_params() -> BattleParams {
        let mut records = Vec::new();
        for (id, element, resistance) in [(20004u16, 0u32, 0i8), (13007, 0, 0), (21000, 0, -20)] {
            let mut record = [0u8; 44];
            record[..4].copy_from_slice(&(element << 23).to_le_bytes());
            record[0x14] = resistance as u8;
            record[0x28..0x2a].copy_from_slice(&(id as i16).to_le_bytes());
            records.extend(record);
        }
        let mut data = (3u32).to_le_bytes().to_vec();
        data.extend(records);
        BattleParams::parse(&data).unwrap()
    }

    fn equipment_stats() -> ItemStats {
        // Copper sword 7 attack; celestial costume 2 defence; the shield 5 block.
        let data = equipment_member(&[
            (20004, 0, 7, 0, 0),
            (13007, 1, 2 << 10, 0, 0),
            (21000, 2, 0, 5, 0),
        ]);
        ItemStats::from_members(&[&data]).unwrap()
    }

    #[test]
    fn a_hero_fighter_sums_the_worn_weapon_and_armour() {
        let mut hero = Character::hero();
        hero.stats = BaseStats {
            strength: 9,
            resilience: 8,
            agility: 8,
            deftness: 12,
            charm: 9,
            magical_might: 6,
            magical_mending: 7,
        };
        hero.hp = 20;
        hero.max_hp = 20;
        hero.mp = 6;
        hero.max_mp = 6;
        hero.equipment.shield = Some(21000);
        let fighter = hero.fighter(&equipment_stats(), &battle_params());
        assert_eq!(fighter.side, Side::Party);
        assert_eq!((fighter.hp, fighter.max_hp, fighter.mp), (20, 20, 6));
        assert_eq!(fighter.attack, 9 + 7);
        assert_eq!(fighter.defence, 8 + 2);
        assert_eq!((fighter.agility, fighter.deftness), (8, 12));
        assert_eq!((fighter.magical_might, fighter.magical_mending), (6, 7));
        // The shield blocks 5 tenths of a hundredth; evasion starts at GetEvasionRate's 2.0.
        assert_eq!(fighter.block_rate, 0.5);
        assert_eq!(fighter.evasion_rate, 2.0);
        // The shield's first resistance byte is element 1; element 8 stays 100.
        assert_eq!(fighter.resistances[0], 80);
        assert_eq!(fighter.resistances[7], 100);
        assert_eq!(fighter.weapon_element, 0);
        assert_eq!(fighter.crit_accessory_bonus, 0.0);
    }

    #[test]
    fn a_won_battle_keeps_a_new_game_hero_at_level_one() {
        let mut party = Party::new_game();
        let tables = synthetic_tables(&leveling_file());
        party.apply_level_tables(&tables);
        let level_ups = party.award(7, 13, &[22087], &tables);
        assert!(level_ups.is_empty());
        let hero = &party.members[0];
        assert_eq!((hero.level(), hero.current_experience()), (1, 7));
        assert_eq!(party.gold, 13);
        assert_eq!(party.bag.count(22087), 1);
        assert_eq!((hero.hp, hero.max_hp), (101, 101));
    }

    #[test]
    fn experience_crossing_levels_takes_the_level_table_rows() {
        let mut party = Party::new_game();
        let tables = synthetic_tables(&leveling_file());
        party.apply_level_tables(&tables);
        let level_ups = party.award(250, 0, &[], &tables);
        assert_eq!(
            level_ups,
            vec![LevelUp {
                member: 0,
                level: 3,
                strength: 2,
                resilience: 2,
                agility: 2,
                deftness: 2,
                charm: 2,
                magical_might: 2,
                magical_mending: 2,
                max_hp: 2,
                max_mp: 2,
                skill_points: 20,
            }]
        );
        let hero = &party.members[0];
        assert_eq!(hero.level(), 3);
        assert_eq!(hero.current_experience(), 250);
        assert_eq!(hero.stats.strength, 3);
        // Level-up raises the maxima and leaves the current HP/MP alone.
        assert_eq!((hero.hp, hero.max_hp), (101, 103));
        assert_eq!((hero.mp, hero.max_mp), (201, 203));
        assert_eq!(hero.skill_points, 20);
        assert_eq!(party.bag.count(22087), 0);
    }

    #[test]
    fn experience_is_shared_by_the_level_weight_of_the_standing_members() {
        let mut party = Party::new_game();
        let mut second = Character::hero();
        second.levels[6] = 2;
        party.members.push(second);
        let tables = synthetic_tables(&leveling_file());
        party.apply_level_tables(&tables);
        let level_ups = party.award(11, 0, &[], &tables);
        // k = 4: weights 5 and 6, sum 11, so 5 and 6 experience (rounded up).
        assert_eq!(party.members[0].current_experience(), 5);
        assert_eq!(party.members[1].current_experience(), 6);
        assert!(level_ups.is_empty());
        // A member who is down receives and weighs nothing.
        party.members[1].hp = 0;
        party.members[0].experience[6] = 0;
        party.members[1].experience[6] = 0;
        let level_ups = party.award(11, 0, &[], &tables);
        assert_eq!(party.members[0].current_experience(), 11);
        assert_eq!(party.members[1].current_experience(), 0);
        assert!(level_ups.is_empty());
    }

    #[test]
    fn gold_is_capped_and_a_stack_holds_ninety_nine() {
        let mut party = Party::new_game();
        party.gold = GOLD_MAX - 5;
        let tables = synthetic_tables(&leveling_file());
        party.award(0, 10, &[22000, 22000, 22000], &tables);
        assert_eq!(party.gold, GOLD_MAX);
        assert_eq!(party.bag.count(22000), 3);
        assert!(party.bag.add_copies(22000, 200));
        assert_eq!(party.bag.count(22000), BAG_STACK as u32);
        let mut small = Bag::empty(1);
        assert!(small.add(22000));
        assert!(!small.add(22087));
    }

    /// The synthetic level table used above: every level row the same, so the assertions do
    /// not depend on the level.
    fn level_file() -> Vec<u8> {
        let mut records = Vec::new();
        for _ in 0..99 {
            let values = [0u32, 40, 41, 42, 43, 44, 45, 46, 47, 48, 200];
            let mut record = 0x66u16.to_le_bytes().to_vec();
            record.push(11);
            record.extend([0x55, 0x55, 0x15]);
            record.extend([0xFF, 0xFF]);
            for v in values {
                record.extend(v.to_le_bytes());
            }
            records.push(record);
        }
        let body: Vec<u8> = records.concat();
        let pool_offset = (0x10 + body.len()).next_multiple_of(16);
        let mut out = Vec::new();
        for word in [records.len(), pool_offset, 0, 0] {
            out.extend((word as u32).to_le_bytes());
        }
        out.extend(body);
        out.resize(pool_offset, 0xFF);
        out
    }

    fn synthetic_tables(file: &[u8]) -> LevelTables {
        let files: [&[u8]; VOCATION_COUNT] = core::array::from_fn(|_| file);
        LevelTables::parse(files).unwrap()
    }

    /// A level table whose rows grow with the level: level `i` is reached at `100 (i − 1)`,
    /// gives each attribute `i + column`, and 10 more skill points than the level before.
    fn leveling_file() -> Vec<u8> {
        let mut records = Vec::new();
        for i in 1..=99u32 {
            let experience = if i == 1 { 0 } else { (i - 1) * 100 };
            let values = [
                experience,
                i,
                i + 1,
                i + 2,
                i + 3,
                i + 4,
                i + 5,
                i + 6,
                100 + i,
                200 + i,
                if i == 1 { 0 } else { (i - 1) * 10 },
            ];
            let mut record = 0x66u16.to_le_bytes().to_vec();
            record.push(11);
            record.extend([0x55, 0x55, 0x15]);
            record.extend([0xFF, 0xFF]);
            for v in values {
                record.extend(v.to_le_bytes());
            }
            records.push(record);
        }
        let body: Vec<u8> = records.concat();
        let pool_offset = (0x10 + body.len()).next_multiple_of(16);
        let mut out = Vec::new();
        for word in [records.len(), pool_offset, 0, 0] {
            out.extend((word as u32).to_le_bytes());
        }
        out.extend(body);
        out.resize(pool_offset, 0xFF);
        out
    }
}
