//! `.obg` pictures: the mini-map's map images. Layout measured on the cartridge in
//! `docs/re/minimap.md` (source: the DQIX formats wiki, `Mini-Map`; 268 files on our ROM,
//! every one exactly `40 + 32 × tiles + 2 × width × height` bytes).
//!
//! A picture is a grid of 8×8 four-bit cells over sixteen BGR555 colours. Colour 0 reads as
//! transparent (the DS's rule for a sixteen-colour background); on our ROM it only occurs
//! outside a picture's torn paper edge.

use nds_rom::bytes::{slice, u16_at};
use nds_rom::nitro2d;
use nds_rom::{Error, Result};

/// Bytes before the colours: the eight-byte head and the sixteen colour words.
pub const HEADER_SIZE: usize = 40;
/// Colours of the picture's palette.
pub const COLOURS: usize = 16;
/// Bytes of one 8×8 four-bit tile.
pub const TILE_BYTES: usize = 32;

/// A picture's pixel size in 8×8 cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

/// One `.obg` picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture<'a> {
    /// Size in 8×8 cells.
    pub width: u8,
    pub height: u8,
    /// Not established; 0 on all 268 files.
    pub unknown_02: u8,
    /// Not established; 137 on 117 files, 247 on 132, nine other values.
    pub unknown_03: u8,
    pub tile_count: u16,
    /// Not established; 0 on all 268 files.
    pub unknown_06: u16,
    /// The sixteen colours, as stored (BGR555).
    pub colours: [u16; COLOURS],
    tiles: &'a [u8],
    /// The tile each cell draws, row by row.
    pub cells: Vec<u16>,
}

impl<'a> Picture<'a> {
    /// Parses a picture and validates its framing, every cell and its tile.
    ///
    /// # Errors
    /// [`Error::Malformed`] unless the size is exactly `40 + 32 × tiles + 2 × width ×
    /// height`, a cell sets a bit above bit 9, or a cell names a tile past the count.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let header = slice(data, 0, HEADER_SIZE)?;
        let width = header[0];
        let height = header[1];
        let tile_count = u16_at(header, 4)?;
        let cells_count = usize::from(width) * usize::from(height);
        let expected = HEADER_SIZE + TILE_BYTES * usize::from(tile_count) + 2 * cells_count;
        if data.len() != expected {
            return Err(Error::Malformed("obg size formula"));
        }
        let mut colours = [0u16; COLOURS];
        for (i, colour) in colours.iter_mut().enumerate() {
            *colour = u16_at(header, 8 + 2 * i)?;
        }
        let tiles = slice(data, HEADER_SIZE, TILE_BYTES * usize::from(tile_count))?;
        let cell_bytes = slice(data, HEADER_SIZE + tiles.len(), 2 * cells_count)?;
        let cells = cell_bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect::<Vec<_>>();
        for &cell in &cells {
            if cell & !0x03FF != 0 {
                return Err(Error::Malformed("obg cell flip or palette bit"));
            }
            if u32::from(cell) >= u32::from(tile_count) {
                return Err(Error::Malformed("obg cell past the tile count"));
            }
        }
        Ok(Self {
            width,
            height,
            unknown_02: header[2],
            unknown_03: header[3],
            tile_count,
            unknown_06: u16_at(header, 6)?,
            colours,
            tiles,
            cells,
        })
    }

    /// Size in cells.
    pub fn size(&self) -> Size {
        Size {
            width: usize::from(self.width),
            height: usize::from(self.height),
        }
    }

    /// Tile `index`: eight rows of four bytes, low nibble left.
    pub fn tile(&self, index: u16) -> Option<&'a [u8]> {
        let start = usize::from(index).checked_mul(TILE_BYTES)?;
        self.tiles.get(start..start + TILE_BYTES)
    }

    /// The picture drawn: eight pixels per cell, colour 0 transparent.
    pub fn image(&self) -> Image {
        let width = usize::from(self.width) * 8;
        let height = usize::from(self.height) * 8;
        let mut rgba = vec![[0u8; 4]; width * height];
        for (index, &cell) in self.cells.iter().enumerate() {
            let cx = (index % usize::from(self.width)) * 8;
            let cy = (index / usize::from(self.width)) * 8;
            let Some(tile) = self.tile(cell) else {
                continue;
            };
            for y in 0..8 {
                for x in 0..8 {
                    let byte = tile[y * 4 + x / 2];
                    let nibble = if x % 2 == 0 { byte & 0x0F } else { byte >> 4 };
                    let colour = self.colours[usize::from(nibble)];
                    let pixel = if nibble == 0 {
                        [0, 0, 0, 0]
                    } else {
                        nitro2d::colour_rgba(colour)
                    };
                    rgba[(cy + y) * width + cx + x] = pixel;
                }
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }
}

/// A drawn picture: RGBA8 rows, top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<[u8; 4]>,
}

impl Image {
    /// The four bytes of the pixel at `x`, `y`, if inside.
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(self.rgba[y * self.width + x])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of `width`×`height` cells over `tiles` one-colour tiles.
    fn build(width: u8, height: u8, tiles: &[u8]) -> Vec<u8> {
        let mut out = vec![width, height, 0, 137];
        out.extend((tiles.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        for i in 0..COLOURS {
            out.extend((i as u16 * 0x0421).to_le_bytes());
        }
        for tile in tiles {
            out.extend(std::iter::repeat_n(*tile, TILE_BYTES));
        }
        for i in 0..width as usize * height as usize {
            out.extend(((i as u16) % tiles.len() as u16).to_le_bytes());
        }
        out
    }

    #[test]
    fn decodes_a_picture() {
        let data = build(2, 1, &[1, 2]);
        let picture = Picture::parse(&data).unwrap();
        assert_eq!(
            picture.size(),
            Size {
                width: 2,
                height: 1
            }
        );
        assert_eq!(picture.tile_count, 2);
        assert_eq!(picture.unknown_03, 137);
        let image = picture.image();
        assert_eq!((image.width, image.height), (16, 8));
        // Tile 0 is colour 1 (odd nibbles are the high nibble, even the low: both 1).
        assert_eq!(image.pixel(0, 0), Some(nitro2d::colour_rgba(0x0421)));
        assert_eq!(image.pixel(8, 0), Some(nitro2d::colour_rgba(2 * 0x0421)));
        assert_eq!(image.pixel(16, 0), None);
    }

    #[test]
    fn colour_zero_is_transparent() {
        let data = build(1, 1, &[0]);
        // Byte 0 of the tile holds colour 0 in both nibbles.
        let image = Picture::parse(&data).unwrap().image();
        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn nibbles_are_read_low_first() {
        let mut data = build(1, 1, &[0]);
        // Tile 0, first row's first byte: pixels 0 and 1 are colours 1 and 2.
        data[HEADER_SIZE] = 0x21;
        let image = Picture::parse(&data).unwrap().image();
        assert_eq!(image.pixel(0, 0), Some(nitro2d::colour_rgba(0x0421)));
        assert_eq!(image.pixel(1, 0), Some(nitro2d::colour_rgba(2 * 0x0421)));
    }

    #[test]
    fn rejects_broken_pictures() {
        let good = build(2, 2, &[0, 1, 2, 3]);
        assert!(Picture::parse(&good).is_ok());
        assert!(Picture::parse(&good[..good.len() - 1]).is_err());
        // A cell past the tile count.
        let mut past = good.clone();
        let last = past.len() - 1;
        past[last] = 0;
        past[last - 1] = 0x10;
        assert!(Picture::parse(&past).is_err());
        // A flip bit, absent from the cartridge.
        let mut flip = good.clone();
        let last = flip.len() - 1;
        flip[last] = 0x04;
        flip[last - 1] = 0x00;
        assert!(Picture::parse(&flip).is_err());
        // No tiles, but a cell naming one.
        let mut empty = build(1, 1, &[0]);
        empty.drain(HEADER_SIZE..HEADER_SIZE + TILE_BYTES);
        empty[4..6].copy_from_slice(&0u16.to_le_bytes());
        empty[HEADER_SIZE] = 4;
        assert!(Picture::parse(&empty).is_err());
    }
}
