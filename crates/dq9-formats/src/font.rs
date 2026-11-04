//! Level-5 bitmap fonts: `data/pack_lv5/fi_<id>.bin` (glyph table) + `fd_<id>.bin` (1bpp
//! strip). Glyphs are keyed by byte strings, which is how markup such as `<'e>` or `<1>`
//! becomes a single glyph. Layout and evidence in `docs/re/font.md`.
//!
//! port: 0x02042814, 0x020420f8, 0x020424f4, 0x020425f4, 0x02042658

use nds_rom::{Error, Result};

/// The two fonts loaded at boot by `Font_LoadAll` (`0x02042954`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontId {
    /// `s7`: small font (index 0).
    Small,
    /// `me`: message font (index 1).
    Message,
}

impl FontId {
    /// Suffix of the font files: `fi_<name>.bin`, `fd_<name>.bin`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Small => "s7",
            Self::Message => "me",
        }
    }

    /// Advance of a byte without glyph (e.g. `' '`) minus the 1 px glyph spacing, from the
    /// ARM9 table at `0x020e7aa4`.
    pub fn space_width(self) -> u8 {
        match self {
            Self::Small => 2,
            Self::Message => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph<'a> {
    /// Byte string the glyph replaces, e.g. `b"A"` or `b"<'e>"`.
    pub text: &'a [u8],
    pub width: u8,
    /// Left column of the glyph in the bitmap strip.
    pub x: u16,
    /// Letter class bits: vowel (bit 6) and lowercase letter (bit 7) of the length byte.
    pub vowel: bool,
    pub lowercase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kerning {
    pub left: u8,
    pub right: u8,
    pub adjust: i8,
}

#[derive(Debug, Clone)]
pub struct Font<'a> {
    pub glyphs: Vec<Glyph<'a>>,
    pub kerning: Vec<Kerning>,
    /// Bitmap strip width and height in pixels.
    pub width: u16,
    pub height: u16,
    /// 1bpp, row-major, most significant bit first.
    bits: &'a [u8],
    space_width: u8,
    /// Glyph indices by first byte of their string, in table order.
    by_first_byte: Vec<Vec<u8>>,
}

fn u32_at(data: &[u8], offset: usize) -> Result<usize> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        .ok_or(Error::OutOfBounds {
            offset,
            len: 4,
            size: data.len(),
        })
}

fn expect_version(data: &[u8], version: &[u8; 4], what: &'static str) -> Result<()> {
    match data.first_chunk::<4>() {
        Some(v) if v == version => Ok(()),
        Some(v) => Err(Error::BadMagic { what, found: *v }),
        None => Err(Error::Malformed("font file truncated")),
    }
}

/// Index stored when the previous character has no glyph (no kerning applies).
const NO_GLYPH: u8 = 0xFF;

impl<'a> Font<'a> {
    /// Parses a font from its `fi_*.bin` (info) and `fd_*.bin` (bitmap) files.
    pub fn parse(info: &'a [u8], data: &'a [u8], id: FontId) -> Result<Self> {
        expect_version(info, b"1.1\0", "font info")?;
        let glyph_count = u32_at(info, 4)?;
        let kerning_count = u32_at(info, 8)?;
        let kerning_offset = u32_at(info, 12)?;
        let glyph_offset = u32_at(info, 16)?;
        if glyph_count >= usize::from(NO_GLYPH) {
            return Err(Error::Malformed("too many glyphs for 8-bit indices"));
        }

        expect_version(data, b"1.0\0", "font bitmap")?;
        let dims = u32_at(data, 4)?;
        let (width, height) = (dims as u16, (dims >> 16) as u16);
        let size = u32_at(data, 8)?;
        let offset = u32_at(data, 12)?;
        let bits = data
            .get(offset..offset + size)
            .filter(|b| b.len() * 8 == usize::from(width) * usize::from(height))
            .ok_or(Error::Malformed("font bitmap size"))?;

        let glyphs = (0..glyph_count)
            .map(|i| {
                let e = glyph_offset + i * 8;
                let text_offset = u32_at(info, e)?;
                let entry = info
                    .get(e + 4..e + 8)
                    .ok_or(Error::Malformed("glyph table"))?;
                let len = usize::from(entry[1] & 0x3F);
                let glyph = Glyph {
                    text: info
                        .get(text_offset..text_offset + len)
                        .filter(|t| !t.is_empty())
                        .ok_or(Error::Malformed("glyph string"))?,
                    width: entry[0],
                    x: u16::from_le_bytes([entry[2], entry[3]]),
                    vowel: entry[1] & 0x40 != 0,
                    lowercase: entry[1] & 0x80 != 0,
                };
                if glyph.x + u16::from(glyph.width) > width {
                    return Err(Error::Malformed("glyph outside bitmap"));
                }
                Ok(glyph)
            })
            .collect::<Result<Vec<_>>>()?;
        let kerning = (0..kerning_count)
            .map(|i| {
                let e = info
                    .get(kerning_offset + i * 4..kerning_offset + i * 4 + 4)
                    .ok_or(Error::Malformed("kerning table"))?;
                if usize::from(e[0].max(e[1])) >= glyphs.len() {
                    return Err(Error::Malformed("kerning glyph index"));
                }
                Ok(Kerning {
                    left: e[0],
                    right: e[1],
                    adjust: e[2] as i8,
                })
            })
            .collect::<Result<_>>()?;
        // Glyph indices grouped by first byte, in table order: same result as the game's
        // linear scan, without testing every glyph at every position.
        let mut by_first_byte = vec![Vec::new(); 256];
        for (i, g) in glyphs.iter().enumerate() {
            by_first_byte[usize::from(g.text[0])].push(i as u8);
        }
        Ok(Self {
            glyphs,
            kerning,
            width,
            height,
            bits,
            space_width: id.space_width(),
            by_first_byte,
        })
    }

    /// First glyph (in table order) whose string starts `text` (`Font_FindGlyph`,
    /// `0x020424f4`).
    pub fn find(&self, text: &[u8]) -> Option<u8> {
        let candidates = &self.by_first_byte[usize::from(*text.first()?)];
        candidates
            .iter()
            .copied()
            .find(|&i| text.starts_with(self.glyphs[usize::from(i)].text))
    }

    /// Horizontal adjustment between two consecutive glyphs (`Font_Kerning`, `0x020425f4`).
    pub fn kerning(&self, left: u8, right: u8) -> i8 {
        self.kerning
            .iter()
            .find(|k| k.left == left && k.right == right)
            .map_or(0, |k| k.adjust)
    }

    /// Positions glyphs left to right: `(glyph index, x)` per glyph found; bytes without a
    /// glyph only advance the pen. Returns the placements and the total advance.
    pub fn layout(&self, mut text: &[u8]) -> (Vec<(u8, i32)>, i32) {
        let mut placed = Vec::new();
        let (mut x, mut previous) = (0i32, NO_GLYPH);
        while !text.is_empty() {
            match self.find(text) {
                Some(index) => {
                    let glyph = &self.glyphs[usize::from(index)];
                    x += i32::from(self.kerning(previous, index));
                    placed.push((index, x));
                    x += i32::from(glyph.width) + 1;
                    text = &text[glyph.text.len()..];
                    previous = index;
                }
                None => {
                    x += i32::from(self.space_width) + 1;
                    text = &text[1..];
                    previous = NO_GLYPH;
                }
            }
        }
        (placed, x)
    }

    /// Pixel width of a single-line string, as computed by `Font_MeasureString`
    /// (`0x020420f8`): the advance without the trailing 1 px spacing.
    pub fn measure(&self, text: &[u8]) -> i32 {
        self.layout(text).1 - 1
    }

    /// Whether the strip pixel at `(x, y)` is set.
    pub fn pixel(&self, x: u16, y: u16) -> bool {
        let i = usize::from(y) * usize::from(self.width) + usize::from(x);
        self.bits
            .get(i / 8)
            .is_some_and(|b| b >> (7 - i % 8) & 1 != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Font with glyphs "A" (width 2, vowel) and "<'e>" (width 1), kerning A,A = -1.
    /// Strip 8x2: "A" columns 0-1 fully set on row 0; "<'e>" column 3 on row 1.
    fn sample() -> (Vec<u8>, Vec<u8>) {
        let mut info = b"1.1\0".to_vec();
        let strings = 0x18 + 4 + 16;
        for v in [2u32, 1, 0x18, 0x1C, strings] {
            info.extend(v.to_le_bytes());
        }
        info.extend([0, 0, 0xFF, 0]); // kerning A,A -1
        info.extend((strings).to_le_bytes());
        info.extend([2, 0x41, 0, 0]); // "A": width 2, len 1 | vowel, x 0
        info.extend((strings + 1).to_le_bytes());
        info.extend([1, 0x84, 3, 0]); // "<'e>": width 1, len 4 | lowercase, x 3
        info.extend(b"A<'e>");
        let mut data = b"1.0\0".to_vec();
        data.extend(8u16.to_le_bytes());
        data.extend(2u16.to_le_bytes());
        data.extend(2u32.to_le_bytes());
        data.extend(16u32.to_le_bytes());
        data.extend([0b1100_0000, 0b0001_0000]);
        (info, data)
    }

    #[test]
    fn finds_markup_glyphs_and_classes() {
        let (info, data) = sample();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        assert_eq!(font.find(b"<'e>t"), Some(1));
        assert_eq!(font.find(b"Ab"), Some(0));
        assert_eq!(font.find(b"<'x>"), None);
        assert!(font.glyphs[0].vowel && !font.glyphs[0].lowercase);
        assert!(font.glyphs[1].lowercase);
        assert!(font.pixel(1, 0) && !font.pixel(2, 0) && font.pixel(3, 1));
    }

    #[test]
    fn layout_applies_kerning_spacing_and_unknown_bytes() {
        let (info, data) = sample();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        // A(0) -> A kerned -1 at 2 -> space (3+1) -> <'e> at 9 (no kerning after a space).
        let (placed, advance) = font.layout(b"AA <'e>");
        assert_eq!(placed, [(0, 0), (0, 2), (1, 9)]);
        assert_eq!(advance, 11);
        assert_eq!(font.measure(b"AA <'e>"), 10);
    }

    #[test]
    fn rejects_inconsistent_files() {
        let (info, mut data) = sample();
        data[8] = 3; // bitmap size no longer matches 8x2
        assert!(Font::parse(&info, &data, FontId::Message).is_err());
        let (mut info, data) = sample();
        info[0x18] = 5; // kerning refers to glyph 5
        assert!(Font::parse(&info, &data, FontId::Message).is_err());
    }
}
