//! The battle's windows as quads on the two 256×192 DS screens: the message window and the
//! damage numbers over the scene (top), the party's status and the command menus (bottom).
//!
//! What the original draws where is not read from the overlay: the layout below is built on
//! the field's message window geometry (`talk.rs`) and is a **[hypothesis]**, listed in
//! `docs/app.md` § "Battle".

use dq9_formats::Font;
use dq9_render::ui::{Atlas, Quad};
use glam::Vec2;

use super::Fighter;
use super::fight::{MenuView, View};
use super::narrate::PopupKind;
/// The message window on the top screen **[hypothesis]**: the field's window (`func_02042b98(
/// win, 2, 0x74, 0xfc, 0x4a)`) sits where the fighters stand once the battle camera frames them,
/// so the battle's is put above them, three lines high, with the field's margins and pitch.
pub const MESSAGE: [i32; 4] = [2, 2, 0xfc, 70];
/// Where its text starts, and the line pitch (the field window's).
const MESSAGE_MARGIN: [i32; 2] = [12, 8];
const LINE: i32 = crate::talk::LINE_HEIGHT;
const FILL: [f32; 4] = crate::talk::FILL;
const SELECTED: [f32; 4] = crate::talk::SELECTED;
const GREY: [f32; 4] = [0.55, 0.55, 0.6, 1.0];
const WHITE: [f32; 4] = [1.0; 4];

/// The party's status window on the bottom screen **[hypothesis]**.
const STATUS_X: i32 = 6;
const STATUS_Y: i32 = 6;
const STATUS_WIDTH: i32 = 244;
const STATUS_ROW: i32 = 22;
/// The command menu below it, and the target list beside it **[hypothesis]**.
const MENU_X: i32 = 6;
const MENU_WIDTH: i32 = 116;
const TARGET_X: i32 = 128;
const TARGET_WIDTH: i32 = 122;
const MENU_ROW: i32 = 20;

/// The quads of both screens.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub top: Vec<Quad>,
    pub bottom: Vec<Quad>,
}

/// Draws the fight's windows. `screen_of` places a fighter's body on the top screen.
pub fn frame(
    atlas: &Atlas,
    font: &Font<'_>,
    view: &View<'_>,
    screen_of: &dyn Fn(Fighter) -> Option<Vec2>,
    tick: u32,
) -> Frame {
    let mut out = Frame::default();
    let text = |bytes: &[u8], x: i32, y: i32, color: [f32; 4], quads: &mut Vec<Quad>| {
        let (glyphs, _) = font.layout(bytes);
        atlas.text(&glyphs, x, y, color, quads);
    };
    let width = |bytes: &[u8]| font.layout(bytes).1 - 1;

    // Top: the message window, the damage numbers, the target marker.
    if let Some(lines) = view.message
        && lines.iter().any(|l| !l.is_empty())
    {
        let wrapped: Vec<Vec<u8>> = lines
            .iter()
            .flat_map(|line| wrap(font, line, MESSAGE[2] - 2 * MESSAGE_MARGIN[0]))
            .collect();
        atlas.window(MESSAGE, FILL, &mut out.top);
        for (i, line) in wrapped.iter().enumerate() {
            text(
                line,
                MESSAGE[0] + MESSAGE_MARGIN[0],
                MESSAGE[1] + MESSAGE_MARGIN[1] + i as i32 * LINE,
                WHITE,
                &mut out.top,
            );
        }
    }
    for popup in view.popups {
        let Some(at) = screen_of(popup.fighter) else {
            continue;
        };
        let bytes = popup.text.as_bytes();
        let rise = (popup.age as f32 * 0.4).min(10.0) as i32;
        let color = match popup.kind {
            PopupKind::Damage => WHITE,
            PopupKind::Heal => [0.55, 1.0, 0.6, 1.0],
            PopupKind::Miss => [1.0, 0.9, 0.5, 1.0],
        };
        let x = at.x as i32 - width(bytes) / 2;
        text(bytes, x, at.y as i32 - 8 - rise, color, &mut out.top);
    }
    if let Some(marker) = view.marker
        && let Some(at) = screen_of(marker)
    {
        let (w, h) = atlas.arrow_size();
        let bounce = (tick / 10 % 2) as i32 * 2;
        atlas.arrow(
            at.x as i32 - w as i32 / 2,
            at.y as i32 - 24 - h as i32 + bounce,
            &mut out.top,
        );
    }

    // Bottom: the status window, then the menu.
    let rows = view.members.len() as i32;
    let status = [
        STATUS_X,
        STATUS_Y,
        STATUS_WIDTH,
        10 + STATUS_ROW * rows.max(1),
    ];
    atlas.window(status, FILL, &mut out.bottom);
    let active = view.menu.as_ref().map(|m| m.member);
    for (i, member) in view.members.iter().enumerate() {
        let y = status[1] + 8 + STATUS_ROW * i as i32;
        if active == Some(i) {
            out.bottom.push(atlas.solid(
                [status[0] + 6, y - 2, status[2] - 12, STATUS_ROW - 2],
                SELECTED,
            ));
        }
        let color = if member.hp <= 0 { GREY } else { WHITE };
        text(&member.name, status[0] + 12, y, color, &mut out.bottom);
        let hp = format!("HP {:>3}/{:<3}", member.hp, member.max_hp);
        let mp = format!("MP {:>3}/{:<3}", member.mp, member.max_mp);
        text(hp.as_bytes(), status[0] + 98, y, color, &mut out.bottom);
        text(mp.as_bytes(), status[0] + 172, y, color, &mut out.bottom);
    }
    if let Some(menu) = &view.menu {
        let menu_y = status[1] + status[3] + 6;
        let (x, w) = if menu.targets {
            (TARGET_X, TARGET_WIDTH)
        } else {
            (MENU_X, MENU_WIDTH)
        };
        draw_menu(atlas, font, menu, [x, menu_y, w], &mut out.bottom);
    }
    out
}

fn draw_menu(atlas: &Atlas, font: &Font<'_>, menu: &MenuView, at: [i32; 3], out: &mut Vec<Quad>) {
    let rows = menu.rows.len().max(1) as i32;
    let window = [at[0], at[1], at[2], 14 + MENU_ROW * rows];
    atlas.window(window, FILL, out);
    for (i, (label, enabled)) in menu.rows.iter().enumerate() {
        let y = window[1] + 8 + MENU_ROW * i as i32;
        if i == menu.cursor {
            out.push(atlas.solid(
                [window[0] + 6, y - 2, window[2] - 12, MENU_ROW - 2],
                SELECTED,
            ));
        }
        let (glyphs, _) = font.layout(label);
        let color = if *enabled { WHITE } else { GREY };
        atlas.text(&glyphs, window[0] + 12, y, color, out);
    }
}

/// Breaks `line` at its spaces so that each part is at most `width` pixels wide (a word wider
/// than that stays whole).
fn wrap(font: &Font<'_>, line: &[u8], width: i32) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    for word in line.split(|&b| b == b' ') {
        match out.last_mut() {
            Some(last) if !last.is_empty() => {
                let mut joined = last.clone();
                joined.push(b' ');
                joined.extend_from_slice(word);
                if font.measure(&joined) <= width {
                    *last = joined;
                } else {
                    out.push(word.to_vec());
                }
            }
            _ => out.push(word.to_vec()),
        }
    }
    if out.is_empty() {
        out.push(Vec::new());
    }
    out
}
