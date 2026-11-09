//! The cutscene layer: what a running `.stb` script has done to the scene, as plain state the
//! application draws. It consumes the [`Event`]s the script asks for (`docs/re/stb.md`), runs
//! the commands the actors and the camera queue, and reports the completions the script polls
//! as [`Signal`]s. Deterministic, no I/O: the only inputs are the events, the fixed ticks and
//! what the application knows (the ground, which animations ended, the poses it starts from).
//!
//! * **Time.** [`Cutscene::tick`] is one tick of 1/60 s. Timed moves, turns, scale and alpha
//!   changes of the actors last `2 × duration` ticks (`docs/re/stb-natives.md`, § Time and
//!   units: the game compares a counter that advances by `numTicks_` = 2 per update to
//!   `2 × duration`). Camera and path durations take the same unit **[hypothesis]**; fades are
//!   counted in ticks.
//! * **Targets.** A script addresses an actor controller (0 to 23) that is bound to a binding
//!   slot (natives 202, 205), which designates a character or an object. Here the cast
//!   declared by 566 binds the declaration's `attach` index as slot *and* actor, which is what
//!   the data shows (`ev01130`: actor 1 is the NPC declared with `attach 1`) **[hypothesis]**.
//!   A [`Target`] is what the application draws: the hero, a map NPC, a model the script
//!   loaded, a cast member that has its own model, a map object.
//! * **Channels.** The game queues the commands of an actor on eight channels and the actor
//!   is busy until all are empty (native 204). The channel of each command is not read from
//!   the game: the commands are grouped as position, rotation, animation, scale, alpha and
//!   radius **[hypothesis]**; within a channel they run in order.
//!
//! No game function is ported whole here; the behaviours cite their origin in comments.

use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::script::event::{ActorCommand, Camera as CameraCommand, Event, Lock, Screen, Text};
use crate::script::natives::{ACTORS, SLOTS};
use crate::script::{Runtime, Signal};

mod actor;
mod camera;
mod light;
mod props;

use actor::{Actor, lane_of, start_animation};
use camera::Cam;
use props::Props;

pub use light::{ClockCommand, SceneLight, SceneTime};
pub use props::{Balloon, Caption, Door, PlacementChange, TexelOp};

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
/// Which physical screen is which is **[unknown]** (`event.rs`, [`Screen`]).
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

/// A camera whose eye and target follow the `eye` and `lookat` bones of the `source`
/// character, and whose `foot` bone drags the `target` character (native 552,
/// `ov_001::02160768`, `0204a170`).
#[derive(Debug, Clone, PartialEq)]
pub struct ThreeBoneCamera {
    pub source: i32,
    pub target: i32,
    pub eye: String,
    pub lookat: String,
    pub foot: String,
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
    /// Set while a native 552 camera is installed: the application resolves its three bones,
    /// replaces `eye` and `target`, and drags the target character to the foot bone.
    pub three_bone: Option<ThreeBoneCamera>,
}

/// Brightness fade target of one command.
fn engines(screen: Screen) -> (bool, bool) {
    match screen {
        Screen::Both => (true, true),
        Screen::Main => (true, false),
        Screen::Sub => (false, true),
    }
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
    /// 109/110: the DS display swap bit (`POWCNT1` bit 15). It persists across chained events
    /// and is not a per-scene effect; the application applies it to its screen layout.
    display_swap: bool,
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
            display_swap: false,
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
        let display_swap = self.display_swap;
        // The map's doors are objects of the zone, not of the event: an event that starts
        // after one opened must keep it open.
        let doors = std::mem::take(&mut self.props.doors);
        *self = Self::new();
        self.fades = fades;
        self.light = light;
        self.ignored = ignored;
        self.display_swap = display_swap;
        self.props.doors = doors;
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

    /// Moves a target the way a camera object does (native 552 drags its second character to
    /// the `foot` bone): position, and facing around Y in radians.
    pub fn place_target(&mut self, target: Target, position: Vec3, facing: f32) {
        let node = self.node_mut(target);
        node.pose.position = position;
        node.pose.rotation.y = facing;
        node.placed = true;
    }

    /// The doors the script opened (native 584), with their current angle.
    pub fn doors(&self) -> &[Door] {
        &self.props.doors
    }

    /// Opens a door the field touched (native 584, `Field_TouchDoorways`): the same state the
    /// scripts use, so the render and the collision read one angle. Returns whether it was
    /// added (an already open door is left to finish).
    pub fn open_door(&mut self, block: i32, placement: i32, sound: bool) -> bool {
        if self
            .props
            .doors
            .iter()
            .any(|door| door.block == block && door.placement == placement)
        {
            return false;
        }
        self.props.doors.push(Door {
            block,
            placement,
            angle: 0.0,
            target: Door::QUARTER_TURN,
            speed: Door::SPEED,
            sound,
        });
        true
    }

    /// One tick of the door angles only (a field door opening with no script).
    pub fn tick_doors(&mut self) {
        for door in &mut self.props.doors {
            door.tick();
        }
    }

    /// Forgets the map's doors (another map is loaded).
    pub fn clear_doors(&mut self) {
        self.props.doors.clear();
    }

    /// The caption fields of the current message (natives 409, 411, 413).
    pub fn caption(&self) -> Caption {
        self.props.caption
    }

    /// Alpha of the timed caption at the current tick (1 when native 409 was not used).
    pub fn caption_alpha(&self) -> f32 {
        self.props.caption.alpha(self.ticks)
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

    /// Whether the DS display engines are swapped (`POWCNT1` bit 15, natives 109/110): the
    /// application draws the field and the message window on the other screen.
    pub fn screen_swapped(&self) -> bool {
        self.display_swap
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
            three_bone: cam.three_bone.clone(),
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
    pub fn target_of_character(&self, character: i32) -> Target {
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
            Event::Message { text, flags } => {
                // 0204501c resets the caption fields: flags 0, 0x9b0 = 0, 0x19b2 = 1.
                self.props.caption = Caption::default();
                out.push(Output::Message {
                    text: text.clone(),
                    flags: *flags,
                });
            }
            Event::CloseMessage => {
                // 401 tears the window down; the next 400 starts from the reset fields.
                self.props.caption = Caption::default();
                out.push(Output::CloseMessage);
            }
            Event::HideMessageFrame => out.push(Output::HideMessageFrame),
            Event::CaptionTime { ticks } => {
                self.props.caption.time = Some(*ticks);
                self.props.caption.started = self.ticks;
            }
            Event::CaptionCentered => self.props.caption.centered = true,
            Event::CaptionSilent => self.props.caption.silent = true,
            Event::OpenDoor {
                block,
                placement,
                sound,
                ..
            } => {
                // 0215fb44: the doorway's own flags select the swing (bit 0x20 clear) and its
                // direction; the prologue's doorway has flags 0x40, so the placement turns by
                // +90°. Other maps' branches (sliding, reverse) are not modelled.
                let door = Door {
                    block: *block,
                    placement: *placement,
                    angle: 0.0,
                    target: Door::QUARTER_TURN,
                    speed: Door::SPEED,
                    sound: *sound,
                };
                match self
                    .props
                    .doors
                    .iter_mut()
                    .find(|d| d.block == door.block && d.placement == door.placement)
                {
                    Some(existing) => *existing = door,
                    None => self.props.doors.push(door),
                }
            }
            // Natives 200 (`0215b72c`) and 233 (`0215ca4c`) allocate and initialise a new
            // `Object3D`, then `SetScale(0x10a, 0x10a, 0x10a)` whatever the third argument,
            // which only picks the allocator (`021658b8`). Scripts resize them with 211.
            Event::LoadModel {
                path, character, ..
            }
            | Event::LoadMonster { path, character } => {
                self.models.insert(*character, path.clone());
                let node = self.node_mut(Target::Model(*character));
                node.model = Some(path.clone());
                node.packages.clear();
                // A new object: a character id that is loaded again starts from the origin,
                // unattached and visible (the slot of an earlier model leaves nothing behind).
                node.pose = Pose::default();
                node.pose.scale = Vec3::splat(CHARACTER_SCALE);
                node.attached = None;
                node.visible = true;
                node.animation = None;
                node.tint = None;
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
            Event::PlacementScale { group, id, scale } => {
                self.props.changes.push(PlacementChange::Scaled {
                    group: *group,
                    id: *id,
                    scale: *scale,
                });
            }
            Event::SlotObjectVisible { slot, visible } => {
                // `0215c730` only acts on a type 2 slot: a script sprite (`.spr`) or a map object.
                if let Some(target) = self.slot_target(*slot)
                    && (matches!(target, Target::Object(_))
                        || self
                            .nodes
                            .get(&target)
                            .and_then(|n| n.model.as_deref())
                            .is_some_and(|m| m.ends_with(".spr")))
                {
                    self.node_mut(target).visible = *visible;
                }
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
                // the event **[hypothesis]** (the game recomputes it from its own state).
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
            Event::SwapScreens(swapped) => self.display_swap = *swapped,
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
            | Event::Fog(_)
            | Event::ScreenCover { .. }
            | Event::ScreenUncover { .. } => {
                self.light.apply(event);
            }
            Event::AddSceneFlags(_) | Event::Chaining(_) | Event::SetQuestSignal(_) => {}
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
    /// actor of the same number **[hypothesis]**).
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
            // **[hypothesis]**.
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

    /// One tick of 1/60 s. `ground(x, z, y)` is the height of the floor under a point (for
    /// the moves on the ground). Returns the completions to report to the script.
    pub fn tick(&mut self, ground: &dyn Fn(f32, f32, f32) -> Option<f32>) -> Vec<Signal> {
        self.ticks += 1;
        self.fades.main.tick(1);
        self.fades.sub.tick(1);
        self.light.tick();
        for door in &mut self.props.doors {
            door.tick();
        }
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
    pub(super) fn actor_position(&self, index: usize) -> Vec3 {
        self.nodes
            .get(&self.target_of_actor(index))
            .map_or(Vec3::ZERO, |n| n.pose.position)
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
