//! Item tables: the item's id and the **compendium index** the flag block records when it is
//! first obtained.
//!
//! Source: `data/prm/itemdt_<category>.gp2`, member `itemdt_<category>_<language>.nat`
//! (`docs/re/item.md`). Each member starts with a 12-byte head (u16 record count, u16 flags,
//! two unused u32) followed by `count` 32-byte records. A record holds the item id as `s16`
//! at `+0x18` and, at `+0xc`, a 23-bit field whose bits 12..22 are the compendium index
//! (`func_0207cc18`, `0x0207cc28`: `(record[0xc] & 0x7fffff) >> 0xc`). Index 0 has no entry
//! in the compendium and sets no flag.
//!
//! The record numbers are the same in every language (wiki DQIX, *Items*), so the reader
//! takes the first member of each category. No I/O: the caller supplies the
//! decompressed bytes.
//!
//! An equipment member continues, after the records, with one **32-byte stats entry per
//! record**, in the order the records' `+0x00` word indexes (0 to `count − 1`, each used
//! once; the tools have none and their word is `0xFFFFFFFF`). Three of the entry's words
//! hold three signed 10-bit fields each, the battle numbers the character screen and
//! `func_02083e28` add to the level table's attributes:
//!
//! | word | fields |
//! |---|---|
//! | `+0x08` | attack, defence, not established |
//! | `+0x0c` | block, evasion, critical, in tenths of a hundredth (`func_02084ef8`,
//!   `func_02084f68`, `func_02084e78` divide each by 10.0) |
//! | `+0x10` | deftness, agility, magical might |
//!
//! The values are INFERRED from the item descriptions and the guide except the three rate
//! fields, which the code reads; `docs/re/item.md` has the checks. The other fields of the
//! entry are not established and are not exposed.

use std::collections::BTreeMap;

use nds_rom::{Error, Result};

/// Item table categories, by the letter of `data/prm/itemdt_<letter>.gp2`: gloves, body,
/// accessories, helms, footwear, shields, tools, legwear, weapons.
pub const CATEGORIES: [&str; 9] = ["a", "b", "d", "h", "l", "s", "t", "u", "w"];

/// Size of a record in a category table.
const RECORD_SIZE: usize = 0x20;
/// Offset of the first record, after the head.
const RECORDS_START: usize = 12;
/// `s16` item id inside a record (`func_020de56c`, `0x020de57c`).
const ID_OFFSET: usize = 0x18;
/// Word holding the compendium index in bits 12..22.
const COMPENDIUM_OFFSET: usize = 0xc;

/// Item id → compendium index, merged from the category tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemTable {
    entries: BTreeMap<u16, u16>,
}

impl ItemTable {
    /// Parses one category member and merges its records.
    pub fn add_member(&mut self, data: &[u8]) -> Result<()> {
        let count = usize::from(u16_at(data, 0)?);
        let end = RECORDS_START
            .checked_add(count * RECORD_SIZE)
            .ok_or(Error::Malformed("item table record count"))?;
        if end > data.len() {
            return Err(Error::Malformed("item table shorter than its records"));
        }
        for index in 0..count {
            let at = RECORDS_START + index * RECORD_SIZE;
            let id = i16::from_le_bytes([data[at + ID_OFFSET], data[at + ID_OFFSET + 1]]);
            if id <= 0 {
                return Err(Error::Malformed("item table id not positive"));
            }
            let word = u32_at(data, at + COMPENDIUM_OFFSET)?;
            let compendium = ((word & 0x7f_ffff) >> 12) as u16;
            // Categories have disjoint id ranges; a repeated id keeps the first record.
            self.entries.entry(id as u16).or_insert(compendium);
        }
        Ok(())
    }

    /// Parses the members of every [`CATEGORIES`] table, in any order.
    pub fn from_members(members: &[&[u8]]) -> Result<Self> {
        let mut table = Self::default();
        for data in members {
            table.add_member(data)?;
        }
        Ok(table)
    }

    /// Compendium index of `id` (`func_0207cc18` reads the same field); `None` when the item
    /// is not in the tables.
    pub fn compendium(&self, id: u16) -> Option<u16> {
        self.entries.get(&id).copied()
    }

    /// Number of items known.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Battle numbers a worn item adds: the 32-byte stats entry of an equipment member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EquipStats {
    /// `+0x08` bits 0–9: a weapon's attack, also on the accessories that grant it.
    pub attack: u16,
    /// `+0x08` bits 10–19: a piece's defence.
    pub defence: u16,
    /// `+0x0c` bits 0–9, in tenths of a hundredth (`func_02084ef8` divides by 10.0).
    pub block: u16,
    /// `+0x0c` bits 10–19, likewise (`func_02084f68`).
    pub evasion: u16,
    /// `+0x0c` bits 20–29, likewise (`func_02084e78`).
    pub critical: u16,
    /// `+0x10` bits 0–9.
    pub deftness: u16,
    /// `+0x10` bits 10–19.
    pub agility: u16,
    /// `+0x10` bits 20–29.
    pub magical_might: u16,
}

impl EquipStats {
    /// The three 10-bit fields of one stats word.
    fn from_words(attack: u32, rates: u32, attributes: u32) -> Self {
        let field = |word: u32, shift: u32| ((word >> shift) & 0x3ff) as u16;
        Self {
            attack: field(attack, 0),
            defence: field(attack, 10),
            block: field(rates, 0),
            evasion: field(rates, 10),
            critical: field(rates, 20),
            deftness: field(attributes, 0),
            agility: field(attributes, 10),
            magical_might: field(attributes, 20),
        }
    }
}

/// Item id → the battle numbers its equipment record adds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemStats {
    entries: BTreeMap<u16, EquipStats>,
}

impl ItemStats {
    /// Parses one equipment category member (weapons, shields, helms, armour, gloves,
    /// legwear, footwear or accessories — not the tools' member, which has no stats table).
    ///
    /// # Errors
    /// [`nds_rom::Error`] when a record or stats entry runs past the member, or a record's
    /// stats index is past the table.
    pub fn add_member(&mut self, data: &[u8]) -> Result<()> {
        let count = usize::from(u16_at(data, 0)?);
        let records_end = RECORDS_START
            .checked_add(count * RECORD_SIZE)
            .ok_or(Error::Malformed("item stats record count"))?;
        let entries_end = records_end
            .checked_add(count * RECORD_SIZE)
            .ok_or(Error::Malformed("item stats count"))?;
        if entries_end > data.len() {
            return Err(Error::Malformed("item table has no complete stats table"));
        }
        for index in 0..count {
            let at = RECORDS_START + index * RECORD_SIZE;
            let id = i16::from_le_bytes([data[at + ID_OFFSET], data[at + ID_OFFSET + 1]]);
            if id <= 0 {
                return Err(Error::Malformed("item table id not positive"));
            }
            let stats_index = u32_at(data, at)? as usize;
            if stats_index >= count {
                return Err(Error::Malformed("item stats index past the table"));
            }
            let stats_at = records_end + stats_index * RECORD_SIZE;
            let stats = EquipStats::from_words(
                u32_at(data, stats_at + 0x08)?,
                u32_at(data, stats_at + 0x0c)?,
                u32_at(data, stats_at + 0x10)?,
            );
            self.entries.entry(id as u16).or_insert(stats);
        }
        Ok(())
    }

    /// Parses the members of the equipment categories, in any order.
    pub fn from_members(members: &[&[u8]]) -> Result<Self> {
        let mut table = Self::default();
        for data in members {
            table.add_member(data)?;
        }
        Ok(table)
    }

    /// The battle numbers of `id`; `None` when the item is not equipment.
    pub fn get(&self, id: u16) -> Option<&EquipStats> {
        self.entries.get(&id)
    }

    /// Number of equipment entries known.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    data.get(offset..offset + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Error::Malformed("item table head truncated"))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Error::Malformed("item table record truncated"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One category member: `count` records of 32 bytes after the 12-byte head.
    fn member(records: &[(u16, u16)]) -> Vec<u8> {
        let mut data = vec![0u8; RECORDS_START + records.len() * RECORD_SIZE];
        data[..2].copy_from_slice(&(records.len() as u16).to_le_bytes());
        for (index, &(id, compendium)) in records.iter().enumerate() {
            let at = RECORDS_START + index * RECORD_SIZE;
            data[at + ID_OFFSET..at + ID_OFFSET + 2].copy_from_slice(&id.to_le_bytes());
            let word = u32::from(compendium) << 12 | 0x1e3;
            data[at + COMPENDIUM_OFFSET..at + COMPENDIUM_OFFSET + 4]
                .copy_from_slice(&word.to_le_bytes());
        }
        data
    }

    #[test]
    fn records_merge_across_categories() {
        let gloves = member(&[(15000, 629), (15001, 630)]);
        let tools = member(&[(22000, 945), (22159, 1093)]);
        let table = ItemTable::from_members(&[&gloves, &tools]).unwrap();
        assert_eq!(table.len(), 4);
        assert_eq!(table.compendium(15000), Some(629));
        assert_eq!(table.compendium(22159), Some(1093));
        assert_eq!(table.compendium(12345), None);
    }

    #[test]
    fn a_truncated_member_is_an_error() {
        let mut data = member(&[(22000, 945)]);
        data.truncate(data.len() - 1);
        assert!(ItemTable::default().add_member(&data).is_err());
    }

    #[test]
    fn an_empty_table_member_parses_to_nothing() {
        let data = member(&[]);
        let table = ItemTable::from_members(&[&data]).unwrap();
        assert!(table.is_empty());
    }

    /// An equipment member: records whose `+0x00` is the stats index, then the stats table.
    fn equipment_member(entries: &[(u16, u16, u32, u32, u32)]) -> Vec<u8> {
        let count = entries.len();
        let records = RECORDS_START + count * RECORD_SIZE;
        let mut data = vec![0u8; records + count * RECORD_SIZE];
        data[..2].copy_from_slice(&(count as u16).to_le_bytes());
        for (i, &(id, stats_index, attack, rates, attributes)) in entries.iter().enumerate() {
            let at = RECORDS_START + i * RECORD_SIZE;
            data[at..at + 4].copy_from_slice(&u32::from(stats_index).to_le_bytes());
            data[at + ID_OFFSET..at + ID_OFFSET + 2].copy_from_slice(&id.to_le_bytes());
            if usize::from(stats_index) < count {
                let stats_at = records + usize::from(stats_index) * RECORD_SIZE;
                data[stats_at + 0x08..stats_at + 0x0c].copy_from_slice(&attack.to_le_bytes());
                data[stats_at + 0x0c..stats_at + 0x10].copy_from_slice(&rates.to_le_bytes());
                data[stats_at + 0x10..stats_at + 0x14].copy_from_slice(&attributes.to_le_bytes());
            }
        }
        data
    }

    #[test]
    fn equipment_stats_follow_the_records_stats_index() {
        // Copper sword: 7 attack; critical acclaim: 3 defence, 40 critical, 20 deftness.
        let sword = (7u32, 0, 0);
        let acclaim = (3u32, 40 << 20, 20);
        let data = equipment_member(&[
            (20004, 0, sword.0, sword.1, sword.2),
            (18054, 1, acclaim.0, acclaim.1, acclaim.2),
        ]);
        let stats = ItemStats::from_members(&[&data]).unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(
            stats.get(20004),
            Some(&EquipStats {
                attack: 7,
                ..EquipStats::default()
            })
        );
        let acclaim = stats.get(18054).unwrap();
        assert_eq!((acclaim.attack, acclaim.defence), (3, 0));
        assert_eq!(
            (acclaim.block, acclaim.evasion, acclaim.critical),
            (0, 0, 40)
        );
        assert_eq!(acclaim.deftness, 20);
        assert_eq!(stats.get(22000), None);
    }

    #[test]
    fn a_stats_index_past_the_table_is_an_error() {
        let data = equipment_member(&[(20004, 2, 7, 0, 0)]);
        assert!(ItemStats::default().add_member(&data).is_err());
    }

    #[test]
    fn a_member_without_its_stats_table_is_an_error() {
        let data = member(&[(22000, 945)]);
        assert!(ItemStats::default().add_member(&data).is_err());
    }
}
