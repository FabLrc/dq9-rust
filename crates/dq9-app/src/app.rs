//! Window, input and frame loop, shared by the native and Web builds.
//!
//! Updates run at a fixed 60 Hz (the DS frame rate) from an accumulator; rendering happens once
//! per redraw.
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
use dq9_core::cutscene::ClockCommand;
use dq9_core::field::{self, Event, Field, Leader, MapChange};
use dq9_core::fx::reduce_angle;
use dq9_core::scenario::{Journal, Step};
use dq9_core::state::{DayClock, GameState, TimeOfDay};
use dq9_core::trigger::{Request, State};
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
use crate::event::{EventPlayer, Outcome, Stage};
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
    proxy: EventLoopProxy<UserEvent>,
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

    fn set_rom(&mut self, rom: Vec<u8>) {
        match load::maps(&rom) {
            Ok(maps) => self.maps = maps,
            Err(e) => {
                status(&format!("not a Dragon Quest IX ROM: {e:#}"));
                return;
            }
        }
        // A previous save's hero must not survive a ROM or save replacement: the identity is
        // set again below once the new save is read.
        self.talk.set_identity(None);
        self.talk.set_rom(&rom);
        self.rom = Some(rom);
        self.state = field::initial_state(&self.options.story);
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

    #[cfg(target_arch = "wasm32")]
    fn set_save(&mut self, save: Vec<u8>) {
        // Picking a save replaces whatever the page started with (a new game from
        // `?newgame=1`, a map preview): the save selects the map, the position and the
        // scenario. Without this, `set_rom` would keep starting the prologue.
        self.options.new_game = false;
        self.options.map = None;
        self.options.event = None;
        self.options.save = Some(save);
        self.talk.set_identity(None);
        if let Some(rom) = &self.rom {
            self.set_rom(rom.clone());
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

    /// Carries out what the field asks for: the first map change (loading its destination
    /// at the arrival pose), and logs the trigger requests that wait for J3. Returns whether
    /// the map changed.
    fn apply_events(&mut self, events: Vec<Event>) -> bool {
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
                Event::Request(Request::RemoveNpc(npc)) => self.talk.remove_npc(npc),
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
        if self.events.prepare(rom, event, &mut stage) {
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
            if self.options.trace_inputs {
                self.input_trace.push((0, self.talk.confirm_pending()));
            }
            self.step_sound();
            self.step_doors();
            self.tick += 1;
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
                (None, Some(direction)) => Vec3::new(direction.x, 0.0, direction.z),
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
                hero.step(&self.collision, pad, camera_yaw, 1);
                self.camera.follow_hero(hero.position());
                if let Some(field) = &mut self.field {
                    let leader = Leader {
                        position: hero.position(),
                        yaw: hero.yaw(),
                        input: move_direction.normalize_or_zero(),
                    };
                    events.extend(field.step(&leader, &mut self.state));
                }
                if let (Some(runtime), Some(index)) = (&mut self.runtime, self.hero_instance) {
                    // Motion from the distance covered this step (`dq9-render::hero`).
                    let moved = hero.position() - before;
                    let speed = Vec3::new(moved.x, 0.0, moved.z).length() / STEP;
                    runtime.set_hero_gait(Gait::from_speed(speed));
                    let world =
                        runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()));
                    if let Some(instance) = runtime.scene.instances.get_mut(index) {
                        instance.world = world;
                    }
                }
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
            let list = |set: &std::collections::BTreeSet<i32>| {
                set.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
            };
            let _ = write!(
                out,
                "{{\"chapter\":{},\"step\":{},\"substep\":{},\"chapter_flags\":[{}],\"step_flags\":[{}]}}",
                track.chapter,
                track.step,
                track.substep,
                list(&track.flags[0]),
                list(&track.flags[1])
            );
        }
        let flags = self
            .state
            .flags
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let quests = self
            .state
            .quests
            .iter()
            .map(|(quest, state)| format!("\"{quest}\":{state}"))
            .collect::<Vec<_>>()
            .join(",");
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
            "],\"flags\":[{flags}],\"quests\":{{{quests}}},\"event\":null,\"hero\":{{\"x\":{:.3},\"y\":{:.3},\"z\":{:.3},\"yaw\":{:.3}}}}}",
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
        let mut out = String::new();
        for (frame, (bits, confirm)) in self.input_trace.iter().enumerate() {
            let mut keys = Vec::new();
            for (bit, name) in [(1u8, "up"), (2, "down"), (4, "left"), (8, "right")] {
                if bits & bit != 0 {
                    keys.push(format!("\"{name}\""));
                }
            }
            out.push_str(&format!(
                "{{\"frame\":{frame},\"pad\":[{}],\"a\":{}}}\n",
                keys.join(","),
                confirm
            ));
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
        let proxy = self.proxy.clone();
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
                self.talk.set_identity(None);
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
