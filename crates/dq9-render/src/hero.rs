//! The protagonist's animated model: the worn parts of `chara_pc.gp2` assembled on the shared
//! 14-bone rig and played with the motions of `mp0200n`/`mp0200ne` (`docs/re/characters.md`
//! § "The hero"). The parts are merged into one scene model so the application moves a single
//! instance.
//!
//! Body and legs carry the rig: the joint animation poses each of them with the same tracks.
//! Faces, hair and headgear have one bone of their own and are hung on the body's `head` bone
//! (the matrix the body's render commands hold after `MulObject(head)`). Weapons and shields are
//! not drawn.

use std::collections::HashMap;
use std::sync::Arc;

use dq9_formats::Gpc2;
use dq9_formats::chr::Chr;
use dq9_formats::hero::{CharacterColours, ColourRule, HeroLook, ItemModels, Outfit, Skin};
use glam::{Mat4, Vec3};
use nds_rom::g3d::{Fx12, G3dFile, Mat4 as DsMat, Model, Palette, RenderOp, Tex0};
use nds_rom::{Error, Result};

use crate::SceneVertex;
use crate::animation::Clip;
use crate::pose::VertexPose;
use crate::scene::{Builder, Scene, SceneDraw, SceneInstance, SceneModel, TextureSources};

/// Scale of character models (`NpcTable_Build`, `0x0203d038`): `0x10a / 4096`.
const MODEL_SCALE: i32 = 0x10a;

/// Below this speed (units per second) the hero stands.
const STAND_SPEED: f32 = 0.05;

/// The motion the hero plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    Stand,
    Walk,
    Run,
}

impl Gait {
    /// Motion of a hero measured moving at `speed` world units per second over a game update.
    ///
    /// The game picks `stand` for state 0 and `run` for state 1, whatever the speed
    /// (`Character_UpdateAnimation` `0x02033de4` → `func_02033ba0`, `walk` being scripts'
    /// only), so any movement runs. No speed threshold may separate walking from running:
    /// the displacement measured here is horizontal and a staircase shortens it by the cosine
    /// of its slope (4.68 units/s on the 36° ramp of `X01M05` at the top speed of 5.76), which
    /// is why a threshold under the top speed played `walk` on stairs (`docs/re/movement.md`
    /// § "Stairs and the run cycle").
    pub fn from_speed(speed: f32) -> Self {
        if speed < STAND_SPEED {
            Self::Stand
        } else {
            Self::Run
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One part of the model: its file, its pose program and its vertices in the merged model.
struct Part {
    bytes: Arc<[u8]>,
    pose: VertexPose,
    /// Vertices of this part alone, posed in rig space.
    vertices: Vec<SceneVertex>,
    /// First vertex in the merged model.
    base: usize,
    kind: Kind,
}

enum Kind {
    /// On the rig: posed by the joint animation.
    Rig,
    /// Hung on the head bone: posed once at rest, then moved with the head.
    Head(Vec<SceneVertex>),
}

/// The assembled, animated protagonist.
#[derive(Debug)]
pub(crate) struct HeroRig {
    /// Merged model and instance in the scene.
    pub(crate) model: usize,
    pub(crate) instance: usize,
    parts: Vec<Part>,
    /// `Stand`, `Walk`, `Run` joint animations.
    clips: [Clip; 3],
    /// A scripted motion that replaces the gait's until the next [`HeroRig::set_gait`].
    custom: Option<Clip>,
    /// Joint motions of the packages an event added: name, file, speed.
    packages: Vec<(String, Vec<u8>, i32)>,
    /// The motion last started, for the tests and the reports.
    playing: Option<String>,
    gait: Gait,
    scale: [f32; 3],
    pub(crate) dirty: bool,
}

impl std::fmt::Debug for Part {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Part")
            .field("base", &self.base)
            .field("vertices", &self.vertices.len())
            .finish()
    }
}

/// World matrix of every object the render commands multiply by, with the objects' local
/// matrices `objects` (the matrix after `MulObject`, before any `ScaleUp`).
pub(crate) fn object_worlds(model: &Model, objects: &[DsMat]) -> Result<Vec<Option<DsMat>>> {
    let mut out = vec![None; objects.len()];
    let mut stack = [DsMat::IDENTITY; 32];
    let mut cur = DsMat::IDENTITY;
    let slot = |s: u8| {
        let s = usize::from(s);
        if s < 32 {
            Ok(s)
        } else {
            Err(Error::Malformed("hero matrix stack"))
        }
    };
    for op in &model.render_ops {
        match op {
            RenderOp::LoadMatrix { stack_pos } => cur = stack[slot(*stack_pos)?],
            RenderOp::StoreMatrix { stack_pos } => stack[slot(*stack_pos)?] = cur,
            RenderOp::MulObject { object_idx } => {
                let i = usize::from(*object_idx);
                let m = objects
                    .get(i)
                    .ok_or(Error::Malformed("hero object index"))?;
                cur = cur.mul(m);
                out[i] = Some(cur);
            }
            RenderOp::Skin { terms } => {
                let mut m = DsMat::ZERO;
                for t in terms {
                    let inv = model
                        .inv_binds
                        .get(usize::from(t.inv_bind_idx))
                        .ok_or(Error::Malformed("hero inverse bind"))?;
                    let term = stack[slot(t.stack_pos)?].mul(&DsMat::from_mat43(inv));
                    m.add_scaled(&term, f32::from(t.weight) / 256.0);
                }
                cur = m;
            }
            RenderOp::ScaleUp => cur = cur.mul(&DsMat::scale([model.up_scale.to_f32(); 3])),
            RenderOp::ScaleDown => cur = cur.mul(&DsMat::scale([model.down_scale.to_f32(); 3])),
            RenderOp::BindMaterial { .. } | RenderOp::Draw { .. } => {}
        }
    }
    Ok(out)
}

fn to_glam(m: &DsMat) -> Mat4 {
    Mat4::from_cols_array_2d(&m.0)
}

/// A part's file, from `chara_pc.gp2`.
fn member(pack: &Gpc2<'_>, name: &str) -> Result<Vec<u8>> {
    let entry = pack
        .find(name)
        .ok_or(Error::Malformed("hero part missing"))?;
    Ok(pack.member(entry)?.into_owned())
}

/// The first joint-animation file of motion `name` of a motion pack (`mpNNNN?.chr`), with the
/// motion's speed and a loop on `[0, frames − 1)` (`AdvanceAnimations_v1`).
fn motion(packs: &Gpc2<'_>, pack: &str, name: &str) -> Result<Clip> {
    let bytes = member(packs, &format!("{pack}.chr"))?;
    let chr = Chr::parse(&bytes)?;
    let found = chr
        .motions()
        .into_iter()
        .find(|m| m.motion.name == name.as_bytes())
        .ok_or(Error::Malformed("hero motion missing"))?;
    let (_, id) = found
        .files
        .iter()
        .find(|(kind, _)| *kind == dq9_formats::chr::AnimKind::Joint)
        .ok_or(Error::Malformed("hero motion without joint animation"))?;
    let mut clip = Clip::new(chr.narc.file(*id)?.to_vec(), found.motion.speed, None)?;
    clip.range = Some((0, (i32::from(clip.frames) - 1) << 12));
    Ok(clip)
}

/// A recolour `func_020730e0` applies to a part's palettes.
#[derive(Clone, Copy)]
struct Recolour<'a> {
    colours: &'a CharacterColours,
    rule: ColourRule,
    look: &'a HeroLook,
}

/// Builds one part's scene model; `overrides` are texture files looked up before the part's own
/// textures. `own` is the palette rule for the part's own TEX0, `shared` the one for the
/// override files (the arm palette of a body part).
fn build_part(
    scene: &mut Scene,
    name: &str,
    bytes: &[u8],
    overrides: &[Vec<u8>],
    own: Option<Recolour<'_>>,
    shared: Option<Recolour<'_>>,
) -> Result<SceneModel> {
    let file = G3dFile::parse(bytes)?;
    let model = file
        .models
        .first()
        .ok_or(Error::Malformed("hero part without model"))?;
    let mut tex = Vec::new();
    for o in overrides {
        tex.push(G3dFile::parse(o)?.tex);
    }
    tex.push(file.tex.clone());

    // The recolour is a palette write (`func_02099e18`, `func_02099d34`): the texels stay,
    // so it happens on the palette blocks before their textures are decoded. The blocks
    // borrow these copies, one per palette, in `tex` order.
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    for (i, t) in tex.iter().enumerate() {
        let recolour = if i < overrides.len() { shared } else { own };
        for p in t.iter().flat_map(|t| &t.palettes) {
            let mut data = p.data.to_vec();
            if let Some(recolour) = recolour {
                recolour
                    .colours
                    .recolour_block(recolour.rule, recolour.look, &mut data);
            }
            blocks.push(data);
        }
    }
    let tex: Vec<Option<Tex0>> = if own.is_some() || shared.is_some() {
        let mut next = blocks.iter();
        tex.iter()
            .map(|t| {
                t.as_ref().map(|t| Tex0 {
                    textures: t.textures.clone(),
                    palettes: t
                        .palettes
                        .iter()
                        .map(|p| Palette {
                            name: p.name,
                            offset: p.offset,
                            data: next.next().map_or(p.data, Vec::as_slice),
                        })
                        .collect(),
                })
            })
            .collect()
    } else {
        tex
    };

    let sources = TextureSources { tex };
    let mut b = Builder {
        sources: &sources,
        scene: std::mem::take(scene),
        texture_ids: HashMap::new(),
        lit: false,
    };
    let built = b.model("data/pack_lv5/chara_pc.gp2", name, model);
    *scene = b.scene;
    built
}

/// One part to assemble: its model, the texture files that override its own, whether it
/// carries the rig, and the recolour `func_020730e0` applies to its own TEX0 and to the
/// overrides.
struct Job {
    name: String,
    overrides: Vec<Vec<u8>>,
    rig: bool,
    own: Option<ColourRule>,
    shared: Option<ColourRule>,
}

impl HeroRig {
    /// Builds the hero of `look` and adds it to `scene` at the origin.
    pub(crate) fn build<'a>(
        scene: &mut Scene,
        look: &HeroLook,
        file: impl Fn(&str) -> Option<&'a [u8]>,
    ) -> Result<Self> {
        let need = |path: &str| file(path).ok_or(Error::Malformed("hero resource missing"));
        let defs = Gpc2::parse(need("data/prm/itemdt_def.gp2")?)?;
        let def = defs
            .find("itemdt_def.nat")
            .ok_or(Error::Malformed("itemdt_def missing"))?;
        let def_bytes = defs.member(def)?;
        let models = ItemModels::parse(&def_bytes)?;
        let pc = Gpc2::parse(need("data/pack_lv5/chara_pc.gp2")?)?;
        let outfit = Outfit::resolve(look, &models, |n| pc.find(n).is_some());
        let colours = CharacterColours::parse(need("data/chara/palette.bin")?)?;

        // Texture files that replace the rig parts' default arms and footwear.
        let tex = |name: &Option<String>| -> Result<Vec<Vec<u8>>> {
            name.as_ref()
                .map(|n| member(&pc, &format!("{n}.nsbtx")))
                .transpose()
                .map(|t| t.into_iter().collect())
        };
        // `func_020730e0`'s part index per job: 0 armour, 2 face, 3/4 hair, 5 footwear,
        // 6 headgear. Only the face is a fully established rule (`func_02099e18`, case 2):
        // a body part takes `func_02099d34` with the shade count of *its own* record
        // (`func_020de2a4`), and which file fills those eleven records is **[unknown]**
        // (wiki Character-Colours § Not established). The arm palette the batch asks for is
        // the override texture of the body part, whose four-shade ramp is at slots 2–5.
        let skin = Some(ColourRule::Skin(Skin::Four));
        let mut jobs: Vec<Job> = Vec::new();
        jobs.push(Job {
            name: outfit.body.clone(),
            overrides: tex(&outfit.arms_texture)?,
            rig: true,
            own: None,
            shared: skin,
        });
        if let Some(legs) = &outfit.legs {
            jobs.push(Job {
                name: legs.clone(),
                overrides: tex(&outfit.feet_texture)?,
                rig: true,
                own: None,
                shared: None,
            });
        }
        if let Some(face) = &outfit.face {
            jobs.push(Job {
                name: face.clone(),
                overrides: Vec::new(),
                rig: false,
                own: Some(ColourRule::Face),
                shared: None,
            });
        }
        if let Some(hair) = &outfit.hair {
            jobs.push(Job {
                name: hair.model.clone(),
                overrides: vec![member(&pc, &format!("{}.nsbtx", hair.texture))?],
                rig: false,
                own: None,
                shared: None,
            });
        }
        if let Some(head) = &outfit.headgear {
            jobs.push(Job {
                name: head.clone(),
                overrides: Vec::new(),
                rig: false,
                own: None,
                shared: None,
            });
        }

        let mut merged = SceneModel {
            archive: "data/pack_lv5/chara_pc.gp2".to_owned(),
            member: "hero".to_owned(),
            name: "hero".to_owned(),
            vertices: Vec::new(),
            indices: Vec::new(),
            materials: Vec::new(),
            draws: Vec::new(),
            bounds: None,
            lit: false,
            tinted: false,
        };
        let mut parts = Vec::new();
        for Job {
            name,
            overrides,
            rig,
            own,
            shared,
        } in jobs
        {
            let bytes: Arc<[u8]> = member(&pc, &format!("{name}.nsbmd"))?.into();
            let recolour = |rule: Option<ColourRule>| {
                rule.map(|rule| Recolour {
                    colours: &colours,
                    rule,
                    look,
                })
            };
            let built = build_part(
                scene,
                &format!("{name}.nsbmd"),
                &bytes,
                &overrides,
                recolour(own),
                recolour(shared),
            )?;
            let f = G3dFile::parse(&bytes)?;
            let m = &f.models[0];
            let pose = VertexPose::compile(m)?;
            let (base, index_base, material_base) = (
                merged.vertices.len(),
                merged.indices.len(),
                merged.materials.len(),
            );
            let to_u32 = |v: usize| u32::try_from(v).map_err(|_| Error::Malformed("hero index"));
            let vertex_base = to_u32(base)?;
            merged
                .indices
                .extend(built.indices.iter().map(|i| i + vertex_base));
            for d in &built.draws {
                merged.draws.push(SceneDraw {
                    material: d.material + material_base,
                    indices: to_u32(index_base)? + d.indices.start
                        ..to_u32(index_base)? + d.indices.end,
                    center: d.center,
                });
            }
            merged.materials.extend(built.materials);
            merged.vertices.extend_from_slice(&built.vertices);
            let mut part = Part {
                bytes: bytes.clone(),
                pose,
                vertices: built.vertices,
                base,
                kind: Kind::Rig,
            };
            if !rig {
                // One bone of its own: posed once at rest in its own space.
                let rest = m.rest_pose()?;
                part.pose
                    .apply(m, &rest, &mut part.vertices, (false, false))?;
                part.kind = Kind::Head(part.vertices.clone());
            }
            parts.push(part);
        }

        let packs = Gpc2::parse(need("data/pack_lv5/chara_pc.gp2")?)?;
        let clips = [
            motion(&packs, "mp0200n", "stand")?,
            motion(&packs, "mp0200ne", "walk")?,
            motion(&packs, "mp0200n", "run")?,
        ];
        let model = scene.models.len();
        scene.models.push(merged);
        let instance = scene.instances.len();
        scene.instances.push(SceneInstance {
            model,
            world: Mat4::IDENTITY,
            home: Mat4::IDENTITY,
            block: usize::MAX,
            instance: usize::MAX,
        });
        // Object3D scale (`+0x5c`): `0x10a × build / 4096`, truncated to fx16.
        let s = |factor: u16| ((MODEL_SCALE * i32::from(factor)) >> 12) as f32 / 4096.0;
        let (width, height) = (s(look.build.0), s(look.build.1));
        let mut rig = Self {
            model,
            instance,
            parts,
            clips,
            custom: None,
            packages: Vec::new(),
            playing: Some("stand".to_owned()),
            gait: Gait::Stand,
            scale: [width, height, width],
            dirty: true,
        };
        rig.evaluate(scene)?;
        Ok(rig)
    }

    /// Rebinds a rig built in a detached scene to the model and instance slots of the scene
    /// it now belongs to ([`HeroRig::build`] numbers them from zero there).
    pub(crate) fn rebind(&mut self, model: usize, instance: usize) {
        self.model = model;
        self.instance = instance;
        self.dirty = true;
    }

    /// World matrix of the model at `position`, turned by the game's object `angle` (0 faces
    /// +z, `(sin a, cos a)` is the direction).
    pub(crate) fn world(&self, position: Vec3, angle: f32) -> Mat4 {
        Mat4::from_translation(position)
            * Mat4::from_rotation_y(angle)
            * Mat4::from_scale(Vec3::from_array(self.scale))
    }

    /// Starts `gait` from its first frame; the same gait keeps playing. A scripted motion
    /// ([`HeroRig::play`]) ends.
    pub(crate) fn set_gait(&mut self, gait: Gait) {
        if self.custom.take().is_some() || gait != self.gait {
            self.gait = gait;
            self.clips[gait.index()].frame = 0;
        }
        self.playing = Some(
            match gait {
                Gait::Stand => "stand",
                Gait::Walk => "walk",
                Gait::Run => "run",
            }
            .to_owned(),
        );
    }

    pub(crate) fn gait(&self) -> Gait {
        self.gait
    }

    /// Adds the joint motions of a character archive (`event_lv5/evNNNNNp000.chr`, natives
    /// 229 and 567) that [`HeroRig::play`] can start.
    pub(crate) fn add_package(&mut self, chr: &[u8]) -> Result<()> {
        let chr = Chr::parse(chr)?;
        for found in chr.motions() {
            if let Some((_, id)) = found
                .files
                .iter()
                .find(|(kind, _)| *kind == dq9_formats::chr::AnimKind::Joint)
            {
                self.packages.push((
                    found.motion.name_str(),
                    chr.narc.file(*id)?.to_vec(),
                    found.motion.speed,
                ));
            }
        }
        Ok(())
    }

    /// Plays a motion by name, from its first frame: those of the added packages (the last
    /// one wins), then `stand`, `walk` and `run`. Returns whether it exists.
    pub(crate) fn play(&mut self, name: &str, once: bool) -> bool {
        let clip = if let Some((_, bytes, speed)) =
            self.packages.iter().rev().find(|(n, _, _)| n == name)
        {
            Clip::new(bytes.clone(), *speed, None).ok().map(|mut clip| {
                clip.range = Some((0, (i32::from(clip.frames) - 1) << 12));
                clip
            })
        } else {
            let gait = match name {
                "stand" => Some(Gait::Stand),
                "walk" => Some(Gait::Walk),
                "run" => Some(Gait::Run),
                _ => None,
            };
            gait.and_then(|g| {
                let base = &self.clips[g.index()];
                Clip::new(base.bytes.clone(), base.speed, base.range).ok()
            })
        };
        let Some(mut clip) = clip else { return false };
        clip.once = once;
        self.custom = Some(clip);
        self.playing = Some(name.to_owned());
        true
    }

    /// The motion playing and its frame, for the tests and the reports.
    pub(crate) fn animation(&self) -> Option<(&str, i32)> {
        let clip = self
            .custom
            .as_ref()
            .unwrap_or(&self.clips[self.gait.index()]);
        Some((self.playing.as_deref()?, clip.frame))
    }

    /// Times the scripted motion reached its end since it started (0 without one).
    pub(crate) fn wraps(&self) -> u32 {
        self.custom.as_ref().map_or(0, |c| c.wraps)
    }

    /// The scale of the model.
    pub(crate) fn scale(&self) -> Vec3 {
        Vec3::from_array(self.scale)
    }

    /// World matrix of the body bone `bone` in rig space, in the pose of the current frame
    /// (the matrix the render commands hold after `MulObject`), or of the frame the next
    /// step leaves (`next`); `None` when no part of the rig has such a bone.
    pub(crate) fn bone(&self, bone: &str, next: bool) -> Result<Option<Mat4>> {
        let clip = self
            .custom
            .as_ref()
            .unwrap_or(&self.clips[self.gait.index()]);
        let frame = if next { clip.stepped().0 } else { clip.frame };
        let anim = G3dFile::parse(&clip.bytes)?;
        let joint = anim
            .joint_animations
            .first()
            .ok_or(Error::Malformed("hero motion without joint animation"))?;
        for part in &self.parts {
            if !matches!(part.kind, Kind::Rig) {
                continue;
            }
            let f = G3dFile::parse(&part.bytes)?;
            let m = &f.models[0];
            let Some(i) = m.objects.iter().position(|o| o.name.is(bone)) else {
                continue;
            };
            let objects = joint.pose_at(m, Fx12(frame))?;
            if let Some(world) = object_worlds(m, &objects)?[i] {
                return Ok(Some(to_glam(&world)));
            }
        }
        Ok(None)
    }

    /// One 60 Hz update of the current motion.
    pub(crate) fn step(&mut self, scene: &mut Scene) -> Result<()> {
        match &mut self.custom {
            Some(clip) => clip.advance(),
            None => self.clips[self.gait.index()].advance(),
        }
        self.evaluate(scene)
    }

    fn evaluate(&mut self, scene: &mut Scene) -> Result<()> {
        let clip = self
            .custom
            .as_ref()
            .unwrap_or(&self.clips[self.gait.index()]);
        let anim = G3dFile::parse(&clip.bytes)?;
        let joint = anim
            .joint_animations
            .first()
            .ok_or(Error::Malformed("hero motion without joint animation"))?;
        let time = Fx12(clip.frame);
        let mut head = DsMat::IDENTITY;
        for part in &mut self.parts {
            if !matches!(part.kind, Kind::Rig) {
                continue;
            }
            let f = G3dFile::parse(&part.bytes)?;
            let m = &f.models[0];
            let objects = joint.pose_at(m, time)?;
            part.pose
                .apply(m, &objects, &mut part.vertices, (false, false))?;
            if let Some(i) = m.objects.iter().position(|o| o.name.is("head"))
                && let Some(world) = object_worlds(m, &objects)?[i]
            {
                head = world;
            }
        }
        let head = to_glam(&head);
        let target = &mut scene.models[self.model];
        for part in &mut self.parts {
            if let Kind::Head(local) = &part.kind {
                for (out, v) in part.vertices.iter_mut().zip(local) {
                    *out = *v;
                    out.position = head
                        .transform_point3(Vec3::from_array(v.position))
                        .to_array();
                }
            }
            target.vertices[part.base..part.base + part.vertices.len()]
                .copy_from_slice(&part.vertices);
        }
        target.recenter_draws();
        self.dirty = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Horizontal speeds a hero held on the d-pad measures during an update, in units per
    /// second: the speed ramp-up on the flat (1.17 … 4.69), the 36° ramp of `X01M05` at the top
    /// speed (4.68, `dq9-core/tests/stairs_rom.rs`) and the flat top speed (5.76). All of them
    /// are state 1 in the game, hence `run`; only a hero that does not move stands.
    #[test]
    fn a_moving_hero_runs_on_the_flat_and_on_stairs() {
        for speed in [1.17, 2.34, 3.52, 4.68, 4.69, 5.0, 5.76] {
            assert_eq!(Gait::from_speed(speed), Gait::Run, "{speed}");
        }
        assert_eq!(Gait::from_speed(0.0), Gait::Stand);
    }

    /// The face and arm palettes of a built hero are the ones `func_020730e0` writes: the
    /// decoded textures equal a decode with [`CharacterColours`]' rows over the cartridge's
    /// palettes, and two tones differ.
    #[test]
    fn the_hero_decodes_its_face_and_arms_from_the_recoloured_palettes() {
        use nds_rom::g3d::tex::decode_texture;

        let Some(bytes) = testrom::load() else { return };
        let rom = nds_rom::Rom::parse(bytes).unwrap();
        let file = |path: &str| dq9_formats::map::rom_file(&rom, path);
        let pc = dq9_formats::Gpc2::parse(file("data/pack_lv5/chara_pc.gp2").unwrap()).unwrap();

        let mut faces = Vec::new();
        for tone in [0u8, 7] {
            let mut look = HeroLook::new_game();
            look.skin_tone = tone;
            let mut scene = Scene {
                textures: Vec::new(),
                models: Vec::new(),
                instances: Vec::new(),
                unbound_textures: Vec::new(),
                placements: Vec::new(),
                undrawn_placements: Vec::new(),
                warnings: Vec::new(),
                billboards: Default::default(),
            };
            HeroRig::build(&mut scene, &look, file).unwrap();

            let expected = |member: &str, rule| {
                let part = pc.member(pc.find(member).unwrap()).unwrap();
                let g3d = G3dFile::parse(&part).unwrap();
                let tex0 = g3d.tex.as_ref().unwrap();
                let texture = &tex0.textures[0];
                let palette = &tex0.palettes[0];
                let mut data = palette.data.to_vec();
                let colours =
                    CharacterColours::parse(file("data/chara/palette.bin").unwrap()).unwrap();
                assert!(colours.recolour_block(rule, &look, &mut data));
                let palette = Palette {
                    name: palette.name,
                    offset: palette.offset,
                    data: &data,
                };
                (
                    texture.name.to_string(),
                    decode_texture(texture, Some(&palette)).unwrap(),
                )
            };
            let (face_name, face) = expected("p_f004.nsbmd", ColourRule::Face);
            let (arm_name, arm) = expected("p_a007.nsbtx", ColourRule::Skin(Skin::Four));
            let built = |name: &str| {
                scene
                    .textures
                    .iter()
                    .find(|t| t.name == name)
                    .map(|t| t.rgba.clone())
                    .unwrap_or_else(|| panic!("no {name} texture"))
            };
            assert_eq!(built(&face_name), face, "tone {tone} face");
            assert_eq!(built(&arm_name), arm, "tone {tone} arms");
            faces.push(built(&face_name));
        }
        assert_ne!(
            faces[0], faces[1],
            "two skin tones paint the face differently"
        );
    }
}
