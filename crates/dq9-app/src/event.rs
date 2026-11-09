//! Script events in the application: the event task (`dq9_core::script::Director`) runs the
//! `.stb` script of an event, the cutscene layer (`dq9_core::cutscene`) keeps what it did to
//! the actors, the camera and the screen brightness, and this module draws it: actor
//! transforms and motions go to the render instances, the camera to the view, messages to
//! the message window, fades to an overlay (`docs/app.md`, § Events).
//!
//! One fixed step of 1/60 s is one tick. The script runs every second tick (an update of the
//! game is 2 ticks, `docs/re/stb-natives.md`, § Time and units); the cutscene advances at
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
use glam::{EulerRot, Mat4, Quat, Vec3};
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
    /// Scene flag from the event's `EventScript` (no separate list parse).
    scene_flag: Option<u16>,
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
        if out.scene_flag.is_none() {
            out.scene_flag = director.scene_flag();
        }
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
    /// Native 559: the low byte of quest-manager `+0x490` becomes `value`; trigger condition
    /// 0x1f reads it (`Raw::Quest490`). The application owns that state.
    SetQuestSignal(u8),
    /// Native 584 opened a door with its own effect: the application looks up the map's
    /// doorway sound (`Field::door_sound`) and plays base effect 200 + low quartet.
    DoorSound { block: i32, placement: i32 },
}

/// Applies the collision side of placement changes the scripts made (natives 574/575),
/// as drained by [`EventPlayer::take_collision_changes`]. The application and the report
/// playback both call it, so no consumer leaves a change queued.
pub(crate) fn apply_collision_changes(
    collision: &mut CollisionWorld,
    changes: Vec<PlacementChange>,
) {
    for change in changes {
        match change {
            PlacementChange::Hidden { group, id, hidden } => {
                collision.set_placement_hidden(group as u32, id as u16, hidden);
            }
            PlacementChange::Moved {
                group,
                id,
                position,
            } => {
                // Natives 575 store `fx32` (float × 4096, truncated like `ToVec3fix`).
                let position = position.map(|v| (v * 4096.0) as i32);
                collision.set_placement_position(group as u32, id as u16, position);
            }
        }
    }
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
    /// Placements the scripts hid (natives 574, actions 0x95/0x96), by `(block entry,
    /// placement id)`: a show restores a descendant only when no other hidden ancestor
    /// remains.
    hidden_placements: BTreeSet<(u32, u16)>,
    /// The palette operation applied to each target.
    tinted: HashMap<Target, Option<TexelOp>>,
}

/// The character a native 552 camera drags: its last position and facing (game angle,
/// radians, 0 faces +z), and the updates since the last turn.
#[derive(Debug, Clone, Copy)]
struct ThreeBoneDrag {
    target: Target,
    position: Vec3,
    facing: f32,
    updates: u32,
}

/// One map NPC as the cast snapshot keeps it (`0x0203e0b0`: ten bytes per NPC table entry
/// from `+0x9e`): position in 1/256 units (`fx >> 4`), 16-bit angle, member number and
/// whether action 0x7c had removed it (bit 9, from the object's flag `0x8000`). The entry's
/// motion count (bits 10–14), bit 15 and the table's `+0x1e4` mask (object flag `0x40000`)
/// are not modelled.
#[derive(Debug, Clone, Copy, PartialEq)]
struct KeptNpc {
    id: u16,
    position: Vec3,
    angle: f32,
    removed: bool,
}

/// The NPC table's snapshot: the map it was taken on (`+0x1de`) and its entries (`+0x1e0`).
#[derive(Debug, Clone, PartialEq)]
struct KeptCast {
    map: u16,
    npcs: Vec<KeptNpc>,
}

/// A world coordinate as the snapshot keeps it: the fx32 value shifted right by 4 into 16
/// bits (`0x0203e0b0`), read back shifted left by 4 (`0x0203e2d4`).
fn kept_coordinate(v: f32) -> f32 {
    f32::from(((v * 4096.0).round() as i32 >> 4) as i16) / 256.0
}

/// An angle in radians as the snapshot keeps it: 4096 units per radian in 16 bits.
fn kept_angle(angle: f32) -> f32 {
    f32::from((angle * 4096.0).round() as i32 as i16) / 4096.0
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
    /// Placement changes the scripts made that the collision must also apply (natives
    /// 574/575 on a collision mesh): drained by the application after each step.
    collision_changes: Vec<PlacementChange>,
    coverage: Coverage,
    running: Option<u16>,
    /// The events that started, in order.
    started: Vec<u16>,
    /// The event whose scene [`EventPlayer::prepare`] built and that has not started yet.
    prepared: Option<u16>,
    /// Scene flag carried by the prepared event's `EventScript`.
    prepared_scene_flag: Option<u16>,
    /// The character a native 552 camera drags to its source's `foot` bone.
    three_bone: Option<ThreeBoneDrag>,
    /// The scene lighting the renderer's environment was last built from.
    applied_light: SceneLight,
    /// Map NPCs native 0x7c removed: their cast instance stays hidden through the following
    /// script updates (a scene seeds every NPC of the cast). A map change or a cast rebuild
    /// forgets them, except those the cast snapshot (`kept_cast`) gives back.
    removed_npcs: BTreeSet<u16>,
    /// The cast snapshot an event start took (`0x0203e0b0`), restored by the next cast load
    /// of the same map and consumed by any (`0x0203e2d4`).
    kept_cast: Option<KeptCast>,
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

/// Wraps an angle to (−π, π], as the game's 16-bit angle does after the halfway turn.
fn wrap_angle(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let angle = angle.rem_euclid(TAU);
    if angle > PI { angle - TAU } else { angle }
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
            collision_changes: Vec::new(),
            coverage: Coverage::default(),
            running: None,
            started: Vec::new(),
            prepared: None,
            prepared_scene_flag: None,
            three_bone: None,
            applied_light: SceneLight::default(),
            removed_npcs: BTreeSet::new(),
            kept_cast: None,
        }
    }

    /// Queues an event (a trigger's action 0x77, 0x80, 0x9b or 0x85/0x8a entry, or `--event`).
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
        // Every binding names the scene of the map that is left: the next scene reuses the
        // indices for other placements, actors, sprites and NPCs, so keeping them would hide
        // or move the new map's instances (a chained event on another map prepares its scene
        // from them). The event state that does not name the scene (events started, coverage,
        // the saved camera) stays.
        self.bindings = Bindings::default();
        self.dirty.clear();
        // The removals of action 0x7c belong to the map that is left.
        self.removed_npcs.clear();
        // The new map's collision is built from the map, not from the old placements.
        self.collision_changes.clear();
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

    /// The placement changes the scripts made that the collision must apply (natives 574
    /// and 575 on a collision mesh). Drained by the application after each event step.
    pub fn take_collision_changes(&mut self) -> Vec<PlacementChange> {
        std::mem::take(&mut self.collision_changes)
    }

    /// Applies one placement change now: a trigger action 0x95/0x96 hides or shows a map
    /// placement, and the drawn instances and the collision both follow, as for natives
    /// 574/575 (`02013390`). Used for trigger requests, outside the cutscene queue.
    pub fn set_placement(
        &mut self,
        scene: &mut AnimatedScene,
        collision: &mut CollisionWorld,
        change: PlacementChange,
    ) {
        // The collision first: it also holds the placement hierarchy the drawn instances
        // follow (a hide reaches the whole subtree).
        apply_collision_changes(collision, vec![change]);
        self.apply_placement_change(scene, collision, change);
    }

    /// The doors the scripts opened, with the angle both the render and the collision must
    /// apply (a rotation added to the placement's home transform, 0 to +π/2).
    #[allow(dead_code)] // the collision side (field worker) reads the same state
    pub fn doors(&self) -> &[dq9_core::cutscene::Door] {
        self.cutscene.doors()
    }

    /// Action 0x7c: NPC `npc` leaves the map (the object's flag `0x8000`). Its cast instance is
    /// hidden now and is not seeded or placed by the next scripts (`seed`, `present`,
    /// `restore_npcs` skip it) until the cast is built again without a snapshot that keeps
    /// the removal ([`EventPlayer::cast_loaded`]).
    pub fn remove_npc(&mut self, scene: &mut AnimatedScene, npc: u16) {
        self.removed_npcs.insert(npc);
        match scene.npc_instance(npc) {
            Some(instance) => self.set_world(scene, instance, Mat4::from_scale(Vec3::ZERO)),
            None => self
                .coverage
                .miss(format!("NPC {npc} not in the map's cast")),
        }
    }

    /// An event that keeps the cast aside starts (`ov_001::02154f24`, event-list flags bit 0
    /// or bit 2): `0x0203e0b0` records every NPC of the table for the map `map` before
    /// `ov_017::021a316c` tears the cast down. The teardown itself is not modelled: the map's
    /// NPCs stay drawn while the event plays.
    pub fn keep_cast(&mut self, scene: &AnimatedScene, map: u16) {
        let ids: BTreeSet<u16> = scene
            .scene
            .instances
            .iter()
            .filter(|i| i.block == usize::MAX)
            .filter_map(|i| u16::try_from(i.instance).ok())
            .collect();
        let mut npcs = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(instance) = scene.npc_instance(id) else {
                continue;
            };
            let removed = self.removed_npcs.contains(&id);
            let world = scene.scene.instances[instance].world;
            let (position, angle) = if removed {
                // A removed NPC's pose is not drawn again: the restore hides it.
                (Vec3::ZERO, 0.0)
            } else if !scene.scene.instances[instance].drawn() {
                // Hidden otherwise, its pose cannot be read: the rebuild keeps the file's.
                continue;
            } else {
                let (_, rotation, position) = world.to_scale_rotation_translation();
                let angle = scene
                    .npc_sprite_angle(id)
                    .unwrap_or_else(|| rotation.to_euler(EulerRot::YXZ).0);
                (position.map(kept_coordinate), kept_angle(angle))
            };
            npcs.push(KeptNpc {
                id,
                position,
                angle,
                removed,
            });
        }
        self.kept_cast = Some(KeptCast { map, npcs });
    }

    /// The map's cast was torn down and built again on map `map` (`ov_017::021a2fa0`: trigger
    /// action 0x8d or the end of an event that kept the cast aside). `ov_017::021a316c` empties
    /// the 32 entries of the NPC table (`0x0203cfc4`) before `Cast_LoadMap`: the flag `0x8000`
    /// action 0x7c set lives in the old table, so the removals are forgotten, then
    /// [`EventPlayer::cast_loaded`] applies the snapshot. Returns the NPCs it removes again.
    pub fn cast_rebuilt(&mut self, scene: &mut AnimatedScene, map: u16) -> Vec<u16> {
        self.removed_npcs.clear();
        // The homes of a running event name the old cast's instances, now hidden.
        self.bindings.homes.clear();
        self.cast_loaded(scene, map)
    }

    /// `Cast_LoadMap` (`ov_017::021a2c14`) built the cast of map `map` from its placement
    /// script; `0x0203e2d4` then applies the snapshot when it was taken on that map: each NPC
    /// of the new cast with a kept member number goes back to its kept position and angle,
    /// and a kept removal hides it again (flag `0x8000`). An NPC the new cast lacks is
    /// skipped, one the snapshot lacks keeps the file's pose. The snapshot is consumed
    /// whatever the map (`+0x1de = 0`). Returns the NPCs removed again.
    pub fn cast_loaded(&mut self, scene: &mut AnimatedScene, map: u16) -> Vec<u16> {
        let Some(kept) = self.kept_cast.take().filter(|kept| kept.map == map) else {
            return Vec::new();
        };
        let mut removed = Vec::new();
        for npc in kept.npcs {
            let Some(instance) = scene.npc_instance(npc.id) else {
                continue;
            };
            if npc.removed {
                self.removed_npcs.insert(npc.id);
                self.set_world(scene, instance, Mat4::from_scale(Vec3::ZERO));
                removed.push(npc.id);
                continue;
            }
            let world = if scene.npc_sprite_angle(npc.id).is_some() {
                // Sprites are billboards: the angle picks the side they show.
                scene.set_npc_angle(npc.id, npc.angle);
                Mat4::from_translation(npc.position)
            } else {
                let (scale, _, _) = scene.scene.instances[instance]
                    .world
                    .to_scale_rotation_translation();
                Mat4::from_scale_rotation_translation(
                    scale,
                    Quat::from_rotation_y(npc.angle),
                    npc.position,
                )
            };
            self.set_world(scene, instance, world);
        }
        removed
    }

    /// The event running now (`None` between scenes), for the checkpoints.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn current(&self) -> Option<u16> {
        self.running
    }

    /// Scene flag of the event prepared by [`EventPlayer::prepare`], from the event list it
    /// already loaded for the scan.
    pub fn prepared_scene_flag(&self) -> Option<u16> {
        self.prepared_scene_flag
    }

    /// A doorway the field touched (native 584): the shared door state both the render
    /// (`apply_placements`) and the collision (`CollisionWorld::set_door_angle`) read.
    /// Returns whether the door was added.
    pub fn open_door(&mut self, block: i32, placement: i32, sound: bool) -> bool {
        self.cutscene.open_door(block, placement, sound)
    }

    /// Another map is loaded: its doors are forgotten.
    pub fn clear_doors(&mut self) {
        self.cutscene.clear_doors();
    }

    /// One tick of the door animations when no script runs (a doorway the field opened):
    /// advances the angles and moves the scene placements with them.
    pub fn tick_doors(&mut self, stage: &mut Stage) {
        if self.cutscene.doors().is_empty() {
            return;
        }
        self.cutscene.tick_doors();
        self.apply_placements(stage);
    }

    /// Whether the DS display engines are swapped (natives 109/110): the field and the message
    /// window must be drawn on the other screen of the layout.
    pub fn screen_swapped(&self) -> bool {
        self.cutscene.screen_swapped()
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
        self.three_bone = None;
        self.prepared = Some(event);
        let scanned = scan(rom, event, stage.world);
        self.prepared_scene_flag = scanned.scene_flag;
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
            // A removed NPC has no node: `present` would otherwise place it again.
            if u16::try_from(id).is_ok_and(|id| self.removed_npcs.contains(&id)) {
                continue;
            }
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
            // An NPC removed after its home was recorded (action 0x7c during the scene) stays
            // away.
            if stage.scene.scene.instances.get(index).is_some_and(|i| {
                u16::try_from(i.instance).is_ok_and(|id| self.removed_npcs.contains(&id))
            }) {
                continue;
            }
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
        // Natives 409/411/413 (02066d00, 0206b6c8, 02045608): the caption's alpha envelope
        // and its layout flags, until the next 400 resets them.
        let caption = self.cutscene.caption();
        stage
            .talk
            .set_event_caption_alpha(self.cutscene.caption_alpha());
        stage.talk.set_event_centered(caption.centered);
        stage.talk.set_event_silent(caption.silent);
        stage.talk.set_event_frozen(caption.time.is_some());
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
            Event::SetQuestSignal(value) => outcomes.push(Outcome::SetQuestSignal(*value)),
            Event::OpenDoor {
                block,
                placement,
                sound: true,
                ..
            } => outcomes.push(Outcome::DoorSound {
                block: *block,
                placement: *placement,
            }),
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
        self.three_bone = None;
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
                    let Some(id) = u16::try_from(id).ok() else {
                        continue;
                    };
                    if self.removed_npcs.contains(&id) {
                        continue;
                    }
                    let Some(instance) = stage.scene.npc_instance(id) else {
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
                        stage.scene.set_npc_angle(id, node.pose.rotation.y);
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
        let mut foot = None;
        if let Some(bones) = &view.three_bone
            && let Some((e, t, f)) = self.three_bone_camera(stage.scene, bones)
        {
            (eye, target, foot) = (e, t, Some(f));
        }
        if (target - eye).length() > 1e-3 {
            stage.camera.look_at(eye, target);
        }
        stage.camera.fovy = (2.0 * view.half_fov).to_radians();
        stage.camera.far = stage.camera.far.max(stage.camera.distance * 4.0);
        stage.camera.roll = view.roll.to_radians();
        self.drag_three_bone(stage, view.three_bone.as_ref(), foot);
    }

    /// The render actor of a model target, looked up by borrowed name (no per-frame clone of
    /// the model path).
    fn actor_of(&self, target: Target, model: Option<&str>) -> Option<usize> {
        let model = model?;
        self.bindings
            .actors
            .iter()
            .find_map(|((t, path), actor)| (*t == target && path == model).then_some(*actor))
    }

    /// Eye, target and the point the `foot` bone gives, for a camera installed by native 552.
    fn three_bone_camera(
        &self,
        scene: &AnimatedScene,
        bones: &dq9_core::cutscene::ThreeBoneCamera,
    ) -> Option<(Vec3, Vec3, Vec3)> {
        let target = self.cutscene.target_of_character(bones.source);
        let node = self.cutscene.node(target)?;
        let actor = self.actor_of(target, node.model.as_deref())?;
        // The camera rig usually has no polygon, hence no instance: its world is its pose.
        let world = world_of(&node.pose, true);
        let at = |name: &str| -> Option<Vec3> {
            let bone = scene.actor_bone(actor, name).ok()??;
            Some(world.transform_point3(bone.w_axis.truncate()))
        };
        Some((at(&bones.eye)?, at(&bones.lookat)?, at(&bones.foot)?))
    }

    /// Native 552's per-update side effect: the `target` character stands on the source's
    /// `foot` bone, and every third update its facing moves halfway to the direction it moved
    /// (`0204a170`: `(rot + atan2(dx, dz) + 1) >> 1`). One game update is two ticks.
    fn drag_three_bone(
        &mut self,
        stage: &mut Stage,
        bones: Option<&dq9_core::cutscene::ThreeBoneCamera>,
        foot: Option<Vec3>,
    ) {
        let Some(bones) = bones else {
            self.three_bone = None;
            return;
        };
        if !self.ticks.is_multiple_of(2) {
            return;
        }
        let Some(foot) = foot else { return };
        let target = self.cutscene.target_of_character(bones.target);
        let current = self.three_bone.filter(|drag| drag.target == target);
        let mut facing = current.map_or_else(
            || {
                self.cutscene
                    .node(target)
                    .map_or(0.0, |node| node.pose.rotation.y)
            },
            |drag| drag.facing,
        );
        let mut updates = current.map_or(0, |drag| drag.updates) + 1;
        if let Some(previous) = current {
            let (dx, dz) = (foot.x - previous.position.x, foot.z - previous.position.z);
            if updates >= 3 {
                if dx.hypot(dz) > 1e-5 {
                    let heading = dx.atan2(dz);
                    facing = wrap_angle((facing + heading) * 0.5);
                }
                updates = 0;
            }
        }
        self.cutscene.place_target(target, foot, facing);
        if target == Target::Hero {
            *stage.hero = Hero::at(foot, yaw_of_angle(facing));
        }
        self.three_bone = Some(ThreeBoneDrag {
            target,
            position: foot,
            facing,
            updates,
        });
    }

    /// Eye and target of a camera driven by two bones of the model bound to a slot.
    fn bone_camera(
        &self,
        scene: &AnimatedScene,
        bones: &dq9_core::cutscene::BoneCamera,
    ) -> Option<(Vec3, Vec3)> {
        let target = self.cutscene.slot_target(bones.slot)?;
        let node = self.cutscene.node(target)?;
        let actor = self.actor_of(target, node.model.as_deref())?;
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
    /// relative to it **[hypothesis]** (the scripts reset the scale of the attached models to
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
                let model = self.cutscene.node(*parent)?;
                match self.actor_of(*parent, model.model.as_deref()) {
                    Some(actor) => scene.actor_bone(actor, bone),
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
    /// keeps these changes until the zone is loaded again. Native 584 turns a door placement
    /// around its own origin while its animation runs.
    fn apply_placements(&mut self, stage: &mut Stage) {
        let doors: Vec<(i32, i32, f32)> = self
            .cutscene
            .doors()
            .iter()
            .map(|door| (door.block, door.placement, door.angle))
            .collect();
        for change in self.cutscene.take_placement_changes() {
            // The collision is the application's (it drains the queue); the drawn instances
            // follow the placement hierarchy it holds.
            self.collision_changes.push(change);
            self.apply_placement_change(stage.scene, stage.collision, change);
        }
        for (block, placement, angle) in doors {
            let instances: Vec<usize> = stage
                .scene
                .scene
                .placements
                .iter()
                .filter(|&&(entry, id, _)| {
                    i64::from(entry) == i64::from(block) && i32::from(id) == placement
                })
                .map(|&(_, _, instance)| instance)
                .collect();
            if instances.is_empty() {
                self.coverage
                    .miss(format!("door ({block}, {placement}) not in the map"));
                continue;
            }
            for instance in instances {
                let state = self
                    .bindings
                    .placements
                    .entry(instance)
                    .or_insert_with(|| Placement::of(&stage.scene.scene.instances[instance]));
                state.angle = Some(angle);
                let world = state.world();
                self.set_world(stage.scene, instance, world);
            }
        }
    }

    /// One placement change on the scene's instances; `hierarchy` is the collision's
    /// placement tree, which the drawn instances have no link to.
    fn apply_placement_change(
        &mut self,
        scene: &mut AnimatedScene,
        hierarchy: &CollisionWorld,
        change: PlacementChange,
    ) {
        let (group, id) = match change {
            PlacementChange::Hidden { group, id, .. }
            | PlacementChange::Moved { group, id, .. } => (group, id),
        };
        let (block, placement) = (group as u32, id as u16);
        let root_instances = placement_instances(&scene.scene, block, placement);
        if root_instances.is_empty()
            && !scene
                .scene
                .undrawn_placements
                .iter()
                .any(|&(entry, part)| entry == block && part == placement)
        {
            // A placement in neither list is not in the loaded map; the game records that for
            // a later load (`func_0206ea8c`), which this runtime does not model, so it is
            // reported instead of pretended applied. A collision mesh with no drawn instance
            // may still have drawn descendants below it, which the hide follows.
            self.coverage.miss(format!(
                "placement ({group}, {id}) absent from the loaded map"
            ));
        }
        match change {
            PlacementChange::Hidden { hidden, .. } => {
                self.apply_placement_hidden(scene, hierarchy, block, placement, hidden);
            }
            PlacementChange::Moved { position, .. } => {
                for instance in root_instances {
                    // The placement at the map's own transform, even when the chosen time of
                    // day hid it: native 574 shows it again where the map put it.
                    let state = self
                        .bindings
                        .placements
                        .entry(instance)
                        .or_insert_with(|| Placement::of(&scene.scene.instances[instance]));
                    state.position = Some(Vec3::from_array(position));
                    let world = state.world();
                    self.set_world(scene, instance, world);
                }
            }
        }
    }

    /// Natives 574 and actions 0x95/0x96 on the drawn scene: hides or shows placement
    /// `(block, placement)` and every placement below it in the map's hierarchy, as the
    /// collision does. The instances keep their home transform (`Placement::home`), so a show
    /// puts each one back where the map built it; a descendant stays hidden while its own
    /// state or another hidden ancestor hides it.
    fn apply_placement_hidden(
        &mut self,
        scene: &mut AnimatedScene,
        hierarchy: &CollisionWorld,
        block: u32,
        placement: u16,
        hidden: bool,
    ) {
        let root = (block, placement);
        let mut subtree = hierarchy.placement_subtree(block, placement);
        if subtree.is_empty() {
            // The hierarchy knows no such placement: the exact instance alone follows.
            subtree.push(root);
        }
        if hidden {
            self.bindings.hidden_placements.insert(root);
        } else {
            self.bindings.hidden_placements.remove(&root);
        }
        for (block, placement) in subtree {
            let is_root = (block, placement) == root;
            // A hide reaches every descendant through the root; a show restores one only when
            // no other hidden ancestor holds it.
            let ancestor_hidden =
                (hidden && !is_root) || self.hidden_ancestor(hierarchy, block, placement);
            for instance in placement_instances(&scene.scene, block, placement) {
                let state = self
                    .bindings
                    .placements
                    .entry(instance)
                    .or_insert_with(|| Placement::of(&scene.scene.instances[instance]));
                if is_root {
                    state.hidden = hidden;
                }
                state.ancestor_hidden = ancestor_hidden;
                let world = state.world();
                self.set_world(scene, instance, world);
            }
        }
    }

    /// Whether a placement above `(block, placement)` in the map's hierarchy is hidden: the
    /// placement then stays hidden when a descendant hide is shown.
    fn hidden_ancestor(&self, hierarchy: &CollisionWorld, block: u32, placement: u16) -> bool {
        hierarchy
            .placement_path(block, placement)
            .iter()
            .rev()
            .skip(1)
            .any(|key| self.bindings.hidden_placements.contains(key))
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

/// The scene instances of placement `(block, id)`: a model placement may draw one instance
/// per model of its archive.
fn placement_instances(scene: &dq9_render::Scene, block: u32, id: u16) -> Vec<usize> {
    scene
        .placements
        .iter()
        .filter(|&&(entry, placement, _)| entry == block && placement == id)
        .map(|&(_, _, instance)| instance)
        .collect()
}

/// A map placement a script hid or moved: its world before the first change and what it is
/// now.
#[derive(Debug, Clone, Copy)]
struct Placement {
    /// Home world matrix of the placement, as built from the map (`0x02013168`).
    home: Mat4,
    /// Hidden by the script or by the chosen time of day (native 574, actions 0x95/0x96): the
    /// placement's own state.
    hidden: bool,
    /// Hidden through an ancestor above it in the map's hierarchy, which the game hides as a
    /// whole (`02013390`); the placement's own state survives so a show of the ancestor does
    /// not re-show it when it was hidden by itself.
    ancestor_hidden: bool,
    position: Option<Vec3>,
    /// Rotation around the placement's own origin, radians (native 584).
    angle: Option<f32>,
}

impl Placement {
    /// The state of a placement before a script changes it: its home transform and whether
    /// the chosen time of day hides it (a null world scale). Nothing above it is hidden when
    /// the state is first recorded, so the null scale is its own.
    fn of(instance: &dq9_render::SceneInstance) -> Self {
        Self {
            home: instance.home,
            hidden: !instance.drawn(),
            ancestor_hidden: false,
            position: None,
            angle: None,
        }
    }

    /// The world matrix the scene draws: the home transform with the translation native 575
    /// last wrote, and a null scale while native 574 hides it or an ancestor hides it. Hiding,
    /// showing and moving are independent, so their order does not change the result.
    fn world(&self) -> Mat4 {
        if self.hidden || self.ancestor_hidden {
            return Mat4::from_scale(Vec3::ZERO);
        }
        let mut world = self.home;
        if let Some(angle) = self.angle {
            // The door turns around the placement's own origin; the home translation and
            // scale stay (native 584 sets the record's angle, `02018310` `+0x34`).
            world *= Mat4::from_rotation_y(angle);
        }
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

/// Real-data regressions of the placement hierarchy and of the map change (the ROM may be
/// absent: every test returns early, as the other ROM-backed tests do).
#[cfg(all(test, not(target_arch = "wasm32")))]
mod rom_tests {
    use super::*;
    use crate::load::{self, LoadedMap};
    use crate::report;
    use crate::talk::{Button, Talk};
    use dq9_core::field::initial_state;
    use dq9_formats::cast::Story;
    use dq9_formats::hero::HeroLook;

    /// The story point and look the CLI and the other ROM tests use by default: `1.1.1`,
    /// daytime (the events below were traced at that point).
    fn pinned_story() -> Story {
        Story {
            chapter: 1,
            step: 1,
            substep: 1,
            daytime: true,
            ..Story::default()
        }
    }

    /// The map of `event` at time 0, with the story and the look the other ROM tests use.
    fn load_event_map(rom: &[u8], event: u16) -> (LoadedMap, Camera, Talk) {
        let place = crate::event::place(rom, event).expect("event record");
        let spawn = place.position.map(|(p, angle)| {
            (
                glam::Vec3::from_array(p),
                dq9_core::field::yaw_of_angle(angle),
            )
        });
        let loaded = load::load(
            rom,
            &place.map.to_string(),
            0,
            &pinned_story(),
            spawn,
            None,
            &HeroLook::new_game(),
        )
        .expect("map of the event");
        let mut camera = Camera::default();
        if let Some(bounds) = loaded.runtime.scene.bounds() {
            camera.frame(bounds);
        }
        let mut talk = Talk::default();
        talk.set_rom(rom);
        (loaded, camera, talk)
    }

    /// One fixed step as the application runs it (`App::step_event`): the pending event's
    /// scene is prepared, the script and the cutscene advance, the collision takes the
    /// placement changes and the scene animates. `press` reads the messages of the script.
    #[allow(clippy::too_many_arguments)]
    fn frame(
        rom: &[u8],
        player: &mut EventPlayer,
        loaded: &mut LoadedMap,
        hero: &mut Hero,
        camera: &mut Camera,
        talk: &mut Talk,
        state: &State,
        press: bool,
    ) {
        if press {
            talk.press(Button::Confirm);
        }
        {
            let mut stage = Stage {
                rom,
                world: state,
                scene: &mut loaded.runtime,
                hero,
                collision: &loaded.collision,
                camera,
                talk,
            };
            if let Some(next) = player.pending_start() {
                player.prepare(rom, next, &mut stage);
            }
            for outcome in player.step(&mut stage) {
                // Nothing is heard here: a jingle is over as soon as it is asked for.
                if matches!(outcome, Outcome::Sound(Sound::Jingle(_))) {
                    player.jingle_ended();
                }
            }
        }
        apply_collision_changes(&mut loaded.collision, player.take_collision_changes());
        talk.step();
        loaded.runtime.step(camera).expect("scene animation");
    }

    /// Event 24590 of D04M02 hides placement `(0, 26)` at its update 2 and shows it at update
    /// 257. The placement has twenty drawn children (ids 0 to 24, parts 12 to 34) that no
    /// action of the script names: the image must follow the collision, which hides the whole
    /// subtree (`Zone3D_FindPlacement` `0x02019518`, `02013390`).
    #[test]
    fn a_scripted_hide_takes_the_drawn_children_and_the_show_restores_them() {
        let Some(rom) = testrom::load() else {
            return;
        };
        let state = initial_state(&pinned_story());
        let (mut loaded, mut camera, mut talk) = load_event_map(rom, 24590);
        let mut hero = loaded.hero;
        let mut player = EventPlayer::new();
        player.queue(24590);

        let subtree = loaded.collision.placement_subtree(0, 26);
        assert!(
            subtree.len() > 1,
            "placement (0, 26) has no drawn hierarchy: {subtree:?}"
        );
        let root = placement_instances(&loaded.runtime.scene, 0, 26);
        assert!(!root.is_empty(), "placement (0, 26) is not drawn");
        let instances: Vec<usize> = subtree
            .iter()
            .flat_map(|&(block, id)| placement_instances(&loaded.runtime.scene, block, id))
            .collect();
        assert!(
            instances.len() > root.len(),
            "placement (0, 26) has no drawn child: {subtree:?}"
        );
        let root_instances: BTreeSet<usize> = root.iter().copied().collect();
        assert!(
            root.iter()
                .any(|&instance| loaded.runtime.scene.instances[instance].drawn()),
            "placement (0, 26) is hidden at the chosen time of day"
        );
        assert!(
            instances.iter().any(|&instance| {
                !root_instances.contains(&instance)
                    && loaded.runtime.scene.instances[instance].drawn()
            }),
            "no drawn child of placement (0, 26): {subtree:?}"
        );
        let original: Vec<Mat4> = instances
            .iter()
            .map(|&instance| loaded.runtime.scene.instances[instance].world)
            .collect();
        let homes: Vec<Mat4> = instances
            .iter()
            .map(|&instance| loaded.runtime.scene.instances[instance].home)
            .collect();
        let drawn_before: Vec<bool> = instances
            .iter()
            .map(|&instance| loaded.runtime.scene.instances[instance].drawn())
            .collect();
        // A model of the same block that no action of the script names: neither the hide nor
        // the show of (0, 26) may touch it.
        let sibling = placement_instances(&loaded.runtime.scene, 0, 2);
        assert!(
            sibling
                .iter()
                .any(|&instance| loaded.runtime.scene.instances[instance].drawn()),
            "the unrelated placement (0, 2) is not drawn"
        );
        let sibling_worlds: Vec<Mat4> = sibling
            .iter()
            .map(|&instance| loaded.runtime.scene.instances[instance].world)
            .collect();

        // The script's own hide: its update 2 runs at the fourth tick.
        let mut hidden = false;
        for tick in 0..120 {
            frame(
                rom,
                &mut player,
                &mut loaded,
                &mut hero,
                &mut camera,
                &mut talk,
                &state,
                tick % 30 == 0,
            );
            if root
                .iter()
                .all(|&instance| !loaded.runtime.scene.instances[instance].drawn())
            {
                hidden = true;
                break;
            }
        }
        assert!(hidden, "event 24590 did not hide placement (0, 26)");
        for &instance in &instances {
            assert!(
                !loaded.runtime.scene.instances[instance].drawn(),
                "instance {instance} below the hidden placement (0, 26) stayed drawn"
            );
        }
        for (&instance, &world) in sibling.iter().zip(&sibling_worlds) {
            assert_eq!(
                loaded.runtime.scene.instances[instance].world, world,
                "the hide of (0, 26) changed the unrelated instance {instance}"
            );
        }

        // The script's own show, at its update 257, puts every one of them back.
        let mut shown = false;
        for tick in 0..4_000 {
            frame(
                rom,
                &mut player,
                &mut loaded,
                &mut hero,
                &mut camera,
                &mut talk,
                &state,
                tick % 30 == 0,
            );
            if root
                .iter()
                .any(|&instance| loaded.runtime.scene.instances[instance].drawn())
            {
                shown = true;
                break;
            }
        }
        assert!(shown, "event 24590 did not show placement (0, 26) again");
        for (index, &instance) in instances.iter().enumerate() {
            let drawn = loaded.runtime.scene.instances[instance].drawn();
            assert_eq!(
                drawn, drawn_before[index],
                "instance {instance} after the show"
            );
            assert_eq!(
                loaded.runtime.scene.instances[instance].world, original[index],
                "the show of (0, 26) changed the world of instance {instance}"
            );
            if drawn {
                assert_eq!(
                    loaded.runtime.scene.instances[instance].world, homes[index],
                    "instance {instance} did not come back on its home transform"
                );
            }
        }
        for (&instance, &world) in sibling.iter().zip(&sibling_worlds) {
            assert_eq!(
                loaded.runtime.scene.instances[instance].world, world,
                "the show of (0, 26) changed the unrelated instance {instance}"
            );
        }

        // The event ends; its actor instances are gone, the map's placements stay.
        for tick in 0..2_000 {
            if !player.is_active() && player.pending_start().is_none() {
                break;
            }
            frame(
                rom,
                &mut player,
                &mut loaded,
                &mut hero,
                &mut camera,
                &mut talk,
                &state,
                tick % 30 == 0,
            );
        }
        assert!(!player.is_active(), "event 24590 did not end");

        // The trigger path (actions 0x95/0x96) on the same placement: the cascade hides the
        // subtree and touches nothing else, and the show restores every instance exactly.
        let before: Vec<Mat4> = loaded
            .runtime
            .scene
            .instances
            .iter()
            .map(|instance| instance.world)
            .collect();
        let drawn: Vec<bool> = loaded
            .runtime
            .scene
            .instances
            .iter()
            .map(dq9_render::SceneInstance::drawn)
            .collect();
        let subtree_instances: BTreeSet<usize> = instances.iter().copied().collect();
        player.set_placement(
            &mut loaded.runtime,
            &mut loaded.collision,
            PlacementChange::Hidden {
                group: 0,
                id: 26,
                hidden: true,
            },
        );
        for &instance in &instances {
            assert!(
                !loaded.runtime.scene.instances[instance].drawn(),
                "action 0x95 left instance {instance} of the subtree drawn"
            );
        }
        for (instance, &world) in before.iter().enumerate() {
            if subtree_instances.contains(&instance) {
                continue;
            }
            assert_eq!(
                loaded.runtime.scene.instances[instance].world, world,
                "hiding placement (0, 26) changed unrelated instance {instance}"
            );
            assert_eq!(
                loaded.runtime.scene.instances[instance].drawn(),
                drawn[instance],
                "hiding placement (0, 26) hid unrelated instance {instance}"
            );
        }

        // A child hidden by itself stays hidden when its hidden parent is shown again.
        let child = subtree
            .iter()
            .copied()
            .find(|&(block, id)| {
                (block, id) != (0, 26)
                    && !placement_instances(&loaded.runtime.scene, block, id).is_empty()
            })
            .expect("a drawn child of placement (0, 26)");
        let child_instances = placement_instances(&loaded.runtime.scene, child.0, child.1);
        player.set_placement(
            &mut loaded.runtime,
            &mut loaded.collision,
            PlacementChange::Hidden {
                group: child.0 as i32,
                id: child.1 as i32,
                hidden: true,
            },
        );
        player.set_placement(
            &mut loaded.runtime,
            &mut loaded.collision,
            PlacementChange::Hidden {
                group: 0,
                id: 26,
                hidden: false,
            },
        );
        for &instance in &child_instances {
            assert!(
                !loaded.runtime.scene.instances[instance].drawn(),
                "the show of (0, 26) re-showed instance {instance} of {child:?}, hidden by itself"
            );
        }
        for &instance in &root {
            assert!(
                loaded.runtime.scene.instances[instance].drawn(),
                "the show of (0, 26) left its own instance {instance} hidden"
            );
        }
        player.set_placement(
            &mut loaded.runtime,
            &mut loaded.collision,
            PlacementChange::Hidden {
                group: child.0 as i32,
                id: child.1 as i32,
                hidden: false,
            },
        );
        for (instance, &world) in before.iter().enumerate() {
            assert_eq!(
                loaded.runtime.scene.instances[instance].world, world,
                "the hide/show cycle changed instance {instance}"
            );
        }

        // A show of the child while its parent stays hidden does not bring it back; the
        // parent's show does.
        player.set_placement(
            &mut loaded.runtime,
            &mut loaded.collision,
            PlacementChange::Hidden {
                group: 0,
                id: 26,
                hidden: true,
            },
        );
        player.set_placement(
            &mut loaded.runtime,
            &mut loaded.collision,
            PlacementChange::Hidden {
                group: child.0 as i32,
                id: child.1 as i32,
                hidden: false,
            },
        );
        for &instance in &child_instances {
            assert!(
                !loaded.runtime.scene.instances[instance].drawn(),
                "instance {instance} of {child:?} showed under its hidden parent (0, 26)"
            );
        }
        player.set_placement(
            &mut loaded.runtime,
            &mut loaded.collision,
            PlacementChange::Hidden {
                group: 0,
                id: 26,
                hidden: false,
            },
        );
        for (instance, &world) in before.iter().enumerate() {
            assert_eq!(
                loaded.runtime.scene.instances[instance].world, world,
                "the second hide/show cycle changed instance {instance}"
            );
        }
    }

    /// A chained event on another map must not apply the left map's scene indices to the new
    /// scene. Event 9210 (M09M02, 7 instances, one NPC) -> 9220 (M09M04, 22) leaves the saved
    /// home of that NPC at index 5, inside the new scene; event 9710 (M05, 35 instances, 15
    /// NPCs) -> 9713 (O00, 31) leaves their homes at indices 19 to 33, inside it too. The
    /// application loads the new map, calls `map_loaded` and prepares the event.
    #[test]
    fn a_chained_event_on_another_map_ignores_the_previous_map_bindings() {
        let Some(rom) = testrom::load() else {
            return;
        };
        let story = pinned_story();
        let state = initial_state(&story);
        for &(event, next) in &[(9210u16, 9220u16), (9710, 9713)] {
            let (mut loaded, mut camera, mut talk) = load_event_map(rom, event);
            let cast: Vec<usize> = loaded
                .runtime
                .scene
                .instances
                .iter()
                .enumerate()
                .filter(|(_, instance)| {
                    instance.block == usize::MAX && instance.instance != usize::MAX
                })
                .map(|(index, _)| index)
                .collect();
            let mut hero = loaded.hero;
            let mut player = EventPlayer::new();
            player.queue(event);
            let played = report::play(
                rom,
                &mut loaded,
                &mut hero,
                &mut camera,
                &mut talk,
                &mut player,
                &state,
                None,
                &[],
                Some(60),
            )
            .expect("playback of the event");
            assert_eq!(
                played.elsewhere,
                Some(next),
                "event {event} must chain to {next} on another map"
            );
            assert_eq!(player.pending_start(), Some(next));
            assert!(
                !cast.is_empty(),
                "event {event}: the map has no NPC whose home could carry over"
            );

            let place = crate::event::place(rom, next).expect("place of the chained event");
            let spawn = place.position.map(|(p, angle)| {
                (
                    glam::Vec3::from_array(p),
                    dq9_core::field::yaw_of_angle(angle),
                )
            });
            let mut there = load::load(
                rom,
                &place.map.to_string(),
                0,
                &story,
                spawn,
                None,
                &HeroLook::new_game(),
            )
            .expect("map of the chained event");
            // A stale home index must reach the new scene, or the test would prove nothing.
            assert!(
                cast.iter()
                    .any(|&index| index < there.runtime.scene.instances.len()),
                "event {event}: none of the {} NPC homes reaches the {} instances of {}",
                cast.len(),
                there.runtime.scene.instances.len(),
                there.entry.code
            );
            let before: Vec<(Mat4, Mat4, bool)> = there
                .runtime
                .scene
                .instances
                .iter()
                .map(|instance| (instance.world, instance.home, instance.drawn()))
                .collect();
            let mut there_hero = there.hero;
            player.map_loaded(&camera);
            {
                let mut stage = Stage {
                    rom,
                    world: &state,
                    scene: &mut there.runtime,
                    hero: &mut there_hero,
                    collision: &there.collision,
                    camera: &mut camera,
                    talk: &mut talk,
                };
                player.prepare(rom, next, &mut stage);
            }
            for (index, &(world, home, drawn)) in before.iter().enumerate() {
                let instance = &there.runtime.scene.instances[index];
                assert_eq!(
                    instance.world, world,
                    "instance {index} of {} moved by the bindings of event {event}",
                    there.entry.code
                );
                assert_eq!(
                    instance.home, home,
                    "instance {index} of {} changed home transform after event {event}",
                    there.entry.code
                );
                assert_eq!(
                    instance.drawn(),
                    drawn,
                    "instance {index} of {} hid on the bindings of event {event}",
                    there.entry.code
                );
            }
        }
    }
}
