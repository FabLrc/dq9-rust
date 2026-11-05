//! `PAT0` texture pattern animations (`BTP0` files): which texture and palette each material
//! shows over time. Ported from apicula `nitro::pattern`.

use super::{Name, at, read_dict};
use crate::bytes::{slice, u8_at, u16_at, u32_at};
use crate::{Error, Result};

/// One pattern animation.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub name: Name,
    pub num_frames: u16,
    pub texture_names: Vec<Name>,
    pub palette_names: Vec<Name>,
    pub tracks: Vec<PatternTrack>,
}

/// The keyframes of one material.
#[derive(Debug, Clone)]
pub struct PatternTrack {
    /// Material name.
    pub material: Name,
    pub keyframes: Vec<PatternKeyframe>,
}

/// From `frame` on, the material shows `texture_names[texture_idx]` with
/// `palette_names[palette_idx]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternKeyframe {
    pub frame: u16,
    pub texture_idx: u8,
    pub palette_idx: u8,
}

fn names(data: &[u8], off: usize, count: usize) -> Result<Vec<Name>> {
    let bytes = slice(data, off, count * 16)?;
    Ok(bytes.as_chunks::<16>().0.iter().map(|n| Name(*n)).collect())
}

pub(crate) fn read_pattern(data: &[u8], base: usize, name: Name) -> Result<Pattern> {
    let num_frames = u16_at(data, at(base, 4u8)?)?;
    let num_tex = u8_at(data, at(base, 6u8)?)?;
    let num_pal = u8_at(data, at(base, 7u8)?)?;
    let texture_names = names(
        data,
        at(base, u16_at(data, at(base, 8u8)?)?)?,
        num_tex.into(),
    )?;
    let palette_names = names(
        data,
        at(base, u16_at(data, at(base, 10u8)?)?)?,
        num_pal.into(),
    )?;
    let tracks = read_dict(data, at(base, 12u8)?, 8)?
        .into_iter()
        .map(|(v, material)| {
            let count = u32_at(v, 0)? as usize;
            let keys = slice(data, at(base, u16_at(v, 6)?)?, count.saturating_mul(4))?;
            let keyframes = keys
                .as_chunks::<4>()
                .0
                .iter()
                .map(|k| {
                    if k[2] >= num_tex || k[3] >= num_pal {
                        return Err(Error::Malformed("G3D pattern keyframe index"));
                    }
                    Ok(PatternKeyframe {
                        frame: u16::from_le_bytes([k[0], k[1]]),
                        texture_idx: k[2],
                        palette_idx: k[3],
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(PatternTrack {
                material,
                keyframes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Pattern {
        name,
        num_frames,
        texture_names,
        palette_names,
        tracks,
    })
}

#[cfg(test)]
mod tests {
    use super::super::G3dFile;
    use super::super::test_util::{dict, file, section};

    #[test]
    fn parses_pattern_animation() {
        // Pattern: header (12 bytes), track dict, keyframes, texture names, palette names.
        let track_dict_len = dict(&[(&[0; 8], "mat")]).len();
        let keys_off = 12 + track_dict_len;
        let tex_off = keys_off + 8;
        let pal_off = tex_off + 32;
        let mut track = 2u32.to_le_bytes().to_vec();
        track.extend(0u16.to_le_bytes());
        track.extend((keys_off as u16).to_le_bytes());
        let mut p = vec![0u8; 4];
        p.extend(30u16.to_le_bytes());
        p.extend([2, 1]);
        p.extend((tex_off as u16).to_le_bytes());
        p.extend((pal_off as u16).to_le_bytes());
        p.extend(dict(&[(&track, "mat")]));
        p.extend([0, 0, 0, 0, 15, 0, 1, 0]);
        for n in ["fir01", "fir02", "fir_pl"] {
            let mut name = [0u8; 16];
            name[..n.len()].copy_from_slice(n.as_bytes());
            p.extend(name);
        }
        let dict_len = dict(&[(&[0; 4], "anim")]).len();
        let body = [dict(&[(&((8 + dict_len) as u32).to_le_bytes(), "anim")]), p].concat();
        let data = file(b"BTP0", &[section(b"PAT0", &body)]);
        let f = G3dFile::parse(&data).unwrap();
        let pat = &f.patterns[0];
        assert_eq!(pat.num_frames, 30);
        assert!(pat.texture_names[1].is("fir02"));
        assert!(pat.palette_names[0].is("fir_pl"));
        assert_eq!(pat.tracks[0].keyframes[1].frame, 15);
        assert_eq!(pat.tracks[0].keyframes[1].texture_idx, 1);
        for len in 16..data.len() {
            let mut cut = data[..len].to_vec();
            cut[8..12].copy_from_slice(&(len as u32).to_le_bytes());
            assert!(G3dFile::parse(&cut).is_err(), "len {len}");
        }
    }
}
