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
//!   fields 7 and 8 the **current** HP and MP and fields 9 and 10 the **maximum** HP and
//!   MP (`func_020830cc` writes both pairs from the member object's `+0x1c`/`+0x1e` and
//!   `+0x6c`/`+0x6e`; `Save_UnpackMain` (`0x020a95b4`) and `func_02082d7c` read only 7 and 8
//!   back into the member's HP and MP (`melonds::copies::member_fields`, `+0x1c`/`+0x1e`).
//!   A save therefore restores a wounded character **wounded**, at the record's own value,
//!   while the load recomputes the attributes and the maxima from the level file, the seeds
//!   and the equipment (`func_02082fc4`, `func_02083e28`). Fields 0 to 6 and 9 to 10 are the
//!   writer's cache, never its input.
//!
//! A new game's hero is the record character creation writes (`ov_009` `0x0218636c` and the
//! `u16` list at `0x0218ab78`): the Minstrel's vocation (6), level 1 in every vocation, no
//! experience, the copper sword and the opening clothes (items 20004, 13007, 16215 and
//! 17120). The values the level table computes are left empty until
//! [`Party::apply_level_tables`] runs.

use dq9_formats::battle::equipment::BattleParams;
use dq9_formats::hero::{HeroLook, Sex};
use dq9_formats::item::ItemStats;
use dq9_formats::level::{LevelStat, LevelTables};
use dq9_formats::save::{
    Block, Character as SaveCharacter, EquipSlot, NAME_LEN, NO_ITEM, Slot, VOCATION_COUNT,
};

use crate::battle::model::{Fighter, Side, WeaponReach};
use crate::collision::CollisionWorld;
use crate::fx::Fx3;
use crate::movement::Hero;

/// Characters a party holds (`cmp r0, #4`, `ov_003::021621a4` and its neighbours).
pub const PARTY_MAX: usize = 4;
/// Held items a party member carries (`7578 + 18 i`, 8 × u16).
pub const HELD_ITEMS: usize = 8;
/// Bytes of the learned skill panels (`+0x11A`): one bit per panel, 36 bytes for the 287
/// panels 0 to 286 (`docs/re/battle-data.md`, "Skill panels").
pub const SKILL_PANELS_BYTES: usize = 36;
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

    /// Removes up to `quantity` copies of `item` (`Bag_Remove`, `0x020a0a18`: "decrements a bag
    /// slot, frees it at 0"), across the stacks that hold it, and returns how many went.
    ///
    /// A stack that reaches 0 empties its slot; no slot is merged with another and the slots
    /// keep their order, so a bag that held one stack of an item still does. A quantity past
    /// what the bag holds removes what there is. `0` for an item the bag does not hold
    /// (`docs/re/save.md` reads `Bag_Remove(…, 0x5612, 99)` as the save's own call).
    pub fn remove(&mut self, item: u16, quantity: u8) -> u32 {
        let mut left = u32::from(quantity);
        let mut removed = 0;
        for stack in &mut self.slots {
            if left == 0 {
                break;
            }
            if stack.item != Some(item) {
                continue;
            }
            let taken = u32::from(stack.quantity).min(left);
            stack.quantity -= taken as u8;
            left -= taken;
            removed += taken;
            if stack.quantity == 0 {
                *stack = ItemStack::default();
            }
        }
        removed
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
    /// Current HP and MP, as the save carries them (`+0x14c` fields 7 and 8): the load
    /// restores the saved value, wounded included, not the maximum.
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
    /// The skill panels learned (`+0x11A`, 36 bytes): bit `p % 8` of byte `p / 8` says panel
    /// `p`, 0 to 286. The original's per-character bit array is `live+0x8EC`, written by the
    /// ownership walk (`0x02083acc`, `docs/re/battle-data.md`); a new game starts with none
    /// **(the creation initialiser's bits are not read)**.
    pub skill_panels: [u8; SKILL_PANELS_BYTES],
    /// The eight held items of the party slot (`7578 + 18 i`), [`NO_ITEM`] or 0 empty.
    pub held_items: [Option<u16>; HELD_ITEMS],
    /// Face, hair, appearance bits and the worn item bands, as the hero rig builds them
    /// (`HeroLook::from_character`, `docs/re/characters.md` § "The hero"). It is read once,
    /// from the same record as the equipment, by [`Character::from_record`].
    pub look: HeroLook,
}

impl Character {
    /// Converts a save record and the held items of its party slot. The stat block's fields 7
    /// and 8 are the saved current HP and MP (`PartyMember_LoadRecord`, `0x02082d7c`), 9 and
    /// 10 the maxima `func_020830cc` cached from the member's `+0x6c`/`+0x6e`. A maximum of 0
    /// (a new game's hero before the level tables) leaves [`Character::apply_level_tables`]
    /// to fill both the maximum and the current value.
    pub fn from_record(record: &SaveCharacter<'_>, held: [u16; HELD_ITEMS]) -> Self {
        let bytes = record.bytes();
        let hp = stat_field(bytes, 7);
        let max_hp = stat_field(bytes, 9);
        let mp = stat_field(bytes, 8);
        let max_mp = stat_field(bytes, 10);
        let mut equipment = Equipment::default();
        for slot in EquipSlot::ALL {
            equipment.set(slot, worn(record.equipment(slot)));
        }
        let flags = record.proficiency_flags();
        let mut skill_panels = [0u8; SKILL_PANELS_BYTES];
        let copied = flags.len().min(SKILL_PANELS_BYTES);
        skill_panels[..copied].copy_from_slice(&flags[..copied]);
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
            hp,
            max_hp,
            mp,
            max_mp,
            stats: BaseStats::default(),
            equipment,
            skill_points: record.unallocated_skill_points(),
            skill_panels,
            held_items: held.map(worn),
            look: HeroLook::from_character(record),
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
            skill_panels: [0; SKILL_PANELS_BYTES],
            held_items: [None; HELD_ITEMS],
            look: HeroLook::new_game(),
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

    /// Whether the character has learned skill panel `place` (`+0x11A` bit, 0 to 286).
    pub fn has_skill_panel(&self, place: u16) -> bool {
        let place = usize::from(place);
        self.skill_panels
            .get(place / 8)
            .is_some_and(|byte| byte >> (place % 8) & 1 != 0)
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
    /// record's own current HP and MP and its cached maxima (`+0x14c` fields 7 to 10); the
    /// original recomputes the maxima the same way at load (`func_02082fc4`) and a fallen
    /// member stays at 0 rather than standing back up.
    pub fn apply_level_tables(&mut self, tables: &LevelTables) {
        let Some(stat) = tables.stat(self.vocation, self.level()) else {
            return;
        };
        self.stats = BaseStats::from_level(stat);
        if self.max_hp == 0 {
            self.max_hp = stat.max_hp;
            if self.hp == 0 {
                self.hp = self.max_hp;
            }
        }
        if self.max_mp == 0 {
            self.max_mp = stat.max_mp;
            if self.mp == 0 {
                self.mp = self.max_mp;
            }
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
                    // `func_ov017_021b3780` copies the worn `itembtlprm` records into the
                    // character (`+0x2F4`, 44 bytes a slot) in the order `08 00 09 07 05 01 06
                    // 0A`: its record 0 is live slot 8, the weapon. `func_02085128` reads that
                    // record's `+0x00` bit 4 (the "+1 against metal") and `func_020851d8` its
                    // bit 10 (the poison needle); both also require a positive weapon id
                    // (`+0x2AC`, the item record slot 8's id).
                    fighter.metal_bonus = (param.flags >> 4) & 1 != 0;
                    fighter.poison_needle = (param.flags >> 10) & 1 != 0;
                    // The same record's bits 0–1 are the weapon's reach, which a plain Attack
                    // takes (`func_ov000_021540fc`): bit 0 a group, bit 1 everyone, neither one
                    // target (`docs/re/item.md`, "The weapon words").
                    fighter.weapon_reach = match param.weapon_reach() {
                        3 => WeaponReach::All,
                        4 => WeaponReach::Group,
                        _ => WeaponReach::One,
                    };
                    // The twelve killer-bonus fields of the same copied record
                    // (`CalculateFinalDamage`'s `func_02085968` … `func_02085888`, tenths with
                    // 10 none). Only the weapon's record is read, and an empty slot keeps the
                    // kernel's "no bonus" ten.
                    fighter.family_bonus = param.family_bonus;
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
        // read here; the trait fields (0x11d, the accuracy die) are not established.
        fighter.crit_book_bonus = 0.0;
        fighter.crit_trait_11d = false;
        fighter.accuracy_die_trait = false;
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
    /// The other members on the field, behind the leader ([`PartyField`]).
    pub field: PartyField,
}

impl Party {
    /// A new game: the hero alone, no gold, an empty bag.
    pub fn new_game() -> Self {
        Self {
            members: vec![Character::hero()],
            gold: 0,
            bag: Bag::empty(BAG_CAPACITY),
            field: PartyField::default(),
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
            field: PartyField::default(),
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

    /// Applies a won battle's experience, gold and drops, and levels the members up, with
    /// the neutral share: every member counts one round and no equipment table is given, so
    /// the counter cancels in every share and no bonus applies. The original increments the
    /// counter once per round for **every** member the battle holds, dead or standing, so the
    /// counter cancels in a battle of a fixed party: this is exact there, and only a member that
    /// joins mid-battle or a worn experience-bonus item differs. [`Party::award_with`] takes the
    /// battle's own counters ([`crate::battle::Battle::passes`]) and the item table.
    pub fn award(
        &mut self,
        experience: u32,
        gold: u32,
        drops: &[u16],
        tables: &LevelTables,
    ) -> Vec<LevelUp> {
        self.award_with(experience, gold, drops, tables, &Share::neutral())
    }

    /// [`Party::award`] with the share's own inputs.
    ///
    /// Experience is shared the way `func_ov023_021f4098` does it: a member who is down
    /// (`func_02061bd8`; this model's `hp == 0`) receives and weighs nothing; for the rest
    ///
    /// ```text
    /// weight = (k + level) × rounds
    /// share  = ⌈ total × weight ÷ Σ weight ⌉      (× 1.05 before the rounding, with the bonus)
    /// ```
    ///
    /// `level` is the member's level in its current vocation (`func_0202053c`, the halfword
    /// at `+0x16c + 2 × vocation`); `rounds` is the battle's per-member counter
    /// ([`Share::rounds`]); `k` is the band of `expadj.nat` the truncated total falls in (4
    /// up to 10,000, 3 up to 20,000, 2 beyond; `func_ov023_021f5534`). The game first
    /// multiplies the total by `1 + (n − 1) ÷ 10` for `n` connected players; this model has
    /// one player, so that factor is exactly 1 (`0x021f4214`).
    ///
    /// The division and the rounding are the original's 32-bit float operations in its order
    /// (`weight ÷ Σ weight` then `× total`, `0x021f4370`), so a share whose exact value the
    /// format cannot represent rounds up where integer arithmetic would not. A sum of 0
    /// counts as 1 (`0x021f4318`), so counters all 0 give every member 0.
    ///
    /// × 1.05 (`0x3f866666`) applies when any worn item's `itembtlprm.nat` record carries
    /// flag bit 16 (`func_0208539c`): the share is then multiplied before the rounding, so
    /// the shares can come to more than the total. The share is added to the current
    /// vocation's experience (`func_ov023_021f5150`), never to the other vocations.
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
    pub fn award_with(
        &mut self,
        experience: u32,
        gold: u32,
        drops: &[u16],
        tables: &LevelTables,
        share: &Share<'_>,
    ) -> Vec<LevelUp> {
        self.gold = self.gold.saturating_add(gold).min(GOLD_MAX);
        for &item in drops {
            self.bag.add(item);
        }
        // `_ffltu([battle + 0x58c0])`: the total is a float, truncated once for the band
        // lookup (`0x021f4240`) and kept whole for the shares (`0x021f4348`).
        let total = experience as f32;
        let band = EXP_BANDS
            .iter()
            .find(|(bound, _)| (total as u32) <= *bound)
            .map_or(2, |(_, k)| *k);
        let k = band as f32;
        let mut weights = [0.0f32; PARTY_MAX];
        for (index, member) in self.members.iter().enumerate().take(PARTY_MAX) {
            if member.hp == 0 {
                continue;
            }
            weights[index] = (k + f32::from(member.level())) * share.rounds[index] as f32;
        }
        // `p3 + (p2 + (p0 + p1))` (`0x021f42e8`), with a sum of 0 taken as 1.
        let mut sum = weights[3] + (weights[2] + (weights[0] + weights[1]));
        if sum == 0.0 {
            sum = 1.0;
        }
        let mut level_ups = Vec::new();
        for (index, &weight) in weights.iter().enumerate() {
            let Some(member) = self.members.get(index) else {
                break;
            };
            if member.hp == 0 {
                continue;
            }
            let mut value = total * (weight / sum);
            if wears_experience_bonus(member, share.equipment) {
                value *= 1.05;
            }
            let amount = rounded_up(value);
            if let Some(level_up) = self.members[index].gain_experience(amount, tables) {
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

/// Slots of a follower's queue of its followed member's positions (`0x020391ac`: the queue at
/// `+0x1d0`, eight bytes per (x, z); the head `+0x1cb` and the tail `+0x1cc` are indices modulo
/// 16).
pub const TRAIL_SLOTS: usize = 16;
/// The followed member must be this far from the queue's newest point for one to be appended
/// (`0x020391ac`'s `iVar8 = 0x800`): 0.5 world unit.
pub const TRAIL_SPACING: i32 = 0x800;
/// The spacing while the follower **and** the member it follows are both down (bit 0 of the
/// status word `+0x130`) and fewer than three points wait (`iRam02039714` = `0xe66`, 0.90
/// unit): a corpse trails further behind.
pub const TRAIL_SPACING_DOWN: i32 = 0xe66;
/// The follower consumes the queue's oldest point within this distance of it (`0x801`).
pub const TRAIL_ARRIVE: i32 = 0x801;
/// Top speeds `+0xb4` by the queue's length (`0x020391ac`): `0x9d` under two queued points
/// (2.30 units/s), `0x189` at two (`0x02039710`, the leader's own, 5.76 units/s) and `0x24d`
/// beyond (`0x02039718`, 8.63 units/s: a companion further behind runs faster).
pub const TRAIL_SLOW_SPEED: i32 = 0x9d;
pub const TRAIL_TOP_SPEED: i32 = 0x189;
pub const TRAIL_FAST_SPEED: i32 = 0x24d;
/// Added to the spacing for the catch-up test of `0x020391ac` (`0xb33`, 0.70 unit): a follower
/// whose queue is empty still walks while the member it follows is more than spacing + `0xb33`
/// away (1.2 units at the normal spacing, 1.6 while both are down).
pub const TRAIL_CATCH_UP: i32 = 0xb33;

/// One follower's queue of the member it follows (`0x020391ac`): up to sixteen (x, z) pairs,
/// a head and a tail. The rule appends the followed member's position when it is more than
/// [`TRAIL_SPACING`] from the newest point, walks to the oldest one and consumes it within
/// [`TRAIL_ARRIVE`], so the companion traces the followed member's steps half a unit apart.
///
/// The queue is empty (head = tail = 0, nothing recorded) until the followed member moves:
/// that is what a melonDS state of a party of four shows after the party is placed on a map
/// (`build/melonds/dq9.ml1`, all four at the same pose).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trail {
    points: [[i32; 2]; TRAIL_SLOTS],
    head: u8,
    tail: u8,
    started: bool,
}

impl Default for Trail {
    fn default() -> Self {
        Self {
            points: [[0; 2]; TRAIL_SLOTS],
            head: 0,
            tail: 0,
            started: false,
        }
    }
}

/// What one update of the follower rule asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrailStep {
    /// The queue's oldest point, to walk to; `None` leaves the member standing.
    pub target: Option<[i32; 2]>,
    /// `+0xb4`, the top speed of this update.
    pub max_speed: i32,
    /// Points waiting, `(head − tail) & 15`.
    pub gap: u8,
}

impl Trail {
    /// Points waiting in the queue.
    pub fn gap(&self) -> u8 {
        self.head.wrapping_sub(self.tail) & 0x0f
    }

    /// The newest point recorded, for the reports.
    pub fn newest(&self) -> [i32; 2] {
        self.points[usize::from(self.head)]
    }

    /// `0x020391ac` on one update, after the follower's own generic update. `own` and
    /// `followed` are x and z in `fx32`; `moving` is the followed member's animation kind 1..5
    /// (its walking state), the gate that starts the queue the first time;
    /// `followed_distance` is the horizontal distance to the member it follows, which makes a
    /// companion with an empty queue keep walking while that member is far; `both_down` is bit
    /// 0 of the status word `+0x130` on both of them.
    pub fn step(
        &mut self,
        own: [i32; 2],
        followed: [i32; 2],
        moving: bool,
        followed_distance: i32,
        both_down: bool,
    ) -> TrailStep {
        if !self.started {
            if !moving {
                return TrailStep {
                    target: None,
                    max_speed: TRAIL_SLOW_SPEED,
                    gap: 0,
                };
            }
            self.started = true;
            self.points[usize::from(self.head)] = followed;
        }
        // The spacing the queue is fed at, and the one the catch-up test adds `0xb33` to: a
        // wider one while both members are down and fewer than three points wait.
        let spacing = if both_down && self.gap() < 3 {
            TRAIL_SPACING_DOWN
        } else {
            TRAIL_SPACING
        };
        if planar(followed, self.points[usize::from(self.head)]) > spacing {
            self.head = (self.head + 1) & 0x0f;
            self.points[usize::from(self.head)] = followed;
        }
        let gap = self.gap();
        let max_speed = match gap {
            0 | 1 => TRAIL_SLOW_SPEED,
            2 => TRAIL_TOP_SPEED,
            _ => TRAIL_FAST_SPEED,
        };
        let target = self.points[usize::from(self.tail)];
        // `iVar6 = func_02046b08(...)`: the field runs this rule, so the gate is taken as true.
        let walking = gap > 0 || spacing + TRAIL_CATCH_UP < followed_distance;
        if !walking {
            return TrailStep {
                target: None,
                max_speed,
                gap,
            };
        }
        if self.tail != self.head && planar(own, target) < TRAIL_ARRIVE {
            self.tail = (self.tail + 1) & 0x0f;
        }
        TrailStep {
            target: Some(target),
            max_speed,
            gap,
        }
    }
}

/// Horizontal `VEC_Distance` of two (x, z) pairs (`fx::mag` of the 3D vector with y = 0).
fn planar(a: [i32; 2], b: [i32; 2]) -> i32 {
    crate::fx::mag([a[0] - b[0], 0, a[1] - b[1]])
}

/// One companion on the field: its body, its queue of the positions of the member it follows,
/// the distance it covered on the last update and what the rule asked for at its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Follower {
    hero: Hero,
    trail: Trail,
    moved: i32,
    /// `+0xae`: the heading the rule set at the end of the last update, used by the next one.
    heading: i32,
    /// `+0xb4`: the top speed the rule set at the end of the last update.
    max_speed: i32,
}

impl Follower {
    /// The follower's body (`+0x44`, `+0x54`; see [`Hero`]).
    pub fn hero(&self) -> &Hero {
        &self.hero
    }

    /// Horizontal distance covered on the last update, `fx32`; the application turns it into
    /// the gait (`Gait::from_speed`).
    pub fn moved(&self) -> i32 {
        self.moved
    }

    /// The queue of the followed member's positions.
    pub fn trail(&self) -> &Trail {
        &self.trail
    }
}

/// The party's other members on the field, behind the leader. Each one follows the member
/// before it in the party order through its own [`Trail`] (`0x020391ac` reads the followed
/// index at `+0x1c8` and the leader's at `+0x1ca`; `ov_017::02190884` and `0x020391ac`'s
/// formation set the chain, and a melonDS state of a save's party of four shows it as
/// `0 ← 3 ← 2 ← 1` in the order `[0, 3, 2, 1]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartyField {
    followers: Vec<Follower>,
}

impl PartyField {
    pub fn is_empty(&self) -> bool {
        self.followers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.followers.len()
    }

    pub fn followers(&self) -> &[Follower] {
        &self.followers
    }

    /// The (x, z) of every companion in world units, hero excluded: the map screen's dots.
    pub fn positions(&self) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.followers.iter().map(|f| {
            let at = f.hero.position();
            (at.x, at.z)
        })
    }

    /// `count` companions standing on the leader's pose. `ov_017::0218f064` creates every
    /// member at `Save_Warp+0x10`, the point the party arrives at, so they start together and
    /// the queues fill from the leader's steps.
    pub fn place(&mut self, leader: Fx3, angle: i32, count: usize) {
        let count = count.min(PARTY_MAX - 1);
        self.followers.clear();
        self.followers.extend((0..count).map(|_| Follower {
            hero: Hero::place(leader, angle),
            trail: Trail::default(),
            moved: 0,
            heading: angle,
            max_speed: 0,
        }));
    }

    /// One field update of every companion, leader first. `down` is the status bit 0 of each
    /// party member (`+0x130`), leader first: two members both down space their queue at
    /// [`TRAIL_SPACING_DOWN`]. The order is the original's (`0x02037d8`: the generic update,
    /// then the rule): the companion moves with the heading and top speed the rule asked for at
    /// the end of the previous update, and the rule then reads the pose it has now.
    pub fn step(
        &mut self,
        world: &CollisionWorld,
        leader: Fx3,
        leader_walking: bool,
        down: &[bool],
        ticks: i32,
    ) {
        let mut followed = [leader[0], leader[2]];
        let mut moving = leader_walking;
        let mut followed_down = down.first().copied().unwrap_or(false);
        for (index, follower) in self.followers.iter_mut().enumerate() {
            let before = follower.hero.position_fx();
            follower
                .hero
                .step_toward(world, follower.heading, follower.max_speed, ticks);
            let after = follower.hero.position_fx();
            let own = [after[0], after[2]];
            follower.moved = planar(own, [before[0], before[2]]);
            let own_down = down.get(index + 1).copied().unwrap_or(false);
            let step = follower.trail.step(
                own,
                followed,
                moving,
                planar(own, followed),
                own_down && followed_down,
            );
            match step.target {
                Some(target) => {
                    let delta = crate::fx::normalize([target[0] - own[0], 0, target[1] - own[1]]);
                    follower.heading = crate::fx::atan2(delta[0], delta[2]);
                    follower.max_speed = step.max_speed;
                }
                None => {
                    follower.heading = follower.hero.angle();
                    follower.max_speed = 0;
                }
            }
            followed = own;
            moving = follower.hero.is_walking();
            followed_down = own_down;
        }
    }
}

/// The inputs `func_ov023_021f4098` takes besides the battle's totals.
#[derive(Debug, Clone, Copy)]
pub struct Share<'a> {
    /// The battle's counter for each party slot, in party order: `[[member + 0x19c] + 8]`
    /// (`func_02053dfc`). The battle installs the block and clears the counter
    /// (`func_02053da0`, `0x02053db0`), then adds one for each member it holds on every pass of
    /// its round (`func_02053dd0`, `ov_026::021dacb0`), unless bit 0 of
    /// `[[member + 0x138] + 0x14]` holds it. The call site tests the slot's presence, not its
    /// HP, so every member present from the start carries the same count and the counter
    /// cancels in every share; it only separates a member that joins mid-battle (fewer passes)
    /// or one the gate holds (0, hence 0 experience). [`crate::battle::Battle::passes`] gives
    /// the battle's own. **[unknown]**: what bit 0 of `live + 0xA8` (`member + 0x138` points at
    /// the live record's `+0x94`) means.
    pub rounds: [u32; PARTY_MAX],
    /// `itembtlprm.nat`, for the experience bonus; `None` applies none.
    pub equipment: Option<&'a BattleParams>,
}

impl<'a> Share<'a> {
    /// One round for every slot and no equipment table: the counter cancels in every share
    /// and no bonus applies, so the split is the level weight alone.
    pub fn neutral() -> Self {
        Self {
            rounds: [1; PARTY_MAX],
            equipment: None,
        }
    }
}

/// `⌈ value ⌉` as the original rounds it (`0x021f43a8`): truncate, and add 1.0 when the
/// fraction is above 0, then truncate again.
fn rounded_up(value: f32) -> u32 {
    let truncated = value.trunc();
    let fraction = value - truncated;
    let adjusted = if 0.0 < fraction { value + 1.0 } else { value };
    adjusted as u32
}

/// Whether a worn item's `itembtlprm.nat` record carries flag bit 16 (`func_0208539c`, which
/// walks the eight equipment slots of the live member).
fn wears_experience_bonus(member: &Character, equipment: Option<&BattleParams>) -> bool {
    let Some(equipment) = equipment else {
        return false;
    };
    EquipSlot::ALL.iter().any(|&slot| {
        member
            .equipment
            .get(slot)
            .and_then(|id| equipment.find(id as i16))
            .is_some_and(|param| param.experience_bonus())
    })
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
            // +0x14C: word 2 fields 7 and 8 are the current HP and MP (30/12), word 3
            // fields 9 and 10 the maxima (40/20) — `func_020830cc`'s layout.
            let word = (30u32 << 10) | (12u32 << 20);
            bytes[at + 0x154..at + 0x158].copy_from_slice(&word.to_le_bytes());
            let word = 40u32 | (20u32 << 10);
            bytes[at + 0x158..at + 0x15c].copy_from_slice(&word.to_le_bytes());
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
        assert_eq!((hero.hp, hero.mp), (30, 12));
        assert_eq!((hero.max_hp, hero.max_mp), (40, 20));
        assert_eq!(party.members[1].vocation, 1);
        assert_eq!(party.gold, 1234);
        assert_eq!(party.bag.capacity(), 3);
        assert_eq!(party.bag.count(22159), 2);
        assert_eq!(party.bag.count(22000), 1);
        assert_eq!(party.bag.count(20004), 0);
        assert_eq!(party.bag.slots()[2], ItemStack::default());
        // The look the rig builds is the record's own (`HeroLook::from_character`), read from
        // the same record as the equipment: the two cannot drift apart at the parse.
        for (i, member) in party.members.iter().enumerate() {
            let record = save.slot(0).unwrap().character(1 + i).unwrap();
            assert_eq!(member.look, HeroLook::from_character(&record));
            assert_eq!(member.look.armour, Some(13007));
            assert_eq!(member.look.sex, Sex::Woman, "+372 bit 0");
        }
        // A party restored from a save places no companion until its map loads.
        assert!(party.field.is_empty());
    }

    #[test]
    fn a_wounded_record_reloads_wounded_and_keeps_its_maxima() {
        let save = party_save();
        let mut party = Party::from_slot(&save.slot(0).unwrap()).unwrap();
        let hero = &party.members[0];
        assert_eq!((hero.hp, hero.max_hp), (30, 40));
        assert_eq!((hero.mp, hero.max_mp), (12, 20));
        // The level tables supply the attributes and would supply the maxima of a character
        // that has none; a restored character keeps both its own maxima and its current
        // value (`func_02082fc4` recomputes only the maxima).
        let tables = synthetic_tables(&level_file());
        party.apply_level_tables(&tables);
        let hero = &party.members[0];
        assert_ne!(
            hero.stats,
            BaseStats::default(),
            "the level fixed the tables"
        );
        assert_eq!((hero.hp, hero.max_hp), (30, 40));
        assert_eq!((hero.mp, hero.max_mp), (12, 20));
        // A fallen member stays down: its maximum is established, so nothing fills its HP.
        party.members[0].hp = 0;
        party.apply_level_tables(&tables);
        assert_eq!(party.members[0].hp, 0);
    }

    #[test]
    fn a_removal_takes_copies_and_frees_the_slot_at_nought() {
        let mut bag = Bag::empty(4);
        bag.add_copies(22000, 5);
        bag.add_copies(22001, 3);
        assert_eq!(bag.remove(22000, 2), 2);
        assert_eq!(bag.count(22000), 3);
        assert_eq!(
            bag.slots()[0],
            ItemStack {
                item: Some(22000),
                quantity: 3
            }
        );
        assert_eq!(bag.slots()[1].item, Some(22001), "the next stack stays put");
        // More than the bag holds removes what there is, and the slot is freed.
        assert_eq!(bag.remove(22000, 9), 3);
        assert_eq!(bag.slots()[0], ItemStack::default());
        assert_eq!(bag.remove(22000, 1), 0);
        assert_eq!(bag.remove(22001, 0), 0);
        assert_eq!(bag.count(22001), 3);
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
    fn one_member_takes_the_whole_total_whatever_its_counter() {
        let mut party = Party::new_game();
        let tables = synthetic_tables(&leveling_file());
        party.apply_level_tables(&tables);
        // The counter cancels against itself: 11 × 5 × c ÷ (5 × c) is 11 for any c above 0.
        for rounds in [1, 7] {
            party.members[0].experience[6] = 0;
            let share = Share {
                rounds: [rounds, 0, 0, 0],
                equipment: None,
            };
            party.award_with(11, 0, &[], &tables, &share);
            assert_eq!(party.members[0].current_experience(), 11);
        }
        // No counter at all: the sum is taken as 1 and every weight is 0 × (k + level).
        party.members[0].experience[6] = 0;
        let share = Share {
            rounds: [0; PARTY_MAX],
            equipment: None,
        };
        party.award_with(11, 0, &[], &tables, &share);
        assert_eq!(party.members[0].current_experience(), 0);
    }

    #[test]
    fn the_counters_weight_each_share() {
        let mut party = two_member_party();
        let tables = synthetic_tables(&leveling_file());
        party.apply_level_tables(&tables);
        // k = 4: the weights are (4 + 1) × 3 and (4 + 2) × 1, 15 and 6 of 21, so 8 and 4
        // after the rounding rather than the even split of the counter-less rule.
        let share = Share {
            rounds: [3, 1, 0, 0],
            equipment: None,
        };
        party.award_with(11, 0, &[], &tables, &share);
        assert_eq!(party.members[0].current_experience(), 8);
        assert_eq!(party.members[1].current_experience(), 4);
    }

    #[test]
    fn the_share_follows_the_originals_float_rounding() {
        let mut party = two_member_party();
        party.members[0].levels[6] = 3;
        party.members[1].levels[6] = 2;
        let tables = synthetic_tables(&leveling_file());
        party.apply_level_tables(&tables);
        // Weights 7 and 6 of 13. In 32-bit floats 7 ÷ 13 is a hair above the exact ratio, so
        // 13 × it is 7.0000005 and rounds up to 8; 6 ÷ 13 lands exactly and stays 6
        // (`0x021f4370`).
        party.award_with(13, 0, &[], &tables, &Share::neutral());
        assert_eq!(party.members[0].current_experience(), 8);
        assert_eq!(party.members[1].current_experience(), 6);
    }

    #[test]
    fn an_experience_bonus_item_multiplies_the_share_before_the_rounding() {
        let mut party = Party::new_game();
        let tables = synthetic_tables(&leveling_file());
        party.apply_level_tables(&tables);
        party.members[0].equipment.accessory = Some(22000);
        let params = bonus_params(22000);
        let share = Share {
            rounds: [1; PARTY_MAX],
            equipment: Some(&params),
        };
        // 7 experience with the whole weight: 7 × 1.05 = 7.35, rounded up to 8.
        party.award_with(7, 0, &[], &tables, &share);
        assert_eq!(party.members[0].current_experience(), 8);
        // The same award without the table keeps the total whole.
        party.members[0].experience[6] = 0;
        party.award_with(7, 0, &[], &tables, &Share::neutral());
        assert_eq!(party.members[0].current_experience(), 7);
    }

    /// The hero and a second Minstrel one level above him, both at full health.
    fn two_member_party() -> Party {
        let mut party = Party::new_game();
        let mut second = Character::hero();
        second.levels[6] = 2;
        party.members.push(second);
        party
    }

    /// `itembtlprm.nat` with one record whose flags carry the experience bonus (bit 16).
    fn bonus_params(id: u16) -> BattleParams {
        let mut record = [0u8; 44];
        record[..4].copy_from_slice(&(1u32 << 16).to_le_bytes());
        record[0x28..0x2a].copy_from_slice(&(id as i16).to_le_bytes());
        let mut data = 1u32.to_le_bytes().to_vec();
        data.extend(record);
        BattleParams::parse(&data).unwrap()
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

#[cfg(test)]
mod trail_tests {
    use super::*;

    /// `u(1.0)` in `fx32`.
    fn u(v: f32) -> i32 {
        (v * 4096.0) as i32
    }

    /// A queue started by one step of the followed member at the origin.
    fn started() -> Trail {
        let mut trail = Trail::default();
        trail.step([0, 0], [0, 0], true, 0, false);
        trail
    }

    #[test]
    fn an_idle_followed_member_records_nothing_until_it_moves() {
        let mut trail = Trail::default();
        // The animation kind is 0 (standing): the queue does not start and the member stands.
        for _ in 0..5 {
            let step = trail.step([0, 0], [0, 0], false, 0, false);
            assert_eq!(step.target, None);
            assert_eq!(step.gap, 0);
        }
        // A walking followed member starts it on its own position, one point; the follower has
        // nothing to walk to yet and stands.
        let step = trail.step([0, 0], [u(1.0), 0], true, 0, false);
        assert_eq!(step.target, None);
        assert_eq!(step.gap, 0);
        assert_eq!(trail.newest(), [u(1.0), 0]);
    }

    #[test]
    fn a_point_is_appended_every_half_unit_and_the_follower_consumes_them() {
        // 0x020391ac: the newest point moves on when the followed member is more than 0x800
        // away from it, and the follower consumes the oldest within 0x801.
        let mut trail = started();
        let away = [u(-5.0), 0];
        // Under half a unit: nothing appended.
        trail.step(away, [u(0.49), 0], true, 0, false);
        assert_eq!(trail.gap(), 0);
        // Beyond half a unit: one point, the target is the old one.
        trail.step(away, [u(0.51), 0], true, 0, false);
        assert_eq!(trail.gap(), 1);
        assert_eq!(trail.newest(), [u(0.51), 0]);
        // Two points queued, the oldest one is the target.
        let step = trail.step(away, [u(1.2), 0], true, 0, false);
        assert_eq!(step.target, Some([0, 0]));
        assert_eq!(trail.gap(), 2);
        // Within 0.5+1 of the oldest point it is consumed: this update's target was still it,
        // the next one is the point after it.
        let step = trail.step([u(0.4), 0], [u(1.2), 0], true, 0, false);
        assert_eq!(step.target, Some([0, 0]));
        assert_eq!(trail.gap(), 1);
        let step = trail.step([u(0.6), 0], [u(1.2), 0], true, 0, false);
        assert_eq!(step.target, Some([u(0.51), 0]));
        assert_eq!(trail.gap(), 0);
    }

    #[test]
    fn the_top_speed_rises_with_the_queue() {
        // A follower one point behind walks at 0x9d, two points at 0x189, three or more at
        // 0x24d (`0x020391ac`, constants `0x02039710`/`0x02039718`).
        let mut trail = started();
        let speed = |trail: &mut Trail, at: f32| {
            trail
                .step([u(-10.0), 0], [u(at), 0], true, 0, false)
                .max_speed
        };
        assert_eq!(speed(&mut trail, 0.6), TRAIL_SLOW_SPEED);
        assert_eq!(speed(&mut trail, 1.2), TRAIL_TOP_SPEED);
        assert_eq!(speed(&mut trail, 1.8), TRAIL_FAST_SPEED);
        assert_eq!(speed(&mut trail, 2.4), TRAIL_FAST_SPEED);
        assert_eq!(TRAIL_SLOW_SPEED, 0x9d);
        assert_eq!(TRAIL_TOP_SPEED, 0x189);
        assert_eq!(TRAIL_FAST_SPEED, 0x24d);
    }

    #[test]
    fn the_queue_holds_sixteen_points_and_stops_on_a_sharp_turn() {
        // The ring is sixteen slots: a seventeenth point overwrites the oldest, and a follower
        // that turns back walks the points in the order they were recorded.
        let mut trail = started();
        let away = [u(-5.0), 0];
        for i in 1..=20 {
            trail.step(away, [u(i as f32), 0], true, 0, false);
        }
        // Twenty points recorded in sixteen slots: the oldest four were overwritten, and the
        // target is the oldest one left (16.0), not the newest (20.0).
        assert_eq!(trail.gap(), 4);
        // A U-turn: the followed member comes back; every point keeps the place it was
        // recorded at, so the follower walks them one by one instead of cutting the corner.
        let step = trail.step(away, [u(20.0), u(0.5)], true, 0, false);
        assert_eq!(step.target, Some([u(16.0), 0]));
        assert_eq!(trail.gap(), 4);
    }

    #[test]
    fn an_empty_queue_still_walks_while_the_followed_member_is_far() {
        // 0x020391ac: with nothing queued, a companion more than `spacing + 0xb33` (0x1333,
        // 1.2 units) from the member it follows keeps walking towards the newest point;
        // nearer, it stands.
        let mut trail = started();
        let catch_up = TRAIL_SPACING + TRAIL_CATCH_UP;
        let far = trail.step([0, 0], [0, 0], false, catch_up + 1, false);
        assert!(far.target.is_some());
        assert_eq!(far.gap, 0);
        let near = trail.step([0, 0], [0, 0], false, catch_up, false);
        assert_eq!(near.target, None);
        assert_eq!(PARTY_MAX, 4);
        assert_eq!(TRAIL_SPACING, 0x800);
        assert_eq!(TRAIL_ARRIVE, 0x801);
        assert_eq!(catch_up, 0x1333);
    }

    #[test]
    fn two_down_members_space_their_queue_at_the_wider_threshold() {
        // 0x020391ac: when the follower and the followed member are both down and fewer than
        // three points wait, the spacing is `iRam02039714` (0xe66) instead of 0x800, and the
        // catch-up test adds 0xb33 to that.
        let mut trail = started();
        let away = [u(-20.0), 0];
        // 0.8 unit away: below 0xe66 (0.90), nothing appended while both are down…
        trail.step(away, [u(0.8), 0], true, 0, true);
        assert_eq!(trail.gap(), 0);
        // …and appended at the normal spacing when they are not (0.8 > 0.5).
        trail.step(away, [u(0.8), 0], true, 0, false);
        assert_eq!(trail.gap(), 1);
        // 1.4 units from the newest point: appended even with both down (1.4 > 0.90).
        trail.step(away, [u(2.2), 0], true, 0, true);
        assert_eq!(trail.gap(), 2);
        // Two points waiting still take the wide spacing: 0.6 unit more is not enough.
        trail.step(away, [u(2.8), 0], true, 0, true);
        assert_eq!(trail.gap(), 2);
        // One more point at the normal spacing; with three waiting the gate `gap < 3` fails,
        // so 0.6 more appends even with both down.
        trail.step(away, [u(2.8), 0], true, 0, false);
        assert_eq!(trail.gap(), 3);
        trail.step(away, [u(3.4), 0], true, 0, true);
        assert_eq!(trail.gap(), 4);
        assert_eq!(TRAIL_SPACING_DOWN, 0xe66);
    }

    #[test]
    fn the_companions_chain_on_the_leader_and_follow_the_queue() {
        // A party of four on a flat floor: each companion follows the one before it, so the
        // last one lags behind the first, and none stands on the leader.
        use crate::collision::STAND_HEIGHT;
        use crate::testing::{floor, world};
        let w = world(&floor(30_000, 0));
        let mut field = PartyField::default();
        let start = [0, STAND_HEIGHT, 0];
        field.place(start, 0, 3);
        assert_eq!(field.len(), 3);
        // The leader walks 4 units north at full speed (0x189 per tick, two ticks a step).
        let mut leader = [0, STAND_HEIGHT, 0];
        for _ in 0..120 {
            leader[2] -= 0x189 * 2;
            field.step(&w, leader, true, &[], 2);
        }
        let zs: Vec<i32> = field
            .followers()
            .iter()
            .map(|f| f.hero().position_fx()[2])
            .collect();
        assert!(zs[0] < -0x1000, "the first companion moved: {zs:?}");
        assert!(zs[0] > leader[2], "and stays behind the leader");
        assert!(zs[1] >= zs[0] && zs[2] >= zs[1], "each one behind: {zs:?}");
        // Party of one: nothing to step.
        let mut solo = PartyField::default();
        solo.place(start, 0, 0);
        assert!(solo.is_empty());
        solo.step(&w, leader, true, &[], 2);
        assert!(solo.followers().is_empty());
    }
}
