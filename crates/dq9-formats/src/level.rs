//! Level tables: `data/prm/level0.bin` to `level12.bin`, one per vocation, giving the
//! experience reached, the base attributes, the maximum HP and MP and the skill points
//! gained at each level from 1 to 99 (wiki *Level-Tables*, `docs/re/party.md`).
//!
//! The thirteen files are tagged record tables ([`crate::table::Table`], the game's `Script`
//! container): 102 records, one kind `0x64`, one kind `0x65`, the ninety-nine level records
//! (kind `0x66`) and one kind `0x67`. A level record is eleven integers:
//!
//! | field | meaning |
//! |---:|---|
//! | 0 | experience the level is reached at (0 at level 1; the wiki infers this column) |
//! | 1 | strength |
//! | 2 | resilience |
//! | 3 | agility |
//! | 4 | deftness |
//! | 5 | charm |
//! | 6 | magical might |
//! | 7 | magical mending |
//! | 8 | maximum HP |
//! | 9 | maximum MP |
//! | 10 | skill points gained, all told (0 at level 1, 200 at 99 on twelve files) |
//!
//! The `0x64`, `0x65` and `0x67` records are kept raw: their meaning is not established.
//! The vocation order is the one the status screen's strings name and the Abbey's list
//! confirms (Guardian 0, Warrior 1 … Ranger 12); `level6`, the Minstrel's, is the one the
//! published guide checks. No I/O: the caller supplies the decompressed files.

use crate::table::{Table, Value};
use nds_rom::{Error, Result};

/// Number of vocations, and of level files.
pub const VOCATION_COUNT: usize = 13;
/// Level records in one file: levels 1 to 99.
pub const LEVEL_COUNT: usize = 99;
/// Size of each file on the cartridge.
pub const FILE_SIZE: usize = 5312;
/// Record kind of a level.
const LEVEL_TAG: u16 = 0x66;
/// Values in a level record.
const LEVEL_FIELDS: usize = 11;

/// The vocations in the files' order (`level<index>.bin`), from the status screen's strings.
pub const VOCATIONS: [&str; VOCATION_COUNT] = [
    "Guardian",
    "Warrior",
    "Priest",
    "Mage",
    "Martial Artist",
    "Thief",
    "Minstrel",
    "Gladiator",
    "Armamentalist",
    "Paladin",
    "Sage",
    "Luminary",
    "Ranger",
];

/// One level record: the values of kind `0x66`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelStat {
    /// Experienced reached at this level (column 0, the wiki's inference).
    pub experience: u32,
    pub strength: u16,
    pub resilience: u16,
    pub agility: u16,
    pub deftness: u16,
    pub charm: u16,
    pub magical_might: u16,
    pub magical_mending: u16,
    pub max_hp: u16,
    pub max_mp: u16,
    /// Skill points the character has gained by this level, all told.
    pub skill_points: u16,
}

/// A value of a record kept without interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawValue {
    /// The raw bits of an integer field.
    Int(u32),
    /// The raw bits of a float field.
    Float(u32),
    /// A string field's bytes (`None` for the "no string" offset).
    Str(Option<Vec<u8>>),
}

/// A record kept without interpretation (kinds `0x64`, `0x65`, `0x67`): the tag and its
/// values as they are encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub tag: u16,
    pub values: Vec<RawValue>,
}

/// One vocation's levels: `level<vocation>.bin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelTable {
    levels: [LevelStat; LEVEL_COUNT],
    other: Vec<RawRecord>,
}

impl LevelTable {
    /// Parses one level file.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut levels = Vec::with_capacity(LEVEL_COUNT);
        let mut other = Vec::new();
        for record in &table.records {
            if record.kind == LEVEL_TAG {
                levels.push(level_stat(&record.values)?);
            } else {
                other.push(RawRecord {
                    tag: record.kind,
                    values: record
                        .values
                        .iter()
                        .map(|v| match v {
                            Value::Int(raw) => RawValue::Int(*raw),
                            Value::Float(f) => RawValue::Float(f.to_bits()),
                            Value::Str(s) => RawValue::Str(s.map(<[u8]>::to_vec)),
                        })
                        .collect(),
                });
            }
        }
        let levels: [LevelStat; LEVEL_COUNT] = levels
            .try_into()
            .map_err(|_| Error::Malformed("level table does not hold 99 level records"))?;
        Ok(Self { levels, other })
    }

    /// The record of `level` (1 to 99); `None` outside that range.
    pub fn level(&self, level: u8) -> Option<&LevelStat> {
        usize::from(level)
            .checked_sub(1)
            .filter(|&i| i < LEVEL_COUNT)
            .map(|i| &self.levels[i])
    }

    /// The ninety-nine level records, level 1 first.
    pub fn levels(&self) -> &[LevelStat; LEVEL_COUNT] {
        &self.levels
    }

    /// The records whose meaning is not established.
    pub fn other(&self) -> &[RawRecord] {
        &self.other
    }
}

/// The thirteen level tables, indexed by vocation (`level0` … `level12`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelTables {
    tables: [LevelTable; VOCATION_COUNT],
}

impl LevelTables {
    /// Parses the thirteen files in vocation order (`data/prm/level0.bin` first).
    pub fn parse(files: [&[u8]; VOCATION_COUNT]) -> Result<Self> {
        let tables: Vec<LevelTable> = files
            .iter()
            .map(|data| LevelTable::parse(data))
            .collect::<Result<_>>()?;
        Ok(Self {
            tables: tables
                .try_into()
                .map_err(|_| Error::Malformed("level tables are not thirteen"))?,
        })
    }

    /// The table of `vocation` (0 Guardian … 12 Ranger); `None` outside that range.
    pub fn table(&self, vocation: u8) -> Option<&LevelTable> {
        self.tables.get(usize::from(vocation))
    }

    /// The record for `vocation` at `level` (1 to 99).
    pub fn stat(&self, vocation: u8, level: u8) -> Option<&LevelStat> {
        self.table(vocation)?.level(level)
    }
}

/// Reads a level record's eleven integers; every column but experience fits a halfword.
fn level_stat(values: &[Value<'_>]) -> Result<LevelStat> {
    if values.len() != LEVEL_FIELDS {
        return Err(Error::Malformed("level record is not eleven values"));
    }
    let mut ints = [0u32; LEVEL_FIELDS];
    for (out, value) in ints.iter_mut().zip(values) {
        let Value::Int(raw) = value else {
            return Err(Error::Malformed("level record holds a non-integer value"));
        };
        *out = *raw;
    }
    let half = |i: usize| {
        u16::try_from(ints[i]).map_err(|_| Error::Malformed("level record exceeds a halfword"))
    };
    Ok(LevelStat {
        experience: ints[0],
        strength: half(1)?,
        resilience: half(2)?,
        agility: half(3)?,
        deftness: half(4)?,
        charm: half(5)?,
        magical_might: half(6)?,
        magical_mending: half(7)?,
        max_hp: half(8)?,
        max_mp: half(9)?,
        skill_points: half(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One record of a tagged table: kind, field types (0 string, 1 integer, 2 float) and
    /// raw values.
    fn record(kind: u16, types: &[u8], values: &[u32]) -> Vec<u8> {
        let mut out = kind.to_le_bytes().to_vec();
        out.push(types.len() as u8);
        for chunk in types.chunks(4) {
            out.push(
                chunk
                    .iter()
                    .enumerate()
                    .fold(0, |acc, (i, t)| acc | t << (2 * i)),
            );
        }
        out.resize(out.len().next_multiple_of(4), 0xFF);
        values.iter().for_each(|v| out.extend(v.to_le_bytes()));
        out
    }

    /// The container of `table.rs`: header, records, optional `0xFF` padding, string pool.
    fn table(records: &[Vec<u8>], pool: &[u8], strings: u32) -> Vec<u8> {
        let body: Vec<u8> = records.concat();
        let pool_offset = (0x10 + body.len()).next_multiple_of(16);
        let mut out = Vec::new();
        for word in [records.len(), pool_offset, pool.len(), strings as usize] {
            out.extend((word as u32).to_le_bytes());
        }
        out.extend(body);
        out.resize(pool_offset, 0xFF);
        out.extend(pool);
        out.resize(out.len().next_multiple_of(16), 0);
        out
    }

    fn level(values: [u32; LEVEL_FIELDS]) -> Vec<u8> {
        record(LEVEL_TAG, &[1; LEVEL_FIELDS], &values)
    }

    fn levels(count: usize) -> Vec<Vec<u8>> {
        (1..=count)
            .map(|i| {
                level([
                    (i as u32) * 10,
                    i as u32,
                    i as u32 + 1,
                    i as u32 + 2,
                    i as u32 + 3,
                    i as u32 + 4,
                    i as u32 + 5,
                    i as u32 + 6,
                    100 + i as u32,
                    200 + i as u32,
                    i as u32,
                ])
            })
            .collect()
    }

    fn file(records: &[Vec<u8>]) -> Vec<u8> {
        let mut all = vec![record(0x65, &[1], &[0]), record(0x64, &[1], &[20])];
        all.extend_from_slice(records);
        all.push(record(0x67, &[1; 22], &[100; 22]));
        table(&all, b"2010/04/09 00:18:34\x00100203\x00", 2)
    }

    #[test]
    fn reads_all_ninety_nine_levels_and_keeps_the_other_records_raw() {
        let bytes = file(&levels(LEVEL_COUNT));
        assert_eq!(bytes.len(), 5312);
        let table = LevelTable::parse(&bytes).unwrap();
        assert_eq!(table.levels().len(), LEVEL_COUNT);
        assert_eq!(
            table.level(1),
            Some(&LevelStat {
                experience: 10,
                strength: 1,
                resilience: 2,
                agility: 3,
                deftness: 4,
                charm: 5,
                magical_might: 6,
                magical_mending: 7,
                max_hp: 101,
                max_mp: 201,
                skill_points: 1,
            })
        );
        assert_eq!(table.level(99).unwrap().experience, 990);
        assert_eq!(table.level(99).unwrap().skill_points, 99);
        assert_eq!(table.level(0), None);
        assert_eq!(table.level(100), None);
        assert_eq!(
            table.other().iter().map(|r| r.tag).collect::<Vec<_>>(),
            [0x65, 0x64, 0x67]
        );
        assert_eq!(table.other()[2].values.len(), 22);
    }

    #[test]
    fn rejects_a_file_without_ninety_nine_level_records() {
        let bytes = file(&levels(LEVEL_COUNT - 1));
        assert!(LevelTable::parse(&bytes).is_err());
    }

    #[test]
    fn rejects_a_level_record_that_is_not_eleven_integers() {
        let mut records = levels(LEVEL_COUNT);
        records[5] = record(LEVEL_TAG, &[1; LEVEL_FIELDS - 1], &[0; LEVEL_FIELDS - 1]);
        assert!(LevelTable::parse(&file(&records)).is_err());

        let mut records = levels(LEVEL_COUNT);
        let mut types = [1u8; LEVEL_FIELDS];
        types[4] = 2;
        records[5] = record(LEVEL_TAG, &types, &[0; LEVEL_FIELDS]);
        assert!(LevelTable::parse(&file(&records)).is_err());
    }

    #[test]
    fn rejects_a_column_that_does_not_fit_a_halfword() {
        let mut records = levels(LEVEL_COUNT);
        let mut values = [1u32; LEVEL_FIELDS];
        values[3] = 0x1_0000;
        records[0] = level(values);
        assert!(LevelTable::parse(&file(&records)).is_err());
    }

    #[test]
    fn thirteen_tables_are_indexed_by_vocation() {
        let files: [Vec<u8>; VOCATION_COUNT] = core::array::from_fn(|_| file(&levels(LEVEL_COUNT)));
        let files: [&[u8]; VOCATION_COUNT] = files.each_ref().map(Vec::as_slice);
        let tables = LevelTables::parse(files).unwrap();
        assert_eq!(VOCATIONS[6], "Minstrel");
        assert_eq!(tables.stat(6, 1).unwrap().strength, 1);
        assert_eq!(tables.stat(12, 99).unwrap().experience, 990);
        assert!(tables.table(13).is_none() && tables.stat(6, 100).is_none());
    }
}
