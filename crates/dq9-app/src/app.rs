//! Window, input and frame loop, shared by the native and Web builds.
//!
//! Updates run at a fixed 60 Hz (the DS frame rate) from an accumulator; rendering happens once
//! per redraw.
//!
//! Controls: drag with the left button to turn, wheel to zoom, `WASD`/arrows to move (the hero
//! in orbit mode, the camera in fly mode), `Q`/`E` the game's `L`/`R` camera buttons in orbit
//! mode (both together return the camera) and down/up in fly mode, `Shift` faster camera, `Tab`
//! orbit/fly, `F` reframe, `R` DS aspect (256×192), `T` next
//! time of day, `M` mute the music, `PageUp`/`PageDown` previous/next map, `Enter`/`Space` talk to the NPC in front
//! of the hero and advance the message window, `Backspace`/`X` answer No.

use std::collections::HashSet;
use std::sync::Arc;

use dq9_core::camera::FieldCamera;
use dq9_core::field::{self, Event, Field, Leader, MapChange};
use dq9_core::fx::reduce_angle;
use dq9_core::state::{DayClock, GameState, TimeOfDay};
use dq9_core::trigger::State;
use dq9_core::{CollisionWorld, Hero, Input};
use dq9_formats::hero::HeroLook;
use dq9_render::camera::{DS_ASPECT, letterbox};
use dq9_render::{AnimatedScene, Camera, CameraInput, CameraMode, Gait, Renderer, Viewport};
use glam::{Vec2, Vec3};
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::audio::Audio;
use crate::gpu::Gpu;
use crate::lighting;
use crate::load::{self, MapEntry};
use crate::talk::{Button, Talk};

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
    pub ds_aspect: bool,
    /// No music, and no sound device opened until `M` unmutes.
    pub mute: bool,
    pub story: dq9_formats::cast::Story,
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
    /// NPC conversations and the message window.
    talk: Talk,
    hero_instance: Option<usize>,
    /// The protagonist's look (the save's hero, else a new game's), kept across maps.
    look: HeroLook,
    renderer: Option<Renderer>,
    audio: Audio,
    /// Lighting, fog and background of the loaded map at the selected time of day.
    environment: dq9_render::Environment,
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
        let audio = Audio::new(options.mute);
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
            talk: Talk::default(),
            hero_instance: None,
            look: HeroLook::new_game(),
            renderer: None,
            audio,
            environment: dq9_render::Environment::default(),
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
        self.talk.set_rom(&rom);
        self.rom = Some(rom);
        self.state = field::initial_state(&self.options.story);
        let start = match load::start(self.options.map.as_deref(), self.options.save.as_deref()) {
            Ok(start) => start,
            Err(e) => {
                status(&format!("{e:#}"));
                return;
            }
        };
        self.day = lighting::day_clock(
            start.state.as_ref().map(|s| s.clock),
            self.options.time,
            self.options.clock,
        );
        self.time = self.day.phase().index();
        self.look = start.look.clone();
        self.load(&start.map, true, None, start.state.as_ref());
    }

    #[cfg(target_arch = "wasm32")]
    fn set_save(&mut self, save: Vec<u8>) {
        self.options.save = Some(save);
        if let Some(rom) = &self.rom {
            self.set_rom(rom.clone());
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
        let Some(rom) = &self.rom else { return false };
        let started = Instant::now();
        let mut loaded = match load::load(
            rom,
            key,
            self.time,
            &self.options.story,
            spawn,
            state,
            &self.look,
        ) {
            Ok(l) => l,
            Err(e) => {
                status(&format!("{e:#}"));
                return false;
            }
        };
        let hero_start = loaded.hero.position();
        self.environment = lighting::environment(loaded.lighting.as_ref(), self.day);
        self.collision = loaded.collision;
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
            if let Some(bounds) = s.bounds() {
                self.camera.frame(bounds);
            }
            self.framed = s.bounds();
        }
        if loaded.placed {
            self.camera.yaw = loaded.hero.yaw();
        }
        self.camera_home = self.camera.yaw;
        self.field_camera = FieldCamera::default();
        self.camera.target = hero_start;
        let flags = |flag: u16| self.state.flags.contains(&i32::from(flag));
        self.audio.set_map(rom, loaded.entry.id, self.time, &flags);
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
                Event::Request(request) => {
                    status(&format!(
                        "trigger request not supported before J3: {request:?}"
                    ));
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
            "dq9-app: {} {} ({label}), time {}, {mode}",
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
                if let Some(bounds) = self.framed {
                    self.camera.frame(bounds);
                }
            }
            KeyCode::KeyM => self.audio.toggle_mute(),
            KeyCode::KeyR => self.options.ds_aspect = !self.options.ds_aspect,
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
        while self.accumulator >= STEP {
            self.accumulator -= STEP;
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
            let talking = self.talk.is_active();
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
            let steering = walking && !talking;
            let pad = Input {
                up: steering && self.axis(&[KeyCode::KeyW, KeyCode::ArrowUp], &[]) > 0.0,
                down: steering && self.axis(&[KeyCode::KeyS, KeyCode::ArrowDown], &[]) > 0.0,
                left: steering && self.axis(&[KeyCode::KeyA, KeyCode::ArrowLeft], &[]) > 0.0,
                right: steering && self.axis(&[KeyCode::KeyD, KeyCode::ArrowRight], &[]) > 0.0,
            };
            if steering {
                self.turn_camera();
            }
            // The game's camera yaw turns the other way round from the application's.
            let camera_yaw = reduce_angle((-self.camera.yaw * 4096.0) as i32);
            let move_direction = if steering {
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
            } else {
                Vec3::ZERO
            };
            if let Some(hero) = &mut self.hero {
                let before = hero.position();
                hero.step(&self.collision, pad, camera_yaw, 1);
                self.camera.target += hero.position() - before;
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
        if let (Some(gpu), Some(renderer), Some(runtime), Some(hero), Some(index)) = (
            &self.gpu,
            &mut self.renderer,
            &self.runtime,
            &self.hero,
            self.hero_instance,
        ) && !renderer.update_instance_world(
            &gpu.queue,
            index,
            runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw())),
        ) {
            status("hero instance is unavailable");
        }
        let (Some(gpu), Some(renderer)) = (&mut self.gpu, &mut self.renderer) else {
            return;
        };
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
        let viewport = if self.options.ds_aspect {
            let (x, y, width, height) = letterbox(w, h, DS_ASPECT);
            Viewport {
                x,
                y,
                width,
                height,
            }
        } else {
            Viewport::full(w, h)
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        renderer.edge_pixels = (!self.options.ds_aspect).then_some(1);
        renderer.render(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            (&view, (w, h)),
            viewport,
            &self.camera,
        );
        self.talk.draw(
            &gpu.device,
            &gpu.queue,
            gpu.config.format,
            &mut encoder,
            &view,
            (w, h),
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
        let attributes = Window::default_attributes().with_title("dq9-app");
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
