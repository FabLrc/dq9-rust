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

use dq9_core::field::{Event, Exit, Leader, exit_contains};
use dq9_core::scenario::Step;
use dq9_core::talk::Target;
use dq9_core::trigger::Request;
use dq9_core::trigger::State;
use glam::Vec3;

use self::nav::{MAX_FLOORS, NavGrid};
use super::{App, status};
use crate::talk::{Button, Partner};

mod nav;

/// Steps between two presses of the action key while a scene runs (a message needs its text
/// typed before the key advances it).
const PRESS_EVERY: u64 = 20;
/// Things the autopilot may do in one run.
const MAX_ACTIONS: u32 = 300;
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

/// Why [`App::play_auto`] stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Leave by a warp or a doorway; `heading` is the direction to walk into it (zero for a
    /// warp, whose volume triggers by itself).
    Exit {
        destination: Option<u16>,
        position: Vec3,
        heading: Vec3,
        /// The doorway's `(class, number)` when this is an internal door (no destination).
        door: Option<(u8, u8)>,
        /// [`exit_key`] of the way out tried: a map may offer several exits to one
        /// destination, and failing one must not forbid the others.
        key: u64,
    },
}

impl Goal {
    fn point(&self) -> Vec3 {
        match *self {
            Self::Talk { position, .. } => position,
            Self::Area { point, .. } => point,
            Self::Exit { position, .. } => position,
        }
    }
}

/// What the autopilot remembers.
#[derive(Default)]
struct Pilot {
    /// Conversations had: `(room, npc, context, story digest)`; one is not had twice at the
    /// same point of the story.
    talked: BTreeSet<(u16, u16, u16, u64)>,
    /// Exits taken: `(room, destination, story digest)`.
    left: BTreeSet<(u16, u16, u64)>,
    /// Single ways out tried, taken or not: `(room, destination, story digest, exit key)`.
    /// A map may lead to one destination by a warp and by doorways; a walk that fails on one
    /// of them must not forbid the others.
    tried_exits: BTreeSet<(u16, u16, u64, u64)>,
    /// Ways out whose walk failed: `(room, destination, exit key)`, across story digests. A
    /// goal that keeps failing must not be retried forever after each flag change.
    failed_exits: BTreeSet<(u16, u16, u64)>,
    /// Areas walked into: `(room, area index, story digest)`.
    entered: BTreeSet<(u16, usize, u64)>,
    /// The story digest at which each room was last looked at for something to do.
    evaluated: BTreeMap<u16, u64>,
    /// The rooms each room leads to (seen so far).
    graph: BTreeMap<u16, Vec<u16>>,
    /// The rooms the hero was in.
    visited: BTreeSet<u16>,
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

impl App {
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
    pub fn play_auto(&mut self, max_steps: u64, pilot: bool) -> Stop {
        // A loaded map is an already started game: a run from a save/map start (`--auto`
        // without a prologue) must still pilot or report control. `NotStarted` is for a run
        // where no map/ROM ever loaded.
        let mut started = self.current.is_some();
        let mut bot = Pilot::default();
        self.checkpoints.clear();
        for _ in 0..max_steps {
            let recorded = self.journal.steps.len();
            let busy = self.events.is_active() || self.events.pending_start().is_some();
            if (busy || self.talk.is_active()) && self.tick.is_multiple_of(PRESS_EVERY) {
                self.talk.press(Button::Confirm);
            }
            if pilot {
                self.pilot_frame(&mut bot);
                if let Some(why) = bot.stuck.take() {
                    let map = self.current.as_ref().map_or(0, |(e, _)| e.id);
                    self.journal.record(self.tick, Step::Unhandled(why.clone()));
                    // The goal could not be walked; it is booked as tried, so choose
                    // another. The run only stops when the pilot has nothing left to try.
                    if !self.pursue(&mut bot) {
                        return Stop::Stuck { map, why };
                    }
                }
            }
            self.run_steps(1);
            self.capture_journal(recorded);
            // The cast task of action 0x8d holds the field like an event: no control yet.
            let busy = self.events.is_active()
                || self.events.pending_start().is_some()
                || self.cast_reload
                || self.battle_is_active();
            started |= busy;
            if started && !busy && !self.talk.is_active() && !self.talk.confirm_pending() {
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
                        continue;
                    }
                    if bot.wait > 0 {
                        bot.wait -= 1;
                        continue;
                    }
                    if self.pursue(&mut bot) {
                        continue;
                    }
                }
                if last.is_some_and(|(m, s)| s == story && m != map) {
                    self.journal.record(self.tick, Step::Control { map, story });
                }
                return Stop::Control { map };
            }
        }
        if started {
            Stop::Limit
        } else {
            Stop::NotStarted
        }
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
        }
    }

    /// A route through the grid of `room`.
    fn route(&self, bot: &Pilot, room: u16, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let blocked: BTreeSet<usize> = bot
            .blocked
            .iter()
            .filter(|(at, _, _)| *at == room)
            .map(|(_, cell, level)| cell * MAX_FLOORS + *level as usize)
            .collect();
        bot.grids.get(&room)?.plan(from, to, &blocked)
    }

    /// At a free control: has the conversation, steps into the area or leaves by the exit that
    /// moves the story most when a real walk reaches it. Returns whether a goal was started.
    fn pursue(&mut self, bot: &mut Pilot) -> bool {
        let Some((entry, _)) = &self.current else {
            return false;
        };
        let room = entry.id;
        bot.visited.insert(room);
        if let Some(field) = &self.field {
            let exits = field.exits();
            let towards = exits
                .iter()
                .filter_map(|e| e.destination)
                .filter(|d| self.same_family(*d))
                .collect::<BTreeSet<_>>();
            bot.graph.insert(room, towards.into_iter().collect());
        }
        let digest = self.story_digest();
        if bot.actions >= MAX_ACTIONS {
            return false;
        }
        // A map load (a new room) built its collision anew, with a revision that starts over:
        // a grid cached on an earlier visit may keep a wall the triggers have since removed.
        if bot.built != Some(room) {
            bot.grids.remove(&room);
            bot.blocked.retain(|(at, _, _)| *at != room);
            bot.grid_revision = self.collision.revision();
            bot.built = Some(room);
        }
        // The triggers (event 20 on a load or a progress change, event 11) may hide or show
        // collision placements: the cached grids then keep a wall the game removed (or miss
        // one it added). Drop them when the collision revision moved.
        let revision = self.collision.revision();
        if bot.grid_revision != revision {
            bot.grids.clear();
            bot.blocked.clear();
            bot.grid_revision = revision;
        }
        let Some(hero) = self.hero else {
            return false;
        };
        let from = hero.position();
        let Some(field) = &self.field else {
            return false;
        };
        // A door that finished opening changed the collision: the walkable graph must be
        // probed again, or the grid keeps the closed door as a wall.
        let done = self
            .events
            .doors()
            .iter()
            .filter(|door| door.angle >= door.target)
            .count();
        if bot.grid_doors != done {
            bot.grids.remove(&room);
            bot.blocked.retain(|(at, _, _)| *at != room);
            bot.grid_doors = done;
        }
        let exits = field.exits();
        let areas = field.area_points();
        let partners = self.talk.partners();
        let mut points = vec![from];
        points.extend(exits.iter().map(|e| e.position));
        points.extend(areas.iter().copied());
        points.extend(partners.iter().map(|p| p.position));
        self.ensure_grid(bot, room, &points, from.y);

        // What the map offers, by how much it moves the story: conversations first on a tie.
        enum Offer {
            Talk(Partner),
            Area { index: usize, point: Vec3 },
        }
        let mut offers: Vec<(u32, u8, Offer)> = Vec::new();
        for partner in partners {
            if bot
                .talked
                .contains(&(room, partner.npc, partner.context, digest))
            {
                continue;
            }
            let (events, after) = self.talk.simulate(&partner, field, &self.state);
            let (score, _) = self.rate(&events, &after);
            if score > 0 {
                offers.push((score, 0, Offer::Talk(partner)));
            }
        }
        for (index, point) in areas.iter().enumerate() {
            if bot.entered.contains(&(room, index, digest)) {
                continue;
            }
            let (mut field, mut state) = (field.clone(), self.state.clone());
            let leader = Leader {
                position: *point,
                yaw: 0.0,
                input: Vec3::ZERO,
            };
            let events = field.step(&leader, &mut state);
            let (score, what) = self.rate(&events, &state);
            if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
                eprintln!(
                    "pilot: map {room} area {index} at {point:?}: score {score} {what} {events:?}"
                );
            }
            if score > 0 {
                offers.push((
                    score,
                    1,
                    Offer::Area {
                        index,
                        point: *point,
                    },
                ));
            }
        }
        offers.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut unreachable_target: Option<Vec3> = None;
        if std::env::var_os("DQ9_PILOT_DEBUG").is_some() && !offers.is_empty() {
            let list: Vec<String> = offers
                .iter()
                .map(|(score, _kind, offer)| match offer {
                    Offer::Talk(p) => format!("{score}:talk {}/{}", p.npc, p.context),
                    Offer::Area { index, .. } => format!("{score}:area {index}"),
                })
                .collect();
            eprintln!("pilot: map {room} offers [{}]", list.join(", "));
        }
        for (_, _, offer) in offers {
            match offer {
                Offer::Talk(partner) => {
                    let target = Target {
                        npc: partner.npc,
                        context: partner.context,
                    };
                    if let Some((position, route)) = self.talk_spot(bot, room, from, &partner) {
                        bot.talked
                            .insert((room, partner.npc, partner.context, digest));
                        bot.actions += 1;
                        let what = format!(
                            "walks to NPC {} (context {}) on map {room} at {position:?}",
                            partner.npc, partner.context
                        );
                        status(&what);
                        self.journal.record(self.tick, Step::Autopilot(what));
                        self.start_goal(
                            bot,
                            room,
                            digest,
                            route,
                            Goal::Talk {
                                target,
                                position,
                                yaw: partner.yaw,
                            },
                        );
                        return true;
                    }
                    unreachable_target.get_or_insert(partner.position);
                }
                Offer::Area { index, point } => {
                    let goal = (|| {
                        let spots = std::iter::once(point).chain(
                            bot.grids
                                .get(&room)?
                                .cells_near(point, 2.0)
                                .into_iter()
                                .filter(|at| field.area_contains(index, *at)),
                        );
                        for spot in spots {
                            if let Some(route) = self.route(bot, room, from, spot) {
                                return Some((spot, route));
                            }
                        }
                        None
                    })();
                    if goal.is_none() {
                        unreachable_target.get_or_insert(point);
                    }
                    if goal.is_none() && std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
                        let cells = bot.grids.get(&room).map(|grid| {
                            (
                                grid.cells_near(point, 2.0)
                                    .iter()
                                    .filter(|at| field.area_contains(index, **at))
                                    .count(),
                                grid.cells_connected(from, point),
                            )
                        });
                        eprintln!(
                            "pilot: area {index} on map {room} has no walkable spot (cells in area/connected: {cells:?}) from {from:?}"
                        );
                    }
                    if let Some((spot, route)) = goal {
                        bot.entered.insert((room, index, digest));
                        bot.actions += 1;
                        let what = format!("walks into area {index} of map {room} at {spot:?}");
                        status(&what);
                        self.journal.record(self.tick, Step::Autopilot(what));
                        self.start_goal(
                            bot,
                            room,
                            digest,
                            route,
                            Goal::Area { index, point: spot },
                        );
                        return true;
                    }
                }
            }
        }
        // A closed internal door stands between us and the place to reach: walk into it and
        // let the field open it (native 584), then choose again.
        if let Some(target) = unreachable_target
            && let Some(field) = &self.field
            && let Some(exit) = door_towards(&field.exits(), from, target, &bot.opened)
            && let Some((spot, route)) = self.exit_spot(bot, room, from, &exit)
        {
            bot.opened.insert((exit.class, exit.number));
            bot.actions += 1;
            let what = format!(
                "walks into the door {}/{} of map {room} at {spot:?}",
                exit.class, exit.number
            );
            status(&what);
            self.journal.record(self.tick, Step::Autopilot(what));
            self.start_goal(
                bot,
                room,
                digest,
                route,
                Goal::Exit {
                    destination: None,
                    position: spot,
                    heading: exit.heading,
                    door: Some((exit.class, exit.number)),
                    key: exit_key(&exit),
                },
            );
            return true;
        }
        // Nothing to say or step into here: take an exit not taken yet, unvisited first.
        let Some(field) = &self.field else {
            return false;
        };
        bot.evaluated.insert(room, digest);
        let exits = field.exits();
        let mut untried: Vec<Exit> = exits
            .iter()
            .filter(|exit| {
                exit.destination.is_some_and(|d| {
                    let key = exit_key(exit);
                    self.same_family(d)
                        && !bot.tried_exits.contains(&(room, d, digest, key))
                        && !bot.failed_exits.contains(&(room, d, key))
                })
            })
            .copied()
            .collect();
        // Unvisited rooms first, in the order of the map's exits; every exit to a destination
        // is tried, since one may be on an unreachable ledge while another is walkable.
        untried.sort_by_key(|exit| exit.destination.is_some_and(|d| bot.visited.contains(&d)));
        for exit in untried {
            let Some(destination) = exit.destination else {
                continue;
            };
            if let Some((spot, route)) = self.exit_spot(bot, room, from, &exit) {
                bot.left.insert((room, destination, digest));
                bot.tried_exits
                    .insert((room, destination, digest, exit_key(&exit)));
                bot.actions += 1;
                let what = format!("walks out of map {room} for {destination}");
                status(&what);
                self.journal.record(self.tick, Step::Autopilot(what));
                self.start_goal(
                    bot,
                    room,
                    digest,
                    route,
                    Goal::Exit {
                        destination: Some(destination),
                        position: spot,
                        heading: exit.heading,
                        door: None,
                        key: exit_key(&exit),
                    },
                );
                return true;
            }
        }
        // Nothing left here: head for the nearest room that has something left to try.
        let Some(hop) = self.next_hop(bot, room, digest) else {
            return false;
        };
        let Some(field) = &self.field else {
            return false;
        };
        for exit in field
            .exits()
            .into_iter()
            .filter(|e| e.destination == Some(hop))
        {
            let Some((spot, route)) = self.exit_spot(bot, room, from, &exit) else {
                continue;
            };
            bot.tried_exits.insert((room, hop, digest, exit_key(&exit)));
            bot.actions += 1;
            let what = format!("walks back through map {room} to {hop}");
            status(&what);
            self.journal.record(self.tick, Step::Autopilot(what));
            self.start_goal(
                bot,
                room,
                digest,
                route,
                Goal::Exit {
                    destination: Some(hop),
                    position: spot,
                    heading: exit.heading,
                    door: None,
                    key: exit_key(&exit),
                },
            );
            return true;
        }
        status(&format!("no walkable way back through map {room} to {hop}"));
        false
    }

    /// A reachable standing point of `partner`: the partner search's own point, else a walkable
    /// cell near it where `Talk_NearestNpc` still picks this NPC and context.
    fn talk_spot(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        partner: &Partner,
    ) -> Option<(Vec3, Vec<Vec3>)> {
        let target = Target {
            npc: partner.npc,
            context: partner.context,
        };
        let grid = bot.grids.get(&room)?;
        let cells = grid.cells_near(partner.position, 2.5);
        let mut same_floor = 0;
        for spot in cells {
            // Same floor as the NPC: the game's partner search is a 3D distance, ours reads
            // the placement heights, so the walk must end on the NPC's walkway.
            if (spot.y - partner.position.y).abs() > 1.0 {
                continue;
            }
            same_floor += 1;
            if self.talk.nearest(spot, partner.yaw) != Some(target) {
                continue;
            }
            if let Some(route) = self.route(bot, room, from, spot) {
                return Some((spot, route));
            }
        }
        if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
            eprintln!(
                "pilot: no talk spot for NPC {} (context {}) on map {room} at {:?} from {from:?}: {same_floor} cells on its floor",
                partner.npc, partner.context, partner.position
            );
        }
        None
    }

    /// A reachable approach point of `exit`: the warp itself, or the ground in front of a
    /// doorway; once there the pilot walks into it.
    fn exit_spot(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        exit: &Exit,
    ) -> Option<(Vec3, Vec<Vec3>)> {
        let grid = bot.grids.get(&room)?;
        // Inside the trigger volume (a warp above a hall is not reached by standing under it,
        // and a doorway only fires where the volume is).
        let inside: Vec<Vec3> = grid
            .cells_near(exit.position, 3.5)
            .into_iter()
            .filter(|spot| exit_contains(exit, *spot))
            .collect();
        let mut spots = inside;
        if exit.heading != Vec3::ZERO {
            // A doorway reacts to a hero walking against its facing: start in front of it when
            // no walkable cell lies inside. The standing point must be on the doorway's floor:
            // a balcony above it cannot walk in.
            let heading = flat(exit.heading).normalize_or_zero();
            spots.extend(
                grid.cells_near(exit.position, 3.0)
                    .into_iter()
                    .filter(|spot| {
                        flat(*spot - exit.position).dot(heading) <= 0.2
                            && (spot.y - exit.position.y).abs() < LEVEL_REACH
                    }),
            );
        }
        let near = grid.cells_near(exit.position, 3.5);
        let count = near
            .iter()
            .filter(|spot| exit_contains(exit, **spot))
            .count();
        for spot in spots {
            if let Some(route) = self.route(bot, room, from, spot) {
                return Some((spot, route));
            }
        }
        status(&format!(
            "exit {exit:?}: {} walkable cells near, {count} inside the volume, from {from:?}",
            near.len()
        ));
        None
    }

    fn start_goal(&self, bot: &mut Pilot, room: u16, digest: u64, route: Vec<Vec3>, goal: Goal) {
        bot.goal = Some(goal);
        bot.goal_room = room;
        bot.goal_digest = digest;
        bot.route = route;
        bot.route_at = 0;
        bot.exit_frames = 0;
        bot.turn_frames = 0;
        bot.stall = 0;
        bot.last_position = None;
        bot.best_waypoint = f32::INFINITY;
        bot.replans = 0;
    }

    /// One fixed step of walking towards the pilot's goal: feeds the d-pad, presses the action
    /// key when the game's partner search sees the target, and clears the goal when it is
    /// reached (the same step lets the field fire the trigger).
    fn pilot_frame(&mut self, bot: &mut Pilot) {
        self.pilot_dir = None;
        let Some(goal) = bot.goal else {
            return;
        };
        let room = self.current.as_ref().map_or(0, |(e, _)| e.id);
        if room != bot.goal_room || self.story_digest() != bot.goal_digest {
            // A scene or a trigger moved the story while walking: choose again.
            bot.goal = None;
            return;
        }
        let Some(hero) = self.hero else {
            bot.goal = None;
            return;
        };
        let position = hero.position();
        // An area is entered as soon as the hero is inside it, not only at its centre.
        if let Goal::Area { index, .. } = goal
            && self
                .field
                .as_ref()
                .is_some_and(|field| field.area_contains(index, position))
        {
            bot.goal = None;
            return;
        }
        // Talk as soon as the game itself would: the action key reads the same partner search,
        // on the NPC's floor (see `talk_spot`).
        if let Goal::Talk {
            target,
            position: end,
            ..
        } = goal
            && (position.y - end.y).abs() < 1.0
            && self.talk.nearest(position, hero.yaw()) == Some(target)
        {
            status(&format!(
                "autopilot: presses the action key for NPC {} (context {})",
                target.npc, target.context
            ));
            self.talk.press(Button::Confirm);
            bot.goal = None;
            return;
        }
        // Stuck: no progress towards the next waypoint (jitter against a wall is not
        // progress, even though the position changes every frame).
        let next = bot.route.get(bot.route_at).copied().unwrap_or(goal.point());
        let distance = flat(position - next).length();
        let moved = bot
            .last_position
            .is_none_or(|last| flat(position - last).length() > 0.01);
        if moved && distance < bot.best_waypoint - 0.02 {
            bot.best_waypoint = distance;
            bot.stall = 0;
            bot.last_position = Some(position);
        } else {
            if moved {
                bot.last_position = Some(position);
            }
            bot.stall += 1;
        }
        if bot.stall > REPLAN_FRAMES && bot.replans < 8 {
            // The waypoint was validated when the route was built; what fails is the approach
            // from where the hero stands, so forbid that node (not the goal's) and plan around
            // it.
            if let Some(grid) = bot.grids.get(&room)
                && let Some(node) = grid.nearest_node(position, 0.75)
                && bot.blocked.len() < 256
            {
                bot.blocked
                    .insert((room, grid.cell(node), (node % MAX_FLOORS) as u8));
            }
            bot.route = self
                .route(bot, room, position, goal.point())
                .unwrap_or_default();
            bot.route_at = 0;
            bot.best_waypoint = f32::INFINITY;
            // One replan per stalled window: the stall budget starts again, so the learned
            // cells differ instead of burning all eight replans in eight frames.
            bot.stall = 0;
            bot.replans += 1;
        }
        if bot.stall > STUCK_FRAMES {
            let next = bot.route.get(bot.route_at).copied();
            status(&format!(
                "autopilot: stuck on map {room} at {position:?} walking to {:?} \
                 (waypoint {:?} of {} at {next:?})",
                goal.point(),
                bot.route_at,
                bot.route.len()
            ));
            // The goal is unreachable: book it like a failed exit and let the pilot choose
            // another; `play_auto` reports the stop only when nothing else is left.
            match goal {
                Goal::Exit {
                    destination: Some(destination),
                    key,
                    ..
                } => {
                    bot.left.insert((room, destination, bot.goal_digest));
                    bot.tried_exits
                        .insert((room, destination, bot.goal_digest, key));
                    bot.failed_exits.insert((room, destination, key));
                }
                Goal::Exit {
                    door: Some((class, number)),
                    ..
                } => {
                    if self.events.doors().iter().any(|door| {
                        door.block == i32::from(class) && door.placement == i32::from(number)
                    }) {
                        bot.wait = 40;
                    } else {
                        bot.opened.insert((class, number));
                    }
                }
                _ => {}
            }
            bot.stuck = Some(format!(
                "pilot stuck on map {room} at {position:?} walking to {:?}",
                goal.point()
            ));
            bot.goal = None;
            return;
        }
        // Reached the end of the route: act. Both tests are 3D: the cell centre of a lower
        // floor sits at the same x/z as the balcony over it.
        let end = goal.point();
        let at_level = |waypoint: Vec3| (position.y - waypoint.y).abs() < LEVEL_REACH;
        let at_end = flat(position - end).length() < WAYPOINT_REACH && at_level(end);
        while bot.route_at < bot.route.len() {
            let waypoint = bot.route[bot.route_at];
            if flat(position - waypoint).length() < WAYPOINT_REACH && at_level(waypoint) {
                bot.route_at += 1;
                bot.best_waypoint = f32::INFINITY;
            } else {
                break;
            }
        }
        if !at_end && bot.route_at >= bot.route.len() {
            // The route ends short of the goal: plan again from here. A plan the grid refuses
            // counts as a failure, so the goal is abandoned instead of taken for reached (an
            // empty route is not an arrival).
            if bot.replans < 8 {
                match self.route(bot, room, position, end) {
                    Some(route) => {
                        bot.route = route;
                        bot.route_at = 0;
                        bot.best_waypoint = f32::INFINITY;
                        bot.replans += 1;
                        return;
                    }
                    None => bot.replans = 8,
                }
            }
            status(&format!(
                "autopilot: cannot reach {end:?} from {position:?} on map {room}"
            ));
            match goal {
                Goal::Exit {
                    destination: Some(destination),
                    key,
                    ..
                } => {
                    bot.left.insert((room, destination, bot.goal_digest));
                    bot.tried_exits
                        .insert((room, destination, bot.goal_digest, key));
                    bot.failed_exits.insert((room, destination, key));
                }
                Goal::Exit {
                    door: Some((class, number)),
                    ..
                } => {
                    if self.events.doors().iter().any(|door| {
                        door.block == i32::from(class) && door.placement == i32::from(number)
                    }) {
                        bot.wait = 40;
                    } else {
                        bot.opened.remove(&(class, number));
                    }
                }
                _ => {}
            };
            bot.goal = None;
            return;
        }
        if at_end {
            match goal {
                Goal::Talk { yaw, .. } => {
                    // Turn to face the NPC: the partner search weights the facing direction.
                    bot.turn_frames += 1;
                    if bot.turn_frames > TURN_FRAMES {
                        status(&format!(
                            "autopilot: cannot face NPC at {end:?} from {position:?}"
                        ));
                        bot.goal = None;
                        return;
                    }
                    self.pilot_dir = Some(facing(yaw));
                }
                Goal::Area { .. } => {
                    // Standing inside the area is enough: the field fires event 2 this step.
                    bot.goal = None;
                }
                Goal::Exit {
                    destination,
                    heading,
                    key,
                    ..
                } => {
                    bot.exit_frames += 1;
                    if bot.exit_frames > DOOR_FRAMES {
                        match destination {
                            Some(destination) => {
                                status(&format!("autopilot: no way out at {end:?}"));
                                bot.left.insert((room, destination, bot.goal_digest));
                                bot.tried_exits
                                    .insert((room, destination, bot.goal_digest, key));
                                bot.failed_exits.insert((room, destination, key));
                            }
                            // A door without destination only opens; let it finish.
                            None => bot.wait = 40,
                        }
                        bot.goal = None;
                        return;
                    }
                    self.pilot_dir = Some(heading);
                }
            }
            return;
        }
        // A dense grid path can put a waypoint at a corner the hero cannot reach exactly;
        // look one cell ahead once progress stalls, as a player would.
        let look = if bot.stall > 15 && bot.route_at + 1 < bot.route.len() {
            bot.route_at + 1
        } else {
            bot.route_at
        };
        let waypoint = bot.route[look];
        let direction = flat(waypoint - position).normalize_or_zero();
        // Wedged diagonally against a corner: press one axis at a time, as a player would,
        // alternating the axis when the first one does not move either.
        if bot.stall > 10 && direction.x != 0.0 && direction.z != 0.0 {
            let (major, minor) = if direction.x.abs() >= direction.z.abs() {
                (
                    Vec3::new(direction.x.signum(), 0.0, 0.0),
                    Vec3::new(0.0, 0.0, direction.z.signum()),
                )
            } else {
                (
                    Vec3::new(0.0, 0.0, direction.z.signum()),
                    Vec3::new(direction.x.signum(), 0.0, 0.0),
                )
            };
            self.pilot_dir = Some(if (bot.stall / 25).is_multiple_of(2) {
                major
            } else {
                minor
            });
        } else {
            self.pilot_dir = Some(direction);
        }
    }

    /// The first room on the shortest known way from `room` to a room the pilot has not looked
    /// at in this state of the story, or that still has an exit to try.
    fn next_hop(&self, bot: &Pilot, room: u16, digest: u64) -> Option<u16> {
        let pending = |r: u16| {
            bot.evaluated.get(&r) != Some(&digest)
                || bot
                    .graph
                    .get(&r)
                    .is_some_and(|exits| exits.iter().any(|d| !bot.left.contains(&(r, *d, digest))))
        };
        let mut parent = BTreeMap::new();
        let mut queue = std::collections::VecDeque::from([room]);
        while let Some(at) = queue.pop_front() {
            for &next in bot.graph.get(&at).into_iter().flatten() {
                if next == room || parent.contains_key(&next) {
                    continue;
                }
                parent.insert(next, at);
                if pending(next) {
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

    /// Whether map `id` is in the family of the current one (the first three letters of the
    /// codes: `X01`, `M01`): the pilot stays in the place the story is told in.
    fn same_family(&self, id: u16) -> bool {
        let family = |id: u16| {
            self.maps
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.code.get(..3).map(str::to_owned))
        };
        let here = self.current.as_ref().and_then(|(e, _)| family(e.id));
        here.is_some() && here == family(id)
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
        out.push_str(&self.party_report());
        out
    }
}
