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
    /// Native 577: the record's scale (`+0x14`, `+0x18`, `+0x1c`), as factors on the axes. The
    /// drawn instance follows; the collision mesh keeps its size **[hypothesis: the factors in
    /// the data are 0.97 and 1]**.
    Scaled {
        group: i32,
        id: i32,
        scale: [f32; 3],
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

/// A map door the script opens (native 584, `ov_001::0215fb44` → `02018310`): the placement
/// `(block entry, placement id)` turns around its own origin. The handler sets the target
/// angle to the placement's angle ± 90° (`g_DoorSwingAngle` 6434) and a speed of 409 angle
/// units per update (`0x02018748`, `0x02018750`); the update that consumes them was not
/// found, so the per-update interpolation is **[hypothesis]**.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Door {
    /// `Zone3D` block entry the door belongs to (the doorway's class).
    pub block: i32,
    /// Placement id inside that block (the doorway's number).
    pub placement: i32,
    /// Current rotation, radians.
    pub angle: f32,
    /// Target rotation, radians: the placement's own angle plus 6434/4096.
    pub target: f32,
    /// Angle added per 1/60 s tick: 409/4096 per game update (two ticks).
    pub speed: f32,
    /// Native 584's `sound` argument: the door sound is not played here (no base-archive
    /// effect index is established), and the report says so.
    pub sound: bool,
}

impl Door {
    /// Target angle of the door the handler opens with the prologue's flags (`0x40`):
    /// `(rotation + 1) >> 1` in `0204a170` is a facing, not this; here the placement's own
    /// angle turns by one quarter turn (`DAT_02018748` 6434 ≈ π/2 × 4096).
    pub const QUARTER_TURN: f32 = 6434.0 / 4096.0;
    /// 409 angle units per update, two ticks per update (`DAT_02018750`).
    pub const SPEED: f32 = 409.0 / 4096.0 / 2.0;

    /// One tick of 1/60 s towards the target.
    pub fn tick(&mut self) {
        let remaining = self.target - self.angle;
        if remaining.abs() <= self.speed {
            self.angle = self.target;
        } else {
            self.angle += remaining.signum() * self.speed;
        }
    }

    /// The delta in the game's angle unit (4096 per turn), for the collision world.
    pub fn angle_fx(&self) -> i32 {
        (self.angle * 4096.0).round() as i32
    }
}

/// The caption fields natives 409, 411 and 413 set on the message window; native 400 resets
/// them (`0204501c` zeroes `+0x195b` and `+0x9b0`, sets `+0x19b2 = 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caption {
    /// Native 409's `n`: frames of hold between the one-second alpha ramps.
    pub time: Option<i32>,
    /// Native 411: centre the text vertically.
    pub centered: bool,
    /// Native 413: no sound while the text types.
    pub silent: bool,
    /// Tick the timed caption started.
    pub started: u64,
}

impl Caption {
    /// Alpha of the caption at tick `now`: one second (60 ticks) in, `time` ticks held, one
    /// second out (`0x1f0000 / 0x8444 = 60`); 1 without 409.
    pub fn alpha(&self, now: u64) -> f32 {
        let Some(hold) = self.time else {
            return 1.0;
        };
        let age = now.saturating_sub(self.started);
        let hold = hold.max(0) as u64;
        if age < 60 {
            age as f32 / 60.0
        } else if age < 60 + hold {
            1.0
        } else if age < 120 + hold {
            1.0 - (age - 60 - hold) as f32 / 60.0
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Props {
    pub(super) balloon: Option<Balloon>,
    pub(super) changes: Vec<PlacementChange>,
    pub(super) doors: Vec<Door>,
    pub(super) caption: Caption,
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
