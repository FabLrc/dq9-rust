//! Top-down PNG view of the collision meshes of an assembled map, for `dq9re map --image`.
//!
//! Every `.col2` piece is projected orthographically onto the `x/z` plane, the way
//! [`dq9_core::collision::CollisionWorld`] places it: the file's integers shifted by the
//! mesh's `shift`, rotated about `y` by the placement's angle, then offset by its position
//! (the transform of `CollisionWorld::spawn`). Floors (normal `y` above `0x800`) are coloured
//! by their interpolated height; walls (at most) are dark, filled when they face up and
//! stroked when they are vertical. The nearest surface along `y` wins, so stacked floors stay
//! readable. Warp and zone volumes are outlined, with the map code of their destination when
//! the map list knows it.
//!
//! This is a development view, not the game's renderer: no textures, models, lighting,
//! animations or camera, and no depth cue other than "highest surface wins".

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use dq9_core::collision::{CollisionWorld, Piece};
use dq9_formats::map::{Collision, Regions, ZONE_AREA, ZONE_DOORWAY};
use glam::Vec3;

use super::reach::Reach;

/// Normal `y` above which a triangle is a floor (the limit of `CollisionWorld::resolve`,
/// `0x800`); walls and ceilings are at most.
pub(crate) const FLOOR_NORMAL_Y: i32 = 0x800;
/// Height of the legend bar and its labels, in pixels.
const LEGEND_HEIGHT: i32 = 96;
const LEGEND_WIDTH: i32 = 16;

/// World-space bounds of the view, in `x/z`; padding is added by [`render`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
    pub min_z: f32,
    pub max_z: f32,
}

impl Bounds {
    fn include(&mut self, x: f32, z: f32) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_z = self.min_z.min(z);
        self.max_z = self.max_z.max(z);
    }

    fn include_point(&mut self, x: f32, y: f32, z: f32) {
        self.include(x, z);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
    }

    fn project(&self, x: f32, z: f32, scale: u32) -> (f64, f64) {
        (
            f64::from(x - self.min_x) * f64::from(scale),
            f64::from(z - self.min_z) * f64::from(scale),
        )
    }
}

/// What to mark on the view. World units, `fx32` positions already divided by 4096.
#[derive(Default)]
pub(crate) struct Markers<'a> {
    /// The map's own start record (`.bmbl` type `0x6e`).
    pub start: Option<[f32; 3]>,
    /// The optional hero point (`--at`), already stood on the ground.
    pub hero: Option<[f32; 3]>,
    /// The optional `--to` target of a reach.
    pub target: Option<[f32; 3]>,
    /// Reachable grid cells to tint (`--reach-image`).
    pub reach: Option<&'a Reach>,
}

/// Size and height range of a written view.
pub(crate) struct Rendered {
    pub width: u32,
    pub height: u32,
    pub min_y: f32,
    pub max_y: f32,
    #[cfg(test)]
    bounds: Bounds,
}

/// A world vertex of `triangle` in `piece`, in world units.
fn world_vertex(piece: &Piece, mesh: &Collision, vertex: [i16; 3]) -> Vec3 {
    let (sin, cos) = (piece.angle as f32 / 4096.0).sin_cos();
    let local = Vec3::new(
        (i32::from(vertex[0]) << mesh.shift) as f32 / 4096.0,
        (i32::from(vertex[1]) << mesh.shift) as f32 / 4096.0,
        (i32::from(vertex[2]) << mesh.shift) as f32 / 4096.0,
    );
    Vec3::new(
        piece.position[0] as f32 / 4096.0 + cos * local.x + sin * local.z,
        piece.position[1] as f32 / 4096.0 + local.y,
        piece.position[2] as f32 / 4096.0 - sin * local.x + cos * local.z,
    )
}

/// Bounds of the collision pieces' triangles, in world units; `None` without geometry.
pub(crate) fn world_bounds(world: &CollisionWorld) -> Option<Bounds> {
    let mut out: Option<Bounds> = None;
    for piece in world.pieces() {
        let mesh = &world.meshes()[piece.mesh];
        for triangle in &mesh.triangles {
            for vertex in triangle.vertices {
                let v = world_vertex(piece, mesh, vertex);
                match &mut out {
                    Some(bounds) => bounds.include_point(v.x, v.y, v.z),
                    None => {
                        out = Some(Bounds {
                            min_x: v.x,
                            max_x: v.x,
                            min_y: v.y,
                            max_y: v.y,
                            min_z: v.z,
                            max_z: v.z,
                        });
                    }
                }
            }
        }
    }
    out
}

/// Colour of a floor at `t` in `[0, 1]` (min to max height): blue, green, yellow, red.
pub(crate) fn height_color(t: f32) -> [u8; 3] {
    const STOPS: [(f32, [u8; 3]); 5] = [
        (0.0, [46, 76, 160]),
        (0.25, [52, 150, 168]),
        (0.5, [92, 190, 120]),
        (0.75, [232, 208, 92]),
        (1.0, [206, 72, 60]),
    ];
    let t = t.clamp(0.0, 1.0);
    for pair in STOPS.windows(2) {
        let (a, ca) = pair[0];
        let (b, cb) = pair[1];
        if t <= b {
            let k = if b > a { (t - a) / (b - a) } else { 0.0 };
            return [0, 1, 2].map(|i| {
                (f32::from(ca[i]) + (f32::from(cb[i]) - f32::from(ca[i])) * k).round() as u8
            });
        }
    }
    STOPS[STOPS.len() - 1].1
}

fn wall_color() -> [u8; 3] {
    [38, 38, 46]
}

fn background() -> [u8; 3] {
    [246, 246, 242]
}

/// One projected triangle: vertices `(x, z, y)` in pixels and world height.
#[derive(Debug, Clone, Copy)]
struct Face {
    p: [(f64, f64, f64); 3],
    floor: bool,
    top: f32,
}

impl Face {
    fn area(&self) -> f64 {
        let [a, b, c] = self.p;
        (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)
    }
}

/// A small canvas of RGB pixels.
struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 3]>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![background(); width * height],
        }
    }

    fn set(&mut self, x: i32, y: i32, color: [u8; 3]) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = color;
        }
    }

    fn blend(&mut self, x: i32, y: i32, color: [u8; 3], alpha: f32) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return;
        }
        let index = y * self.width + x;
        let old = self.pixels[index];
        self.pixels[index] = [0, 1, 2].map(|i| {
            (f32::from(old[i]) + (f32::from(color[i]) - f32::from(old[i])) * alpha).round() as u8
        });
    }

    fn rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [u8; 3]) {
        for dy in 0..height {
            for dx in 0..width {
                self.set(x + dx, y + dy, color);
            }
        }
    }

    fn outline_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [u8; 3]) {
        for dy in 0..height {
            self.set(x, y + dy, color);
            self.set(x + width - 1, y + dy, color);
        }
        for dx in 0..width {
            self.set(x + dx, y, color);
            self.set(x + dx, y + height - 1, color);
        }
    }

    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y) = (x0, y0);
        let mut error = dx - dy;
        loop {
            self.set(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * error;
            if e2 > -dy {
                error -= dy;
                x += sx;
            }
            if e2 < dx {
                error += dx;
                y += sy;
            }
        }
    }

    fn thick_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        self.line(x0, y0, x1, y1, color);
        self.line(x0 + 1, y0, x1 + 1, y1, color);
        self.line(x0, y0 + 1, x1, y1 + 1, color);
    }

    fn save(&self, path: &Path) -> Result<()> {
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut encoder =
            png::Encoder::new(BufWriter::new(file), self.width as u32, self.height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut data = Vec::with_capacity(self.width * self.height * 3);
        for pixel in &self.pixels {
            data.extend_from_slice(pixel);
        }
        encoder
            .write_header()
            .with_context(|| format!("writing {}", path.display()))?
            .write_image_data(&data)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

/// Five-by-seven bitmap rows of `c`, top row first, bit 4 at the left.
fn glyph(c: char) -> Option<[u8; 7]> {
    Some(match c {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '.' => [0, 0, 0, 0, 0, 0b00100, 0b00100],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        ':' => [0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0],
        '/' => [
            0b00001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b10000,
        ],
        '>' => [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
        ' ' => [0; 7],
        _ => return None,
    })
}

fn text_width(text: &str) -> i32 {
    text.chars().count() as i32 * 6
}

fn draw_text(canvas: &mut Canvas, x: i32, y: i32, text: &str, color: [u8; 3]) {
    for (index, c) in text.chars().enumerate() {
        let Some(rows) = glyph(c) else { continue };
        let left = x + index as i32 * 6;
        for (row, bits) in rows.iter().enumerate() {
            for bit in 0..5 {
                if bits & (1 << (4 - bit)) != 0 {
                    canvas.set(left + bit, y + row as i32, color);
                }
            }
        }
    }
}

/// Text on a dark backing rectangle, for destinations and the legend.
fn draw_label(canvas: &mut Canvas, x: i32, y: i32, text: &str, color: [u8; 3]) {
    let width = text_width(text);
    canvas.rect(x - 2, y - 2, width + 4, 11, [20, 20, 24]);
    draw_text(canvas, x, y, text, color);
}

fn marker(canvas: &mut Canvas, (x, y): (i32, i32), color: [u8; 3]) {
    canvas.rect(x - 4, y - 4, 9, 9, [20, 20, 24]);
    canvas.rect(x - 3, y - 3, 7, 7, color);
    canvas.set(x, y, [255, 255, 255]);
}

fn cross(canvas: &mut Canvas, (x, y): (i32, i32), color: [u8; 3]) {
    canvas.thick_line(x - 5, y - 5, x + 5, y + 5, [20, 20, 24]);
    canvas.thick_line(x + 5, y - 5, x - 5, y + 5, [20, 20, 24]);
    canvas.line(x - 5, y - 5, x + 5, y + 5, color);
    canvas.line(x + 5, y - 5, x - 5, y + 5, color);
}

/// Corners of a box of half extents `hx`/`hz` centred at `(cx, cz)` and turned by `angle`
/// radians, the rotation of `Region_InRotatedBox` (`0x020321f0`).
fn box_corners(cx: f32, cz: f32, hx: f32, hz: f32, angle: f32) -> [(f32, f32); 4] {
    let (sin, cos) = angle.sin_cos();
    [(-hx, -hz), (hx, -hz), (hx, hz), (-hx, hz)]
        .map(|(x, z)| (cx + x * cos - z * sin, cz + x * sin + z * cos))
}

fn circle_points(cx: f32, cz: f32, radius: f32) -> Vec<(f32, f32)> {
    (0..24)
        .map(|i| {
            let angle = std::f32::consts::TAU * i as f32 / 24.0;
            (cx + radius * angle.cos(), cz + radius * angle.sin())
        })
        .collect()
}

/// Draws `regions` outlines and their destinations; returns the estimated size in world
/// units added around the collision bounds.
fn draw_regions(
    canvas: &mut Canvas,
    bounds: &Bounds,
    scale: u32,
    regions: &Regions,
    codes: &HashMap<u16, String>,
) {
    let point = |(x, z): (f32, f32)| -> (i32, i32) {
        let (px, py) = bounds.project(x, z, scale);
        (px.round() as i32, py.round() as i32)
    };
    let mut label_index = 0;
    let mut draw_label_at =
        |canvas: &mut Canvas, center: (f32, f32), text: &str, color: [u8; 3]| {
            let (x, y) = point(center);
            let offset = (label_index % 3) * 11;
            label_index += 1;
            draw_label(canvas, x + 4, (y - 12 - offset).max(2), text, color);
        };
    let outline = |canvas: &mut Canvas, points: &[(f32, f32)], color: [u8; 3]| {
        for i in 0..points.len() {
            let a = point(points[i]);
            let b = point(points[(i + 1) % points.len()]);
            canvas.thick_line(a.0, a.1, b.0, b.1, color);
        }
    };
    for warp in &regions.warps {
        let v = &warp.volume;
        let center = [v.center[0] as f32 / 4096.0, v.center[2] as f32 / 4096.0];
        let half = [
            (v.high[0] - v.low[0]) as f32 / 8192.0,
            (v.high[2] - v.low[2]) as f32 / 8192.0,
        ];
        let color = [232, 44, 210];
        let corners = box_corners(
            center[0],
            center[1],
            half[0],
            half[1],
            v.angle as f32 / 4096.0,
        );
        outline(canvas, &corners, color);
        if let Some(code) = codes.get(&warp.destination) {
            draw_label_at(canvas, (center[0], center[1]), &format!(">{code}"), color);
        }
    }
    for zone in &regions.zones {
        let v = &zone.volume;
        let center = [v.center[0] as f32 / 4096.0, v.center[2] as f32 / 4096.0];
        let color = match zone.kind {
            ZONE_DOORWAY => [255, 150, 20],
            ZONE_AREA => [40, 190, 220],
            _ => [160, 160, 170],
        };
        let half = [v.size[0] as f32 / 8192.0, v.size[2] as f32 / 8192.0];
        if v.size[2] < 0x1000 {
            let radius = half[0].max(0.25);
            let points = circle_points(center[0], center[1], radius);
            outline(canvas, &points, color);
        } else {
            let corners = box_corners(
                center[0],
                center[1],
                half[0],
                half[1],
                v.angle as f32 / 4096.0,
            );
            outline(canvas, &corners, color);
        }
        if let Some(warp) = &zone.warp
            && let Some(code) = codes.get(&warp.destination)
        {
            draw_label_at(canvas, (center[0], center[1]), &format!(">{code}"), color);
        }
    }
}

fn draw_legend(canvas: &mut Canvas, min_y: f32, max_y: f32) {
    let bar_x = 14;
    let bar_y = 12;
    let height = LEGEND_HEIGHT;
    let box_width = LEGEND_WIDTH + 48;
    canvas.rect(
        bar_x - 6,
        bar_y - 6,
        box_width,
        height + 26,
        [250, 250, 250],
    );
    canvas.outline_rect(bar_x - 6, bar_y - 6, box_width, height + 26, [60, 60, 60]);
    for i in 0..height {
        let t = 1.0 - i as f32 / (height - 1) as f32;
        canvas.rect(bar_x, bar_y + i, LEGEND_WIDTH, 1, height_color(t));
    }
    let label = |y: f32| format!("{y:.2}");
    draw_text(
        canvas,
        bar_x + LEGEND_WIDTH + 4,
        bar_y - 1,
        &label(max_y),
        [30, 30, 30],
    );
    draw_text(
        canvas,
        bar_x + LEGEND_WIDTH + 4,
        bar_y + height - 6,
        &label(min_y),
        [30, 30, 30],
    );
    draw_text(canvas, bar_x, bar_y + height + 6, "HEIGHT", [30, 30, 30]);
}

/// Renders `world` to `path` and returns its size and height range in world units.
pub(crate) fn render(
    world: &CollisionWorld,
    regions: &Regions,
    codes: &HashMap<u16, String>,
    markers: Markers<'_>,
    scale: u32,
    path: &Path,
) -> Result<Rendered> {
    if scale == 0 {
        bail!("--scale must be at least 1 pixel per world unit");
    }
    let Some(mut bounds) = world_bounds(world) else {
        bail!("the map has no collision geometry to draw");
    };
    for point in markers
        .start
        .into_iter()
        .chain(markers.hero)
        .chain(markers.target)
    {
        bounds.include(point[0], point[2]);
    }
    for warp in &regions.warps {
        let v = &warp.volume;
        let center = [v.center[0] as f32 / 4096.0, v.center[2] as f32 / 4096.0];
        let half = [
            (v.high[0] - v.low[0]) as f32 / 8192.0,
            (v.high[2] - v.low[2]) as f32 / 8192.0,
        ];
        for (x, z) in box_corners(
            center[0],
            center[1],
            half[0],
            half[1],
            v.angle as f32 / 4096.0,
        ) {
            bounds.include(x, z);
        }
    }
    for zone in &regions.zones {
        let v = &zone.volume;
        let center = [v.center[0] as f32 / 4096.0, v.center[2] as f32 / 4096.0];
        let radius = v.size[0] as f32 / 8192.0;
        let half = [radius, v.size[2] as f32 / 8192.0];
        if v.size[2] < 0x1000 {
            for (x, z) in circle_points(center[0], center[1], radius.max(0.25)) {
                bounds.include(x, z);
            }
        } else {
            for (x, z) in box_corners(
                center[0],
                center[1],
                half[0],
                half[1],
                v.angle as f32 / 4096.0,
            ) {
                bounds.include(x, z);
            }
        }
    }
    // One world unit of margin on every side, so markers and outlines near the edge fit.
    bounds.min_x -= 1.0;
    bounds.max_x += 1.0;
    bounds.min_z -= 1.0;
    bounds.max_z += 1.0;
    let width = ((bounds.max_x - bounds.min_x) * scale as f32).ceil() as u32;
    let height = ((bounds.max_z - bounds.min_z) * scale as f32).ceil() as u32;
    if width == 0 || height == 0 {
        bail!("the map's collision geometry has an empty footprint");
    }
    if width > 10_000 || height > 10_000 || u64::from(width) * u64::from(height) > 64_000_000 {
        bail!("{width}x{height} pixels is too large; lower --scale (was {scale}) or its bounds");
    }

    let mut canvas = Canvas::new(width as usize, height as usize);
    let mut faces = Vec::new();
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for piece in world.pieces() {
        let mesh = &world.meshes()[piece.mesh];
        for triangle in &mesh.triangles {
            let vertices = triangle.vertices.map(|v| world_vertex(piece, mesh, v));
            let p = vertices.map(|v| {
                let (x, y) = bounds.project(v.x, v.z, scale);
                (x, y, f64::from(v.y))
            });
            let floor = i32::from(triangle.normal[1]) > FLOOR_NORMAL_Y;
            if floor {
                for v in &vertices {
                    min_y = min_y.min(v.y);
                    max_y = max_y.max(v.y);
                }
            }
            faces.push(Face {
                p,
                floor,
                top: vertices
                    .iter()
                    .map(|v| v.y)
                    .fold(f32::NEG_INFINITY, f32::max),
            });
        }
    }
    if !min_y.is_finite() {
        min_y = 0.0;
        max_y = 0.0;
    }
    let span = max_y - min_y;
    let flat = span < 1e-5;
    let span = span.max(1e-6);
    let mut depth = vec![f32::NEG_INFINITY; canvas.width * canvas.height];
    for face in &faces {
        let area = face.area();
        if area.abs() < 1e-9 {
            continue;
        }
        let xs = face.p.map(|p| p.0);
        let ys = face.p.map(|p| p.1);
        let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min).floor() as i32;
        let x1 = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil() as i32;
        let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min).floor() as i32;
        let y1 = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil() as i32;
        let [a, b, c] = face.p;
        for pixel_y in y0.max(0)..=y1.min(canvas.height as i32 - 1) {
            for pixel_x in x0.max(0)..=x1.min(canvas.width as i32 - 1) {
                let (x, y) = (f64::from(pixel_x) + 0.5, f64::from(pixel_y) + 0.5);
                let w0 = ((b.0 - x) * (c.1 - y) - (c.0 - x) * (b.1 - y)) / area;
                let w1 = ((c.0 - x) * (a.1 - y) - (a.0 - x) * (c.1 - y)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                    continue;
                }
                let height = w0 * a.2 + w1 * b.2 + w2 * c.2;
                let index = pixel_y as usize * canvas.width + pixel_x as usize;
                if height <= f64::from(depth[index]) {
                    continue;
                }
                depth[index] = height as f32;
                let color = if face.floor {
                    let t = if flat {
                        0.5
                    } else {
                        (height as f32 - min_y) / span
                    };
                    height_color(t.clamp(0.0, 1.0))
                } else {
                    wall_color()
                };
                canvas.pixels[index] = color;
            }
        }
    }
    // Vertical wall faces project to a line: stroke every wall edge above the surface
    // already drawn there, so walls stay visible in a top view.
    for face in faces.iter().filter(|face| !face.floor) {
        let edges = [(0, 1), (1, 2), (2, 0)];
        for (i, j) in edges {
            let a = face.p[i];
            let b = face.p[j];
            let (mut x0, mut y0) = (a.0.round() as i32, a.1.round() as i32);
            let (x1, y1) = (b.0.round() as i32, b.1.round() as i32);
            let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
            let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
            let mut error = dx - dy;
            loop {
                if x0 >= 0
                    && y0 >= 0
                    && (x0 as usize) < canvas.width
                    && (y0 as usize) < canvas.height
                {
                    let index = y0 as usize * canvas.width + x0 as usize;
                    if face.top >= depth[index] - 0.01 {
                        canvas.pixels[index] = wall_color();
                    }
                }
                if x0 == x1 && y0 == y1 {
                    break;
                }
                let e2 = 2 * error;
                if e2 > -dy {
                    error -= dy;
                    x0 += sx;
                }
                if e2 < dx {
                    error += dx;
                    y0 += sy;
                }
            }
        }
    }

    if let Some(reach) = markers.reach {
        for cell in &reach.cells {
            let (x, y) = bounds.project(cell.x, cell.z, scale);
            // Keep the top view honest: a lower cell under a higher floor is not visible.
            let (cx, cy) = (x.round() as i32, y.round() as i32);
            if cx >= 0 && cy >= 0 && (cx as usize) < canvas.width && (cy as usize) < canvas.height {
                let index = cy as usize * canvas.width + cx as usize;
                if depth[index] > cell.y + 0.2 {
                    continue;
                }
            }
            let half = 0.25 * f64::from(scale);
            let left = (x - half).round() as i32;
            let top = (y - half).round() as i32;
            let size = (2.0 * half).round() as i32;
            for dy in 0..size {
                for dx in 0..size {
                    canvas.blend(left + dx, top + dy, [0, 220, 255], 0.35);
                }
            }
        }
    }
    draw_regions(&mut canvas, &bounds, scale, regions, codes);
    if let Some(start) = markers.start {
        let (x, y) = bounds.project(start[0], start[2], scale);
        marker(
            &mut canvas,
            (x.round() as i32, y.round() as i32),
            [250, 250, 250],
        );
    }
    if let Some(target) = markers.target {
        let (x, y) = bounds.project(target[0], target[2], scale);
        cross(
            &mut canvas,
            (x.round() as i32, y.round() as i32),
            [230, 40, 40],
        );
    }
    if let Some(hero) = markers.hero {
        let (x, y) = bounds.project(hero[0], hero[2], scale);
        marker(
            &mut canvas,
            (x.round() as i32, y.round() as i32),
            [230, 40, 40],
        );
    }
    draw_legend(&mut canvas, min_y, max_y);
    canvas.save(path)?;
    Ok(Rendered {
        width,
        height,
        min_y,
        max_y,
        #[cfg(test)]
        bounds,
    })
}

#[cfg(test)]
pub(super) mod fixtures {
    use dq9_core::collision::{CollisionWorld, Piece};
    use dq9_formats::map::{Collision, Triangle};

    pub type Tri3 = [[i16; 3]; 3];

    fn attributes(v: &Tri3) -> u32 {
        let coord = |i: usize| v[i / 3][i % 3];
        let pick = |axis: usize, want_max: bool| {
            (0..3)
                .map(|k| k * 3 + axis)
                .reduce(|a, b| {
                    let better = if want_max {
                        coord(b) > coord(a)
                    } else {
                        coord(b) < coord(a)
                    };
                    if better { b } else { a }
                })
                .unwrap() as u32
        };
        pick(0, false)
            | pick(1, false) << 4
            | pick(2, false) << 8
            | pick(0, true) << 12
            | pick(1, true) << 16
            | pick(2, true) << 20
    }

    fn normal(v: &Tri3) -> [i16; 3] {
        let f = |i: usize, a: usize| f64::from(v[i][a]);
        let e1 = [f(1, 0) - f(0, 0), f(1, 1) - f(0, 1), f(1, 2) - f(0, 2)];
        let e2 = [f(2, 0) - f(0, 0), f(2, 1) - f(0, 1), f(2, 2) - f(0, 2)];
        let c = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let length = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        c.map(|x| (x / length * 4096.0).round() as i16)
    }

    /// A mesh whose one cell holds every triangle; shift 4 (one world unit is 256 file
    /// units), grid 1×1, cell size 0x4000 as in `dq9_core::testing`.
    pub fn mesh(triangles: &[Tri3]) -> Collision {
        let mut low = [i16::MAX; 3];
        let mut high = [i16::MIN; 3];
        for t in triangles {
            for v in t {
                for a in 0..3 {
                    low[a] = low[a].min(v[a]);
                    high[a] = high[a].max(v[a]);
                }
            }
        }
        let n = triangles.len();
        Collision {
            shift: 4,
            bounds: [low[0], low[1], low[2], high[0], high[1], high[2]],
            cell_size: 0x4000,
            grid_x: 1,
            grid_z: 1,
            triangles: triangles
                .iter()
                .map(|t| Triangle {
                    vertices: *t,
                    normal: normal(t),
                    attributes: attributes(t),
                })
                .collect(),
            cells: (0..2).map(|i| i * n..(i + 1) * n).collect(),
            cell_indices: (0..2).flat_map(|_| 0..n as u16).collect(),
            ground_records: Vec::new(),
        }
    }

    pub fn world(triangles: &[Tri3]) -> CollisionWorld {
        CollisionWorld::from_pieces(
            vec![mesh(triangles)],
            vec![Piece {
                mesh: 0,
                position: [0; 3],
                angle: 0,
            }],
        )
    }

    /// Two upward triangles covering `[-size, size]²` at height `y` (file units).
    pub fn floor(size: i16, y: i16) -> [Tri3; 2] {
        [
            [[-size, y, -size], [-size, y, size], [size, y, -size]],
            [[size, y, -size], [-size, y, size], [size, y, size]],
        ]
    }

    /// Two triangles forming a vertical wall on `x = at`, `z0..z1`, `y0..y1`, facing `-x`.
    pub fn wall_x(at: i16, z0: i16, z1: i16, y0: i16, y1: i16) -> [Tri3; 2] {
        [
            [[at, y0, z0], [at, y0, z1], [at, y1, z0]],
            [[at, y0, z1], [at, y1, z1], [at, y1, z0]],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{floor, wall_x, world};
    use super::*;
    use dq9_formats::map::Regions;

    fn rendered(world: &CollisionWorld) -> (Rendered, Vec<u8>) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("dq9re-map-test-{}-{id}.png", std::process::id()));
        let out = render(
            world,
            &Regions::default(),
            &HashMap::new(),
            Markers::default(),
            4,
            &path,
        )
        .expect("render");
        let bytes = std::fs::read(&path).expect("png");
        std::fs::remove_file(&path).ok();
        (out, bytes)
    }

    #[test]
    fn height_colours_are_ordered_and_distinct() {
        let low = height_color(0.0);
        let middle = height_color(0.5);
        let high = height_color(1.0);
        assert_ne!(low, middle);
        assert_ne!(middle, high);
        assert!(low[2] > low[0], "low should be blue");
        assert!(high[0] > high[2], "high should be red");
    }

    #[test]
    fn a_flat_floor_is_drawn_at_scale_and_north_up() {
        let world = world(&floor(2560, 0));
        let (image, bytes) = rendered(&world);
        assert!(bytes.starts_with(b"\x89PNG"));
        assert!(image.width > 20 && image.height > 20);
        assert_eq!(image.min_y, 0.0);
        assert_eq!(image.max_y, 0.0);
        // The floor is the only surface: its middle colour must be somewhere in the image.
        let canvas = png_pixels(&bytes);
        assert!(canvas.iter().any(|p| *p == height_color(0.5)));
        // North (-z) is the top row: the bounds place it there.
        let bounds = image.bounds;
        let (_, top) = bounds.project(0.0, bounds.min_z, 4);
        let (_, bottom) = bounds.project(0.0, bounds.max_z, 4);
        assert!(top < bottom);
    }

    #[test]
    fn a_vertical_wall_is_drawn_dark() {
        let world = world(
            &[
                floor(2560, 0).as_slice(),
                wall_x(256, -2560, 2560, 0, 2048).as_slice(),
            ]
            .concat(),
        );
        let (_, bytes) = rendered(&world);
        let pixels = png_pixels(&bytes);
        assert!(pixels.iter().any(|p| *p == wall_color()), "no wall pixel");
        assert!(
            pixels.iter().any(|p| *p == height_color(0.5)),
            "no floor pixel"
        );
    }

    /// Minimal PNG decode for the tests: only what `png` wrote (8-bit RGB, no filter).
    fn png_pixels(bytes: &[u8]) -> Vec<[u8; 3]> {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        // Let the decoder undo filters: the test only checks colours, not raw bytes.
        let mut reader = decoder.read_info().expect("png info");
        let mut buf = vec![0; reader.output_buffer_size().expect("buffer size")];
        let info = reader.next_frame(&mut buf).expect("png frame");
        buf[..info.buffer_size()]
            .chunks(3)
            .map(|p| [p[0], p[1], p[2]])
            .collect()
    }
}
