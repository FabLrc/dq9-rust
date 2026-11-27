//! The title screen: the rows the original's slot check ranks (`ov_004::0216dad0`) and what
//! the port does with them.
//!
//! The original boots into overlay `ov_004`, whose slot check reads `Save_SlotStatus` of both
//! slots and builds an entry code: a present suspend save (status 4) ranks above a normal log
//! (status 0 on slot 0), which ranks above an empty pair (`docs/re/save.md`, § "Which slot a
//! start-up takes"). Continue resumes the ranked slot through `Save_LoadSlot`
//! (`ov_004::0216d504` loads slot 0, `ov_004::0216e490` slot 1), then the field entry
//! (`ov_017::0218b688`) switches on `GameState+4` to load the same slot again. The function
//! that consumes the entry code and builds the menu is **[unknown]**: it is not in the
//! decompilation corpus, so the two rows here are the port's own presentation
//! (`docs/app/title.md`).
//!
//! The title's graphics are not decoded either: the window draws the port's own rows on a
//! plain background, with the game's font, and nothing of the original's art.
//!
//! Input: up/down (arrows or `W`/`S`) move the cursor, `Enter`/`Space`/`Z` choose a row, a tap
//! on a row chooses it, `M` mutes and `R` cycles the layout, as on the other screens. `Escape`
//! closes the native window.

use super::App;
use super::status;
use crate::load;
use crate::talk::HeroIdentity;
use dq9_core::party::Party;
use dq9_core::state::GameState;
use dq9_formats::Font;
use dq9_formats::hero::HeroLook;
use dq9_formats::save::{SUSPEND_SLOT, Save, SlotStatus};
use dq9_render::ui::{Atlas, Quad, SCREEN};
use winit::keyboard::KeyCode;

/// One row of the title's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    /// Resume the slot `Save::title_slot` ranks (a suspend save above a normal log).
    Continue,
    /// The current new-game path (`App::start_new_game`).
    NewGame,
}

impl Row {
    fn label(self) -> &'static [u8] {
        match self {
            Self::Continue => b"Continue your adventure",
            Self::NewGame => b"Create a new adventure log",
        }
    }
}

/// The title screen's state: its rows and the cursor.
#[derive(Debug)]
pub(super) struct Title {
    rows: Vec<Row>,
    cursor: usize,
}

impl Title {
    /// A title with Continue when a container has a resumable slot, New Game otherwise.
    fn new(resume: bool) -> Self {
        let rows = if resume {
            vec![Row::Continue, Row::NewGame]
        } else {
            vec![Row::NewGame]
        };
        Self { rows, cursor: 0 }
    }

    /// The row a tap at `(x, y)` of the 256×192 screen takes.
    fn row_at(&self, (x, y): (f32, f32)) -> Option<usize> {
        let inside = |i: usize| {
            let top = ROW_TOP + i as i32 * ROW;
            x >= (WINDOW[0] + MARGIN) as f32
                && x < (WINDOW[0] + WINDOW[2] - MARGIN) as f32
                && y >= top as f32
                && y < (top + ROW) as f32
        };
        (0..self.rows.len()).find(|&i| inside(i))
    }
}

/// The title's window on the 256×192 screen: the field message window's rectangle grown to
/// the screen, as the port's other menus (`docs/app/title.md`).
const WINDOW: [i32; 4] = [8, 8, 240, 176];
const MARGIN: i32 = 16;
const HEADING_TOP: i32 = 40;
const ROW_TOP: i32 = 104;
const ROW: i32 = 24;
const BACKGROUND: [f32; 4] = [0.02, 0.03, 0.10, 1.0];
const WHITE: [f32; 4] = [1.0; 4];
const GREY: [f32; 4] = [0.55, 0.55, 0.6, 1.0];

impl App {
    /// Whether this interactive start has no start-forcing option: the title screen then
    /// boots before the field (`App::set_rom`).
    pub(super) fn shows_title(&self) -> bool {
        self.interactive
            && !(self.options.new_game
                || self.options.save.is_some()
                || self.options.map.is_some()
                || self.options.event.is_some()
                || self.options.battle.is_some())
    }

    /// Shows the title: the rows come from the container the session reads (`--save`'s file
    /// re-read from disk when it exists, else the bytes it started with). The field, if any,
    /// stays loaded but is not stepped nor drawn while the title holds.
    pub(super) fn show_title(&mut self) {
        self.session_over = false;
        self.title_pending = false;
        let resume = self
            .title_container()
            .and_then(|bytes| Save::parse(&bytes).ok())
            .is_some_and(|save| save.title_slot().is_some());
        self.title = Some(Title::new(resume));
        status(&format!(
            "title: {} row(s), Continue {}",
            if resume { 2 } else { 1 },
            if resume {
                "offered"
            } else {
                "absent (no resumable slot)"
            }
        ));
    }

    /// The container the title reads: `--save`'s file when it parses, else the bytes the app
    /// started with. `--suspend-out` writes elsewhere: a quick save written there is not the
    /// cartridge the title reads, as on the DS.
    fn title_container(&self) -> Option<Vec<u8>> {
        if let Some(path) = &self.save_path
            && let Ok(bytes) = std::fs::read(path)
            && Save::parse(&bytes).is_ok()
        {
            return Some(bytes);
        }
        self.options.save.clone()
    }

    /// A key of the title screen; `true` asks the window to close (`Escape`).
    pub(super) fn title_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::ArrowUp | KeyCode::KeyW => self.title_move(-1),
            KeyCode::ArrowDown | KeyCode::KeyS => self.title_move(1),
            KeyCode::Enter | KeyCode::Space | KeyCode::KeyZ => self.title_activate(),
            KeyCode::Escape => return true,
            KeyCode::KeyM => self.audio.toggle_mute(),
            KeyCode::KeyR => self.set_layout(self.options.layout.next()),
            _ => {}
        }
        false
    }

    fn title_move(&mut self, delta: i32) {
        let Some(title) = &mut self.title else {
            return;
        };
        let count = title.rows.len() as i32;
        title.cursor = (title.cursor as i32 + delta).rem_euclid(count) as usize;
    }

    /// A tap on the title screen's field (`App::tap`): a row is chosen, elsewhere nothing.
    pub(super) fn title_tap(&mut self, point: (f32, f32)) {
        let Some(index) = self.title.as_ref().and_then(|title| title.row_at(point)) else {
            return;
        };
        if let Some(title) = &mut self.title {
            title.cursor = index;
        }
        self.title_activate();
    }

    fn title_activate(&mut self) {
        let Some(row) = self
            .title
            .as_ref()
            .and_then(|title| title.rows.get(title.cursor).copied())
        else {
            return;
        };
        match row {
            Row::Continue => self.title_continue(),
            Row::NewGame => self.title_new_game(),
        }
    }

    /// Continue: the slot `Save::title_slot` ranks, through [`App::begin`] (the save's trigger
    /// state, party, look, day and map). A resumed suspend log is deleted from the container
    /// afterwards, as `Save_WriteHeaderA` (`0x020aba6c`) does after the resume.
    fn title_continue(&mut self) {
        let Some(bytes) = self.title_container() else {
            status("title: Continue has no save container");
            return;
        };
        let (slot, start) = match start_from_container(&bytes) {
            Ok(start) => start,
            Err(e) => {
                status(&format!("title: Continue failed: {e}"));
                return;
            }
        };
        self.title = None;
        self.options.save = Some(bytes.clone());
        self.options.new_game = false;
        self.options.map = None;
        self.options.event = None;
        self.options.battle = None;
        self.reset_session();
        self.reset_title_start();
        let map = start.map.clone();
        self.begin(start);
        let track = &self.state.tracks[self.state.track];
        status(&format!(
            "title: Continue resumed slot {slot} ({map}), story {}.{}.{}",
            track.chapter, track.step, track.substep
        ));
        let mut image = bytes.clone();
        if slot == SUSPEND_SLOT
            && let Some(path) = &self.save_path
        {
            let mut parsed = match Save::parse(&image) {
                Ok(save) => save,
                Err(e) => {
                    status(&format!("title: the container does not parse: {e}"));
                    return;
                }
            };
            match delete_suspend_log(path, &mut parsed) {
                Ok(backup) => {
                    image = parsed.as_bytes().to_vec();
                    self.options.save = Some(image);
                    if backup {
                        status(&format!(
                            "title: the quick save log in {} was backed up (`Creating backup.`), \
                             as the resume does",
                            path.display()
                        ));
                    } else {
                        status(&format!(
                            "title: the quick save log in {} was deleted, as the resume does",
                            path.display()
                        ));
                    }
                }
                Err(e) => status(&format!("title: the quick save log was not deleted: {e:#}")),
            }
        }
    }

    /// New Game: the character-creation screen (`App::show_creation`). The new game itself
    /// only starts once its name and sex are confirmed, so `options.new_game` stays false
    /// until then; `--new-game` keeps the default hero and skips the screen.
    fn title_new_game(&mut self) {
        self.title = None;
        self.options.save = None;
        self.options.map = None;
        self.options.event = None;
        self.options.battle = None;
        self.show_creation();
    }

    /// Clears the previous session's leftovers before a title start: the encounter and fight
    /// state, the journal and the title's own pending notice. `begin` resets the rest.
    fn reset_title_start(&mut self) {
        self.encounter.battle = None;
        self.combat.fight = None;
        self.encounter.battle_change = None;
        self.encounter.saved_field = None;
        self.encounter.battle_cover = 0.0;
        self.encounter.last_battle = None;
        self.quiet_events = false;
        self.cast_reload = false;
        self.journal = Default::default();
        self.checkpoints.clear();
        self.tick = 0;
    }

    /// The quads of the title on the 256×192 screen: the plain background, the heading, the
    /// rows with their cursor and a footer hint. The original's title art is not decoded, so
    /// this is the port's own placeholder (`docs/app/title.md`).
    pub(super) fn title_quads(&self, atlas: &Atlas, font: &Font<'_>) -> Vec<Quad> {
        let mut out = Vec::new();
        out.push(atlas.solid([0, 0, SCREEN.0 as i32, SCREEN.1 as i32], BACKGROUND));
        atlas.window(WINDOW, crate::talk::FILL, &mut out);
        let heading = b"Dragon Quest IX";
        let width = font.measure(heading);
        atlas_text(
            atlas,
            font,
            heading,
            WINDOW[0] + (WINDOW[2] - width) / 2,
            HEADING_TOP,
            WHITE,
            &mut out,
        );
        let Some(title) = &self.title else {
            return out;
        };
        for (index, row) in title.rows.iter().enumerate() {
            let y = ROW_TOP + index as i32 * ROW;
            if index == title.cursor {
                out.push(atlas.solid(
                    [WINDOW[0] + 8, y - 3, WINDOW[2] - 16, ROW - 2],
                    crate::talk::SELECTED,
                ));
            }
            let label = row.label();
            let width = font.measure(label);
            atlas_text(
                atlas,
                font,
                label,
                WINDOW[0] + (WINDOW[2] - width) / 2,
                y,
                WHITE,
                &mut out,
            );
        }
        let hint = b"Enter: choose";
        atlas_text(
            atlas,
            font,
            hint,
            WINDOW[0] + MARGIN,
            WINDOW[1] + WINDOW[3] - MARGIN,
            GREY,
            &mut out,
        );
        out
    }
}

fn atlas_text(
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

/// The [`load::Start`] of the slot the title ranks (`Save::title_slot`), with the same fields
/// `load::start` builds from a save: the state (`GameState::from_slot`, the warp's map on a
/// suspend slot), the party, the hero's look and the message identity.
fn start_from_container(bytes: &[u8]) -> Result<(usize, load::Start), String> {
    let save = Save::parse(bytes).map_err(|e| format!("parsing the save: {e}"))?;
    let index = save
        .title_slot()
        .ok_or_else(|| "no resumable adventure log".to_owned())?;
    let slot = save
        .slot(index)
        .ok_or_else(|| format!("slot {index} is out of range"))?;
    let game = GameState::from_slot(&slot).map_err(|e| format!("restoring the save: {e:?}"))?;
    let hero = slot.character(usize::from(slot.standby_count()));
    let look = hero
        .as_ref()
        .map_or_else(HeroLook::new_game, HeroLook::from_character);
    let identity = hero.as_ref().and_then(|character| {
        Some(HeroIdentity {
            name: character.name_raw().try_into().ok()?,
            male: character.gender() == 0,
        })
    });
    let party = Party::from_slot(&slot).unwrap_or_else(|_| Party::new_game());
    let map = game.map.to_string();
    Ok((
        index,
        load::Start {
            map,
            state: Some(game),
            party,
            look,
            identity,
        },
    ))
}

/// The write the game leaves after a resume. `Save_WriteHeaderA` (`0x020aba6c`) empties slot
/// 1's header A; a container that also holds a **valid normal log** ends differently — the
/// resume writes that log as a normal save ("Creating backup."), whose mirror step
/// (`Save_WriteStep` state 2, `0x020aad2c`) leaves slot 1 as slot 0's image — so
/// `Save::finish_suspend_resume` decides. Only an intermission header is touched, and the file
/// is replaced through a temporary and a rename. Returns whether the backup was written.
fn delete_suspend_log(path: &std::path::Path, image: &mut Save) -> anyhow::Result<bool> {
    let Some(slot) = image.slot(SUSPEND_SLOT) else {
        return Ok(false);
    };
    if slot.status() != SlotStatus::Intermission {
        return Ok(false);
    }
    let backup = image.finish_suspend_resume(SUSPEND_SLOT);
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("the path has no file name"))?;
    let mut temp = path.to_path_buf();
    temp.set_file_name(format!("{}.tmp", name.to_string_lossy()));
    std::fs::write(&temp, image.as_bytes())?;
    std::fs::rename(&temp, path)?;
    Ok(backup)
}

#[cfg(all(test, windows))]
mod tests;
