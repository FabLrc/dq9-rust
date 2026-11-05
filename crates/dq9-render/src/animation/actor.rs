//! Scripted actors: character models an event loads (`.chr` archives under `chara_sub/`,
//! `event_lv5/`, `effect/`) that the cutscene layer places and animates by name, and the
//! lookups the application needs to move the cast the map already has.
//!
//! An actor is one scene model and one instance, hidden until the application gives it a
//! world matrix. Its motions are the named stretches of the archive's motion table (`BCFG`,
//! `docs/re/characters.md`) plus those of the packages added later (natives 229, 567):
//! a motion with its own animation files plays them; one without plays the stretch
//! `start..end` of the archive's type-0 package file (`InternalLoadFromCHRArchive`,
//! `0x0203644c`). Playing swaps the clips of the model's [`ModelAnimation`], so the generic
//! 60 Hz step advances them.
//!
//! A model without any polygon (the camera rigs of the opening, `ev01000cc000.chr`) has no
//! instance: it only keeps its skeleton and motions, so the camera can follow its bones.

use std::collections::HashMap;
use std::sync::Arc;

use dq9_formats::chr::{AnimKind, Chr};
use glam::{Mat4, Vec3};
use nds_rom::g3d::{Fx12, G3dFile};
use nds_rom::{Error, Result};

use super::{AnimatedScene, Clip};
use crate::hero::object_worlds;
use crate::scene::{Builder, SceneInstance, TextureSources};

/// Frames a motion's clip covers.
#[derive(Debug, Clone, Copy)]
enum Span {
    /// A motion with files of its own loops on `[0, frames − 1)` (`AdvanceAnimations_v1`).
    Whole,
    /// A stretch of a shared package file, `fx32` frames.
    Frames(i32, i32),
}

#[derive(Debug)]
struct ClipSpec {
    bytes: Vec<u8>,
    speed: i32,
    span: Span,
}

#[derive(Debug)]
struct Motion {
    name: String,
    clips: Vec<ClipSpec>,
}

/// The motions of an archive, in table order.
fn collect_motions(chr: &Chr<'_>) -> Result<Vec<Motion>> {
    let slots = chr.slots();
    let mut out = Vec::new();
    for found in chr.motions() {
        let mut clips = Vec::new();
        if found.files.is_empty() {
            // Type-0 package: the whole file, the motion selecting a stretch of it.
            for kind in [
                AnimKind::Joint,
                AnimKind::Material,
                AnimKind::TextureSrt,
                AnimKind::Pattern,
            ] {
                if let Some(id) = slots[kind.slot()] {
                    clips.push(ClipSpec {
                        bytes: chr.narc.file(id)?.to_vec(),
                        speed: found.motion.speed,
                        span: Span::Frames(found.motion.start, found.motion.end),
                    });
                }
            }
        } else {
            for (_, id) in &found.files {
                clips.push(ClipSpec {
                    bytes: chr.narc.file(*id)?.to_vec(),
                    speed: found.motion.speed,
                    span: Span::Whole,
                });
            }
        }
        if !clips.is_empty() {
            out.push(Motion {
                name: found.motion.name_str(),
                clips,
            });
        }
    }
    Ok(out)
}

fn clip(spec: &ClipSpec, once: bool, factor: f32) -> Result<Clip> {
    let speed = (spec.speed as f32 * factor).round() as i32;
    let mut clip = Clip::new(spec.bytes.clone(), speed, None)?;
    clip.range = Some(match spec.span {
        Span::Whole => (0, (i32::from(clip.frames) - 1) << 12),
        Span::Frames(start, end) => (start, end),
    });
    clip.frame = clip.range.map_or(0, |r| r.0);
    clip.once = once;
    Ok(clip)
}

/// The skeleton and clips of a model that is not drawn.
#[derive(Debug)]
struct Ghost {
    bytes: Arc<[u8]>,
    clips: Vec<Clip>,
}

/// One scripted model.
#[derive(Debug)]
pub(crate) struct ActorRig {
    /// Scene instance and index in [`AnimatedScene::models`] of the drawn model.
    drawn: Option<(usize, usize)>,
    ghost: Option<Ghost>,
    motions: Vec<Motion>,
}

impl ActorRig {
    fn clips<'a>(&'a self, models: &'a [super::ModelAnimation]) -> &'a [Clip] {
        match (&self.drawn, &self.ghost) {
            (Some((_, anim)), _) => &models[*anim].clips,
            (None, Some(ghost)) => &ghost.clips,
            (None, None) => &[],
        }
    }
}

impl AnimatedScene {
    /// Adds the model of the character archive `chr` as a scripted actor, hidden until it is
    /// given a world matrix (`scene.instances[`[`actor_instance`](Self::actor_instance)`]`);
    /// `packages` are archives whose motions it can play. Returns the actor's number. Call
    /// before the scene is uploaded.
    pub fn add_actor(&mut self, chr: &[u8], packages: &[&[u8]]) -> Result<usize> {
        let own = Chr::parse(chr)?;
        let model_bytes = own
            .model_data()?
            .ok_or(Error::Malformed("actor archive without model"))?;
        let mut motions = collect_motions(&own)?;
        for package in packages {
            motions.extend(collect_motions(&Chr::parse(package)?)?);
        }
        let file = G3dFile::parse(model_bytes)?;
        let model = file
            .models
            .first()
            .ok_or(Error::Malformed("actor archive without model"))?;
        let sources = TextureSources {
            tex: vec![file.tex.clone()],
        };
        let mut builder = Builder {
            sources: &sources,
            scene: std::mem::take(&mut self.scene),
            texture_ids: HashMap::new(),
            lit: false,
        };
        let built = builder.model("event actor", "model.nsbmd", model);
        self.scene = builder.scene;
        let built = built?;
        let bytes: Arc<[u8]> = model_bytes.into();
        if built.vertices.is_empty() || built.indices.is_empty() {
            self.actors.push(ActorRig {
                drawn: None,
                ghost: Some(Ghost {
                    bytes,
                    clips: Vec::new(),
                }),
                motions,
            });
            return Ok(self.actors.len() - 1);
        }
        let model_id = self.scene.models.len();
        self.scene.models.push(built);
        let instance = self.scene.instances.len();
        self.scene.instances.push(SceneInstance {
            model: model_id,
            world: Mat4::from_scale(Vec3::ZERO),
            block: usize::MAX,
            instance: usize::MAX,
        });
        // Every clip is attached once so the pattern animations' textures are all bound;
        // none plays until asked.
        let all = motions
            .iter()
            .flat_map(|m| m.clips.iter())
            .map(|spec| clip(spec, false, 1.0))
            .collect::<Result<Vec<_>>>()?;
        self.attach(model_id, bytes, 0, all, &sources, None)?;
        let anim = self.models.len() - 1;
        self.models[anim].clips.clear();
        self.models[anim].vertices_dirty = true;
        self.actors.push(ActorRig {
            drawn: Some((instance, anim)),
            ghost: None,
            motions,
        });
        Ok(self.actors.len() - 1)
    }

    /// The scene instance of an actor; `None` for a model without polygons.
    pub fn actor_instance(&self, actor: usize) -> Option<usize> {
        self.actors.get(actor)?.drawn.map(|(instance, _)| instance)
    }

    /// The names of the motions an actor can play.
    pub fn actor_motions(&self, actor: usize) -> Vec<&str> {
        self.actors.get(actor).map_or(Vec::new(), |a| {
            a.motions.iter().map(|m| m.name.as_str()).collect()
        })
    }

    /// Starts motion `name` of an actor from its first frame (`once`: stops on the last
    /// frame instead of looping; `factor` scales the motion's speed). Returns whether the
    /// actor has such a motion; the motion it played goes on otherwise.
    pub fn play_actor(&mut self, actor: usize, name: &str, once: bool, factor: f32) -> bool {
        let Some(rig) = self.actors.get_mut(actor) else {
            return false;
        };
        // Later packages shadow earlier motions of the same name.
        let Some(motion) = rig.motions.iter().rev().find(|m| m.name == name) else {
            return false;
        };
        let Ok(clips) = motion
            .clips
            .iter()
            .map(|spec| clip(spec, once, factor))
            .collect::<Result<Vec<_>>>()
        else {
            return false;
        };
        match (&rig.drawn, &mut rig.ghost) {
            (Some((_, anim)), _) => {
                let state = &mut self.models[*anim];
                state.clips = clips;
                state.vertices_dirty = true;
            }
            (None, Some(ghost)) => ghost.clips = clips,
            (None, None) => return false,
        }
        true
    }

    /// Times the actor's motion reached its end (looping ones included) since it started.
    pub fn actor_wraps(&self, actor: usize) -> u32 {
        self.actors.get(actor).map_or(0, |a| {
            a.clips(&self.models)
                .iter()
                .map(|c| c.wraps)
                .max()
                .unwrap_or(0)
        })
    }

    /// World matrix of the bone `bone` of an actor in its model space, in the pose of the
    /// current frame (the rest pose when no motion plays). `None` when the actor or the bone
    /// does not exist.
    pub fn actor_bone(&self, actor: usize, bone: &str) -> Result<Option<Mat4>> {
        let Some(rig) = self.actors.get(actor) else {
            return Ok(None);
        };
        let bytes = match (&rig.drawn, &rig.ghost) {
            (Some((_, anim)), _) => &self.models[*anim].bytes,
            (None, Some(ghost)) => &ghost.bytes,
            (None, None) => return Ok(None),
        };
        bone_world(bytes, rig.clips(&self.models), bone)
    }

    /// Turns a sprite NPC (`id` as in [`AnimatedScene::npc_instance`]) towards the game's
    /// object `angle` (radians): the sprite shows the side that faces the camera from there
    /// (`NpcSprite_FaceCamera`, `0x0203cbe8`). Models turn with their instance matrix.
    pub fn set_npc_angle(&mut self, id: u16, angle: f32) {
        let Some(instance) = self.npc_instance(id) else {
            return;
        };
        let model = self.scene.instances[instance].model;
        if let Some(sprite) = self.sprites.iter_mut().find(|s| s.model == model) {
            sprite.angle = (angle * 4096.0).round() as i32;
        }
    }

    /// One 60 Hz step of the motions of the models that are not drawn.
    pub(super) fn advance_ghosts(&mut self) {
        for rig in &mut self.actors {
            if let Some(ghost) = &mut rig.ghost {
                for clip in &mut ghost.clips {
                    clip.advance();
                }
            }
        }
    }

    /// The instance of the map NPC `id` (`<map>.npc` field 0), whatever it is drawn with.
    pub fn npc_instance(&self, id: u16) -> Option<usize> {
        self.scene
            .instances
            .iter()
            // The hero and the actors carry `usize::MAX` as their placement.
            .position(|i| i.block == usize::MAX && i.instance == usize::from(id))
    }
}

fn bone_world(bytes: &[u8], clips: &[Clip], bone: &str) -> Result<Option<Mat4>> {
    let file = G3dFile::parse(bytes)?;
    let model = file
        .models
        .first()
        .ok_or(Error::Malformed("actor archive without model"))?;
    let Some(index) = model.objects.iter().position(|o| o.name.is(bone)) else {
        return Ok(None);
    };
    let mut objects = None;
    for clip in clips {
        let anim = G3dFile::parse(&clip.bytes)?;
        if let Some(joint) = anim.joint_animations.first() {
            objects = Some(joint.pose_at(model, Fx12(clip.frame))?);
            break;
        }
    }
    let objects = match objects {
        Some(o) => o,
        None => model.rest_pose()?,
    };
    Ok(object_worlds(model, &objects)?[index]
        .as_ref()
        .map(|m| Mat4::from_cols_array_2d(&m.0)))
}

impl AnimatedScene {
    /// Adds the motions of a character archive the hero can play by name (natives 229, 567).
    pub fn add_hero_package(&mut self, chr: &[u8]) -> Result<()> {
        match &mut self.hero {
            Some(hero) => hero.add_package(chr),
            None => Ok(()),
        }
    }

    /// Plays a motion of the hero by name (`once`: stops on the last frame); the gait the
    /// application sets next ends it. Returns whether the hero has it.
    pub fn play_hero(&mut self, name: &str, once: bool) -> bool {
        self.hero.as_mut().is_some_and(|h| h.play(name, once))
    }

    /// Times the hero's scripted motion reached its end since it started.
    pub fn hero_wraps(&self) -> u32 {
        self.hero.as_ref().map_or(0, |h| h.wraps())
    }

    /// The scale of the hero's model (`0x10a × build / 4096`).
    pub fn hero_scale(&self) -> Vec3 {
        self.hero.as_ref().map_or(Vec3::ONE, |h| h.scale())
    }
}
