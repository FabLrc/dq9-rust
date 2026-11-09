//! Shared CPU animation state for native, Web and deterministic offscreen frames.
//! ROM access stays with the caller. Only animated models have mutable vertex state.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dq9_formats::Gpc2;
use dq9_formats::cast::{self, Appearance, Story};
use dq9_formats::chr::{Chr, MotionTable};
use dq9_formats::map::{AnimKind, Map};
use glam::{Mat4, Vec3};
use nds_rom::g3d::{Fx12, G3dFile, Name};
use nds_rom::{Error, Narc, Result};

mod actor;
mod sprite;

pub(crate) use sprite::SpriteAnimation;

use crate::hero::{Gait, HeroRig};
use crate::pose::VertexPose;
use crate::scene::{Builder, SceneModel, TextureSources, member, unpack};
use crate::{Camera, Renderer, Scene, SceneInstance};

#[derive(Debug)]
pub(crate) struct Clip {
    pub(crate) bytes: Vec<u8>,
    pub(crate) frame: i32,
    pub(crate) frames: u16,
    pub(crate) speed: i32,
    /// BCFG and per-motion characters provide ranges; plain map models use the full count.
    pub(crate) range: Option<(i32, i32)>,
    /// Scripted motions (flag `0x01` of native 210) stop on their last frame instead of
    /// looping.
    pub(crate) once: bool,
    /// Times the clock reached the end of its range since it was made.
    pub(crate) wraps: u32,
}

impl Clip {
    pub(crate) fn new(bytes: Vec<u8>, speed: i32, range: Option<(i32, i32)>) -> Result<Self> {
        let f = G3dFile::parse(&bytes)?;
        let frames = f
            .joint_animations
            .first()
            .map(|a| a.frame_count)
            .or_else(|| f.material_animations.first().map(|a| a.frame_count))
            .or_else(|| f.tex_srt_animations.first().map(|a| a.frame_count))
            .or_else(|| f.patterns.first().map(|a| a.num_frames))
            .ok_or(Error::Malformed("animation file without animation"))?;
        Ok(Self {
            bytes,
            frame: range.map_or(0, |r| r.0),
            frames,
            speed,
            range,
            once: false,
            wraps: 0,
        })
    }
    pub(crate) fn advance(&mut self) {
        // Plain map models: Animation3D::AdvanceTimer (0x0207e178), full frame count.
        // Object3D::AdvanceAnimations_v0/v1 (0x02034e38/0x02035188) supplies explicit ranges.
        let (start, end) = self.range.unwrap_or((0, i32::from(self.frames) << 12));
        let length = i64::from(end) - i64::from(start);
        if length <= 0 {
            self.frame = start;
            return;
        }
        let raw = i64::from(self.frame) - i64::from(start) + i64::from(self.speed);
        if self.once {
            // Holds the last pose once the end is reached.
            let last = if self.range.is_some() {
                end
            } else {
                end - 0x1000
            };
            if self.wraps > 0 {
                self.frame = last;
            } else if raw >= length || raw < 0 {
                self.wraps += 1;
                self.frame = last;
            } else {
                self.frame = (i64::from(start) + raw) as i32;
            }
            return;
        }
        if raw >= length || raw < 0 {
            self.wraps += 1;
        }
        self.frame = (i64::from(start) + raw.rem_euclid(length)) as i32;
    }
}

#[derive(Debug)]
struct ModelAnimation {
    model: usize,
    bytes: Arc<[u8]>,
    model_index: usize,
    clips: Vec<Clip>,
    pose: VertexPose,
    textures: HashMap<(Name, Option<Name>), usize>,
    vertices_dirty: bool,
}

/// An uploaded scene's independent CPU clock and deformation state. [`Scene::build`] and
/// static [`Renderer`] users do not pay for animation state or per-tick geometry work.
#[derive(Debug)]
pub struct AnimatedScene {
    pub scene: Scene,
    models: Vec<ModelAnimation>,
    sprites: Vec<SpriteAnimation>,
    hero: Option<HeroRig>,
    actors: Vec<actor::ActorRig>,
    /// The model NPCs of the cast and the motions an event can make them play.
    npc_rigs: Vec<actor::NpcRig>,
    /// Scene instances of the map's cast (`<area>.npc`), replaced by
    /// [`AnimatedScene::rebuild_cast`].
    cast: Vec<usize>,
    pub frame: u64,
}

fn required<'a>(file: &impl Fn(&str) -> Option<&'a [u8]>, path: &str) -> Result<&'a [u8]> {
    file(path).ok_or(Error::Malformed("runtime resource missing"))
}

fn decoded_member(narc: &Narc<'_>, name: &str) -> Result<Vec<u8>> {
    Ok(
        unpack(member(narc, name).ok_or(Error::Malformed("animation member missing"))?)?
            .into_owned(),
    )
}

/// Re-uploads the posed geometry of `model`: its vertices, then the draw centres sorted from
/// them. A vertex-count mismatch is an error, never a silently frozen animation.
fn upload_geometry(
    renderer: &mut Renderer,
    queue: &wgpu::Queue,
    model: usize,
    data: &SceneModel,
    error: &'static str,
) -> Result<()> {
    if !renderer.update_vertices(queue, model, &data.vertices) {
        return Err(Error::Malformed(error));
    }
    renderer.update_draws(model, &data.draws);
    Ok(())
}

impl AnimatedScene {
    /// Builds map animations selected by the parts' resource flags, then the cast selected
    /// by `story`. Missing optional cast archives mean no NPCs; malformed resources fail.
    pub fn build<'a>(
        map: &Map,
        map_id: u16,
        code: &[u8],
        story: &Story,
        file: impl Fn(&str) -> Option<&'a [u8]>,
    ) -> Result<Self> {
        let mut out = Self {
            scene: Scene::build(map, &file),
            models: Vec::new(),
            sprites: Vec::new(),
            hero: None,
            actors: Vec::new(),
            frame: 0,
            npc_rigs: Vec::new(),
            cast: Vec::new(),
        };
        let ambl = Narc::parse(required(&file, &map.files.ambl)?)?;
        let tex_bytes = map
            .textures
            .iter()
            .map(|n| decoded_member(&ambl, n))
            .collect::<Result<Vec<_>>>()?;
        let sources = TextureSources {
            tex: tex_bytes
                .iter()
                .map(|b| G3dFile::parse(b).map(|f| f.tex))
                .collect::<Result<_>>()?,
        };
        let mut seen = HashSet::new();
        // Scene model cache keys include part animation selection, so shared geometry never
        // accidentally animates a static placement of the same .nsbmd.
        for instance in 0..out.scene.instances.len() {
            let inst = out.scene.instances[instance].clone();
            if !seen.insert(inst.model) {
                continue;
            }
            let block = &map.blocks[inst.block];
            let placement = &block.instances[inst.instance];
            let part = &block.parts[placement
                .part
                .ok_or(Error::Malformed("runtime part missing"))?];
            if part.animations.is_empty() {
                continue;
            }
            let narc = Narc::parse(required(&file, &block.archive)?)?;
            let name = part
                .file
                .as_ref()
                .ok_or(Error::Malformed("animated model missing"))?;
            let bytes: Arc<[u8]> = decoded_member(&narc, name)?.into();
            let mut speed = 4096;
            let mut range = None;
            if let Some(bcfg) = part.animations.iter().find(|a| a.kind == AnimKind::Bcfg) {
                let table_bytes = decoded_member(&narc, &bcfg.member)?;
                let table = MotionTable::parse(&table_bytes)?;
                if let Some(motion) = table.motions.first() {
                    speed = motion.speed;
                    range = Some((motion.start, motion.end));
                }
            }
            let clips = part
                .animations
                .iter()
                .filter(|a| a.kind != AnimKind::Bcfg)
                .map(|a| Clip::new(decoded_member(&narc, &a.member)?, speed, range))
                .collect::<Result<Vec<_>>>()?;
            if !clips.is_empty() {
                // Models are matched by their position in the file, not by a guessed name.
                let f = G3dFile::parse(&bytes)?;
                let index = f
                    .models
                    .iter()
                    .position(|m| m.name.is(&out.scene.models[inst.model].name))
                    .ok_or(Error::Malformed("runtime model missing"))?;
                out.attach(
                    inst.model,
                    bytes,
                    index,
                    clips,
                    &sources,
                    part.texture_source,
                )?;
            }
        }
        out.add_cast(map_id, code, story, &file)?;
        out.evaluate()?;
        out.face_camera(&Camera::default())?;
        Ok(out)
    }

    /// The cast task of trigger action 0x8d (`ov_017::021c16cc` → `ov_017::021a2fa0`): the
    /// map's cast is torn down and `Cast_LoadMap` (`ov_017::021a2c14`) runs again for
    /// `story`, on the same map. The old instances stay in the scene, hidden and without an
    /// NPC id, so the indices the hero, the event actors and their bindings hold remain
    /// valid; their animations stop. Upload the scene again afterwards.
    pub fn rebuild_cast<'a>(
        &mut self,
        map_id: u16,
        code: &[u8],
        story: &Story,
        file: impl Fn(&str) -> Option<&'a [u8]>,
    ) -> Result<()> {
        let mut old = HashSet::new();
        for index in std::mem::take(&mut self.cast) {
            let instance = &mut self.scene.instances[index];
            old.insert(instance.model);
            instance.world = Mat4::from_scale(Vec3::ZERO);
            instance.home = instance.world;
            instance.instance = usize::MAX;
        }
        self.npc_rigs.clear();
        self.sprites.retain(|sprite| !old.contains(&sprite.model));
        self.models.retain(|model| !old.contains(&model.model));
        self.add_cast(map_id, code, story, &file)?;
        self.evaluate()
    }

    /// Adds the cast of `map_id` at `story` from its area's `.npc` archive (none without
    /// it): sprites, models and monsters.
    fn add_cast<'a>(
        &mut self,
        map_id: u16,
        code: &[u8],
        story: &Story,
        file: &impl Fn(&str) -> Option<&'a [u8]>,
    ) -> Result<()> {
        let path = cast::archive_path(code);
        let Some(bytes) = file(&path) else {
            return Ok(());
        };
        let cast = cast::load(bytes, map_id, story)?;
        for npc in &cast.npcs {
            match npc.appearance(|p| file(p).is_some()) {
                Appearance::None => {}
                Appearance::Sprite { sheet } => {
                    self.sprite(&sheet, required(file, &sheet)?, npc)?
                }
                appearance => {
                    let (path, bytes) = match appearance {
                        Appearance::Model { chr } => {
                            let bytes = required(file, &chr)?.to_vec();
                            (chr, bytes)
                        }
                        Appearance::Monster { member: name } => {
                            let enemies = Gpc2::parse(required(file, "data/pack_lv5/enemy.gp2")?)?;
                            let entry = enemies
                                .entries
                                .iter()
                                .find(|e| e.name == name)
                                .ok_or(Error::Malformed("monster member missing"))?;
                            let mon = enemies.member(entry)?;
                            let monster = Narc::parse(&mon)?;
                            let cchr = monster
                                .names
                                .files
                                .iter()
                                .find(|e| e.path.ends_with(".cchr"))
                                .ok_or(Error::Malformed("monster CCHR missing"))?;
                            let raw = monster.file(cchr.id)?;
                            let bytes = nds_rom::compress::decompress(raw)?;
                            (name, bytes)
                        }
                        _ => unreachable!(),
                    };
                    let own = Chr::parse(&bytes)?;
                    let extra_path = npc
                        .placement
                        .chr
                        .map(|p| format!("data/{}", String::from_utf8_lossy(p)));
                    let extra = extra_path
                        .as_deref()
                        .map(|p| required(file, p).and_then(Chr::parse))
                        .transpose()?;
                    let clips = match cast::initial_motion(&npc.placement, &own, extra.as_ref()) {
                        Some((package, motion)) => {
                            let chr = if package == 0 {
                                &own
                            } else {
                                extra
                                    .as_ref()
                                    .ok_or(Error::Malformed("motion package missing"))?
                            };
                            let motions = chr.motions();
                            let motion = &motions[motion];
                            motion
                                .files
                                .iter()
                                .map(|(_, id)| {
                                    let mut clip = Clip::new(
                                        chr.narc.file(*id)?.to_vec(),
                                        motion.motion.speed,
                                        None,
                                    )?;
                                    // Per-motion characters loop before the last image,
                                    // unlike plain map models (AdvanceAnimations_v1).
                                    clip.range = Some((0, (i32::from(clip.frames) - 1) << 12));
                                    Ok(clip)
                                })
                                .collect::<Result<Vec<_>>>()?
                        }
                        None => Vec::new(),
                    };
                    let model = own
                        .model_data()?
                        .ok_or(Error::Malformed("NPC archive without model"))?;
                    // The named motions an event can start later (`play_npc`): the NPC's
                    // own archive, then its extra package.
                    let mut named = actor::collect_motions(&own).unwrap_or_default();
                    if let Some(extra) = &extra {
                        named.extend(actor::collect_motions(extra).unwrap_or_default());
                    }
                    self.character(&path, model, clips, named, npc)?;
                }
            }
        }
        Ok(())
    }

    fn attach(
        &mut self,
        model: usize,
        bytes: Arc<[u8]>,
        model_index: usize,
        clips: Vec<Clip>,
        sources: &TextureSources<'_>,
        primary: Option<usize>,
    ) -> Result<()> {
        let f = G3dFile::parse(&bytes)?;
        let m = &f.models[model_index];
        let pose = VertexPose::compile(m)?;
        let mut textures = HashMap::new();
        let ordered = TextureSources {
            tex: primary
                .into_iter()
                .filter_map(|i| sources.tex.get(i).cloned())
                .chain(sources.tex.iter().cloned())
                .collect(),
        };
        self.bind_patterns(m, &clips, &ordered, &mut textures)?;
        self.models.push(ModelAnimation {
            model,
            bytes,
            model_index,
            clips,
            pose,
            textures,
            vertices_dirty: true,
        });
        Ok(())
    }

    /// Decodes into the scene the textures that the pattern animations of `clips` switch the
    /// materials of `m` between, so that every frame finds its texture bound.
    fn bind_patterns(
        &mut self,
        m: &nds_rom::g3d::Model<'_>,
        clips: &[Clip],
        ordered: &TextureSources<'_>,
        textures: &mut HashMap<(Name, Option<Name>), usize>,
    ) -> Result<()> {
        let mut b = Builder {
            sources: ordered,
            scene: std::mem::take(&mut self.scene),
            texture_ids: HashMap::new(),
            lit: false,
        };
        let bound = (|| -> Result<()> {
            for clip in clips {
                let f = G3dFile::parse(&clip.bytes)?;
                for pattern in &f.patterns {
                    for track in &pattern.tracks {
                        let Some(material) = m.materials.iter().find(|m| m.name == track.material)
                        else {
                            continue;
                        };
                        let mut palette = material.palette_name;
                        // The first pass starts with the model palette; subsequent loops
                        // retain the last explicitly selected palette for 0xff keys.
                        for _ in 0..2 {
                            for key in &track.keyframes {
                                if let Some(p) = key.palette() {
                                    palette = Some(pattern.palette_names[usize::from(p)]);
                                }
                                let texture = pattern.texture_names[usize::from(key.texture_idx)];
                                if let std::collections::hash_map::Entry::Vacant(e) =
                                    textures.entry((texture, palette))
                                {
                                    let id = b.decode(&texture, palette.as_ref()).ok_or(
                                        Error::Malformed("PAT texture or palette missing"),
                                    )?;
                                    e.insert(id);
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        self.scene = b.scene;
        bound
    }

    fn character(
        &mut self,
        path: &str,
        model_bytes: &[u8],
        clips: Vec<Clip>,
        motions: Vec<actor::Motion>,
        npc: &cast::Npc<'_>,
    ) -> Result<()> {
        let f = G3dFile::parse(model_bytes)?;
        let sources = TextureSources {
            tex: vec![f.tex.clone()],
        };
        let mut b = Builder {
            sources: &sources,
            scene: std::mem::take(&mut self.scene),
            texture_ids: HashMap::new(),
            lit: false,
        };
        // NpcTable_Build (0x0203d038): world position, Y angle and 0x10a uniform scale.
        let world = Mat4::from_translation(Vec3::from_array(
            npc.placement.position.map(|v| v as f32 / 4096.0),
        )) * Mat4::from_rotation_y(f32::from(npc.placement.angle) / 4096.0)
            * Mat4::from_scale(Vec3::splat(266.0 / 4096.0));
        let mut ids = Vec::new();
        for model in &f.models {
            let scene_model = b.model(path, "model.nsbmd", model)?;
            let id = b.scene.models.len();
            b.scene.models.push(scene_model);
            self.cast.push(b.scene.instances.len());
            b.scene.instances.push(SceneInstance {
                model: id,
                world,
                home: world,
                block: usize::MAX,
                instance: usize::from(npc.member.id),
            });
            ids.push(id);
        }
        self.scene = b.scene;
        let bytes: Arc<[u8]> = model_bytes.into();
        if let Some(&model) = ids.first() {
            self.npc_rigs.push(actor::NpcRig::new(
                npc.member.id,
                model,
                bytes.clone(),
                motions,
            ));
        }
        // NPC archives contain one model; keep a separate clock for each placed NPC.
        if !clips.is_empty() {
            for (index, id) in ids.into_iter().enumerate() {
                let copies = clips
                    .iter()
                    .map(|c| Clip::new(c.bytes.clone(), c.speed, c.range))
                    .collect::<Result<Vec<_>>>()?;
                self.attach(id, bytes.clone(), index, copies, &sources, None)?;
            }
        }
        Ok(())
    }

    /// Adds the protagonist (`look`: a new game's hero or a save's) at the origin and returns
    /// its instance index. Call before the scene is uploaded; the application then places it
    /// each update with [`AnimatedScene::hero_world`].
    pub fn add_hero<'a>(
        &mut self,
        look: &dq9_formats::hero::HeroLook,
        file: impl Fn(&str) -> Option<&'a [u8]>,
    ) -> Result<usize> {
        let rig = HeroRig::build(&mut self.scene, look, file)?;
        let instance = rig.instance;
        self.hero = Some(rig);
        Ok(instance)
    }

    /// World matrix of the hero at `position`, facing the game's object `angle` (0 faces +z,
    /// `(sin a, cos a)` is the direction; `dq9_core::field::angle_of_yaw` converts a
    /// `dq9_core::Hero` yaw).
    pub fn hero_world(&self, position: Vec3, angle: f32) -> Mat4 {
        self.hero
            .as_ref()
            .map_or(Mat4::from_translation(position), |h| {
                h.world(position, angle)
            })
    }

    /// Chooses the hero's motion (idle, walk, run); the same motion keeps playing.
    pub fn set_hero_gait(&mut self, gait: Gait) {
        if let Some(hero) = &mut self.hero {
            hero.set_gait(gait);
        }
    }

    /// The motion the hero plays.
    pub fn hero_gait(&self) -> Option<Gait> {
        self.hero.as_ref().map(HeroRig::gait)
    }

    /// One deterministic 60 Hz update. GPU uploads can be batched after several updates.
    pub fn step(&mut self, camera: &Camera) -> Result<()> {
        self.face_camera(camera)?;
        self.frame += 1;
        self.advance_ghosts();
        for model in &mut self.models {
            for clip in &mut model.clips {
                clip.advance();
            }
        }
        for sprite in &mut self.sprites {
            sprite.advance()?;
        }
        if let Some(hero) = &mut self.hero {
            hero.step(&mut self.scene)?;
        }
        self.evaluate()
    }

    fn evaluate(&mut self) -> Result<()> {
        for state in &mut self.models {
            let mut file = G3dFile::parse(&state.bytes)?;
            let model = &mut file.models[state.model_index];
            let mut objects = None;
            let mut vertices_dirty = false;
            let target = &mut self.scene.models[state.model];
            for clip in &state.clips {
                let anim = G3dFile::parse(&clip.bytes)?;
                for joint in &anim.joint_animations {
                    objects = Some(joint.pose_at(model, Fx12(clip.frame))?);
                    vertices_dirty = true;
                }
                for mat in &anim.material_animations {
                    for (i, m) in model.materials.iter_mut().enumerate() {
                        if let Some(t) = mat.track(&m.name) {
                            mat.colors(t, (clip.frame >> 12) as u16)?.apply(m);
                            target.materials[i].alpha = m.alpha();
                            let light = &mut target.materials[i].light;
                            light.diffuse = m.diffuse();
                            light.ambient = m.ambient();
                            light.specular = m.specular();
                            light.emission = m.emission();
                            vertices_dirty = true;
                        }
                    }
                }
                for srt in &anim.tex_srt_animations {
                    for (i, m) in model.materials.iter().enumerate() {
                        if let Some(matrix) = srt.matrix(model, i, (clip.frame >> 12) as u16)? {
                            target.materials[i].uv_transform = matrix
                                .normalized(f32::from(m.width.max(1)), f32::from(m.height.max(1)));
                        }
                    }
                }
                for pat in &anim.patterns {
                    for (i, m) in model.materials.iter().enumerate() {
                        if let Some(t) = pat.track(&m.name)
                            && let Some((texture, palette)) = pat.at(t, (clip.frame >> 12) as u16)
                        {
                            let palette = palette.or_else(|| {
                                target.materials[i].palette_name.as_deref().and_then(name)
                            });
                            let id = state
                                .textures
                                .get(&(texture, palette))
                                .copied()
                                .ok_or(Error::Malformed("PAT texture or palette unbound"))?;
                            target.materials[i].texture = Some(id);
                            if target.materials[i].palette_name.as_deref().and_then(name) != palette
                            {
                                target.materials[i].palette_name = palette.map(|n| n.to_string());
                            }
                        }
                    }
                }
            }
            if vertices_dirty {
                let objects = match objects {
                    Some(o) => o,
                    None => model.rest_pose()?,
                };
                state.pose.apply(
                    model,
                    &objects,
                    &mut target.vertices,
                    (target.lit, target.tinted),
                )?;
                target.recenter_draws();
                state.vertices_dirty = true;
            }
        }
        Ok(())
    }

    /// Face sprites toward the exact render camera, including when no simulation step was
    /// due. This must run after selecting a screenshot camera and before uploading.
    pub fn face_camera(&mut self, camera: &Camera) -> Result<()> {
        for sprite in &mut self.sprites {
            sprite.face(camera, &mut self.scene)?;
        }
        Ok(())
    }

    /// Upload only animated vertex buffers/materials; the renderer keeps topology and
    /// static models intact. A mismatch is an error, never a silently frozen animation.
    pub fn upload(
        &mut self,
        renderer: &mut Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<()> {
        for state in &mut self.models {
            let model = &self.scene.models[state.model];
            if state.vertices_dirty {
                upload_geometry(
                    renderer,
                    queue,
                    state.model,
                    model,
                    "animated GPU vertex count",
                )?;
                state.vertices_dirty = false;
            }
            for (i, material) in model.materials.iter().enumerate() {
                renderer.update_material(device, queue, state.model, i, material)?;
            }
        }
        if let Some(hero) = &mut self.hero
            && hero.dirty
        {
            let model = &self.scene.models[hero.model];
            upload_geometry(renderer, queue, hero.model, model, "hero GPU vertex count")?;
            hero.dirty = false;
        }
        if let Some(hero) = &self.hero {
            for (i, material) in self.scene.models[hero.model].materials.iter().enumerate() {
                renderer.update_material(device, queue, hero.model, i, material)?;
            }
        }
        for sprite in &self.sprites {
            let model = &self.scene.models[sprite.model];
            upload_geometry(
                renderer,
                queue,
                sprite.model,
                model,
                "sprite GPU vertex count",
            )?;
            renderer.update_material(device, queue, sprite.model, 0, &model.materials[0])?;
        }
        Ok(())
    }
}

fn name(s: &str) -> Option<Name> {
    if s.len() > 16 {
        return None;
    }
    let mut bytes = [0; 16];
    bytes[..s.len()].copy_from_slice(s.as_bytes());
    Some(Name(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_wraps_full_files_and_bcfg_ranges() {
        let mut file = Clip {
            bytes: Vec::new(),
            frame: 0,
            frames: 9,
            speed: 2048,
            range: None,
            once: false,
            wraps: 0,
        };
        for _ in 0..16 {
            file.advance();
        }
        assert_eq!(file.frame, 8 << 12);
        file.advance();
        file.advance();
        assert_eq!(file.frame, 0);
        let mut ranged = Clip {
            bytes: Vec::new(),
            frame: 10 << 12,
            frames: 30,
            speed: 4096,
            range: Some((10 << 12, 14 << 12)),
            once: false,
            wraps: 0,
        };
        for _ in 0..5 {
            ranged.advance();
        }
        assert_eq!(ranged.frame, 11 << 12);
        ranged.speed = -4096;
        ranged.advance();
        ranged.advance();
        assert_eq!(ranged.frame, 13 << 12);
    }

    #[test]
    fn one_frame_animation_stays_at_zero() {
        let mut clip = Clip {
            bytes: Vec::new(),
            frame: 0,
            frames: 1,
            speed: 8192,
            range: None,
            once: false,
            wraps: 0,
        };
        for _ in 0..60 {
            clip.advance();
        }
        assert_eq!(clip.frame, 0);
    }
}
