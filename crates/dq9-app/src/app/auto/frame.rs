//! One frame of the pilot: the d-pad it holds, the replans, the stuck and door handling.

use super::super::App;
use super::super::status;
use super::DOOR_FRAMES;
use super::Goal;
use super::LEVEL_REACH;
use super::Pilot;
use super::REPLAN_FRAMES;
use super::STUCK_FRAMES;
use super::TURN_FRAMES;
use super::WARP_FRAMES;
use super::WAYPOINT_REACH;
use super::approach;
use super::book_failed_exit;
use super::facing;
use super::flat;
use super::hop_towards;
use super::nav::MAX_FLOORS;
use super::pending_room;
use crate::talk::Button;
use glam::Vec3;

impl App {
    /// One fixed step of walking towards the pilot's goal: feeds the d-pad, presses the action
    /// key when the game's partner search sees the target, and clears the goal when it is
    /// reached (the same step lets the field fire the trigger).
    pub(super) fn pilot_frame(&mut self, bot: &mut Pilot) {
        self.pilot_dir = None;
        let Some(goal) = bot.goal else {
            return;
        };
        let room = self.current.as_ref().map_or(0, |(e, _)| e.id);
        if room != bot.goal_room || self.story_digest() != bot.goal_digest {
            // A scene or a trigger moved the story while walking: choose again.
            if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
                eprintln!(
                    "pilot: goal cancelled by {} (room {room} vs {}, digest {} vs {}), flags {:?}, tracks {:?}",
                    if room != bot.goal_room {
                        "map"
                    } else {
                        "story"
                    },
                    bot.goal_room,
                    self.story_digest(),
                    bot.goal_digest,
                    self.state.flags,
                    self.state
                        .tracks
                        .iter()
                        .map(|t| (t.chapter, t.step, t.substep, t.flags.clone()))
                        .collect::<Vec<_>>()
                );
            }
            bot.goal = None;
            return;
        }
        let Some(hero) = self.hero else {
            bot.goal = None;
            return;
        };
        let position = hero.position();
        // A walk into a warp's volume that the search found is replayed first: its presses
        // were searched with the hero's own movement from the pose he was in.
        if !bot.approach.is_empty() {
            if let Goal::Exit { exit, .. } = goal {
                self.press_approach(bot, room, &exit);
            }
            return;
        }
        // The route mode's hunts and roaming walks last a few seconds: they give way to a new
        // choice when the roamer moved, one shows up, or a member is low.
        if matches!(goal, Goal::Hunt { .. } | Goal::Roam { .. }) && !self.route_keeps(bot, goal) {
            bot.goal = None;
            return;
        }
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
            // The route replanned while walking must avoid the same warps the goal was chosen
            // around (see `warp_nodes`).
            let avoid = self.warp_avoid(bot, room, position, goal.exit_key());
            bot.route = self
                .route(bot, room, position, goal.point(), &avoid)
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
            if let Goal::Hunt { point } | Goal::Roam { point } = goal {
                // A walk to a roamer that fails costs nothing: that point is not tried again.
                if let Some(mode) = bot.mode.as_mut() {
                    mode.avoid(point);
                }
                bot.goal = None;
                return;
            }
            // The goal is unreachable: book it like a failed exit and let the pilot choose
            // another; `play_auto` reports the stop only when nothing else is left.
            match goal {
                Goal::Exit { exit, .. } if exit.destination.is_some() => {
                    book_failed_exit(bot, room, &exit);
                }
                Goal::Exit { exit, .. } if exit.door.is_some() => {
                    let (class, number) = (exit.class, exit.number);
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
        let reach = WAYPOINT_REACH;
        while bot.route_at < bot.route.len() {
            let waypoint = bot.route[bot.route_at];
            if flat(position - waypoint).length() < reach && at_level(waypoint) {
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
                let avoid = self.warp_avoid(bot, room, position, goal.exit_key());
                match self.route(bot, room, position, end, &avoid) {
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
            if let Goal::Hunt { point } | Goal::Roam { point } = goal {
                if let Some(mode) = bot.mode.as_mut() {
                    mode.avoid(point);
                }
                bot.goal = None;
                return;
            }
            status(&format!(
                "autopilot: cannot reach {end:?} from {position:?} on map {room}"
            ));
            match goal {
                Goal::Exit { exit, .. } if exit.destination.is_some() => {
                    book_failed_exit(bot, room, &exit);
                }
                Goal::Exit { exit, .. } if exit.door.is_some() => {
                    let (class, number) = (exit.class, exit.number);
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
                Goal::Hunt { .. } | Goal::Roam { .. } => {
                    // Standing where the roamer was: look for it again.
                    bot.goal = None;
                }
                Goal::Area { .. } => {
                    // Standing inside the area is enough: the field fires event 2 this step.
                    bot.goal = None;
                }
                Goal::Exit { exit, stage, .. } => {
                    if stage && bot.approach.is_empty() {
                        // The route ends near a volume the grid does not join: search the
                        // last metres from the pose and the speed the hero gets here with.
                        let Some(plan) = self.approach_plan(hero, &exit) else {
                            status(&format!(
                                "autopilot: no walk into the volume of {:?} from {position:?}",
                                exit.destination
                            ));
                            book_failed_exit(bot, room, &exit);
                            bot.goal = None;
                            return;
                        };
                        bot.approach = plan;
                        bot.approach_at = 0;
                    }
                    if !bot.approach.is_empty() {
                        self.press_approach(bot, room, &exit);
                        return;
                    }
                    bot.exit_frames += 1;
                    let frames = if exit.door.is_some() {
                        DOOR_FRAMES
                    } else {
                        WARP_FRAMES
                    };
                    if bot.exit_frames > frames {
                        match exit.destination {
                            Some(_) => {
                                status(&format!("autopilot: no way out at {end:?}"));
                                book_failed_exit(bot, room, &exit);
                            }
                            // A door without destination only opens; let it finish.
                            None => bot.wait = 40,
                        }
                        bot.goal = None;
                        return;
                    }
                    self.pilot_dir = Some(match exit.warp {
                        // A warp fires from presence: walk to the nearest point of its volume,
                        // which the route's end may miss by the edge of the box.
                        Some(volume) => flat(approach::nearest(&volume, position) - position)
                            .normalize_or_zero(),
                        // A doorway fires when the hero walks against its facing; when the
                        // standing cell lies inside the zone itself the stored heading may
                        // point out of it (`M01M10` door `0/2`: heading `-z`, the door opens
                        // walking `+z`). Try the stored direction, then the opposite.
                        None if bot.exit_frames > DOOR_FRAMES / 2 => -exit.heading,
                        None => exit.heading,
                    });
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
        let waypoint = bot.route.get(look).copied().unwrap_or(end);
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
    pub(super) fn next_hop(&self, bot: &Pilot, room: u16, digest: u64) -> Option<u16> {
        hop_towards(&bot.graph, room, |next| pending_room(bot, next, digest))
    }

    /// The first room on the shortest known way from `room` to a room with a way out to
    /// another family of maps not taken yet at this point of the story.
    pub(super) fn next_foreign_hop(&self, bot: &Pilot, room: u16, digest: u64) -> Option<u16> {
        hop_towards(&bot.graph, room, |next| {
            bot.foreign
                .get(&next)
                .is_some_and(|ways| ways.iter().any(|d| !bot.left.contains(&(next, *d, digest))))
        })
    }

    /// Whether map `id` is in the family of the current one (the first three letters of the
    /// codes: `X01`, `M01`): the pilot stays in the place the story is told in.
    pub(super) fn same_family(&self, id: u16) -> bool {
        let family = |id: u16| {
            self.maps
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.code.get(..3).map(str::to_owned))
        };
        let here = self.current.as_ref().and_then(|(e, _)| family(e.id));
        here.is_some() && here == family(id)
    }
}
