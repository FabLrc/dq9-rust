//! Which sequence of `bgm.sdat` plays on a map, as the game's `func_0209cd50` (`0x0209cd60`)
//! decides it: the map list's music slot through a 47-byte table (`0x020e8ee8`) with the time
//! of day's variants, then the overrides of `data/bin/mapbgm.bin` that a story flag enables.
//! Evidence and the parts not read in `docs/re/sdat.md`.
//!
//! The result is an index of the SDAT sequence list (the sound manager's loader
//! `0x0209c3c4` hands it to the SDK's loader unchanged); 0 or less means no music.
//!
//! port: 0x0209cc4c

use crate::maplist::MapList;
use crate::table::Table;
use nds_rom::{Error, Result};

/// Path of the overrides in the ROM filesystem.
pub const PATH: &str = "data/bin/mapbgm.bin";

/// Map list music slot (0 to 46, `+0xd` bits 0-6) to a sequence id; `-1` means none
/// (`0x020e8ee8`, read on the European ARM9). Mostly the identity.
pub const SLOT_TABLE: [i8; 47] = [
    -1, 1, 2, 3, 4, 5, 6, 7, 8, -1, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, -1,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, -1, 40, 41, -1, 43, 44, 45, 46,
];

/// Record kind giving the number of overrides.
const KIND_COUNT: u16 = 1;
/// Record kind of one override.
const KIND_OVERRIDE: u16 = 2;

/// One entry of `mapbgm.bin`, a packed word (`func_0209cc3c`, `0x0209cc4c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Override {
    /// Bits 0-15: the map id it applies to.
    pub map: u16,
    /// Bits 16-21: the sequence that replaces the map's own (0 silences it).
    pub sequence: u8,
    /// Bits 22-30: the story flag that must be set (`Flags_Test` on the flag bank at
    /// `+0x8c`).
    pub flag: u16,
    /// Bit 31: applies only at time of day 0 (`LightingManager+0x98`, the night).
    pub night_only: bool,
}

impl Override {
    fn from_word(word: u32) -> Self {
        Self {
            map: word as u16,
            sequence: (word >> 16) as u8 & 0x3f,
            flag: ((word & 0x7fff_ffff) >> 22) as u16,
            night_only: word >> 31 != 0,
        }
    }
}

/// The overrides of `mapbgm.bin`, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub entries: Vec<Override>,
}

impl Overrides {
    /// Reads the table: a count record, then one record per entry. Entries past the count
    /// are dropped **[hypothesis: the loader's capacity works like the map list's]**.
    pub fn parse(table: &Table<'_>) -> Result<Self> {
        let mut capacity = None;
        let mut entries = Vec::new();
        for record in &table.records {
            match record.kind {
                KIND_COUNT => {
                    let count = record
                        .values
                        .first()
                        .ok_or(Error::Malformed("mapbgm count record without value"))?
                        .int();
                    capacity = Some(count as usize);
                }
                KIND_OVERRIDE => {
                    let word = record
                        .values
                        .first()
                        .ok_or(Error::Malformed("mapbgm entry without value"))?
                        .int();
                    if entries.len() < capacity.unwrap_or(usize::MAX) {
                        entries.push(Override::from_word(word));
                    }
                }
                _ => {}
            }
        }
        Ok(Self { entries })
    }

    /// The first override that applies to `map` (`func_0209cc3c`): `flag_set` tells whether
    /// a story flag is set and `time` is the time of day (0 night, 1 morning, 2 day,
    /// 3 evening). `Some(0)` silences the map.
    pub fn find(&self, map: u16, time: u32, flag_set: impl Fn(u16) -> bool) -> Option<u8> {
        self.entries
            .iter()
            .find(|e| e.map == map && flag_set(e.flag) && (!e.night_only || time == 0))
            .map(|e| e.sequence)
    }
}

/// The time-of-day variant of a music slot (`0x0209cb00`): a village's and a town's tracks
/// (3 and 5) become 4 and 6 at night. `None` for a slot past the table or one it leaves
/// empty.
///
/// **[unknown]** The game also turns slot 14 (the field regions) into 16 while a global flag
/// is set (`0x0202ae28`/`0x0202b7e8`); what it stands for is not read, so the field keeps
/// 14 here.
pub fn slot_sequence(slot: u8, time: u32) -> Option<u8> {
    let id = *SLOT_TABLE.get(usize::from(slot))?;
    let id = match (time, id) {
        (0, 3) => 4,
        (0, 5) => 6,
        _ => id,
    };
    u8::try_from(id).ok().filter(|&id| id > 0)
}

/// The sequence that plays on map `map` (`0x0209cd60`), or `None` for silence. The game's
/// special case for the 5900 maps (their id replaced by 1900 by two fields of the zone
/// object, `0x0209cd94`) is not modelled **[unknown]**.
pub fn sequence_for_map(
    list: &MapList<'_>,
    overrides: &Overrides,
    map: u16,
    time: u32,
    flag_set: impl Fn(u16) -> bool,
) -> Option<u8> {
    let own = list.find(map).and_then(|r| slot_sequence(r.music(), time));
    match overrides.find(map, time, flag_set) {
        Some(0) => None,
        Some(over) => Some(over),
        None => own,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Record, Value};

    fn entry(word: u32) -> Record<'static> {
        Record {
            kind: KIND_OVERRIDE,
            values: vec![Value::Int(word)],
        }
    }

    #[test]
    fn override_words_unpack_like_the_game() {
        // Map 1906: sequence 4 from flag 26, at night only.
        let o = Override::from_word(0x8684_0772);
        assert_eq!(
            o,
            Override {
                map: 1906,
                sequence: 4,
                flag: 26,
                night_only: true
            }
        );
    }

    #[test]
    fn overrides_need_their_flag_and_time() {
        let table = Table {
            records: vec![
                Record {
                    kind: KIND_COUNT,
                    values: vec![Value::Int(2)],
                },
                entry(0x8684_0772),
                entry(0x0683_0772),
                entry(0x0001_0001), // past the count
            ],
            string_count: 0,
        };
        let over = Overrides::parse(&table).unwrap();
        assert_eq!(over.entries.len(), 2);
        // Flag 26 off: nothing applies; on: the night entry at night, the other by day.
        assert_eq!(over.find(1906, 0, |_| false), None);
        assert_eq!(over.find(1906, 0, |f| f == 26), Some(4));
        assert_eq!(over.find(1906, 2, |f| f == 26), Some(3));
        assert_eq!(over.find(1907, 0, |_| true), None);
    }

    #[test]
    fn slots_follow_the_table_and_the_night() {
        assert_eq!(slot_sequence(5, 2), Some(5));
        assert_eq!(slot_sequence(5, 0), Some(6));
        assert_eq!(slot_sequence(3, 0), Some(4));
        assert_eq!(slot_sequence(14, 0), Some(14));
        assert_eq!(slot_sequence(0, 2), None);
        assert_eq!(slot_sequence(9, 2), None);
        assert_eq!(slot_sequence(47, 2), None);
    }
}
