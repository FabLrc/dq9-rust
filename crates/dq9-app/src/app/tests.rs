use super::*;
use dq9_core::state::GameState;
use dq9_formats::save::Save;

/// Start-up options of a ROM-backed regression on an explicit map and story point, with
/// no save and no pinned time (`--time`/`--clock` absent: the field may set the night).
fn map_options(map: &str, story: (u8, u8, u8, bool)) -> Options {
    Options {
        map: Some(map.into()),
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: story.0,
            step: story.1,
            substep: story.2,
            daytime: story.3,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: false,
    }
}

/// Whether the cast instance of NPC `npc` is drawn on the loaded scene.
fn cast_drawn(app: &App, npc: u16) -> bool {
    let runtime = app.runtime.as_ref().expect("a map is loaded");
    runtime
        .npc_instance(npc)
        .is_some_and(|index| runtime.scene.instances[index].drawn())
}

/// Number of `Step::Map` entries for `map` in the journal.
fn map_loads(app: &App, map: u16) -> usize {
    app.journal
        .steps
        .iter()
        .filter(|(_, step)| matches!(step, Step::Map { id, .. } if *id == map))
        .count()
}

/// Plays the pending event until it ends, pressing the action key to read the messages.
fn play_pending_event(app: &mut App, event: u16) {
    let mut played = false;
    for frame in 0..60_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        played |= app.events.started().contains(&event);
        if played && !app.events.is_active() && app.events.pending_start().is_none() {
            return;
        }
    }
    panic!(
        "event {event} did not end: started {:?}, current {:?}",
        app.events.started(),
        app.events.current()
    );
}

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

/// Stands the test hero on the collision's floor at `(x, z)`, probing from `from_y`
/// (the map's start helper probes from 3.0 and cannot reach the tower's upper floors).
/// This uses the real collision height, not a hard-coded pose.
fn stand_on(app: &mut App, x: f32, z: f32, from_y: f32) {
    let y = app
        .collision
        .ground_height(x, z, from_y)
        .unwrap_or_else(|| panic!("no ground at ({x}, {z}) probing from {from_y}"));
    app.hero = Some(Hero::at(Vec3::new(x, y, z), 0.0));
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
            app.door_angles.get(&(*block, *placement)),
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
            app.door_angles.get(&(block, placement)),
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

/// Action 0x7c after 1390: the cast instance of NPC 210 disappears from the scene, and no
/// other NPC of the village is hidden.
#[test]
fn removing_npc_210_hides_its_cast_instance() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
    assert!(cast_drawn(&app, 210), "NPC 210 stands in the village");
    assert!(cast_drawn(&app, 95), "the dog stands in the village");
    let events = app
        .field
        .as_mut()
        .unwrap()
        .event_ended(1390, &mut app.state);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Request(Request::RemoveNpc(210)))),
        "event 11 of 1390 removes NPC 210: {events:?}"
    );
    app.apply_events(events);
    assert!(
        !cast_drawn(&app, 210),
        "the keepsake-ring NPC must leave the scene"
    );
    assert!(cast_drawn(&app, 95), "the other villagers stay");
    assert!(
        app.state.tracks[0].flags[1].contains(&3),
        "game-1 flag 3 is raised"
    );
}

/// The keepsake ring through the real talk trigger: at 1.2.1 the village's arrival scene
/// 21592 runs first (`room 1100 1.2..1.2 event 3`), then talking to NPC 210 fires its
/// event 0 (`if 0x6(210) 0x5(3) then 0x77(1390)`). The end of 1390 gives item 22158,
/// raises game-1 flag 3 and removes NPC 210; no other flag moves.
#[test]
fn keepsake_ring_task_gives_22158_raises_flag_3_and_removes_npc_210() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
    // A bare `--map` loads without the arrival triggers; the real arrival from 4105 is a
    // transition (`0xe1(1100)`), and only then does event 3 queue 21592 and raise game-1
    // flag 1. Enter once more with the transition, as `App::change_map` does.
    assert!(app.load_map("1100", true, None, None, true));
    // Event 3 of room 1100 queues the 1.2 arrival scene: it is the game's own arrival,
    // not a test flag, and it must finish before the village is talkable.
    assert_eq!(
        app.events.pending_start(),
        Some(21592),
        "the 1.2 arrival scene waits on the loaded village"
    );
    play_pending_event(&mut app, 21592);
    assert!(cast_drawn(&app, 210), "NPC 210 stands in the village");
    assert!(cast_drawn(&app, 95), "the dog stands in the village");
    assert!(!app.state.items.contains_key(&22158));
    let before = app.state.tracks[0].flags.clone();

    talk_to(&mut app, 210);
    assert_eq!(
        app.events.pending_start(),
        Some(1390),
        "NPC 210's event 0 queues the keepsake-ring scene"
    );
    assert!(
        !app.talk.is_active(),
        "the trigger replaces the conversation"
    );
    play_pending_event(&mut app, 1390);

    assert_eq!(
        app.state.items.get(&22158),
        Some(&1),
        "the end of 1390 gives the keepsake ring 22158"
    );
    assert!(
        app.state.tracks[0].flags[1].contains(&3),
        "the end of 1390 raises game-1 flag 3"
    );
    assert!(!cast_drawn(&app, 210), "NPC 210 leaves the scene");
    assert!(cast_drawn(&app, 95), "no other villager is removed");
    let added: Vec<i32> = app.state.tracks[0].flags[1]
        .difference(&before[1])
        .copied()
        .collect();
    assert_eq!(added, vec![3], "only game-1 flag 3 is raised");
    assert_eq!(
        app.state.tracks[0].flags[0], before[0],
        "the ring touches no game-0 flag"
    );
    assert_eq!(
        (
            app.state.tracks[0].chapter,
            app.state.tracks[0].step,
            app.state.tracks[0].substep,
        ),
        (1, 2, 1),
        "the ring does not advance the track"
    );
}

/// The widow's task through the real conversation: with game-1 flag 3 and the ring 22158
/// held, NPC 12's event 0 opens context 193 (`if 0x6(12) 0x4(3) 0x5(4)`). Answering Yes
/// closes it into event 1, which queues 1330, takes 22158, gives 22159 and raises flag 4.
/// The scene leaves the widow on the map.
#[test]
fn widow_task_takes_22158_gives_22159_and_raises_flag_4() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1106", (1, 2, 1, true)), Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1106));
    // Documented preconditions: the keepsake ring (its own test proves the trigger gives
    // it) and game-1 flag 3, raised with it.
    app.state.obtain(22158, 1);
    app.state.tracks[0].flags[1].insert(3);
    let before = app.state.tracks[0].flags.clone();
    assert!(cast_drawn(&app, 12), "the widow stands in the house");

    talk_to(&mut app, 12);
    assert!(
        app.talk.is_active(),
        "event 0 opens the widow's context 193"
    );
    finish_conversation(&mut app);
    assert_eq!(
        app.events.pending_start(),
        Some(1330),
        "answering Yes queues the widow scene"
    );
    play_pending_event(&mut app, 1330);

    assert_eq!(app.state.items.get(&22158), Some(&0), "the ring is taken");
    assert_eq!(
        app.state.items.get(&22159),
        Some(&1),
        "the benevolessence is given"
    );
    assert!(
        app.state.tracks[0].flags[1].contains(&4),
        "event 1 raises game-1 flag 4"
    );
    assert!(cast_drawn(&app, 12), "the widow stays in the house");
    let added: Vec<i32> = app.state.tracks[0].flags[1]
        .difference(&before[1])
        .copied()
        .collect();
    assert_eq!(added, vec![4], "only game-1 flag 4 is raised");
    assert_eq!(
        app.state.tracks[0].flags[0], before[0],
        "no game-0 flag moves"
    );
}

/// Without game-1 flag 3 the widow offers context 192, and event 1 requires context 193:
/// the conversation closes without queueing 1330 or touching the ring.
#[test]
fn widow_without_flag_3_starts_no_scene() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1106", (1, 2, 1, true)), Some(rom.to_vec()));
    app.state.obtain(22158, 1);
    let before = app.state.tracks[0].flags.clone();
    talk_to(&mut app, 12);
    assert!(
        app.talk.is_active(),
        "the widow still talks, with context 192"
    );
    finish_conversation(&mut app);
    assert_eq!(app.events.pending_start(), None, "no scene is queued");
    assert_eq!(
        app.state.items.get(&22158),
        Some(&1),
        "the ring stays in the bag"
    );
    assert!(!app.state.items.contains_key(&22159));
    assert_eq!(
        app.state.tracks[0].flags[1], before[1],
        "no game-1 flag moves"
    );
    assert_eq!(
        app.state.tracks[0].flags[0], before[0],
        "no game-0 flag moves"
    );
    assert!(cast_drawn(&app, 12), "the widow stays");
}

/// Each stable pickup (event 1 of NPCs 211 to 215) removes its own pile from the scene,
/// the fifth starts 1340, and the following script keeps every removed pile hidden.
#[test]
fn stable_pickups_hide_each_pile_and_the_scene_keeps_them_hidden() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1104", (1, 2, 1, true)), Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1104));
    for npc in 211..=215 {
        assert!(cast_drawn(&app, npc), "pile {npc} stands in the stable");
    }
    assert!(
        cast_drawn(&app, 15),
        "the stablemaster stands in the stable"
    );
    for npc in 211..=215 {
        let events = app
            .field
            .as_mut()
            .unwrap()
            .talk_ended(npc, 0, Some(0), &mut app.state);
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::Request(Request::RemoveNpc(removed)) if *removed == npc
            )),
            "talking to pile {npc} removes it: {events:?}"
        );
        app.apply_events(events);
        assert!(!cast_drawn(&app, npc), "pile {npc} must leave the scene");
        // The piles not picked up yet must stay. The piles removed before this one are
        // checked at their own turn and again after 1340.
        for other in npc + 1..=215 {
            assert!(
                cast_drawn(&app, other),
                "pile {other} must stay while {npc} is removed"
            );
        }
    }
    assert_eq!(
        app.events.pending_start(),
        Some(1340),
        "the fifth pickup starts the stable scene"
    );
    play_pending_event(&mut app, 1340);
    assert!(
        app.state.tracks[0].flags[1].contains(&5),
        "the end of 1340 raises game-1 flag 5"
    );
    for npc in 211..=215 {
        assert!(
            !cast_drawn(&app, npc),
            "pile {npc} must not come back after the scene"
        );
    }
    assert!(cast_drawn(&app, 15), "the stablemaster stays drawn");
}

/// Index of the first area whose entry, read by the field task on a copy, asks for
/// `wanted`. Probing copies proves which area is the map's added area 12 without
/// touching the application.
fn area_triggering(app: &App, wanted: u16) -> usize {
    let field = app.field.as_ref().expect("a map is loaded");
    for (index, point) in field.area_points().into_iter().enumerate() {
        let mut probe = field.clone();
        let mut state = app.state.clone();
        let events = probe.step(
            &Leader {
                position: point,
                yaw: 0.0,
                input: Vec3::ZERO,
            },
            &mut state,
        );
        if events
            .iter()
            .any(|event| matches!(event, Event::Request(Request::Event(id)) if *id == wanted))
        {
            return index;
        }
    }
    panic!("no area of the map triggers event {wanted}");
}

/// A point of the collision floor inside area `index`: the game has the leader walk in;
/// the test only needs the pose the field task reads, but the pose must be a real floor
/// of the map, not the volume's centre.
fn floor_inside_area(app: &App, index: usize) -> Vec3 {
    let field = app.field.as_ref().expect("a map is loaded");
    let centre = field.area_points()[index];
    const OFFSETS: [f32; 9] = [0.0, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1.0, -1.0];
    for probe in [3.0, 2.0, 1.0, 0.5, 0.0, -0.5] {
        for &dz in &OFFSETS {
            for &dx in &OFFSETS {
                let (x, z) = (centre.x + dx, centre.z + dz);
                let Some(y) = app.collision.ground_height(x, z, probe) else {
                    continue;
                };
                let at = Vec3::new(x, y, z);
                if field.area_contains(index, at) {
                    return at;
                }
            }
        }
    }
    panic!("no collision floor inside area {index} around {centre:?}");
}

/// A standing point the game's partner search picks for `npc` (the autopilot's source,
/// `Talk::partners`), with the yaw that faces it.
fn talk_pose(app: &App, npc: u16) -> (Vec3, f32) {
    let partner = app
        .talk
        .partners()
        .into_iter()
        .find(|partner| partner.npc == npc)
        .unwrap_or_else(|| panic!("NPC {npc} has no talk spot on the loaded map"));
    (partner.position, partner.yaw)
}

/// Stands at an actual talk spot and presses the action key, so the trigger event 0 runs
/// through `Talk::begin` as in the window.
fn talk_to(app: &mut App, npc: u16) {
    let (position, yaw) = talk_pose(app, npc);
    app.hero = Some(Hero::at(position, yaw));
    app.talk.press(Button::Confirm);
    app.run_steps(1);
}

/// Presses the action key until the open conversation closes, as a player reading the
/// pages would. The first choice is the cursor of every prompt, so a `<YESNO>` gets Yes.
fn finish_conversation(app: &mut App) {
    for frame in 0..60_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        if !app.talk.is_active() {
            return;
        }
    }
    panic!("the conversation did not close");
}

/// `room 1104 1.2..1.2 event 6` / `room 1106 ... event 6`: the two task flags are the
/// story lock. With both, one step in the house's `0x72` exit fires event 6, which sets
/// the night (`0xb1(1,0)`), raises the house's game-0 flag (`0x66(0|1)`), advances the
/// track to 1.3.1 (`0x84(0,1,3,1)`) and selects 1350 or 1360 on arrival. The application
/// clock follows the night **before** 1100 is loaded (`App::apply_events` starts with
/// `sync_day_phase`). The `Field::step` call is the one `App::run_steps` makes at the
/// field's 30 Hz cadence; calling it directly lets the test read the `MapChange` and the
/// trigger state before `apply_events` loads 1100.
#[test]
fn house_exit_requires_both_task_flags_and_advances_to_1_3() {
    let Some(rom) = testrom::load() else { return };
    for (map, house, arrival, game_flag) in
        [("1104", 1104u16, 1360u16, 1i32), ("1106", 1106, 1350, 0)]
    {
        let mut app = App::new_headless(map_options(map, (1, 2, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(house));
        // A bare `--map` starts the day clock at night; the story is daytime, so pin the
        // application clock as `house_exit_time_of_day...` does. What the trigger must
        // change is the clock itself, not a time pinned by `--time`.
        app.day = lighting::day_clock(None, Some(TimeOfDay::Day.index()), None);
        app.time = app.day.phase().index();
        assert_eq!((app.time, app.state.time_of_day), (2, 2));
        app.state.tracks[0].flags[1].extend([3, 4, 5]);

        let exit = app
            .field
            .as_ref()
            .expect("the house is loaded")
            .exits()
            .into_iter()
            .find(|exit| exit.warp.is_some() && exit.destination == Some(1100))
            .expect("the house has its 0x72 exit to the village");
        let events = app.field.as_mut().unwrap().step(
            &Leader {
                position: exit.position,
                yaw: 0.0,
                input: Vec3::ZERO,
            },
            &mut app.state,
        );
        let change = events
            .iter()
            .find_map(|event| match event {
                Event::MapChange(change) => Some(*change),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{map}: the house exit must change map: {events:?}"));
        assert_eq!(change.destination, 1100, "{map}");
        assert!(
            matches!(change.cause, Cause::Warp { .. }),
            "{map}: the exit is the house's own 0x72 warp, got {:?}",
            change.cause
        );
        assert_eq!(app.state.time_of_day, 0, "{map}: 0xb1(1, 0) sets the night");
        let track = &app.state.tracks[0];
        assert_eq!(
            (track.chapter, track.step, track.substep),
            (1, 3, 1),
            "{map}: 0x84 advances to 1.3.1"
        );
        assert!(
            track.flags[0].contains(&game_flag),
            "{map}: 0x66 raises game-0 flag {game_flag}: {:?}",
            track.flags[0]
        );
        assert!(
            track.flags[1].is_empty(),
            "{map}: 0x84 clears the step's game-1 flags: {:?}",
            track.flags[1]
        );
        // The application lighting flips before the map load: this is the first half of
        // `apply_events`, with 1100 still not loaded.
        assert_eq!(app.day.phase(), TimeOfDay::Day);
        app.sync_day_phase();
        assert_eq!(
            (app.time, app.day.phase()),
            (TimeOfDay::Night.index(), TimeOfDay::Night)
        );
        assert_eq!(
            map_loads(&app, 1100),
            0,
            "the lighting flipped before the load"
        );
        assert!(app.apply_events(events));
        assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
        assert_eq!(map_loads(&app, house), 1, "{map} must not be reloaded");
        assert_eq!(map_loads(&app, 1100), 1);
        assert_eq!((app.time, app.state.time_of_day), (0, 0));
        assert_eq!(app.day.phase(), TimeOfDay::Night);
        assert_eq!(
            app.events.pending_start(),
            Some(arrival),
            "{map}: event 3 of 1100 selects the 1.3 arrival scene"
        );
    }
}

/// Without game-1 flag 4 or 5, event 6 does not run: no `0xb1`, no `0x84`, no 1350/1360.
/// The `0x72` house exit is still the house's **own** warp, so the hero returns to 1100
/// anyway (`Field::step_warps` passes its destination as the fallback and `Field::collect`
/// uses it when event 6 has no warp request); the story lock is the track and the night,
/// not the map change. Game-1 flag 1 is set so the unrelated 1.2 arrival event 3 cannot
/// queue 21592 and muddy the check.
#[test]
fn house_exit_without_a_task_flag_returns_to_1100_without_advancing() {
    let Some(rom) = testrom::load() else { return };
    for map in ["1104", "1106"] {
        for missing in [4i32, 5] {
            let mut app = App::new_headless(map_options(map, (1, 2, 1, true)), Some(rom.to_vec()));
            app.day = lighting::day_clock(None, Some(TimeOfDay::Day.index()), None);
            app.time = app.day.phase().index();
            {
                let flags = &mut app.state.tracks[0].flags[1];
                flags.insert(1);
                for flag in [3, 4, 5] {
                    if flag != missing {
                        flags.insert(flag);
                    }
                }
            }
            let exit = app
                .field
                .as_ref()
                .expect("the house is loaded")
                .exits()
                .into_iter()
                .find(|exit| exit.warp.is_some() && exit.destination == Some(1100))
                .expect("the house has its 0x72 exit to the village");
            let events = app.field.as_mut().unwrap().step(
                &Leader {
                    position: exit.position,
                    yaw: 0.0,
                    input: Vec3::ZERO,
                },
                &mut app.state,
            );
            let change = events
                .iter()
                .find_map(|event| match event {
                    Event::MapChange(change) => Some(*change),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!("{map} without flag {missing}: the 0x72 warp must remain: {events:?}")
                });
            assert_eq!(change.destination, 1100, "{map} without flag {missing}");
            assert!(matches!(change.cause, Cause::Warp { .. }));
            assert_eq!(
                (
                    app.state.tracks[0].chapter,
                    app.state.tracks[0].step,
                    app.state.tracks[0].substep,
                ),
                (1, 2, 1),
                "{map} without flag {missing}: no 0x84"
            );
            assert_eq!(
                app.state.time_of_day,
                TimeOfDay::Day.index(),
                "{map} without flag {missing}: no 0xb1"
            );
            assert!(
                !app.state.tracks[0].flags[0].contains(&0)
                    && !app.state.tracks[0].flags[0].contains(&1),
                "{map} without flag {missing}: no 0x66"
            );
            assert!(app.apply_events(events));
            assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
            assert_eq!(map_loads(&app, 1100), 1);
            assert_eq!(app.time, TimeOfDay::Day.index());
            assert_eq!(app.day.phase(), TimeOfDay::Day);
            assert_eq!(app.state.time_of_day, TimeOfDay::Day.index());
            assert!(
                !matches!(app.events.pending_start(), Some(1350 | 1360)),
                "{map} without flag {missing}: no 1.3 arrival scene, pending {:?}",
                app.events.pending_start()
            );
        }
    }
}

/// `room 1100 1.3..1.3 event 2` (added area 12) starts 1410, then `event 0` of NPC 94
/// starts the wandering soul scene 21595. Its normal end calls native 559, whose 1
/// selects the `0x1f(1)` branch: track 1.4.1 and the `0x85(4105, 21597)` entry. The
/// destination loads once, 21597 starts once, 21593 never does, and the night remains.
#[test]
fn village_area_12_plays_1410_then_npc_94_takes_the_1_4_1_return() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1100", (1, 3, 1, false)), Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
    assert_eq!((app.time, app.state.time_of_day), (0, 0));
    assert_eq!(app.day.phase(), TimeOfDay::Night);

    // The load event added area 12 (`0x8f(12, 3,0,0, 0,-1,-4, 17)`); find it by the
    // trigger its entry fires, then stand on a real floor point inside it.
    let area = area_triggering(&app, 1410);
    let spot = floor_inside_area(&app, area);
    app.hero = Some(Hero::at(spot, 0.0));
    app.run_steps(1);
    assert!(
        app.events.pending_start() == Some(1410) || app.events.started().contains(&1410),
        "the area entry must queue 1410: pending {:?}, started {:?}",
        app.events.pending_start(),
        app.events.started()
    );
    play_pending_event(&mut app, 1410);
    assert!(
        app.state.tracks[0].flags[1].contains(&0),
        "the end of 1410 raises game-1 flag 0"
    );
    assert_eq!(app.state.time_of_day, 0);
    assert_eq!(app.day.phase(), TimeOfDay::Night);

    // NPC 94 (`n007g`) event 0: `if 0x6(94) 0x5(1) then 0x77(21595) 0x72(22159)`.
    talk_to(&mut app, 94);
    assert_eq!(
        app.events.pending_start(),
        Some(21595),
        "NPC 94's trigger queues 21595"
    );
    assert_eq!(app.state.time_of_day, 0);

    // `play_pending_event` keeps going while something is pending, so it also plays the
    // 21597 entry of the 1.4.1 map change and the count below proves it ran once.
    play_pending_event(&mut app, 21595);
    assert_eq!(
        app.state.raw.get(&Raw::Quest490),
        Some(&1),
        "native 559 sets quest manager +0x490 to 1"
    );
    let track = &app.state.tracks[0];
    assert_eq!(
        (track.chapter, track.step, track.substep),
        (1, 4, 1),
        "the 0x1f(1) branch takes story 1.4.1"
    );
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(4105));
    assert_eq!(map_loads(&app, 4105), 1, "4105 loads once");
    assert_eq!(map_loads(&app, 1100), 1, "1100 is not reloaded");
    let started = app.events.started();
    let count = |event: u16| started.iter().filter(|&&id| id == event).count();
    assert_eq!(count(1410), 1, "{started:?}");
    assert_eq!(count(21595), 1, "{started:?}");
    assert_eq!(count(21597), 1, "the 0x85 entry starts once: {started:?}");
    assert_eq!(count(21593), 0, "the fallback must not start: {started:?}");
    assert!(!app.events.is_active());
    assert_eq!(app.events.pending_start(), None);
    assert_eq!(
        (app.time, app.state.time_of_day),
        (0, 0),
        "the return stays at night"
    );
    assert_eq!(app.day.phase(), TimeOfDay::Night);
}

/// Action 0x7c is per map: the 1.3 arrival trigger (event 3) fired on the loaded map is
/// what `load_map` runs after a transition, minus the rebuild that clears the removal
/// list. Its presentation (1350 for game-0 flag 0, 1360 for flag 1) must not seed NPC
/// 210 again, and 211–215 are not part of the 1100 cast. A map change rebuilds the cast
/// and forgets the removals, the documented exception.
#[test]
fn the_1_3_arrival_scene_does_not_revive_removed_npcs() {
    let Some(rom) = testrom::load() else { return };
    for (flag, scene) in [(0i32, 1350u16), (1, 1360)] {
        let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
        assert!(cast_drawn(&app, 210), "NPC 210 stands in the village");
        // Event 11 of 1390: the real end of the keepsake-ring scene.
        let events = app
            .field
            .as_mut()
            .unwrap()
            .event_ended(1390, &mut app.state);
        app.apply_events(events);
        assert!(!cast_drawn(&app, 210), "1390 removes NPC 210");
        assert!(app.state.tracks[0].flags[1].contains(&3));

        // 1.3 arrival: the house's game-0 flag selects 1350 or 1360.
        {
            let track = &mut app.state.tracks[0];
            (track.chapter, track.step, track.substep) = (1, 3, 1);
            track.flags[0].insert(flag);
        }
        let events = app.field.as_mut().unwrap().enter(&mut app.state, true);
        app.apply_events(events);
        assert_eq!(
            app.events.pending_start(),
            Some(scene),
            "event 3 selects the 1.3 arrival scene"
        );
        play_pending_event(&mut app, scene);
        assert!(
            !cast_drawn(&app, 210),
            "the arrival presentation must keep NPC 210 hidden"
        );
        for npc in 211..=215 {
            assert!(
                !cast_drawn(&app, npc),
                "NPC {npc} is not part of the 1100 cast"
            );
        }

        // Only a map change forgets the removals.
        assert!(app.load_map("1100", true, None, None, true));
        assert!(
            cast_drawn(&app, 210),
            "a map change rebuilds the cast; 0x7c removals are per map"
        );
    }
}

/// The end of 21510 on `X01M04` (`room 4104 1.1..1.1 event 11`: `0x84(0, 1, 2, 1)` then
/// `0x8d(1)`) rebuilds the map's cast at 1.2.1 without a map change (`Trig_RunDeferred`
/// queues task 0x30, `ov_017::021c16cc`, which calls `ov_017::021a2fa0`). Aquila (NPC
/// 99) is then drawn and talkable, and talking to him (`room 4104 1.2..1.3 event 0: if
/// 0x6(99) 0x3(4) then 0x76(99, …) 0x66(4)`) raises the track's game-0 flag 4. Before the
/// fix only the autopilot's own map reload showed him: a player had no NPC 99.
#[test]
fn the_end_of_21510_rebuilds_the_cast_and_npc_99_raises_game_0_flag_4() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("4104", (1, 1, 2, true)), Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(4104));
    assert!(!cast_drawn(&app, 99), "Aquila is not on the 1.1 cast");
    let loads = map_loads(&app, 4104);

    let events = app
        .field
        .as_mut()
        .unwrap()
        .event_ended(21510, &mut app.state);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Request(Request::ReloadCast))),
        "event 11 of 21510 asks for the cast rebuild (0x8d): {events:?}"
    );
    assert!(!Field::changes_map(&events), "21510 ends on the same map");
    app.apply_events(events);
    // The rebuild is a task of the queue: it runs on the next step of the free field.
    app.run_steps(1);
    let track = app.state.track;
    let story = &app.state.tracks[track];
    assert_eq!((story.chapter, story.step, story.substep), (1, 2, 1));
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(4104));
    assert_eq!(map_loads(&app, 4104), loads, "the map is not loaded again");
    assert!(cast_drawn(&app, 99), "the 1.2.1 cast draws Aquila");
    assert!(
        !app.state.tracks[track].flags[0].contains(&4),
        "game-0 flag 4 waits for the conversation"
    );

    talk_to(&mut app, 99);
    finish_conversation(&mut app);
    assert!(
        app.state.tracks[track].flags[0].contains(&4),
        "talking to Aquila raises game-0 flag 4"
    );
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(4104));
}

/// 21595 has flags 0x1 in its list record: its start records the NPC table
/// (`ov_001::02154f24` → `0x0203e0b0`) and `EventTask_Update` rebuilds the cast before
/// event 11 (`ov_017::021a2fa0` → `Cast_LoadMap` → `0x0203e2d4`). NPC 210, removed by the
/// real end of 1390 (action 0x7c) before the scene, stays removed through that rebuild,
/// and NPC 7, moved and turned before it, gets that pose back instead of the file's. The
/// scene is queued at 1.2.1, outside its 1.3 rules, so that event 11 neither warps nor
/// rebuilds again and the rebuild of the end is what the scene shows. A later 0x8d
/// rebuild starts from the placement script: the snapshot is consumed. Before the
/// snapshot the end of 21595 rebuilt nothing.
#[test]
fn the_end_of_21595_rebuilds_the_cast_with_the_poses_and_removals_kept_at_its_start() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    assert!(cast_drawn(&app, 95), "the dog stands in the village");
    let events = app
        .field
        .as_mut()
        .unwrap()
        .event_ended(1390, &mut app.state);
    app.apply_events(events);
    assert!(!cast_drawn(&app, 210), "1390 removes NPC 210");
    let instance = |app: &App, npc: u16| app.runtime.as_ref().unwrap().npc_instance(npc);
    let world = |app: &App, npc: u16| {
        let runtime = app.runtime.as_ref().unwrap();
        runtime.scene.instances[runtime.npc_instance(npc).unwrap()].world
    };
    // NPC 7 (a model) somewhere else than the file puts it, as a walk would leave it.
    let file = world(&app, 7);
    let (scale, _, at) = file.to_scale_rotation_translation();
    let moved = at + Vec3::new(0.5, 0.0, -0.25);
    {
        let runtime = app.runtime.as_mut().unwrap();
        let index = runtime.npc_instance(7).unwrap();
        runtime.scene.instances[index].world = glam::Mat4::from_scale_rotation_translation(
            scale,
            glam::Quat::from_rotation_y(1.0),
            moved,
        );
    }
    let before = instance(&app, 95);

    app.events.queue(21595);
    play_pending_event(&mut app, 21595);
    assert_eq!(app.current.as_ref().map(|(entry, _)| entry.id), Some(1100));
    assert_ne!(
        instance(&app, 95),
        before,
        "the end of 21595 rebuilt the cast"
    );
    assert!(cast_drawn(&app, 95), "the dog is back");
    assert!(
        !cast_drawn(&app, 210),
        "the snapshot keeps NPC 210 removed through the rebuild"
    );
    assert!(
        app.talk.partners().iter().all(|p| p.npc != 210),
        "a kept removal is not talkable either"
    );
    let (_, rotation, kept) = world(&app, 7).to_scale_rotation_translation();
    assert!(
        kept.distance(moved) < 1.0 / 128.0,
        "NPC 7 stands where the scene found it: {kept} for {moved}"
    );
    let (yaw, _, _) = rotation.to_euler(glam::EulerRot::YXZ);
    assert!((yaw - 1.0).abs() < 1e-3, "NPC 7 keeps its angle: {yaw}");
    assert!(!app.cast_reload, "event 11 has no 1.2 rule for 21595");

    // A later rebuild (action 0x8d) finds the snapshot consumed.
    app.apply_events(vec![Event::Request(Request::ReloadCast)]);
    app.run_steps(1);
    assert!(!app.cast_reload);
    assert!(
        cast_drawn(&app, 210),
        "a later rebuild starts from the placement script: NPC 210 stands again"
    );
    assert_eq!(world(&app, 7), file, "NPC 7 is back on the file's pose");
}

#[test]
fn trigger_1170_hides_the_tower_gate_and_the_hero_walks_to_x01m03() {
    let Some(rom) = testrom::load() else { return };
    let options = Options {
        map: Some("X01M01".into()),
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
    };
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(4101));

    // The ramp's low corridor, east of the gate (placement 16) and inside the S1 wall.
    stand_on(&mut app, 1.0, 17.0, 12.0);
    for _ in 0..80 {
        app.walk_step(Vec3::new(-1.0, 0.0, 0.0));
    }
    let blocked = app.hero.unwrap().position();
    assert!(
        blocked.x > 0.5,
        "the map's S1 barrier must block before the trigger: {blocked:?}"
    );

    // The real trigger, not a direct collision call: event 11 of room 4101, after 1170.
    let events = app
        .field
        .as_mut()
        .unwrap()
        .event_ended(1170, &mut app.state);
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::Request(Request::SetPlacementHidden {
                block: 0,
                placement: 16,
                hidden: true
            })
        )),
        "event 11 must hide placement (0, 16): {events:?}"
    );
    app.apply_events(events);
    assert_eq!(app.state.tracks[0].substep, 2, "the story reaches 1.1.2");

    // The gate is gone: the same west walk now crosses it.
    for _ in 0..40 {
        app.walk_step(Vec3::new(-1.0, 0.0, 0.0));
    }
    let through = app.hero.unwrap().position();
    assert!(
        through.x < 0.0,
        "the hidden placement must no longer block: {through:?}"
    );
    // From the 1.1.2 pose, the pilot walks with the d-pad; it must reach X01M03 by the
    // warp, with no placement other than this test's starting pose. The journal is the
    // proof even if it goes on to another map afterwards.
    stand_on(&mut app, 2.77, 18.19, 12.0);
    app.play_auto(120_000, true);
    let crossed = app.journal.steps.iter().any(|(_, step)| {
        matches!(step, Step::Map { id, .. } if *id == 4103)
            || matches!(step, Step::MapChange { to, .. } if *to == 4103)
    });
    assert!(
        crossed,
        "the pilot must walk from X01M01 to X01M03 through warp 9: {:?}",
        app.hero.map(|h| h.position())
    );
}

/// The original updates the field on one VBlank out of two: this walks a flat, straight
/// segment of `X01M05` from the capture's start pose and asserts the exact raw angle and
/// fx position at every VBlank. Expected values are the verified samples of
/// `build/v01-20261107/review/cadence-30hz-field-review.md` §7 (original
/// `original-prep.jsonl` and `Hero::step(ticks = 2)` model): even steps hold the previous
/// pose, odd steps turn by `0x324 × 2` and accelerate/move by two ticks.
#[test]
fn field_cadence_holds_the_pose_and_matches_the_original_every_vblank() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("X01M05", (1, 1, 1, true)), Some(rom.to_vec()));
    assert_eq!(app.tick, 0, "a fresh app starts on phase 0");
    assert!(
        !app.events.is_active() && app.events.pending_start().is_none(),
        "the flat segment must not be interrupted by an event"
    );
    let y = app
        .collision
        .ground_height(1.756, -3.041, 3.0)
        .expect("ground under the capture's start pose");
    // App yaw π is raw angle 0, facing +z at speed 0: the original's prepared state.
    app.hero = Some(Hero::at(Vec3::new(1.756, y, -3.041), std::f32::consts::PI));
    let hero = app.hero.expect("the map placed a hero");
    assert_eq!(hero.angle(), 0);
    assert_eq!(hero.speed(), 0);
    assert_eq!(hero.position_fx(), [7192, (y * 4096.0) as i32, -12455]);

    // `Hero::at` truncates -3.041 × 4096; the original's exact fx is not observable.
    const BASE_Z: i32 = -12455;
    const BASE_X: i32 = 7192;
    const ANGLES: [i32; 10] = [0, 1608, 3216, 4824, 6432, 8040, 9648, 11256, 12864, 12868];
    const Z_OFFSETS: [i32; 10] = [0, 0, -160, -454, -846, -1338, -1928, -2616, -3402, -4188];

    let mut held = app.hero.expect("hero").position_fx();
    let mut held_angle = 0;
    for step in 1..=19u64 {
        app.walk_step(Vec3::NEG_Z);
        assert_eq!(app.tick, step);
        let hero = app.hero.expect("the hero stays placed");
        let position = hero.position_fx();
        if step % 2 == 0 {
            // The intermediate VBlank holds the pose byte for byte; the camera still
            // follows it (per VBlank, not per game update).
            assert_eq!(hero.angle(), held_angle, "angle held on step {step}");
            assert_eq!(position, held, "position held on step {step}");
            assert_eq!(
                app.camera.target.z,
                hero.position().z,
                "camera follows the held pose on step {step}"
            );
            continue;
        }
        let sample = (step as usize - 1) / 2;
        assert_eq!(hero.angle(), ANGLES[sample], "raw angle on step {step}");
        assert!(
            (position[2] - (BASE_Z + Z_OFFSETS[sample])).abs() <= 2,
            "z fx on step {step}: {} (expected {} ± 2)",
            position[2],
            BASE_Z + Z_OFFSETS[sample]
        );
        assert!(
            (position[0] - BASE_X).abs() <= 1,
            "x fx on step {step}: {} (expected {BASE_X} ± 1)",
            position[0]
        );
        held = position;
        held_angle = hero.angle();
    }
    assert_eq!(held_angle, 12868, "the turn clamps on the target");
    assert!(!app.events.is_active());
}
