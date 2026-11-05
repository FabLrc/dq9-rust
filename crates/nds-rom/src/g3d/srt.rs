//! Texture matrices: a material's texture scale, rotation and translation (`TexSrt`), static
//! or animated by an `SRT0` section (`BTA0` files, `.nsbta`), turned into the GPU texture
//! matrix ([`TexMatrix`]) the way the NITRO-System library builds it.
//!
//! Ported from the library as the game links it (USA decompilation,
//! `src/NitroSDK/NSBXX/MAT.cpp` and `RenderCommandProcs.cpp`): `SampleScalarFromMATTrack`,
//! `SampleSinCosFromMATTrack`, `CalculateTextureTransformFromMAT`,
//! `MATAnimationProcessingCallback` (EU `0x020b9288`, `0x020b9388`, `0x020b9494`,
//! `0x020b9668`) and `MaterialTextureMatrixLoadProc_Type0..3` with their
//! `CreateTextureMatrix_v*` builders (EU `0x020ba12c`, `0x020ba5bc`, `0x020bac84`,
//! `0x020bb2ac`). Each builder variant is the general formula with identity scale, rotation or
//! translation, so one formula per mode gives the same fixed-point result.

use super::curve::{self, Pick, Samples};
use super::fx::{Fx12, fx_div, mul64};
use super::model::{Material, Model};
use super::{Name, at, read_dict};
use crate::bytes::{u16_at, u32_at};
use crate::{Error, Result};

/// Texture scale, rotation (`sin`, `cos`) and translation of a material; `None` parts are
/// identities.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TexSrt {
    pub scale: Option<[Fx12; 2]>,
    /// `[sin, cos]` of the angle.
    pub rotation: Option<[Fx12; 2]>,
    /// In texture sizes.
    pub translation: Option<[Fx12; 2]>,
}

/// The six meaningful entries of a GPU texture matrix in "texture coordinate source" mode:
/// `s' = a s + c t + e / 16`, `t' = b s + d t + f / 16`, with `s`, `t` in texels (GBATEK "DS
/// 3D Texture Coordinates"). Entries are 1.19.12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexMatrix {
    pub a: Fx12,
    pub b: Fx12,
    pub c: Fx12,
    pub d: Fx12,
    pub e: Fx12,
    pub f: Fx12,
}

impl TexMatrix {
    pub const IDENTITY: TexMatrix = TexMatrix {
        a: Fx12::ONE,
        b: Fx12::ZERO,
        c: Fx12::ZERO,
        d: Fx12::ONE,
        e: Fx12::ZERO,
        f: Fx12::ZERO,
    };

    /// The matrix the library loads for `srt` on a `width` x `height` material whose texture
    /// is scaled by `mag` (`magW`, `magH`), under the model's texture matrix mode
    /// (`tex_mtx_mode`: 0 Maya, 1 Softimage 3D, 2 3ds Max, 3 Softimage XSI). `None` for an
    /// unknown mode.
    pub fn new(mode: u8, width: u16, height: u16, mag: [Fx12; 2], srt: &TexSrt) -> Option<Self> {
        let [sx, sy] = srt.scale.map_or([0x1000; 2], |s| s.map(|v| v.0));
        let [sin, cos] = srt.rotation.map_or([0, 0x1000], |r| r.map(|v| v.0));
        let [tx, ty] = srt.translation.map_or([0; 2], |t| t.map(|v| v.0));
        let (w, h) = (i32::from(width), i32::from(height));
        let m = |a: i32, b: i32| a.wrapping_mul(b);
        let shr8 = |a: i32, b: i32| ((i64::from(a) * i64::from(b)) >> 8) as i32;
        let (sxc, sxs, syc, sys) = (
            mul64(sx, cos),
            mul64(sx, sin),
            mul64(sy, cos),
            mul64(sy, sin),
        );
        let aspect = || fx_div(w << 12, h << 12);
        let aspect_hw = fx_div(h << 12, w << 12);
        let [a, b, c, d, e, f] = match mode {
            // CreateTextureMatrix_v0_RotateTranslateScale (EU 0x020b9c90).
            0 => [
                sxc,
                m(sys.wrapping_neg(), aspect_hw) >> 12,
                m(sxs, aspect()) >> 12,
                syc,
                m(m(w, sx.wrapping_sub(sxs.wrapping_add(sxc))), 8).wrapping_sub(m(w, shr8(sx, tx))),
                m(
                    m(
                        h,
                        sys.wrapping_sub(syc).wrapping_sub(sy).wrapping_add(0x2000),
                    ),
                    8,
                )
                .wrapping_add(m(h, shr8(sy, ty))),
            ],
            // MaterialTextureMatrixLoadProc_Type1 (EU 0x020ba5bc): translation then scale.
            1 => [
                sx,
                0,
                0,
                sy,
                m(w, shr8(sx, tx).wrapping_neg()),
                m(h, shr8(sy, ty).wrapping_neg()),
            ],
            // CreateTextureMatrix_v2_TranslateRotateScale (EU 0x020ba788).
            2 => {
                let stx = m(w.wrapping_neg(), 1 << 11).wrapping_sub(m(tx, w));
                let sty = m(h.wrapping_neg(), 1 << 11).wrapping_add(m(ty, h));
                let wide = |p: i32, q: i32, r: i32, s: i32| {
                    ((i64::from(p) * i64::from(q) + i64::from(r) * i64::from(s)) >> 8) as i32
                };
                [
                    sxc,
                    m(sys, aspect_hw) >> 12,
                    m(sxs.wrapping_neg(), aspect()) >> 12,
                    syc,
                    wide(sxc, stx, sxs.wrapping_neg(), sty).wrapping_add(m(w, 8 << 12)),
                    wide(sys, stx, syc, sty).wrapping_add(m(h, 8 << 12)),
                ]
            }
            // CreateTextureMatrix_v3_ScaleTranslateRotate (EU 0x020badcc).
            3 => {
                let (tx, ty, s, co) =
                    (i64::from(tx), i64::from(ty), i64::from(sin), i64::from(cos));
                let rty = (tx * s - ty * co) >> 12;
                let rtx = (tx * co + ty * s) >> 12;
                let m42 = syc
                    .wrapping_add(((rty * i64::from(sy)) >> 12) as i32)
                    .wrapping_sub(0x1000);
                let m41 = sxs.wrapping_sub(((rtx * i64::from(sx)) >> 12) as i32);
                [
                    sxc,
                    m(sys, aspect_hw) >> 12,
                    m(sxs.wrapping_neg(), aspect()) >> 12,
                    syc,
                    m(m(w, m41), 16),
                    m(m(h.wrapping_neg(), m42), 16),
                ]
            }
            _ => return None,
        };
        let mut out = TexMatrix {
            a: Fx12(a),
            b: Fx12(b),
            c: Fx12(c),
            d: Fx12(d),
            e: Fx12(e),
            f: Fx12(f),
        };
        // The texture's own magnification: the first row and the s translation by magW, the
        // second row and the t translation by magH.
        let [mw, mh] = mag.map(|v| v.0);
        if mw != 0x1000 {
            if mode == 1 {
                (out.a, out.e) = (Fx12(mul64(mw, out.a.0)), Fx12(mul64(mw, out.e.0)));
            } else {
                for v in [&mut out.a, &mut out.b, &mut out.e] {
                    v.0 = mul64(mw, v.0);
                }
            }
        }
        if mh != 0x1000 {
            if mode == 1 {
                (out.d, out.f) = (Fx12(mul64(mh, out.d.0)), Fx12(mul64(mh, out.f.0)));
            } else {
                for v in [&mut out.c, &mut out.d, &mut out.f] {
                    v.0 = mul64(mh, v.0);
                }
            }
        }
        Some(out)
    }

    /// Transforms texture coordinates in texels.
    pub fn apply(&self, [s, t]: [f32; 2]) -> [f32; 2] {
        let f = |v: Fx12| v.to_f32();
        [
            f(self.a) * s + f(self.c) * t + f(self.e) / 16.0,
            f(self.b) * s + f(self.d) * t + f(self.f) / 16.0,
        ]
    }

    /// The same transform for coordinates normalised by the texture size `w` x `h` (as in
    /// [`Vertex::texcoord`](super::Vertex::texcoord)): rows of `[u', v'] = M [u, v, 1]`.
    pub fn normalized(&self, w: f32, h: f32) -> [[f32; 3]; 2] {
        let f = |v: Fx12| v.to_f32();
        [
            [f(self.a), f(self.c) * h / w, f(self.e) / 16.0 / w],
            [f(self.b) * w / h, f(self.d), f(self.f) / 16.0 / h],
        ]
    }
}

impl Material {
    /// The material's own texture transform (`flags` bit 0), if any.
    pub fn tex_srt(&self) -> Option<TexSrt> {
        (self.flags & 1 != 0).then_some(TexSrt {
            scale: self.tex_scale,
            rotation: self.tex_rotation,
            translation: self.tex_translation,
        })
    }

    /// The texture matrix the library loads for `srt` on this material (`MaterialBindProc`,
    /// EU `0x020b45f0`: material size and magnification, model's `tex_mtx_mode`).
    pub fn tex_matrix(&self, tex_mtx_mode: u8, srt: &TexSrt) -> Option<TexMatrix> {
        TexMatrix::new(
            tex_mtx_mode,
            self.width,
            self.height,
            [self.mag_w, self.mag_h],
            srt,
        )
    }
}

impl Model<'_> {
    /// Each material's static texture matrix (`None` without one), for
    /// [`Geometry::apply_tex_matrices`](super::Geometry::apply_tex_matrices).
    pub fn tex_matrices(&self) -> Vec<Option<TexMatrix>> {
        self.materials
            .iter()
            .map(|m| {
                m.tex_srt()
                    .and_then(|s| m.tex_matrix(self.tex_mtx_mode, &s))
            })
            .collect()
    }
}

/// One curve of a texture SRT track: a constant (bit 29 of `info`, value in `value`) or
/// samples at `value` in the `M\0AT` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SrtCurve {
    /// Bits 0-15 last frame sampled at the reduced rate, 28 16-bit samples, 29 constant,
    /// 30-31 `log2` of the sampling period.
    pub info: u32,
    pub value: u32,
}

/// The curves of one material: scale s, scale t, rotation (`sin`, `cos` pairs), translation
/// s, translation t.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TexSrtTrack {
    pub material: Name,
    pub curves: [SrtCurve; 5],
}

/// One texture SRT animation (`M\0AT` block).
#[derive(Debug, Clone)]
pub struct TexSrtAnimation<'a> {
    pub name: Name,
    pub frame_count: u16,
    /// Header word **[unknown]**: `0x300` with bits 0-1 like the joint animation flags.
    pub flags: u16,
    pub tracks: Vec<TexSrtTrack>,
    data: &'a [u8],
    base: usize,
}

pub(crate) fn read_tex_srt_animation(
    data: &[u8],
    base: usize,
    name: Name,
) -> Result<TexSrtAnimation<'_>> {
    if data.get(base..base.saturating_add(4)) != Some(b"M\0AT") {
        return Err(Error::Malformed("G3D texture SRT animation stamp"));
    }
    let frame_count = u16_at(data, at(base, 4u8)?)?;
    if frame_count == 0 {
        return Err(Error::Malformed("G3D texture SRT animation without frames"));
    }
    let tracks = read_dict(data, at(base, 8u8)?, 40)?
        .into_iter()
        .map(|(v, material)| {
            let mut curves = [SrtCurve { info: 0, value: 0 }; 5];
            for (i, c) in curves.iter_mut().enumerate() {
                *c = SrtCurve {
                    info: u32_at(v, 8 * i)?,
                    value: u32_at(v, 8 * i + 4)?,
                };
            }
            Ok(TexSrtTrack { material, curves })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(TexSrtAnimation {
        name,
        frame_count,
        flags: u16_at(data, at(base, 6u8)?)?,
        tracks,
        data,
        base,
    })
}

impl TexSrtAnimation<'_> {
    /// The track animating the material named `material` (the library matches by name).
    pub fn track(&self, material: &Name) -> Option<usize> {
        self.tracks.iter().position(|t| &t.material == material)
    }

    fn samples(&self, c: &SrtCurve) -> Result<Samples<'_>> {
        Ok(Samples {
            data: self.data,
            base: at(self.base, c.value)?,
        })
    }

    /// `SampleScalarFromMATTrack`.
    fn scalar(&self, c: &SrtCurve, frame: u32) -> Result<i32> {
        if c.info & 0x2000_0000 != 0 {
            return Ok(c.value as i32);
        }
        let s = self.samples(c)?;
        let get = |i: usize| {
            if c.info & 0x1000_0000 != 0 {
                s.i16(i)
            } else {
                s.i32(i)
            }
        };
        Ok(match curve::pick(frame, c.info >> 30, c.info & 0xffff) {
            Pick::Direct(i) => get(i)?,
            Pick::Average(i) => get(i)?.wrapping_add(get(i + 1)?) >> 1,
            Pick::Weighted { high, low } => get(high)?.wrapping_mul(3).wrapping_add(get(low)?) >> 2,
        })
    }

    /// `SampleSinCosFromMATTrack`: `[sin, cos]`.
    fn sin_cos(&self, c: &SrtCurve, frame: u32) -> Result<[i32; 2]> {
        let split = |v: u32| {
            [
                i32::from(v as u16 as i16),
                i32::from((v >> 16) as u16 as i16),
            ]
        };
        if c.info & 0x2000_0000 != 0 {
            return Ok(split(c.value));
        }
        let s = self.samples(c)?;
        Ok(match curve::pick(frame, c.info >> 30, c.info & 0xffff) {
            Pick::Direct(i) => s.i16_pair(i)?,
            Pick::Average(i) => {
                let (a, b) = (s.i16_pair(i)?, s.i16_pair(i + 1)?);
                // Repacked into 16-bit halves.
                split(((a[0] + b[0]) >> 1) as u16 as u32 | (((a[1] + b[1]) >> 1) as u32) << 16)
            }
            Pick::Weighted { high, low } => {
                let (h, l) = (s.i16_pair(high)?, s.i16_pair(low)?);
                split(
                    ((h[0] * 3 + l[0]) >> 2) as u16 as u32
                        | (((h[1] * 3 + l[1]) >> 2) as u32) << 16,
                )
            }
        })
    }

    /// The transform of track `track_idx` at `frame` (clamped to the last frame).
    pub fn srt(&self, track_idx: usize, frame: u16) -> Result<TexSrt> {
        let track = self
            .tracks
            .get(track_idx)
            .ok_or(Error::Malformed("G3D texture SRT track out of range"))?;
        let frame = u32::from(frame.min(self.frame_count - 1));
        let c = &track.curves;
        let fx = |c: &SrtCurve| self.scalar(c, frame).map(Fx12);
        Ok(TexSrt {
            scale: Some([fx(&c[0])?, fx(&c[1])?]),
            rotation: Some(self.sin_cos(&c[2], frame)?.map(Fx12)),
            translation: Some([fx(&c[3])?, fx(&c[4])?]),
        })
    }

    /// The texture matrix of material `material_idx` of `model` at `frame`, or `None` when no
    /// track animates it. The animated transform replaces the material's own.
    pub fn matrix(
        &self,
        model: &Model,
        material_idx: usize,
        frame: u16,
    ) -> Result<Option<TexMatrix>> {
        let material = model
            .materials
            .get(material_idx)
            .ok_or(Error::Malformed("G3D material index out of range"))?;
        let Some(track) = self.track(&material.name) else {
            return Ok(None);
        };
        let srt = self.srt(track, frame)?;
        Ok(material.tex_matrix(model.tex_mtx_mode, &srt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g3d::G3dFile;
    use crate::g3d::test_util::{dict, file, section};

    fn identity_srt(mode: u8) -> TexMatrix {
        TexMatrix::new(mode, 32, 16, [Fx12::ONE; 2], &TexSrt::default()).unwrap()
    }

    #[test]
    fn identity_in_every_mode() {
        for mode in 0..4 {
            assert_eq!(identity_srt(mode), TexMatrix::IDENTITY, "mode {mode}");
        }
        assert!(TexMatrix::new(4, 1, 1, [Fx12::ONE; 2], &TexSrt::default()).is_none());
    }

    #[test]
    fn translation_scrolls_by_texture_sizes() {
        let srt = TexSrt {
            translation: Some([Fx12(0x800), Fx12(0x400)]),
            ..TexSrt::default()
        };
        let m = TexMatrix::new(3, 32, 16, [Fx12::ONE; 2], &srt).unwrap();
        // Half a width left, a quarter height down.
        assert_eq!(m.apply([0.0, 0.0]), [-16.0, 4.0]);
        let n = m.normalized(32.0, 16.0);
        assert_eq!(n[0][2], -0.5);
        assert_eq!(n[1][2], 0.25);
        let m1 = TexMatrix::new(1, 32, 16, [Fx12::ONE; 2], &srt).unwrap();
        assert_eq!(m1.apply([0.0, 0.0]), [-16.0, -4.0]);
    }

    #[test]
    fn mode3_scales_about_the_bottom_left() {
        let srt = TexSrt {
            scale: Some([Fx12(0x2000), Fx12(0x2000)]),
            ..TexSrt::default()
        };
        let m = TexMatrix::new(3, 32, 16, [Fx12::ONE; 2], &srt).unwrap();
        assert_eq!(m.apply([0.0, 16.0]), [0.0, 16.0]);
        assert_eq!(m.apply([1.0, 15.0]), [2.0, 14.0]);
        // 90° rotation, square texture: about the bottom-left corner too.
        let srt = TexSrt {
            rotation: Some([Fx12::ONE, Fx12::ZERO]),
            ..TexSrt::default()
        };
        let m = TexMatrix::new(3, 16, 16, [Fx12::ONE; 2], &srt).unwrap();
        assert_eq!(m.apply([0.0, 16.0]), [0.0, 16.0]);
    }

    /// `M\0AT` block: 6 frames, one track "mat": scale s constant 2, scale t 16-bit every
    /// frame, rotation constant identity, translation s 32-bit every second frame, t constant.
    fn bta() -> Vec<u8> {
        let track_dict_len = dict(&[(&[0; 40], "mat")]).len();
        let samples = 8 + track_dict_len;
        let mut track = Vec::new();
        for (info, value) in [
            (0x2000_0000u32, 0x2000u32),
            (0x1000_0000 | 6, samples as u32),
            (0x2000_0000, 0x1000_0000),
            (0x4000_0000 | 4, (samples + 12) as u32),
            (0x2000_0000, 0),
        ] {
            track.extend(info.to_le_bytes());
            track.extend(value.to_le_bytes());
        }
        let mut b = b"M\0AT".to_vec();
        b.extend(6u16.to_le_bytes());
        b.extend(0x300u16.to_le_bytes());
        b.extend(dict(&[(&track, "mat")]));
        for s in [0x1000i16, 0x1100, 0x1200, 0x1300, 0x1400, 0x1500] {
            b.extend(s.to_le_bytes());
        }
        for t in [0i32, 0x200, 0x400, 0x500] {
            b.extend(t.to_le_bytes());
        }
        let dict_len = dict(&[(&[0; 4], "srt")]).len();
        let body = [dict(&[(&((8 + dict_len) as u32).to_le_bytes(), "srt")]), b].concat();
        file(b"BTA0", &[section(b"SRT0", &body)])
    }

    #[test]
    fn evaluates_srt_tracks() {
        let data = bta();
        let f = G3dFile::parse(&data).unwrap();
        let a = &f.tex_srt_animations[0];
        assert_eq!(a.frame_count, 6);
        assert_eq!(a.track(&a.tracks[0].material), Some(0));
        let srt = a.srt(0, 3).unwrap();
        assert_eq!(srt.scale.unwrap().map(|v| v.0), [0x2000, 0x1300]);
        assert_eq!(srt.rotation.unwrap().map(|v| v.0), [0, 0x1000]);
        // Frame 3 averages samples 1 and 2; frame 5 (past frame 4) reads sample 3.
        assert_eq!(srt.translation.unwrap()[0].0, 0x300);
        assert_eq!(a.srt(0, 5).unwrap().translation.unwrap()[0].0, 0x500);
        assert_eq!(a.srt(0, 99).unwrap(), a.srt(0, 5).unwrap());
        assert!(a.srt(1, 0).is_err());
        for len in 16..data.len() {
            let mut cut = data[..len].to_vec();
            cut[8..12].copy_from_slice(&(len as u32).to_le_bytes());
            if let Ok(f) = G3dFile::parse(&cut) {
                let a = &f.tex_srt_animations[0];
                assert!((0..6).any(|fr| a.srt(0, fr).is_err()), "len {len}");
            }
        }
    }
}
