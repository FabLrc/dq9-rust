//! The camera of a scene: the cartesian, orbital, field-of-view and shake queues of its
//! commands, the timers that advance them and the orbit the eye is derived from. The state
//! and the view the application draws stay in `cutscene.rs`.
//!
//! Evidence: `docs/re/stb-natives.md`.

use std::collections::VecDeque;

use glam::Vec3;

use crate::script::event::Camera as CameraCommand;
use crate::script::natives::ACTORS;

use super::{BoneCamera, Cutscene, ThreeBoneCamera, Timer, lerp_angle};

/// `eye = target + Ry(θ)·(0, h, √(r² − h²))` (`0202e5e8`, `docs/re/stb-natives.md`).
pub(super) fn orbit_eye(target: Vec3, azimuth: f32, height: f32, distance: f32) -> Vec3 {
    let horizontal = (distance * distance - height * height).max(0.0).sqrt();
    target
        + Vec3::new(
            horizontal * azimuth.sin(),
            height,
            horizontal * azimuth.cos(),
        )
}

/// A camera command in progress.
#[derive(Debug, Clone)]
pub(super) enum CamRunning {
    Both {
        from: [Vec3; 2],
        to: [Vec3; 2],
        snap: bool,
        timer: Timer,
    },
    Target {
        from: Vec3,
        to: Vec3,
        timer: Timer,
    },
    Position {
        from: Vec3,
        to: Vec3,
        timer: Timer,
    },
    Orbit {
        from: [f32; 3],
        to: [f32; 3],
        direction: i32,
        snap: bool,
        timer: Timer,
    },
    Fov {
        from: f32,
        to: f32,
        timer: Timer,
    },
    Shake {
        amplitude: Vec3,
        target_only: bool,
        /// `None`: never ends.
        timer: Option<Timer>,
    },
}

#[derive(Debug, Clone, Default)]
pub(super) struct CamLane {
    pub(super) queue: VecDeque<CameraCommand>,
    pub(super) running: Option<CamRunning>,
}

#[derive(Debug, Clone)]
pub(super) struct Orbit {
    azimuth: f32,
    height: f32,
    distance: f32,
}

#[derive(Debug, Clone)]
pub(super) struct Cam {
    pub(super) eye: Vec3,
    pub(super) target: Vec3,
    orbit: Option<Orbit>,
    pub(super) half_fov: f32,
    pub(super) roll: f32,
    follow: Option<(i32, Vec3)>,
    pub(super) bones: Option<BoneCamera>,
    pub(super) three_bone: Option<ThreeBoneCamera>,
    /// Queues: cartesian moves, orbital moves, field of view, shake.
    pub(super) lanes: [CamLane; 4],
    pub(super) shake: Vec3,
    pub(super) shake_target_only: bool,
    updates: u32,
    pub(super) pending: bool,
}

impl Default for Cam {
    fn default() -> Self {
        Self {
            eye: Vec3::new(0.0, 5.0, 20.0),
            target: Vec3::ZERO,
            orbit: None,
            half_fov: 15.0,
            roll: 0.0,
            follow: None,
            bones: None,
            three_bone: None,
            lanes: Default::default(),
            shake: Vec3::ZERO,
            shake_target_only: false,
            updates: 0,
            pending: false,
        }
    }
}

pub(super) const CART: usize = 0;
pub(super) const ORBITAL: usize = 1;
pub(super) const FOV: usize = 2;
pub(super) const SHAKE: usize = 3;

impl Cutscene {
    pub(super) fn camera_command(&mut self, command: &CameraCommand) {
        use CameraCommand as C;
        let cam = &mut self.cam;
        cam.pending = true;
        let lane = match command {
            C::Reset => {
                // `021588e4`: empties the queues and counters; the view stays.
                cam.lanes = Default::default();
                cam.follow = None;
                cam.shake = Vec3::ZERO;
                cam.bones = None;
                cam.three_bone = None;
                // The native raises the busy flag: the idle report follows.
                return;
            }
            C::SetPosition(_)
            | C::SetTarget(_)
            | C::MoveBoth { .. }
            | C::MoveTarget { .. }
            | C::MovePosition { .. }
            | C::MoveTargetKeepOrbit { .. } => CART,
            C::SetOrbit { .. } | C::MoveOrbit { .. } => ORBITAL,
            C::MoveTargetAndOrbit {
                target,
                azimuth,
                height,
                distance,
                duration,
                direction,
            } => {
                // `0215dd58`: a cartesian move of the target and an orbital move.
                cam.lanes[CART].queue.push_back(C::MoveTarget {
                    target: *target,
                    duration: *duration,
                });
                cam.lanes[ORBITAL].queue.push_back(C::MoveOrbit {
                    azimuth: *azimuth,
                    height: *height,
                    distance: *distance,
                    duration: *duration,
                    direction: *direction,
                    snap: false,
                });
                return;
            }
            C::Shake { .. } => SHAKE,
            C::MoveFov { .. } => FOV,
            C::SetFov(degrees) => {
                cam.half_fov = *degrees;
                return;
            }
            C::Roll(degrees) => {
                cam.roll = *degrees;
                return;
            }
            C::FollowActor { actor, offset } => {
                cam.follow = Some((*actor, Vec3::from_array(*offset)));
                return;
            }
            C::StopFollow => {
                cam.follow = None;
                return;
            }
            C::RestoreSaved => {
                if let Some(saved) = self.saved_cam.take() {
                    self.cam = *saved;
                }
                return;
            }
            C::BoneCamera { slot, eye, target } => {
                // 572 saves the running camera first; 531 gives it back.
                self.saved_cam = Some(Box::new(self.cam.clone()));
                self.cam.three_bone = None;
                self.cam.bones = Some(BoneCamera {
                    slot: *slot,
                    eye: eye.clone(),
                    target: target.clone(),
                });
                return;
            }
            C::ThreeBoneCamera {
                source,
                target,
                eye,
                lookat,
                foot,
            } => {
                // 552 writes the same stash word as 572 (`ctx+4`, `020100cc`); 531 restores it.
                self.saved_cam = Some(Box::new(self.cam.clone()));
                self.cam.bones = None;
                self.cam.three_bone = Some(ThreeBoneCamera {
                    source: *source,
                    target: *target,
                    eye: eye.clone(),
                    lookat: lookat.clone(),
                    foot: foot.clone(),
                });
                return;
            }
        };
        cam.lanes[lane].queue.push_back(command.clone());
    }

    pub(super) fn tick_camera(&mut self) {
        // Following an actor writes the target at each update.
        if let Some((actor, offset)) = self.cam.follow
            && let Some(actor) = usize::try_from(actor).ok().filter(|a| *a < ACTORS)
        {
            self.cam.target = self.actor_position(actor) + offset;
        }
        for lane in 0..self.cam.lanes.len() {
            for _ in 0..64 {
                if !step_camera_lane(&mut self.cam, lane) {
                    break;
                }
            }
        }
        let cam = &mut self.cam;
        if let Some(orbit) = &cam.orbit {
            cam.eye = orbit_eye(cam.target, orbit.azimuth, orbit.height, orbit.distance);
        }
        // The shake offset alternates every update (`02158494`: modulo 4).
        cam.updates += 1;
        let shaking = cam.lanes[SHAKE].running.clone();
        cam.shake = match shaking {
            Some(CamRunning::Shake {
                amplitude,
                target_only,
                ..
            }) => {
                cam.shake_target_only = target_only;
                let phase = (cam.updates / 2) % 4;
                amplitude * [1.0, 0.0, -1.0, 0.0][phase as usize]
            }
            _ => Vec3::ZERO,
        };
    }
}

/// Advances one camera lane by one tick; returns whether the next command may start now.
pub(super) fn step_camera_lane(cam: &mut Cam, lane: usize) -> bool {
    use CameraCommand as C;
    if let Some(running) = cam.lanes[lane].running.take() {
        return match advance_camera(running, cam) {
            Some(running) => {
                cam.lanes[lane].running = Some(running);
                false
            }
            None => true,
        };
    }
    let Some(command) = cam.lanes[lane].queue.pop_front() else {
        return false;
    };
    match command {
        C::SetPosition(p) => {
            cam.eye = Vec3::from_array(p);
            cam.orbit = None;
        }
        C::SetTarget(p) => cam.target = Vec3::from_array(p),
        C::MoveBoth {
            position,
            target,
            duration,
            snap,
        } => {
            let to = [Vec3::from_array(position), Vec3::from_array(target)];
            cam.orbit = None;
            match Timer::units(duration) {
                Some(timer) => {
                    cam.lanes[lane].running = Some(CamRunning::Both {
                        from: [cam.eye, cam.target],
                        to,
                        snap,
                        timer,
                    });
                    return false;
                }
                None => {
                    cam.eye = to[0];
                    cam.target = to[1];
                }
            }
        }
        C::MoveTargetKeepOrbit { target, duration } => {
            // `0215dbd0`: the eye keeps the offset it has at the call.
            let target = Vec3::from_array(target);
            let offset = cam.eye - cam.target;
            cam.orbit = None;
            match Timer::units(duration) {
                Some(timer) => {
                    cam.lanes[lane].running = Some(CamRunning::Both {
                        from: [cam.eye, cam.target],
                        to: [target + offset, target],
                        snap: true,
                        timer,
                    });
                    return false;
                }
                None => {
                    cam.eye = target + offset;
                    cam.target = target;
                }
            }
        }
        C::MoveTarget { target, duration } => {
            let to = Vec3::from_array(target);
            match Timer::units(duration) {
                Some(timer) => {
                    cam.lanes[lane].running = Some(CamRunning::Target {
                        from: cam.target,
                        to,
                        timer,
                    });
                    return false;
                }
                None => cam.target = to,
            }
        }
        C::MovePosition { position, duration } => {
            let to = Vec3::from_array(position);
            cam.orbit = None;
            match Timer::units(duration) {
                Some(timer) => {
                    cam.lanes[lane].running = Some(CamRunning::Position {
                        from: cam.eye,
                        to,
                        timer,
                    });
                    return false;
                }
                None => cam.eye = to,
            }
        }
        C::SetOrbit {
            azimuth,
            height,
            distance,
        } => {
            cam.orbit = Some(Orbit {
                azimuth,
                height,
                distance,
            });
        }
        C::MoveOrbit {
            azimuth,
            height,
            distance,
            duration,
            direction,
            snap,
        } => {
            let current = cam.orbit.clone().unwrap_or_else(|| {
                // An orbit derived from the eye the camera has.
                let offset = cam.eye - cam.target;
                Orbit {
                    azimuth: offset.x.atan2(offset.z),
                    height: offset.y,
                    distance: offset.length(),
                }
            });
            let from = [current.azimuth, current.height, current.distance];
            let to = [azimuth, height, distance];
            match Timer::units(duration) {
                Some(timer) => {
                    cam.orbit = Some(current);
                    cam.lanes[lane].running = Some(CamRunning::Orbit {
                        from,
                        to,
                        direction,
                        snap,
                        timer,
                    });
                    return false;
                }
                None => {
                    cam.orbit = Some(Orbit {
                        azimuth,
                        height,
                        distance,
                    });
                }
            }
        }
        C::MoveFov { degrees, duration } => match Timer::units(duration) {
            Some(timer) => {
                cam.lanes[lane].running = Some(CamRunning::Fov {
                    from: cam.half_fov,
                    to: degrees,
                    timer,
                });
                return false;
            }
            None => cam.half_fov = degrees,
        },
        C::Shake {
            amplitude,
            duration,
            target_only,
        } => {
            cam.lanes[lane].running = Some(CamRunning::Shake {
                amplitude: Vec3::from_array(amplitude),
                target_only,
                timer: Timer::units(duration),
            });
            // A shake of zero updates does nothing; negative never ends (`02158494`).
            if duration == 0 {
                cam.lanes[lane].running = None;
            }
            return false;
        }
        // Handled when queued.
        C::Reset
        | C::MoveTargetAndOrbit { .. }
        | C::FollowActor { .. }
        | C::StopFollow
        | C::Roll(_)
        | C::RestoreSaved
        | C::SetFov(_)
        | C::BoneCamera { .. }
        | C::ThreeBoneCamera { .. } => {}
    }
    true
}

pub(super) fn advance_camera(running: CamRunning, cam: &mut Cam) -> Option<CamRunning> {
    match running {
        CamRunning::Both {
            from,
            to,
            snap,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            cam.eye = from[0].lerp(to[0], t);
            cam.target = from[1].lerp(to[1], t);
            if done {
                if snap {
                    cam.eye = to[0];
                    cam.target = to[1];
                }
                None
            } else {
                Some(CamRunning::Both {
                    from,
                    to,
                    snap,
                    timer,
                })
            }
        }
        CamRunning::Target {
            from,
            to,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            cam.target = from.lerp(to, t);
            (!done).then_some(CamRunning::Target { from, to, timer })
        }
        CamRunning::Position {
            from,
            to,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            cam.eye = from.lerp(to, t);
            (!done).then_some(CamRunning::Position { from, to, timer })
        }
        CamRunning::Orbit {
            from,
            to,
            direction,
            snap,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            // −1: the short way round; 0: decreasing; else increasing (native 311).
            let azimuth = lerp_angle(
                from[0],
                to[0],
                t,
                match direction {
                    -1 => None,
                    0 => Some(1),
                    _ => Some(0),
                },
            );
            cam.orbit = Some(Orbit {
                azimuth,
                height: from[1] + (to[1] - from[1]) * t,
                distance: from[2] + (to[2] - from[2]) * t,
            });
            if done {
                if snap {
                    cam.orbit = Some(Orbit {
                        azimuth: to[0],
                        height: to[1],
                        distance: to[2],
                    });
                }
                None
            } else {
                Some(CamRunning::Orbit {
                    from,
                    to,
                    direction,
                    snap,
                    timer,
                })
            }
        }
        CamRunning::Fov {
            from,
            to,
            mut timer,
        } => {
            let (t, done) = timer.advance(1.0);
            cam.half_fov = from + (to - from) * t;
            (!done).then_some(CamRunning::Fov { from, to, timer })
        }
        CamRunning::Shake {
            amplitude,
            target_only,
            timer,
        } => match timer {
            None => Some(CamRunning::Shake {
                amplitude,
                target_only,
                timer: None,
            }),
            Some(mut timer) => {
                let (_, done) = timer.advance(1.0);
                (!done).then_some(CamRunning::Shake {
                    amplitude,
                    target_only,
                    timer: Some(timer),
                })
            }
        },
    }
}
