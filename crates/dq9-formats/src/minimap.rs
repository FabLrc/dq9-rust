//! The field's map screen data: `data/pack_lv5/minimap.gp2` with its `.obg` pictures and
//! `.bmmp` layouts, and the `obj_minimap` cells the party strip and the dots are drawn with.
//!
//! Formats measured on the cartridge in `docs/re/minimap.md`; sources: the DQIX formats wiki
//! (`Mini-Map`, `2D-Graphics`, `Pac`, `Tagged-Data-Table`) and `docs/re/bottom-screen.md` for
//! the screen's elements. Readers only: no drawing and no window.

pub mod layout;
pub mod obg;
pub mod pac;

pub use layout::{Layout, Mark};
pub use obg::{Image, Picture};
pub use pac::{Member, Pac};

/// The cell of `obj_minimap.NCER` holding the party panel: one 64×64 part.
pub const PARTY_PANEL_CELL: usize = 2;
/// The cells holding the five coloured dots (blue, green, pink, yellow, red).
pub const DOT_CELLS: core::ops::RangeInclusive<usize> = 3..=7;
/// Rows of the party panel that make the 64×16 name strip drawn under the map.
pub const NAME_STRIP_HEIGHT: u16 = 16;

/// One axis of `pixel = position × scale − corner × 8`: the picture's origin is at tile
/// `corner`, that is `corner × 8` pixels, and the scale is pixels per map unit.
pub fn axis(position: f32, scale: f32, corner: f32) -> f32 {
    position * scale - corner * 8.0
}

/// The name strip drawn from the party panel: the panel's box clipped to its top
/// [`NAME_STRIP_HEIGHT`] rows, in cell pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strip {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// Builds the party strip from the cells of `obj_minimap.NCER`.
///
/// `None` when the file has no cell [`PARTY_PANEL_CELL`], that cell has no parts, or the panel
/// starts below the strip.
pub fn party_strip(cells: &[nds_rom::nitro2d::Cell]) -> Option<Strip> {
    let bounds = cells.get(PARTY_PANEL_CELL)?.bounds()?;
    let strip_height = i32::from(NAME_STRIP_HEIGHT);
    let top = i32::from(bounds.y).max(0);
    let bottom = (i32::from(bounds.y) + i32::from(bounds.height)).min(strip_height);
    if bottom <= top {
        return None;
    }
    Some(Strip {
        x: bounds.x,
        y: top as i16,
        width: bounds.width,
        height: (bottom - top) as u16,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nds_rom::nitro2d::{Cell, Part, Shape};

    fn part(x: i16, y: i16, size: u8) -> Part {
        Part {
            x,
            y,
            shape: Shape::Square,
            size,
            flip_x: false,
            flip_y: false,
            affine: None,
            tile: 0,
            palette: 0,
            priority: 0,
        }
    }

    fn cells() -> Vec<Cell> {
        let mut cells = vec![
            Cell {
                attribute: 0,
                parts: vec![part(0, 0, 0)],
            };
            PARTY_PANEL_CELL
        ];
        cells.push(Cell {
            attribute: 23,
            parts: vec![part(0, 0, 3)],
        });
        cells
    }

    #[test]
    fn the_party_strip_is_the_panels_top_16_rows() {
        let strip = party_strip(&cells()).unwrap();
        assert_eq!(
            strip,
            Strip {
                x: 0,
                y: 0,
                width: 64,
                height: 16
            }
        );
        assert_eq!(party_strip(&cells()[..PARTY_PANEL_CELL]), None);
    }

    #[test]
    fn a_panel_below_the_strip_has_no_strip() {
        let mut cells = cells();
        cells[PARTY_PANEL_CELL].parts[0].y = 40;
        assert_eq!(party_strip(&cells), None);
        cells[PARTY_PANEL_CELL].parts[0].y = 8;
        assert_eq!(party_strip(&cells).unwrap().height, 8);
    }

    #[test]
    fn the_position_formula() {
        assert!((axis(16.31, 3.2, -12.0) - 148.192).abs() < 1e-3);
        assert_eq!(axis(8.0, 4.0, 0.0), 32.0);
        assert_eq!(axis(0.0, 1.0, 0.0), 0.0);
    }
}
