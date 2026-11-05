//! CPU side of the renderer: everything the GPU needs for one assembled map, without touching
//! the GPU. [`Scene::build`] decodes each shown part's `.nsbmd` (`nds_rom::g3d`), binds its
//! materials to the textures of the map's `.nsbtx` files, runs the display lists into indexed
//! triangles (`Geometry::build`, rest pose) and places one instance per visible placement with
//! its world transform.
//!
//! Approximations of the DS 3D engine are listed in `docs/app.md`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use dq9_formats::map::{Map, PartKind, Transform};
use glam::{Mat4, Vec3};
use nds_rom::compress::{Codec, decode};
use nds_rom::g3d::tex::decode_texture;
use nds_rom::g3d::{G3dFile, Geometry, Material, Model, Name, Palette, Tex0, TexMatrix, Texture};
use nds_rom::{Error, Narc, Result};

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

/// A placed model.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneInstance {
    pub model: usize,
    pub world: Mat4,
    /// Block (index in [`Map::blocks`]) and instance in it.
    pub block: usize,
    pub instance: usize,
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
    /// Files that could not be read or decoded, skipped.
    pub warnings: Vec<String>,
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
        Ok(SceneModel {
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
        })
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
    /// [`warnings`](Self::warnings). Hidden instances and collision parts are left out.
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
                if inst.hidden || !matches!(part.kind, PartKind::Model | PartKind::Object) {
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
                let world = world_matrix(&inst.world);
                for &model in models.iter() {
                    b.scene.instances.push(SceneInstance {
                        model,
                        world,
                        block: block_idx,
                        instance: inst_idx,
                    });
                }
            }
        }
        b.scene
    }

    /// World-space bounding box of every instance.
    pub fn bounds(&self) -> Option<[Vec3; 2]> {
        let mut out: Option<[Vec3; 2]> = None;
        for inst in &self.instances {
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
            .map(|i| self.models[i.model].indices.len() / 3)
            .sum()
    }
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
                let m = b.model(archive, name, model)?;
                b.scene.models.push(m);
                ids.push(b.scene.models.len() - 1);
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
