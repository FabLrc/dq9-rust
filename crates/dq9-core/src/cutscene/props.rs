//! What a script does to the map's props and to the balloons over its characters: the
//! placement natives 574 and 575 and the balloon natives 541, 561 and 542.
//!
//! Evidence: `docs/re/stb-natives.md`; handlers `ov_001::02163dec` (574), `02163e50` (575),
//! `0215fe50` (541), `02160a48` (561), `0215fef4` (542), `ov_017::0219b43c` (the balloon:
//! kind to sprite, offset), `0219b578` (the nudge), `02019518` (the placement search).

/// A change of a map placement the application applies to the instance it draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlacementChange {
    /// Native 574: bit 2 of the record's `+2` (hidden at this time of day) set or cleared
    /// (`02013390`).
    Hidden { group: i32, id: i32, hidden: bool },
    /// Native 575: the record's translation (`+8`, `+0xc`, `+0x10`) replaced, in world units.
    Moved {
        group: i32,
        id: i32,
        position: [f32; 3],
    },
}

/// A balloon over a character's head (`ov_017::0219b43c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Balloon {
    /// The actor (0 to 23) whose position the balloon follows.
    pub actor: i32,
    /// The field's sprite id (`docs/re/stb-natives.md`): 2 `fuki_com`, 3 `fuki_in`, 4
    /// `fuki_hkn`, 12 `field_mon01`, 13 `ev_mark`, 14 `field_qu`.
    pub sprite: i32,
    /// Pixels on the 256 × 192 screen from the actor's projected position to the sprite's
    /// origin: `(-6 or -12, -76)` plus the offsets of natives 541 and 561.
    pub offset: [i32; 2],
}

impl Balloon {
    /// The script's `kind` mapped to the field's sprite id, and the base offset of the sprite
    /// (`0219b43c`: sprites 12 to 14 sit 6 pixels left of the character, the others 12).
    pub fn new(actor: i32, kind: i32, offset: [i32; 2]) -> Self {
        let sprite = match kind {
            2 => 2,
            5 => 3,
            6 => 4,
            14 => 12,
            15 => 13,
            16 => 14,
            other => other,
        };
        let x = if matches!(sprite, 12..=14) { -6 } else { -12 };
        Self {
            actor,
            sprite,
            offset: [x + offset[0], -0x4c + offset[1]],
        }
    }
}

/// A palette operation on a character's model (native 238; `ov_017::0218f968`, `0218fd34`):
/// mode 0 adds the colour to every palette entry (clamped at 31 per channel), mode 1 replaces
/// every entry with it, mode 2 multiplies each channel by `colour / 31`. The game redoes it from
/// the original palette each time, so `(0, 0, 0)` in mode 0 restores the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexelOp {
    pub mode: i32,
    /// The 5-bit channels `c0 | c1 << 5 | c2 << 10` of the palette colour.
    pub rgb: [i32; 3],
}

impl TexelOp {
    /// The operation, or `None` when it leaves the colours alone (a mode the game does not
    /// handle, or an addition of black).
    pub fn new(mode: i32, rgb: [i32; 3]) -> Option<Self> {
        let rgb = rgb.map(|c| c.clamp(0, 31));
        match mode {
            0 if rgb == [0; 3] => None,
            0..=2 => Some(Self { mode, rgb }),
            _ => None,
        }
    }

    fn unit(&self) -> [f32; 3] {
        self.rgb.map(|c| c as f32 / 31.0)
    }

    /// The factor on the texel colour.
    pub fn mul(&self) -> [f32; 3] {
        match self.mode {
            0 => [1.0; 3],
            1 => [0.0; 3],
            _ => self.unit(),
        }
    }

    /// The colour added after the factor.
    pub fn add(&self) -> [f32; 3] {
        match self.mode {
            2 => [0.0; 3],
            _ => self.unit(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Props {
    pub(super) balloon: Option<Balloon>,
    pub(super) changes: Vec<PlacementChange>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balloon_kinds_map_to_the_field_sprites() {
        let b = Balloon::new(1, 15, [0, 0]);
        assert_eq!((b.sprite, b.offset), (13, [-6, -76]));
        let b = Balloon::new(1, 5, [2, -3]);
        assert_eq!((b.sprite, b.offset), (3, [-10, -79]));
    }

    #[test]
    fn palette_operations_by_mode() {
        assert_eq!(TexelOp::new(0, [0, 0, 0]), None);
        let add = TexelOp::new(0, [31, 0, 0]).unwrap();
        assert_eq!((add.mul(), add.add()), ([1.0; 3], [1.0, 0.0, 0.0]));
        let fill = TexelOp::new(1, [31, 31, 31]).unwrap();
        assert_eq!((fill.mul(), fill.add()), ([0.0; 3], [1.0; 3]));
        let times = TexelOp::new(2, [8, 6, 16]).unwrap();
        assert_eq!(times.add(), [0.0; 3]);
        assert!((times.mul()[2] - 16.0 / 31.0).abs() < 1e-6);
        // A mode the game does not handle leaves the palette alone.
        assert_eq!(TexelOp::new(5, [8, 6, 16]), None);
    }
}
