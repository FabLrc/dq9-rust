//! Skill panels: `data/prm/skilltable.bin`, a [tagged data table](crate::table) listing what a
//! skill point buys, tree by tree. Layout and evidence in `docs/re/battle-data.md`; wiki
//! *Skill-Panels*.
//!
//! The file opens with a `0x65` record (`0`) and a `0x64` record (`20`), as the level tables
//! do; not established and not read here. Then 287 `0x66` records of nine integers: the panel's
//! id, its tree, its cost, the action it teaches, what it gives and how much, a second action,
//! a second index and the message shown when it is bought. 26 trees hold eleven panels each,
//! plus one record outside every tree (`[286, 0, 0, 168, 1, 0, 0, 286, 0]`, "Egg On").

use nds_rom::{Error, Result};

use crate::table::{Table, Value};

/// Record kind of a skill panel.
const KIND_PANEL: u16 = 0x66;

/// One skill panel: a record of `skilltable.bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    /// The panel's id, 0 to 286, present exactly once: the join to `sklname` and `sta_skl`.
    pub place: u16,
    /// Its tree, 1 to 26; 0 on the one record outside every tree.
    pub tree: u8,
    /// The skill points it costs, 0 to 100 (the eleventh panel of each tree is 0 and is
    /// unreachable: the ownership walk stops at the hundred-point one).
    pub cost: u8,
    /// The action it teaches, 0 for none. Every non-zero action is in `actdt_a`/`actdt_b`; all
    /// 148 carry [`effect`] 1 (an ability), and one further panel with effect 1 — 230, "Force" —
    /// names action 0 (a placeholder, wiki *Skill-Panels*).
    ///
    /// [`effect`]: Panel::effect
    pub action: u16,
    /// What it gives: 1 an ability, 0 its message alone, 2 attack, 3 critical hit rate, 4 may
    /// equip the tree's weapon, 5 shield block, 6 to 10 strength, resilience, agility,
    /// deftness, charm, 11 to 12 maximum HP and MP, 13 MP absorption, 14 evasion, 15 to 16
    /// magical mending and might, 17 spell critical rate. **INFERRED** from the `str_gskl`
    /// message each value goes with (wiki *Skill-Panels*, "What value 4 means").
    pub effect: u8,
    /// How much: the message's `<val_1>`.
    pub amount: u16,
    /// A second action, 0 on all but ten panels; **INFERRED** to be the out-of-battle form (it
    /// equals [`action`](Panel::action) on eight of the ten).
    pub field_action: u16,
    /// A second 0 to 286 index, also eleven to a tree but ordering the trees differently; **not
    /// established** which consumer reads it.
    pub order: u16,
    /// The `str_gskl` message shown when it is bought, 1 to 23; 0 on the one record outside
    /// every tree.
    pub message: u8,
}

/// `skilltable.bin`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillTable {
    pub panels: Vec<Panel>,
}

impl SkillTable {
    /// # Errors
    /// [`nds_rom::Error`] when the table is malformed, a panel's value count is not nine or a
    /// value does not fit its field.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut panels = Vec::new();
        for record in &table.records {
            if record.kind != KIND_PANEL {
                // 0x64 and 0x65 open the file; not established.
                continue;
            }
            if record.values.len() != 9 {
                return Err(Error::Malformed("skill panel record needs nine values"));
            }
            let int = |i: usize| -> Result<u32> {
                match record.values.get(i) {
                    Some(Value::Int(v)) => Ok(*v),
                    _ => Err(Error::Malformed("skill panel record needs an integer")),
                }
            };
            panels.push(Panel {
                place: u16::try_from(int(0)?)
                    .map_err(|_| Error::Malformed("skill panel place past 0xffff"))?,
                tree: u8::try_from(int(1)?)
                    .map_err(|_| Error::Malformed("skill tree past 0xff"))?,
                cost: u8::try_from(int(2)?)
                    .map_err(|_| Error::Malformed("skill cost past 0xff"))?,
                action: u16::try_from(int(3)?)
                    .map_err(|_| Error::Malformed("skill action past 0xffff"))?,
                effect: u8::try_from(int(4)?)
                    .map_err(|_| Error::Malformed("skill effect past 0xff"))?,
                amount: u16::try_from(int(5)?)
                    .map_err(|_| Error::Malformed("skill amount past 0xffff"))?,
                field_action: u16::try_from(int(6)?)
                    .map_err(|_| Error::Malformed("skill field action past 0xffff"))?,
                order: u16::try_from(int(7)?)
                    .map_err(|_| Error::Malformed("skill order past 0xffff"))?,
                message: u8::try_from(int(8)?)
                    .map_err(|_| Error::Malformed("skill message past 0xff"))?,
            });
        }
        Ok(Self { panels })
    }

    /// The panel with id `place`.
    pub fn at(&self, place: u16) -> Option<&Panel> {
        self.panels.iter().find(|p| p.place == place)
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
    fn panels_decode() {
        let data = table(&[
            record(0x65, &[0]),
            record(0x64, &[20]),
            record(0x66, &[0, 1, 3, 63, 1, 0, 0, 0, 1]),
            record(0x66, &[286, 0, 0, 168, 1, 0, 0, 286, 0]),
        ]);
        let skills = SkillTable::parse(&data).unwrap();
        assert_eq!(skills.panels.len(), 2);
        assert_eq!(
            skills.at(0),
            Some(&Panel {
                place: 0,
                tree: 1,
                cost: 3,
                action: 63,
                effect: 1,
                amount: 0,
                field_action: 0,
                order: 0,
                message: 1,
            })
        );
        assert_eq!(skills.at(286).unwrap().tree, 0);
    }

    #[test]
    fn wrong_value_count_is_an_error() {
        let data = table(&[record(0x66, &[0, 1, 3, 63, 1, 0, 0, 0])]);
        assert!(SkillTable::parse(&data).is_err());
    }

    #[test]
    fn values_past_their_field_are_an_error() {
        let data = table(&[record(0x66, &[0, 300, 3, 63, 1, 0, 0, 0, 1])]);
        assert!(SkillTable::parse(&data).is_err());
    }

    #[test]
    fn truncated_table_is_an_error() {
        let mut data = table(&[record(0x66, &[0, 1, 3, 63, 1, 0, 0, 0, 1])]);
        data.truncate(data.len() - 1);
        assert!(SkillTable::parse(&data).is_err());
    }
}
