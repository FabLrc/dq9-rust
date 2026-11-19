//! Start-up options of the application and what reads them: the story point they name
//! and the item tables a ROM load needs.

use super::{fight, status};
use crate::record;
use crate::screens::Layout;

/// Start-up options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Explicit map override; save map is used if absent.
    pub map: Option<String>,
    pub save: Option<Vec<u8>>,
    /// Time of day of the map assembly (0-3); the save's clock phase, else 0, when absent.
    pub time: Option<u32>,
    /// Explicit second of the 420-second day (`--clock`); its phase replaces `time`.
    pub clock: Option<f32>,
    /// Which DS screens are shown and where (`screens.rs`).
    pub layout: Layout,
    /// No music, and no sound device opened until `M` unmutes.
    pub mute: bool,
    /// Mix the sound offline (no device) so that `App::take_recording` can write it.
    pub record_sound: bool,
    /// Record the pad and action key of every fixed step (`--trace-inputs`).
    pub trace_inputs: bool,
    /// An event to start as soon as the map is loaded (`--event`).
    pub event: Option<u16>,
    pub story: dq9_formats::cast::Story,
    /// Explicit chapter/step/substep overrides for a loaded save.
    pub story_override: [Option<u8>; 3],
    /// Start a new game: the first scene of the story (`docs/app.md`, § New game).
    pub new_game: bool,
    /// Show a battle scene from the start (`--battle`, `?battle=`): an `eventbattle.bin`
    /// record or an explicit stage and its monsters.
    pub battle: Option<crate::battle::Request>,
    /// How battles are played (`--fight`, `--seed`).
    pub play: fight::BattlePlay,
}

impl Options {
    /// The options a recorded session started with (`--play-record`): the header's start
    /// options, the seed, the window's presentation. `mute` is the replay's choice: the
    /// record carries the sound device's effect on the logic (the jingle ends).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn from_record(
        header: &record::Header,
        save: Option<Vec<u8>>,
        layout: Layout,
        mute: bool,
    ) -> Self {
        Self {
            map: header.map.clone(),
            save,
            time: header.time,
            clock: header.clock,
            layout,
            mute,
            record_sound: false,
            trace_inputs: false,
            event: header.event,
            story: story_of(
                [header.chapter, header.step, header.substep],
                !header.night,
                header.stage,
            ),
            story_override: [header.chapter, header.step, header.substep],
            new_game: header.new_game,
            battle: None,
            play: fight::BattlePlay {
                fight: true,
                seed: Some(header.seed),
            },
        }
    }
}

/// The story point the start options name: the chapter, step and substep (1.1.1 by default),
/// then a `--stage MAJOR.MINOR`'s own.
pub(crate) fn story_of(
    point: [Option<u8>; 3],
    daytime: bool,
    stage: Option<(u32, u32)>,
) -> dq9_formats::cast::Story {
    let mut story = dq9_formats::cast::Story {
        chapter: point[0].unwrap_or(1),
        step: point[1].unwrap_or(1),
        substep: point[2].unwrap_or(1),
        daytime,
        ..dq9_formats::cast::Story::default()
    };
    if let Some((major, minor)) = stage {
        story.chapter = major as u8;
        story.step = minor as u8;
    }
    story
}

/// Reads the item id → compendium index tables (`data/prm/itemdt_<category>.gp2`,
/// `docs/re/item.md`) of one language. On a missing or malformed member the table stops
/// there: item flags are then not raised rather than guessed.
pub(super) fn item_table(rom: &[u8]) -> dq9_formats::item::ItemTable {
    use dq9_formats::item::{CATEGORIES, ItemTable};
    let Ok(image) = nds_rom::Rom::parse(rom) else {
        return ItemTable::default();
    };
    let mut table = ItemTable::default();
    for category in CATEGORIES {
        let path = format!("data/prm/itemdt_{category}.gp2");
        let member = image.file_by_path(&path).ok().and_then(|file| {
            let archive = dq9_formats::Gpc2::parse(file).ok()?;
            let name = format!("itemdt_{category}_en.nat");
            let entry = archive.entries.iter().find(|e| e.name == name)?;
            archive.member(entry).ok()
        });
        let Some(member) = member else {
            status(&format!("item table {path}: no {category} member"));
            return table;
        };
        if let Err(e) = table.add_member(&member) {
            status(&format!("item table {path}: {e}"));
            return table;
        }
    }
    table
}
