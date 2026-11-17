//! The enhanced interface: a modern HUD over the enhanced field and the game's message window
//! moved to a safe corner. Every quad is laid out in a virtual [`Canvas`] and mapped onto the
//! field's viewport (`dq9_render::ui::Ui::draw_in`): 960 × 540 for the 16:9 layout (`enhanced`)
//! and the window's own aspect for the reactive one, whose shorter side stays 540 canvas units
//! so the interface scales with it.
//!
//! This is a demonstration interface, not a reconstruction: the original DS layout is the
//! `stacked`/`side`/`top` presentation (`screens.rs`, `docs/app.md`).

use dq9_core::party::{Character, Party};
use dq9_formats::font::Font;
use dq9_render::ui::{Atlas, Quad};

/// Width and height of the 16:9 interface's virtual canvas.
pub const CANVAS: (i32, i32) = (960, 540);

/// The shorter side of every canvas, in canvas units: the interface scales with the window's
/// shorter side, so its proportions do not change between the 16:9 and the reactive layouts.
const SHORT: i32 = CANVAS.1;

/// Margin from the canvas edges (`MARGIN / SHORT` of the shorter side) and inner padding.
const MARGIN: i32 = 28;
const PAD: i32 = 16;

/// The virtual canvas a HUD frame is laid out in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Canvas {
    pub width: i32,
    pub height: i32,
}

impl Canvas {
    /// The 16:9 canvas of the `enhanced` layout.
    pub const ENHANCED: Self = Self {
        width: CANVAS.0,
        height: CANVAS.1,
    };

    /// The reactive layout's canvas: the window's aspect with its shorter side at [`SHORT`],
    /// so it maps onto the whole window at a uniform scale (`shorter side / 540`) and the
    /// corners of the canvas are the corners of the window.
    pub fn for_window(width: u32, height: u32) -> Self {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let (cw, ch) = if w >= h {
            (SHORT as f32 * w / h, SHORT as f32)
        } else {
            (SHORT as f32, SHORT as f32 * h / w)
        };
        Self {
            width: cw.round().max(1.0) as i32,
            height: ch.round().max(1.0) as i32,
        }
    }

    /// The canvas aspect, which is the window's aspect.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn aspect(self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }
}

/// The time of day as the HUD spells it.
pub fn time_label(phase: dq9_core::state::TimeOfDay) -> &'static str {
    match phase {
        dq9_core::state::TimeOfDay::Night => "Night",
        dq9_core::state::TimeOfDay::Morning => "Morning",
        dq9_core::state::TimeOfDay::Day => "Day",
        dq9_core::state::TimeOfDay::Evening => "Evening",
    }
}

/// What the HUD shows beyond the message window.
pub struct HudInfo<'a> {
    /// The map's developer label (`LoadedMap::label`).
    pub title: &'a str,
    /// The map code (`M01`, `C01M09`).
    pub code: &'a str,
    /// Time of day, already spelled for the interface (`Evening`).
    pub time: &'a str,
    /// The party, when the run has one with values (a new game fills them at the first fight).
    pub party: Option<&'a Party>,
    /// Translucent glass panels behind the interface and the field's message window
    /// (`glass` of [`dq9_render::EnhancedOptions`]).
    pub glass: bool,
}

/// The members the HUD shows: the party, at most four, up to the first slot without a maximum
/// HP (a new game's companions are filled when they join, at the first fight).
pub fn party_members(party: &Party) -> &[Character] {
    let end = party
        .members
        .iter()
        .take(4)
        .position(|member| member.max_hp == 0)
        .unwrap_or_else(|| party.members.len().min(4));
    &party.members[..end]
}

/// Gold accent of the panels, and the panel and text colours.
const ACCENT: [f32; 4] = [0.85, 0.68, 0.30, 0.95];
const PANEL: [f32; 4] = [0.012, 0.018, 0.035, 0.62];
const TEXT: [f32; 4] = [0.95, 0.93, 0.86, 1.0];
const MUTED: [f32; 4] = [0.66, 0.70, 0.78, 1.0];
const BAR_BACK: [f32; 4] = [0.10, 0.12, 0.16, 0.90];
const HP: [f32; 4] = [0.35, 0.86, 0.52, 0.95];
const MP: [f32; 4] = [0.35, 0.62, 0.95, 0.95];
/// Behind the field's message window (a little darker than the HUD's panels).
const BACKDROP: [f32; 4] = [0.008, 0.012, 0.025, 0.55];

/// Height of one party member's block: name, HP line and MP line.
const ROW: i32 = 78;
/// Width of the party panel (narrower on a narrow canvas).
const PARTY_WIDTH: i32 = 320;

/// Where the 256×192 message window is placed on the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogueLayout {
    pub scale: f32,
    pub offset: (f32, f32),
    /// A glass panel behind the window (field conversations), not for centered captions.
    pub backdrop: bool,
}

impl DialogueLayout {
    /// The DS window drawn twice, bottom left with the HUD's margin.
    pub fn bottom(canvas: Canvas) -> Self {
        Self {
            scale: 2.0,
            offset: (MARGIN as f32, (canvas.height - 192 * 2 - MARGIN) as f32),
            backdrop: true,
        }
    }

    /// The centered captions of cutscenes: the whole DS canvas scaled twice in the middle.
    pub fn center(canvas: Canvas) -> Self {
        Self {
            scale: 2.0,
            offset: (
                (canvas.width - 256 * 2) as f32 / 2.0,
                (canvas.height - 192 * 2) as f32 / 2.0,
            ),
            backdrop: false,
        }
    }

    /// The rectangle of the glass panel behind a bottom window, in canvas pixels: the window's
    /// bounds (`x0, y0, x1, y1` in DS pixels) scaled and moved, padded and kept inside the
    /// canvas.
    fn backdrop_rect(self, canvas: Canvas, [x0, y0, x1, y1]: [f32; 4]) -> [i32; 4] {
        let pad = PAD / 2;
        let x = (x0 * self.scale + self.offset.0 - pad as f32).max(0.0) as i32;
        let y = (y0 * self.scale + self.offset.1 - pad as f32).max(0.0) as i32;
        let w = (((x1 - x0) * self.scale) as i32 + PAD).min(canvas.width - x);
        let h = (((y1 - y0) * self.scale) as i32 + PAD).min(canvas.height - y);
        [x, y, w.max(0), h.max(0)]
    }
}

/// The HUD and the message window, in canvas order (background panels first).
pub fn frame(
    atlas: &Atlas,
    font: &Font<'_>,
    info: &HudInfo<'_>,
    dialogue: &[Quad],
    layout: DialogueLayout,
    canvas: Canvas,
) -> Vec<Quad> {
    let mut out = Vec::new();
    let text = |bytes: &[u8], x: i32, y: i32, color: [f32; 4], out: &mut Vec<Quad>| {
        let (glyphs, _) = font.layout(bytes);
        atlas.text(&glyphs, x, y, color, out);
    };
    let panel = |[x, y, w, h]: [i32; 4], color: [f32; 4], out: &mut Vec<Quad>| {
        out.push(atlas.solid([x, y, w, h], color));
    };
    let width = |bytes: &[u8]| font.layout(bytes).1 - 1;

    // Location, top left: the developer label and the map code with the time of day. The time
    // chip keeps the top right corner; the panel stops before it, so a narrow canvas does not
    // overlap the two.
    let chip = info.time.to_uppercase();
    let chip_w = width(chip.as_bytes()).max(24) + 2 * PAD;
    let chip_x = canvas.width - MARGIN - chip_w;
    let panel_w = PARTY_WIDTH.min((chip_x - 12 - MARGIN).max(160));
    if info.glass {
        panel([MARGIN, MARGIN, panel_w, 68], PANEL, &mut out);
        out.push(atlas.solid([MARGIN, MARGIN, 4, 68], ACCENT));
    }
    let title = info.title.to_uppercase();
    text(title.as_bytes(), MARGIN + PAD, MARGIN + 12, TEXT, &mut out);
    let subtitle = format!("{}  ·  {}", info.code, info.time.to_uppercase());
    text(
        subtitle.as_bytes(),
        MARGIN + PAD,
        MARGIN + 34,
        MUTED,
        &mut out,
    );

    // Time of day, top right, as a small chip.
    if info.glass {
        panel([chip_x, MARGIN, chip_w, 40], PANEL, &mut out);
        out.push(atlas.solid([chip_x, MARGIN, 3, 40], ACCENT));
    }
    text(chip.as_bytes(), chip_x + PAD, MARGIN + 14, TEXT, &mut out);

    // Party, bottom right: every member (up to four) with name, level, HP and MP bars and
    // numbers, then the purse.
    if let Some(party) = info.party {
        let members = party_members(party);
        if !members.is_empty() {
            let count = members.len() as i32;
            let w = PARTY_WIDTH.min(canvas.width - 2 * MARGIN).max(180);
            let h = 2 * PAD + 24 + count * ROW;
            let x = canvas.width - MARGIN - w;
            let y = canvas.height - MARGIN - h;
            if info.glass {
                panel([x, y, w, h], PANEL, &mut out);
                out.push(atlas.solid([x, y, 4, h], ACCENT));
            }
            text(b"PARTY", x + PAD, y + PAD - 2, MUTED, &mut out);
            let gold = format!("{} G", party.gold);
            text(
                gold.as_bytes(),
                x + w - PAD - width(gold.as_bytes()),
                y + PAD - 2,
                ACCENT,
                &mut out,
            );
            for (i, member) in members.iter().enumerate() {
                let row = y + PAD + 24 + i as i32 * ROW;
                draw_name(
                    atlas,
                    font,
                    member,
                    if i == 0 { b"HERO" } else { b"ALLY" },
                    x + PAD,
                    row,
                    TEXT,
                    &mut out,
                );
                let level = format!("LV {}", member.level());
                text(
                    level.as_bytes(),
                    x + w - PAD - width(level.as_bytes()),
                    row,
                    MUTED,
                    &mut out,
                );
                stat(
                    atlas,
                    font,
                    "HP",
                    member.hp,
                    member.max_hp,
                    HP,
                    [x + PAD, row + 16],
                    w - 2 * PAD,
                    &mut out,
                );
                stat(
                    atlas,
                    font,
                    "MP",
                    member.mp,
                    member.max_mp,
                    MP,
                    [x + PAD, row + 44],
                    w - 2 * PAD,
                    &mut out,
                );
            }
        }
    }

    // Controls, bottom left (the web page and the window help repeat them).
    text(
        b"WASD  ENTER  Q/E  R  M",
        MARGIN,
        canvas.height - MARGIN - 12,
        [0.85, 0.87, 0.92, 0.55],
        &mut out,
    );

    // The message window: the DS layout scaled and anchored (bottom left for the field,
    // centered for a cutscene's captions), with a glass panel behind the field's window. A
    // full-screen fade stays a full-screen fade.
    let has_window = dialogue.iter().any(|q| q.dst != [0.0, 0.0, 256.0, 192.0]);
    if has_window && layout.backdrop && info.glass {
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for quad in dialogue {
            if quad.dst == [0.0, 0.0, 256.0, 192.0] {
                continue;
            }
            bounds[0] = bounds[0].min(quad.dst[0]);
            bounds[1] = bounds[1].min(quad.dst[1]);
            bounds[2] = bounds[2].max(quad.dst[0] + quad.dst[2]);
            bounds[3] = bounds[3].max(quad.dst[1] + quad.dst[3]);
        }
        panel(layout.backdrop_rect(canvas, bounds), BACKDROP, &mut out);
    }
    for quad in dialogue {
        if quad.dst == [0.0, 0.0, 256.0, 192.0] {
            out.push(atlas.solid([0, 0, canvas.width, canvas.height], quad.color));
            continue;
        }
        let mut moved = *quad;
        moved.dst = [
            quad.dst[0] * layout.scale + layout.offset.0,
            quad.dst[1] * layout.scale + layout.offset.1,
            quad.dst[2] * layout.scale,
            quad.dst[3] * layout.scale,
        ];
        out.push(moved);
    }
    out
}

/// Draws a character's name glyph indices, advancing by each glyph's width; `fallback` is the
/// label of a member whose name is not known (a new game types the hero's on the creation
/// screen).
#[allow(clippy::too_many_arguments)]
fn draw_name(
    atlas: &Atlas,
    font: &Font<'_>,
    member: &Character,
    fallback: &[u8],
    x: i32,
    y: i32,
    color: [f32; 4],
    out: &mut Vec<Quad>,
) {
    let name = member.name();
    if name.is_empty() {
        let (glyphs, _) = font.layout(fallback);
        atlas.text(&glyphs, x, y, color, out);
        return;
    }
    let mut at = x;
    for &glyph in name {
        let pair = [(glyph, 0)];
        let advance = atlas.line_width(&pair);
        atlas.text(&pair, at, y, color, out);
        at += advance;
    }
}

/// One stat: label and values above a bar that fills with `value / max`.
#[allow(clippy::too_many_arguments)]
fn stat(
    atlas: &Atlas,
    font: &Font<'_>,
    label: &str,
    value: u16,
    max: u16,
    color: [f32; 4],
    [x, y]: [i32; 2],
    width: i32,
    out: &mut Vec<Quad>,
) {
    let line = format!("{label} {value:>3}/{max:<3}");
    let (glyphs, _) = font.layout(line.as_bytes());
    atlas.text(&glyphs, x, y, color, out);
    let bar = [x, y + 18, width, 8];
    out.push(atlas.solid(bar, BAR_BACK));
    let ratio = if max == 0 {
        0.0
    } else {
        f32::from(value.min(max)) / f32::from(max)
    };
    out.push(atlas.solid(
        [
            bar[0],
            bar[1],
            (bar[2] as f32 * ratio).round() as i32,
            bar[3],
        ],
        color,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::font::FontId;
    use dq9_render::ui::Image;

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

    fn atlas() -> dq9_render::ui::Atlas {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        let piece = |value: u8| Image {
            width: 8,
            height: 8,
            rgba: vec![value; 256],
        };
        let pieces: [Image; 8] = std::array::from_fn(|i| piece(i as u8 + 1));
        Atlas::build(&font, &pieces, &piece(9))
    }

    fn font() -> (Vec<u8>, Vec<u8>) {
        font_files()
    }

    /// A party of `count` members with values, the leader first.
    fn party(count: usize) -> Party {
        let base = Party::new_game();
        let mut leader = base.leader().unwrap().clone();
        leader.max_hp = 30;
        leader.hp = 12;
        leader.max_mp = 8;
        leader.mp = 3;
        let members = (0..count)
            .map(|i| {
                let mut member = leader.clone();
                member.hp = (12 - i as u16).max(1);
                member
            })
            .collect();
        Party {
            members,
            gold: 120,
            ..base
        }
    }

    #[test]
    fn the_canvas_follows_the_window_aspect_and_scales_by_its_shorter_side() {
        assert_eq!(Canvas::ENHANCED, Canvas::for_window(1920, 1080));
        assert_eq!(Canvas::ENHANCED.aspect(), 16.0 / 9.0);
        for (width, height) in [(800u32, 1200u32), (1000, 1000), (2560, 1080), (200, 300)] {
            let canvas = Canvas::for_window(width, height);
            assert_eq!(
                canvas.width.min(canvas.height),
                SHORT,
                "{width} × {height}: shorter side"
            );
            assert!(
                (canvas.aspect() - width as f32 / height as f32).abs() < 0.01,
                "{width} × {height}: aspect {}",
                canvas.aspect()
            );
        }
        // Degenerate input stays a canvas, never a zero-sized one.
        let tiny = Canvas::for_window(0, 0);
        assert!(tiny.width >= 1 && tiny.height >= 1);
    }

    #[test]
    fn hud_stays_on_its_canvas_and_shows_the_whole_party() {
        let (info_bytes, data) = font();
        let font = Font::parse(&info_bytes, &data, FontId::Message).unwrap();
        let atlas = atlas();
        let party = party(4);
        let info = HudInfo {
            title: "Angel Falls",
            code: "M01",
            time: "Evening",
            party: Some(&party),
            glass: true,
        };
        for canvas in [
            Canvas::ENHANCED,
            Canvas::for_window(800, 1200),
            Canvas::for_window(2560, 1080),
            Canvas::for_window(200, 300),
        ] {
            let quads = frame(
                &atlas,
                &font,
                &info,
                &[],
                DialogueLayout::bottom(canvas),
                canvas,
            );
            assert!(!quads.is_empty());
            for quad in quads {
                assert!(quad.dst[0] >= 0.0 && quad.dst[1] >= 0.0);
                assert!(quad.dst[0] + quad.dst[2] <= canvas.width as f32 + 0.01);
                assert!(quad.dst[1] + quad.dst[3] <= canvas.height as f32 + 0.01);
            }
        }
    }

    #[test]
    fn the_party_is_shown_up_to_four_members_and_stops_at_an_empty_slot() {
        let full = party(5);
        assert_eq!(full.members.len(), 5);
        assert_eq!(party_members(&full).len(), 4);
        let mut gapped = party(4);
        gapped.members[1].max_hp = 0;
        assert_eq!(party_members(&gapped).len(), 1);
        let empty = Party {
            members: Vec::new(),
            ..Party::new_game()
        };
        assert!(party_members(&empty).is_empty());
    }

    #[test]
    fn the_glass_panel_follows_the_message_window_bounds() {
        let layout = DialogueLayout::bottom(Canvas::ENHANCED);
        let rect = layout.backdrop_rect(Canvas::ENHANCED, [0.0, 120.0, 256.0, 192.0]);
        // The window (0, 120)–(256, 192) scaled twice and moved, padded by half a pad.
        assert_eq!(rect, [20, 360, 528, 160]);
        // A narrower canvas clamps the panel instead of drawing outside it.
        let portrait = Canvas::for_window(800, 1200);
        let rect = layout.backdrop_rect(portrait, [0.0, 120.0, 256.0, 192.0]);
        assert!(rect[0] + rect[2] <= portrait.width);
        assert!(rect[1] + rect[3] <= portrait.height);
    }

    #[test]
    fn glass_off_removes_the_panels() {
        let (info_bytes, data) = font();
        let font = Font::parse(&info_bytes, &data, FontId::Message).unwrap();
        let atlas = atlas();
        let party = party(2);
        let window = Quad {
            dst: [0.0, 120.0, 256.0, 72.0],
            src: [0, 0, 1, 1],
            color: [1.0; 4],
        };
        let draw = |glass: bool| {
            let info = HudInfo {
                title: "M01",
                code: "M01",
                time: "Night",
                party: Some(&party),
                glass,
            };
            frame(
                &atlas,
                &font,
                &info,
                &[window],
                DialogueLayout::bottom(Canvas::ENHANCED),
                Canvas::ENHANCED,
            )
        };
        assert!(draw(true).iter().any(|q| q.color == PANEL));
        assert!(draw(true).iter().any(|q| q.color == BACKDROP));
        assert!(!draw(false).iter().any(|q| q.color == PANEL));
        assert!(!draw(false).iter().any(|q| q.color == BACKDROP));
    }

    #[test]
    fn the_message_window_is_anchored_to_the_canvas_corner() {
        let (info_bytes, data) = font();
        let font = Font::parse(&info_bytes, &data, FontId::Message).unwrap();
        let atlas = atlas();
        let window = Quad {
            dst: [0.0, 120.0, 256.0, 72.0],
            src: [0, 0, 1, 1],
            color: [1.0; 4],
        };
        let canvas = Canvas::for_window(800, 1200);
        let info = HudInfo {
            title: "M01",
            code: "M01",
            time: "Night",
            party: None,
            glass: false,
        };
        let quads = frame(
            &atlas,
            &font,
            &info,
            &[window],
            DialogueLayout::bottom(canvas),
            canvas,
        );
        let moved = quads
            .iter()
            .find(|q| q.dst[2] == 512.0)
            .expect("the message window, scaled twice");
        // The DS window's own origin (0, 120) is scaled twice and moved to the bottom-left
        // anchor of the canvas.
        assert_eq!(moved.dst[0], MARGIN as f32);
        assert_eq!(moved.dst[1], (canvas.height - 384 - MARGIN + 240) as f32);
        assert_eq!(moved.dst[0] + moved.dst[2], (MARGIN + 512) as f32);
        assert!(moved.dst[1] + moved.dst[3] <= canvas.height as f32);
    }

    #[test]
    fn a_full_screen_fade_stays_full_screen() {
        let (info_bytes, data) = font();
        let font = Font::parse(&info_bytes, &data, FontId::Message).unwrap();
        let atlas = atlas();
        let info = HudInfo {
            title: "M01",
            code: "M01",
            time: "Night",
            party: None,
            glass: true,
        };
        let fade = Quad {
            dst: [0.0, 0.0, 256.0, 192.0],
            src: [0, 0, 1, 1],
            color: [0.0, 0.0, 0.0, 0.5],
        };
        for canvas in [Canvas::ENHANCED, Canvas::for_window(800, 1200)] {
            let quads = frame(
                &atlas,
                &font,
                &info,
                &[fade],
                DialogueLayout::center(canvas),
                canvas,
            );
            let full = quads
                .iter()
                .find(|q| q.color == [0.0, 0.0, 0.0, 0.5])
                .expect("the fade covers the canvas");
            assert_eq!(
                full.dst,
                [0.0, 0.0, canvas.width as f32, canvas.height as f32]
            );
        }
    }
}
