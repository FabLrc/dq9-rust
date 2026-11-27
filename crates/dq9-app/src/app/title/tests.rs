//! The title screen's state machine, on the application (real ROM and save where the paths
//! need them; the ranking itself is `dq9_formats::save`'s test).

use super::App;
use super::Row;
use crate::app::Options;
use dq9_formats::save::{SUSPEND_SLOT, Save, SlotStatus};
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

fn rows(app: &App) -> Vec<Row> {
    app.title.as_ref().expect("the title is shown").rows.clone()
}

fn cursor(app: &App) -> usize {
    app.title.as_ref().expect("the title is shown").cursor
}

/// `DQ9_SAVE`, a normal save of the demo cartridge (no save is shipped with the repository).
fn saved_container() -> Option<Vec<u8>> {
    let path = std::env::var_os("DQ9_SAVE")?;
    Some(std::fs::read(path).expect("DQ9_SAVE is readable"))
}

/// A container whose slot 1 is the mirror copy turned into a present suspend save.
fn with_a_suspend_save(bytes: &[u8]) -> Vec<u8> {
    let mut save = Save::parse(bytes).expect("the demo save parses");
    {
        let mut slot = save.slot_mut(SUSPEND_SLOT).expect("slot 1");
        slot.set_intermission(true);
        slot.set_exists(true);
        slot.seal();
    }
    save.as_bytes().to_vec()
}

/// An interactive start with no start-forcing option boots on the title (the port's gate);
/// a headless driver keeps today's field start.
#[test]
fn the_interactive_start_boots_on_the_title_and_a_headless_one_does_not() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    assert!(!app.shows_title(), "a headless start never shows the title");
    assert!(app.title.is_none());

    app.interactive = true;
    assert!(app.shows_title(), "no --map/--save/--new-game: the title");
    app.show_title();
    assert_eq!(rows(&app), [Row::NewGame], "no container: no Continue");
    assert_eq!(cursor(&app), 0);
}

/// Title → New Game: the character-creation screen (`app/character.rs`) comes first; the
/// opening map only loads once its name is confirmed.
#[test]
fn new_game_opens_character_creation_before_the_field() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.interactive = true;
    app.show_title();
    assert_eq!(rows(&app), [Row::NewGame]);
    assert!(!app.title_key(KeyCode::Enter), "Enter does not close");
    assert!(
        app.title.is_none(),
        "the title gave way to the creation screen"
    );
    assert!(app.creation.is_some(), "the creation screen is shown");
    assert!(!app.options.new_game, "the new game starts on confirm only");

    type_name(&mut app, "adenine");
    assert!(!app.creation_key(KeyCode::Enter), "Enter does not close");
    assert!(app.creation.is_none(), "the creation screen gave way");
    assert!(app.options.new_game, "the new-game path ran");
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(6405),
        "the new game loads the opening map (S14M05)"
    );
    assert_eq!(app.talk.hero_name(), b"adenine");
}

/// Types `text` as the creation screen's keys (lower-case letters only).
fn type_name(app: &mut App, text: &str) {
    for byte in text.bytes() {
        let code = match byte {
            b'a' => KeyCode::KeyA,
            b'b' => KeyCode::KeyB,
            b'c' => KeyCode::KeyC,
            b'd' => KeyCode::KeyD,
            b'e' => KeyCode::KeyE,
            b'f' => KeyCode::KeyF,
            b'g' => KeyCode::KeyG,
            b'h' => KeyCode::KeyH,
            b'i' => KeyCode::KeyI,
            b'j' => KeyCode::KeyJ,
            b'k' => KeyCode::KeyK,
            b'l' => KeyCode::KeyL,
            b'm' => KeyCode::KeyM,
            b'n' => KeyCode::KeyN,
            b'o' => KeyCode::KeyO,
            b'p' => KeyCode::KeyP,
            b'q' => KeyCode::KeyQ,
            b'r' => KeyCode::KeyR,
            b's' => KeyCode::KeyS,
            b't' => KeyCode::KeyT,
            b'u' => KeyCode::KeyU,
            b'v' => KeyCode::KeyV,
            b'w' => KeyCode::KeyW,
            b'x' => KeyCode::KeyX,
            b'y' => KeyCode::KeyY,
            b'z' => KeyCode::KeyZ,
            _ => panic!("test name has an unmapped character: {byte}"),
        };
        app.creation_key(code);
    }
}

/// Title → Continue: the slot `Save::title_slot` ranks (the suspend save), the warp's map,
/// party and look through the existing start, and the write the original's resume leaves in
/// slot 1 — the "Creating backup." mirror of the adventure log, since this container holds one
/// (`build/evidence/v3-suspend-ds/after-normal-suspend.sav`: slot 1 becomes slot 0's image
/// from `0x10` on, 21 bytes changed).
#[test]
fn continue_resumes_the_ranked_suspend_slot_and_writes_the_backup() {
    let (Some(rom), Some(bytes)) = (testrom::load(), saved_container()) else {
        eprintln!("skipping: DQ9_ROM or DQ9_SAVE missing");
        return;
    };
    let container = with_a_suspend_save(&bytes);
    let save = Save::parse(&container).unwrap();
    assert_eq!(
        save.title_slot(),
        Some(SUSPEND_SLOT),
        "the title ranks the suspend save first"
    );
    assert_eq!(
        save.resume_slot(),
        Some(0),
        "the boot order keeps the normal log first"
    );
    let expected = dq9_core::state::GameState::from_slot(&save.slot(SUSPEND_SLOT).unwrap())
        .expect("the suspend slot restores");

    let dir = std::env::temp_dir().join(format!("dq9-title-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.sav");
    std::fs::write(&path, &container).unwrap();

    let mut app = app(rom, Some(container.clone()));
    app.interactive = true;
    app.set_save_files(Some(path.clone()), None);
    app.show_title();
    assert_eq!(rows(&app), [Row::Continue, Row::NewGame]);
    assert_eq!(cursor(&app), 0, "Continue first, as the ranking puts it");
    assert!(!app.title_key(KeyCode::Enter), "Enter chooses the row");

    assert!(app.title.is_none(), "the title gave way to the field");
    assert!(
        !app.options.new_game,
        "Continue resumes, it does not start over"
    );
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(expected.map),
        "the map the suspend slot names"
    );
    assert_eq!(
        app.party.leader().map(|member| member.hp),
        dq9_core::party::Party::from_slot(&save.slot(SUSPEND_SLOT).unwrap())
            .ok()
            .and_then(|party| party.leader().map(|member| member.hp)),
        "the party the slot holds"
    );

    // `Save_WriteStep` state 2 copies slot 0's `0x10..0x6ff4` at slot 1's `0x10` (0x6fe4
    // bytes); the 16 bytes before and the 0x100c erased bytes after keep the input's bytes.
    let after = Save::parse(&std::fs::read(&path).unwrap()).unwrap();
    let log = save.slot(0).unwrap().bytes();
    let resumed = after.slot(SUSPEND_SLOT).unwrap();
    assert_eq!(resumed.status(), SlotStatus::Valid, "an adventure log now");
    assert_eq!(
        &resumed.bytes()[16..16 + 0x6fe4],
        &log[16..16 + 0x6fe4],
        "slot 1 is slot 0's image from 0x10 on"
    );
    assert_eq!(
        &resumed.bytes()[..16],
        &container[0x8000..0x8010],
        "slot 1's first 16 bytes stay"
    );
    assert_eq!(
        &resumed.bytes()[16 + 0x6fe4..],
        &container[0x8000 + 16 + 0x6fe4..],
        "and its tail"
    );
    assert!(resumed.checksums_valid(), "slot 0's checksums, which hold");
    assert_eq!(
        after.slot(0).unwrap().bytes(),
        save.slot(0).unwrap().bytes(),
        "the normal log is untouched"
    );
    // `DQ9_EVIDENCE_DIR` keeps the two cartridges for the batch's evidence: `build/evidence/
    // v3-suspend-backup/<name>-normal-suspend-port.sav` (the same convention as the encounter
    // captures).
    if let Some(dir) = std::env::var_os("DQ9_EVIDENCE_DIR").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("before-normal-suspend-port.sav"), &container).unwrap();
        std::fs::write(dir.join("after-normal-suspend-port.sav"), after.as_bytes()).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other container kind: slot 0 holds no adventure log, so the resume leaves
/// `Save_WriteHeaderA`'s empty header A on slot 1 and keeps its blocks
/// (`build/evidence/v3-suspend-ds/after-suspend-only.sav`).
#[test]
fn continue_with_no_adventure_log_clears_the_suspend_header() {
    let (Some(rom), Some(bytes)) = (testrom::load(), saved_container()) else {
        eprintln!("skipping: DQ9_ROM or DQ9_SAVE missing");
        return;
    };
    let mut save = Save::parse(&with_a_suspend_save(&bytes)).unwrap();
    {
        let mut slot = save.slot_mut(0).unwrap();
        slot.set_exists(false);
        slot.seal();
    }
    let container = save.as_bytes().to_vec();
    let save = Save::parse(&container).unwrap();
    assert_eq!(save.title_slot(), Some(SUSPEND_SLOT));
    assert_eq!(save.resume_slot(), Some(SUSPEND_SLOT), "no normal log");

    let dir = std::env::temp_dir().join(format!("dq9-title-only-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.sav");
    std::fs::write(&path, &container).unwrap();
    let mut app = app(rom, Some(container.clone()));
    app.interactive = true;
    app.set_save_files(Some(path.clone()), None);
    app.show_title();
    assert!(!app.title_key(KeyCode::Enter), "Enter chooses Continue");

    let after = Save::parse(&std::fs::read(&path).unwrap()).unwrap();
    let resumed = after.slot(SUSPEND_SLOT).unwrap();
    assert_eq!(resumed.status(), SlotStatus::Empty);
    assert_eq!(
        &resumed.bytes()[36..],
        &save.slot(SUSPEND_SLOT).unwrap().bytes()[36..],
        "header A is emptied, the blocks keep their bytes"
    );
    assert_eq!(after.slot(0).unwrap().status(), SlotStatus::Empty);
    if let Some(dir) = std::env::var_os("DQ9_EVIDENCE_DIR").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("before-suspend-only-port.sav"), &container).unwrap();
        std::fs::write(dir.join("after-suspend-only-port.sav"), after.as_bytes()).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The church's 110 returns the interactive app to the title instead of ending the window; a
/// headless run keeps `session_over` and its steps.
#[test]
fn a_headless_end_session_marks_the_flag_instead_of_the_title() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.end_session();
    assert!(app.session_over, "headless keeps today's flag");
    assert!(app.title.is_none());

    app.interactive = true;
    app.end_session();
    assert!(!app.session_over, "the window must not leave");
    assert!(
        app.title.is_some(),
        "the title is back, as after a power-off"
    );
}

/// The field menu's Misc. → Quick Save writes the suspend slot and its 4702, once dismissed,
/// returns to the title as `ov_002::02169efc` does; the fresh container then offers Continue.
#[test]
fn a_quick_save_returns_to_the_title_after_its_4702() {
    use crate::app::services::misc_line;
    use crate::talk::Button;

    let (Some(rom), Some(bytes)) = (testrom::load(), saved_container()) else {
        eprintln!("skipping: DQ9_ROM or DQ9_SAVE missing");
        return;
    };
    let dir = std::env::temp_dir().join(format!("dq9-title-qs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.sav");
    std::fs::write(&path, &bytes).unwrap();

    let mut app = app(rom, Some(bytes));
    app.interactive = true;
    app.set_save_files(Some(path.clone()), None);
    assert!(app.open_field_menu(), "the field menu opens");
    for _ in 0..5 {
        app.service_button(Button::Down); // Items…Close, then Misc.
    }
    app.service_button(Button::Confirm);
    for _ in 0..misc_line::QUICK_SAVE_ROW {
        app.service_button(Button::Down);
    }
    app.service_button(Button::Confirm); // 4700: "Do you want to make a quick save log…?"
    app.service_button(Button::Confirm); // Yes: the write and 4702
    assert!(app.title.is_none(), "4702 is still on the screen");
    app.service_button(Button::Confirm); // dismiss 4702 → the title
    assert!(app.title.is_some(), "the title is back after 4702");
    assert_eq!(rows(&app), [Row::Continue, Row::NewGame]);

    let written = Save::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        written.slot(SUSPEND_SLOT).unwrap().status(),
        SlotStatus::Intermission,
        "the quick save log is in the container"
    );
    assert_eq!(written.title_slot(), Some(SUSPEND_SLOT));
    let _ = std::fs::remove_dir_all(&dir);
}
