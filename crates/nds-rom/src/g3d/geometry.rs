//! Runs a model's render ops and display lists into indexed triangles in model space.
//!
//! Ported from apicula `primitives` (triangle-only output). Texture matrices and
//! billboards are not applied.

use std::ops::Range;

use super::fx::{Fx12, Mat4, bits};
use super::gpu::{DisplayList, GpuCmd};
use super::model::{Model, RenderOp, SkinTerm};
use crate::{Error, Result};

/// One output vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// Model-space position, after the matrix stack (object transforms, `up_scale`).
    pub position: [f32; 3],
    /// Texture coordinate normalised by the material's texture size, DS orientation (`v`
    /// grows downwards).
    pub texcoord: [f32; 2],
    pub color: [f32; 3],
    /// Model-space unit normal, zero when the vertex has a colour instead.
    pub normal: [f32; 3],
}

impl Default for Vertex {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            texcoord: [0.0; 2],
            color: [1.0; 3],
            normal: [0.0; 3],
        }
    }
}

/// The result of drawing one mesh with one material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawCall {
    pub material_idx: u8,
    pub mesh_idx: u8,
    pub vertex_range: Range<usize>,
    /// Range of [`Geometry::indices`], three per triangle.
    pub index_range: Range<usize>,
    pub used_texcoords: bool,
    pub used_colors: bool,
    pub used_normals: bool,
}

/// Indexed triangle list of a whole model.
#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub draw_calls: Vec<DrawCall>,
}

impl Geometry {
    /// Builds the model in its rest pose (object matrices from [`Model::objects`]).
    pub fn build(model: &Model) -> Result<Geometry> {
        let objects: Vec<Mat4> = model.objects.iter().map(|o| o.matrix()).collect();
        Self::build_posed(model, &objects)
    }

    /// Builds the model with the given object matrices (one per object, e.g. animated).
    pub fn build_posed(model: &Model, objects: &[Mat4]) -> Result<Geometry> {
        let mut b = Builder {
            model,
            objects,
            cur: Mat4::IDENTITY,
            stack: [Mat4::IDENTITY; 32],
            sp: 0,
            material: 0,
            out: Geometry::default(),
            prim_start: 0,
            prim_type: 0,
            next: Vertex::default(),
            call: None,
        };
        for op in &model.render_ops {
            b.op(op)?;
        }
        Ok(b.out)
    }
}

struct Builder<'m, 'a> {
    model: &'m Model<'a>,
    objects: &'m [Mat4],
    cur: Mat4,
    stack: [Mat4; 32],
    /// Position matrix stack pointer, for `MTX_PUSH`/`MTX_POP`.
    sp: usize,
    material: u8,
    out: Geometry,
    prim_start: usize,
    prim_type: u32,
    next: Vertex,
    call: Option<DrawCall>,
}

fn slot(stack_pos: impl Into<usize>) -> Result<usize> {
    let s = stack_pos.into();
    if s < 32 {
        Ok(s)
    } else {
        Err(Error::Malformed("G3D matrix stack slot out of range"))
    }
}

fn fx_columns(entries: &[Fx12], rows: usize) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    for (c, col) in entries.chunks_exact(rows).enumerate() {
        for (r, v) in col.iter().enumerate() {
            m.0[c][r] = v.to_f32();
        }
    }
    m
}

impl Builder<'_, '_> {
    fn op(&mut self, op: &RenderOp) -> Result<()> {
        match op {
            RenderOp::LoadMatrix { stack_pos } => self.cur = self.stack[slot(*stack_pos)?],
            RenderOp::StoreMatrix { stack_pos } => self.stack[slot(*stack_pos)?] = self.cur,
            RenderOp::MulObject { object_idx } => {
                let m = self
                    .objects
                    .get(usize::from(*object_idx))
                    .ok_or(Error::Malformed("G3D object index out of range"))?;
                self.cur = self.cur.mul(m);
            }
            RenderOp::Skin { terms } => self.skin(terms)?,
            RenderOp::ScaleUp => {
                let s = self.model.up_scale.to_f32();
                self.cur = self.cur.mul(&Mat4::scale([s; 3]));
            }
            RenderOp::ScaleDown => {
                let s = self.model.down_scale.to_f32();
                self.cur = self.cur.mul(&Mat4::scale([s; 3]));
            }
            RenderOp::BindMaterial { material_idx } => self.material = *material_idx,
            RenderOp::Draw { mesh_idx } => self.draw(*mesh_idx)?,
        }
        Ok(())
    }

    fn skin(&mut self, terms: &[SkinTerm]) -> Result<()> {
        let mut m = Mat4::ZERO;
        for t in terms {
            let inv = self
                .model
                .inv_binds
                .get(usize::from(t.inv_bind_idx))
                .ok_or(Error::Malformed("G3D inverse bind index out of range"))?;
            let term = self.stack[slot(t.stack_pos)?].mul(&Mat4::from_mat43(inv));
            m.add_scaled(&term, f32::from(t.weight) / 256.0);
        }
        self.cur = m;
        Ok(())
    }

    fn draw(&mut self, mesh_idx: u8) -> Result<()> {
        let mesh = self
            .model
            .meshes
            .get(usize::from(mesh_idx))
            .ok_or(Error::Malformed("G3D mesh index out of range"))?;
        let material = self.model.materials.get(usize::from(self.material));
        let (w, h) = material.map_or((1, 1), |m| (m.width.max(1), m.height.max(1)));
        self.next = Vertex::default();
        let mut call = DrawCall {
            material_idx: self.material,
            mesh_idx,
            vertex_range: self.out.vertices.len()..self.out.vertices.len(),
            index_range: self.out.indices.len()..self.out.indices.len(),
            used_texcoords: false,
            used_colors: false,
            used_normals: false,
        };
        if let Some(m) = material
            && m.diffuse_is_vertex_color()
            && m.diffuse() != [1.0; 3]
        {
            self.next.color = m.diffuse();
            call.used_colors = true;
        }
        self.call = Some(call);
        self.prim_start = self.out.vertices.len();
        for cmd in DisplayList::new(mesh.commands) {
            self.gpu(cmd?, f32::from(w), f32::from(h))?;
        }
        self.end_prim();
        if let Some(mut call) = self.call.take() {
            call.vertex_range.end = self.out.vertices.len();
            call.index_range.end = self.out.indices.len();
            self.out.draw_calls.push(call);
        }
        Ok(())
    }

    fn gpu(&mut self, cmd: GpuCmd, w: f32, h: f32) -> Result<()> {
        match cmd {
            GpuCmd::Restore(i) => self.cur = self.stack[slot(i as usize)?],
            GpuCmd::Store(i) => self.stack[slot(i as usize)?] = self.cur,
            GpuCmd::Push => {
                self.stack[slot(self.sp)?] = self.cur;
                self.sp += 1;
            }
            GpuCmd::Pop(n) => {
                self.sp = usize::try_from(self.sp as i64 - i64::from(n))
                    .map_err(|_| Error::Malformed("G3D matrix stack underflow"))?;
                self.cur = self.stack[slot(self.sp)?];
            }
            GpuCmd::Identity => self.cur = Mat4::IDENTITY,
            GpuCmd::LoadMatrix(m) => self.cur = fx_columns(&m, if m.len() == 16 { 4 } else { 3 }),
            GpuCmd::MultMatrix(m) => {
                let rows = if m.len() == 16 { 4 } else { 3 };
                self.cur = self.cur.mul(&fx_columns(&m, rows));
            }
            GpuCmd::Scale(s) => self.cur = self.cur.mul(&Mat4::scale(s.map(Fx12::to_f32))),
            GpuCmd::Translate(t) => {
                self.cur = self.cur.mul(&Mat4::translation(t.map(Fx12::to_f32)));
            }
            GpuCmd::Begin(t) => {
                self.end_prim();
                self.prim_type = t;
            }
            GpuCmd::End => self.end_prim(),
            GpuCmd::TexCoord([s, t]) => {
                self.mark(|c| c.used_texcoords = true);
                self.next.texcoord = [f32::from(s) / 16.0 / w, f32::from(t) / 16.0 / h];
            }
            // The DS lights a vertex from its normal into a colour: a vertex has one or the
            // other, so setting one clears the other (as apicula does).
            GpuCmd::Color(c) => {
                self.mark(|c| c.used_colors = true);
                let c = u32::from(c);
                self.next.color = [0, 5, 10].map(|lo| bits(c, lo, lo + 5) as f32 / 31.0);
                self.next.normal = [0.0; 3];
            }
            GpuCmd::Normal(n) => {
                self.mark(|c| c.used_normals = true);
                let n = self.cur.transform_vector(n.map(|v| f32::from(v) / 512.0));
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                self.next.normal = if len > 0.0 { n.map(|v| v / len) } else { n };
                self.next.color = [1.0; 3];
            }
            GpuCmd::Vertex(p) => {
                self.next.position = self.cur.transform_point(p.map(Fx12::to_f32));
                self.out.vertices.push(self.next);
            }
            GpuCmd::Nop | GpuCmd::MatrixMode(_) | GpuCmd::Other { .. } => {}
        }
        Ok(())
    }

    fn mark(&mut self, f: impl FnOnce(&mut DrawCall)) {
        if let Some(c) = &mut self.call {
            f(c);
        }
    }

    fn end_prim(&mut self) {
        let (start, end) = (self.prim_start, self.out.vertices.len());
        let idx = &mut self.out.indices;
        let mut tri = |a: usize, b: usize, c: usize| idx.extend([a, b, c].map(|i| i as u32));
        match self.prim_type {
            0 => (start..end.saturating_sub(2))
                .step_by(3)
                .for_each(|i| tri(i, i + 1, i + 2)),
            1 => (start..end.saturating_sub(3)).step_by(4).for_each(|i| {
                tri(i, i + 1, i + 2);
                tri(i, i + 2, i + 3);
            }),
            2 => (start..end.saturating_sub(2)).for_each(|i| {
                if (i - start) % 2 == 0 {
                    tri(i, i + 1, i + 2);
                } else {
                    tri(i, i + 2, i + 1);
                }
            }),
            _ => (start..end.saturating_sub(3)).step_by(2).for_each(|i| {
                tri(i, i + 1, i + 3);
                tri(i, i + 3, i + 2);
            }),
        }
        self.prim_start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::super::G3dFile;
    use super::super::model::tests::{sample_mdl0, triangle_list};
    use super::super::test_util::file;
    use super::*;

    #[test]
    fn builds_triangles_in_model_space() {
        let data = file(b"BMD0", &[sample_mdl0(&triangle_list())]);
        let f = G3dFile::parse(&data).unwrap();
        let g = Geometry::build(&f.models[0]).unwrap();
        assert_eq!(g.indices, vec![0, 1, 2]);
        assert_eq!(g.draw_calls.len(), 1);
        assert!(g.draw_calls[0].used_colors);
        // Object translation (1, 2, 3) then up_scale 2.
        let p: Vec<_> = g.vertices.iter().map(|v| v.position).collect();
        assert_eq!(p, vec![[1.0, 2.0, 3.0], [3.0, 2.0, 3.0], [1.0, 4.0, 3.0]]);
        assert_eq!(g.vertices[0].color, [1.0; 3]);
    }

    #[test]
    fn strips_and_quads() {
        let data = file(b"BMD0", &[sample_mdl0(&triangle_list())]);
        let f = G3dFile::parse(&data).unwrap();
        let mut g = Builder {
            model: &f.models[0],
            objects: &[],
            cur: Mat4::IDENTITY,
            stack: [Mat4::IDENTITY; 32],
            sp: 0,
            material: 0,
            out: Geometry::default(),
            prim_start: 0,
            prim_type: 2,
            next: Vertex::default(),
            call: None,
        };
        g.out.vertices = vec![Vertex::default(); 5];
        g.end_prim();
        assert_eq!(g.out.indices, vec![0, 1, 2, 1, 3, 2, 2, 3, 4]);
        g.out.indices.clear();
        g.prim_start = 0;
        g.prim_type = 3;
        g.out.vertices.push(Vertex::default());
        g.end_prim();
        assert_eq!(g.out.indices, vec![0, 1, 3, 0, 3, 2, 2, 3, 5, 2, 5, 4]);
    }

    #[test]
    fn bad_display_list_is_an_error() {
        let data = file(b"BMD0", &[sample_mdl0(&[0xFF, 0, 0, 0])]);
        let f = G3dFile::parse(&data).unwrap();
        assert!(Geometry::build(&f.models[0]).is_err());
    }
}
