//! The cutscene layer: what a running `.stb` script has done to the scene, as plain state the
//! application draws. It consumes the [`Event`]s the script asks for (`docs/re/stb.md`), runs
//! the commands the actors and the camera queue, and reports the completions the script polls
//! as [`Signal`]s. Deterministic, no I/O: the only inputs are the events, the fixed ticks and
//! what the application knows (the ground, which animations ended, the poses it starts from).
//!
//! * **Time.** [`Cutscene::tick`] is one tick of 1/60 s. Timed moves, turns, scale and alpha
//!   changes of the actors last `2 × duration` ticks (`docs/re/stb-natives.md`, § Temps et
//!   unités: the game compares a counter that advances by `numTicks_` = 2 per update to
//!   `2 × duration`). Camera and path durations take the same unit **[hypothèse]**; fades are
//!   counted in ticks.
//! * **Targets.** A script addresses an actor controller (0 to 23) that is bound to a binding
//!   slot (natives 202, 205), which designates a character or an object. Here the cast
//!   declared by 566 binds the declaration's `attach` index as slot *and* actor, which is what
//!   the data shows (`ev01130`: actor 1 is the NPC declared with `attach 1`) **[hypothèse]**.
//!   A [`Target`] is what the application draws: the hero, a map NPC, a model the script
//!   loaded, a cast member that has its own model, a map object.
//! * **Channels.** The game queues the commands of an actor on eight channels and the actor
//!   is busy until all are empty (native 204). The channel of each command is not read from
//!   the game: the commands are grouped as position, rotation, animation, scale, alpha and
//!   radius **[hypothèse]**; within a channel they run in order.
//!
//! No game function is ported whole here; the behaviours cite their origin in comments.

use std::collections::{BTreeMap, VecDeque};
use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::script::event::{ActorCommand, Camera as CameraCommand, Event, Lock, Screen, Text};
use crate::script::natives::{ACTORS, SLOTS};
use crate::script::{Runtime, Signal};

mod light;
mod props;

use props::Props;

pub use light::{ClockCommand, SceneLight, SceneTime};
pub use props::{Balloon, PlacementChange, TexelOp};

/// Ticks per script update: durations of timed commands count updates (`stb-natives.md`).
const TICKS_PER_UNIT: f32 = 2.0;
/// Alpha of an opaque object (5 bits, `Object3D` `+0x40`).
pub const OPAQUE: i32 = 31;
/// Scale of the character models (`NpcTable_Build`, `0x0203d038`; `ShallowCloneTo` constant of
/// native 228): `0x10a / 4096`.
pub const CHARACTER_SCALE: f32 = 266.0 / 4096.0;

/// What the application draws for a script's character or object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// The party leader (binding type 4, character 0; cast mode 0).
    Hero,
    /// A map NPC by its id (cast modes 4 and 5, binding of a positive character).
    Npc(i32),
    /// A model the script loaded (natives 200, 228, 233) under a negative character id.
    Model(i32),
    /// A cast member declared with a model of its own (566 modes 2, 3, 6, 7), by its `attach`
    /// index.
    Cast(i32),
    /// A map object (binding types 2 and 6).
    Object(i32),
    /// An actor commanded while bound to nothing.
    Detached(i32),
}

/// Position, rotation (radians; the middle component turns around Y), scale and alpha of a
/// target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,
    /// 0 to 31.
    pub alpha: i32,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
            alpha: OPAQUE,
        }
    }
}

/// The animation a script asked a target to play (natives 210, 224).
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub flags: i32,
    pub speed: f32,
    /// Counts the requests of the target: the application starts the motion when it changes.
    pub serial: u32,
    /// The motion reached its end at least once (reported by the application).
    pub finished: bool,
}

impl Animation {
    /// Flag `0x01` of native 210: stops at the end instead of looping (`02036e44`).
    pub fn plays_once(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// A character or object of the scene and what the script did to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub pose: Pose,
    /// `false` until the application [seeds](Cutscene::seed) a pose or a command sets one.
    pub placed: bool,
    pub visible: bool,
    /// The model of a [`Target::Model`] or [`Target::Cast`] while it is loaded.
    pub model: Option<String>,
    /// Archives (natives 229, 567) whose motions the model can play, with their tag.
    pub packages: Vec<(String, i32)>,
    pub animation: Option<Animation>,
    /// The target a bone of this one carries it on (natives 235, 236).
    pub attached: Option<(Target, String)>,
    /// The model this one was cloned from (native 228).
    pub cloned_from: Option<Target>,
    /// The palette operation the script did on the model (native 238).
    pub tint: Option<TexelOp>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            pose: Pose::default(),
            placed: false,
            visible: true,
            model: None,
            packages: Vec::new(),
            animation: None,
            attached: None,
            cloned_from: None,
            tint: None,
        }
    }
}

/// One brightness engine (`Brightness.cpp`): level −16 black to +16 white.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Engine {
    pub level: f32,
    from: f32,
    to: f32,
    total: u32,
    left: u32,
    /// A locked engine ignores plain fades (`+0x24`, `+0x25`).
    pub locked: bool,
}

impl Engine {
    const fn new() -> Self {
        Self {
            level: 0.0,
            from: 0.0,
            to: 0.0,
            total: 0,
            left: 0,
            locked: false,
        }
    }

    fn start(&mut self, level: i32, ticks: i32) {
        self.from = self.level;
        self.to = level as f32;
        if ticks <= 0 {
            self.level = self.to;
            self.total = 0;
            self.left = 0;
        } else {
            self.total = ticks as u32;
            self.left = ticks as u32;
        }
    }

    fn tick(&mut self, ticks: u32) {
        if self.left == 0 {
            return;
        }
        self.left = self.left.saturating_sub(ticks);
        let done = (self.total - self.left) as f32 / self.total as f32;
        self.level = self.from + (self.to - self.from) * done;
    }
}

/// The screen brightness: the engine drawing the 3D scene (`Main`) and the other one.
/// Which physical screen is which is **[inconnu]** (`event.rs`, [`Screen`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fades {
    pub main: Engine,
    pub sub: Engine,
}

impl Fades {
    const fn new() -> Self {
        Self {
            main: Engine::new(),
            sub: Engine::new(),
        }
    }
}

/// A camera whose eye and target follow two bones of the model bound to `slot` (native 572).
#[derive(Debug, Clone, PartialEq)]
pub struct BoneCamera {
    pub slot: i32,
    pub eye: String,
    pub target: String,
}

/// What the camera looks like now.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraView {
    pub eye: Vec3,
    pub target: Vec3,
    /// Half of the vertical field of view, degrees (15 by default, `0202df78`).
    pub half_fov: f32,
    /// Degrees (native 327).
    pub roll: f32,
    /// Set while the camera is driven by bones: the application resolves them and replaces
    /// `eye` and `target`.
    pub bones: Option<BoneCamera>,
}

/// Brightness fade target of one command.
fn engines(screen: Screen) -> (bool, bool) {
    match screen {
        Screen::Both => (true, true),
        Screen::Main => (true, false),
        Screen::Sub => (false, true),
    }
}

/// `eye = target + Ry(θ)·(0, h, √(r² − h²))` (`0202e5e8`, `docs/re/stb-natives.md`).
fn orbit_eye(target: Vec3, azimuth: f32, height: f32, distance: f32) -> Vec3 {
    let horizontal = (distance * distance - height * height).max(0.0).sqrt();
    target
        + Vec3::new(
            horizontal * azimuth.sin(),
            height,
            horizontal * azimuth.cos(),
        )
}

fn wrap_angle(a: f32) -> f32 {
    let a = a.rem_euclid(TAU);
    if a > PI { a - TAU } else { a }
}

/// Angle reached `t` (0 to 1) of the way from `from` to `to`: `direction` `None` the short
/// way round (`ov_001::021594cc` for `TurnTowards`, threshold `0x3244` = π), `Some(0)` with an
/// increasing angle, any other with a decreasing one (native 209).
fn lerp_angle(from: f32, to: f32, t: f32, direction: Option<i32>) -> f32 {
    let delta = match direction {
        None => wrap_angle(to - from),
        Some(0) => (to - from).rem_euclid(TAU),
        Some(_) => -(from - to).rem_euclid(TAU),
    };
    from + delta * t
}

/// A timed value change: the counter advances by the ticks of each call.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Timer {
    total: f32,
    elapsed: f32,
}

impl Timer {
    /// A timer of `duration` units (`None` when the command is instantaneous).
    fn units(duration: i32) -> Option<Self> {
        (duration > 0).then_some(Self {
            total: duration as f32 * TICKS_PER_UNIT,
            elapsed: 0.0,
        })
    }

    fn advance(&mut self, ticks: f32) -> (f32, bool) {
        self.elapsed = (self.elapsed + ticks).min(self.total);
        (self.elapsed / self.total, self.elapsed >= self.total)
    }
}

/// A command in progress.
#[derive(Debug, Clone)]
enum Running {
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
struct Channel {
    queue: VecDeque<ActorCommand>,
    running: Option<Running>,
}

impl Channel {
    fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.running.is_none()
    }
}

/// Which channel runs a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    Position,
    Rotation,
    Animation,
    Scale,
    Alpha,
    Radius,
}

const LANES: [Lane; 6] = [
    Lane::Position,
    Lane::Rotation,
    Lane::Animation,
    Lane::Scale,
    Lane::Alpha,
    Lane::Radius,
];

fn lane_of(command: &ActorCommand) -> Option<Lane> {
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
struct Actor {
    slot: Option<usize>,
    channels: [Channel; 6],
    /// Path points and speed (214 to 217), run by `PathRun`.
    path: Vec<Vec3>,
    path_speed: i32,
    /// Commands were queued since the last report of idleness.
    pending: bool,
    /// Continuous rotation of a negative-duration 209, radians per update.
    spin: Option<Vec3>,
}

impl Actor {
    fn lane(&mut self, lane: Lane) -> &mut Channel {
        &mut self.channels[LANES.iter().position(|l| *l == lane).unwrap_or(0)]
    }

    fn is_idle(&self) -> bool {
        self.channels.iter().all(Channel::is_idle)
    }
}

/// A camera command in progress.
#[derive(Debug, Clone)]
enum CamRunning {
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
struct CamLane {
    queue: VecDeque<CameraCommand>,
    running: Option<CamRunning>,
}

#[derive(Debug, Clone)]
struct Orbit {
    azimuth: f32,
    height: f32,
    distance: f32,
}

#[derive(Debug, Clone)]
struct Cam {
    eye: Vec3,
    target: Vec3,
    orbit: Option<Orbit>,
    half_fov: f32,
    roll: f32,
    follow: Option<(i32, Vec3)>,
    bones: Option<BoneCamera>,
    /// Queues: cartesian moves, orbital moves, field of view, shake.
    lanes: [CamLane; 4],
    shake: Vec3,
    shake_target_only: bool,
    updates: u32,
    pending: bool,
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
            lanes: Default::default(),
            shake: Vec3::ZERO,
            shake_target_only: false,
            updates: 0,
            pending: false,
        }
    }
}

const CART: usize = 0;
const ORBITAL: usize = 1;
const FOV: usize = 2;
const SHAKE: usize = 3;

/// What the application does besides the model [`Cutscene`] keeps.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Native 400: opens the message window on `text`.
    Message { text: Text, flags: i32 },
    /// Native 401.
    CloseMessage,
    /// Native 410.
    HideMessageFrame,
}

/// The cutscene state. Built once per event with [`Cutscene::new`]; the brightness is kept
/// by [`Cutscene::restart`] between chained events.
#[derive(Debug, Clone)]
pub struct Cutscene {
    slots: [Option<Target>; SLOTS],
    actors: Vec<Actor>,
    nodes: BTreeMap<Target, Node>,
    /// Models loaded by natives 200 and 233, by character id.
    models: BTreeMap<i32, String>,
    fades: Fades,
    light: SceneLight,
    props: Props,
    /// Eye and target of the field camera relative to the hero (native 328).
    field_camera: (Vec3, Vec3),
    cam: Cam,
    saved_cam: Option<Box<Cam>>,
    serial: u32,
    ticks: u64,
    /// Events the model does not execute, by kind, with their counts.
    ignored: BTreeMap<String, u32>,
}

impl Default for Cutscene {
    fn default() -> Self {
        Self::new()
    }
}

impl Cutscene {
    pub fn new() -> Self {
        Self {
            slots: [None; SLOTS],
            actors: vec![Actor::default(); ACTORS],
            nodes: BTreeMap::new(),
            models: BTreeMap::new(),
            fades: Fades::new(),
            light: SceneLight::default(),
            props: Props::default(),
            cam: Cam::default(),
            saved_cam: None,
            serial: 0,
            ticks: 0,
            ignored: BTreeMap::new(),
            field_camera: (Vec3::new(0.0, 5.0, 20.0), Vec3::ZERO),
        }
    }

    /// A new event starts: the scene is empty again, the brightness and the lighting stay (a
    /// chained event begins where the previous one left the screen).
    pub fn restart(&mut self) {
        let fades = self.fades;
        let light = std::mem::take(&mut self.light);
        let ignored = std::mem::take(&mut self.ignored);
        *self = Self::new();
        self.fades = fades;
        self.light = light;
        self.ignored = ignored;
    }

    /// Brightness back to normal, unlocked, and the lighting back to the field's: the event
    /// task ended and the game goes on.
    pub fn clear_fades(&mut self) {
        self.fades = Fades::new();
        self.light = SceneLight::default();
        self.props.balloon = None;
    }

    /// The lighting the script set up.
    pub fn light(&self) -> &SceneLight {
        &self.light
    }

    /// The requests on the game's clock (natives 579, 808) made since the last call.
    pub fn take_clock_commands(&mut self) -> Vec<ClockCommand> {
        self.light.take_clock_commands()
    }

    /// The changes of map placements (natives 574, 575) made since the last call.
    pub fn take_placement_changes(&mut self) -> Vec<PlacementChange> {
        std::mem::take(&mut self.props.changes)
    }

    /// The balloon over a character, if the script put one up (natives 541, 561, 542).
    pub fn balloon(&self) -> Option<&Balloon> {
        self.props.balloon.as_ref()
    }

    /// Starts the camera where the field's is (until the script moves it).
    pub fn seed_camera(&mut self, eye: Vec3, target: Vec3, half_fov: f32) {
        self.cam.eye = eye;
        self.cam.target = target;
        self.cam.half_fov = half_fov;
        // The field camera follows the hero: keep its offsets from the hero as seeded.
        let hero = self
            .nodes
            .get(&Target::Hero)
            .map_or(Vec3::ZERO, |n| n.pose.position);
        self.field_camera = (eye - hero, target - hero);
    }

    /// Declares the pose a target starts from (the hero, a map NPC) before its first command.
    pub fn seed(&mut self, target: Target, pose: Pose) {
        let node = self.nodes.entry(target).or_default();
        node.pose = pose;
        node.placed = true;
    }

    pub fn node(&self, target: Target) -> Option<&Node> {
        self.nodes.get(&target)
    }

    /// Every target the script touched, with its state.
    pub fn nodes(&self) -> impl Iterator<Item = (Target, &Node)> {
        self.nodes.iter().map(|(t, n)| (*t, n))
    }

    /// The target of a binding slot.
    pub fn slot_target(&self, slot: i32) -> Option<Target> {
        usize::try_from(slot)
            .ok()
            .and_then(|s| self.slots.get(s))
            .copied()
            .flatten()
    }

    /// The target an actor controller drives.
    pub fn actor_target(&self, actor: usize) -> Option<Target> {
        let slot = self.actors.get(actor)?.slot?;
        self.slots[slot]
    }

    pub fn fades(&self) -> &Fades {
        &self.fades
    }

    /// The model loaded under a negative character id.
    pub fn model_of(&self, character: i32) -> Option<&str> {
        self.models.get(&character).map(String::as_str)
    }

    /// Events received that the model does not execute, by kind.
    pub fn ignored(&self) -> &BTreeMap<String, u32> {
        &self.ignored
    }

    fn ignore(&mut self, kind: impl Into<String>) {
        *self.ignored.entry(kind.into()).or_default() += 1;
    }

    /// The application reports that the animation `serial` of `target` reached its end.
    pub fn animation_finished(&mut self, target: Target, serial: u32) {
        if let Some(animation) = self
            .nodes
            .get_mut(&target)
            .and_then(|n| n.animation.as_mut())
            .filter(|a| a.serial == serial)
        {
            animation.finished = true;
        }
    }

    /// Where the camera looks now.
    pub fn camera(&self) -> CameraView {
        let cam = &self.cam;
        let (mut eye, mut target) = (cam.eye, cam.target);
        if cam.shake != Vec3::ZERO {
            target += cam.shake;
            if !cam.shake_target_only {
                eye += cam.shake;
            }
        }
        CameraView {
            eye,
            target,
            half_fov: cam.half_fov,
            roll: cam.roll,
            bones: cam.bones.clone(),
        }
    }

    fn node_mut(&mut self, target: Target) -> &mut Node {
        self.nodes.entry(target).or_default()
    }

    fn target_of_actor(&self, actor: usize) -> Target {
        self.actor_target(actor)
            .unwrap_or(Target::Detached(actor as i32))
    }

    /// The target a character id designates for the visibility natives (201): negative ids
    /// are the models the script loaded, positive ones map NPCs, 0 the hero.
    fn target_of_character(&self, character: i32) -> Target {
        match character {
            0 => Target::Hero,
            c if c < 0 => Target::Model(c),
            c => Target::Npc(c),
        }
    }

    /// One event of the script. Returns what the application must do besides drawing the
    /// state (messages).
    pub fn apply(&mut self, event: &Event) -> Vec<Output> {
        let mut out = Vec::new();
        match event {
            Event::Fade {
                screen,
                level,
                ticks,
                lock,
            } => self.fade(*screen, *level, *ticks, *lock),
            Event::Message { text, flags } => out.push(Output::Message {
                text: text.clone(),
                flags: *flags,
            }),
            Event::CloseMessage => out.push(Output::CloseMessage),
            Event::HideMessageFrame => out.push(Output::HideMessageFrame),
            Event::LoadModel {
                path,
                character,
                selector,
            } => {
                self.models.insert(*character, path.clone());
                let node = self.node_mut(Target::Model(*character));
                node.model = Some(path.clone());
                node.packages.clear();
                // A new object: a character id that is loaded again starts from the origin,
                // unattached and visible (the slot of an earlier model leaves nothing behind).
                node.pose = Pose::default();
                node.attached = None;
                node.visible = true;
                node.animation = None;
                node.tint = None;
                // Selector 0 models are the size of the map's characters; those the scripts
                // load with selector 1 are sized by the script (`SetScale`) **[hypothèse]**:
                // it fits the camera rigs and the attached models of the prologue.
                node.pose.scale = if *selector == 0 {
                    Vec3::splat(CHARACTER_SCALE)
                } else {
                    Vec3::ONE
                };
            }
            Event::LoadMonster { path, character } => {
                self.models.insert(*character, path.clone());
                let node = self.node_mut(Target::Model(*character));
                node.model = Some(path.clone());
                node.packages.clear();
            }
            Event::ReleaseModel { character } => {
                self.models.remove(character);
                if let Some(node) = self.nodes.get_mut(&Target::Model(*character)) {
                    node.model = None;
                    node.animation = None;
                    node.tint = None;
                }
            }
            Event::CloneModel {
                source,
                destination,
            } => {
                if let Some(path) = self.models.get(source).cloned() {
                    self.models.insert(*destination, path.clone());
                    let packages = self
                        .nodes
                        .get(&Target::Model(*source))
                        .map(|n| n.packages.clone())
                        .unwrap_or_default();
                    let node = self.node_mut(Target::Model(*destination));
                    node.model = Some(path);
                    node.packages = packages;
                    node.cloned_from = Some(Target::Model(*source));
                    // `ShallowCloneTo` then `SetScale` with the character constant (`0215ba64`).
                    node.pose.scale = Vec3::splat(CHARACTER_SCALE);
                }
            }
            Event::LoadModelArchive {
                path,
                character,
                tag,
            } => {
                let target = self.target_of_character(*character);
                self.node_mut(target).packages.push((path.clone(), *tag));
                // A clone shares the animations of its source (`ShallowCloneTo`): the
                // monsters of the opening play motions added to the original only.
                for node in self.nodes.values_mut() {
                    if node.cloned_from == Some(target) {
                        node.packages.push((path.clone(), *tag));
                    }
                }
            }
            Event::RemoveAnimationPackage { character, tag } => {
                let target = self.target_of_character(*character);
                self.node_mut(target).packages.retain(|(_, t)| t != tag);
            }
            Event::CharacterVisible { character, visible } => {
                let target = self.target_of_character(*character);
                self.node_mut(target).visible = *visible;
            }
            Event::BindSlot {
                slot,
                character,
                kind,
            } => self.bind_slot(*slot, *character, *kind),
            Event::SlotVisible { slot, visible } => {
                if let Some(target) = self.slot_target(*slot) {
                    self.node_mut(target).visible = *visible;
                }
            }
            Event::SlotMotion { slot, moving } => {
                // `02036e44(object, "run" | "stand", 0)`, at once (545, 546 queue nothing).
                let name = if *moving { "run" } else { "stand" };
                match self.slot_target(*slot) {
                    Some(
                        target @ (Target::Hero
                        | Target::Npc(_)
                        | Target::Model(_)
                        | Target::Cast(_)
                        | Target::Object(_)),
                    ) => {
                        let serial = &mut self.serial;
                        start_animation(
                            self.nodes.entry(target).or_default(),
                            serial,
                            name,
                            0,
                            1.0,
                        );
                    }
                    _ => self.ignore("slot motion of a map object"),
                }
            }
            Event::Placement { group, id, hidden } => {
                self.props.changes.push(PlacementChange::Hidden {
                    group: *group,
                    id: *id,
                    hidden: *hidden,
                });
            }
            Event::PlacementMove {
                group,
                id,
                position,
            } => {
                self.props.changes.push(PlacementChange::Moved {
                    group: *group,
                    id: *id,
                    position: *position,
                });
            }
            Event::Balloon {
                actor,
                kind,
                offset,
            } => self.props.balloon = Some(Balloon::new(*actor, *kind, *offset)),
            Event::BalloonNudge { offset } => {
                if let Some(balloon) = &mut self.props.balloon {
                    balloon.offset[0] += offset[0];
                    balloon.offset[1] += offset[1];
                }
            }
            Event::BalloonEnd => self.props.balloon = None,
            Event::Tint { slot, rgb, mode } => match self.slot_target(*slot) {
                Some(target) => self.node_mut(target).tint = TexelOp::new(*mode, *rgb),
                None => self.ignore("palette operation on an empty slot"),
            },
            Event::AttachBone {
                child,
                parent,
                bone,
            } => {
                // `02037868`: without a name the bone index stored by the previous attachment
                // is reused.
                let attached = match (self.slot_target(*child), self.slot_target(*parent)) {
                    (Some(child), Some(parent)) => {
                        let node = self.node_mut(child);
                        let bone = bone
                            .clone()
                            .or_else(|| node.attached.as_ref().map(|a| a.1.clone()));
                        node.attached = Some((parent, bone.unwrap_or_default()));
                        true
                    }
                    _ => false,
                };
                if !attached {
                    self.ignore("bone attachments");
                }
            }
            Event::DetachBone { slot } => {
                if let Some(target) = self.slot_target(*slot) {
                    self.node_mut(target).attached = None;
                }
            }
            Event::Actor { actor, command } => self.actor_command(*actor, command),
            Event::CameraToField { duration } => {
                // `0215e14c`: the standing of the field camera (`020a2b48`) as a move of both
                // eye and target. It follows the hero, which is where it was at the start of
                // the event **[hypothèse]** (the game recomputes it from its own state).
                let hero = self
                    .nodes
                    .get(&Target::Hero)
                    .map_or(Vec3::ZERO, |n| n.pose.position);
                let (eye, target) = self.field_camera;
                self.camera_command(&CameraCommand::MoveBoth {
                    position: (hero + eye).to_array(),
                    target: (hero + target).to_array(),
                    duration: *duration,
                    snap: false,
                });
            }
            Event::Camera(command) => self.camera_command(command),
            Event::DeclareCast {
                mode,
                path,
                reference,
                attach,
                ..
            } => self.declare(*mode, path.as_deref(), *reference, *attach),
            Event::DeclareCharacterArchive { path, attach } => {
                if let Some(target) = self.slot_target(*attach) {
                    self.node_mut(target).packages.push((path.clone(), 0));
                }
            }
            Event::Started { .. } | Event::Phase { .. } | Event::Ended { .. } => {}
            Event::Unimplemented { id } => self.ignore(format!("native {id}")),
            Event::Sound(_) => {}
            Event::SwapScreens(_) => self.ignore("screen swap"),
            Event::Preload(_) | Event::ReleasePreloaded => {}
            Event::CreateSprite { name, handle } => {
                // `0215f3fc`: a new object under the handle; the slots of type 2 and 6 designate
                // it (`bind_slot`). A sheet name without suffix gets `.spr`.
                let node = self.node_mut(Target::Object(*handle));
                *node = Node::default();
                node.model = Some(if name.ends_with(".spr") {
                    name.clone()
                } else {
                    format!("{name}.spr")
                });
            }
            Event::DestroySprite { handle } => {
                // `0215f740`: the object is freed.
                if let Some(node) = self.nodes.get_mut(&Target::Object(*handle)) {
                    node.model = None;
                    node.animation = None;
                }
            }
            Event::LightingOverride(_)
            | Event::LightIntensity { .. }
            | Event::DayTimerStopped(_)
            | Event::SceneTimeOfDay(_)
            | Event::SyncSceneTimeOfDay
            | Event::SetTimeOfDay(_)
            | Event::Fog(_) => {
                self.light.apply(event);
            }
            Event::AddSceneFlags(_) | Event::Chaining(_) => {}
            Event::LoadFailed { .. } | Event::Fault(_) => {}
        }
        out
    }

    fn fade(&mut self, screen: Screen, level: i32, ticks: i32, lock: Lock) {
        let (main, sub) = engines(screen);
        for (selected, engine) in [(main, &mut self.fades.main), (sub, &mut self.fades.sub)] {
            if !selected {
                continue;
            }
            match lock {
                Lock::Keep if engine.locked => continue,
                Lock::Unlock => engine.locked = false,
                _ => {}
            }
            engine.start(level, ticks);
            if lock == Lock::Lock {
                engine.locked = true;
            }
        }
    }

    /// Native 566: the cast member `attach` (`Event_BuildScene` binds it to the slot and the
    /// actor of the same number **[hypothèse]**).
    fn declare(&mut self, mode: i32, path: Option<&str>, reference: Option<i32>, attach: i32) {
        let Some(slot) = usize::try_from(attach).ok().filter(|s| *s < SLOTS) else {
            return;
        };
        let target = match (mode, reference) {
            (0, _) => Target::Hero,
            (4 | 5, Some(reference)) => Target::Npc(reference),
            (_, _) if path.is_some() => Target::Cast(attach),
            _ => {
                self.ignore(format!("cast mode {mode}"));
                return;
            }
        };
        self.slots[slot] = Some(target);
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.slot = Some(slot);
        }
        let node = self.node_mut(target);
        if let Target::Cast(_) = target {
            node.model = path.map(str::to_owned);
            // Characters are drawn at the size of the map's; effects are sized by the script
            // **[hypothèse]**.
            node.pose.scale = if path.is_some_and(|p| p.starts_with("effect/")) {
                Vec3::ONE
            } else {
                Vec3::splat(CHARACTER_SCALE)
            };
        }
    }

    /// Native 202.
    fn bind_slot(&mut self, slot: i32, character: i32, kind: i32) {
        let Some(slot) = usize::try_from(slot).ok().filter(|s| *s < SLOTS) else {
            return;
        };
        let target = match kind {
            2 | 6 => Target::Object(character),
            _ if character == 0 => Target::Hero,
            _ if character < 0 => Target::Model(character),
            _ => Target::Npc(character),
        };
        self.slots[slot] = Some(target);
        self.node_mut(target);
    }

    fn actor_command(&mut self, actor: i32, command: &ActorCommand) {
        let Some(index) = usize::try_from(actor).ok().filter(|a| *a < ACTORS) else {
            return;
        };
        match command {
            // The natives raise the busy flag for these too: the report of idleness follows.
            ActorCommand::Reset => {
                self.actors[index] = Actor {
                    pending: true,
                    ..Actor::default()
                };
            }
            ActorCommand::Bind { slot } => {
                let slot = usize::try_from(*slot).ok().filter(|s| *s < SLOTS);
                self.actors[index].slot = slot;
                self.actors[index].pending = true;
            }
            _ => {
                let Some(lane) = lane_of(command) else { return };
                let actor = &mut self.actors[index];
                actor.pending = true;
                actor.lane(lane).queue.push_back(command.clone());
            }
        }
    }

    fn camera_command(&mut self, command: &CameraCommand) {
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
                self.cam.bones = Some(BoneCamera {
                    slot: *slot,
                    eye: eye.clone(),
                    target: target.clone(),
                });
                return;
            }
        };
        cam.lanes[lane].queue.push_back(command.clone());
    }

    /// One tick of 1/60 s. `ground(x, z, y)` is the height of the floor under a point (for
    /// the moves on the ground). Returns the completions to report to the script.
    pub fn tick(&mut self, ground: &dyn Fn(f32, f32, f32) -> Option<f32>) -> Vec<Signal> {
        self.ticks += 1;
        self.fades.main.tick(1);
        self.fades.sub.tick(1);
        self.light.tick();
        let mut signals = Vec::new();
        for index in 0..self.actors.len() {
            self.tick_actor(index, ground);
            let actor = &mut self.actors[index];
            if actor.pending && actor.is_idle() {
                actor.pending = false;
                signals.push(Signal::ActorIdle(index as i32));
            }
        }
        self.tick_camera();
        if self.cam.pending
            && self
                .cam
                .lanes
                .iter()
                .all(|l| l.queue.is_empty() && l.running.is_none())
        {
            self.cam.pending = false;
            signals.push(Signal::CameraIdle);
        }
        signals
    }

    /// Position of the object an actor drives.
    fn actor_position(&self, index: usize) -> Vec3 {
        self.nodes
            .get(&self.target_of_actor(index))
            .map_or(Vec3::ZERO, |n| n.pose.position)
    }

    fn tick_actor(&mut self, index: usize, ground: &dyn Fn(f32, f32, f32) -> Option<f32>) {
        let target = self.target_of_actor(index);
        if let Some(spin) = self.actors[index].spin {
            // 209 with a negative duration adds the angles at each update.
            let node = self.node_mut(target);
            node.pose.rotation += spin / TICKS_PER_UNIT;
        }
        for lane in LANES {
            // The first command of a lane that completes at once lets the next one start in
            // the same tick, as the game drains its queue.
            for _ in 0..64 {
                if self.actors[index].channels[LANES.iter().position(|l| *l == lane).unwrap_or(0)]
                    .is_idle()
                {
                    break;
                }
                let animation_done = self
                    .nodes
                    .get(&target)
                    .and_then(|n| n.animation.as_ref())
                    .is_none_or(|a| a.finished);
                let positions: Vec<Vec3> = (0..ACTORS).map(|a| self.actor_position(a)).collect();
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

    fn tick_camera(&mut self) {
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

/// Advances one lane of an actor by one tick. Returns whether another command may start in
/// the same tick (the running one completed, or there was none to start).
#[allow(clippy::too_many_arguments)]
fn step_lane(
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
                // The divisor of the segment lengths (`021579ac`); its unit is **[inconnu]**.
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

fn start_animation(node: &mut Node, serial: &mut u32, name: &str, flags: i32, speed: f32) {
    *serial += 1;
    node.animation = Some(Animation {
        name: name.to_owned(),
        flags,
        speed,
        serial: *serial,
        finished: false,
    });
}

/// Catmull-Rom point of a path at `t` in `0..=1` (the game's spline type is **[hypothèse]**,
/// `docs/re/stb-natives.md`, native 217).
fn spline(points: &[Vec3], t: f32) -> (Vec3, Vec3) {
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
fn advance(
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

/// Advances one camera lane by one tick; returns whether the next command may start now.
fn step_camera_lane(cam: &mut Cam, lane: usize) -> bool {
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
        | C::BoneCamera { .. } => {}
    }
    true
}

fn advance_camera(running: CamRunning, cam: &mut Cam) -> Option<CamRunning> {
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

impl Cutscene {
    /// Copies what the application knows into the script's polling model: which animation of
    /// each bound slot still plays (native 234), and where each actor is (543, 544).
    pub fn sync_runtime(&self, runtime: &mut Runtime) {
        for (slot, target) in self.slots.iter().enumerate() {
            let Some(node) = target.and_then(|t| self.nodes.get(&t)) else {
                continue;
            };
            runtime.animating[slot] = node.animation.as_ref().is_some_and(|a| !a.finished);
        }
        for (index, actor) in self.actors.iter().enumerate() {
            if let Some(node) = actor
                .slot
                .and_then(|s| self.slots[s])
                .and_then(|t| self.nodes.get(&t))
            {
                runtime.pose[index] =
                    (node.pose.position.to_array(), node.pose.rotation.to_array());
            }
        }
    }
}

#[cfg(test)]
mod tests;
