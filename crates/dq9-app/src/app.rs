//! Window, input and frame loop, shared by the native and Web builds.
//!
//! Updates run at a fixed 60 Hz (the DS frame rate) from an accumulator; rendering happens once
//! per redraw.
//!
//! Controls: drag with the left button to turn, wheel to zoom, `WASD`/arrows to move, `Q`/`E`
//! down/up, `Shift` faster, `Tab` orbit/fly, `F` reframe, `R` DS aspect (256×192), `T` next
//! time of day, `PageUp`/`PageDown` previous/next map.

use std::collections::HashSet;
use std::sync::Arc;

use dq9_core::{CollisionWorld, Hero};
use dq9_formats::map::Doorway;
use dq9_render::camera::{DS_ASPECT, letterbox};
use dq9_render::{AnimatedScene, Camera, CameraInput, CameraMode, Renderer, Viewport};
use glam::{Mat4, Vec2, Vec3};
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::gpu::Gpu;
use crate::load::{self, MapEntry};

fn doorway_transition(
    doorways: &[Doorway],
    maps: &[MapEntry],
    before: Vec3,
    after: Vec3,
) -> Option<(String, Vec3, f32)> {
    doorways
        .iter()
        .find(|door| {
            !door.volume.contains(before.to_array()) && door.volume.contains(after.to_array())
        })
        .and_then(|door| {
            maps.iter()
                .find(|map| map.id == door.destination)
                .map(|map| {
                    (
                        map.code.clone(),
                        Vec3::from_array(door.arrival),
                        door.facing,
                    )
                })
        })
}

/// Fixed update step (60 Hz, the DS frame rate).
pub const STEP: f32 = 1.0 / 60.0;
/// Times of day of the map assembly (`LightingManager+0x98`).
const TIMES: u32 = 4;

/// Events delivered to the loop from outside (async GPU setup, ROM/save picked on the web).
#[derive(Debug)]
pub enum UserEvent {
    Gpu(Box<Gpu>),
    GpuFailed(String),
    /// ROM picked in the page (the native build reads it before starting).
    #[cfg(target_arch = "wasm32")]
    Rom(Vec<u8>),
    /// Save picked in the page; only its map id is used for startup.
    #[cfg(target_arch = "wasm32")]
    Save(Vec<u8>),
}

/// Start-up options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Explicit map override; save map is used if absent.
    pub map: Option<String>,
    pub save: Option<Vec<u8>>,
    pub time: u32,
    pub ds_aspect: bool,
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
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    rom: Option<Vec<u8>>,
    maps: Vec<MapEntry>,
    current: Option<(MapEntry, String)>,
    /// CPU state is retained after upload; native and Web advance the same animation clock.
    runtime: Option<AnimatedScene>,
    collision: CollisionWorld,
    doorways: Vec<Doorway>,
    hero: Option<Hero>,
    hero_instance: Option<usize>,
    renderer: Option<Renderer>,
    camera: Camera,
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
        let mut app = Self {
            proxy,
            options,
            window: None,
            gpu: None,
            rom: None,
            maps: Vec::new(),
            current: None,
            runtime: None,
            collision: CollisionWorld::default(),
            doorways: Vec::new(),
            hero: None,
            hero_instance: None,
            renderer: None,
            camera: Camera::default(),
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
        self.rom = Some(rom);
        let map =
            match load::starting_map(self.options.map.as_deref(), self.options.save.as_deref()) {
                Ok(map) => map,
                Err(e) => {
                    status(&format!("{e:#}"));
                    return;
                }
            };
        self.load(&map, true, None);
    }

    #[cfg(target_arch = "wasm32")]
    fn set_save(&mut self, save: Vec<u8>) {
        self.options.save = Some(save);
        if let Some(rom) = &self.rom {
            self.set_rom(rom.clone());
        }
    }

    /// Loads map `key` at the current time of day; `frame` resets the camera onto it.
    fn load(&mut self, key: &str, frame: bool, spawn: Option<(Vec3, f32)>) {
        let Some(rom) = &self.rom else { return };
        let started = Instant::now();
        let mut loaded = match load::load(rom, key, self.options.time, &self.options.story, spawn) {
            Ok(l) => l,
            Err(e) => {
                status(&format!("{e:#}"));
                return;
            }
        };
        let hero_start = loaded.hero.position;
        self.collision = loaded.collision;
        self.doorways = loaded.doorways;
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
        if let Some((_, yaw)) = spawn {
            self.camera.yaw = yaw;
        }
        self.camera.target = hero_start;
        self.current = Some((loaded.entry, loaded.label));
        if let Err(e) = loaded.runtime.face_camera(&self.camera) {
            status(&format!("facing cast: {e}"));
            return;
        }
        self.runtime = Some(loaded.runtime);
        self.upload();
        self.update_title();
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
            entry.id, entry.code, self.options.time
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
        self.load(&id, true, None);
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, pressed: bool) {
        if !pressed {
            self.held.remove(&code);
            return;
        }
        if !self.held.insert(code) {
            return; // key repeat
        }
        match code {
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
            KeyCode::KeyR => self.options.ds_aspect = !self.options.ds_aspect,
            KeyCode::KeyT => {
                self.options.time = (self.options.time + 1) % TIMES;
                let map = self
                    .current
                    .as_ref()
                    .map(|(entry, _)| entry.code.clone())
                    .unwrap_or_else(|| "M01".into());
                self.load(&map, false, None);
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
    fn update(&mut self) {
        let now = Instant::now();
        // Clamp long pauses (window dragged, tab hidden) instead of catching up.
        self.accumulator += now.duration_since(self.last).as_secs_f32().min(0.25);
        self.last = now;
        while self.accumulator >= STEP {
            self.accumulator -= STEP;
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
            if walking {
                input.forward = 0.0;
                input.right = 0.0;
            }
            self.camera.step(&input, STEP);
            let move_direction = if walking {
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
            let mut transition = None;
            if let Some(hero) = &mut self.hero {
                let before = hero.position;
                hero.step(
                    &self.collision,
                    move_direction,
                    STEP * if input.fast { 2.0 } else { 1.0 },
                );
                self.camera.target += hero.position - before;
                transition = doorway_transition(&self.doorways, &self.maps, before, hero.position);
                if let (Some(runtime), Some(index)) = (&mut self.runtime, self.hero_instance)
                    && let Some(instance) = runtime.scene.instances.get_mut(index)
                {
                    instance.world = Mat4::from_translation(hero.position);
                }
            }
            if let Some((destination, arrival, yaw)) = transition {
                self.load(&destination, true, Some((arrival, yaw)));
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
        if let (Some(gpu), Some(renderer), Some(hero), Some(index)) = (
            &self.gpu,
            &mut self.renderer,
            &self.hero,
            self.hero_instance,
        ) && !renderer.update_instance_world(
            &gpu.queue,
            index,
            Mat4::from_translation(hero.position),
        ) {
            status("hero marker instance is unavailable");
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
        renderer.render(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            (&view, (w, h)),
            viewport,
            &self.camera,
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
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.key(event_loop, code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::Focused(false) => self.held.clear(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.dragging = state == ElementState::Pressed,
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
#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::map::DoorVolume;

    #[test]
    fn entering_a_doorway_resolves_map_and_arrival_pose_once() {
        let doorways = [Doorway {
            destination: 109,
            volume: DoorVolume {
                center: Vec3::ZERO.to_array(),
                size: [2.0, 2.0, 2.0],
                angle: 0.0,
            },
            arrival: [10.0, 2.0, 30.0],
            facing: 1.25,
        }];
        let maps = [MapEntry {
            id: 109,
            code: "C01M09".into(),
        }];
        let transition = doorway_transition(
            &doorways,
            &maps,
            Vec3::new(-1.1, 0.0, 0.0),
            Vec3::new(-0.9, 100.0, 0.0),
        );
        assert_eq!(
            transition,
            Some(("C01M09".into(), Vec3::new(10.0, 2.0, 30.0), 1.25))
        );
        assert_eq!(
            doorway_transition(
                &doorways,
                &maps,
                Vec3::new(-0.9, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 0.0),
            ),
            None
        );
    }

    #[test]
    fn doorway_with_no_loaded_destination_cannot_transition() {
        let doorways = [Doorway {
            destination: 109,
            volume: DoorVolume {
                center: Vec3::ZERO.to_array(),
                size: [2.0, 2.0, 2.0],
                angle: 0.0,
            },
            arrival: [0.0; 3],
            facing: 0.0,
        }];
        assert_eq!(
            doorway_transition(
                &doorways,
                &[],
                Vec3::new(-1.1, 0.0, 0.0),
                Vec3::new(-0.9, 0.0, 0.0),
            ),
            None
        );
    }
}
