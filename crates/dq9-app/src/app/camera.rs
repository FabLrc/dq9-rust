//! The game's camera turn (`L`/`R`) and the framing view.

use super::App;
use super::STEP;
use dq9_core::camera::FieldCamera;
use dq9_core::fx::reduce_angle;
use dq9_render::Camera;
use winit::keyboard::KeyCode;

/// The field camera and what frames it.
pub(super) struct View {
    pub(super) camera: Camera,
    /// The game's camera turn (`L`/`R`) and the yaw it is measured from: the one the map was
    /// entered with.
    pub(super) field_camera: FieldCamera,
    pub(super) camera_home: f32,
    /// Milliseconds of fixed steps not yet given to the camera turn.
    pub(super) clock_ms: f32,
    pub(super) framed: Option<[glam::Vec3; 2]>,
}

impl View {
    pub(super) fn new() -> Self {
        Self {
            camera: Camera::default(),
            field_camera: FieldCamera::default(),
            camera_home: 0.0,
            clock_ms: 0.0,
            framed: None,
        }
    }
}

impl App {
    /// Runs the fixed steps due since the last frame.
    /// The game's camera turn (`FieldCamera_Update`): `Q`/`E` are its `L`/`R` buttons. The yaw is
    /// measured from the one the map was entered with, and turns the other way round from the
    /// application's.
    pub(super) fn turn_camera(&mut self) {
        let left = self.controls.held.contains(&KeyCode::KeyQ);
        let right = self.controls.held.contains(&KeyCode::KeyE);
        if !(left || right || self.view.field_camera.is_returning()) {
            return;
        }
        // Whole milliseconds of the fixed steps, as the game's delta time is.
        self.view.clock_ms += STEP * 1000.0;
        let delta = self.view.clock_ms.floor();
        self.view.clock_ms -= delta;
        let offset = -(self.view.camera.yaw - self.view.camera_home);
        let yaw = reduce_angle((offset * 4096.0).round() as i32);
        let yaw = self
            .view
            .field_camera
            .step(yaw, 0, left, right, delta as u32);
        self.view.camera.yaw =
            (self.view.camera_home - yaw as f32 / 4096.0).rem_euclid(std::f32::consts::TAU);
    }

    /// Replaces the framing view.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn look_at(&mut self, eye: glam::Vec3, target: glam::Vec3) {
        self.view.camera.look_at(eye, target);
    }
}

#[cfg(all(test, windows))]
mod tests;
