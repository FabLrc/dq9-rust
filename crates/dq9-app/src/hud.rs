//! The wide 16:9 interface: a modern HUD over the cinematic field and the game's message
//! window moved to the bottom of the screen. Every quad is in a 960 × 540 virtual canvas
//! (`dq9_render::ui::Ui::draw_in`), so the interface keeps its proportions at any window size.
//!
//! This is a demonstration interface, not a reconstruction: the original DS layout is the
//! `stacked`/`side`/`top` presentation and stays the default (`screens.rs`, `docs/app.md`).

use dq9_core::party::{Character, Party};
use dq9_formats::font::Font;
use dq9_render::ui::{Atlas, Quad};

/// Width and height of the interface's virtual canvas (16:9).
pub const CANVAS: (i32, i32) = (960, 540);

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
}

/// Gold accent of the panels, and the panel and text colours.
const ACCENT: [f32; 4] = [0.85, 0.68, 0.30, 0.95];
const PANEL: [f32; 4] = [0.012, 0.018, 0.035, 0.62];
const TEXT: [f32; 4] = [0.95, 0.93, 0.86, 1.0];
const MUTED: [f32; 4] = [0.66, 0.70, 0.78, 1.0];
const BAR_BACK: [f32; 4] = [0.10, 0.12, 0.16, 0.90];
const HP: [f32; 4] = [0.35, 0.86, 0.52, 0.95];
const MP: [f32; 4] = [0.35, 0.62, 0.95, 0.95];

/// Where the 256×192 message window is placed on the 960×540 canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogueLayout {
    pub scale: f32,
    pub offset: (f32, f32),
    /// A glass panel behind the window (field conversations), not for centered captions.
    pub backdrop: bool,
}

impl DialogueLayout {
    /// Bottom left, the DS window scaled twice.
    pub const BOTTOM: Self = Self {
        scale: 2.0,
        offset: (24.0, (CANVAS.1 - 192 * 2 - 16) as f32),
        backdrop: true,
    };

    /// The centered captions of cutscenes: the whole DS canvas scaled twice in the middle.
    pub const CENTER: Self = Self {
        scale: 2.0,
        offset: (
            (CANVAS.0 - 256 * 2) as f32 / 2.0,
            (CANVAS.1 - 192 * 2) as f32 / 2.0,
        ),
        backdrop: false,
    };
}

/// The HUD and the message window, in canvas order (background panels first).
pub fn frame(
    atlas: &Atlas,
    font: &Font<'_>,
    info: &HudInfo<'_>,
    dialogue: &[Quad],
    layout: DialogueLayout,
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

    // Location, top left: the developer label and the map code with the time of day.
    panel([28, 24, 372, 68], PANEL, &mut out);
    out.push(atlas.solid([28, 24, 4, 68], ACCENT));
    let title = info.title.to_uppercase();
    text(title.as_bytes(), 48, 36, TEXT, &mut out);
    let subtitle = format!("{}  ·  {}", info.code, info.time.to_uppercase());
    text(subtitle.as_bytes(), 48, 58, MUTED, &mut out);

    // Time of day, top right, as a small chip.
    let label = info.time.to_uppercase();
    let w = width(label.as_bytes()).max(24);
    let x = CANVAS.0 - 28 - w - 28;
    panel([x, 24, w + 28, 40], PANEL, &mut out);
    out.push(atlas.solid([x, 24, 3, 40], ACCENT));
    text(label.as_bytes(), x + 16, 38, TEXT, &mut out);

    // Party, bottom right: the leader's name and health, then the purse.
    if let Some(party) = info.party
        && let Some(leader) = party.members.first()
        && leader.max_hp > 0
    {
        let w = 308;
        let h = 128;
        let x = CANVAS.0 - 28 - w;
        let y = CANVAS.1 - 28 - h;
        panel([x, y, w, h], PANEL, &mut out);
        out.push(atlas.solid([x, y, 4, h], ACCENT));
        draw_name(atlas, font, leader, x + 18, y + 14, TEXT, &mut out);
        let level = format!("LV {}", leader.level());
        text(
            level.as_bytes(),
            x + w - 18 - width(level.as_bytes()),
            y + 14,
            MUTED,
            &mut out,
        );
        stat(
            atlas,
            font,
            "HP",
            leader.hp,
            leader.max_hp,
            HP,
            [x + 18, y + 40],
            w - 36,
            &mut out,
        );
        stat(
            atlas,
            font,
            "MP",
            leader.mp,
            leader.max_mp,
            MP,
            [x + 18, y + 78],
            w - 36,
            &mut out,
        );
        if party.gold > 0 {
            let gold = format!("{} G", party.gold);
            text(gold.as_bytes(), x + 18, y + h - 24, ACCENT, &mut out);
        }
    }

    // Controls, bottom left (the web page and the window help repeat them).
    text(
        b"WASD  ENTER  Q/E  R  M",
        28,
        CANVAS.1 - 34,
        [0.85, 0.87, 0.92, 0.55],
        &mut out,
    );

    // The message window: the DS layout scaled and anchored (bottom left for the field,
    // centered for a cutscene's captions), with a glass panel behind the field's window. A
    // full-screen fade stays a full-screen fade.
    let has_window = dialogue.iter().any(|q| q.dst != [0.0, 0.0, 256.0, 192.0]);
    if has_window && layout.backdrop {
        panel([16, 328, 528, 196], [0.008, 0.012, 0.025, 0.55], &mut out);
    }
    for quad in dialogue {
        if quad.dst == [0.0, 0.0, 256.0, 192.0] {
            out.push(atlas.solid([0, 0, CANVAS.0, CANVAS.1], quad.color));
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

/// Draws a character's name glyph indices, advancing by each glyph's width.
fn draw_name(
    atlas: &Atlas,
    font: &Font<'_>,
    leader: &Character,
    x: i32,
    y: i32,
    color: [f32; 4],
    out: &mut Vec<Quad>,
) {
    let name = leader.name();
    if name.is_empty() {
        // A new game's name is typed on the creation screen; the documented default identity.
        let (glyphs, _) = font.layout(b"HERO");
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

    #[test]
    fn hud_stays_on_its_canvas_and_shows_the_party() {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        let atlas = atlas();
        let party = Party::new_game();
        let mut leader = party.leader().unwrap().clone();
        leader.max_hp = 30;
        leader.hp = 12;
        leader.max_mp = 8;
        leader.mp = 3;
        let party = Party {
            members: vec![leader],
            ..party
        };
        let info = HudInfo {
            title: "Angel Falls",
            code: "M01",
            time: "Evening",
            party: Some(&party),
        };
        let quads = frame(&atlas, &font, &info, &[], DialogueLayout::BOTTOM);
        assert!(!quads.is_empty());
        for quad in quads {
            assert!(quad.dst[0] >= 0.0 && quad.dst[1] >= 0.0);
            assert!(quad.dst[0] + quad.dst[2] <= CANVAS.0 as f32 + 0.01);
            assert!(quad.dst[1] + quad.dst[3] <= CANVAS.1 as f32 + 0.01);
        }
    }

    #[test]
    fn a_full_screen_fade_stays_full_screen() {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        let atlas = atlas();
        let info = HudInfo {
            title: "M01",
            code: "M01",
            time: "Night",
            party: None,
        };
        let fade = Quad {
            dst: [0.0, 0.0, 256.0, 192.0],
            src: [0, 0, 1, 1],
            color: [0.0, 0.0, 0.0, 0.5],
        };
        let quads = frame(&atlas, &font, &info, &[fade], DialogueLayout::CENTER);
        let full = quads
            .iter()
            .find(|q| q.dst == [0.0, 0.0, 960.0, 540.0])
            .expect("the fade covers the canvas");
        assert_eq!(full.color, [0.0, 0.0, 0.0, 0.5]);
    }
}
