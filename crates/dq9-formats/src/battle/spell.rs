//! Spells: `data/prm/spelltable.bin`, a [tagged data table](crate::table) listing the spells
//! in order and which vocation learns which spell at which level. Layout and evidence in
//! `docs/re/battle-data.md`; wiki *Spell Table*.
//!
//! The file opens with a `0x65` record (`0`) and a `0x64` record (`20`), as the level tables
//! do; not established and not read here. Then 65 `0x66` records of a place in the spell list
//! and the action there, 108 `0x67` records of vocation, place and level.

use nds_rom::{Error, Result};

use crate::table::{Table, Value};

/// Record kind of a spell list entry: (place, action).
const KIND_SPELL: u16 = 0x66;
/// Record kind of a learning entry: (vocation, place, level).
const KIND_LEARNED: u16 = 0x67;

/// One spell list entry: a place in the list and the action there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spell {
    /// Its place in the list, 0 to 65 (25 is missing on this ROM).
    pub place: u16,
    /// The action numbered there (wiki *Actions*).
    pub action: u16,
}

/// One learning entry: a vocation learning a spell at a level. Confirmed on the Minstrel's
/// records; INFERRED on the other vocations'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Learned {
    /// The vocation, numbered as the level tables are (0 Guardian, 1 Warrior … 12 Ranger).
    pub vocation: u8,
    /// The place in the spell list.
    pub place: u16,
    /// The level learned, never past 99.
    pub level: u8,
}

/// `spelltable.bin`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellTable {
    pub spells: Vec<Spell>,
    pub learned: Vec<Learned>,
}

impl SpellTable {
    /// # Errors
    /// [`nds_rom::Error`] when the table is malformed, a record's value count is wrong or a
    /// value does not fit its field.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut spells = Vec::new();
        let mut learned = Vec::new();
        for record in &table.records {
            let int = |i: usize| -> Result<u32> {
                match record.values.get(i) {
                    Some(Value::Int(v)) => Ok(*v),
                    _ => Err(Error::Malformed("spell table record needs an integer")),
                }
            };
            match record.kind {
                KIND_SPELL => {
                    if record.values.len() != 2 {
                        return Err(Error::Malformed("spell list record needs two values"));
                    }
                    spells.push(Spell {
                        place: u16::try_from(int(0)?)
                            .map_err(|_| Error::Malformed("spell place past 0xffff"))?,
                        action: u16::try_from(int(1)?)
                            .map_err(|_| Error::Malformed("spell action past 0xffff"))?,
                    });
                }
                KIND_LEARNED => {
                    if record.values.len() != 3 {
                        return Err(Error::Malformed("learning record needs three values"));
                    }
                    learned.push(Learned {
                        vocation: u8::try_from(int(0)?)
                            .map_err(|_| Error::Malformed("vocation past 0xff"))?,
                        place: u16::try_from(int(1)?)
                            .map_err(|_| Error::Malformed("spell place past 0xffff"))?,
                        level: u8::try_from(int(2)?)
                            .map_err(|_| Error::Malformed("level past 0xff"))?,
                    });
                }
                // 0x64 and 0x65 open the file; not established.
                _ => {}
            }
        }
        Ok(Self { spells, learned })
    }

    /// The spell at `place`.
    pub fn at(&self, place: u16) -> Option<&Spell> {
        self.spells.iter().find(|s| s.place == place)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tagged record: kind, integer values.
    fn record(kind: u16, values: &[u32]) -> Vec<u8> {
        let mut out = kind.to_le_bytes().to_vec();
        out.push(values.len() as u8);
        let bytes = values.len().div_ceil(4);
        for i in 0..bytes {
            let four = values
                .iter()
                .skip(i * 4)
                .take(4)
                .enumerate()
                .fold(0u8, |acc, (slot, _)| acc | 1 << (2 * slot));
            out.push(four);
        }
        out.resize(out.len().next_multiple_of(4), 0xFF);
        for v in values {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    fn table(records: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = records.concat();
        let pool_offset = (0x10 + body.len()).next_multiple_of(16);
        let mut data = Vec::new();
        data.extend_from_slice(&(records.len() as u32).to_le_bytes());
        data.extend_from_slice(&(pool_offset as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&body);
        data.resize(pool_offset, 0xFF);
        data
    }

    #[test]
    fn spells_and_learned_entries_decode() {
        let data = table(&[
            record(0x65, &[0]),
            record(0x64, &[20]),
            record(0x66, &[0, 9]),
            record(0x66, &[26, 30]),
            record(0x67, &[6, 26, 3]),
        ]);
        let spells = SpellTable::parse(&data).unwrap();
        assert_eq!(spells.spells.len(), 2);
        assert_eq!(
            spells.at(0),
            Some(&Spell {
                place: 0,
                action: 9
            })
        );
        assert_eq!(spells.at(26).unwrap().action, 30);
        assert_eq!(
            spells.learned,
            vec![Learned {
                vocation: 6,
                place: 26,
                level: 3
            }]
        );
    }

    #[test]
    fn wrong_value_count_is_an_error() {
        let data = table(&[record(0x66, &[0])]);
        assert!(SpellTable::parse(&data).is_err());
        let data = table(&[record(0x67, &[6, 26])]);
        assert!(SpellTable::parse(&data).is_err());
    }

    #[test]
    fn truncated_table_is_an_error() {
        let mut data = table(&[record(0x66, &[0, 9])]);
        data.truncate(data.len() - 1);
        assert!(SpellTable::parse(&data).is_err());
    }
}
