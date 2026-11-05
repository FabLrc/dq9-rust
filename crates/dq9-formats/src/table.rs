//! Level-5 record tables: the container of most `.bin` files (dialogue, menus, parameters,
//! maps), `.bmmp` and `.bcfg`. Layout and evidence in `docs/re/table.md`.
//!
//! port: 0x02030744, 0x02030784, 0x020307e0, 0x02030b1c, 0x02030b8c

use nds_rom::{Error, Result};

/// Offset value meaning "no string".
const NO_STRING: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    /// Raw bytes of a NUL-terminated string from the pool (ASCII with `<tag>` markup;
    /// some debug strings are Shift-JIS). `None` for an absent string.
    Str(Option<&'a [u8]>),
    Int(u32),
    Float(f32),
}

impl Value<'_> {
    /// Integer view of a field (`Tbl_FieldInt`, `0x02030b1c`): floats are converted
    /// (`0x0200c5fc`, [assumed truncating, like the ARM EABI helper]), strings read as 0.
    pub fn int(&self) -> u32 {
        match *self {
            Value::Int(v) => v,
            Value::Float(f) => f as i32 as u32,
            Value::Str(_) => 0,
        }
    }

    /// Float view of a field (`Script::Parameter::ToFloat`): integers converted, strings 0.
    pub fn float(&self) -> f32 {
        match *self {
            Value::Int(v) => v as i32 as f32,
            Value::Float(f) => f,
            Value::Str(_) => 0.0,
        }
    }

    /// String view of a field (`Tbl_FieldStr`, `0x02030b8c`): `None` unless a string.
    pub fn str(&self) -> Option<&[u8]> {
        match *self {
            Value::Str(s) => s,
            _ => None,
        }
    }
}

/// Most fields a record passes to its handler (`Tbl_Next` stops decoding at `0x80`).
pub const MAX_FIELDS: usize = 0x80;

/// The field buffer of `Tbl_Next` (`0x020307e0`), for handlers that read past their record:
/// each record overwrites only its own fields, so a short record's missing fields keep the
/// values of earlier records of the same run. The buffer is never cleared (`Tbl_Init`,
/// `0x02030644`): a field no record of the run has written is stack garbage in the game;
/// here it reads as integer 0 and [`Fields::read_unset`] reports it.
#[derive(Debug, Clone)]
pub struct Fields<'a> {
    values: [Option<Value<'a>>; MAX_FIELDS],
    count: usize,
    read_unset: core::cell::Cell<bool>,
}

impl Default for Fields<'_> {
    fn default() -> Self {
        Self {
            values: [None; MAX_FIELDS],
            count: 0,
            read_unset: core::cell::Cell::new(false),
        }
    }
}

impl<'a> Fields<'a> {
    /// Decodes `record`'s fields over the previous ones, as `Tbl_Next` does before calling the
    /// record's handler.
    pub fn load(&mut self, record: &Record<'a>) {
        self.count = record.values.len().min(MAX_FIELDS);
        for (slot, &v) in self.values.iter_mut().zip(&record.values) {
            *slot = Some(v);
        }
    }

    /// Field count of the current record, as passed to the handler.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Field `i` of the buffer (stale past [`Fields::count`]).
    pub fn get(&self, i: usize) -> Value<'a> {
        match self.values.get(i).copied().flatten() {
            Some(v) => v,
            None => {
                self.read_unset.set(true);
                Value::Int(0)
            }
        }
    }

    /// `Tbl_FieldInt` of field `i`.
    pub fn int(&self, i: usize) -> i32 {
        self.get(i).int() as i32
    }

    /// `Script::Parameter::ToFloat` of field `i`.
    pub fn float(&self, i: usize) -> f32 {
        self.get(i).float()
    }

    /// `Tbl_FieldStr` of field `i`.
    pub fn str(&self, i: usize) -> Option<&'a [u8]> {
        match self.get(i) {
            Value::Str(s) => s,
            _ => None,
        }
    }

    /// Whether a handler read a field that no record of the run had written.
    pub fn read_unset(&self) -> bool {
        self.read_unset.get()
    }
}

/// `4096 * x` truncated toward zero, the game's float to `fx32` conversion (`_fmul` then
/// `_ffix`, as in `Script::Parameter::ToVec3fix`).
pub fn fx32(x: f32) -> i32 {
    (4096.0 * x) as i32
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record<'a> {
    /// Record type; its meaning depends on the file (e.g. 1/2 in dialogue files).
    pub kind: u16,
    pub values: Vec<Value<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table<'a> {
    pub records: Vec<Record<'a>>,
    /// Number of distinct strings in the pool (header word 3).
    pub string_count: u32,
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Error::OutOfBounds {
            offset,
            len: 4,
            size: data.len(),
        })
}

impl<'a> Table<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let count = u32_at(data, 0)? as usize;
        let pool_offset = u32_at(data, 4)? as usize;
        let pool_size = u32_at(data, 8)? as usize;
        let string_count = u32_at(data, 12)?;
        let pool_end = pool_offset
            .checked_add(pool_size)
            .filter(|&end| end <= data.len() && data.len() - end < 16)
            .ok_or(Error::Malformed("table string pool does not end the file"))?;
        let pool = &data[pool_offset..pool_end];

        let mut pos = 0x10;
        let mut records = Vec::with_capacity(count.min(data.len() / 4));
        for _ in 0..count {
            let header = data
                .get(pos..pos + 3)
                .ok_or(Error::Malformed("table record header past end"))?;
            let kind = u16::from_le_bytes([header[0], header[1]]);
            let fields = usize::from(header[2]);
            // One type byte per 4 fields (2 bits each, LSB first), padded with 0xFF to 4 bytes.
            let type_bytes = fields.div_ceil(4);
            let header_len = (3 + type_bytes).next_multiple_of(4);
            let types = data
                .get(pos + 3..pos + 3 + type_bytes)
                .ok_or(Error::Malformed("table record types past end"))?;
            let padding = data
                .get(pos + 3 + type_bytes..pos + header_len)
                .ok_or(Error::Malformed("table record header past end"))?;
            if padding.iter().any(|&b| b != 0xFF) {
                return Err(Error::Malformed("table record header padding"));
            }
            let values = (0..fields)
                .map(|i| {
                    let raw = u32_at(data, pos + header_len + i * 4)?;
                    match types[i / 4] >> (2 * (i % 4)) & 3 {
                        0 => string(pool, raw).map(Value::Str),
                        1 => Ok(Value::Int(raw)),
                        2 => Ok(Value::Float(f32::from_bits(raw))),
                        _ => Err(Error::Malformed("table value type 3")),
                    }
                })
                .collect::<Result<_>>()?;
            records.push(Record { kind, values });
            pos += header_len + fields * 4;
        }
        let padding = data
            .get(pos..pool_offset)
            .ok_or(Error::Malformed("table records overlap the string pool"))?;
        if padding.iter().any(|&b| b != 0xFF) {
            return Err(Error::Malformed("table padding before string pool"));
        }
        Ok(Self {
            records,
            string_count,
        })
    }
}

fn string(pool: &[u8], offset: u32) -> Result<Option<&[u8]>> {
    if offset == NO_STRING {
        return Ok(None);
    }
    let start = offset as usize;
    // Offsets always point at the start of a string.
    if start >= pool.len() || (start > 0 && pool[start - 1] != 0) {
        return Err(Error::Malformed("table string offset"));
    }
    let len = pool[start..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(Error::Malformed("unterminated table string"))?;
    Ok(Some(&pool[start..start + len]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Record header for `fields` fields of the given 2-bit types.
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

    #[test]
    fn decodes_typed_values_and_multi_byte_type_headers() {
        let data = table(
            &[
                record(1, &[1, 1, 1, 0], &[7, 99, 0xD0, 0]),
                // 5 fields: two type bytes, padded with one 0xFF.
                record(2, &[1, 1, 2, 1, 0], &[1, 2, 1.5f32.to_bits(), 4, 3]),
                record(3, &[0], &[NO_STRING]),
            ],
            b"Hi\0Yo!\0",
            2,
        );
        let t = Table::parse(&data).unwrap();
        assert_eq!(t.string_count, 2);
        assert_eq!(t.records[0].values[3], Value::Str(Some(b"Hi")));
        assert_eq!(t.records[1].values[2], Value::Float(1.5));
        assert_eq!(t.records[1].values[4], Value::Str(Some(b"Yo!")));
        assert_eq!(t.records[2].values[0], Value::Str(None));
    }

    #[test]
    fn rejects_offset_inside_a_string() {
        let data = table(&[record(1, &[0], &[1])], b"Hi\0", 1);
        assert!(Table::parse(&data).is_err());
    }

    #[test]
    fn rejects_records_running_into_pool() {
        let mut data = table(&[record(1, &[1], &[5])], b"x\0", 1);
        data[0] = 50; // claims 50 records
        assert!(Table::parse(&data).is_err());
    }

    #[test]
    fn rejects_non_table_bytes() {
        assert!(Table::parse(b"NARC\xfe\xff\0\x01").is_err());
        assert!(Table::parse(&[0xFF; 64]).is_err());
    }
}
