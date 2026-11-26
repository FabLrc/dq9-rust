//! The `winit` side of the application: the events delivered from outside (async GPU setup,
//! ROM and save picked in the page) and the [`ApplicationHandler`] that feeds the frame loop
//! with the window's own.

use super::App;
use super::status;
use crate::gpu::Gpu;
#[cfg(target_arch = "wasm32")]
use crate::lighting;
#[cfg(target_arch = "wasm32")]
use crate::record;
#[cfg(target_arch = "wasm32")]
use crate::screens::Layout;
#[cfg(target_arch = "wasm32")]
use crate::screens::ScreenMode;
#[cfg(target_arch = "wasm32")]
use dq9_render::EnhancedOptions;
use glam::Vec2;
use std::sync::Arc;
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::ElementState;
use winit::event::MouseButton;
use winit::event::MouseScrollDelta;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;
use winit::keyboard::PhysicalKey;
use winit::window::Window;
use winit::window::WindowId;

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
    /// Enhanced effects picked in the page's checkboxes (also set from `?fx=`/`?nofx=`).
    #[cfg(target_arch = "wasm32")]
    Fx(EnhancedOptions),
    /// Field screen assignment picked in the page (`--screens`).
    #[cfg(target_arch = "wasm32")]
    Screens(ScreenMode),
    /// "New game" clicked in the page: starts the prologue (now, or once the ROM is read).
    #[cfg(target_arch = "wasm32")]
    NewGame,
    /// `?record=1`: the page sends this right before the ROM, so the recording starts at the
    /// first step of the session (`docs/app/record.md`, § Recording a human session).
    #[cfg(target_arch = "wasm32")]
    Record,
    /// "Save recording" clicked in the page: the record so far, as a download.
    #[cfg(target_arch = "wasm32")]
    RecordSave,
    /// A `.dq9rec` picked in the page (`?play-record=1`): its start options replace the page's.
    #[cfg(target_arch = "wasm32")]
    PlayRecord(Box<record::Record>),
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
            UserEvent::Rom(rom) => match self.recording.pending_replay.take() {
                // A record picked before the ROM names the session the ROM starts: its options
                // are applied before the load, so the page's own start options never run.
                Some(record) => self.start_page_replay(record, Some(rom)),
                None => {
                    self.set_rom(rom);
                    // `?record=1`: the recording starts on the session the ROM loaded (a
                    // refused ROM leaves `self.rom` empty and the page offers another).
                    if self.recording.record_wanted && self.rom.is_some() {
                        self.recording.record_wanted = false;
                        self.start_page_recording();
                    }
                }
            },
            #[cfg(target_arch = "wasm32")]
            UserEvent::Save(save) => self.set_save(save),
            #[cfg(target_arch = "wasm32")]
            UserEvent::Time(time) => self.set_time(lighting::phase(time)),
            #[cfg(target_arch = "wasm32")]
            UserEvent::Layout(layout) => self.set_layout(layout),
            #[cfg(target_arch = "wasm32")]
            UserEvent::Fx(fx) => self.set_fx(fx),
            #[cfg(target_arch = "wasm32")]
            UserEvent::Screens(screens) => self.set_screens(screens),
            #[cfg(target_arch = "wasm32")]
            UserEvent::NewGame => {
                self.options.new_game = true;
                self.start_new_game();
            }
            #[cfg(target_arch = "wasm32")]
            UserEvent::Record => self.recording.record_wanted = true,
            #[cfg(target_arch = "wasm32")]
            UserEvent::RecordSave => self.save_page_record(),
            #[cfg(target_arch = "wasm32")]
            UserEvent::PlayRecord(record) => {
                if self.rom.is_some() {
                    self.start_page_replay(*record, None);
                } else {
                    self.recording.pending_replay = Some(*record);
                }
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
                    let pressed = event.state == ElementState::Pressed;
                    if self.recording.replay.is_some() {
                        // A replay drives the logic; the keyboard only closes the window (not on
                        // the page: Escape there does nothing) and changes what is heard/seen.
                        match code {
                            KeyCode::Escape if pressed && cfg!(not(target_arch = "wasm32")) => {
                                event_loop.exit()
                            }
                            KeyCode::KeyM if pressed => self.audio.toggle_mute(),
                            KeyCode::KeyR if pressed => {
                                self.set_layout(self.options.layout.next());
                            }
                            _ => {}
                        }
                    } else if self.key(code, pressed) || self.session_over {
                        // `self.key` asks for the escape/close; `session_over` is the church's
                        // Confession having said its last line, 110 (state 9's power-off
                        // instruction). This build has no title screen to stay on.
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::Focused(false) if self.recording.replay.is_none() => self.focus_lost(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.audio.gesture();
                match state {
                    ElementState::Pressed => {
                        self.controls.dragging = true;
                        self.controls.begin_press();
                    }
                    ElementState::Released => {
                        self.controls.dragging = false;
                        // A press released without dragging the camera is a tap on the touch
                        // screen (`Input`, § "Touch" of `docs/app/screens.md`).
                        let tap = self.controls.finish_press();
                        // The same window size the frame's screen rectangles were computed
                        // from (`present_frame`: the GPU config), so a tap lands on the
                        // screen the user saw.
                        let size = match &self.gpu {
                            Some(gpu) => Some((gpu.config.width, gpu.config.height)),
                            None => self.window.as_ref().map(|window| {
                                let size = window.inner_size();
                                (size.width, size.height)
                            }),
                        };
                        if let (Some(at), Some(size)) = (tap, size) {
                            self.tap(at, size);
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((x, y)) = self.controls.cursor
                    && self.controls.dragging
                    && self.recording.replay.is_none()
                {
                    self.controls.travel_to((x, y), (position.x, position.y));
                    let speed = 0.005;
                    let rotate = Vec2::new(
                        ((position.x - x) * speed) as f32,
                        ((position.y - y) * speed) as f32,
                    );
                    self.mouse(rotate, 0.0);
                }
                self.controls.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } if self.recording.replay.is_none() => {
                let zoom = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                self.mouse(Vec2::ZERO, zoom);
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.session_over {
            event_loop.exit();
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        // The record is written when the window closes, however it was closed. (The Web
        // build offers it to the page instead.)
        #[cfg(not(target_arch = "wasm32"))]
        self.finish_recording();
    }
}
