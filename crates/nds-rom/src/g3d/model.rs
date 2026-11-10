//! `MDL0` models: header, materials, meshes (display lists), objects (nodes) and the render
//! command bytecode. Ported from apicula `nitro::model`, `nitro::render_cmds` and
//! `nitro::rotation`; field names follow the NITRO-System resource structures where known.

use super::fx::{Fx12, Mat4, bits};
use super::tex::TextureParams;
use super::{Name, at, read_dict, read_offset_dict};
use crate::bytes::{slice, u8_at, u16_at, u32_at};
use crate::{Error, Result};

/// One model of an `MDL0` section.
#[derive(Debug, Clone)]
pub struct Model<'a> {
    pub name: Name,
    /// Render command bytecode type (`sbcType`).
    pub sbc_type: u8,
    /// Scaling rule (0 standard, 1 Maya SSC, 2 Softimage).
    pub scaling_rule: u8,
    /// Texture matrix mode (0 Maya, 1 Softimage, 2 3ds Max, 3 XSI).
    pub tex_mtx_mode: u8,
    /// First matrix stack slot not used by the render commands.
    pub first_unused_mtx_stack_id: u8,
    /// Factor applied to vertex positions by [`RenderOp::ScaleUp`] (`posScale`).
    pub up_scale: Fx12,
    /// Inverse of [`up_scale`](Self::up_scale) (`invPosScale`).
    pub down_scale: Fx12,
    pub num_vertices: u16,
    pub num_polygons: u16,
    pub num_triangles: u16,
    pub num_quads: u16,
    /// Bounding box origin `(x, y, z)` and size `(w, h, d)`, to multiply by
    /// [`box_pos_scale`](Self::box_pos_scale).
    pub bbox_origin: [Fx12; 3],
    pub bbox_size: [Fx12; 3],
    pub box_pos_scale: Fx12,
    pub box_inv_pos_scale: Fx12,
    pub materials: Vec<Material>,
    pub meshes: Vec<Mesh<'a>>,
    pub objects: Vec<Object>,
    /// Inverse bind matrices (4x3, DS layout) for skinning, as many as could be read up to the
    /// object count.
    pub inv_binds: Vec<[Fx12; 12]>,
    pub render_ops: Vec<RenderOp>,
    /// Every "multiply by object" render command (`0x06`), in bytecode order: the object
    /// hierarchy the Maya and Softimage scaling rules walk.
    pub object_links: Vec<ObjectLink>,
    /// Every billboard command (`0x07`/`0x08`), in bytecode order. The commands carry no
    /// object: they replace the rotation of the current matrix by a camera-facing one, so the
    /// draws that follow them are billboarded. Kept beside [`render_ops`](Self::render_ops)
    /// rather than in it so that existing op order and indices stay as they are.
    pub billboards: Vec<BillboardCommand>,
}

/// One billboard render command: `0x07` (`BB`, the object faces the camera flat) or `0x08`
/// (`BBY`, the object keeps its up axis and turns around it).
///
/// NITRO SDK: `RenderCommand_7` / `RenderCommand_8` (`src/NitroSDK/NSBXX/RenderCommands.cpp`,
/// USA addresses in the note `docs/re/g3d.md`). The opcode's low five bits select the
/// operation and its top three bits its modifier; the cartridge only holds the plain form
/// (modifier 0), which consumes one parameter byte and ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillboardCommand {
    /// Index in [`Model::render_ops`] before which the command sits.
    pub op: usize,
    /// The command's parameter byte.
    pub parameter: u8,
    /// `0x08`: billboard about the vertical axis only.
    pub y_only: bool,
}

/// One render command `0x06`: multiply the current matrix by `object_idx`, child of
/// `parent_idx` (itself for a root); bit 0 of `flags` makes the object compensate its parent's
/// scale and bit 1 records its own scale for its children (Maya SSC only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectLink {
    pub object_idx: u8,
    pub parent_idx: u8,
    pub flags: u8,
}

pub(crate) fn read_model<'a>(data: &'a [u8], base: usize, name: Name) -> Result<Model<'a>> {
    let u16f =
        |o: usize| -> Result<Fx12> { Ok(Fx12::from_fx16(u16_at(data, at(base, o as u64)?)?)) };
    let u32f =
        |o: usize| -> Result<Fx12> { Ok(Fx12::from_fx32(u32_at(data, at(base, o as u64)?)?)) };
    let u8h = |o: usize| -> Result<u8> { u8_at(data, at(base, o as u64)?) };
    let u16h = |o: usize| -> Result<u16> { u16_at(data, at(base, o as u64)?) };
    let u32h = |o: usize| -> Result<u32> { u32_at(data, at(base, o as u64)?) };

    let render_cmds_off = u32h(4)?;
    let materials_off = u32h(8)?;
    let meshes_off = u32h(12)?;
    let inv_binds_off = u32h(16)?;
    let num_objects = u8h(23)?;

    let (render_ops, object_links, billboards) =
        read_render_cmds(data, at(base, render_cmds_off)?)?;
    let meshes = read_meshes(data, at(base, meshes_off)?)?;
    let materials = read_materials(data, at(base, materials_off)?)?;
    let objects = read_objects(data, at(base, 64u8)?)?;
    let inv_binds = read_inv_binds(data, at(base, inv_binds_off)?, usize::from(num_objects));

    let model = Model {
        name,
        sbc_type: u8h(20)?,
        scaling_rule: u8h(21)?,
        tex_mtx_mode: u8h(22)?,
        first_unused_mtx_stack_id: u8h(26)?,
        up_scale: u32f(28)?,
        down_scale: u32f(32)?,
        num_vertices: u16h(36)?,
        num_polygons: u16h(38)?,
        num_triangles: u16h(40)?,
        num_quads: u16h(42)?,
        bbox_origin: [u16f(44)?, u16f(46)?, u16f(48)?],
        bbox_size: [u16f(50)?, u16f(52)?, u16f(54)?],
        box_pos_scale: u32f(56)?,
        box_inv_pos_scale: u32f(60)?,
        materials,
        meshes,
        objects,
        inv_binds,
        render_ops,
        object_links,
        billboards,
    };
    validate_render_ops(&model)?;
    Ok(model)
}

/// Checks that every index in the render ops is in bounds.
fn validate_render_ops(model: &Model) -> Result<()> {
    for op in &model.render_ops {
        let good = match op {
            RenderOp::MulObject { object_idx } => usize::from(*object_idx) < model.objects.len(),
            RenderOp::BindMaterial { material_idx } => {
                usize::from(*material_idx) < model.materials.len()
            }
            RenderOp::Draw { mesh_idx } => usize::from(*mesh_idx) < model.meshes.len(),
            RenderOp::Skin { terms } => terms
                .iter()
                .all(|t| usize::from(t.inv_bind_idx) < model.inv_binds.len()),
            _ => true,
        };
        if !good {
            return Err(Error::Malformed("G3D render command index out of bounds"));
        }
    }
    if model
        .object_links
        .iter()
        .any(|l| usize::from(l.parent_idx) >= model.objects.len())
    {
        return Err(Error::Malformed("G3D object parent index out of bounds"));
    }
    if model
        .billboards
        .iter()
        .any(|b| b.op > model.render_ops.len())
    {
        return Err(Error::Malformed("G3D billboard op out of bounds"));
    }
    Ok(())
}

/// A mesh ("shape", apicula "piece"): a display list of GPU commands.
#[derive(Debug, Clone)]
pub struct Mesh<'a> {
    pub name: Name,
    /// Shape flags (`NNSG3dResShp.flag`: uses normals/colours/texcoords/restore...).
    pub flags: u32,
    /// Display list, packed GPU commands; decode with [`DisplayList`](super::DisplayList).
    pub commands: &'a [u8],
}

fn read_meshes(data: &[u8], base: usize) -> Result<Vec<Mesh<'_>>> {
    read_offset_dict(data, base)?
        .into_iter()
        .map(|(off, name)| {
            let mesh = at(base, off)?;
            if u16_at(data, at(mesh, 2u8)?)? != 16 {
                return Err(Error::Malformed("G3D mesh header size"));
            }
            let flags = u32_at(data, at(mesh, 4u8)?)?;
            let cmds_off = u32_at(data, at(mesh, 8u8)?)?;
            let cmds_len = u32_at(data, at(mesh, 12u8)?)?;
            if cmds_len % 4 != 0 {
                return Err(Error::Malformed("G3D display list length"));
            }
            let commands = slice(data, at(mesh, cmds_off)?, cmds_len as usize)?;
            Ok(Mesh {
                name,
                flags,
                commands,
            })
        })
        .collect()
}

/// Material: drawing state bound before meshes (colours, polygon attributes, texture).
#[derive(Debug, Clone)]
pub struct Material {
    pub name: Name,
    /// Texture and palette names, resolved through the material pairing dictionaries.
    pub texture_name: Option<Name>,
    pub palette_name: Option<Name>,
    /// GPU `DIF_AMB` word.
    pub dif_amb: u32,
    /// GPU `SPE_EMI` word.
    pub spe_emi: u32,
    /// GPU `POLYGON_ATTR` word and the mask of its bits the material sets.
    pub polygon_attr: u32,
    pub polygon_attr_mask: u32,
    /// GPU `TEXIMAGE_PARAM` bits set by the material (repeat, flip, texgen) and their mask;
    /// format, size and address come from the bound texture.
    pub tex_image_param: u32,
    pub tex_image_param_mask: u32,
    pub palette_base: u16,
    /// `NNSG3dResMatData.flag` (texture matrix presence bits 0..=3, ...).
    pub flags: u16,
    /// Texture size the material was authored for, used to normalise texture coordinates.
    pub width: u16,
    pub height: u16,
    pub mag_w: Fx12,
    pub mag_h: Fx12,
    /// Texture matrix components, present when flag bit 0 is set: scale (absent = 1),
    /// rotation as `(sin, cos)` (absent = 0) and translation (absent = 0).
    pub tex_scale: Option<[Fx12; 2]>,
    pub tex_rotation: Option<[Fx12; 2]>,
    pub tex_translation: Option<[Fx12; 2]>,
}

fn rgb555(x: u32) -> [f32; 3] {
    [0, 5, 10].map(|lo| bits(x, lo, lo + 5) as f32 / 31.0)
}

impl Material {
    pub fn diffuse(&self) -> [f32; 3] {
        rgb555(self.dif_amb)
    }
    /// The diffuse colour also sets the vertex colour.
    pub fn diffuse_is_vertex_color(&self) -> bool {
        self.dif_amb & 0x8000 != 0
    }
    pub fn ambient(&self) -> [f32; 3] {
        rgb555(self.dif_amb >> 16)
    }
    pub fn specular(&self) -> [f32; 3] {
        rgb555(self.spe_emi)
    }
    pub fn shininess_table(&self) -> bool {
        self.spe_emi & 0x8000 != 0
    }
    pub fn emission(&self) -> [f32; 3] {
        rgb555(self.spe_emi >> 16)
    }
    /// Polygon alpha, 0..=31.
    pub fn alpha(&self) -> u8 {
        bits(self.polygon_attr, 16, 21) as u8
    }
    pub fn cull_back(&self) -> bool {
        self.polygon_attr & 0x40 == 0
    }
    pub fn cull_front(&self) -> bool {
        self.polygon_attr & 0x80 == 0
    }
    pub fn params(&self) -> TextureParams {
        TextureParams(self.tex_image_param)
    }
}

fn read_materials(data: &[u8], base: usize) -> Result<Vec<Material>> {
    let tex_pairing = at(base, u16_at(data, base)?)?;
    let pal_pairing = at(base, u16_at(data, at(base, 2u8)?)?)?;
    let mut materials = read_offset_dict(data, at(base, 4u8)?)?
        .into_iter()
        .map(|(off, name)| read_material(data, at(base, off)?, name))
        .collect::<Result<Vec<_>>>()?;

    for (dict, is_tex) in [(tex_pairing, true), (pal_pairing, false)] {
        for (value, name) in read_dict(data, dict, 4)? {
            let off = u16_at(value, 0)?;
            let num = usize::from(u8_at(value, 2)?);
            for &id in slice(data, at(base, off)?, num)? {
                let mat = materials
                    .get_mut(usize::from(id))
                    .ok_or(Error::Malformed("G3D material pairing index"))?;
                if is_tex {
                    mat.texture_name = Some(name);
                } else {
                    mat.palette_name = Some(name);
                }
            }
        }
    }
    Ok(materials)
}

fn read_material(data: &[u8], base: usize, name: Name) -> Result<Material> {
    let u16h = |o: usize| -> Result<u16> { u16_at(data, at(base, o as u64)?) };
    let u32h = |o: usize| -> Result<u32> { u32_at(data, at(base, o as u64)?) };
    let flags = u16h(30)?;
    let mut cur = at(base, 44u8)?;
    let fx32_pair = |cur: &mut usize| -> Result<[Fx12; 2]> {
        let a = Fx12::from_fx32(u32_at(data, *cur)?);
        let b = Fx12::from_fx32(u32_at(data, at(*cur, 4u8)?)?);
        *cur = at(*cur, 8u8)?;
        Ok([a, b])
    };
    let (mut tex_scale, mut tex_rotation, mut tex_translation) = (None, None, None);
    if flags & 1 != 0 {
        if flags & 2 == 0 {
            tex_scale = Some(fx32_pair(&mut cur)?);
        }
        if flags & 4 == 0 {
            let s = Fx12::from_fx16(u16_at(data, cur)?);
            let c = Fx12::from_fx16(u16_at(data, at(cur, 2u8)?)?);
            tex_rotation = Some([s, c]);
            cur = at(cur, 4u8)?;
        }
        if flags & 8 == 0 {
            tex_translation = Some(fx32_pair(&mut cur)?);
        }
    }
    Ok(Material {
        name,
        texture_name: None,
        palette_name: None,
        dif_amb: u32h(4)?,
        spe_emi: u32h(8)?,
        polygon_attr: u32h(12)?,
        polygon_attr_mask: u32h(16)?,
        tex_image_param: u32h(20)?,
        tex_image_param_mask: u32h(24)?,
        palette_base: u16h(28)?,
        flags,
        width: u16h(32)?,
        height: u16h(34)?,
        mag_w: Fx12::from_fx32(u32h(36)?),
        mag_h: Fx12::from_fx32(u32h(40)?),
        tex_scale,
        tex_rotation,
        tex_translation,
    })
}

/// An object (node, bone): its rest-pose transform.
#[derive(Debug, Clone)]
pub struct Object {
    pub name: Name,
    pub translation: Option<[Fx12; 3]>,
    /// 3x3 matrix, column by column for column vectors (the DS row-major order).
    pub rotation: Option<[Fx12; 9]>,
    pub scale: Option<[Fx12; 3]>,
    /// The inverse of [`scale`](Self::scale), stored next to it for the Maya and Softimage
    /// scaling rules.
    pub inv_scale: Option<[Fx12; 3]>,
}

impl Object {
    /// The `T * R * S` matrix of this object (standard scaling rule; see
    /// [`Model::rest_pose`] for the model's own rule).
    pub fn matrix(&self) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        if let Some(s) = self.scale {
            m = Mat4::scale(s.map(Fx12::to_f32));
        }
        if let Some(r) = &self.rotation {
            m = Mat4::from_mat3(r).mul(&m);
        }
        if let Some(t) = self.translation {
            m = Mat4::translation(t.map(Fx12::to_f32)).mul(&m);
        }
        m
    }
}

fn read_objects(data: &[u8], base: usize) -> Result<Vec<Object>> {
    read_offset_dict(data, base)?
        .into_iter()
        .map(|(off, name)| read_object(data, at(base, off)?, name))
        .collect()
}

fn read_object(data: &[u8], mut cur: usize, name: Name) -> Result<Object> {
    let next16 = |cur: &mut usize| -> Result<Fx12> {
        let v = Fx12::from_fx16(u16_at(data, *cur)?);
        *cur = at(*cur, 2u8)?;
        Ok(v)
    };
    let flags = u32::from(u16_at(data, cur)?);
    cur = at(cur, 2u8)?;
    // First rotation entry, stored here for alignment.
    let m0 = next16(&mut cur)?;
    let next32x3 = |cur: &mut usize| -> Result<[Fx12; 3]> {
        let s = slice(data, *cur, 12)?;
        *cur = at(*cur, 12u8)?;
        Ok(std::array::from_fn(|i| {
            Fx12::from_fx32(u32::from_le_bytes([
                s[4 * i],
                s[4 * i + 1],
                s[4 * i + 2],
                s[4 * i + 3],
            ]))
        }))
    };

    let translation = if flags & 1 == 0 {
        Some(next32x3(&mut cur)?)
    } else {
        None
    };
    let rotation = if flags & 8 != 0 {
        let a = next16(&mut cur)?;
        let b = next16(&mut cur)?;
        Some(pivot_mat(bits(flags, 4, 8), bits(flags, 8, 12), a, b))
    } else if flags & 2 == 0 {
        let mut m = [m0; 9];
        for v in &mut m[1..] {
            *v = next16(&mut cur)?;
        }
        Some(m)
    } else {
        None
    };
    let (scale, inv_scale) = if flags & 4 == 0 {
        let s = next32x3(&mut cur)?;
        (Some(s), Some(next32x3(&mut cur)?))
    } else {
        (None, None)
    };
    Ok(Object {
        name,
        translation,
        rotation,
        scale,
        inv_scale,
    })
}

/// Compressed "pivot" rotation: one axis is `±1`, the remaining 2x2 block is `(a, b, c, d)`
/// with `c = ±b`, `d = ±a`. apicula `nitro::rotation::pivot_mat`.
pub fn pivot_mat(select: u32, neg: u32, a: Fx12, b: Fx12) -> [Fx12; 9] {
    let z = Fx12::ZERO;
    if select >= 9 {
        return [a.wrapping_neg(), z, z, z, z, z, z, z, z];
    }
    let o = if neg & 1 == 0 {
        Fx12::ONE
    } else {
        Fx12::ONE.wrapping_neg()
    };
    let c = if neg & 2 == 0 { b } else { b.wrapping_neg() };
    let d = if neg & 4 == 0 { a } else { a.wrapping_neg() };
    match select {
        0 => [o, z, z, z, a, b, z, c, d],
        1 => [z, o, z, a, z, b, c, z, d],
        2 => [z, z, o, a, b, z, c, d, z],
        3 => [z, a, b, o, z, z, z, c, d],
        4 => [a, z, b, z, o, z, c, z, d],
        5 => [a, b, z, z, z, o, c, d, z],
        6 => [z, a, b, z, c, d, o, z, z],
        7 => [a, z, b, c, z, d, z, o, z],
        _ => [a, b, z, c, d, z, z, z, o],
    }
}

/// Reads up to `count` inverse bind matrices; each entry is a 4x3 matrix followed by a 3x3
/// matrix (for normals, ignored), all `fx32`.
fn read_inv_binds(data: &[u8], base: usize, count: usize) -> Vec<[Fx12; 12]> {
    const ENTRY: usize = (12 + 9) * 4;
    (0..count)
        .map_while(|i| {
            let s = slice(data, base.checked_add(i * ENTRY)?, ENTRY).ok()?;
            Some(std::array::from_fn(|k| {
                Fx12::from_fx32(u32::from_le_bytes([
                    s[4 * k],
                    s[4 * k + 1],
                    s[4 * k + 2],
                    s[4 * k + 3],
                ]))
            }))
        })
        .collect()
}

/// One weighted influence of a skinning matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinTerm {
    /// Weight in 1/256ths.
    pub weight: u8,
    pub stack_pos: u8,
    pub inv_bind_idx: u8,
}

/// Render op: one micro-operation of the render command bytecode (apicula `Op`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderOp {
    /// `current = stack[stack_pos]`.
    LoadMatrix { stack_pos: u8 },
    /// `stack[stack_pos] = current`.
    StoreMatrix { stack_pos: u8 },
    /// `current = current * objects[object_idx]`.
    MulObject { object_idx: u8 },
    /// `current = Σ weight * stack[stack_pos] * inv_binds[inv_bind_idx]`.
    Skin { terms: Vec<SkinTerm> },
    /// `current = current * scale(up_scale)`.
    ScaleUp,
    /// `current = current * scale(down_scale)`.
    ScaleDown,
    /// Bind `materials[material_idx]` for the following draws.
    BindMaterial { material_idx: u8 },
    /// Draw `meshes[mesh_idx]`.
    Draw { mesh_idx: u8 },
}

/// Decodes the render command bytecode at `off` into render ops, up to its end command.
/// apicula `nitro::render_cmds::parse_render_cmds`; billboard, env/projection map and other
/// commands are skipped (billboards are in [`Model::billboards`]).
pub fn parse_render_cmds(data: &[u8], cur: usize) -> Result<Vec<RenderOp>> {
    Ok(read_render_cmds(data, cur)?.0)
}

/// [`parse_render_cmds`], also returning the object links of the `0x06` commands and the
/// billboard commands.
fn read_render_cmds(
    data: &[u8],
    mut cur: usize,
) -> Result<(Vec<RenderOp>, Vec<ObjectLink>, Vec<BillboardCommand>)> {
    let mut ops = Vec::new();
    let mut links = Vec::new();
    let mut billboards = Vec::new();
    loop {
        let opcode = u8_at(data, cur)?;
        cur = at(cur, 1u8)?;
        let len = match opcode {
            // Skinning: store slot, term count, then 3 bytes per term.
            0x09 => 2 + 3 * usize::from(u8_at(data, at(cur, 1u8)?)?),
            0x00 | 0x01 | 0x0b | 0x2b | 0x40 | 0x80 => 0,
            0x03 | 0x04 | 0x05 | 0x07 | 0x08 | 0x24 | 0x44 => 1,
            0x02 | 0x0c | 0x0d | 0x47 => 2,
            0x06 => 3,
            0x26 | 0x46 => 4,
            0x66 => 5,
            _ => return Err(Error::Malformed("unknown G3D render command")),
        };
        let p = slice(data, cur, len)?;
        cur = at(cur, len as u64)?;
        match opcode {
            0x01 => return Ok((ops, links, billboards)),
            0x07 | 0x08 => billboards.push(BillboardCommand {
                op: ops.len(),
                parameter: p[0],
                y_only: opcode == 0x08,
            }),
            0x03 => ops.push(RenderOp::LoadMatrix { stack_pos: p[0] }),
            0x04 | 0x24 | 0x44 => ops.push(RenderOp::BindMaterial { material_idx: p[0] }),
            0x05 => ops.push(RenderOp::Draw { mesh_idx: p[0] }),
            0x06 | 0x26 | 0x46 | 0x66 => {
                // object, parent, flags, then optional store and load slots.
                let (store, load) = match opcode {
                    0x06 => (None, None),
                    0x26 => (Some(p[3]), None),
                    0x46 => (None, Some(p[3])),
                    _ => (Some(p[3]), Some(p[4])),
                };
                if let Some(stack_pos) = load {
                    ops.push(RenderOp::LoadMatrix { stack_pos });
                }
                ops.push(RenderOp::MulObject { object_idx: p[0] });
                links.push(ObjectLink {
                    object_idx: p[0],
                    parent_idx: p[1],
                    flags: p[2],
                });
                if let Some(stack_pos) = store {
                    ops.push(RenderOp::StoreMatrix { stack_pos });
                }
            }
            0x09 => {
                let terms = p[2..]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|t| SkinTerm {
                        stack_pos: t[0],
                        inv_bind_idx: t[1],
                        weight: t[2],
                    })
                    .collect();
                ops.push(RenderOp::Skin { terms });
                ops.push(RenderOp::StoreMatrix { stack_pos: p[0] });
            }
            0x0b => ops.push(RenderOp::ScaleUp),
            0x2b => ops.push(RenderOp::ScaleDown),
            _ => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::G3dFile;
    use super::super::test_util::{dict, file, section};
    use super::*;

    fn le32(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    /// A model with one object (translation (1,2,3)), one material bound to texture "tex"
    /// and palette "tex_pl", and one mesh whose display list draws one triangle.
    pub fn sample_mdl0(display_list: &[u8]) -> Vec<u8> {
        sample_mdl0_with(
            display_list,
            // mul object 0 & store 0, bind material 0, scale up, draw mesh 0, end.
            &[0x26, 0, 0, 0, 0, 0x04, 0, 0x0b, 0x05, 0, 0x01, 0, 0, 0],
        )
    }

    /// [`sample_mdl0`] with its own render commands.
    pub fn sample_mdl0_with(display_list: &[u8], render: &[u8]) -> Vec<u8> {
        // Objects dictionary at model+64, object data right after it.
        let obj_dict_len = dict(&[(&[0; 4], "root")]).len();
        let mut object = Vec::new();
        object.extend(0x0006u16.to_le_bytes()); // rotation zero, scale one
        object.extend(0u16.to_le_bytes());
        for v in [1u32 << 12, 2 << 12, 3 << 12] {
            object.extend(le32(v));
        }
        let objects = dict(&[(&le32(obj_dict_len as u32), "root")]);
        let mut objects = [objects, object].concat();
        while objects.len() % 4 != 0 {
            objects.push(0);
        }

        // Render commands: as given (the plain model: mul object 0 & store 0, bind material 0,
        // scale up, draw mesh 0, end).
        let render: Vec<u8> = render.to_vec();

        // Materials block: pairing offsets, material dict, material data, pairings.
        let mat_dict_len = dict(&[(&[0; 4], "mat")]).len();
        let mat_off = 4 + mat_dict_len;
        let mut material = vec![0u8; 44];
        material[2..4].copy_from_slice(&44u16.to_le_bytes());
        material[4..8].copy_from_slice(&le32(0x7FFF_FFFF));
        material[12..16].copy_from_slice(&le32(31 << 16));
        material[32..34].copy_from_slice(&8u16.to_le_bytes());
        material[34..36].copy_from_slice(&8u16.to_le_bytes());
        let pair_dict_len = dict(&[(&[0; 4], "tex")]).len();
        let tex_pair = mat_off + material.len();
        let pal_pair = tex_pair + pair_dict_len;
        let ids = pal_pair + pair_dict_len;
        let pair = [(ids as u16).to_le_bytes(), [1, 0]].concat();
        let mut materials = Vec::new();
        materials.extend((tex_pair as u16).to_le_bytes());
        materials.extend((pal_pair as u16).to_le_bytes());
        materials.extend(dict(&[(&le32(mat_off as u32), "mat")]));
        materials.extend(material);
        materials.extend(dict(&[(&pair, "tex")]));
        materials.extend(dict(&[(&pair, "tex_pl")]));
        materials.extend([0, 0, 0, 0]);

        // Meshes block: dict then 16-byte header then display list.
        let mesh_dict_len = dict(&[(&[0; 4], "mesh")]).len();
        let mut meshes = dict(&[(&le32(mesh_dict_len as u32), "mesh")]);
        meshes.extend(0u16.to_le_bytes());
        meshes.extend(16u16.to_le_bytes());
        meshes.extend(le32(0));
        meshes.extend(le32(16));
        meshes.extend(le32(display_list.len() as u32));
        meshes.extend(display_list);

        let render_off = 64 + objects.len();
        let materials_off = render_off + render.len();
        let meshes_off = materials_off + materials.len();
        let inv_off = meshes_off + meshes.len();
        let mut model = Vec::new();
        model.extend(le32(0));
        for off in [render_off, materials_off, meshes_off, inv_off] {
            model.extend(le32(off as u32));
        }
        model.extend([0, 0, 0, 1, 1, 1, 0, 0]);
        model.extend(le32(2 << 12)); // up_scale 2.0
        model.extend(le32(1 << 11));
        model.extend([0u8; 8]);
        model.extend([0u8; 12]);
        model.extend(le32(1 << 12));
        model.extend(le32(1 << 12));
        assert_eq!(model.len(), 64);
        model.extend(objects);
        model.extend(render);
        model.extend(materials);
        model.extend(meshes);
        let size = model.len() as u32;
        model[0..4].copy_from_slice(&le32(size));

        let mdl_dict_len = dict(&[(&[0; 4], "model")]).len();
        let body = [dict(&[(&le32((8 + mdl_dict_len) as u32), "model")]), model].concat();
        section(b"MDL0", &body)
    }

    /// Display list: BEGIN(tris), COLOR, VTX_16 x3, END.
    pub fn triangle_list() -> Vec<u8> {
        let mut dl = vec![0x40, 0x20, 0x23, 0x23];
        dl.extend(le32(0));
        dl.extend(le32(0x7FFF));
        dl.extend(le32(0));
        dl.extend(le32(0));
        dl.extend(le32(0x1000));
        dl.extend(le32(0));
        dl.extend([0x23, 0x41, 0, 0]);
        dl.extend(le32(0x1000_0000));
        dl.extend(le32(0));
        dl
    }

    #[test]
    fn parses_model() {
        let data = file(b"BMD0", &[sample_mdl0(&triangle_list())]);
        let f = G3dFile::parse(&data).unwrap();
        let m = &f.models[0];
        assert!(m.name.is("model"));
        assert_eq!(m.up_scale.to_f32(), 2.0);
        assert_eq!(m.objects[0].translation.unwrap()[2].to_f32(), 3.0);
        assert_eq!(m.objects[0].rotation, None);
        assert!(m.materials[0].texture_name.unwrap().is("tex"));
        assert!(m.materials[0].palette_name.unwrap().is("tex_pl"));
        assert_eq!(m.materials[0].alpha(), 31);
        assert!(m.materials[0].diffuse_is_vertex_color());
        assert_eq!(m.meshes[0].commands.len(), triangle_list().len());
        assert_eq!(
            m.render_ops,
            vec![
                RenderOp::MulObject { object_idx: 0 },
                RenderOp::StoreMatrix { stack_pos: 0 },
                RenderOp::BindMaterial { material_idx: 0 },
                RenderOp::ScaleUp,
                RenderOp::Draw { mesh_idx: 0 },
            ]
        );
        assert!(m.inv_binds.is_empty());
    }

    #[test]
    fn billboard_commands_keep_their_op_position() {
        // mul object 0; billboard 0x08 (parameter 0); draw 0; billboard 0x07 (parameter 3);
        // draw 1; end. Each billboard consumes its parameter byte and adds no render op.
        let cmds = [0x06, 0, 0, 0, 0x08, 0, 0x05, 0, 0x07, 3, 0x05, 1, 0x01];
        let (ops, links, billboards) = read_render_cmds(&cmds, 0).unwrap();
        assert_eq!(ops.len(), 3);
        assert_eq!(links.len(), 1);
        assert_eq!(
            billboards,
            vec![
                BillboardCommand {
                    op: 1,
                    parameter: 0,
                    y_only: true
                },
                BillboardCommand {
                    op: 2,
                    parameter: 3,
                    y_only: false
                },
            ]
        );
    }

    #[test]
    fn truncated_models_are_errors() {
        let data = file(b"BMD0", &[sample_mdl0(&triangle_list())]);
        for len in 0..data.len() {
            let mut cut = data[..len].to_vec();
            if cut.len() >= 12 {
                cut[8..12].copy_from_slice(&le32(len as u32));
            }
            assert!(G3dFile::parse(&cut).is_err(), "len {len}");
        }
    }

    #[test]
    fn rejects_out_of_range_render_indices() {
        let mut data = file(b"BMD0", &[sample_mdl0(&triangle_list())]);
        // Locate the draw command (0x05 0x00 0x01) and point it at mesh 9.
        let pos = data.windows(3).position(|w| w == [0x05, 0, 0x01]).unwrap();
        data[pos + 1] = 9;
        assert!(G3dFile::parse(&data).is_err());
    }

    /// A plain material named "mat" (no texture, white, opaque).
    pub fn sample_material() -> Material {
        Material {
            name: Name(*b"mat\0\0\0\0\0\0\0\0\0\0\0\0\0"),
            texture_name: None,
            palette_name: None,
            dif_amb: 0x7fff,
            spe_emi: 0,
            polygon_attr: 31 << 16,
            polygon_attr_mask: 0,
            tex_image_param: 0,
            tex_image_param_mask: 0,
            palette_base: 0,
            flags: 0,
            width: 8,
            height: 8,
            mag_w: Fx12::ONE,
            mag_h: Fx12::ONE,
            tex_scale: None,
            tex_rotation: None,
            tex_translation: None,
        }
    }

    #[test]
    fn pivot_matrices() {
        let (a, b) = (Fx12(100), Fx12(200));
        let m = pivot_mat(0, 0b111, a, b);
        assert_eq!(m[0], Fx12(-4096));
        assert_eq!(m[4..], [a, b, Fx12::ZERO, Fx12(-200), Fx12(-100)]);
        assert_eq!(pivot_mat(12, 0, a, b)[0], Fx12(-100));
    }

    #[test]
    fn object_matrix_is_trs() {
        let o = Object {
            name: Name([0; 16]),
            translation: Some([Fx12::ONE, Fx12::ZERO, Fx12::ZERO]),
            rotation: Some(pivot_mat(2, 0, Fx12::ZERO, Fx12::ONE)),
            scale: Some([Fx12(2 << 12); 3]),
            inv_scale: Some([Fx12(1 << 11); 3]),
        };
        // select 2: columns (0,0,1), (0,1,0), (1,0,0) with a = 0, b = 1, c = 1, d = 0.
        let p = o.matrix().transform_point([1.0, 0.0, 0.0]);
        assert_eq!(p, [1.0, 0.0, 2.0]);
    }
}
