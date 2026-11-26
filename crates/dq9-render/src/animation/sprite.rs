//! Sprite objects of the scene: the billboards of map NPCs, script sprites and balloons.
//! Their sheets animate on the scene clock and their quads face the render camera.
use dq9_formats::cast;
use dq9_formats::spr::{Sprite, Step};
use glam::{Mat4, Vec3};
use nds_rom::{Error, Result};

use super::AnimatedScene;
use crate::Camera;
use crate::scene::{
    Cull, MaterialLight, PolygonMode, Scene, SceneDraw, SceneInstance, SceneMaterial, SceneModel,
    SceneTexture, SceneVertex, Shade, Wrap,
};

#[derive(Debug)]
pub(crate) struct SpriteAnimation {
    pub(super) model: usize,
    pub(super) angle: i32,
    pub(super) frames: Vec<(usize, f32, f32)>,
    animations: Vec<(Vec<u8>, Vec<Step>)>,
    animation: usize,
    step: usize,
    elapsed: u32,
    duration: u32,
    pub(super) seated: bool,
    /// Walking (`+0x5c` bit 3, native 545): the four `walk_*` animations instead of the eight
    /// `stand_*` ones.
    pub(super) walking: bool,
    flip_y: bool,
    /// The sheet's animation `animation` plays whatever the camera does.
    fixed: bool,
}

/// What distinguishes a sprite object of the scene: a map NPC, a sprite of a script, a balloon.
struct SpriteSpec {
    name: String,
    /// Half translucent (alpha 16): the NPC placement flag `0x20`.
    half_alpha: bool,
    position: Vec3,
    /// [`SceneInstance::instance`]: the NPC's id, `usize::MAX` for the others.
    instance: usize,
    angle: i32,
    seated: bool,
    flip_y: bool,
    /// The animation of the sheet that plays whatever the camera does (a balloon).
    fixed: Option<usize>,
    /// A 2D sprite of the interface: neither fogged nor lit by the zone.
    overlay: bool,
}

impl AnimatedScene {
    pub(super) fn sprite(&mut self, path: &str, bytes: &[u8], npc: &cast::Npc<'_>) -> Result<()> {
        let instance = self.sprite_object(
            path,
            bytes,
            SpriteSpec {
                name: npc.member.name_str(),
                half_alpha: npc.placement.flags & 0x20 != 0,
                position: Vec3::from_array(npc.placement.position.map(|v| v as f32 / 4096.0)),
                instance: usize::from(npc.member.id),
                angle: i32::from(npc.placement.angle).wrapping_add(npc.placement.seat_angle),
                seated: npc.placement.flags & 0x800 != 0,
                flip_y: npc.placement.flags & 0x400 != 0,
                fixed: None,
                overlay: false,
            },
        )?;
        self.cast.push(instance);
        Ok(())
    }

    /// Adds the sprite sheet `bytes` (a `.spr`) to the scene as a billboard hidden until it is
    /// given a world matrix, and returns its scene instance. A script's sprite (`overlay`
    /// false) shows its `stand_*` or `walk_*` animation like a map NPC; with `fixed` set it
    /// plays that animation of the sheet whatever the camera does, and an `overlay` is lit
    /// and fogged by nothing. Call before the scene is uploaded.
    pub fn add_sprite(
        &mut self,
        name: &str,
        bytes: &[u8],
        fixed: Option<usize>,
        overlay: bool,
    ) -> Result<usize> {
        self.sprite_object(
            name,
            bytes,
            SpriteSpec {
                name: name.to_owned(),
                half_alpha: false,
                position: Vec3::ZERO,
                instance: usize::MAX,
                angle: 0,
                seated: false,
                flip_y: false,
                fixed,
                overlay,
            },
        )
    }

    fn sprite_object(&mut self, path: &str, bytes: &[u8], spec: SpriteSpec) -> Result<usize> {
        let sprite = Sprite::parse(bytes)?;
        let mut frames = Vec::with_capacity(sprite.frames.len());
        for i in 0..sprite.frames.len() {
            let (width, height, rgba) = sprite
                .frame_rgba(i)
                .ok_or(Error::Malformed("sprite frame missing"))?;
            if width == 0 || height == 0 {
                return Err(Error::Malformed("empty sprite frame"));
            }
            let id = self.scene.textures.len();
            self.scene.textures.push(SceneTexture {
                name: format!("{path}:{i}"),
                palette: None,
                source: usize::MAX,
                width,
                height,
                rgba,
                translucent: false,
            });
            frames.push((id, width as f32, height as f32));
        }
        if frames.is_empty() {
            return Err(Error::Malformed("sprite without frames"));
        }
        let model = self.scene.models.len();
        self.scene.models.push(SceneModel {
            archive: path.into(),
            member: path.into(),
            name: spec.name,
            vertices: vec![
                SceneVertex {
                    color: [1.0; 3],
                    ..SceneVertex::default()
                };
                4
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            materials: vec![SceneMaterial {
                name: path.into(),
                texture_name: None,
                palette_name: None,
                texture: Some(frames[0].0),
                wrap: [Wrap::Clamp; 2],
                alpha: if spec.half_alpha { 16 } else { 31 },
                cull: Cull::None,
                mode: PolygonMode::Modulate,
                depth_equal: false,
                translucent_depth_write: false,
                uv_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                light: MaterialLight {
                    shade: if spec.overlay {
                        Shade::Unlit
                    } else {
                        Shade::Sprite
                    },
                    // Observed in melonDS: the DS fogs the cast's polygons.
                    fog: !spec.overlay,
                    ..MaterialLight::default()
                },
            }],
            draws: vec![SceneDraw {
                material: 0,
                indices: 0..6,
                center: [0.0; 3],
            }],
            bounds: None,
            lit: false,
            tinted: false,
        });
        let instance = self.scene.instances.len();
        let world = if spec.instance == usize::MAX {
            Mat4::from_scale(Vec3::ZERO)
        } else {
            Mat4::from_translation(spec.position)
        };
        self.scene.instances.push(SceneInstance {
            model,
            world,
            home: world,
            block: usize::MAX,
            instance: spec.instance,
        });
        let animation = spec
            .fixed
            .unwrap_or_else(|| sprite.first_animation())
            .min(sprite.animations.len().saturating_sub(1));
        let duration = sprite
            .animations
            .get(animation)
            .and_then(|a| a.steps.first())
            .map_or(0, |s| s.duration);
        self.sprites.push(SpriteAnimation {
            model,
            angle: spec.angle,
            frames,
            animations: sprite
                .animations
                .iter()
                .map(|a| (a.name.to_vec(), a.steps.clone()))
                .collect(),
            animation,
            step: 0,
            elapsed: 0,
            duration,
            seated: spec.seated,
            walking: false,
            flip_y: spec.flip_y,
            fixed: spec.fixed.is_some(),
        });
        Ok(instance)
    }
}

/// The axes of a seated sprite's quad: the billboard's `right`, `up` and view `forward` turned
/// the way the sprite draw (`0x0203f54c`) turns its matrix for a seated sheet, for an NPC angle
/// `relative` to the camera's (radians, `NpcSprite_FaceCamera`'s `+0x54` is its opposite).
///
/// The draw puts the pivot at the sheet's centre and multiplies, in the DS convention (row
/// vectors, each command applied closer to the vertex than the one before):
/// `MTX_Concat33(MTX_RotX33(a), C)` (`0x020c15b4`: `RotX · C`, `C` the camera's inverse-view
/// rotation whose rows are right, up and back = `−forward`), then `RotZ(r)` (`0x02031288`).
/// `a = −|cos θ| / 2` (`fix32abs(fix32cos(θ)) >> 1`, negated) and `r` is `θ` reduced to
/// `[−π/8, π/8)` by whole `π/4` (the loop on `0xc91`): the sheet shows the sector's picture
/// and the roll takes up the rest. `MTX_RotX33_` (`0x020c1274`) stores `_11 = cos`, `_12 = sin`,
/// `_21 = −sin`, `_22 = cos`, so the sheet's up `y` goes to `y·(cos a · up + sin a · back)`:
/// with `a < 0` the top leans **away** from the eye and the quad lies back, as a person lying
/// in a bed seen from above. The tilt is about the camera's right axis, after the roll.
fn seated_frame(right: Vec3, up: Vec3, forward: Vec3, relative: f32) -> (Vec3, Vec3) {
    use std::f32::consts::{FRAC_PI_4, FRAC_PI_8};
    let tilt = -relative.cos().abs() * 0.5;
    let roll = (relative + FRAC_PI_8).rem_euclid(FRAC_PI_4) - FRAC_PI_8;
    // `back` is `-forward`: `up · cos a + back · sin a`.
    let tilted = up * tilt.cos() - forward * tilt.sin();
    (
        right * roll.cos() + tilted * roll.sin(),
        tilted * roll.cos() - right * roll.sin(),
    )
}

impl SpriteAnimation {
    /// A two-frame standing sheet facing `angle` (4096ths of a radian), for the tests of the
    /// scene's NPC heading (`super::facing`): no sheet is read from the ROM there.
    #[cfg(test)]
    pub(super) fn test_sheet(angle: i32) -> Self {
        use dq9_formats::spr::Step as SpriteStep;
        let step = |frame| SpriteStep {
            next: 0,
            duration: 8,
            frame,
        };
        Self {
            model: 0,
            angle,
            frames: vec![(0, 16.0, 32.0), (1, 16.0, 32.0)],
            animations: vec![
                (b"stand_up".to_vec(), vec![step(0)]),
                (b"stand_down".to_vec(), vec![step(1)]),
            ],
            animation: 1,
            step: 0,
            elapsed: 3,
            duration: 8,
            seated: false,
            walking: false,
            flip_y: false,
            fixed: false,
        }
    }

    pub(super) fn advance(&mut self) -> Result<()> {
        let Some((_, steps)) = self.animations.get(self.animation) else {
            return Ok(());
        };
        if steps.is_empty() {
            return Ok(());
        }
        self.elapsed = self.elapsed.wrapping_add(1);
        if self.elapsed > self.duration {
            let step = steps
                .get(self.step)
                .ok_or(Error::Malformed("sprite step missing"))?;
            let next = step.next as usize;
            if next >= steps.len() {
                // A sheet of one still frame (`ev_mark`, `field_qu`) points past its only step:
                // the game does not animate it (`SPRSprite::Update` needs `unknown_84_1_`).
                if self.fixed {
                    return Ok(());
                }
                return Err(Error::Malformed("sprite next step out of range"));
            }
            self.step = next;
            self.duration = step.duration;
            self.elapsed = 0;
        }
        Ok(())
    }

    pub(super) fn face(&mut self, camera: &Camera, scene: &mut Scene) -> Result<()> {
        // NpcSprite_FaceCamera (0x0203cbe8), horizontal camera direction.
        // The game's orbit angle rotates the eye's +Z vector; Camera::yaw rotates the
        // view direction in the opposite sense.
        let camera_angle = (-camera.yaw * 4096.0) as i32;
        let stand = cast::sprite_stand(self.angle, camera_angle);
        let (wanted, flip) = if self.seated {
            match stand {
                "stand_down" => ("sleep_down", false),
                "stand_r_down" => ("sleep_r_down", false),
                "stand_right" => ("sleep_right", false),
                "stand_r_up" => ("sleep_r_up", false),
                "stand_up" => ("sleep_up", false),
                "stand_l_up" => ("sleep_r_up", true),
                "stand_left" => ("sleep_right", true),
                _ => ("sleep_r_down", true),
            }
        } else if self.walking {
            (cast::sprite_walk(self.angle, camera_angle), false)
        } else {
            (stand, false)
        };
        let selected = (!self.fixed)
            .then(|| {
                self.animations
                    .iter()
                    .position(|(n, _)| n == wanted.as_bytes())
            })
            .flatten();
        if let Some(index) = selected.filter(|&i| i != self.animation) {
            self.animation = index;
            self.step = 0;
            self.elapsed = 0;
            self.duration = self.animations[index].1.first().map_or(0, |s| s.duration);
        }
        let frame = self
            .animations
            .get(self.animation)
            .and_then(|(_, s)| s.get(self.step))
            .map_or(0, |s| s.frame) as usize;
        let &(texture, width, height) = self
            .frames
            .get(frame)
            .ok_or(Error::Malformed("sprite texture frame out of range"))?;
        let model = &mut scene.models[self.model];
        model.materials[0].texture = Some(texture);
        let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin());
        let up = right.cross(camera.forward()).normalize_or_zero();
        let (right, up) = if self.seated {
            let relative = self.angle.wrapping_sub(camera_angle) as f32 / 4096.0;
            seated_frame(right, up, camera.forward(), relative)
        } else {
            (right, up)
        };
        // NpcTable_Build (0x0203d038): sprites use scale 0x8f, models 0x10a.
        let (width, height) = (width * (143.0 / 4096.0), height * (143.0 / 4096.0));
        // 0x0203f54c: billboard pivot is half the sheet's height above its placement.
        let center = Vec3::Y * (height * 0.5);
        for (v, (x, y, u, t)) in model.vertices.iter_mut().zip([
            (-0.5, 0.5, 0.0, 0.0),
            (0.5, 0.5, 1.0, 0.0),
            (0.5, -0.5, 1.0, 1.0),
            (-0.5, -0.5, 0.0, 1.0),
        ]) {
            v.position = (center + right * (x * width) + up * (y * height)).to_array();
            v.texcoord = [
                if flip { 1.0 - u } else { u },
                if self.flip_y { 1.0 - t } else { t },
            ];
        }
        model.draws[0].center = center.to_array();
        let extent = Vec3::splat(width.max(height));
        model.bounds = Some([-extent, extent]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_camera_can_select_animation_zero() {
        let material = SceneMaterial {
            name: String::new(),
            texture_name: None,
            palette_name: None,
            texture: Some(1),
            wrap: [Wrap::Clamp; 2],
            alpha: 31,
            cull: Cull::None,
            mode: PolygonMode::Modulate,
            depth_equal: false,
            translucent_depth_write: false,
            uv_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            light: MaterialLight::default(),
        };
        let mut scene = Scene::default();
        scene.models.push(SceneModel {
            archive: String::new(),
            member: String::new(),
            name: String::new(),
            vertices: vec![SceneVertex::default(); 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            materials: vec![material],
            draws: vec![SceneDraw {
                material: 0,
                indices: 0..6,
                center: [0.0; 3],
            }],
            bounds: None,
            lit: false,
            tinted: false,
        });
        let mut sprite = SpriteAnimation {
            model: 0,
            angle: 0x3244,
            frames: vec![(0, 16.0, 32.0), (1, 16.0, 32.0)],
            animations: vec![
                (
                    b"stand_up".to_vec(),
                    vec![Step {
                        next: 0,
                        duration: 8,
                        frame: 0,
                    }],
                ),
                (
                    b"stand_down".to_vec(),
                    vec![Step {
                        next: 0,
                        duration: 8,
                        frame: 1,
                    }],
                ),
            ],
            animation: 1,
            step: 0,
            elapsed: 3,
            duration: 8,
            seated: false,
            walking: false,
            flip_y: false,
            fixed: false,
        };
        sprite.face(&Camera::default(), &mut scene).unwrap();
        assert_eq!(sprite.animation, 0);
        assert_eq!(sprite.elapsed, 0);
        assert_eq!(scene.models[0].materials[0].texture, Some(0));
        let width = Vec3::from_array(scene.models[0].vertices[1].position)
            - Vec3::from_array(scene.models[0].vertices[0].position);
        assert!((width.length() - 16.0 * 143.0 / 4096.0).abs() < 1e-6);
    }

    /// A point `(x, y)` of the sheet (centred on its pivot) through the sprite draw's matrix
    /// chain, written the way the DS applies it: row vectors, `v · RotZ(r) · RotX(a) · C`
    /// (`0x02031288`, `MTX_Concat33(RotX, C)` of `0x020c15b4`, `MTX_RotX33_` of `0x020c1274`:
    /// `_11 = cos`, `_12 = sin`, `_21 = −sin`, `_22 = cos`), with `C`'s rows the camera's right,
    /// up and back.
    fn ds_chain(camera: &Camera, relative: f32, x: f32, y: f32) -> Vec3 {
        let right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin());
        let up = right.cross(camera.forward()).normalize();
        let back = -camera.forward();
        let (sin_a, cos_a) = (-relative.cos().abs() * 0.5).sin_cos();
        let step = std::f32::consts::FRAC_PI_4;
        let residue = (relative + step / 2.0).rem_euclid(step) - step / 2.0;
        let (sin_r, cos_r) = residue.sin_cos();
        // v · RotZ: `x' = x cos − y sin`, `y' = x sin + y cos`.
        let (x, y, z) = (x * cos_r - y * sin_r, x * sin_r + y * cos_r, 0.0);
        // · RotX: `y' = y cos − z sin`, `z' = y sin + z cos`.
        let (x, y, z) = (x, y * cos_a - z * sin_a, y * sin_a + z * cos_a);
        // · C: the rows are the images of the local axes.
        right * x + up * y + back * z
    }

    fn seated(angle: i32) -> (SpriteAnimation, Scene) {
        let mut scene = Scene::default();
        scene.models.push(SceneModel {
            archive: String::new(),
            member: String::new(),
            name: String::new(),
            vertices: vec![SceneVertex::default(); 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            materials: vec![SceneMaterial {
                name: String::new(),
                texture_name: None,
                palette_name: None,
                texture: Some(0),
                wrap: [Wrap::Clamp; 2],
                alpha: 31,
                cull: Cull::None,
                mode: PolygonMode::Modulate,
                depth_equal: false,
                translucent_depth_write: false,
                uv_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                light: MaterialLight::default(),
            }],
            draws: vec![SceneDraw {
                material: 0,
                indices: 0..6,
                center: [0.0; 3],
            }],
            bounds: None,
            lit: false,
            tinted: false,
        });
        let step = Step {
            next: 0,
            duration: 8,
            frame: 0,
        };
        let names = [
            "sleep_down",
            "sleep_r_down",
            "sleep_right",
            "sleep_r_up",
            "sleep_up",
        ];
        let sprite = SpriteAnimation {
            model: 0,
            angle,
            frames: vec![(0, 64.0, 32.0)],
            animations: names
                .iter()
                .map(|n| (n.as_bytes().to_vec(), vec![step]))
                .collect(),
            animation: 0,
            step: 0,
            elapsed: 0,
            duration: 8,
            seated: true,
            walking: false,
            flip_y: false,
            fixed: false,
        };
        (sprite, scene)
    }

    /// A seated sprite is not a plain billboard (`0x0203f54c`): its quad lies back by half of
    /// `|cos θ|` radians, the top **away** from the eye, rolled by what the sector's picture
    /// leaves of `θ`. The vertices equal the DS matrix chain's, whatever the camera does.
    #[test]
    fn a_seated_sprite_leans_away_from_the_eye_like_the_ds_chain() {
        let (mut sprite, mut scene) = seated(0x0f00);
        let (width, height) = (64.0 * 143.0 / 4096.0, 32.0 * 143.0 / 4096.0);
        for pitch in [0.2, 0.55] {
            for k in 0..8 {
                let camera = Camera {
                    yaw: k as f32 * std::f32::consts::FRAC_PI_4 + 0.3,
                    pitch,
                    ..Camera::default()
                };
                sprite.face(&camera, &mut scene).unwrap();
                let relative =
                    (sprite.angle.wrapping_sub((-camera.yaw * 4096.0) as i32)) as f32 / 4096.0;
                let at = |i: usize| Vec3::from_array(scene.models[0].vertices[i].position);
                for (i, (x, y)) in [(-0.5, 0.5), (0.5, 0.5), (0.5, -0.5), (-0.5, -0.5)]
                    .into_iter()
                    .enumerate()
                {
                    let want = Vec3::Y * (height * 0.5)
                        + ds_chain(&camera, relative, x * width, y * height);
                    assert!(
                        (at(i) - want).length() < 2e-3,
                        "yaw {k} pitch {pitch} vertex {i}: {:?} against {want:?}",
                        at(i)
                    );
                }
                // The observable: the top is never nearer the eye than the bottom.
                let lean = ((at(0) + at(1)) * 0.5 - (at(2) + at(3)) * 0.5)
                    .normalize()
                    .dot(camera.forward());
                assert!(lean >= -1e-4, "yaw {k}: the top leans to the eye ({lean})");
                if relative.cos().abs() > 0.9 {
                    assert!(lean > 0.3, "yaw {k}: lies back by half a radian ({lean})");
                }
            }
        }
    }
}
