//! The battle swirl's model (`data/effect/ev999991500.chr`) placed over a field camera.
//!
//! The original draws it in an orthographic 64 × 48 box centred on the screen, eye (0, 0, 10)
//! looking down −z, the disc at world (0, −10, 1) (`ov_017::02195ecc`,
//! `docs/re/battle-transition.md`), then darkens both screens with the master brightness. Here
//! the disc is an ordinary scene actor kept in front of the camera: every step its world matrix
//! is `inverse(view) · translate(0, 0, −d) · scale(k)` with `k` chosen so that the box's 48 units
//! of height are the view's height at distance `d`, whatever the field of view the swirl is
//! itself narrowing. The roll and the zoom act on the field alone, as in the original, because
//! the disc follows the camera.

use glam::{Mat4, Vec3};
use nds_rom::Result;

use crate::{AnimatedScene, Camera};

/// The model's path in the ROM.
pub const CHR_PATH: &str = "data/effect/ev999991500.chr";
/// The chr's only motion (`ev999991500.bcfg`: frames 1 to 40, speed 0.5).
const MOTION: &str = "0";
/// Distance in front of the camera, in world units: inside the near plane's range and nearer
/// than any field geometry the camera sees.
const DISTANCE: f32 = 4.0;
/// Height of the original's orthographic box (`MTX_OrthoW` top 14, bottom −34).
const BOX_HEIGHT: f32 = 48.0;

/// The swirl actor of a scene.
#[derive(Debug, Clone, Copy)]
pub struct Swirl {
    actor: usize,
    instance: usize,
}

impl Swirl {
    /// Adds the swirl to `scene`, hidden. Call before the scene is uploaded. `None` when the
    /// model draws nothing.
    pub fn add(scene: &mut AnimatedScene, chr: &[u8]) -> Result<Option<Self>> {
        let actor = scene.add_actor(chr, &[])?;
        let Some(instance) = scene.actor_instance(actor) else {
            return Ok(None);
        };
        scene.play_actor(actor, MOTION, false, 1.0);
        Ok(Some(Self { actor, instance }))
    }

    /// The scene instance the renderer's `update_instance_world` moves.
    pub const fn instance(&self) -> usize {
        self.instance
    }

    /// Poses the model at `frame` (the game's 1-based model frame).
    pub fn pose(&self, scene: &mut AnimatedScene, frame: f32) -> Result<()> {
        scene.set_actor_frame(self.actor, (frame * 4096.0).round() as i32)?;
        Ok(())
    }

    /// The disc's world matrix for `camera`.
    pub fn world(camera: &Camera) -> Mat4 {
        let k = 2.0 * DISTANCE * (camera.fovy * 0.5).tan() / BOX_HEIGHT;
        camera.view().inverse()
            * Mat4::from_translation(Vec3::new(0.0, 0.0, -DISTANCE))
            * Mat4::from_scale(Vec3::splat(k))
    }

    /// Places the disc in front of `camera` (`Some`) or hides it (`None`) and returns the
    /// matrix to give the renderer.
    pub fn place(&self, scene: &mut AnimatedScene, camera: Option<&Camera>) -> Mat4 {
        let world = camera.map_or(Mat4::from_scale(Vec3::ZERO), Self::world);
        scene.scene.instances[self.instance].world = world;
        world
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The top edge of the original's box (y = +24 from its centre) is the top of the view,
    /// the bottom edge the bottom, at every field of view the swirl passes through.
    #[test]
    fn the_box_height_is_the_view_height() {
        for fovy_degrees in [30.0f32, 22.0, 14.4] {
            let mut camera = Camera::field(Vec3::new(3.0, 1.0, -4.0), 0.7);
            camera.fovy = fovy_degrees.to_radians();
            camera.roll = 1.1;
            let clip = camera.view_proj(4.0 / 3.0) * Swirl::world(&camera);
            let top = clip * glam::Vec4::new(0.0, 24.0, 0.0, 1.0);
            let bottom = clip * glam::Vec4::new(0.0, -24.0, 0.0, 1.0);
            let centre = clip * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);
            assert!((top.y / top.w - 1.0).abs() < 1e-4, "{fovy_degrees}");
            assert!((bottom.y / bottom.w + 1.0).abs() < 1e-4, "{fovy_degrees}");
            assert!((centre.x / centre.w).abs() < 1e-4 && (centre.y / centre.w).abs() < 1e-4);
            assert!(centre.w > 0.0, "the disc is in front of the camera");
        }
    }
}
