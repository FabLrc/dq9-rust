//! The creation screen's state machine and what it hands to the game: the message identity,
//! the look and the save record (real ROM and save where those paths need them; the glyph
//! encoding itself is exercised on the ROM's message font).

use super::{App, NAME_MAX};
use crate::app::Options;
use dq9_core::state::GameState;
use dq9_formats::hero::Sex;
use dq9_formats::save::{NAME_LEN, Save};
use winit::keyboard::KeyCode;

fn options(save: Option<Vec<u8>>) -> Options {
    Options {
        map: None,
        save,
        time: None,
        clock: None,
        layout: crate::screens::Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story::default(),
        story_override: [None; 3],
        new_game: false,
        battle: None,
        play: Default::default(),
    }
}

fn app(rom: &[u8], save: Option<Vec<u8>>) -> App {
    App::new_headless(options(save), Some(rom.to_vec()))
}

/// `DQ9_SAVE`: the demo cartridge's normal save (no save is shipped with the repository).
fn saved_container() -> Option<Vec<u8>> {
    let path = std::env::var_os("DQ9_SAVE")?;
    Some(std::fs::read(path).expect("DQ9_SAVE is readable"))
}

/// Types `text` on the creation screen, with Shift for an upper-case letter.
fn type_name(app: &mut App, text: &str) {
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' => {
                app.controls.held.insert(KeyCode::ShiftLeft);
                app.creation_key(letter(byte));
                app.controls.held.remove(&KeyCode::ShiftLeft);
            }
            b'a'..=b'z' => {
                app.creation_key(letter(byte.to_ascii_uppercase()));
            }
            b'0'..=b'9' => {
                let code = match byte {
                    b'0' => KeyCode::Digit0,
                    b'1' => KeyCode::Digit1,
                    b'2' => KeyCode::Digit2,
                    b'3' => KeyCode::Digit3,
                    b'4' => KeyCode::Digit4,
                    b'5' => KeyCode::Digit5,
                    b'6' => KeyCode::Digit6,
                    b'7' => KeyCode::Digit7,
                    b'8' => KeyCode::Digit8,
                    _ => KeyCode::Digit9,
                };
                app.creation_key(code);
            }
            _ => panic!("test name has an unmapped character: {byte}"),
        }
    }
}

fn letter(upper: u8) -> KeyCode {
    match upper {
        b'A' => KeyCode::KeyA,
        b'B' => KeyCode::KeyB,
        b'C' => KeyCode::KeyC,
        b'D' => KeyCode::KeyD,
        b'E' => KeyCode::KeyE,
        b'F' => KeyCode::KeyF,
        b'G' => KeyCode::KeyG,
        b'H' => KeyCode::KeyH,
        b'I' => KeyCode::KeyI,
        b'J' => KeyCode::KeyJ,
        b'K' => KeyCode::KeyK,
        b'L' => KeyCode::KeyL,
        b'M' => KeyCode::KeyM,
        b'N' => KeyCode::KeyN,
        b'O' => KeyCode::KeyO,
        b'P' => KeyCode::KeyP,
        b'Q' => KeyCode::KeyQ,
        b'R' => KeyCode::KeyR,
        b'S' => KeyCode::KeyS,
        b'T' => KeyCode::KeyT,
        b'U' => KeyCode::KeyU,
        b'V' => KeyCode::KeyV,
        b'W' => KeyCode::KeyW,
        b'X' => KeyCode::KeyX,
        b'Y' => KeyCode::KeyY,
        b'Z' => KeyCode::KeyZ,
        _ => panic!("not a letter: {upper}"),
    }
}

fn glyphs(app: &App) -> Vec<u8> {
    app.creation
        .as_ref()
        .expect("the creation screen is shown")
        .glyphs()
        .to_vec()
}

/// New Game opens the creation screen, Escape returns to the title, and the field has not
/// started meanwhile (`options.new_game` stays false).
#[test]
fn new_game_opens_creation_and_escape_returns_to_the_title() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.interactive = true;
    app.show_title();
    assert!(!app.title_key(KeyCode::Enter), "Enter does not close");
    assert!(
        app.title.is_none(),
        "the title gave way to the creation screen"
    );
    assert!(app.creation.is_some(), "the creation screen is shown");
    assert!(!app.options.new_game, "the new game starts on confirm only");

    assert!(!app.creation_key(KeyCode::Escape), "Escape does not close");
    assert!(app.creation.is_none(), "Escape left the creation screen");
    assert!(app.title.is_some(), "Escape returned to the title");
}

/// Title → creation → confirm: the typed name and sex reach the message identity, the look,
/// the party and the opening map (6405, the original's `S14M05` before the prologue).
#[test]
fn the_flow_runs_from_the_title_to_the_opening_map() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.interactive = true;
    app.show_title();
    app.title_key(KeyCode::Enter);
    type_name(&mut app, "Adenine");
    assert!(!app.creation_key(KeyCode::Enter), "Enter does not close");
    assert!(app.creation.is_none(), "the creation screen gave way");
    assert!(app.options.new_game, "the new-game path ran");
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(6405),
        "the new game loads the opening map (S14M05)"
    );
    assert_eq!(app.talk.hero_name(), b"Adenine", "the message identity");
    assert_eq!(app.look.sex, Sex::Man);
    assert_eq!(app.look.face_item, 9024, "the man's face preset");
    assert_eq!(
        app.look.hair_item, 9000,
        "the man's hair preset (table entry 0)"
    );
}

/// The name is stored as message-font glyph indices: the demo save's "Adenine" is
/// `12 2f 30 39 34 39 30` (`docs/re/save.md`), and the identity decodes back to the text.
#[test]
fn a_typed_name_is_stored_as_message_font_glyphs() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.show_creation();
    type_name(&mut app, "Adenine");
    assert_eq!(
        glyphs(&app),
        [0x12, 0x2f, 0x30, 0x39, 0x34, 0x39, 0x30],
        "Adenine's glyph indices in the me font"
    );
    assert_eq!(
        app.creation.as_ref().unwrap().saved_name(),
        [0x12, 0x2f, 0x30, 0x39, 0x34, 0x39, 0x30, 0, 0, 0],
        "zero-padded to NAME_LEN"
    );
    app.creation_key(KeyCode::Enter);
    let hero = app.party.members.first().expect("the hero is in the party");
    assert_eq!(
        hero.name,
        [0x12, 0x2f, 0x30, 0x39, 0x34, 0x39, 0x30, 0, 0, 0],
        "the party record"
    );
    assert_eq!(app.talk.hero_name(), b"Adenine");
}

/// Left/Right moves the sex, and the chosen sex reaches the look and the party record.
#[test]
fn the_sex_moves_with_the_arrows() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.show_creation();
    app.creation_key(KeyCode::ArrowRight);
    assert_eq!(app.creation.as_ref().unwrap().sex(), Sex::Woman);
    type_name(&mut app, "Aria");
    app.creation_key(KeyCode::Enter);
    assert_eq!(app.look.sex, Sex::Woman, "the look the scene assembles");
    assert_eq!(app.look.face_item, 9021, "the woman's face preset");
    assert_eq!(app.look.hair_item, 9006, "the woman's hair preset");
    assert_eq!(app.look.eye_color, 5, "the woman's eye colour");
    assert_eq!(app.look.hair_color, 3, "the woman's hair colour");
    let hero = app.party.members.first().expect("the hero is in the party");
    assert_eq!(hero.sex, Sex::Woman);
}

/// The window's key path (not just `creation_key`): `App::key` routes every key to the
/// creation screen, Shift selects the upper-case letter, and Left/Right move the sex.
#[test]
fn the_window_key_path_types_and_moves_the_sex() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.show_creation();
    app.key(KeyCode::ShiftLeft, true);
    app.key(KeyCode::KeyA, true);
    app.key(KeyCode::ShiftLeft, false);
    app.key(KeyCode::KeyA, false);
    app.key(KeyCode::KeyD, true);
    app.key(KeyCode::KeyD, false);
    assert_eq!(glyphs(&app), [0x12, 0x2f], "A d, through App::key");
    app.key(KeyCode::ArrowRight, true);
    app.key(KeyCode::ArrowRight, false);
    assert_eq!(app.creation.as_ref().unwrap().sex(), Sex::Woman);
    app.key(KeyCode::ArrowLeft, true);
    app.key(KeyCode::ArrowLeft, false);
    assert_eq!(app.creation.as_ref().unwrap().sex(), Sex::Man);
    app.key(KeyCode::Backspace, true);
    app.key(KeyCode::Backspace, false);
    assert_eq!(glyphs(&app), [0x12]);
}

/// A space is not a name character: the font's space is glyph 0, the save field's zero
/// terminator, so storing it would truncate the name.
#[test]
fn a_space_is_not_a_name_character() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.show_creation();
    app.key(KeyCode::Space, true);
    assert!(app.creation.as_ref().unwrap().glyphs().is_empty());
    app.key(KeyCode::Space, false);
    app.key(KeyCode::Enter, true);
    assert!(app.creation.is_some(), "an empty name does not confirm");
    assert!(!app.options.new_game);
}

/// An empty name does not confirm (`ov_009::02189a4c` returns before accepting one).
#[test]
fn an_empty_name_does_not_confirm() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.show_creation();
    app.creation_key(KeyCode::Enter);
    assert!(app.creation.is_some(), "the screen stays");
    assert!(!app.options.new_game, "no game started");
    assert!(
        app.party
            .members
            .first()
            .is_some_and(|hero| hero.name == [0; NAME_LEN])
    );
}

/// Backspace removes a glyph and the eight-glyph limit holds.
#[test]
fn backspace_and_the_eight_glyph_limit() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.show_creation();
    type_name(&mut app, "Abcdefghij");
    assert_eq!(glyphs(&app).len(), NAME_MAX, "the original's eight glyphs");
    app.creation_key(KeyCode::Backspace);
    assert_eq!(glyphs(&app).len(), NAME_MAX - 1);
}

/// `--new-game` keeps the default hero: no title, no creation screen, the male look and an
/// empty name (the headless drivers' start).
#[test]
fn the_cli_new_game_keeps_the_default_hero() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut options = options(None);
    options.new_game = true;
    let app = App::new_headless(options, Some(rom.to_vec()));
    assert!(app.creation.is_none(), "the CLI path skips the screen");
    assert!(app.title.is_none());
    assert_eq!(app.look, dq9_formats::hero::HeroLook::new_game());
    assert!(
        app.party
            .members
            .first()
            .is_some_and(|hero| hero.name == [0; NAME_LEN])
    );
}

/// The Confession's write carries the created name and sex: the demo container's "Adenine"
/// is replaced by the typed glyphs and the `+0x174` bit 0 follows the choice.
#[test]
fn a_written_save_carries_the_created_name_and_sex() {
    let (Some(rom), Some(bytes)) = (testrom::load(), saved_container()) else {
        eprintln!("skipping: DQ9_ROM or DQ9_SAVE missing");
        return;
    };
    let dir = std::env::temp_dir().join(format!("dq9-creation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("out.sav");

    let mut app = app(rom, Some(bytes));
    app.creation = Some(super::Creation::new());
    type_name(&mut app, "Borin");
    app.creation_key(KeyCode::ArrowRight);
    let creation = app.creation.take().expect("the creation screen is shown");
    app.start_created_game(creation);

    let state = app.write_save_out(&out).expect("the save is written");
    let written = Save::parse(&std::fs::read(&out).unwrap()).expect("the output parses");
    let reloaded = GameState::from_save(&written).expect("the output restores");
    let hero = reloaded
        .party
        .members
        .first()
        .expect("the party has its hero");
    assert_eq!(
        hero.name,
        [0x13, 0x3a, 0x3d, 0x34, 0x39, 0, 0, 0, 0, 0],
        "\"Borin\"'s glyph indices, replacing the demo save's Adenine"
    );
    assert_eq!(hero.sex, Sex::Woman, "the +0x174 bit 0 follows the choice");
    assert_eq!(state.map, 6405);
    assert_eq!(
        reloaded.party.members.first().map(|h| h.name),
        Some(hero.name),
        "the state the write reported and the reload agree"
    );
}
