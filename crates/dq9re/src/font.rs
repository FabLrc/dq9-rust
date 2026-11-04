//! `dq9re font`: render a game font (glyph sheet or a line of text) to PNG.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use dq9_formats::{Font, FontId};
use nds_rom::Rom;

/// Loads `data/pack_lv5/fi_<id>.bin` + `fd_<id>.bin`.
pub fn load<'a>(rom: &Rom<'a>, id: FontId) -> Result<Font<'a>> {
    let info = rom.file_by_path(&format!("data/pack_lv5/fi_{}.bin", id.name()))?;
    let data = rom.file_by_path(&format!("data/pack_lv5/fd_{}.bin", id.name()))?;
    Font::parse(info, data, id).context("parsing font")
}

/// 8-bit grayscale canvas: ink is black on white.
pub(crate) struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    pub(crate) fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0xFF; width * height],
        }
    }

    /// Light grey separator line across the canvas.
    pub(crate) fn hline(&mut self, y: usize) {
        if y < self.height {
            self.pixels[y * self.width..(y + 1) * self.width].fill(0xC0);
        }
    }

    pub(crate) fn glyph(&mut self, font: &Font, index: u8, x: i32, y: i32) {
        let g = &font.glyphs[usize::from(index)];
        for gy in 0..font.height {
            for gx in 0..u16::from(g.width) {
                let (px, py) = (x + i32::from(gx), y + i32::from(gy));
                if font.pixel(g.x + gx, gy) && px >= 0 && py >= 0 {
                    let (px, py) = (px as usize, py as usize);
                    if px < self.width && py < self.height {
                        self.pixels[py * self.width + px] = 0;
                    }
                }
            }
        }
    }

    pub(crate) fn save(&self, path: &Path, scale: usize) -> Result<()> {
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut encoder = png::Encoder::new(
            BufWriter::new(file),
            (self.width * scale) as u32,
            (self.height * scale) as u32,
        );
        encoder.set_color(png::ColorType::Grayscale);
        let mut out = Vec::with_capacity(self.pixels.len() * scale * scale);
        for row in self.pixels.chunks(self.width) {
            let wide: Vec<u8> = row
                .iter()
                .flat_map(|&p| std::iter::repeat_n(p, scale))
                .collect();
            for _ in 0..scale {
                out.extend_from_slice(&wide);
            }
        }
        encoder.write_header()?.write_image_data(&out)?;
        Ok(())
    }
}

pub fn run(rom: &Rom, id: FontId, text: Option<&str>, out: &Path, scale: usize) -> Result<()> {
    let font = load(rom, id)?;
    let height = usize::from(font.height);
    let canvas = match text {
        Some(text) => {
            let (placed, advance) = font.layout(text.as_bytes());
            let mut canvas = Canvas::new(advance.max(1) as usize + 2, height + 2);
            for (index, x) in placed {
                canvas.glyph(&font, index, x + 1, 1);
            }
            println!(
                "{} px wide (Font_MeasureString)",
                font.measure(text.as_bytes())
            );
            canvas
        }
        None => {
            // Sheet: 16 glyphs per row, each in a cell as wide as the widest glyph.
            let cell = usize::from(font.glyphs.iter().map(|g| g.width).max().unwrap_or(1)) + 2;
            let rows = font.glyphs.len().div_ceil(16);
            let mut canvas = Canvas::new(16 * cell, rows * (height + 2));
            for (i, g) in font.glyphs.iter().enumerate() {
                let (cx, cy) = ((i % 16) * cell, (i / 16) * (height + 2));
                canvas.glyph(&font, i as u8, cx as i32 + 1, cy as i32 + 1);
                println!("{i:3} {:>2}px {}", g.width, String::from_utf8_lossy(g.text));
            }
            canvas
        }
    };
    canvas.save(out, scale)?;
    println!("wrote {}", out.display());
    Ok(())
}
