//! Zone settings (`Zone3D+0xc`, 0x40 bytes): filled from the map list when the zone switches,
//! then from the `.dat` member of the `.ambl`, and the lighting file they name.
//!
//! port: 0x0201c734, 0x0201c60c, 0x0201c604, 0x0201c724, 0x0201c72c, 0x0205e114,
//! port: 0x0205e044, 0x0205e04c, 0x0205e06c, 0x0205e08c, 0x0205e094, 0x0205e0c0, 0x0205e0c8,
//! port: 0x0205e0d0, 0x0205e0d8, 0x0205e0e0, 0x0205e10c, 0x02014b14

use super::{DIR, c_str, field_str, fx32, is_grotto_floor, transform::reduce_angle};
use crate::table::Table;
use nds_rom::{Error, Result};

/// Map list record kind of one map (handled by `0x0201c60c` here).
const KIND_MAP: u16 = 0x67;
/// Values the map list handler reads (0 to 15).
const MAP_FIELDS: usize = 16;

/// `Zone3D+0xc..+0x4c` as the game fills it. Field offsets are from `Zone3D`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneInfo {
    /// `+0xc` (10 bytes): lighting name, map list value 11, replaced on grotto floors
    /// (`0x02014b14`); empty: no lighting file.
    pub lighting: Vec<u8>,
    /// `+0x16` (16 bytes): atmospheric effects, `.dat` kind `0x6d`; loaded as
    /// `data/map/<name>.mse` by `AtmosphericEffectSet::LoadArchive` (`0x0207a624`).
    pub effects: Vec<u8>,
    /// `+0x26` (16 bytes): `.dat` kind `0x68` (e.g. `C01`, `T00`) **[unknown]**.
    pub dat_name: Vec<u8>,
    /// `+0x36`: `.dat` kind `0x65`, 0x7fff by default **[unknown]**.
    pub field_36: u16,
    /// `+0x3c`: `.dat` kind `0x66`, 10 by default **[unknown]**.
    pub field_3c: u32,
    /// `+0x40`: map list value 13 (`fx32` radians, reduced to `[0, 2π]`)
    /// **[hypothesis: the map's orientation on the world map]**.
    pub world_angle: u16,
    /// `+0x44`, `+0x48`: map list values 14 and 15 (`fx32`)
    /// **[hypothesis: the map's position on the world map]**.
    pub world_position: [i32; 2],
}

impl Default for ZoneInfo {
    /// The values `Zone3D::SwitchZone` (`0x0201384c`) leaves before loading.
    fn default() -> Self {
        Self {
            lighting: Vec::new(),
            effects: Vec::new(),
            dat_name: Vec::new(),
            field_36: 0x7fff,
            field_3c: 10,
            world_angle: 0,
            world_position: [0; 2],
        }
    }
}

/// The lighting script of a zone: member `member` of archive `archive`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingFile {
    pub archive: String,
    pub member: String,
}

impl ZoneInfo {
    /// Runs the map list through the zone's handlers (`0x0201c734`): every record of kind
    /// `0x67` whose id is `id` sets the lighting name (when present) and the world placement
    /// (`0x0201c60c`); the other kinds (`0x66`, `0x68`, `0x69`) are no-ops.
    pub fn load_map_list(&mut self, table: &Table<'_>, id: u16) -> Result<()> {
        for record in table.records.iter().filter(|r| r.kind == KIND_MAP) {
            let v = &record.values;
            if v.first().is_none_or(|v| v.int() != u32::from(id)) {
                continue;
            }
            if v.len() < MAP_FIELDS {
                return Err(Error::Malformed("map record with fewer than 16 values"));
            }
            if let Some(name) = field_str(&v[11]) {
                self.lighting = name.to_vec();
            }
            self.world_angle = reduce_angle(fx32(&v[13])) as u16;
            self.world_position = [fx32(&v[14]), fx32(&v[15])];
        }
        Ok(())
    }

    /// Runs a `.dat` table through its handlers (`0x0205e114`, table `0x020f051c`): kind
    /// `0x65` sets [`field_36`](Self::field_36), `0x66` [`field_3c`](Self::field_3c), `0x68`
    /// [`dat_name`](Self::dat_name), `0x6d` [`effects`](Self::effects); kinds `0x64`, `0x67`
    /// and `0x69` to `0x6c`, `0x6e` are no-ops.
    pub fn load_dat(&mut self, table: &Table<'_>) -> Result<()> {
        for record in &table.records {
            let Some(first) = record.values.first() else {
                if matches!(record.kind, 0x65 | 0x66 | 0x68 | 0x6d) {
                    return Err(Error::Malformed(".dat record without value"));
                }
                continue;
            };
            match record.kind {
                0x65 => self.field_36 = first.int() as u16,
                0x66 => self.field_3c = first.int(),
                0x68 => {
                    if let Some(s) = field_str(first) {
                        self.dat_name = s.to_vec();
                    }
                }
                0x6d => {
                    if let Some(s) = field_str(first) {
                        self.effects = s.to_vec();
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The lighting file queued after the `.ambl` (`0x02014b14`). On a main grotto floor the
    /// name first becomes `Z0<environ>M01<nn>` from the floor (`id % 20`): `00` below 5,
    /// `01` below 9, `02` below 13, `03` below 17. An empty name loads nothing.
    pub fn lighting_file(&mut self, id: u16, grotto_environ: i32) -> Option<LightingFile> {
        if is_grotto_floor(id) {
            let env = if grotto_environ == 0 {
                1
            } else {
                grotto_environ
            };
            let part = match id % 20 {
                0..=4 => Some(0),
                5..=8 => Some(1),
                9..=12 => Some(2),
                13..=16 => Some(3),
                _ => None,
            };
            if let Some(part) = part {
                self.lighting = format!("Z0{env}M010{part}").into_bytes();
            }
        }
        let name = c_str(&self.lighting);
        let first = *name.first()?;
        let name = String::from_utf8_lossy(name);
        Some(LightingFile {
            archive: format!("{DIR}/ats_{}.ambl", char::from(first)),
            member: format!("{name}.bats"),
        })
    }

    /// `data/map/<effects>.mse`, queued when the models are loaded (`0x0201470c`).
    pub fn effects_file(&self) -> Option<String> {
        let name = c_str(&self.effects);
        (!name.is_empty()).then(|| format!("{DIR}/{}.mse", String::from_utf8_lossy(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Record, Value};

    fn map(id: u32, lighting: Option<&'static [u8]>, angle: f32) -> Record<'static> {
        let mut values = vec![Value::Int(0); 22];
        values[0] = Value::Int(id);
        values[11] = Value::Str(lighting);
        values[13] = Value::Float(angle);
        values[14] = Value::Float(-60.055);
        values[15] = Value::Int(2);
        Record {
            kind: KIND_MAP,
            values,
        }
    }

    fn table(records: Vec<Record<'static>>) -> Table<'static> {
        Table {
            records,
            string_count: 0,
        }
    }

    #[test]
    fn map_list_and_dat_fill_the_settings() {
        let mut zone = ZoneInfo::default();
        let t = table(vec![
            map(108, Some(b"X"), 0.0),
            map(109, Some(b"C01M0300"), -1.0),
            map(109, None, 1.0),
        ]);
        zone.load_map_list(&t, 109).unwrap();
        assert_eq!(zone.lighting, b"C01M0300");
        // The last record wins for the placement: 1.0 rad.
        assert_eq!(zone.world_angle, 4096);
        assert_eq!(zone.world_position, [-245985, 8192]);
        let dat = table(vec![
            Record {
                kind: 0x66,
                values: vec![Value::Int(21)],
            },
            Record {
                kind: 0x68,
                values: vec![Value::Str(Some(b"C01"))],
            },
            Record {
                kind: 0x6d,
                values: vec![Value::Str(Some(b"F17E1"))],
            },
            Record {
                kind: 0x69,
                values: vec![],
            },
        ]);
        zone.load_dat(&dat).unwrap();
        assert_eq!((zone.field_3c, zone.field_36), (21, 0x7fff));
        assert_eq!(zone.dat_name, b"C01");
        assert_eq!(zone.effects_file().unwrap(), "data/map/F17E1.mse");
        let light = zone.lighting_file(109, 0).unwrap();
        assert_eq!(light.archive, "data/map/ats_C.ambl");
        assert_eq!(light.member, "C01M0300.bats");
    }

    #[test]
    fn grotto_floors_pick_their_lighting() {
        let mut zone = ZoneInfo::default();
        assert_eq!(zone.lighting_file(109, 0), None);
        // 40013 % 20 = 13.
        let light = zone.lighting_file(40013, 0).unwrap();
        assert_eq!(light.archive, "data/map/ats_Z.ambl");
        assert_eq!(light.member, "Z01M0103.bats");
        // Floors 17 to 19 keep the name already there.
        assert_eq!(
            zone.lighting_file(40017, 4).unwrap().member,
            "Z01M0103.bats"
        );
    }
}
