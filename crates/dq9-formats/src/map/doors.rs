//! Region records of a map's `.bmbl`: map-changing warps (`0x72`) and typed zones (`0x73`
//! followed by `0x74`: doorways, areas). Layout, handler addresses and evidence in
//! `docs/re/map-archive.md` ("Doors, zones and map changes") and `docs/re/trigger.md`
//! ("Zones"). This module parses the records the way their handlers store them (fixed
//! point, like the game); the presence tests and what entering a region does are in
//! `dq9_core::field`.
//!
//! Warps are `0x0201ce80` (`0x72`, tail `0x0201cb70`), zones `0x0201d530` (`0x73`) and
//! `0x0201d638` (`0x74`, dispatched on the zone's type).

use super::reduce_angle;
use crate::maplist::MapRecord;
use crate::table::{Table, Value, fx32};
use nds_rom::{Error, Result};

/// `FIX32_MULTIPLY` of `a` by itself, as the handlers square half sizes
/// (`(a * a + 0x800) >> 12`).
fn square(a: i32) -> i32 {
    ((i64::from(a) * i64::from(a) + 0x800) >> 12) as i32
}

/// Squared ground radius the handlers store for quick refusal: `(width/2)² + (depth/2)²`
/// from the full fixed-point `width` and `depth`.
fn radius_sq(width: i32, depth: i32) -> i32 {
    square(width / 2).wrapping_add(square(depth / 2))
}

/// Box of a `0x72` warp as `0x0201ce80` stores it (all fixed point, 4096 = 1.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpVolume {
    /// `center + size / 2` (struct `+0x4`): upper corner.
    pub high: [i32; 3],
    /// `center - size / 2` (`+0x10`): lower corner.
    pub low: [i32; 3],
    pub center: [i32; 3],
    /// Rotation about Y, radians as `fx32`, **not** reduced (`+0x52`).
    pub angle: i16,
    /// `(width/2)² + (depth/2)²` (`+0x54`).
    pub radius_sq: i32,
}

/// Box of a `0x73` zone as `0x0201d530` stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneVolume {
    pub center: [i32; 3],
    /// Full width, height and depth (`+0x14`). A depth of 0 or less makes the zone a
    /// cylinder (`0x02094bac`).
    pub size: [i32; 3],
    /// Box rotation about Y, reduced to `[0, 2π]` (`+0x22`).
    pub angle: i16,
    /// The one more angle (`+0x20`, not reduced): the direction the doorway faces.
    pub facing: i16,
    pub radius_sq: i32,
}

/// A map-changing `0x72` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warp {
    pub volume: WarpVolume,
    /// Value 7 (struct `+0`): 1 is a door whose approach is animated, 0 changes map at once
    /// (`0x0219ca88`). **[hypothesis]** for the exact meaning of 1.
    pub kind: i32,
    /// Map id the code of value 8 names.
    pub destination: u16,
    /// Arrival position (values 11-13) and orientation (value 14, radians as `fx32`).
    pub arrival: [i32; 3],
    pub arrival_angle: i16,
    /// Value 15 (`+0x6c`), -1 for most doors **[unknown]**.
    pub entry: i32,
}

/// What a zone does when it holds a destination (flag `0x8` of a type 2 zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneWarp {
    pub destination: u16,
    /// Arrival position (values 7-9) and orientation (value 10, radians as `fx32`).
    pub arrival: [i32; 3],
    pub arrival_angle: i16,
}

/// A `0x73` zone and the `0x74` record that completes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    /// Value 0 of the `0x73`: 2 is a doorway, 3 an area of the map. Other types (0, 1,
    /// 4 to 12) are kept without their action.
    pub kind: u32,
    pub volume: ZoneVolume,
    /// `+0x2c`: first value of the action; the number of an area (type 3).
    pub class: u8,
    /// `+0x2d`: second value of a doorway's action, its number.
    pub number: u8,
    /// Value 2 of a doorway's action (12 bits at `+0x2e`): `0x8` has a destination, `0x10`
    /// tests the side the hero stands on rather than where he looks; `0x1` and `0x4` are
    /// runtime bits (no record has them).
    pub flags: u16,
    /// Value 3 of the doorway's `0x74` record, low quartet: the sound the door plays
    /// (`g_DoorSeTable`, effects 200-204) and, for other kinds, unused.
    pub sound: u8,
    pub warp: Option<ZoneWarp>,
}

/// Zone type of a doorway.
pub const ZONE_DOORWAY: u32 = 2;
/// Zone type of an area of the map.
pub const ZONE_AREA: u32 = 3;
/// Doorway flag: the action holds a destination.
pub const FLAG_WARP: u16 = 0x8;
/// Doorway flag: the side the hero stands on decides, not the direction he looks or walks.
pub const FLAG_SIDE: u16 = 0x10;

/// Every region of a map, in the order of the file. The game keeps the zones of one type in
/// a list **newest first** (`0x0201e710`); `0x72` records are an array in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Regions {
    pub warps: Vec<Warp>,
    pub zones: Vec<Zone>,
}

impl Regions {
    /// Zones of one type in the game's list order (newest first).
    pub fn zones_of(&self, kind: u32) -> impl Iterator<Item = &Zone> {
        self.zones.iter().rev().filter(move |z| z.kind == kind)
    }
}

fn number(value: &Value<'_>) -> Result<f32> {
    let number = match value {
        Value::Int(i) => *i as i32 as f32,
        Value::Float(f) => *f,
        Value::Str(_) => return Err(Error::Malformed("region field is not numeric")),
    };
    if !number.is_finite() {
        return Err(Error::Malformed("region field is not finite"));
    }
    Ok(number)
}

/// `Script::Parameter::ToVec3fix`: three fields as `fx32`.
fn vector(values: &[Value<'_>], start: usize) -> Result<[i32; 3]> {
    let value = |i: usize| number(&values[start + i]).map(fx32);
    Ok([value(0)?, value(1)?, value(2)?])
}

/// `fx32` of one field (`ToFloat`, `* 4096`, `_ffix`), kept in 16 bits like the `strh`.
fn angle(value: &Value<'_>) -> Result<i16> {
    Ok(fx32(number(value)?) as i16)
}

/// The destination a code names: the map list is searched by code (`0x0209999c`), an
/// integer is a map id. `None` when no map answers: the game's handler fails there; the
/// record is dropped here.
fn destination(value: &Value<'_>, maps: &[MapRecord<'_>]) -> Option<u16> {
    match value {
        Value::Str(code) => {
            let code = (*code)?;
            maps.iter()
                .find(|map| map.id != 0 && map.code.eq_ignore_ascii_case(code))
                .map(|map| map.id)
        }
        Value::Int(id) => maps
            .iter()
            .find(|map| map.id != 0 && u32::from(map.id) == *id)
            .map(|map| map.id),
        Value::Float(_) => None,
    }
}

/// `0x0201ce80`: the box of a `0x72` record (values 0-6).
fn warp_volume(values: &[Value<'_>]) -> Result<WarpVolume> {
    let center = vector(values, 0)?;
    let size = vector(values, 3)?;
    let half = size.map(|s| s / 2);
    Ok(WarpVolume {
        high: [0, 1, 2].map(|i| center[i] + half[i]),
        low: [0, 1, 2].map(|i| center[i] - half[i]),
        center,
        angle: angle(&values[6])?,
        radius_sq: radius_sq(size[0], size[2]),
    })
}

/// `0x0201d530`: a `0x73` record (type, center, size, box angle, facing).
fn zone(values: &[Value<'_>]) -> Result<Zone> {
    let center = vector(values, 1)?;
    let size = vector(values, 4)?;
    Ok(Zone {
        kind: values[0].int(),
        volume: ZoneVolume {
            center,
            size,
            angle: reduce_angle(fx32(number(&values[7])?)) as i16,
            facing: values.get(8).map(angle).transpose()?.unwrap_or(0),
            radius_sq: radius_sq(size[0], size[2]),
        },
        class: 0,
        number: 0,
        flags: 0,
        sound: 0,
        warp: None,
    })
}

/// `0x0201d638`: completes `zone` from its `0x74` record, by the zone's type. Type 3 only
/// takes its number; type 2 takes class, number, flags and, with [`FLAG_WARP`], the
/// destination and arrival.
fn complete(zone: &mut Zone, values: &[Value<'_>], maps: &[MapRecord<'_>]) -> Result<()> {
    match zone.kind {
        ZONE_AREA => zone.class = values.first().map_or(0, Value::int) as u8,
        ZONE_DOORWAY => {
            if values.len() < 3 {
                return Err(Error::Malformed("doorway action with fewer than 3 values"));
            }
            zone.class = values[0].int() as u8;
            zone.number = values[1].int() as u8;
            zone.flags = values[2].int() as u16 & 0xfff;
            zone.sound = values.get(3).map_or(0, |value| value.int() as u8 & 0xf);
            zone.warp = None;
            // A short record with the flag would read fields of an earlier record in the game
            // (`Fields`); it has no destination here.
            if zone.flags & FLAG_WARP != 0
                && values.len() >= 11
                && let Some(destination) = destination(&values[4], maps)
            {
                zone.warp = Some(ZoneWarp {
                    destination,
                    arrival: vector(values, 7)?,
                    arrival_angle: angle(&values[10])?,
                });
            }
        }
        _ => {}
    }
    Ok(())
}

/// Reads the `0x72` records and the `0x73`/`0x74` pairs of a `.bmbl` table. A `0x74` completes
/// the last zone read, even after other records (the handler keeps a global pointer to it).
/// Records whose destination no map answers are dropped.
pub fn parse(table: &Table<'_>, maps: &[MapRecord<'_>]) -> Result<Regions> {
    let mut regions = Regions::default();
    for record in &table.records {
        let values = &record.values;
        match record.kind {
            0x72 => {
                if values.len() < 15 {
                    return Err(Error::Malformed("warp record with fewer than 15 values"));
                }
                let Some(destination) = destination(&values[8], maps) else {
                    continue;
                };
                regions.warps.push(Warp {
                    volume: warp_volume(values)?,
                    kind: values[7].int() as i32,
                    destination,
                    arrival: vector(values, 11)?,
                    arrival_angle: angle(&values[14])?,
                    entry: values.get(15).map_or(-1, |v| v.int() as i32),
                });
            }
            0x73 => {
                if values.len() < 8 {
                    return Err(Error::Malformed("zone record with fewer than 8 values"));
                }
                regions.zones.push(zone(values)?);
            }
            0x74 => {
                if let Some(last) = regions.zones.last_mut() {
                    complete(last, values, maps)?;
                }
            }
            _ => {}
        }
    }
    Ok(regions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Record;

    fn maps() -> Vec<MapRecord<'static>> {
        vec![MapRecord {
            id: 109,
            area: 198,
            flag_4: false,
            code: b"C01M09",
            byte_c: 0,
            byte_d: 0,
            byte_e: 0,
            region: None,
            label: None,
        }]
    }

    fn table(records: Vec<Record<'static>>) -> Table<'static> {
        Table {
            records,
            string_count: 0,
        }
    }

    fn floats(values: &[f32]) -> Vec<Value<'static>> {
        values.iter().map(|&v| Value::Float(v)).collect()
    }

    fn warp72(destination: Option<&'static [u8]>) -> Record<'static> {
        let mut values = floats(&[1.0; 25]);
        values[0..7].copy_from_slice(&floats(&[2.0, 3.0, 4.0, 0.8, 2.0, 1.2, 0.5]));
        values[7] = Value::Int(1);
        values[8] = Value::Str(destination);
        values[11..15].copy_from_slice(&floats(&[10.0, 11.0, 12.0, 1.5]));
        Record { kind: 0x72, values }
    }

    fn zone73(kind: u32) -> Record<'static> {
        let mut values = vec![Value::Int(kind)];
        values.extend(floats(&[2.0, 3.0, 4.0, 0.8, 2.0, 1.2, 0.25, 1.0]));
        Record { kind: 0x73, values }
    }

    fn doorway74(flags: u32, destination: Option<&'static [u8]>) -> Record<'static> {
        let mut values = vec![
            Value::Int(0),
            Value::Int(9),
            Value::Int(flags),
            Value::Int(0),
            Value::Str(destination),
            Value::Int(u32::MAX),
            Value::Int(u32::MAX),
        ];
        values.extend(floats(&[10.0, 11.0, 12.0, 1.5]));
        Record { kind: 0x74, values }
    }

    #[test]
    fn warp_box_is_center_plus_and_minus_half_the_size() {
        let regions = parse(&table(vec![warp72(Some(b"C01M09"))]), &maps()).unwrap();
        let warp = &regions.warps[0];
        assert_eq!(warp.destination, 109);
        assert_eq!(warp.kind, 1);
        let volume = warp.volume;
        assert_eq!(volume.center, [8192, 12288, 16384]);
        // 0.8 → 3276 (truncated), half 1638; 2.0 → 8192, half 4096; 1.2 → 4915, half 2457.
        assert_eq!(volume.high, [8192 + 1638, 12288 + 4096, 16384 + 2457]);
        assert_eq!(volume.low, [8192 - 1638, 12288 - 4096, 16384 - 2457]);
        assert_eq!(volume.angle, 2048);
        assert_eq!(volume.radius_sq, square(1638) + square(2457));
        assert_eq!(warp.arrival, [40960, 45056, 49152]);
        assert_eq!(warp.arrival_angle, 6144);
    }

    #[test]
    fn warps_without_a_known_destination_are_dropped() {
        let regions = parse(&table(vec![warp72(None), warp72(Some(b"NOPE"))]), &maps()).unwrap();
        assert!(regions.warps.is_empty());
    }

    #[test]
    fn a_doorway_takes_its_action_and_destination() {
        let records = vec![zone73(2), doorway74(10, Some(b"C01M09"))];
        let regions = parse(&table(records), &maps()).unwrap();
        let door = &regions.zones[0];
        assert_eq!(
            (door.kind, door.class, door.number, door.flags),
            (2, 0, 9, 10)
        );
        // The box angle is reduced, the facing is not.
        assert_eq!((door.volume.angle, door.volume.facing), (1024, 4096));
        assert_eq!(
            door.warp,
            Some(ZoneWarp {
                destination: 109,
                arrival: [40960, 45056, 49152],
                arrival_angle: 6144,
            })
        );
    }

    #[test]
    fn a_doorway_without_the_warp_flag_has_no_destination() {
        let records = vec![zone73(2), doorway74(2, Some(b"C01M09"))];
        let regions = parse(&table(records), &maps()).unwrap();
        assert_eq!(regions.zones[0].warp, None);
    }

    #[test]
    fn an_area_takes_its_number_from_the_first_value() {
        let area = Record {
            kind: 0x74,
            values: vec![Value::Int(7)],
        };
        let regions = parse(&table(vec![zone73(3), area]), &maps()).unwrap();
        assert_eq!((regions.zones[0].kind, regions.zones[0].class), (3, 7));
    }

    #[test]
    fn an_action_completes_the_last_zone_even_after_other_records() {
        let other = Record {
            kind: 0x66,
            values: Vec::new(),
        };
        let area = Record {
            kind: 0x74,
            values: vec![Value::Int(4)],
        };
        let regions = parse(&table(vec![zone73(3), other, area]), &maps()).unwrap();
        assert_eq!(regions.zones[0].class, 4);
    }

    #[test]
    fn zones_come_newest_first_like_the_games_lists() {
        let a = Record {
            kind: 0x74,
            values: vec![Value::Int(1)],
        };
        let b = Record {
            kind: 0x74,
            values: vec![Value::Int(2)],
        };
        let regions = parse(&table(vec![zone73(3), a, zone73(3), b]), &maps()).unwrap();
        let numbers: Vec<_> = regions.zones_of(ZONE_AREA).map(|z| z.class).collect();
        assert_eq!(numbers, [2, 1]);
    }

    #[test]
    fn malformed_records_are_errors() {
        let short = Record {
            kind: 0x72,
            values: floats(&[1.0; 14]),
        };
        assert!(parse(&table(vec![short]), &maps()).is_err());
        let mut values = vec![Value::Int(3)];
        values.extend([Value::Str(None); 8]);
        let text = Record { kind: 0x73, values };
        assert!(parse(&table(vec![text]), &maps()).is_err());
    }
}
