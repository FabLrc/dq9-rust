//! Block list (`Zone3D+0x6c`): the `.bmbl` (and `.bpos`) records naming the `.bmdj`
//! manifests the game loads from the `.amdj`, each with an offset.
//!
//! The same table also carries other zone data (kinds `0x66` to `0x7e`, handlers at
//! `0x020ef388`), not read here; see `docs/re/map-archive.md`.
//!
//! port: 0x0201c7ac, 0x0201c7d4, 0x0201e26c, 0x0201e314, 0x0201e258, 0x0201e2e8

use super::{field_str, fx32};
use crate::table::Table;
use nds_rom::{Error, Result};

/// Record kind giving the capacity of the list (`0x0201c7ac`).
const KIND_COUNT: u16 = 0x64;
/// Record kind of one entry (`0x0201c7d4`).
const KIND_ENTRY: u16 = 0x65;
/// Size of each name field of an entry (`0x0201e314`).
const NAME_LEN: usize = 16;

/// One entry (0x58 bytes in the game).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntry {
    /// `+0`, value 0: copied into each block loaded for this entry.
    pub id: u32,
    /// `+4`, values 1-3 (`fx32`): copied into each block (`Block+0x48`).
    pub offset: [i32; 3],
    /// `+0x10`, value 4: every `.bmdj` member whose name contains it is loaded.
    pub name: Vec<u8>,
    /// `+0x20`, value 5 when present **[unknown]**.
    pub name2: Vec<u8>,
    /// `+0x30`, value 6 when present **[unknown]**.
    pub name3: Vec<u8>,
}

/// The list: entries and the capacity set by the last count record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockList {
    pub entries: Vec<BlockEntry>,
    pub capacity: usize,
}

impl BlockList {
    /// `0x0201e258`: the `.bmbl` handler clears the list (0x88 bytes) before reading it.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Reads the block records of a `.bmbl` or `.bpos` table (part of `0x0201e1e0`): a count
    /// record allocates a new list of that capacity (`0x0201e26c`), an entry with a name is
    /// appended while there is room (`0x0201e314`, `0x0201c7d4`).
    pub fn load(&mut self, table: &Table<'_>) -> Result<()> {
        for record in &table.records {
            let v = &record.values;
            match record.kind {
                KIND_COUNT => {
                    let count = v
                        .first()
                        .ok_or(Error::Malformed("block count record without value"))?;
                    self.entries.clear();
                    self.capacity = count.int() as usize;
                }
                KIND_ENTRY => {
                    if v.len() < 5 {
                        return Err(Error::Malformed("block entry with fewer than 5 values"));
                    }
                    let Some(name) = field_str(&v[4]) else {
                        continue;
                    };
                    let optional = |i: usize| v.get(i).and_then(field_str).unwrap_or_default();
                    let (name2, name3) = (optional(5), optional(6));
                    // `strcpy` into 16-byte fields: longer names would run into the next one.
                    if [name, name2, name3].iter().any(|n| n.len() >= NAME_LEN) {
                        return Err(Error::Malformed("block entry name longer than 15 bytes"));
                    }
                    if self.entries.len() < self.capacity {
                        self.entries.push(BlockEntry {
                            id: v[0].int(),
                            offset: [fx32(&v[1]), fx32(&v[2]), fx32(&v[3])],
                            name: name.to_vec(),
                            name2: name2.to_vec(),
                            name3: name3.to_vec(),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Record, Value};

    fn entry(id: u32, name: Option<&'static [u8]>) -> Record<'static> {
        Record {
            kind: KIND_ENTRY,
            values: vec![
                Value::Int(id),
                Value::Float(0.018),
                Value::Float(-0.037),
                Value::Int(1),
                Value::Str(name),
            ],
        }
    }

    #[test]
    fn entries_fill_the_declared_capacity() {
        let t = Table {
            records: vec![
                entry(9, Some(b"early")),
                Record {
                    kind: KIND_COUNT,
                    values: vec![Value::Int(2)],
                },
                entry(0, Some(b"F07M0000")),
                entry(1, None),
                entry(2, Some(b"B")),
                entry(3, Some(b"C")),
            ],
            string_count: 0,
        };
        let mut list = BlockList::default();
        list.load(&t).unwrap();
        let ids: Vec<_> = list.entries.iter().map(|e| e.id).collect();
        // Before the count record there is no room; nameless entries are skipped.
        assert_eq!(ids, [0, 2]);
        assert_eq!(list.entries[0].offset, [73, -151, 4096]);
        assert_eq!(list.entries[0].name, b"F07M0000");
        assert!(list.entries[0].name2.is_empty());
    }
}
