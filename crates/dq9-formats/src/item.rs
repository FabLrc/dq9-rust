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
}
