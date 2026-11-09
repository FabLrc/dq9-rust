//! A roaming monster in the field: the object `func_02028a54` allocates (`0x198` bytes, twelve
//! per map) and its thirteen-state machine (`func_02076b58` runs the update of state `+0x130`
//! from the table at `0x020e893c`; `func_02076ccc` changes state through the enter functions of
//! `0x020e89a4`).
//!
//! The movement is `Character_Update` (`0x020332bc`, `docs/re/movement.md`) with the roamer's
//! own constants: the turn rate `+0xb0`, the speed set by each state's enter function
//! (`+0xb2` = `+0xb4`, so no acceleration), the collision body of its name record.

use super::tables::MonsterField;
use crate::collision::{Body, CollisionWorld, Mode, Wall};
use crate::fx::{self, Fx3, fix32_cos, fix32_sin, reduce_angle};

/// The states of `+0x130`.
pub mod state {
    /// Just spawned: waits 1 s, then stands.
    pub const APPEAR: u8 = 0;
    /// Standing; after 4 s it picks a destination (`0x02076e04`).
    pub const IDLE: u8 = 1;
    /// Walking to the node `+0x164` (`0x0207720c`).
    pub const WALK: u8 = 2;
    /// Fading out, then in at the destination node (`0x02077640`).
    pub const BLINK: u8 = 3;
    /// Walking after the party (`0x02077798`).
    pub const CHASE: u8 = 4;
    /// Dashing at where the party stood, ten units ahead (`0x02077bf0`).
    pub const DASH: u8 = 5;
    /// Running ten units away (`0x02078060`).
    pub const FLEE: u8 = 6;
    /// Walking away, then fading out and gone (`0x020783a8`).
    pub const VANISH: u8 = 7;
    /// Touched by the party: faces it, stands still (the encounter, `0x02196430`).
    pub const TOUCHED: u8 = 8;
    /// The monster the battle is about.
    pub const FOUGHT: u8 = 9;
    /// Has seen the party: after 226 ms it decides to fight or flee (`0x020789a8`).
    pub const NOTICE: u8 = 10;
    /// Defeated: fading out.
    pub const DEFEATED: u8 = 11;
    /// Startled, stands 3 s then resumes wandering (`0x02078d94`).
    pub const STARTLED: u8 = 12;
}

/// Alpha of a fully visible object (`GetInheritedAlpha` = 31).
pub const OPAQUE: i32 = 0x1f;
/// Turn rate of a walking roamer: `0x328` per tick (`+0xb0`, set by `0x02076db0`).
const TURN: i32 = 0x328;
/// Speed of a running roamer (`0x1c2`) and of the walk classes 1 and 2.
const RUN: i32 = 0x1c2;
const WALK_SLOW: i32 = 0x9a;
const WALK_FAST: i32 = 0xe6;
/// Model scale of a roamer of size 1.0: `0x10a` (the constants at `0x021a23ac`, `0x021d48ec`).
pub const MODEL_SCALE: i32 = 0x10a;

/// One roaming monster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roamer {
    /// `+2`: the monster's number.
    pub number: u16,
    /// `+0x16a`: its id among the map's roamers (what a battle request names it by).
    pub id: u16,
    /// `+0x168`: the zone it came from.
    pub zone: u16,
    /// `+0x44`.
    pub position: Fx3,
    /// `+0x54`: where the model faces.
    pub angle: i32,
    /// `+0x130`.
    pub state: u8,
    /// `+0x134`: the state before.
    pub previous: u8,
    /// `+0x14c`: milliseconds in this state.
    pub state_ms: u32,
    /// `+0x154`: milliseconds out of the appear state (the notice test waits 2 s).
    pub awake_ms: u32,
    /// `GetInheritedAlpha`, 0 to 31.
    pub alpha: i32,
    /// The alpha a fade is going to and the milliseconds it takes.
    pub fade: Option<(i32, i32)>,
    /// `+0xb8`: the index of the node it stands on.
    pub node: usize,
    /// `+0x164`: the id of the node it is going to.
    pub destination: u8,
    /// `+0x158`: where it is going.
    pub target: Fx3,
    /// `+0xae`: the heading it turns to.
    pub heading: i32,
    /// `+0xb0`: the turn rate.
    pub turn: i32,
    /// `+0xb2`: the speed per tick.
    pub speed: i32,
    pub walking: bool,
    /// `+0x17b`: it gave up or arrived beside the party.
    pub arrived: bool,
    /// `+0x17c`: it was blocked by a wall.
    pub blocked: bool,
    /// `+0x17d` bit 6: it has picked a node since it stood on its spawn node.
    pub started: bool,
    /// The size it was spawned at (`fx32`, 4096 = 1.0) and the model scale that gives.
    pub size: i32,
    pub scale: i32,
    /// Gravity (`+0xac`), collision size (`+0x64`, `+0x68`).
    body: Body,
    pub field: MonsterField,
}

impl Roamer {
    /// `func_02021a2128`'s object: at `position`, facing `angle`, standing on node index
    /// `node` (id `destination`), in the appear state.
    #[allow(clippy::too_many_arguments)] // the eight values `ov_017::021a2128` takes
    pub(super) fn spawn(
        number: u16,
        id: u16,
        zone: u16,
        field: MonsterField,
        size: i32,
        position: Fx3,
        angle: i32,
        node: usize,
        destination: u8,
    ) -> Self {
        let scale = fx::mul(MODEL_SCALE, size);
        Self {
            number,
            id,
            zone,
            position,
            angle: reduce_angle(angle),
            state: state::APPEAR,
            previous: state::APPEAR,
            state_ms: 0,
            awake_ms: 0,
            alpha: OPAQUE,
            fade: None,
            node,
            destination,
            target: position,
            heading: reduce_angle(angle),
            turn: TURN,
            speed: 0,
            walking: false,
            arrived: false,
            blocked: false,
            started: false,
            size,
            scale,
            body: Body {
                position,
                fall: 0,
                radius: field.radius,
                height: field.height,
                wall: None,
            },
            field,
        }
    }

    /// The touch radius: `(size / 2) × (scale × 15)` in `fx32`, rounded
    /// (`func_ov017_02196fc8` with the model flag, `func_02077a20`).
    pub fn touch_radius(&self) -> i32 {
        let product = i64::from(i32::from(self.field.size) >> 1) * i64::from(self.scale * 15);
        ((product + 0x800) >> 12) as i32
    }

    /// `func_02076ccc`: changes state through the enter function. States 8 to 10 need a party
    /// member to face (`hero`).
    pub fn enter(&mut self, to: u8, hero: Fx3) -> bool {
        if self.state == to {
            return true;
        }
        let toward = |from: Fx3, at: Fx3| fx::normalize(fx::sub(at, from));
        match to {
            state::APPEAR => {
                self.speed = 0;
                self.walking = false;
            }
            state::IDLE => {
                self.walking = false;
                self.turn = TURN;
                self.speed = 0;
                self.started = self.started && self.state != state::APPEAR;
            }
            state::WALK | state::BLINK => {
                // The destination node's position is put in `target` by the caller.
                if to == state::BLINK {
                    self.speed = 0;
                    self.fade = Some((0, 250));
                } else {
                    self.set_speed_class();
                    self.walking = true;
                }
            }
            state::CHASE => {
                self.face(hero);
                self.turn = 0x97;
                self.speed = RUN;
                self.walking = true;
            }
            state::DASH | state::FLEE | state::VANISH => {
                let mut direction = toward(self.position, hero);
                if to != state::DASH {
                    direction = direction.map(|c| -c);
                }
                self.heading = fx::atan2(direction[0], direction[2]);
                if to != state::DASH {
                    direction[1] = 0;
                }
                let reach = match to {
                    state::VANISH => 0x5000,
                    _ => 0xa000,
                };
                self.target = fx::add(self.position, fx::scale(direction, reach));
                self.speed = RUN;
                self.walking = true;
            }
            state::TOUCHED | state::FOUGHT | state::DEFEATED => {
                self.face(hero);
                self.speed = 0;
                self.walking = false;
                if to == state::DEFEATED {
                    self.fade = Some((0, 125));
                } else {
                    self.alpha = OPAQUE;
                    self.fade = None;
                }
            }
            state::NOTICE => {
                self.face(hero);
                self.speed = 0;
                self.walking = false;
                self.alpha = OPAQUE;
                self.fade = None;
                self.blocked = false;
            }
            state::STARTLED => {
                self.walking = false;
                self.speed = 0;
                self.turn = TURN;
                self.face(hero);
            }
            _ => return false,
        }
        self.previous = self.state;
        self.state = to;
        self.state_ms = 0;
        true
    }

    fn set_speed_class(&mut self) {
        self.speed = match self.field.walk {
            1 => WALK_SLOW,
            2 => WALK_FAST,
            3 => RUN,
            _ => 0,
        };
    }

    /// Faces `at` (`FX_Atan2` of the normalised bearing).
    fn face(&mut self, at: Fx3) {
        let direction = fx::normalize(fx::sub(at, self.position));
        self.heading = fx::atan2(direction[0], direction[2]);
    }

    /// The ground distance to `at` (`y` zeroed).
    pub(super) fn flat_distance(&self, at: Fx3) -> i32 {
        fx::distance([self.position[0], 0, self.position[2]], [at[0], 0, at[2]])
    }

    /// The fade of one 30 Hz update of `delta_ms`: the alpha moves `31 × delta / duration`
    /// towards its target (**[hypothesis]** for the steps; the end points are the game's).
    pub(super) fn fade_step(&mut self, delta_ms: i32) {
        let Some((target, duration)) = self.fade else {
            return;
        };
        let step = ((OPAQUE * delta_ms) / duration).max(1);
        self.alpha = if self.alpha < target {
            (self.alpha + step).min(target)
        } else {
            (self.alpha - step).max(target)
        };
        if self.alpha == target {
            self.fade = None;
        }
    }

    /// `Character_Update` for one update of `ticks` frames: turn to [`Self::heading`], move
    /// along it while walking, collide, fall. Returns the wall it touched, if any.
    pub(super) fn advance(&mut self, world: &CollisionWorld, ticks: i32) -> Option<Wall> {
        let turn = self.turn * ticks;
        let distance = fx::signed_angle_distance(self.angle, self.heading);
        if distance > 0 {
            self.angle = if distance < turn {
                self.heading
            } else {
                self.angle + turn
            };
        } else if distance < 0 {
            self.angle = if -distance < turn {
                self.heading
            } else {
                self.angle - turn
            };
        }
        self.angle = reduce_angle(self.angle);
        let previous = self.body.position;
        if self.walking && self.speed > 0 {
            let direction = [fix32_sin(self.heading), 0, fix32_cos(self.heading)];
            self.body.position =
                fx::add(self.body.position, fx::scale(direction, self.speed * ticks));
        }
        let y = self.body.position[1] - i32::from(self.body.fall as i16);
        self.body.position[1] = y;
        let travelled = fx::distance(previous, self.body.position);
        let mode = if travelled > 0x333 {
            Mode::Halves
        } else {
            Mode::Whole
        };
        world.resolve(&mut self.body, previous, mode);
        self.position = self.body.position;
        self.body.wall
    }

    /// Puts the object at `position` standing (a blink's arrival, a snap to the ground).
    pub(super) fn place(&mut self, position: Fx3) {
        self.body.position = position;
        self.body.fall = 0;
        self.position = position;
    }

    /// The angle between where `facing` points and the bearing from `from` to `to`, as the
    /// signed 16-bit value `ov_017::021a4754` returns (absolute).
    pub fn angle_to(from: Fx3, facing: i32, to: Fx3) -> i32 {
        let direction = fx::normalize(fx::sub(to, from));
        let bearing = fx::atan2(direction[0], direction[2]);
        let angle = fx::signed_angle_distance(facing, bearing);
        i32::from(fx::abs(i32::from(angle as i16)) as i16)
    }

    /// Whether the party at `hero` is within `reach` and within 45° of where the monster
    /// heads (`ov_017::02079248`: `FX_Div(dot, |v|) > 0xb50`), or all round when `ahead` is false
    /// (`0x020793d0`).
    pub(super) fn sees(&self, hero: Fx3, reach: i32, ahead: bool) -> bool {
        let v = fx::sub(hero, self.position);
        let distance = fx::mag(v);
        if ahead {
            if distance > reach {
                return false;
            }
            let facing = [fix32_sin(self.heading), 0, fix32_cos(self.heading)];
            fx::div(fx::dot(v, facing), distance) > 0xb50
        } else {
            distance < reach
        }
    }

    /// The facing tests compare an angle with `0.7 × π` (`0x460cbe66`, 9007.6 in `fx32` angle
    /// units): the wiki's 49.48° reads the angle as a fraction of `0x10000`.
    pub const BACK_TURNED: f32 = f32::from_bits(0x460c_be66);
}
