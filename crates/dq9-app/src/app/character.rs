//! The character-creation screen between the title's New Game row and the prologue: the
//! hero's name and sex, the two fields the message window, the scene and the save consume.
//!
//! The original's creation is overlay `ov_009` (`ov_009::021842a0` allocates it, the state
//! machine of `ov_009::02185634` builds the preview and the pages): a paginated editor whose
//! first page is the name entry (`ov_009::02186e00`) and whose second is the sex
//! (`ov_009::0218742c` page 1 writes `+0xda3`). The name-entry widget is `ov_003::0215f000`
//! with the key handlers of `ov_003_0215ec90`; the creation screen sizes it to **eight**
//! glyphs (`*(widget + 0x14) = 8` in `ov_009::02186e00`) and the save keeps one message-font
//! glyph index per character ([`dq9_formats::save::NAME_LEN`] bytes at `+0x140` / 320,
//! zero-padded). An empty name does not confirm: `ov_009::02189a4c` returns before setting
//! the accepted flag when the buffer's first byte is zero, so the screen stays.
//!
//! Facts and addresses: `docs/re/characters.md` § "Character creation"; the port's UX and
//! limits: `docs/app/character-creation.md`.

use super::App;
use super::status;
use dq9_formats::Font;
use dq9_formats::hero::{HeroLook, Sex};
use dq9_formats::save::NAME_LEN;
use dq9_render::ui::{Atlas, Quad, SCREEN};
use winit::keyboard::KeyCode;

/// The original's name limit, in glyphs (`*(entry + 0x14) = 8`, `ov_009::02186e00`); the save
/// field is wider ([`NAME_LEN`] bytes, zero-padded).
pub(super) const NAME_MAX: usize = 8;

/// The creation screen's state: the typed name and the chosen sex.
#[derive(Debug)]
pub(super) struct Creation {
    /// The name as message-font glyph indices (`Font::find` while typing).
    glyphs: Vec<u8>,
    sex: Sex,
}

impl Creation {
    fn new() -> Self {
        Self {
            glyphs: Vec::new(),
            sex: Sex::Man,
        }
    }

    /// The typed glyphs, without the save's zero padding.
    pub(super) fn glyphs(&self) -> &[u8] {
        &self.glyphs
    }

    pub(super) fn sex(&self) -> Sex {
        self.sex
    }

    /// The name as the save stores it: the glyphs, zero-padded to [`NAME_LEN`].
    pub(super) fn saved_name(&self) -> [u8; NAME_LEN] {
        let mut out = [0; NAME_LEN];
        let n = self.glyphs.len().min(NAME_LEN);
        out[..n].copy_from_slice(&self.glyphs[..n]);
        out
    }

    /// The appearance the chosen sex starts from (`HeroLook::for_sex`, the creation defaults
    /// of `ov_009::02185634`).
    pub(super) fn look(&self) -> HeroLook {
        HeroLook::for_sex(self.sex)
    }
}

/// The glyph index of a typed character in the message font. `None` when the font has no such
/// glyph, so a key the original's keyboard does not offer is not stored. A space is refused:
/// the message font's space is glyph 0 (`< >`), which is also the save field's zero
/// terminator, so a stored space would truncate the name (`docs/app/character-creation.md`).
fn glyph_for(font: &Font<'_>, byte: u8) -> Option<u8> {
    if !byte.is_ascii_graphic() {
        return None;
    }
    font.find(&[byte])
}

/// The character a key types (`None` for a key that is not a character): the DS keyboard's
/// letters (upper case with Shift), digits and the punctuation the port offers.
fn typed_char(code: KeyCode, shift: bool) -> Option<u8> {
    let letter = |upper: u8| if shift { upper } else { upper + b'a' - b'A' };
    Some(match code {
        KeyCode::KeyA => letter(b'A'),
        KeyCode::KeyB => letter(b'B'),
        KeyCode::KeyC => letter(b'C'),
        KeyCode::KeyD => letter(b'D'),
        KeyCode::KeyE => letter(b'E'),
        KeyCode::KeyF => letter(b'F'),
        KeyCode::KeyG => letter(b'G'),
        KeyCode::KeyH => letter(b'H'),
        KeyCode::KeyI => letter(b'I'),
        KeyCode::KeyJ => letter(b'J'),
        KeyCode::KeyK => letter(b'K'),
        KeyCode::KeyL => letter(b'L'),
        KeyCode::KeyM => letter(b'M'),
        KeyCode::KeyN => letter(b'N'),
        KeyCode::KeyO => letter(b'O'),
        KeyCode::KeyP => letter(b'P'),
        KeyCode::KeyQ => letter(b'Q'),
        KeyCode::KeyR => letter(b'R'),
        KeyCode::KeyS => letter(b'S'),
        KeyCode::KeyT => letter(b'T'),
        KeyCode::KeyU => letter(b'U'),
        KeyCode::KeyV => letter(b'V'),
        KeyCode::KeyW => letter(b'W'),
        KeyCode::KeyX => letter(b'X'),
        KeyCode::KeyY => letter(b'Y'),
        KeyCode::KeyZ => letter(b'Z'),
        KeyCode::Digit0 => b'0',
        KeyCode::Digit1 => b'1',
        KeyCode::Digit2 => b'2',
        KeyCode::Digit3 => b'3',
        KeyCode::Digit4 => b'4',
        KeyCode::Digit5 => b'5',
        KeyCode::Digit6 => b'6',
        KeyCode::Digit7 => b'7',
        KeyCode::Digit8 => b'8',
        KeyCode::Digit9 => b'9',
        KeyCode::Period => b'.',
        KeyCode::Comma => b',',
        KeyCode::Quote => b'\'',
        KeyCode::Minus => b'-',
        KeyCode::Slash => b'/',
        KeyCode::Semicolon => b';',
        _ => return None,
    })
}

/// The positions of a run of glyph indices: [`Font::layout`]'s advance and kerning, without a
/// text encoding step.
fn glyph_run(font: &Font<'_>, glyphs: &[u8]) -> (Vec<(u8, i32)>, i32) {
    let mut out = Vec::new();
    let (mut x, mut previous) = (0i32, 0xffu8);
    for &index in glyphs {
        let Some(glyph) = font.glyphs.get(usize::from(index)) else {
            continue;
        };
        x += i32::from(font.kerning(previous, index));
        out.push((index, x));
        x += i32::from(glyph.width) + 1;
        previous = index;
    }
    (out, x)
}

/// The creation screen's window on the 256×192 screen, as the title's (`docs/app/title.md`).
const WINDOW: [i32; 4] = [8, 8, 240, 176];
const MARGIN: i32 = 24;
const HEADING_TOP: i32 = 32;
const NAME_LABEL_TOP: i32 = 72;
const NAME_X: i32 = 88;
const SEX_LABEL_TOP: i32 = 104;
const SEX_X: i32 = 88;
const SEX_GAP: i32 = 64;
const FOOTER_TOP: i32 = 152;
const BACKGROUND: [f32; 4] = [0.02, 0.03, 0.10, 1.0];
const WHITE: [f32; 4] = [1.0; 4];
const GREY: [f32; 4] = [0.55, 0.55, 0.6, 1.0];

impl App {
    /// Opens the creation screen from the title's New Game row. The field is not started
    /// yet: `options.new_game` only becomes true when the name is confirmed.
    pub(super) fn show_creation(&mut self) {
        self.session_over = false;
        self.title_pending = false;
        self.creation = Some(Creation::new());
        status(
            "character creation: type a name (8 glyphs at most), Left/Right: sex, Enter: \
             confirm, Esc: back to the title",
        );
    }

    /// A key of the creation screen; `true` asks the window to close (never: Escape returns to
    /// the title, as the batch's flow asks).
    pub(super) fn creation_key(&mut self, code: KeyCode) -> bool {
        let shift = self.controls.held.contains(&KeyCode::ShiftLeft)
            || self.controls.held.contains(&KeyCode::ShiftRight);
        match code {
            KeyCode::Escape => {
                self.creation = None;
                self.show_title();
            }
            KeyCode::Enter => self.creation_confirm(),
            KeyCode::Backspace => {
                if let Some(creation) = &mut self.creation {
                    creation.glyphs.pop();
                }
            }
            KeyCode::ArrowLeft => self.creation_sex(Sex::Man),
            KeyCode::ArrowRight => self.creation_sex(Sex::Woman),
            _ => {
                if let Some(byte) = typed_char(code, shift) {
                    self.creation_type(byte);
                }
            }
        }
        false
    }

    fn creation_sex(&mut self, sex: Sex) {
        if let Some(creation) = &mut self.creation {
            creation.sex = sex;
        }
    }

    /// Appends one typed character as a message-font glyph index, up to [`NAME_MAX`]. A glyph
    /// the font does not have is dropped (the original's keyboard only offers glyphs it has).
    fn creation_type(&mut self, byte: u8) {
        let Some(creation) = &self.creation else {
            return;
        };
        if creation.glyphs().len() >= NAME_MAX {
            return;
        }
        let Some(glyph) = self.talk.with_ui(|_, font| glyph_for(font, byte)).flatten() else {
            return;
        };
        if let Some(creation) = &mut self.creation {
            creation.glyphs.push(glyph);
        }
    }

    /// Enter on the creation screen: the selected name and sex start the new game. An empty
    /// name is refused (`ov_009::02189a4c` never accepts one) and the screen stays; so is a
    /// name whose first glyph is 0, which the save would read back as an empty name.
    fn creation_confirm(&mut self) {
        let Some(creation) = &self.creation else {
            return;
        };
        if creation.glyphs().first().copied().unwrap_or(0) == 0 {
            status("character creation: the name cannot be empty");
            return;
        }
        let creation = self.creation.take().expect("checked above");
        self.options.new_game = true;
        self.options.save = None;
        self.options.map = None;
        self.options.event = None;
        self.options.battle = None;
        self.start_created_game(creation);
    }

    /// The quads of the creation screen: the port's own window, the name with its caret, the
    /// sex with Left/Right, and the key hints, drawn with the game's font.
    pub(super) fn creation_quads(&self, atlas: &Atlas, font: &Font<'_>) -> Vec<Quad> {
        let mut out = Vec::new();
        out.push(atlas.solid([0, 0, SCREEN.0 as i32, SCREEN.1 as i32], BACKGROUND));
        atlas.window(WINDOW, crate::talk::FILL, &mut out);
        let heading = b"Create your hero";
        let width = font.measure(heading);
        text(
            atlas,
            font,
            heading,
            WINDOW[0] + (WINDOW[2] - width) / 2,
            HEADING_TOP,
            WHITE,
            &mut out,
        );
        let Some(creation) = &self.creation else {
            return out;
        };
        text(
            atlas,
            font,
            b"Name:",
            WINDOW[0] + MARGIN,
            NAME_LABEL_TOP,
            WHITE,
            &mut out,
        );
        let (run, advance) = glyph_run(font, creation.glyphs());
        atlas.text(&run, WINDOW[0] + NAME_X, NAME_LABEL_TOP, WHITE, &mut out);
        if creation.glyphs().len() < NAME_MAX {
            let caret = WINDOW[0] + NAME_X + advance;
            out.push(atlas.solid(
                [caret, NAME_LABEL_TOP + 1, 2, i32::from(font.height) - 2],
                WHITE,
            ));
        }
        text(
            atlas,
            font,
            b"Sex:",
            WINDOW[0] + MARGIN,
            SEX_LABEL_TOP,
            WHITE,
            &mut out,
        );
        for (index, label) in [b"Man".as_slice(), b"Woman".as_slice()]
            .into_iter()
            .enumerate()
        {
            let x = WINDOW[0] + SEX_X + index as i32 * SEX_GAP;
            let selected = (index == 0) == (creation.sex == Sex::Man);
            if selected {
                out.push(atlas.solid(
                    [
                        x - 4,
                        SEX_LABEL_TOP - 3,
                        font.measure(label) + 8,
                        i32::from(font.height) + 4,
                    ],
                    crate::talk::SELECTED,
                ));
            }
            text(atlas, font, label, x, SEX_LABEL_TOP, WHITE, &mut out);
        }
        let hint = b"Left/Right: sex   Enter: confirm   Esc: back";
        let width = font.measure(hint);
        text(
            atlas,
            font,
            hint,
            WINDOW[0] + (WINDOW[2] - width) / 2,
            WINDOW[1] + FOOTER_TOP,
            GREY,
            &mut out,
        );
        out
    }
}

fn text(
    atlas: &Atlas,
    font: &Font<'_>,
    text: &[u8],
    x: i32,
    y: i32,
    color: [f32; 4],
    out: &mut Vec<Quad>,
) {
    let (glyphs, _) = font.layout(text);
    atlas.text(&glyphs, x, y, color, out);
}

#[cfg(all(test, windows))]
mod tests;
