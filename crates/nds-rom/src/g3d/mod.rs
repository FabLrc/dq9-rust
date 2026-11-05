//! Nintendo NITRO-System G3D files: `BMD0` models (`.nsbmd`), `BTX0` textures (`.nsbtx`) and
//! the headers of the animation files (`BCA0`, `BTA0`, `BTP0`, `BMA0`).
//!
//! Ported from [apicula](https://github.com/scurest/apicula) (0BSD, © 2019 scurest), module
//! `nitro` (container, info blocks, MDL0, TEX0, render commands, pivot matrices) and module
//! `nds` (texture formats and decoding, GPU command parsing, primitive assembly). Differences
//! from apicula: values stay exact fixed point ([`fx::Fx12`]) instead of `f64`, `cgmath` is
//! replaced by [`fx::Mat4`], data is borrowed instead of copied, and every read is
//! bounds-checked so malformed files yield [`Error`](crate::Error) instead of panicking. The
//! hardware side (texture formats, display-list commands) follows GBATEK. Format notes:
//! `docs/re/g3d.md`.

pub mod fx;
pub mod geometry;
pub mod gpu;
pub mod model;
pub mod pattern;
pub mod tex;

use std::fmt;

use crate::bytes::{magic_at, slice, u8_at, u16_at, u32_at};
use crate::{Error, Result};

pub use fx::{Fx12, Mat4};
pub use geometry::{DrawCall, Geometry, Vertex};
pub use gpu::{DisplayList, GpuCmd};
pub use model::{Material, Mesh, Model, Object, RenderOp, SkinTerm};
pub use pattern::{Pattern, PatternKeyframe, PatternTrack};
pub use tex::{Palette, Tex0, TexFormat, Texture, TextureParams};

/// Sixteen-byte NUL-padded name used for every G3D resource.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(pub [u8; 16]);

impl Name {
    /// Bytes up to the first trailing NUL.
    pub fn bytes(&self) -> &[u8] {
        let len = self.0.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        &self.0[..len]
    }

    /// `true` if this name equals `s` (ignoring NUL padding).
    pub fn is(&self, s: &str) -> bool {
        self.bytes() == s.as_bytes()
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.bytes() {
            let c = if (0x20..0x7F).contains(&b) {
                b as char
            } else {
                '.'
            };
            fmt::Write::write_char(f, c)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string())
    }
}

/// `base + off` without overflow.
pub(crate) fn at(base: usize, off: impl Into<u64>) -> Result<usize> {
    usize::try_from(off.into())
        .ok()
        .and_then(|off| base.checked_add(off))
        .ok_or(Error::Malformed("G3D offset overflow"))
}

/// Reads an info block (the NITRO "dictionary": a Patricia tree followed by fixed-size data
/// entries and their names) at `off`. Returns each entry's `datum_size` bytes with its name.
///
/// apicula `nitro::info_block::read`.
pub(crate) fn read_dict(data: &[u8], off: usize, datum_size: usize) -> Result<Vec<(&[u8], Name)>> {
    if u8_at(data, off)? != 0 {
        return Err(Error::Malformed("G3D dictionary revision"));
    }
    let count = usize::from(u8_at(data, at(off, 1u8)?)?);
    // Patricia tree: 8-byte header, a root node and one node per entry, 4 bytes each.
    let entries = at(off, 12 + 4 * count as u64)?;
    if usize::from(u16_at(data, entries)?) != datum_size {
        return Err(Error::Malformed("G3D dictionary entry size"));
    }
    let values = slice(data, at(entries, 4u8)?, count * datum_size)?;
    let names = slice(
        data,
        at(entries, (4 + count * datum_size) as u64)?,
        count * 16,
    )?;
    Ok(values
        .chunks_exact(datum_size.max(1))
        .take(count)
        .zip(names.as_chunks::<16>().0)
        .map(|(v, n)| (v, Name(*n)))
        .collect())
}

/// Reads a dictionary whose entries are `u32` offsets.
pub(crate) fn read_offset_dict(data: &[u8], off: usize) -> Result<Vec<(u32, Name)>> {
    read_dict(data, off, 4)?
        .into_iter()
        .map(|(v, name)| Ok((u32_at(v, 0)?, name)))
        .collect()
}

/// Kind of a G3D file, from its stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// `BMD0` (`.nsbmd`): models, optionally with their textures.
    Model,
    /// `BTX0` (`.nsbtx`): textures and palettes.
    Texture,
    /// `BCA0` (`.nsbca`): joint (skeletal) animations, section `JNT0`.
    JointAnim,
    /// `BTA0` (`.nsbta`): texture SRT animations, section `SRT0`.
    TexSrtAnim,
    /// `BTP0` (`.nsbtp`): texture pattern animations, section `PAT0`.
    TexPatternAnim,
    /// `BMA0` (`.nsbma`): material colour animations, section `MAT0`.
    MaterialAnim,
}

impl FileKind {
    pub fn from_stamp(stamp: [u8; 4]) -> Option<FileKind> {
        Some(match &stamp {
            b"BMD0" => Self::Model,
            b"BTX0" => Self::Texture,
            b"BCA0" => Self::JointAnim,
            b"BTA0" => Self::TexSrtAnim,
            b"BTP0" => Self::TexPatternAnim,
            b"BMA0" => Self::MaterialAnim,
            _ => return None,
        })
    }
}

/// An animation section, only enumerated: its stamp and the names of its animations.
#[derive(Debug, Clone)]
pub struct AnimSection {
    /// `JNT0`, `SRT0`, `PAT0` or `MAT0`.
    pub stamp: [u8; 4],
    pub names: Vec<Name>,
}

/// A parsed G3D file.
#[derive(Debug, Clone)]
pub struct G3dFile<'a> {
    pub kind: FileKind,
    pub version: u16,
    pub models: Vec<Model<'a>>,
    /// Texture section (`TEX0`), in `BTX0` files and in `BMD0` files that embed textures.
    pub tex: Option<Tex0<'a>>,
    /// Every animation section, enumerated by name.
    pub animations: Vec<AnimSection>,
    /// Texture pattern animations (`PAT0`), decoded.
    pub patterns: Vec<Pattern>,
}

impl<'a> G3dFile<'a> {
    pub fn is_g3d(data: &[u8]) -> bool {
        magic_at(data, 0).is_ok_and(|m| FileKind::from_stamp(m).is_some())
    }

    /// Parses the NITRO file header and every section.
    ///
    /// apicula `nitro::container::read_container`, but strict: a section that fails to parse
    /// fails the file instead of being skipped.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let stamp = magic_at(data, 0)?;
        let kind = FileKind::from_stamp(stamp).ok_or(Error::BadMagic {
            what: "G3D file",
            found: stamp,
        })?;
        if u16_at(data, 4)? != 0xFEFF {
            return Err(Error::Malformed("G3D byte order mark"));
        }
        let version = u16_at(data, 6)?;
        let file_size = u32_at(data, 8)? as usize;
        if u16_at(data, 12)? != 16 || file_size < 16 {
            return Err(Error::Malformed("G3D header size"));
        }
        // Decompressed members may carry padding after the declared size.
        let data = slice(data, 0, file_size)?;
        let count = usize::from(u16_at(data, 14)?);
        let mut file = G3dFile {
            kind,
            version,
            models: Vec::new(),
            tex: None,
            animations: Vec::new(),
            patterns: Vec::new(),
        };
        for i in 0..count {
            let off = u32_at(data, 16 + 4 * i)? as usize;
            let section = magic_at(data, off)?;
            match &section {
                b"MDL0" => {
                    for (model_off, name) in read_offset_dict(data, at(off, 8u8)?)? {
                        file.models
                            .push(model::read_model(data, at(off, model_off)?, name)?);
                    }
                }
                b"TEX0" => {
                    if file.tex.is_some() {
                        return Err(Error::Malformed("G3D file with two TEX0 sections"));
                    }
                    file.tex = Some(tex::read_tex0(data, off)?);
                }
                b"JNT0" | b"SRT0" | b"PAT0" | b"MAT0" => {
                    let names = read_offset_dict(data, at(off, 8u8)?)?;
                    if &section == b"PAT0" {
                        for &(pat_off, name) in &names {
                            let base = at(off, pat_off)?;
                            file.patterns.push(pattern::read_pattern(data, base, name)?);
                        }
                    }
                    file.animations.push(AnimSection {
                        stamp: section,
                        names: names.into_iter().map(|(_, n)| n).collect(),
                    });
                }
                _ => {
                    return Err(Error::BadMagic {
                        what: "G3D section",
                        found: section,
                    });
                }
            }
        }
        Ok(file)
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    //! Builders for synthetic G3D files.

    /// Serialises a dictionary with the given entries (`datum` bytes each).
    pub fn dict(entries: &[(&[u8], &str)]) -> Vec<u8> {
        let count = entries.len();
        let datum = entries.first().map_or(4, |e| e.0.len());
        let mut out = vec![0, count as u8, 0, 0];
        out.extend(8u16.to_le_bytes());
        out.extend(((count + 1) as u16 * 4 + 8).to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(std::iter::repeat_n(0u8, 4 * count));
        out.extend((datum as u16).to_le_bytes());
        out.extend(((4 + count * (datum + 16)) as u16).to_le_bytes());
        for (v, _) in entries {
            out.extend(*v);
        }
        for (_, n) in entries {
            let mut name = [0u8; 16];
            name[..n.len()].copy_from_slice(n.as_bytes());
            out.extend(name);
        }
        let size = out.len() as u16;
        out[2..4].copy_from_slice(&size.to_le_bytes());
        out
    }

    /// Wraps sections into a NITRO file with the given stamp.
    pub fn file(stamp: &[u8; 4], sections: &[Vec<u8>]) -> Vec<u8> {
        let header = 16 + 4 * sections.len();
        let mut out = stamp.to_vec();
        out.extend(0xFEFFu16.to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend((sections.len() as u16).to_le_bytes());
        let mut off = header;
        for s in sections {
            out.extend((off as u32).to_le_bytes());
            off += s.len();
        }
        for s in sections {
            out.extend(s);
        }
        let size = out.len() as u32;
        out[8..12].copy_from_slice(&size.to_le_bytes());
        out
    }

    /// A section: stamp, size, then `body`.
    pub fn section(stamp: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = stamp.to_vec();
        out.extend(((body.len() + 8) as u32).to_le_bytes());
        out.extend(body);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::*;
    use super::*;

    #[test]
    fn reads_dictionary() {
        let d = dict(&[(&[1, 0, 0, 0], "one"), (&[2, 0, 0, 0], "two")]);
        let entries = read_offset_dict(&d, 0).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].0, 2);
        assert!(entries[1].1.is("two"));
        assert_eq!(entries[0].1.to_string(), "one");
        assert!(read_dict(&d, 0, 8).is_err());
        for len in 0..d.len() {
            assert!(read_offset_dict(&d[..len], 0).is_err());
        }
    }

    #[test]
    fn enumerates_animation_sections() {
        let body = dict(&[(&[0, 0, 0, 0], "walk")]);
        let data = file(b"BCA0", &[section(b"JNT0", &body)]);
        let f = G3dFile::parse(&data).unwrap();
        assert_eq!(f.kind, FileKind::JointAnim);
        assert_eq!(f.animations.len(), 1);
        assert_eq!(&f.animations[0].stamp, b"JNT0");
        assert!(f.animations[0].names[0].is("walk"));
        for len in 0..data.len() {
            assert!(G3dFile::parse(&data[..len]).is_err());
        }
    }

    #[test]
    fn rejects_unknown_stamps() {
        let data = file(b"BXX0", &[]);
        assert!(matches!(G3dFile::parse(&data), Err(Error::BadMagic { .. })));
        let data = file(b"BMD0", &[section(b"XXX0", &[])]);
        assert!(G3dFile::parse(&data).is_err());
    }
}
