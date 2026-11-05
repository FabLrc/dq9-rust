//! `MAT0` material colour animations (`BMA0` files, `.nsbma`): diffuse, ambient, specular,
//! emission colours and polygon alpha of materials over time.
//!
//! Not in apicula; ported from the NITRO-System library as the game links it (USA
//! decompilation `src/NitroSDK/NSBXX/MAM.cpp`): `GetColorFromMaterialAnimation`,
//! `GetAlphaFromMaterialAnimation` and `MAMAnimationProcessingCallback` (EU `0x020b8e70`,
//! `0x020b8fd8`, `0x020b9198`).

use super::curve::{self, Pick, Samples};
use super::model::Material;
use super::{Name, at, read_dict};
use crate::bytes::{u16_at, u32_at};
use crate::{Error, Result};

/// The curves of one material: diffuse, ambient, specular, emission (RGB555) and alpha
/// (0..=31). Each word: bits 0-15 sample offset in the `M\0AM` block or the constant value,
/// 16-28 last frame sampled at the reduced rate, 29 constant, 30-31 `log2` of the sampling
/// period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialTrack {
    pub material: Name,
    pub curves: [u32; 5],
}

/// Material colours at one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialColors {
    pub diffuse: u16,
    pub ambient: u16,
    pub specular: u16,
    pub emission: u16,
    /// 0..=31.
    pub alpha: u8,
}

impl MaterialColors {
    /// Writes the colours into `material`'s GPU words as the library does, keeping the
    /// "diffuse is vertex colour" and "shininess table" bits.
    pub fn apply(&self, material: &mut Material) {
        material.dif_amb =
            u32::from(self.diffuse) | u32::from(self.ambient) << 16 | (material.dif_amb & 0x8000);
        material.spe_emi =
            u32::from(self.specular) | u32::from(self.emission) << 16 | (material.spe_emi & 0x8000);
        material.polygon_attr = (material.polygon_attr & !0x1f_0000) | u32::from(self.alpha) << 16;
    }
}

/// One material colour animation (`M\0AM` block).
#[derive(Debug, Clone)]
pub struct MaterialAnimation<'a> {
    pub name: Name,
    pub frame_count: u16,
    /// Header word **[unknown]**: bits 0-1 like the joint animation flags.
    pub flags: u16,
    pub tracks: Vec<MaterialTrack>,
    data: &'a [u8],
    base: usize,
}

pub(crate) fn read_material_animation(
    data: &[u8],
    base: usize,
    name: Name,
) -> Result<MaterialAnimation<'_>> {
    if data.get(base..base.saturating_add(4)) != Some(b"M\0AM") {
        return Err(Error::Malformed("G3D material animation stamp"));
    }
    let frame_count = u16_at(data, at(base, 4u8)?)?;
    if frame_count == 0 {
        return Err(Error::Malformed("G3D material animation without frames"));
    }
    let tracks = read_dict(data, at(base, 8u8)?, 20)?
        .into_iter()
        .map(|(v, material)| {
            let mut curves = [0u32; 5];
            for (i, c) in curves.iter_mut().enumerate() {
                *c = u32_at(v, 4 * i)?;
            }
            Ok(MaterialTrack { material, curves })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(MaterialAnimation {
        name,
        frame_count,
        flags: u16_at(data, at(base, 6u8)?)?,
        tracks,
        data,
        base,
    })
}

impl MaterialAnimation<'_> {
    /// The track animating the material named `material` (the library matches by name).
    pub fn track(&self, material: &Name) -> Option<usize> {
        self.tracks.iter().position(|t| &t.material == material)
    }

    fn samples(&self, word: u32) -> Result<Samples<'_>> {
        Ok(Samples {
            data: self.data,
            base: at(self.base, word & 0xffff)?,
        })
    }

    /// `GetColorFromMaterialAnimation`: RGB555 blends are per channel.
    fn color(&self, word: u32, frame: u32) -> Result<u16> {
        if word & 0x2000_0000 != 0 {
            return Ok(word as u16);
        }
        let s = self.samples(word)?;
        const RB: u32 = 0x7c1f;
        const G: u32 = 0x3e0;
        Ok(
            match curve::pick(frame, word >> 30, (word >> 16) & 0x1fff) {
                Pick::Direct(i) => s.u16(i)?,
                Pick::Average(i) => {
                    let (a, b) = (u32::from(s.u16(i)?), u32::from(s.u16(i + 1)?));
                    ((((a & RB) + (b & RB)) >> 1) & RB | (((a & G) + (b & G)) >> 1) & G) as u16
                }
                Pick::Weighted { high, low } => {
                    let (h, l) = (u32::from(s.u16(high)?), u32::from(s.u16(low)?));
                    let rb = (h & RB) * 3 + (l & RB);
                    let g = (h & G) * 3 + (l & G);
                    ((rb >> 2) & RB | (g >> 2) & G) as u16
                }
            },
        )
    }

    /// `GetAlphaFromMaterialAnimation`.
    fn alpha(&self, word: u32, frame: u32) -> Result<u8> {
        if word & 0x2000_0000 != 0 {
            return Ok(word as u8);
        }
        let s = self.samples(word)?;
        let get = |i: usize| s.u8(i).map(u32::from);
        Ok(
            match curve::pick(frame, word >> 30, (word >> 16) & 0x1fff) {
                Pick::Direct(i) => get(i)?,
                Pick::Average(i) => (get(i)? + get(i + 1)?) >> 1,
                Pick::Weighted { high, low } => (get(high)? * 3 + get(low)?) >> 2,
            } as u8,
        )
    }

    /// The colours of track `track_idx` at `frame` (clamped to the last frame).
    pub fn colors(&self, track_idx: usize, frame: u16) -> Result<MaterialColors> {
        let track = self
            .tracks
            .get(track_idx)
            .ok_or(Error::Malformed("G3D material track out of range"))?;
        let frame = u32::from(frame.min(self.frame_count - 1));
        let [dif, amb, spe, emi, alpha] = track.curves;
        Ok(MaterialColors {
            diffuse: self.color(dif, frame)?,
            ambient: self.color(amb, frame)?,
            specular: self.color(spe, frame)?,
            emission: self.color(emi, frame)?,
            alpha: self.alpha(alpha, frame)?,
        })
    }

    /// The colours of `material` at `frame`, or `None` when no track animates it.
    pub fn material_colors(
        &self,
        material: &Material,
        frame: u16,
    ) -> Result<Option<MaterialColors>> {
        self.track(&material.name)
            .map(|t| self.colors(t, frame))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g3d::G3dFile;
    use crate::g3d::test_util::{dict, file, section};

    /// 5 frames; diffuse sampled every fourth frame (frames 0, 4), the rest constant except
    /// alpha sampled every frame.
    fn bma() -> Vec<u8> {
        let track_dict_len = dict(&[(&[0; 20], "mat")]).len();
        let samples = (8 + track_dict_len) as u32;
        let words = [
            (2 << 30) | (4 << 16) | samples,
            0x2000_0000 | 0x7fff,
            0x2000_0000 | 0x001f,
            0x2000_0000,
            (5 << 16) | (samples + 4),
        ];
        let track: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let mut b = b"M\0AM".to_vec();
        b.extend(5u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(dict(&[(&track, "mat")]));
        // Diffuse samples: black then white (0x7fff).
        b.extend(0u16.to_le_bytes());
        b.extend(0x7fffu16.to_le_bytes());
        b.extend([31, 20, 10, 5, 0]);
        let dict_len = dict(&[(&[0; 4], "col")]).len();
        let body = [dict(&[(&((8 + dict_len) as u32).to_le_bytes(), "col")]), b].concat();
        file(b"BMA0", &[section(b"MAT0", &body)])
    }

    #[test]
    fn evaluates_colors() {
        let data = bma();
        let f = G3dFile::parse(&data).unwrap();
        let a = &f.material_animations[0];
        assert_eq!((a.frame_count, a.flags), (5, 1));
        let c = a.colors(0, 1).unwrap();
        // Frame 1: 3/4 black + 1/4 white per channel: 31 / 4 = 7.
        assert_eq!(c.diffuse, 7 | 7 << 5 | 7 << 10);
        assert_eq!(a.colors(0, 2).unwrap().diffuse, 15 | 15 << 5 | 15 << 10);
        assert_eq!(a.colors(0, 3).unwrap().diffuse, 23 | 23 << 5 | 23 << 10);
        assert_eq!(a.colors(0, 4).unwrap().diffuse, 0x7fff);
        assert_eq!(
            (c.ambient, c.specular, c.emission, c.alpha),
            (0x7fff, 0x1f, 0, 20)
        );
        assert_eq!(a.colors(0, 400).unwrap().alpha, 0);
        let mut m = crate::g3d::model::tests::sample_material();
        m.dif_amb = 0x8000;
        c.apply(&mut m);
        assert_eq!(m.dif_amb, 0x7fff << 16 | 0x8000 | u32::from(c.diffuse));
        assert_eq!(m.alpha(), 20);
        assert_eq!(
            a.material_colors(&m, 0).unwrap(),
            Some(a.colors(0, 0).unwrap())
        );
        m.name = Name([0; 16]);
        assert_eq!(a.material_colors(&m, 0).unwrap(), None);
        for len in 16..data.len() {
            let mut cut = data[..len].to_vec();
            cut[8..12].copy_from_slice(&(len as u32).to_le_bytes());
            if let Ok(f) = G3dFile::parse(&cut) {
                let a = &f.material_animations[0];
                assert!((0..5).any(|fr| a.colors(0, fr).is_err()), "len {len}");
            }
        }
    }
}
