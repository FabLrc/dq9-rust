//! Map start-up: a save picked after a new game, the opening flags, doors across a reload and
//! the house exits' time of day (real-ROM regressions; they skip without a ROM).

use crate::app::App;
use crate::app::Options;
use crate::app::test_support::{map_loads, map_options, play_pending_event};
use crate::lighting;
use crate::screens::Layout;
use crate::talk::Button;
use dq9_core::Hero;
use dq9_core::field::Cause;
use dq9_core::field::Event;
use dq9_core::field::MapChange;
use dq9_core::state::GameState;
use dq9_core::state::TimeOfDay;
use dq9_core::trigger::Raw;
use dq9_formats::save::Save;

/// The web bug: after "New game", picking a save must load that save, not start
/// the prologue again. Exercises the same `set_save` path with a real save and ROM,
/// through the proxy-less headless constructor.
#[test]
fn a_save_selected_after_a_new_game_loads_that_save() {
    let (Some(rom), Some(save_path)) = (testrom::load(), std::env::var_os("DQ9_SAVE")) else {
        eprintln!("skipping: DQ9_ROM or DQ9_SAVE missing");
        return;
    };
    let save = std::fs::read(save_path).unwrap();
    let expected = GameState::from_save(&Save::parse(&save).unwrap()).unwrap();
    let options = Options {
        map: None,
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: 1,
            step: 1,
            substep: 1,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: true,
        battle: None,
        play: Default::default(),
    };
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    assert!(app.options.new_game);
    assert_eq!(app.events.pending_start(), Some(21500));
    app.set_save(save.clone());
    assert!(!app.options.new_game, "the save selects save mode");
    assert_eq!(app.options.map, None);
    let map = app.current.as_ref().map(|(entry, _)| entry.id);
    assert_eq!(map, Some(expected.map), "the save's map is loaded");
    assert!(!app.events.is_active());
    assert_eq!(app.events.pending_start(), None);
    assert!(!app.talk.is_active());

    // Replacing an already-running prologue must also discard its active caption,
    // not merely select the save's map while the old task keeps running.
    app.options.new_game = true;
    app.start_new_game();
    for _ in 0..480 {
        app.run_steps(1);
        if app.talk.is_active() {
            break;
        }
    }
    assert!(app.events.is_active(), "the prologue must have started");
    assert!(app.talk.is_active(), "the prologue caption must be open");
    app.set_save(save);
    assert!(!app.events.is_active());
    assert_eq!(app.events.pending_start(), None);
    assert!(!app.talk.is_active());
    app.run_steps(120);
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(expected.map)
    );
    assert!(!app.events.is_active(), "the old prologue must not resume");
    assert!(!app.talk.is_active(), "the old caption must not reopen");
}

/// A new game raises the opening flags by their causes, without a save: the starting
/// equipment through the item table (`item_obtained_flag`), the `S14M05` map load, and
/// the first held button (`KeyManager::RefreshKeys`).
#[test]
fn a_new_game_raises_the_opening_flags() {
    let Some(rom) = testrom::load() else { return };
    let options = Options {
        map: None,
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: 1,
            step: 1,
            substep: 1,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: true,
        battle: None,
        play: Default::default(),
    };
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(6405),
        "the opening loads S14M05 before the prologue"
    );
    for flag in [3191, 3775, 3955, 4046, 2774] {
        assert!(
            app.state.flags.contains(&flag),
            "flag {flag} after a new game"
        );
    }
    app.talk.press(Button::Confirm);
    app.run_steps(1);
    assert!(
        app.state.flags.contains(&4414),
        "the first button raises 0x113e"
    );
}

#[test]
fn a_same_map_reload_reapplies_open_doors_to_the_collision() {
    let Some(rom) = testrom::load() else { return };
    let options = Options {
        map: Some("X01M07".into()),
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: 1,
            step: 1,
            substep: 1,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: false,
        battle: None,
        play: Default::default(),
    };
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(4107));
    assert!(app.events.open_door(0, 2, true));
    assert!(app.events.open_door(0, 3, true));
    for _ in 0..160 {
        app.run_steps(1);
    }
    let open: Vec<(i32, i32, i32)> = app
        .events
        .doors()
        .iter()
        .map(|door| (door.block, door.placement, door.angle_fx()))
        .collect();
    assert_eq!(open.len(), 2, "both library door leaves are open");
    for (block, placement, angle) in &open {
        assert_eq!(
            app.doors.door_angles.get(&(*block, *placement)),
            Some(angle),
            "the open door angle reached the collision"
        );
    }
    // A reload of the same map (`set_time`) rebuilds the collision with doors closed;
    // the open doors must be turned again before the walkable grid is probed.
    let before = app.collision.pieces().to_vec();
    let spawn = app.hero.map(|hero| (hero.position(), hero.yaw()));
    assert!(app.load_map("X01M07", false, spawn, None, false));
    assert_eq!(
        app.collision.pieces(),
        before.as_slice(),
        "the rebuilt collision must match the open-door one"
    );
    for (block, placement, angle) in open {
        assert_eq!(
            app.doors.door_angles.get(&(block, placement)),
            Some(&angle),
            "the reload must re-apply door {block}/{placement}"
        );
    }
}

#[test]
fn event_21595_sets_quest490_and_takes_the_1_4_1_warp() {
    let Some(rom) = testrom::load() else { return };
    let options = Options {
        map: Some("M01".into()),
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: Some(21595),
        story: dq9_formats::cast::Story {
            chapter: 1,
            step: 3,
            substep: 1,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: false,
        battle: None,
        play: Default::default(),
    };
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    for frame in 0..120_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        if app.current.as_ref().map(|(entry, _)| entry.id) == Some(4105) {
            break;
        }
    }
    assert_eq!(
        app.state.raw.get(&Raw::Quest490),
        Some(&1),
        "native 559 must set quest manager +0x490 to 1"
    );
    let track = &app.state.tracks[0];
    assert_eq!(
        (track.chapter, track.step, track.substep),
        (1, 4, 1),
        "the 0x1f(1) branch takes story 1.4.1"
    );
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(4105),
        "the 1.4.1 warp goes to X01M05"
    );
    assert_ne!(app.events.pending_start(), Some(21593));
    assert!(
        !app.events.started().contains(&21593),
        "the fallback event 21593 must not start: {:?}",
        app.events.started()
    );

    // `0x85(4105, 21597)`: the arrival event the trigger encodes (Save_Warp+0x20) starts
    // once the destination map is loaded, exactly once, and its end raises game-1 flag 0.
    assert_eq!(
        app.events.pending_start(),
        Some(21597),
        "the 0x85 entry event must be queued after the map change"
    );
    assert_eq!(
        map_loads(&app, 4105),
        1,
        "the entry event must not reload 4105"
    );
    play_pending_event(&mut app, 21597);
    assert_eq!(
        app.events
            .started()
            .iter()
            .filter(|&&event| event == 21597)
            .count(),
        1,
        "the entry event is queued once: {:?}",
        app.events.started()
    );
    assert!(
        app.state.tracks[0].flags[1].contains(&0),
        "the end of 21597 raises game-1 flag 0 (room 4105, 1.4 event 11)"
    );
    assert_eq!(map_loads(&app, 4105), 1, "21597 plays on the loaded map");
    assert!(!app.events.started().contains(&21593));
}

/// Stands the hero inside the `0x72` exit of the loaded house, with the two game-1 flags
/// the exit rule waits for, and takes one fixed step.
fn leave_house(app: &mut App) {
    app.state.tracks[0].flags[1].insert(4);
    app.state.tracks[0].flags[1].insert(5);
    let exit = app
        .field
        .as_ref()
        .expect("the house is loaded")
        .exits()
        .into_iter()
        .find(|exit| exit.warp.is_some())
        .expect("M01M04 has its house exit");
    app.hero = Some(Hero::at(exit.position, 0.0));
    app.run_steps(1);
}

/// `room 1104 ... event 6` (`0xb1(1, 0)`) sets the night before the house-exit warp: the
/// application clock follows before the map change, 1100 loads at night, and 1104 is not
/// reloaded (a reload would replay its entry events).
#[test]
fn house_exit_time_of_day_reaches_the_application_before_the_map_change() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1104", (1, 2, 1, true)), Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1104));
    // A daytime start with no --time/--clock: the field's own 0xb1 is what must move the
    // clock, so the application's daytime state is set explicitly here.
    app.day = lighting::day_clock(None, Some(TimeOfDay::Day.index()), None);
    app.time = app.day.phase().index();
    assert_eq!((app.time, app.state.time_of_day), (2, 2));
    assert_eq!(map_loads(&app, 1104), 1);
    leave_house(&mut app);
    assert_eq!(app.state.time_of_day, 0, "0xb1(1, 0) sets the night");
    assert_eq!(
        app.time,
        TimeOfDay::Night.index(),
        "the application clock follows the field"
    );
    assert_eq!(app.day.phase(), TimeOfDay::Night);
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        Some(1100),
        "the exit warp loads the village"
    );
    assert_eq!(
        map_loads(&app, 1104),
        1,
        "the change must not reload the house"
    );
}

/// `--time`/`--clock` keep the priority: a trigger's `0xb1` changes the trigger state but
/// not the lighting an oracle capture pinned.
#[test]
fn a_pinned_time_of_day_wins_over_the_trigger() {
    let Some(rom) = testrom::load() else { return };
    let mut options = map_options("1104", (1, 2, 1, true));
    options.time = Some(TimeOfDay::Day.index());
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    assert_eq!(
        (app.time, app.day.phase()),
        (TimeOfDay::Day.index(), TimeOfDay::Day)
    );
    leave_house(&mut app);
    assert_eq!(app.state.time_of_day, 0, "the trigger still sets the night");
    assert_eq!(
        app.time,
        TimeOfDay::Day.index(),
        "--time keeps the lighting it pinned"
    );
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
}

/// A `MapChange` that carries an entry number but is not a trigger warp starts nothing.
fn assert_entry_ignored(rom: &[u8], cause: Cause) {
    let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    let change = MapChange {
        destination: 4105,
        position: None,
        angle: None,
        entry: Some(21597),
        cause,
    };
    assert!(app.apply_events(vec![Event::MapChange(change)]));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(4105));
    assert_eq!(
        app.events.pending_start(),
        None,
        "{cause:?} is not a trigger warp"
    );
    assert!(app.events.started().is_empty());
}

/// Only a trigger warp starts its `entry` event: a doorway or a `0x72` spawn entry that
/// carries the same number starts nothing.
#[test]
fn only_a_trigger_warp_starts_its_entry_event() {
    let Some(rom) = testrom::load() else { return };
    assert_entry_ignored(rom, Cause::Warp { index: 0, kind: 0 });
    assert_entry_ignored(
        rom,
        Cause::Doorway {
            class: 0,
            number: 0,
        },
    );
    let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    let change = MapChange {
        destination: 4105,
        position: None,
        angle: None,
        entry: Some(21597),
        cause: Cause::Trigger,
    };
    assert!(app.apply_events(vec![Event::MapChange(change)]));
    assert_eq!(
        app.events.pending_start(),
        Some(21597),
        "a trigger warp starts its entry event"
    );
}
