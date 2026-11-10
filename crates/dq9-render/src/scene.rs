//! CPU side of the renderer: everything the GPU needs for one assembled map, without touching
//! the GPU. [`Scene::build`] decodes each shown part's `.nsbmd` (`nds_rom::g3d`), binds its
//! materials to the textures of the map's `.nsbtx` files, runs the display lists into indexed
//! triangles (`Geometry::build`, rest pose) and places one instance per placement with its
//! world transform. A placement hidden at the chosen time of day is built too, with a null
//! scale, because natives 574 and 575 can show or move it.
//!
//! Approximations of the DS 3D engine are listed in `docs/app.md`.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use dq9_formats::map::{Map, PartKind, Transform};
use glam::{Mat3, Mat4, Vec3};
use nds_rom::compress::{Codec, decode};
use nds_rom::g3d::tex::decode_texture;
use nds_rom::g3d::{G3dFile, Geometry, Material, Model, Name, Palette, Tex0, TexMatrix, Texture};
use nds_rom::{Error, Narc, Result};

use crate::Camera;

/// Texture addressing along one axis (`TEXIMAGE_PARAM` bits 16-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wrap {
    Clamp,
    Repeat,
    Mirror,
}

/// Faces a material hides (`POLYGON_ATTR` bits 6-7: render back, render front).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cull {
    None,
    Back,
    Front,
    /// Neither side is rendered.
    All,
}

/// `POLYGON_ATTR` bits 4-5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolygonMode {
    /// Texture × vertex colour.
    Modulate,
    /// Texture over the vertex colour by texture alpha.
    Decal,
    /// Toon or highlight shading (drawn as [`Modulate`](Self::Modulate) here).
    Toon,
    /// Shadow volume polygons (not drawn here).
    Shadow,
}

/// GPU vertex: model-space position, texture coordinate normalised to the texture size (DS
/// orientation, `v` down, which is also wgpu's), vertex colour.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct SceneVertex {
    pub position: [f32; 3],
    pub texcoord: [f32; 2],
    pub color: [f32; 3],
    /// Unit normal in model space; zero for a vertex that carries its colour. The renderer
    /// lights the others with the DS lights of the map ([`Shade::Map`]).
    pub normal: [f32; 3],
    /// 1 when the zone's basic lighting transforms this vertex colour (the mesh sends `COLOR`
    /// commands, `LightingManager::ModelTransformTintBrightnessContrast`), else 0.
    pub tint: f32,
}

/// A decoded texture with the palette it was bound with.
#[derive(Debug, Clone)]
pub struct SceneTexture {
    pub name: String,
    pub palette: Option<String>,
    /// Index of its `.nsbtx` in [`Map::textures`].
    pub source: usize,
    pub width: u32,
    pub height: u32,
    /// RGBA8 rows, top to bottom.
    pub rgba: Vec<u8>,
    /// Some texel has an alpha strictly between 0 and 255 (A3I5, A5I3 formats).
    pub translucent: bool,
}

/// How the renderer lights a material's vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Shade {
    /// Vertex colour only.
    #[default]
    Unlit,
    /// Map model: vertices with a normal get the DS lighting of the zone, the others the
    /// zone's colour transform (basic lighting).
    Map,
    /// Billboard sprite, multiplied by the zone's sprite diffuse colour.
    Sprite,
    /// Character model (colours baked with a stand-in light), multiplied by the zone's model
    /// diffuse colour.
    Character,
}

/// Default `POLYGON_ATTR` of `RenderConfig::Reset` (`0x02050?`: alpha 31, front faces, four
/// lights), completed by the bits a material's mask selects.
const DEFAULT_POLYGON_ATTR: u32 = (0x1f << 16) | (1 << 7) | 0xf;

/// `texel × mul + add`, clamped, on the colour of every texel of a model: what the game does to
/// the palettes of a character (native 238, `ov_017::0218f968`, `0218fd34`): adds a colour
/// (mode 0, `mul` 1), fills with it (mode 1, `mul` 0) or multiplies by it (mode 2, `add` 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexelTint {
    pub mul: [f32; 3],
    pub add: [f32; 3],
}

/// What the DS lighting equation needs from a material (`DIF_AMB`, `SPE_EMI`, `POLYGON_ATTR`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialLight {
    pub shade: Shade,
    pub diffuse: [f32; 3],
    pub ambient: [f32; 3],
    pub specular: [f32; 3],
    pub emission: [f32; 3],
    /// `POLYGON_ATTR` bits 0-3: the lights that shine on it.
    pub lights: u8,
    /// `POLYGON_ATTR` bit 15: fog applies.
    pub fog: bool,
    /// `POLYGON_ATTR` bits 24-29.
    pub polygon_id: u8,
    /// Terrain model of an advanced zone: its ambient colour is the zone's
    /// (`LightingManager::ApplyAmbientColorToModel`, `0x02052348`).
    pub terrain: bool,
    /// An operation on the colours of the model's palettes (native 238), done on the texels.
    pub texel_tint: Option<TexelTint>,
}

impl Default for MaterialLight {
    fn default() -> Self {
        Self {
            shade: Shade::Unlit,
            diffuse: [1.0; 3],
            ambient: [0.0; 3],
            specular: [0.0; 3],
            emission: [0.0; 3],
            lights: 0,
            fog: false,
            polygon_id: 0,
            terrain: false,
            texel_tint: None,
        }
    }
}

impl MaterialLight {
    /// Colours and bits of a G3D material, for a map or character model.
    pub fn from_material(m: &Material, shade: Shade) -> Self {
        let attr =
            (m.polygon_attr & m.polygon_attr_mask) | (DEFAULT_POLYGON_ATTR & !m.polygon_attr_mask);
        Self {
            shade,
            diffuse: m.diffuse(),
            ambient: m.ambient(),
            specular: m.specular(),
            emission: m.emission(),
            lights: (attr & 0xf) as u8,
            fog: attr & (1 << 15) != 0,
            polygon_id: ((attr >> 24) & 0x3f) as u8,
            terrain: false,
            texel_tint: None,
        }
    }
}

/// Drawing state of one material.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneMaterial {
    pub name: String,
    /// G3D texture and palette names, for pattern animations to rebind later.
    pub texture_name: Option<String>,
    pub palette_name: Option<String>,
    /// Bound texture (index in [`Scene::textures`]).
    pub texture: Option<usize>,
    pub wrap: [Wrap; 2],
    /// Polygon alpha, 0 to 31 (0: wireframe on the DS, not drawn here).
    pub alpha: u8,
    pub cull: Cull,
    pub mode: PolygonMode,
    /// `POLYGON_ATTR` bit 14: depth test "equal" instead of "less" (decals).
    pub depth_equal: bool,
    /// `POLYGON_ATTR` bit 11: translucent pixels update the depth buffer.
    pub translucent_depth_write: bool,
    /// Affine transform of the normalised texture coordinate (rows of a 2×3 matrix): the
    /// material's own texture matrix, else identity; SRT animations replace it per frame.
    pub uv_transform: [[f32; 3]; 2],
    pub light: MaterialLight,
}

impl SceneMaterial {
    /// Whether it is drawn at all.
    pub fn visible(&self) -> bool {
        self.alpha != 0 && self.cull != Cull::All && self.mode != PolygonMode::Shadow
    }

    /// Whether it belongs to the translucent pass: polygon alpha below 31, or a texture with
    /// partial alpha.
    pub fn translucent(&self, textures: &[SceneTexture]) -> bool {
        self.alpha < 31 || self.texture.is_some_and(|t| textures[t].translucent)
    }
}

/// One mesh drawn with one material.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDraw {
    pub material: usize,
    /// Range of [`SceneModel::indices`].
    pub indices: Range<u32>,
    /// Model-space centroid of its vertices, to sort translucent draws.
    pub center: [f32; 3],
}

/// One G3D model, in its rest pose.
#[derive(Debug, Clone)]
pub struct SceneModel {
    /// `.amdj` archive and `.nsbmd` member it comes from.
    pub archive: String,
    pub member: String,
    pub name: String,
    pub vertices: Vec<SceneVertex>,
    pub indices: Vec<u32>,
    pub materials: Vec<SceneMaterial>,
    pub draws: Vec<SceneDraw>,
    /// Model-space bounding box of the vertices.
    pub bounds: Option<[Vec3; 2]>,
    /// Vertices with a normal are lit by the zone's lights at draw time (map models);
    /// otherwise their colours are baked.
    pub lit: bool,
    /// The zone's basic lighting may transform its vertex colours (the resource is not
    /// flagged `UNTINTED`).
    pub tinted: bool,
}

/// Vertices drawn under a billboard render command (`0x07`/`0x08`), kept in the frame of the
/// command so that [`Scene::face_billboards`] can turn them toward the camera.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneBillboard {
    /// `0x08`: the object keeps its up axis and turns around it; `0x07`: it faces the camera
    /// flat (`RenderCommand_7`, `RenderCommand_8`).
    pub y_only: bool,
    /// Model-space pivot: the translation of the matrix the vertices were drawn with.
    pub pivot: Vec3,
    /// Rotation of that matrix (columns: the object's axes, unit length).
    pub rotation: Mat3,
    /// Each billboarded vertex ([`SceneModel::vertices`] index) and its offset from the pivot
    /// along the object's own scaled axes: only the rotation of the matrix is replaced every
    /// frame, so this offset stays.
    pub vertices: Vec<(u32, Vec3)>,
}

impl SceneBillboard {
    /// Recomputes the offsets from the vertices as they stand, after an animation posed them.
    pub(crate) fn rebase(&mut self, vertices: &[SceneVertex]) {
        let inverse = self.rotation.transpose();
        for entry in &mut self.vertices {
            let Some(v) = vertices.get(entry.0 as usize) else {
                continue;
            };
            entry.1 = inverse * (Vec3::from_array(v.position) - self.pivot);
        }
    }

    /// The object's up axis in model space (the image of its local `y` axis).
    pub fn up(&self) -> Vec3 {
        self.rotation.col(1)
    }
}

impl SceneModel {
    /// Recomputes the model-space centroid of every draw from its posed vertices, to sort
    /// translucent draws.
    pub(crate) fn recenter_draws(&mut self) {
        for draw in &mut self.draws {
            let indices = &self.indices[draw.indices.start as usize..draw.indices.end as usize];
            let sum = indices.iter().fold(Vec3::ZERO, |s, &i| {
                s + Vec3::from_array(self.vertices[i as usize].position)
            });
            draw.center = (sum / indices.len().max(1) as f32).to_array();
        }
    }
}

/// A placed model.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneInstance {
    pub model: usize,
    /// Drawn world matrix; a null scale hides the instance (natives 570, 574).
    pub world: Mat4,
    /// Home world matrix of a map placement (`0x02013168`), kept while a null [`world`]
    /// hides it so that native 574 can show it again at the transform of the map. Generated
    /// instances (hero, actors, sprites) keep the world they were created with.
    pub home: Mat4,
    /// Block (index in [`Map::blocks`]) and instance in it.
    pub block: usize,
    pub instance: usize,
}

impl SceneInstance {
    /// A map placement: hidden at the chosen time of day it is still built, with a null scale,
    /// so that natives 574 and 575 can show or move it (`02019518` finds the record,
    /// `02163dec` and `02163e50` change it).
    fn placed(model: usize, home: Mat4, hidden: bool, block: usize, instance: usize) -> Self {
        Self {
            model,
            world: if hidden {
                Mat4::from_scale(Vec3::ZERO)
            } else {
                home
            },
            home,
            block,
            instance,
        }
    }

    /// Whether an image can show it: the null world scale is how the application hides an
    /// instance (`EventPlayer`, [`SceneInstance::world`]).
    pub fn drawn(&self) -> bool {
        self.world.x_axis.truncate().length_squared()
            + self.world.y_axis.truncate().length_squared()
            + self.world.z_axis.truncate().length_squared()
            > 0.0
    }
}

/// Everything to draw for one map.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub textures: Vec<SceneTexture>,
    pub models: Vec<SceneModel>,
    pub instances: Vec<SceneInstance>,
    /// Material textures that no `.nsbtx` of the map provides: nothing binds them, and the
    /// materials draw untextured **[hypothesis: as the DS draws a material whose texture the
    /// library left unbound]**.
    pub unbound_textures: Vec<String>,
    /// The scene instances of each map placement: `(block entry, placement id, instance)`,
    /// which is how scripts name them (natives 574, 575). A placement hidden at the chosen
    /// time of day has an instance too, drawn with a null scale.
    pub placements: Vec<(u32, u16, usize)>,
    /// The placements of the map with no instance in the scene: `(block entry, placement id)`
    /// of a collision part, which has no model to show.
    pub undrawn_placements: Vec<(u32, u16)>,
    /// Files that could not be read or decoded, skipped.
    pub warnings: Vec<String>,
    /// Billboards by [`SceneModel`] index: the vertices drawn under a billboard render command
    /// of a map model, re-oriented toward the camera every frame
    /// ([`Scene::face_billboards`]). A model with one is copied per placement, because the
    /// result depends on the placement's own world transform.
    pub billboards: BTreeMap<usize, Vec<SceneBillboard>>,
}

/// World matrix of an instance: translation, then the `y` rotation, then the scale, as the
/// placement walk computes them (`0x02013168`).
pub fn world_matrix(t: &Transform) -> Mat4 {
    let fx = |v: i32| v as f32 / 4096.0;
    Mat4::from_translation(Vec3::from_array(t.position.map(fx)))
        * Mat4::from_rotation_y(fx(i32::from(t.angle)))
        * Mat4::from_scale(Vec3::from_array(t.scale.map(fx)))
}

/// Member lookup shared with the animated scene loader.
pub(crate) fn member<'a>(narc: &Narc<'a>, name: &str) -> Option<&'a [u8]> {
    let entry = narc
        .names
        .files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case(name))?;
    narc.file(entry.id).ok()
}

/// A G3D member as the game reads it: map archive members are LZ10 streams, decoded whatever
/// the type byte (`DecompressLZ77FileIntoAllocatedSpace`, `0x02075664`); an uncompressed G3D
/// file is taken as is.
pub(crate) fn unpack(data: &[u8]) -> Result<Cow<'_, [u8]>> {
    if G3dFile::is_g3d(data) {
        return Ok(Cow::Borrowed(data));
    }
    let header = data
        .get(..4)
        .ok_or(Error::Malformed("compressed member header"))?;
    let size = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) >> 8;
    Ok(Cow::Owned(decode(Codec::Lz10, &data[4..], size as usize)?))
}

fn name_string(n: &Name) -> String {
    n.to_string()
}

/// The texture sections of the map's `.nsbtx` files, in [`Map::textures`] order.
pub(crate) struct TextureSources<'a> {
    pub(crate) tex: Vec<Option<Tex0<'a>>>,
}

impl<'a> TextureSources<'a> {
    /// The first `.nsbtx` with texture `name`, and the palette for it: `palette` in that
    /// order too, else `<name>_pl`, else the only palette of that file.
    fn find(
        &self,
        name: &Name,
        palette: Option<&Name>,
    ) -> Option<(usize, &Texture<'a>, Option<&Palette<'a>>)> {
        let (src, texture) = self
            .tex
            .iter()
            .enumerate()
            .find_map(|(i, t)| Some((i, t.as_ref()?.texture(name)?)))?;
        if !texture.params.format().needs_palette() {
            return Some((src, texture, None));
        }
        let named = palette.and_then(|p| self.tex.iter().find_map(|t| t.as_ref()?.palette(p)));
        let fallback = || {
            let tex0 = self.tex[src].as_ref()?;
            let mut pl = name.bytes().to_vec();
            pl.extend(b"_pl");
            tex0.palettes
                .iter()
                .find(|p| p.name.bytes() == pl)
                .or(match tex0.palettes.as_slice() {
                    [only] => Some(only),
                    _ => None,
                })
        };
        Some((src, texture, named.or_else(fallback)))
    }
}

type ModelCacheKey<'a> = (
    &'a str,
    &'a str,
    &'a [dq9_formats::map::Animation],
    Option<usize>,
    bool,
);

/// Builder state: the scene so far and its decoded textures by material texture and palette
/// names (`None`: not found or not decodable).
pub(crate) struct Builder<'s, 'a> {
    pub(crate) sources: &'s TextureSources<'a>,
    pub(crate) scene: Scene,
    pub(crate) texture_ids: HashMap<(Name, Option<Name>), Option<usize>>,
    /// Build map models: normals stay in the vertices for the zone's lights.
    pub(crate) lit: bool,
}

impl Builder<'_, '_> {
    fn texture(&mut self, m: &Material) -> Option<usize> {
        let name = m.texture_name?;
        let key = (name, m.palette_name);
        if let Some(&id) = self.texture_ids.get(&key) {
            return id;
        }
        let id = self.decode(&name, m.palette_name.as_ref());
        self.texture_ids.insert(key, id);
        id
    }

    pub(crate) fn decode(&mut self, name: &Name, palette: Option<&Name>) -> Option<usize> {
        let Some((source, texture, pal)) = self.sources.find(name, palette) else {
            self.scene.unbound_textures.push(name_string(name));
            return None;
        };
        let rgba = match decode_texture(texture, pal) {
            Ok(rgba) => rgba,
            Err(e) => {
                self.scene
                    .warnings
                    .push(format!("texture {name}: not decoded ({e})"));
                return None;
            }
        };
        let translucent = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] != 0 && p[3] != 255);
        self.scene.textures.push(SceneTexture {
            name: name_string(name),
            palette: pal.map(|p| name_string(&p.name)),
            source,
            width: texture.params.width(),
            height: texture.params.height(),
            rgba,
            translucent,
        });
        Some(self.scene.textures.len() - 1)
    }

    fn material(&mut self, m: &Material, tex_matrix: Option<&TexMatrix>) -> SceneMaterial {
        let attr = m.polygon_attr;
        let params = m.params();
        let wrap = |repeat: bool, mirror: bool| match (repeat, mirror) {
            (false, _) => Wrap::Clamp,
            (true, false) => Wrap::Repeat,
            (true, true) => Wrap::Mirror,
        };
        SceneMaterial {
            name: name_string(&m.name),
            texture_name: m.texture_name.as_ref().map(name_string),
            palette_name: m.palette_name.as_ref().map(name_string),
            texture: self.texture(m),
            wrap: [
                wrap(params.repeat_s(), params.mirror_s()),
                wrap(params.repeat_t(), params.mirror_t()),
            ],
            alpha: m.alpha(),
            cull: match (m.cull_back(), m.cull_front()) {
                (false, false) => Cull::None,
                (true, false) => Cull::Back,
                (false, true) => Cull::Front,
                (true, true) => Cull::All,
            },
            mode: match (attr >> 4) & 3 {
                0 => PolygonMode::Modulate,
                1 => PolygonMode::Decal,
                2 => PolygonMode::Toon,
                _ => PolygonMode::Shadow,
            },
            depth_equal: attr & (1 << 14) != 0,
            translucent_depth_write: attr & (1 << 11) != 0,
            // The material's texture matrix (MaterialBindProc), on normalised coordinates.
            uv_transform: tex_matrix.map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], |t| {
                t.normalized(f32::from(m.width.max(1)), f32::from(m.height.max(1)))
            }),
            light: MaterialLight::from_material(
                m,
                if self.lit {
                    Shade::Map
                } else {
                    Shade::Character
                },
            ),
        }
    }

    pub(crate) fn model(
        &mut self,
        archive: &str,
        member: &str,
        model: &Model,
    ) -> Result<SceneModel> {
        Ok(self.model_with_billboards(archive, member, model)?.0)
    }

    /// [`model`](Self::model), also returning the billboards of the vertices it draws. The
    /// caller stores them with the model index it chooses (the scene's own loader does; a
    /// character or a hero part has none and can drop them).
    pub(crate) fn model_with_billboards(
        &mut self,
        archive: &str,
        member: &str,
        model: &Model,
    ) -> Result<(SceneModel, Vec<SceneBillboard>)> {
        let geometry = Geometry::build(model)?;
        let matrices = model.tex_matrices();
        let materials: Vec<SceneMaterial> = model
            .materials
            .iter()
            .zip(&matrices)
            .map(|(m, t)| self.material(m, t.as_ref()))
            .collect();
        let mut vertices: Vec<SceneVertex> = geometry
            .vertices
            .iter()
            .map(|v| SceneVertex {
                position: v.position,
                texcoord: v.texcoord,
                color: v.color,
                ..SceneVertex::default()
            })
            .collect();
        let mut draws = Vec::with_capacity(geometry.draw_calls.len());
        for call in &geometry.draw_calls {
            let material = usize::from(call.material_idx);
            if material >= materials.len() {
                return Err(Error::Malformed("G3D draw call material out of range"));
            }
            let tint = model
                .meshes
                .get(usize::from(call.mesh_idx))
                .is_some_and(|mesh| mesh.flags & 2 != 0);
            let m = &model.materials[material];
            for (out, v) in vertices[call.vertex_range.clone()]
                .iter_mut()
                .zip(&geometry.vertices[call.vertex_range.clone()])
            {
                if v.normal == [0.0; 3] {
                    out.tint = f32::from(u8::from(self.lit && tint));
                } else if self.lit {
                    // Lit at draw time by the zone's lights (DS lighting equation).
                    out.normal = v.normal;
                    out.color = [1.0; 3];
                } else {
                    // Characters: a fixed light stands in for the DS lights.
                    out.color = fixed_light(m, v.normal);
                }
            }
            let range = &vertices[call.vertex_range.clone()];
            let sum = range
                .iter()
                .fold(Vec3::ZERO, |s, v| s + Vec3::from_array(v.position));
            let center = sum / range.len().max(1) as f32;
            let to_u32 = |i: usize| u32::try_from(i).map_err(|_| Error::Malformed("index"));
            draws.push(SceneDraw {
                material,
                indices: to_u32(call.index_range.start)?..to_u32(call.index_range.end)?,
                center: center.to_array(),
            });
        }
        let bounds = vertices.iter().fold(None, |b: Option<[Vec3; 2]>, v| {
            let p = Vec3::from_array(v.position);
            Some(b.map_or([p, p], |[lo, hi]| [lo.min(p), hi.max(p)]))
        });
        let mut billboards = Vec::with_capacity(geometry.billboards.len());
        for b in &geometry.billboards {
            let pivot = Vec3::from_array(b.pivot);
            let rotation = Mat3::from_cols_array(&b.rotation);
            let inverse = rotation.transpose();
            let ids = b
                .vertex_range
                .clone()
                .map(|i| u32::try_from(i).map_err(|_| Error::Malformed("index")))
                .collect::<Result<Vec<_>>>()?;
            let vertices: Vec<(u32, Vec3)> = ids
                .iter()
                .map(|&i| {
                    let p = Vec3::from_array(vertices[i as usize].position);
                    (i, inverse * (p - pivot))
                })
                .collect();
            billboards.push(SceneBillboard {
                y_only: b.y_only,
                pivot,
                rotation,
                vertices,
            });
        }
        Ok((
            SceneModel {
                archive: archive.to_owned(),
                member: member.to_owned(),
                name: name_string(&model.name),
                vertices,
                indices: geometry.indices,
                materials,
                draws,
                bounds,
                lit: self.lit,
                tinted: true,
            },
            billboards,
        ))
    }
}

/// Stand-in for the DS lighting of a vertex with a normal: one white light from above-front,
/// `emission + ambient / 2 + diffuse · (1 + n·l) / 2`.
pub(crate) fn fixed_light(m: &Material, normal: [f32; 3]) -> [f32; 3] {
    let light = Vec3::new(0.3, 1.0, 0.5).normalize();
    let lambert = 0.5 * (1.0 + Vec3::from_array(normal).dot(light));
    let (d, a, e) = (m.diffuse(), m.ambient(), m.emission());
    std::array::from_fn(|i| (e[i] + 0.5 * a[i] + d[i] * lambert).min(1.0))
}

impl Scene {
    /// Builds the scene of `map`; `file` returns a ROM file by path (compared without case,
    /// [`dq9_formats::map::rom_file`]). Unreadable files are skipped into
    /// [`warnings`](Self::warnings). Collision parts are left out; a placement hidden at the
    /// chosen time of day gets an instance with a null scale and its home matrix.
    pub fn build<'r>(map: &Map, file: impl Fn(&str) -> Option<&'r [u8]>) -> Scene {
        let mut warnings = Vec::new();
        // Zone3D_StepAmbl (0x02014118): every .nsbtx of the .ambl, newest first.
        let ambl = file(&map.files.ambl).and_then(|a| Narc::parse(a).ok());
        let texture_bytes: Vec<Option<Cow<[u8]>>> = map
            .textures
            .iter()
            .map(|name| {
                let raw = ambl.as_ref().and_then(|n| member(n, name));
                match raw.map(unpack) {
                    Some(Ok(bytes)) => Some(bytes),
                    Some(Err(e)) => {
                        warnings.push(format!("{name}: not decompressed ({e})"));
                        None
                    }
                    None => {
                        warnings.push(format!("{name}: missing"));
                        None
                    }
                }
            })
            .collect();
        let sources = TextureSources {
            tex: texture_bytes
                .iter()
                .zip(&map.textures)
                .map(|(bytes, name)| match G3dFile::parse(bytes.as_deref()?) {
                    Ok(f) => f.tex,
                    Err(e) => {
                        warnings.push(format!("{name}: not parsed ({e})"));
                        None
                    }
                })
                .collect(),
        };
        let mut b = Builder {
            sources: &sources,
            scene: Scene {
                warnings,
                ..Scene::default()
            },
            texture_ids: HashMap::new(),
            lit: true,
        };
        let mut archives: HashMap<&str, Option<Narc>> = HashMap::new();
        let mut loaded: HashMap<ModelCacheKey<'_>, Vec<usize>> = HashMap::new();
        // Billboard models already placed: the others need a copy of their own.
        let mut shared: HashSet<usize> = HashSet::new();
        for (block_idx, block) in map.blocks.iter().enumerate() {
            if !block.processed {
                continue;
            }
            let narc = archives
                .entry(&block.archive)
                .or_insert_with(|| file(&block.archive).and_then(|a| Narc::parse(a).ok()));
            let Some(narc) = narc.as_ref() else {
                b.scene.warnings.push(format!("{}: missing", block.archive));
                continue;
            };
            for (inst_idx, inst) in block.instances.iter().enumerate() {
                let Some(part) = inst.part.and_then(|p| block.parts.get(p)) else {
                    continue;
                };
                // A collision part changes the floor and the walls, not the image; a model or
                // an object hidden at this time of day is still built, because natives 574
                // and 575 can show or move it (`02019518`, `02163dec`, `02163e50`).
                if !matches!(part.kind, PartKind::Model | PartKind::Object) {
                    if part.kind == PartKind::Collision {
                        b.scene.undrawn_placements.push((block.entry, inst.id));
                    }
                    continue;
                }
                let Some(name) = &part.file else { continue };
                let models = loaded
                    .entry((
                        &block.archive,
                        name,
                        &part.animations,
                        part.texture_source,
                        part.untinted,
                    ))
                    .or_insert_with(|| {
                        let ids = load_models(&mut b, &block.archive, narc, name);
                        for &id in &ids {
                            let model = &mut b.scene.models[id];
                            if part.untinted {
                                model.tinted = false;
                                model.vertices.iter_mut().for_each(|v| v.tint = 0.0);
                            }
                            if part.kind == PartKind::Model {
                                for m in &mut model.materials {
                                    m.light.terrain = true;
                                }
                            }
                        }
                        ids
                    });
                let home = world_matrix(&inst.world);
                for &model in models.iter() {
                    // A billboard re-orients its vertices from the render camera and from the
                    // instance's own world transform, so two placements cannot share one
                    // vertex buffer: each instance after the first gets its own copy.
                    let model = if !b.scene.billboards.contains_key(&model) || shared.insert(model)
                    {
                        model
                    } else {
                        let copy = b.scene.models[model].clone();
                        b.scene.models.push(copy);
                        let index = b.scene.models.len() - 1;
                        b.scene
                            .billboards
                            .insert(index, b.scene.billboards[&model].clone());
                        index
                    };
                    let instance = b.scene.instances.len();
                    b.scene.placements.push((block.entry, inst.id, instance));
                    b.scene.instances.push(SceneInstance::placed(
                        model,
                        home,
                        inst.hidden,
                        block_idx,
                        inst_idx,
                    ));
                }
            }
        }
        b.scene
    }

    /// Re-orients the vertices drawn under a billboard render command toward `camera` and
    /// returns the number of instances it updated. The original replaces the rotation of the
    /// current matrix in camera space (`RenderCommand_7`, `RenderCommand_8`): the pivot and the
    /// scale stay, the rotation becomes the camera's, so `0x08` keeps the object's up axis and
    /// turns it around that axis while `0x07` faces the camera flat.
    pub fn face_billboards(&mut self, camera: &Camera) -> usize {
        // The camera's rotation and its inverse; the view matrix carries no scale.
        let view = Mat3::from_mat4(camera.view());
        let back = view.transpose();
        let work: Vec<(usize, usize)> = self
            .instances
            .iter()
            .enumerate()
            .filter(|(_, inst)| inst.drawn() && self.billboards.contains_key(&inst.model))
            .map(|(index, inst)| (index, inst.model))
            .collect();
        for &(index, model_index) in &work {
            let world = self.instances[index].world;
            let inverse = world.inverse();
            let (rotation, scale) = columns(&world);
            let (Some(billboards), Some(model)) = (
                self.billboards.get(&model_index),
                self.models.get_mut(model_index),
            ) else {
                continue;
            };
            for b in billboards {
                let pivot = world.transform_point3(b.pivot);
                let facing = if b.y_only {
                    let up = (rotation * b.up()).normalize_or_zero();
                    let up = (view * up).normalize_or_zero();
                    // The object's up axis stays; its width becomes the screen's, and the
                    // third axis is the normal of that plane.
                    let side = Vec3::new(0.0, -up.z, up.y);
                    back * Mat3::from_cols(Vec3::X, up, side)
                } else {
                    back
                };
                let matrix = inverse
                    * Mat4::from_translation(pivot)
                    * Mat4::from_mat3(facing)
                    * Mat4::from_scale(scale);
                for &(i, offset) in &b.vertices {
                    if let Some(v) = model.vertices.get_mut(i as usize) {
                        v.position = matrix.transform_point3(offset).to_array();
                    }
                }
            }
        }
        work.len()
    }

    /// World-space bounding box of every instance drawn (hidden ones have a null scale).
    pub fn bounds(&self) -> Option<[Vec3; 2]> {
        let mut out: Option<[Vec3; 2]> = None;
        for inst in self.instances.iter().filter(|i| i.drawn()) {
            let Some([lo, hi]) = self.models[inst.model].bounds else {
                continue;
            };
            for i in 0..8 {
                let corner = Vec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                let p = inst.world.transform_point3(corner);
                out = Some(out.map_or([p, p], |[a, b]| [a.min(p), b.max(p)]));
            }
        }
        out
    }

    /// Number of triangles drawn by all instances.
    pub fn triangle_count(&self) -> usize {
        self.instances
            .iter()
            .filter(|i| i.drawn())
            .map(|i| self.models[i.model].indices.len() / 3)
            .sum()
    }
}

/// The rotation and the scale of a world matrix: its columns normalized, and their lengths.
fn columns(world: &Mat4) -> (Mat3, Vec3) {
    let mut lengths = Vec3::ONE;
    let mut unit = [Vec3::X, Vec3::Y, Vec3::Z];
    for (axis, col) in [world.x_axis, world.y_axis, world.z_axis]
        .iter()
        .enumerate()
    {
        let len = col.truncate().length();
        if len > 0.0 {
            lengths[axis] = len;
            unit[axis] = col.truncate() / len;
        }
    }
    (Mat3::from_cols(unit[0], unit[1], unit[2]), lengths)
}

/// Every model of an `.nsbmd` member, added to the scene; nothing on failure (warned).
fn load_models(b: &mut Builder<'_, '_>, archive: &str, narc: &Narc<'_>, name: &str) -> Vec<usize> {
    let result = member(narc, name)
        .ok_or(Error::Malformed("member missing"))
        .and_then(unpack)
        .and_then(|bytes| {
            let file = G3dFile::parse(&bytes)?;
            let mut ids = Vec::new();
            for model in &file.models {
                let (m, billboards) = b.model_with_billboards(archive, name, model)?;
                let id = b.scene.models.len();
                b.scene.models.push(m);
                if !billboards.is_empty() {
                    b.scene.billboards.insert(id, billboards);
                }
                ids.push(id);
            }
            Ok(ids)
        });
    result.unwrap_or_else(|e| {
        b.scene.warnings.push(format!("{archive}:{name}: {e}"));
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn world_matrix_translates_rotates_scales() {
        let t = Transform {
            position: [4096, 2 * 4096, -4096],
            // 2π / 4 in fx32.
            angle: 0x1922,
            scale: [2 * 4096; 3],
        };
        let m = world_matrix(&t);
        let p = m.transform_point3(Vec3::X);
        // x scaled to 2, turned a quarter around y (to -z), then moved.
        let expected = Vec3::new(1.0, 2.0, -1.0)
            + Mat4::from_rotation_y(FRAC_PI_2).transform_vector3(Vec3::new(2.0, 0.0, 0.0));
        assert!((p - expected).length() < 1e-3, "{p} vs {expected}");
    }

    #[test]
    fn unpack_lz10_member() {
        // LZ10 header (size 4) and one flag byte of literals.
        let data = [0x10, 4, 0, 0, 0x00, b'B', b'M', b'D', b'0'];
        assert_eq!(&*unpack(&data).unwrap(), b"BMD0");
        assert!(unpack(&[0x10]).is_err());
    }

    #[test]
    fn bounds_and_triangle_count_ignore_hidden_instances() {
        let mut scene = Scene::default();
        scene.models.push(SceneModel {
            archive: String::new(),
            member: String::new(),
            name: String::new(),
            vertices: Vec::new(),
            indices: vec![0, 1, 2],
            materials: Vec::new(),
            draws: Vec::new(),
            bounds: Some([Vec3::splat(-1.0), Vec3::splat(1.0)]),
            lit: false,
            tinted: false,
        });
        scene.instances.push(SceneInstance {
            model: 0,
            world: Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)),
            home: Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)),
            block: 0,
            instance: 0,
        });
        scene.instances.push(SceneInstance {
            model: 0,
            world: Mat4::from_scale(Vec3::ZERO),
            home: Mat4::from_translation(Vec3::new(-100.0, 0.0, 0.0)),
            block: 0,
            instance: 1,
        });
        let [lo, hi] = scene.bounds().unwrap();
        assert_eq!(lo, Vec3::new(9.0, -1.0, -1.0));
        assert_eq!(hi, Vec3::new(11.0, 1.0, 1.0));
        assert_eq!(scene.triangle_count(), 1);
    }

    /// A map model drawn with one billboard command: a vertical quad whose offsets are what
    /// `Builder::model` keeps (the object's scaled axes), placed with a rotation and a scale.
    fn billboard_scene(y_only: bool, world: Mat4) -> Scene {
        let mut scene = Scene::default();
        let offsets = [
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ];
        scene.models.push(SceneModel {
            archive: String::new(),
            member: String::new(),
            name: String::new(),
            vertices: offsets
                .iter()
                .map(|&position| SceneVertex {
                    position,
                    ..SceneVertex::default()
                })
                .collect(),
            indices: vec![0, 1, 2, 0, 2, 3],
            materials: Vec::new(),
            draws: Vec::new(),
            bounds: None,
            lit: false,
            tinted: false,
        });
        scene.instances.push(SceneInstance {
            model: 0,
            world,
            home: world,
            block: 0,
            instance: 0,
        });
        scene.billboards.insert(
            0,
            vec![SceneBillboard {
                y_only,
                pivot: Vec3::ZERO,
                rotation: Mat3::IDENTITY,
                vertices: offsets
                    .iter()
                    .enumerate()
                    .map(|(i, &o)| (i as u32, Vec3::from_array(o)))
                    .collect(),
            }],
        );
        scene
    }

    /// The world-space normal of the quad, from the model-space vertices it drew.
    fn quad_normal(scene: &Scene) -> (Vec3, Vec3) {
        let world = scene.instances[0].world;
        let p = |i: usize| {
            world.transform_point3(Vec3::from_array(scene.models[0].vertices[i].position))
        };
        let (a, b, d) = (p(0), p(1), p(3));
        // `d - a` is the quad's up axis; the cross of the two edges is its normal.
        ((b - a).cross(d - a).normalize(), (d - a).normalize())
    }

    /// The game replaces the rotation of the current matrix in camera space
    /// (`RenderCommand_7`/`_8`): whatever the placement's own rotation, the quad faces the
    /// camera from every direction, and `0x08` keeps it upright.
    #[test]
    fn billboards_face_the_camera_from_every_direction() {
        let world = Mat4::from_translation(Vec3::new(3.0, 1.0, -2.0))
            * Mat4::from_rotation_y(0.7)
            * Mat4::from_scale(Vec3::splat(2.0));
        let pivot = world.transform_point3(Vec3::ZERO);
        for y_only in [false, true] {
            let mut scene = billboard_scene(y_only, world);
            for step in 0..4 {
                let mut camera = Camera {
                    yaw: step as f32 * std::f32::consts::FRAC_PI_2 + 0.3,
                    pitch: 0.6,
                    distance: 10.0,
                    ..Camera::default()
                };
                camera.target = pivot;
                assert_eq!(scene.face_billboards(&camera), 1);
                let (normal, up) = quad_normal(&scene);
                let to_eye = camera.eye() - pivot;
                let view = Mat3::from_mat4(camera.view());
                // `0x08` keeps the object's up axis (the placement's is the world's);
                // `0x07` is screen-aligned, so its up is the camera's.
                let wanted_up = if y_only {
                    Vec3::Y
                } else {
                    view.transpose() * Vec3::Y
                };
                assert!(
                    up.dot(wanted_up) > 0.999,
                    "y_only {y_only}: up {up} vs {wanted_up}"
                );
                // `0x08` turns about the vertical: the plane faces the camera's horizontal
                // direction. `0x07` faces the eye exactly, pitch included.
                let target = if y_only {
                    to_eye.with_y(0.0).normalize()
                } else {
                    to_eye.normalize()
                };
                assert!(
                    normal.dot(target) > 0.999,
                    "y_only {y_only}, yaw {step}: normal {normal} vs {target}"
                );
            }
        }
    }

    #[test]
    fn translucency_and_visibility() {
        let mut m = SceneMaterial {
            name: String::new(),
            texture_name: None,
            palette_name: None,
            texture: None,
            wrap: [Wrap::Clamp; 2],
            alpha: 31,
            cull: Cull::Back,
            mode: PolygonMode::Modulate,
            depth_equal: false,
            translucent_depth_write: false,
            uv_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            light: MaterialLight::default(),
        };
        let textures = [SceneTexture {
            name: String::new(),
            palette: None,
            source: 0,
            width: 8,
            height: 8,
            rgba: vec![0; 256],
            translucent: true,
        }];
        assert!(m.visible() && !m.translucent(&textures));
        m.texture = Some(0);
        assert!(m.translucent(&textures));
        m.texture = None;
        m.alpha = 16;
        assert!(m.translucent(&textures));
        m.alpha = 0;
        assert!(!m.visible());
    }
}
