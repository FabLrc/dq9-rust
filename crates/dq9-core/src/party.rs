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

use dq9_formats::hero::Sex;
use dq9_formats::level::{LevelStat, LevelTables};
use dq9_formats::save::{
    Block, Character as SaveCharacter, EquipSlot, NAME_LEN, NO_ITEM, Slot, VOCATION_COUNT,
};

/// Characters a party holds (`cmp r0, #4`, `ov_003::021621a4` and its neighbours).
pub const PARTY_MAX: usize = 4;
/// Held items a party member carries (`7578 + 18 i`, 8 × u16).
pub const HELD_ITEMS: usize = 8;
/// Bag slots the game keeps (`GameState_Party+0`, capacity 152 in the oracle states).
pub const BAG_CAPACITY: usize = 152;

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
}
