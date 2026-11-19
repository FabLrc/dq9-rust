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

use std::sync::Arc;

use dq9_core::field::{self, Field, Leader};
use dq9_core::fx::reduce_angle;
use dq9_core::party::Party;
use dq9_core::scenario::Journal;
use dq9_core::state::DayClock;
use dq9_core::trigger::State;
use dq9_core::{CollisionWorld, Hero, Input};
use dq9_formats::hero::HeroLook;
use dq9_render::{AnimatedScene, CameraInput, CameraMode, Gait, Renderer};
use glam::Vec3;
use web_time::Instant;
use winit::event_loop::EventLoopProxy;
use winit::keyboard::KeyCode;
use winit::window::Window;

use crate::audio::Audio;
use crate::event::EventPlayer;
use crate::gpu::Gpu;
use crate::lighting;
use crate::load::{self, MapEntry};
use crate::talk::Talk;

use self::camera::View;
use self::doors::Doors;
use self::draw::Presentation;
use self::encounter::Encounter;
use self::fight::Combat;
use self::input::Controls;
use self::input::{input_bits, pad_direction, pad_input};
use self::options::item_table;
use self::recording::Recording;
use self::services::Services;
use self::services::containers::Containers;

mod auto;
#[cfg(target_arch = "wasm32")]
pub(crate) use self::auto::SessionSpec;
mod camera;
mod daytime;
mod doors;
mod draw;
mod encounter;
mod facility;
mod fight;
#[cfg(not(target_arch = "wasm32"))]
pub use self::fight::BattlePlay;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::fight::regions as fight_regions;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::fight::run_to_phase;
mod flow;
// The checkpoint writer of the pilot lives here: the Web session records the same JSONL the
// native `--checkpoints` writes. The native-only drivers (`--walk`, `--replay`, the
// headless steps) are unused in the wasm build.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) mod headless;
mod input;
mod map;
mod options;
pub use self::options::Options;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::options::story_of;
mod recording;
mod roaming;
mod script;
pub(crate) mod services;
mod window;

#[cfg(all(test, windows))]
mod test_support;
pub use self::window::UserEvent;

/// Fixed update step (60 Hz, the DS frame rate).
pub const STEP: f32 = 1.0 / 60.0;

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
    /// See [`Presentation`].
    presentation: Presentation,
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
    /// See [`Doors`].
    doors: Doors,
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
    /// What the story did since the start, and the fixed step it did it at.
    journal: Journal,
    /// JSONL snapshots of the story state at the checkpoints the oracle compares.
    checkpoints: Vec<String>,
    /// The pilot's run asked for by the Web page (`app/auto/session.rs`).
    session: Option<auto::Session>,
    tick: u64,
    hero_instance: Option<usize>,
    /// The protagonist's look (the save's hero, else a new game's), kept across maps.
    look: HeroLook,
    /// See [`Encounter`].
    encounter: Encounter,
    /// See [`Combat`].
    combat: Combat,
    /// The party: members, gold and bag. Its level tables are applied at the first fight.
    party: Party,
    party_ready: bool,
    /// The roaming monsters of the loaded map and their field models (`app/roaming.rs`).
    roaming: self::roaming::Roaming,
    renderer: Option<Renderer>,
    audio: Audio,
    /// Lighting, fog and background of the loaded map at the selected time of day.
    environment: dq9_render::Environment,
    /// The lighting script of the loaded map (`None` without one).
    lighting_info: Option<dq9_formats::lighting::LightingInfo>,
    /// See [`View`].
    view: View,
    /// See [`Controls`].
    controls: Controls,
    last: Instant,
    accumulator: f32,
    /// Fixed steps completed since the start: the stamp of a record's inputs and digests
    /// (`record.rs`). Unlike `tick` it is never reset, except when the page starts the session
    /// over (`restart_clock`: a recording or a replay counts from its own first step).
    steps: u64,
    /// A windowless app that presents like a window: the field's monsters roam and battles
    /// are shown, not settled in the step that starts them (a replayed human session).
    presented: bool,
    /// See [`Recording`].
    recording: Recording,
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
        Self::with_proxy(Some(proxy), false, options, rom)
    }

    /// The same application without an event loop proxy: no `resumed`/GPU setup, no
    /// `UserEvent` delivery. The native headless tests use it so they never depend on winit
    /// accepting a second `EventLoop` in the same process.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_headless(options: Options, rom: Option<Vec<u8>>) -> Self {
        Self::with_proxy(None, false, options, rom)
    }

    /// A windowless app that plays like a window: roaming monsters and shown battles, so a
    /// recorded human session replays on it (`--play-record`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_replay(options: Options, rom: Option<Vec<u8>>) -> Self {
        Self::with_proxy(None, true, options, rom)
    }

    fn with_proxy(
        proxy: Option<EventLoopProxy<UserEvent>>,
        presented: bool,
        options: Options,
        rom: Option<Vec<u8>>,
    ) -> Self {
        let state = field::initial_state(&options.story);
        let mut audio = Audio::new(options.mute);
        if options.record_sound {
            audio.record();
        }
        let roaming = self::roaming::Roaming::new(proxy.is_some() || presented);
        let mut app = Self {
            proxy,
            options,
            presentation: Presentation::new(),
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
            doors: Doors::new(),
            talk: Talk::default(),
            services: Services::default(),
            containers: Containers::default(),
            events: EventPlayer::new(),
            quiet_events: false,
            cast_reload: false,
            journal: Journal::default(),
            checkpoints: Vec::new(),
            session: None,
            tick: 0,
            hero_instance: None,
            look: HeroLook::new_game(),
            encounter: Encounter::new(),
            combat: Combat::new(),
            party: Party::new_game(),
            party_ready: false,
            roaming,
            renderer: None,
            audio,
            environment: dq9_render::Environment::default(),
            lighting_info: None,
            view: View::new(),
            controls: Controls::new(),
            last: Instant::now(),
            accumulator: 0.0,
            steps: 0,
            presented,
            recording: Recording::new(),
        };
        if let Some(rom) = rom {
            app.set_rom(rom);
        }
        app
    }

    /// An external ROM, save or new game replaces the old event and dialogue together.
    fn reset_session(&mut self) {
        self.events = EventPlayer::new();
        self.talk.reset_session();
        self.services = Services::default();
        self.containers = Containers::default();
        self.doors.doors_opened.clear();
        self.doors.door_angles.clear();
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
        self.combat.world_seeded = false;
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

    fn update(&mut self) {
        if self.session_running() {
            self.session_frame();
            return;
        }
        let now = Instant::now();
        // Clamp long pauses (window dragged, tab hidden) instead of catching up.
        self.accumulator += now.duration_since(self.last).as_secs_f32().min(0.25);
        self.last = now;
        let mut due = 0;
        while self.accumulator >= STEP {
            self.accumulator -= STEP;
            due += 1;
        }
        if self.recording.replay.is_some() {
            self.replay_frame(due);
        } else {
            self.run_steps(due);
        }
    }

    /// Runs `due` fixed steps of the game logic.
    fn run_steps(&mut self, due: u32) {
        for _ in 0..due {
            self.step_begin();
            // `KeyManager::RefreshKeys` (`0x020122ac`): the first frame a button is held
            // raises flag 0x113e. The app's buttons are the d-pad, the action key, the
            // headless pilot's walk and a replayed input.
            let buttons = self.talk.confirm_pending()
                || self.pilot_dir.is_some()
                || self.recording.replay_pad.is_some_and(|bits| bits != 0)
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
                .any(|key| self.controls.held.contains(&key));
            if buttons {
                self.state
                    .flags
                    .insert(i32::from(dq9_core::state::KEY_REFRESH_FLAG));
            }
            if self.options.trace_inputs {
                self.recording
                    .input_trace
                    .push((0, self.talk.confirm_pending()));
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
            let replay = self.recording.replay_pad.take();
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
                rotate: std::mem::take(&mut self.controls.rotate),
                zoom: std::mem::take(&mut self.controls.zoom),
                fast: self.controls.held.contains(&KeyCode::ShiftLeft)
                    || self.controls.held.contains(&KeyCode::ShiftRight),
            };
            let walking = self.view.camera.mode == CameraMode::Orbit;
            if walking || talking {
                input.forward = 0.0;
                input.right = 0.0;
            }
            if walking {
                input.up = 0.0;
            }
            self.view.camera.step(&input, STEP);
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
                && let Some(last) = self.recording.input_trace.last_mut()
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
                reduce_angle((-self.view.camera.yaw * 4096.0) as i32)
            };
            let move_direction = match (replay, piloted) {
                (Some(bits), _) => pad_direction(bits),
                // The d-pad the hero actually presses defines the field's walk direction too
                // (a door compares it): the raw steering vector would make a replay diverge.
                (None, Some(direction)) => {
                    pad_direction(input_bits(Input::from_direction(direction)))
                }
                (None, None) if steering => {
                    let (sin, cos) = self.view.camera.yaw.sin_cos();
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
                self.view.camera.follow_hero(hero.position());
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
                && let Err(e) = runtime.step(&self.view.camera)
            {
                status(&format!("animation update failed: {e}"));
                self.runtime = None;
                self.renderer = None;
                return;
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests;
