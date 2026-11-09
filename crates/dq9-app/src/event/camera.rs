//! Cameras the scripts install (natives 552, 572 and 584) and the balloons (natives 541, 561,
//! 542) the scene draws in the world.

use dq9_core::Hero;
use dq9_core::cutscene::{Balloon, Node, Target};
use dq9_core::field::yaw_of_angle;
use dq9_render::AnimatedScene;
use dq9_render::camera::DS_ASPECT;
use glam::{Mat4, Vec3};

use super::{EventPlayer, Stage, world_of};

/// The character a native 552 camera drags: its last position and facing (game angle,
/// radians, 0 faces +z), and the updates since the last turn.
#[derive(Debug, Clone, Copy)]
pub(super) struct ThreeBoneDrag {
    target: Target,
    position: Vec3,
    facing: f32,
    updates: u32,
}

/// Wraps an angle to (−π, π], as the game's 16-bit angle does after the halfway turn.
fn wrap_angle(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let angle = angle.rem_euclid(TAU);
    if angle > PI { angle - TAU } else { angle }
}

/// The point of a bone of `actor`, in the world of `world`: the rig's own pose when it has no
/// polygon, hence no instance.
fn bone_point(scene: &AnimatedScene, actor: usize, world: Mat4, name: &str) -> Option<Vec3> {
    let bone = scene.actor_bone(actor, name).ok()??;
    Some(world.transform_point3(bone.w_axis.truncate()))
}

impl EventPlayer {
    pub(super) fn present_camera(&mut self, stage: &mut Stage) {
        let view = self.cutscene.camera();
        let (mut eye, mut target) = (view.eye, view.target);
        if let Some(bones) = &view.bones
            && let Some((e, t)) = self.bone_camera(stage.scene, bones)
        {
            (eye, target) = (e, t);
        }
        let mut foot = None;
        if let Some(bones) = &view.three_bone
            && let Some((e, t, f)) = self.three_bone_camera(stage.scene, bones)
        {
            (eye, target, foot) = (e, t, Some(f));
        }
        if (target - eye).length() > 1e-3 {
            stage.camera.look_at(eye, target);
        }
        stage.camera.fovy = (2.0 * view.half_fov).to_radians();
        stage.camera.far = stage.camera.far.max(stage.camera.distance * 4.0);
        stage.camera.roll = view.roll.to_radians();
        self.drag_three_bone(stage, view.three_bone.as_ref(), foot);
    }

    /// The render actor of a model target, looked up by borrowed name (no per-frame clone of
    /// the model path).
    pub(super) fn actor_of(&self, target: Target, model: Option<&str>) -> Option<usize> {
        let model = model?;
        self.bindings
            .actors
            .iter()
            .find_map(|((t, path), actor)| (*t == target && path == model).then_some(*actor))
    }

    /// Eye, target and the point the `foot` bone gives, for a camera installed by native 552.
    fn three_bone_camera(
        &self,
        scene: &AnimatedScene,
        bones: &dq9_core::cutscene::ThreeBoneCamera,
    ) -> Option<(Vec3, Vec3, Vec3)> {
        let target = self.cutscene.target_of_character(bones.source);
        let node = self.cutscene.node(target)?;
        let actor = self.actor_of(target, node.model.as_deref())?;
        // The camera rig usually has no polygon, hence no instance: its world is its pose.
        let world = world_of(&node.pose, true);
        Some((
            bone_point(scene, actor, world, &bones.eye)?,
            bone_point(scene, actor, world, &bones.lookat)?,
            bone_point(scene, actor, world, &bones.foot)?,
        ))
    }

    /// Native 552's per-update side effect: the `target` character stands on the source's
    /// `foot` bone, and every third update its facing moves halfway to the direction it moved
    /// (`0204a170`: `(rot + atan2(dx, dz) + 1) >> 1`). One game update is two ticks.
    fn drag_three_bone(
        &mut self,
        stage: &mut Stage,
        bones: Option<&dq9_core::cutscene::ThreeBoneCamera>,
        foot: Option<Vec3>,
    ) {
        let Some(bones) = bones else {
            self.three_bone = None;
            return;
        };
        if !self.ticks.is_multiple_of(2) {
            return;
        }
        let Some(foot) = foot else { return };
        let target = self.cutscene.target_of_character(bones.target);
        let current = self.three_bone.filter(|drag| drag.target == target);
        let mut facing = current.map_or_else(
            || {
                self.cutscene
                    .node(target)
                    .map_or(0.0, |node| node.pose.rotation.y)
            },
            |drag| drag.facing,
        );
        let mut updates = current.map_or(0, |drag| drag.updates) + 1;
        if let Some(previous) = current {
            let (dx, dz) = (foot.x - previous.position.x, foot.z - previous.position.z);
            if updates >= 3 {
                if dx.hypot(dz) > 1e-5 {
                    let heading = dx.atan2(dz);
                    facing = wrap_angle((facing + heading) * 0.5);
                }
                updates = 0;
            }
        }
        self.cutscene.place_target(target, foot, facing);
        if target == Target::Hero {
            *stage.hero = Hero::at(foot, yaw_of_angle(facing));
        }
        self.three_bone = Some(ThreeBoneDrag {
            target,
            position: foot,
            facing,
            updates,
        });
    }

    /// Eye and target of a camera driven by two bones of the model bound to a slot.
    fn bone_camera(
        &self,
        scene: &AnimatedScene,
        bones: &dq9_core::cutscene::BoneCamera,
    ) -> Option<(Vec3, Vec3)> {
        let target = self.cutscene.slot_target(bones.slot)?;
        let node = self.cutscene.node(target)?;
        let actor = self.actor_of(target, node.model.as_deref())?;
        // The model may have no polygon, hence no instance: its world is its pose.
        let world = world_of(&node.pose, true);
        Some((
            bone_point(scene, actor, world, &bones.eye)?,
            bone_point(scene, actor, world, &bones.target)?,
        ))
    }

    /// World matrix a target is drawn with now: the hero, a map NPC, or a model, which hangs on
    /// a bone of another target while it is attached.
    fn target_world(&self, scene: &AnimatedScene, target: Target, depth: u32) -> Option<Mat4> {
        let node = self.cutscene.node(target)?;
        match target {
            Target::Hero => Some(scene.hero_world(node.pose.position, node.pose.rotation.y)),
            Target::Npc(id) => {
                let instance = scene.npc_instance(u16::try_from(id).ok()?)?;
                scene.scene.instances.get(instance).map(|i| i.world)
            }
            Target::Model(_) | Target::Cast(_) => {
                let local = world_of(&node.pose, node.visible && node.model.is_some());
                Some(
                    self.attachment_base(scene, node, depth)
                        .map_or(local, |base| base * local),
                )
            }
            Target::Object(_) | Target::Detached(_) => None,
        }
    }

    /// The matrix a model attached to a bone of another target (natives 235, 236) is placed in:
    /// the parent's world times the bone's (`02037868`, `020378a0`); the model's own pose is
    /// relative to it **[hypothesis]** (the scripts reset the scale of the attached models to
    /// 1, which only makes sense if the parent's scale is inherited). `None` when the model
    /// is not attached or its parent cannot be resolved.
    pub(super) fn attachment_base(
        &self,
        scene: &AnimatedScene,
        node: &Node,
        depth: u32,
    ) -> Option<Mat4> {
        let (parent, bone) = node.attached.as_ref()?;
        if depth > 4 {
            return None;
        }
        if !self.cutscene.node(*parent)?.visible {
            return Some(Mat4::from_scale(Vec3::ZERO));
        }
        let world = self.target_world(scene, *parent, depth + 1)?;
        let bone = match parent {
            Target::Hero => scene.hero_bone(bone),
            Target::Model(_) | Target::Cast(_) => {
                let model = self.cutscene.node(*parent)?;
                match self.actor_of(*parent, model.model.as_deref()) {
                    Some(actor) => scene.actor_bone(actor, bone),
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        };
        Some(world * bone.ok().flatten().unwrap_or(Mat4::IDENTITY))
    }

    /// The balloon of natives 541, 561 and 542: a billboard sprite that lands where the game
    /// draws its 2D sprite, the actor's position projected on the 256 × 192 screen, then offset
    /// (`ov_017::0219b43c`). The scene draws it in the world instead: it is placed on the ray
    /// through that pixel, a little in front of the actor, and scaled so that a texel is one DS
    /// pixel at that depth.
    pub(super) fn present_balloon(&mut self, stage: &mut Stage) {
        let shown = self.cutscene.balloon().copied().and_then(|balloon| {
            let instance = *self.bindings.balloons.get(&balloon.sprite)?;
            Some((instance, self.balloon_world(stage, &balloon, instance)?))
        });
        let all: Vec<usize> = self.bindings.balloons.values().copied().collect();
        for instance in all {
            let world = match shown {
                Some((i, world)) if i == instance => world,
                _ => Mat4::from_scale(Vec3::ZERO),
            };
            self.set_world(stage.scene, instance, world);
        }
    }

    fn balloon_world(&self, stage: &Stage, balloon: &Balloon, instance: usize) -> Option<Mat4> {
        let target = self
            .cutscene
            .actor_target(usize::try_from(balloon.actor).ok()?)?;
        let node = self.cutscene.node(target)?;
        if !node.visible || !node.placed {
            return None;
        }
        let camera = &*stage.camera;
        let view = camera.view();
        let at = view.transform_point3(node.pose.position);
        let depth = -at.z;
        if depth <= camera.near {
            return None;
        }
        let (width, height) = stage.scene.sprite_size(instance)?;
        let half = (camera.fovy / 2.0).tan();
        // DS pixels (256 × 192, y down) of the actor, then of the sprite's centre.
        let px = (
            (at.x / (depth * half * DS_ASPECT) + 1.0) * 0.5 * 256.0,
            (1.0 - at.y / (depth * half)) * 0.5 * 192.0,
        );
        let centre = (
            px.0 + balloon.offset[0] as f32 + width * 0.5,
            px.1 + balloon.offset[1] as f32 + height * 0.5,
        );
        let distance = (depth - 0.25).max(camera.near + 0.05);
        let pixel = 2.0 * distance * half / 192.0;
        let ndc = (centre.0 / 256.0 * 2.0 - 1.0, 1.0 - centre.1 / 192.0 * 2.0);
        let point = Vec3::new(
            ndc.0 * distance * half * DS_ASPECT,
            ndc.1 * distance * half,
            -distance,
        );
        let centre = view.inverse().transform_point3(point);
        // A billboard has 143/4096 world units per texel and its pivot half a sheet above the
        // placement (`0x0203f54c`).
        let scale = pixel / (143.0 / 4096.0);
        Some(
            Mat4::from_translation(centre - Vec3::Y * (height * 0.5 * pixel))
                * Mat4::from_scale(Vec3::splat(scale)),
        )
    }
}
