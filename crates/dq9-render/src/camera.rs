//! Free camera (orbit or fly) with the game's default projection.
//!
//! The game's camera constructor (`0x0202df78`) sets a half vertical field of view of 15°
//! (`0xf000` degrees, through `0x0202e9b4`, which stores its sine and cosine for
//! `NNS_G3dGlbPerspective`), a near plane at 1 and a far plane at 400 (`fx32`). The DS draws
//! each screen at 256×192.

use glam::{Mat4, Vec2, Vec3};

/// Vertical field of view of the game's default camera (2 × 15°, `0x0202df78`).
pub const DS_FOVY: f32 = 30.0 * std::f32::consts::PI / 180.0;
/// Near and far planes of the game's default camera (`0x0202df78`: `0x1000`, `0x190000`).
pub const DS_NEAR: f32 = 1.0;
pub const DS_FAR: f32 = 400.0;
/// Aspect ratio of one DS screen.
pub const DS_ASPECT: f32 = 256.0 / 192.0;

/// How movement input moves the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// Turns around [`Camera::target`]; movement pans the target on the ground plane.
    Orbit,
    /// Turns around the eye; movement follows the view direction.
    Fly,
}

/// Input accumulated over one fixed update step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CameraInput {
    /// Movement axes, each in `-1..=1`: forward, right, up.
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    /// Rotation this step in radians: yaw (x) and pitch (y).
    pub rotate: Vec2,
    /// Zoom steps this step (mouse wheel), positive towards the target.
    pub zoom: f32,
    /// Move faster.
    pub fast: bool,
}

/// A camera looking at `target` from `distance` away, turned by `yaw` (around `y`, 0 looking
/// towards `-z`) and `pitch` (positive looking down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub mode: CameraMode,
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fovy: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            mode: CameraMode::Orbit,
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.5,
            distance: 30.0,
            fovy: DS_FOVY,
            near: DS_NEAR,
            far: DS_FAR,
        }
    }
}

impl Camera {
    /// Unit view direction.
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(cp * sy, -sp, -cp * cy)
    }

    pub fn eye(&self) -> Vec3 {
        self.target - self.forward() * self.distance
    }

    /// Looks from `eye` at `target`.
    pub fn look_at(&mut self, eye: Vec3, target: Vec3) {
        let d = target - eye;
        self.distance = d.length().max(1e-3);
        let d = d / self.distance;
        self.pitch = (-d.y).clamp(-1.0, 1.0).asin();
        self.yaw = d.x.atan2(-d.z);
        self.target = target;
    }

    /// Frames a world-space box from a three-quarter view above it.
    pub fn frame(&mut self, bounds: [Vec3; 2]) {
        let [lo, hi] = bounds;
        let radius = ((hi - lo).length() * 0.5).max(1.0);
        self.target = (lo + hi) * 0.5;
        self.yaw = std::f32::consts::FRAC_PI_4;
        self.pitch = 0.6;
        // The bounding sphere is loose for flat maps: 0.6 of its fitting distance fills the
        // view.
        self.distance = 0.6 * radius / (self.fovy * 0.5).tan();
        self.far = self.far.max(self.distance + 2.0 * radius);
    }

    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Y)
    }

    /// Perspective projection to wgpu clip space (depth `0..1`).
    pub fn projection(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fovy, aspect, self.near, self.far)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Advances one fixed step of `dt` seconds.
    pub fn step(&mut self, input: &CameraInput, dt: f32) {
        let limit = std::f32::consts::FRAC_PI_2 - 0.01;
        let eye = self.eye();
        self.yaw = (self.yaw + input.rotate.x).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + input.rotate.y).clamp(-limit, limit);
        let speed = self.distance.max(5.0) * if input.fast { 2.0 } else { 0.6 } * dt;
        match self.mode {
            CameraMode::Orbit => {
                let (sy, cy) = self.yaw.sin_cos();
                let ahead = Vec3::new(sy, 0.0, -cy);
                let right = Vec3::new(cy, 0.0, sy);
                self.target +=
                    (ahead * input.forward + right * input.right + Vec3::Y * input.up) * speed;
                self.distance = (self.distance * 0.9f32.powf(input.zoom)).clamp(1.0, 2000.0);
            }
            CameraMode::Fly => {
                // Keep the eye where it was while turning.
                self.target = eye + self.forward() * self.distance;
                let right = self.forward().cross(Vec3::Y).normalize_or_zero();
                let step = (self.forward() * (input.forward + input.zoom)
                    + right * input.right
                    + Vec3::Y * input.up)
                    * speed;
                self.target += step;
            }
        }
        self.far = self.far.max(self.distance * 4.0);
    }
}

/// The largest centred rectangle of aspect `aspect` in a `width` × `height` target:
/// `(x, y, w, h)`.
pub fn letterbox(width: u32, height: u32, aspect: f32) -> (u32, u32, u32, u32) {
    let (w, h) = (width as f32, height as f32);
    let (vw, vh) = if w / h > aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    let (vw, vh) = (vw.round().max(1.0) as u32, vh.round().max(1.0) as u32);
    (
        (width - vw.min(width)) / 2,
        (height - vh.min(height)) / 2,
        vw,
        vh,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_at_round_trips() {
        let mut c = Camera::default();
        let (eye, target) = (Vec3::new(3.0, 10.0, 8.0), Vec3::new(-1.0, 0.0, 2.0));
        c.look_at(eye, target);
        assert!((c.eye() - eye).length() < 1e-4);
        assert!((c.target - target).length() < 1e-4);
    }

    #[test]
    fn fly_turns_around_the_eye() {
        let mut c = Camera {
            mode: CameraMode::Fly,
            ..Camera::default()
        };
        let eye = c.eye();
        c.step(
            &CameraInput {
                rotate: Vec2::new(0.5, 0.1),
                ..CameraInput::default()
            },
            1.0 / 60.0,
        );
        assert!((c.eye() - eye).length() < 1e-4);
    }

    #[test]
    fn letterbox_keeps_ds_aspect() {
        assert_eq!(letterbox(256, 192, DS_ASPECT), (0, 0, 256, 192));
        assert_eq!(letterbox(1000, 192, DS_ASPECT), (372, 0, 256, 192));
        assert_eq!(letterbox(256, 1000, DS_ASPECT), (0, 404, 256, 192));
    }
}
