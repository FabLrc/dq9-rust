use super::*;
use dq9_core::battle::BattleResult;
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
        battle: None,
        play: Default::default(),
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
        battle: None,
        play: Default::default(),
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

/// The battle set-up puts the party on `+z` and the monsters on `−z`, on the 9 by 9 grid
/// (`ov_000::02164d74`, `ov_000::0216f74c`), and the camera keeps its eye within 17 and under
/// 5 (`ov_000::0216f2b8`).
#[test]
fn a_three_slime_battle_places_the_fighters_on_the_grid() {
    use crate::battle::{Fighter, Formation, Shot, Side};

    let Some(rom) = testrom::load() else { return };
    let clock = DayClock::new(0.0, true);
    let request = crate::battle::Request {
        record: None,
        stage: crate::battle::DEFAULT_STAGE,
        monsters: vec![(1, 3)],
        track: None,
        surprise: None,
    };
    let resolved = request.resolve(rom).unwrap();
    let mut scene = BattleScene::build(
        rom,
        &resolved,
        clock.phase().index(),
        clock,
        &dq9_formats::cast::Story::default(),
        &HeroLook::new_game(),
    )
    .unwrap();
    assert_eq!(scene.stage(), 30116);
    assert_eq!(scene.formation(), Formation::Grid);

    // One hero: slot 58 (column 4, row 6) → (0, 4.5), facing π.
    let hero = *scene.fighter(Fighter::party(0)).unwrap();
    assert!(hero.position.x.abs() < 1e-3, "{:?}", hero.position);
    assert!((hero.position.z - 4.5).abs() < 1e-3, "{:?}", hero.position);
    assert!((hero.heading - std::f32::consts::PI).abs() < 1e-6);
    // Three slimes: slots 29, 22, 32, facing 0.
    let expected = [(-3.897, -2.25), (0.0, -4.5), (3.897, -2.25)];
    for (index, (x, z)) in expected.into_iter().enumerate() {
        let monster = *scene.fighter(Fighter::monster(index)).unwrap();
        assert!(
            (monster.position.x - x).abs() < 1e-3,
            "{:?}",
            monster.position
        );
        assert!(
            (monster.position.z - z).abs() < 1e-3,
            "{:?}",
            monster.position
        );
        assert_eq!(monster.heading, 0.0);
        assert_eq!(monster.number, Some(1));
    }
    assert_eq!(scene.fighters().len(), 4);

    // The eye never reaches further than 17 nor higher than 5; the wide opening frames from
    // 22°, so the clamp bites on the normal side shot (15°).
    let eye = scene.camera().eye();
    let offset = eye - scene.camera().target;
    let reach = glam::Vec2::new(offset.x, offset.z).length();
    assert!(reach <= 17.0 + 1e-3, "the eye reaches {reach}");
    assert!(eye.y <= 5.0 + 1e-3, "the eye is {:.3} high", eye.y);
    assert!(
        (scene.camera().fovy.to_degrees() - 44.0).abs() < 1e-3,
        "the opening is the wide shot"
    );
    // The normal side shot and the monsters' side turn the framing round.
    scene.set_shot(Shot::Side(Side::Monsters));
    let offset = scene.camera().eye() - scene.camera().target;
    let reach = glam::Vec2::new(offset.x, offset.z).length();
    assert!(
        (reach - 17.0).abs() < 1e-3,
        "the eye is clamped at 17: {reach}"
    );
    assert!(scene.camera().eye().z < 0.0);
    assert!((scene.camera().fovy.to_degrees() - 30.0).abs() < 1e-3);

    // The idle of a slime is `stand`; the defeat fade hides it and takes its screen position.
    assert!(scene.play(Fighter::monster(0), "stand"));
    let at = scene.fighter_screen_pos(Fighter::party(0)).unwrap();
    assert!(
        (0.0..=256.0).contains(&at.x) && (0.0..=192.0).contains(&at.y),
        "{at:?}"
    );
    assert!(scene.remove(Fighter::monster(1)));
    assert!(scene.fighter_screen_pos(Fighter::monster(1)).is_none());
}

/// `eventbattle.bin` index 2 is the Hexagoon alone (monster 300): its set battle loads the
/// stage the record names with the hero facing it.
#[test]
fn the_hexagoon_set_battle_stands_on_its_stage() {
    use crate::battle::{Fighter, Side};

    let Some(rom) = testrom::load() else { return };
    let request = crate::battle::Request {
        record: Some(2),
        ..Default::default()
    };
    let resolved = request.resolve(rom).unwrap();
    assert_eq!(resolved.number, Some(2));
    assert_eq!(resolved.monsters, vec![(300, 1)]);
    assert_ne!(resolved.stage, 0);
    let clock = DayClock::new(0.0, true);
    let mut scene = BattleScene::build(
        rom,
        &resolved,
        clock.phase().index(),
        clock,
        &dq9_formats::cast::Story::default(),
        &HeroLook::new_game(),
    )
    .unwrap();
    assert_eq!(scene.stage(), resolved.stage);
    assert_eq!(scene.entry().id, resolved.stage);
    assert_eq!(scene.fighters().len(), 2);
    let monster = *scene.fighter(Fighter::monster(0)).unwrap();
    assert_eq!(monster.number, Some(300));
    assert!(monster.position.x.abs() < 1e-3 && (monster.position.z + 4.5).abs() < 1e-3);
    let hero = *scene.fighter(Fighter::party(0)).unwrap();
    assert!((hero.position.z - 4.5).abs() < 1e-3);
    // The hexagoon's model archive has its idle.
    assert!(scene.play(Fighter::monster(0), "stand"));
    assert!(
        scene.camera().eye().z > 0.0,
        "the opening looks from the party's side"
    );
    let at = scene.fighter_screen_pos(Fighter::monster(0)).unwrap();
    assert!(
        (0.0..=256.0).contains(&at.x) && (0.0..=192.0).contains(&at.y),
        "{at:?}"
    );
    let _ = Side::Party;
}

/// Scripted battle 26 (`0x78(26)` after `21507`) shows its scene — stage 30116, monsters 290,
/// 292, 290 — and gives the field back at the same map and pose.
#[test]
fn scripted_battle_26_enters_and_leaves_the_scene_and_gives_the_field_back() {
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
    let mut before: Option<(u16, Vec3)> = None;
    let mut settled = false;
    for frame in 0..40_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        let previous = app
            .current
            .as_ref()
            .zip(app.hero)
            .map(|((entry, _), hero)| (entry.id, hero.position()));
        if previous.is_some() {
            before = previous;
        }
        app.run_steps(1);
        if app.journal.steps.iter().any(|(_, step)| {
            matches!(
                step,
                Step::Battle {
                    number: 26,
                    record: Some(_),
                    won: true,
                    fought: None
                }
            )
        }) {
            settled = true;
            break;
        }
    }
    assert!(settled, "battle 26 must be settled by the stand-in");
    // The headless run builds the scene and settles it at once (no fixed step is spent on
    // it), so the timeline is the story's; the window shows the transition itself.
    let shown = app.last_battle().expect("battle 26's scene was built");
    assert_eq!(shown.stage, 30116, "battle 26 is fought on the field stage");
    assert_eq!(shown.fighters, 4, "the hero and three monsters");
    let (map, pose) = before.expect("the field before the battle");
    let now = app.current.as_ref().map(|(entry, _)| entry.id).unwrap();
    assert_eq!(now, map, "the battle gives the same map back");
    let hero = app.hero.expect("the hero is back");
    assert!(
        (hero.position() - pose).length() < 1e-3,
        "{:?} vs {pose:?}",
        hero.position()
    );
    assert!(!app.battle_is_active());
    assert!(!app.events.is_active() || app.events.pending_start().is_some());
}

/// A headless run that plays its battles with the engine, the world's generators on `seed`.
fn fighting(mut options: Options, seed: u64) -> Options {
    options.play = BattlePlay {
        fight: true,
        seed: Some(seed),
    };
    options
}

/// The journal's battle step for scripted battle `number`.
fn battle_step(app: &App, number: u16) -> Option<&Step> {
    app.journal
        .steps
        .iter()
        .map(|(_, step)| step)
        .find(|step| matches!(step, Step::Battle { number: n, .. } if *n == number))
}

/// `--fight`: scripted battle 26 of a new game is played by the engine instead of declared
/// won. It pays the party (`Party::award`, the gold), keeps the opening's monsters and says
/// the game's lines, and the story chains on as after the stand-in.
#[test]
fn battle_26_is_fought_from_a_new_game_and_pays_the_party() {
    let Some(rom) = testrom::load() else { return };
    let mut options = fighting(map_options("1100", (1, 1, 1, true)), 7);
    options.map = None;
    options.new_game = true;
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    assert_eq!(app.party().gold, 0);
    for frame in 0..40_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        if battle_step(&app, 26).is_some() {
            break;
        }
    }
    let Some(Step::Battle {
        won,
        fought: Some(fought),
        ..
    }) = battle_step(&app, 26).cloned()
    else {
        panic!("battle 26 must be fought: {:?}", battle_step(&app, 26));
    };
    assert!(won, "seed 7 wins battle 26");
    assert_eq!(fought.result, "Won");
    assert!(fought.experience > 0 && fought.gold > 0, "{fought:?}");
    // The party shows the award: one member takes the whole experience.
    let hero = &app.party().members[0];
    assert_eq!(hero.current_experience(), fought.experience);
    assert_eq!(app.party().gold, fought.gold);
    assert!(hero.hp > 0 && hero.hp <= hero.max_hp);
    let last = app.last_fight().expect("the fight is summarised");
    assert_eq!(last.number, Some(26));
    assert_eq!(
        &last.log[..2],
        ["Some slimes draw near!", "A cruelcumber draws near!"]
    );
    assert!(
        last.log.iter().any(|line| line.contains("gold coins!")),
        "{:?}",
        last.log
    );
    // The same scenes follow the battle as after the stand-in.
    assert!(!app.battle_is_active());
    assert!(!app.events.is_active() || app.events.pending_start().is_some());
}

/// A level-1 hero loses the Hexagoon (battle 2, a set battle): trigger event 16 of its room
/// runs (its action sets track flag 4), the fallen hero is raised, the purse is halved and
/// the party comes round in Angel Falls' church (map 1106, `GameState+0x5698` after a new
/// game).
#[test]
fn a_lost_hexagoon_fires_trigger_16_and_the_party_wakes_in_the_church() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(
        fighting(map_options("7105", (2, 4, 1, true)), 3),
        Some(rom.to_vec()),
    );
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(7105));
    app.party.gold = 101;
    let flags_before = app.state.tracks[app.state.track].flags.clone();
    app.run_battle(2);
    let Some(Step::Battle {
        won,
        fought: Some(fought),
        ..
    }) = battle_step(&app, 2).cloned()
    else {
        panic!("battle 2 must be fought: {:?}", battle_step(&app, 2));
    };
    assert!(!won);
    assert_eq!(fought.result, "Lost");
    assert_eq!((fought.experience, fought.gold), (0, 0));
    let flags = &app.state.tracks[app.state.track].flags;
    assert_ne!(flags, &flags_before, "trigger 16 ran: {flags:?}");
    assert!(flags.iter().any(|set| set.contains(&4)), "{flags:?}");
    assert_eq!(app.party().gold, 50, "the purse is halved, rounding down");
    let hero = &app.party().members[0];
    assert_eq!(hero.hp, hero.max_hp, "the fallen hero gets up");
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(1106));
    assert!(
        app.journal.steps.iter().any(
            |(_, s)| matches!(s, Step::MapChange { to: 1106, cause, .. } if cause == "wipe-out")
        )
    );
}

/// `start_encounter`, the roaming monsters' entry: an encounter has no battle number, so no
/// trigger fires and no step is journaled; the party is paid and the field is back.
#[test]
fn an_encounter_pays_the_party_and_fires_no_trigger() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(
        fighting(map_options("1100", (1, 2, 1, true)), 5),
        Some(rom.to_vec()),
    );
    let map = app.current.as_ref().map(|(e, _)| e.id);
    app.start_encounter(vec![(1, 2)], 0, dq9_core::battle::Surprise::None);
    let last = app.last_fight().expect("the encounter was fought").clone();
    assert_eq!(last.result, BattleResult::Won);
    assert_eq!((last.experience, last.gold), (4, 8), "two slimes");
    assert_eq!(app.party().gold, 8);
    assert!(
        !app.journal
            .steps
            .iter()
            .any(|(_, s)| matches!(s, Step::Battle { .. })),
        "no battle number, nothing to report to the triggers"
    );
    assert_eq!(
        app.current.as_ref().map(|(e, _)| e.id),
        map,
        "the field is back"
    );
    assert!(!app.battle_is_active());
}

/// Without `--fight` the stand-in still settles a headless battle at once and leaves the party
/// as it was: the recorded traces replay as before.
#[test]
fn the_stand_in_leaves_the_party_untouched() {
    let Some(rom) = testrom::load() else { return };
    let mut options = map_options("1100", (1, 2, 1, true));
    options.map = None;
    options.new_game = true;
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    for frame in 0..40_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        if battle_step(&app, 26).is_some() {
            break;
        }
    }
    assert!(matches!(
        battle_step(&app, 26),
        Some(Step::Battle {
            won: true,
            fought: None,
            ..
        })
    ));
    assert_eq!(app.party().gold, 0);
    assert_eq!(app.party().members[0].current_experience(), 0);
    assert!(app.last_fight().is_none());
}

/// The window's path without a window: the transition builds the scene, the fight reads the
/// pad the keyboard fills (`confirm` held), the scene is left, the field is back and the party
/// is paid. `proxy` is only what makes a run fight by itself; the phases are the same.
#[test]
fn the_window_path_plays_a_battle_from_the_pad() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(
        fighting(map_options("1100", (1, 2, 1, true)), 9),
        Some(rom.to_vec()),
    );
    let map = app.current.as_ref().map(|(e, _)| e.id);
    let request = crate::battle::Request {
        stage: 0,
        monsters: vec![(1, 2)],
        surprise: Some(dq9_core::battle::Surprise::None),
        ..Default::default()
    };
    let resolved = request.resolve(rom).unwrap();
    app.battle_change = Some(BattleChange::Entering {
        ticks: BATTLE_ENTER_DONE - 1,
        request: resolved,
        auto_leave: false,
    });
    let mut seen_menu = false;
    let mut seen_message = false;
    for _ in 0..20_000 {
        if !app.battle_is_active() {
            break;
        }
        if let Some(fight) = &app.fight {
            let view = fight.view();
            seen_menu |= view.menu.is_some();
            seen_message |= view.message.is_some();
        }
        app.battle_pad.confirm = true;
        app.run_steps(1);
    }
    assert!(!app.battle_is_active(), "the battle must end");
    assert!(
        seen_menu && seen_message,
        "the fight showed its menu and windows"
    );
    let last = app.last_fight().expect("the fight is summarised");
    assert_eq!(last.result, BattleResult::Won);
    assert_eq!(app.party().gold, last.gold);
    assert!(app.fight.is_none() && app.battle().is_none());
    assert_eq!(
        app.current.as_ref().map(|(e, _)| e.id),
        map,
        "the field is back"
    );
}

/// A monster that joins the fight (a call for backup) is drawn on the next place of the grid,
/// the monsters take their places again for the new count, and a fallen one stays hidden.
#[test]
fn a_monster_that_joins_the_fight_is_placed_and_the_fallen_stay_hidden() {
    use crate::battle::Fighter;

    let Some(rom) = testrom::load() else { return };
    let clock = DayClock::new(0.0, true);
    let resolved = crate::battle::Request {
        stage: crate::battle::DEFAULT_STAGE,
        monsters: vec![(1, 2)],
        ..Default::default()
    }
    .resolve(rom)
    .unwrap();
    let mut scene = BattleScene::build(
        rom,
        &resolved,
        clock.phase().index(),
        clock,
        &dq9_formats::cast::Story::default(),
        &HeroLook::new_game(),
    )
    .unwrap();
    assert!(scene.remove(Fighter::monster(0)));
    scene.add_monster(rom, 43).unwrap();
    assert_eq!(scene.fighters().len(), 4);
    let joined = *scene.fighter(Fighter::monster(2)).unwrap();
    assert_eq!(joined.number, Some(43));
    assert!(joined.alive && scene.fighter_screen_pos(Fighter::monster(2)).is_some());
    assert!(
        scene.fighter_screen_pos(Fighter::monster(0)).is_none(),
        "the fallen monster is still hidden"
    );
    // Three monsters stand on the three-monster slots (29, 22, 32: x −3.897, 0, 3.897).
    let xs: Vec<f32> = (0..3)
        .map(|i| scene.fighter(Fighter::monster(i)).unwrap().position.x)
        .collect();
    assert!(
        (xs[0] + 3.897).abs() < 1e-3 && xs[1].abs() < 1e-3 && (xs[2] - 3.897).abs() < 1e-3,
        "{xs:?}"
    );
}

/// Options for the roaming-monster tests: a field map, day, battles played by the engine.
fn roaming_options(map: &str) -> Options {
    let mut options = map_options(map, (1, 1, 1, true));
    options.play = BattlePlay {
        fight: true,
        seed: Some(7),
    };
    options
}

/// A party that wins the level-1 monsters of F01 whatever the company draw: the level tables
/// are skipped, the attributes are low enough that the monsters attack (a stronger party makes
/// them flee and pay nothing) and the HP survives every round.
fn a_winning_party(app: &mut App) {
    app.party.members[0].stats.strength = 20;
    app.party.members[0].stats.resilience = 20;
    app.party.members[0].max_hp = 9999;
    app.party.members[0].hp = 9999;
    app.party_ready = true;
}

/// Walks the pilot in one direction for `steps` single fixed steps (the field task runs on
/// every other one), so the roamers see a stepped party.
fn walk_steps(app: &mut App, direction: Vec3, steps: usize) {
    for _ in 0..steps {
        app.pilot_dir = Some(direction);
        app.run_steps(1);
    }
    app.pilot_dir = None;
}

/// Waits (walking) for the first roamer, in `steps` single fixed steps.
fn wait_for_roamer(
    app: &mut App,
    steps: usize,
) -> Option<(usize, dq9_core::encounter::roamer::Roamer)> {
    for _ in 0..steps {
        app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
        app.run_steps(1);
        if let Some(found) = app.roaming_roamers().into_iter().next() {
            return Some(found);
        }
    }
    None
}

/// Natural spawn on F01: the map has encfld zones and a spawn graph, its monsters get field
/// models at load, and a roamer appears with a drawn model after the party steps.
#[test]
fn f01_spawns_a_visible_roamer_after_the_party_steps() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    assert_eq!(app.roaming_map(), Some(20001));
    // F01's three zones roam six monsters; every one has its `<code>_f.mon` field model.
    assert_eq!(app.roaming_model_count(), 6);
    let (slot, roamer) = wait_for_roamer(&mut app, 2_000).expect("F01 must spawn a roamer");
    assert_ne!(roamer.number, 0);
    assert!(
        roamer.position.iter().any(|coordinate| *coordinate != 0),
        "a real spawn point, not the origin: {:?}",
        roamer.position
    );
    let world = app
        .roaming_slot_world(slot)
        .expect("the roamer has a field model");
    assert!(world.x_axis.length() > 0.0, "the model is drawn: {world:?}");
    let at = world.w_axis;
    assert!(
        (at.x - roamer.position[0] as f32 / 4096.0).abs() < 0.01
            && (at.z - roamer.position[2] as f32 / 4096.0).abs() < 0.01,
        "the model stands at the roamer's position: {at:?} against {:?}",
        roamer.position
    );
}

/// Contact -> the existing encounter -> a played battle -> the same field, the defeated
/// roamer gone, the party paid once.
#[test]
fn a_contact_fights_on_the_same_field_and_removes_its_roamer_once() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    // Let the roamer leave its appear state (touches skips state 0).
    let mut target = None;
    for _ in 0..2_000 {
        app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
        app.run_steps(1);
        target = app
            .roaming_roamers()
            .into_iter()
            .find(|(_, roamer)| roamer.state != dq9_core::encounter::roamer::state::APPEAR);
        if target.is_some() {
            break;
        }
    }
    let target_slot = target
        .map(|(slot, _)| slot)
        .expect("a roamer must be past its appear state");
    let map = app.current.as_ref().map(|(entry, _)| entry.id);
    let gold = app.party().gold;
    let mut fought = false;
    for _ in 0..8 {
        // Stand on it: the touch is `touches`, the very function the original tick uses.
        let Some(current) = app
            .roaming_roamers()
            .into_iter()
            .find(|(slot, _)| *slot == target_slot)
            .map(|(_, roamer)| roamer)
            .or_else(|| app.roaming_roamers().into_iter().next().map(|(_, r)| r))
        else {
            continue;
        };
        app.hero = Some(Hero::at(
            Vec3::new(
                current.position[0] as f32 / 4096.0 + 0.15,
                current.position[1] as f32 / 4096.0,
                current.position[2] as f32 / 4096.0,
            ),
            0.0,
        ));
        app.run_steps(1);
        if app.roaming_contacts() > 0 {
            fought = true;
            break;
        }
    }
    assert!(fought, "the hero must touch the roamer");
    assert_eq!(app.roaming_contacts(), 1);
    assert!(
        !app.battle_is_active(),
        "the encounter settled in the same step"
    );
    let last = app.last_fight().expect("the encounter was fought");
    assert_eq!(last.result, BattleResult::Won);
    assert!(
        last.gold > 0 && last.experience > 0,
        "the win pays: {last:?}"
    );
    assert_eq!(
        app.party().gold,
        last.gold,
        "the award went to the party once"
    );
    assert!(last.gold >= gold, "an encounter pays, it does not reset");
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        map,
        "the field is back, on the same map"
    );
    // The won monster is in its fade (state 11) when the battle settles; the field's next
    // steps finish it and release the model. The touch picks the first eligible slot, which
    // need not be the one the test stood on.
    let (contact_slot, contact_id) = {
        let (slot, roamer) = app
            .roaming_roamers()
            .into_iter()
            .find(|(_, roamer)| roamer.state == dq9_core::encounter::roamer::state::DEFEATED)
            .expect("the contacted roamer fades first");
        (slot, roamer.id)
    };
    let contact_instance = app
        .roaming_slot_instance(contact_slot)
        .expect("the contacted roamer has a field model");
    // The slot's spawn timer may already be near its second, so wait for the fade to end and
    // check the released model on the first step the old id is gone, before the slot can be
    // given another roamer.
    for _ in 0..40 {
        app.run_steps(1);
        if !app
            .roaming_roamers()
            .iter()
            .any(|(_, roamer)| roamer.id == contact_id)
        {
            break;
        }
    }
    assert!(
        !app.roaming_roamers()
            .iter()
            .any(|(_, roamer)| roamer.id == contact_id),
        "the defeated roamer left its slot after the fade"
    );
    let released = app
        .runtime
        .as_ref()
        .expect("the field is loaded")
        .scene
        .instances[contact_instance]
        .world;
    assert_eq!(
        released.x_axis.length(),
        0.0,
        "the released field model is hidden: {released:?}"
    );
    // Settled once: more steps do not fight it again or pay twice.
    let gold = app.party().gold;
    for _ in 0..120 {
        app.run_steps(1);
    }
    assert_eq!(app.roaming_contacts(), 1);
    assert_eq!(app.party().gold, gold, "no second award");
    assert!(
        !app.roaming_roamers()
            .iter()
            .any(|(_, roamer)| roamer.id == contact_id),
        "no respawn is the defeated roamer"
    );
}

/// Maps with no `encfld` group load no roamer and no field model: `S01M01`, `D01M01` and
/// `D01M05` are the maps the encounter note names.
#[test]
fn encounter_free_maps_spawn_no_roamers() {
    let Some(rom) = testrom::load() else { return };
    for (map, id) in [("S01M01", 5101), ("D01M01", 7101), ("D01M05", 7105)] {
        let mut app = App::new_headless(roaming_options(map), Some(rom.to_vec()));
        a_winning_party(&mut app);
        app.set_roaming_enabled(true);
        assert_eq!(app.roaming_map(), Some(id), "{map} has no encounter table");
        assert_eq!(app.roaming_model_count(), 0, "{map} has no field models");
        walk_steps(&mut app, Vec3::new(1.0, 0.0, 0.0), 80);
        assert!(
            app.roaming_roamers().is_empty(),
            "{map} must not spawn a roamer"
        );
        assert_eq!(app.roaming_contacts(), 0);
    }
}

/// A battle preview (`--battle`, `?battle=`) is fought once: closing its reward windows must
/// not start it again (the Web defect observed after a won battle 26).
#[test]
fn a_battle_preview_is_fought_once_and_not_restarted() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("1100"), Some(rom.to_vec()));
    let request = crate::battle::Request {
        record: None,
        stage: 0,
        monsters: vec![(1, 2)],
        track: None,
        surprise: Some(dq9_core::battle::Surprise::None),
    };
    app.options.battle = Some(request.clone());
    // The preview has no field behind it, as `set_rom` sets it up.
    app.save_field();
    let resolved = request.resolve(rom).unwrap();
    app.battle_change = Some(BattleChange::Entering {
        ticks: BATTLE_ENTER_DONE - 1,
        request: resolved,
        auto_leave: false,
    });
    let mut fought = false;
    for _ in 0..20_000 {
        if !app.battle_is_active() {
            fought = true;
            break;
        }
        app.battle_pad.confirm = true;
        app.run_steps(1);
    }
    assert!(fought, "the preview must end");
    let last = app.last_fight().expect("the preview was played").clone();
    assert!(!app.battle_is_active());
    assert!(app.battle().is_none() && app.fight.is_none());
    let gold = app.party().gold;
    for _ in 0..2_000 {
        app.run_steps(1);
    }
    assert_eq!(app.party().gold, gold, "the preview must not pay twice");
    assert!(
        !app.battle_is_active(),
        "the preview must not start again: {:?}",
        app.last_fight().map(|fight| fight.gold)
    );
    assert_eq!(app.last_fight().map(|fight| fight.gold), Some(last.gold));
}

/// Esc cancels in a fight (the pad owns it); the field's Esc is not touched.
#[test]
fn escape_is_cancel_in_a_fight() {
    let mut pad = Pad::default();
    super::fight::pad_press(&mut pad, winit::keyboard::KeyCode::Escape);
    assert!(pad.cancel && !pad.confirm);
}

/// A Hexagon floor with `encfld`/spawn graph loads the field models of its zones (`D01M02` to
/// `D01M04`); the map with no group loads none.
#[test]
fn hexagon_encounter_floors_load_their_field_models() {
    let Some(rom) = testrom::load() else { return };
    for (map, id) in [("D01M02", 7102), ("D01M03", 7103), ("D01M04", 7104)] {
        let mut app = App::new_headless(roaming_options(map), Some(rom.to_vec()));
        a_winning_party(&mut app);
        app.set_roaming_enabled(true);
        assert_eq!(app.roaming_map(), Some(id));
        assert!(
            app.roaming_model_count() > 0,
            "{map} must load its roamer field models"
        );
        let mut spawned = false;
        for _ in 0..2_000 {
            app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
            app.run_steps(1);
            if !app.roaming_roamers().is_empty() {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "{map} must spawn a roamer");
        let (slot, roamer) = app.roaming_roamers().into_iter().next().unwrap();
        assert!(
            app.roaming_slot_world(slot)
                .is_some_and(|world| world.x_axis.length() > 0.0),
            "{map}: the {:?} model is drawn",
            roamer.number
        );
    }
}

/// The leader's attack plus defence comes from `Character::fighter`, so worn equipment changes
/// the number the notice decision reads (not the level table alone).
#[test]
fn equipment_is_counted_in_the_party_strength() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    app.set_roaming_enabled(true);
    app.party.members[0].stats.strength = 10;
    app.party.members[0].stats.resilience = 10;
    app.party.members[0].hp = 100;
    app.party.members[0].max_hp = 100;
    app.party.members[0].equipment = Default::default();
    app.party_ready = true;
    let unarmed = app.roaming_party_strength();
    app.party.members[0].equipment.weapon = Some(dq9_core::state::NEW_GAME_WEAPON);
    let armed = app.roaming_party_strength();
    assert!(
        armed.0 > unarmed.0,
        "the copper sword adds attack: {unarmed:?} -> {armed:?}"
    );
    // A dead member is neither the leader's sum nor a deftness source.
    app.party.members[0].hp = 0;
    assert_eq!(app.roaming_party_strength(), (0, 0));
}

/// The scripted companies the `encbtl` roamer words name are read from `eventbattle.bin`, not
/// dropped: every found preset equals its record's monsters and counts.
#[test]
fn scripted_companies_come_from_eventbattle() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    let presets = app.roaming_presets();
    assert!(!presets.is_empty(), "the encbtl tables name presets");
    let parsed = nds_rom::Rom::parse(rom).unwrap();
    let mut found = 0;
    for preset in presets {
        let Some(company) = app.roaming_event_company(preset) else {
            continue;
        };
        let record = dq9_core::scenario::battle(&parsed, u16::from(preset))
            .unwrap_or_else(|| panic!("preset {preset} has no eventbattle record"));
        let mut expected = [(0u16, 0i16); 3];
        for (slot, &(monster, count)) in record.monsters.iter().take(3).enumerate() {
            expected[slot] = (monster as u16, count as i16);
        }
        assert_eq!(company, expected, "preset {preset}");
        found += 1;
    }
    assert!(found > 0, "at least one encbtl preset names a battle");
}

/// The two-way generator transfer: a battle's draws replace the field's stream (a scripted
/// battle has no contact pending), and the next field step hands the same state back, so no
/// draw is replayed and the state is not a constant copy.
#[test]
fn battle_generators_continue_the_field_stream() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
    app.run_steps(2);
    let before = app.roaming_world().expect("roamers loaded");
    // What a battle does to the world's generators.
    let mut advanced = before.clone();
    advanced.crand.rand();
    advanced.random.next_u32();
    app.world = advanced.clone();
    // A scripted battle settles with no contacted roamer; the field must adopt the stream.
    app.settle_roaming(None);
    let adopted = app.roaming_world().expect("roamers loaded");
    assert_eq!(
        (adopted.crand.clone(), adopted.random.clone()),
        (advanced.crand.clone(), advanced.random.clone())
    );
    assert_ne!(
        (adopted.crand.clone(), adopted.random.clone()),
        (before.crand, before.random),
        "the transfer is a real advance, not a copy of the pre-battle state"
    );
    // The next field step exports the same stream again (field -> world).
    app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
    app.run_steps(2);
    let field = app.roaming_world().expect("roamers loaded");
    assert_eq!(
        (field.crand.clone(), field.random.clone()),
        (app.world.crand.clone(), app.world.random.clone()),
        "the world and the field share one live stream"
    );
}

/// A map change seeds the new map's roamers from the live stream (the world the last field
/// step exported), not from a fresh seed: no pre-change draw is replayed.
#[test]
fn a_map_change_continues_the_same_generator_stream() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
    app.run_steps(2);
    // A battle (or a script) advances the world; the field adopts it, then the map changes.
    app.world.crand.rand();
    app.world.random.next_u32();
    let advanced = app.world.clone();
    app.settle_roaming(None);
    assert!(app.load("D01M02", true, None, None), "D01M02 loads");
    let next = app.roaming_world().expect("the new map has roamers");
    assert_eq!(
        (next.crand.clone(), next.random.clone()),
        (advanced.crand, advanced.random),
        "the new map's roamers start on the live stream"
    );
}

/// A flight does not delete the live roamer: the settlement puts it in STARTLED, its model
/// stays drawn, and its id is recent so the party cannot touch it at once.
#[test]
fn a_flight_keeps_and_resumes_the_roamer() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    let (slot, found) = wait_for_roamer(&mut app, 2_000).expect("F01 spawns");
    let id = found.id;
    app.roaming.set_pending(slot);
    app.settle_roaming(Some(BattleResult::Fled));
    let roamer = app
        .roaming_roamers()
        .into_iter()
        .find(|(slot_now, _)| *slot_now == slot)
        .map(|(_, roamer)| roamer)
        .expect("a flight leaves the roamer alive");
    assert_eq!(roamer.id, id);
    assert_eq!(roamer.state, dq9_core::encounter::roamer::state::STARTLED);
    let instance = app
        .roaming_slot_instance(slot)
        .expect("the fled roamer keeps its model");
    for _ in 0..6 {
        app.run_steps(1);
    }
    assert_eq!(
        app.roaming_slot_instance(slot),
        Some(instance),
        "the model is still bound to the fled roamer"
    );
    assert!(
        app.runtime
            .as_ref()
            .expect("the field is loaded")
            .scene
            .instances[instance]
            .world
            .x_axis
            .length()
            > 0.0,
        "the fled roamer stays drawn"
    );
    assert!(app.roaming_roamers().iter().any(|(_, r)| r.id == id));
}
