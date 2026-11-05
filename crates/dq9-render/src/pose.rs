//! Decodes display lists once; subsequent poses replay only matrix/vertex operations.
//! Topology, indices, UVs and GPU allocations remain unchanged.
use nds_rom::g3d::{DisplayList, Fx12, GpuCmd, Mat4, Model, RenderOp};
use nds_rom::{Error, Result};

use crate::SceneVertex;
use crate::scene::fixed_light;

#[derive(Debug)]
enum Op {
    Render(RenderOp),
    Matrix(Mat4, bool),
    Gpu(GpuCmd),
    /// Material index, and whether the mesh sends `COLOR` commands (shape flag bit 1).
    Draw(usize, bool),
}

#[derive(Debug)]
pub(crate) struct VertexPose(Vec<Op>);

impl VertexPose {
    pub fn compile(model: &Model<'_>) -> Result<Self> {
        let mut ops = Vec::new();
        let mut material = 0;
        for op in &model.render_ops {
            match op {
                RenderOp::BindMaterial { material_idx } => material = usize::from(*material_idx),
                RenderOp::Draw { mesh_idx } => {
                    let mesh = model
                        .meshes
                        .get(usize::from(*mesh_idx))
                        .ok_or(Error::Malformed("pose mesh index"))?;
                    ops.push(Op::Draw(material, mesh.flags & 2 != 0));
                    for cmd in DisplayList::new(mesh.commands) {
                        let cmd = cmd?;
                        let matrix = |entries: &[Fx12]| {
                            let rows = if entries.len() == 16 { 4 } else { 3 };
                            let mut m = Mat4::IDENTITY;
                            for (c, col) in entries.chunks_exact(rows).enumerate() {
                                for (r, v) in col.iter().enumerate() {
                                    m.0[c][r] = v.to_f32();
                                }
                            }
                            m
                        };
                        match &cmd {
                            GpuCmd::LoadMatrix(m) => ops.push(Op::Matrix(matrix(m), false)),
                            GpuCmd::MultMatrix(m) => ops.push(Op::Matrix(matrix(m), true)),
                            GpuCmd::Scale(s) => {
                                ops.push(Op::Matrix(Mat4::scale(s.map(Fx12::to_f32)), true))
                            }
                            GpuCmd::Translate(t) => {
                                ops.push(Op::Matrix(Mat4::translation(t.map(Fx12::to_f32)), true))
                            }
                            GpuCmd::Begin(_)
                            | GpuCmd::End
                            | GpuCmd::TexCoord(_)
                            | GpuCmd::Other { .. }
                            | GpuCmd::Nop
                            | GpuCmd::MatrixMode(_) => {}
                            _ => ops.push(Op::Gpu(cmd)),
                        }
                    }
                }
                _ => ops.push(Op::Render(op.clone())),
            }
        }
        Ok(Self(ops))
    }

    pub fn apply(
        &self,
        model: &Model<'_>,
        objects: &[Mat4],
        vertices: &mut [SceneVertex],
        (lit, tinted): (bool, bool),
    ) -> Result<()> {
        let mut cur = Mat4::IDENTITY;
        let mut stack = [Mat4::IDENTITY; 32];
        let mut sp = 0usize;
        let mut material = 0;
        let mut color = [1.0; 3];
        let mut normal = None;
        let mut mesh_color = false;
        let mut at = 0;
        let slot = |s: usize| {
            if s < 32 {
                Ok(s)
            } else {
                Err(Error::Malformed("pose matrix stack"))
            }
        };
        for op in &self.0 {
            match op {
                Op::Draw(m, has_color) => {
                    material = *m;
                    mesh_color = *has_color;
                    let m = &model.materials[material];
                    color = if m.diffuse_is_vertex_color() {
                        m.diffuse()
                    } else {
                        [1.0; 3]
                    };
                    normal = None;
                }
                Op::Matrix(m, mul) => cur = if *mul { cur.mul(m) } else { *m },
                Op::Render(op) => match op {
                    RenderOp::LoadMatrix { stack_pos } => {
                        cur = stack[slot(usize::from(*stack_pos))?]
                    }
                    RenderOp::StoreMatrix { stack_pos } => {
                        stack[slot(usize::from(*stack_pos))?] = cur
                    }
                    RenderOp::MulObject { object_idx } => {
                        cur = cur.mul(
                            objects
                                .get(usize::from(*object_idx))
                                .ok_or(Error::Malformed("pose object index"))?,
                        )
                    }
                    RenderOp::ScaleUp => cur = cur.mul(&Mat4::scale([model.up_scale.to_f32(); 3])),
                    RenderOp::ScaleDown => {
                        cur = cur.mul(&Mat4::scale([model.down_scale.to_f32(); 3]))
                    }
                    RenderOp::Skin { terms } => {
                        let mut m = Mat4::ZERO;
                        for t in terms {
                            let inv = model
                                .inv_binds
                                .get(usize::from(t.inv_bind_idx))
                                .ok_or(Error::Malformed("pose inverse bind"))?;
                            let term =
                                stack[slot(usize::from(t.stack_pos))?].mul(&Mat4::from_mat43(inv));
                            m.add_scaled(&term, f32::from(t.weight) / 256.0);
                        }
                        cur = m;
                    }
                    RenderOp::BindMaterial { .. } | RenderOp::Draw { .. } => unreachable!(),
                },
                Op::Gpu(cmd) => match cmd {
                    GpuCmd::Restore(i) => cur = stack[slot(*i as usize)?],
                    GpuCmd::Store(i) => stack[slot(*i as usize)?] = cur,
                    GpuCmd::Push => {
                        stack[slot(sp)?] = cur;
                        sp += 1;
                    }
                    GpuCmd::Pop(n) => {
                        sp = usize::try_from(sp as i64 - i64::from(*n))
                            .map_err(|_| Error::Malformed("pose matrix underflow"))?;
                        cur = stack[slot(sp)?];
                    }
                    GpuCmd::Identity => cur = Mat4::IDENTITY,
                    GpuCmd::Color(c) => {
                        color = [0, 5, 10].map(|s| f32::from(c >> s & 31) / 31.0);
                        normal = None;
                    }
                    GpuCmd::Normal(n) => {
                        let n = glam::Vec3::from_array(
                            cur.transform_vector(n.map(|v| f32::from(v) / 512.0)),
                        );
                        normal = (n != glam::Vec3::ZERO).then(|| n.normalize().to_array());
                        color = [1.0; 3];
                    }
                    GpuCmd::Vertex(p) => {
                        let v = vertices
                            .get_mut(at)
                            .ok_or(Error::Malformed("pose vertex count"))?;
                        v.position = cur.transform_point(p.map(Fx12::to_f32));
                        match normal {
                            // Map models: the zone's lights light the vertex at draw time.
                            Some(n) if lit => {
                                v.normal = n;
                                v.color = [1.0; 3];
                                v.tint = 0.0;
                            }
                            Some(n) => {
                                v.normal = [0.0; 3];
                                v.color = fixed_light(&model.materials[material], n);
                                v.tint = 0.0;
                            }
                            None => {
                                v.normal = [0.0; 3];
                                v.color = color;
                                v.tint = f32::from(u8::from(lit && tinted && mesh_color));
                            }
                        }
                        at += 1;
                    }
                    _ => unreachable!(),
                },
            }
        }
        if at != vertices.len() {
            return Err(Error::Malformed("pose vertex count"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nds_rom::g3d::{G3dFile, Geometry};

    #[test]
    fn cached_character_pose_matches_geometry_builder() {
        let Some(bytes) = testrom::load() else { return };
        let rom = nds_rom::Rom::parse(bytes).unwrap();
        let chr =
            dq9_formats::chr::Chr::parse(rom.file_by_path("data/chara_sub/s000.chr").unwrap())
                .unwrap();
        let model = G3dFile::parse(chr.model_data().unwrap().unwrap()).unwrap();
        let model = &model.models[0];
        let anim_id = (0..chr.narc.len())
            .map(|i| i as u16)
            .find(|&i| chr.narc.name(i) == Some("walk.nsbca"))
            .unwrap();
        let animation = G3dFile::parse(chr.narc.file(anim_id).unwrap()).unwrap();
        let animation = &animation.joint_animations[0];
        let plan = VertexPose::compile(model).unwrap();
        let mut vertices =
            vec![SceneVertex::default(); Geometry::build(model).unwrap().vertices.len()];
        for frame in [0, 4, 8] {
            let objects = animation.pose(model, frame).unwrap();
            let reference = Geometry::build_posed(model, &objects).unwrap();
            plan.apply(model, &objects, &mut vertices, (false, true))
                .unwrap();
            for (posed, expected) in vertices.iter().zip(&reference.vertices) {
                assert_eq!(posed.position, expected.position);
            }
            for draw in &reference.draw_calls {
                let material = &model.materials[usize::from(draw.material_idx)];
                for i in draw.vertex_range.clone() {
                    let expected = &reference.vertices[i];
                    let color = if expected.normal != [0.0; 3] {
                        fixed_light(material, expected.normal)
                    } else {
                        expected.color
                    };
                    for (a, b) in vertices[i].color.iter().zip(color) {
                        assert!((a - b).abs() < 1e-6);
                    }
                }
            }
        }
    }
}
