//! The natives of `.stb` scripts (`g_StbNatives`, `ov_001::02164d6c`; handlers `ov_001::0215ae1c…`)
//! as requests to the application and a small model of what the scripts poll.
//!
//! A native pushes nothing: it returns results through address arguments, and a script waits
//! by looping on such a result with `yield` (`docs/re/stb.md`). The application is the only
//! one who knows when a message closed or a move ended, so [`Runtime`] holds what the scripts
//! poll (405, 558, 204, 301, 234, 507, 543, 544): the script raises it when it queues the work
//! and [`super::Signal`]s lower it.
//!
//! Natives whose role is established but that act on the DS's memory or hardware only (the
//! texture arenas, resource flags, allocator) have no equivalent here and are skipped without
//! a report. Those whose role is not known are reported once ([`Event::Unimplemented`]).

use std::collections::{BTreeMap, BTreeSet};

use super::event::{ActorCommand, Camera, Event, Lock, Screen, Sound, Text};
use super::vm::{Args, Natives, Outs};
use crate::trigger::World;

/// Actor controllers of an event (`Stb_Actor`, `ov_001::0215ab20`).
pub const ACTORS: usize = 24;
/// Binding slots (native 202).
pub const SLOTS: usize = 32;
/// Sound-effect handles (natives 728, 732).
const HANDLES: i32 = 16;

/// What the scripts poll, and what the natives accumulate.
#[derive(Debug, Clone)]
pub struct Runtime {
    /// Native 405: a message is on screen or still being written.
    pub message_open: bool,
    /// Native 558: the answer of the last question (YESNO: 0 yes, 1 no).
    pub answer: i32,
    /// Native 204: an actor has queued commands left.
    pub actor_busy: [bool; ACTORS],
    /// Native 301: camera commands are running.
    pub camera_busy: bool,
    /// Native 234: the animation of a binding slot is playing.
    pub animating: [bool; SLOTS],
    /// Native 507.
    pub preload_pending: bool,
    /// The message numbers of the event's text table, when known (native 400).
    pub messages: Option<BTreeSet<i32>>,
    /// Natives 543, 544: position and rotation of each actor, as last commanded unless the
    /// application overrides them.
    pub pose: [([f32; 3], [f32; 3]); ACTORS],
    /// Natives 588, 589, 597: the scene's time-of-day profile; `None` follows the world.
    scene_time: Option<i32>,
    /// Native 538: the event to chain to when this one ends (0: none).
    pub(super) next_event: u16,
    /// Count of cast members that took a character id (native 566).
    cast_count: i32,
    /// Cast members declared so far, `(attach id, category)` (natives 566, 596).
    cast: Vec<(i32, i32)>,
    next_handle: i32,
    next_sprite: i32,
    /// Native id → calls, for natives reported as unimplemented.
    pub(super) unimplemented: BTreeMap<i32, u32>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            message_open: false,
            answer: 0,
            actor_busy: [false; ACTORS],
            camera_busy: false,
            animating: [false; SLOTS],
            preload_pending: false,
            messages: None,
            pose: [([0.0; 3], [0.0; 3]); ACTORS],
            scene_time: None,
            next_event: 0,
            cast_count: 0,
            cast: Vec::new(),
            next_handle: 0,
            next_sprite: 0,
            unimplemented: BTreeMap::new(),
        }
    }
}

/// The per-update inputs of the natives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// Ticks of 1/60 s since the previous update (`GameState::GetTickCount`, capped at 3;
    /// measured 2, i.e. updates at 30 Hz): native 840.
    pub ticks: u32,
}

impl Default for Frame {
    fn default() -> Self {
        Self { ticks: 2 }
    }
}

/// The natives of one slice of the interpreter.
pub(super) struct Ctx<'a> {
    pub rt: &'a mut Runtime,
    pub events: &'a mut Vec<Event>,
    pub world: &'a dyn World,
    pub frame: Frame,
}

fn text(args: &Args, i: usize) -> String {
    args.str(i)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}

fn vec3(args: &Args, i: usize) -> [f32; 3] {
    [args.float(i), args.float(i + 1), args.float(i + 2)]
}

/// The `[&y]`-style optional string argument of the actor natives.
fn opt_text(args: &Args, i: usize) -> Option<String> {
    args.is_str(i).then(|| text(args, i))
}

impl Ctx<'_> {
    fn actor(&mut self, args: &Args, command: ActorCommand) {
        let actor = args.int(0);
        if let Some(busy) = usize::try_from(actor)
            .ok()
            .and_then(|a| self.rt.actor_busy.get_mut(a))
        {
            *busy = true;
        }
        self.events.push(Event::Actor { actor, command });
    }

    fn camera(&mut self, camera: Camera) {
        self.rt.camera_busy = true;
        self.events.push(Event::Camera(camera));
    }

    fn fade(&mut self, args: &Args, screen: Screen, level: Option<i32>, lock: Lock) {
        // 101, 105, 107, 112, 114, 116, 118, 120: the target defaults to black; the
        // fixed-target forms (100, 106, 117, 119, 121) pass `level` themselves.
        self.events.push(Event::Fade {
            screen,
            level: level.unwrap_or_else(|| args.int_or(1, -16)),
            ticks: args.int(0),
            lock,
        });
    }

    fn pose_mut(&mut self, actor: i32) -> Option<&mut ([f32; 3], [f32; 3])> {
        usize::try_from(actor)
            .ok()
            .and_then(|a| self.rt.pose.get_mut(a))
    }

    fn scene_time(&self) -> i32 {
        self.rt
            .scene_time
            .unwrap_or(self.world.time_of_day() as i32)
    }

    /// The `StbNat_*` handlers: `id` is the native number, `a` its arguments.
    fn dispatch(&mut self, id: i32, a: &Args, out: &mut Outs) -> bool {
        use ActorCommand as Act;
        match id {
            // --- fades (`ov_001::0215b2cc…`), ticks of 1/60 s ---
            100 => self.fade(a, Screen::Both, Some(0), Lock::Keep),
            101 => self.fade(a, Screen::Both, None, Lock::Keep),
            105 => self.fade(a, Screen::Sub, None, Lock::Keep),
            106 => self.fade(a, Screen::Main, Some(0), Lock::Keep),
            107 => self.fade(a, Screen::Main, None, Lock::Keep),
            109 => self.events.push(Event::SwapScreens(true)),
            110 => self.events.push(Event::SwapScreens(false)),
            112 => self.fade(a, Screen::Both, None, Lock::Lock),
            114 => self.fade(a, Screen::Sub, None, Lock::Lock),
            116 => self.fade(a, Screen::Main, None, Lock::Lock),
            117 => self.fade(a, Screen::Both, Some(0), Lock::Unlock),
            118 => self.fade(a, Screen::Both, None, Lock::Unlock),
            119 => self.fade(a, Screen::Sub, Some(0), Lock::Unlock),
            120 => self.fade(a, Screen::Sub, None, Lock::Unlock),
            121 => self.fade(a, Screen::Main, Some(0), Lock::Unlock),
            122 => self.fade(a, Screen::Main, None, Lock::Unlock),
            // --- characters and slots ---
            200 => self.events.push(Event::LoadModel {
                path: text(a, 0),
                character: a.int(1),
                selector: a.int_or(2, 0),
            }),
            201 => self.events.push(Event::CharacterVisible {
                character: a.int(0),
                visible: a.int_or(1, 1) != 0,
            }),
            202 => self.events.push(Event::BindSlot {
                slot: a.int(0),
                character: a.int(1),
                kind: a.int(2),
            }),
            212 => self.events.push(Event::ReleaseModel {
                character: a.int(0),
            }),
            228 => self.events.push(Event::CloneModel {
                source: a.int(0),
                destination: a.int(1),
            }),
            229 => self.events.push(Event::LoadModelArchive {
                path: text(a, 0),
                character: a.int(1),
                tag: a.int_or(3, 3),
            }),
            230 => self.events.push(Event::RemoveAnimationPackage {
                character: a.int(0),
                tag: a.int_or(1, 3),
            }),
            233 => self.events.push(Event::LoadMonster {
                path: text(a, 0),
                character: a.int(1),
            }),
            235 => self.events.push(Event::AttachBone {
                child: a.int(0),
                parent: a.int(1),
                bone: opt_text(a, 2),
            }),
            236 => self.events.push(Event::DetachBone { slot: a.int(0) }),
            570 => self.events.push(Event::SlotVisible {
                slot: a.int(0),
                visible: a.int(1) != 0,
            }),
            // --- actor queues ---
            203 => self.actor(a, Act::Reset),
            204 => {
                let busy = usize::try_from(a.int(0))
                    .ok()
                    .and_then(|i| self.rt.actor_busy.get(i))
                    .copied()
                    .unwrap_or(false);
                out.int(a, 1, i32::from(busy));
            }
            205 => self.actor(a, Act::Bind { slot: a.int(1) }),
            206 => {
                let position = vec3(a, 1);
                if let Some(pose) = self.pose_mut(a.int(0)) {
                    pose.0 = position;
                }
                self.actor(a, Act::SetPosition(position));
            }
            207 => {
                let position = vec3(a, 1);
                if let Some(pose) = self.pose_mut(a.int(0)) {
                    pose.0 = position;
                }
                self.actor(
                    a,
                    Act::MoveTo {
                        position,
                        duration: a.int(4),
                        arrival_animation: opt_text(a, 5),
                    },
                );
            }
            208 => {
                let rotation = vec3(a, 1);
                if let Some(pose) = self.pose_mut(a.int(0)) {
                    pose.1 = rotation;
                }
                self.actor(a, Act::SetRotation(rotation));
            }
            209 => {
                let rotation = vec3(a, 1);
                let duration = a.int(4);
                if duration >= 0
                    && let Some(pose) = self.pose_mut(a.int(0))
                {
                    pose.1 = rotation;
                }
                self.actor(
                    a,
                    Act::RotateTo {
                        rotation,
                        duration,
                        direction: a.int_or(5, 0),
                    },
                );
            }
            210 | 224 => {
                let (name, flags, speed) = (
                    text(a, 1),
                    a.int_or(2, 0),
                    if a.len() > 3 { a.float(3) } else { 1.0 },
                );
                let animation = if id == 210 {
                    Act::Animate { name, flags, speed }
                } else {
                    Act::QueueAnimation { name, flags, speed }
                };
                self.actor(a, animation);
            }
            211 => self.actor(
                a,
                Act::SetScale {
                    scale: vec3(a, 1),
                    duration: a.int_or(4, 0),
                },
            ),
            213 => self.actor(a, Act::WaitAnimation),
            214 => self.actor(a, Act::PathReset),
            215 => self.actor(a, Act::PathSpeed(a.int(1))),
            216 => self.actor(a, Act::PathPoint(vec3(a, 1))),
            217 => self.actor(a, Act::PathRun),
            218 => self.actor(a, Act::DelayAnimation(a.int(1))),
            219 => self.actor(a, Act::SetAlpha(a.int(1))),
            220 => self.actor(
                a,
                Act::FadeAlpha {
                    alpha: a.int(1),
                    duration: a.int_or(2, 0),
                },
            ),
            221 => self.actor(
                a,
                Act::TurnTowards {
                    target: a.int(1),
                    duration: a.int(2),
                    direction: (a.len() > 3).then(|| a.int(3)),
                },
            ),
            222 => self.actor(a, Act::StopAnimation),
            226 => self.actor(a, Act::SetRadius(a.float(1))),
            227 => self.actor(
                a,
                Act::FadeRadius {
                    target: a.float(1),
                    duration: a.int(2),
                },
            ),
            232 => {
                let (x, z) = (a.float(1), a.float(2));
                if let Some(pose) = self.pose_mut(a.int(0)) {
                    pose.0[0] = x;
                    pose.0[2] = z;
                }
                self.actor(
                    a,
                    Act::MoveOnGround {
                        x,
                        z,
                        duration: a.int(3),
                        arrival_animation: opt_text(a, 4),
                    },
                );
            }
            234 => {
                let playing = usize::try_from(a.int(0))
                    .ok()
                    .and_then(|i| self.rt.animating.get(i))
                    .copied()
                    .unwrap_or(false);
                out.int(a, 1, i32::from(playing));
            }
            543 | 544 => {
                let (position, rotation) = usize::try_from(a.int(0))
                    .ok()
                    .and_then(|i| self.rt.pose.get(i))
                    .copied()
                    .unwrap_or_default();
                let v = if id == 543 { position } else { rotation };
                for (i, value) in v.into_iter().enumerate() {
                    out.float(a, 1 + i, value);
                }
            }
            // --- camera ---
            300 => self.camera(Camera::Reset),
            301 => out.int(a, 0, i32::from(self.rt.camera_busy)),
            302 => self.camera(Camera::SetPosition(vec3(a, 0))),
            303 => self.camera(Camera::SetTarget(vec3(a, 0))),
            304 => self.camera(Camera::MoveBoth {
                position: vec3(a, 0),
                target: vec3(a, 3),
                duration: a.int(6),
                snap: a.int_or(7, 0) != 0,
            }),
            305 => self.camera(Camera::MoveTarget {
                target: vec3(a, 0),
                duration: a.int(3),
            }),
            306 => self.camera(Camera::MovePosition {
                position: vec3(a, 0),
                duration: a.int(3),
            }),
            310 => self.camera(Camera::SetOrbit {
                azimuth: a.float(0),
                height: a.float(1),
                distance: a.float(2),
            }),
            311 => self.camera(Camera::MoveOrbit {
                azimuth: a.float(0),
                height: a.float(1),
                distance: a.float(2),
                duration: a.int(3),
                direction: a.int_or(4, -1),
                snap: a.int_or(5, 0) != 0,
            }),
            317 | 326 => self.camera(Camera::Shake {
                amplitude: vec3(a, 0),
                duration: a.int(3),
                target_only: id == 326,
            }),
            321 => self.camera(Camera::MoveTargetKeepOrbit {
                target: vec3(a, 0),
                duration: a.int(3),
            }),
            322 => self.camera(Camera::MoveTargetAndOrbit {
                target: vec3(a, 0),
                azimuth: a.float(3),
                height: a.float(4),
                distance: a.float(5),
                duration: a.int(6),
                direction: a.int_or(7, -1),
            }),
            324 => self.camera(Camera::FollowActor {
                actor: a.int(0),
                offset: if a.len() > 3 { vec3(a, 1) } else { [0.0; 3] },
            }),
            325 => self.camera(Camera::StopFollow),
            327 => self.camera(Camera::Roll(a.float(0))),
            531 => self.camera(Camera::RestoreSaved),
            532 => self.camera(Camera::SetFov(a.float(0))),
            572 => self.camera(Camera::BoneCamera {
                slot: a.int(0),
                eye: text(a, 1),
                target: text(a, 2),
            }),
            580 => self.camera(Camera::MoveFov {
                degrees: a.float(0),
                duration: a.int(1),
            }),
            // --- messages ---
            400 => {
                let text = if a.is_str(0) {
                    Text::Literal(text(a, 0))
                } else {
                    Text::Id(a.int(0))
                };
                // `ov_001::0215e2a4`: a number absent from the event's table shows nothing
                // and leaves the message state as it was.
                let known = match (&text, &self.rt.messages) {
                    (Text::Id(id), Some(ids)) => ids.contains(id),
                    _ => true,
                };
                if known {
                    self.rt.message_open = true;
                    self.events.push(Event::Message {
                        text,
                        flags: a.int_or(1, 0),
                    });
                }
            }
            401 => {
                self.rt.message_open = false;
                self.events.push(Event::CloseMessage);
            }
            405 => out.int(a, 0, i32::from(self.rt.message_open)),
            410 => self.events.push(Event::HideMessageFrame),
            558 => out.int(a, 0, self.rt.answer),
            // --- scene ---
            506 | 845 => {
                let paths = (0..a.len()).filter(|&i| a.is_str(i)).map(|i| text(a, i));
                self.rt.preload_pending = true;
                self.events.push(Event::Preload(paths.collect()));
            }
            507 => out.int(a, 0, i32::from(self.rt.preload_pending)),
            508 => self.events.push(Event::ReleasePreloaded),
            521 => {
                let handle = self.rt.next_sprite;
                self.rt.next_sprite = (handle + 1) % SLOTS as i32;
                out.int(a, 1, handle);
                self.events.push(Event::CreateSprite {
                    name: text(a, 0),
                    handle,
                });
            }
            522 => self.events.push(Event::DestroySprite { handle: a.int(0) }),
            538 => self.rt.next_event = a.int(0) as u16,
            548 => self.events.push(Event::LightingOverride(Some(a.int(0)))),
            549 => self.events.push(Event::LightingOverride(None)),
            566 => self.declare_cast(a),
            567 => self.events.push(Event::DeclareCharacterArchive {
                path: text(a, 0),
                attach: a.int(1),
            }),
            568 => {
                let mask = (0..a.len()).fold(0, |m, i| m | (a.int(i) & 0x07ff_ffff));
                self.events.push(Event::AddSceneFlags(mask));
            }
            578 => self.events.push(Event::LightIntensity {
                intensity: a.float(0),
                duration: a.int_or(1, 0),
            }),
            579 => self.events.push(Event::DayTimerStopped(a.int(0) != 0)),
            588 => {
                self.rt.scene_time = Some(a.int(0));
                self.events.push(Event::SceneTimeOfDay(a.int(0)));
            }
            589 => {
                self.rt.scene_time = None;
                self.events.push(Event::SyncSceneTimeOfDay);
            }
            595 => out.int(a, 0, 0),
            596 => {
                // `ov_001::02161ab8`: the declaration whose attach id is `a[0]`. Absent: kind
                // -1, state 2. A declared member has state 0 while its object exists and
                // idles; the game's other states (1, 2) come from the object's animation
                // controller **[hypothesis]**.
                let (kind, state) = match self.rt.cast.iter().find(|&&(n, _)| n == a.int(0)) {
                    Some(&(_, category)) => (category, 0),
                    None => (-1, 2),
                };
                out.int(a, 1, kind);
                out.int(a, 2, state);
            }
            597 => out.int(a, 0, self.scene_time()),
            598 => out.int(a, 0, self.world.local_character() as i32),
            600 => out.int(a, 1, i32::from(self.world.flag(a.int(0)))),
            602 => out.int(a, 1, i32::from(self.world.track_flag(1, a.int(0)))),
            603 => {
                let index = a.int(0);
                let index = if index < 0x400 { index } else { index + 0x6fa };
                out.int(a, 1, i32::from(self.world.flag(index)));
            }
            804 => self.events.push(Event::Fog(a.int(0) != 0)),
            808 => self.events.push(Event::SetTimeOfDay(a.int(0))),
            8 => self.events.push(Event::Chaining(true)),
            9 => self.events.push(Event::Chaining(false)),
            840 => out.int(a, 0, self.frame.ticks as i32),
            // --- sound ---
            713 => self.sound(Sound::SelectBgm(a.int(a.len().saturating_sub(1)))),
            714 => self.sound(Sound::RestartBgm),
            715 => self.sound(Sound::BgmVolume {
                volume: a.int(0).clamp(0, 127),
                duration: a.int_or(1, 0),
            }),
            721 => self.sound(Sound::StopBgm {
                duration: a.int_or(0, 30),
            }),
            723 | 729 => self.sound(Sound::StopEffect {
                handle: a.int(0),
                duration: a.int_or(1, 0),
            }),
            726 | 730 => self.sound(Sound::LoadEffects {
                group: a.int(0),
                secondary: id == 730,
            }),
            727 | 731 => self.sound(Sound::RestoreEffects {
                secondary: id == 731,
            }),
            728 | 732 => {
                let handle = self.rt.next_handle;
                self.rt.next_handle = (handle + 1) % HANDLES;
                // The handle is written only when the script passes an address.
                out.int(a, 1, handle);
                self.sound(Sound::PlayEffect {
                    index: a.int(0),
                    handle,
                    secondary: id == 732,
                });
            }
            737 => self.sound(Sound::PlaySequence(a.int(0))),
            738 => self.sound(Sound::ResetBgm),
            // Established, but DS memory or hardware management with no counterpart: the
            // texture arenas (502, 503), resource flags (512), the allocator (587) and the
            // natives that return at once (706 to 709, 724).
            502 | 503 | 512 | 587 | 706..=709 | 724 => {}
            _ => return false,
        }
        true
    }

    fn sound(&mut self, sound: Sound) {
        self.events.push(Event::Sound(sound));
    }

    /// Native 566 (`ov_001::02160da8`): `mode` is the low nibble of the first argument. The
    /// argument shapes per mode are in `docs/re/stb-natives.md`.
    fn declare_cast(&mut self, a: &Args) {
        let mode = a.int(0) & 0xf;
        let (path, reference, n) = match mode {
            2 | 3 | 6 | 7 => (Some(text(a, 1)), None, 2),
            4 | 5 => (None, Some(a.int(1)), 2),
            _ => (None, None, 1),
        };
        let character = match mode {
            4 | 6 | 7 | 8 => {
                let id = -1 - self.rt.cast_count;
                self.rt.cast_count += 1;
                Some(id)
            }
            5 => reference,
            _ => None,
        };
        // The category native 596 reports (high nibble of the descriptor's first byte): 1 for
        // the local hero, 2 for another existing character, 0 where the game leaves it
        // unset. Modes 9 (reads `E+0x116`) and 10 (searches the party) are not modelled
        // **[unknown]**.
        let category = match mode {
            0 => 1,
            1 => 1 + i32::from(self.world.local_character() != 0),
            11..=13 => 2,
            _ => 0,
        };
        self.rt.cast.push((a.int_or(n, 0), category));
        self.events.push(Event::DeclareCast {
            mode,
            path,
            reference,
            attach: a.int_or(n, 0),
            option: (a.len() > n + 1).then(|| a.int(n + 1)),
            character,
        });
    }
}

/// The number ranges of `g_StbNatives` (`docs/re/stb.md`); `Stb_CallNative` ignores the
/// other numbers, and the table has holes inside the ranges that are not told apart here.
const TABLE: [std::ops::RangeInclusive<i32>; 8] = [
    0..=9,
    100..=122,
    200..=240,
    300..=328,
    400..=421,
    500..=603,
    700..=738,
    800..=845,
];

impl Natives for Ctx<'_> {
    fn call(&mut self, id: i32, args: &Args, out: &mut Outs) {
        if !self.dispatch(id, args, out) && TABLE.iter().any(|r| r.contains(&id)) {
            let calls = self.rt.unimplemented.entry(id).or_default();
            *calls += 1;
            if *calls == 1 {
                self.events.push(Event::Unimplemented { id });
            }
        }
    }
}
