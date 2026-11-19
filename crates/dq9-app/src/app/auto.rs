//! `--headless --auto`: plays the story without a window, pressing the action key to read the
//! messages, until the player would get control (no event runs or waits) or the run is stuck,
//! and prints what the story did (`Journal`). With `--pilot`, a free control is not the end:
//! the autopilot walks to the conversation, area or door that moves the story on.
//!
//! The pilot uses the game's own movement: every frame it feeds the d-pad a world direction
//! (the keys a player would hold), the hero turns and accelerates (`Hero::step`), the map's
//! spheres and floor push it (`CollisionWorld`), and only [`Field::step`] decides that a door,
//! an area or a talk zone was reached. It never moves the hero: routes come from a walkable
//! grid built by probing the map's collision with the hero's own radius, and the action key is
//! pressed with [`Talk::press`] once `Talk_NearestNpc` sees the intended NPC. What it walks
//! to is what the game offers: the standing points of the partner search, the areas of the
//! `.bmbl`, and the map's warps and doorways.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use dq9_core::field::{Event, Exit, exit_contains, trigger_path};
use dq9_core::scenario::Step;
use dq9_core::talk::Target;
use dq9_core::trigger::Request;
use dq9_core::trigger::State;
use dq9_formats::Table;
use dq9_formats::trigger::parse as parse_triggers;
use glam::Vec3;
use nds_rom::Rom;

use self::nav::{MAX_FLOORS, NavGrid};
use super::App;
use crate::talk::Button;

mod approach;
mod frame;
mod nav;
mod pursue;
mod route;
mod session;
pub(crate) use self::session::Session;
#[cfg(target_arch = "wasm32")]
pub(crate) use self::session::SessionSpec;
#[cfg(test)]
mod tests;

/// Steps between two presses of the action key while a scene runs (a message needs its text
/// typed before the key advances it).
const PRESS_EVERY: u64 = 20;
/// Things the autopilot may do in one run.
const MAX_ACTIONS: u32 = 600;
/// Things the autopilot may do in one route-mode run: the walks to an inn and to a field and
/// the exits they take count too.
const MAX_ROUTE_ACTIONS: u32 = 50_000;
/// Conversations the pilot has with one NPC, on one line, at one point of the story.
const MAX_TALKS: u8 = 2;
/// Distance under which a waypoint counts as reached.
const WAYPOINT_REACH: f32 = 0.55;
/// Height under which a waypoint or a goal counts as reached. A walkway above a cell centre
/// is a **different** node of the grid: a flat-only test accepts the landing's centre while
/// the hero still stands on a balcony four units above it (`X01M01`, y 8.25 over 4.25), and a
/// loose tolerance lets the hero "reach" the first stairway step from the ramp surface above
/// it, then press east against geometry without ever descending. Half an edge's climb
/// ([`nav::STEP_HEIGHT`]) plus the standing offset keeps the test on the waypoint's own floor.
const LEVEL_REACH: f32 = 0.8;
/// Frames of walking without progress before the route is planned again, then given up.
const REPLAN_FRAMES: u32 = 45;
const STUCK_FRAMES: u32 = 120;
/// Frames the pilot may spend turning to a partner before it gives up.
const TURN_FRAMES: u32 = 90;
/// Frames spent walking into a doorway before the exit counts as taken.
const DOOR_FRAMES: u32 = 20;
/// Frames spent pressing into a warp's trigger volume from the end of a route that already
/// stands in it: a warp fires from presence, so this only absorbs the edge of the box.
const WARP_FRAMES: u32 = 60;
/// How far from a warp's volume, on the ground, the hero may start walking in by himself.
const APPROACH_REACH: f32 = 7.0;
/// Places tried as the start of such a walk, the nearest to the volume first.
const APPROACH_STARTS: usize = 6;

/// Why [`App::play_auto`] stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub enum Stop {
    /// No event runs or waits and the autopilot, if any, has nothing left to try: the player
    /// has control on this map.
    Control { map: u16 },
    /// A walking goal could not be reached.
    Stuck { map: u16, why: String },
    /// The step limit was reached with a scene still running.
    Limit,
    /// The game was never started (no ROM, or the first scene did not load).
    NotStarted,
    /// Route mode: the event the run was asked to reach has ended.
    Reached { event: u16 },
    /// The caller's own condition held ([`App::play_auto_until`]): a run asked to stop once
    /// the journal shows one thing does not spend its budget on what the pilot does after it.
    Done,
}

/// One thing the pilot decided to do, with the point it walks to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Goal {
    /// Talk to `target` from `position`, facing roughly `yaw`.
    Talk {
        target: Target,
        position: Vec3,
        yaw: f32,
    },
    /// Step into area `index` at `point`.
    Area { index: usize, point: Vec3 },
    /// Route mode: walk to the roamer that stands at `point`, and let it touch the party.
    Hunt { point: Vec3 },
    /// Route mode: walk to `point`, somewhere the field's monsters will find the party.
    Roam { point: Vec3 },
    /// Leave by `exit`. `position` is where the route ends: a standing pose inside a warp's
    /// trigger volume, the ground in front of a doorway, or, with `stage`, a place the grid
    /// reaches near a warp whose volume it does not join.
    Exit {
        exit: Exit,
        position: Vec3,
        /// The last metres are searched with the hero's own movement from the pose he gets
        /// to (`approach::search`), then fed as presses.
        stage: bool,
    },
}

impl Goal {
    fn point(&self) -> Vec3 {
        match *self {
            Self::Talk { position, .. } => position,
            Self::Area { point, .. } | Self::Hunt { point } | Self::Roam { point } => point,
            Self::Exit { position, .. } => position,
        }
    }

    /// [`exit_key`] of the way out the goal means to take, if it is one.
    fn exit_key(&self) -> Option<u64> {
        match *self {
            Self::Exit { exit, .. } => Some(exit_key(&exit)),
            _ => None,
        }
    }
}

/// What the autopilot remembers.
#[derive(Default)]
struct Pilot {
    /// Conversations had: `(room, npc, context, story digest)`; one is not had twice at the
    /// same point of the story.
    talked: BTreeSet<(u16, u16, u16, u64)>,
    /// Conversations started per `(room, npc, context, story point)`. A talk that only toggles
    /// flags changes the story digest each time, so `talked` alone would have it for ever
    /// (NPCs 5, 6 and 8 of Angel Falls at 2.2.3 did, six times each, in `run14`).
    talks: BTreeMap<(u16, u16, u16, u64), u8>,
    /// Exits taken: `(room, destination, story digest)`.
    left: BTreeSet<(u16, u16, u64)>,
    /// Single ways out tried, taken or not: `(room, destination, story digest, exit key)`.
    /// A map may lead to one destination by a warp and by doorways; a walk that fails on one
    /// of them must not forbid the others.
    tried_exits: BTreeSet<(u16, u16, u64, u64)>,
    /// Ways out whose walk failed: `(room, destination, exit key)`, across story digests. A
    /// goal that keeps failing must not be retried forever after each flag change.
    failed_exits: BTreeSet<(u16, u16, u64)>,
    /// Route mode: the way out the pilot last took, `(room, destination, exit key)`, to know the
    /// entrance of the room it is in.
    arrived_by: Option<(u16, u16, u64)>,
    /// Route mode: entrances that lead nowhere on the way to a room (a floor with two stairs
    /// and one way on): `(room, destination, exit key, story digest)`.
    dead_ends: BTreeSet<(u16, u16, u64, u64)>,
    /// Areas walked into: `(room, area index, story digest)`.
    entered: BTreeSet<(u16, usize, u64)>,
    /// The story digest at which each room was last looked at for something to do.
    evaluated: BTreeMap<u16, u64>,
    /// The rooms each room leads to (seen so far).
    graph: BTreeMap<u16, Vec<u16>>,
    /// The ways out of each room to another family of maps (seen so far).
    foreign: BTreeMap<u16, BTreeSet<u16>>,
    /// Every room each room leads to, whatever its family: the map of the whole trip, for the
    /// route mode's walks to an inn or a field.
    links: BTreeMap<u16, Vec<u16>>,
    /// The route mode's state (`auto/route.rs`): none in the story pilot.
    mode: Option<route::Route>,
    /// The rooms the hero was in.
    visited: BTreeSet<u16>,
    /// The room the pilot last chose in, and the one it came from by a map change: an exit
    /// back to it repeats the walk (the tower of the Observatory offers several doorways to
    /// one room, and taking them in map order never reached the next floor).
    room: Option<u16>,
    came_from: Option<u16>,
    /// The room the walkable grids were last checked against: a map load builds a new
    /// collision whose revision starts again.
    built: Option<u16>,
    actions: u32,
    /// The goal being walked to, and the room and story it was chosen for.
    goal: Option<Goal>,
    goal_room: u16,
    goal_digest: u64,
    route: Vec<Vec3>,
    route_at: usize,
    exit_frames: u32,
    turn_frames: u32,
    stall: u32,
    last_position: Option<Vec3>,
    /// Best distance to the current waypoint (progress, not mere jitter).
    best_waypoint: f32,
    replans: u32,
    stuck: Option<String>,
    /// Walkable grids by room (rebuilt when the collision changes: a story-driven placement
    /// hide, native 574/575, or a door opening).
    grids: BTreeMap<u16, NavGrid>,
    /// [`dq9_core::CollisionWorld::revision`] the grids were built with.
    grid_revision: u64,
    /// What the grid of each room was built under (`App::grid_key`): a grid is kept across two
    /// visits of a room, in the route mode, only when none of it changed.
    grid_keys: BTreeMap<u16, (u64, u32, u64)>,
    /// Doorways already opened: `(class, number)`.
    opened: BTreeSet<(u8, u8)>,
    /// Fixed steps to let a door finish opening before choosing again.
    wait: u32,
    /// Completed doors the walkable grid was built with.
    grid_doors: usize,
    /// Grid nodes the walk could not leave: `(room, cell, level)`, learned when stuck. A cell
    /// may stack floors (a walkway over a hall), so blocking the cell alone would forbid the
    /// other floor too.
    blocked: BTreeSet<(u16, usize, u8)>,
    /// The presses of the walk into a warp's volume (one per field update) and how many were
    /// fed: the hero walks it with his own movement, the same one the search ran.
    approach: Vec<Vec3>,
    approach_at: usize,
}

/// Identity of one way out of a map for the pilot's book: class and number name a doorway,
/// while the position and destination separate the warps of one map (a library warp and the
/// doorway to the same map are distinct exits).
fn exit_key(exit: &Exit) -> u64 {
    let mut hasher = DefaultHasher::new();
    exit.destination.hash(&mut hasher);
    exit.door.is_some().hash(&mut hasher);
    exit.class.hash(&mut hasher);
    exit.number.hash(&mut hasher);
    exit.position.x.to_bits().hash(&mut hasher);
    exit.position.z.to_bits().hash(&mut hasher);
    hasher.finish()
}

/// The closed internal doorway (type 2, no destination) nearest to the straight path from
/// `from` to `target`: opening it may join the two sides of a wall. Doors already opened are
/// skipped.
fn door_towards(
    exits: &[Exit],
    from: Vec3,
    target: Vec3,
    opened: &BTreeSet<(u8, u8)>,
) -> Option<Exit> {
    let a = flat(from);
    let b = flat(target);
    let segment = b - a;
    let length_sq = segment.length_squared();
    let mut candidates: Vec<(f32, Exit)> = exits
        .iter()
        .filter(|exit| {
            exit.destination.is_none()
                && exit.door.is_some()
                && !opened.contains(&(exit.class, exit.number))
        })
        .map(|exit| {
            let point = flat(exit.position);
            let along = if length_sq > 0.0 {
                ((point - a).dot(segment) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let near = a + segment * along;
            ((point - near).length(), *exit)
        })
        .collect();
    candidates.sort_by(|x, y| x.0.total_cmp(&y.0));
    candidates
        .into_iter()
        .find(|(distance, _)| *distance < 6.0)
        .map(|(_, exit)| exit)
}

/// The world direction of a [`dq9_core::Hero`] yaw (0 faces `-z`).
fn facing(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, -yaw.cos())
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The grid nodes of `room` a route must not use: the cells learned when stuck and `avoid`.
fn blocked_nodes(bot: &Pilot, room: u16, avoid: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut blocked: BTreeSet<usize> = bot
        .blocked
        .iter()
        .filter(|(at, _, _)| *at == room)
        .map(|(_, cell, level)| cell * MAX_FLOORS + *level as usize)
        .collect();
    blocked.extend(avoid.iter().copied());
    blocked
}

/// Books the way out as tried and failed: the pilot chooses another, and does not come back to
/// this one after each change of the story.
fn book_failed_exit(bot: &mut Pilot, room: u16, exit: &Exit) {
    let Some(destination) = exit.destination else {
        return;
    };
    let key = exit_key(exit);
    bot.left.insert((room, destination, bot.goal_digest));
    bot.tried_exits
        .insert((room, destination, bot.goal_digest, key));
    bot.failed_exits.insert((room, destination, key));
}

/// The first room on the shortest known way from `room` to a room for which `goal` holds,
/// over the `graph` of rooms to the rooms they lead to.
fn hop_towards(
    graph: &BTreeMap<u16, Vec<u16>>,
    room: u16,
    goal: impl Fn(u16) -> bool,
) -> Option<u16> {
    let mut parent = BTreeMap::new();
    let mut queue = std::collections::VecDeque::from([room]);
    while let Some(at) = queue.pop_front() {
        for &next in graph.get(&at).into_iter().flatten() {
            if next == room || parent.contains_key(&next) {
                continue;
            }
            parent.insert(next, at);
            if goal(next) {
                let mut hop = next;
                while parent[&hop] != room {
                    hop = parent[&hop];
                }
                return Some(hop);
            }
            queue.push_back(next);
        }
    }
    None
}

/// Whether `room` still has something to do at `digest`: the pilot has not looked at it since
/// this state of the story, or one of its known exits has not been taken. The exit choice and
/// the walk back to a room to look at both use this test.
fn pending_room(bot: &Pilot, room: u16, digest: u64) -> bool {
    bot.evaluated.get(&room) != Some(&digest)
        || bot.graph.get(&room).is_some_and(|exits| {
            exits
                .iter()
                .any(|d| !bot.left.contains(&(room, *d, digest)))
        })
}

impl App {
    /// How many scenario rules of room `id` start an event or move the story at this point of
    /// it (`Trigger::loaded`, actions `0x77`, `0x80`, `0x84`, `0x9b`): what the story has to
    /// say there now, from the same trigger data the game loads on arrival. 0 when the room
    /// or its file is unknown.
    fn story_rules_in(&self, id: u16) -> usize {
        let (Some(rom), Some(map)) = (self.rom.as_deref(), self.maps.iter().find(|m| m.id == id))
        else {
            return 0;
        };
        let Some(path) = trigger_path(&map.code) else {
            return 0;
        };
        let Ok(rom) = Rom::parse(rom) else {
            return 0;
        };
        let Some(table) = rom
            .file_by_path(&path)
            .ok()
            .and_then(|data| Table::parse(data).ok())
        else {
            return 0;
        };
        let Ok(rules) = parse_triggers(&table) else {
            return 0;
        };
        let track = &self.state.tracks[self.state.track];
        let (chapter, step) = (i32::from(track.chapter), i32::from(track.step));
        rules
            .iter()
            .filter(|rule| rule.loaded(i32::from(id), chapter, step))
            .filter(|rule| {
                rule.actions
                    .iter()
                    .any(|action| matches!(action.code, 0x77 | 0x80 | 0x84 | 0x9b))
            })
            .count()
    }

    /// A hash of the point of the story only: chapter, step and sub-step of the track.
    fn story_point(&self) -> u64 {
        let track = &self.state.tracks[self.state.track];
        let mut hasher = DefaultHasher::new();
        (track.chapter, track.step, track.substep).hash(&mut hasher);
        hasher.finish()
    }

    /// A hash of where the story is: the progress of every track, their flags and the global
    /// flags.
    fn story_digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for track in &self.state.tracks {
            (track.chapter, track.step, track.substep).hash(&mut hasher);
            track.flags.hash(&mut hasher);
        }
        self.state.flags.hash(&mut hasher);
        hasher.finish()
    }

    /// Runs the game logic for at most `max_steps` fixed steps; stops at the first moment the
    /// player would get control after a scene (`Stop::Control`), or, with `pilot`, when the
    /// autopilot finds nothing that moves the story or cannot walk to it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn play_auto(&mut self, max_steps: u64, pilot: bool) -> Stop {
        self.play_loop(&mut Pilot::default(), max_steps, pilot)
    }

    /// The route mode (`auto/route.rs`): the pilot, with the roaming monsters of the fields on,
    /// resting at the inn and fighting for levels after a defeat, until `event` ends
    /// (`Stop::Reached`) or `max_steps` run out. The closing lines of the journal give the
    /// levels, gold and fights the run took.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn play_route(&mut self, max_steps: u64, event: u16) -> Stop {
        self.set_roaming_enabled(true);
        let seed = self.options.play.seed.unwrap_or(0);
        let mut bot = Pilot {
            mode: Some(route::Route::new(event, seed)),
            ..Pilot::default()
        };
        let stop = self.play_loop(&mut bot, max_steps, true);
        self.route_summary(&bot);
        stop
    }

    /// [`App::play_auto`], stopped as soon as `done` holds after a fixed step: a caller that
    /// waits for one thing in the journal (a map reached, an event played) does not spend the
    /// rest of the budget on what the pilot does afterwards. The pilot's state and the run are
    /// otherwise the ones of [`App::play_auto`]: one `Pilot` for the whole run.
    // Only the ROM-backed tests ask for a condition: the bin targets have no caller.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn play_auto_until(
        &mut self,
        max_steps: u64,
        pilot: bool,
        done: impl Fn(&Self) -> bool,
    ) -> Stop {
        self.play_loop_until(&mut Pilot::default(), max_steps, pilot, &done)
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn play_loop(&mut self, bot: &mut Pilot, max_steps: u64, pilot: bool) -> Stop {
        self.play_loop_until(bot, max_steps, pilot, &|_: &Self| false)
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn play_loop_until(
        &mut self,
        bot: &mut Pilot,
        max_steps: u64,
        pilot: bool,
        done: &dyn Fn(&Self) -> bool,
    ) -> Stop {
        // A loaded map is an already started game: a run from a save/map start (`--auto`
        // without a prologue) must still pilot or report control. `NotStarted` is for a run
        // where no map/ROM ever loaded.
        let mut started = self.current.is_some();
        self.checkpoints.clear();
        for _ in 0..max_steps {
            if let Some(stop) = self.auto_step(bot, pilot, &mut started) {
                return stop;
            }
            if done(self) {
                return Stop::Done;
            }
        }
        if started {
            Stop::Limit
        } else {
            Stop::NotStarted
        }
    }

    /// One fixed step of an automatic run: the action key, the pilot's frame, the game's step,
    /// the checkpoints and the route's bookkeeping. `Some` ends the run. The native
    /// `--headless --auto` loop and the Web session (`auto/session.rs`) both call it, so they
    /// run the same logic.
    fn auto_step(&mut self, bot: &mut Pilot, pilot: bool, started: &mut bool) -> Option<Stop> {
        let recorded = self.journal.steps.len();
        // A fight shown on the window or the Web (`battles=show`) takes many fixed steps: the
        // pilot waits for its end and plays it with the confirm key held, the policy that
        // `Fight::play_out` has (attack the first standing monster). A headless run settles
        // its battles inside the step that starts them, so this never holds there.
        let battling = self.battle_is_active();
        if battling {
            if self.combat.fight.is_some() {
                self.combat.battle_pad.confirm = true;
            }
        } else {
            let busy = self.events.is_active() || self.events.pending_start().is_some();
            if (busy || self.talk.is_active()) && self.tick.is_multiple_of(PRESS_EVERY) {
                self.talk.press(Button::Confirm);
            }
            if pilot {
                self.pilot_frame(bot);
                if let Some(why) = bot.stuck.take() {
                    let map = self.current.as_ref().map_or(0, |(e, _)| e.id);
                    self.journal.record(self.tick, Step::Unhandled(why.clone()));
                    // The goal could not be walked; it is booked as tried, so choose
                    // another. The run only stops when the pilot has nothing left to try.
                    if !self.pursue(bot) {
                        return Some(Stop::Stuck { map, why });
                    }
                }
            }
        }
        self.run_steps(1);
        self.capture_journal(recorded);
        // The route reads a roamer's fight when it is settled: inside the step that made the
        // contact headless, at the end of the shown battle otherwise.
        if bot.mode.is_some() && !self.battle_is_active() {
            self.route_observe(bot, recorded);
            if let Some(event) = bot.mode.as_ref().filter(|m| m.reached).map(|m| m.until) {
                return Some(Stop::Reached { event });
            }
            if bot.mode.as_ref().is_some_and(route::Route::capped) {
                let map = self.current.as_ref().map_or(0, |(e, _)| e.id);
                let why = "route: the grind is capped without the party getting there".into();
                return Some(Stop::Stuck { map, why });
            }
            if self.services.is_open() {
                self.route_service(bot);
            }
        }
        // The cast task of action 0x8d holds the field like an event: no control yet. A
        // facility screen of the route mode holds it as well.
        let busy = self.events.is_active()
            || self.events.pending_start().is_some()
            || self.cast_reload
            || self.battle_is_active()
            || (bot.mode.is_some() && self.services.is_open());
        *started |= busy;
        if *started && !busy && !self.talk.is_active() && !self.talk.confirm_pending() {
            let map = self.current.as_ref().map_or(0, |(e, _)| e.id);
            let track = &self.state.tracks[self.state.track];
            let story = (track.chapter, track.step, track.substep);
            let last = self.journal.steps.iter().rev().find_map(|(_, s)| match s {
                Step::Control { map, story } => Some((*map, *story)),
                _ => None,
            });
            // The pilot crosses rooms: one line per story point while it works, and one
            // for the place where it stops.
            if last.is_none_or(|(_, s)| s != story) {
                self.journal.record(self.tick, Step::Control { map, story });
                self.capture_checkpoint("control");
            }
            if pilot {
                if bot.goal.is_some() {
                    return None;
                }
                if bot.wait > 0 {
                    bot.wait -= 1;
                    return None;
                }
                if self.pursue(bot) {
                    return None;
                }
            }
            if last.is_some_and(|(m, s)| s == story && m != map) {
                self.journal.record(self.tick, Step::Control { map, story });
            }
            return Some(Stop::Control { map });
        }
        None
    }

    /// Captures a checkpoint for every journal step recorded since `from`, with the state at
    /// the end of the step.
    pub(crate) fn capture_journal(&mut self, from: usize) {
        for index in from..self.journal.steps.len() {
            let kind = match &self.journal.steps[index].1 {
                Step::Map { .. } | Step::MapChange { .. } => Some("map"),
                Step::Progress { .. } => Some("progress"),
                Step::EventStarted(_) => Some("event_start"),
                Step::EventEnded(_) => Some("event_end"),
                Step::Battle { .. } => Some("battle"),
                _ => None,
            };
            if let Some(kind) = kind {
                self.capture_checkpoint(kind);
            }
        }
    }

    /// Rebuilds the walkable grid of `room` when the places of this map are not all inside it.
    fn ensure_grid(&self, bot: &mut Pilot, room: u16, points: &[Vec3], base_y: f32) {
        if bot
            .grids
            .get(&room)
            .is_none_or(|grid| !grid.contains(points))
            && let Some(grid) = NavGrid::build(&self.collision, points, base_y)
        {
            bot.blocked.retain(|(at, _, _)| *at != room);
            bot.grids.insert(room, grid);
            bot.grid_keys.insert(room, self.grid_key());
        }
    }

    /// What decides whether the world a grid was probed in is the one loaded now: the story,
    /// the time of day and how many times the collision changed since the map was loaded.
    fn grid_key(&self) -> (u64, u32, u64) {
        (
            self.story_digest(),
            self.state.time_of_day,
            self.collision.revision(),
        )
    }

    /// A route through the grid of `room`, avoiding `avoid` besides the learned obstacles.
    fn route(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        to: Vec3,
        avoid: &BTreeSet<usize>,
    ) -> Option<Vec<Vec3>> {
        bot.grids
            .get(&room)?
            .plan(from, to, &blocked_nodes(bot, room, avoid))
    }

    /// The nodes of a route that must be avoided: the walkable ground inside the trigger volume
    /// of every warp the goal does not mean to take. A `0x72` warp fires from mere presence
    /// (`ov_017::0219ca88`), so a route that crossed one teleported the hero: from `M01`'s
    /// eastern arrival point `(33.17, -0.35, -6)`, every walk to another door crossed the well
    /// warp `(31.25, 0, -6)` of index 6 and landed in `M01M08` again (log of the first chapter-2
    /// route run, `build/evidence/j4-chapter2-route/run1.log`). The exit the goal means to take
    /// is exempt, and so is a volume the hero already stands in: a map spawns him inside its own
    /// return warp (`M01M08` → `1100` at `(33.17, -0.35, -6)`, 1.9 from the centre) and walking
    /// out of it does not fire.
    fn warp_nodes(
        &self,
        grid: &NavGrid,
        exits: &[Exit],
        from: Vec3,
        intended: Option<u64>,
    ) -> BTreeSet<usize> {
        let mut avoid = BTreeSet::new();
        for exit in exits {
            if exit.warp.is_none() || Some(exit_key(exit)) == intended {
                continue;
            }
            if exit_contains(exit, from) {
                continue;
            }
            let nodes = grid.nodes_inside(|point| exit_contains(exit, point));
            if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
                eprintln!(
                    "pilot: warp {} at {:?} holds {} nodes",
                    exit.destination.map_or(0, u16::from),
                    exit.position,
                    nodes.len()
                );
            }
            avoid.extend(grid.expand(&nodes, 1));
        }
        avoid
    }

    /// [`App::warp_nodes`] for the loaded map: empty when no map is loaded or no grid exists
    /// yet.
    fn warp_avoid(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        intended: Option<u64>,
    ) -> BTreeSet<usize> {
        let (Some(field), Some(grid)) = (&self.field, bot.grids.get(&room)) else {
            return BTreeSet::new();
        };
        self.warp_nodes(grid, &field.exits(), from, intended)
    }

    /// How much a conversation or a step into an area moves the story: what it does to the
    /// progress, the events it starts, the map it changes and the flags it raises.
    fn rate(&self, events: &[Event], after: &State) -> (u32, String) {
        let track = &self.state.tracks[self.state.track];
        let after_track = &after.tracks[after.track];
        let mut score = 0;
        let mut what = Vec::new();
        if (track.chapter, track.step, track.substep)
            != (after_track.chapter, after_track.step, after_track.substep)
        {
            score += 4;
            what.push(format!(
                "story {}.{}.{}",
                after_track.chapter, after_track.step, after_track.substep
            ));
        }
        for event in events {
            match event {
                Event::Request(Request::Event(id)) => {
                    score += 3;
                    what.push(format!("event {id}"));
                }
                Event::Request(Request::Battle(n)) => {
                    score += 3;
                    what.push(format!("battle {n}"));
                }
                Event::MapChange(change) if self.same_family(change.destination) => {
                    score += 2;
                    what.push(format!("map {}", change.destination));
                }
                _ => {}
            }
        }
        if after.flags != self.state.flags || after_track.flags != track.flags {
            score += 1;
            what.push("flags".to_owned());
        }
        (score, what.join(", "))
    }

    /// The report of a run: the journal, and where and why it stopped.
    pub fn auto_report(&self, stop: &Stop) -> String {
        let mut out = String::new();
        out.push_str(&self.journal.to_string());
        let events = self.journal.events();
        out.push_str(&format!("events played: {events:?}\n"));
        match stop {
            Stop::Control { map } => out.push_str(&format!(
                "stopped: free control on map {map} after {:.1} s of game time\n",
                self.tick as f64 / 60.0
            )),
            Stop::Stuck { map, why } => out.push_str(&format!(
                "stopped: stuck on map {map} after {:.1} s of game time: {why}\n",
                self.tick as f64 / 60.0
            )),
            Stop::Limit => out.push_str("stopped: step limit reached while a scene was running\n"),
            Stop::NotStarted => out.push_str("stopped: no scene started\n"),
            Stop::Reached { event } => out.push_str(&format!(
                "stopped: event {event} ended (route reached) after {:.1} s of game time\n",
                self.tick as f64 / 60.0
            )),
            Stop::Done => out.push_str(&format!(
                "stopped: the caller's condition held after {:.1} s of game time\n",
                self.tick as f64 / 60.0
            )),
        }
        for (i, t) in self.state.tracks.iter().enumerate() {
            out.push_str(&format!(
                "track {i}: {}.{}.{} flags {:?}\n",
                t.chapter, t.step, t.substep, t.flags
            ));
        }
        out.push_str(&format!("global flags: {:?}\n", self.state.flags));
        if let Some(hero) = &self.hero {
            out.push_str(&format!("hero at {:?}\n", hero.position()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        out.push_str(&self.party_report());
        #[cfg(target_arch = "wasm32")]
        out.push_str(&self.party_lines());
        out
    }
}
