//! What the actors of a scene do: the channels an actor command waits in (natives 202 to
//! 228), the lanes that run them, and the spline of a path (214 to 217). The state they act
//! on and the view the application draws stay in `cutscene.rs`.
//!
//! Evidence: `docs/re/stb-natives.md`.

use std::collections::VecDeque;

use glam::Vec3;

use crate::script::event::ActorCommand;
use crate::script::natives::ACTORS;

use super::{Animation, Cutscene, Node, OPAQUE, TICKS_PER_UNIT, Timer, lerp_angle};

/// A command in progress.
#[derive(Debug, Clone)]
pub(super) enum Running {
    Move {
        from: Vec3,
        to: Vec3,
        ground: bool,
        timer: Timer,
        arrival: Option<String>,
    },
    Rotate {
        from: Vec3,
        to: Vec3,
        direction: Option<i32>,
        timer: Timer,
    },
    Scale {
        from: Vec3,
        to: Vec3,
        timer: Timer,
    },
    Alpha {
        from: f32,
        to: f32,
        timer: Timer,
    },
    /// Waits for the current animation to end.
    WaitAnimation,
    /// Waits `count` updates.
    Delay(Timer),
    /// A spline through the path points (214 to 217).
    Path {
        points: Vec<Vec3>,
        timer: Timer,
    },
}

#[derive(Debug, Clone, Default)]
pub(super) struct Channel {
    pub(super) queue: VecDeque<ActorCommand>,
    running: Option<Running>,
}

impl Channel {
    fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.running.is_none()
    }
}

/// Which channel runs a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    Position,
    Rotation,
    Animation,
    Scale,
    Alpha,
    Radius,
}

pub(super) const LANES: [Lane; 6] = [
    Lane::Position,
    Lane::Rotation,
    Lane::Animation,
    Lane::Scale,
    Lane::Alpha,
    Lane::Radius,
];

pub(super) fn lane_of(command: &ActorCommand) -> Option<Lane> {
    use ActorCommand as C;
    Some(match command {
        C::Reset | C::Bind { .. } => return None,
        C::SetPosition(_)
        | C::MoveTo { .. }
        | C::MoveOnGround { .. }
        | C::PathReset
        | C::PathSpeed(_)
        | C::PathPoint(_)
        | C::PathRun => Lane::Position,
        C::SetRotation(_) | C::RotateTo { .. } | C::TurnTowards { .. } => Lane::Rotation,
        C::Animate { .. }
        | C::QueueAnimation { .. }
        | C::StopAnimation
        | C::WaitAnimation
        | C::DelayAnimation(_) => Lane::Animation,
        C::SetScale { .. } => Lane::Scale,
        C::SetAlpha(_) | C::FadeAlpha { .. } => Lane::Alpha,
        C::SetRadius(_) | C::FadeRadius { .. } => Lane::Radius,
    })
}

#[derive(Debug, Clone, Default)]
pub(super) struct Actor {
    pub(super) slot: Option<usize>,
    pub(super) channels: [Channel; 6],
    /// Path points and speed (214 to 217), run by `PathRun`.
    pub(super) path: Vec<Vec3>,
    pub(super) path_speed: i32,
    /// Commands were queued since the last report of idleness.
    pub(super) pending: bool,
    /// Continuous rotation of a negative-duration 209, radians per update.
    pub(super) spin: Option<Vec3>,
}

impl Actor {
    pub(super) fn lane(&mut self, lane: Lane) -> &mut Channel {
        &mut self.channels[LANES.iter().position(|l| *l == lane).unwrap_or(0)]
    }

    pub(super) fn is_idle(&self) -> bool {
        self.channels.iter().all(Channel::is_idle)
    }
}

impl Cutscene {
    pub(super) fn tick_actor(
        &mut self,
        index: usize,
        ground: &dyn Fn(f32, f32, f32) -> Option<f32>,
    ) {
        let target = self.target_of_actor(index);
        if let Some(spin) = self.actors[index].spin {
            // 209 with a negative duration adds the angles at each update.
            let node = self.node_mut(target);
            node.pose.rotation += spin / TICKS_PER_UNIT;
        }
        for (lane_index, lane) in LANES.iter().copied().enumerate() {
            // The first command of a lane that completes at once lets the next one start in
            // the same tick, as the game drains its queue.
            for _ in 0..64 {
                if self.actors[index].channels[lane_index].is_idle() {
                    break;
                }
                let animation_done = self
                    .nodes
                    .get(&target)
                    .and_then(|n| n.animation.as_ref())
                    .is_none_or(|a| a.finished);
                let positions: [Vec3; ACTORS] = std::array::from_fn(|a| self.actor_position(a));
                let own_position = positions[index];
                let serial = &mut self.serial;
                let actor = &mut self.actors[index];
                let node = self.nodes.entry(target).or_default();
                let progressed = step_lane(
                    actor,
                    lane,
                    node,
                    serial,
                    animation_done,
                    own_position,
                    &positions,
                    ground,
                );
                if !progressed {
                    break;
                }
            }
        }
    }
}

/// Advances one lane of an actor by one tick. Returns whether another command may start in
/// the same tick (the running one completed, or there was none to start).
#[allow(clippy::too_many_arguments)]
pub(super) fn step_lane(
    actor: &mut Actor,
    lane: Lane,
    node: &mut Node,
    serial: &mut u32,
    animation_done: bool,
    own_position: Vec3,
    positions: &[Vec3],
    ground: &dyn Fn(f32, f32, f32) -> Option<f32>,
) -> bool {
    let index = LANES.iter().position(|l| *l == lane).unwrap_or(0);
    // The running command first.
    if let Some(running) = actor.channels[index].running.take() {
        let (keep, finished) = advance(running, node, animation_done, ground);
        if let Some(arrival) = finished {
            start_animation(node, serial, &arrival, 0, 1.0);
        }
        return match keep {
            Some(running) => {
                actor.channels[index].running = Some(running);
                false
            }
            None => true,
        };
    }
    let Some(command) = actor.channels[index].queue.pop_front() else {
        return false;
    };
    use ActorCommand as C;
    match command {
        C::SetPosition(p) => {
            node.pose.position = Vec3::from_array(p);
            node.placed = true;
        }
        C::MoveTo {
            position,
            duration,
            arrival_animation,
        } => {
            let to = Vec3::from_array(position);
            match Timer::units(duration) {
                Some(timer) => {
                    actor.channels[index].running = Some(Running::Move {
                        from: node.pose.position,
                        to,
                        ground: false,
                        timer,
                        arrival: arrival_animation,
                    });
                }
                None => {
                    node.pose.position = to;
                    node.placed = true;
                    if let Some(name) = arrival_animation {
                        start_animation(node, serial, &name, 0, 1.0);
                    }
                }
            }
        }
        C::MoveOnGround {
            x,
            z,
            duration,
            arrival_animation,
        } => {
            let from = node.pose.position;
            let to = Vec3::new(x, from.y, z);
            match Timer::units(duration) {
                Some(timer) => {
                    actor.channels[index].running = Some(Running::Move {
                        from,
                        to,
                        ground: true,
                        timer,
                        arrival: arrival_animation,
                    });
                }
                None => {
                    node.pose.position = to;
                    if let Some(y) = ground(x, z, from.y) {
                        node.pose.position.y = y;
                    }
                    if let Some(name) = arrival_animation {
                        start_animation(node, serial, &name, 0, 1.0);
                    }
                }
            }
        }
        C::SetRotation(r) => {
            node.pose.rotation = Vec3::from_array(r);
            actor.spin = None;
        }
        C::RotateTo {
            rotation,
            duration,
            direction,
        } => {
            let to = Vec3::from_array(rotation);
            if duration < 0 {
                actor.spin = Some(to);
            } else {
                actor.spin = None;
                match Timer::units(duration) {
                    Some(timer) => {
                        actor.channels[index].running = Some(Running::Rotate {
                            from: node.pose.rotation,
                            to,
                            direction: Some(direction),
                            timer,
                        });
                    }
                    None => node.pose.rotation = to,
                }
            }
        }
        C::TurnTowards {
            target,
            duration,
            direction,
        } => {
            let at = usize::try_from(target)
                .ok()
                .and_then(|t| positions.get(t))
                .copied()
                .unwrap_or(own_position);
            let delta = at - own_position;
            let mut to = node.pose.rotation;
            to.y = delta.x.atan2(delta.z);
            match Timer::units(duration) {
                Some(timer) => {
                    actor.channels[index].running = Some(Running::Rotate {
                        from: node.pose.rotation,
                        to,
                        direction,
                        timer,
                    });
                }
                None => node.pose.rotation = to,
            }
        }
        C::Animate { name, flags, speed } => {
            start_animation(node, serial, &name, flags, speed);
        }
        C::QueueAnimation { name, flags, speed } => {
            if animation_done || node.animation.is_none() {
                start_animation(node, serial, &name, flags, speed);
            } else {
                // Waits for the current animation to end, then starts.
                actor.channels[index]
                    .queue
                    .push_front(C::QueueAnimation { name, flags, speed });
                return false;
            }
        }
        C::StopAnimation => node.animation = None,
        C::WaitAnimation => {
            if !(animation_done || node.animation.is_none()) {
                actor.channels[index].running = Some(Running::WaitAnimation);
                return false;
            }
        }
        C::DelayAnimation(count) => {
            if let Some(timer) = Timer::units(count) {
                actor.channels[index].running = Some(Running::Delay(timer));
                return false;
            }
        }
        C::SetScale { scale, duration } => {
            let to = Vec3::from_array(scale);
            match Timer::units(duration) {
                Some(timer) => {
                    actor.channels[index].running = Some(Running::Scale {
                        from: node.pose.scale,
                        to,
                        timer,
                    });
                }
                None => node.pose.scale = to,
            }
        }
        C::SetAlpha(alpha) => node.pose.alpha = alpha.clamp(0, OPAQUE),
        C::FadeAlpha { alpha, duration } => match Timer::units(duration) {
            Some(timer) => {
                actor.channels[index].running = Some(Running::Alpha {
                    from: node.pose.alpha as f32,
                    to: alpha.clamp(0, OPAQUE) as f32,
                    timer,
                });
            }
            None => node.pose.alpha = alpha.clamp(0, OPAQUE),
        },
        // The collision radius of the object has no visible effect.
        C::SetRadius(_) | C::FadeRadius { .. } => {}
        C::PathReset => actor.path.clear(),
        C::PathSpeed(speed) => actor.path_speed = speed,
        C::PathPoint(p) => {
            // 16 points at most; the others are ignored (`02157964`).
            if actor.path.len() < 16 {
                actor.path.push(Vec3::from_array(p));
            }
        }
        C::PathRun => {
            let points = std::mem::take(&mut actor.path);
            if points.len() >= 2 {
                let length: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
                // The divisor of the segment lengths (`021579ac`); its unit is **[unknown]**.
                let speed = actor.path_speed.max(1) as f32;
                let units = (length / speed).ceil().max(1.0) as i32;
                if let Some(timer) = Timer::units(units) {
                    actor.channels[index].running = Some(Running::Path { points, timer });
                    return false;
                }
            }
        }
        C::Reset | C::Bind { .. } => {}
    }
    true
}

pub(super) fn start_animation(
    node: &mut Node,
    serial: &mut u32,
    name: &str,
    flags: i32,
    speed: f32,
) {
    *serial += 1;
    node.animation = Some(Animation {
        name: name.to_owned(),
        flags,
        speed,
        serial: *serial,
        finished: false,
    });
}

/// Catmull-Rom point of a path at `t` in `0..=1` (the game's spline type is **[hypothesis]**,
/// `docs/re/stb-natives.md`, native 217).
pub(super) fn spline(points: &[Vec3], t: f32) -> (Vec3, Vec3) {
    let segments = points.len() - 1;
    let scaled = t.clamp(0.0, 1.0) * segments as f32;
    let i = (scaled.floor() as usize).min(segments - 1);
    let u = scaled - i as f32;
    let at = |k: isize| points[k.clamp(0, segments as isize) as usize];
    let (p0, p1, p2, p3) = (
        at(i as isize - 1),
        at(i as isize),
        at(i as isize + 1),
        at(i as isize + 2),
    );
    let point = 0.5
        * (2.0 * p1
            + (p2 - p0) * u
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u * u
            + (3.0 * p1 - p0 - 3.0 * p2 + p3) * u * u * u);
    let tangent = 0.5
        * ((p2 - p0)
            + (4.0 * p0 - 10.0 * p1 + 8.0 * p2 - 2.0 * p3) * u
            + (9.0 * p1 - 3.0 * p0 - 9.0 * p2 + 3.0 * p3) * u * u);
    (point, tangent)
}

/// Advances a running command by one tick. Returns it back while it goes on, and the arrival
/// animation to start when it ended.
pub(super) fn advance(
    running: Running,
    node: &mut Node,
    animation_done: bool,
    ground: &dyn Fn(f32, f32, f32) -> Option<f32>,
) -> (Option<Running>, Option<String>) {
    match running {
        Running::Move {
            from,
            to,
            ground: on_ground,
            mut timer,
            arrival,
        } => {
            let (t, done) = timer.advance(1.0);
            let mut p = from.lerp(to, t);
            if on_ground && let Some(y) = ground(p.x, p.z, node.pose.position.y) {
                p.y = y;
            }
            node.pose.position = p;
            node.placed = true;
            if done {
                (None, arrival)
            } else {
                (
                    Some(Running::Move {
                        from,
                        to,
                        ground: on_ground,
                        timer,
                        arrival,
                    }),
                    None,
                )
            }
        }
        Running::Rotate {
            from,
            to,
            direction,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            node.pose.rotation = Vec3::new(
                from.x + (to.x - from.x) * t,
                lerp_angle(from.y, to.y, t, direction),
                from.z + (to.z - from.z) * t,
            );
            if done {
                (None, None)
            } else {
                (
                    Some(Running::Rotate {
                        from,
                        to,
                        direction,
                        timer,
                    }),
                    None,
                )
            }
        }
        Running::Scale {
            from,
            to,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            node.pose.scale = from.lerp(to, t);
            if done {
                (None, None)
            } else {
                (Some(Running::Scale { from, to, timer }), None)
            }
        }
        Running::Alpha {
            from,
            to,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            node.pose.alpha = (from + (to - from) * t).round() as i32;
            if done {
                (None, None)
            } else {
                (Some(Running::Alpha { from, to, timer }), None)
            }
        }
        Running::WaitAnimation => {
            if animation_done || node.animation.is_none() {
                (None, None)
            } else {
                (Some(Running::WaitAnimation), None)
            }
        }
        Running::Delay(mut timer) => {
            let (_, done) = timer.advance(1.0);
            ((!done).then_some(Running::Delay(timer)), None)
        }
        Running::Path { points, mut timer } => {
            let (t, done) = timer.advance(1.0);
            let (p, tangent) = spline(&points, t);
            node.pose.position = p;
            node.placed = true;
            if tangent.x != 0.0 || tangent.z != 0.0 {
                node.pose.rotation.y = tangent.x.atan2(tangent.z);
            }
            if done {
                (None, None)
            } else {
                (Some(Running::Path { points, timer }), None)
            }
        }
    }
}
