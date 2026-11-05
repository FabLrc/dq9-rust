//! `.bmdj` manifests: a record table listing a block's resources (models, collisions,
//! animated objects), its texture links and the placements of its resources. Read by
//! `0x0201f050` with the handlers at `0x020ef418` into a 0x3c-byte structure (`Block+4`).
//!
//! port: 0x0201f02c, 0x0201ebd4, 0x0201ec08, 0x0201ec10, 0x0201edd0, 0x0201edf8, 0x0201ee00,
//! port: 0x0201f000, 0x0201ece4, 0x0201f100, 0x0201f154, 0x0201f180, 0x0201f1c0, 0x0201f200,
//! port: 0x0201f224, 0x0201f2b4, 0x0201f318, 0x0201f378

use super::{field_str, fx32};
use crate::table::Table;
use nds_rom::{Error, Result};

/// Texture links a manifest can hold (`+0x18..+0x38`, count at `+0x38`).
const MAX_TEXTURES: usize = 8;

/// Bits of [`Resource::flags`] (value 2 of a resource record, masked with `0x3f`).
pub mod resource_flags {
    /// Load `<name>.nsbca` (skeletal animation).
    pub const NSBCA: u8 = 0x01;
    /// Load `<name>.nsbma` (material animation).
    pub const NSBMA: u8 = 0x02;
    /// Load `<name>.nsbta` (texture SRT animation).
    pub const NSBTA: u8 = 0x04;
    /// Load `<name>.nsbtp` (texture pattern animation).
    pub const NSBTP: u8 = 0x08;
    /// Animated object (`Object3D`): also `<name>.bcfg`, its animation package.
    pub const OBJECT: u8 = 0x10;
    /// Not tinted by the lighting (`LightingManager::ModelTransformTintBrightnessContrast`).
    pub const UNTINTED: u8 = 0x20;
}

/// A resource record (kind `0x6c`, 12 bytes in the game).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// `+0`, value 0: what placements refer to.
    pub id: i16,
    /// `+2`, value 2 & `0x3f` when present: [`resource_flags`].
    pub flags: u8,
    /// `+3`, value 3 when present: bit `i` links texture [`Manifest::textures`]`[i]`, the
    /// archive whose textures the animations use.
    pub textures: u8,
    /// `+4`, value 1: source name (`C01M0900.imd`); the files are this name with their
    /// own extension. Its 4th character `A` marks a collision.
    pub name: Vec<u8>,
    /// `+8`: alternatives chained by kind `0x72` records.
    pub variants: Vec<Variant>,
}

/// An alternative of a resource for one time of day (kind `0x72`, 12 bytes). None in the
/// European ROM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// `+0`, value 0: the time of day (`LightingManager+0x98`) it replaces the resource for.
    pub time_of_day: u16,
    /// `+2`, value 2 & `0x1f` when present.
    pub flags: u8,
    /// `+3`, value 3 when present.
    pub textures: u8,
    /// `+4`, value 1.
    pub name: Vec<u8>,
}

/// A placement record (kind `0x6f`, 32 bytes): one instance of a resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// `+0`, value 0: what other placements name as their parent.
    pub instance: u16,
    /// `+2`, value 1: [`Resource::id`]; negative: the resource slot numbered `instance`.
    pub resource: i16,
    /// `+4`, value 5: the parent's `instance`, negative for none.
    pub parent: i16,
    /// `+6`, value 13 (0xf when absent) with bits 4 and 5 inverted: bits 0-3 show the
    /// instance at times of day 0-3, bit 4 hides it always, bit 5 **[unknown]**.
    pub flags: u16,
    /// `+8`, values 2-4 (`fx32`), relative to the parent.
    pub translation: [i32; 3],
    /// `+0x14`, values 10-12 (or 7-9 in 8- to 10-value records): `fx32` radians kept on
    /// 16 bits; only `y` is used for the transforms.
    pub rotation: [i16; 3],
    /// `+0x1a`, values 7-9 when there are more than 10 (1.0 otherwise), `fx32` on 16 bits.
    pub scale: [i16; 3],
    /// Value 6, read then dropped by the game (the parent's name in the data).
    pub name: Option<Vec<u8>>,
}

impl Placement {
    /// Whether the instance is shown at `time_of_day` (`0x020177e4`).
    pub fn visible(&self, time_of_day: u32) -> bool {
        self.flags & 0x10 == 0
            && 1u32
                .checked_shl(time_of_day)
                .is_some_and(|bit| u32::from(self.flags) & bit != 0)
    }
}

/// A `.bmdj` as the game stores it (`Block+4`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// `+0`, `+4`: resources, appended while there is room.
    pub resources: Vec<Resource>,
    /// `+8`: capacity from the kind `0x6a` record; the game builds this many parts.
    pub resource_capacity: usize,
    /// `+0xc`, `+0x10`: placements, appended while there is room.
    pub placements: Vec<Placement>,
    /// `+0x14`: capacity from the kind `0x6d` record; the game builds this many instances.
    pub placement_capacity: usize,
    /// `+0x18..+0x38`: texture links (kind `0x70`), substrings of `.nsbtx` member names.
    pub textures: Vec<Vec<u8>>,
}

impl Manifest {
    /// Runs a `.bmdj` table through the game's handlers. Kinds `0x6b`, `0x6e` and `0x71` are
    /// ignored. Errors where the game would write past its arrays or read missing values.
    pub fn parse(table: &Table<'_>) -> Result<Self> {
        let mut m = Self::default();
        let (mut resources_set, mut placements_set) = (false, false);
        // Resource `0x72` records attach to (`0x0201f180` after each `0x6c`).
        let mut current: Option<usize> = None;
        for record in &table.records {
            let v = &record.values;
            let n = v.len();
            match record.kind {
                0x6a => {
                    let count = v
                        .first()
                        .ok_or(Error::Malformed("bmdj count without value"))?;
                    if std::mem::replace(&mut resources_set, true) {
                        return Err(Error::Malformed("bmdj with two resource counts"));
                    }
                    m.resource_capacity = count.int() as usize;
                }
                0x6c => {
                    if n < 2 {
                        return Err(Error::Malformed("bmdj resource with fewer than 2 values"));
                    }
                    let id = v[0].int() as i16;
                    let Some(name) = field_str(&v[1]) else {
                        continue;
                    };
                    if m.resources.len() < m.resource_capacity {
                        m.resources.push(Resource {
                            id,
                            flags: v.get(2).map_or(0, |f| f.int() as u8 & 0x3f),
                            textures: v.get(3).map_or(0, |f| f.int() as u8),
                            name: name.to_vec(),
                            variants: Vec::new(),
                        });
                    }
                    current = m.resource_index(id);
                }
                0x72 => {
                    if n < 2 {
                        return Err(Error::Malformed("bmdj variant with fewer than 2 values"));
                    }
                    let Some(index) = current else { continue };
                    let Some(name) = field_str(&v[1]) else {
                        continue;
                    };
                    m.resources[index].variants.push(Variant {
                        time_of_day: v[0].int() as u16,
                        flags: v.get(2).map_or(0, |f| f.int() as u8 & 0x1f),
                        textures: v.get(3).map_or(0, |f| f.int() as u8),
                        name: name.to_vec(),
                    });
                }
                0x6d => {
                    let count = v
                        .first()
                        .ok_or(Error::Malformed("bmdj count without value"))?;
                    if std::mem::replace(&mut placements_set, true) {
                        return Err(Error::Malformed("bmdj with two placement counts"));
                    }
                    m.placement_capacity = count.int() as usize;
                }
                0x6f => {
                    let placement = placement(v)?;
                    if m.placements.len() < m.placement_capacity {
                        m.placements.push(placement);
                    }
                }
                0x70 => {
                    let name = v
                        .first()
                        .ok_or(Error::Malformed("bmdj texture without value"))?;
                    if let Some(name) = field_str(name) {
                        if m.textures.len() == MAX_TEXTURES {
                            return Err(Error::Malformed("bmdj with more than 8 texture links"));
                        }
                        m.textures.push(name.to_vec());
                    }
                }
                _ => {}
            }
        }
        Ok(m)
    }

    /// Index of the first resource with this id (`0x0201f1c0`).
    pub fn resource_index(&self, id: i16) -> Option<usize> {
        self.resources.iter().position(|r| r.id == id)
    }
}

/// A kind `0x6f` record (`0x0201ee00`).
fn placement(v: &[crate::Value<'_>]) -> Result<Placement> {
    let n = v.len();
    if n < 7 {
        return Err(Error::Malformed("bmdj placement with fewer than 7 values"));
    }
    let fx16 = |i: usize| fx32(&v[i]) as i16;
    let mut next = 7;
    let mut scale = [0x1000; 3];
    if n > 10 {
        scale = [fx16(7), fx16(8), fx16(9)];
        next = 10;
    }
    let mut rotation = [0; 3];
    if n > 7 {
        rotation = [fx16(next), fx16(next + 1), fx16(next + 2)];
        next += 3;
    }
    let mut flags = 0xf;
    if n > 13 {
        flags = v[next].int() as u16;
        flags = if flags & 0x10 == 0 {
            flags & 0xff | 0x10
        } else {
            flags & 0xef
        };
        flags = if flags & 0x20 == 0 {
            flags | 0x20
        } else {
            flags & 0xffdf
        };
    }
    Ok(Placement {
        instance: v[0].int() as u16,
        resource: v[1].int() as i16,
        parent: v[5].int() as i16,
        flags,
        translation: [fx32(&v[2]), fx32(&v[3]), fx32(&v[4])],
        rotation,
        scale,
        name: field_str(&v[6]).map(<[u8]>::to_vec),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Record, Value};

    fn rec(kind: u16, values: Vec<Value<'static>>) -> Record<'static> {
        Record { kind, values }
    }

    fn pl(fields: &[f32], flags: Option<u32>) -> Record<'static> {
        let mut v = vec![
            Value::Int(1),
            Value::Int(0xffff_ffff),
            Value::Float(fields[0]),
            Value::Float(fields[1]),
            Value::Float(fields[2]),
            Value::Int(0),
            Value::Str(Some(b"P")),
        ];
        v.extend(fields[3..].iter().map(|&f| Value::Float(f)));
        v.extend(flags.map(Value::Int));
        rec(0x6f, v)
    }

    #[test]
    fn records_build_the_manifest() {
        let t = Table {
            records: vec![
                rec(0x71, vec![Value::Float(1.2)]),
                rec(0x70, vec![Value::Str(Some(b"C01M09T1"))]),
                rec(0x6a, vec![Value::Int(2)]),
                rec(0x6c, vec![Value::Int(0), Value::Str(Some(b"C01M0900.imd"))]),
                rec(
                    0x6c,
                    vec![
                        Value::Int(3),
                        Value::Str(Some(b"C01M0901.imd")),
                        Value::Int(0x6a),
                        Value::Int(1),
                    ],
                ),
                rec(
                    0x72,
                    vec![Value::Int(2), Value::Str(Some(b"N.imd")), Value::Int(0x3f)],
                ),
                rec(0x6c, vec![Value::Int(4), Value::Str(Some(b"full.imd"))]),
                rec(0x6d, vec![Value::Int(3)]),
                pl(&[0.0001, -1.5, 2.0], None),
                pl(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 0.5, 0.25, 2.0], Some(49)),
                pl(&[0.0, 0.0, 0.0, 0.5, 0.25, 2.0], None),
                pl(&[0.0; 3], None),
            ],
            string_count: 0,
        };
        let m = Manifest::parse(&t).unwrap();
        assert_eq!(m.textures, [b"C01M09T1"]);
        assert_eq!(m.resources.len(), 2);
        assert_eq!((m.resources[1].flags, m.resources[1].textures), (0x2a, 1));
        assert_eq!(m.resources[1].variants[0].flags, 0x1f);
        assert_eq!(m.resource_index(3), Some(1));
        assert_eq!(m.placements.len(), 3);
        let [a, b, c] = [&m.placements[0], &m.placements[1], &m.placements[2]];
        assert_eq!(a.translation, [0, -6144, 8192]);
        assert_eq!((a.resource, a.flags, a.scale), (-1, 0xf, [0x1000; 3]));
        assert_eq!(a.name.as_deref(), Some(&b"P"[..]));
        // 13 values and more: scale, then rotation, then flags.
        assert_eq!(b.scale, [0x1000, 0x2000, 0x3000]);
        assert_eq!(b.rotation, [0x800, 0x400, 0x2000]);
        assert_eq!(b.flags, 0x01);
        // 8 to 10 values: rotation only.
        assert_eq!((c.rotation, c.scale), ([0x800, 0x400, 0x2000], [0x1000; 3]));
        assert_eq!(c.flags, 0xf);
        assert!(b.visible(0) && !b.visible(1) && !b.visible(40));
    }

    #[test]
    fn flag_bits_4_and_5_are_inverted() {
        let m = |raw: u32| {
            let r = pl(&[0.0; 9], Some(raw));
            placement(&r.values).unwrap().flags
        };
        assert_eq!(m(63), 0x0f);
        assert_eq!(m(62), 0x0e);
        assert_eq!(m(0x10f), 0x3f);
        assert_eq!(m(0x13f), 0x0f);
        assert_eq!(m(0x12f), 0x1f);
    }

    #[test]
    fn rejects_overflowing_links_and_short_records() {
        let mut records = vec![];
        for _ in 0..9 {
            records.push(rec(0x70, vec![Value::Str(Some(b"T"))]));
        }
        let t = Table {
            records,
            string_count: 0,
        };
        assert!(Manifest::parse(&t).is_err());
        assert!(placement(&[Value::Int(0); 6]).is_err());
    }
}
