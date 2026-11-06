//! Script events in the application: the event task (`dq9_core::script::Director`) runs the
//! `.stb` script of an event, the cutscene layer (`dq9_core::cutscene`) keeps what it did to
//! the actors, the camera and the screen brightness, and this module draws it: actor
//! transforms and motions go to the render instances, the camera to the view, messages to
//! the message window, fades to an overlay (`docs/app.md`, § Événements).
//!
//! One fixed step of 1/60 s is one tick. The script runs every second tick (an update of the
//! game is 2 ticks, `docs/re/stb-natives.md`, § Temps et unités); the cutscene advances at
//! each tick and reports what ended as [`Signal`]s.
//!
//! Models are not loaded while the script runs: [`EventPlayer::prepare`] first plays the
//! script headless (every wait completes at once) to learn which `.chr` archives each
//! character needs, adds them to the scene, and the application uploads the scene once.

use std::cell::OnceCell;
use std::collections::{BTreeMap, HashMap};

use dq9_core::cutscene::{Cutscene, Output, Pose, Target};
use dq9_core::field::{angle_of_yaw, yaw_of_angle};
use dq9_core::script::{Director, Event, Files, Frame, Signal, Sound, Text};
use dq9_core::trigger::{Request, State};
use dq9_core::{CollisionWorld, Hero};
use dq9_render::{AnimatedScene, Camera};
use glam::{EulerRot, Mat4, Vec3};
use nds_rom::Rom;

use crate::app::status;
use crate::talk::Talk;

/// Language of the event texts.
const LANGUAGE: &str = "en";
/// Updates after which the script of a scan is declared stuck.
const MAX_UPDATES: u32 = 40_000;
/// Scale of the character models the map cast uses (`NpcTable_Build`, `0x0203d038`).
const NPC_SCALE: f32 = 266.0 / 4096.0;

/// The ROM's files, parsed when the first one is asked for (a step that starts no event never
/// reads any).
struct LazyRom<'a> {
    bytes: &'a [u8],
    rom: OnceCell<Option<Rom<'a>>>,
}

impl<'a> LazyRom<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            rom: OnceCell::new(),
        }
    }

    fn rom(&self) -> Option<&Rom<'a>> {
        self.rom
            .get_or_init(|| Rom::parse(self.bytes).ok())
            .as_ref()
    }
}

impl Files for LazyRom<'_> {
    fn file(&self, path: &str) -> Option<&[u8]> {
        self.rom()?.file_by_path(path).ok()
    }
}

/// The model one character of the script needs, found by a headless run.
#[derive(Debug, Clone, PartialEq)]
struct Plan {
    target: Target,
    path: String,
    packages: Vec<String>,
}

/// What a headless run of an event found out.
#[derive(Debug, Default)]
struct Scan {
    models: Vec<Plan>,
    /// Motion packages of the hero, in the order they are added.
    hero_packages: Vec<String>,
    /// Updates the script took with every wait completed at once.
    updates: u32,
    /// The script did not load or stopped on an error.
    failure: Option<String>,
}

/// Plays `event` headless up to its end, noting the models and packages each target gets.
fn scan(rom: &[u8], event: u16, world: &State) -> Scan {
    let files = LazyRom::new(rom);
    let mut director = Director::new(LANGUAGE);
    director.dispatch(vec![Request::Event(event)]);
    let mut cutscene = Cutscene::new();
    let mut out = Scan::default();
    'run: while out.updates < MAX_UPDATES {
        let events = director.step(&files, Frame::default(), world);
        out.updates += 1;
        for e in &events {
            cutscene.apply(e);
            if let Some(signal) = e.instant_signal() {
                director.signal(signal);
            }
            match e {
                Event::Ended { .. } => {
                    note(&cutscene, &mut out);
                    break 'run;
                }
                Event::LoadFailed { reason, .. } => {
                    out.failure = Some(reason.clone());
                    break 'run;
                }
                Event::Fault(fault) => {
                    out.failure = Some(fault.to_string());
                    break 'run;
                }
                _ => {}
            }
        }
        note(&cutscene, &mut out);
        if !director.is_active() {
            break;
        }
    }
    out
}

fn note(cutscene: &Cutscene, out: &mut Scan) {
    for (target, node) in cutscene.nodes() {
        match target {
            Target::Hero => {
                for (path, _) in &node.packages {
                    if !out.hero_packages.contains(path) {
                        out.hero_packages.push(path.clone());
                    }
                }
            }
            Target::Model(_) | Target::Cast(_) => {
                let Some(path) = &node.model else { continue };
                let plan = match out
                    .models
                    .iter_mut()
                    .find(|p| p.target == target && p.path == *path)
                {
                    Some(plan) => plan,
                    None => {
                        out.models.push(Plan {
                            target,
                            path: path.clone(),
                            packages: Vec::new(),
                        });
                        out.models.last_mut().expect("just pushed")
                    }
                };
                for (package, _) in &node.packages {
                    if !plan.packages.contains(package) {
                        plan.packages.push(package.clone());
                    }
                }
            }
            _ => {}
        }
    }
}

/// What a step of the player asks the application to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// An event ended: the application fires trigger event 11 (`Field::event_ended`).
    Ended(u16),
    /// The last event ended: the scene is back to the field.
    Finished,
    /// A sound native (712 to 738): the application's sound executes it (`audio::Audio`).
    Sound(Sound),
}

/// What a step works on.
pub struct Stage<'a> {
    pub rom: &'a [u8],
    pub world: &'a State,
    pub scene: &'a mut AnimatedScene,
    pub hero: &'a mut Hero,
    pub collision: &'a CollisionWorld,
    pub camera: &'a mut Camera,
    pub talk: &'a mut Talk,
}

/// Counters of what an event did or could not do, for the coverage report.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Coverage {
    pub events: u32,
    /// Natives and effects the application does not execute, with their counts.
    pub missing: BTreeMap<String, u32>,
}

impl Coverage {
    fn miss(&mut self, what: impl Into<String>) {
        *self.missing.entry(what.into()).or_default() += 1;
    }
}

/// The render side of the characters of the running event.
#[derive(Debug, Default)]
struct Bindings {
    /// Scene actor of each scripted model.
    actors: HashMap<(Target, String), usize>,
    /// Instances of the actors, hidden again when the event is over.
    instances: Vec<usize>,
    /// Instance and world matrix of each map NPC before the event moved it.
    homes: Vec<(usize, Mat4)>,
    /// Animation request last started for each target.
    played: HashMap<Target, u32>,
}

/// The event task and what it did to the scene.
#[derive(Debug)]
pub struct EventPlayer {
    director: Director,
    cutscene: Cutscene,
    bindings: Bindings,
    ticks: u32,
    /// The camera of the field before the first event.
    saved_camera: Option<Camera>,
    hero_hidden: bool,
    /// Instances whose world matrix changed: the renderer must be told.
    dirty: Vec<usize>,
    coverage: Coverage,
    running: Option<u16>,
    /// The events that started, in order.
    started: Vec<u16>,
    /// The event whose scene [`EventPlayer::prepare`] built and that has not started yet.
    prepared: Option<u16>,
}

impl Default for EventPlayer {
    fn default() -> Self {
        Self::new()
    }
}

/// World matrix of a pose: translation, rotation (Y, X, Z) and scale; hidden objects have a
/// null scale.
fn world_of(pose: &Pose, visible: bool) -> Mat4 {
    if !visible {
        return Mat4::from_scale(Vec3::ZERO);
    }
    Mat4::from_translation(pose.position)
        * Mat4::from_euler(
            EulerRot::YXZ,
            pose.rotation.y,
            pose.rotation.x,
            pose.rotation.z,
        )
        * Mat4::from_scale(pose.scale)
}

impl EventPlayer {
    pub fn new() -> Self {
        Self {
            director: Director::new(LANGUAGE),
            cutscene: Cutscene::new(),
            bindings: Bindings::default(),
            ticks: 0,
            saved_camera: None,
            hero_hidden: false,
            dirty: Vec::new(),
            coverage: Coverage::default(),
            running: None,
            started: Vec::new(),
            prepared: None,
        }
    }

    /// Queues an event (a trigger's action 0x77, 0x80 or 0x9b, or `--event`).
    pub fn queue(&mut self, event: u16) {
        status(&format!("event {event} queued"));
        self.director.dispatch(vec![Request::Event(event)]);
    }

    /// An event runs or waits: the field is not playable.
    pub fn is_active(&self) -> bool {
        self.director.is_active()
    }

    /// The event the next step starts and whose scene is not prepared yet, when none runs
    /// (the script starts on the next even tick).
    pub fn pending_start(&self) -> Option<u16> {
        self.director
            .pending_start()
            .filter(|event| self.prepared != Some(*event))
    }

    /// The events that started, in order.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn started(&self) -> &[u16] {
        &self.started
    }

    /// The hero is hidden by the script.
    pub fn hero_hidden(&self) -> bool {
        self.hero_hidden
    }

    /// The instances whose world matrix changed since the last call.
    pub fn take_dirty(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.dirty)
    }

    /// What the events asked that is not executed (`--events-report`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn coverage(&self) -> &Coverage {
        &self.coverage
    }

    /// Natives called without an effect, with their call counts.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn unimplemented(&self) -> &BTreeMap<i32, u32> {
        self.director.unimplemented()
    }

    /// Gets the scene ready for `event` (see the module documentation): adds the models the
    /// script loads, the motions the hero gets, and seeds the cutscene with the poses of the
    /// hero and of the map's NPCs. Returns whether models were added, so the application
    /// must upload the scene again.
    pub fn prepare(&mut self, rom: &[u8], event: u16, stage: &mut Stage) -> bool {
        // The actors of the previous event are not drawn any more.
        for instance in self.bindings.instances.drain(..) {
            if let Some(i) = stage.scene.scene.instances.get_mut(instance) {
                i.world = Mat4::from_scale(Vec3::ZERO);
                self.dirty.push(instance);
            }
        }
        self.bindings.actors.clear();
        self.bindings.played.clear();
        self.restore_npcs(stage);
        self.cutscene.restart();
        self.prepared = Some(event);
        let scanned = scan(rom, event, stage.world);
        if let Some(reason) = &scanned.failure {
            status(&format!("event {event}: {reason}"));
        }
        status(&format!(
            "event {event}: {} models to load, {} hero packages, {} updates",
            scanned.models.len(),
            scanned.hero_packages.len(),
            scanned.updates
        ));
        let files = LazyRom::new(rom);
        let data = |path: &str| files.file(&format!("data/{path}"));
        let mut added = false;
        for path in &scanned.hero_packages {
            match data(path) {
                Some(bytes) => match stage.scene.add_hero_package(bytes) {
                    Ok(()) => added = true,
                    Err(e) => self.fail(format!("hero package {path}: {e}")),
                },
                None => self.fail(format!("hero package {path}: not in the ROM")),
            }
        }
        for plan in &scanned.models {
            if plan.path.ends_with(".spr") {
                self.coverage.miss(format!("2D sprite actor {}", plan.path));
                continue;
            }
            // A monster is the `.cchr` of its member of `enemy.gp2` (`native 233`).
            let monster = plan
                .path
                .ends_with(".mon")
                .then(|| monster_archive(&files, &plan.path))
                .flatten();
            let chr = match (&monster, plan.path.ends_with(".mon")) {
                (Some(bytes), _) => bytes.as_slice(),
                (None, true) => {
                    self.fail(format!("monster {}: not in enemy.gp2", plan.path));
                    continue;
                }
                (None, false) => match data(&plan.path) {
                    Some(bytes) => bytes,
                    None => {
                        self.fail(format!("model {}: not in the ROM", plan.path));
                        continue;
                    }
                },
            };
            let packages: Vec<&[u8]> = plan
                .packages
                .iter()
                .filter_map(|p| match data(p) {
                    Some(bytes) => Some(bytes),
                    None => {
                        self.coverage.miss(format!("package {p} not in the ROM"));
                        None
                    }
                })
                .collect();
            match stage.scene.add_actor(chr, &packages) {
                Ok(actor) => {
                    if let Some(instance) = stage.scene.actor_instance(actor) {
                        self.bindings.instances.push(instance);
                    }
                    self.bindings
                        .actors
                        .insert((plan.target, plan.path.clone()), actor);
                    added = true;
                }
                Err(e) => self.fail(format!("model {}: {e}", plan.path)),
            }
        }
        self.seed(stage);
        self.cutscene.seed_camera(
            stage.camera.eye(),
            stage.camera.target,
            (stage.camera.fovy / 2.0).to_degrees(),
        );
        added
    }

    fn fail(&mut self, message: String) {
        status(&format!("  warning: {message}"));
        self.coverage.miss(message);
    }

    /// The poses the script's first commands start from.
    fn seed(&mut self, stage: &mut Stage) {
        self.cutscene.seed(
            Target::Hero,
            Pose {
                position: stage.hero.position(),
                rotation: Vec3::new(0.0, angle_of_yaw(stage.hero.yaw()), 0.0),
                scale: stage.scene.hero_scale(),
                ..Pose::default()
            },
        );
        self.bindings.homes.clear();
        for (index, instance) in stage.scene.scene.instances.iter().enumerate() {
            // Map NPCs: a placement of the cast (`block` unset) that is not the hero.
            if instance.block != usize::MAX || instance.instance == usize::MAX {
                continue;
            }
            let id = instance.instance as i32;
            let (scale, rotation, position) = instance.world.to_scale_rotation_translation();
            let (y, x, z) = rotation.to_euler(EulerRot::YXZ);
            self.cutscene.seed(
                Target::Npc(id),
                Pose {
                    position,
                    rotation: Vec3::new(x, y, z),
                    scale,
                    ..Pose::default()
                },
            );
            self.bindings.homes.push((index, instance.world));
        }
    }

    /// Puts the map NPCs back where the cast placed them.
    fn restore_npcs(&mut self, stage: &mut Stage) {
        for (index, world) in std::mem::take(&mut self.bindings.homes) {
            if let Some(i) = stage.scene.scene.instances.get_mut(index)
                && i.world != world
            {
                i.world = world;
                self.dirty.push(index);
            }
        }
    }

    /// One fixed step (one tick of 1/60 s). Returns what the application must do.
    pub fn step(&mut self, stage: &mut Stage) -> Vec<Outcome> {
        let mut outcomes = Vec::new();
        if self.ticks.is_multiple_of(2) {
            // An update of the game: the script runs once.
            let files = LazyRom::new(stage.rom);
            let events = self.director.step(&files, Frame::default(), stage.world);
            for event in events {
                self.handle(event, stage, &mut outcomes);
            }
            if self.running.is_some() && !self.director.is_active() {
                self.finish(stage);
                outcomes.push(Outcome::Finished);
            }
        }
        self.ticks = self.ticks.wrapping_add(1);
        let collision = stage.collision;
        let signals = self
            .cutscene
            .tick(&|x, z, y| collision.ground_height(x, z, y));
        for signal in signals {
            self.director.signal(signal);
        }
        if stage.talk.take_closed() {
            self.director.signal(Signal::MessageClosed);
        }
        if let Some(runtime) = self.director.runtime_mut() {
            self.cutscene.sync_runtime(runtime);
        }
        self.present(stage);
        outcomes
    }

    /// The jingle a script asked for played out (native 725 reads 0 from now on).
    pub fn jingle_ended(&mut self) {
        self.director.signal(Signal::JingleEnded);
    }

    fn handle(&mut self, event: Event, stage: &mut Stage, outcomes: &mut Vec<Outcome>) {
        self.coverage.events += 1;
        // The files are in memory already (the models were added before the script ran).
        if matches!(event, Event::Preload(_)) {
            self.director.signal(Signal::Preloaded);
        }
        match &event {
            Event::Started { event } => {
                status(&format!("event {event} starts"));
                self.running = Some(*event);
                self.started.push(*event);
                self.prepared = None;
                if self.saved_camera.is_none() {
                    self.saved_camera = Some(*stage.camera);
                }
            }
            Event::LoadFailed { event, reason } => {
                status(&format!("event {event} not played: {reason}"));
                self.coverage.miss(format!("event {event}: {reason}"));
            }
            Event::Fault(fault) => {
                status(&format!("event script fault: {fault}"));
                self.coverage.miss(format!("script fault: {fault}"));
            }
            Event::Ended { event } => {
                status(&format!("event {event} ended"));
                outcomes.push(Outcome::Ended(*event));
            }
            Event::Sound(sound) => outcomes.push(Outcome::Sound(sound.clone())),
            _ => {}
        }
        for output in self.cutscene.apply(&event) {
            match output {
                Output::Message { text, .. } => {
                    let text = match &text {
                        Text::Id(id) => self.director.texts().get(id).cloned().unwrap_or_default(),
                        Text::Literal(s) => s.clone(),
                    };
                    if let Err(e) = stage.talk.show_event(text.as_bytes()) {
                        status(&format!("event message: {e:#}"));
                        self.coverage.miss("message window unavailable");
                        self.director.signal(Signal::MessageClosed);
                    }
                }
                Output::CloseMessage => stage.talk.close_event(),
                Output::HideMessageFrame => stage.talk.hide_event_frame(),
            }
        }
    }

    /// The last event ended: the field takes the scene back.
    fn finish(&mut self, stage: &mut Stage) {
        self.running = None;
        self.cutscene.clear_fades();
        stage.talk.set_fade(None);
        stage.talk.close_event();
        self.restore_npcs(stage);
        for instance in self.bindings.instances.drain(..) {
            if let Some(i) = stage.scene.scene.instances.get_mut(instance) {
                i.world = Mat4::from_scale(Vec3::ZERO);
                self.dirty.push(instance);
            }
        }
        self.bindings.actors.clear();
        self.hero_hidden = false;
        if let Some(camera) = self.saved_camera.take() {
            *stage.camera = camera;
        }
        // Whatever the cutscene made of the unrendered parts is gone with it.
        for (what, n) in self.cutscene.ignored() {
            *self.coverage.missing.entry(what.clone()).or_default() += n;
        }
        status(&format!(
            "events {:?} over: not executed {:?}, natives without effect {:?}",
            self.started,
            self.coverage.missing,
            self.director.unimplemented()
        ));
    }

    fn set_world(&mut self, scene: &mut AnimatedScene, instance: usize, world: Mat4) {
        if let Some(i) = scene.scene.instances.get_mut(instance)
            && i.world != world
        {
            i.world = world;
            self.dirty.push(instance);
        }
    }

    /// Applies the cutscene to the scene, the hero and the camera.
    fn present(&mut self, stage: &mut Stage) {
        let fade = self.cutscene.fades().main.level;
        stage.talk.set_fade((fade.abs() > 0.05).then(|| {
            let alpha = (fade.abs() / 16.0).min(1.0);
            if fade < 0.0 {
                [0.0, 0.0, 0.0, alpha]
            } else {
                [1.0, 1.0, 1.0, alpha]
            }
        }));
        let mut finished: Vec<(Target, u32)> = Vec::new();
        let nodes: Vec<(Target, dq9_core::cutscene::Node)> =
            self.cutscene.nodes().map(|(t, n)| (t, n.clone())).collect();
        for (target, node) in nodes {
            // The motion the script asked for, started once per request.
            let requested = node
                .animation
                .as_ref()
                .filter(|a| self.bindings.played.get(&target) != Some(&a.serial));
            let actor = match &node.model {
                Some(path) => self.bindings.actors.get(&(target, path.clone())).copied(),
                None => None,
            };
            if let Some(animation) = requested {
                self.bindings.played.insert(target, animation.serial);
                let started = match target {
                    Target::Hero => stage
                        .scene
                        .play_hero(&animation.name, animation.plays_once()),
                    Target::Model(_) | Target::Cast(_) => actor.is_some_and(|a| {
                        stage
                            .scene
                            .play_actor(a, &animation.name, animation.plays_once(), 1.0)
                    }),
                    _ => false,
                };
                if !started {
                    // Nothing to wait for: the script goes on.
                    self.coverage
                        .miss(format!("motion {} of {target:?}", animation.name));
                    finished.push((target, animation.serial));
                }
            }
            if let Some(animation) = &node.animation {
                let wraps = match target {
                    Target::Hero => stage.scene.hero_wraps(),
                    Target::Model(_) | Target::Cast(_) => {
                        actor.map_or(0, |a| stage.scene.actor_wraps(a))
                    }
                    _ => 0,
                };
                if wraps > 0 && self.bindings.played.get(&target) == Some(&animation.serial) {
                    finished.push((target, animation.serial));
                }
            }
            match target {
                Target::Hero => {
                    if node.placed {
                        *stage.hero =
                            Hero::at(node.pose.position, yaw_of_angle(node.pose.rotation.y));
                    }
                    self.hero_hidden = !node.visible;
                }
                Target::Npc(id) => {
                    let Some(instance) = u16::try_from(id)
                        .ok()
                        .and_then(|id| stage.scene.npc_instance(id))
                    else {
                        continue;
                    };
                    if !node.placed {
                        continue;
                    }
                    // Sprites are billboards: no rotation, no scale.
                    let sprite = stage
                        .scene
                        .scene
                        .instances
                        .get(instance)
                        .and_then(|i| stage.scene.scene.models.get(i.model))
                        .is_some_and(|m| m.archive.ends_with(".spr"));
                    let world = if sprite {
                        stage.scene.set_npc_angle(id as u16, node.pose.rotation.y);
                        if node.visible {
                            Mat4::from_translation(node.pose.position)
                        } else {
                            Mat4::from_scale(Vec3::ZERO)
                        }
                    } else {
                        let mut pose = node.pose;
                        if pose.scale == Vec3::ONE {
                            pose.scale = Vec3::splat(NPC_SCALE);
                        }
                        world_of(&pose, node.visible)
                    };
                    self.set_world(stage.scene, instance, world);
                }
                Target::Model(_) | Target::Cast(_) => {
                    let Some(instance) = actor.and_then(|a| stage.scene.actor_instance(a)) else {
                        continue;
                    };
                    let shown = node.visible && node.placed && node.model.is_some();
                    self.set_world(stage.scene, instance, world_of(&node.pose, shown));
                }
                Target::Object(_) | Target::Detached(_) => {}
            }
        }
        for (target, serial) in finished {
            self.cutscene.animation_finished(target, serial);
        }
        self.present_camera(stage);
    }

    fn present_camera(&mut self, stage: &mut Stage) {
        let view = self.cutscene.camera();
        let (mut eye, mut target) = (view.eye, view.target);
        if let Some(bones) = &view.bones
            && let Some((e, t)) = self.bone_camera(stage.scene, bones)
        {
            (eye, target) = (e, t);
        }
        if (target - eye).length() > 1e-3 {
            stage.camera.look_at(eye, target);
        }
        stage.camera.fovy = (2.0 * view.half_fov).to_radians();
        stage.camera.far = stage.camera.far.max(stage.camera.distance * 4.0);
        stage.camera.roll = view.roll.to_radians();
    }

    /// Eye and target of a camera driven by two bones of the model bound to a slot.
    fn bone_camera(
        &self,
        scene: &AnimatedScene,
        bones: &dq9_core::cutscene::BoneCamera,
    ) -> Option<(Vec3, Vec3)> {
        let target = self.cutscene.slot_target(bones.slot)?;
        let node = self.cutscene.node(target)?;
        let actor = *self.bindings.actors.get(&(target, node.model.clone()?))?;
        // The model may have no polygon, hence no instance: its world is its pose.
        let world = world_of(&node.pose, true);
        let at = |name: &str| -> Option<Vec3> {
            let bone = scene.actor_bone(actor, name).ok()??;
            Some(world.transform_point3(bone.w_axis.truncate()))
        };
        Some((at(&bones.eye)?, at(&bones.target)?))
    }
}

/// Where `event` happens (`dq9_core::script::load::place`).
pub fn place(rom: &[u8], event: u16) -> Option<dq9_core::script::load::EventPlace> {
    dq9_core::script::load::place(&LazyRom::new(rom), event)
}

#[cfg(not(target_arch = "wasm32"))]
impl EventPlayer {
    /// What the running script is waiting for, for the log of a screenshot.
    pub fn waiting(&mut self) -> String {
        let Some(runtime) = self.director.runtime_mut() else {
            return "no event".into();
        };
        let actors: Vec<usize> = (0..runtime.actor_busy.len())
            .filter(|&a| runtime.actor_busy[a])
            .collect();
        let animating: Vec<usize> = (0..runtime.animating.len())
            .filter(|&s| runtime.animating[s])
            .collect();
        format!(
            "message open {}, camera busy {}, preload {}, busy actors {actors:?}, animating slots {animating:?}",
            runtime.message_open, runtime.camera_busy, runtime.preload_pending
        )
    }
}

/// The character archive of a monster (`enemy/z000a.mon`): the first `.cchr` of its member of
/// `data/pack_lv5/enemy.gp2`, LZ-decompressed (`LoadMonsterCharacter`, native 233).
fn monster_archive(files: &LazyRom, path: &str) -> Option<Vec<u8>> {
    let member = path.strip_prefix("enemy/").unwrap_or(path);
    let enemies = dq9_formats::Gpc2::parse(files.file("data/pack_lv5/enemy.gp2")?).ok()?;
    let entry = enemies.entries.iter().find(|e| e.name == member)?;
    let mon = enemies.member(entry).ok()?;
    let monster = nds_rom::Narc::parse(&mon).ok()?;
    let cchr = monster
        .names
        .files
        .iter()
        .find(|e| e.path.ends_with(".cchr"))?;
    nds_rom::compress::decompress(monster.file(cchr.id).ok()?).ok()
}
