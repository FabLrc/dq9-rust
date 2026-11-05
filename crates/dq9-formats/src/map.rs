//! Map assembly: which files the game reads for a map and how it puts their pieces together,
//! as the `Zone3D` load state machine does (`0x02013e28`, after `Zone3D::SwitchZone`,
//! `0x0201384c`). Layout and evidence in `docs/re/map-archive.md`.
//!
//! - [`MapFiles`]: archive names of a map (`<code>.ambl`, `<code>.amdj`, grottoes, ocean/sky).
//! - [`ZoneInfo`]: the zone settings read from the map list, the `.dat` member and the
//!   lighting choice (`ats_<c>.ambl` / `<name>.bats`).
//! - [`BlockList`]: the `.bmbl`/`.bpos` list naming the `.bmdj` manifests to load.
//! - [`Manifest`]: one `.bmdj` (resources, placements, texture links).
//! - [`Map::assemble`]: the whole chain, down to each resource's loaded files and each
//!   placement's world transform.
//!
//! port: 0x0201b5c0, 0x0201b5e8, 0x0201404c, 0x020145b8, 0x02014d28

mod assemble;
mod blocks;
mod collision;
mod doors;
mod manifest;
mod transform;
mod zone;

pub use assemble::{AnimKind, Animation, Block, Context, Instance, Map, Part, PartKind};
pub use blocks::{BlockEntry, BlockList};
pub use collision::{Collision, GroundRecord, Triangle};
pub use doors::parse as parse_regions;
pub use doors::{
    FLAG_SIDE, FLAG_WARP, Regions, Warp, WarpVolume, ZONE_AREA, ZONE_DOORWAY, Zone, ZoneVolume,
    ZoneWarp,
};
pub use manifest::{Manifest, Placement, Resource, Variant, resource_flags};
pub use transform::{Transform, fix32_cos, fix32_sin, reduce_angle, sin_cos_entry};
pub use zone::{LightingFile, ZoneInfo};

use crate::table::Value;
use nds_rom::compress::{Codec, decode};
use nds_rom::{Error, Result, Rom};

/// Directory of the map archives (`data_020ef116`).
pub const DIR: &str = "data/map";

/// Map id of the ocean (`O00`), whose models come in two archives (`0x020145b8`).
pub const OCEAN: u16 = 10000;
/// Map id of the sky (`O01`), loaded like [`OCEAN`].
pub const SKY: u16 = OCEAN + 100;

/// Main grotto floors (`0x0201b5c0`: `40001..=40001 + 0x5f`).
pub fn is_grotto_floor(id: u16) -> bool {
    (40001..=40001 + 0x5f).contains(&id)
}

/// Other grotto maps (`0x0201b5e8`: `41101..=41101 + 0x194`), e.g. boss rooms.
pub fn is_grotto_other(id: u16) -> bool {
    (41101..=41101 + 0x194).contains(&id)
}

/// The archives the game queues for a map, in load order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapFiles {
    /// Textures, block list and zone settings (`0x0201404c`).
    pub ambl: String,
    /// Models, collisions and their manifests (`0x020145b8`): one archive, or `b` then `a`
    /// for the ocean and the sky.
    pub amdj: Vec<String>,
}

impl MapFiles {
    /// Archive names for map `id` with map code `code`; `grotto_environ` is the active
    /// grotto's environment (`ActiveGrottoClass::GetActiveGrottoEnviron`), read for grotto
    /// maps only.
    pub fn new(id: u16, code: &[u8], grotto_environ: i32) -> Self {
        let code = String::from_utf8_lossy(c_str(code));
        if is_grotto_floor(id) {
            // 0 reads as 1 and values above 5 as 5, for both archives.
            let env = match grotto_environ {
                0 => 1,
                e => e.min(5),
            };
            Self {
                ambl: format!("{DIR}/Z0{env}M01.ambl"),
                amdj: vec![format!("{DIR}/Z0{env}M01.amdj")],
            }
        } else if is_grotto_other(id) {
            Self {
                ambl: format!("{DIR}/Z0{grotto_environ}M99.ambl"),
                amdj: vec![format!("{DIR}/Z0{grotto_environ}M99.amdj")],
            }
        } else {
            let amdj = if id == OCEAN || id == SKY {
                // Zone3D+0x42c counts the queued halves: `b` first, then `a`.
                vec![format!("{DIR}/{code}b.amdj"), format!("{DIR}/{code}a.amdj")]
            } else {
                vec![format!("{DIR}/{code}.amdj")]
            };
            Self {
                ambl: format!("{DIR}/{code}.ambl"),
                amdj,
            }
        }
    }
}

/// A ROM file as the game opens it: the SDK's file system compares paths without case
/// (`FSi_StrNICmp`), and maps rely on it (`S09M03` is `data/map/S09m03.ambl`).
pub fn rom_file<'a>(rom: &Rom<'a>, path: &str) -> Option<&'a [u8]> {
    let entry = rom
        .files
        .files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case(path))?;
    rom.file(entry.id).ok()
}

/// Bytes up to the first NUL, as C string functions see them.
fn c_str(s: &[u8]) -> &[u8] {
    s.iter().position(|&b| b == 0).map_or(s, |end| &s[..end])
}

/// C `strstr(haystack, needle) != NULL`: an empty needle matches.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

/// The path the game looks up in the mounted archive for resource `name` with extension
/// `ext` (`0x02014d28`: `ARC:/<name>`, the text after its last `.` replaced by `ext`, or
/// `.<ext>` appended). Returned without the `ARC:/` mount prefix.
pub fn member_name(name: &[u8], ext: &str) -> String {
    let name = c_str(name);
    let stem = match name.iter().rposition(|&b| b == b'.') {
        Some(dot) => &name[..dot],
        None => name,
    };
    format!("{}.{ext}", String::from_utf8_lossy(stem))
}

/// Text from the last `.` of a member name, as `strrchr(name, '.')` gives it.
fn extension(name: &str) -> Option<&str> {
    name.rfind('.').map(|dot| &name[dot..])
}

/// `DecompressLZ77FileIntoScratchSpace` / `IntoAllocatedSpace` (`0x02075618`,
/// `0x02075664`): the size is the header word's upper 24 bits and the body is always
/// decoded as LZ10, whatever the type byte.
fn lz77(data: &[u8]) -> Result<Vec<u8>> {
    let header = data.first_chunk::<4>().ok_or(Error::OutOfBounds {
        offset: 0,
        len: 4,
        size: data.len(),
    })?;
    let size = (u32::from_le_bytes(*header) >> 8) as usize;
    decode(Codec::Lz10, &data[4..], size)
}

/// `Script::Parameter::ToFloat`: integers converted, strings 0.
fn to_float(v: &Value<'_>) -> f32 {
    match *v {
        Value::Int(i) => i as i32 as f32,
        Value::Float(f) => f,
        Value::Str(_) => 0.0,
    }
}

/// `4096 * value`, converted to an integer like the game's `_ffix` (truncation toward zero;
/// out-of-range values saturate here **[unknown on the console]**).
fn fx32(v: &Value<'_>) -> i32 {
    (4096.0 * to_float(v)) as i32
}

/// `Tbl_FieldStr`: the string of a string field, else nothing.
fn field_str<'a>(v: &Value<'a>) -> Option<&'a [u8]> {
    match *v {
        Value::Str(s) => s,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_names() {
        let f = MapFiles::new(109, b"C01M09", 0);
        assert_eq!(f.ambl, "data/map/C01M09.ambl");
        assert_eq!(f.amdj, ["data/map/C01M09.amdj"]);
        let f = MapFiles::new(OCEAN, b"O00", 0);
        assert_eq!(f.amdj, ["data/map/O00b.amdj", "data/map/O00a.amdj"]);
        // Grotto floors clamp the environment, other grotto maps do not.
        assert_eq!(
            MapFiles::new(40001, b"Z00M01", 0).ambl,
            "data/map/Z01M01.ambl"
        );
        assert_eq!(MapFiles::new(40096, b"", 9).amdj, ["data/map/Z05M01.amdj"]);
        assert!(!is_grotto_floor(40097));
        assert_eq!(MapFiles::new(41101, b"", 0).ambl, "data/map/Z00M99.ambl");
        assert_eq!(MapFiles::new(41505, b"", 7).amdj, ["data/map/Z07M99.amdj"]);
        assert!(!is_grotto_other(41506));
    }

    #[test]
    fn member_names_replace_the_extension() {
        assert_eq!(member_name(b"C01M0900.imd", "nsbmd"), "C01M0900.nsbmd");
        assert_eq!(member_name(b"a.b.imd", "col2"), "a.b.col2");
        assert_eq!(member_name(b"NOEXT", "nsbca"), "NOEXT.nsbca");
        assert!(contains(b"C01M0900.bmdj", b"C01M0900"));
        assert!(contains(b"x", b""));
        assert!(!contains(b"C01M09", b"C01M0900"));
    }

    #[test]
    fn lz77_ignores_the_type_byte() {
        // One literal block: flags 0, then 3 literal bytes.
        let data = [0x00, 3, 0, 0, 0x00, b'a', b'b', b'c'];
        assert_eq!(lz77(&data).unwrap(), b"abc");
        assert!(lz77(&[0x10]).is_err());
    }
}
