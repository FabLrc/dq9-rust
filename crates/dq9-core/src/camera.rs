//! The field camera's turn: the shoulder buttons rotate it around the hero within a quarter
//! turn of its home direction, both together bring it back (`FieldCamera_Update`,
//! `0x020a213c`; `docs/re/movement.md`, « Caméra de terrain »). Only the yaw is modelled: the
//! camera's height, distance and target follow the application's camera.
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

/// The camera's turn state: whether it is returning to its home direction (`+0x245`, bit 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldCamera {
    returning: bool,
}

impl FieldCamera {
    pub fn is_returning(&self) -> bool {
        self.returning
    }

    /// One update of `delta_ms` milliseconds (the game's effective delta time, 33 at 30 Hz)
    /// with the left (`L`, turns the yaw up) and right (`R`, down) shoulder buttons held;
    /// `home` is the direction both buttons return to. Returns the new yaw.
    pub fn step(&mut self, yaw: i32, home: i32, left: bool, right: bool, delta_ms: u32) -> i32 {
        let (mut left, mut right) = (left, right);
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
