//! Map lighting scripts (`<name>.bats`), run by `LightingInfo::LoadFromScript` (EU
//! `0x0207ba1c`) over the tagged-table container of `table.rs` (the `Script` class of the
//! game). Opcode handlers mirror `src/Graphics/LightingInfo.cpp` of the USA decompilation
//! (`LightingScript_*`); EU addresses are in `re/functions.txt`. Evidence and open questions:
//! `docs/re/lighting.md`.

use crate::table::{Fields, Table};
use nds_rom::{Error, Result};

/// Number of time-of-day slots (`0..=6`) that a lighting script can fill.
pub const SLOTS: usize = 7;

/// Three-component vector of the basic lighting entries (`Vector3float`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// One of the DS 3D lights (`LightingManager::Light3DConfig`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Light3DConfig {
    /// BGR555 colour.
    pub color: u16,
    pub direction: [f32; 3],
    pub enabled: bool,
}

/// One fog entry (`LightingManager::FogInfo`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FogInfo {
    /// Script field 1 compared with zero (`unk_0`, "is fog enabled" by guess).
    pub enabled: bool,
    /// 0 = colour and alpha, 1 = alpha only.
    pub kind: i32,
    pub depth_shift: i32,
    pub offset: i32,
    /// BGR555 colour.
    pub color: u16,
    pub alpha: i32,
    /// Eight packed script words, stored little-endian byte by byte.
    pub density_table: [u8; 0x20],
}

/// Basic lighting (mode 1, `LightingManager::BasicLighting`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BasicLighting {
    /// `unk_0`: a direction-like vector per slot.
    pub positions: [Vec3; SLOTS],
    /// `unk_54`: script field 8, meaning unknown.
    pub unk_54: [f32; SLOTS],
    /// `unk_70`: script field 7, meaning unknown.
    pub unk_70: [f32; SLOTS],
    pub background: [u16; SLOTS],
    pub background_second: [u16; SLOTS],
    pub pot_barrel_diffuse: [u16; SLOTS],
    pub sprite_diffuse: [u16; SLOTS],
    pub model_diffuse: [u16; SLOTS],
    pub edge: [u16; SLOTS],
    /// `unk_e0`: set by every basic entry record.
    pub entry_set: [bool; SLOTS],
}

/// Advanced lighting (mode 2, `LightingManager::AdvancedLighting`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AdvancedLighting {
    pub light0: [Light3DConfig; SLOTS],
    pub light1: [Light3DConfig; SLOTS],
    pub ambient: [u16; SLOTS],
    pub background: [u16; SLOTS],
    pub background_second: [u16; SLOTS],
    /// `unk_142`: script field 4 of the colour group, meaning unknown.
    pub unk_142: [u16; SLOTS],
    pub sprite_diffuse: [u16; SLOTS],
    pub model_diffuse: [u16; SLOTS],
    pub edge: [u16; SLOTS],
}

/// Which of the two lighting blocks the script declared (`maybeMode_`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightingMode {
    #[default]
    None,
    Basic,
    Advanced,
}

/// State filled by a lighting script (`LightingInfo`, sizeof 0x30c in the game).
///
/// The game stores `basic_` and `advanced_` in a union; here both are kept, so a script
/// that writes both keeps both values. [inconnu] whether the game ever relies on the
/// aliasing (no ROM script found that mixes both blocks; see `docs/re/lighting.md`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LightingInfo {
    pub mode: LightingMode,
    pub gradient_center_offset: f32,
    pub basic: BasicLighting,
    pub advanced: AdvancedLighting,
    pub fog: [FogInfo; SLOTS],
}

impl LightingInfo {
    /// Parses a `.bats` script and runs it, as `LightingInfo::LoadFromScript` does on a
    /// freshly initialised block.
    ///
    /// port: 0x0207ba1c
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut info = Self::default();
        // ExecuteLightingScript returns at once on an empty script.
        // port: 0x0207b938
        if data.is_empty() {
            return Ok(info);
        }
        let table = Table::parse(data)?;
        let mut fields = Fields::default();
        for record in &table.records {
            // Every record fills the field buffer, known opcode or not (Tbl_Next).
            fields.load(record);
            if let Some(handler) = handler(record.kind) {
                handler(&mut info, &fields);
                if fields.read_unset() {
                    return Err(Error::Malformed(
                        "lighting script reads a parameter that no record wrote",
                    ));
                }
            }
        }
        Ok(info)
    }
}

type Handler = fn(&mut LightingInfo, &Fields<'_>);

/// Opcode lookup of `s_lightingOpcodes` (USA `LightingInfo.cpp`); unknown opcodes are
/// skipped, as the game's lookup returns NULL for them.
fn handler(kind: u16) -> Option<Handler> {
    Some(match kind {
        0x64 => declare_advanced_lighting,
        0x65 => nop,
        0x66 => create_advanced_lighting_entry_alternate,
        0x67 => declare_basic_lighting,
        0x68 => create_basic_lighting_entry,
        0x69 => create_fog_entry,
        0x6a => create_advanced_lighting_entry,
        _ => return None,
    })
}

/// Slot index of field 0, or `None` when it is outside `0..SLOTS`. The game does not check
/// negative indices (0x68, 0x6a) and writes out of bounds; they are ignored here.
fn slot(fields: &Fields<'_>) -> Option<usize> {
    usize::try_from(fields.int(0)).ok().filter(|&s| s < SLOTS)
}

/// `LightingScript_DeclareAdvancedLighting`.
///
/// port: 0x0207b2dc
fn declare_advanced_lighting(info: &mut LightingInfo, f: &Fields<'_>) {
    if f.count() > 0 {
        info.gradient_center_offset = f.float(0);
    }
    info.mode = LightingMode::Advanced;
}

/// `LightingScript_Nop`.
///
/// port: 0x0207b314
fn nop(_info: &mut LightingInfo, _f: &Fields<'_>) {}

/// `LightingScript_CreateAdvancedLightingEntry_Alternate`, opcode 0x66. Never emitted by
/// the ROM's scripts in the USA decompilation's notes.
///
/// port: 0x0207b31c
fn create_advanced_lighting_entry_alternate(info: &mut LightingInfo, f: &Fields<'_>) {
    let Some(s) = slot(f) else { return };
    let adv = &mut info.advanced;
    adv.light1[s] = Light3DConfig {
        enabled: f.int(1) != 0,
        direction: [f.float(2), f.float(3), f.float(4)],
        color: f.int(5) as u16,
    };
    // Field 6 is read and discarded: the game overwrites the primary enable flag.
    let _ = f.int(6);
    adv.light0[s] = Light3DConfig {
        enabled: true,
        direction: [0.5, -1.0, 0.5],
        color: f.int(7) as u16,
    };
    set_advanced_colors(adv, s, f, 8);
}

/// `LightingScript_DeclareBasicLighting`.
///
/// port: 0x0207b4d0
fn declare_basic_lighting(info: &mut LightingInfo, f: &Fields<'_>) {
    if f.count() > 0 {
        info.gradient_center_offset = f.float(0);
    }
    info.mode = LightingMode::Basic;
}

/// `LightingScript_CreateBasicLightingEntry`, opcode 0x68.
///
/// port: 0x0207b504
fn create_basic_lighting_entry(info: &mut LightingInfo, f: &Fields<'_>) {
    let Some(s) = slot(f) else { return };
    let basic = &mut info.basic;
    basic.positions[s] = Vec3 {
        x: f.float(1),
        y: f.float(2),
        z: f.float(3),
    };
    basic.background[s] = f.int(4) as u16;
    basic.background_second[s] = f.int(5) as u16;
    basic.pot_barrel_diffuse[s] = f.int(6) as u16;
    basic.unk_70[s] = f.float(7);
    basic.unk_54[s] = f.float(8);
    basic.entry_set[s] = true;
    basic.sprite_diffuse[s] = f.int(9) as u16;
    basic.model_diffuse[s] = f.int(10) as u16;
    basic.edge[s] = f.int(11) as u16;
}

/// `LightingScript_CreateFogEntry`, opcode 0x69. Fog entries have 7 slots, as the game's
/// `FogList`.
///
/// port: 0x0207b648
fn create_fog_entry(info: &mut LightingInfo, f: &Fields<'_>) {
    let Some(s) = slot(f) else { return };
    let mut density_table = [0u8; 0x20];
    for i in 0..8 {
        let packed = f.int(6 + i) as u32;
        density_table[4 * i..4 * i + 4].copy_from_slice(&packed.to_le_bytes());
    }
    info.fog[s] = FogInfo {
        enabled: f.int(1) != 0,
        kind: f.int(3),
        depth_shift: f.int(4),
        offset: f.int(5),
        color: f.int(2) as u16,
        alpha: f.int(14),
        density_table,
    };
}

/// `LightingScript_CreateAdvancedLightingEntry`, opcode 0x6a.
///
/// port: 0x0207b78c
fn create_advanced_lighting_entry(info: &mut LightingInfo, f: &Fields<'_>) {
    let Some(s) = slot(f) else { return };
    let adv = &mut info.advanced;
    adv.light1[s] = Light3DConfig {
        enabled: f.int(1) != 0,
        direction: [f.float(2), f.float(3), f.float(4)],
        color: f.int(5) as u16,
    };
    adv.light0[s] = Light3DConfig {
        enabled: f.int(6) != 0,
        direction: [f.float(7), f.float(8), f.float(9)],
        color: f.int(10) as u16,
    };
    set_advanced_colors(adv, s, f, 11);
}

/// The seven colour fields shared by the advanced entry records, from field `first`.
fn set_advanced_colors(adv: &mut AdvancedLighting, s: usize, f: &Fields<'_>, first: usize) {
    adv.background[s] = f.int(first) as u16;
    adv.background_second[s] = f.int(first + 1) as u16;
    adv.ambient[s] = f.int(first + 2) as u16;
    adv.unk_142[s] = f.int(first + 3) as u16;
    adv.sprite_diffuse[s] = f.int(first + 4) as u16;
    adv.model_diffuse[s] = f.int(first + 5) as u16;
    adv.edge[s] = f.int(first + 6) as u16;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Field type codes of the tagged table: 1 = integer, 2 = float.
    const INT: u8 = 1;
    const FLOAT: u8 = 2;

    fn int(v: i32) -> (u8, u32) {
        (INT, v as u32)
    }

    fn float(v: f32) -> (u8, u32) {
        (FLOAT, v.to_bits())
    }

    /// Encodes one record: kind, field count, type bytes padded with 0xFF, then the fields.
    fn record(kind: u16, values: &[(u8, u32)]) -> Vec<u8> {
        let mut out = vec![kind as u8, (kind >> 8) as u8, values.len() as u8];
        for group in values.chunks(4) {
            let mut byte = 0u8;
            for (j, &(t, _)) in group.iter().enumerate() {
                byte |= t << (2 * j);
            }
            out.push(byte);
        }
        while out.len() % 4 != 0 {
            out.push(0xFF);
        }
        for &(_, raw) in values {
            out.extend_from_slice(&raw.to_le_bytes());
        }
        out
    }

    /// Builds a `.bats` container: 16-byte header, records, empty string pool at the end.
    fn bats(records: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = records.concat();
        let pool_offset = 0x10 + body.len();
        let mut out = Vec::new();
        out.extend_from_slice(&(records.len() as u32).to_le_bytes());
        out.extend_from_slice(&(pool_offset as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn empty_script_gives_default_state() {
        assert_eq!(LightingInfo::parse(&[]).unwrap(), LightingInfo::default());
    }

    #[test]
    fn declare_advanced_sets_mode_and_offset() {
        let data = bats(&[record(0x64, &[float(0.25)])]);
        let info = LightingInfo::parse(&data).unwrap();
        assert_eq!(info.mode, LightingMode::Advanced);
        assert_eq!(info.gradient_center_offset, 0.25);
    }

    #[test]
    fn declare_without_parameter_keeps_offset() {
        let data = bats(&[record(0x67, &[])]);
        let info = LightingInfo::parse(&data).unwrap();
        assert_eq!(info.mode, LightingMode::Basic);
        assert_eq!(info.gradient_center_offset, 0.0);
    }

    #[test]
    fn basic_entry_fills_its_slot_only() {
        let data = bats(&[record(
            0x68,
            &[
                int(2),
                float(1.0),
                float(2.0),
                float(3.0),
                int(0x1234),
                int(0x0042),
                int(7),
                float(0.5),
                float(-0.5),
                int(9),
                int(10),
                int(11),
            ],
        )]);
        let info = LightingInfo::parse(&data).unwrap();
        let b = &info.basic;
        assert_eq!(
            b.positions[2],
            Vec3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
        assert_eq!(b.background[2], 0x1234);
        assert_eq!(b.background_second[2], 0x42);
        assert_eq!(b.pot_barrel_diffuse[2], 7);
        assert_eq!(b.unk_70[2], 0.5);
        assert_eq!(b.unk_54[2], -0.5);
        assert!(b.entry_set[2]);
        assert_eq!(
            (b.sprite_diffuse[2], b.model_diffuse[2], b.edge[2]),
            (9, 10, 11)
        );
        assert!(!b.entry_set[0]);
        assert_eq!(b.background[0], 0);
    }

    #[test]
    fn fog_entry_unpacks_density_table_and_ignores_bad_index() {
        let fog = |index: i32| {
            let mut v = vec![int(index), int(1), int(0x7fff), int(1), int(2), int(3)];
            v.push(int(0x04030201));
            v.extend(std::iter::repeat_n(int(0), 7));
            v.push(int(200));
            v
        };
        let data = bats(&[record(0x69, &fog(3)), record(0x69, &fog(9))]);
        let info = LightingInfo::parse(&data).unwrap();
        let f = &info.fog[3];
        assert!(f.enabled);
        assert_eq!((f.kind, f.depth_shift, f.offset, f.alpha), (1, 2, 3, 200));
        assert_eq!(f.color, 0x7fff);
        assert_eq!(&f.density_table[..4], &[1, 2, 3, 4]);
        assert_eq!(f.density_table[4..], [0u8; 0x1c]);
        assert_eq!(info.fog.iter().filter(|f| f.enabled).count(), 1);
    }

    #[test]
    fn advanced_entry_fills_both_lights_and_colours() {
        let data = bats(&[record(
            0x6a,
            &[
                int(6),
                int(1),
                float(0.1),
                float(0.2),
                float(0.3),
                int(0x1f),
                int(0),
                float(1.0),
                float(2.0),
                float(3.0),
                int(0x3e0),
                int(1),
                int(2),
                int(3),
                int(4),
                int(5),
                int(6),
                int(7),
            ],
        )]);
        let info = LightingInfo::parse(&data).unwrap();
        let a = &info.advanced;
        assert!(a.light1[6].enabled);
        assert_eq!(a.light1[6].direction, [0.1, 0.2, 0.3]);
        assert_eq!(a.light1[6].color, 0x1f);
        assert!(!a.light0[6].enabled);
        assert_eq!(a.light0[6].direction, [1.0, 2.0, 3.0]);
        assert_eq!(a.light0[6].color, 0x3e0);
        assert_eq!(
            (
                a.background[6],
                a.background_second[6],
                a.ambient[6],
                a.unk_142[6]
            ),
            (1, 2, 3, 4)
        );
        assert_eq!(
            (a.sprite_diffuse[6], a.model_diffuse[6], a.edge[6]),
            (5, 6, 7)
        );
    }

    #[test]
    fn alternate_entry_sets_fixed_primary_direction() {
        let data = bats(&[record(
            0x66,
            &[
                int(1),
                int(1),
                float(0.0),
                float(0.0),
                float(1.0),
                int(0x10),
                int(0),
                int(0x20),
                int(0),
                int(0),
                int(0),
                int(0),
                int(0),
                int(0),
                int(0),
            ],
        )]);
        let info = LightingInfo::parse(&data).unwrap();
        let a = &info.advanced;
        assert!(a.light1[1].enabled);
        assert_eq!(a.light1[1].direction, [0.0, 0.0, 1.0]);
        assert_eq!(a.light0[1].direction, [0.5, -1.0, 0.5]);
        assert!(a.light0[1].enabled);
        assert_eq!(a.light0[1].color, 0x20);
    }

    #[test]
    fn unknown_and_nop_records_are_skipped() {
        let data = bats(&[
            record(0x65, &[]),
            record(0x10, &[int(5)]),
            record(0x64, &[]),
        ]);
        let info = LightingInfo::parse(&data).unwrap();
        assert_eq!(info.mode, LightingMode::Advanced);
    }

    #[test]
    fn negative_or_large_index_is_ignored() {
        let data = bats(&[record(
            0x68,
            &[
                int(-1),
                float(1.0),
                float(0.0),
                float(0.0),
                int(0),
                int(0),
                int(0),
                float(0.0),
                float(0.0),
                int(0),
                int(0),
                int(0),
            ],
        )]);
        assert_eq!(
            LightingInfo::parse(&data).unwrap().basic,
            BasicLighting::default()
        );
    }

    #[test]
    fn string_field_reads_as_zero() {
        // String field with the NO_STRING offset: `Value::Str(None)`, integer view 0, so the
        // gradient offset of 0x64 is 0.0 and the mode is still declared.
        let data = bats(&[record(0x64, &[(0, u32::MAX)])]);
        let info = LightingInfo::parse(&data).unwrap();
        assert_eq!(info.gradient_center_offset, 0.0);
        assert_eq!(info.mode, LightingMode::Advanced);
    }

    #[test]
    fn missing_parameter_is_an_error() {
        let data = bats(&[record(0x6a, &[int(0)])]);
        assert!(LightingInfo::parse(&data).is_err());
    }

    #[test]
    fn invalid_field_type_is_an_error() {
        let mut rec = record(0x64, &[int(0)]);
        rec[3] = 0b11;
        assert!(LightingInfo::parse(&bats(&[rec])).is_err());
    }

    #[test]
    fn truncated_script_is_an_error() {
        let data = bats(&[record(0x64, &[float(0.5)])]);
        assert!(LightingInfo::parse(&data[..data.len() - 1]).is_err());
        assert!(LightingInfo::parse(&data[..8]).is_err());
    }
}
