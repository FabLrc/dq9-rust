//! 2D overlay for the game's interface (message window, prompts): quads in DS screen pixels
//! (256×192) cut from one texture atlas, drawn over the finished 3D frame.
//!
//! [`Atlas`] is the CPU side: the font strip (ink as alpha), the eight pieces of the window
//! border (`data/ani/win0.spr` to `win7.spr`), the wait arrow and a white texel, with helpers
//! that turn text and frames into [`Quad`]s. [`Ui`] is the GPU side. Nothing here reads the
//! ROM: the caller decodes the images and passes them in. What the interface shows and where:
//! `docs/re/dialogue.md`.

use dq9_formats::font::Font;
use wgpu::util::DeviceExt as _;

use crate::renderer::Viewport;

/// Size of the DS screen, the unit of every coordinate of [`Quad`].
pub const SCREEN: (u32, u32) = (256, 192);

/// An RGBA8 image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A rectangle `[x, y, width, height]`.
pub type Rect = [u32; 4];

/// One textured rectangle: `dst` on the 256×192 screen, `src` in the atlas (both in pixels,
/// as `[x, y, width, height]`), tinted by `color` (RGBA, multiplied with the texel).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub dst: [f32; 4],
    pub src: [u32; 4],
    pub color: [f32; 4],
}

/// The texture the interface draws from.
#[derive(Debug, Clone)]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    /// RGBA8, rows top to bottom.
    pub rgba: Vec<u8>,
    /// Per glyph of the font: column in the strip and width.
    glyphs: Vec<(u16, u8)>,
    font_height: u32,
    white: Rect,
    border: [Rect; 8],
    arrow: Rect,
}

/// Size of the border pieces (`win*.spr` are 8×8).
pub const PIECE: u32 = 8;

impl Atlas {
    /// Packs the font strip (ink white on transparent, row 0), the eight border pieces in
    /// the order of `win0..win7` (top-left corner, top, top-right corner, right, bottom-right
    /// corner, bottom, bottom-left corner, left) and the wait arrow. Pieces must be 8×8.
    pub fn build(font: &Font, border: &[Image; 8], arrow: &Image) -> Self {
        let strip = (u32::from(font.width), u32::from(font.height));
        let width = strip.0.max(256);
        // One shelf under the strip: white texel, border pieces, arrow.
        let shelf = strip.1 + 1;
        let height = shelf + arrow.height.max(PIECE).max(2);
        let mut atlas = Self {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
            glyphs: font.glyphs.iter().map(|g| (g.x, g.width)).collect(),
            font_height: strip.1,
            white: [0; 4],
            border: [[0; 4]; 8],
            arrow: [0; 4],
        };
        for y in 0..strip.1 {
            for x in 0..strip.0 {
                if font.pixel(x as u16, y as u16) {
                    let o = ((y * width + x) * 4) as usize;
                    atlas.rgba[o..o + 4].copy_from_slice(&[255; 4]);
                }
            }
        }
        let mut x = 0;
        let mut place = |atlas: &mut Self, image: &Image| -> Rect {
            let at = [x, shelf, image.width, image.height];
            atlas.blit(image, x, shelf);
            x += image.width;
            at
        };
        atlas.white = place(
            &mut atlas,
            &Image {
                width: 2,
                height: 2,
                rgba: vec![255; 16],
            },
        );
        for (i, piece) in border.iter().enumerate() {
            atlas.border[i] = place(&mut atlas, piece);
        }
        atlas.arrow = place(&mut atlas, arrow);
        atlas
    }

    fn blit(&mut self, image: &Image, at_x: u32, at_y: u32) {
        for y in 0..image.height {
            let src = (y * image.width * 4) as usize;
            let dst = (((at_y + y) * self.width + at_x) * 4) as usize;
            let n = (image.width * 4) as usize;
            self.rgba[dst..dst + n].copy_from_slice(&image.rgba[src..src + n]);
        }
    }

    /// Pixel height of the font strip.
    pub fn font_height(&self) -> u32 {
        self.font_height
    }

    /// Pixel width of a laid-out line: the right edge of its last glyph.
    pub fn line_width(&self, glyphs: &[(u8, i32)]) -> i32 {
        glyphs
            .iter()
            .map(|&(g, x)| {
                x + self
                    .glyphs
                    .get(usize::from(g))
                    .map_or(0, |&(_, w)| i32::from(w))
            })
            .max()
            .unwrap_or(0)
    }

    /// Size of the wait arrow.
    pub fn arrow_size(&self) -> (u32, u32) {
        (self.arrow[2], self.arrow[3])
    }

    /// A filled rectangle `[x, y, w, h]`.
    pub fn solid(&self, dst: [i32; 4], color: [f32; 4]) -> Quad {
        Quad {
            dst: dst.map(|v| v as f32),
            src: [self.white[0], self.white[1], 1, 1],
            color,
        }
    }

    /// The glyphs of a line placed by `Font::layout`, from `(x, y)`.
    pub fn text(&self, glyphs: &[(u8, i32)], x: i32, y: i32, color: [f32; 4], out: &mut Vec<Quad>) {
        for &(glyph, offset) in glyphs {
            let Some(&(column, width)) = self.glyphs.get(usize::from(glyph)) else {
                continue;
            };
            out.push(Quad {
                dst: [
                    (x + offset) as f32,
                    y as f32,
                    f32::from(width),
                    self.font_height as f32,
                ],
                src: [u32::from(column), 0, u32::from(width), self.font_height],
                color,
            });
        }
    }

    /// The wait arrow with its top-left corner at `(x, y)`.
    pub fn arrow(&self, x: i32, y: i32, out: &mut Vec<Quad>) {
        out.push(Quad {
            dst: [
                x as f32,
                y as f32,
                self.arrow[2] as f32,
                self.arrow[3] as f32,
            ],
            src: self.arrow,
            color: [1.0; 4],
        });
    }

    /// A window: `fill` under the border pieces along the rectangle `[x, y, w, h]` (at least
    /// 16 by 16). The pieces are 8×8 with a transparent margin of one pixel, so they step by 6
    /// pixels: edges first, from just after a corner up to the opposite corner, then the four
    /// corners over them.
    pub fn window(&self, [x, y, w, h]: [i32; 4], fill: [f32; 4], out: &mut Vec<Quad>) {
        const STEP: i32 = PIECE as i32 - 2;
        let piece = PIECE as i32;
        out.push(self.solid([x + 2, y + 2, w - 4, h - 4], fill));
        let mut put = |i: usize, px: i32, py: i32| {
            let [sx, sy, ..] = self.border[i];
            out.push(Quad {
                dst: [px as f32, py as f32, piece as f32, piece as f32],
                src: [sx, sy, PIECE, PIECE],
                color: [1.0; 4],
            });
        };
        let mut at = x + STEP;
        while at < x + w - piece {
            put(1, at, y);
            put(5, at, y + h - piece);
            at += STEP;
        }
        let mut at = y + STEP;
        while at < y + h - piece {
            put(7, x, at);
            put(3, x + w - piece, at);
            at += STEP;
        }
        put(0, x, y);
        put(2, x + w - piece, y);
        put(4, x + w - piece, y + h - piece);
        put(6, x, y + h - piece);
    }
}

const VERTEX_FLOATS: usize = 8;

/// Six vertices (two triangles) per quad: position, atlas coordinates, colour.
fn vertices(quads: &[Quad], atlas: (u32, u32)) -> Vec<[f32; VERTEX_FLOATS]> {
    let (aw, ah) = (atlas.0 as f32, atlas.1 as f32);
    let mut out = Vec::with_capacity(quads.len() * 6);
    for q in quads {
        let [x, y, w, h] = q.dst;
        let [sx, sy, sw, sh] = q.src.map(|v| v as f32);
        let corner = |dx: f32, dy: f32| {
            let [r, g, b, a] = q.color;
            [
                x + dx * w,
                y + dy * h,
                (sx + dx * sw) / aw,
                (sy + dy * sh) / ah,
                r,
                g,
                b,
                a,
            ]
        };
        let (tl, tr, bl, br) = (
            corner(0.0, 0.0),
            corner(1.0, 0.0),
            corner(0.0, 1.0),
            corner(1.0, 1.0),
        );
        out.extend([tl, tr, bl, tr, br, bl]);
    }
    out
}

/// GPU resources of the overlay.
#[derive(Debug)]
pub struct Ui {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    size: (u32, u32),
}

impl Ui {
    /// Uploads `atlas` and builds the pipeline for targets of `color_format` (non-sRGB, like
    /// the map renderer: the interface colours are display values too).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color_format: wgpu::TextureFormat,
        atlas: &Atlas,
    ) -> Self {
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("ui atlas"),
                size: wgpu::Extent3d {
                    width: atlas.width,
                    height: atlas.height,
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
            &atlas.rgba,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dq9 ui shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui"),
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
        Self {
            pipeline,
            group,
            size: (atlas.width, atlas.height),
        }
    }

    /// Draws `quads` over `target` inside `region` (the 256×192 screen is stretched to it),
    /// keeping what the target holds.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        region: Viewport,
        quads: &[Quad],
    ) {
        if quads.is_empty() {
            return;
        }
        let data = vertices(quads, self.size);
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        // One buffer per draw: several draws of one frame (the two DS screens) are recorded
        // into the same encoder, and a buffer rewritten by `queue.write_buffer` would hold only
        // the last one's quads when the encoder runs.
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ui vertices"),
            contents: bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut pass = crate::gpu::load_pass(encoder, "ui", target);
        pass.set_viewport(
            region.x as f32,
            region.y as f32,
            region.width as f32,
            region.height as f32,
            0.0,
            1.0,
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..data.len() as u32, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::font::FontId;

    /// One glyph "a" (4 wide) in a 8×2 strip: column 0 set on row 0.
    fn font_files() -> (Vec<u8>, Vec<u8>) {
        let mut info = b"1.1\0".to_vec();
        for v in [1u32, 0, 0x18, 0x18, 0x20] {
            info.extend(v.to_le_bytes());
        }
        info.extend(0x20u32.to_le_bytes());
        info.extend([4, 1, 0, 0]);
        info.extend(b"a");
        let mut data = b"1.0\0".to_vec();
        for v in [8u16, 2] {
            data.extend(v.to_le_bytes());
        }
        data.extend(2u32.to_le_bytes());
        data.extend(16u32.to_le_bytes());
        data.extend([0b1000_0000, 0]);
        (info, data)
    }

    fn image(width: u32, height: u32, value: u8) -> Image {
        Image {
            width,
            height,
            rgba: vec![value; (width * height * 4) as usize],
        }
    }

    fn atlas() -> Atlas {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        let pieces: [Image; 8] = std::array::from_fn(|i| image(8, 8, i as u8 + 1));
        Atlas::build(&font, &pieces, &image(16, 16, 9))
    }

    fn texel(atlas: &Atlas, x: u32, y: u32) -> u8 {
        atlas.rgba[((y * atlas.width + x) * 4) as usize]
    }

    #[test]
    fn atlas_holds_ink_pieces_and_white() {
        let atlas = atlas();
        assert_eq!((texel(&atlas, 0, 0), texel(&atlas, 1, 0)), (255, 0));
        let white = atlas.solid([0, 0, 1, 1], [1.0; 4]).src;
        assert_eq!(texel(&atlas, white[0], white[1]), 255);
        // Each border piece sits where its quad says it does.
        let mut quads = Vec::new();
        atlas.window([0, 0, 32, 24], [0.0; 4], &mut quads);
        let corner = quads[quads.len() - 4];
        assert_eq!(corner.dst, [0.0, 0.0, 8.0, 8.0]);
        assert_eq!(texel(&atlas, corner.src[0], corner.src[1]), 1);
        let arrow = {
            let mut out = Vec::new();
            atlas.arrow(5, 6, &mut out);
            out[0]
        };
        assert_eq!(texel(&atlas, arrow.src[0], arrow.src[1]), 9);
        assert_eq!(atlas.arrow_size(), (16, 16));
    }

    #[test]
    fn windows_step_their_edge_pieces_by_six_pixels_under_the_corners() {
        let atlas = atlas();
        let mut quads = Vec::new();
        atlas.window([10, 20, 28, 20], [0.0, 0.0, 1.0, 1.0], &mut quads);
        assert_eq!(quads[0].dst, [12.0, 22.0, 24.0, 16.0]);
        // Top and bottom edges at 16, 22 and 28 (before the right corner at 30), the sides
        // at 26 only (before the bottom corner at 32), then the corners.
        let origins = |select: fn(&Quad) -> bool| -> Vec<f32> {
            quads
                .iter()
                .filter(|q| select(q))
                .map(|q| q.dst[0])
                .collect()
        };
        assert_eq!(
            origins(|q| q.dst[1] == 20.0 && q.dst[0] < 30.0 && q.dst[0] > 10.0),
            [16.0, 22.0, 28.0]
        );
        assert_eq!(quads.len(), 1 + 3 + 3 + 1 + 1 + 4);
        // The corners come last, so they cover the overlap of the last edge piece.
        assert_eq!(quads[quads.len() - 4].dst, [10.0, 20.0, 8.0, 8.0]);
    }

    #[test]
    fn text_quads_follow_the_layout() {
        let atlas = atlas();
        let mut out = Vec::new();
        atlas.text(&[(0, 0), (0, 5), (9, 7)], 10, 20, [1.0; 4], &mut out);
        assert_eq!(out.len(), 2, "unknown glyph indices are skipped");
        assert_eq!(out[1].dst, [15.0, 20.0, 4.0, 2.0]);
        assert_eq!(out[1].src, [0, 0, 4, 2]);
    }

    #[test]
    fn vertices_cover_the_quad_in_atlas_units() {
        let quad = Quad {
            dst: [10.0, 20.0, 8.0, 4.0],
            src: [16, 2, 8, 4],
            color: [1.0, 0.5, 0.25, 1.0],
        };
        let v = vertices(&[quad], (32, 8));
        assert_eq!(v.len(), 6);
        assert_eq!(v[0][..4], [10.0, 20.0, 0.5, 0.25]);
        assert_eq!(v[4][..4], [18.0, 24.0, 0.75, 0.75]);
        assert_eq!(v[0][4..], [1.0, 0.5, 0.25, 1.0]);
    }
}
