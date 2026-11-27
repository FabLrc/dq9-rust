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

/// Title → New Game: the current new-game path (the default hero, the opening map).
#[test]
fn new_game_leaves_the_title_through_the_new_game_path() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app(rom, None);
    app.interactive = true;
    app.show_title();
    assert_eq!(rows(&app), [Row::NewGame]);
    assert!(!app.title_key(KeyCode::Enter), "Enter does not close");
    assert!(app.title.is_none(), "the title gave way to the field");
    assert!(app.options.new_game, "the new-game path ran");
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(6405),
        "the new game loads the opening map (S14M05)"
    );
}

/// Title → Continue: the slot `Save::title_slot` ranks (the suspend save), the warp's map,
/// party and look through the existing start, and the suspend log deleted from the container
/// as the original's resume does.
#[test]
fn continue_resumes_the_ranked_suspend_slot_and_deletes_its_log() {
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

    let written = Save::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        written.slot(SUSPEND_SLOT).unwrap().status(),
        SlotStatus::Empty,
        "the resumed quick save log is deleted, as `Save_WriteHeaderA` does"
    );
    assert_eq!(
        written.slot(0).unwrap().status(),
        SlotStatus::Valid,
        "the normal log is untouched"
    );
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
