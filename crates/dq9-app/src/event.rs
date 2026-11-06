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
use std::collections::{BTreeMap, BTreeSet, HashMap};

use dq9_core::cutscene::{
    Balloon, ClockCommand, Cutscene, Node, Output, PlacementChange, Pose, SceneLight, Target,
    TexelOp,
};
use dq9_core::field::{angle_of_yaw, yaw_of_angle};
use dq9_core::script::{Director, Event, Files, Frame, Signal, Sound, Text};
use dq9_core::trigger::{Request, State};
use dq9_core::{CollisionWorld, Hero};
use dq9_formats::lighting::LightingInfo;
use dq9_render::camera::DS_ASPECT;
use dq9_render::{AnimatedScene, Camera, Environment, TexelTint};
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
    /// Map NPCs the script gives a motion (`AnimatedScene::prepare_npc_motions`).
    npc_motions: BTreeSet<u16>,
    /// Motion packages the script adds to map NPCs (natives 229, 567), by NPC id.
    npc_packages: Vec<(u16, String)>,
    /// Field sprite ids of the balloons it puts up.
    balloons: BTreeSet<i32>,
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
            if let Some(balloon) = cutscene.balloon() {
                out.balloons.insert(balloon.sprite);
            }
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
            Target::Model(_) | Target::Cast(_) | Target::Object(_) => {
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
            Target::Npc(id) => {
                if let Ok(id) = u16::try_from(id) {
                    if node.animation.is_some() {
                        out.npc_motions.insert(id);
                    }
                    for (path, _) in &node.packages {
                        if !out.npc_packages.contains(&(id, path.clone())) {
                            out.npc_packages.push((id, path.clone()));
                        }
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
    /// An event started: its script runs from the next update.
    Started(u16),
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
    /// Scene instance of each scripted sprite sheet (cast mode 3).
    sprites: HashMap<(Target, String), usize>,
    /// Scene instance of each balloon sprite, by the field's sprite id.
    balloons: HashMap<i32, usize>,
    /// What the scripts did to each map placement instance: its world before, and now.
    placements: HashMap<usize, Placement>,
    /// The palette operation applied to each target.
    tinted: HashMap<Target, Option<TexelOp>>,
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
    /// The scene lighting the renderer's environment was last built from.
    applied_light: SceneLight,
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
            applied_light: SceneLight::default(),
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

    /// The environment the running events give the map (time of day, light fade, fog), when it
    /// changed since the last call; `info` and `clock` are the map's lighting script and the
    /// game's day clock. After the last event ends this returns the map's own environment once.
    pub fn environment(
        &mut self,
        info: Option<&LightingInfo>,
        clock: dq9_core::state::DayClock,
    ) -> Option<Environment> {
        let light = self.cutscene.light();
        if light.same_look(&self.applied_light) {
            return None;
        }
        self.applied_light = light.clone();
        Some(crate::lighting::scene_environment(
            info,
            clock,
            &self.applied_light,
        ))
    }

    /// A map was loaded: its environment and its placements are the map's own again.
    pub fn map_loaded(&mut self, camera: &Camera) {
        self.applied_light = SceneLight::default();
        self.bindings.placements.clear();
        if self.saved_camera.is_some() {
            self.saved_camera = Some(*camera);
        }
    }

    /// The requests of the scripts on the day clock (natives 579, 808) since the last call.
    pub fn take_clock_commands(&mut self) -> Vec<ClockCommand> {
        self.cutscene.take_clock_commands()
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
        // The hero and the map's NPCs keep their palettes until the script resets them; the
        // models of the previous event are gone.
        self.bindings
            .tinted
            .retain(|target, _| matches!(target, Target::Hero | Target::Npc(_)));
        self.bindings.sprites.clear();
        self.bindings.balloons.clear();
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
                match data(&format!("ani/{}", plan.path)) {
                    Some(bytes) => match stage.scene.add_sprite(&plan.path, bytes, None, false) {
                        Ok(instance) => {
                            self.bindings.instances.push(instance);
                            self.bindings
                                .sprites
                                .insert((plan.target, plan.path.clone()), instance);
                            added = true;
                        }
                        Err(e) => self.fail(format!("sprite {}: {e}", plan.path)),
                    },
                    None => self.fail(format!("sprite {}: not in the ROM", plan.path)),
                }
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
        for (id, path) in &scanned.npc_packages {
            match data(path) {
                Some(bytes) => {
                    if let Err(e) = stage.scene.add_npc_package(*id, bytes) {
                        self.fail(format!("package {path} of NPC {id}: {e}"));
                    }
                }
                None => self.fail(format!("package {path}: not in the ROM")),
            }
        }
        for &id in &scanned.npc_motions {
            match stage.scene.prepare_npc_motions(id) {
                Ok(changed) => added |= changed,
                Err(e) => self.fail(format!("motions of NPC {id}: {e}")),
            }
        }
        for &sprite in &scanned.balloons {
            let Some(name) = field_sprite(sprite) else {
                self.fail(format!("balloon sprite {sprite} unknown"));
                continue;
            };
            let Some(bytes) = icon_sprite(&files, name) else {
                self.fail(format!("balloon sprite {name}: not in icon.nsarc"));
                continue;
            };
            // `SetAnimation(1)`, which is also the animation a sheet starts with.
            match stage.scene.add_sprite(name, &bytes, Some(1), true) {
                Ok(instance) => {
                    self.bindings.balloons.insert(sprite, instance);
                    added = true;
                }
                Err(e) => self.fail(format!("balloon sprite {name}: {e}")),
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
        if self.running.is_none() && !self.director.is_active() {
            return Vec::new();
        }
        let mut outcomes = Vec::new();
        if self.ticks.is_multiple_of(2) {
            // An update of the game: the script runs once.
            let files = LazyRom::new(stage.rom);
            let events = self.director.step(&files, Frame::default(), stage.world);
            for event in events {
                self.handle(event, stage, &mut outcomes);
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
        self.apply_placements(stage);
        self.present(stage);
        // Present the final script pose before returning ownership. Presenting after
        // `finish` would overwrite the restored camera with the last cinematic view.
        if self.running.is_some() && !self.director.is_active() {
            self.finish(stage);
            outcomes.push(Outcome::Finished);
        }
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
                outcomes.push(Outcome::Started(*event));
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
        // The field draws the hero and the map's NPCs with their own colours again.
        for (target, applied) in std::mem::take(&mut self.bindings.tinted) {
            if applied.is_none() {
                continue;
            }
            match target {
                Target::Hero => stage.scene.tint_hero(None),
                Target::Npc(id) => {
                    if let Ok(id) = u16::try_from(id) {
                        stage.scene.tint_npc(id, None);
                    }
                }
                _ => {}
            }
        }
        self.hero_hidden = false;
        if let Some(mut camera) = self.saved_camera.take() {
            camera.follow_hero(stage.hero.position());
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
            let sprite = node
                .model
                .as_ref()
                .and_then(|path| self.bindings.sprites.get(&(target, path.clone())).copied());
            if self.bindings.tinted.get(&target).copied().flatten() != node.tint
                && (node.tint.is_some() || self.bindings.tinted.contains_key(&target))
            {
                let tint = node.tint.map(|op| TexelTint {
                    mul: op.mul(),
                    add: op.add(),
                });
                let done = match target {
                    Target::Hero => {
                        stage.scene.tint_hero(tint);
                        true
                    }
                    Target::Model(_) | Target::Cast(_) => {
                        actor.is_some_and(|a| stage.scene.tint_actor(a, tint))
                    }
                    Target::Npc(id) => {
                        u16::try_from(id).is_ok_and(|id| stage.scene.tint_npc(id, tint))
                    }
                    _ => false,
                };
                if !done && node.tint.is_some() {
                    self.coverage
                        .miss(format!("palette operation on {target:?}"));
                }
                // Whether it was done or has nothing to act on (a model that is gone).
                self.bindings.tinted.insert(target, node.tint);
            }
            if let Some(animation) = requested {
                self.bindings.played.insert(target, animation.serial);
                let started = match target {
                    Target::Hero => stage
                        .scene
                        .play_hero(&animation.name, animation.plays_once()),
                    Target::Model(_) | Target::Cast(_) | Target::Object(_) => match sprite {
                        Some(s) => stage.scene.play_sprite(s, &animation.name),
                        None => actor.is_some_and(|a| {
                            stage
                                .scene
                                .play_actor(a, &animation.name, animation.plays_once(), 1.0)
                        }),
                    },
                    Target::Npc(id) => u16::try_from(id).is_ok_and(|id| {
                        stage
                            .scene
                            .play_npc(id, &animation.name, animation.plays_once(), 1.0)
                    }),
                    _ => false,
                };
                if !started {
                    // Nothing to wait for: the script goes on.
                    let has_model = match target {
                        Target::Model(_) | Target::Cast(_) => sprite.is_none() && actor.is_some(),
                        Target::Npc(id) => {
                            u16::try_from(id).is_ok_and(|id| stage.scene.npc_has_model(id))
                        }
                        _ => false,
                    };
                    if has_model {
                        // `MaybeSetRegularAnimation` (`02036e44`) searches the model's motion
                        // tables by name and returns without changing anything when none has it.
                        self.coverage.miss(ABSENT_MOTION);
                    } else {
                        self.coverage
                            .miss(format!("motion {} of {target:?}", animation.name));
                    }
                    finished.push((target, animation.serial));
                }
            }
            if let Some(animation) = &node.animation {
                let wraps = match target {
                    Target::Hero => stage.scene.hero_wraps(),
                    Target::Model(_) | Target::Cast(_) | Target::Object(_) if sprite.is_some() => 1,
                    Target::Model(_) | Target::Cast(_) => {
                        actor.map_or(0, |a| stage.scene.actor_wraps(a))
                    }
                    Target::Npc(id) => u16::try_from(id).map_or(1, |id| stage.scene.npc_wraps(id)),
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
                Target::Model(_) | Target::Cast(_) | Target::Object(_) => {
                    if let Some(instance) = sprite {
                        // A script's sprite is a billboard turned by its rotation (`NpcSprite_FaceCamera`).
                        stage.scene.set_sprite_angle(instance, node.pose.rotation.y);
                        let world = if node.visible && node.placed {
                            Mat4::from_translation(node.pose.position)
                                * Mat4::from_scale(node.pose.scale)
                        } else {
                            Mat4::from_scale(Vec3::ZERO)
                        };
                        self.set_world(stage.scene, instance, world);
                        continue;
                    }
                    let Some(instance) = actor.and_then(|a| stage.scene.actor_instance(a)) else {
                        continue;
                    };
                    let world = match self.attachment_base(stage.scene, &node, 0) {
                        Some(base) => {
                            base * world_of(&node.pose, node.visible && node.model.is_some())
                        }
                        None => world_of(
                            &node.pose,
                            node.visible && node.placed && node.model.is_some(),
                        ),
                    };
                    self.set_world(stage.scene, instance, world);
                }
                Target::Detached(_) => {}
            }
        }
        for (target, serial) in finished {
            self.cutscene.animation_finished(target, serial);
        }
        self.present_camera(stage);
        self.present_balloon(stage);
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

    /// World matrix a target is drawn with now: the hero, a map NPC, or a model, which hangs on
    /// a bone of another target while it is attached.
    fn target_world(&self, scene: &AnimatedScene, target: Target, depth: u32) -> Option<Mat4> {
        let node = self.cutscene.node(target)?;
        match target {
            Target::Hero => Some(scene.hero_world(node.pose.position, node.pose.rotation.y)),
            Target::Npc(id) => {
                let instance = scene.npc_instance(u16::try_from(id).ok()?)?;
                scene.scene.instances.get(instance).map(|i| i.world)
            }
            Target::Model(_) | Target::Cast(_) => {
                let local = world_of(&node.pose, node.visible && node.model.is_some());
                Some(
                    self.attachment_base(scene, node, depth)
                        .map_or(local, |base| base * local),
                )
            }
            Target::Object(_) | Target::Detached(_) => None,
        }
    }

    /// The matrix a model attached to a bone of another target (natives 235, 236) is placed in:
    /// the parent's world times the bone's (`02037868`, `020378a0`); the model's own pose is
    /// relative to it **[hypothèse]** (the scripts reset the scale of the attached models to
    /// 1, which only makes sense if the parent's scale is inherited). `None` when the model
    /// is not attached or its parent cannot be resolved.
    fn attachment_base(&self, scene: &AnimatedScene, node: &Node, depth: u32) -> Option<Mat4> {
        let (parent, bone) = node.attached.as_ref()?;
        if depth > 4 {
            return None;
        }
        if !self.cutscene.node(*parent)?.visible {
            return Some(Mat4::from_scale(Vec3::ZERO));
        }
        let world = self.target_world(scene, *parent, depth + 1)?;
        let bone = match parent {
            Target::Hero => scene.hero_bone(bone),
            Target::Model(_) | Target::Cast(_) => {
                let model = self.cutscene.node(*parent)?.model.clone()?;
                match self.bindings.actors.get(&(*parent, model)) {
                    Some(actor) => scene.actor_bone(*actor, bone),
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        };
        Some(world * bone.ok().flatten().unwrap_or(Mat4::IDENTITY))
    }

    /// The balloon of natives 541, 561 and 542: a billboard sprite that lands where the game
    /// draws its 2D sprite, the actor's position projected on the 256 × 192 screen, then offset
    /// (`ov_017::0219b43c`). The scene draws it in the world instead: it is placed on the ray
    /// through that pixel, a little in front of the actor, and scaled so that a texel is one DS
    /// pixel at that depth.
    fn present_balloon(&mut self, stage: &mut Stage) {
        let shown = self.cutscene.balloon().copied().and_then(|balloon| {
            let instance = *self.bindings.balloons.get(&balloon.sprite)?;
            Some((instance, self.balloon_world(stage, &balloon, instance)?))
        });
        let all: Vec<usize> = self.bindings.balloons.values().copied().collect();
        for instance in all {
            let world = match shown {
                Some((i, world)) if i == instance => world,
                _ => Mat4::from_scale(Vec3::ZERO),
            };
            self.set_world(stage.scene, instance, world);
        }
    }

    fn balloon_world(&self, stage: &Stage, balloon: &Balloon, instance: usize) -> Option<Mat4> {
        let target = self
            .cutscene
            .actor_target(usize::try_from(balloon.actor).ok()?)?;
        let node = self.cutscene.node(target)?;
        if !node.visible || !node.placed {
            return None;
        }
        let camera = &*stage.camera;
        let view = camera.view();
        let at = view.transform_point3(node.pose.position);
        let depth = -at.z;
        if depth <= camera.near {
            return None;
        }
        let (width, height) = stage.scene.sprite_size(instance)?;
        let half = (camera.fovy / 2.0).tan();
        // DS pixels (256 × 192, y down) of the actor, then of the sprite's centre.
        let px = (
            (at.x / (depth * half * DS_ASPECT) + 1.0) * 0.5 * 256.0,
            (1.0 - at.y / (depth * half)) * 0.5 * 192.0,
        );
        let centre = (
            px.0 + balloon.offset[0] as f32 + width * 0.5,
            px.1 + balloon.offset[1] as f32 + height * 0.5,
        );
        let distance = (depth - 0.25).max(camera.near + 0.05);
        let pixel = 2.0 * distance * half / 192.0;
        let ndc = (centre.0 / 256.0 * 2.0 - 1.0, 1.0 - centre.1 / 192.0 * 2.0);
        let point = Vec3::new(
            ndc.0 * distance * half * DS_ASPECT,
            ndc.1 * distance * half,
            -distance,
        );
        let centre = view.inverse().transform_point3(point);
        // A billboard has 143/4096 world units per texel and its pivot half a sheet above the
        // placement (`0x0203f54c`).
        let scale = pixel / (143.0 / 4096.0);
        Some(
            Mat4::from_translation(centre - Vec3::Y * (height * 0.5 * pixel))
                * Mat4::from_scale(Vec3::splat(scale)),
        )
    }

    /// Natives 574 and 575: the placements of the map the scripts hide, show or move. The game
    /// keeps these changes until the zone is loaded again.
    fn apply_placements(&mut self, stage: &mut Stage) {
        for change in self.cutscene.take_placement_changes() {
            let (group, id) = match change {
                PlacementChange::Hidden { group, id, .. }
                | PlacementChange::Moved { group, id, .. } => (group, id),
            };
            let instances: Vec<usize> = stage
                .scene
                .scene
                .placements
                .iter()
                .filter(|&&(entry, placement, _)| {
                    i64::from(entry) == i64::from(group) && i32::from(placement) == id
                })
                .map(|&(_, _, instance)| instance)
                .collect();
            if instances.is_empty() {
                let collision =
                    stage
                        .scene
                        .scene
                        .undrawn_placements
                        .iter()
                        .any(|&(entry, placement)| {
                            i64::from(entry) == i64::from(group) && i32::from(placement) == id
                        });
                if collision {
                    // A collision mesh: only the walls and floor change, nothing to draw.
                    self.coverage.miss(COLLISION_PLACEMENT);
                } else {
                    self.coverage
                        .miss(format!("placement ({group}, {id}) not in the map"));
                }
                continue;
            }
            for instance in instances {
                // The placement at the map's own transform, even when the chosen time of day
                // hid it: native 574 shows it again where the map put it.
                let state = self
                    .bindings
                    .placements
                    .entry(instance)
                    .or_insert_with(|| Placement::of(&stage.scene.scene.instances[instance]));
                match change {
                    PlacementChange::Hidden { hidden, .. } => state.hidden = hidden,
                    PlacementChange::Moved { position, .. } => {
                        state.position = Some(Vec3::from_array(position));
                    }
                }
                let world = state.world();
                self.set_world(stage.scene, instance, world);
            }
        }
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

/// A map placement a script hid or moved: its world before the first change and what it is
/// now.
#[derive(Debug, Clone, Copy)]
struct Placement {
    /// Home world matrix of the placement, as built from the map (`0x02013168`).
    home: Mat4,
    hidden: bool,
    position: Option<Vec3>,
}

impl Placement {
    /// The state of a placement before a script changes it: its home transform and whether
    /// the chosen time of day hides it (a null world scale).
    fn of(instance: &dq9_render::SceneInstance) -> Self {
        Self {
            home: instance.home,
            hidden: !instance.drawn(),
            position: None,
        }
    }

    /// The world matrix the scene draws: the home transform with the translation native 575
    /// last wrote, and a null scale while native 574 hides it. Hiding, showing and moving are
    /// independent, so their order does not change the result.
    fn world(&self) -> Mat4 {
        if self.hidden {
            return Mat4::from_scale(Vec3::ZERO);
        }
        let mut world = self.home;
        if let Some(position) = self.position {
            world.w_axis = position.extend(1.0);
        }
        world
    }
}

/// The sheet of a sprite the field draws by itself (`fieldSpriteFiles`, `docs/re/stb-natives.md`).
fn field_sprite(id: i32) -> Option<&'static str> {
    Some(match id {
        2 => "fuki_com",
        3 => "fuki_in",
        4 => "fuki_hkn",
        12 => "field_mon01",
        13 => "ev_mark",
        14 => "field_qu",
        15 => "fuki_com_q",
        16 => "fuki_com_apc",
        _ => return None,
    })
}

/// A sprite sheet of `data/bin/icon.nsarc`, where the field keeps its own.
fn icon_sprite(files: &LazyRom, name: &str) -> Option<Vec<u8>> {
    let narc = nds_rom::Narc::parse(files.file("data/bin/icon.nsarc")?).ok()?;
    let wanted = format!("{name}.spr");
    let entry = narc
        .names
        .files
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case(&wanted))?;
    narc.file(entry.id).ok().map(<[u8]>::to_vec)
}

/// Natives 574 and 575 on a collision mesh of the map: the walls and the floor change, which no
/// image shows (`docs/re/stb-natives.md`). Not counted against an event by the report.
pub const COLLISION_PLACEMENT: &str = "collision placement";

/// A script asks a model for a motion none of its archives has: the game's name search fails
/// and nothing changes (`ov_001::02164578` → `Object3D::MaybeSetRegularAnimation`, `02036e44`).
/// Not counted against an event by the report.
pub const ABSENT_MOTION: &str = "motion absent from the model";

#[cfg(test)]
mod tests {
    use super::*;

    /// Home transform of a placement as `world_matrix` builds it (`0x02013168`): translation,
    /// a `y` rotation and a non-uniform scale, so that a show that lost either would fail.
    fn home() -> Mat4 {
        Mat4::from_translation(Vec3::new(10.0, 2.0, -3.0))
            * Mat4::from_rotation_y(0.5)
            * Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0))
    }

    fn scene_instance(world: Mat4) -> dq9_render::SceneInstance {
        dq9_render::SceneInstance {
            model: 0,
            world,
            home: home(),
            block: 3,
            instance: 7,
        }
    }

    fn placement() -> Placement {
        Placement::of(&scene_instance(home()))
    }

    #[test]
    fn a_move_alone_does_not_show_a_placement_hidden_at_the_chosen_time() {
        let mut placement = Placement::of(&scene_instance(Mat4::from_scale(Vec3::ZERO)));
        let position = Vec3::new(1.0, 2.0, 3.0);
        placement.position = Some(position);
        assert_eq!(placement.world(), Mat4::from_scale(Vec3::ZERO));
        placement.hidden = false;
        assert_eq!(placement.world().w_axis.truncate(), position);
    }

    #[test]
    fn placed_placement_keeps_rotation_and_scale_when_hidden_then_shown() {
        let mut placement = placement();
        let position = Vec3::new(-7.0, 0.5, 12.0);
        placement.position = Some(position);
        placement.hidden = true;
        assert_eq!(placement.world(), Mat4::from_scale(Vec3::ZERO));
        placement.hidden = false;
        let world = placement.world();
        assert_eq!(world.w_axis.truncate(), position);
        let home = home();
        assert_eq!(world.x_axis.truncate(), home.x_axis.truncate());
        assert_eq!(world.y_axis.truncate(), home.y_axis.truncate());
        assert_eq!(world.z_axis.truncate(), home.z_axis.truncate());
    }
}
