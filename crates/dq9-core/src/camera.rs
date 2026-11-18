//! The field camera's turn: the shoulder buttons rotate it around the hero within a quarter
//! turn of its home direction, both together bring it back (`FieldCamera_Update`,
//! `0x020a213c`; `docs/re/movement.md`, "Field camera"). Only the yaw is modelled: the
//! camera's height, distance and target follow the application's camera.
//!
//! Which maps let the buttons turn it is [`rotation_allowed`] (`ov_017::0219e234`): a
//! terrain region (`F` maps) keeps the camera fixed, as does a record without bit 7.
//!
//! Angles are `fx32` (0x6488 per turn) as in the game, where yaw 0 looks towards `-z` and a
//! positive yaw swings the eye towards `+x`.

use crate::fx::{self, reduce_angle};

/// Yaw speed per millisecond of the frame, in turns of `fx32` (`0x020a2714`, 0.0015 × 4096).
const TURN_RATE: f32 = f32::from_bits(0x3ac4_9ba6);
/// Speed of the return to the home direction (`0x020a270c`).
const RETURN_RATE: f32 = f32::from_bits(0x3b13_74bc);
/// The camera may not enter this band of yaws, `(π/4, 7π/4)` (`0x020a2718`, `0x020a271c`):
/// it stops on its edge.
const BAND_LOW: i32 = 0xc91;
const BAND_HIGH: i32 = 0x57f7;
/// The return stops this close to the home direction.
const ARRIVED: i32 = 0x28;

/// `maplist9.bin` record `+0xc` bits 0-3: the map kind (`MapRecord::kind`).
const MAP_KIND_MASK: u8 = 0xf;
/// Kind of the 65 terrain regions (`F` maps): `Zone3D::SwitchZone` records the last one in
/// the protagonist; their camera is fixed.
const MAP_KIND_TERRAIN: u8 = 0;
/// `maplist9.bin` record `+0xc` bit 7: keeps the field camera's turn. Clear on 12 story
/// maps (`E01M01`…), `O00`/`O01`, `X03M01`, `X04` and `F99M01`/`F99M02`.
const MAP_TURN_BIT: u8 = 0x80;

/// Whether `L`/`R` may turn the field camera on a map, from its `maplist9.bin` record's
/// `+0xc` byte (`ov_017::0219e234`, called on every map change): the game sets bit 4 of the
/// camera's `+0x244` when the map kind is 0 or when `+0xc` bit 7 is clear, and
/// `FieldCamera_Update` (`0x020a213c`) then reads no shoulder button.
///
/// Bit 1 of the same byte is a different, transient lock (a message window or a warp menu
/// sets it; `ov_017::021a7378`, `ov_017::021a9454`), not a map rule.
pub fn rotation_allowed(byte_c: u8) -> bool {
    byte_c & MAP_KIND_MASK != MAP_KIND_TERRAIN && byte_c & MAP_TURN_BIT != 0
}

/// The camera's turn state: whether it is returning to its home direction (`+0x245`, bit 1),
/// and whether this map keeps the camera fixed (`+0x244` bit 4, [`rotation_allowed`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldCamera {
    returning: bool,
    locked: bool,
}

impl FieldCamera {
    /// A camera for a map whose `maplist9.bin` record has this `+0xc` byte: the lock bit is
    /// [`rotation_allowed`] (`ov_017::0219e234`). A locked camera still updates (it follows the
    /// hero and finishes a return), but reads no shoulder button.
    pub fn new(byte_c: u8) -> Self {
        Self {
            returning: false,
            locked: !rotation_allowed(byte_c),
        }
    }

    pub fn is_returning(&self) -> bool {
        self.returning
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// One update of `delta_ms` milliseconds (the game's effective delta time, 33 at 30 Hz)
    /// with the left (`L`, turns the yaw up) and right (`R`, down) shoulder buttons held;
    /// `home` is the direction both buttons return to. Returns the new yaw.
    pub fn step(&mut self, yaw: i32, home: i32, left: bool, right: bool, delta_ms: u32) -> i32 {
        let (mut left, mut right) = (left, right);
        if self.locked {
            left = false;
            right = false;
        }
        if self.returning {
            if !left && !right && yaw == home {
                self.returning = false;
            }
            left = false;
            right = false;
        }
        let delta = delta_ms as f32;
        if self.returning {
            let difference = fx::signed_angle_distance(yaw, home);
            let far = fx::abs(difference);
            let step = ((RETURN_RATE * delta) * 4096.0) as i32;
            let step = step.min(far >> 1);
            if far < ARRIVED {
                return home;
            }
            return reduce_angle(yaw + if difference < 0 { -step } else { step });
        }
        let step = ((TURN_RATE * delta) * 4096.0) as i32;
        match (left, right) {
            (true, true) => {
                self.returning = true;
                yaw
            }
            (true, false) => {
                let turned = reduce_angle(yaw + step);
                if turned > BAND_LOW && turned < BAND_HIGH {
                    BAND_LOW
                } else {
                    turned
                }
            }
            (false, true) => {
                let turned = reduce_angle(yaw - step);
                if turned > BAND_LOW && turned < BAND_HIGH {
                    BAND_HIGH
                } else {
                    turned
                }
            }
            (false, false) => yaw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terrain_region_locks_the_turn() {
        // `F02` (0x10), `K03` (0xa0): kind 0.
        assert!(!rotation_allowed(0x10));
        assert!(!rotation_allowed(0x00));
        assert!(!rotation_allowed(0xa0));
        // A kind-0 record with bit 7 set is still a terrain region.
        assert!(!rotation_allowed(0x90));
        assert!(rotation_allowed(0x92));
    }

    #[test]
    fn a_record_without_bit_7_locks_the_turn() {
        // `E01M01` (0x02), `O00` (0x57), `X04` (0x11), `F99M01` (0x11).
        assert!(!rotation_allowed(0x02));
        assert!(!rotation_allowed(0x57));
        assert!(!rotation_allowed(0x11));
        assert!(!rotation_allowed(0x66));
    }

    #[test]
    fn an_ordinary_map_allows_the_turn() {
        // `X01M05` (0x82), `C01M09` (0xa2), a `B` backdrop (0xa4), every kind with bit 7.
        assert!(rotation_allowed(0x82));
        assert!(rotation_allowed(0xa2));
        assert!(rotation_allowed(0xa4));
        for kind in 1..=0xf {
            assert!(rotation_allowed(0x80 | kind), "kind {kind}");
        }
    }

    #[test]
    fn a_locked_map_ignores_the_buttons() {
        // `F02` (`+0xc` = 0x10): holding `L` or `R` leaves the yaw where it is.
        let mut camera = FieldCamera::new(0x10);
        assert!(camera.is_locked());
        assert_eq!(camera.step(0, 0, true, false, 33), 0);
        assert_eq!(camera.step(0x600, 0, false, true, 33), 0x600);
        // Both buttons do not start a return either.
        assert_eq!(camera.step(0x600, 0, true, true, 33), 0x600);
        assert!(!camera.is_returning());
        // An ordinary map (`C01M09`, 0xa2) is not locked.
        assert!(!FieldCamera::new(0xa2).is_locked());
        // A map without bit 7 (`E01M01`, 0x02) locks too.
        assert!(FieldCamera::new(0x02).is_locked());
    }

    #[test]
    fn one_button_turns_and_stops_at_the_band() {
        let mut camera = FieldCamera::default();
        let mut yaw = 0;
        for _ in 0..200 {
            yaw = camera.step(yaw, 0, true, false, 17);
        }
        assert_eq!(yaw, BAND_LOW);
        for _ in 0..400 {
            yaw = camera.step(yaw, 0, false, true, 17);
        }
        assert_eq!(yaw, BAND_HIGH);
        assert_eq!(camera.step(yaw, 0, false, false, 17), yaw);
    }

    #[test]
    fn turn_speed_follows_the_frame_time() {
        let mut camera = FieldCamera::default();
        // 0.0015 * 33 ms * 4096 = 202.7: 202 units at 30 Hz.
        assert_eq!(camera.step(0, 0, true, false, 33), 202);
        assert_eq!(camera.step(0, 0, false, true, 33), 0x6488 - 202);
    }

    #[test]
    fn both_buttons_bring_the_camera_home() {
        let mut camera = FieldCamera::default();
        let mut yaw = 0x600;
        yaw = camera.step(yaw, 0, true, true, 17);
        assert!(camera.is_returning());
        assert_eq!(yaw, 0x600);
        let mut frames = 0;
        while yaw != 0 {
            yaw = camera.step(yaw, 0, false, false, 17);
            frames += 1;
            assert!(frames < 100);
        }
        camera.step(yaw, 0, false, false, 17);
        assert!(!camera.is_returning());
    }
}
