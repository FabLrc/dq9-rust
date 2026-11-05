//! `TEX0` sections (textures and palettes) and texture decoding to RGBA8.
//!
//! Ported from apicula `nitro::tex`, `nds::texture_params`, `nds::texture_formats` and
//! `nds::decode_texture`; formats per GBATEK "DS 3D Texture Formats".

use super::fx::bits;
use super::{Name, at, read_dict};
use crate::bytes::{slice, u16_at, u32_at};
use crate::{Error, Result};

/// GPU `TEXIMAGE_PARAM` word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureParams(pub u32);

impl TextureParams {
    /// Byte offset of the texel data in its block.
    pub fn offset(self) -> u32 {
        bits(self.0, 0, 16) << 3
    }
    pub fn repeat_s(self) -> bool {
        self.0 & (1 << 16) != 0
    }
    pub fn repeat_t(self) -> bool {
        self.0 & (1 << 17) != 0
    }
    pub fn mirror_s(self) -> bool {
        self.0 & (1 << 18) != 0
    }
    pub fn mirror_t(self) -> bool {
        self.0 & (1 << 19) != 0
    }
    pub fn width(self) -> u32 {
        8 << bits(self.0, 20, 23)
    }
    pub fn height(self) -> u32 {
        8 << bits(self.0, 23, 26)
    }
    pub fn format(self) -> TexFormat {
        TexFormat::from_bits(bits(self.0, 26, 29))
    }
    pub fn color0_transparent(self) -> bool {
        self.0 & (1 << 29) != 0
    }
    /// Texture coordinate transformation mode (0 none, 1 texcoord, 2 normal, 3 vertex).
    pub fn texcoord_transform(self) -> u8 {
        bits(self.0, 30, 32) as u8
    }
}

/// The seven DS texture formats (and the "no texture" value 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexFormat {
    None,
    /// 3-bit alpha, 5-bit palette index.
    A3I5,
    Palette4,
    Palette16,
    Palette256,
    /// 4x4 texel blocks, 2 bits per texel plus a 16-bit palette/mode word per block.
    Compressed4x4,
    /// 5-bit alpha, 3-bit palette index.
    A5I3,
    /// Direct 15-bit colour with a 1-bit alpha.
    Direct,
}

impl TexFormat {
    fn from_bits(v: u32) -> TexFormat {
        match v {
            1 => Self::A3I5,
            2 => Self::Palette4,
            3 => Self::Palette16,
            4 => Self::Palette256,
            5 => Self::Compressed4x4,
            6 => Self::A5I3,
            7 => Self::Direct,
            _ => Self::None,
        }
    }

    pub fn bits_per_texel(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Palette4 | Self::Compressed4x4 => 2,
            Self::Palette16 => 4,
            Self::A3I5 | Self::Palette256 | Self::A5I3 => 8,
            Self::Direct => 16,
        }
    }

    pub fn needs_palette(self) -> bool {
        !matches!(self, Self::None | Self::Direct)
    }

    /// Size in bytes of a `width` x `height` texture (main block only for 4x4 compressed).
    pub fn byte_len(self, width: u32, height: u32) -> usize {
        (width * height * self.bits_per_texel() / 8) as usize
    }
}

/// A texture: parameters and texel bytes.
#[derive(Debug, Clone)]
pub struct Texture<'a> {
    pub name: Name,
    pub params: TextureParams,
    /// Texel data.
    pub data: &'a [u8],
    /// Per-block palette/mode words, for [`TexFormat::Compressed4x4`] only.
    pub index_data: &'a [u8],
}

/// A palette: its offset and the palette block from there to its end (palettes have no size).
#[derive(Debug, Clone)]
pub struct Palette<'a> {
    pub name: Name,
    /// Byte offset in the palette block.
    pub offset: u32,
    pub data: &'a [u8],
}

impl Palette<'_> {
    /// Colour `i` (RGB555). Out-of-range indices read 0 like apicula: the hardware would read
    /// whatever follows in palette VRAM.
    pub fn color(&self, i: usize) -> u16 {
        u16_at(self.data, i * 2).unwrap_or(0)
    }
}

/// A `TEX0` section.
#[derive(Debug, Clone)]
pub struct Tex0<'a> {
    pub textures: Vec<Texture<'a>>,
    pub palettes: Vec<Palette<'a>>,
}

impl<'a> Tex0<'a> {
    pub fn texture(&self, name: &Name) -> Option<&Texture<'a>> {
        self.textures.iter().find(|t| t.name == *name)
    }
    pub fn palette(&self, name: &Name) -> Option<&Palette<'a>> {
        self.palettes.iter().find(|p| p.name == *name)
    }
}

/// apicula `nitro::tex::read_tex`.
pub(crate) fn read_tex0(data: &[u8], base: usize) -> Result<Tex0<'_>> {
    let u16h = |o: u64| -> Result<u16> { u16_at(data, at(base, o)?) };
    let u32h = |o: u64| -> Result<u32> { u32_at(data, at(base, o)?) };
    let section = slice(data, base, u32h(4)? as usize)?;
    let tex_dict = usize::from(u16h(14)?);
    let tex_block = u32h(20)? as usize;
    let comp1_block = u32h(36)? as usize;
    let comp2_block = u32h(40)? as usize;
    let pal_len = usize::from(u16h(48)?) << 3;
    let pal_dict = u32h(52)? as usize;
    let pal_block = slice(section, u32h(56)? as usize, pal_len)?;

    let textures = read_dict(section, tex_dict, 8)?
        .into_iter()
        .map(|(v, name)| {
            let params = TextureParams(u32_at(v, 0)?);
            let off = params.offset() as usize;
            let format = params.format();
            if format == TexFormat::None {
                return Err(Error::Malformed("G3D texture without format"));
            }
            let len = format.byte_len(params.width(), params.height());
            let (data, index_data) = if format == TexFormat::Compressed4x4 {
                (
                    slice(section, at(comp1_block, off as u64)?, len)?,
                    slice(section, at(comp2_block, (off / 2) as u64)?, len / 2)?,
                )
            } else {
                (slice(section, at(tex_block, off as u64)?, len)?, &[][..])
            };
            Ok(Texture {
                name,
                params,
                data,
                index_data,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let palettes = read_dict(section, pal_dict, 4)?
        .into_iter()
        .map(|(v, name)| {
            let offset = u32::from(u16_at(v, 0)?) << 3;
            let data = pal_block
                .get(offset as usize..)
                .ok_or(Error::Malformed("G3D palette offset"))?;
            Ok(Palette { name, offset, data })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Tex0 { textures, palettes })
}

fn extend5(x: u32) -> u8 {
    ((x << 3) | (x >> 2)) as u8
}

/// RGB555 + 5-bit alpha to RGBA8.
fn rgba(c: u16, a5: u32) -> [u8; 4] {
    let c = u32::from(c);
    [
        extend5(bits(c, 0, 5)),
        extend5(bits(c, 5, 10)),
        extend5(bits(c, 10, 15)),
        extend5(a5),
    ]
}

fn mix(c1: [u8; 4], w1: u32, c2: [u8; 4], w2: u32) -> [u8; 4] {
    let d = w1 + w2;
    std::array::from_fn(|i| ((u32::from(c1[i]) * w1 + u32::from(c2[i]) * w2) / d) as u8)
}

/// Decodes a texture to RGBA8 rows (top to bottom), `width * height * 4` bytes.
///
/// apicula `nds::decode_texture::decode_texture`. Palette formats require `palette`.
pub fn decode_texture(texture: &Texture, palette: Option<&Palette>) -> Result<Vec<u8>> {
    let params = texture.params;
    let (w, h) = (params.width() as usize, params.height() as usize);
    let format = params.format();
    let pal = match (format.needs_palette(), palette) {
        (true, None) => return Err(Error::Malformed("G3D texture needs a palette")),
        (_, p) => p,
    };
    let color = |i: usize| pal.map_or(0, |p| p.color(i));
    let len = format.byte_len(w as u32, h as u32);
    let data = texture
        .data
        .get(..len)
        .ok_or(Error::Malformed("G3D texture data too short"))?;
    let transparent0 = params.color0_transparent();
    let indexed = |i: u32| {
        rgba(
            color(i as usize),
            if i == 0 && transparent0 { 0 } else { 31 },
        )
    };
    let mut out = Vec::with_capacity(w * h * 4);
    match format {
        TexFormat::None => return Err(Error::Malformed("G3D texture without format")),
        TexFormat::A3I5 => {
            for &x in data {
                let a3 = u32::from(x >> 5);
                out.extend(rgba(color(usize::from(x & 31)), (a3 << 2) | (a3 >> 1)));
            }
        }
        TexFormat::A5I3 => {
            for &x in data {
                out.extend(rgba(color(usize::from(x & 7)), u32::from(x >> 3)));
            }
        }
        TexFormat::Palette4 | TexFormat::Palette16 | TexFormat::Palette256 => {
            let bpp = format.bits_per_texel();
            for &x in data {
                for k in 0..8 / bpp {
                    out.extend(indexed(bits(u32::from(x), k * bpp, (k + 1) * bpp)));
                }
            }
        }
        TexFormat::Direct => {
            for t in data.as_chunks::<2>().0 {
                let t = u16::from_le_bytes(*t);
                out.extend(rgba(t, if t & 0x8000 != 0 { 31 } else { 0 }));
            }
        }
        TexFormat::Compressed4x4 => {
            let blocks_x = w / 4;
            let index = texture
                .index_data
                .get(..len / 2)
                .ok_or(Error::Malformed("G3D 4x4 texture index data too short"))?;
            for y in 0..h {
                for x in 0..w {
                    let block = blocks_x * (y / 4) + x / 4;
                    let texels = u32_at(data, block * 4)?;
                    let extra = u32::from(u16_at(index, block * 2)?);
                    let shift = 2 * (4 * (y % 4) + x % 4) as u32;
                    let texel = bits(texels, shift, shift + 2);
                    let mode = bits(extra, 14, 16);
                    let base = (bits(extra, 0, 14) as usize) << 1;
                    let c = |n: usize| rgba(color(base + n), 31);
                    let clear = [0, 0, 0, 0];
                    out.extend(match (mode, texel) {
                        (_, 0) => c(0),
                        (_, 1) => c(1),
                        (0 | 2, 2) => c(2),
                        (1, 2) => mix(c(0), 1, c(1), 1),
                        (3, 2) => mix(c(0), 5, c(1), 3),
                        (0 | 1, _) => clear,
                        (2, _) => c(3),
                        _ => mix(c(0), 3, c(1), 5),
                    });
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::G3dFile;
    use super::super::test_util::{dict, file};
    use super::*;

    /// A TEX0 with a 8x8 16-colour texture "t", a 8x8 4x4-compressed texture "c" and palette
    /// "t_pl".
    fn sample_tex0() -> Vec<u8> {
        let p16 = TextureParams((3 << 26) | (1 << 29));
        let c4 = TextureParams(5 << 26);
        let tex_dict = dict(&[
            (&[p16.0.to_le_bytes(), [0; 4]].concat(), "t"),
            (&[c4.0.to_le_bytes(), [0; 4]].concat(), "c"),
        ]);
        let pal_dict = dict(&[(&[0, 0, 0, 0], "t_pl")]);
        let tex_dict_off = 60;
        let pal_dict_off = tex_dict_off + tex_dict.len();
        let tex_block = pal_dict_off + pal_dict.len();
        let texels: Vec<u8> = (0..32).map(|i| (i % 16) as u8 * 0x11).collect();
        let comp1 = tex_block + texels.len();
        // Four blocks: modes 0..=3, every texel = index 2.
        let comp1_data = [0xAAu8; 16];
        let comp2 = comp1 + comp1_data.len();
        let comp2_data: Vec<u8> = (0..4u16).flat_map(|m| (m << 14).to_le_bytes()).collect();
        let pal_block = comp2 + comp2_data.len();
        let palette: Vec<u8> = (0..16u16).flat_map(|i| (i * 2).to_le_bytes()).collect();
        let size = pal_block + palette.len();

        let mut s = b"TEX0".to_vec();
        s.extend((size as u32).to_le_bytes());
        s.extend([0; 4]);
        s.extend(((texels.len() >> 3) as u16).to_le_bytes());
        s.extend((tex_dict_off as u16).to_le_bytes());
        s.extend([0; 4]);
        s.extend((tex_block as u32).to_le_bytes());
        s.extend([0; 4]);
        s.extend(2u16.to_le_bytes());
        s.extend(0u16.to_le_bytes());
        s.extend([0; 4]);
        s.extend((comp1 as u32).to_le_bytes());
        s.extend((comp2 as u32).to_le_bytes());
        s.extend([0; 4]);
        s.extend(((palette.len() >> 3) as u16).to_le_bytes());
        s.extend(0u16.to_le_bytes());
        s.extend((pal_dict_off as u32).to_le_bytes());
        s.extend((pal_block as u32).to_le_bytes());
        assert_eq!(s.len(), 60);
        for part in [
            &tex_dict[..],
            &pal_dict,
            &texels,
            &comp1_data,
            &comp2_data,
            &palette,
        ] {
            s.extend(part);
        }
        s
    }

    #[test]
    fn parses_and_decodes_textures() {
        let data = file(b"BTX0", &[sample_tex0()]);
        let f = G3dFile::parse(&data).unwrap();
        let tex = f.tex.as_ref().unwrap();
        assert_eq!(tex.textures.len(), 2);
        let t = tex.texture(&tex.textures[0].name).unwrap();
        let pal = &tex.palettes[0];
        assert!(pal.name.is("t_pl"));
        let rgba = decode_texture(t, Some(pal)).unwrap();
        assert_eq!(rgba.len(), 8 * 8 * 4);
        // Texel 0 uses colour 0: transparent.
        assert_eq!(rgba[3], 0);
        // Texel 2 uses colour 1 = RGB555 0x0002 -> red 2.
        assert_eq!(&rgba[8..12], &[extend5(2), 0, 0, 255]);
        assert!(decode_texture(t, None).is_err());

        let c = &tex.textures[1];
        assert_eq!(c.params.format(), TexFormat::Compressed4x4);
        let rgba = decode_texture(c, Some(pal)).unwrap();
        // Block (0,0) mode 0 index 2 -> colour 2 = 4; block (1,0) mode 1 -> average of
        // colours 2 and 3 of its palette base 0 (+0) -> (0+2)/2.
        assert_eq!(rgba[0], extend5(4));
        assert_eq!(
            rgba[4 * 4],
            (u32::from(extend5(0)) + u32::from(extend5(2))) as u8 / 2
        );
    }

    #[test]
    fn truncated_textures_are_errors() {
        let data = file(b"BTX0", &[sample_tex0()]);
        for len in 16..data.len() {
            let mut cut = data[..len].to_vec();
            cut[8..12].copy_from_slice(&(len as u32).to_le_bytes());
            assert!(G3dFile::parse(&cut).is_err(), "len {len}");
        }
    }

    #[test]
    fn decodes_every_format() {
        let pal_bytes: Vec<u8> = (0..256u16).flat_map(|i| i.to_le_bytes()).collect();
        let pal = Palette {
            name: Name([0; 16]),
            offset: 0,
            data: &pal_bytes,
        };
        let texels = vec![0xFFu8; 8 * 8 * 2];
        for f in 1..=7u32 {
            let params = TextureParams(f << 26);
            let t = Texture {
                name: Name([0; 16]),
                params,
                data: &texels,
                index_data: &texels,
            };
            let rgba = decode_texture(&t, Some(&pal)).unwrap();
            assert_eq!(rgba.len(), 8 * 8 * 4, "format {f}");
            let short = Texture {
                data: &texels[..1],
                ..t
            };
            assert!(decode_texture(&short, Some(&pal)).is_err());
        }
    }
}
