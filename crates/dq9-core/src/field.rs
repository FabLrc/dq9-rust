//! Field regions: what the game does each frame with the leader's position and the regions
//! of the map's `.bmbl` (`docs/re/map-archive.md`, « Portes et changements de carte »,
//! `docs/re/trigger.md`, « Zones »). No I/O: the caller supplies the parsed regions
//! (`dq9_formats::map::Regions`) and the bytes of the scenario trigger file.
//!
//! [`Field::step`] runs, for one map and one leader pose, the checks of the field task
//! (`ov_017::0219755c`): doorways (`ov_017::02197a64`, `02198f84`), areas of the map
//! (`ov_017::0219814c`), areas added by triggers (`ov_017::02198e30`) and map-changing warps
//! (`ov_017::0219ca88`). It returns [`Event`]s: map changes the application carries out, and
//! trigger requests (`dq9_core::trigger::Request`) it cannot honour yet.
//!
//! Positions are fixed point (`fx32`, 4096 = 1.0) inside the tests, like the game; the
//! public pose is in world units (`f32`).
//!
//! port: 0x020321f0, 0x02031128, 0x02032158, 0x02094bac, 0x02064b08
//! port: ov_017::0219814c, ov_017::02198e30

use std::f32::consts::PI;

use dq9_formats::map::{
    FLAG_SIDE, FLAG_WARP, Regions, Warp, ZONE_AREA, ZONE_DOORWAY, Zone, fix32_cos, fix32_sin,
    reduce_angle,
};
use dq9_formats::table::{Table, Value};
use dq9_formats::trigger::{Op, Trigger};
use glam::Vec3;
use nds_rom::Result;

use crate::trigger::{
    Flow, Raw, Request, State, TRACKS, Unsupported, degrees_to_angle, first_match, run_action,
    run_deferred, track_of_room,
};

/// One turn as `fx32` (`0x6488`).
const TWO_PI: i32 = 0x6488;
/// Events of `docs/re/trigger.md`.
const EVENT_TALK: i32 = 0;
const EVENT_TALK_END: i32 = 1;
const EVENT_END: i32 = 11;
const EVENT_ENTER_AREA: i32 = 2;
const EVENT_ARRIVAL: i32 = 3;
const EVENT_LEAVE_AREA: i32 = 5;
const EVENT_WARP: i32 = 6;
const EVENT_BATTLE_WON: i32 = 15;
const EVENT_BATTLE_LOST: i32 = 16;
const EVENT_DOORWAY: i32 = 17;
const EVENT_LOADED: i32 = 20;
/// Dot-product band of the doorway approach test (`0x170 - 0xccc`, `ov_017::02197a64`):
/// the hero must walk or look within about 45 degrees of the door's axis.
const APPROACH_BAND: i32 = 0xccc - 0x170;
/// Consecutive frames walking into a doorway before it reacts (`+0x42ec` counts to 3).
const APPROACH_FRAMES: i16 = 3;
/// Doorway flag set by trigger actions 0x6c/0x88 (« already opened »): the doorway is
/// skipped (`ov_017::02197a64` tests bits 0 and 2).
const DOORWAY_USED: u16 = 0x4;
const DOORWAY_DISABLED: u16 = 0x1;

pub use crate::fx::Fx3;

/// `fx32` of a world coordinate (`_ffix` of `4096 * x`, truncating).
pub fn to_fx(position: Vec3) -> Fx3 {
    position.to_array().map(|v| (v * 4096.0) as i32)
}

fn from_fx(v: Fx3) -> Vec3 {
    Vec3::from_array(v.map(|c| c as f32 / 4096.0))
}

/// The game's object angle (0 faces +z, `(sin a, cos a)` is the direction) as the yaw of
/// [`crate::Hero`] and the application camera (0 faces -z).
pub fn yaw_of_angle(angle: f32) -> f32 {
    PI - angle
}

/// Inverse of [`yaw_of_angle`].
pub fn angle_of_yaw(yaw: f32) -> f32 {
    PI - yaw
}

/// `(a * a + 0x800) >> 12`, the handlers' fixed-point square.
fn square(a: i32) -> i32 {
    ((i64::from(a) * i64::from(a) + 0x800) >> 12) as i32
}

/// `func_02031118` (`0x02031128`): whether `p` is inside the box `[high x y z, low x y z]`,
/// edges included.
fn in_box(p: Fx3, b: &[i32; 6]) -> bool {
    !(b[0] < p[0] || b[1] < p[1] || b[2] < p[2] || p[0] < b[3] || p[1] < b[4]) && b[5] <= p[2]
}

/// `func_020321e0` (`0x020321f0`): the box test with a rotation about Y. A rotated box first
/// refuses a point further across the ground from `center` than `radius_sq` allows, then
/// turns the point back about the center by `-angle` (`RotationMatrixY`,
/// `MTX_MultVec43`: `x' = x cos a - z sin a`, `z' = x sin a + z cos a`).
fn in_rotated_box(p: Fx3, b: &[i32; 6], angle: i32, center: Fx3, radius_sq: i32) -> bool {
    let mut q = p;
    if angle != 0 {
        let d = [p[0] - center[0], p[1] - center[1], p[2] - center[2]];
        if radius_sq < square(d[0]).wrapping_add(square(d[2])) {
            return false;
        }
        // `RotationMatrixY(fix32ReduceAngle0To2Pi(-angle))`: the matrix of `MTX_RotY43_`
        // is `[c 0 -s; 0 1 0; s 0 c]` applied to a row vector.
        let turn = reduce_angle(angle.wrapping_neg());
        let (sin, cos) = (i64::from(fix32_sin(turn)), i64::from(fix32_cos(turn)));
        let x = ((i64::from(d[0]) * cos + i64::from(d[2]) * sin) >> 12) as i32;
        let z = ((-i64::from(d[0]) * sin + i64::from(d[2]) * cos) >> 12) as i32;
        q = [
            x.wrapping_add(center[0]),
            d[1].wrapping_add(center[1]),
            z.wrapping_add(center[2]),
        ];
    }
    in_box(q, b)
}

/// `func_02032148` (`0x02032158`): the vertical cylinder from `center` up by `height`, of
/// radius `radius`.
fn in_cylinder(p: Fx3, center: Fx3, radius: i32, height: i32) -> bool {
    if p[1] < center[1] || center[1].wrapping_add(height) < p[1] {
        return false;
    }
    square(p[2].wrapping_sub(center[2])).wrapping_add(square(p[0].wrapping_sub(center[0])))
        <= square(radius)
}

/// Region bit: the zone holds once, then never again (`+0x28 & 1`).
const ONCE: u32 = 1;
/// Region bit: the hero has been inside a once-only zone.
const ENTERED: u32 = 2;
/// Region bit: a once-only zone was entered and left.
const SPENT: u32 = 4;
/// Region bit: the zone never holds.
const OFF: u32 = 8;

/// A zone of the map and its runtime bits (`+0x28`, and `+0x2e >> 4` of a doorway).
#[derive(Debug, Clone)]
struct Slot {
    zone: Zone,
    /// `+0x28`: [`ONCE`], [`ENTERED`], [`SPENT`], [`OFF`]. No record sets any at load.
    bits: u32,
    /// The doorway's flags with their runtime bits.
    flags: u16,
}

impl Slot {
    fn new(zone: &Zone) -> Self {
        Self {
            flags: zone.flags,
            zone: zone.clone(),
            bits: 0,
        }
    }

    /// `func_02094b9c` (`0x02094bac`): whether the zone holds the point; may update its
    /// once-only bits. A depth below 1 makes a cylinder of radius width and height `height`.
    fn holds(&mut self, p: Fx3) -> bool {
        if self.bits & OFF != 0 {
            return false;
        }
        let v = &self.zone.volume;
        let half = v.size.map(|s| s >> 1);
        let high = [0, 1, 2].map(|i| v.center[i].wrapping_add(half[i]));
        let low = [0, 1, 2].map(|i| v.center[i].wrapping_sub(half[i]));
        let inside = if v.size[2] < 1 {
            in_cylinder(p, v.center, v.size[0], v.size[1])
        } else {
            let b = [high[0], high[1], high[2], low[0], low[1], low[2]];
            in_rotated_box(p, &b, i32::from(v.angle), v.center, v.radius_sq)
        };
        let mut holds = inside;
        if inside {
            if self.bits & ONCE != 0 {
                let live = self.bits & SPENT == 0;
                if live {
                    self.bits |= ENTERED;
                }
                holds = live;
            }
        } else if self.bits & ONCE != 0 && self.bits & ENTERED != 0 {
            self.bits |= SPENT;
        }
        holds
    }
}

/// An area added by a trigger's action 0x8f (`Trig_RunAction` case `0x2b`, struct of 0x30
/// bytes, appended by `Trig_AddArea`, `0x02064b08`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct AddedArea {
    id: u16,
    angle: i32,
    center: Fx3,
    radius_sq: i32,
    /// Upper then lower corner.
    bounds: [i32; 6],
}

/// Float operand as the compiler stores positions: times 4096, rounded half away from zero.
fn fixed(value: &Value<'_>) -> i32 {
    let f = value.float() * 4096.0;
    (if f >= 0.0 { f + 0.5 } else { f - 0.5 }) as i32
}

impl AddedArea {
    /// Action 0x8f: area `arg`, upper corner, lower corner, angle in degrees. The squared
    /// radius is made from the box's width and **height** (`docs/re/trigger.md`).
    fn new(op: &Op<'_>) -> Option<Self> {
        let v = op.operands.get(..7)?;
        let high = [0, 1, 2].map(|i| fixed(&v[i]));
        let low = [0, 1, 2].map(|i| fixed(&v[3 + i]));
        // degrees * pi / 180 * 4096 in `f32`, truncated, then folded into one turn.
        let degrees = v[6].int() as i32 as f32;
        let mut angle = i32::from(((degrees * PI) / 180.0 * 4096.0) as i16);
        if angle > TWO_PI {
            angle -= TWO_PI;
        } else if angle < 0 {
            angle += TWO_PI;
        }
        let size = [0, 1, 2].map(|i| high[i].wrapping_sub(low[i]));
        Some(Self {
            id: op.arg,
            angle,
            center: [0, 1, 2].map(|i| high[i].wrapping_add(low[i]) >> 1),
            radius_sq: square(size[0] / 2).wrapping_add(square(size[1] / 2)),
            bounds: [high[0], high[1], high[2], low[0], low[1], low[2]],
        })
    }

    fn holds(&self, p: Fx3) -> bool {
        in_rotated_box(p, &self.bounds, self.angle, self.center, self.radius_sq)
    }
}

/// Whether `position` is inside the box of a `0x72` warp (`ov_017::0219ca88`'s test).
pub fn warp_contains(warp: &Warp, position: Vec3) -> bool {
    let v = &warp.volume;
    let b = [
        v.high[0], v.high[1], v.high[2], v.low[0], v.low[1], v.low[2],
    ];
    in_rotated_box(
        to_fx(position),
        &b,
        i32::from(v.angle),
        v.center,
        v.radius_sq,
    )
}

/// Whether `position` is inside a zone, without its once-only bits (`0x02094bac`).
pub fn zone_contains(zone: &Zone, position: Vec3) -> bool {
    Slot::new(zone).holds(to_fx(position))
}

/// The leader: position, facing and requested direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leader {
    pub position: Vec3,
    /// Facing as a [`crate::Hero`] yaw.
    pub yaw: f32,
    /// World direction the player asks for (horizontal), zero when idle.
    pub input: Vec3,
}

/// Where a map change comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// A `0x72` warp (`kind`: 1 for the doors whose approach the game animates).
    Warp { index: usize, kind: i32 },
    /// A doorway touched, by its class and number.
    Doorway { class: u8, number: u8 },
    /// A trigger action asked for it.
    Trigger,
}

/// Leave for another map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapChange {
    /// Map id (`maplist9.bin`).
    pub destination: u16,
    /// Arrival position, `None` when the cause gives none (the application picks one).
    pub position: Option<Vec3>,
    /// Arrival orientation as the game's object angle; see [`yaw_of_angle`].
    pub angle: Option<f32>,
    /// Entry point of the destination (`0x72` value 15, trigger action 0x85…) **[inconnu]**.
    pub entry: Option<i32>,
    pub cause: Cause,
}

/// What the talk event (event 0) did, see [`Field::talk`].
#[derive(Debug, Clone, PartialEq)]
pub enum TalkTrigger {
    /// No trigger holds: the conversation starts with the zone's context.
    NoTrigger,
    /// A trigger ran; its requests replace the conversation (a [`Request::Talk`] starts it with
    /// the context the trigger gives).
    Fired(Vec<Event>),
    /// The trigger reached a condition that is not modelled and was not evaluated.
    Unsupported(u16),
}

/// What [`Field`] asks of the application.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    MapChange(MapChange),
    /// A request of a fired trigger the engine cannot carry out before J3 (conversations,
    /// scripted events, removals…).
    Request(Request),
    /// A trigger reached a condition that is not modelled and was not evaluated.
    Unsupported(u16),
}

/// A way out of a map (see [`Field::exits`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exit {
    /// The map it leads to by itself; `None` for a doorway only a trigger sends somewhere.
    pub destination: Option<u16>,
    /// A point inside its volume.
    pub position: Vec3,
    /// Direction to walk to touch a doorway (zero for a warp).
    pub heading: Vec3,
}

/// Map state of the field: regions with their runtime bits, the area the leader stands in,
/// the areas triggers added, and the trigger file of the map.
#[derive(Debug, Clone)]
pub struct Field {
    room: u16,
    warps: Vec<Warp>,
    /// Newest first, like the game's lists.
    doorways: Vec<Slot>,
    areas: Vec<Slot>,
    added: Vec<AddedArea>,
    trigger_file: Option<Vec<u8>>,
    /// Index in `areas` of the area holding the leader (stored on the leader object).
    area: Option<usize>,
    /// Id of the added area holding the leader (`+0x491` of the trigger object).
    added_area: Option<u16>,
    /// Field manager `+0x4446`: id of the added area the leader last entered.
    pub entered_area: Option<u16>,
    /// `+0x42ec`: frames spent walking into a doorway.
    approach: i16,
}

/// `data/scenario/trigger<xxx>.bin` of a map code (`Trig_LoadMap`, `0x0206462c`): the first
/// three characters, or `F` alone for the field maps.
pub fn trigger_path(code: &str) -> Option<String> {
    let prefix = if code.starts_with('F') {
        "F"
    } else {
        code.get(..3)?
    };
    Some(format!("data/scenario/trigger{prefix}.bin"))
}

/// Trigger state of a new game at `story`: flags, quests and the same progress on every
/// scenario track (the real game keeps one per track; a single story point is all the
/// application is given). The time of day is a nonzero value by day **[hypothèse]**.
pub fn initial_state(story: &dq9_formats::cast::Story) -> State {
    let mut state = State {
        flags: story.flags.clone(),
        quests: story.quests.clone(),
        time_of_day: if story.daytime { 2 } else { 0 },
        ..State::default()
    };
    for track in &mut state.tracks[..TRACKS] {
        (track.chapter, track.step, track.substep) = (story.chapter, story.step, story.substep);
    }
    state
}

impl Field {
    /// The field of map `room`: its regions and the bytes of its trigger file, if any.
    /// Errors when the trigger file is not a record table.
    pub fn new(room: u16, regions: &Regions, trigger_file: Option<Vec<u8>>) -> Result<Self> {
        if let Some(bytes) = &trigger_file {
            dq9_formats::trigger::parse(&Table::parse(bytes)?)?;
        }
        Ok(Self {
            room,
            warps: regions.warps.clone(),
            doorways: regions.zones_of(ZONE_DOORWAY).map(Slot::new).collect(),
            areas: regions.zones_of(ZONE_AREA).map(Slot::new).collect(),
            added: Vec::new(),
            trigger_file,
            area: None,
            added_area: None,
            entered_area: None,
            approach: 0,
        })
    }

    pub fn room(&self) -> u16 {
        self.room
    }

    /// The map is loaded: selects the scenario track of the room (`FUN_02064ba8`), then
    /// runs event 20 (the trigger table was loaded: where triggers add their areas) and,
    /// after a transition, event 3 (arrival).
    pub fn enter(&mut self, state: &mut State, after_transition: bool) -> Vec<Event> {
        state.track = track_of_room(i32::from(self.room));
        let mut events = Vec::new();
        let loaded = self.fire(EVENT_LOADED, &[(3, u32::from(self.room))], state);
        self.collect(loaded, None, &mut events);
        if after_transition {
            let arrival = self.fire(EVENT_ARRIVAL, &[(3, u32::from(self.room))], state);
            self.collect(arrival, None, &mut events);
        }
        events
    }

    /// One fixed step of the field task with the leader's pose after movement.
    pub fn step(&mut self, leader: &Leader, state: &mut State) -> Vec<Event> {
        let mut events = Vec::new();
        let p = to_fx(leader.position);
        if self.step_doorways(leader, p, state, &mut events) {
            return events;
        }
        self.step_areas(p, state, &mut events);
        self.step_added_areas(p, state, &mut events);
        self.step_warps(p, state, &mut events);
        events
    }

    /// An event ended (`EventTask_Update`, `ov_017::021bc77c`): fires trigger event 11 with
    /// the event number in word 2, and returns what its actions ask for (typically the next
    /// event or a map change).
    pub fn event_ended(&mut self, event: u16, state: &mut State) -> Vec<Event> {
        let mut events = Vec::new();
        let ended = self.fire(EVENT_END, &[(2, u32::from(event))], state);
        self.collect(ended, None, &mut events);
        events
    }

    /// A conversation ended (`TalkTask_Update`, `ov_017::021b8e8c`, closing states): fires trigger
    /// event 1 with the NPC in word 0 and the conversation's context in word 5; the answer to
    /// the last prompt (0 first choice, 1 second; 0 without a prompt: the message window sets
    /// it to 0 as it opens) is what condition 0x10 reads. The actions are what a reply sets in
    /// motion: an event, an item, the story's progress.
    pub fn talk_ended(
        &mut self,
        npc: u16,
        context: u16,
        answer: Option<u8>,
        state: &mut State,
    ) -> Vec<Event> {
        state
            .raw
            .insert(Raw::Field954, u32::from(answer.unwrap_or(0)));
        let words = [(0, u32::from(npc)), (5, u32::from(context))];
        let mut events = Vec::new();
        let ended = self.fire(EVENT_TALK_END, &words, state);
        self.collect(ended, None, &mut events);
        events
    }

    /// A scripted battle ended (`BattleTask_Update`, `ov_017::021b790c`, `Trig_FireBattleEnd` and
    /// `ov_017::021b7c3c`): fires trigger event 15 (won) or 16 (lost) with the battle number in
    /// word 6, and returns what its actions ask for (the event that follows the fight).
    pub fn battle_ended(&mut self, battle: u16, won: bool, state: &mut State) -> Vec<Event> {
        let kind = if won {
            EVENT_BATTLE_WON
        } else {
            EVENT_BATTLE_LOST
        };
        let mut events = Vec::new();
        let ended = self.fire(kind, &[(6, u32::from(battle))], state);
        self.collect(ended, None, &mut events);
        events
    }

    /// The ways out of the map: the `0x72` warps and the doorways, with a point inside each and,
    /// for a doorway, the direction to walk to touch it. For an autopilot that has to leave a
    /// room; the game only has the hero walk into them.
    pub fn exits(&self) -> Vec<Exit> {
        let warps = self.warps.iter().map(|w| Exit {
            destination: Some(w.destination),
            position: from_fx(w.volume.center),
            heading: Vec3::ZERO,
        });
        let doorways = self.doorways.iter().map(|slot| {
            let facing = direction(i32::from(slot.zone.volume.facing));
            Exit {
                destination: slot
                    .zone
                    .warp
                    .filter(|_| slot.flags & FLAG_WARP != 0)
                    .map(|w| w.destination),
                position: from_fx(slot.zone.volume.center),
                heading: Vec3::new(facing[0], 0.0, facing[1]),
            }
        });
        warps.chain(doorways).collect()
    }

    /// A point inside each area of the map and each area a trigger added (event 2 fires when the
    /// leader gets into one). For an autopilot; the game has the hero walk in.
    pub fn area_points(&self) -> Vec<Vec3> {
        let map = self.areas.iter().map(|s| s.zone.volume.center);
        let added = self.added.iter().map(|a| a.center);
        map.chain(added).map(from_fx).collect()
    }

    /// Whether any map change is among `events`.
    pub fn changes_map(events: &[Event]) -> bool {
        events.iter().any(|e| matches!(e, Event::MapChange(_)))
    }

    /// `ov_017::02197a64`, then `02198f84` once the hero has walked into a doorway long
    /// enough: event 17, then the doorway's own destination unless a trigger replaced it.
    /// Returns whether the map changes.
    fn step_doorways(
        &mut self,
        leader: &Leader,
        p: Fx3,
        state: &mut State,
        events: &mut Vec<Event>,
    ) -> bool {
        let facing = fx_angle(angle_of_yaw(leader.yaw));
        let look = direction(facing);
        let walk = [leader.input.x, leader.input.z];
        let moving = walk != [0.0, 0.0];
        let mut touched: Vec<usize> = Vec::new();
        let mut counted = false;
        let mut process = false;
        for index in 0..self.doorways.len() {
            if touched.len() > 1 {
                break;
            }
            let slot = &mut self.doorways[index];
            if slot.flags & (DOORWAY_DISABLED | DOORWAY_USED) != 0 || !slot.holds(p) {
                continue;
            }
            let door = direction(i32::from(slot.zone.volume.facing));
            // Where he looks, or where he asks to go when he asks for a direction.
            let toward = dot(door, walk);
            let aligned = if moving { toward } else { dot(door, look) };
            let accepted = if slot.flags & FLAG_SIDE == 0 {
                aligned < 1
            } else {
                let to_hero = normalized([
                    (p[0] - slot.zone.volume.center[0]) as f32,
                    (p[2] - slot.zone.volume.center[2]) as f32,
                ]);
                let side = dot(door, to_hero);
                if side < 1 { aligned > 0 } else { aligned < 0 }
            };
            if !accepted {
                continue;
            }
            touched.push(index);
            if self.approach < APPROACH_FRAMES {
                let straight = toward.abs() > APPROACH_BAND || aligned.abs() > APPROACH_BAND;
                if moving && straight {
                    if !counted {
                        counted = true;
                        self.approach += 1;
                    }
                } else {
                    self.approach = 0;
                }
            } else if !counted {
                process = true;
            }
        }
        if !process {
            return false;
        }
        for index in touched {
            if self.touch_doorway(index, state, events) {
                return true;
            }
        }
        false
    }

    /// `ov_017::02198f84` for one queued doorway.
    fn touch_doorway(&mut self, index: usize, state: &mut State, events: &mut Vec<Event>) -> bool {
        let slot = &self.doorways[index];
        let (class, number, facing) = (slot.zone.class, slot.zone.number, slot.zone.volume.facing);
        let words = [
            (7, u32::from(class) | u32::from(number) << 16),
            (10, u32::from(facing as u16)),
        ];
        let fired = self.fire(EVENT_DOORWAY, &words, state);
        let slot = &mut self.doorways[index];
        if slot.flags & DOORWAY_USED != 0 {
            // Already opened: forget it and start over.
            slot.flags &= !DOORWAY_USED;
            self.approach = 0;
            self.collect(fired, None, events);
            return false;
        }
        let own = slot.zone.warp.filter(|_| slot.flags & FLAG_WARP != 0);
        let cause = Cause::Doorway { class, number };
        let fallback = own.map(|w| MapChange {
            destination: w.destination,
            position: Some(from_fx(w.arrival)),
            angle: Some(f32::from(w.arrival_angle) / 4096.0),
            entry: None,
            cause,
        });
        let changed = self.collect(fired, fallback, events);
        if own.is_none() {
            self.approach = 0;
        }
        changed
    }

    /// `ov_017::0219814c`: events 5 and 2 as the leader moves between the areas of the map.
    fn step_areas(&mut self, p: Fx3, state: &mut State, events: &mut Vec<Event>) {
        let now = self.areas.iter_mut().position(|slot| slot.holds(p));
        let previous = self.area;
        if previous != now {
            if let Some(left) = previous {
                let number = u32::from(self.areas[left].zone.class);
                let fired = self.fire(EVENT_LEAVE_AREA, &[(1, number)], state);
                self.collect(fired, None, events);
            }
            if let Some(entered) = now {
                let number = u32::from(self.areas[entered].zone.class);
                let fired = self.fire(EVENT_ENTER_AREA, &[(1, number)], state);
                self.collect(fired, None, events);
            }
        }
        self.area = now;
    }

    /// `ov_017::02198e30`: the same for the areas triggers added with action 0x8f.
    fn step_added_areas(&mut self, p: Fx3, state: &mut State, events: &mut Vec<Event>) {
        let now = self.added.iter().find(|area| area.holds(p)).map(|a| a.id);
        let previous = self.added_area;
        match (previous, now) {
            (None, None) => {}
            (Some(left), None) => {
                let fired = self.fire(EVENT_LEAVE_AREA, &[(1, u32::from(left))], state);
                self.collect(fired, None, events);
                self.added_area = None;
            }
            (previous, Some(entered)) => {
                if previous != Some(entered) {
                    if let Some(left) = previous {
                        let fired = self.fire(EVENT_LEAVE_AREA, &[(1, u32::from(left))], state);
                        self.collect(fired, None, events);
                    }
                    self.entered_area = Some(entered);
                    let fired = self.fire(EVENT_ENTER_AREA, &[(1, u32::from(entered))], state);
                    self.collect(fired, None, events);
                    self.added_area = Some(entered);
                }
            }
        }
    }

    /// `ov_017::0219ca88`: the leader inside the box of a `0x72` warp changes map; event 6
    /// runs first with the room. The game animates the approach of kind 1 doors and tests
    /// a leader state this model lacks (`func_02039820`): the change is immediate here.
    fn step_warps(&mut self, p: Fx3, state: &mut State, events: &mut Vec<Event>) {
        let Some((index, warp)) = self.warps.iter().enumerate().find(|(_, w)| {
            let v = &w.volume;
            let b = [
                v.high[0], v.high[1], v.high[2], v.low[0], v.low[1], v.low[2],
            ];
            in_rotated_box(p, &b, i32::from(v.angle), v.center, v.radius_sq)
        }) else {
            return;
        };
        let warp = warp.clone();
        let fired = self.fire(EVENT_WARP, &[(3, u32::from(self.room))], state);
        let own = MapChange {
            destination: warp.destination,
            position: Some(from_fx(warp.arrival)),
            angle: Some(f32::from(warp.arrival_angle) / 4096.0),
            entry: (warp.entry >= 0).then_some(warp.entry),
            cause: Cause::Warp {
                index,
                kind: warp.kind,
            },
        };
        self.collect(fired, Some(own), events);
    }

    /// The doorways in the game's list order as `(class, number, flags)`, flags with their
    /// runtime bits.
    pub fn doorway_flags(&self) -> Vec<(u8, u8, u16)> {
        self.doorways
            .iter()
            .map(|s| (s.zone.class, s.zone.number, s.flags))
            .collect()
    }

    /// Fires `event` with parameter words `words` (index, value): the first matching
    /// trigger's actions run until one stops the list, adding the areas of action 0x8f as
    /// they come; the deferred actions of a list that ran to its end are then applied.
    /// `Trig_Fire` (`0x020649c0`) and `Trig_RunDeferred` (`0x0206f82c`).
    fn fire(
        &mut self,
        event: i32,
        words: &[(usize, u32)],
        state: &mut State,
    ) -> std::result::Result<Vec<Request>, Unsupported> {
        self.fire_matched(event, words, state)
            .map(Option::unwrap_or_default)
    }

    /// [`Field::fire`], telling whether a trigger matched at all: `None` when none did (what
    /// `Trig_Fire` reports as 0), `Some(requests)` when one ran, possibly without a request
    /// (its action list stopped, or it only changed progress).
    fn fire_matched(
        &mut self,
        event: i32,
        words: &[(usize, u32)],
        state: &mut State,
    ) -> std::result::Result<Option<Vec<Request>>, Unsupported> {
        let Some(bytes) = self.trigger_file.as_deref() else {
            return Ok(None);
        };
        let Ok(table) = Table::parse(bytes) else {
            return Ok(None);
        };
        let Ok(all) = dq9_formats::trigger::parse(&table) else {
            return Ok(None);
        };
        let track = &state.tracks[state.track];
        let (chapter, step) = (i32::from(track.chapter), i32::from(track.step));
        let triggers: Vec<Trigger<'_>> = all
            .into_iter()
            .filter(|t| t.loaded(i32::from(self.room), chapter, step))
            .collect();
        state.event = [0; 12];
        for &(index, value) in words {
            state.event[index] = value;
        }
        let matched = first_match(&triggers, event, &*state);
        let result = match matched {
            Err(e) => Err(e),
            Ok(None) => Ok(None),
            Ok(Some(trigger)) => {
                let mut deferred = Vec::new();
                let mut completed = true;
                for action in &trigger.actions {
                    match action.code {
                        0x8f => {
                            if let Some(area) = AddedArea::new(action) {
                                self.added.push(area);
                            }
                            continue;
                        }
                        0x6c | 0x88 => {
                            mark_doorways(&mut self.doorways, action, action.code == 0x6c);
                            continue;
                        }
                        _ => {}
                    }
                    match run_action(action, state) {
                        Flow::Next => {}
                        Flow::Defer(op) => deferred.push(op),
                        Flow::Stop => {
                            completed = false;
                            break;
                        }
                    }
                }
                Ok(Some(if completed {
                    run_deferred(&deferred, state)
                } else {
                    Vec::new()
                }))
            }
        };
        state.event = [0; 12];
        result
    }

    /// Event 0 of the map's triggers: the hero talks to NPC `npc`, standing in its talk zone
    /// `context` (0 outside any), after `Talk_Start` (`ov_017::021a4cf0`). The event words
    /// are 0 = NPC and 5 = zone; conditions on the hero's zone read `state.zones`
    /// (`crate::talk::zones_containing`).
    pub fn talk(&mut self, npc: u16, context: u16, state: &mut State) -> TalkTrigger {
        let words = [(0, u32::from(npc)), (5, u32::from(context))];
        match self.fire_matched(EVENT_TALK, &words, state) {
            Ok(None) => TalkTrigger::NoTrigger,
            Ok(Some(requests)) => {
                let mut events = Vec::new();
                self.collect(Ok(requests), None, &mut events);
                TalkTrigger::Fired(events)
            }
            Err(Unsupported(code)) => TalkTrigger::Unsupported(code),
        }
    }

    /// Turns the outcome of a fired event into events. A warp request replaces `fallback`,
    /// the map change the region would make by itself; without a request `fallback` is
    /// returned as is. Returns whether a map change was pushed.
    fn collect(
        &mut self,
        fired: std::result::Result<Vec<Request>, Unsupported>,
        fallback: Option<MapChange>,
        events: &mut Vec<Event>,
    ) -> bool {
        let requests = match fired {
            Ok(requests) => requests,
            Err(Unsupported(code)) => {
                events.push(Event::Unsupported(code));
                Vec::new()
            }
        };
        let mut change = fallback;
        for request in requests {
            match request {
                Request::Warp {
                    room,
                    entry,
                    position,
                    direction,
                } => {
                    let own = change.filter(|c| c.destination == room);
                    change = Some(MapChange {
                        destination: room,
                        position: position.map(from_fx).or(own.and_then(|c| c.position)),
                        angle: direction
                            .map(|d| f32::from(d) / 4096.0)
                            .or(own.and_then(|c| c.angle)),
                        entry: entry.map(i32::from).or(own.and_then(|c| c.entry)),
                        cause: Cause::Trigger,
                    });
                }
                // The trigger objects are rebuilt from the file at each event.
                Request::ReloadTriggers => {}
                other => events.push(Event::Request(other)),
            }
        }
        let changed = change.is_some();
        if let Some(change) = change {
            events.push(Event::MapChange(change));
        }
        changed
    }
}

/// Actions 0x6c and 0x88 (`Trig_RunAction` cases 8 and `0x24`): set or clear the « used »
/// bit of the doorways of class `arg`, number operand 0 and facing operand 1 (degrees).
/// A used doorway is skipped; setting it leaves a doorway with the runtime bit 0 alone.
fn mark_doorways(doorways: &mut [Slot], op: &Op<'_>, used: bool) {
    let (Some(number), Some(angle)) = (op.operands.first(), op.operands.get(1)) else {
        return;
    };
    let number = number.int() as u16;
    let angle = degrees_to_angle(angle.int() as i32);
    for slot in doorways {
        let zone = &slot.zone;
        if u16::from(zone.class) != op.arg
            || u16::from(zone.number) != number
            || zone.volume.facing as u16 != angle
        {
            continue;
        }
        if !used {
            slot.flags &= !DOORWAY_USED;
        } else if slot.flags & DOORWAY_DISABLED == 0 {
            slot.flags |= DOORWAY_USED;
        }
    }
}

/// `fx32` of an angle in radians, as 16 bits kept in a `fix32sin` argument.
fn fx_angle(radians: f32) -> i32 {
    reduce_angle((radians * 4096.0) as i32)
}

/// Horizontal unit vector `(sin a, cos a)` of an `fx32` angle (`ov_017::02197a64`).
fn direction(angle: i32) -> [f32; 2] {
    normalized([
        fix32_sin(angle) as f32 / 4096.0,
        fix32_cos(angle) as f32 / 4096.0,
    ])
}

fn normalized(v: [f32; 2]) -> [f32; 2] {
    let length = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if length == 0.0 {
        [0.0, 0.0]
    } else {
        [v[0] / length, v[1] / length]
    }
}

/// `VEC_DotProduct` of two horizontal vectors, as `fx32`.
fn dot(a: [f32; 2], b: [f32; 2]) -> i32 {
    ((a[0] * b[0] + a[1] * b[1]) * 4096.0) as i32
}

#[cfg(test)]
mod tests;
