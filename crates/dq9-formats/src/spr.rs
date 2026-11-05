//! Sprite sheets (`.spr`, `data/ani/`): the 2D NPCs, pots and barrels. Frames built of
//! rectangular 4-bit (or 8-bit) parts, a BGR555 palette and named animations, read as
//! `SPRSprite::LoadFromSPR` (`0x02047b50`, decomp `src/Graphics/SPRSprite.cpp`) reads them.
//! Layout and evidence in `docs/re/characters.md`.

use nds_rom::{Error, Result};

/// One rectangular part of a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part<'a> {
    /// Top-left corner in the frame, from the frame's top (the game stores the distance to
    /// the bottom instead).
    pub x: u16,
    pub y: u16,
    /// `8 << n` on each side.
    pub width: u32,
    pub height: u32,
    /// Texels row by row; with format 3 two per byte, low nibble first.
    pub texels: &'a [u8],
}

/// One frame (`Texture`): its size and parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Kept on 8 bits by the game.
    pub width: u16,
    pub height: u16,
    pub parts: Vec<Part<'a>>,
}

/// One step of an animation (`SPRSprite::Frame`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Step shown after this one.
    pub next: u32,
    /// Ticks of the game's frame counter the step lasts; `SPRSprite::Update` moves on once
    /// more than that has passed.
    pub duration: u32,
    /// Frame of the sheet.
    pub frame: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Animation<'a> {
    /// Name, from a 30-byte slot (up to its first NUL).
    pub name: &'a [u8],
    pub steps: Vec<Step>,
}

/// A parsed sprite sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite<'a> {
    /// GX texture format: 3 (16 colours, 4 bits per texel) on the sheets, else 8 bits.
    pub format: u16,
    pub frames: Vec<Frame<'a>>,
    /// BGR555 colours; index 0 is transparent.
    pub palette: Vec<u16>,
    /// The 30 bytes the game skips before the names (a developer string).
    pub skipped: &'a [u8],
    pub animations: Vec<Animation<'a>>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or(Error::OutOfBounds {
            offset: self.at,
            len: n,
            size: self.data.len(),
        })?;
        let b = &self.data[self.at..end];
        self.at = end;
        Ok(b)
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

impl<'a> Sprite<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let mut r = Reader { data, at: 0 };
        let count = r.u16()?;
        let format = r.u16()?;
        let mut frames = Vec::with_capacity(usize::from(count).min(data.len() / 8));
        for _ in 0..count {
            let (width, height) = (r.u16()?, r.u16()?);
            let parts_count = r.u32()? as u8;
            let mut parts = Vec::with_capacity(usize::from(parts_count));
            for _ in 0..parts_count {
                let (x, y) = (r.u16()?, r.u16()?);
                let (lw, lh) = (r.u16()?, r.u16()?);
                if lw > 3 || lh > 3 {
                    return Err(Error::Malformed("sprite part larger than 64"));
                }
                let (pw, ph) = (8u32 << lw, 8u32 << lh);
                let len = if format == 3 { pw * ph / 2 } else { pw * ph };
                parts.push(Part {
                    x,
                    y,
                    width: pw,
                    height: ph,
                    texels: r.take(len as usize)?,
                });
            }
            frames.push(Frame {
                width: width & 0xff,
                height: height & 0xff,
                parts,
            });
        }
        let colours = r.u32()? as usize;
        let palette = r
            .take(
                colours
                    .checked_mul(2)
                    .ok_or(Error::Malformed("sprite palette"))?,
            )?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let skipped = r.take(30)?;
        let names = r.u32()? as usize;
        let mut slots = Vec::with_capacity(names.min(data.len() / 30));
        for _ in 0..names {
            let slot = r.take(30)?;
            slots.push(slot.split(|&b| b == 0).next().unwrap_or_default());
        }
        let mut animations = Vec::with_capacity(slots.len());
        for name in slots {
            let n = r.u32()? as usize;
            let read =
                |r: &mut Reader<'a>| -> Result<Vec<u32>> { (0..n).map(|_| r.u32()).collect() };
            if n > data.len() / 12 {
                return Err(Error::Malformed("sprite animation step count"));
            }
            let (next, duration, frame) = (read(&mut r)?, read(&mut r)?, read(&mut r)?);
            let steps = (0..n)
                .map(|i| Step {
                    next: next[i],
                    duration: duration[i],
                    frame: frame[i],
                })
                .collect();
            animations.push(Animation { name, steps });
        }
        Ok(Self {
            format,
            frames,
            palette,
            skipped,
            animations,
        })
    }

    /// `SPRSprite::Names::Find`: index of the animation with this name, 0 when none (the
    /// game's `SetAnimation` ignores index 0).
    pub fn find(&self, name: &[u8]) -> usize {
        self.animations
            .iter()
            .position(|a| a.name == name)
            .unwrap_or(0)
    }

    /// Animation a freshly loaded sheet plays: 1, or 0 for a single empty animation.
    pub fn first_animation(&self) -> usize {
        match self.animations.first() {
            Some(a) if a.steps.is_empty() && self.animations.len() == 1 => 0,
            Some(_) => 1,
            None => 0,
        }
    }

    /// Frame `index` as RGBA8 rows (`width × height × 4` bytes); colour 0 and indices past
    /// the palette are transparent. `None` for a missing frame.
    pub fn frame_rgba(&self, index: usize) -> Option<(u32, u32, Vec<u8>)> {
        let frame = self.frames.get(index)?;
        let (w, h) = (u32::from(frame.width), u32::from(frame.height));
        let mut out = vec![0u8; (w * h * 4) as usize];
        for part in &frame.parts {
            for py in 0..part.height {
                for px in 0..part.width {
                    let i = (py * part.width + px) as usize;
                    let texel = if self.format == 3 {
                        part.texels[i / 2] >> (4 * (i % 2)) & 0xf
                    } else {
                        part.texels[i]
                    };
                    let (x, y) = (u32::from(part.x) + px, u32::from(part.y) + py);
                    let Some(&c) = self.palette.get(usize::from(texel)) else {
                        continue;
                    };
                    if texel == 0 || x >= w || y >= h {
                        continue;
                    }
                    let o = ((y * w + x) * 4) as usize;
                    let ch = |s: u16| {
                        let v = (c >> s & 0x1f) as u8;
                        v << 3 | v >> 2
                    };
                    out[o..o + 4].copy_from_slice(&[ch(0), ch(5), ch(10), 255]);
                }
            }
        }
        Some((w, h, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(1u16.to_le_bytes());
        d.extend(3u16.to_le_bytes());
        // Frame 8×16: one 8×8 part at (0, 8).
        for v in [8u16, 16] {
            d.extend(v.to_le_bytes());
        }
        d.extend(1u32.to_le_bytes());
        for v in [0u16, 8, 0, 0] {
            d.extend(v.to_le_bytes());
        }
        let mut texels = vec![0u8; 32];
        texels[0] = 0x21; // (0, 0) colour 1, (1, 0) colour 2
        texels[1] = 0x05; // (2, 0) colour 5: past the palette
        d.extend(&texels);
        d.extend(3u32.to_le_bytes());
        for c in [0u16, 0x001f, 0x7c00] {
            d.extend(c.to_le_bytes());
        }
        d.extend([0u8; 30]);
        d.extend(2u32.to_le_bytes());
        for name in ["walk_down", "stand_down"] {
            let mut slot = name.as_bytes().to_vec();
            slot.resize(30, 0);
            d.extend(slot);
        }
        for (n, steps) in [
            (2u32, [[1u32, 0], [8, 8], [0, 0]]),
            (1, [[0, 0], [60, 0], [0, 0]]),
        ] {
            d.extend(n.to_le_bytes());
            for array in steps {
                for v in &array[..n as usize] {
                    d.extend(v.to_le_bytes());
                }
            }
        }
        d
    }

    #[test]
    fn parses_frames_palette_and_animations() {
        let data = sheet();
        let s = Sprite::parse(&data).unwrap();
        assert_eq!((s.format, s.frames.len(), s.palette.len()), (3, 1, 3));
        let a = &s.animations[0];
        assert_eq!(a.name, b"walk_down");
        assert_eq!(
            a.steps[0],
            Step {
                next: 1,
                duration: 8,
                frame: 0
            }
        );
        assert_eq!(s.find(b"stand_down"), 1);
        assert_eq!(s.find(b"nope"), 0);
        assert_eq!(s.first_animation(), 1);
        let (w, h, px) = s.frame_rgba(0).unwrap();
        assert_eq!((w, h), (8, 16));
        let at = |x: usize, y: usize| &px[(y * 8 + x) * 4..][..4];
        assert_eq!(at(0, 8), [255, 0, 0, 255]);
        assert_eq!(at(1, 8), [0, 0, 255, 255]);
        assert_eq!(at(2, 8), [0, 0, 0, 0]);
        assert_eq!(at(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn truncated_sheets_are_errors() {
        let data = sheet();
        for len in [0, 3, 20, data.len() - 1] {
            assert!(Sprite::parse(&data[..len]).is_err(), "{len}");
        }
    }
}
