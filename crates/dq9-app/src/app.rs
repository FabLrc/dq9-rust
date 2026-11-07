//! Window, input and frame loop, shared by the native and Web builds.
//!
//! VBlanks run at a fixed 60 Hz (the DS frame rate) from an accumulator; rendering happens
//! once per redraw. The field logic (hero, field task, gait) runs every second VBlank with
//! `ticks = 2`, the original's 30 Hz game update; the other tasks stay at 60 Hz.
//!
//! Controls: drag with the left button to turn, wheel to zoom, `WASD`/arrows to move (the hero
//! in orbit mode, the camera in fly mode), `Q`/`E` the game's `L`/`R` camera buttons in orbit
//! mode (both together return the camera) and down/up in fly mode, `Shift` faster camera, `Tab`
//! orbit/fly, `F` reframe, `R` cycles the DS screen layout (`screens.rs`), `T` next
//! time of day, `M` mute the music, `PageUp`/`PageDown` previous/next map, `Enter`/`Space` talk to the NPC in front
//! of the hero and advance the message window, `Backspace`/`X` answer No.

use std::collections::HashSet;
use std::sync::Arc;

use crate::screens::{self, Layout};
use dq9_core::camera::FieldCamera;
use dq9_core::cutscene::{ClockCommand, PlacementChange};
use dq9_core::field::{self, Cause, Event, Field, Leader, MapChange};
use dq9_core::fx::reduce_angle;
use dq9_core::scenario::{Journal, Step};
use dq9_core::state::{DayClock, GameState, TimeOfDay};
use dq9_core::trigger::{Raw, Request, State};
use dq9_core::{CollisionWorld, Hero, Input};
use dq9_formats::hero::HeroLook;
use dq9_render::{AnimatedScene, Camera, CameraInput, CameraMode, Gait, Panel, Renderer};
use glam::{Vec2, Vec3};
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::audio::Audio;
use crate::event::{EventPlayer, Outcome, Stage, apply_collision_changes};
use crate::gpu::Gpu;
use crate::lighting;
use crate::load::{self, MapEntry};
use crate::talk::{Button, Talk};

#[cfg(not(target_arch = "wasm32"))]
mod auto;
mod flow;

/// Fixed update step (60 Hz, the DS frame rate).
pub const STEP: f32 = 1.0 / 60.0;

/// Events delivered to the loop from outside (async GPU setup, ROM/save picked on the web).
#[derive(Debug)]
pub enum UserEvent {
    Gpu(Box<Gpu>),
    GpuFailed(String),
    /// ROM picked in the page (the native build reads it before starting).
    #[cfg(target_arch = "wasm32")]
    Rom(Vec<u8>),
    /// Save picked in the page: it selects the map, the position and the time of day.
    #[cfg(target_arch = "wasm32")]
    Save(Vec<u8>),
    /// Time of day picked in the page (0 to 3).
    #[cfg(target_arch = "wasm32")]
    Time(u32),
    /// Layout picked in the page.
    #[cfg(target_arch = "wasm32")]
    Layout(Layout),
    /// "Nouvelle partie" clicked in the page: starts the prologue (now, or once the ROM is read).
    #[cfg(target_arch = "wasm32")]
    NewGame,
}

/// Start-up options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Explicit map override; save map is used if absent.
    pub map: Option<String>,
    pub save: Option<Vec<u8>>,
    /// Time of day of the map assembly (0-3); the save's clock phase, else 0, when absent.
    pub time: Option<u32>,
    /// Explicit second of the 420-second day (`--clock`); its phase replaces `time`.
    pub clock: Option<f32>,
    /// Which DS screens are shown and where (`screens.rs`).
    pub layout: Layout,
    /// No music, and no sound device opened until `M` unmutes.
    pub mute: bool,
    /// Mix the sound offline (no device) so that `App::take_recording` can write it.
    pub record_sound: bool,
    /// Record the pad and action key of every fixed step (`--trace-inputs`).
    pub trace_inputs: bool,
    /// An event to start as soon as the map is loaded (`--event`).
    pub event: Option<u16>,
    pub story: dq9_formats::cast::Story,
    /// Explicit chapter/step/substep overrides for a loaded save.
    pub story_override: [Option<u8>; 3],
    /// Start a new game: the first scene of the story (`docs/app.md`, § Nouvelle partie).
    pub new_game: bool,
}

/// Writes a flag set as a JSON number array, without a per-flag allocation.
#[cfg(not(target_arch = "wasm32"))]
fn write_flag_list(out: &mut String, flags: &std::collections::BTreeSet<i32>) {
    use std::fmt::Write as _;
    for (index, flag) in flags.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{flag}");
    }
}

/// Reads the item id → compendium index tables (`data/prm/itemdt_<category>.gp2`,
/// `docs/re/item.md`) of one language. On a missing or malformed member the table stops
/// there: item flags are then not raised rather than guessed.
fn item_table(rom: &[u8]) -> dq9_formats::item::ItemTable {
    use dq9_formats::item::{CATEGORIES, ItemTable};
    let Ok(image) = nds_rom::Rom::parse(rom) else {
        return ItemTable::default();
    };
    let mut table = ItemTable::default();
    for category in CATEGORIES {
        let path = format!("data/prm/itemdt_{category}.gp2");
        let member = image.file_by_path(&path).ok().and_then(|file| {
            let archive = dq9_formats::Gpc2::parse(file).ok()?;
            let name = format!("itemdt_{category}_en.nat");
            let entry = archive.entries.iter().find(|e| e.name == name)?;
            archive.member(entry).ok()
        });
        let Some(member) = member else {
            status(&format!("item table {path}: no {category} member"));
            return table;
        };
        if let Err(e) = table.add_member(&member) {
            status(&format!("item table {path}: {e}"));
            return table;
        }
    }
    table
}

/// The d-pad bits of an [`Input`] (bit 0 up, 1 down, 2 left, 3 right).
fn input_bits(input: Input) -> u8 {
    u8::from(input.up)
        | u8::from(input.down) << 1
        | u8::from(input.left) << 2
        | u8::from(input.right) << 3
}

/// The d-pad keys of a replay frame (bit 0 up, 1 down, 2 left, 3 right).
fn pad_input(bits: u8) -> Input {
    Input {
        up: bits & 1 != 0,
        down: bits & 2 != 0,
        left: bits & 4 != 0,
        right: bits & 8 != 0,
    }
}

/// The world direction the d-pad asks for with the game's camera at yaw 0, diagonal first as
/// `Hero_ReadDirectionInput` does.
fn pad_direction(bits: u8) -> Vec3 {
    let (up, down, left, right) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
    let direction = match (up, down, left, right) {
        (true, _, true, _) => Vec3::new(-1.0, 0.0, -1.0),
        (true, _, _, true) => Vec3::new(1.0, 0.0, -1.0),
        (_, true, true, _) => Vec3::new(-1.0, 0.0, 1.0),
        (_, true, _, true) => Vec3::new(1.0, 0.0, 1.0),
        (true, _, _, _) => Vec3::NEG_Z,
        (_, true, _, _) => Vec3::Z,
        (_, _, true, _) => Vec3::NEG_X,
        (_, _, _, true) => Vec3::X,
        _ => Vec3::ZERO,
    };
    direction.normalize_or_zero()
}

/// Prints a status line (stderr natively, the console and `#status` element on the web).
pub fn status(message: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{message}");
    #[cfg(target_arch = "wasm32")]
    crate::web::status(message);
}

pub struct App {
    /// The event loop proxy (window/web paths); `None` for the native headless tests and
    /// callers that never deliver a `UserEvent`.
    proxy: Option<EventLoopProxy<UserEvent>>,
    options: Options,
    /// Time of day in use (`LightingManager+0x98`).
    time: u32,
    /// The one day clock (a save's, else the start of the night): drives the lighting.
    day: DayClock,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    rom: Option<Vec<u8>>,
    maps: Vec<MapEntry>,
    current: Option<(MapEntry, String)>,
    /// CPU state is retained after upload; native and Web advance the same animation clock.
    runtime: Option<AnimatedScene>,
    collision: CollisionWorld,
    /// Regions and trigger file of the current map (`dq9_core::field`).
    field: Option<Field>,
    /// Scenario trigger state, kept across maps.
    state: State,
    /// Item id → compendium index (`itemdt_*.gp2`), for the "item obtained" flags of
    /// [`dq9_core::state::item_obtained_flag`].
    item_table: dq9_formats::item::ItemTable,
    /// Map changes chained since the last fixed step (guards against warp loops).
    chain: u8,
    hero: Option<Hero>,
    /// Direction the headless pilot (`app/auto.rs`) asks the d-pad to walk, in world space.
    /// Overrides the keyboard for one fixed step; the camera turn is left alone.
    pilot_dir: Option<Vec3>,
    /// Doors whose opening already fired the arrival trigger.
    doors_opened: std::collections::BTreeSet<(i32, i32)>,
    /// Door angle last applied to the collision, to avoid redoing it every tick.
    door_angles: std::collections::BTreeMap<(i32, i32), i32>,
    /// NPC conversations and the message window.
    talk: Talk,
    /// Script events: the event task and what it did to the scene.
    events: EventPlayer,
    /// Set while an event loads its map: the map's own `Request::Event`s are not queued.
    quiet_events: bool,
    /// Placeholder of the bottom DS screen, created with the surface format.
    panel: Option<Panel>,
    /// What the story did since the start, and the fixed step it did it at.
    journal: Journal,
    /// JSONL snapshots of the story state at the checkpoints the oracle compares.
    #[cfg(not(target_arch = "wasm32"))]
    checkpoints: Vec<String>,
    /// Pad bits and action key of every fixed step (`--trace-inputs`).
    input_trace: Vec<(u8, bool)>,
    /// Pad bits a replay frame asks for, consumed by one fixed step.
    replay_pad: Option<u8>,
    tick: u64,
    hero_instance: Option<usize>,
    /// The protagonist's look (the save's hero, else a new game's), kept across maps.
    look: HeroLook,
    renderer: Option<Renderer>,
    audio: Audio,
    /// Lighting, fog and background of the loaded map at the selected time of day.
    environment: dq9_render::Environment,
    /// The lighting script of the loaded map (`None` without one).
    lighting_info: Option<dq9_formats::lighting::LightingInfo>,
    camera: Camera,
    /// The game's camera turn (`L`/`R`) and the yaw it is measured from: the one the map was
    /// entered with.
    field_camera: FieldCamera,
    camera_home: f32,
    /// Milliseconds of fixed steps not yet given to the camera turn.
    clock_ms: f32,
    framed: Option<[glam::Vec3; 2]>,
    held: HashSet<KeyCode>,
    cursor: Option<(f64, f64)>,
    dragging: bool,
    rotate: Vec2,
    zoom: f32,
    last: Instant,
    accumulator: f32,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("options", &self.options)
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>, options: Options, rom: Option<Vec<u8>>) -> Self {
        Self::with_proxy(Some(proxy), options, rom)
    }

    /// The same application without an event loop proxy: no `resumed`/GPU setup, no
    /// `UserEvent` delivery. The native headless tests use it so they never depend on winit
    /// accepting a second `EventLoop` in the same process.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_headless(options: Options, rom: Option<Vec<u8>>) -> Self {
        Self::with_proxy(None, options, rom)
    }

    fn with_proxy(
        proxy: Option<EventLoopProxy<UserEvent>>,
        options: Options,
        rom: Option<Vec<u8>>,
    ) -> Self {
        let state = field::initial_state(&options.story);
        let mut audio = Audio::new(options.mute);
        if options.record_sound {
            audio.record();
        }
        let mut app = Self {
            proxy,
            options,
            time: 0,
            day: DayClock::new(0.0, true),
            window: None,
            gpu: None,
            rom: None,
            maps: Vec::new(),
            current: None,
            runtime: None,
            collision: CollisionWorld::default(),
            field: None,
            state,
            item_table: dq9_formats::item::ItemTable::default(),
            chain: 0,
            hero: None,
            pilot_dir: None,
            doors_opened: std::collections::BTreeSet::new(),
            door_angles: std::collections::BTreeMap::new(),
            talk: Talk::default(),
            events: EventPlayer::new(),
            quiet_events: false,
            panel: None,
            journal: Journal::default(),
            #[cfg(not(target_arch = "wasm32"))]
            checkpoints: Vec::new(),
            input_trace: Vec::new(),
            replay_pad: None,
            tick: 0,
            hero_instance: None,
            look: HeroLook::new_game(),
            renderer: None,
            audio,
            environment: dq9_render::Environment::default(),
            lighting_info: None,
            camera: Camera::default(),
            field_camera: FieldCamera::default(),
            camera_home: 0.0,
            clock_ms: 0.0,
            framed: None,
            held: HashSet::new(),
            cursor: None,
            dragging: false,
            rotate: Vec2::ZERO,
            zoom: 0.0,
            last: Instant::now(),
            accumulator: 0.0,
        };
        if let Some(rom) = rom {
            app.set_rom(rom);
        }
        app
    }

    /// Replaces the framing view.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn look_at(&mut self, eye: glam::Vec3, target: glam::Vec3) {
        self.camera.look_at(eye, target);
    }

    /// An external ROM, save or new game replaces the old event and dialogue together.
    fn reset_session(&mut self) {
        self.events = EventPlayer::new();
        self.talk.reset_session();
        self.doors_opened.clear();
        self.door_angles.clear();
    }

    fn set_rom(&mut self, rom: Vec<u8>) {
        match load::maps(&rom) {
            Ok(maps) => self.maps = maps,
            Err(e) => {
                status(&format!("not a Dragon Quest IX ROM: {e:#}"));
                return;
            }
        }
        // No task or caption from the old session may run over the replacement save.
        self.reset_session();
        self.item_table = item_table(&rom);
        self.talk.set_rom(&rom);
        self.rom = Some(rom);
        self.state = field::initial_state(&self.options.story);
        self.state.compendium = self.item_table.clone();
        if self.options.new_game {
            self.start_new_game();
            return;
        }
        let start = match load::start(self.options.map.as_deref(), self.options.save.as_deref()) {
            Ok(start) => start,
            Err(e) => {
                status(&format!("{e:#}"));
                return;
            }
        };
        self.talk.set_identity(start.identity);
        self.state = load::trigger_state(
            start.state.as_ref(),
            &self.options.story,
            self.options.story_override,
        );
        self.state.compendium = self.item_table.clone();
        self.day = lighting::day_clock(
            start.state.as_ref().map(|s| s.clock),
            self.options.time,
            self.options.clock,
        );
        self.time = self.day.phase().index();
        self.look = start.look.clone();
        self.load(&start.map, true, None, start.state.as_ref());
        if let Some(event) = self.options.event {
            self.events.queue(event);
        }
    }

    #[cfg(any(target_arch = "wasm32", all(test, windows)))]
    fn set_save(&mut self, save: Vec<u8>) {
        // Picking a save replaces whatever the page started with (a new game from
        // `?newgame=1`, a map preview): the save selects the map, the position and the
        // scenario. Without this, `set_rom` would keep starting the prologue.
        self.options.new_game = false;
        self.options.map = None;
        self.options.event = None;
        self.options.save = Some(save);
        self.talk.set_identity(None);
        if let Some(rom) = self.rom.take() {
            self.set_rom(rom);
        }
    }

    /// What the running events did to the lighting and to the day clock (natives 578 to 589,
    /// 548, 549, 804, 808): the renderer's environment follows, the clock obeys.
    fn apply_event_lighting(&mut self) {
        for command in self.events.take_clock_commands() {
            match command {
                ClockCommand::Running(running) => self.day.set_running(running),
                ClockCommand::SetPhase(phase) => {
                    self.day.set_phase(lighting::phase(phase as u32));
                    self.time = self.day.phase().index();
                }
            }
        }
        if let Some(environment) = self
            .events
            .environment(self.lighting_info.as_ref(), self.day)
        {
            if let Some(renderer) = &mut self.renderer {
                renderer.environment = environment.clone();
            }
            self.environment = environment;
        }
    }

    /// Shows the current map at another time of day.
    fn set_time(&mut self, phase: TimeOfDay) {
        self.day.set_phase(phase);
        self.time = phase.index();
        if let Some((entry, _)) = &self.current {
            let code = entry.code.clone();
            self.load(&code, false, None, None);
        }
    }

    /// Loads map `key` at the current time of day; `frame` resets the camera onto it. `state`
    /// (a restored save) places the hero on its own map when there is no `spawn`.
    fn load(
        &mut self,
        key: &str,
        frame: bool,
        spawn: Option<(Vec3, f32)>,
        state: Option<&GameState>,
    ) -> bool {
        self.load_map(key, frame, spawn, state, false)
    }

    /// [`App::load`]; `transition` is set when the hero arrives through a door or a warp,
    /// which runs the arrival triggers. Returns whether the map loaded.
    fn load_map(
        &mut self,
        key: &str,
        frame: bool,
        spawn: Option<(Vec3, f32)>,
        state: Option<&GameState>,
        transition: bool,
    ) -> bool {
        let story = self.current_story(key);
        let Some(rom) = &self.rom else { return false };
        let started = Instant::now();
        let mut loaded = match load::load(rom, key, self.time, &story, spawn, state, &self.look) {
            Ok(l) => l,
            Err(e) => {
                status(&format!("{e:#}"));
                return false;
            }
        };
        let hero_start = loaded.hero.position();
        self.environment = lighting::environment(loaded.lighting.as_ref(), self.day);
        self.lighting_info = loaded.lighting.clone();
        let previous = self.current.as_ref().map(|(entry, _)| entry.id);
        self.collision = loaded.collision;
        // The collision was rebuilt: even on the same map, every open door must be applied
        // again. A real map change also forgets the doors and their triggers.
        self.door_angles.clear();
        if previous != Some(loaded.entry.id) {
            self.events.clear_doors();
            self.doors_opened.clear();
        }
        // A same-map reload keeps the doors open (`events.doors()` survived), so the freshly
        // built collision must be turned to their current angles before anything (the pilot's
        // walkable grid, the first hero step) queries it. Otherwise the doors read as closed
        // until the next `step_doors`, and a grid built in that window never reopens.
        let doors: Vec<(i32, i32, i32)> = self
            .events
            .doors()
            .iter()
            .map(|door| (door.block, door.placement, door.angle_fx()))
            .collect();
        for (block, placement, angle) in doors {
            self.collision
                .set_door_angle(block as u32, placement as u16, angle);
            self.door_angles.insert((block, placement), angle);
        }
        self.field = Some(loaded.field);
        self.talk.enter_map(loaded.npcs);
        self.hero = Some(loaded.hero);
        self.hero_instance = Some(loaded.hero_instance);
        let s = &loaded.runtime.scene;
        status(&format!(
            "{} {} ({}), time {}: {} models, {} textures, {} instances, {} triangles, {:.0} ms",
            loaded.entry.id,
            loaded.entry.code,
            loaded.label,
            loaded.time,
            s.models.len(),
            s.textures.len(),
            s.instances.len(),
            s.triangle_count(),
            started.elapsed().as_secs_f64() * 1000.0
        ));
        for w in &s.warnings {
            status(&format!("  warning: {w}"));
        }
        if frame || self.framed.is_none() {
            if self.camera.mode == CameraMode::Orbit {
                self.camera = Camera::field(
                    hero_start,
                    if loaded.placed {
                        loaded.hero.yaw()
                    } else {
                        0.0
                    },
                );
            } else {
                if let Some(bounds) = s.bounds() {
                    self.camera.frame(bounds);
                }
                self.camera.target = hero_start;
                if loaded.placed {
                    self.camera.yaw = loaded.hero.yaw();
                }
            }
            self.framed = s.bounds();
        } else {
            self.camera.follow_hero(hero_start);
        }
        self.camera_home = self.camera.yaw;
        self.field_camera = FieldCamera::default();
        self.events.map_loaded(&self.camera);
        let flags = |flag: u16| self.state.flags.contains(&i32::from(flag));
        self.audio.set_map(rom, loaded.entry.id, self.time, &flags);
        self.journal.record(
            self.tick,
            Step::Map {
                id: loaded.entry.id,
                code: loaded.entry.code.clone(),
            },
        );
        // `0201ba78` (map visited): the original sets `2710 + map / 100` when the map loads,
        // before its triggers run.
        if let Some(flag) = dq9_core::state::map_visit_flag(loaded.entry.id) {
            self.state.flags.insert(i32::from(flag));
        }
        self.current = Some((loaded.entry, loaded.label));
        if let Err(e) = loaded.runtime.face_camera(&self.camera) {
            status(&format!("facing cast: {e}"));
            return true;
        }
        self.runtime = Some(loaded.runtime);
        self.upload();
        self.update_title();
        if let Some(field) = &mut self.field {
            let events = field.enter(&mut self.state, transition);
            self.apply_events(events);
        }
        true
    }

    /// Action 0xb1 (`0x6e`) of a trigger: the field changed the time of day, and the
    /// application's clock and lighting follow before anything else in the same event list.
    /// Fired before a `MapChange`, it makes the destination map load at the new time.
    ///
    /// `--time`/`--clock` keep the priority for the oracle captures. The map is **not**
    /// reloaded: `set_time` would rebuild the scene and replay its entry events; a same-map
    /// change only moves the environment.
    fn sync_day_phase(&mut self) {
        if self.options.time.is_some() || self.options.clock.is_some() {
            return;
        }
        if self.state.time_of_day == self.day.phase().index() {
            return;
        }
        self.day.set_phase(lighting::phase(self.state.time_of_day));
        self.time = self.day.phase().index();
        self.environment = lighting::environment(self.lighting_info.as_ref(), self.day);
        if let Some(renderer) = &mut self.renderer {
            renderer.environment = self.environment.clone();
        }
    }

    /// Carries out what the field asks for: the first map change (loading its destination
    /// at the arrival pose), and logs the trigger requests that wait for J3. Returns whether
    /// the map changed.
    fn apply_events(&mut self, events: Vec<Event>) -> bool {
        self.sync_day_phase();
        let mut changed = false;
        for event in events {
            match event {
                Event::MapChange(change) if !changed => {
                    changed = self.change_map(&change);
                }
                Event::MapChange(change) => {
                    status(&format!(
                        "map change ignored (already changing): {change:?}"
                    ));
                }
                Event::Request(Request::Event(id)) if !self.quiet_events => {
                    self.events.queue(id);
                }
                Event::Request(Request::Event(id)) => {
                    status(&format!("event {id} ignored while an event loads its map"));
                }
                Event::Request(Request::Battle(number)) => self.run_battle(number),
                Event::Request(Request::RemoveNpc(npc)) => self.remove_npc(npc),
                Event::Request(Request::SetPlacementHidden {
                    block,
                    placement,
                    hidden,
                }) => self.set_placement_hidden(block, placement, hidden),
                Event::Request(Request::OpenDoor { block, placement }) => {
                    self.open_door(u32::from(block), placement)
                }
                Event::Request(Request::Talk { npc, context }) => self.talk_to(npc, context),
                Event::Request(request) => {
                    status(&format!(
                        "trigger request not supported before J3: {request:?}"
                    ));
                    self.journal
                        .record(self.tick, Step::Unhandled(format!("{request:?}")));
                }
                Event::Unsupported(code) => {
                    status(&format!("trigger condition {code:#x} is not modelled"));
                }
            }
        }
        changed
    }

    /// Action 0x7c: the NPC leaves the map. It stops being a talk target and its cast
    /// instance is hidden in the scene (the object's flag `0x8000`, `02013380`); the event
    /// player keeps it hidden through the following script updates.
    fn remove_npc(&mut self, npc: u16) {
        self.talk.remove_npc(npc);
        if let Some(scene) = self.runtime.as_mut() {
            self.events.remove_npc(scene, npc);
        }
    }

    /// Trigger actions 0x95/0x96 (`Trig_RunAction` cases 0x31/0x32): the placement `(block,
    /// id)` of the map is hidden or shown, for both the drawn instances and the collision.
    fn set_placement_hidden(&mut self, block: u16, placement: u16, hidden: bool) {
        let change = PlacementChange::Hidden {
            group: i32::from(block),
            id: i32::from(placement),
            hidden,
        };
        let Some(scene) = self.runtime.as_mut() else {
            self.collision
                .set_placement_hidden(u32::from(block), placement, hidden);
            return;
        };
        self.events
            .set_placement(scene, &mut self.collision, change);
    }

    /// Natives 574/575 applied by a running event: the scene bindings were updated by the
    /// event player, and the collision follows here (the collision world belongs to the app).
    fn drain_placement_changes(&mut self) {
        let changes = self.events.take_collision_changes();
        apply_collision_changes(&mut self.collision, changes);
    }

    fn change_map(&mut self, change: &MapChange) -> bool {
        /// Map changes a single step may chain before the loop is cut.
        const MAX_CHAIN: u8 = 4;
        if self.chain >= MAX_CHAIN {
            status(&format!("map change chain cut: {change:?}"));
            return false;
        }
        let from = self.current.as_ref().map(|(e, _)| e.id);
        self.journal.record(
            self.tick,
            Step::MapChange {
                from,
                to: change.destination,
                cause: format!("{:?}", change.cause),
            },
        );
        status(&format!(
            "map change {from:?} -> {} at {:?} facing {:?}: {:?}",
            change.destination, change.position, change.angle, change.cause
        ));
        let spawn = change
            .position
            .map(|p| (p, change.angle.map_or(0.0, field::yaw_of_angle)));
        self.chain += 1;
        let loaded = self.load_map(&change.destination.to_string(), true, spawn, None, true);
        self.chain -= 1;
        // Actions 0x85/0x8a encode the arrival event (`Save_Warp+0x20`, read by
        // `ov_017::0219e384` → `ov_017::0219fc88`, which puts it in the event task's list):
        // it starts once the destination map and its own entry triggers are in place. A
        // doorway or the `0x72` spawn entry is not an event, and the buffer holds no event
        // number 0 (the ROM has no `ev00000`).
        if loaded
            && change.cause == Cause::Trigger
            && let Some(entry) = change.entry
            && let Ok(entry) = u16::try_from(entry)
            && entry != 0
        {
            self.events.queue(entry);
        }
        loaded
    }

    fn upload(&mut self) {
        let (Some(gpu), Some(runtime)) = (&self.gpu, &self.runtime) else {
            return;
        };
        self.renderer = Some(Renderer::new(
            &gpu.device,
            &gpu.queue,
            gpu.config.format,
            &runtime.scene,
        ));
        if let Some(renderer) = &mut self.renderer {
            renderer.environment = self.environment.clone();
        }
    }

    fn update_title(&self) {
        let (Some(window), Some((entry, label))) = (&self.window, &self.current) else {
            return;
        };
        let mode = match self.camera.mode {
            CameraMode::Orbit => "orbit",
            CameraMode::Fly => "fly",
        };
        window.set_title(&format!(
            "DQ9 Rust: {} {} ({label}), time {}, {mode}",
            entry.id, entry.code, self.time
        ));
    }

    fn cycle_map(&mut self, delta: isize) {
        if self.maps.is_empty() {
            return;
        }
        let at = self
            .current
            .as_ref()
            .and_then(|(e, _)| self.maps.iter().position(|m| m == e))
            .unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(self.maps.len() as isize) as usize;
        let id = self.maps[next].id.to_string();
        self.load(&id, true, None, None);
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, pressed: bool) {
        if !pressed {
            self.held.remove(&code);
            return;
        }
        if !self.held.insert(code) {
            return; // key repeat
        }
        if self.talk.is_active() {
            match code {
                KeyCode::ArrowUp | KeyCode::KeyW => self.talk.press(Button::Up),
                KeyCode::ArrowDown | KeyCode::KeyS => self.talk.press(Button::Down),
                _ => {}
            }
        }
        match code {
            KeyCode::Enter | KeyCode::Space | KeyCode::KeyZ => self.talk.press(Button::Confirm),
            KeyCode::Backspace | KeyCode::KeyX => self.talk.press(Button::Cancel),
            KeyCode::Escape => event_loop.exit(),
            KeyCode::Tab => {
                self.camera.mode = match self.camera.mode {
                    CameraMode::Orbit => CameraMode::Fly,
                    CameraMode::Fly => CameraMode::Orbit,
                };
                self.update_title();
            }
            KeyCode::KeyF => {
                if self.camera.mode == CameraMode::Orbit {
                    if let Some(hero) = &self.hero {
                        self.camera = Camera::field(hero.position(), self.camera_home);
                    }
                } else if let Some(bounds) = self.framed {
                    self.camera.frame(bounds);
                }
            }
            KeyCode::KeyM => self.audio.toggle_mute(),
            KeyCode::KeyR => self.options.layout = self.options.layout.next(),
            KeyCode::KeyT => {
                let next = match self.day.phase() {
                    TimeOfDay::Night => TimeOfDay::Morning,
                    TimeOfDay::Morning => TimeOfDay::Day,
                    TimeOfDay::Day => TimeOfDay::Evening,
                    TimeOfDay::Evening => TimeOfDay::Night,
                };
                self.set_time(next);
            }
            KeyCode::PageDown => self.cycle_map(1),
            KeyCode::PageUp => self.cycle_map(-1),
            _ => {}
        }
    }

    /// An event is next in line: loads its map when it is elsewhere, then has the player add
    /// the models its script needs.
    fn begin_event(&mut self, event: u16) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let place = crate::event::place(rom, event);
        let here = self.current.as_ref().map(|(entry, _)| entry.id);
        if let Some(place) = place
            && Some(place.map) != here
        {
            let spawn = place
                .position
                .map(|(p, angle)| (Vec3::from_array(p), field::yaw_of_angle(angle)));
            status(&format!("event {event} happens on map {}", place.map));
            self.quiet_events = true;
            let loaded = self.load_map(&place.map.to_string(), true, spawn, None, false);
            self.quiet_events = false;
            if !loaded {
                status(&format!("event {event}: map {} did not load", place.map));
            }
        }
        let (Some(rom), Some(scene), Some(hero)) = (
            self.rom.as_deref(),
            self.runtime.as_mut(),
            self.hero.as_mut(),
        ) else {
            return;
        };
        let mut stage = Stage {
            rom,
            world: &self.state,
            scene,
            hero,
            collision: &self.collision,
            camera: &mut self.camera,
            talk: &mut self.talk,
        };
        let prepared = self.events.prepare(rom, event, &mut stage);
        if prepared {
            // `EventTask_Update` (`021bc77c`): the opening scenes set `540/770 + list rank +
            // 0x38e` on their first non-loading update, from the `EventScript` already loaded.
            if let Some(flag) = self.events.prepared_scene_flag() {
                self.state.flags.insert(i32::from(flag));
            }
            self.upload();
        }
    }

    /// One fixed step while an event runs: the player is locked out and the script drives
    /// the actors, the camera and the message window. Returns `false` when the scene broke.
    fn step_event(&mut self) -> bool {
        if let Some(event) = self.events.pending_start() {
            self.begin_event(event);
        }
        let outcomes = {
            let (Some(rom), Some(scene), Some(hero)) = (
                self.rom.as_deref(),
                self.runtime.as_mut(),
                self.hero.as_mut(),
            ) else {
                return true;
            };
            let mut stage = Stage {
                rom,
                world: &self.state,
                scene,
                hero,
                collision: &self.collision,
                camera: &mut self.camera,
                talk: &mut self.talk,
            };
            self.events.step(&mut stage)
        };
        self.drain_placement_changes();
        self.talk.step();
        self.apply_event_lighting();
        for outcome in outcomes {
            match outcome {
                Outcome::Started(id) => self.journal.record(self.tick, Step::EventStarted(id)),
                Outcome::Ended(id) => {
                    self.journal.record(self.tick, Step::EventEnded(id));
                    // The game fires trigger event 11 when an event ends (`EventTask_Update`).
                    if let Some(field) = &mut self.field {
                        let events = field.event_ended(id, &mut self.state);
                        self.apply_events(events);
                    }
                }
                Outcome::Sound(sound) => {
                    if let Some(rom) = self.rom.as_deref() {
                        self.audio.script_sound(rom, &sound);
                    }
                }
                Outcome::SetQuestSignal(value) => {
                    // Native 559: quest manager `+0x490`, read by trigger condition 0x1f
                    // (`Raw::Quest490`). Applied in outcome order, before the `Ended` that
                    // runs event 11 of the same event.
                    self.state.raw.insert(Raw::Quest490, u32::from(value));
                }
                Outcome::DoorSound { block, placement } => {
                    self.play_door_sound(block, placement);
                }
                Outcome::Finished => {}
            }
        }
        if let Some(runtime) = &mut self.runtime
            && let Err(e) = runtime.step(&self.camera)
        {
            status(&format!("animation update failed: {e}"));
            self.runtime = None;
            self.renderer = None;
            return false;
        }
        true
    }

    /// One fixed step of the sound: the waits of a jingle, which a script may be polling.
    fn step_sound(&mut self) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        self.audio.step(rom);
        if self.audio.take_jingle_ended() {
            self.events.jingle_ended();
        }
        for sound in self.talk.take_sounds() {
            self.audio.talk_sound(rom, sound);
        }
    }

    fn axis(&self, plus: &[KeyCode], minus: &[KeyCode]) -> f32 {
        let any = |keys: &[KeyCode]| keys.iter().any(|k| self.held.contains(k));
        f32::from(i8::from(any(plus)) - i8::from(any(minus)))
    }

    /// Runs the fixed steps due since the last frame.
    /// The game's camera turn (`FieldCamera_Update`): `Q`/`E` are its `L`/`R` buttons. The yaw is
    /// measured from the one the map was entered with, and turns the other way round from the
    /// application's.
    fn turn_camera(&mut self) {
        let left = self.held.contains(&KeyCode::KeyQ);
        let right = self.held.contains(&KeyCode::KeyE);
        if !(left || right || self.field_camera.is_returning()) {
            return;
        }
        // Whole milliseconds of the fixed steps, as the game's delta time is.
        self.clock_ms += STEP * 1000.0;
        let delta = self.clock_ms.floor();
        self.clock_ms -= delta;
        let offset = -(self.camera.yaw - self.camera_home);
        let yaw = reduce_angle((offset * 4096.0).round() as i32);
        let yaw = self.field_camera.step(yaw, 0, left, right, delta as u32);
        self.camera.yaw =
            (self.camera_home - yaw as f32 / 4096.0).rem_euclid(std::f32::consts::TAU);
    }

    fn update(&mut self) {
        let now = Instant::now();
        // Clamp long pauses (window dragged, tab hidden) instead of catching up.
        self.accumulator += now.duration_since(self.last).as_secs_f32().min(0.25);
        self.last = now;
        let mut due = 0;
        while self.accumulator >= STEP {
            self.accumulator -= STEP;
            due += 1;
        }
        self.run_steps(due);
    }

    /// Runs `due` fixed steps of the game logic.
    fn run_steps(&mut self, due: u32) {
        for _ in 0..due {
            // `KeyManager::RefreshKeys` (`0x020122ac`): the first frame a button is held
            // raises flag 0x113e. The app's buttons are the d-pad, the action key, the
            // headless pilot's walk and a replayed input.
            let buttons = self.talk.confirm_pending()
                || self.pilot_dir.is_some()
                || self.replay_pad.is_some_and(|bits| bits != 0)
                || [
                    KeyCode::KeyW,
                    KeyCode::KeyA,
                    KeyCode::KeyS,
                    KeyCode::KeyD,
                    KeyCode::ArrowUp,
                    KeyCode::ArrowDown,
                    KeyCode::ArrowLeft,
                    KeyCode::ArrowRight,
                ]
                .into_iter()
                .any(|key| self.held.contains(&key));
            if buttons {
                self.state
                    .flags
                    .insert(i32::from(dq9_core::state::KEY_REFRESH_FLAG));
            }
            if self.options.trace_inputs {
                self.input_trace.push((0, self.talk.confirm_pending()));
            }
            self.step_sound();
            self.step_doors();
            self.tick += 1;
            // The DS game updates its field once per two VBlanks (`GameState+0x3c4` delta 2,
            // `docs/re/movement.md`). `tick` is the free-running VBlank counter (reset only by
            // `start_new_game`), so the original's update parity survives map loads and
            // events. Everything not gated below (audio, doors, messages, event VM, camera,
            // animation) stays at 60 Hz.
            let field_due = self.tick % 2 == 1;
            self.journal.note_progress(self.tick, &self.state);
            if self.events.pending_start().is_some() || self.events.is_active() {
                if !self.step_event() {
                    return;
                }
                continue;
            }
            let mut events = Vec::new();
            if !self.talk.is_active() && self.talk.confirm_pending() {
                if let (Some(rom), Some(hero), Some(field), Some((entry, _))) =
                    (&self.rom, &self.hero, &mut self.field, &self.current)
                {
                    events = self
                        .talk
                        .begin(rom, &entry.code, hero, field, &mut self.state);
                }
                self.talk.discard_input();
            }
            self.talk.step();
            // The game fires trigger event 1 when a conversation closes (`TalkTask_Update`).
            if let Some(ended) = self.talk.take_ended()
                && let Some(field) = &mut self.field
            {
                events.extend(field.talk_ended(
                    ended.npc,
                    ended.context,
                    ended.answer,
                    &mut self.state,
                ));
            }
            let talking = self.talk.is_active();
            let piloted = std::mem::take(&mut self.pilot_dir);
            let replay = self.replay_pad.take();
            let mut input = CameraInput {
                forward: self.axis(
                    &[KeyCode::KeyW, KeyCode::ArrowUp],
                    &[KeyCode::KeyS, KeyCode::ArrowDown],
                ),
                right: self.axis(
                    &[KeyCode::KeyD, KeyCode::ArrowRight],
                    &[KeyCode::KeyA, KeyCode::ArrowLeft],
                ),
                up: self.axis(&[KeyCode::KeyE], &[KeyCode::KeyQ]),
                rotate: std::mem::take(&mut self.rotate),
                zoom: std::mem::take(&mut self.zoom),
                fast: self.held.contains(&KeyCode::ShiftLeft)
                    || self.held.contains(&KeyCode::ShiftRight),
            };
            let walking = self.camera.mode == CameraMode::Orbit;
            if walking || talking {
                input.forward = 0.0;
                input.right = 0.0;
            }
            if walking {
                input.up = 0.0;
            }
            self.camera.step(&input, STEP);
            let steering = walking && !talking && piloted.is_none() && replay.is_none();
            let pad = match (replay, piloted) {
                (Some(bits), _) => pad_input(bits),
                (None, Some(direction)) => Input::from_direction(direction),
                (None, None) => Input {
                    up: steering && self.axis(&[KeyCode::KeyW, KeyCode::ArrowUp], &[]) > 0.0,
                    down: steering && self.axis(&[KeyCode::KeyS, KeyCode::ArrowDown], &[]) > 0.0,
                    left: steering && self.axis(&[KeyCode::KeyA, KeyCode::ArrowLeft], &[]) > 0.0,
                    right: steering && self.axis(&[KeyCode::KeyD, KeyCode::ArrowRight], &[]) > 0.0,
                },
            };
            if self.options.trace_inputs
                && let Some(last) = self.input_trace.last_mut()
            {
                last.0 = u8::from(pad.up)
                    | u8::from(pad.down) << 1
                    | u8::from(pad.left) << 2
                    | u8::from(pad.right) << 3;
            }
            if steering {
                self.turn_camera();
            }
            // The game's camera yaw turns the other way round from the application's. The
            // headless pilot gives world directions, so it walks with the camera at yaw 0.
            let camera_yaw = if piloted.is_some() || replay.is_some() {
                0
            } else {
                reduce_angle((-self.camera.yaw * 4096.0) as i32)
            };
            let move_direction = match (replay, piloted) {
                (Some(bits), _) => pad_direction(bits),
                // The d-pad the hero actually presses defines the field's walk direction too
                // (a door compares it): the raw steering vector would make a replay diverge.
                (None, Some(direction)) => {
                    pad_direction(input_bits(Input::from_direction(direction)))
                }
                (None, None) if steering => {
                    let (sin, cos) = self.camera.yaw.sin_cos();
                    let forward = Vec3::new(sin, 0.0, -cos);
                    let right = Vec3::new(cos, 0.0, sin);
                    forward
                        * self.axis(
                            &[KeyCode::KeyW, KeyCode::ArrowUp],
                            &[KeyCode::KeyS, KeyCode::ArrowDown],
                        )
                        + right
                            * self.axis(
                                &[KeyCode::KeyD, KeyCode::ArrowRight],
                                &[KeyCode::KeyA, KeyCode::ArrowLeft],
                            )
                }
                (None, None) => Vec3::ZERO,
            };
            if let Some(hero) = &mut self.hero {
                let before = hero.position();
                if field_due {
                    // Two elapsed VBlanks, as `Character_Update` reads them (`ticks = 2`):
                    // `update` turns, accelerates and moves for both ticks before
                    // `read_input` samples the pad. The field task also runs on this same
                    // game update, with the post-2-tick pose (the original's task order).
                    hero.step(&self.collision, pad, camera_yaw, 2);
                    if let Some(field) = &mut self.field {
                        // `Field::step` carries the doorway approach counter
                        // (`APPROACH_FRAMES = 3`): its 4th call fires event 17, so a door
                        // now triggers on 8 VBlanks instead of 4. The original task's
                        // cadence is the documented hypothesis (`docs/re/movement.md`);
                        // this is not a separately re-timed door.
                        let leader = Leader {
                            position: hero.position(),
                            yaw: hero.yaw(),
                            input: move_direction.normalize_or_zero(),
                        };
                        events.extend(field.step(&leader, &mut self.state));
                    }
                    if let (Some(runtime), Some(index)) = (&mut self.runtime, self.hero_instance) {
                        // Motion over the two ticks: gait thresholds are in units/s, so
                        // divide by `2 * STEP` (dividing by `STEP` would double the speed
                        // and flip Walk/Run).
                        let moved = hero.position() - before;
                        let speed = Vec3::new(moved.x, 0.0, moved.z).length() / (2.0 * STEP);
                        runtime.set_hero_gait(Gait::from_speed(speed));
                        let world =
                            runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()));
                        if let Some(instance) = runtime.scene.instances.get_mut(index) {
                            instance.world = world;
                        }
                    }
                }
                // Held pose on the intermediate VBlank: the camera still follows it, as the
                // original's follow runs per VBlank, not per game update.
                self.camera.follow_hero(hero.position());
            }
            if self.apply_events(events) {
                continue;
            }
            if let Some(runtime) = &mut self.runtime
                && let Err(e) = runtime.step(&self.camera)
            {
                status(&format!("animation update failed: {e}"));
                self.runtime = None;
                self.renderer = None;
                return;
            }
        }
    }

    /// A doorway the field touched (native 584): the shared door state, the same the scripts
    /// use, and its own effect once.
    fn open_door(&mut self, block: u32, placement: u16) {
        if !self.events.open_door(block as i32, placement as i32, false) {
            return;
        }
        self.play_door_sound(block as i32, placement as i32);
        status(&format!("door {block}/{placement} opens"));
    }

    /// Native 584's effect: `g_DoorSeTable[low quartet]`, base effects 200 to 204, when the
    /// map's doorway names that quartet.
    fn play_door_sound(&mut self, block: i32, placement: i32) {
        let Some(low) = self
            .field
            .as_ref()
            .and_then(|field| field.door_sound(block as u8, placement as u8))
        else {
            status(&format!(
                "door {block}/{placement}: no sound in this map's doorway"
            ));
            return;
        };
        if let Some(rom) = self.rom.as_deref() {
            self.audio.play_base_effect(rom, u16::from(low) + 200);
        }
    }

    /// One fixed step of the doors: when no script runs, advance the shared angles and move
    /// the scene placements; then turn the collision subtree and fire the arrival trigger
    /// once a door reaches its target.
    fn step_doors(&mut self) {
        if self.events.doors().is_empty() {
            return;
        }
        if !(self.events.pending_start().is_some() || self.events.is_active()) {
            let (Some(rom), Some(scene), Some(hero)) = (
                self.rom.as_deref(),
                self.runtime.as_mut(),
                self.hero.as_mut(),
            ) else {
                return;
            };
            let mut stage = Stage {
                rom,
                world: &self.state,
                scene,
                hero,
                collision: &self.collision,
                camera: &mut self.camera,
                talk: &mut self.talk,
            };
            self.events.tick_doors(&mut stage);
        }
        self.drain_placement_changes();
        let count = self.events.doors().len();
        let mut opened = false;
        for index in 0..count {
            let (block, placement, angle, done) = {
                let door = &self.events.doors()[index];
                (
                    door.block,
                    door.placement,
                    door.angle_fx(),
                    door.angle >= door.target,
                )
            };
            let key = (block, placement);
            if self.door_angles.get(&key).copied() != Some(angle) {
                self.collision
                    .set_door_angle(block as u32, placement as u16, angle);
                self.door_angles.insert(key, angle);
            }
            if done && self.doors_opened.insert(key) {
                opened = true;
            }
        }
        if opened && let Some(field) = &mut self.field {
            let events = field.door_opened(&mut self.state);
            self.apply_events(events);
        }
    }

    /// One fixed step of a native `--walk` script: the world direction is fed exactly as the
    /// pilot feeds it, through the same `Hero::step`/`Field::step`/door path as the window.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn walk_step(&mut self, direction: Vec3) {
        self.pilot_dir = Some(direction);
        self.run_steps(1);
    }

    /// Places the hero at `(x, z)` on the current map (`--start`), on the collision's ground.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn place_hero(&mut self, x: f32, z: f32) -> anyhow::Result<()> {
        let code = self
            .current
            .as_ref()
            .map(|(entry, _)| entry.code.as_str())
            .unwrap_or_default();
        let hero = crate::walk::start_hero(&self.collision, code, (x, z))?;
        self.hero = Some(hero);
        if let (Some(runtime), Some(index)) = (&mut self.runtime, self.hero_instance) {
            let world = runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()));
            if let Some(instance) = runtime.scene.instances.get_mut(index) {
                instance.world = world;
            }
        }
        Ok(())
    }

    /// The loaded map, its code and the hero's pose, for the `--walk` logs.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn walk_pose(&self) -> Option<(u16, &str, Vec3, f32)> {
        let (entry, _) = self.current.as_ref()?;
        let hero = self.hero?;
        Some((entry.id, entry.code.as_str(), hero.position(), hero.yaw()))
    }

    /// One fixed step of a recorded input trace (`--trace-inputs`, replayed): the pad keys and
    /// the action key are fed exactly as recorded, and the journal checkpoints are captured.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn replay_step(&mut self, bits: u8, confirm: bool) {
        let recorded = self.journal.steps.len();
        self.replay_pad = Some(bits);
        if confirm {
            self.talk.press(Button::Confirm);
        }
        self.run_steps(1);
        self.capture_journal(recorded);
        // The pilot's control points are part of the proof: record them the same way.
        let busy = self.events.is_active() || self.events.pending_start().is_some();
        if !busy && !self.talk.is_active() && !self.talk.confirm_pending() {
            let map = self.current.as_ref().map_or(0, |(entry, _)| entry.id);
            let track = &self.state.tracks[self.state.track];
            let story = (track.chapter, track.step, track.substep);
            let last = self
                .journal
                .steps
                .iter()
                .rev()
                .find_map(|(_, step)| match step {
                    Step::Control { story, .. } => Some(*story),
                    _ => None,
                });
            if last.is_none_or(|seen| seen != story) {
                self.journal.record(self.tick, Step::Control { map, story });
                self.capture_checkpoint("control");
            }
        }
    }

    /// Records the story state for the oracle (`dq9re state --json` field names): one JSON
    /// object per checkpoint, in the order the journal recorded them.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn capture_checkpoint(&mut self, kind: &'static str) {
        use std::fmt::Write as _;
        let map = self.current.as_ref().map_or(0, |(entry, _)| entry.id);
        let area = self
            .current
            .as_ref()
            .and_then(|(entry, _)| entry.code.get(..3))
            .and_then(|family| self.maps.iter().find(|map| map.code == family))
            .map_or(0, |map| map.id);
        let daytime = self.day.phase().is_daytime();
        let mut out = String::new();
        let _ = write!(
            out,
            "{{\"tick\":{},\"seconds\":{:.2},\"kind\":\"{kind}\",\"vblank\":{},\"map\":{},\"area\":{},\"track\":{},\"daytime\":{},",
            self.tick,
            self.tick as f64 / 60.0,
            self.tick,
            map,
            area,
            self.state.track,
            daytime,
        );
        out.push_str("\"tracks\":[");
        for (index, track) in self.state.tracks.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"chapter\":{},\"step\":{},\"substep\":{},\"chapter_flags\":[",
                track.chapter, track.step, track.substep
            );
            write_flag_list(&mut out, &track.flags[0]);
            out.push_str("],\"step_flags\":[");
            write_flag_list(&mut out, &track.flags[1]);
            out.push_str("]}");
        }
        out.push_str("],\"flags\":[");
        write_flag_list(&mut out, &self.state.flags);
        out.push(']');
        out.push_str(",\"quests\":{");
        for (index, (quest, state)) in self.state.quests.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "\"{quest}\":{state}");
        }
        out.push('}');
        let event = self
            .events
            .current()
            .map_or_else(|| "null".to_owned(), |event| event.to_string());
        let hero = self.hero.map_or([0.0; 4], |hero| {
            [
                hero.position().x,
                hero.position().y,
                hero.position().z,
                hero.yaw(),
            ]
        });
        let _ = write!(
            out,
            ",\"event\":{event},\"hero\":{{\"x\":{:.3},\"y\":{:.3},\"z\":{:.3},\"yaw\":{:.3}}}}}",
            hero[0], hero[1], hero[2], hero[3]
        );
        self.checkpoints.push(out);
    }

    /// The checkpoints captured so far, one JSON object per line.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn checkpoints_jsonl(&self) -> String {
        let mut out = self.checkpoints.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }

    /// The recorded pad and action key, one JSON object per fixed step.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn input_trace_jsonl(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (frame, (bits, confirm)) in self.input_trace.iter().enumerate() {
            let _ = write!(out, "{{\"frame\":{frame},\"pad\":[");
            let mut first = true;
            for (bit, name) in [(1u8, "up"), (2, "down"), (4, "left"), (8, "right")] {
                if bits & bit == 0 {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                let _ = write!(out, "\"{name}\"");
            }
            let _ = writeln!(out, "],\"a\":{confirm}}}");
        }
        out
    }

    fn redraw(&mut self) {
        self.update();
        if let (Some(runtime), Some(gpu), Some(renderer)) =
            (&mut self.runtime, &self.gpu, &mut self.renderer)
            && let Err(e) = runtime
                .face_camera(&self.camera)
                .and_then(|()| runtime.upload(renderer, &gpu.device, &gpu.queue))
        {
            status(&format!("animation upload failed: {e}"));
            self.runtime = None;
            self.renderer = None;
            return;
        }
        if let (Some(gpu), Some(renderer), Some(runtime)) =
            (&self.gpu, &mut self.renderer, &self.runtime)
        {
            // Instances an event moved.
            for index in self.events.take_dirty() {
                if let Some(instance) = runtime.scene.instances.get(index) {
                    renderer.update_instance_world(&gpu.queue, index, instance.world);
                }
            }
        }
        if let (Some(gpu), Some(renderer), Some(runtime), Some(hero), Some(index)) = (
            &self.gpu,
            &mut self.renderer,
            &self.runtime,
            &self.hero,
            self.hero_instance,
        ) {
            let world = if self.events.hero_hidden() {
                glam::Mat4::from_scale(Vec3::ZERO)
            } else {
                runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()))
            };
            if !renderer.update_instance_world(&gpu.queue, index, world) {
                status("hero instance is unavailable");
            }
        }
        let (Some(gpu), Some(renderer)) = (&mut self.gpu, &mut self.renderer) else {
            return;
        };
        #[cfg(target_arch = "wasm32")]
        gpu.sync_canvas_size();
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (w, h) = (gpu.config.width, gpu.config.height);
        let screens = screens::screens(self.options.layout, w, h);
        // 109/110 (`POWCNT1` bit 15): the field and the message window move to the other DS
        // screen; the panel fills the one they leave.
        let swapped = self.events.screen_swapped();
        let field = screens.field(swapped);
        let panel = self
            .panel
            .get_or_insert_with(|| Panel::new(&gpu.device, gpu.config.format));
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        renderer.render(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            (&view, (w, h)),
            field,
            &self.camera,
        );
        if let Some(overlay) = screens.overlay(swapped) {
            panel.draw(&mut encoder, &view, overlay);
        }
        self.talk.draw(
            &gpu.device,
            &gpu.queue,
            gpu.config.format,
            &mut encoder,
            &view,
            field,
        );
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("DQ9 Rust");
        // On the web the page's CSS sizes the canvas.
        #[cfg(not(target_arch = "wasm32"))]
        let attributes = attributes.with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0));
        #[cfg(target_arch = "wasm32")]
        let attributes = crate::web::window_attributes(attributes);
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                status(&format!("cannot create the window: {e}"));
                event_loop.exit();
                return;
            }
        };
        self.window = Some(window.clone());
        self.update_title();
        let Some(proxy) = self.proxy.clone() else {
            status("no event loop proxy: no GPU setup");
            return;
        };
        let setup = async move {
            let event = match Gpu::new(window).await {
                Ok(gpu) => UserEvent::Gpu(Box::new(gpu)),
                Err(e) => UserEvent::GpuFailed(format!("{e:#}")),
            };
            let _ = proxy.send_event(event);
        };
        #[cfg(not(target_arch = "wasm32"))]
        pollster::block_on(setup);
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(setup);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Gpu(gpu) => {
                self.gpu = Some(*gpu);
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    if let Some(gpu) = &mut self.gpu {
                        gpu.resize(size.width, size.height);
                    }
                }
                self.upload();
                self.last = Instant::now();
            }
            UserEvent::GpuFailed(e) => {
                status(&format!("GPU: {e}"));
                event_loop.exit();
            }
            #[cfg(target_arch = "wasm32")]
            UserEvent::Rom(rom) => self.set_rom(rom),
            #[cfg(target_arch = "wasm32")]
            UserEvent::Save(save) => self.set_save(save),
            #[cfg(target_arch = "wasm32")]
            UserEvent::Time(time) => self.set_time(lighting::phase(time)),
            #[cfg(target_arch = "wasm32")]
            UserEvent::Layout(layout) => self.options.layout = layout,
            #[cfg(target_arch = "wasm32")]
            UserEvent::NewGame => {
                self.options.new_game = true;
                self.start_new_game();
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            #[cfg(target_arch = "wasm32")]
            WindowEvent::ScaleFactorChanged { .. } => {
                if let (Some(gpu), Some(window)) = (&mut self.gpu, &self.window) {
                    let size = window.inner_size();
                    gpu.resize(size.width, size.height);
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.audio.gesture();
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.key(event_loop, code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::Focused(false) => self.held.clear(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.audio.gesture();
                self.dragging = state == ElementState::Pressed;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((x, y)) = self.cursor
                    && self.dragging
                {
                    let speed = 0.005;
                    self.rotate += Vec2::new(
                        ((position.x - x) * speed) as f32,
                        ((position.y - y) * speed) as f32,
                    );
                }
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.zoom += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl App {
    /// Plays the game logic without a window for at most `steps` fixed steps, until the
    /// events that were started are over, pressing the action key every `press_every` steps
    /// (`--headless-steps`, `docs/app.md`). The log shows every event, map change and trigger.
    pub fn play_headless(&mut self, steps: u32, press_every: Option<u32>) {
        let mut started = false;
        for step in 0..steps {
            if press_every.is_some_and(|n| step > 0 && step % n == 0) {
                self.talk.press(Button::Confirm);
            }
            self.run_steps(1);
            let busy = self.events.is_active() || self.events.pending_start().is_some();
            started |= busy;
            if started && !busy {
                status(&format!("events over after {step} steps"));
                return;
            }
        }
        status(&format!("stopped after {steps} steps"));
    }

    /// What the sound mixed during the steps so far, with its sample rate, when the
    /// application records it (`Options::record_sound`).
    pub fn take_recording(&mut self) -> Option<(u32, Vec<f32>)> {
        self.audio
            .take_recording()
            .map(|samples| (crate::audio::RECORD_RATE, samples))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use dq9_core::state::GameState;
    use dq9_formats::save::Save;

    /// Start-up options of a ROM-backed regression on an explicit map and story point, with
    /// no save and no pinned time (`--time`/`--clock` absent: the field may set the night).
    fn map_options(map: &str, story: (u8, u8, u8, bool)) -> Options {
        Options {
            map: Some(map.into()),
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story {
                chapter: story.0,
                step: story.1,
                substep: story.2,
                daytime: story.3,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game: false,
        }
    }

    /// Whether the cast instance of NPC `npc` is drawn on the loaded scene.
    fn cast_drawn(app: &App, npc: u16) -> bool {
        let runtime = app.runtime.as_ref().expect("a map is loaded");
        runtime
            .npc_instance(npc)
            .is_some_and(|index| runtime.scene.instances[index].drawn())
    }

    /// Number of `Step::Map` entries for `map` in the journal.
    fn map_loads(app: &App, map: u16) -> usize {
        app.journal
            .steps
            .iter()
            .filter(|(_, step)| matches!(step, Step::Map { id, .. } if *id == map))
            .count()
    }

    /// Plays the pending event until it ends, pressing the action key to read the messages.
    fn play_pending_event(app: &mut App, event: u16) {
        let mut played = false;
        for frame in 0..60_000 {
            if frame % 20 == 0 {
                app.talk.press(Button::Confirm);
            }
            app.run_steps(1);
            played |= app.events.started().contains(&event);
            if played && !app.events.is_active() && app.events.pending_start().is_none() {
                return;
            }
        }
        panic!(
            "event {event} did not end: started {:?}, current {:?}",
            app.events.started(),
            app.events.current()
        );
    }

    /// The web bug: after "Nouvelle partie", picking a save must load that save, not start
    /// the prologue again. Exercises the same `set_save` path with a real save and ROM,
    /// through the proxy-less headless constructor.
    #[test]
    fn a_save_selected_after_a_new_game_loads_that_save() {
        let (Some(rom), Some(save_path)) = (testrom::load(), std::env::var_os("DQ9_SAVE")) else {
            eprintln!("skipping: DQ9_ROM or DQ9_SAVE missing");
            return;
        };
        let save = std::fs::read(save_path).unwrap();
        let expected = GameState::from_save(&Save::parse(&save).unwrap()).unwrap();
        let options = Options {
            map: None,
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story {
                chapter: 1,
                step: 1,
                substep: 1,
                daytime: true,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game: true,
        };
        let mut app = App::new_headless(options, Some(rom.to_vec()));
        assert!(app.options.new_game);
        assert_eq!(app.events.pending_start(), Some(21500));
        app.set_save(save.clone());
        assert!(!app.options.new_game, "the save selects save mode");
        assert_eq!(app.options.map, None);
        let map = app.current.as_ref().map(|(entry, _)| entry.id);
        assert_eq!(map, Some(expected.map), "the save's map is loaded");
        assert!(!app.events.is_active());
        assert_eq!(app.events.pending_start(), None);
        assert!(!app.talk.is_active());

        // Replacing an already-running prologue must also discard its active caption,
        // not merely select the save's map while the old task keeps running.
        app.options.new_game = true;
        app.start_new_game();
        for _ in 0..480 {
            app.run_steps(1);
            if app.talk.is_active() {
                break;
            }
        }
        assert!(app.events.is_active(), "the prologue must have started");
        assert!(app.talk.is_active(), "the prologue caption must be open");
        app.set_save(save);
        assert!(!app.events.is_active());
        assert_eq!(app.events.pending_start(), None);
        assert!(!app.talk.is_active());
        app.run_steps(120);
        assert_eq!(
            app.current.as_ref().map(|(entry, _)| entry.id),
            Some(expected.map)
        );
        assert!(!app.events.is_active(), "the old prologue must not resume");
        assert!(!app.talk.is_active(), "the old caption must not reopen");
    }
    /// A new game raises the opening flags by their causes, without a save: the starting
    /// equipment through the item table (`item_obtained_flag`), the `S14M05` map load, and
    /// the first held button (`KeyManager::RefreshKeys`).
    #[test]
    fn a_new_game_raises_the_opening_flags() {
        let Some(rom) = testrom::load() else { return };
        let options = Options {
            map: None,
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story {
                chapter: 1,
                step: 1,
                substep: 1,
                daytime: true,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game: true,
        };
        let mut app = App::new_headless(options, Some(rom.to_vec()));
        assert_eq!(
            app.current.as_ref().map(|(entry, _)| entry.id),
            Some(6405),
            "the opening loads S14M05 before the prologue"
        );
        for flag in [3191, 3775, 3955, 4046, 2774] {
            assert!(
                app.state.flags.contains(&flag),
                "flag {flag} after a new game"
            );
        }
        app.talk.press(Button::Confirm);
        app.run_steps(1);
        assert!(
            app.state.flags.contains(&4414),
            "the first button raises 0x113e"
        );
    }

    /// Stands the test hero on the collision's floor at `(x, z)`, probing from `from_y`
    /// (the map's start helper probes from 3.0 and cannot reach the tower's upper floors).
    /// This uses the real collision height, not a hard-coded pose.
    fn stand_on(app: &mut App, x: f32, z: f32, from_y: f32) {
        let y = app
            .collision
            .ground_height(x, z, from_y)
            .unwrap_or_else(|| panic!("no ground at ({x}, {z}) probing from {from_y}"));
        app.hero = Some(Hero::at(Vec3::new(x, y, z), 0.0));
    }

    #[test]
    fn a_same_map_reload_reapplies_open_doors_to_the_collision() {
        let Some(rom) = testrom::load() else { return };
        let options = Options {
            map: Some("X01M07".into()),
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story {
                chapter: 1,
                step: 1,
                substep: 1,
                daytime: true,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game: false,
        };
        let mut app = App::new_headless(options, Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(4107));
        assert!(app.events.open_door(0, 2, true));
        assert!(app.events.open_door(0, 3, true));
        for _ in 0..160 {
            app.run_steps(1);
        }
        let open: Vec<(i32, i32, i32)> = app
            .events
            .doors()
            .iter()
            .map(|door| (door.block, door.placement, door.angle_fx()))
            .collect();
        assert_eq!(open.len(), 2, "both library door leaves are open");
        for (block, placement, angle) in &open {
            assert_eq!(
                app.door_angles.get(&(*block, *placement)),
                Some(angle),
                "the open door angle reached the collision"
            );
        }
        // A story-driven reload of the same map rebuilds the collision with doors closed;
        // the open doors must be turned again before the walkable grid is probed.
        let before = app.collision.pieces().to_vec();
        let spawn = app.hero.map(|hero| (hero.position(), hero.yaw()));
        assert!(app.load_map("X01M07", false, spawn, None, false));
        assert_eq!(
            app.collision.pieces(),
            before.as_slice(),
            "the rebuilt collision must match the open-door one"
        );
        for (block, placement, angle) in open {
            assert_eq!(
                app.door_angles.get(&(block, placement)),
                Some(&angle),
                "the reload must re-apply door {block}/{placement}"
            );
        }
    }

    #[test]
    fn event_21595_sets_quest490_and_takes_the_1_4_1_warp() {
        let Some(rom) = testrom::load() else { return };
        let options = Options {
            map: Some("M01".into()),
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: Some(21595),
            story: dq9_formats::cast::Story {
                chapter: 1,
                step: 3,
                substep: 1,
                daytime: true,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game: false,
        };
        let mut app = App::new_headless(options, Some(rom.to_vec()));
        for frame in 0..120_000 {
            if frame % 20 == 0 {
                app.talk.press(Button::Confirm);
            }
            app.run_steps(1);
            if app.current.as_ref().map(|(entry, _)| entry.id) == Some(4105) {
                break;
            }
        }
        assert_eq!(
            app.state.raw.get(&Raw::Quest490),
            Some(&1),
            "native 559 must set quest manager +0x490 to 1"
        );
        let track = &app.state.tracks[0];
        assert_eq!(
            (track.chapter, track.step, track.substep),
            (1, 4, 1),
            "the 0x1f(1) branch takes story 1.4.1"
        );
        assert_eq!(
            app.current.as_ref().map(|(entry, _)| entry.id),
            Some(4105),
            "the 1.4.1 warp goes to X01M05"
        );
        assert_ne!(app.events.pending_start(), Some(21593));
        assert!(
            !app.events.started().contains(&21593),
            "the fallback event 21593 must not start: {:?}",
            app.events.started()
        );

        // `0x85(4105, 21597)`: the arrival event the trigger encodes (Save_Warp+0x20) starts
        // once the destination map is loaded, exactly once, and its end raises game-1 flag 0.
        assert_eq!(
            app.events.pending_start(),
            Some(21597),
            "the 0x85 entry event must be queued after the map change"
        );
        assert_eq!(
            map_loads(&app, 4105),
            1,
            "the entry event must not reload 4105"
        );
        play_pending_event(&mut app, 21597);
        assert_eq!(
            app.events
                .started()
                .iter()
                .filter(|&&event| event == 21597)
                .count(),
            1,
            "the entry event is queued once: {:?}",
            app.events.started()
        );
        assert!(
            app.state.tracks[0].flags[1].contains(&0),
            "the end of 21597 raises game-1 flag 0 (room 4105, 1.4 event 11)"
        );
        assert_eq!(map_loads(&app, 4105), 1, "21597 plays on the loaded map");
        assert!(!app.events.started().contains(&21593));
    }

    /// Stands the hero inside the `0x72` exit of the loaded house, with the two game-1 flags
    /// the exit rule waits for, and takes one fixed step.
    fn leave_house(app: &mut App) {
        app.state.tracks[0].flags[1].insert(4);
        app.state.tracks[0].flags[1].insert(5);
        let exit = app
            .field
            .as_ref()
            .expect("the house is loaded")
            .exits()
            .into_iter()
            .find(|exit| exit.warp.is_some())
            .expect("M01M04 has its house exit");
        app.hero = Some(Hero::at(exit.position, 0.0));
        app.run_steps(1);
    }

    /// `room 1104 ... event 6` (`0xb1(1, 0)`) sets the night before the house-exit warp: the
    /// application clock follows before the map change, 1100 loads at night, and 1104 is not
    /// reloaded (a reload would replay its entry events).
    #[test]
    fn house_exit_time_of_day_reaches_the_application_before_the_map_change() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("1104", (1, 2, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1104));
        // A daytime start with no --time/--clock: the field's own 0xb1 is what must move the
        // clock, so the application's daytime state is set explicitly here.
        app.day = lighting::day_clock(None, Some(TimeOfDay::Day.index()), None);
        app.time = app.day.phase().index();
        assert_eq!((app.time, app.state.time_of_day), (2, 2));
        assert_eq!(map_loads(&app, 1104), 1);
        leave_house(&mut app);
        assert_eq!(app.state.time_of_day, 0, "0xb1(1, 0) sets the night");
        assert_eq!(
            app.time,
            TimeOfDay::Night.index(),
            "the application clock follows the field"
        );
        assert_eq!(app.day.phase(), TimeOfDay::Night);
        assert_eq!(
            app.current.as_ref().map(|(entry, _)| entry.id),
            Some(1100),
            "the exit warp loads the village"
        );
        assert_eq!(
            map_loads(&app, 1104),
            1,
            "the change must not reload the house"
        );
    }

    /// `--time`/`--clock` keep the priority: a trigger's `0xb1` changes the trigger state but
    /// not the lighting an oracle capture pinned.
    #[test]
    fn a_pinned_time_of_day_wins_over_the_trigger() {
        let Some(rom) = testrom::load() else { return };
        let mut options = map_options("1104", (1, 2, 1, true));
        options.time = Some(TimeOfDay::Day.index());
        let mut app = App::new_headless(options, Some(rom.to_vec()));
        assert_eq!(
            (app.time, app.day.phase()),
            (TimeOfDay::Day.index(), TimeOfDay::Day)
        );
        leave_house(&mut app);
        assert_eq!(app.state.time_of_day, 0, "the trigger still sets the night");
        assert_eq!(
            app.time,
            TimeOfDay::Day.index(),
            "--time keeps the lighting it pinned"
        );
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
    }

    /// A `MapChange` that carries an entry number but is not a trigger warp starts nothing.
    fn assert_entry_ignored(rom: &[u8], cause: Cause) {
        let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
        let change = MapChange {
            destination: 4105,
            position: None,
            angle: None,
            entry: Some(21597),
            cause,
        };
        assert!(app.apply_events(vec![Event::MapChange(change)]));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(4105));
        assert_eq!(
            app.events.pending_start(),
            None,
            "{cause:?} is not a trigger warp"
        );
        assert!(app.events.started().is_empty());
    }

    /// Only a trigger warp starts its `entry` event: a doorway or a `0x72` spawn entry that
    /// carries the same number starts nothing.
    #[test]
    fn only_a_trigger_warp_starts_its_entry_event() {
        let Some(rom) = testrom::load() else { return };
        assert_entry_ignored(rom, Cause::Warp { index: 0, kind: 0 });
        assert_entry_ignored(
            rom,
            Cause::Doorway {
                class: 0,
                number: 0,
            },
        );
        let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
        let change = MapChange {
            destination: 4105,
            position: None,
            angle: None,
            entry: Some(21597),
            cause: Cause::Trigger,
        };
        assert!(app.apply_events(vec![Event::MapChange(change)]));
        assert_eq!(
            app.events.pending_start(),
            Some(21597),
            "a trigger warp starts its entry event"
        );
    }

    /// Action 0x7c after 1390: the cast instance of NPC 210 disappears from the scene, and no
    /// other NPC of the village is hidden.
    #[test]
    fn removing_npc_210_hides_its_cast_instance() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
        assert!(cast_drawn(&app, 210), "NPC 210 stands in the village");
        assert!(cast_drawn(&app, 95), "the dog stands in the village");
        let events = app
            .field
            .as_mut()
            .unwrap()
            .event_ended(1390, &mut app.state);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Request(Request::RemoveNpc(210)))),
            "event 11 of 1390 removes NPC 210: {events:?}"
        );
        app.apply_events(events);
        assert!(
            !cast_drawn(&app, 210),
            "the keepsake-ring NPC must leave the scene"
        );
        assert!(cast_drawn(&app, 95), "the other villagers stay");
        assert!(
            app.state.tracks[0].flags[1].contains(&3),
            "game-1 flag 3 is raised"
        );
    }

    /// The keepsake ring through the real talk trigger: at 1.2.1 the village's arrival scene
    /// 21592 runs first (`room 1100 1.2..1.2 event 3`), then talking to NPC 210 fires its
    /// event 0 (`if 0x6(210) 0x5(3) then 0x77(1390)`). The end of 1390 gives item 22158,
    /// raises game-1 flag 3 and removes NPC 210; no other flag moves.
    #[test]
    fn keepsake_ring_task_gives_22158_raises_flag_3_and_removes_npc_210() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
        // A bare `--map` loads without the arrival triggers; the real arrival from 4105 is a
        // transition (`0xe1(1100)`), and only then does event 3 queue 21592 and raise game-1
        // flag 1. Enter once more with the transition, as `App::change_map` does.
        assert!(app.load_map("1100", true, None, None, true));
        // Event 3 of room 1100 queues the 1.2 arrival scene: it is the game's own arrival,
        // not a test flag, and it must finish before the village is talkable.
        assert_eq!(
            app.events.pending_start(),
            Some(21592),
            "the 1.2 arrival scene waits on the loaded village"
        );
        play_pending_event(&mut app, 21592);
        assert!(cast_drawn(&app, 210), "NPC 210 stands in the village");
        assert!(cast_drawn(&app, 95), "the dog stands in the village");
        assert!(!app.state.items.contains_key(&22158));
        let before = app.state.tracks[0].flags.clone();

        talk_to(&mut app, 210);
        assert_eq!(
            app.events.pending_start(),
            Some(1390),
            "NPC 210's event 0 queues the keepsake-ring scene"
        );
        assert!(
            !app.talk.is_active(),
            "the trigger replaces the conversation"
        );
        play_pending_event(&mut app, 1390);

        assert_eq!(
            app.state.items.get(&22158),
            Some(&1),
            "the end of 1390 gives the keepsake ring 22158"
        );
        assert!(
            app.state.tracks[0].flags[1].contains(&3),
            "the end of 1390 raises game-1 flag 3"
        );
        assert!(!cast_drawn(&app, 210), "NPC 210 leaves the scene");
        assert!(cast_drawn(&app, 95), "no other villager is removed");
        let added: Vec<i32> = app.state.tracks[0].flags[1]
            .difference(&before[1])
            .copied()
            .collect();
        assert_eq!(added, vec![3], "only game-1 flag 3 is raised");
        assert_eq!(
            app.state.tracks[0].flags[0], before[0],
            "the ring touches no game-0 flag"
        );
        assert_eq!(
            (
                app.state.tracks[0].chapter,
                app.state.tracks[0].step,
                app.state.tracks[0].substep,
            ),
            (1, 2, 1),
            "the ring does not advance the track"
        );
    }

    /// The widow's task through the real conversation: with game-1 flag 3 and the ring 22158
    /// held, NPC 12's event 0 opens context 193 (`if 0x6(12) 0x4(3) 0x5(4)`). Answering Yes
    /// closes it into event 1, which queues 1330, takes 22158, gives 22159 and raises flag 4.
    /// The scene leaves the widow on the map.
    #[test]
    fn widow_task_takes_22158_gives_22159_and_raises_flag_4() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("1106", (1, 2, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1106));
        // Documented preconditions: the keepsake ring (its own test proves the trigger gives
        // it) and game-1 flag 3, raised with it.
        app.state.obtain(22158, 1);
        app.state.tracks[0].flags[1].insert(3);
        let before = app.state.tracks[0].flags.clone();
        assert!(cast_drawn(&app, 12), "the widow stands in the house");

        talk_to(&mut app, 12);
        assert!(
            app.talk.is_active(),
            "event 0 opens the widow's context 193"
        );
        finish_conversation(&mut app);
        assert_eq!(
            app.events.pending_start(),
            Some(1330),
            "answering Yes queues the widow scene"
        );
        play_pending_event(&mut app, 1330);

        assert_eq!(app.state.items.get(&22158), Some(&0), "the ring is taken");
        assert_eq!(
            app.state.items.get(&22159),
            Some(&1),
            "the benevolessence is given"
        );
        assert!(
            app.state.tracks[0].flags[1].contains(&4),
            "event 1 raises game-1 flag 4"
        );
        assert!(cast_drawn(&app, 12), "the widow stays in the house");
        let added: Vec<i32> = app.state.tracks[0].flags[1]
            .difference(&before[1])
            .copied()
            .collect();
        assert_eq!(added, vec![4], "only game-1 flag 4 is raised");
        assert_eq!(
            app.state.tracks[0].flags[0], before[0],
            "no game-0 flag moves"
        );
    }

    /// Without game-1 flag 3 the widow offers context 192, and event 1 requires context 193:
    /// the conversation closes without queueing 1330 or touching the ring.
    #[test]
    fn widow_without_flag_3_starts_no_scene() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("1106", (1, 2, 1, true)), Some(rom.to_vec()));
        app.state.obtain(22158, 1);
        let before = app.state.tracks[0].flags.clone();
        talk_to(&mut app, 12);
        assert!(
            app.talk.is_active(),
            "the widow still talks, with context 192"
        );
        finish_conversation(&mut app);
        assert_eq!(app.events.pending_start(), None, "no scene is queued");
        assert_eq!(
            app.state.items.get(&22158),
            Some(&1),
            "the ring stays in the bag"
        );
        assert!(!app.state.items.contains_key(&22159));
        assert_eq!(
            app.state.tracks[0].flags[1], before[1],
            "no game-1 flag moves"
        );
        assert_eq!(
            app.state.tracks[0].flags[0], before[0],
            "no game-0 flag moves"
        );
        assert!(cast_drawn(&app, 12), "the widow stays");
    }

    /// Each stable pickup (event 1 of NPCs 211 to 215) removes its own pile from the scene,
    /// the fifth starts 1340, and the following script keeps every removed pile hidden.
    #[test]
    fn stable_pickups_hide_each_pile_and_the_scene_keeps_them_hidden() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("1104", (1, 2, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1104));
        for npc in 211..=215 {
            assert!(cast_drawn(&app, npc), "pile {npc} stands in the stable");
        }
        assert!(
            cast_drawn(&app, 15),
            "the stablemaster stands in the stable"
        );
        for npc in 211..=215 {
            let events = app
                .field
                .as_mut()
                .unwrap()
                .talk_ended(npc, 0, Some(0), &mut app.state);
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    Event::Request(Request::RemoveNpc(removed)) if *removed == npc
                )),
                "talking to pile {npc} removes it: {events:?}"
            );
            app.apply_events(events);
            assert!(!cast_drawn(&app, npc), "pile {npc} must leave the scene");
            // The piles not picked up yet must stay. The piles removed before this one are
            // checked at their own turn and again after 1340.
            for other in npc + 1..=215 {
                assert!(
                    cast_drawn(&app, other),
                    "pile {other} must stay while {npc} is removed"
                );
            }
        }
        assert_eq!(
            app.events.pending_start(),
            Some(1340),
            "the fifth pickup starts the stable scene"
        );
        play_pending_event(&mut app, 1340);
        assert!(
            app.state.tracks[0].flags[1].contains(&5),
            "the end of 1340 raises game-1 flag 5"
        );
        for npc in 211..=215 {
            assert!(
                !cast_drawn(&app, npc),
                "pile {npc} must not come back after the scene"
            );
        }
        assert!(cast_drawn(&app, 15), "the stablemaster stays drawn");
    }

    /// Index of the first area whose entry, read by the field task on a copy, asks for
    /// `wanted`. Probing copies proves which area is the map's added area 12 without
    /// touching the application.
    fn area_triggering(app: &App, wanted: u16) -> usize {
        let field = app.field.as_ref().expect("a map is loaded");
        for (index, point) in field.area_points().into_iter().enumerate() {
            let mut probe = field.clone();
            let mut state = app.state.clone();
            let events = probe.step(
                &Leader {
                    position: point,
                    yaw: 0.0,
                    input: Vec3::ZERO,
                },
                &mut state,
            );
            if events
                .iter()
                .any(|event| matches!(event, Event::Request(Request::Event(id)) if *id == wanted))
            {
                return index;
            }
        }
        panic!("no area of the map triggers event {wanted}");
    }

    /// A point of the collision floor inside area `index`: the game has the leader walk in;
    /// the test only needs the pose the field task reads, but the pose must be a real floor
    /// of the map, not the volume's centre.
    fn floor_inside_area(app: &App, index: usize) -> Vec3 {
        let field = app.field.as_ref().expect("a map is loaded");
        let centre = field.area_points()[index];
        const OFFSETS: [f32; 9] = [0.0, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1.0, -1.0];
        for probe in [3.0, 2.0, 1.0, 0.5, 0.0, -0.5] {
            for &dz in &OFFSETS {
                for &dx in &OFFSETS {
                    let (x, z) = (centre.x + dx, centre.z + dz);
                    let Some(y) = app.collision.ground_height(x, z, probe) else {
                        continue;
                    };
                    let at = Vec3::new(x, y, z);
                    if field.area_contains(index, at) {
                        return at;
                    }
                }
            }
        }
        panic!("no collision floor inside area {index} around {centre:?}");
    }

    /// A standing point the game's partner search picks for `npc` (the autopilot's source,
    /// `Talk::partners`), with the yaw that faces it.
    fn talk_pose(app: &App, npc: u16) -> (Vec3, f32) {
        let partner = app
            .talk
            .partners()
            .into_iter()
            .find(|partner| partner.npc == npc)
            .unwrap_or_else(|| panic!("NPC {npc} has no talk spot on the loaded map"));
        (partner.position, partner.yaw)
    }

    /// Stands at an actual talk spot and presses the action key, so the trigger event 0 runs
    /// through `Talk::begin` as in the window.
    fn talk_to(app: &mut App, npc: u16) {
        let (position, yaw) = talk_pose(app, npc);
        app.hero = Some(Hero::at(position, yaw));
        app.talk.press(Button::Confirm);
        app.run_steps(1);
    }

    /// Presses the action key until the open conversation closes, as a player reading the
    /// pages would. The first choice is the cursor of every prompt, so a `<YESNO>` gets Yes.
    fn finish_conversation(app: &mut App) {
        for frame in 0..60_000 {
            if frame % 20 == 0 {
                app.talk.press(Button::Confirm);
            }
            app.run_steps(1);
            if !app.talk.is_active() {
                return;
            }
        }
        panic!("the conversation did not close");
    }

    /// `room 1104 1.2..1.2 event 6` / `room 1106 ... event 6`: the two task flags are the
    /// story lock. With both, one step in the house's `0x72` exit fires event 6, which sets
    /// the night (`0xb1(1,0)`), raises the house's game-0 flag (`0x66(0|1)`), advances the
    /// track to 1.3.1 (`0x84(0,1,3,1)`) and selects 1350 or 1360 on arrival. The application
    /// clock follows the night **before** 1100 is loaded (`App::apply_events` starts with
    /// `sync_day_phase`). The `Field::step` call is the one `App::run_steps` makes at the
    /// field's 30 Hz cadence; calling it directly lets the test read the `MapChange` and the
    /// trigger state before `apply_events` loads 1100.
    #[test]
    fn house_exit_requires_both_task_flags_and_advances_to_1_3() {
        let Some(rom) = testrom::load() else { return };
        for (map, house, arrival, game_flag) in
            [("1104", 1104u16, 1360u16, 1i32), ("1106", 1106, 1350, 0)]
        {
            let mut app = App::new_headless(map_options(map, (1, 2, 1, true)), Some(rom.to_vec()));
            assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(house));
            // A bare `--map` starts the day clock at night; the story is daytime, so pin the
            // application clock as `house_exit_time_of_day...` does. What the trigger must
            // change is the clock itself, not a time pinned by `--time`.
            app.day = lighting::day_clock(None, Some(TimeOfDay::Day.index()), None);
            app.time = app.day.phase().index();
            assert_eq!((app.time, app.state.time_of_day), (2, 2));
            app.state.tracks[0].flags[1].extend([3, 4, 5]);

            let exit = app
                .field
                .as_ref()
                .expect("the house is loaded")
                .exits()
                .into_iter()
                .find(|exit| exit.warp.is_some() && exit.destination == Some(1100))
                .expect("the house has its 0x72 exit to the village");
            let events = app.field.as_mut().unwrap().step(
                &Leader {
                    position: exit.position,
                    yaw: 0.0,
                    input: Vec3::ZERO,
                },
                &mut app.state,
            );
            let change = events
                .iter()
                .find_map(|event| match event {
                    Event::MapChange(change) => Some(*change),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{map}: the house exit must change map: {events:?}"));
            assert_eq!(change.destination, 1100, "{map}");
            assert!(
                matches!(change.cause, Cause::Warp { .. }),
                "{map}: the exit is the house's own 0x72 warp, got {:?}",
                change.cause
            );
            assert_eq!(app.state.time_of_day, 0, "{map}: 0xb1(1, 0) sets the night");
            let track = &app.state.tracks[0];
            assert_eq!(
                (track.chapter, track.step, track.substep),
                (1, 3, 1),
                "{map}: 0x84 advances to 1.3.1"
            );
            assert!(
                track.flags[0].contains(&game_flag),
                "{map}: 0x66 raises game-0 flag {game_flag}: {:?}",
                track.flags[0]
            );
            assert!(
                track.flags[1].is_empty(),
                "{map}: 0x84 clears the step's game-1 flags: {:?}",
                track.flags[1]
            );
            // The application lighting flips before the map load: this is the first half of
            // `apply_events`, with 1100 still not loaded.
            assert_eq!(app.day.phase(), TimeOfDay::Day);
            app.sync_day_phase();
            assert_eq!(
                (app.time, app.day.phase()),
                (TimeOfDay::Night.index(), TimeOfDay::Night)
            );
            assert_eq!(
                map_loads(&app, 1100),
                0,
                "the lighting flipped before the load"
            );
            assert!(app.apply_events(events));
            assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
            assert_eq!(map_loads(&app, house), 1, "{map} must not be reloaded");
            assert_eq!(map_loads(&app, 1100), 1);
            assert_eq!((app.time, app.state.time_of_day), (0, 0));
            assert_eq!(app.day.phase(), TimeOfDay::Night);
            assert_eq!(
                app.events.pending_start(),
                Some(arrival),
                "{map}: event 3 of 1100 selects the 1.3 arrival scene"
            );
        }
    }

    /// Without game-1 flag 4 or 5, event 6 does not run: no `0xb1`, no `0x84`, no 1350/1360.
    /// The `0x72` house exit is still the house's **own** warp, so the hero returns to 1100
    /// anyway (`Field::step_warps` passes its destination as the fallback and `Field::collect`
    /// uses it when event 6 has no warp request); the story lock is the track and the night,
    /// not the map change. Game-1 flag 1 is set so the unrelated 1.2 arrival event 3 cannot
    /// queue 21592 and muddy the check.
    #[test]
    fn house_exit_without_a_task_flag_returns_to_1100_without_advancing() {
        let Some(rom) = testrom::load() else { return };
        for map in ["1104", "1106"] {
            for missing in [4i32, 5] {
                let mut app =
                    App::new_headless(map_options(map, (1, 2, 1, true)), Some(rom.to_vec()));
                app.day = lighting::day_clock(None, Some(TimeOfDay::Day.index()), None);
                app.time = app.day.phase().index();
                {
                    let flags = &mut app.state.tracks[0].flags[1];
                    flags.insert(1);
                    for flag in [3, 4, 5] {
                        if flag != missing {
                            flags.insert(flag);
                        }
                    }
                }
                let exit = app
                    .field
                    .as_ref()
                    .expect("the house is loaded")
                    .exits()
                    .into_iter()
                    .find(|exit| exit.warp.is_some() && exit.destination == Some(1100))
                    .expect("the house has its 0x72 exit to the village");
                let events = app.field.as_mut().unwrap().step(
                    &Leader {
                        position: exit.position,
                        yaw: 0.0,
                        input: Vec3::ZERO,
                    },
                    &mut app.state,
                );
                let change = events
                    .iter()
                    .find_map(|event| match event {
                        Event::MapChange(change) => Some(*change),
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "{map} without flag {missing}: the 0x72 warp must remain: {events:?}"
                        )
                    });
                assert_eq!(change.destination, 1100, "{map} without flag {missing}");
                assert!(matches!(change.cause, Cause::Warp { .. }));
                assert_eq!(
                    (
                        app.state.tracks[0].chapter,
                        app.state.tracks[0].step,
                        app.state.tracks[0].substep,
                    ),
                    (1, 2, 1),
                    "{map} without flag {missing}: no 0x84"
                );
                assert_eq!(
                    app.state.time_of_day,
                    TimeOfDay::Day.index(),
                    "{map} without flag {missing}: no 0xb1"
                );
                assert!(
                    !app.state.tracks[0].flags[0].contains(&0)
                        && !app.state.tracks[0].flags[0].contains(&1),
                    "{map} without flag {missing}: no 0x66"
                );
                assert!(app.apply_events(events));
                assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
                assert_eq!(map_loads(&app, 1100), 1);
                assert_eq!(app.time, TimeOfDay::Day.index());
                assert_eq!(app.day.phase(), TimeOfDay::Day);
                assert_eq!(app.state.time_of_day, TimeOfDay::Day.index());
                assert!(
                    !matches!(app.events.pending_start(), Some(1350 | 1360)),
                    "{map} without flag {missing}: no 1.3 arrival scene, pending {:?}",
                    app.events.pending_start()
                );
            }
        }
    }

    /// `room 1100 1.3..1.3 event 2` (added area 12) starts 1410, then `event 0` of NPC 94
    /// starts the wandering soul scene 21595. Its normal end calls native 559, whose 1
    /// selects the `0x1f(1)` branch: track 1.4.1 and the `0x85(4105, 21597)` entry. The
    /// destination loads once, 21597 starts once, 21593 never does, and the night remains.
    #[test]
    fn village_area_12_plays_1410_then_npc_94_takes_the_1_4_1_return() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("1100", (1, 3, 1, false)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
        assert_eq!((app.time, app.state.time_of_day), (0, 0));
        assert_eq!(app.day.phase(), TimeOfDay::Night);

        // The load event added area 12 (`0x8f(12, 3,0,0, 0,-1,-4, 17)`); find it by the
        // trigger its entry fires, then stand on a real floor point inside it.
        let area = area_triggering(&app, 1410);
        let spot = floor_inside_area(&app, area);
        app.hero = Some(Hero::at(spot, 0.0));
        app.run_steps(1);
        assert!(
            app.events.pending_start() == Some(1410) || app.events.started().contains(&1410),
            "the area entry must queue 1410: pending {:?}, started {:?}",
            app.events.pending_start(),
            app.events.started()
        );
        play_pending_event(&mut app, 1410);
        assert!(
            app.state.tracks[0].flags[1].contains(&0),
            "the end of 1410 raises game-1 flag 0"
        );
        assert_eq!(app.state.time_of_day, 0);
        assert_eq!(app.day.phase(), TimeOfDay::Night);

        // NPC 94 (`n007g`) event 0: `if 0x6(94) 0x5(1) then 0x77(21595) 0x72(22159)`.
        talk_to(&mut app, 94);
        assert_eq!(
            app.events.pending_start(),
            Some(21595),
            "NPC 94's trigger queues 21595"
        );
        assert_eq!(app.state.time_of_day, 0);

        // `play_pending_event` keeps going while something is pending, so it also plays the
        // 21597 entry of the 1.4.1 map change and the count below proves it ran once.
        play_pending_event(&mut app, 21595);
        assert_eq!(
            app.state.raw.get(&Raw::Quest490),
            Some(&1),
            "native 559 sets quest manager +0x490 to 1"
        );
        let track = &app.state.tracks[0];
        assert_eq!(
            (track.chapter, track.step, track.substep),
            (1, 4, 1),
            "the 0x1f(1) branch takes story 1.4.1"
        );
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(4105));
        assert_eq!(map_loads(&app, 4105), 1, "4105 loads once");
        assert_eq!(map_loads(&app, 1100), 1, "1100 is not reloaded");
        let started = app.events.started();
        let count = |event: u16| started.iter().filter(|&&id| id == event).count();
        assert_eq!(count(1410), 1, "{started:?}");
        assert_eq!(count(21595), 1, "{started:?}");
        assert_eq!(count(21597), 1, "the 0x85 entry starts once: {started:?}");
        assert_eq!(count(21593), 0, "the fallback must not start: {started:?}");
        assert!(!app.events.is_active());
        assert_eq!(app.events.pending_start(), None);
        assert_eq!(
            (app.time, app.state.time_of_day),
            (0, 0),
            "the return stays at night"
        );
        assert_eq!(app.day.phase(), TimeOfDay::Night);
    }

    /// Action 0x7c is per map: the 1.3 arrival trigger (event 3) fired on the loaded map is
    /// what `load_map` runs after a transition, minus the rebuild that clears the removal
    /// list. Its presentation (1350 for game-0 flag 0, 1360 for flag 1) must not seed NPC
    /// 210 again, and 211–215 are not part of the 1100 cast. A map change rebuilds the cast
    /// and forgets the removals, the documented exception.
    #[test]
    fn the_1_3_arrival_scene_does_not_revive_removed_npcs() {
        let Some(rom) = testrom::load() else { return };
        for (flag, scene) in [(0i32, 1350u16), (1, 1360)] {
            let mut app =
                App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
            assert!(cast_drawn(&app, 210), "NPC 210 stands in the village");
            // Event 11 of 1390: the real end of the keepsake-ring scene.
            let events = app
                .field
                .as_mut()
                .unwrap()
                .event_ended(1390, &mut app.state);
            app.apply_events(events);
            assert!(!cast_drawn(&app, 210), "1390 removes NPC 210");
            assert!(app.state.tracks[0].flags[1].contains(&3));

            // 1.3 arrival: the house's game-0 flag selects 1350 or 1360.
            {
                let track = &mut app.state.tracks[0];
                (track.chapter, track.step, track.substep) = (1, 3, 1);
                track.flags[0].insert(flag);
            }
            let events = app.field.as_mut().unwrap().enter(&mut app.state, true);
            app.apply_events(events);
            assert_eq!(
                app.events.pending_start(),
                Some(scene),
                "event 3 selects the 1.3 arrival scene"
            );
            play_pending_event(&mut app, scene);
            assert!(
                !cast_drawn(&app, 210),
                "the arrival presentation must keep NPC 210 hidden"
            );
            for npc in 211..=215 {
                assert!(
                    !cast_drawn(&app, npc),
                    "NPC {npc} is not part of the 1100 cast"
                );
            }

            // Only a map change forgets the removals.
            assert!(app.load_map("1100", true, None, None, true));
            assert!(
                cast_drawn(&app, 210),
                "a map change rebuilds the cast; 0x7c removals are per map"
            );
        }
    }

    #[test]
    fn trigger_1170_hides_the_tower_gate_and_the_hero_walks_to_x01m03() {
        let Some(rom) = testrom::load() else { return };
        let options = Options {
            map: Some("X01M01".into()),
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story {
                chapter: 1,
                step: 1,
                substep: 1,
                daytime: true,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game: false,
        };
        let mut app = App::new_headless(options, Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(4101));

        // The ramp's low corridor, east of the gate (placement 16) and inside the S1 wall.
        stand_on(&mut app, 1.0, 17.0, 12.0);
        for _ in 0..80 {
            app.walk_step(Vec3::new(-1.0, 0.0, 0.0));
        }
        let blocked = app.hero.unwrap().position();
        assert!(
            blocked.x > 0.5,
            "the map's S1 barrier must block before the trigger: {blocked:?}"
        );

        // The real trigger, not a direct collision call: event 11 of room 4101, after 1170.
        let events = app
            .field
            .as_mut()
            .unwrap()
            .event_ended(1170, &mut app.state);
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::Request(Request::SetPlacementHidden {
                    block: 0,
                    placement: 16,
                    hidden: true
                })
            )),
            "event 11 must hide placement (0, 16): {events:?}"
        );
        app.apply_events(events);
        assert_eq!(app.state.tracks[0].substep, 2, "the story reaches 1.1.2");

        // The gate is gone: the same west walk now crosses it.
        for _ in 0..40 {
            app.walk_step(Vec3::new(-1.0, 0.0, 0.0));
        }
        let through = app.hero.unwrap().position();
        assert!(
            through.x < 0.0,
            "the hidden placement must no longer block: {through:?}"
        );
        // From the 1.1.2 pose, the pilot walks with the d-pad; it must reach X01M03 by the
        // warp, with no placement other than this test's starting pose. The journal is the
        // proof even if it goes on to another map afterwards.
        stand_on(&mut app, 2.77, 18.19, 12.0);
        app.play_auto(120_000, true);
        let crossed = app.journal.steps.iter().any(|(_, step)| {
            matches!(step, Step::Map { id, .. } if *id == 4103)
                || matches!(step, Step::MapChange { to, .. } if *to == 4103)
        });
        assert!(
            crossed,
            "the pilot must walk from X01M01 to X01M03 through warp 9: {:?}",
            app.hero.map(|h| h.position())
        );
    }

    /// The original updates the field on one VBlank out of two: this walks a flat, straight
    /// segment of `X01M05` from the capture's start pose and asserts the exact raw angle and
    /// fx position at every VBlank. Expected values are the verified samples of
    /// `build/v01-20261107/review/cadence-30hz-field-review.md` §7 (original
    /// `original-prep.jsonl` and `Hero::step(ticks = 2)` model): even steps hold the previous
    /// pose, odd steps turn by `0x324 × 2` and accelerate/move by two ticks.
    #[test]
    fn field_cadence_holds_the_pose_and_matches_the_original_every_vblank() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("X01M05", (1, 1, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.tick, 0, "a fresh app starts on phase 0");
        assert!(
            !app.events.is_active() && app.events.pending_start().is_none(),
            "the flat segment must not be interrupted by an event"
        );
        let y = app
            .collision
            .ground_height(1.756, -3.041, 3.0)
            .expect("ground under the capture's start pose");
        // App yaw π is raw angle 0, facing +z at speed 0: the original's prepared state.
        app.hero = Some(Hero::at(Vec3::new(1.756, y, -3.041), std::f32::consts::PI));
        let hero = app.hero.expect("the map placed a hero");
        assert_eq!(hero.angle(), 0);
        assert_eq!(hero.speed(), 0);
        assert_eq!(hero.position_fx(), [7192, (y * 4096.0) as i32, -12455]);

        // `Hero::at` truncates -3.041 × 4096; the original's exact fx is not observable.
        const BASE_Z: i32 = -12455;
        const BASE_X: i32 = 7192;
        const ANGLES: [i32; 10] = [0, 1608, 3216, 4824, 6432, 8040, 9648, 11256, 12864, 12868];
        const Z_OFFSETS: [i32; 10] = [0, 0, -160, -454, -846, -1338, -1928, -2616, -3402, -4188];

        let mut held = app.hero.expect("hero").position_fx();
        let mut held_angle = 0;
        for step in 1..=19u64 {
            app.walk_step(Vec3::NEG_Z);
            assert_eq!(app.tick, step);
            let hero = app.hero.expect("the hero stays placed");
            let position = hero.position_fx();
            if step % 2 == 0 {
                // The intermediate VBlank holds the pose byte for byte; the camera still
                // follows it (per VBlank, not per game update).
                assert_eq!(hero.angle(), held_angle, "angle held on step {step}");
                assert_eq!(position, held, "position held on step {step}");
                assert_eq!(
                    app.camera.target.z,
                    hero.position().z,
                    "camera follows the held pose on step {step}"
                );
                continue;
            }
            let sample = (step as usize - 1) / 2;
            assert_eq!(hero.angle(), ANGLES[sample], "raw angle on step {step}");
            assert!(
                (position[2] - (BASE_Z + Z_OFFSETS[sample])).abs() <= 2,
                "z fx on step {step}: {} (expected {} ± 2)",
                position[2],
                BASE_Z + Z_OFFSETS[sample]
            );
            assert!(
                (position[0] - BASE_X).abs() <= 1,
                "x fx on step {step}: {} (expected {BASE_X} ± 1)",
                position[0]
            );
            held = position;
            held_angle = hero.angle();
        }
        assert_eq!(held_angle, 12868, "the turn clamps on the target");
        assert!(!app.events.is_active());
    }
}
