//! The character-creation screen between the title's New Game row and the prologue: the
//! hero's name and sex, then the appearance pages the original's overlay 9 has (hair, hair
//! colour, face, skin tone, eye colour).
//!
//! The original's creation is overlay `ov_009` (`ov_009::021842a0` allocates it, the state
//! machine of `ov_009::02185634` builds the preview and the pages): a paginated editor whose
//! first page is the name entry (`ov_009::02186e00`) and whose second is the sex
//! (`ov_009::0218742c` page 1 writes `+0xda3`), followed by the hair style (`021868c4`,
//! page 3), the hair colour (`021869d8`, page 4), a face-offset page (`021866f0`'s neighbour,
//! page 2, preview only: **not modelled**), the face (`021866f0`, page 5), the skin tone
//! (`02186aec`, page 6) and the eye colour (`02186c00`, page 7), and a 3D preview page
//! (`02187b34`, page 8). Each option page edits one per-sex byte `+0xda4`..`+0xdaf`; the
//! option counts are the list lengths `func_ov009_02188c2c` passes (10 hair, 10 hair colour,
//! 10 face, 8 skin tone, 8 eye colour).
//!
//! The name-entry widget is `ov_003::0215f000` with the key handlers of `ov_003_0215ec90`;
//! the creation screen sizes it to **eight** glyphs (`*(widget + 0x14) = 8` in
//! `ov_009::02186e00`) and the save keeps one message-font glyph index per character
//! ([`dq9_formats::save::NAME_LEN`] bytes at `+0x140` / 320, zero-padded). An empty name does
//! not confirm: `ov_009::02189a4c` returns before setting the accepted flag when the buffer's
//! first byte is zero, so the screen stays.
//!
//! Facts and addresses: `docs/re/characters.md` § "Character creation"; the port's UX and
//! limits: `docs/app/character-creation.md`.

use super::App;
use super::status;
use dq9_formats::Font;
use dq9_formats::hero::{CreationAppearance, HeroLook, Part, Sex};
use dq9_formats::hero::{ItemModels, Outfit};
use dq9_formats::save::NAME_LEN;
use dq9_render::ui::{Atlas, Quad, SCREEN};
use winit::keyboard::KeyCode;

/// The original's name limit, in glyphs (`*(entry + 0x14) = 8`, `ov_009::02186e00`); the save
/// field is wider ([`NAME_LEN`] bytes, zero-padded).
pub(super) const NAME_MAX: usize = 8;

/// The page the creation screen shows: the port's own screen for the name and the sex, then
/// one page per appearance option (the original's pages 3–7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    /// The name with its caret and the sex, enter confirms.
    Name,
    Appearance(Part),
}

/// The parts the current look resolves to, as the rig will assemble them: the preview line of
/// the appearance pages (`Outfit::resolve`, the same code the scene builds the hero with).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Preview {
    face: Option<String>,
    hair: Option<String>,
    hair_texture: Option<String>,
}

/// The creation screen's state: the typed name, the sex, the appearance columns and the page.
#[derive(Debug)]
pub(super) struct Creation {
    /// The name as message-font glyph indices (`Font::find` while typing).
    glyphs: Vec<u8>,
    sex: Sex,
    page: Page,
    /// One column per sex, as `ov_009` keeps them (`+0xda3` indexes `+0xda4`..`+0xdaf`).
    looks: [CreationAppearance; 2],
    /// The look's resolved parts, refreshed when the choice changes; `None` until a ROM is
    /// available (the window's screen always has one).
    preview: Option<Preview>,
}

impl Creation {
    fn new() -> Self {
        Self {
            glyphs: Vec::new(),
            sex: Sex::Man,
            page: Page::Name,
            looks: [
                CreationAppearance::default_for(Sex::Man),
                CreationAppearance::default_for(Sex::Woman),
            ],
            preview: None,
        }
    }

    /// The typed glyphs, without the save's zero padding.
    pub(super) fn glyphs(&self) -> &[u8] {
        &self.glyphs
    }

    pub(super) fn sex(&self) -> Sex {
        self.sex
    }

    /// The choices of the selected sex (the column the arrows edit).
    fn appearance(&self) -> CreationAppearance {
        self.looks[self.sex.index()]
    }

    /// The appearance the chosen options give the selected sex: the creation defaults of
    /// `ov_009::02185634` for everything the pages do not touch (outfit, body build).
    pub(super) fn look(&self) -> HeroLook {
        self.appearance().to_look(self.sex)
    }

    /// The name as the save stores it: the glyphs, zero-padded to [`NAME_LEN`].
    pub(super) fn saved_name(&self) -> [u8; NAME_LEN] {
        let mut out = [0; NAME_LEN];
        let n = self.glyphs.len().min(NAME_LEN);
        out[..n].copy_from_slice(&self.glyphs[..n]);
        out
    }
}

/// Resolves `look`'s parts the way `HeroRig::build` does, without building the rig: the face
/// and hair the preview line names (`ItemModels` + `Outfit::resolve` over `chara_pc.gp2`).
fn preview_of(rom: &[u8], look: &HeroLook) -> Option<Preview> {
    let image = nds_rom::Rom::parse(rom).ok()?;
    let defs = dq9_formats::map::rom_file(&image, "data/prm/itemdt_def.gp2")?;
    let defs = dq9_formats::Gpc2::parse(defs).ok()?;
    let member = defs.find("itemdt_def.nat")?;
    let member = defs.member(member).ok()?;
    let models = ItemModels::parse(&member).ok()?;
    let pc = dq9_formats::map::rom_file(&image, "data/pack_lv5/chara_pc.gp2")?;
    let pc = dq9_formats::Gpc2::parse(pc).ok()?;
    let outfit = Outfit::resolve(look, &models, |name| pc.find(name).is_some());
    Some(Preview {
        face: outfit.face,
        hair: outfit.hair.as_ref().map(|hair| hair.model.clone()),
        hair_texture: outfit.hair.map(|hair| hair.texture),
    })
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
const APPEARANCE_LABEL_TOP: i32 = 64;
const OPTIONS_TOP: i32 = 88;
const OPTION_GAP: i32 = 18;
const PREVIEW_TOP: i32 = 112;
const TEXTURE_TOP: i32 = 128;
const FIRST_FOOTER_TOP: i32 = 136;
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
        self.refresh_creation_preview();
        status(
            "character creation: type a name (8 glyphs at most), Left/Right: sex, Down: \
             appearance, Enter: confirm, Esc: back to the title",
        );
    }

    /// A key of the creation screen; `true` asks the window to close (never: Escape returns to
    /// the title, as the batch's flow asks).
    pub(super) fn creation_key(&mut self, code: KeyCode) -> bool {
        let shift = self.controls.held.contains(&KeyCode::ShiftLeft)
            || self.controls.held.contains(&KeyCode::ShiftRight);
        let page = self.creation.as_ref().map(|creation| creation.page);
        match code {
            KeyCode::Escape => match page {
                Some(Page::Appearance(_)) => self.creation_page_prev(),
                _ => {
                    self.creation = None;
                    self.show_title();
                }
            },
            KeyCode::Enter => match page {
                Some(Page::Appearance(_)) => self.creation_page_next(),
                _ => self.creation_confirm(),
            },
            KeyCode::ArrowDown => self.creation_page_next(),
            KeyCode::ArrowUp => self.creation_page_prev(),
            KeyCode::Backspace => {
                if page == Some(Page::Name)
                    && let Some(creation) = &mut self.creation
                {
                    creation.glyphs.pop();
                }
            }
            KeyCode::ArrowLeft => self.creation_step(-1),
            KeyCode::ArrowRight => self.creation_step(1),
            _ => {
                if page == Some(Page::Name)
                    && let Some(byte) = typed_char(code, shift)
                {
                    self.creation_type(byte);
                }
            }
        }
        false
    }

    /// Left/Right: the sex on the name page (as the original's sex page), the option on an
    /// appearance page (clamped to the list's ends).
    fn creation_step(&mut self, delta: i32) {
        let Some(page) = self.creation.as_ref().map(|creation| creation.page) else {
            return;
        };
        match page {
            Page::Name => {
                let sex = if delta < 0 { Sex::Man } else { Sex::Woman };
                if let Some(creation) = &mut self.creation {
                    creation.sex = sex;
                }
                self.refresh_creation_preview();
            }
            Page::Appearance(part) => {
                if let Some(creation) = &mut self.creation {
                    let sex = creation.sex;
                    creation.looks[sex.index()].step(part, delta);
                }
                self.refresh_creation_preview();
            }
        }
    }

    /// Down/Enter: the next page (name → hair → … → eyes → name).
    fn creation_page_next(&mut self) {
        let Some(creation) = &self.creation else {
            return;
        };
        let page = match creation.page {
            Page::Name => Page::Appearance(Part::Hair),
            Page::Appearance(part) => match Part::ALL.iter().position(|&p| p == part) {
                Some(index) if index + 1 < Part::ALL.len() => {
                    Page::Appearance(Part::ALL[index + 1])
                }
                _ => Page::Name,
            },
        };
        if let Some(creation) = &mut self.creation {
            creation.page = page;
        }
    }

    /// Up/Escape: the previous page (eyes → … → hair → name; the name page stays).
    fn creation_page_prev(&mut self) {
        let Some(creation) = &self.creation else {
            return;
        };
        let page = match creation.page {
            Page::Name => Page::Name,
            Page::Appearance(part) => match Part::ALL.iter().position(|&p| p == part) {
                Some(0) | None => Page::Name,
                Some(index) => Page::Appearance(Part::ALL[index - 1]),
            },
        };
        if let Some(creation) = &mut self.creation {
            creation.page = page;
        }
    }

    /// Re-resolves the look's parts for the preview line after a change (`Outfit::resolve`).
    fn refresh_creation_preview(&mut self) {
        let Some(look) = self.creation.as_ref().map(Creation::look) else {
            return;
        };
        let preview = self.rom.as_deref().and_then(|rom| preview_of(rom, &look));
        if let Some(creation) = &mut self.creation {
            creation.preview = preview;
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

    /// Enter on the creation screen: the selected name, sex and appearance start the new game.
    /// An empty name is refused (`ov_009::02189a4c` never accepts one) and the screen stays;
    /// so is a name whose first glyph is 0, which the save would read back as an empty name.
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

    /// The quads of the creation screen: the port's own window, then the name/sex page or one
    /// appearance page, drawn with the game's font.
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
        match creation.page {
            Page::Name => self.name_page_quads(creation, atlas, font, &mut out),
            Page::Appearance(part) => Self::appearance_quads(creation, part, atlas, font, &mut out),
        }
        out
    }

    /// The name with its caret, the sex with Left/Right and the key hints.
    fn name_page_quads(
        &self,
        creation: &Creation,
        atlas: &Atlas,
        font: &Font<'_>,
        out: &mut Vec<Quad>,
    ) {
        text(
            atlas,
            font,
            b"Name:",
            WINDOW[0] + MARGIN,
            NAME_LABEL_TOP,
            WHITE,
            out,
        );
        let (run, advance) = glyph_run(font, creation.glyphs());
        atlas.text(&run, WINDOW[0] + NAME_X, NAME_LABEL_TOP, WHITE, out);
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
            out,
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
            text(atlas, font, label, x, SEX_LABEL_TOP, WHITE, out);
        }
        let pages = b"Down: hair, face and colours";
        let width = font.measure(pages);
        text(
            atlas,
            font,
            pages,
            WINDOW[0] + (WINDOW[2] - width) / 2,
            WINDOW[1] + FIRST_FOOTER_TOP,
            GREY,
            out,
        );
        let hint = b"Left/Right: sex   Enter: confirm   Esc: back";
        let width = font.measure(hint);
        text(
            atlas,
            font,
            hint,
            WINDOW[0] + (WINDOW[2] - width) / 2,
            WINDOW[1] + FOOTER_TOP,
            GREY,
            out,
        );
    }

    /// One appearance page: the option's number, the page's whole list with the selection
    /// highlighted, and the parts the look resolves to (the preview the rig builds).
    fn appearance_quads(
        creation: &Creation,
        part: Part,
        atlas: &Atlas,
        font: &Font<'_>,
        out: &mut Vec<Quad>,
    ) {
        let value = creation.appearance().get(part);
        let label = format!("{}: {} / {}", part.label(), value + 1, part.options());
        text(
            atlas,
            font,
            label.as_bytes(),
            WINDOW[0] + MARGIN,
            APPEARANCE_LABEL_TOP,
            WHITE,
            out,
        );
        for index in 0..part.options() {
            let x = WINDOW[0] + MARGIN + i32::from(index) * OPTION_GAP;
            let number = (index + 1).to_string();
            if index == value {
                out.push(atlas.solid(
                    [
                        x - 3,
                        OPTIONS_TOP - 3,
                        font.measure(number.as_bytes()) + 6,
                        i32::from(font.height) + 4,
                    ],
                    crate::talk::SELECTED,
                ));
            }
            text(atlas, font, number.as_bytes(), x, OPTIONS_TOP, WHITE, out);
        }
        let preview = match &creation.preview {
            Some(preview) => format!(
                "{}  {}",
                preview.face.as_deref().unwrap_or("-"),
                preview.hair.as_deref().unwrap_or("-")
            ),
            None => "no model preview (no ROM)".to_owned(),
        };
        let width = font.measure(preview.as_bytes());
        text(
            atlas,
            font,
            preview.as_bytes(),
            WINDOW[0] + (WINDOW[2] - width) / 2,
            PREVIEW_TOP,
            WHITE,
            out,
        );
        if part == Part::HairColour
            && let Some(Some(texture)) = creation.preview.as_ref().map(|p| p.hair_texture.as_ref())
        {
            let texture = format!("{texture}.nsbtx");
            let width = font.measure(texture.as_bytes());
            text(
                atlas,
                font,
                texture.as_bytes(),
                WINDOW[0] + (WINDOW[2] - width) / 2,
                TEXTURE_TOP,
                GREY,
                out,
            );
        }
        let hint = b"Left/Right: change   Enter: next   Esc: back";
        let width = font.measure(hint);
        text(
            atlas,
            font,
            hint,
            WINDOW[0] + (WINDOW[2] - width) / 2,
            WINDOW[1] + FOOTER_TOP,
            GREY,
            out,
        );
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
