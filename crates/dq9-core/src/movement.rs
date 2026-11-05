//! The hero on foot in the field: the d-pad and the camera give a heading, the character
//! turns towards it and accelerates, then the map pushes it out of walls and stands it on the
//! floor (`docs/re/movement.md`). Fixed point, deterministic, no I/O.
//!
//! One [`Hero::step`] follows `ov_017::0218e210` for the party leader: the generic update
//! (`PartyMember_Update` `0x02037d98`: `Character_Update` `0x020332bc` — turn, speed, motion,
//! gravity — then the d-pad `0x02038148`), the map collision
//! ([`CollisionWorld::resolve`]), the slide along the wall touched, and the limits on the
//! distance travelled. A key pressed on one step moves the character on the next, as in the
//! game.
//!
//! Not modelled: the characters and zone objects that push the leader, the magnet of the
//! oriented zones, the stylus, ladders, events that take the controls, and the animations
//! (states other than standing and walking).

use dq9_formats::map::{fix32_cos, fix32_sin};
use glam::Vec3;

use crate::collision::{Body, CollisionWorld, Mode};
use crate::field::{angle_of_yaw, yaw_of_angle};
use crate::fx::{self, Fx3, PI, reduce_angle};

/// Top walking speed per tick (`+0xb4`; `ov_017::0218f064` stores `0x189`): 0.096 unit.
pub const WALK_SPEED: i32 = 0x189;
/// Speed gained or lost per tick (`+0xb6`, `0x02032e68`).
pub const ACCELERATION: i32 = 0x28;
/// Angle the model turns per tick (`+0xb0`, `0x324`).
pub const TURN_SPEED: i32 = 0x324;
/// Collision sphere of the party members (`ov_017::0218f064`: `SetRadius(0x1000)`,
/// `SetHeight(0x1800)`).
pub const RADIUS: i32 = 0x1000;
pub const HEIGHT: i32 = 0x1800;
/// A move longer than this in one step is resolved in two halves (`iRam0218e660`).
const FAST_MOVE: i32 = 0x333;
/// Motion shorter than this per axis is cancelled (`0x0218e5b4`).
const REST: i32 = 5;
/// Headings of the eight d-pad directions, in the camera's frame (`0x020e90b8`): up, down,
/// left, right, up-left, up-right, down-left, down-right.
const HEADINGS: [i32; 8] = [0x3244, 0, 0x4b66, 0x1922, 0x3ed5, 0x25b3, 0x57f7, 0xc91];
/// The slide along a wall (`ov_017::0218e210`): floats of `0x0218e664`, `0x0218e66c`,
/// `0x0218e670`, and the right angle `0x0218e668`.
const SLIDE_LIMIT: f32 = f32::from_bits(0x4620_d99a);
const SLIDE_TURN: f32 = f32::from_bits(0x45d3_a50e);
const SLIDE_TOLERANCE: f32 = f32::from_bits(0x4420_d99a);
const RIGHT_ANGLE: i32 = 0x1922;

/// The direction keys held on a step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl Input {
    /// The keys that walk along a world direction as seen by a camera at yaw 0 (north up,
    /// `-z`): east `+x` is right, `-z` up.
    pub fn from_direction(direction: Vec3) -> Self {
        Self {
            up: direction.z < -0.5,
            down: direction.z > 0.5,
            left: direction.x < -0.5,
            right: direction.x > 0.5,
        }
    }

    /// Index into [`HEADINGS`] (`0x02038148`: the diagonals win, then up, down, left, right);
    /// `None` without a direction key.
    fn heading_index(self) -> Option<usize> {
        Some(match (self.up, self.down, self.left, self.right) {
            (true, _, true, _) => 4,
            (true, _, _, true) => 5,
            (_, true, true, _) => 6,
            (_, true, _, true) => 7,
            (true, ..) => 0,
            (_, true, ..) => 1,
            (_, _, true, _) => 2,
            (_, _, _, true) => 3,
            _ => return None,
        })
    }
}

/// The party leader walking in the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hero {
    /// Position, fall speed, collision size and last wall (`+0x44`, `+0xac`).
    body: Body,
    /// `+0x54`: where the model faces.
    angle: i32,
    /// `+0xae`: where the d-pad (or a wall) turns it to.
    target: i32,
    /// `+0xb2`: current speed per tick.
    speed: i32,
    /// State 1 (walking) against 0 (standing).
    walking: bool,
}

impl Hero {
    /// Stands a hero at `position` facing `angle` (`fx32`; 0 faces `+z`), as the party is
    /// placed on a map (`0x020399c0`: target and model angle set, speed 0).
    pub fn place(position: Fx3, angle: i32) -> Self {
        let angle = reduce_angle(angle);
        Self {
            body: Body {
                position,
                fall: 0,
                radius: RADIUS,
                height: HEIGHT,
                wall: None,
            },
            angle,
            target: angle,
            speed: 0,
            walking: false,
        }
    }

    /// [`Hero::place`] from a pose in world units, `yaw` being the application's heading
    /// (see [`yaw_of_angle`]).
    pub fn at(position: Vec3, yaw: f32) -> Self {
        let angle = reduce_angle((angle_of_yaw(yaw) * 4096.0) as i32);
        Self::place(position.to_array().map(|v| (v * 4096.0) as i32), angle)
    }

    pub fn position_fx(&self) -> Fx3 {
        self.body.position
    }

    pub fn position(&self) -> Vec3 {
        Vec3::from_array(self.body.position.map(|c| c as f32 / 4096.0))
    }

    /// The model's facing, `fx32` (0 faces `+z`).
    pub fn angle(&self) -> i32 {
        self.angle
    }

    /// The model's facing as the application's yaw.
    pub fn yaw(&self) -> f32 {
        yaw_of_angle(self.angle as f32 / 4096.0)
    }

    /// The angle the hero is turning to.
    pub fn target_angle(&self) -> i32 {
        self.target
    }

    /// Current speed per tick (`fx32`).
    pub fn speed(&self) -> i32 {
        self.speed
    }

    pub fn is_walking(&self) -> bool {
        self.walking
    }

    /// The wall touched on the last step.
    pub fn wall(&self) -> Option<crate::collision::Wall> {
        self.body.wall
    }

    /// Moves the hero to `position` standing, keeping its facing (a teleport inside the map).
    pub fn set_position(&mut self, position: Fx3) {
        self.body.position = position;
        self.body.fall = 0;
    }

    /// One update of `ticks` frames (1 at 60 Hz; the game counts the frames since its last
    /// update, 2 when it runs at 30 Hz) with `input` held and the camera at `camera_yaw`
    /// (`fx32`, 0 looks towards `-z`).
    pub fn step(&mut self, world: &CollisionWorld, input: Input, camera_yaw: i32, ticks: i32) {
        let previous = self.body.position;
        self.update(ticks);
        self.read_input(input, camera_yaw);
        let travelled = fx::distance(previous, self.body.position);
        let mode = if travelled > FAST_MOVE {
            Mode::Halves
        } else {
            Mode::Whole
        };
        if world
            .resolve(&mut self.body, previous, mode)
            .started_falling
        {
            self.walking = false;
        }
        self.slide_along_wall(previous);
        let position = &mut self.body.position;
        let moved = fx::sub(*position, previous);
        if moved.iter().all(|&c| fx::abs(c) < REST) {
            *position = previous;
        }
        let corrected = fx::distance(previous, *position);
        if 0 < travelled && travelled < corrected {
            let direction = fx::normalize(fx::sub(*position, previous));
            *position = fx::add(previous, fx::scale(direction, travelled));
        }
    }

    /// `Character_Update` (`0x020332bc`) for the hero: turn, speed, motion, gravity.
    fn update(&mut self, ticks: i32) {
        // `0x02033720`: the model turns to the target at `TURN_SPEED` per tick, stopping on it.
        let turn = TURN_SPEED * ticks;
        let distance = fx::signed_angle_distance(self.angle, self.target);
        if distance > 0 {
            self.angle = if distance < turn {
                self.target
            } else {
                self.angle + turn
            };
        } else if distance < 0 {
            self.angle = if -distance < turn {
                self.target
            } else {
                self.angle - turn
            };
        }
        self.angle = reduce_angle(self.angle);
        // `0x0203349c`: the speed moves towards the top speed while walking and towards 0
        // otherwise; the motion of this tick uses the speed of the previous one.
        let before = self.speed;
        let gain = (ACCELERATION * ticks) as i16;
        self.speed = i32::from(if self.walking {
            if before < WALK_SPEED {
                (before as i16).wrapping_add(gain)
            } else if before - WALK_SPEED < ACCELERATION * ticks {
                WALK_SPEED as i16
            } else {
                (before as i16).wrapping_sub(gain)
            }
        } else if before < 1 {
            0
        } else {
            (before as i16).wrapping_sub(gain)
        });
        if before > 0 {
            // Turning slows the walk down in proportion to the angle left to turn; the
            // direction followed is the target's (the hero's `0x100` flag).
            let left = fx::abs(fx::signed_angle_distance(self.angle, self.target));
            let ratio = fx::div(left, PI);
            let limit = ((i64::from(WALK_SPEED) * i64::from(0x1000 - ratio) + 0x800) >> 12) as i32;
            let speed = before.min(limit);
            let direction = [fix32_sin(self.target), 0, fix32_cos(self.target)];
            self.body.position = fx::add(self.body.position, fx::scale(direction, speed * ticks));
        }
        let y = self.body.position[1] - i32::from(self.body.fall as i16);
        self.body.position[1] = if y < -0x32000 { 0x32000 } else { y };
    }

    /// `Hero_ReadDirectionInput` (`0x02038148`): the keys give the heading, relative to the
    /// camera, and the walking state.
    fn read_input(&mut self, input: Input, camera_yaw: i32) {
        match input.heading_index() {
            None => self.walking = false,
            Some(i) => {
                self.walking = true;
                self.target = reduce_angle(reduce_angle(HEADINGS[i] + camera_yaw));
            }
        }
    }

    /// The end of `ov_017::0218e210`: after a wall was touched, a hero that still moves turns
    /// to follow it, unless it was heading straight into it.
    fn slide_along_wall(&mut self, previous: Fx3) {
        let Some(wall) = self.body.wall else { return };
        if fx::distance(previous, self.body.position) <= 0x27 {
            return;
        }
        let travel = fx::normalize(fx::sub(self.body.position, previous));
        let travel_heading = fx::atan2(travel[0], travel[2]);
        let normal = fx::normalize([wall.normal[0], 0, wall.normal[2]]);
        let wall_heading = fx::atan2(normal[0], normal[2]);
        let difference = fx::signed_angle_distance(wall_heading, self.target);
        let away = fx::abs(difference);
        if away as f32 >= SLIDE_LIMIT {
            return;
        }
        let mut heading = self.target;
        if away > RIGHT_ANGLE {
            let wall = wall_heading as f32;
            let slid = if difference > 0 {
                SLIDE_TURN + wall
            } else {
                wall - SLIDE_TURN
            };
            heading = reduce_angle(slid as i32);
        }
        if (fx::abs(fx::signed_angle_distance(heading, travel_heading)) as f32) < SLIDE_TOLERANCE {
            self.target = reduce_angle(heading);
        }
    }
}

#[cfg(test)]
mod tests;
