//! Map list (`data/map/maplist9.bin`): the game's index of every map, a record table read
//! into an array of 16-byte map records (`GameState+0x468`). Layout and evidence in
//! `docs/re/maplist.md`.
//!
//! port: 0x020995c0, 0x020995c8, 0x020995d0, 0x02099608, 0x02099898, 0x020998a0, 0x02099960,
//! port: 0x02099a00

use crate::table::{Table, Value};
use nds_rom::{Error, Result};

/// Path of the map list in the ROM filesystem (`Zone3D` constructor, `data_020ef0f0`).
pub const PATH: &str = "data/map/maplist9.bin";

/// Record kind giving the capacity of the list (`0x020995d0`).
const KIND_COUNT: u16 = 0x66;
/// Record kind of one map (`0x02099608`).
const KIND_MAP: u16 = 0x67;
/// Values read from a map record (slots 0 to 21).
const MAP_FIELDS: usize = 22;
/// Map code bytes kept by the game (`+5..+0xb`, NUL-terminated at `+0xb`).
pub const CODE_LEN: usize = 6;

/// One map, as the game packs it (`0x02099608`, appended by `0x02099a00`). Slot numbers
/// are the record's value indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapRecord<'a> {
    /// `+0`, slot 0: map id, how triggers, placements and doors name a map.
    pub id: u16,
    /// `+2` bits 0-14, slot 1: area shared by a place's maps (`Zone3D+4`).
    pub area: u16,
    /// `+4` bit 0, slot 21 **[unknown]**.
    pub flag_4: bool,
    /// `+5`, slot 4: map code naming the archives (`<code>.ambl`/`.amdj`), at most
    /// [`CODE_LEN`] bytes.
    pub code: &'a [u8],
    /// `+0xc`: bits 0-3 slot 3 (`Zone3D::SwitchZone` records the map id in the protagonist
    /// when it is 0 **[unknown]**), bit 4 slot 9 (copied to `func_0208a9b4()+0x9c` by
    /// `SwitchZone` **[unknown]**), bits 5-6 slot 10, bit 7 slot 12 **[unknown]**.
    pub byte_c: u8,
    /// `+0xd`: bits 0-6 slot 6, mapped by `0x0209cb00` through a 47-entry table
    /// **[hypothesis: music]**; bit 7 slot 16 **[unknown]**.
    pub byte_d: u8,
    /// `+0xe`: bits 0-1 slot 17 (space: 1 indoors, 2 outdoors), bits 2-5 slot 18, bit 6
    /// slot 19, bit 7 slot 20 **[unknown]**.
    pub byte_e: u8,
    /// Slot 2, read by the handler but not kept: region name (developer text).
    pub region: Option<&'a [u8]>,
    /// Slot 5, read but not kept: the map's label (developer text).
    pub label: Option<&'a [u8]>,
}

impl MapRecord<'_> {
    /// Slot 3 (`+0xc` bits 0-3).
    pub fn kind(&self) -> u8 {
        self.byte_c & 0xf
    }

    /// Slot 6 (`+0xd` bits 0-6).
    pub fn music(&self) -> u8 {
        self.byte_d & 0x7f
    }

    /// Slot 17 (`+0xe` bits 0-1): 1 indoors, 2 outdoors, 0 neither.
    pub fn space(&self) -> u8 {
        self.byte_e & 3
    }

    /// The record's 16 bytes as the game stores them, except bytes it leaves undefined, 0
    /// here: `+4` bits 1-7 and `+0xf` (uninitialized stack/heap), and code bytes after its
    /// terminator (the game copies 7 bytes from the string pool).
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0; 16];
        b[0..2].copy_from_slice(&self.id.to_le_bytes());
        b[2..4].copy_from_slice(&self.area.to_le_bytes());
        b[4] = u8::from(self.flag_4);
        b[5..5 + self.code.len()].copy_from_slice(self.code);
        b[0xc] = self.byte_c;
        b[0xd] = self.byte_d;
        b[0xe] = self.byte_e;
        b
    }
}

/// The map list (`GameState+0x468`: records, count, capacity).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapList<'a> {
    pub records: Vec<MapRecord<'a>>,
}

impl<'a> MapList<'a> {
    /// Runs the table's records through the game's handlers (`0x020998d4`): kind 0x66 sets
    /// the capacity, each kind 0x67 with a code appends a map while there is room; kinds
    /// 0x64, 0x65, 0x68 and 0x69 are no-ops.
    pub fn parse(table: &Table<'a>) -> Result<Self> {
        let mut capacity = None;
        let mut records = Vec::new();
        for record in &table.records {
            match record.kind {
                KIND_COUNT => {
                    if capacity.is_some() {
                        return Err(Error::Malformed("map list with two count records"));
                    }
                    let count = record
                        .values
                        .first()
                        .ok_or(Error::Malformed("map list count record without value"))?
                        .int();
                    capacity = Some(count as usize);
                }
                KIND_MAP => {
                    let v = &record.values;
                    if v.len() < MAP_FIELDS {
                        return Err(Error::Malformed("map record with fewer than 22 values"));
                    }
                    // Tbl_FieldStr: no pointer for an absent string or a number.
                    let str = |i: usize| match v[i] {
                        Value::Str(s) => s,
                        _ => None,
                    };
                    let Some(code) = str(4) else { continue };
                    if records.len() >= capacity.unwrap_or(0) {
                        continue;
                    }
                    let int = |i: usize| v[i].int() as u8;
                    records.push(MapRecord {
                        id: v[0].int() as u16,
                        area: v[1].int() as u16 & 0x7fff,
                        flag_4: int(21) & 1 != 0,
                        code: &code[..code.len().min(CODE_LEN)],
                        byte_c: int(3) & 0xf
                            | (int(9) & 1) << 4
                            | (int(10) & 3) << 5
                            | int(12) << 7,
                        byte_d: int(6) & 0x7f | int(16) << 7,
                        byte_e: int(17) & 3
                            | (int(18) & 0xf) << 2
                            | (int(19) & 1) << 6
                            | int(20) << 7,
                        region: str(2),
                        label: str(5),
                    });
                }
                _ => {}
            }
        }
        Ok(Self { records })
    }

    /// The first map with this id (`0x02099960`). Maps that do not ship all have id 0.
    pub fn find(&self, id: u16) -> Option<&MapRecord<'a>> {
        self.records.iter().find(|r| r.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Record;

    fn map(values: Vec<Value<'static>>) -> Record<'static> {
        Record {
            kind: KIND_MAP,
            values,
        }
    }

    fn values(
        id: u32,
        code: &'static [u8],
        f: impl Fn(&mut Vec<Value<'static>>),
    ) -> Vec<Value<'static>> {
        let mut v = vec![Value::Int(0); MAP_FIELDS];
        v[0] = Value::Int(id);
        v[4] = Value::Str(Some(code));
        f(&mut v);
        v
    }

    fn table(records: Vec<Record<'static>>) -> Table<'static> {
        Table {
            records,
            string_count: 0,
        }
    }

    #[test]
    fn fields_are_packed_like_the_game() {
        let v = values(109, b"C01M05", |v| {
            v[1] = Value::Int(0x80c6);
            v[3] = Value::Int(0x12);
            v[6] = Value::Float(14.7);
            v[9] = Value::Int(3);
            v[10] = Value::Int(6);
            v[12] = Value::Int(1);
            v[16] = Value::Int(1);
            v[17] = Value::Int(2);
            v[18] = Value::Int(0x15);
            v[20] = Value::Int(1);
            v[21] = Value::Int(1);
        });
        let t = table(vec![
            Record {
                kind: KIND_COUNT,
                values: vec![Value::Int(4)],
            },
            map(v),
        ]);
        let list = MapList::parse(&t).unwrap();
        let r = list.find(109).unwrap();
        assert_eq!(r.area, 0xc6);
        assert_eq!((r.kind(), r.music(), r.space()), (2, 14, 2));
        assert_eq!(
            r.to_bytes(),
            [
                109, 0, 0xc6, 0, 1, b'C', b'0', b'1', b'M', b'0', b'5', 0, 0xd2, 0x8e, 0x96, 0
            ]
        );
    }

    #[test]
    fn capacity_and_missing_codes_drop_maps() {
        let mut no_code = values(2, b"", |_| {});
        no_code[4] = Value::Int(7);
        let t = table(vec![
            map(values(1, b"M01", |_| {})),
            Record {
                kind: KIND_COUNT,
                values: vec![Value::Int(2)],
            },
            map(no_code),
            map(values(3, b"LONGCODE", |_| {})),
            map(values(0, b"Z01", |_| {})),
            map(values(0, b"Z02", |_| {})),
            map(values(5, b"M05", |_| {})),
        ]);
        let list = MapList::parse(&t).unwrap();
        // Map 1 comes before the capacity (0), map 2 has no code, map 5 finds the list full.
        let ids: Vec<_> = list.records.iter().map(|r| r.id).collect();
        assert_eq!(ids, [3, 0]);
        assert_eq!(list.records[0].code, b"LONGCO");
        assert_eq!(list.find(0).unwrap().code, b"Z01");
        assert!(list.find(5).is_none());
    }
}
