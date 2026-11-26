//! Nitro 2D graphics files: `NCLR` palettes, `NCGR` characters and `NCER` cells (OAM parts).
//!
//! File and block framing, the palette and character blocks and the parts' attributes are
//! those measured on the cartridge in `docs/re/minimap.md` (source: the DQIX formats wiki,
//! `2D-Graphics`; re-read on our ROM: 501 files). Malformed input yields [`Error`]; nothing
//! here panics.

use crate::bytes::{magic_at, u16_at, u32_at};
use crate::{Error, Result};

/// Bytes every 2D file and every block starts with.
pub const HEADER_SIZE: usize = 0x10;
/// Bytes of a block's head: the reversed stamp and the block's size, head included.
pub const BLOCK_HEAD: usize = 8;
/// Bytes of one 8×8 four-bit character.
pub const TILE_BYTES: usize = 32;

const BOM: u16 = 0xFEFF;
const VERSION_PALETTE_CELL: u16 = 0x0100;
const VERSION_CHARACTER: u16 = 0x0101;

/// What a 2D file holds, from its magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `NCLR`: palettes (`PLTT`, optionally `PCMP`).
    Palette,
    /// `NCGR`: characters (`CHAR`).
    Character,
    /// `NCER`: cells (`CEBK`).
    Cell,
}

impl Kind {
    /// The kind of a file from its first four bytes (`RLCN`, `RGCN`, `RECN`).
    pub fn of(data: &[u8]) -> Option<Self> {
        match &magic_at(data, 0).ok()? {
            b"RLCN" => Some(Self::Palette),
            b"RGCN" => Some(Self::Character),
            b"RECN" => Some(Self::Cell),
            _ => None,
        }
    }

    fn version(self) -> u16 {
        match self {
            Self::Palette | Self::Cell => VERSION_PALETTE_CELL,
            Self::Character => VERSION_CHARACTER,
        }
    }
}

/// The file head, shared by the three kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub version: u16,
    /// The declared file size, which must be the bytes given to the parser.
    pub size: usize,
}

/// Reads and validates the file head.
///
/// # Errors
/// [`Error::BadMagic`] for another file, [`Error::Malformed`] for a wrong byte-order mark,
/// version, header size, declared size, or a block count of zero.
pub fn header(data: &[u8]) -> Result<Header> {
    let kind = Kind::of(data).ok_or_else(|| Error::BadMagic {
        what: "Nitro 2D file",
        found: data.get(..4).map_or([0; 4], |m| [m[0], m[1], m[2], m[3]]),
    })?;
    if u16_at(data, 4)? != BOM {
        return Err(Error::Malformed("2D byte order mark"));
    }
    let version = u16_at(data, 6)?;
    if version != kind.version() {
        return Err(Error::Malformed("2D version"));
    }
    let size = u32_at(data, 8)? as usize;
    if size != data.len() {
        return Err(Error::Malformed("2D file size field"));
    }
    if u16_at(data, 12)? as usize != HEADER_SIZE {
        return Err(Error::Malformed("2D header size"));
    }
    if u16_at(data, 14)? == 0 {
        return Err(Error::Malformed("2D file without blocks"));
    }
    Ok(Header {
        kind,
        version,
        size,
    })
}

/// `true` when `data` is an `NCLR`, `NCGR` or `NCER` file with a valid head.
pub fn is_nitro2d(data: &[u8]) -> bool {
    header(data).is_ok()
}

/// A block's stamp and content (the stamp and size words excluded).
#[derive(Debug, Clone, Copy)]
pub struct Block<'a> {
    /// The reversed stamp as stored (`TTLP` for `PLTT`).
    pub stamp: [u8; 4],
    pub content: &'a [u8],
}

fn blocks<'a>(data: &'a [u8], head: &Header) -> Result<Vec<Block<'a>>> {
    let count = usize::from(u16_at(data, 14)?);
    let mut pos = HEADER_SIZE;
    let mut out = Vec::with_capacity(count.min(8));
    for _ in 0..count {
        let stamp = magic_at(data, pos)?;
        let size = u32_at(data, pos + 4)? as usize;
        let end = pos
            .checked_add(size)
            .filter(|&end| size >= BLOCK_HEAD && end <= head.size)
            .ok_or(Error::Malformed("2D block size"))?;
        out.push(Block {
            stamp,
            content: &data[pos + BLOCK_HEAD..end],
        });
        pos = end;
    }
    if pos != head.size {
        return Err(Error::Malformed("2D blocks do not end at the file size"));
    }
    Ok(out)
}

fn find<'a>(blocks: &[Block<'a>], stamp: &[u8; 4]) -> Option<&'a [u8]> {
    blocks
        .iter()
        .find(|b| b.stamp.map(|c| c.to_ascii_uppercase()) == *stamp)
        .map(|b| b.content)
}

/// A BGR555 colour as RGBA8, opaque: five bits per channel, scaled to eight.
pub fn colour_rgba(colour: u16) -> [u8; 4] {
    let expand = |v: u16| {
        let v = (v & 0x1F) as u8;
        (v << 3) | (v >> 2)
    };
    [
        expand(colour),
        expand(colour >> 5),
        expand(colour >> 10),
        0xFF,
    ]
}

/// One `NCLR` file: every stored palette, decoded, and the slot each fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nclr {
    pub version: u16,
    /// 16 (depth 3) or 256 (depth 4) colours per palette.
    pub colours_per_palette: usize,
    /// The slot each stored palette fills, in stored order (`PCMP`), or `0..` without one.
    pub slots: Vec<u16>,
    /// The stored palettes, in stored order; a part's palette number names a *slot*.
    pub palettes: Vec<Vec<[u8; 4]>>,
}

impl Nclr {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let head = header(data)?;
        if head.kind != Kind::Palette {
            return Err(Error::Malformed("not an NCLR file"));
        }
        let blocks = blocks(data, &head)?;
        let pltt = find(&blocks, b"TTLP").ok_or(Error::Malformed("NCLR without PLTT"))?;
        let depth = u32_at(pltt, 0)?;
        if !(3..=4).contains(&depth) {
            return Err(Error::Malformed("NCLR palette depth"));
        }
        // Depth is the number of bits per pixel: 4 bpp is sixteen colours, 8 bpp 256.
        let colours_per_palette = 1usize << (1usize << (depth - 1));
        let colours_offset = u32_at(pltt, 12)? as usize;
        let colours = pltt
            .get(colours_offset..)
            .ok_or(Error::Malformed("NCLR colours offset"))?;
        let palette_bytes = colours_per_palette * 2;
        if colours.len() % palette_bytes != 0 {
            return Err(Error::Malformed("NCLR colours do not fill palettes"));
        }
        let stored = colours.len() / palette_bytes;
        let palettes: Vec<Vec<[u8; 4]>> = colours
            .chunks_exact(palette_bytes)
            .map(|p| {
                p.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| colour_rgba(u16::from_le_bytes(*c)))
                    .collect()
            })
            .collect();
        let slots = match find(&blocks, b"PMCP") {
            Some(pcmp) => {
                let count = usize::from(u16_at(pcmp, 0)?);
                if count != stored {
                    return Err(Error::Malformed("NCLR PCMP count"));
                }
                let offset = u32_at(pcmp, 4)? as usize;
                let list = pcmp
                    .get(offset..offset + 2 * count)
                    .ok_or(Error::Malformed("NCLR PCMP slot list"))?;
                list.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| u16::from_le_bytes(*s))
                    .collect()
            }
            None => (0..stored as u16).collect(),
        };
        Ok(Self {
            version: head.version,
            colours_per_palette,
            slots,
            palettes,
        })
    }

    /// The stored palette filling `slot`, if any.
    pub fn palette(&self, slot: u16) -> Option<&[[u8; 4]]> {
        let index = self.slots.iter().position(|&s| s == slot)?;
        self.palettes.get(index).map(Vec::as_slice)
    }
}

/// One `NCGR` file: four-bit 8×8 characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ncgr<'a> {
    pub version: u16,
    /// Declared height and width in tiles; `0xFFFF` for none, as on the cartridge.
    pub height: u16,
    pub width: u16,
    pub depth: u32,
    /// The OBJ VRAM mode word as stored (`0x10`, or `0x200010` beside mapping 2).
    pub vram_mode: u32,
    /// `1` for bitmap data.
    pub bitmap: u32,
    /// The characters: eight rows of four bytes each, low nibble left.
    pub chars: &'a [u8],
}

impl<'a> Ncgr<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let head = header(data)?;
        if head.kind != Kind::Character {
            return Err(Error::Malformed("not an NCGR file"));
        }
        let blocks = blocks(data, &head)?;
        let char_block = find(&blocks, b"RAHC").ok_or(Error::Malformed("NCGR without CHAR"))?;
        let depth = u32_at(char_block, 4)?;
        if depth != 3 {
            return Err(Error::Malformed("NCGR character depth"));
        }
        let size = u32_at(char_block, 16)? as usize;
        let offset = u32_at(char_block, 20)? as usize;
        let chars = char_block
            .get(offset..)
            .ok_or(Error::Malformed("NCGR characters offset"))?;
        if size != chars.len() {
            return Err(Error::Malformed("NCGR character size"));
        }
        if chars.len() % TILE_BYTES != 0 {
            return Err(Error::Malformed("NCGR characters do not fill tiles"));
        }
        Ok(Self {
            version: head.version,
            height: u16_at(char_block, 0)?,
            width: u16_at(char_block, 2)?,
            depth,
            vram_mode: u32_at(char_block, 8)?,
            bitmap: u32_at(char_block, 12)?,
            chars,
        })
    }

    /// Number of 8×8 characters.
    pub fn tile_count(&self) -> usize {
        self.chars.len() / TILE_BYTES
    }

    /// Character `index`: eight rows of four bytes, low nibble left.
    pub fn tile(&self, index: usize) -> Option<&'a [u8]> {
        let start = index.checked_mul(TILE_BYTES)?;
        self.chars.get(start..start + TILE_BYTES)
    }
}

/// A cell part's shape (OAM attribute 0, bits 14-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Square,
    Wide,
    Tall,
    Forbidden,
}

impl Shape {
    fn of(attribute: u16) -> Self {
        match attribute >> 14 {
            0 => Self::Square,
            1 => Self::Wide,
            2 => Self::Tall,
            _ => Self::Forbidden,
        }
    }

    /// The part's size in pixels for OAM size `size` (0-3), or `None` for shape 3.
    pub fn dimensions(self, size: u8) -> Option<(u16, u16)> {
        let size = usize::from(size & 3);
        Some(match self {
            Self::Square => [(8, 8), (16, 16), (32, 32), (64, 64)][size],
            Self::Wide => [(16, 8), (32, 8), (32, 16), (64, 32)][size],
            Self::Tall => [(8, 16), (8, 32), (16, 32), (32, 64)][size],
            Self::Forbidden => return None,
        })
    }
}

/// One OAM part of a cell: GBATEK's three attribute words, decoded.
///
/// `x` and `y` are the part's top-left corner in cell pixels. They are read as signed (nine
/// bits and eight bits): a centred part has negative coordinates. That sign is the wiki's
/// only inferred point of the format; every cell drawn from it with [x centred] looks right
/// (see `docs/re/minimap.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    pub x: i16,
    pub y: i16,
    pub shape: Shape,
    /// OAM size 0-3, for [`Shape::dimensions`].
    pub size: u8,
    pub flip_x: bool,
    pub flip_y: bool,
    /// Rotation/scale parameter selection for an affine part; the matrix itself is set at run
    /// time, so the file alone does not give its appearance.
    pub affine: Option<u8>,
    pub tile: u16,
    /// Palette slot, not a stored palette index.
    pub palette: u8,
    pub priority: u8,
}

impl Part {
    fn parse(word: &[u8; 6]) -> Result<Self> {
        let attribute0 = u16::from_le_bytes([word[0], word[1]]);
        let attribute1 = u16::from_le_bytes([word[2], word[3]]);
        let attribute2 = u16::from_le_bytes([word[4], word[5]]);
        let affine = attribute0 & 0x0100 != 0;
        let raw_x = (attribute1 & 0x01FF) as i16;
        Ok(Self {
            x: if raw_x >= 0x100 { raw_x - 0x200 } else { raw_x },
            y: i16::from(attribute0 as u8 as i8),
            shape: Shape::of(attribute0),
            size: (attribute1 >> 14) as u8,
            flip_x: !affine && attribute1 & 0x1000 != 0,
            flip_y: !affine && attribute1 & 0x2000 != 0,
            affine: affine.then_some(((attribute1 >> 9) & 0x1F) as u8),
            tile: attribute2 & 0x03FF,
            palette: (attribute2 >> 12) as u8,
            priority: (attribute2 >> 10) as u8 & 3,
        })
    }
}

/// A cell's bounding box in cell pixels: the union of its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// One cell: its own (unestablished) attribute and its parts, the first drawn in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub attribute: u16,
    pub parts: Vec<Part>,
}

impl Cell {
    /// The union of the parts' boxes; `None` for a cell without parts.
    pub fn bounds(&self) -> Option<Rect> {
        let mut min_x = i16::MAX;
        let mut min_y = i16::MAX;
        let mut max_x = i16::MIN;
        let mut max_y = i16::MIN;
        for part in &self.parts {
            let (width, height) = part.shape.dimensions(part.size)?;
            min_x = min_x.min(part.x);
            min_y = min_y.min(part.y);
            max_x = max_x.max(part.x.saturating_add(width as i16));
            max_y = max_y.max(part.y.saturating_add(height as i16));
        }
        if min_x > max_x {
            return None;
        }
        Some(Rect {
            x: min_x,
            y: min_y,
            width: (max_x - min_x) as u16,
            height: (max_y - min_y) as u16,
        })
    }
}

/// One `NCER` file: cells of parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ncer {
    pub version: u16,
    /// The CHAR mapping: 0-3 one-dimensional, 4 two-dimensional.
    pub mapping: u32,
    pub cells: Vec<Cell>,
}

impl Ncer {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let head = header(data)?;
        if head.kind != Kind::Cell {
            return Err(Error::Malformed("not an NCER file"));
        }
        let blocks = blocks(data, &head)?;
        let cebk = find(&blocks, b"KBEC").ok_or(Error::Malformed("NCER without CEBK"))?;
        let count = usize::from(u16_at(cebk, 0)?);
        let boxes = u16_at(cebk, 2)? != 0;
        let cells_offset = u32_at(cebk, 4)? as usize;
        let mapping = u32_at(cebk, 8)?;
        if mapping > 4 {
            return Err(Error::Malformed("NCER mapping"));
        }
        // Cells run from `cells_offset`, each followed by its box when the flag is set; the
        // parts of every cell follow the table, at the cell's own offset from its end.
        let stride = if boxes { 16 } else { 8 };
        let table_end = cells_offset
            .checked_add(count * stride)
            .ok_or(Error::Malformed("NCER cell table"))?;
        if table_end > cebk.len() {
            return Err(Error::Malformed("NCER cell table past block end"));
        }
        let mut cells = Vec::with_capacity(count.min(256));
        for i in 0..count {
            let at = cells_offset + i * stride;
            let parts_count = usize::from(u16_at(cebk, at)?);
            let attribute = u16_at(cebk, at + 2)?;
            let parts_offset = u32_at(cebk, at + 4)? as usize;
            let start = table_end
                .checked_add(parts_offset)
                .ok_or(Error::Malformed("NCER parts offset"))?;
            let bytes = cebk
                .get(start..)
                .and_then(|rest| rest.get(..parts_count * 6))
                .ok_or(Error::Malformed("NCER parts past block end"))?;
            let parts = bytes
                .as_chunks::<6>()
                .0
                .iter()
                .map(Part::parse)
                .collect::<Result<_>>()?;
            cells.push(Cell { attribute, parts });
        }
        Ok(Self {
            version: head.version,
            mapping,
            cells,
        })
    }

    /// Bytes a tile *number* stands for: `32 << mapping` in one-dimensional mapping.
    ///
    /// # Errors
    /// [`Error::Malformed`] for two-dimensional mapping (4), absent from the cartridge.
    pub fn tile_bytes(&self) -> Result<usize> {
        if self.mapping > 3 {
            return Err(Error::Malformed("two-dimensional CHAR mapping"));
        }
        Ok(TILE_BYTES << self.mapping)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(kind: Kind, blocks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = kind_stamp(kind).to_vec();
        out.extend(BOM.to_le_bytes());
        out.extend(kind.version().to_le_bytes());
        out.extend(0u32.to_le_bytes()); // size, patched below
        out.extend((HEADER_SIZE as u16).to_le_bytes());
        out.extend((blocks.len() as u16).to_le_bytes());
        for (stamp, content) in blocks {
            out.extend(stamp.iter());
            out.extend(((content.len() + BLOCK_HEAD) as u32).to_le_bytes());
            out.extend(content);
        }
        let size = out.len() as u32;
        out[8..12].copy_from_slice(&size.to_le_bytes());
        out
    }

    fn kind_stamp(kind: Kind) -> [u8; 4] {
        match kind {
            Kind::Palette => *b"RLCN",
            Kind::Character => *b"RGCN",
            Kind::Cell => *b"RECN",
        }
    }

    /// `depth` 3 is sixteen colours a palette, 4 is 256.
    fn nclr_file(depth: u32, palettes: &[&[u16]], slots: Option<&[u16]>) -> Vec<u8> {
        let colours: Vec<u8> = palettes
            .iter()
            .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
            .collect();
        let mut pltt = Vec::new();
        for word in [depth, 0, 0, 0x10] {
            pltt.extend(word.to_le_bytes());
        }
        pltt.extend(&colours);
        let mut blocks = vec![(*b"TTLP", pltt)];
        if let Some(slots) = slots {
            let mut pcmp = Vec::new();
            pcmp.extend((slots.len() as u16).to_le_bytes());
            pcmp.extend(0xBEEFu16.to_le_bytes());
            pcmp.extend(8u32.to_le_bytes());
            slots.iter().for_each(|s| pcmp.extend(s.to_le_bytes()));
            blocks.push((*b"PMCP", pcmp));
        }
        file(Kind::Palette, &blocks)
    }

    fn ncgr_file(chars: &[u8]) -> Vec<u8> {
        let mut char_block = Vec::new();
        char_block.extend(0xFFFFu16.to_le_bytes());
        char_block.extend(0xFFFFu16.to_le_bytes());
        char_block.extend(3u32.to_le_bytes());
        char_block.extend(0x10u32.to_le_bytes());
        char_block.extend(0u32.to_le_bytes());
        char_block.extend((chars.len() as u32).to_le_bytes());
        char_block.extend(0x18u32.to_le_bytes());
        char_block.extend(chars);
        file(Kind::Character, &[(*b"RAHC", char_block)])
    }

    fn part(x: i16, y: i16, shape: u16, size: u16, tile: u16, palette: u8) -> [u8; 6] {
        let attribute0 = ((y as u8) as u16) | shape << 14;
        let attribute1 = (x as u16 & 0x1FF) | (size & 3) << 14;
        let attribute2 = tile | u16::from(palette) << 12;
        let mut out = [0; 6];
        out[0..2].copy_from_slice(&attribute0.to_le_bytes());
        out[2..4].copy_from_slice(&attribute1.to_le_bytes());
        out[4..6].copy_from_slice(&attribute2.to_le_bytes());
        out
    }

    fn ncer_file(cells: &[&[[u8; 6]]], boxes: bool, mapping: u32) -> Vec<u8> {
        let stride = if boxes { 16 } else { 8 };
        let table_end = 0x18 + cells.len() * stride;
        let mut cebk = Vec::new();
        cebk.extend((cells.len() as u16).to_le_bytes());
        cebk.extend(u16::from(boxes).to_le_bytes());
        cebk.extend(0x18u32.to_le_bytes());
        cebk.extend(mapping.to_le_bytes());
        cebk.extend(0u32.to_le_bytes());
        cebk.extend(0u32.to_le_bytes());
        cebk.extend(0u32.to_le_bytes());
        let mut parts_offset = 0usize;
        for cell in cells {
            cebk.extend((cell.len() as u16).to_le_bytes());
            cebk.extend(0u16.to_le_bytes());
            cebk.extend((parts_offset as u32).to_le_bytes());
            if boxes {
                cebk.extend([0u8; 8]);
            }
            parts_offset += cell.len() * 6;
        }
        for cell in cells {
            for p in cell.iter() {
                cebk.extend(*p);
            }
        }
        assert_eq!(cebk.len(), table_end + parts_offset);
        file(Kind::Cell, &[(*b"KBEC", cebk)])
    }

    #[test]
    fn reads_palettes_and_slots() {
        let red = [0x001Fu16; 16];
        let green = [0x03E0u16; 16];
        let data = nclr_file(3, &[&red, &green], Some(&[0, 1]));
        let nclr = Nclr::parse(&data).unwrap();
        assert_eq!(nclr.colours_per_palette, 16);
        assert_eq!(nclr.slots, [0, 1]);
        assert_eq!(nclr.palette(0).unwrap()[0], [255, 0, 0, 255]);
        assert_eq!(nclr.palette(1).unwrap()[0], [0, 255, 0, 255]);
        assert!(nclr.palette(2).is_none());
    }

    #[test]
    fn depth_four_is_256_colours_a_palette() {
        let data = nclr_file(4, &[&[0x001Fu16; 256]], None);
        let nclr = Nclr::parse(&data).unwrap();
        assert_eq!(nclr.colours_per_palette, 256);
        assert_eq!(nclr.palettes[0].len(), 256);
    }

    #[test]
    fn without_pcmp_slots_count_from_zero() {
        let data = nclr_file(3, &[&[0x001Fu16; 16]], None);
        let nclr = Nclr::parse(&data).unwrap();
        assert_eq!(nclr.slots, [0]);
    }

    #[test]
    fn rejects_bad_palette_files() {
        let one = nclr_file(3, &[&[0u16; 16]], None);
        let mut magic = one.clone();
        magic[0] = b'X';
        assert!(Nclr::parse(&magic).is_err());
        let mut version = one.clone();
        version[6] = 1;
        assert!(Nclr::parse(&version).is_err());
        let mut size = one.clone();
        size[8] = size[8].wrapping_add(1);
        assert!(Nclr::parse(&size).is_err());
        // Colours that do not fill their last palette.
        let mut truncated = nclr_file(3, &[&[0u16; 16], &[0u16; 16]], None);
        truncated.pop();
        let size = truncated.len() as u32;
        truncated[8..12].copy_from_slice(&size.to_le_bytes());
        assert!(Nclr::parse(&truncated).is_err());
        // Two stored palettes, a PCMP that says one.
        let pcmp_one = nclr_file(3, &[&[0u16; 16], &[0u16; 16]], Some(&[0]));
        assert!(Nclr::parse(&pcmp_one).is_err());
    }

    #[test]
    fn reads_characters() {
        let chars: Vec<u8> = (0..TILE_BYTES as u8 * 2).collect();
        let data = ncgr_file(&chars);
        let ncgr = Ncgr::parse(&data).unwrap();
        assert_eq!(ncgr.tile_count(), 2);
        assert_eq!(ncgr.tile(1).unwrap(), &chars[TILE_BYTES..]);
        assert!(ncgr.tile(2).is_none());
        // Characters that do not fill a tile are rejected.
        assert!(Ncgr::parse(&ncgr_file(&chars[..40])).is_err());
    }

    #[test]
    fn reads_cells_and_parts() {
        let panel = [part(0, 0, 0, 3, 8, 0)];
        let dot = [part(0, 0, 0, 0, 0x48, 4)];
        let centred = [part(-16, -4, 1, 0, 0, 0)];
        let ncer = Ncer::parse(&ncer_file(&[&panel, &dot, &centred], false, 0)).unwrap();
        assert_eq!(ncer.tile_bytes().unwrap(), 32);
        assert_eq!(ncer.cells.len(), 3);
        assert_eq!(
            ncer.cells[0].bounds(),
            Some(Rect {
                x: 0,
                y: 0,
                width: 64,
                height: 64
            })
        );
        assert_eq!(ncer.cells[1].parts[0].palette, 4);
        assert_eq!(ncer.cells[1].parts[0].tile, 0x48);
        assert_eq!(ncer.cells[2].parts[0].x, -16);
        assert_eq!(ncer.cells[2].bounds().unwrap().width, 16);
        let mapped = ncer_file(&[&panel], false, 2);
        assert_eq!(Ncer::parse(&mapped).unwrap().tile_bytes().unwrap(), 128);
    }

    #[test]
    fn rejects_bad_cell_files() {
        let small = ncer_file(&[&[part(0, 0, 0, 0, 0, 0)]], false, 0);
        let mut mapping = small.clone();
        mapping[0x18 + 8] = 5;
        assert!(Ncer::parse(&mapping).is_err());
        let mut count = small.clone();
        count[0x18] = 9;
        assert!(Ncer::parse(&count).is_err());
        let mut parts = small.clone();
        parts[0x18 + 4] = 0x40;
        assert!(Ncer::parse(&parts).is_err());
    }

    #[test]
    fn boxed_cells_skip_their_boxes() {
        let cell = [part(0, 0, 0, 0, 0, 0)];
        let ncer = Ncer::parse(&ncer_file(&[&cell, &cell], true, 0)).unwrap();
        assert_eq!(ncer.cells.len(), 2);
        assert_eq!(ncer.cells[1].parts[0].tile, 0);
    }

    #[test]
    fn detects_kind() {
        assert_eq!(Kind::of(b"RLCN...."), Some(Kind::Palette));
        assert_eq!(Kind::of(b"RGCN...."), Some(Kind::Character));
        assert_eq!(Kind::of(b"RECN...."), Some(Kind::Cell));
        assert_eq!(Kind::of(b"GPC2...."), None);
    }
}
