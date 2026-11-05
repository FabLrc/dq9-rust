//! Object transforms and the model's scaling rule: from each object's translation, rotation and
//! scale (rest pose or animated) to the matrix the render command `0x06` multiplies by.
//!
//! Ported from the NITRO-System library as the game links it (USA decompilation,
//! `src/NitroSDK/NSBXX/RenderCommandProcs.cpp`): `BoneMatrixScaleCalculationProc_Type0/1/2`
//! (EU `0x020b9a3c`, `0x020b9b40`, `0x020ba3a0`) and `BoneMatrixDataSubmissionProc_Type0/1/2`
//! (EU `0x020b99c0`, `0x020b9a7c`, `0x020ba274`).

use super::fx::{Fx12, Mat4, mul64};
use super::model::{Model, Object};
use crate::{Error, Result};

/// One object's transform before the scaling rule is applied (the library's
/// `BoneMatrixRenderData` inputs), exact fixed point. `None` parts are identities.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectTransform {
    pub translation: Option<[Fx12; 3]>,
    /// 3x3 matrix, column by column for column vectors (the DS row-major order).
    pub rotation: Option<[Fx12; 9]>,
    /// Scale and its inverse (the inverse only matters to the Maya and Softimage rules).
    pub scale: Option<([Fx12; 3], [Fx12; 3])>,
}

impl ObjectTransform {
    /// The rest pose of `object`.
    pub fn rest(object: &Object) -> ObjectTransform {
        ObjectTransform {
            translation: object.translation,
            rotation: object.rotation,
            scale: object
                .scale
                .map(|s| (s, object.inv_scale.unwrap_or([Fx12::ONE; 3]))),
        }
    }
}

/// Accumulated scale of an object for its children (Maya, Softimage rules): scale product and
/// inverse-scale product, or `None` while it is the identity.
type Accumulated = Option<([i32; 3], [i32; 3])>;

/// What the submission procedure sends for one object.
#[derive(Default)]
struct Submitted {
    /// Flags `0x08`/`0x10`: the parent's accumulated scale is the identity.
    parent_identity: bool,
    /// Flag `0x20` (Maya): compensate the parent's scale.
    compensate: bool,
    v1: [i32; 3],
    v2: [i32; 3],
}

fn fx3(v: [i32; 3]) -> [f32; 3] {
    v.map(|x| Fx12(x).to_f32())
}

fn raw3(v: [Fx12; 3]) -> [i32; 3] {
    v.map(|x| x.0)
}

/// `current * [R | T]` as the GPU's `MTX_MULT_4x3`, or `MTX_MULT_3x3` without translation.
fn rot_trans(r: Option<&[Fx12; 9]>, t: Option<[f32; 3]>) -> Mat4 {
    let mut m = r.map_or(Mat4::IDENTITY, Mat4::from_mat3);
    if let Some(t) = t {
        m.0[3][0] = t[0];
        m.0[3][1] = t[1];
        m.0[3][2] = t[2];
    }
    m
}

impl Model<'_> {
    /// The rest-pose object matrices under this model's scaling rule, for
    /// [`Geometry::build_posed`](super::Geometry::build_posed).
    pub fn rest_pose(&self) -> Result<Vec<Mat4>> {
        let rest: Vec<_> = self.objects.iter().map(ObjectTransform::rest).collect();
        self.pose_matrices(&rest)
    }

    /// Object matrices from one transform per object, applying the scaling rule along the
    /// object hierarchy in render command order (`scaling_rule` 0 standard `T R S`, 1 Maya
    /// segment scale compensation, 2 Softimage classic scale off). An object that no render
    /// command multiplies gets the standard `T R S`.
    pub fn pose_matrices(&self, transforms: &[ObjectTransform]) -> Result<Vec<Mat4>> {
        if transforms.len() != self.objects.len() {
            return Err(Error::Malformed(
                "G3D pose: one transform per object expected",
            ));
        }
        let mut out: Vec<Mat4> = transforms.iter().map(standard).collect();
        if self.scaling_rule == 0 {
            return Ok(out);
        }
        let n = self.objects.len();
        // Render handler state (`SetUpRenderCommandHandler`, EU 0x020b4078): object 0 starts
        // with an identity accumulated scale, the others with zeroes.
        let mut identity = vec![false; n];
        let mut acc = vec![([0i32; 3], [0i32; 3]); n];
        if let Some(first) = identity.first_mut() {
            *first = true;
        }
        for link in &self.object_links {
            let (this, parent) = (usize::from(link.object_idx), usize::from(link.parent_idx));
            let (Some(t), true) = (transforms.get(this), parent < n) else {
                return Err(Error::Malformed("G3D object link out of range"));
            };
            let mut s = Submitted::default();
            if self.scaling_rule == 1 {
                // BoneMatrixScaleCalculationProc_Type1.
                if link.flags & 2 != 0 {
                    match t.scale {
                        None => identity[this] = true,
                        Some((_, inv)) => {
                            identity[this] = false;
                            acc[this].1 = raw3(inv);
                        }
                    }
                }
                if link.flags & 1 != 0 {
                    s.compensate = true;
                    if identity[parent] {
                        s.parent_identity = true;
                    } else {
                        s.v1 = acc[parent].1;
                    }
                }
                out[this] = maya(t, &s);
            } else {
                // BoneMatrixScaleCalculationProc_Type2.
                let p: Accumulated = (!identity[parent]).then_some(acc[parent]);
                match (t.scale, p) {
                    (None, None) => {
                        identity[this] = true;
                        s.parent_identity = true;
                    }
                    (None, Some(p)) => {
                        acc[this] = p;
                        (s.v1, s.v2) = p;
                    }
                    (Some((sc, inv)), None) => {
                        acc[this] = (raw3(sc), raw3(inv));
                        identity[this] = false;
                        s.parent_identity = true;
                    }
                    (Some((sc, inv)), Some(p)) => {
                        identity[this] = false;
                        let (sc, inv) = (raw3(sc), raw3(inv));
                        acc[this] = (
                            std::array::from_fn(|i| mul64(sc[i], p.0[i])),
                            std::array::from_fn(|i| mul64(inv[i], p.1[i])),
                        );
                        // Copied after the update, as the library does (matters only for an
                        // object that is its own parent).
                        (s.v1, s.v2) = acc[parent];
                    }
                }
                out[this] = softimage(t, &s);
            }
        }
        Ok(out)
    }
}

/// `BoneMatrixDataSubmissionProc_Type0`: `T R S`.
fn standard(t: &ObjectTransform) -> Mat4 {
    let mut m = rot_trans(
        t.rotation.as_ref(),
        t.translation.map(|v| v.map(Fx12::to_f32)),
    );
    if let Some((s, _)) = t.scale {
        m = m.mul(&Mat4::scale(s.map(Fx12::to_f32)));
    }
    m
}

/// `BoneMatrixDataSubmissionProc_Type1`: `T S(parent⁻¹) R S` when compensating.
fn maya(t: &ObjectTransform, s: &Submitted) -> Mat4 {
    let mut trans = t.translation.map(|v| v.map(Fx12::to_f32));
    let mut m = Mat4::IDENTITY;
    if s.compensate && !s.parent_identity {
        if let Some(v) = trans.take() {
            m = Mat4::translation(v);
        }
        m = m.mul(&Mat4::scale(fx3(s.v1)));
    }
    m = m.mul(&rot_trans(t.rotation.as_ref(), trans));
    if let Some((sc, _)) = t.scale {
        m = m.mul(&Mat4::scale(sc.map(Fx12::to_f32)));
    }
    m
}

/// `BoneMatrixDataSubmissionProc_Type2`: `S(parent⁻¹) T(t ∘ parent) R S(parent) S`.
fn softimage(t: &ObjectTransform, s: &Submitted) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    let mut trans = t.translation.map(|v| v.map(Fx12::to_f32));
    if !s.parent_identity {
        m = Mat4::scale(fx3(s.v2));
        if let Some(v) = t.translation {
            let v = raw3(v);
            m = m.mul(&Mat4::translation(fx3(std::array::from_fn(|i| {
                mul64(v[i], s.v1[i])
            }))));
        }
        trans = None;
    }
    m = m.mul(&rot_trans(t.rotation.as_ref(), trans));
    if !s.parent_identity {
        m = m.mul(&Mat4::scale(fx3(s.v1)));
    }
    if let Some((sc, _)) = t.scale {
        m = m.mul(&Mat4::scale(sc.map(Fx12::to_f32)));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g3d::model::ObjectLink;

    fn fx(v: f32) -> Fx12 {
        Fx12((v * 4096.0) as i32)
    }

    fn transform(t: [f32; 3], s: Option<f32>) -> ObjectTransform {
        ObjectTransform {
            translation: Some(t.map(fx)),
            rotation: None,
            scale: s.map(|s| ([fx(s); 3], [fx(1.0 / s); 3])),
        }
    }

    fn chain(rule: u8, flags: u8) -> Vec<Mat4> {
        let data = crate::g3d::test_util::file(
            b"BMD0",
            &[crate::g3d::model::tests::sample_mdl0(
                &crate::g3d::model::tests::triangle_list(),
            )],
        );
        let file = crate::g3d::G3dFile::parse(&data).unwrap();
        let mut model = file.models[0].clone();
        model.scaling_rule = rule;
        model.objects = vec![model.objects[0].clone(); 2];
        model.object_links = vec![
            ObjectLink {
                object_idx: 0,
                parent_idx: 0,
                flags: 2,
            },
            ObjectLink {
                object_idx: 1,
                parent_idx: 0,
                flags,
            },
        ];
        let t = [
            transform([0.0; 3], Some(2.0)),
            transform([1.0, 0.0, 0.0], Some(3.0)),
        ];
        model.pose_matrices(&t).unwrap()
    }

    #[test]
    fn standard_rule_is_trs() {
        let m = chain(0, 0);
        let world = m[0].mul(&m[1]);
        // Parent scale 2 scales the child's translation and scale.
        assert_eq!(world.transform_point([1.0, 0.0, 0.0]), [8.0, 0.0, 0.0]);
    }

    #[test]
    fn maya_compensates_parent_scale() {
        let m = chain(1, 1);
        let world = m[0].mul(&m[1]);
        // Translation scaled by the parent, the child's own scale undone from the parent's.
        assert_eq!(world.transform_point([1.0, 0.0, 0.0]), [5.0, 0.0, 0.0]);
        // Without the compensate flag it is the standard rule.
        let m = chain(1, 0);
        assert_eq!(
            m[0].mul(&m[1]).transform_point([1.0, 0.0, 0.0]),
            [8.0, 0.0, 0.0]
        );
    }

    #[test]
    fn softimage_scales_in_local_space() {
        let m = chain(2, 0);
        let world = m[0].mul(&m[1]);
        // Parent scale undone, translation scaled by the parent's accumulated scale and
        // the child scaled by both.
        let p = world.transform_point([1.0, 0.0, 0.0]);
        assert!((p[0] - 8.0).abs() < 1e-2, "{p:?}");
        assert_eq!(chain(2, 0)[0], chain(0, 0)[0]);
    }

    #[test]
    fn rejects_mismatched_transforms() {
        let data = crate::g3d::test_util::file(
            b"BMD0",
            &[crate::g3d::model::tests::sample_mdl0(
                &crate::g3d::model::tests::triangle_list(),
            )],
        );
        let file = crate::g3d::G3dFile::parse(&data).unwrap();
        assert!(file.models[0].pose_matrices(&[]).is_err());
        assert_eq!(file.models[0].rest_pose().unwrap().len(), 1);
    }
}
