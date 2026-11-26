//! A map NPC's heading while a conversation holds it. `Msg_ConsumeLeadingTags` (`0x0206a3d0`)
//! gives the window the angle from the partner to the hero, `func_020656f4` hands it to the
//! partner (`func_02040af0`), and the partner's own representation turns towards it at its own
//! rate: a 2D sprite (chara `+0x14`) by `+0x68 = 0x324` × the tick count (`func_0203c818`,
//! `func_0203c794`), a model (chara `+0x18`) by the constant `0x333` of `func_02041378`
//! (`0x0204147c`) once per NPC update. `docs/re/dialogue.md`, § "Turned to the hero".
//!
//! A pivot is kept per NPC id: it starts from the heading the NPC has, turns to the requested
//! angle at [`TURN_STEP`] per 60 Hz step and holds the heading it reaches, so an idle sprite or
//! model animation goes on while the heading stays put. A script writing the heading
//! ([`AnimatedScene::set_npc_angle`], the event natives) ends the pivot: the scene is the
//! script's again. [`AnimatedScene::release_npc`] turns the NPC back to the heading the pivot
//! found, the way `func_02040af0` gives a character the angle the talk task saved.

use glam::{EulerRot, Mat4, Quat, Vec3};

use super::AnimatedScene;

/// A sprite's step per 60 Hz step: `func_0203c794` stores `0x324` in the sprite's `+0x68`, and
/// `func_0203c818` moves its angle `+0x60` towards `+0x64` by `+0x68 × GetTickCount()`. The
/// field's update runs every second VBlank (tick count 2), so one 60 Hz step here is one tick.
pub(super) const SPRITE_STEP: i32 = 0x324;
/// A model's step per NPC update: `func_02041378` adds the constant `iRam0204147c` (`0x333`)
/// to the model angle `+0x54` towards its target `+0xb0`, with no tick multiplication: the
/// model turns at half the sprite's rate per second (`Character_SetFacing`'s `0x324` is the
/// Character/party path, the hero's).
pub(super) const MODEL_STEP: i32 = 0x333;
/// One turn, `FIX_2PI` (`0x6488`): the game stores angles as 4096 per radian.
pub(super) const TURN: i32 = 0x6488;
/// Half a turn, `FIX_PI` (`0x3244`), where [`difference`] changes sign.
pub(super) const HALF: i32 = 0x3244;

/// A conversation pivot: the heading an NPC was given, the one it holds and the one it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pivot {
    pub id: u16,
    /// Angle the model shows, 1/4096 radian, in `[0, TURN)`.
    pub current: i32,
    /// Angle the turn aims at, in `[0, TURN)`.
    pub target: i32,
    /// Heading the NPC had when the pivot started: where a release turns it back.
    pub saved: i32,
    /// Drawn with a 2D sheet (chara `+0x14`) rather than a model (chara `+0x18`): the two turn
    /// at different rates.
    pub sprite: bool,
    /// The release was asked: the pivot goes once `current` is back on `saved`.
    pub released: bool,
}

/// `fix32ReduceAngle0To2Pi` (`0x02030d18`) on a whole number of 4096ths.
pub(super) fn wrap(angle: i32) -> i32 {
    angle.rem_euclid(TURN)
}

/// `fix32SignedAngleDistance` (`0x02030ce8`): the signed angle from `a` to `b`, the short way.
pub(super) fn difference(a: i32, b: i32) -> i32 {
    let mut d = (b - a) % TURN;
    if d > HALF {
        d -= TURN;
    } else if d < -HALF {
        d += TURN;
    }
    d
}

impl AnimatedScene {
    /// Turns the map NPC `id` to the game's object `angle` (radians, 0 faces `+z`,
    /// `(sin, cos)` is the direction) at [`TURN_STEP`] a step, then holds that heading
    /// (`Character_SetFacing` `0x02033844` for the target, `Character_TurnToFacing` `0x02033720`
    /// for the step). The heading the NPC had is kept so [`release_npc`](Self::release_npc)
    /// can turn it back. Returns whether the scene has that NPC.
    pub fn turn_npc(&mut self, id: u16, angle: f32) -> bool {
        let Some(current) = self.npc_heading(id) else {
            return false;
        };
        let target = wrap((angle * 4096.0).round() as i32);
        let sprite = self.npc_is_sprite(id);
        match self.pivots.iter_mut().find(|p| p.id == id) {
            Some(pivot) => {
                // A conversation opening re-saves the partner's heading
                // (`+0x183c = func_02040a90(chara)` at every `TalkTask_Update` case 0), so a
                // later `<END_R_TURN>` gives back the heading of that conversation, not of the
                // first one.
                pivot.saved = pivot.current;
                pivot.target = target;
                pivot.released = false;
            }
            None => self.pivots.push(Pivot {
                id,
                current,
                target,
                saved: current,
                released: false,
                sprite,
            }),
        }
        true
    }

    /// Turns the map NPC `id` towards the world point `point`, at the same rate and with the
    /// same hold ([`func_020338d4`](Self::turn_npc)): `func_020338d4` (`0x020338e4`) subtracts
    /// the object's position from the target, normalises and takes `FX_Atan2` of the ground
    /// plane. The scene uses the position it draws the NPC at, so an NPC a scene moved is
    /// turned from where it stands. Returns whether the scene has that NPC.
    pub fn turn_npc_toward(&mut self, id: u16, point: Vec3) -> bool {
        let Some(position) = self.npc_position(id) else {
            return false;
        };
        let (dx, dz) = (point.x - position.x, point.z - position.z);
        if dx.abs() < 1e-6 && dz.abs() < 1e-6 {
            return true;
        }
        self.turn_npc(id, dx.atan2(dz))
    }

    /// The world position the scene draws the map NPC `id` at: what
    /// [`turn_npc_toward`](Self::turn_npc_toward) aims from. `None` for an unknown id.
    pub fn npc_position(&self, id: u16) -> Option<Vec3> {
        let instance = self.npc_instance(id)?;
        Some(self.scene.instances[instance].world.w_axis.truncate())
    }

    /// Gives the NPC back to its own heading: the pivot turns back to the angle it found
    /// (`func_02040af0(chara, +0x183c)`, the angle the talk task saved) and is dropped on
    /// arrival. Returns whether a pivot was held.
    pub fn release_npc(&mut self, id: u16) -> bool {
        let Some(pivot) = self.pivots.iter_mut().find(|p| p.id == id) else {
            return false;
        };
        pivot.target = pivot.saved;
        pivot.released = true;
        true
    }

    /// The angle the map NPC `id` shows, in radians (see [`turn_npc`](Self::turn_npc)):
    /// the pivot's current heading while one is held, the sprite's own angle or the world
    /// matrix's yaw otherwise. `None` for an unknown id.
    pub fn npc_angle(&self, id: u16) -> Option<f32> {
        self.npc_heading(id).map(|a| a as f32 / 4096.0)
    }

    /// Whether the map NPC `id` is still turning towards its target heading.
    pub fn npc_turning(&self, id: u16) -> bool {
        self.pivots
            .iter()
            .find(|p| p.id == id)
            .is_some_and(|p| p.current != p.target)
    }

    /// Whether a pivot holds the map NPC `id` (turning or holding).
    pub fn npc_held(&self, id: u16) -> bool {
        self.pivots.iter().any(|p| p.id == id)
    }

    /// The heading of the map NPC `id` in 4096ths of a radian: the pivot's while one is held,
    /// the sprite's angle or the world matrix's yaw otherwise.
    fn npc_heading(&self, id: u16) -> Option<i32> {
        if let Some(pivot) = self.pivots.iter().find(|p| p.id == id) {
            return Some(pivot.current);
        }
        let instance = self.npc_instance(id)?;
        if let Some(sprite) = self
            .sprites
            .iter()
            .find(|s| Some(s.model) == self.scene.instances.get(instance).map(|i| i.model))
        {
            return Some(sprite.angle);
        }
        Some(world_yaw(self.scene.instances[instance].world))
    }

    /// Whether the scene draws the map NPC `id` with a 2D sheet (`NpcSprite`, chara `+0x14`)
    /// rather than a model (chara `+0x18`): the two turn at different rates.
    fn npc_is_sprite(&self, id: u16) -> bool {
        let Some(instance) = self
            .npc_instance(id)
            .and_then(|i| self.scene.instances.get(i))
        else {
            return false;
        };
        self.sprites
            .iter()
            .any(|sprite| sprite.model == instance.model)
    }

    /// Writes `angle` (4096ths of a radian) into what draws the map NPC `id`: the sprite's
    /// angle, or the yaw of the model instance, keeping its position and scale.
    fn set_npc_heading(&mut self, id: u16, angle: i32) {
        let Some(instance) = self.npc_instance(id) else {
            return;
        };
        let model = self.scene.instances[instance].model;
        if let Some(sprite) = self.sprites.iter_mut().find(|s| s.model == model) {
            sprite.angle = angle;
            return;
        }
        let (scale, _, translation) = self.scene.instances[instance]
            .world
            .to_scale_rotation_translation();
        self.scene.instances[instance].world = Mat4::from_scale_rotation_translation(
            scale,
            Quat::from_rotation_y(angle as f32 / 4096.0),
            translation,
        );
    }

    /// One step of the pivots alone, without the model clock or the camera: the preview path
    /// (`--screenshot` with `--input`) has neither, and a conversation there must still show
    /// the turn. The window (`App::run_steps`) gets this from [`step`](Self::step).
    pub fn step_turns(&mut self) {
        self.step_pivots();
    }

    /// One step of every pivot, before the scene is evaluated: each turns by [`TURN_STEP`]
    /// towards its target, stops on it, and a released one that is back on its saved heading
    /// is dropped.
    pub(super) fn step_pivots(&mut self) {
        // The model path runs once per NPC update, i.e. once per two 60 Hz steps (the field's
        // 30 Hz cadence, `docs/architecture.md`); the sprite path every step.
        self.pivot_phase = !self.pivot_phase;
        let field_due = self.pivot_phase;
        for pivot in &mut self.pivots {
            let step = match (pivot.sprite, field_due) {
                (true, _) => SPRITE_STEP,
                (false, true) => MODEL_STEP,
                (false, false) => continue,
            };
            let distance = difference(pivot.current, pivot.target);
            pivot.current = if distance.abs() <= step {
                pivot.target
            } else {
                wrap(pivot.current + step * distance.signum())
            };
        }
        let headings: Vec<(u16, i32)> = self
            .pivots
            .iter()
            .map(|pivot| (pivot.id, pivot.current))
            .collect();
        for (id, angle) in headings {
            self.set_npc_heading(id, angle);
        }
        self.pivots
            .retain(|p| !(p.released && p.current == p.target));
    }
}

/// The yaw of a world matrix (`0x54` of the model the game stores, rebuilt from the matrix
/// [`AnimatedScene::character`] placed), in 4096ths of a radian.
fn world_yaw(world: Mat4) -> i32 {
    let (_, rotation, _) = world.to_scale_rotation_translation();
    wrap((rotation.to_euler(EulerRot::YXZ).0 * 4096.0).round() as i32)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::animation::SpriteAnimation;
    use crate::scene::{
        Cull, MaterialLight, PolygonMode, Scene, SceneDraw, SceneInstance, SceneMaterial,
        SceneModel, SceneVertex, Wrap,
    };

    /// A scene of one NPC instance: with a sprite when `sprite` is set, else a model whose
    /// world matrix carries the heading.
    fn scene(id: u16, angle: f32, sprite: bool) -> AnimatedScene {
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
        let world = Mat4::from_translation(Vec3::new(1.0, 0.0, 2.0))
            * Mat4::from_rotation_y(angle)
            * Mat4::from_scale(Vec3::splat(0.25));
        scene.instances.push(SceneInstance {
            model: 0,
            world,
            home: world,
            block: usize::MAX,
            instance: usize::from(id),
        });
        let sprites = if sprite {
            vec![SpriteAnimation::test_sheet((angle * 4096.0).round() as i32)]
        } else {
            Vec::new()
        };
        AnimatedScene {
            scene,
            models: Vec::new(),
            sprites,
            hero: None,
            actors: Vec::new(),
            npc_rigs: Vec::new(),
            cast: Vec::new(),
            pivots: Vec::new(),
            pivot_phase: false,
            billboards_dirty: false,
            frame: 0,
        }
    }

    #[test]
    fn a_turn_steps_at_the_field_rate_and_stops_on_the_target() {
        let mut scene = scene(3, 0.0, true);
        assert!(scene.turn_npc(3, std::f32::consts::PI));
        assert!(scene.npc_turning(3));
        let mut steps = 0;
        while scene.npc_turning(3) {
            scene.step_pivots();
            steps += 1;
            assert!(steps < 100, "the turn converges");
        }
        // Half a turn is 0x3244: 16 steps of 0x324 fall 4 short, the 17th lands on it.
        assert_eq!(steps, 17);
        assert_eq!(scene.npc_angle(3), Some(HALF as f32 / 4096.0));
        // Held: the heading does not drift while the idle sheet keeps playing.
        for _ in 0..10 {
            scene.step_pivots();
        }
        assert_eq!(scene.npc_angle(3), Some(HALF as f32 / 4096.0));
        assert!(scene.npc_held(3));
    }

    #[test]
    fn a_turn_wraps_the_short_way_across_zero() {
        let mut scene = scene(4, 0.0, true);
        // -3π/4 wraps to 5π/4 on the stored side of the circle; the short way crosses 0.
        assert!(scene.turn_npc(4, -0.75 * std::f32::consts::PI));
        let mut steps = 0;
        while scene.npc_turning(4) {
            scene.step_pivots();
            steps += 1;
            assert!(steps < 100, "the turn converges");
        }
        // It went down through 0 instead of up through π: 0x2648 / 0x324, rounded up.
        assert!((13i32 - steps).abs() <= 1);
        let angle = scene.npc_angle(4).unwrap();
        let wanted = wrap((-0.75 * std::f32::consts::PI * 4096.0).round() as i32) as f32 / 4096.0;
        assert!((angle - wanted).abs() < 1e-3, "{angle} against {wanted}");
        assert!(scene.npc_angle(4).unwrap() > std::f32::consts::PI);
    }

    #[test]
    fn a_release_turns_back_and_a_script_takes_the_npc_over() {
        let mut scene = scene(5, 0.5, false);
        let start = scene.npc_angle(5).unwrap();
        assert!(scene.turn_npc(5, start + 1.0));
        // A model turns 0x333 per NPC update, i.e. every second call of `step_pivots`.
        for _ in 0..12 {
            scene.step_pivots();
        }
        assert!((scene.npc_angle(5).unwrap() - (start + 1.0)).abs() < 1e-3);
        let turned = scene.scene.instances[0].world;
        assert!((world_yaw(turned) as f32 / 4096.0 - (start + 1.0)).abs() < 1e-3);
        assert!(scene.release_npc(5));
        assert!(scene.npc_held(5));
        for _ in 0..24 {
            scene.step_pivots();
        }
        assert!(!scene.npc_held(5));
        assert!((scene.npc_angle(5).unwrap() - start).abs() < 1e-3);
        // The model instance kept its position and scale through every step.
        let (scale, _, translation) = scene.scene.instances[0]
            .world
            .to_scale_rotation_translation();
        assert!((scale - Vec3::splat(0.25)).length() < 1e-6);
        assert!((translation - Vec3::new(1.0, 0.0, 2.0)).length() < 1e-6);
        // A script that writes the heading ends the pivot.
        assert!(scene.turn_npc(5, start + 1.0));
        scene.set_npc_angle(5, start);
        assert!(!scene.npc_held(5));
        assert!((scene.npc_angle(5).unwrap() - start).abs() < 1e-3);
    }

    #[test]
    fn a_turn_towards_a_point_uses_the_drawn_position() {
        // The instance stands at (1, 0, 2) (`scene`), whatever the placement said.
        let mut scene = scene(7, 0.0, false);
        assert!(scene.turn_npc_toward(7, Vec3::new(1.0, 0.0, 5.0)));
        assert_eq!(scene.npc_angle(7), Some(0.0), "straight along +z");
        let mut scene = crate::animation::facing::tests::scene(8, 0.0, false);
        assert!(scene.turn_npc_toward(8, Vec3::new(4.0, 3.0, 2.0)));
        let wanted = 3.0f32.atan2(0.0);
        while scene.npc_turning(8) {
            scene.step_pivots();
        }
        assert!((scene.npc_angle(8).unwrap() - wanted).abs() < 1e-3);
        // The target stands on the NPC: no direction, nothing to turn to.
        assert!(scene.turn_npc_toward(8, Vec3::new(1.0, 0.0, 2.0)));
    }

    #[test]
    fn a_model_turns_at_the_field_update_rate_and_a_sprite_at_the_tick_rate() {
        // `func_02041378` adds `0x333` once per NPC update; `func_0203c818` adds `0x324` per
        // tick, i.e. every 60 Hz step.
        let mut model = scene(9, 0.0, false);
        model.turn_npc(9, 1.0);
        model.step_pivots(); // the field update: one model step
        assert_eq!(model.npc_angle(9), Some(MODEL_STEP as f32 / 4096.0));
        model.step_pivots(); // no field update: the model does not move
        assert_eq!(model.npc_angle(9), Some(MODEL_STEP as f32 / 4096.0));
        model.step_pivots();
        assert_eq!(model.npc_angle(9), Some(2.0 * MODEL_STEP as f32 / 4096.0));
        let mut sprite = scene(10, 0.0, true);
        sprite.turn_npc(10, 1.0);
        sprite.step_pivots();
        sprite.step_pivots();
        assert_eq!(
            sprite.npc_angle(10),
            Some(2.0 * SPRITE_STEP as f32 / 4096.0)
        );
    }

    #[test]
    fn a_new_conversation_saves_the_heading_it_finds() {
        // `TalkTask_Update` case 0 re-saves `+0x183c` at every open: after a plain `<END>`
        // leaves the partner turned, the next conversation's `<END_R_TURN>` gives back the
        // heading of *that* conversation, not the first one.
        let mut scene = scene(11, 0.5, true);
        let start = scene.npc_angle(11).unwrap();
        scene.turn_npc(11, start + 2.0);
        while scene.npc_turning(11) {
            scene.step_pivots();
        }
        let turned = scene.npc_angle(11).unwrap();
        scene.turn_npc(11, start + 1.0);
        while scene.npc_turning(11) {
            scene.step_pivots();
        }
        scene.release_npc(11);
        while scene.npc_held(11) {
            scene.step_pivots();
        }
        assert!((scene.npc_angle(11).unwrap() - turned).abs() < 1e-3);
    }

    #[test]
    fn an_unknown_npc_is_not_turned() {
        let mut scene = scene(6, 0.0, true);
        assert!(!scene.turn_npc(16, 1.0));
        assert!(!scene.release_npc(16));
        assert_eq!(scene.npc_angle(16), None);
        assert!(!scene.npc_turning(16));
    }
}
