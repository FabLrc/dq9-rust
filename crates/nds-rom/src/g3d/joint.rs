//! `JNT0` joint animations (`BCA0` files, `.nsbca`): per-object translation, rotation and scale
//! curves, evaluated into [`ObjectTransform`]s and object matrices.
//!
//! Format from apicula `nitro::animation` (0BSD); evaluation ported from the NITRO-System
//! library as the game links it (USA decompilation `src/NitroSDK/NSBXX/JAC.cpp`):
//! `JACAnimationProcessingCallback` (EU `0x020b78cc`), `CalculateBoneMatrixRenderDataFromJAC`
//! (EU `0x020b7bb0`), the frame-aligned and smooth translation, scale and rotation samplers
//! (EU `0x020b7f64`..`0x020b89c0`) and `GetMatrixFromIndex` (EU `0x020b8d04`). Blending of
//! several joint animations (`ProcessJointAnimationsOnBoneMatrix`) is not ported.

use super::curve::{self, Pick, Samples, Smooth};
use super::fx::{Fx12, Mat4, normalize};
use super::model::{Model, pivot_mat};
use super::pose::ObjectTransform;
use super::{Name, at};
use crate::bytes::{u16_at, u32_at};
use crate::{Error, Result};

/// One joint animation (`J\0AC` block).
#[derive(Debug, Clone)]
pub struct JointAnimation<'a> {
    pub name: Name,
    /// Number of frames; times are clamped to `[0, frame_count)`.
    pub frame_count: u16,
    /// Bit 0: interpolate between frames; bit 1: the last frame blends into the first
    /// (looping). Bits 8.. **[unknown]**.
    pub flags: u32,
    pub tracks: Vec<JointTrack>,
    data: &'a [u8],
    base: usize,
    pivots: usize,
    bases: usize,
}

/// How one part (translation, rotation or scale) of an object is animated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel<T> {
    /// Identity.
    Absent,
    /// The object's rest-pose value.
    Rest,
    Animated(T),
}

/// A sampled curve: its metadata word and the offset of its samples in the `J\0AC` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurveRef {
    /// Bits 0-15 first frame (unused by the library), 16-28 last frame sampled at the reduced
    /// rate, 29 16-bit samples, 30-31 `log2` of the sampling period.
    pub info: u32,
    pub offset: u32,
}

impl CurveRef {
    fn rate(&self) -> u32 {
        self.info >> 30
    }
    fn end(&self) -> u32 {
        (self.info >> 16) & 0x1fff
    }
    fn short(&self) -> bool {
        self.info & 0x2000_0000 != 0
    }
}

/// A translation component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointValue {
    Constant(Fx12),
    Curve(CurveRef),
}

/// A rotation: index of a pivot (bit 15) or basis matrix of the block, or a curve of indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointRotation {
    Constant(u16),
    Curve(CurveRef),
}

/// A scale component and its inverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointScale {
    Constant([Fx12; 2]),
    Curve(CurveRef),
}

/// The curves of one object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JointTrack {
    /// Flag word: bits 0-13 which parts are present or constant, bits 24-31 the track index
    /// the library uses for the object at this track's position.
    pub tag: u32,
    pub translation: Channel<[JointValue; 3]>,
    pub rotation: Channel<JointRotation>,
    pub scale: Channel<[JointScale; 3]>,
}

impl JointTrack {
    /// Track the library reads for the object at this track's position
    /// (`InitializeModelAnimationFromJAC`, EU `0x020b7850`).
    pub fn data_index(&self) -> usize {
        (self.tag >> 24) as usize
    }
}

pub(crate) fn read_joint_animation(
    data: &[u8],
    base: usize,
    name: Name,
) -> Result<JointAnimation<'_>> {
    if data.get(base..base.saturating_add(4)) != Some(b"J\0AC") {
        return Err(Error::Malformed("G3D joint animation stamp"));
    }
    let frame_count = u16_at(data, at(base, 4u8)?)?;
    if frame_count == 0 {
        return Err(Error::Malformed("G3D joint animation without frames"));
    }
    let num_tracks = u16_at(data, at(base, 6u8)?)?;
    let flags = u32_at(data, at(base, 8u8)?)?;
    let pivots = at(base, u32_at(data, at(base, 12u8)?)?)?;
    let bases = at(base, u32_at(data, at(base, 16u8)?)?)?;
    let tracks = (0..u64::from(num_tracks))
        .map(|i| {
            let off = u16_at(data, at(base, 20 + 2 * i)?)?;
            read_track(data, at(base, off)?)
        })
        .collect::<Result<Vec<_>>>()?;
    if tracks.iter().any(|t| t.data_index() >= tracks.len()) {
        return Err(Error::Malformed("G3D joint track index out of range"));
    }
    Ok(JointAnimation {
        name,
        frame_count,
        flags,
        tracks,
        data,
        base,
        pivots,
        bases,
    })
}

/// Parses a track's channel layout (the walk of `CalculateBoneMatrixRenderDataFromJAC`).
fn read_track(data: &[u8], mut cur: usize) -> Result<JointTrack> {
    let tag = u32_at(data, cur)?;
    cur = at(cur, 4u8)?;
    let next = |cur: &mut usize| -> Result<u32> {
        let v = u32_at(data, *cur)?;
        *cur = at(*cur, 4u8)?;
        Ok(v)
    };
    let mut track = JointTrack {
        tag,
        translation: Channel::Absent,
        rotation: Channel::Absent,
        scale: Channel::Absent,
    };
    if tag & 1 != 0 {
        return Ok(track);
    }
    track.translation = if tag & 6 == 0 {
        let mut v = [JointValue::Constant(Fx12::ZERO); 3];
        for (i, v) in v.iter_mut().enumerate() {
            *v = if tag & (8 << i) != 0 {
                JointValue::Constant(Fx12::from_fx32(next(&mut cur)?))
            } else {
                let info = next(&mut cur)?;
                JointValue::Curve(CurveRef {
                    info,
                    offset: next(&mut cur)?,
                })
            };
        }
        Channel::Animated(v)
    } else if tag & 2 != 0 {
        Channel::Absent
    } else {
        Channel::Rest
    };
    track.rotation = if tag & 0xc0 == 0 {
        Channel::Animated(if tag & 0x100 != 0 {
            JointRotation::Constant(next(&mut cur)? as u16)
        } else {
            let info = next(&mut cur)?;
            JointRotation::Curve(CurveRef {
                info,
                offset: next(&mut cur)?,
            })
        })
    } else if tag & 0x40 != 0 {
        Channel::Absent
    } else {
        Channel::Rest
    };
    track.scale = if tag & 0x600 == 0 {
        let mut v = [JointScale::Constant([Fx12::ONE; 2]); 3];
        for (i, v) in v.iter_mut().enumerate() {
            let (a, b) = (next(&mut cur)?, next(&mut cur)?);
            *v = if tag & (0x800 << i) != 0 {
                JointScale::Constant([Fx12::from_fx32(a), Fx12::from_fx32(b)])
            } else {
                JointScale::Curve(CurveRef { info: a, offset: b })
            };
        }
        Channel::Animated(v)
    } else if tag & 0x200 != 0 {
        Channel::Absent
    } else {
        Channel::Rest
    };
    Ok(track)
}

/// `(m12 m23 - m13 m22, m13 m21 - m11 m23, m11 m22 - m12 m21)`: the third row from the first
/// two, in the library's 32-bit arithmetic.
fn cross_third_row(m: &mut [i32; 9]) {
    let p = |a: i32, b: i32| a.wrapping_mul(b);
    m[6] = p(m[1], m[5]).wrapping_sub(p(m[2], m[4])) >> 12;
    m[7] = p(m[2], m[3]).wrapping_sub(p(m[0], m[5])) >> 12;
    m[8] = p(m[0], m[4]).wrapping_sub(p(m[1], m[3])) >> 12;
}

fn normalize_row(m: &mut [i32; 9], row: usize) {
    let r = normalize([m[3 * row], m[3 * row + 1], m[3 * row + 2]]);
    m[3 * row..3 * row + 3].copy_from_slice(&r);
}

/// Finishes a blended rotation: rows 0 and 1 normalised, row 2 normalised from the blend or,
/// when a basis matrix took part, their cross product.
fn finish_blend(m: &mut [i32; 9], any_basis: bool) {
    normalize_row(m, 0);
    normalize_row(m, 1);
    if any_basis {
        cross_third_row(m);
    } else {
        normalize_row(m, 2);
    }
}

impl JointAnimation<'_> {
    pub fn interpolates(&self) -> bool {
        self.flags & 1 != 0
    }

    pub fn loops(&self) -> bool {
        self.flags & 2 != 0
    }

    fn samples(&self, curve: &CurveRef) -> Result<Samples<'_>> {
        Ok(Samples {
            data: self.data,
            base: at(self.base, curve.offset)?,
        })
    }

    /// `GetMatrixFromIndex`: a pivot (bit 15) or basis matrix; `true` for a basis matrix,
    /// whose third row is left zero.
    fn matrix(&self, index: u32) -> Result<([i32; 9], bool)> {
        let idx = u64::from(index & 0x7fff);
        if index & 0x8000 != 0 {
            let s = Samples {
                data: self.data,
                base: at(self.pivots, idx * 6)?,
            };
            let flags = s.u16(0)?;
            let form = u32::from(flags & 0xf);
            if form >= 9 {
                return Err(Error::Malformed("G3D joint pivot form"));
            }
            let (a, b) = (s.i16(1)?, s.i16(2)?);
            let m = pivot_mat(form, u32::from(flags >> 4) & 7, Fx12(a), Fx12(b));
            Ok((m.map(|v| v.0), false))
        } else {
            let s = Samples {
                data: self.data,
                base: at(self.bases, idx * 10)?,
            };
            let d: [i32; 5] = [s.i16(0)?, s.i16(1)?, s.i16(2)?, s.i16(3)?, s.i16(4)?];
            let mut m = [0i32; 9];
            let mut acc = 0i32;
            for (i, e) in [(4, 4), (0, 0), (1, 1), (2, 2), (3, 3)] {
                m[e] = d[i] >> 3;
                acc = (acc << 3) | (d[i] & 7);
            }
            m[5] = (acc << 19) >> 19;
            Ok((m, true))
        }
    }

    fn translation(&self, v: &JointValue, time: i32, smooth: bool) -> Result<i32> {
        let c = match v {
            JointValue::Constant(x) => return Ok(x.0),
            JointValue::Curve(c) => c,
        };
        let s = self.samples(c)?;
        let get = |i: usize| if c.short() { s.i16(i) } else { s.i32(i) };
        if smooth {
            return Ok(
                match curve::smooth(time, c.rate(), c.end(), self.frame_count, self.loops()) {
                    Smooth::Direct(i) => get(i)?,
                    Smooth::Lerp {
                        a,
                        b,
                        t,
                        premul,
                        shift,
                    } => curve::lerp(get(a)?, get(b)?, t, premul, shift),
                },
            );
        }
        Ok(match curve::pick((time >> 12) as u32, c.rate(), c.end()) {
            Pick::Direct(i) => get(i)?,
            Pick::Average(i) if c.short() => (get(i)? + get(i + 1)?) >> 1,
            Pick::Average(i) => (get(i)? >> 1) + (get(i + 1)? >> 1),
            Pick::Weighted { high, low } if c.short() => (get(high)? * 3 + get(low)?) >> 2,
            Pick::Weighted { high, low } => {
                ((i64::from(get(high)?) * 3 + i64::from(get(low)?)) >> 2) as i32
            }
        })
    }

    fn scale(&self, v: &JointScale, time: i32, smooth: bool) -> Result<[i32; 2]> {
        let c = match v {
            JointScale::Constant(x) => return Ok(x.map(|x| x.0)),
            JointScale::Curve(c) => c,
        };
        let s = self.samples(c)?;
        let get = |i: usize| {
            if c.short() {
                s.i16_pair(i)
            } else {
                s.i32_pair(i)
            }
        };
        if smooth {
            return Ok(
                match curve::smooth(time, c.rate(), c.end(), self.frame_count, self.loops()) {
                    Smooth::Direct(i) => get(i)?,
                    Smooth::Lerp {
                        a,
                        b,
                        t,
                        premul,
                        shift,
                    } => {
                        let (a, b) = (get(a)?, get(b)?);
                        [0, 1].map(|k| curve::lerp(a[k], b[k], t, premul, shift))
                    }
                },
            );
        }
        Ok(match curve::pick((time >> 12) as u32, c.rate(), c.end()) {
            Pick::Direct(i) => get(i)?,
            Pick::Average(i) => {
                let (a, b) = (get(i)?, get(i + 1)?);
                [0, 1].map(|k| a[k].wrapping_add(b[k]) >> 1)
            }
            Pick::Weighted { high, low } => {
                let (h, l) = (get(high)?, get(low)?);
                [0, 1].map(|k| {
                    if c.short() {
                        (h[k] * 3 + l[k]) >> 2
                    } else {
                        ((i64::from(h[k]) * 3 + i64::from(l[k])) >> 2) as i32
                    }
                })
            }
        })
    }

    fn rotation(&self, v: &JointRotation, time: i32, smooth: bool) -> Result<[i32; 9]> {
        let c = match v {
            JointRotation::Constant(index) => {
                let (mut m, basis) = self.matrix(u32::from(*index))?;
                if basis {
                    cross_third_row(&mut m);
                }
                return Ok(m);
            }
            JointRotation::Curve(c) => c,
        };
        let s = self.samples(c)?;
        let get = |i: usize| self.matrix(u32::from(s.u16(i)?));
        let exact = |i: usize| -> Result<[i32; 9]> {
            let (mut m, basis) = get(i)?;
            if basis {
                cross_third_row(&mut m);
            } else {
                normalize_row(&mut m, 2);
            }
            Ok(m)
        };
        // Blends `a * wa + b * wb` (the first two rows, and the third without basis matrix),
        // with `t`-interpolation for the smooth case.
        let blend = |a: usize, b: usize, f: &dyn Fn(i32, i32) -> i32| -> Result<[i32; 9]> {
            let ((ma, ba), (mb, bb)) = (get(a)?, get(b)?);
            let mut m = ma;
            let rows = if ba || bb { 6 } else { 9 };
            for k in 0..rows {
                m[k] = f(ma[k], mb[k]);
            }
            finish_blend(&mut m, ba || bb);
            Ok(m)
        };
        if smooth {
            return match curve::smooth(time, c.rate(), c.end(), self.frame_count, self.loops()) {
                Smooth::Direct(i) => exact(i),
                Smooth::Lerp {
                    a, b, t, premul, ..
                } => blend(a, b, &|x, y| {
                    x.wrapping_mul(premul)
                        .wrapping_add(t.wrapping_mul(y.wrapping_sub(x)) >> 12)
                }),
            };
        }
        match curve::pick((time >> 12) as u32, c.rate(), c.end()) {
            Pick::Direct(i) => exact(i),
            Pick::Average(i) => blend(i, i + 1, &|x, y| x.wrapping_add(y)),
            Pick::Weighted { high, low } => {
                blend(high, low, &|h, l| l.wrapping_add(h.wrapping_mul(3)))
            }
        }
    }

    /// The transform of the object driven by track `track_idx` at `time` (frames, 1.19.12),
    /// `rest` supplying the parts the track leaves at their rest pose.
    pub fn track_transform(
        &self,
        track_idx: usize,
        rest: &ObjectTransform,
        time: Fx12,
    ) -> Result<ObjectTransform> {
        let track = self
            .tracks
            .get(track_idx)
            .ok_or(Error::Malformed("G3D joint track out of range"))?;
        // JACAnimationProcessingCallback: clamp to the animation.
        let time = time.0.clamp(0, (i32::from(self.frame_count) << 12) - 1);
        let smooth = time & 0xfff != 0 && self.interpolates();
        let translation = match &track.translation {
            Channel::Absent => None,
            Channel::Rest => rest.translation,
            Channel::Animated(v) => {
                let mut out = [Fx12::ZERO; 3];
                for (o, v) in out.iter_mut().zip(v) {
                    *o = Fx12(self.translation(v, time, smooth)?);
                }
                Some(out)
            }
        };
        let rotation = match &track.rotation {
            Channel::Absent => None,
            Channel::Rest => rest.rotation,
            Channel::Animated(v) => Some(self.rotation(v, time, smooth)?.map(Fx12)),
        };
        let scale = match &track.scale {
            Channel::Absent => None,
            Channel::Rest => rest.scale,
            Channel::Animated(v) => {
                let mut s = [Fx12::ONE; 3];
                let mut inv = [Fx12::ONE; 3];
                for (k, v) in v.iter().enumerate() {
                    let [a, b] = self.scale(v, time, smooth)?;
                    (s[k], inv[k]) = (Fx12(a), Fx12(b));
                }
                Some((s, inv))
            }
        };
        Ok(ObjectTransform {
            translation,
            rotation,
            scale,
        })
    }

    /// Every object's transform at `time` (frames, 1.19.12): object `i` follows the track
    /// named by track `i`'s [`data_index`](JointTrack::data_index), objects past the last
    /// track keep their rest pose.
    pub fn transforms(&self, model: &Model, time: Fx12) -> Result<Vec<ObjectTransform>> {
        model
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let rest = ObjectTransform::rest(o);
                match self.tracks.get(i) {
                    Some(t) => self.track_transform(t.data_index(), &rest, time),
                    None => Ok(rest),
                }
            })
            .collect()
    }

    /// Object matrices at `time` (frames, 1.19.12) for
    /// [`Geometry::build_posed`](super::Geometry::build_posed), under the model's scaling rule.
    pub fn pose_at(&self, model: &Model, time: Fx12) -> Result<Vec<Mat4>> {
        model.pose_matrices(&self.transforms(model, time)?)
    }

    /// Object matrices at integer `frame`.
    pub fn pose(&self, model: &Model, frame: u16) -> Result<Vec<Mat4>> {
        self.pose_at(model, Fx12(i32::from(frame) << 12))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::g3d::G3dFile;
    use crate::g3d::test_util::{dict, file, section};

    /// A `J\0AC` block with one track: x translation sampled every frame (0, 1, 2, 3 as fx32),
    /// constant rotation (pivot 0: rotation about x by 90°), y scale every second frame.
    pub fn sample_jac(flags: u32) -> Vec<u8> {
        let mut b = b"J\0AC".to_vec();
        b.extend(4u16.to_le_bytes()); // frames
        b.extend(1u16.to_le_bytes()); // tracks
        b.extend(flags.to_le_bytes());
        let pivots = 24 + 4 + 8 + 4 + 4 + 4 + 8 + 4 * 4;
        b.extend((pivots as u32).to_le_bytes());
        b.extend(((pivots + 6) as u32).to_le_bytes());
        b.extend(24u16.to_le_bytes());
        b.extend([0, 0]);
        assert_eq!(b.len(), 24);
        // y, z translation constant; rotation constant; x, z scale constant.
        let tag: u32 = 0x10 | 0x20 | 0x100 | 0x800 | 0x2000;
        b.extend(tag.to_le_bytes());
        let samples = (pivots + 6 + 10) as u32;
        b.extend(((4u32 << 16) | 3).to_le_bytes()); // x translation: 32-bit, every frame
        b.extend(samples.to_le_bytes());
        b.extend((5i32 << 12).to_le_bytes()); // y
        b.extend(0i32.to_le_bytes()); // z
        b.extend(0x8000u32.to_le_bytes()); // rotation: pivot 0
        b.extend((1i32 << 12).to_le_bytes()); // x scale and inverse
        b.extend((1i32 << 12).to_le_bytes());
        // y scale: 16-bit pairs, every second frame up to frame 2, frame 3 alone.
        b.extend(((1u32 << 30) | (1 << 29) | (2 << 16)).to_le_bytes());
        b.extend((samples + 16).to_le_bytes());
        b.extend((1i32 << 12).to_le_bytes()); // z scale
        b.extend((1i32 << 12).to_le_bytes());
        assert_eq!(b.len(), pivots);
        // Pivot 0: form 0, a = cos 90° = 0, b = sin 90° = 1.
        b.extend(0u16.to_le_bytes());
        b.extend(0i16.to_le_bytes());
        b.extend(0x1000i16.to_le_bytes());
        b.extend([0u8; 10]); // basis 0 (unused)
        for x in 0..4i32 {
            b.extend((x << 12).to_le_bytes());
        }
        for s in [0x1000i16, 0x2000, 0x3000] {
            b.extend(s.to_le_bytes());
            b.extend(0x800i16.to_le_bytes());
        }
        b
    }

    pub fn bca(flags: u32) -> Vec<u8> {
        let jac = sample_jac(flags);
        let dict_len = dict(&[(&[0; 4], "anim")]).len();
        let body = [
            dict(&[(&((8 + dict_len) as u32).to_le_bytes(), "anim")]),
            jac,
        ]
        .concat();
        file(b"BCA0", &[section(b"JNT0", &body)])
    }

    fn rest() -> ObjectTransform {
        ObjectTransform::default()
    }

    #[test]
    fn evaluates_joint_curves() {
        let data = bca(0);
        let f = G3dFile::parse(&data).unwrap();
        let a = &f.joint_animations[0];
        assert_eq!(a.frame_count, 4);
        assert!(a.name.is("anim"));
        assert!(matches!(
            a.tracks[0].translation,
            Channel::Animated([JointValue::Curve(_), JointValue::Constant(Fx12(0x5000)), _])
        ));
        let at = |frame: i32| a.track_transform(0, &rest(), Fx12(frame)).unwrap();
        let t = at(2 << 12);
        assert_eq!(t.translation.unwrap().map(|v| v.0), [2 << 12, 5 << 12, 0]);
        let r = t.rotation.unwrap().map(|v| v.0);
        assert_eq!(r, [0x1000, 0, 0, 0, 0, 0x1000, 0, 0x1000, 0]);
        // y scale: frame 1 averages samples 0 and 1, frame 3 reads sample 2.
        assert_eq!(at(1 << 12).scale.unwrap().0[1].0, 0x1800);
        assert_eq!(at(3 << 12).scale.unwrap().0[1].0, 0x3000);
        assert_eq!(at(3 << 12).scale.unwrap().1[1].0, 0x800);
        // Without the interpolation flag a fractional time floors; past the end it clamps.
        assert_eq!(at(0x1800).translation.unwrap()[0].0, 1 << 12);
        assert_eq!(at(100 << 12).translation.unwrap()[0].0, 3 << 12);
    }

    #[test]
    fn interpolates_and_loops() {
        let data = bca(3);
        let f = G3dFile::parse(&data).unwrap();
        let a = &f.joint_animations[0];
        let x = |time: i32| {
            a.track_transform(0, &rest(), Fx12(time))
                .unwrap()
                .translation
                .unwrap()[0]
                .0
        };
        assert_eq!(x(0x1800), 0x1800);
        // The last frame blends back into frame 0 when looping.
        assert_eq!(x(0x3800), 0x1800);
        let s = a
            .track_transform(0, &rest(), Fx12(0x1800))
            .unwrap()
            .scale
            .unwrap();
        // Frame 1.5 between samples at 0 and 2 (every second frame).
        assert_eq!(s.0[1].0, 0x1c00);
    }

    #[test]
    fn rest_parts_and_poses() {
        let data = bca(0);
        let f = G3dFile::parse(&data).unwrap();
        let a = &f.joint_animations[0];
        let mdl = file(
            b"BMD0",
            &[crate::g3d::model::tests::sample_mdl0(
                &crate::g3d::model::tests::triangle_list(),
            )],
        );
        let m = G3dFile::parse(&mdl).unwrap();
        let model = &m.models[0];
        let pose = a.pose(model, 1).unwrap();
        assert_eq!(pose.len(), 1);
        assert_eq!(pose[0].transform_point([0.0; 3]), [1.0, 5.0, 0.0]);
        assert_eq!(a.transforms(model, Fx12(0)).unwrap().len(), 1);
    }

    #[test]
    fn basis_matrices() {
        // Rows (-1, 0, 0) and (0, -1, 0): entries are 13-bit values in the top bits, so +1
        // cannot be stored but -1 can.
        let mut b = vec![0u8; 10];
        b[0..2].copy_from_slice(&(-0x8000i16).to_le_bytes()); // d0 -> m0
        b[8..10].copy_from_slice(&(-0x8000i16).to_le_bytes()); // d4 -> m4
        fn anim(data: &[u8]) -> JointAnimation<'_> {
            JointAnimation {
                name: Name([0; 16]),
                frame_count: 1,
                flags: 0,
                tracks: vec![],
                data,
                base: 0,
                pivots: 0,
                bases: 0,
            }
        }
        let (mut m, basis) = anim(&b).matrix(0).unwrap();
        assert!(basis);
        cross_third_row(&mut m);
        assert_eq!(m, [-0x1000, 0, 0, 0, -0x1000, 0, 0, 0, 0x1000]);
        // The sixth entry gathers the low three bits of the five words.
        b[2..4].copy_from_slice(&7i16.to_le_bytes()); // d1
        assert_eq!(anim(&b).matrix(0).unwrap().0[5], 7 << 6);
        assert!(anim(&b).matrix(1).is_err());
    }

    #[test]
    fn truncated_animations_are_errors() {
        let data = bca(0);
        for len in 16..data.len() {
            let mut cut = data[..len].to_vec();
            cut[8..12].copy_from_slice(&(len as u32).to_le_bytes());
            if let Ok(f) = G3dFile::parse(&cut) {
                // Sample arrays are read lazily: evaluation reports the missing bytes.
                let a = &f.joint_animations[0];
                assert!(
                    (0..4).any(|fr| a.track_transform(0, &rest(), Fx12(fr << 12)).is_err()),
                    "len {len}"
                );
            }
        }
    }
}
