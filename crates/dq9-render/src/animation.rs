//! Shared CPU animation state for native, Web and deterministic offscreen frames.
//! ROM access stays with the caller. Only animated models have mutable vertex state.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dq9_formats::Gpc2;
use dq9_formats::cast::{self, Appearance, Story};
use dq9_formats::chr::{Chr, MotionTable};
use dq9_formats::map::{AnimKind, Map};
use dq9_formats::spr::{Sprite, Step};
use glam::{Mat4, Vec3};
use nds_rom::g3d::{Fx12, G3dFile, Name};
use nds_rom::{Error, Narc, Result};

mod actor;

use crate::hero::{Gait, HeroRig};
use crate::pose::VertexPose;
use crate::scene::{
    Builder, Cull, MaterialLight, PolygonMode, SceneDraw, Shade, TextureSources, Wrap, member,
    unpack,
};
use crate::{
    Camera, Renderer, Scene, SceneInstance, SceneMaterial, SceneModel, SceneTexture, SceneVertex,
};

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

#[derive(Debug)]
struct SpriteAnimation {
    model: usize,
    angle: i32,
    frames: Vec<(usize, f32, f32)>,
    animations: Vec<(Vec<u8>, Vec<Step>)>,
    animation: usize,
    step: usize,
    elapsed: u32,
    duration: u32,
    seated: bool,
    flip_y: bool,
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
        let path = cast::archive_path(code);
        if let Some(bytes) = file(&path) {
            let cast = cast::load(bytes, map_id, story)?;
            for npc in &cast.npcs {
                match npc.appearance(|p| file(p).is_some()) {
                    Appearance::None => {}
                    Appearance::Sprite { sheet } => {
                        out.sprite(&sheet, required(&file, &sheet)?, npc)?
                    }
                    appearance => {
                        let (path, bytes) = match appearance {
                            Appearance::Model { chr } => {
                                let bytes = required(&file, &chr)?.to_vec();
                                (chr, bytes)
                            }
                            Appearance::Monster { member: name } => {
                                let enemies =
                                    Gpc2::parse(required(&file, "data/pack_lv5/enemy.gp2")?)?;
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
                            .map(|p| required(&file, p).and_then(Chr::parse))
                            .transpose()?;
                        let clips = match cast::initial_motion(&npc.placement, &own, extra.as_ref())
                        {
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
                        out.character(&path, model, clips, npc)?;
                    }
                }
            }
        }
        out.evaluate()?;
        out.face_camera(&Camera::default())?;
        Ok(out)
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
        let mut b = Builder {
            sources: &ordered,
            scene: std::mem::take(&mut self.scene),
            texture_ids: HashMap::new(),
            lit: false,
        };
        for clip in &clips {
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
                                let id = b
                                    .decode(&texture, palette.as_ref())
                                    .ok_or(Error::Malformed("PAT texture or palette missing"))?;
                                e.insert(id);
                            }
                        }
                    }
                }
            }
        }
        self.scene = b.scene;
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

    fn character(
        &mut self,
        path: &str,
        model_bytes: &[u8],
        clips: Vec<Clip>,
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
            b.scene.instances.push(SceneInstance {
                model: id,
                world,
                block: usize::MAX,
                instance: usize::from(npc.member.id),
            });
            ids.push(id);
        }
        self.scene = b.scene;
        // NPC archives contain one model; keep a separate clock for each placed NPC.
        if !clips.is_empty() {
            let bytes: Arc<[u8]> = model_bytes.into();
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
                for draw in &mut target.draws {
                    let indices =
                        &target.indices[draw.indices.start as usize..draw.indices.end as usize];
                    let sum = indices.iter().fold(Vec3::ZERO, |s, &i| {
                        s + Vec3::from_array(target.vertices[i as usize].position)
                    });
                    draw.center = (sum / indices.len().max(1) as f32).to_array();
                }
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
                if !renderer.update_vertices(queue, state.model, &model.vertices) {
                    return Err(Error::Malformed("animated GPU vertex count"));
                }
                renderer.update_draws(state.model, &model.draws);
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
            if !renderer.update_vertices(queue, hero.model, &model.vertices) {
                return Err(Error::Malformed("hero GPU vertex count"));
            }
            renderer.update_draws(hero.model, &model.draws);
            hero.dirty = false;
        }
        for sprite in &self.sprites {
            let model = &self.scene.models[sprite.model];
            if !renderer.update_vertices(queue, sprite.model, &model.vertices) {
                return Err(Error::Malformed("sprite GPU vertex count"));
            }
            renderer.update_draws(sprite.model, &model.draws);
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

impl AnimatedScene {
    fn sprite(&mut self, path: &str, bytes: &[u8], npc: &cast::Npc<'_>) -> Result<()> {
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
            name: npc.member.name_str(),
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
                alpha: if npc.placement.flags & 0x20 != 0 {
                    16
                } else {
                    31
                },
                cull: Cull::None,
                mode: PolygonMode::Modulate,
                depth_equal: false,
                translucent_depth_write: false,
                uv_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                light: MaterialLight {
                    shade: Shade::Sprite,
                    // Observed in melonDS: the DS fogs the cast's polygons.
                    fog: true,
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
        let position = Vec3::from_array(npc.placement.position.map(|v| v as f32 / 4096.0));
        self.scene.instances.push(SceneInstance {
            model,
            world: Mat4::from_translation(position),
            block: usize::MAX,
            instance: usize::from(npc.member.id),
        });
        let animation = sprite
            .first_animation()
            .min(sprite.animations.len().saturating_sub(1));
        let duration = sprite
            .animations
            .get(animation)
            .and_then(|a| a.steps.first())
            .map_or(0, |s| s.duration);
        self.sprites.push(SpriteAnimation {
            model,
            angle: i32::from(npc.placement.angle).wrapping_add(npc.placement.seat_angle),
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
            seated: npc.placement.flags & 0x800 != 0,
            flip_y: npc.placement.flags & 0x400 != 0,
        });
        Ok(())
    }
}

impl SpriteAnimation {
    fn advance(&mut self) -> Result<()> {
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
            self.step = step.next as usize;
            if self.step >= steps.len() {
                return Err(Error::Malformed("sprite next step out of range"));
            }
            self.duration = step.duration;
            self.elapsed = 0;
        }
        Ok(())
    }

    fn face(&mut self, camera: &Camera, scene: &mut Scene) -> Result<()> {
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
        } else {
            (stand, false)
        };
        let selected = self
            .animations
            .iter()
            .position(|(n, _)| n == wanted.as_bytes());
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
            flip_y: false,
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
