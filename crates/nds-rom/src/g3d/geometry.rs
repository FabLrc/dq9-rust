//! Runs a model's render ops and display lists into indexed triangles in model space.
//!
//! Ported from apicula `primitives` (triangle-only output). Texture coordinates are the
//! display lists' own; [`Geometry::apply_tex_matrices`] applies texture matrices. Billboards
//! are not applied.

use std::ops::Range;

use super::fx::{Fx12, Mat4, bits};
use super::gpu::{DisplayList, GpuCmd};
use super::model::{Model, RenderOp, SkinTerm};
use super::srt::TexMatrix;
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

/// Vertices drawn while a billboard command (`0x07`/`0x08`) was active, and the current matrix
/// at that point decomposed into what a billboard keeps: the pivot (its translation) and the
/// object's axes (its 3x3 part, normalized). The rotation is replaced every frame by a
/// camera-facing one, so the drawn vertex `v` ends up at `pivot + rotation · (scale · v)`, as
/// the camera-space matrix of `RenderCommand_7`/`RenderCommand_8` has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Billboard {
    /// Range of [`Geometry::vertices`] drawn under the command.
    pub vertex_range: Range<usize>,
    /// Translation of the current matrix.
    pub pivot: [f32; 3],
    /// Normalized columns of the current matrix's 3x3 part, column-major.
    pub rotation: [f32; 9],
    /// `0x08`: the object keeps its up axis and turns around it; `0x07`: it faces the camera
    /// flat.
    pub y_only: bool,
}

/// The billboard a draw is under, taken from the current matrix when the command runs.
#[derive(Debug, Clone, PartialEq)]
struct ActiveBillboard {
    pivot: [f32; 3],
    rotation: [f32; 9],
    y_only: bool,
}

impl ActiveBillboard {
    /// `current = T · R · S` for column vectors: the pivot is its translation and each column
    /// of the 3x3 part is a scaled axis.
    fn of(cur: &Mat4, y_only: bool) -> Self {
        let mut out = [0.0; 9];
        for (c, col) in cur.0.iter().take(3).enumerate() {
            let len = (col[0] * col[0] + col[1] * col[1] + col[2] * col[2]).sqrt();
            let len = if len > 0.0 { len } else { 1.0 };
            for r in 0..3 {
                out[c * 3 + r] = col[r] / len;
            }
        }
        Self {
            pivot: [cur.0[3][0], cur.0[3][1], cur.0[3][2]],
            rotation: out,
            y_only,
        }
    }
}

/// Indexed triangle list of a whole model.
#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub draw_calls: Vec<DrawCall>,
    /// Vertices drawn under a billboard command (`0x07`/`0x08`), with the current matrix they
    /// were drawn with: a renderer turns them toward the camera instead of using its rotation.
    pub billboards: Vec<Billboard>,
}

impl Geometry {
    /// Builds the model in its rest pose ([`Model::rest_pose`]).
    pub fn build(model: &Model) -> Result<Geometry> {
        Self::build_posed(model, &model.rest_pose()?)
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
            billboard: None,
            out: Geometry::default(),
            prim_start: 0,
            prim_type: 0,
            next: Vertex::default(),
            call: None,
        };
        let mut commands = model.billboards.iter().peekable();
        for (index, op) in model.render_ops.iter().enumerate() {
            while let Some(command) = commands.next_if(|c| c.op == index) {
                b.billboard = Some(ActiveBillboard::of(&b.cur, command.y_only));
            }
            b.op(op)?;
        }
        Ok(b.out)
    }

    /// Transforms the texture coordinates of every draw call whose material has a matrix in
    /// `matrices` (indexed by material, e.g. [`Model::tex_matrices`] or animated ones), in the
    /// material's texel space.
    pub fn apply_tex_matrices(&mut self, model: &Model, matrices: &[Option<TexMatrix>]) {
        for call in &self.draw_calls {
            let idx = usize::from(call.material_idx);
            let (Some(Some(m)), Some(mat)) = (matrices.get(idx), model.materials.get(idx)) else {
                continue;
            };
            let (w, h) = (f32::from(mat.width.max(1)), f32::from(mat.height.max(1)));
            for v in &mut self.vertices[call.vertex_range.clone()] {
                let [s, t] = m.apply([v.texcoord[0] * w, v.texcoord[1] * h]);
                v.texcoord = [s / w, t / h];
            }
        }
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
    /// Billboard the current matrix is under, until a command replaces that matrix.
    billboard: Option<ActiveBillboard>,
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
            RenderOp::LoadMatrix { stack_pos } => {
                self.cur = self.stack[slot(*stack_pos)?];
                self.billboard = None;
            }
            RenderOp::StoreMatrix { stack_pos } => self.stack[slot(*stack_pos)?] = self.cur,
            RenderOp::MulObject { object_idx } => {
                let m = self
                    .objects
                    .get(usize::from(*object_idx))
                    .ok_or(Error::Malformed("G3D object index out of range"))?;
                self.cur = self.cur.mul(m);
                self.billboard = None;
            }
            RenderOp::Skin { terms } => {
                self.skin(terms)?;
                self.billboard = None;
            }
            // A scale applied after a billboard only stretches the object's axes; the
            // billboard keeps the rotation decision it already made.
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
        let start = self.out.vertices.len();
        for cmd in DisplayList::new(mesh.commands) {
            self.gpu(cmd?, f32::from(w), f32::from(h))?;
        }
        self.end_prim();
        if let Some(mut call) = self.call.take() {
            call.vertex_range.end = self.out.vertices.len();
            call.index_range.end = self.out.indices.len();
            self.out.draw_calls.push(call);
        }
        if let Some(b) = &self.billboard
            && start < self.out.vertices.len()
        {
            self.out.billboards.push(Billboard {
                vertex_range: start..self.out.vertices.len(),
                pivot: b.pivot,
                rotation: b.rotation,
                y_only: b.y_only,
            });
        }
        Ok(())
    }

    fn gpu(&mut self, cmd: GpuCmd, w: f32, h: f32) -> Result<()> {
        match cmd {
            GpuCmd::Restore(i) => {
                self.cur = self.stack[slot(i as usize)?];
                self.billboard = None;
            }
            GpuCmd::Store(i) => self.stack[slot(i as usize)?] = self.cur,
            GpuCmd::Push => {
                self.stack[slot(self.sp)?] = self.cur;
                self.sp += 1;
            }
            GpuCmd::Pop(n) => {
                self.sp = usize::try_from(self.sp as i64 - i64::from(n))
                    .map_err(|_| Error::Malformed("G3D matrix stack underflow"))?;
                self.cur = self.stack[slot(self.sp)?];
                self.billboard = None;
            }
            GpuCmd::Identity => {
                self.cur = Mat4::IDENTITY;
                self.billboard = None;
            }
            GpuCmd::LoadMatrix(m) => {
                self.cur = fx_columns(&m, if m.len() == 16 { 4 } else { 3 });
                self.billboard = None;
            }
            GpuCmd::MultMatrix(m) => {
                let rows = if m.len() == 16 { 4 } else { 3 };
                self.cur = self.cur.mul(&fx_columns(&m, rows));
                self.billboard = None;
            }
            GpuCmd::Scale(s) => self.cur = self.cur.mul(&Mat4::scale(s.map(Fx12::to_f32))),
            GpuCmd::Translate(t) => {
                self.cur = self.cur.mul(&Mat4::translation(t.map(Fx12::to_f32)));
                self.billboard = None;
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
    use super::super::model::tests::{sample_mdl0, sample_mdl0_with, triangle_list};
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

    /// A billboard command leaves the draws that follow it to the camera: the record keeps the
    /// pivot and the object's axes of the current matrix, and the vertices keep the object's
    /// own rotation (`x` of the triangle stays to the right, `up_scale` still applies).
    #[test]
    fn billboard_commands_leave_their_draws_to_the_camera() {
        // Multiply by object 0 and store it; billboard about Y; scale up; draw mesh 0;
        // billboard flat; draw mesh 0 again; end.
        let render = [
            0x26, 0, 0, 0, 0, 0x08, 0, 0x0b, 0x05, 0, 0x07, 0, 0x05, 0, 0x01, 0,
        ];
        let data = file(b"BMD0", &[sample_mdl0_with(&triangle_list(), &render)]);
        let f = G3dFile::parse(&data).unwrap();
        assert_eq!(f.models[0].billboards.len(), 2);
        let g = Geometry::build(&f.models[0]).unwrap();
        assert_eq!(g.billboards.len(), 2);
        let [first, second] = [&g.billboards[0], &g.billboards[1]];
        // One record per draw; the first scales up, the second re-decomposes the matrix that
        // scale left (same pivot, same axes).
        assert_eq!(first.vertex_range, 0..3);
        assert_eq!(second.vertex_range, 3..6);
        assert!(first.y_only && !second.y_only);
        assert_eq!(first.pivot, [1.0, 2.0, 3.0]);
        assert_eq!(second.pivot, [1.0, 2.0, 3.0]);
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert_eq!(first.rotation, identity);
        assert_eq!(second.rotation, identity);
        // Unchanged geometry: the billboard is applied by the renderer, per frame.
        let p: Vec<_> = g.vertices.iter().map(|v| v.position).collect();
        assert_eq!(
            p,
            vec![
                [1.0, 2.0, 3.0],
                [3.0, 2.0, 3.0],
                [1.0, 4.0, 3.0],
                [1.0, 2.0, 3.0],
                [3.0, 2.0, 3.0],
                [1.0, 4.0, 3.0],
            ]
        );
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
            billboard: None,
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
