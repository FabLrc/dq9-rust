//! The DS screens in the window. Each screen is 256×192 pixels; a layout places one or two of
//! them, scaled to fit, except `wide`: one 16:9 field over the whole window. Where the field
//! and its message window (`talk.rs`) go is a [`ScreenMode`]: the port's own assignment (top,
//! the bottom one a placeholder) or the original's (bottom, `docs/re/bottom-screen.md`).

use dq9_render::Viewport;

/// Which screens are shown and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// Top screen above the bottom one.
    #[default]
    Stacked,
    /// Top screen left of the bottom one.
    Side,
    /// Top screen only.
    Top,
    /// One 16:9 field over the whole window (`Renderer` in cinematic mode, HUD in `hud.rs`):
    /// neither DS screen shape nor the bottom panel.
    Wide,
}

impl Layout {
    /// Parses a layout name (`stacked`, `side`, `top` or `wide`).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "stacked" => Ok(Self::Stacked),
            "side" => Ok(Self::Side),
            "top" => Ok(Self::Top),
            "wide" => Ok(Self::Wide),
            _ => Err(format!("expected stacked, side, top or wide, not `{s}`")),
        }
    }

    /// The name `parse` accepts for this layout.
    #[cfg(target_arch = "wasm32")]
    pub fn name(self) -> &'static str {
        match self {
            Self::Stacked => "stacked",
            Self::Side => "side",
            Self::Top => "top",
            Self::Wide => "wide",
        }
    }

    /// The next layout, in the order the `R` key cycles through them.
    pub fn next(self) -> Self {
        match self {
            Self::Stacked => Self::Side,
            Self::Side => Self::Top,
            Self::Top => Self::Wide,
            Self::Wide => Self::Stacked,
        }
    }

    /// Whether the layout is the single 16:9 field: no DS screen shape, no panel and the
    /// cinematic renderer.
    pub fn is_wide(self) -> bool {
        self == Self::Wide
    }

    /// Screens across and down the layout.
    fn grid(self) -> (u32, u32) {
        match self {
            Self::Stacked => (1, 2),
            Self::Side => (2, 1),
            Self::Top | Self::Wide => (1, 1),
        }
    }
}

/// Which screen holds the field's 3D view and its message window (`--screens`, the page's
/// "Screens" select, `?screens=`). The other screen shows the placeholder panel; where the
/// original draws its map screen, nothing is read or drawn yet (`re/bottom-screen.md` § 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenMode {
    /// The port's own assignment: the field on the top screen. A cutscene's natives 109/110
    /// move it to the bottom for the length of the script's swap.
    #[default]
    Port,
    /// The original's field assignment: the 3D view and the message window on the bottom
    /// (touch) screen, as the field runs with `POWCNT1` bit 15 set (`ov_017::021b17ec`). A
    /// battle keeps the port's assignment, which is the original's for a battle.
    Original,
}

impl ScreenMode {
    /// Parses a mode name (`port` or `original`).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "port" => Ok(Self::Port),
            "original" => Ok(Self::Original),
            _ => Err(format!("expected port or original, not `{s}`")),
        }
    }

    /// The name `parse` accepts for this mode.
    #[cfg(target_arch = "wasm32")]
    pub fn name(self) -> &'static str {
        match self {
            Self::Port => "port",
            Self::Original => "original",
        }
    }

    /// Whether the field and its message window are drawn on the bottom screen. `swapped` is
    /// the script's display swap (natives 109/110, `POWCNT1` bit 15).
    ///
    /// The port starts with the bit clear, so only a cutscene moves the field down. The
    /// original's field runs with the bit set for free control and for field cutscenes alike,
    /// so `original` ignores the script's swap: `re/bottom-screen.md` § 1 gives the bit for the
    /// whole field and says nothing of a 110 inside a field cutscene **[hypothesis]**.
    pub fn field_on_bottom(self, swapped: bool) -> bool {
        match self {
            Self::Port => swapped,
            Self::Original => true,
        }
    }
}

/// Rectangles of the screens in a window, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screens {
    pub top: Viewport,
    /// `None` for [`Layout::Top`] and [`Layout::Wide`].
    pub bottom: Option<Viewport>,
    /// [`Layout::Wide`]: the field fills the window at the window's own aspect.
    pub wide: bool,
}

/// Places the screens of `layout` in a `width` × `height` window, centred and at 4:3; `wide`
/// is one 16:9 rectangle centred in the window.
pub fn screens(layout: Layout, width: u32, height: u32) -> Screens {
    if layout.is_wide() {
        let unit = (width / 16).min(height / 9).max(1);
        let (w, h) = (unit * 16, unit * 9);
        return Screens {
            top: Viewport {
                x: width.saturating_sub(w) / 2,
                y: height.saturating_sub(h) / 2,
                width: w,
                height: h,
            },
            bottom: None,
            wide: true,
        };
    }
    let (columns, rows) = layout.grid();
    // One unit is four by three physical pixels. This keeps both screens exactly 4:3
    // while allowing fractional DS-pixel scales in windows between integer multiples.
    let unit = (width / columns / 4).min(height / rows / 3);
    let cell_w = unit * 4;
    let cell_h = unit * 3;
    let x0 = width.saturating_sub(cell_w * columns) / 2;
    let y0 = height.saturating_sub(cell_h * rows) / 2;
    let at = |column: u32, row: u32| Viewport {
        x: x0 + column * cell_w,
        y: y0 + row * cell_h,
        width: cell_w,
        height: cell_h,
    };
    let bottom = match layout {
        Layout::Stacked => Some(at(0, 1)),
        Layout::Side => Some(at(1, 0)),
        Layout::Top | Layout::Wide => None,
    };
    Screens {
        top: at(0, 0),
        bottom,
        wide: false,
    }
}

impl Screens {
    /// The viewport of the 3D field and of the message window for `mode`.
    pub fn field_for(self, mode: ScreenMode, swapped: bool) -> Viewport {
        if mode.field_on_bottom(swapped) {
            self.bottom.unwrap_or(self.top)
        } else {
            self.top
        }
    }

    /// The viewport of the other DS engine for `mode`: the panel, the original's map screen or
    /// the field menus. A one-screen layout has no second engine: no overlay.
    pub fn overlay_for(self, mode: ScreenMode, swapped: bool) -> Option<Viewport> {
        let bottom = self.bottom?;
        Some(if mode.field_on_bottom(swapped) {
            self.top
        } else {
            bottom
        })
    }

    /// The viewport of the 3D field and of the message window in the port's assignment. The
    /// battle and the cutscene previews use it: a fight keeps the field's screen of the port
    /// (the original's for a battle, `re/bottom-screen.md` § 3).
    pub fn field(self, swapped: bool) -> Viewport {
        self.field_for(ScreenMode::Port, swapped)
    }

    /// The other DS engine in the port's assignment: the bottom screen normally, the top one
    /// after a display swap. A one-screen layout has no second engine: no overlay.
    pub fn overlay(self, swapped: bool) -> Option<Viewport> {
        self.overlay_for(ScreenMode::Port, swapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(v: Viewport, width: u32, height: u32) -> bool {
        v.x + v.width <= width && v.y + v.height <= height
    }

    #[test]
    fn integer_scale_keeps_each_screen_at_ds_aspect() {
        let s = screens(Layout::Stacked, 512, 768);
        assert_eq!(
            s.top,
            Viewport {
                x: 0,
                y: 0,
                width: 512,
                height: 384
            }
        );
        assert_eq!(
            s.bottom,
            Some(Viewport {
                x: 0,
                y: 384,
                width: 512,
                height: 384
            })
        );
    }

    #[test]
    fn side_by_side_is_centred_in_a_wider_window() {
        let s = screens(Layout::Side, 1024, 768);
        assert_eq!(
            s.top,
            Viewport {
                x: 0,
                y: 192,
                width: 512,
                height: 384
            }
        );
        assert_eq!(
            s.bottom,
            Some(Viewport {
                x: 512,
                y: 192,
                width: 512,
                height: 384
            })
        );
    }

    #[test]
    fn top_only_has_no_bottom_screen() {
        let s = screens(Layout::Top, 1024, 768);
        assert_eq!(
            s.top,
            Viewport {
                x: 0,
                y: 0,
                width: 1024,
                height: 768
            }
        );
        assert_eq!(s.bottom, None);
        assert!(!s.wide);
    }

    #[test]
    fn wide_is_a_centred_16_9_rectangle() {
        let s = screens(Layout::Wide, 1920, 1080);
        assert_eq!(
            s.top,
            Viewport {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(s.bottom, None);
        assert!(s.wide);
        // A taller window gets black bars instead of a stretched field.
        let portrait = screens(Layout::Wide, 800, 1200);
        assert_eq!(
            portrait.top,
            Viewport {
                x: 0,
                y: 375,
                width: 800,
                height: 450
            }
        );
        let square = screens(Layout::Wide, 500, 500);
        assert_eq!(
            (square.top.width, square.top.height),
            (496, 279),
            "16:9 inside 500 × 500"
        );
    }

    #[test]
    fn intermediate_window_uses_available_space() {
        let s = screens(Layout::Stacked, 1000, 658);
        assert_eq!((s.top.width, s.top.height), (436, 327));
        assert_eq!(s.bottom.unwrap().y, s.top.y + s.top.height);
    }

    #[test]
    fn small_window_shrinks_screens_inside_it() {
        for layout in [Layout::Stacked, Layout::Side, Layout::Top, Layout::Wide] {
            let s = screens(layout, 200, 300);
            assert!(inside(s.top, 200, 300), "{layout:?} top");
            if let Some(b) = s.bottom {
                assert!(inside(b, 200, 300), "{layout:?} bottom");
            }
        }
    }

    #[test]
    fn names_parse_and_cycle() {
        assert_eq!(Layout::parse("side"), Ok(Layout::Side));
        assert_eq!(Layout::parse("wide"), Ok(Layout::Wide));
        assert!(Layout::parse("cinema").is_err());
        assert_eq!(Layout::Stacked.next().next().next().next(), Layout::Stacked);
        assert_eq!(Layout::Top.next(), Layout::Wide);
    }

    #[test]
    fn a_display_swap_moves_the_field_to_the_bottom_screen() {
        let s = screens(Layout::Stacked, 512, 768);
        assert_eq!(s.field(false), s.top);
        assert_eq!(s.field(true), s.bottom.unwrap());
        assert_eq!(s.overlay(false), s.bottom);
        assert_eq!(s.overlay(true), Some(s.top));
        // A one-screen layout has nothing to swap to.
        let one = screens(Layout::Top, 1024, 768);
        assert_eq!(one.field(true), one.top);
        assert_eq!(one.overlay(true), None, "no panel over the only screen");
        assert_eq!(one.overlay(false), None);
        let wide = screens(Layout::Wide, 1600, 900);
        assert_eq!(wide.field(true), wide.top);
        assert_eq!(wide.overlay(true), None);
        assert_eq!(wide.overlay(false), None);
    }

    #[test]
    fn the_original_assignment_puts_the_field_and_window_on_the_bottom_screen() {
        for layout in [Layout::Stacked, Layout::Side] {
            let s = screens(layout, 1024, 768);
            assert_eq!(
                s.field_for(ScreenMode::Original, false),
                s.bottom.unwrap(),
                "{layout:?}: the 3D field and its window"
            );
            // The original's field runs with `POWCNT1` bit 15 set: a script's swap changes
            // nothing there.
            assert_eq!(s.field_for(ScreenMode::Original, true), s.bottom.unwrap());
            assert_eq!(
                s.overlay_for(ScreenMode::Original, false),
                Some(s.top),
                "{layout:?}: the freed screen"
            );
        }
        // A one-screen layout keeps the message window visible: the field takes the only
        // screen and there is no placeholder to draw.
        let top = screens(Layout::Top, 1024, 768);
        assert_eq!(top.field_for(ScreenMode::Original, false), top.top);
        assert_eq!(top.overlay_for(ScreenMode::Original, false), None);
        // `wide` is one 16:9 field: neither DS screen shape nor a second engine.
        let wide = screens(Layout::Wide, 1600, 900);
        assert_eq!(wide.field_for(ScreenMode::Original, false), wide.top);
        assert_eq!(wide.overlay_for(ScreenMode::Original, false), None);
    }

    #[test]
    fn the_port_assignment_is_the_default_and_names_parse() {
        assert_eq!(ScreenMode::default(), ScreenMode::Port);
        assert_eq!(ScreenMode::parse("port"), Ok(ScreenMode::Port));
        assert_eq!(ScreenMode::parse("original"), Ok(ScreenMode::Original));
        assert!(ScreenMode::parse("swapped").is_err());
        assert!(!ScreenMode::Port.field_on_bottom(false));
        assert!(ScreenMode::Port.field_on_bottom(true));
        let s = screens(Layout::Stacked, 512, 768);
        assert_eq!(s.field_for(ScreenMode::Port, false), s.field(false));
        assert_eq!(s.field_for(ScreenMode::Port, true), s.field(true));
        assert_eq!(s.overlay_for(ScreenMode::Port, false), s.overlay(false));
        assert_eq!(s.overlay_for(ScreenMode::Port, true), s.overlay(true));
    }
}
