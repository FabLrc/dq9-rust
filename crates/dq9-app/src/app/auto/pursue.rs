//! The pilot's goal pursuit: choosing the next conversation, area or exit, the walk back, the
//! start of an exit and the approach to a door.

use super::super::App;
use super::super::status;
use super::APPROACH_REACH;
use super::APPROACH_STARTS;
use super::Goal;
use super::LEVEL_REACH;
use super::MAX_ACTIONS;
use super::MAX_ROUTE_ACTIONS;
use super::MAX_TALKS;
use super::Pilot;
use super::approach;
use super::blocked_nodes;
use super::book_failed_exit;
use super::door_towards;
use super::exit_key;
use super::flat;
use super::pending_room;
use super::route;
use crate::talk::Partner;
use dq9_core::Hero;
use dq9_core::field::Exit;
use dq9_core::field::Leader;
use dq9_core::field::exit_contains;
use dq9_core::scenario::Step;
use dq9_core::talk::Target;
use dq9_formats::map::WarpVolume;
use glam::Vec3;
use std::collections::BTreeSet;

impl App {
    /// At a free control: has the conversation, steps into the area or leaves by the exit that
    /// moves the story most when a real walk reaches it. Returns whether a goal was started.
    pub(super) fn pursue(&mut self, bot: &mut Pilot) -> bool {
        let Some((entry, _)) = &self.current else {
            return false;
        };
        let room = entry.id;
        bot.visited.insert(room);
        if bot.room != Some(room) {
            bot.came_from = bot.room;
            bot.room = Some(room);
        }
        if let Some(field) = &self.field {
            let exits = field.exits();
            let towards = exits
                .iter()
                .filter_map(|e| e.destination)
                .filter(|d| self.same_family(*d))
                .collect::<BTreeSet<_>>();
            bot.graph.insert(room, towards.into_iter().collect());
            bot.foreign.insert(
                room,
                exits
                    .iter()
                    .filter_map(|e| e.destination)
                    .filter(|d| !self.same_family(*d))
                    .collect(),
            );
            bot.links.insert(
                room,
                exits
                    .iter()
                    .filter_map(|e| e.destination)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            );
        }
        let digest = self.story_digest();
        let cap = if bot.mode.is_some() {
            MAX_ROUTE_ACTIONS
        } else {
            MAX_ACTIONS
        };
        if bot.actions >= cap {
            return false;
        }
        // A map load (a new room) built its collision anew, with a revision that starts over:
        // a grid cached on an earlier visit may keep a wall the triggers have since removed.
        if bot.built != Some(room) {
            // The route mode walks to and fro between a few rooms, and the grid of a field
            // takes seconds to probe: one built under the same story, time of day and
            // collision changes is the same world and stays. The story pilot builds anew.
            if bot.mode.is_none() || bot.grid_keys.get(&room) != Some(&self.grid_key()) {
                bot.grids.remove(&room);
            }
            bot.blocked.retain(|(at, _, _)| *at != room);
            bot.grid_revision = self.collision.revision();
            bot.built = Some(room);
            // Every map load shuts its doors again: a route that comes back to a room (NPC 98 of
            // Erinn's house sits behind door 0/8 of 1110) pushes them again. The story pilot
            // keeps its own memory.
            if bot.mode.is_some() {
                bot.opened.clear();
            }
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
        let Some((exits, areas)) = self
            .field
            .as_ref()
            .map(|field| (field.exits(), field.area_points()))
        else {
            return false;
        };
        let partners = self.talk.partners();
        let mut points = vec![from];
        points.extend(exits.iter().map(|e| e.position));
        points.extend(areas.iter().copied());
        points.extend(partners.iter().map(|p| p.position));
        self.ensure_grid(bot, room, &points, from.y);
        // The party lost and is not stronger yet: the story's offers wait, the rooms may still
        // be explored (the way to a field or an inn that is not known yet).
        let mut waiting = false;
        if bot.mode.is_some() {
            self.route_enter(bot, room);
            if self.route_pursue(bot, room, digest, from) {
                return true;
            }
            waiting = bot.mode.as_ref().is_some_and(route::Route::waiting);
        }
        let Some(field) = &self.field else {
            return false;
        };

        // What the map offers, by how much it moves the story: conversations first on a tie.
        enum Offer {
            Talk(Partner),
            Area { index: usize, point: Vec3 },
        }
        let mut offers: Vec<(u32, u8, Offer)> = Vec::new();
        let point = self.story_point();
        for partner in partners.into_iter().filter(|_| !waiting) {
            if bot
                .talked
                .contains(&(room, partner.npc, partner.context, digest))
                || bot
                    .talks
                    .get(&(room, partner.npc, partner.context, point))
                    .is_some_and(|count| *count >= MAX_TALKS)
            {
                continue;
            }
            let (events, after) = self.talk.simulate(&partner, field, &self.state);
            let (score, _) = self.rate(&events, &after);
            if score > 0 {
                offers.push((score, 0, Offer::Talk(partner)));
            }
        }
        for (index, point) in areas.iter().enumerate().filter(|_| !waiting) {
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
                        *bot.talks
                            .entry((room, partner.npc, partner.context, point))
                            .or_default() += 1;
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
                    let avoid = self.warp_avoid(bot, room, from, None);
                    let goal = (|| {
                        let spots = std::iter::once(point).chain(
                            bot.grids
                                .get(&room)?
                                .cells_near(point, 2.0)
                                .into_iter()
                                .filter(|at| field.area_contains(index, *at)),
                        );
                        for spot in spots {
                            if let Some(route) = self.route(bot, room, from, spot, &avoid) {
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
            && self.open_door_towards(bot, room, digest, from, target)
        {
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
        // A room that still has something to do first, then the room the hero did not just
        // come from, then unvisited rooms, in the order of the map's exits. Every exit to a
        // destination is tried, since one may be on an unreachable ledge while another is
        // walkable; but an exit back to the room just left only repeats the walk (the tower
        // of the Observatory offers several doorways to `4105`, and taking them in map order
        // spent the run bouncing between two floors instead of climbing to `4104`).
        untried.sort_by_key(|exit| {
            exit.destination.map_or((true, true, true), |d| {
                (
                    !pending_room(bot, d, digest),
                    bot.came_from == Some(d),
                    bot.visited.contains(&d),
                )
            })
        });
        for exit in untried {
            if self.start_exit(bot, room, digest, from, &exit, false) {
                return true;
            }
            // No walk reaches it: a closed door may stand in the way (`M01M10` after
            // event 2130). Push the nearest one and choose again.
            if self.open_door_towards(bot, room, digest, from, exit.position) {
                return true;
            }
        }
        // Nothing left here: head for the nearest room that has something left to try. When
        // there is none, leave this family of maps, here or from the nearest room that can.
        if let Some(hop) = self.next_hop(bot, room, digest) {
            return self.walk_back(bot, room, digest, from, hop);
        }
        if self.leave_family(bot, room, digest, from) {
            return true;
        }
        match self.next_foreign_hop(bot, room, digest) {
            Some(hop) => self.walk_back(bot, room, digest, from, hop),
            None => false,
        }
    }

    /// Walks to the neighbouring room `hop` on the way to another one. The way back may use
    /// exits taken before at this point of the story (a room with two doors, both used): only
    /// an exit whose walk failed is out.
    pub(super) fn walk_back(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
        hop: u16,
    ) -> bool {
        let Some(field) = &self.field else {
            return false;
        };
        let toward: Vec<Exit> = field
            .exits()
            .into_iter()
            .filter(|e| {
                e.destination == Some(hop)
                    && !bot.failed_exits.contains(&(room, hop, exit_key(e)))
                    && !bot.dead_ends.contains(&(room, hop, exit_key(e), digest))
            })
            .collect();
        let mut blocked: Option<Exit> = None;
        for exit in toward {
            if self.start_exit(bot, room, digest, from, &exit, true) {
                return true;
            }
            blocked.get_or_insert(exit);
        }
        if let Some(exit) = blocked
            && self.open_door_towards(bot, room, digest, from, exit.position)
        {
            return true;
        }
        status(&format!("no walkable way back through map {room} to {hop}"));
        false
    }

    /// Books and starts the walk out by `exit` when a route reaches it (`back`: on the way
    /// to a room that has something left to try).
    fn start_exit(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
        exit: &Exit,
        back: bool,
    ) -> bool {
        let Some(destination) = exit.destination else {
            return false;
        };
        let Some((spot, route, stage)) = self.exit_spot(bot, room, from, exit) else {
            return false;
        };
        bot.left.insert((room, destination, digest));
        bot.tried_exits
            .insert((room, destination, digest, exit_key(exit)));
        bot.arrived_by = Some((room, destination, exit_key(exit)));
        bot.actions += 1;
        let what = if back {
            format!("walks back through map {room} to {destination}")
        } else {
            format!("walks out of map {room} for {destination}")
        };
        status(&what);
        self.journal.record(self.tick, Step::Autopilot(what));
        self.start_goal(
            bot,
            room,
            digest,
            route,
            Goal::Exit {
                exit: *exit,
                position: spot,
                stage,
            },
        );
        true
    }

    /// Nothing is left to say, step into or visit in this family of maps at this point of the
    /// story: take a way out to another family, unvisited first. Angel Falls has done what it
    /// offers at 2.2.3, and the story goes on in the field and the Mountain Pass, which only
    /// its south gate leads to (`dq9re doors M01`: the warp to `F01`).
    fn leave_family(&mut self, bot: &mut Pilot, room: u16, digest: u64, from: Vec3) -> bool {
        let Some(field) = &self.field else {
            return false;
        };
        let mut ways: Vec<Exit> = field
            .exits()
            .into_iter()
            .filter(|exit| {
                exit.destination.is_some_and(|d| {
                    !self.same_family(d)
                        && !bot.tried_exits.contains(&(room, d, digest, exit_key(exit)))
                        && !bot.failed_exits.contains(&(room, d, exit_key(exit)))
                })
            })
            .collect();
        // The rooms the story has something to start in at this point come first, then those
        // not visited: from the field, the Mountain Pass (arrival rule of event 2300) before
        // the Hexagon, which has no rule loaded at 2.2 (`dq9re triggers D01 --story 2.2`).
        ways.sort_by_key(|exit| {
            exit.destination
                .map(|d| (self.story_rules_in(d) == 0, bot.visited.contains(&d)))
        });
        ways.iter()
            .any(|exit| self.start_exit(bot, room, digest, from, exit, false))
    }

    /// A reachable standing point of `partner`: the partner search's own point, else a walkable
    /// cell near it where `Talk_NearestNpc` still picks this NPC and context.
    pub(super) fn talk_spot(
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
        let avoid = self.warp_avoid(bot, room, from, None);
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
            if let Some(route) = self.route(bot, room, from, spot, &avoid) {
                if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
                    eprintln!(
                        "pilot: talk spot {spot:?} for NPC {} (from {from:?}, avoid {} nodes, {} waypoints)",
                        partner.npc,
                        avoid.len(),
                        route.len()
                    );
                }
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

    /// A reachable end for the walk out by `exit`: a standing pose inside its trigger volume,
    /// the ground in front of a doorway, or, for a warp whose volume the grid does not join to
    /// the walkway, a place the hero reaches from which his own movement enters it (the flag:
    /// see [`App::approach_start`]).
    pub(super) fn exit_spot(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        exit: &Exit,
    ) -> Option<(Vec3, Vec<Vec3>, bool)> {
        let grid = bot.grids.get(&room)?;
        let avoid = self.warp_avoid(bot, room, from, Some(exit_key(exit)));
        // Standing poses inside the trigger volume. A node is a pose at the height of its own
        // floor and the volume is a box with heights too: the node under or over it (another
        // floor of the same column) is not inside, and a warp above a hall is not reached by
        // standing under it.
        let mut spots: Vec<Vec3> = grid
            .cells_near(exit.position, 3.5)
            .into_iter()
            .filter(|spot| exit_contains(exit, *spot))
            .collect();
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
        for spot in spots {
            if let Some(route) = self.route(bot, room, from, spot, &avoid) {
                return Some((spot, route, false));
            }
        }
        // The grid does not join the walkway to the volume: only a warp may still be entered,
        // by walking, from a place the hero reaches.
        let volume = exit.warp?;
        if exit_contains(exit, from) {
            return None;
        }
        let (start, route) = self.approach_start(bot, room, from, exit, &volume, &avoid)?;
        Some((start, route, true))
    }

    /// Where to start walking into the volume of a warp the grid does not join to the
    /// walkway: the hero's own place when he is near it, else the places he reaches that lie
    /// nearest to it, the first one from which [`approach::search`] finds a walk. Returns the
    /// start and the route to it (`M01M10`: the stair is a lane 0.2 wide, reached round a
    /// wall end through a gap of centimetres).
    fn approach_start(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        exit: &Exit,
        volume: &WarpVolume,
        avoid: &BTreeSet<usize>,
    ) -> Option<(Vec3, Vec<Vec3>)> {
        let grid = bot.grids.get(&room)?;
        let near = |at: Vec3| flat(approach::nearest(volume, at) - at).length();
        if near(from) < APPROACH_REACH
            && let Some(hero) = self.hero
            && self.approach_plan(hero, exit).is_some()
        {
            return Some((from, vec![from]));
        }
        let mut starts = grid.reachable(from, &blocked_nodes(bot, room, avoid));
        starts.retain(|at| near(*at) < APPROACH_REACH);
        starts.sort_by(|a, b| near(*a).total_cmp(&near(*b)));
        let mut tried: Vec<Vec3> = Vec::new();
        for start in starts {
            if tried.iter().any(|seen| (*seen - start).length() < 1.0) {
                continue;
            }
            if tried.len() == APPROACH_STARTS {
                break;
            }
            tried.push(start);
            let toward = flat(approach::nearest(volume, start) - start).normalize_or_zero();
            let hero = Hero::at(start, toward.x.atan2(-toward.z));
            if self.approach_plan(hero, exit).is_some()
                && let Some(route) = self.route(bot, room, from, start, avoid)
            {
                return Some((start, route));
            }
        }
        None
    }

    /// The presses that take `hero` into the volume of the warp `exit`, searched with his own
    /// movement; none when no walk gets there, or when every one crosses another warp's volume
    /// first (a `0x72` warp fires from presence and would carry him elsewhere).
    pub(super) fn approach_plan(&self, hero: Hero, exit: &Exit) -> Option<Vec<Vec3>> {
        let volume = exit.warp?;
        let others: Vec<Exit> = self
            .field
            .as_ref()?
            .exits()
            .into_iter()
            .filter(|other| {
                other.warp.is_some()
                    && exit_key(other) != exit_key(exit)
                    && !exit_contains(other, hero.position())
            })
            .collect();
        approach::search(
            &self.collision,
            hero,
            &volume,
            |at| exit_contains(exit, at),
            |at| others.iter().any(|other| exit_contains(other, at)),
        )
    }

    /// Feeds the next press of the walk into `exit`'s volume. The field takes the pad on every
    /// second step (`field_due` of `App::run_steps`), so the walk advances on those. When the
    /// presses are spent and the warp has not fired, the exit is booked as failed.
    pub(super) fn press_approach(&mut self, bot: &mut Pilot, room: u16, exit: &Exit) {
        if let Some(press) = bot.approach.get(bot.approach_at).copied() {
            self.pilot_dir = Some(press);
            if self.tick.is_multiple_of(2) {
                bot.approach_at += 1;
            }
            return;
        }
        status(&format!(
            "autopilot: the walk ended outside the volume of {:?}",
            exit.destination
        ));
        bot.approach.clear();
        book_failed_exit(bot, room, exit);
        bot.goal = None;
    }

    pub(super) fn start_goal(
        &self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        route: Vec<Vec3>,
        goal: Goal,
    ) {
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
        bot.approach.clear();
        bot.approach_at = 0;
    }

    /// Walks into the closed internal doorway nearest to `target` when the place behind it
    /// cannot be reached: the field opens it (native 584), and the next choice plans through
    /// the changed collision. `M01M10` at story 2.2.1 has its only way out to `M01M07` behind
    /// door `0/18`; without this push the pilot reported "no walkable way back through map
    /// 1110 to 1107" (`build/evidence/j4-chapter2-route/run7.log`).
    fn open_door_towards(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
        target: Vec3,
    ) -> bool {
        let Some(field) = &self.field else {
            return false;
        };
        let Some(exit) = door_towards(&field.exits(), from, target, &bot.opened) else {
            return false;
        };
        let Some((spot, route, _stage)) = self.exit_spot(bot, room, from, &exit) else {
            return false;
        };
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
                exit,
                position: spot,
                stage: false,
            },
        );
        true
    }
}
