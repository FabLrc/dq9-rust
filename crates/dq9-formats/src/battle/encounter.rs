//! Field encounters: `encfld.bin` (by map, each map's zones and the monsters that roam
//! them), `encbtl.bin` (by zone, the same roamers and the monsters that may join a battle
//! there) and `fld_mondata.bin` (each monster's field numbers). Three
//! [tagged data tables](crate::table). Layout and evidence in `docs/re/battle-data.md`; wiki
//! *Encounters*.
//!
//! The map ids, zone membership and monster numbers are read; weights, join counts, zone
//! kinds, levels and run-away margins are INFERRED; how a map chooses among its zones is not
//! established. Fields the wiki leaves open are kept raw.

use nds_rom::{Error, Result};

use crate::table::{Table, Value};

/// Opens a map; value 0 is the map's id.
const KIND_MAP: u16 = 0x69;
/// Names a zone.
const KIND_ZONE: u16 = 0x68;
/// The zone's word (`encfld`) or one of its roamers (`encbtl`).
const KIND_WORD: u16 = 0x66;
/// A roamer (`encfld`) or a companion (`encbtl`).
const KIND_MONSTER: u16 = 0x67;
/// `fld_mondata`'s count record.
const KIND_COUNT: u16 = 0x64;
/// `fld_mondata`'s seven-value monster record.
const KIND_FIELD_MONSTER: u16 = 0x65;

fn int(value: &Value<'_>) -> Result<u32> {
    match value {
        Value::Int(v) => Ok(*v),
        _ => Err(Error::Malformed("encounter value is not an integer")),
    }
}

fn values(record: &crate::table::Record<'_>, count: usize) -> Result<()> {
    if record.values.len() != count {
        return Err(Error::Malformed(
            "encounter record with the wrong value count",
        ));
    }
    Ok(())
}

/// A monster roaming one of a map's zones, `encfld`'s `0x67`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldRoamer<'a> {
    /// Bits 0–11 of the record's word: the monster's number.
    pub monster: u16,
    /// Bits 12–14: its weight among the zone's, INFERRED.
    pub weight: u8,
    /// The record's second value: 1 on most records, a float on some (`F06`); not
    /// established.
    pub second: Value<'a>,
    /// The record's word, for the bits above 14 (not established).
    pub word: u32,
}

/// One zone of a map in `encfld.bin`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldZone<'a> {
    /// The zone's number (`0x68`).
    pub id: u16,
    /// The zone's word (`0x66`); its low three bits are [`kind`](FieldZone::kind), the rest
    /// is not established.
    pub word: u32,
    pub roamers: Vec<FieldRoamer<'a>>,
}

impl FieldZone<'_> {
    /// The word's low three bits: 0 and 1 a pair (only on fields), 2 a lone zone or a
    /// field's third and fourth (INFERRED).
    pub fn kind(&self) -> u8 {
        (self.word & 7) as u8
    }
}

/// One map of `encfld.bin`: its id and its zones. All 210 name a map of `maplist9.bin`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldMap<'a> {
    pub map: u16,
    pub zones: Vec<FieldZone<'a>>,
}

/// `encfld.bin`: the zones of every map that has encounters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Field<'a> {
    pub maps: Vec<FieldMap<'a>>,
}

impl<'a> Field<'a> {
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table, a zone before its map, a roamer before its
    /// zone or a record with the wrong value count.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut maps: Vec<FieldMap<'a>> = Vec::new();
        for record in &table.records {
            match record.kind {
                KIND_MAP => {
                    values(record, 5)?;
                    maps.push(FieldMap {
                        map: u16::try_from(int(&record.values[0])?)
                            .map_err(|_| Error::Malformed("map id past 0xffff"))?,
                        zones: Vec::new(),
                    });
                }
                KIND_ZONE => {
                    values(record, 1)?;
                    let map = maps
                        .last_mut()
                        .ok_or(Error::Malformed("zone before its map"))?;
                    map.zones.push(FieldZone {
                        id: u16::try_from(int(&record.values[0])?)
                            .map_err(|_| Error::Malformed("zone id past 0xffff"))?,
                        word: 0,
                        roamers: Vec::new(),
                    });
                }
                KIND_WORD => {
                    values(record, 1)?;
                    let zone = maps
                        .last_mut()
                        .and_then(|m| m.zones.last_mut())
                        .ok_or(Error::Malformed("zone word before its zone"))?;
                    zone.word = int(&record.values[0])?;
                }
                KIND_MONSTER => {
                    values(record, 2)?;
                    let zone = maps
                        .last_mut()
                        .and_then(|m| m.zones.last_mut())
                        .ok_or(Error::Malformed("roamer before its zone"))?;
                    let word = int(&record.values[0])?;
                    zone.roamers.push(FieldRoamer {
                        monster: (word & 0xfff) as u16,
                        weight: ((word >> 12) & 7) as u8,
                        second: record.values[1],
                        word,
                    });
                }
                // 0x64 and 0x65 open the file; not established.
                _ => {}
            }
        }
        Ok(Self { maps })
    }

    /// The encounters of map `id`.
    pub fn find(&self, id: u16) -> Option<&FieldMap<'a>> {
        self.maps.iter().find(|map| map.map == id)
    }
}

/// A monster that may join a battle, `encbtl`'s `0x67`: its word packs a weight among the
/// zone's company, then the least and the most of it that join (all INFERRED).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Companion {
    /// Bits 0–11: the monster's number.
    pub monster: u16,
    /// Bits 12–14: a weight among the zone's company; 0 on ten records, which never join.
    pub weight: u8,
    /// Bits 15–17: the least that join (1, 2 or 3).
    pub least: u8,
    /// Bits 18–20: the most that join (1 to 5).
    pub most: u8,
}

/// One roamer of `encbtl`'s `0x66`: the same three fields and more above them; what the upper
/// bits say is not established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneRoamer {
    /// Bits 0–11: the monster's number.
    pub monster: u16,
    /// Bits 12–14: a weight.
    pub weight: u8,
    /// Bits 15–17.
    pub least: u8,
    /// Bits 18–20.
    pub most: u8,
    /// Bits 21 and up: not established (the roamer's fields break least ≤ most on 31 of
    /// 1,058 records, all with large values here).
    pub upper: u32,
    /// The record's word.
    pub word: u32,
}

/// One zone of `encbtl.bin`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleZone {
    /// The zone's number (`0x68`): 290 of them, covering all of `encfld`'s.
    pub id: u16,
    pub roamers: Vec<ZoneRoamer>,
    pub company: Vec<Companion>,
}

/// `encbtl.bin`: by zone, the roamers again and the company that may join.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Battle {
    pub zones: Vec<BattleZone>,
}

impl Battle {
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table, a roamer or companion before its zone, or a
    /// record with the wrong value count.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut zones: Vec<BattleZone> = Vec::new();
        for record in &table.records {
            match record.kind {
                KIND_ZONE => {
                    values(record, 1)?;
                    zones.push(BattleZone {
                        id: u16::try_from(int(&record.values[0])?)
                            .map_err(|_| Error::Malformed("zone id past 0xffff"))?,
                        roamers: Vec::new(),
                        company: Vec::new(),
                    });
                }
                KIND_WORD => {
                    values(record, 2)?;
                    let zone = zones
                        .last_mut()
                        .ok_or(Error::Malformed("roamer before its zone"))?;
                    let word = int(&record.values[0])?;
                    zone.roamers.push(ZoneRoamer {
                        monster: (word & 0xfff) as u16,
                        weight: ((word >> 12) & 7) as u8,
                        least: ((word >> 15) & 7) as u8,
                        most: ((word >> 18) & 7) as u8,
                        upper: word >> 21,
                        word,
                    });
                }
                KIND_MONSTER => {
                    values(record, 1)?;
                    let zone = zones
                        .last_mut()
                        .ok_or(Error::Malformed("companion before its zone"))?;
                    let word = int(&record.values[0])?;
                    zone.company.push(Companion {
                        monster: (word & 0xfff) as u16,
                        weight: ((word >> 12) & 7) as u8,
                        least: ((word >> 15) & 7) as u8,
                        most: ((word >> 18) & 7) as u8,
                    });
                }
                // 0x64 and 0x65 open the file; not established.
                _ => {}
            }
        }
        Ok(Self { zones })
    }

    /// The battle records of zone `id`.
    pub fn find(&self, id: u16) -> Option<&BattleZone> {
        self.zones.iter().find(|zone| zone.id == id)
    }
}

/// One `fld_mondata.bin` record: a monster's field numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldMonster {
    /// Value 0: the monster's number.
    pub number: u16,
    /// Value 1: the monster's level, INFERRED (agrees with maximum HP's rank at 0.84).
    pub level: i32,
    /// Value 2: the margin by which the party's level must pass [`level`](Self::level) before
    /// it runs, INFERRED (99 never, −99 at once).
    pub run_margin: i32,
    /// Value 3: a packed word, not established.
    pub packed: u32,
    /// Value 4: a float, INFERRED a speed.
    pub speed: f32,
    /// Value 5: attack, equal to the battle data's on all 438.
    pub attack: u16,
    /// Value 6: defence, equal to the battle data's on all 438.
    pub defence: u16,
}

/// `fld_mondata.bin`: 438 field records.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldMonsters {
    pub monsters: Vec<FieldMonster>,
}

impl FieldMonsters {
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table, a value of the wrong type or a record count
    /// that disagrees with the records.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut declared = None;
        let mut monsters = Vec::new();
        for record in &table.records {
            match record.kind {
                KIND_COUNT => {
                    values(record, 1)?;
                    declared = Some(int(&record.values[0])? as usize);
                }
                KIND_FIELD_MONSTER => {
                    if declared.is_none() {
                        return Err(Error::Malformed("field monster before the count"));
                    }
                    values(record, 7)?;
                    let number = u16::try_from(int(&record.values[0])?)
                        .map_err(|_| Error::Malformed("monster number past 0xffff"))?;
                    let speed = match record.values[4] {
                        Value::Float(f) => f,
                        Value::Int(v) => v as i32 as f32,
                        Value::Str(_) => return Err(Error::Malformed("speed is not a number")),
                    };
                    monsters.push(FieldMonster {
                        number,
                        level: int(&record.values[1])? as i32,
                        run_margin: int(&record.values[2])? as i32,
                        packed: int(&record.values[3])?,
                        speed,
                        attack: u16::try_from(int(&record.values[5])?)
                            .map_err(|_| Error::Malformed("attack past 0xffff"))?,
                        defence: u16::try_from(int(&record.values[6])?)
                            .map_err(|_| Error::Malformed("defence past 0xffff"))?,
                    });
                }
                // 0x66 and 0x67 open the file; not established.
                _ => {}
            }
        }
        match declared {
            Some(count) if count == monsters.len() => Ok(Self { monsters }),
            Some(_) => Err(Error::Malformed("field monster count disagrees")),
            None => Err(Error::Malformed("field monsters without a count")),
        }
    }

    /// The field record of `number`.
    pub fn find(&self, number: u16) -> Option<&FieldMonster> {
        self.monsters.iter().find(|m| m.number == number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tagged record: kind, integer and float values as the game stores them.
    fn record(kind: u16, values: &[Value<'_>]) -> Vec<u8> {
        let mut out = kind.to_le_bytes().to_vec();
        out.push(values.len() as u8);
        let bytes = values.len().div_ceil(4);
        for i in 0..bytes {
            let group =
                values
                    .iter()
                    .skip(i * 4)
                    .take(4)
                    .enumerate()
                    .fold(0u8, |acc, (slot, value)| {
                        let kind = match value {
                            Value::Str(_) => 0u8,
                            Value::Int(_) => 1,
                            Value::Float(_) => 2,
                        };
                        acc | kind << (2 * slot)
                    });
            out.push(group);
        }
        out.resize(out.len().next_multiple_of(4), 0xFF);
        for value in values {
            let raw = match value {
                Value::Str(Some(s)) => s.len() as u32,
                Value::Str(None) => u32::MAX,
                Value::Int(v) => *v,
                Value::Float(f) => f.to_bits(),
            };
            out.extend_from_slice(&raw.to_le_bytes());
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
    fn field_encounters_decode() {
        let data = table(&[
            record(0x65, &[Value::Int(0)]),
            record(
                0x69,
                &[
                    Value::Int(20001),
                    Value::Int(0),
                    Value::Int(0),
                    Value::Int(0),
                    Value::Int(0),
                ],
            ),
            record(0x68, &[Value::Int(12)]),
            record(0x66, &[Value::Int(0xe0_2810)]),
            record(0x67, &[Value::Int(1 | 7 << 12), Value::Int(1)]),
            record(0x67, &[Value::Int(56 | 6 << 12), Value::Float(0.9)]),
        ]);
        let field = Field::parse(&data).unwrap();
        let map = field.find(20001).unwrap();
        assert_eq!(map.zones.len(), 1);
        let zone = &map.zones[0];
        assert_eq!(zone.id, 12);
        assert_eq!(zone.kind(), 0);
        assert_eq!(zone.roamers.len(), 2);
        assert_eq!((zone.roamers[0].monster, zone.roamers[0].weight), (1, 7));
        assert_eq!(zone.roamers[1].monster, 56);
        assert_eq!(zone.roamers[1].second, Value::Float(0.9));
    }

    #[test]
    fn a_roamer_before_its_zone_is_an_error() {
        let data = table(&[record(0x67, &[Value::Int(1), Value::Int(1)])]);
        assert!(Field::parse(&data).is_err());
        let data = table(&[
            record(
                0x69,
                &[
                    Value::Int(1),
                    Value::Int(0),
                    Value::Int(0),
                    Value::Int(0),
                    Value::Int(0),
                ],
            ),
            record(0x66, &[Value::Int(3)]),
        ]);
        assert!(Field::parse(&data).is_err());
    }

    #[test]
    fn battle_encounters_decode_and_check_least_most() {
        let data = table(&[
            record(0x68, &[Value::Int(12)]),
            record(
                0x66,
                &[Value::Int(1 | 7 << 12 | 1 << 15 | 2 << 18), Value::Int(0)],
            ),
            record(0x67, &[Value::Int(1 | 5 << 12 | 1 << 15 | 1 << 18)]),
        ]);
        let battle = Battle::parse(&data).unwrap();
        let zone = battle.find(12).unwrap();
        assert_eq!(
            zone.roamers[0],
            ZoneRoamer {
                monster: 1,
                weight: 7,
                least: 1,
                most: 2,
                upper: 0,
                word: 1 | 7 << 12 | 1 << 15 | 2 << 18,
            }
        );
        assert_eq!(
            zone.company[0],
            Companion {
                monster: 1,
                weight: 5,
                least: 1,
                most: 1
            }
        );
    }

    #[test]
    fn field_monsters_decode_and_the_count_is_checked() {
        let data = table(&[
            record(0x66, &[Value::Int(0)]),
            record(0x64, &[Value::Int(1)]),
            record(
                0x65,
                &[
                    Value::Int(1),
                    Value::Int(1),
                    Value::Int(5),
                    Value::Int(0x3c0251),
                    Value::Float(0.4),
                    Value::Int(10),
                    Value::Int(7),
                ],
            ),
        ]);
        let monsters = FieldMonsters::parse(&data).unwrap();
        let slime = monsters.find(1).unwrap();
        assert_eq!((slime.level, slime.run_margin), (1, 5));
        assert_eq!(slime.packed, 0x3c0251);
        assert_eq!(slime.speed, 0.4);
        assert_eq!((slime.attack, slime.defence), (10, 7));

        let data = table(&[record(0x64, &[Value::Int(2)])]);
        assert!(
            FieldMonsters::parse(&data).is_err(),
            "count without records"
        );
        let data = table(&[record(0x65, &[Value::Int(1)])]);
        assert!(FieldMonsters::parse(&data).is_err(), "record without count");
    }
}
