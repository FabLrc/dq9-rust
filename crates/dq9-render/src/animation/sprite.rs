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
    seated: bool,
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

impl SpriteAnimation {
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
        let mut right = Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin());
        let mut up = right.cross(camera.forward()).normalize_or_zero();
        if self.seated {
            let relative = self.angle.wrapping_sub(camera_angle) as f32 / 4096.0;
            let tilt = -relative.cos().abs() * 0.5;
            let roll = (relative + std::f32::consts::FRAC_PI_8)
                .rem_euclid(std::f32::consts::FRAC_PI_4)
                - std::f32::consts::FRAC_PI_8;
            let forward = camera.forward();
            let tilted = up * tilt.cos() + forward * tilt.sin();
            let r = right * roll.cos() + tilted * roll.sin();
            up = tilted * roll.cos() - right * roll.sin();
            right = r;
        }
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
}
