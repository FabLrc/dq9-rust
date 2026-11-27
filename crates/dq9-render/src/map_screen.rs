//! The field's map screen: the original's top LCD during the field (`re/bottom-screen.md` § 2.1),
//! drawn over the finished panel in screen pixels. Layer split: this module decodes the map
//! screen's data from file bytes and lays out its quads; the application reads the ROM, passes
//! the party's positions and names and draws the result (`app/draw.rs`).
//!
//! What is drawn, and what establishes it:
//!
//! - the backdrop: the `minimapbg2`-`4` `.obg` (8×8-cell paper) the layout's tag `0x6a` names
//!   (`docs/re/minimap.md` § 1, § 3), tiled at one texel per pixel over the whole 256×192
//!   screen from its top-left and fixed: the original stages it on sub BG1 and fills the
//!   32×24-tile screen with it (`0x02020fc4`, `docs/re/minimap.md` § 4.2, [`backdrop_tiles`]);
//! - the picture: the `.obg` the map's `.bmmp` names (`docs/re/minimap.md` § 2), at one pixel
//!   per texel, scrolled so the leader's picture pixel sits at the screen's (128, 96)
//!   (`0x02027438`, `0x02024518`, § 4.2): [`follow`] reproduces the original's 30-pixel
//!   horizontal and 10/20-pixel vertical dead zone; maps whose map kind is 0 or 7 do not use
//!   this mode and keep their picture at its top-left ([`picture_dst`], [unknown]);
//! - the party strips: cell 2 of `obj_minimap.NCER`, rows 0..15, 64×16 at y 176, one per member
//!   in party order, palette slot 0..3 (blue, green, pink, orange in the file);
//! - the party dots: cells 3..7 (8×8, palette slot 4) at the member's projected position
//!   (`pixel = position × scale − corner × 8`, rounded);
//! - the area tab is the application's (`Atlas`), from the capture's geometry.
//!
//! The dot and strip colours of a real party are not the file's (`docs/re/minimap.md` § 5,
//! `tools/wiki/Mini-Map.md`): the file's slots are used, a **[hypothesis]**.
//!
//! The layout's marks (tag `0x6c`) are decoded and projected ([`Content::marks`]) but not
//! drawn: the five `marker0`-`4` dots are blue, green, pink, yellow and red in that order
//! (`docs/re/minimap.md` § 1), and nothing establishes which marker an ordinary door or house
//! mark takes — no record carries an index, and the note leaves the other dots' meaning
//! [unknown] (§ 6). A colour is not invented ([`Frame::new`]).

use std::fmt;

use dq9_formats::Gpc2;
use dq9_formats::minimap::{Layout, Mark, Picture, party_strip};
use nds_rom::nitro2d::{Ncer, Ncgr, Nclr};
use wgpu::util::DeviceExt as _;

use crate::renderer::Viewport;

/// The archive the field's map screen loads (mode 0, `MapScreen_Load`'s `func_02020b98`).
pub const ARCHIVE: &str = "data/pack_lv5/minimap.gp2";
/// The loose Nitro 2D files the party strips and dots come from.
pub const SPRITE_CELLS: &str = "data/ani/obj_minimap.NCER";
pub const SPRITE_CHARS: &str = "data/ani/obj_minimap.NCGR";
pub const SPRITE_PALETTE: &str = "data/ani/obj_minimap.NCLR";

/// Width and height of the DS screen this layer draws on.
pub const SCREEN: (u32, u32) = (256, 192);
/// The party strip's slot pitch, width and top edge (`re/bottom-screen.md` § 2.1).
pub const STRIP_PITCH: u32 = 64;
pub const STRIP_Y: u32 = 176;
/// The dots' size: one 8×8 part each.
pub const DOT_SIZE: u32 = 8;
/// The screen point the original's scrolling window puts the leader at: sub BG0 is scrolled
/// to `(scroll mod 8) + (128, 32)` and its tilemap is blitted at
/// `corner + entry - scroll / 8 + (32, 16)`, so a picture pixel lands at
/// `pixel - scroll + (128, 96)` (`0x02027438`, `0x02024518`).
pub const ANCHOR: (f32, f32) = (128.0, 96.0);
/// The scroll follows the leader only past this horizontal dead zone, in pixels
/// (`0x02024518`: the difference is clamped to 30 either way).
pub const DEAD_ZONE_X: f32 = 30.0;
/// The vertical dead zone: the leader may fall 10 pixels below the anchor or 20 above it
/// before the picture follows (`0x02024518`).
pub const DEAD_ZONE_BELOW: f32 = 10.0;
pub const DEAD_ZONE_ABOVE: f32 = 20.0;
/// Member slots a party can fill (`dq9_core::party::PARTY_MAX`).
pub const MEMBERS: usize = 4;
/// The area tab's rectangle and colours, measured on the original's capture (a dark tab with a
/// light edge at the picture's top right); the original's own palette source is [unknown].
pub const TAB: [u32; 4] = [200, 0, 54, 16];
pub const TAB_EDGE: [f32; 4] = [211.0 / 255.0, 146.0 / 255.0, 48.0 / 255.0, 1.0];
pub const TAB_FILL: [f32; 4] = [97.0 / 255.0, 65.0 / 255.0, 40.0 / 255.0, 1.0];
pub const TAB_TEXT: [f32; 4] = [1.0; 4];

/// An RGBA8 image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// Size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Why the map screen cannot be drawn: the placeholder stays and the application says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No `.bmmp` layout names this map code.
    NoLayout(String),
    /// The layout's picture is not in `minimap.gp2`.
    NoPicture(String),
    /// An archive, picture or sprite file does not parse.
    Format(String),
    /// `obj_minimap` has no party panel cell to cut the strip from.
    NoStrip,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLayout(code) => write!(f, "no .bmmp layout for {code}"),
            Self::NoPicture(name) => write!(f, "no {name} picture in {ARCHIVE}"),
            Self::Format(what) => write!(f, "{what}"),
            Self::NoStrip => write!(f, "{SPRITE_CELLS} has no party panel cell"),
        }
    }
}

impl std::error::Error for Error {}

/// What the top screen draws for one map: the decoded picture, the party sprite sheet and the
/// layout's projection.
#[derive(Debug, Clone, PartialEq)]
pub struct Content {
    /// The `.obg` picture, colour 0 transparent.
    pub picture: Image,
    /// The backdrop the layout names (tag `0x6a`), drawn under the picture; `None` when the
    /// layout has none or the archive does not hold it.
    pub backdrop: Option<Image>,
    /// The sprite sheet: the four 64-pixel strips at rows 0..16, the five dots at row 16.
    pub sprites: Image,
    /// The strip cut from the party panel (64×16).
    pub strip: (u32, u32),
    /// Pixels per map unit (`.bmmp` tag 0x69).
    pub scale: f32,
    /// The picture's corner in tiles (`.bmmp` tag 0x64).
    pub corner: (f32, f32),
    /// The layout's marks (`.bmmp` tag 0x6c), projected but not drawn: which of the five
    /// markers a mark takes is [unknown].
    pub marks: Vec<Mark>,
}

/// Decodes the map screen of `map` (a map code such as `M01` or `M01M01`) from the four files
/// the original loads. `archive` is `minimap.gp2`, the three others the `obj_minimap` files.
///
/// The layout is `<map>.bmmp`, else the layout listing `map` among its `0x70` codes (a room of
/// an area draws the area's picture, `docs/re/minimap.md` § 3).
///
/// # Errors
/// [`Error`] with the datum that is missing; nothing is drawn then.
pub fn content(
    archive: &[u8],
    ncer: &[u8],
    ncgr: &[u8],
    nclr: &[u8],
    map: &str,
) -> Result<Content, Error> {
    let archive = Gpc2::parse(archive).map_err(|e| Error::Format(format!("{ARCHIVE}: {e}")))?;
    let (name, data) = find_layout(&archive, map)?;
    let layout = Layout::parse(&data).map_err(|e| Error::Format(format!("{name}: {e}")))?;
    let picture_name = layout
        .picture()
        .ok_or_else(|| Error::Format(format!("{name} names no picture")))?;
    let scale = layout
        .scale()
        .ok_or_else(|| Error::Format(format!("{name} has no scale")))?;
    let corner = layout
        .corner()
        .ok_or_else(|| Error::Format(format!("{name} has no corner")))?;
    let entry = archive
        .find(picture_name)
        .or_else(|| archive.find(&format!("{picture_name}.obg")))
        .ok_or_else(|| Error::NoPicture(picture_name.to_owned()))?;
    let bytes = archive
        .member(entry)
        .map_err(|e| Error::Format(format!("{picture_name}: {e}")))?;
    let picture = decode(
        Picture::parse(&bytes)
            .map_err(|e| Error::Format(format!("{picture_name}: {e}")))?
            .image(),
        picture_name,
    )?;
    let backdrop = backdrop(&archive, layout.backdrop())?;
    let (sprites, strip) = sprite_sheet(ncer, ncgr, nclr)?;
    Ok(Content {
        picture,
        backdrop,
        sprites,
        strip,
        scale,
        corner,
        marks: layout.marks(),
    })
}

/// The backdrop the layout names: the `.obg` member of the same archive, `None` when the layout
/// has no tag `0x6a` or the archive does not hold the name (no draw, not an error).
fn backdrop(archive: &Gpc2<'_>, name: Option<&str>) -> Result<Option<Image>, Error> {
    let Some(name) = name else { return Ok(None) };
    let entry = archive
        .find(name)
        .or_else(|| archive.find(&format!("{name}.obg")));
    let Some(entry) = entry else { return Ok(None) };
    let bytes = archive
        .member(entry)
        .map_err(|e| Error::Format(format!("{name}: {e}")))?;
    let image = Picture::parse(&bytes)
        .map_err(|e| Error::Format(format!("{name}: {e}")))?
        .image();
    if image.width == 0 || image.height == 0 {
        return Ok(None);
    }
    let image = Image {
        width: image.width as u32,
        height: image.height as u32,
        rgba: image.rgba.into_iter().flatten().collect(),
    };
    Ok(Some(image))
}

/// The RGBA8 image of a decoded `.obg`, as the screen's [`Image`].
fn decode(image: dq9_formats::minimap::Image, label: &str) -> Result<Image, Error> {
    if image.width == 0 || image.height == 0 {
        return Err(Error::Format(format!("{label} has no pixels")));
    }
    Ok(Image {
        width: image.width as u32,
        height: image.height as u32,
        rgba: image.rgba.into_iter().flatten().collect(),
    })
}

/// Finds the layout of a map: `<map>.bmmp`, else the layout listing `map` among its codes.
fn find_layout<'a>(
    archive: &'a Gpc2<'a>,
    map: &str,
) -> Result<(String, std::borrow::Cow<'a, [u8]>), Error> {
    if let Some(entry) = archive.find(&format!("{map}.bmmp")) {
        let data = archive
            .member(entry)
            .map_err(|e| Error::Format(format!("{}: {e}", entry.name)))?;
        return Ok((entry.name.clone(), data));
    }
    for entry in archive.entries.iter().filter(|e| e.name.ends_with(".bmmp")) {
        let data = archive
            .member(entry)
            .map_err(|e| Error::Format(format!("{}: {e}", entry.name)))?;
        let layout =
            Layout::parse(&data).map_err(|e| Error::Format(format!("{}: {e}", entry.name)))?;
        if layout
            .codes()
            .iter()
            .any(|(_, code)| code.eq_ignore_ascii_case(map))
        {
            return Ok((entry.name.clone(), data));
        }
    }
    Err(Error::NoLayout(map.to_owned()))
}

/// The party sprite sheet: the four name strips (palette slots 0..3 of the panel part) and the
/// five dots (cells 3..7, palette slot 4).
fn sprite_sheet(ncer: &[u8], ncgr: &[u8], nclr: &[u8]) -> Result<(Image, (u32, u32)), Error> {
    let cells = Ncer::parse(ncer).map_err(|e| Error::Format(format!("{SPRITE_CELLS}: {e}")))?;
    let chars = Ncgr::parse(ncgr).map_err(|e| Error::Format(format!("{SPRITE_CHARS}: {e}")))?;
    let palette = Nclr::parse(nclr).map_err(|e| Error::Format(format!("{SPRITE_PALETTE}: {e}")))?;
    let strip = party_strip(&cells.cells).ok_or(Error::NoStrip)?;
    let (strip_w, strip_h) = (u32::from(strip.width), u32::from(strip.height));
    if strip_w == 0 || strip_h == 0 {
        return Err(Error::NoStrip);
    }
    let height = strip_h + DOT_SIZE;
    let width = STRIP_PITCH * MEMBERS as u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let panel = cells
        .cells
        .get(dq9_formats::minimap::PARTY_PANEL_CELL)
        .and_then(|cell| cell.parts.first())
        .ok_or(Error::NoStrip)?;
    // The panel is eight tiles across, sixteen tall.
    let pitch = strip_w / 8;
    for slot in 0..MEMBERS as u32 {
        let Some(colours) = palette.palette(slot as u16) else {
            continue;
        };
        for y in 0..strip_h {
            for x in 0..strip_w {
                let tile = panel.tile + ((y / 8) * pitch + x / 8) as u16;
                let Some(colour) = pixel(&chars, colours, tile, x % 8, y % 8) else {
                    continue;
                };
                blit(&mut rgba, width, (STRIP_PITCH * slot + x, y), *colour);
            }
        }
    }
    // The dots follow the strips: cells 3..7, their own palette slot, one 8×8 tile each.
    let first_dot = *dq9_formats::minimap::DOT_CELLS.start();
    for (index, cell) in cells.cells.iter().skip(first_dot).take(MEMBERS).enumerate() {
        let Some(part) = cell.parts.first() else {
            continue;
        };
        let Some(colours) = palette.palette(u16::from(part.palette)) else {
            continue;
        };
        for y in 0..DOT_SIZE {
            for x in 0..DOT_SIZE {
                let Some(colour) = pixel(&chars, colours, part.tile, x, y) else {
                    continue;
                };
                blit(
                    &mut rgba,
                    width,
                    (index as u32 * DOT_SIZE + x, strip_h + y),
                    *colour,
                );
            }
        }
    }
    Ok((
        Image {
            width,
            height,
            rgba,
        },
        (strip_w, strip_h),
    ))
}

/// The RGBA of a 4-bit tile pixel, or `None` for colour 0 (transparent).
fn pixel<'a>(
    chars: &Ncgr<'_>,
    colours: &'a [[u8; 4]],
    tile: u16,
    x: u32,
    y: u32,
) -> Option<&'a [u8; 4]> {
    let data = chars.tile(usize::from(tile))?;
    let byte = *data.get((y * 4 + x / 2) as usize)?;
    let nibble = if x.is_multiple_of(2) {
        byte & 0x0F
    } else {
        byte >> 4
    };
    if nibble == 0 {
        return None;
    }
    colours.get(usize::from(nibble))
}

fn blit(rgba: &mut [u8], width: u32, at: (u32, u32), colour: [u8; 4]) {
    let offset = ((at.1 * width + at.0) * 4) as usize;
    if let Some(dst) = rgba.get_mut(offset..offset + 4) {
        dst.copy_from_slice(&colour);
    }
}

/// Where a map position falls on the picture: `pixel = position × scale − corner × 8`, x across
/// and z down, rounded to the nearest pixel (the wiki's pass mark lands on row 4).
pub fn project(position: (f32, f32), scale: f32, corner: (f32, f32)) -> (f32, f32) {
    (
        (position.0 * scale - corner.0 * 8.0).round(),
        (position.1 * scale - corner.1 * 8.0).round(),
    )
}

/// The picture's rectangle in screen pixels: its top-left at `offset`, one pixel per texel.
/// With `None` as the frame's scroll (a map kind that does not use the original's mode 0) the
/// caller passes (0, 0), the picture's top-left.
pub fn picture_dst(size: (u32, u32), offset: (f32, f32)) -> [f32; 4] {
    [offset.0, offset.1, size.0 as f32, size.1 as f32]
}

/// The scroll after moving the leader to `leader` (both in picture pixels): the original holds
/// the picture while the leader stays within 30 pixels horizontally, 10 below the anchor and
/// 20 above it, then jumps the difference past that
/// (`0x02024518`; `docs/re/minimap.md` § 4.2). The first frame's scroll is the leader's own
/// pixel, which puts its dot on [`ANCHOR`].
pub fn follow(previous: (f32, f32), leader: (f32, f32)) -> (f32, f32) {
    fn axis(previous: f32, leader: f32, below: f32, above: f32) -> f32 {
        let difference = leader - previous;
        if difference > below {
            leader - below
        } else if difference < -above {
            leader + above
        } else {
            previous
        }
    }
    (
        axis(previous.0, leader.0, DEAD_ZONE_X, DEAD_ZONE_X),
        axis(previous.1, leader.1, DEAD_ZONE_BELOW, DEAD_ZONE_ABOVE),
    )
}

/// The picture's top-left for a scroll: the leader's picture pixel sits on the screen's
/// [`ANCHOR`], so the top-left is `ANCHOR - scroll` (the sub BG0 viewport of `0x02027438`).
pub fn picture_offset(scroll: (f32, f32)) -> (f32, f32) {
    (ANCHOR.0 - scroll.0, ANCHOR.1 - scroll.1)
}

/// The backdrop's quads: the 8×8-cell paper pattern ([`Content::backdrop`], 64×64 pixels)
/// repeated at one texel per pixel over `screen`, top-left first, fixed. The original stages
/// the pattern on sub BG1 and fills the 32×24-tile screen with it from (0, 0)
/// (`0x02020fc4`); the last column and row are cut to fit.
///
/// Each entry is `(destination, texture coordinate)` in the screen's pixels and the backdrop's
/// texels.
pub fn backdrop_tiles(pattern: (u32, u32), screen: [f32; 4]) -> Vec<([f32; 4], [f32; 4])> {
    let (mut quads, pw, ph) = (Vec::new(), pattern.0 as f32, pattern.1 as f32);
    if pw <= 0.0 || ph <= 0.0 || screen[2] <= 0.0 || screen[3] <= 0.0 {
        return quads;
    }
    let [x, y, w, h] = screen;
    let mut top = 0.0;
    while top < h {
        let tile_h = (h - top).min(ph);
        let mut left = 0.0;
        while left < w {
            let tile_w = (w - left).min(pw);
            quads.push((
                [x + left, y + top, tile_w, tile_h],
                [0.0, 0.0, tile_w / pw, tile_h / ph],
            ));
            left += pw;
        }
        top += ph;
    }
    quads
}

/// The strip of party slot `slot`: 64 wide at y 176, `content.strip` tall.
pub fn strip_dst(content: &Content, slot: usize) -> [f32; 4] {
    [
        (slot as u32 * STRIP_PITCH) as f32,
        STRIP_Y as f32,
        content.strip.0 as f32,
        content.strip.1 as f32,
    ]
}

/// A dot of 8×8 centred on the projected pixel.
pub fn dot_dst(pixel: (f32, f32)) -> [f32; 4] {
    let half = DOT_SIZE as f32 / 2.0;
    [
        pixel.0 - half,
        pixel.1 - half,
        DOT_SIZE as f32,
        DOT_SIZE as f32,
    ]
}

/// The strip's rectangle in [`Content::sprites`].
pub fn strip_src(content: &Content, slot: usize) -> [u32; 4] {
    [
        slot as u32 * STRIP_PITCH,
        0,
        content.strip.0,
        content.strip.1,
    ]
}

/// The dot's rectangle in [`Content::sprites`]; `DOT_CELLS` 3..7 in order.
pub fn dot_src(content: &Content, slot: usize) -> [u32; 4] {
    [slot as u32 * DOT_SIZE, content.strip.1, DOT_SIZE, DOT_SIZE]
}

/// One textured rectangle of the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    /// Source in [`Content::sprites`].
    pub src: [u32; 4],
    /// Destination on the 256×192 screen.
    pub dst: [f32; 4],
}

/// What the top screen shows this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The picture's top-left on the screen: `ANCHOR - scroll` for a scrolling map, its own
    /// top-left (0, 0) for a fixed one.
    pub picture_offset: (f32, f32),
    /// The picture's size in texels.
    pub picture: (u32, u32),
    /// The backdrop's size in texels, tiled under the picture; `None` when the layout has none.
    pub backdrop: Option<(u32, u32)>,
    /// The sprite sheet's size in texels, for the sprites' texture coordinates.
    pub sheet: (u32, u32),
    /// The party strips and dots, hero first.
    pub sprites: Vec<Sprite>,
}

impl Frame {
    /// The frame of `content` for the party at `party` (map positions `x, z`), hero first: one
    /// strip and one dot per member, up to [`MEMBERS`]. The dots sit at each member's own field
    /// position (`v3-party-companions`; the app passes the followers' places).
    ///
    /// `scroll` is the original's scrolling window in picture pixels ([`follow`]); `None` keeps
    /// the picture at its top-left and the dots on their picture pixels (the draw modes 1 and 3
    /// the mode-0 loader picks for map kinds 7 and 0, `0x02021578`, [unknown]).
    pub fn new(content: &Content, party: &[(f32, f32)], scroll: Option<(f32, f32)>) -> Self {
        let mut sprites = Vec::with_capacity(party.len().min(MEMBERS) * 2);
        for (slot, &position) in party.iter().take(MEMBERS).enumerate() {
            sprites.push(Sprite {
                src: strip_src(content, slot),
                dst: strip_dst(content, slot),
            });
            let pixel = project(position, content.scale, content.corner);
            // The dot's 8×8 sprite is centred on the party's picture pixel once it is on screen.
            let on_screen = match scroll {
                Some(scroll) => (pixel.0 - scroll.0 + ANCHOR.0, pixel.1 - scroll.1 + ANCHOR.1),
                None => pixel,
            };
            sprites.push(Sprite {
                src: dot_src(content, slot),
                dst: dot_dst(on_screen),
            });
        }
        Self {
            picture_offset: scroll.map_or((0.0, 0.0), picture_offset),
            picture: content.picture.size(),
            backdrop: content.backdrop.as_ref().map(Image::size),
            sheet: content.sprites.size(),
            sprites,
        }
    }
}

const VERTEX_FLOATS: usize = 5;

/// Two triangles per rectangle: position, texture coordinate, layer (0 backdrop, 1 picture,
/// 2 sprites, bottom to top).
fn vertices(frame: &Frame) -> Vec<[f32; VERTEX_FLOATS]> {
    let quad = |dst: [f32; 4], uv: [f32; 4], layer: f32, out: &mut Vec<[f32; VERTEX_FLOATS]>| {
        let [x, y, w, h] = dst;
        let [u0, v0, u1, v1] = uv;
        let corner = |dx: f32, dy: f32| {
            [
                x + dx * w,
                y + dy * h,
                u0 + dx * (u1 - u0),
                v0 + dy * (v1 - v0),
                layer,
            ]
        };
        let (tl, tr, bl, br) = (
            corner(0.0, 0.0),
            corner(1.0, 0.0),
            corner(0.0, 1.0),
            corner(1.0, 1.0),
        );
        out.extend([tl, tr, bl, tr, br, bl]);
    };
    let mut out = Vec::with_capacity((1 + frame.sprites.len()) * 6);
    let (w, h) = (frame.picture.0 as f32, frame.picture.1 as f32);
    if w > 0.0 && h > 0.0 {
        let picture = picture_dst(frame.picture, frame.picture_offset);
        if let Some(pattern) = frame.backdrop {
            let screen = [0.0, 0.0, SCREEN.0 as f32, SCREEN.1 as f32];
            for (dst, uv) in backdrop_tiles(pattern, screen) {
                quad(dst, uv, 0.0, &mut out);
            }
        }
        quad(picture, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
    }
    let sheet_uv = |sprite: &Sprite, size: (u32, u32)| {
        let [sx, sy, sw, sh] = sprite.src;
        [
            sx as f32 / size.0 as f32,
            sy as f32 / size.1 as f32,
            (sx + sw) as f32 / size.0 as f32,
            (sy + sh) as f32 / size.1 as f32,
        ]
    };
    for sprite in &frame.sprites {
        let uv = sheet_uv(sprite, frame.sheet);
        quad(sprite.dst, uv, 2.0, &mut out);
    }
    out
}

/// GPU side: the pipeline and the sampler; the textures are per map ([`Layers`]).
#[derive(Debug)]
pub struct MapScreen {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

/// The map screen's textures and their bind group: the backdrop (a 1×1 clear pixel when the
/// layout has none), the picture and the sprite sheet.
#[derive(Debug)]
pub struct Layers {
    _backdrop: wgpu::Texture,
    _picture: wgpu::Texture,
    _sprites: wgpu::Texture,
    bind: wgpu::BindGroup,
}

fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &Image,
    label: &str,
) -> wgpu::Texture {
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: image.width.max(1),
                height: image.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &image.rgba,
    )
}

impl MapScreen {
    /// Creates the pipeline for targets of `color_format`.
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dq9 map screen shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map_screen/shader.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map screen"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map screen"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map screen"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (VERTEX_FLOATS * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("map screen"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    /// Uploads one map's backdrop, picture and sprite sheet.
    pub fn layers(&self, device: &wgpu::Device, queue: &wgpu::Queue, content: &Content) -> Layers {
        let clear = Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        let backdrop = texture(
            device,
            queue,
            content.backdrop.as_ref().unwrap_or(&clear),
            "map backdrop",
        );
        let picture = texture(device, queue, &content.picture, "map picture");
        let sprites = texture(device, queue, &content.sprites, "map sprites");
        let backdrop_view = backdrop.create_view(&wgpu::TextureViewDescriptor::default());
        let picture_view = picture.create_view(&wgpu::TextureViewDescriptor::default());
        let sprites_view = sprites.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map screen"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&picture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&sprites_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&backdrop_view),
                },
            ],
        });
        Layers {
            _backdrop: backdrop,
            _picture: picture,
            _sprites: sprites,
            bind,
        }
    }

    /// Draws `frame` inside `region` of `target`, keeping what the target holds elsewhere.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        region: Viewport,
        layers: &Layers,
        frame: &Frame,
    ) {
        if frame.picture.0 == 0 || frame.picture.1 == 0 || frame.sheet.0 == 0 {
            return;
        }
        let data = vertices(frame);
        if data.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map screen vertices"),
            contents: bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut pass = crate::gpu::load_pass(encoder, "map screen", target);
        pass.set_viewport(
            region.x as f32,
            region.y as f32,
            region.width as f32,
            region.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region.x, region.y, region.width, region.height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &layers.bind, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..data.len() as u32, 0..1);
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::minimap::layout::{TAG_CORNER, TAG_MARK, TAG_SCALE};

    fn sample(scale: f32, corner: (f32, f32)) -> Content {
        Content {
            picture: Image {
                width: 280,
                height: 152,
                rgba: vec![0; 280 * 152 * 4],
            },
            backdrop: None,
            sprites: Image {
                width: 256,
                height: 32,
                rgba: vec![0; 256 * 32 * 4],
            },
            strip: (64, 16),
            scale,
            corner,
            marks: Vec::new(),
        }
    }

    #[test]
    fn the_position_formula_lands_where_the_wiki_says() {
        // docs/re/minimap.md § 4: the village's road mark, the pass's two ends.
        assert_eq!(project((-7.16, 16.31), 3.2, (-18.0, -12.0)), (121.0, 148.0));
        assert_eq!(project((-18.52, 41.69), 2.0, (-10.0, -13.0)), (43.0, 187.0));
        assert_eq!(project((14.33, -50.19), 2.0, (-10.0, -13.0)), (109.0, 4.0));
    }

    #[test]
    fn the_strips_sit_in_four_slots_at_the_foot_of_the_screen() {
        let c = sample(3.2, (-18.0, -12.0));
        for slot in 0..MEMBERS {
            assert_eq!(strip_dst(&c, slot), [(slot * 64) as f32, 176.0, 64.0, 16.0]);
            assert_eq!(strip_src(&c, slot), [(slot * 64) as u32, 0, 64, 16]);
        }
    }

    #[test]
    fn a_dot_is_centred_on_the_projected_pixel() {
        assert_eq!(dot_dst((121.0, 148.0)), [117.0, 144.0, 8.0, 8.0]);
        let c = sample(3.2, (-18.0, -12.0));
        assert_eq!(dot_src(&c, 2), [16, 16, 8, 8]);
    }

    #[test]
    fn a_fixed_picture_keeps_its_top_left() {
        // The village's picture is 280×152 on a 256×192 screen: 24 pixels fall off the right.
        assert_eq!(
            picture_dst((280, 152), (0.0, 0.0)),
            [0.0, 0.0, 280.0, 152.0]
        );
        assert_eq!(
            picture_dst((168, 200), (0.0, 0.0)),
            [0.0, 0.0, 168.0, 200.0]
        );
    }

    #[test]
    fn the_scroll_holds_inside_the_dead_zone() {
        // 30 pixels horizontally, 10 below the anchor and 20 above it (`0x02024518`).
        assert_eq!(follow((100.0, 100.0), (129.0, 109.0)), (100.0, 100.0));
        assert_eq!(follow((100.0, 100.0), (71.0, 81.0)), (100.0, 100.0));
        assert_eq!(follow((100.0, 100.0), (140.0, 100.0)), (110.0, 100.0));
        assert_eq!(follow((100.0, 100.0), (60.0, 100.0)), (90.0, 100.0));
        assert_eq!(follow((100.0, 100.0), (100.0, 115.0)), (100.0, 105.0));
        assert_eq!(follow((100.0, 100.0), (100.0, 75.0)), (100.0, 95.0));
    }

    #[test]
    fn a_scroll_puts_the_leader_on_the_anchor() {
        assert_eq!(ANCHOR, (128.0, 96.0));
        assert_eq!(picture_offset((121.0, 148.0)), (7.0, -52.0));
        // A dot whose party pixel equals the scroll lands on the anchor, top-left 4 up and left.
        assert_eq!(dot_dst(ANCHOR), [124.0, 92.0, 8.0, 8.0]);
        let c = sample(1.0, (0.0, 0.0));
        let frame = Frame::new(&c, &[(121.0, 148.0)], Some((121.0, 148.0)));
        assert_eq!(frame.picture_offset, (7.0, -52.0));
        assert_eq!(frame.sprites[1].dst, [124.0, 92.0, 8.0, 8.0]);
    }

    #[test]
    fn the_frame_has_a_strip_and_a_dot_per_member() {
        let c = sample(2.0, (-10.0, -13.0));
        let frame = Frame::new(&c, &[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0)], None);
        assert_eq!(frame.sprites.len(), 6);
        assert_eq!(frame.sheet, (256, 32));
        for slot in 0..3 {
            let strip = frame.sprites[2 * slot];
            assert_eq!(strip.src, strip_src(&c, slot));
            assert_eq!(strip.dst, strip_dst(&c, slot));
            let dot = frame.sprites[2 * slot + 1];
            assert_eq!(dot.src, dot_src(&c, slot));
            // (0, 0) projects to (80, 104): the dot's top-left is four pixels up and left.
            assert_eq!(dot.dst, [76.0, 100.0, 8.0, 8.0]);
        }
        // More than four members would leave the sheet: only four strips and dots exist.
        let frame = Frame::new(&c, &[(0.0, 0.0); 6], None);
        assert_eq!(frame.sprites.len(), 8);
    }

    #[test]
    fn vertices_cover_the_picture_then_the_sprites() {
        let c = sample(1.0, (0.0, 0.0));
        let frame = Frame::new(&c, &[(0.0, 0.0)], None);
        let data = vertices(&frame);
        // One picture, one strip and one dot: three quads.
        assert_eq!(data.len(), 18);
        assert_eq!(&data[0][2..5], &[0.0, 0.0, 1.0]);
        assert_eq!(&data[6][2..5], &[0.0, 0.0, 2.0]);
    }

    #[test]
    fn a_backdrop_fills_the_whole_screen() {
        // The paper pattern is fixed on sub BG1: 64×64 over 256×192 is 4 columns and 3 rows,
        // and the one-cell pattern (8×8) tiles the 32×24-tile screen.
        let screen = [0.0, 0.0, SCREEN.0 as f32, SCREEN.1 as f32];
        let tiles = backdrop_tiles((64, 64), screen);
        assert_eq!(tiles.len(), 12);
        assert_eq!(tiles[0], ([0.0, 0.0, 64.0, 64.0], [0.0, 0.0, 1.0, 1.0]));
        assert_eq!(tiles[11].0, [192.0, 128.0, 64.0, 64.0]);
        assert_eq!(backdrop_tiles((8, 8), [0.0, 0.0, 128.0, 64.0]).len(), 128);
        assert_eq!(backdrop_tiles((8, 8), screen).len(), 32 * 24);
        // No backdrop or no screen has no quads.
        assert!(backdrop_tiles((0, 0), screen).is_empty());
        assert!(backdrop_tiles((64, 64), [0.0, 0.0, 0.0, 0.0]).is_empty());
    }

    #[test]
    fn the_backdrop_quads_are_drawn_first() {
        let mut c = sample(1.0, (0.0, 0.0));
        c.backdrop = Some(Image {
            width: 64,
            height: 64,
            rgba: vec![0; 64 * 64 * 4],
        });
        let frame = Frame::new(&c, &[(0.0, 0.0)], None);
        assert_eq!(frame.backdrop, Some((64, 64)));
        let data = vertices(&frame);
        // 12 backdrop quads, then the picture, the strip and the dot.
        assert_eq!(data.len(), (12 + 3) * 6);
        for vertex in &data[..12 * 6] {
            assert_eq!(vertex[4], 0.0);
        }
        assert_eq!(data[12 * 6][4], 1.0);
        assert_eq!(data[13 * 6][4], 2.0);
    }

    #[test]
    fn a_mark_is_projected_but_not_drawn() {
        // The pass's first mark, projected by the layout's own scale and corner (§ 4), and the
        // record's maps kept: no marker is chosen, so no quad is added.
        let data = synthetic_layout();
        let layout = Layout::parse(&data).unwrap();
        let mark = &layout.marks()[0];
        assert_eq!(
            project((mark.x, mark.z), 2.0, (-10.0, -13.0)),
            (43.0, 187.0)
        );
        assert_eq!(mark.maps, [20001]);
        let mut c = sample(2.0, (-10.0, -13.0));
        c.marks = layout.marks();
        let frame = Frame::new(&c, &[(0.0, 0.0)], None);
        assert_eq!(vertices(&frame).len(), 18);
    }

    #[test]
    fn the_backdrop_resolves_to_an_obg_or_is_not_drawn() {
        let archive = build_archive(&[("minimapbg2.obg", &one_cell_picture())]);
        let archive = Gpc2::parse(&archive).unwrap();
        let found = backdrop(&archive, Some("minimapbg2")).unwrap().unwrap();
        assert_eq!(found.size(), (8, 8));
        // The base name resolves through `.obg`; a layout without the tag and an absent member
        // both draw nothing.
        assert!(backdrop(&archive, Some("minimapbg2")).unwrap().is_some());
        assert_eq!(backdrop(&archive, None).unwrap(), None);
        assert_eq!(backdrop(&archive, Some("minimapbg3")).unwrap(), None);
    }

    fn content_of(archive: &[u8], map: &str) -> Result<Content, Error> {
        content(archive, &[], &[], &[], map)
    }

    #[test]
    fn an_unknown_map_is_a_named_error() {
        // Not a layout name and no layout lists it.
        let error = content_of(b"not a GPC2", "M01");
        assert!(matches!(error, Err(Error::Format(_))), "{error:?}");
    }

    /// A 1×1-cell `.obg`, one tile (`docs/re/minimap.md` § 2).
    fn one_cell_picture() -> Vec<u8> {
        let mut out = vec![1, 1, 0, 137, 1, 0, 0, 0];
        for i in 0..16 {
            out.extend((i as u16 * 0x0421).to_le_bytes());
        }
        out.extend([0x11; 32]);
        out.extend(0u16.to_le_bytes());
        out
    }

    /// A GPC2 archive of stored members, hashed with the archive's CRC-32 (`gpc2.rs`).
    fn build_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
        let stored = |body: &[u8]| {
            let mut b = ((body.len() as u32) << 3).to_le_bytes().to_vec();
            b.extend(body);
            b.resize(b.len().next_multiple_of(4), 0);
            b
        };
        let (mut names, mut blobs, mut index) = (Vec::new(), Vec::new(), Vec::new());
        for (name, body) in members {
            let name_offset = names.len() as u32;
            names.extend(name.as_bytes());
            names.push(0);
            let blob = stored(body);
            let location = (blobs.len() / 4) as u32 | (name_offset & 0xFF) << 24;
            let size = blob.len() as u32 | (name_offset >> 8) << 24;
            index.push((crc32(name.as_bytes()), location, size));
            blobs.extend(&blob);
            blobs.resize(blobs.len().next_multiple_of(4), 0);
        }
        index.sort_by_key(|e| e.0);
        let index: Vec<u8> = index
            .iter()
            .flat_map(|(h, l, s)| [h.to_le_bytes(), l.to_le_bytes(), s.to_le_bytes()].concat())
            .collect();
        let (index, names) = (stored(&index), stored(&names));
        let (header, names_offset) = (0x14usize, 0x14 + index.len());
        let data_offset = names_offset + names.len();
        let mut out = b"GPC2".to_vec();
        out.extend((members.len() as u16).to_le_bytes());
        for words in [header / 4, names_offset / 4, data_offset / 4, 0, 0] {
            out.extend((words as u16).to_le_bytes());
        }
        out.extend(((blobs.len() / 4) as u32).to_le_bytes());
        out.extend(index);
        out.extend(names);
        out.extend(blobs);
        out
    }

    /// The archive's name hash: CRC-32 (IEEE), as `gpc2.rs` computes it.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
            }
        }
        !crc
    }

    /// A `.bmmp` with the pass's scale, corner and two marks (85 bytes of table).
    fn synthetic_layout() -> Vec<u8> {
        let mut records = Vec::new();
        let mut push = |tag: u16, types: &[u8], values: &[u32]| {
            let mut out = tag.to_le_bytes().to_vec();
            out.push(types.len() as u8);
            for chunk in types.chunks(4) {
                out.push(
                    chunk
                        .iter()
                        .enumerate()
                        .fold(0, |acc, (i, t)| acc | t << (2 * i)),
                );
            }
            out.resize(out.len().next_multiple_of(4), 0xFF);
            values.iter().for_each(|v| out.extend(v.to_le_bytes()));
            records.extend(out);
        };
        push(TAG_SCALE, &[2], &[2.0f32.to_bits()]);
        push(TAG_CORNER, &[1, 1], &[(-10i32) as u32, (-13i32) as u32]);
        push(
            TAG_MARK,
            &[2, 2, 1],
            &[(-18.52f32).to_bits(), 41.69f32.to_bits(), 20001],
        );
        let body: Vec<u8> = records.clone();
        let mut out = Vec::new();
        for word in [3u32, (0x10 + body.len() as u32), 0, 2] {
            out.extend(word.to_le_bytes());
        }
        out.extend(body);
        out.resize(out.len().next_multiple_of(16), 0);
        out
    }
}
