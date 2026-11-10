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
use dq9_core::cutscene::PlacementChange;
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

use self::services::Services;
use self::services::containers::{Containers, Taken};
use crate::audio::Audio;
use crate::battle::{self, BattleScene};
use crate::event::{EventPlayer, Outcome, Stage, apply_collision_changes};
use crate::gpu::Gpu;
use crate::hud::{HudInfo, time_label};
use crate::lighting;
use crate::load::{self, MapEntry};
use crate::talk::ServiceTag;
use crate::talk::{Button, Talk};
use dq9_core::party::Party;

use self::fight::FightSummary;
use crate::battle::fight::{Fight, Outcome as FightOutcome, Pad, World};

#[cfg(not(target_arch = "wasm32"))]
mod auto;
mod fight;
#[cfg(not(target_arch = "wasm32"))]
pub use self::fight::BattlePlay;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::fight::regions as fight_regions;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::fight::run_to_phase;
mod flow;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod headless;
mod roaming;
pub(crate) mod services;

/// Fixed update step (60 Hz, the DS frame rate).
pub const STEP: f32 = 1.0 / 60.0;

/// Fixed steps of a battle transition (native 547): the swirl narrows the view for 15, then
/// both screens go to black over the next 20 (`SetMainBrightness(−16, 20)`).
const BATTLE_ENTER_AT: u32 = 15;
const BATTLE_ENTER_DONE: u32 = 35;
/// Steps the scene is shown before the stand-in settles it (there is no battle logic yet).
const BATTLE_HOLD: u32 = 90;
/// Steps the scripted transition holds black when no battle follows it.
const BATTLE_TRANSITION_HOLD: u32 = 90;
/// Steps from the black to the field coming back (state 4, `SetMainBrightness(0, 30)` is the
/// field's own; the fade is 15 here) and the total of the leave.
const BATTLE_LEAVE_AT: u32 = 15;
const BATTLE_LEAVE_DONE: u32 = 45;

/// The black cover's alpha during the 15 → 35 step fade of the transition.
fn fade_in(ticks: u32) -> f32 {
    if ticks <= BATTLE_ENTER_AT {
        0.0
    } else {
        ((ticks - BATTLE_ENTER_AT) as f32 / (BATTLE_ENTER_DONE - BATTLE_ENTER_AT) as f32).min(1.0)
    }
}

/// A battle phase (`docs/app.md`, § "Battle scene"): native 547's transition, the scene,
/// and the way back to the field.
#[derive(Debug)]
enum BattleChange {
    /// Native 547 with no battle asked for (yet): both screens to black, the battle track on.
    Transition { ticks: u32 },
    /// The scene is being built; the screens are black when it appears.
    Entering {
        ticks: u32,
        request: battle::Resolved,
        auto_leave: bool,
    },
    /// The scene is shown; the animations run.
    Fighting {
        ticks: u32,
        request: battle::Resolved,
        auto_leave: bool,
    },
    /// Both screens to black, the field back, then up; the fight's outcome settles the
    /// story when the field is back.
    Leaving {
        ticks: u32,
        request: battle::Resolved,
        restored: bool,
        outcome: Option<FightOutcome>,
    },
}

/// A battle scene a headless run built: no fixed step is spent showing it (the stand-in
/// settles it at once), so the headless timeline stays the story's and recorded input traces
/// replay identically. The window and the Web show the transition itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShownBattle {
    pub stage: u16,
    pub fighters: usize,
}

/// What the field was and what it must be again when the battle is over (native 844: the
/// map's placements are kept as the scene left them across the battle). The map, its cast,
/// its doors, its collision and the party's pose are put back unchanged.
#[derive(Debug)]
struct SavedField {
    current: Option<(MapEntry, String)>,
    runtime: Option<AnimatedScene>,
    collision: CollisionWorld,
    field: Option<Field>,
    hero: Option<Hero>,
    hero_instance: Option<usize>,
    environment: dq9_render::Environment,
    lighting: Option<dq9_formats::lighting::LightingInfo>,
    framed: Option<[glam::Vec3; 2]>,
    camera: Camera,
    camera_home: f32,
}

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
    /// "New game" clicked in the page: starts the prologue (now, or once the ROM is read).
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
    /// Start a new game: the first scene of the story (`docs/app.md`, § New game).
    pub new_game: bool,
    /// Show a battle scene from the start (`--battle`, `?battle=`): an `eventbattle.bin`
    /// record or an explicit stage and its monsters.
    pub battle: Option<crate::battle::Request>,
    /// How battles are played (`--fight`, `--seed`).
    pub play: fight::BattlePlay,
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
    /// The field menu and the facility screens (`app/services.rs`).
    services: Services,
    /// The current map's treasure containers (`app/services/containers.rs`).
    containers: Containers,
    /// Script events: the event task and what it did to the scene.
    events: EventPlayer,
    /// Set while an event loads its map: the map's own `Request::Event`s are not queued.
    quiet_events: bool,
    /// Trigger action 0x8d queued the cast task (`Request::ReloadCast`): it runs once the
    /// events queued before it are over, like the game's task queue.
    cast_reload: bool,
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
    /// The battle scene while a battle runs; the field's scene is set aside in
    /// [`saved_field`](Self::saved_field).
    battle: Option<BattleScene>,
    /// The battle transition or scene phase; `None` at the field.
    battle_change: Option<BattleChange>,
    /// The field's state across a battle (native 844).
    saved_field: Option<SavedField>,
    /// Alpha of the black cover of the transition, 0 to 1.
    battle_cover: f32,
    /// The last battle a headless run built and settled at once ([`ShownBattle`]).
    last_battle: Option<ShownBattle>,
    /// The fight in play (windows and the Web, or `--fight`): the engine, its menus and its
    /// messages; its end settles the battle.
    fight: Option<Box<Fight>>,
    /// Pad presses of the window since the last fixed step, for the fight.
    battle_pad: Pad,
    /// The world's generators the battle draws on (monsters' HP, flight, drops).
    world: World,
    world_seeded: bool,
    /// The party: members, gold and bag. Its level tables are applied at the first fight.
    party: Party,
    party_ready: bool,
    /// The roaming monsters of the loaded map and their field models (`app/roaming.rs`).
    roaming: self::roaming::Roaming,
    /// Where a wiped-out party comes round.
    revival_map: u16,
    /// How the last fight went.
    last_fight: Option<FightSummary>,
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
        let roaming = self::roaming::Roaming::new(proxy.is_some());
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
            services: Services::default(),
            containers: Containers::default(),
            events: EventPlayer::new(),
            quiet_events: false,
            cast_reload: false,
            panel: None,
            journal: Journal::default(),
            #[cfg(not(target_arch = "wasm32"))]
            checkpoints: Vec::new(),
            input_trace: Vec::new(),
            replay_pad: None,
            tick: 0,
            hero_instance: None,
            look: HeroLook::new_game(),
            battle: None,
            battle_change: None,
            saved_field: None,
            battle_cover: 0.0,
            last_battle: None,
            fight: None,
            battle_pad: Default::default(),
            world: World::seeded(0),
            world_seeded: false,
            party: Party::new_game(),
            party_ready: false,
            roaming,
            revival_map: fight::NEW_GAME_REVIVAL_MAP,
            last_fight: None,
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
        self.services = Services::default();
        self.containers = Containers::default();
        self.doors_opened.clear();
        self.door_angles.clear();
        self.roaming.reset_rom();
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
        if let Some(request) = self.options.battle.clone() {
            // A battle preview (`--battle`, `?battle=`): no field behind it, and no
            // automatic return to one.
            self.day = lighting::day_clock(None, self.options.time, self.options.clock);
            self.time = self.day.phase().index();
            self.start_battle(request, false, true);
            return;
        }
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
        // The field's start clears the 700 container refill flags (`func_ov017_0218b688`,
        // `0x0218c180`): a pot, barrel, cupboard or blue chest is full again, a red chest is
        // not. A save carries the refill flags it was written with; a new game has none.
        dq9_core::facility::treasure::reset_field(&mut self.state.flags);
        self.state.compendium = self.item_table.clone();
        self.day = lighting::day_clock(
            start.state.as_ref().map(|s| s.clock),
            self.options.time,
            self.options.clock,
        );
        self.time = self.day.phase().index();
        self.look = start.look.clone();
        self.party = start.party.clone();
        self.party_ready = false;
        self.world_seeded = false;
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
        self.options.battle = None;
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
            lighting::apply_clock_command(&mut self.day, command);
            // The application's time index is the clock's phase (set with it everywhere).
            self.time = self.day.phase().index();
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
        if self.battle_is_active() {
            return;
        }
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
        // A map load tears the talk and menu tasks down; a facility screen does not survive it.
        self.services.close();
        self.talk.abort_service();
        let started = Instant::now();
        let mut loaded = match load::load(rom, key, self.time, &story, spawn, state, &self.look) {
            Ok(l) => l,
            Err(e) => {
                status(&format!("{e:#}"));
                return false;
            }
        };
        let hero_start = loaded.hero.position();
        let summary = loaded.summary();
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
        // The map load builds its cast (`ov_017::0219e384` → `ov_017::021a2fa0`): a cast task
        // queued on the map that is left has nothing more to do.
        self.cast_reload = false;
        self.talk
            .enter_map(loaded.npcs, &mut self.state.talk_counts);
        self.hero = Some(loaded.hero);
        self.hero_instance = Some(loaded.hero_instance);
        let s = &loaded.runtime.scene;
        let milliseconds = started.elapsed().as_secs_f64() * 1000.0;
        // The scene line gets the load time, then the warnings; same bytes as `summary`.
        match summary.split_once('\n') {
            Some((line, warnings)) => {
                status(&format!("{line}, {milliseconds:.0} ms\n{warnings}"));
            }
            None => status(&format!("{summary}, {milliseconds:.0} ms")),
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
        // `Cast_LoadMap` ends with `0x0203e2d4`: a snapshot kept on this map gives its NPCs
        // back, one kept elsewhere is dropped.
        for npc in self
            .events
            .cast_loaded(&mut loaded.runtime, loaded.entry.id)
        {
            self.talk.remove_npc(npc);
        }
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
        // The map's containers (`treasure.nsarc`): its `<map>.bin` and the three tables.
        self.containers = match &self.rom {
            Some(bytes) => match Containers::load(bytes, &loaded.entry.code) {
                Ok(containers) => {
                    if !containers.records().is_empty() {
                        status(&containers.summary());
                    }
                    containers
                }
                Err(e) => {
                    status(&format!("containers: {e:#}"));
                    Containers::default()
                }
            },
            None => Containers::default(),
        };
        self.current = Some((loaded.entry, loaded.label));
        if let Err(e) = loaded.runtime.face_camera(&self.camera) {
            status(&format!("facing cast: {e}"));
            return true;
        }
        self.runtime = Some(loaded.runtime);
        self.enter_roaming();
        self.upload();
        self.update_title();
        if let Some(field) = &mut self.field {
            let mut events = field.enter(&mut self.state, transition);
            // `func_020ae53c`, the map's gimmick initialiser: the gate pair (trigger action
            // 0xa9) follows global flag 0x347 on every load, so a room re-entered after a
            // lost battle gets the open pair back. The application applies it because the
            // gate objects are its scene's placements.
            let gate =
                dq9_core::field::gate_pair(self.state.flags.contains(&dq9_core::field::GATE_FLAG));
            events.splice(0..0, gate.into_iter().map(Event::Request));
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
                Event::Request(Request::ReloadCast) => self.cast_reload = true,
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

    /// `ov_017::021a2fa0`, the cast rebuild of task 0x30 (trigger action 0x8d,
    /// `ov_017::021c16cc`, run by the task queue `ov_017::021a3544`) and of the end of an
    /// event that kept the cast aside (`EventTask_Update`, `ov_017::021bc77c`): the cast is
    /// torn down (`ov_017::021a316c`) and loaded again with `Cast_LoadMap`
    /// (`ov_017::021a2c14`) for the current story point, on the same map, then the snapshot
    /// an event start took on this map gives back its NPCs' poses and removals
    /// (`0x0203e2d4`). The hero, the map's placements, doors, collision and triggers stay.
    /// The fade task 0x30 wraps around it (out over 15 frames, back in over 15) is not
    /// modelled: the cast changes at once.
    fn reload_cast(&mut self) {
        let Some((entry, _)) = &self.current else {
            return;
        };
        let story = load::story_from_state(&self.options.story, &self.state, Some(entry.id));
        let (Some(rom), Some(runtime)) = (self.rom.as_deref(), self.runtime.as_mut()) else {
            return;
        };
        let npcs = match load::reload_cast(rom, runtime, entry.id, &entry.code, &story) {
            Ok(npcs) => npcs,
            Err(e) => {
                status(&format!("{e:#}"));
                return;
            }
        };
        status(&format!(
            "cast of {} rebuilt at {}.{}.{}: {} NPCs",
            entry.id,
            story.chapter,
            story.step,
            story.substep,
            npcs.len()
        ));
        self.talk.reload_npcs(npcs);
        for npc in self.events.cast_rebuilt(runtime, entry.id) {
            self.talk.remove_npc(npc);
        }
        if let Err(e) = runtime.face_camera(&self.camera) {
            status(&format!("facing cast: {e}"));
        }
        self.upload();
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
        let Some(gpu) = &self.gpu else {
            return;
        };
        let (scene, environment) = match (&self.battle, &self.runtime) {
            (Some(battle), _) => (&battle.runtime().scene, battle.environment.clone()),
            (None, Some(runtime)) => (&runtime.scene, self.environment.clone()),
            (None, None) => return,
        };
        self.renderer = Some(Renderer::new_with(
            &gpu.device,
            &gpu.queue,
            gpu.config.format,
            scene,
            self.options.layout == Layout::Wide,
        ));
        if let Some(renderer) = &mut self.renderer {
            renderer.environment = environment;
            renderer.environment.cover =
                (self.battle_cover > 0.0).then_some([0.0, 0.0, 0.0, self.battle_cover]);
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
        if self.battle_is_active() {
            return;
        }
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

    /// Changes the presentation; entering or leaving `wide` rebuilds the renderer because the
    /// cinematic mode changes texture filtering and adds the post pass.
    fn set_layout(&mut self, layout: Layout) {
        if layout == self.options.layout {
            return;
        }
        let rebuild = layout.is_wide() != self.options.layout.is_wide();
        self.options.layout = layout;
        if rebuild {
            self.upload();
        }
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, pressed: bool) {
        if !pressed {
            self.held.remove(&code);
            return;
        }
        if !self.held.insert(code) {
            return; // key repeat
        }
        if self.fight.is_some() {
            // The fight reads the pad itself; the conversation's presses must not pile up.
            match code {
                KeyCode::KeyM => self.audio.toggle_mute(),
                KeyCode::KeyR => self.set_layout(self.options.layout.next()),
                _ => fight::pad_press(&mut self.battle_pad, code),
            }
            return;
        }
        if self.services.is_open() {
            match code {
                KeyCode::KeyM => self.audio.toggle_mute(),
                KeyCode::KeyR => self.set_layout(self.options.layout.next()),
                KeyCode::Enter | KeyCode::Space | KeyCode::KeyZ => {
                    self.service_button(Button::Confirm);
                }
                KeyCode::Backspace | KeyCode::KeyX | KeyCode::Escape => {
                    self.service_button(Button::Cancel);
                }
                KeyCode::ArrowUp | KeyCode::KeyW => self.service_button(Button::Up),
                KeyCode::ArrowDown | KeyCode::KeyS => self.service_button(Button::Down),
                KeyCode::ArrowLeft | KeyCode::KeyA => self.service_side(false),
                KeyCode::ArrowRight | KeyCode::KeyD => self.service_side(true),
                KeyCode::KeyH => self.service_hold(),
                KeyCode::KeyT => self.service_take(),
                _ => {}
            }
            return;
        }
        if code == KeyCode::KeyI
            && !self.talk.is_active()
            && !self.battle_is_active()
            && !self.events.is_active()
            && self.events.pending_start().is_none()
            && self.open_field_menu()
        {
            return;
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
            KeyCode::KeyR => self.set_layout(self.options.layout.next()),
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

    /// The party members' names as the font draws them, for the menu's rows.
    fn service_names(&self) -> Vec<Vec<u8>> {
        let hero = self.talk.hero_name();
        self.party
            .members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                if index == 0 {
                    return hero.clone();
                }
                self.talk
                    .with_ui(|_, font| {
                        let mut name = Vec::new();
                        for &glyph in member.name() {
                            match font.glyphs.get(usize::from(glyph)) {
                                Some(glyph) => name.extend_from_slice(glyph.text),
                                None => name.push(b'?'),
                            }
                        }
                        name
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    /// `I` on the field: opens the menu, unless a conversation, an event or a battle holds
    /// the player. Returns whether the menu opened.
    fn open_field_menu(&mut self) -> bool {
        let Some(rom) = self.rom.as_deref() else {
            return false;
        };
        let names = self.service_names();
        self.services.set_names(names);
        self.talk.discard_input();
        match self.services.open_menu(rom, &mut self.party) {
            Ok(()) => {
                status("field menu");
                true
            }
            Err(e) => {
                status(&format!("field menu unavailable: {e:#}"));
                false
            }
        }
    }

    /// The facility a talk line named.
    fn open_service(&mut self, tag: ServiceTag) -> bool {
        let Some(rom) = self.rom.as_deref() else {
            return false;
        };
        let names = self.service_names();
        self.services.set_names(names);
        self.talk.discard_input();
        match self.services.open(tag, rom, &mut self.party, &self.day) {
            Ok(()) => {
                status(&format!("service: {tag:?}"));
                true
            }
            Err(e) => {
                status(&format!("service {tag:?} unavailable: {e:#}"));
                false
            }
        }
    }

    /// The action key on a container: opens the one the hero faces, gives its content and
    /// shows a placeholder line. Returns whether a container was there: the key is spent even
    /// when it refuses (`AlreadyOpen`, a lock). The draw comes from the **world** generator
    /// (`GetBTRandom`, `0x020741e0`; the core rule labels which generator is open
    /// **[hypothesis]**).
    fn examine_container(&mut self) -> bool {
        let Some(hero) = &self.hero else {
            return false;
        };
        let (position, yaw) = (hero.position(), hero.yaw());
        let Some((container, open)) =
            self.containers
                .open(position, yaw, &mut self.state, &mut self.world.random)
        else {
            return false;
        };
        let taken = Taken::of(open);
        let stored = taken.give(&mut self.party, &mut self.state);
        status(&format!(
            "container {:?} #{} (id {}): {}",
            container.container,
            container.number,
            container.id,
            taken.line()
        ));
        if let (false, Taken::Item(id)) = (stored, taken) {
            status(&format!(
                "container #{}: item {id} did not fit the bag (the flag stays set)",
                container.number
            ));
        }
        if let Err(e) = self.talk.show_event(&taken.text()) {
            status(&format!("container #{}: {e:#}", container.number));
        }
        true
    }

    fn service_button(&mut self, button: Button) {
        let effect = self.services.press(
            button,
            &mut self.party,
            &mut self.world.random,
            &mut self.day,
        );
        self.after_service(effect);
    }

    fn service_side(&mut self, right: bool) {
        self.services.side(right);
    }

    fn service_hold(&mut self) {
        self.services.hold(&self.party);
    }

    fn service_take(&mut self) {
        self.services.take(&mut self.party);
    }

    /// What follows a command: the facility's conversation ends when its screen closes, the
    /// lighting follows an inn's clock and the field's scene is rebuilt once equipment
    /// changed and the menu is gone.
    fn after_service(&mut self, effect: services::Effect) {
        if !self.services.is_open() {
            self.talk.finish_service();
        }
        if effect == services::Effect::Clock {
            self.sync_clock_from_day();
        }
        if !self.services.is_open() && self.services.look_dirty() {
            self.services.clear_look_dirty();
            self.refresh_hero_look();
        }
    }

    /// The inn moved the day clock: the application's time, lighting and the trigger state's
    /// `time_of_day` follow (`GameState::SetTimeOfDay`, `0x02010374`).
    fn sync_clock_from_day(&mut self) {
        self.time = self.day.phase().index();
        self.state.time_of_day = self.time;
        self.environment = lighting::environment(self.lighting_info.as_ref(), self.day);
        if let Some(renderer) = &mut self.renderer {
            renderer.environment = self.environment.clone();
        }
    }

    /// Rebuilds only the hero the renderer draws for the leader's current equipment, through
    /// [`AnimatedScene::replace_hero`]: the same model and instance slots are reused in place
    /// and obsolete hero textures are released. The field, its cast, talk counters, doors,
    /// camera, events and generators are untouched: this must not reload the map, whose entry
    /// would replay its triggers (`sync_day_phase` warns against it).
    fn refresh_hero_look(&mut self) {
        let mut look = self.look.clone();
        if let Some(member) = self.party.leader() {
            look.armour = member.equipment.torso;
            look.legwear = member.equipment.legs;
            look.gloves = member.equipment.arms;
            look.footwear = member.equipment.feet;
            look.headgear = member.equipment.head;
        }
        // The rig draws no weapon, shield or accessory: when the visible fields did not move
        // (same clothing, a weapon-only swap, equip then undo), leave the current rig alone.
        if look == self.look {
            return;
        }
        self.look = look;
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let Some(hero) = self.hero else {
            return;
        };
        let image = match nds_rom::Rom::parse(rom) {
            Ok(image) => image,
            Err(e) => {
                status(&format!("hero look: {e:#}"));
                return;
            }
        };
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        // The same scene model and instance are reused in place; a malformed new hero
        // leaves the previous rig and its storage untouched.
        match runtime.replace_hero(&self.look, |path| dq9_formats::map::rom_file(&image, path)) {
            Ok(()) => {
                if let Some(instance) = self.hero_instance {
                    let world =
                        runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()));
                    if let Some(shown) = runtime.scene.instances.get_mut(instance) {
                        shown.world = world;
                    }
                }
                self.upload();
            }
            Err(e) => status(&format!("hero look: {e:#}")),
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
            let spawn = crate::event::spawn(&place);
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
        // `ov_001::02154f24`: an event whose list record has bit 0 or 2 in its flags records
        // the cast (`0x0203e0b0`) for the end of the event to give it back.
        if place.is_some_and(|place| place.keeps_cast())
            && let (Some((entry, _)), Some(scene)) = (&self.current, self.runtime.as_ref())
        {
            self.events.keep_cast(scene, entry.id);
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
                    // `EventTask_Update` (`ov_017::021bc77c`) rebuilds the cast an event kept
                    // aside (`ov_017::021a2fa0`, task `+0x90` or `+0x50 & 4`) before it fires
                    // trigger event 11.
                    if self
                        .rom
                        .as_deref()
                        .and_then(|rom| crate::event::place(rom, id))
                        .is_some_and(|place| place.keeps_cast())
                    {
                        self.reload_cast();
                    }
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
                Outcome::BattleTransition => self.battle_transition(),
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
            if self.battle_is_active() {
                // The field waits for the transition and the battle (`func_ov017_021b6f18`
                // runs the transition ahead of the encounter); the player has no control.
                self.tick += 1;
                self.step_battle();
                continue;
            }
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
            // The cast task of action 0x8d waits behind the events and the conversation
            // queued before it, and holds the field for its step.
            if self.cast_reload && !self.talk.is_active() {
                self.cast_reload = false;
                self.reload_cast();
                continue;
            }
            let mut events = Vec::new();
            let menu_open = self.services.is_open();
            if !self.talk.is_active() && !menu_open && self.talk.confirm_pending() {
                // A container in front takes the key when no conversation partner does: the
                // original's containers are "things to examine", reached by the action key.
                let partner = self
                    .hero
                    .as_ref()
                    .is_some_and(|hero| self.talk.has_partner(hero));
                if !partner && self.examine_container() {
                    // The key was spent on a container.
                } else if let (Some(rom), Some(hero), Some(field), Some((entry, _))) =
                    (&self.rom, &self.hero, &mut self.field, &self.current)
                {
                    events = self
                        .talk
                        .begin(rom, &entry.code, hero, field, &mut self.state);
                }
                self.talk.discard_input();
            }
            self.talk.step();
            // A line that names a facility opens its screen; the conversation's end waits
            // until that screen is over (`Talk::finish_service`).
            if let Some(tag) = self.talk.take_service()
                && !self.open_service(tag)
            {
                self.talk.finish_service();
            }
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
            let menu_open = self.services.is_open();
            let talking = self.talk.is_active() || menu_open;
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
                last.0 = input_bits(pad);
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
            let mut stepped = false;
            if let Some(hero) = &mut self.hero {
                let before = hero.position();
                if field_due && !menu_open {
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
                    stepped = hero.position() != before;
                }
                // Held pose on the intermediate VBlank: the camera still follows it, as the
                // original's follow runs per VBlank, not per game update.
                self.camera.follow_hero(hero.position());
            }
            if self.apply_events(events) {
                continue;
            }
            if field_due
                && !talking
                && !menu_open
                && let Some(contact) = self.step_roaming(stepped)
            {
                self.begin_encounter(contact);
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

    fn redraw(&mut self) {
        self.update();
        if self.battle().is_some() {
            let camera = self.draw_battle();
            self.present_frame(&camera, false, false);
            return;
        }
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
            // The roaming monsters move each field step (at most twelve instances).
            for (index, world) in self.roaming.instance_worlds(runtime) {
                renderer.update_instance_world(&gpu.queue, index, world);
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
        let camera = self.camera;
        let swapped = self.events.screen_swapped();
        self.present_frame(&camera, swapped, true);
    }

    /// Presents one frame of the active scene: the 3D screen, the panel that fills the freed
    /// screen, and the message window when the field draws one. `swapped` is the script's
    /// screen swap (natives 109/110); the battle keeps the default layout.
    fn present_frame(&mut self, camera: &Camera, swapped: bool, messages: bool) {
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
            camera,
        );
        if let Some(overlay) = screens.overlay(swapped) {
            panel.draw(&mut encoder, &view, overlay);
        }
        if messages {
            if self.options.layout.is_wide() {
                if let Some((entry, label)) = &self.current {
                    let info = HudInfo {
                        title: label,
                        code: &entry.code,
                        time: time_label(self.day.phase()),
                        party: Some(&self.party),
                    };
                    self.talk.draw_hud(
                        &gpu.device,
                        &gpu.queue,
                        gpu.config.format,
                        &mut encoder,
                        &view,
                        field,
                        &info,
                    );
                }
            } else {
                self.talk.draw(
                    &gpu.device,
                    &gpu.queue,
                    gpu.config.format,
                    &mut encoder,
                    &view,
                    field,
                );
            }
        }
        if messages
            && self.services.is_open()
            && let Some(quads) = self
                .talk
                .with_ui(|atlas, font| self.services.quads(atlas, font, &self.party))
                .flatten()
        {
            // The menu takes the screen the field does not use; a one-screen layout lays it
            // over the field so that it stays visible.
            let region = screens.overlay(swapped).unwrap_or(screens.top);
            self.talk.draw_screens(
                &gpu.device,
                &gpu.queue,
                gpu.config.format,
                &mut encoder,
                &view,
                &[(region, quads)],
            );
        }
        if self.battle_cover <= 0.0
            && let (Some(fight), Some(scene)) = (&self.fight, &self.battle)
        {
            fight::draw_ui(
                &mut self.talk,
                fight,
                scene,
                gpu,
                &mut encoder,
                &view,
                &screens,
                self.tick as u32,
            );
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
    }
}

impl App {
    /// The battle scene, when one is shown (tests and the next wave's UI).
    pub fn battle(&self) -> Option<&BattleScene> {
        self.battle.as_ref()
    }

    /// Whether a battle transition or scene holds the game.
    pub fn battle_is_active(&self) -> bool {
        self.battle_change.is_some()
    }

    /// The last battle a headless run built and settled at once (window and Web runs do not
    /// set it: they show the scene).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn last_battle(&self) -> Option<ShownBattle> {
        self.last_battle
    }

    /// Native 547 executed by the event player: the scripted battle's transition. The fight
    /// itself is a trigger's `Request::Battle` (or [`App::run_battle`]); the transition blacks
    /// both screens and starts the battle track, and gives the field back after
    /// [`BATTLE_TRANSITION_HOLD`] steps when no battle follows.
    pub(crate) fn battle_transition(&mut self) {
        if self.proxy.is_none() {
            // Headless: no fixed step is spent on a presentation (the recorded traces replay
            // exactly, `scripts/parity.ps1`).
            status("battle transition (native 547): headless, no step spent");
            return;
        }
        if self.battle_change.is_some() && self.battle.is_none() {
            return;
        }
        if self.battle_change.is_some() {
            return;
        }
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        self.audio.set_sequence(rom, Some(battle::BATTLE_TRACK));
        status("battle transition (native 547): the screens go to black");
        self.battle_change = Some(BattleChange::Transition { ticks: 0 });
    }

    /// Starts a battle: the record (or the explicit stage and monsters) is read now, then the
    /// transition builds the scene once both screens are black and the fight starts on it.
    /// `auto_leave` settles it after [`BATTLE_HOLD`] steps when no fight could be built (the
    /// J4 stand-in); `immediate` skips a transition that has no field to fade from. A
    /// headless run keeps the stand-in unless `--fight` asks for the engine.
    pub(crate) fn start_battle(
        &mut self,
        request: battle::Request,
        auto_leave: bool,
        immediate: bool,
    ) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let resolved = match request.resolve(rom) {
            Ok(resolved) => resolved,
            Err(e) => {
                status(&format!("battle: {e:#}"));
                return;
            }
        };
        self.audio.set_sequence(rom, Some(resolved.track));
        status(&format!(
            "battle{}: stage {}, monsters {:?}, track {}",
            resolved
                .number
                .map_or_else(String::new, |n| format!(" {n}")),
            resolved.stage,
            resolved.monsters,
            resolved.track
        ));
        if self.proxy.is_none() {
            // Headless: the scene is built (and checked) but no fixed step is spent showing
            // it, so the story's timeline is exactly the one recorded traces describe. The
            // stand-in then settles it as before, or `--fight` plays it with the engine.
            if self.enter_battle(&resolved) {
                self.last_battle = self.battle.as_ref().map(|scene| ShownBattle {
                    stage: scene.stage(),
                    fighters: scene.fighters().len(),
                });
                self.restore_field();
            }
            self.set_battle_cover(0.0);
            if self.options.play.fight {
                self.play_headless_fight(&resolved);
            } else {
                self.settle_battle(&resolved, None);
            }
            return;
        }
        let ticks = if immediate { BATTLE_ENTER_DONE } else { 0 };
        self.battle_change = Some(BattleChange::Entering {
            ticks,
            request: resolved,
            auto_leave,
        });
    }

    /// One fixed step of the battle phase. Called before the field when a transition or a
    /// battle runs: the player has no control and the field's tasks wait.
    fn step_battle(&mut self) {
        let Some(mut change) = self.battle_change.take() else {
            return;
        };
        let mut finished = false;
        match &mut change {
            BattleChange::Transition { ticks } => {
                *ticks += 1;
                let alpha = fade_in(*ticks);
                self.set_battle_cover(alpha);
                if *ticks >= BATTLE_TRANSITION_HOLD + 20 {
                    // No battle followed: the field comes back.
                    self.set_battle_cover(0.0);
                    if let Some(map) = self.current.as_ref().map(|(entry, _)| entry.id) {
                        self.restore_field_music(map);
                    }
                    finished = true;
                }
            }
            BattleChange::Entering {
                ticks,
                request,
                auto_leave,
            } => {
                *ticks += 1;
                self.set_battle_cover(fade_in(*ticks));
                if *ticks >= BATTLE_ENTER_DONE {
                    let request = request.clone();
                    let auto_leave = *auto_leave;
                    if self.enter_battle(&request) {
                        // The fight starts on the scene; one that cannot be built leaves the
                        // stand-in to settle the battle after its hold.
                        self.fight = self.make_fight(&request);
                        self.battle_pad = Default::default();
                        self.talk.discard_input();
                        change = BattleChange::Fighting {
                            ticks: 0,
                            request,
                            auto_leave,
                        };
                    } else {
                        // No scene: the story does not stop on it (the stand-in of J4).
                        self.set_battle_cover(0.0);
                        self.settle_battle(&request, None);
                        finished = true;
                    }
                }
            }
            BattleChange::Fighting {
                ticks,
                request,
                auto_leave,
            } => {
                if let Some(scene) = &mut self.battle
                    && let Err(e) = scene.step()
                {
                    status(&format!("battle animation: {e:#}"));
                }
                // The screens come up over 15 frames (`0x021d9d18`).
                let alpha = (1.0 - *ticks as f32 / 15.0).max(0.0);
                *ticks += 1;
                self.set_battle_cover(alpha);
                if self.fight.is_some() {
                    if let Some(outcome) = self.drive_fight() {
                        let request = request.clone();
                        change = BattleChange::Leaving {
                            ticks: 0,
                            request,
                            restored: false,
                            outcome: Some(outcome),
                        };
                    }
                } else if *auto_leave && *ticks >= BATTLE_HOLD {
                    let request = request.clone();
                    change = BattleChange::Leaving {
                        ticks: 0,
                        request,
                        restored: false,
                        outcome: None,
                    };
                }
            }
            BattleChange::Leaving {
                ticks,
                request,
                restored,
                outcome,
            } => {
                *ticks += 1;
                if *ticks < BATTLE_LEAVE_AT {
                    self.set_battle_cover(fade_in(*ticks));
                } else {
                    if !*restored {
                        self.restore_field();
                        *restored = true;
                    }
                    let alpha = 1.0
                        - (*ticks - BATTLE_LEAVE_AT) as f32
                            / (BATTLE_LEAVE_DONE - BATTLE_LEAVE_AT) as f32;
                    self.set_battle_cover(alpha.max(0.0));
                }
                if *ticks >= BATTLE_LEAVE_DONE {
                    self.set_battle_cover(0.0);
                    let request = request.clone();
                    let outcome = outcome.take();
                    finished = true;
                    self.settle_battle(&request, outcome);
                }
            }
        }
        if !finished {
            self.battle_change = Some(change);
        }
    }

    /// The black cover of the transition: both screens (`SetMainBrightness`/`SetSubBrightness`).
    /// The renderer applies it to the 3D screen it draws; the panel covers the other one.
    fn set_battle_cover(&mut self, alpha: f32) {
        self.battle_cover = alpha.clamp(0.0, 1.0);
        let cover = (self.battle_cover > 0.0).then_some([0.0, 0.0, 0.0, self.battle_cover]);
        if self.battle.is_some() {
            if let Some(renderer) = &mut self.renderer {
                renderer.environment.cover = cover;
            }
        } else {
            self.environment.cover = cover;
            if let Some(renderer) = &mut self.renderer {
                renderer.environment.cover = cover;
            }
        }
    }

    /// Builds the scene of `request` and swaps it in for the field's.
    fn enter_battle(&mut self, request: &battle::Resolved) -> bool {
        let Some(rom) = self.rom.as_deref() else {
            return false;
        };
        let story = load::story_from_state(&self.options.story, &self.state, Some(request.stage));
        let built = BattleScene::build(rom, request, self.time, self.day, &story, &self.look);
        let scene = match built {
            Ok(scene) => scene,
            Err(e) => {
                status(&format!("battle: {e:#}"));
                return false;
            }
        };
        status(&format!(
            "battle {} on stage {} {}: {} fighters, {} triangles",
            request.number.map_or_else(|| "-".into(), |n| n.to_string()),
            request.stage,
            scene.label(),
            scene.fighters().len(),
            scene.runtime().scene.triangle_count()
        ));
        self.save_field();
        let entry = scene.entry().clone();
        let label = scene.label().to_owned();
        self.lighting_info = scene.lighting().cloned();
        self.current = Some((entry, label));
        self.runtime = None;
        self.battle = Some(scene);
        self.upload();
        self.update_title();
        true
    }

    /// Sets the field aside: native 844 records the placements so that the battle leaves them
    /// as they were. The scene, the collision, the doors and the party's pose are put back
    /// unchanged when the battle is over.
    fn save_field(&mut self) {
        let mut environment = self.environment.clone();
        environment.cover = None;
        self.saved_field = Some(SavedField {
            current: self.current.take(),
            runtime: self.runtime.take(),
            collision: std::mem::take(&mut self.collision),
            field: self.field.take(),
            hero: self.hero,
            hero_instance: self.hero_instance,
            environment,
            lighting: self.lighting_info.take(),
            framed: self.framed,
            camera: self.camera,
            camera_home: self.camera_home,
        });
    }

    /// Gives the field back after the battle: the same map, cast, doors, collision and party
    /// pose (`0x02017a94` restores the placements 844 recorded; nothing here moved them).
    fn restore_field(&mut self) {
        let Some(saved) = self.saved_field.take() else {
            return;
        };
        // A standalone preview has no field to restore. Keep its stage and battle camera;
        // clearing only the CPU scene would leave its GPU geometry under the field camera.
        if saved.runtime.is_none() && saved.field.is_none() {
            return;
        }
        self.battle = None;
        self.current = saved.current;
        self.runtime = saved.runtime;
        self.collision = saved.collision;
        self.field = saved.field;
        self.hero = saved.hero;
        self.hero_instance = saved.hero_instance;
        self.environment = saved.environment;
        self.lighting_info = saved.lighting;
        self.framed = saved.framed;
        self.camera = saved.camera;
        self.camera_home = saved.camera_home;
        if let Some((entry, _)) = &self.current {
            let map = entry.id;
            self.restore_field_music(map);
        }
        self.upload();
        self.update_title();
    }

    /// The field's tune again (`func_0209c530`), from the map and the story's flags.
    fn restore_field_music(&mut self, map: u16) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let flags = |flag: u16| self.state.flags.contains(&i32::from(flag));
        self.audio.set_map(rom, map, self.time, &flags);
    }

    /// Draws the battle scene once: the animations at the battle camera, its light and the
    /// transition's cover.
    fn draw_battle(&mut self) -> Camera {
        let Some(camera) = self.battle.as_ref().map(|scene| *scene.camera()) else {
            return self.camera;
        };
        let cover = (self.battle_cover > 0.0).then_some([0.0, 0.0, 0.0, self.battle_cover]);
        let (Some(gpu), Some(renderer), Some(battle)) =
            (&self.gpu, &mut self.renderer, &mut self.battle)
        else {
            return camera;
        };
        if let Err(e) = battle.upload(renderer, &gpu.device, &gpu.queue) {
            status(&format!("battle upload: {e:#}"));
        }
        renderer.environment = battle.environment.clone();
        renderer.environment.cover = cover;
        camera
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
            UserEvent::Layout(layout) => self.set_layout(layout),
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

#[cfg(all(test, windows))]
mod tests;
