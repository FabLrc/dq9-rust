//! Battle tables: monsters, actions, spells, equipment battle parameters and field
//! encounters. Layouts and evidence in `docs/re/battle-data.md`; wiki pages cited there.
//!
//! Every reader works on borrowed bytes and returns [`nds_rom::Error`] on malformed input:
//! a truncated file, a record size that does not fit the count, a string offset that does not
//! start a string. None panics.
//!
//! Fields the wiki marks INFERRED or not established are carried raw (`u32`/`u16`/bytes)
//! with a doc comment saying so; established ones have typed names.

use nds_rom::bytes::{slice, u32_at};
use nds_rom::{Error, Result};

pub mod action;
pub mod encounter;
pub mod equipment;
pub mod monster;
pub mod skill;
pub mod spell;
pub mod text;

/// Low 12 bits of a `.nat` head word: the record count.
const HEAD_COUNT_MASK: u32 = 0xfff;
/// Bits 12 to 27 of a `.nat` head word: the string section's size.
const HEAD_STRINGS_SHIFT: u32 = 12;
const HEAD_STRINGS_MASK: u32 = 0xffff;

/// A `.nat` file: a head word, fixed-size records, then the string section.
///
/// The head word packs the record count in bits 0–11 and the string section's size in
/// bits 12–27 (`mon_list_en.nat` opens `0x01545159`: 345 records, 5,445 bytes of strings).
/// The top nibble is 0 on every file read here. The string section ends the file exactly on
/// the monster list, the battle data, the name records, `actname.nat` and both action tables,
/// which this reader checks. Wiki *Monsters*, *System Strings*, *Actions*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedRecords<'a> {
    /// Record count (head word bits 0–11).
    pub count: usize,
    pub record_size: usize,
    /// The records, `count * record_size` bytes.
    pub records: &'a [u8],
    /// The strings, `strings.len()` bytes as the head word declares.
    pub strings: &'a [u8],
}

impl<'a> FixedRecords<'a> {
    /// Splits `data` with `record_size`-byte records and a trailing string section.
    ///
    /// # Errors
    /// [`Error::Malformed`] when the count overflows the file, the string section runs past
    /// the end or does not end the file exactly.
    pub fn parse(data: &'a [u8], record_size: usize) -> Result<Self> {
        if record_size == 0 {
            return Err(Error::Malformed("record size is zero"));
        }
        let head = u32_at(data, 0)?;
        let count = (head & HEAD_COUNT_MASK) as usize;
        let strings_len = ((head >> HEAD_STRINGS_SHIFT) & HEAD_STRINGS_MASK) as usize;
        let records_len = count
            .checked_mul(record_size)
            .ok_or(Error::Malformed("record table too large"))?;
        let records = slice(data, 4, records_len)?;
        let strings = slice(data, 4 + records_len, strings_len)?;
        if 4 + records_len + strings_len != data.len() {
            return Err(Error::Malformed("string section does not end the file"));
        }
        Ok(Self {
            count,
            record_size,
            records,
            strings,
        })
    }

    /// Record `index`, `record_size` bytes.
    ///
    /// # Errors
    /// [`Error::OutOfBounds`] when `index` is past [`FixedRecords::count`].
    pub fn record(&self, index: usize) -> Result<&'a [u8]> {
        let offset = index
            .checked_mul(self.record_size)
            .ok_or(Error::Malformed("record offset overflow"))?;
        slice(self.records, offset, self.record_size)
    }

    /// The string at `offset`, without its NUL.
    ///
    /// # Errors
    /// [`Error::Malformed`] when the offset is past the pool, does not point at the start of
    /// a string, or the string is unterminated.
    pub fn string(&self, offset: u32) -> Result<&'a [u8]> {
        let start = offset as usize;
        if start >= self.strings.len() {
            return Err(Error::Malformed("string offset past the pool"));
        }
        if start > 0 && self.strings[start - 1] != 0 {
            return Err(Error::Malformed("string offset not at a string start"));
        }
        let len = self.strings[start..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::Malformed("unterminated string"))?;
        Ok(&self.strings[start..start + len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Head word, `count` records of `record_size` zero bytes, `strings`.
    fn nat(count: u32, record_size: usize, strings: &[u8]) -> Vec<u8> {
        let mut data = ((strings.len() as u32) << 12 | count)
            .to_le_bytes()
            .to_vec();
        data.resize(4 + count as usize * record_size, 0);
        data.extend_from_slice(strings);
        data
    }

    #[test]
    fn head_word_splits_count_and_strings() {
        let data = nat(2, 4, b"\0abc\0");
        let table = FixedRecords::parse(&data, 4).unwrap();
        assert_eq!(table.count, 2);
        assert_eq!(table.records.len(), 8);
        assert_eq!(table.strings, b"\0abc\0");
        assert_eq!(table.string(1).unwrap(), b"abc");
        assert_eq!(table.string(0).unwrap(), b"");
    }

    #[test]
    fn truncated_or_overlong_file_is_an_error() {
        let mut data = nat(2, 4, b"\0a\0");
        assert!(FixedRecords::parse(&data, 4).is_ok());
        data.pop();
        assert!(FixedRecords::parse(&data, 4).is_err());
        data.push(0);
        data.push(0);
        assert!(FixedRecords::parse(&data, 4).is_err());
    }

    #[test]
    fn string_offsets_are_checked() {
        let data = nat(1, 4, b"\0abc\0");
        let table = FixedRecords::parse(&data, 4).unwrap();
        assert!(table.string(2).is_err(), "inside a string");
        assert!(table.string(99).is_err(), "past the pool");
        assert!(table.record(1).is_err(), "past the record count");
    }
}
