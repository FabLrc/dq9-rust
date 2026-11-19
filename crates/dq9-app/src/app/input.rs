//! Keyboard and pointer input of the window (and of a replayed record): the key map, the held
//! keys, the d-pad bits a step reads.

use super::App;
use super::fight;
use crate::record::Input as Logged;
use crate::talk::Button;
use dq9_core::Input;
use dq9_core::state::TimeOfDay;
use dq9_render::Camera;
use dq9_render::CameraMode;
use glam::Vec2;
use glam::Vec3;
use std::collections::HashSet;
use winit::keyboard::KeyCode;

/// The d-pad bits of an [`Input`] (bit 0 up, 1 down, 2 left, 3 right).
pub(super) fn input_bits(input: Input) -> u8 {
    u8::from(input.up)
        | u8::from(input.down) << 1
        | u8::from(input.left) << 2
        | u8::from(input.right) << 3
}

/// The d-pad keys of a replay frame (bit 0 up, 1 down, 2 left, 3 right).
pub(super) fn pad_input(bits: u8) -> Input {
    Input {
        up: bits & 1 != 0,
        down: bits & 2 != 0,
        left: bits & 4 != 0,
        right: bits & 8 != 0,
    }
}

/// The world direction the d-pad asks for with the game's camera at yaw 0, diagonal first as
/// `Hero_ReadDirectionInput` does.
pub(super) fn pad_direction(bits: u8) -> Vec3 {
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

/// The window's held keys and pointer.
pub(super) struct Controls {
    pub(super) held: HashSet<KeyCode>,
    pub(super) cursor: Option<(f64, f64)>,
    pub(super) dragging: bool,
    pub(super) rotate: Vec2,
    pub(super) zoom: f32,
}

impl Controls {
    pub(super) fn new() -> Self {
        Self {
            held: HashSet::new(),
            cursor: None,
            dragging: false,
            rotate: Vec2::ZERO,
            zoom: 0.0,
        }
    }
}

impl App {
    /// Handles a key of the window or of a replay; `true` asks the window to close.
    pub(super) fn key(&mut self, code: KeyCode, pressed: bool) -> bool {
        if !pressed {
            if self.controls.held.remove(&code) {
                self.log_key(code, false);
            }
            return false;
        }
        if !self.controls.held.insert(code) {
            return false; // key repeat
        }
        self.log_key(code, true);
        if self.combat.fight.is_some() {
            // The fight reads the pad itself; the conversation's presses must not pile up.
            match code {
                KeyCode::KeyM => self.audio.toggle_mute(),
                KeyCode::KeyR => self.set_layout(self.options.layout.next()),
                _ => fight::pad_press(&mut self.combat.battle_pad, code),
            }
            return false;
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
            return false;
        }
        if code == KeyCode::KeyI
            && !self.talk.is_active()
            && !self.battle_is_active()
            && !self.events.is_active()
            && self.events.pending_start().is_none()
            && self.open_field_menu()
        {
            return false;
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
            KeyCode::Escape => {
                // From the field Escape ends the session: the native window closes. The page
                // has no window to close, and its loop would stop with the recording
                // (`docs/app.md`, § "Recording a human session"): Escape does nothing there.
                return !cfg!(target_arch = "wasm32");
            }
            KeyCode::Tab => {
                self.view.camera.mode = match self.view.camera.mode {
                    CameraMode::Orbit => CameraMode::Fly,
                    CameraMode::Fly => CameraMode::Orbit,
                };
                self.update_title();
            }
            KeyCode::KeyF => {
                if self.view.camera.mode == CameraMode::Orbit {
                    if let Some(hero) = &self.hero {
                        self.view.camera = Camera::field(hero.position(), self.view.camera_home);
                    }
                } else if let Some(bounds) = self.view.framed {
                    self.view.camera.frame(bounds);
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
        false
    }

    pub(super) fn axis(&self, plus: &[KeyCode], minus: &[KeyCode]) -> f32 {
        let any = |keys: &[KeyCode]| keys.iter().any(|k| self.controls.held.contains(k));
        f32::from(i8::from(any(plus)) - i8::from(any(minus)))
    }

    /// A mouse drag (camera yaw, pitch) and wheel, as the window hands them to the camera.
    pub(super) fn mouse(&mut self, rotate: Vec2, zoom: f32) {
        self.controls.rotate += rotate;
        self.controls.zoom += zoom;
        self.log_input(Logged::Mouse {
            rotate: rotate.to_array(),
            zoom,
        });
    }

    /// The window lost focus: the held keys are forgotten.
    pub(super) fn focus_lost(&mut self) {
        self.controls.held.clear();
        self.log_input(Logged::FocusLost);
    }
}
