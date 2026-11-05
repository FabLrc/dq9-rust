//! Map-to-map doorway records from `.bmbl` (`0x0201d530`, `0x0201d638`).

use crate::maplist::MapRecord;
use crate::table::{Table, Value};
use nds_rom::{Error, Result};

/// A rectangular doorway trigger in map coordinates. Height is retained as metadata; the
/// runtime entry check uses only the horizontal footprint.
#[derive(Debug, Clone, PartialEq)]
pub struct DoorVolume {
    pub center: [f32; 3],
    /// Full width, height and depth, not half-extents.
    pub size: [f32; 3],
    /// Rotation around Y in radians.
    pub angle: f32,
}

impl DoorVolume {
    /// Whether this record has a usable horizontal footprint.
    fn has_footprint(&self) -> bool {
        self.size[0] > 0.0 && self.size[2] > 0.0
    }

    /// Whether a point is inside the rotated XZ footprint, including its edges.
    pub fn contains(&self, point: [f32; 3]) -> bool {
        if !self.has_footprint() {
            return false;
        }
        let dx = point[0] - self.center[0];
        let dz = point[2] - self.center[2];
        let (sin, cos) = self.angle.sin_cos();
        let x = dx * cos + dz * sin;
        let z = -dx * sin + dz * cos;
        x.abs() <= self.size[0] * 0.5 && z.abs() <= self.size[2] * 0.5
    }
}

/// One map-changing doorway: trigger, destination map and arrival pose.
#[derive(Debug, Clone, PartialEq)]
pub struct Doorway {
    pub destination: u16,
    pub volume: DoorVolume,
    pub arrival: [f32; 3],
    pub facing: f32,
}

fn number(value: &Value<'_>) -> Result<f32> {
    let number = match value {
        Value::Int(i) => *i as i32 as f32,
        Value::Float(f) => *f,
        Value::Str(_) => return Err(Error::Malformed("doorway field is not numeric")),
    };
    if !number.is_finite() {
        return Err(Error::Malformed("doorway field is not finite"));
    }
    Ok(number)
}

fn vector(values: &[Value<'_>], start: usize) -> Result<[f32; 3]> {
    Ok([
        number(&values[start])?,
        number(&values[start + 1])?,
        number(&values[start + 2])?,
    ])
}

fn destination(value: &Value<'_>, maps: &[MapRecord<'_>]) -> Option<u16> {
    let code = value.str()?;
    maps.iter()
        .find(|map| map.id != 0 && map.code.eq_ignore_ascii_case(code))
        .map(|map| map.id)
}

fn volume(values: &[Value<'_>], start: usize) -> Result<DoorVolume> {
    let center = vector(values, start)?;
    let size = vector(values, start + 3)?;
    let angle = number(&values[start + 6])?;
    Ok(DoorVolume {
        center,
        size,
        angle,
    })
}

/// Reads live `0x72` door records and map-changing `0x73`/`0x74` pairs. Other zone records and
/// non-map-changing `0x74` actions remain outside this model.
pub fn parse(table: &Table<'_>, maps: &[MapRecord<'_>]) -> Result<Vec<Doorway>> {
    let mut doorways = Vec::new();
    let mut pending_region = None;
    for record in &table.records {
        let values = &record.values;
        match record.kind {
            0x72 => {
                pending_region = None;
                if values.len() < 15 {
                    return Err(Error::Malformed("door record with fewer than 15 values"));
                }
                let Some(destination) = destination(&values[8], maps) else {
                    continue;
                };
                let volume = volume(values, 0)?;
                if !volume.has_footprint() {
                    continue;
                }
                doorways.push(Doorway {
                    destination,
                    volume,
                    arrival: vector(values, 11)?,
                    facing: number(&values[14])?,
                });
            }
            0x73 => {
                pending_region = if values.len() >= 8 && values[0].int() == 2 {
                    let volume = volume(values, 1)?;
                    volume.has_footprint().then_some(volume)
                } else {
                    None
                };
            }
            0x74 => {
                if let Some(volume) = pending_region.take()
                    && values.len() >= 11
                    && let Some(destination) = destination(&values[4], maps)
                {
                    doorways.push(Doorway {
                        destination,
                        volume,
                        arrival: vector(values, 7)?,
                        facing: number(&values[10])?,
                    });
                }
            }
            _ => pending_region = None,
        }
    }
    Ok(doorways)
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

    fn door72(destination: Option<&'static [u8]>) -> Record<'static> {
        let mut values = vec![Value::Float(1.0); 25];
        values[0..7].copy_from_slice(&[
            Value::Float(2.0),
            Value::Float(3.0),
            Value::Float(4.0),
            Value::Float(0.8),
            Value::Float(2.0),
            Value::Float(1.2),
            Value::Float(0.0),
        ]);
        values[8] = Value::Str(destination);
        values[11..15].copy_from_slice(&[
            Value::Float(10.0),
            Value::Float(11.0),
            Value::Float(12.0),
            Value::Float(1.5),
        ]);
        Record { kind: 0x72, values }
    }

    #[test]
    fn parses_map_changing_72_records_and_ignores_missing_destinations() {
        let doors = parse(&table(vec![door72(Some(b"C01M09")), door72(None)]), &maps()).unwrap();
        assert_eq!(doors.len(), 1);
        assert_eq!(doors[0].destination, 109);
        assert_eq!(doors[0].volume.center, [2.0, 3.0, 4.0]);
        assert_eq!(doors[0].volume.size, [0.8, 2.0, 1.2]);
        assert_eq!(doors[0].arrival, [10.0, 11.0, 12.0]);
        assert_eq!(doors[0].facing, 1.5);
    }

    #[test]
    fn paired_regions_create_doors_only_for_type_two_and_known_maps() {
        let region = Record {
            kind: 0x73,
            values: vec![
                Value::Int(2),
                Value::Float(2.0),
                Value::Float(3.0),
                Value::Float(4.0),
                Value::Float(0.8),
                Value::Float(2.0),
                Value::Float(1.2),
                Value::Float(0.25),
            ],
        };
        let action = Record {
            kind: 0x74,
            values: vec![
                Value::Int(0),
                Value::Int(0),
                Value::Int(0),
                Value::Int(0),
                Value::Str(Some(b"C01M09")),
                Value::Int(0),
                Value::Int(0),
                Value::Float(10.0),
                Value::Float(11.0),
                Value::Float(12.0),
                Value::Float(1.5),
            ],
        };
        let doors = parse(&table(vec![region, action]), &maps()).unwrap();
        assert_eq!(doors.len(), 1);
        assert_eq!(doors[0].volume.angle, 0.25);
        assert_eq!(doors[0].arrival, [10.0, 11.0, 12.0]);
    }

    #[test]
    fn doorway_volume_uses_rotated_full_xz_sizes_and_ignores_height() {
        let volume = DoorVolume {
            center: [0.0, 0.0, 0.0],
            size: [2.0, 1.0, 4.0],
            angle: core::f32::consts::FRAC_PI_2,
        };
        assert!(volume.contains([1.9, 100.0, 0.0]));
        assert!(!volume.contains([0.0, 0.0, 1.1]));
    }
}
