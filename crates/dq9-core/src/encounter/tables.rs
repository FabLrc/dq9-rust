//! The data the roaming monsters are built from: `encfld.bin` (zones of a map), `encbtl.bin`
//! (company of a zone), `fld_mondata.bin` (a monster's field behaviour), the monster's body in
//! its name record, and the map's `P` file (the spawn graph, [`super::graph`]).
//!
//! Layouts are the game's in-memory ones as the table handlers build them (`docs/re/encounter.md`).

use dq9_formats::battle::encounter::Field;
use dq9_formats::battle::monster::Names;
use dq9_formats::table::{Table, Value};
use nds_rom::{Error, Result};

/// A monster of a zone (`encfld` `0x67`, `func_0209ba80`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoamerDef {
    /// Bits 0–11 the monster's number, 12–14 its weight among the zone's.
    pub word: u32,
    /// The size the monster is spawned at, `fx32` (4096 = 1.0): the second value, a float times
    /// 4096 or an integer shifted by 12, **1.0 when 0** (`0x0209ba80`).
    pub scale: i16,
}

impl RoamerDef {
    pub const fn monster(&self) -> u16 {
        (self.word & 0xfff) as u16
    }

    /// Bits 12–14 (`(word & 0x7fff) >> 12`).
    pub const fn weight(&self) -> u32 {
        (self.word & 0x7fff) >> 12
    }
}

/// A zone of a map (`encfld` `0x68` and its `0x66` word).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneDef {
    pub id: u16,
    pub word: u32,
    pub roamers: Vec<RoamerDef>,
}

impl ZoneDef {
    /// Bits 0–2: 0 roams when it is not night, 1 only at night, 2 at all hours
    /// (`0x02073ed4`, `0x0209bdd8`).
    pub const fn kind(&self) -> u32 {
        self.word & 7
    }

    /// Bits 10–12: most monsters in a battle fought there (`0x0209afe4`; 5 with no zone).
    pub const fn party_limit(&self) -> i32 {
        ((self.word & 0x1fff) >> 10) as i32
    }

    /// Bits 13–20: the region mask it is taken with (`0` any, else it must meet the mask of the
    /// spawn node or of the ground).
    pub const fn mask(&self) -> u32 {
        (self.word & 0x1f_ffff) >> 13
    }

    /// Bits 21–24 (`(word & 0x1ffffff) >> 21`): the zone waits `(7 − n)` seconds since the last
    /// spawn (`0x020733e8`).
    pub const fn delay(&self) -> i32 {
        ((self.word & 0x1ff_ffff) >> 21) as i32
    }
}

/// One map's zones (a `0x69` group).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapZones {
    pub map: u16,
    pub zones: Vec<ZoneDef>,
}

/// A roamer of `encbtl`'s `0x66` (`0x0209b8e4`, `0x021b8a34`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleRoamer {
    /// Bits 0–11 the monster, 12–14 and 15–17 the least and the most of it, 18–20 / 21–23 /
    /// 24–26 the weights of a battle with one, two or three kinds.
    pub word: u32,
    /// The second value's bits 8–15: the chance in a hundred of a scripted company.
    pub chance: u8,
    /// Its bits 0–7: the number of that company in `eventbattle.bin`.
    pub preset: u8,
}

/// A zone of `encbtl.bin`: its roamers and the company words (at most six of each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleZone {
    pub id: u16,
    pub roamers: Vec<BattleRoamer>,
    /// Bits 0–11 the monster, 12–14 its weight, 15–17 the least and 18–20 the most.
    pub company: Vec<u32>,
}

/// What `fld_mondata.bin` and the name record say of a roaming monster
/// (`func_0206ed54`, `func_020795f8`, `func_020771fc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonsterField {
    pub number: u16,
    /// Value 1 as the byte the game keeps (`+2`): the run-away test compares it with 99.
    pub level: u8,
    /// Value 2 as a signed byte (`+3`); **not read by any roaming code found**.
    pub margin: i8,
    /// Packed word, bits 0–2: how it wanders (0 stays, 1 3 4 5 walk between nodes, 2 blinks
    /// between them). Forced to 0 when the walk class is 0.
    pub wander: u8,
    /// Bits 3–5: walk speed class (0 none, 1 `0x9a`, 2 `0xe6`, 3 `0x1c2` per tick).
    pub walk: u8,
    /// Bits 6–8: how it notices the party (0 never, 1 and 2 ahead within 3.5 or 7.5 units, 3
    /// and 4 all round within 3.5 or 7.5).
    pub vision: u8,
    /// Bits 9–11: what a monster that would lose does (0 startles, 1 runs, 2 blinks away, 3
    /// either).
    pub flee: u8,
    /// Bits 12–14: what one that would win does (0 dashes, 1 chases, 2 flees).
    pub attack: u8,
    /// Value 4 × 4096 as a `s16` (`+8`): half of it times the model scale is the monster's
    /// touch radius. **Not a speed.**
    pub size: i16,
    pub attack_power: u16,
    pub defence_power: u16,
    /// The name record's collision radius times four (`fx32`) and height.
    pub radius: i32,
    pub height: i32,
}

/// Everything [`super::Roamers`] reads from the ROM tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncounterTables {
    pub fields: Vec<MapZones>,
    pub battle: Vec<BattleZone>,
    pub monsters: Vec<MonsterField>,
}

fn int(value: &Value<'_>) -> Result<u32> {
    match value {
        Value::Int(v) => Ok(*v),
        Value::Float(f) => Ok(*f as i32 as u32),
        Value::Str(_) => Err(Error::Malformed("a string where a number is expected")),
    }
}

impl EncounterTables {
    /// Reads the three tables and the monsters' bodies.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table.
    pub fn parse(
        encfld: &[u8],
        encbtl: &[u8],
        fld_mondata: &[u8],
        names: &Names<'_>,
    ) -> Result<Self> {
        let field = Field::parse(encfld)?;
        let fields = field
            .maps
            .iter()
            .map(|map| MapZones {
                map: map.map,
                zones: map
                    .zones
                    .iter()
                    .map(|zone| ZoneDef {
                        id: zone.id,
                        word: zone.word,
                        roamers: zone
                            .roamers
                            .iter()
                            .map(|r| RoamerDef {
                                word: r.word,
                                scale: roamer_scale(&r.second),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        let mut battle: Vec<BattleZone> = Vec::new();
        for record in &Table::parse(encbtl)?.records {
            match record.kind {
                0x68 => battle.push(BattleZone {
                    id: u16::try_from(int(&record.values[0])?)
                        .map_err(|_| Error::Malformed("zone id past 0xffff"))?,
                    roamers: Vec::new(),
                    company: Vec::new(),
                }),
                0x66 | 0x67 => {
                    let zone = battle
                        .last_mut()
                        .ok_or(Error::Malformed("encbtl word before its zone"))?;
                    let word = int(&record.values[0])?;
                    if record.kind == 0x66 {
                        let second = record.values.get(1).map(int).transpose()?.unwrap_or(0);
                        zone.roamers.push(BattleRoamer {
                            word,
                            chance: ((second >> 8) & 0xff) as u8,
                            preset: (second & 0xff) as u8,
                        });
                    } else {
                        zone.company.push(word);
                    }
                }
                _ => {}
            }
        }
        let mut monsters = Vec::new();
        for record in &Table::parse(fld_mondata)?.records {
            if record.kind != 0x65 || record.values.len() < 7 {
                continue;
            }
            let number = u16::try_from(int(&record.values[0])?)
                .map_err(|_| Error::Malformed("monster number past 0xffff"))?;
            let mut packed = int(&record.values[3])?;
            if (packed & 0x3f) >> 3 == 0 {
                packed &= !7;
            }
            let body = names.find(number);
            monsters.push(MonsterField {
                number,
                level: int(&record.values[1])? as u8,
                margin: int(&record.values[2])? as i8,
                wander: (packed & 7) as u8,
                walk: ((packed & 0x3f) >> 3) as u8,
                vision: ((packed & 0x1ff) >> 6) as u8,
                flee: ((packed & 0xfff) >> 9) as u8,
                attack: ((packed & 0x7fff) >> 12) as u8,
                size: (record.values[4].float() * 4096.0) as i32 as i16,
                attack_power: int(&record.values[5])? as u16,
                defence_power: int(&record.values[6])? as u16,
                radius: body.map_or(0, |b| i32::from(b.radius) << 2),
                height: body.map_or(0, |b| i32::from(b.height)),
            });
        }
        Ok(Self {
            fields,
            battle,
            monsters,
        })
    }

    /// The zones of `map`, the first group naming it (`0x0209bb7c`; the story range the group
    /// can carry is 0 on all 210).
    pub fn zones(&self, map: u16) -> Option<&MapZones> {
        self.fields.iter().find(|m| m.map == map)
    }

    pub fn battle_zone(&self, id: u16) -> Option<&BattleZone> {
        self.battle.iter().find(|z| z.id == id)
    }

    pub fn monster(&self, number: u16) -> Option<&MonsterField> {
        self.monsters.iter().find(|m| m.number == number)
    }
}

/// `func_0209ba80`: a float second value is multiplied by 4096, an integer shifted by 12; the
/// result is a `s16` and 0 means 1.0.
fn roamer_scale(second: &Value<'_>) -> i16 {
    let scale = match second {
        Value::Float(f) => (f * 4096.0) as i32 as i16,
        Value::Int(v) => ((*v as i32) << 12) as i16,
        Value::Str(_) => 0,
    };
    if scale == 0 { 0x1000 } else { scale }
}
