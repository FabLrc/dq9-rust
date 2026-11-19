//! Recording and replaying a human session (real-ROM regressions).

use crate::app::App;
use crate::app::Options;
use crate::app::STEP;
use crate::app::fight::BattlePlay;
use crate::app::headless;
use crate::app::test_support::map_options;
use crate::record;
use crate::record::Input as Logged;
use crate::record::Recorder;
use crate::screens::Layout;
use dq9_core::scenario::Step;
use glam::Vec2;
use web_time::Instant;
use winit::keyboard::KeyCode;

/// The start options of a recorded new game, with the header that stands for them.
fn record_start(rom: &[u8], seed: u64) -> (Options, record::Header) {
    let header = nds_rom::Header::parse(rom).expect("a ROM header");
    let options = Options {
        play: BattlePlay {
            fight: true,
            seed: Some(seed),
        },
        new_game: true,
        ..map_options("M01", (1, 1, 1, true))
    };
    let header = record::Header {
        rom: format!("{}{}", header.game_code, header.maker_code),
        rom_version: header.version,
        rom_crc: header.header_crc,
        new_game: true,
        seed,
        map: None,
        save: None,
        time: None,
        clock: None,
        chapter: None,
        step: None,
        substep: None,
        night: false,
        stage: None,
        event: None,
        battles: "show".into(),
    };
    (options, header)
}

/// A scripted human: the keys pressed and released at a step (taps of the confirm key to read
/// the opening, a held walk, the camera's turn), a drag of the mouse and a focus loss.
fn human_script(steps: u64) -> Vec<(u64, Logged)> {
    let tap = |at, key| {
        [
            (at, Logged::Key { key, pressed: true }),
            (
                at + 4,
                Logged::Key {
                    key,
                    pressed: false,
                },
            ),
        ]
    };
    let mut script: Vec<(u64, Logged)> = (0..steps / 25)
        .flat_map(|n| tap(n * 25 + 3, KeyCode::KeyZ))
        .collect();
    script.extend(tap(500, KeyCode::KeyI));
    script.extend(tap(700, KeyCode::Tab));
    script.push((
        900,
        Logged::Key {
            key: KeyCode::ArrowUp,
            pressed: true,
        },
    ));
    script.push((
        1000,
        Logged::Key {
            key: KeyCode::KeyQ,
            pressed: true,
        },
    ));
    script.push((
        1100,
        Logged::Mouse {
            rotate: [0.021, -0.004],
            zoom: 0.0,
        },
    ));
    script.push((1300, Logged::FocusLost));
    script.push((
        1500,
        Logged::Key {
            key: KeyCode::ArrowLeft,
            pressed: true,
        },
    ));
    script.sort_by_key(|(at, _)| *at);
    script
}

/// Plays `script` on `app` the way the window does: its inputs are handled between frames,
/// each frame is `update` after the wall time of `frames[n]` fixed steps (a slow frame owes
/// several, a fast one none), until `steps` have run.
fn play_like_the_window(app: &mut App, script: &[(u64, Logged)], steps: u64, frames: &[u32]) {
    let mut next = 0;
    let mut frame = 0;
    while app.steps < steps {
        while let Some(&(at, input)) = script.get(next)
            && at <= app.steps
        {
            match input {
                Logged::Key { key, pressed } => {
                    app.key(key, pressed);
                }
                Logged::FocusLost => app.focus_lost(),
                Logged::Mouse { rotate, zoom } => app.mouse(Vec2::from(rotate), zoom),
                Logged::JingleEnded => {}
            }
            next += 1;
        }
        let due = frames[frame % frames.len()].min((steps - app.steps) as u32);
        frame += 1;
        app.accumulator = 0.0;
        app.last = Instant::now() - std::time::Duration::from_secs_f32(due as f32 * STEP + 0.004);
        app.update();
    }
}

#[test]
fn a_recorded_session_replays_to_the_same_digests_at_any_pacing() {
    let Some(rom) = testrom::load() else { return };
    let steps = 9_000;
    let script = human_script(steps);

    let (options, header) = record_start(rom, 7);
    let mut played = App::new_replay(options, Some(rom.to_vec()));
    played.start_recording(Recorder::new(header));
    play_like_the_window(&mut played, &script, steps, &[1, 3, 2, 5, 1, 1, 7, 2, 0, 4]);
    let want = played.state_digest();
    let record = played.finish_recording().expect("a recording");
    assert_eq!(record.end, steps);
    assert_eq!(
        record.digests.len(),
        (steps / record::DIGEST_EVERY + 1) as usize
            + usize::from(steps % record::DIGEST_EVERY != 0),
        "one digest every {} steps, from step 0, and the closing one",
        record::DIGEST_EVERY
    );
    let record = record::Record::parse(&record.to_text()).expect("the file reads back");

    // Headless, one step at a time.
    let (options, _) = record_start(rom, 7);
    let mut headless = App::new_replay(options, Some(rom.to_vec()));
    headless.start_replay(record.clone());
    let stop = headless.play_record(None);
    assert_eq!(
        stop,
        headless::RecordStop::End,
        "{}",
        headless.record_stop_line(&stop)
    );
    assert_eq!(headless.state_digest(), want);
    assert_eq!(
        headless.recording.replay.as_ref().unwrap().checked(),
        record.digests.len()
    );

    // In the window's loop, with other frames: the same checkpoints.
    let (options, _) = record_start(rom, 7);
    let mut framed = App::new_replay(options, Some(rom.to_vec()));
    framed.start_replay(record);
    while framed.steps < steps {
        let due = [6, 1, 1, 2][framed.steps as usize % 4];
        framed.accumulator = 0.0;
        framed.last =
            Instant::now() - std::time::Duration::from_secs_f32(due as f32 * STEP + 0.004);
        framed.update();
    }
    assert_eq!(framed.state_digest(), want);
    assert_eq!(framed.checkpoints_jsonl(), headless.checkpoints_jsonl());
    assert!(
        headless.journal.events().len() > 1,
        "the confirm key must have played the opening: {:?}",
        headless.journal.events()
    );
}

#[test]
fn a_replay_without_the_players_inputs_fails_at_the_first_digest_that_differs() {
    let Some(rom) = testrom::load() else { return };
    let steps = 9_000;
    let (options, header) = record_start(rom, 7);
    let mut played = App::new_replay(options, Some(rom.to_vec()));
    played.start_recording(Recorder::new(header));
    play_like_the_window(&mut played, &human_script(steps), steps, &[2, 1, 3]);
    let mut record = played.finish_recording().expect("a recording");
    record.inputs.clear();

    let (options, _) = record_start(rom, 7);
    let mut app = App::new_replay(options, Some(rom.to_vec()));
    app.start_replay(record);
    let stop = app.play_record(None);
    let headless::RecordStop::Mismatch(why) = stop else {
        panic!("a replay that never presses a key must differ: {stop:?}");
    };
    assert!(why.contains("digest mismatch at step"), "{why}");
    assert!(
        app.steps < steps,
        "it stops at the first difference, not at the end"
    );
}

/// The pad bits and action key of an `--trace-inputs` JSONL line, as `replay_step` takes them.
fn replay_frames(jsonl: &str) -> Vec<(u8, bool)> {
    jsonl
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let pad = line
                .split_once("\"pad\":[")
                .and_then(|(_, rest)| rest.split_once(']'))
                .map(|(pad, _)| pad)
                .unwrap_or_else(|| panic!("no pad in {line}"));
            let mut bits = 0;
            for (bit, name) in headless::PAD_NAMES {
                if pad.contains(&format!("\"{name}\"")) {
                    bits |= bit;
                }
            }
            (bits, line.contains("\"a\":true"))
        })
        .collect()
}

/// The pilot's own `--trace-inputs` pad, replayed headless, reaches the pilot's own
/// checkpoints, the first control point (story 1.1.1) included: both paths must spend the
/// same fixed steps (`app/headless.rs`, `replay_step`). The parity references are such traces
/// replayed on the headless runtime, so a step that only one path spends moves the replay off
/// the route it was recorded on (batch `j4-parity-regress`: native 560 made event 1140 take
/// the script's guest branch, 20 ticks earlier, and the recorded route aged).
#[test]
fn a_pilot_trace_replays_to_the_first_control_point() {
    let Some(rom) = testrom::load() else { return };
    let new_game = |trace_inputs: bool| Options {
        map: None,
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs,
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
        play: BattlePlay {
            fight: true,
            seed: Some(7),
        },
    };
    // The prologue to the first free control takes 13 785 fixed steps at the original message
    // rate (the opening, two scenes and battle 26 fought by the engine); the budget leaves
    // the pilot walking after it, so the trace carries pad bits as well as the confirm key.
    const STEPS: u64 = 15_000;
    let controls = |app: &App| -> Vec<(u8, u8, u8)> {
        app.journal
            .steps
            .iter()
            .filter_map(|(_, step)| match step {
                Step::Control { story, .. } => Some(*story),
                _ => None,
            })
            .collect()
    };
    let mut pilot = App::new_headless(new_game(true), Some(rom.to_vec()));
    pilot.play_auto(STEPS, true);
    let reached = controls(&pilot);
    assert!(
        reached.contains(&(1, 1, 1)),
        "the prologue must reach the first control point within {STEPS} steps: {reached:?}"
    );

    let trace = pilot.input_trace_jsonl();
    assert_eq!(
        trace.lines().count() as u64,
        STEPS,
        "one line per fixed step"
    );
    let mut replay = App::new_headless(new_game(false), Some(rom.to_vec()));
    for (bits, confirm) in replay_frames(&trace) {
        replay.replay_step(bits, confirm);
    }
    let replayed = controls(&replay);
    assert!(
        replayed.contains(&(1, 1, 1)),
        "the replayed trace must reach the first control point: {replayed:?}"
    );
    assert_eq!(
        replay.checkpoints_jsonl(),
        pilot.checkpoints_jsonl(),
        "the replayed trace must land on the pilot's own checkpoints"
    );
}

/// A route session that shows its battles, as a window does, to the end of event 2750: the
/// tick, the line it stopped with, its checkpoints and the state it ended in.
fn shown_route(rom: &[u8]) -> (u64, String, String, record::Digest) {
    let (options, _) = record_start(rom, 7);
    let mut app = App::new_headless(options, None);
    app.set_route_session(2750, 2_000, true);
    app.set_rom(rom.to_vec());
    while app.session_running() {
        app.session_frame();
    }
    let stop = app.session_stop().expect("the session ended");
    (app.tick, stop, app.checkpoints_jsonl(), app.state_digest())
}

/// The determinism hunt of the human-record batch: nothing the logic reads may depend on the
/// wall clock, hash order or uninitialised state. The same shown route (battles presented,
/// like a window) run twice gives the same steps, checkpoints and final state.
#[test]
#[ignore = "plays the route to event 2750 twice (minutes in release): cargo test --release -- --ignored"]
fn the_shown_route_runs_twice_identically() {
    let Some(rom) = testrom::load() else { return };
    let (first_tick, first_stop, first, first_digest) = shown_route(rom);
    let (second_tick, second_stop, second, second_digest) = shown_route(rom);
    assert!(
        first_stop.starts_with("stopped: event 2750 ended"),
        "the route must reach 2750: {first_stop}"
    );
    assert_eq!(first_stop, second_stop);
    assert_eq!(first_tick, second_tick);
    let differing = first.lines().zip(second.lines()).position(|(a, b)| a != b);
    assert_eq!(differing, None, "first differing checkpoint line");
    assert_eq!(first.lines().count(), second.lines().count());
    assert_eq!(first_digest, second_digest);
}

/// A human record played on this machine reaches the event it was made for. `DQ9_RECORD` is
/// the file (`dq9-app --new-game --seed N --record <file>`), `DQ9_RECORD_EVENT` the event
/// (default 2750), `DQ9_RECORD_SAVE` the save it started from, if any. Skipped without the
/// file, so CI and `verify.ps1` never need one.
#[test]
#[ignore = "needs DQ9_RECORD (a record of a human session): scripts/replay-record.ps1 runs it"]
fn human_record_reaches_the_expected_event() {
    let (Some(path), Some(rom)) = (std::env::var_os("DQ9_RECORD"), testrom::load()) else {
        eprintln!("skipping: DQ9_RECORD or the ROM is missing");
        return;
    };
    let event: u16 = std::env::var("DQ9_RECORD_EVENT").ok().map_or(2750, |text| {
        text.parse().expect("DQ9_RECORD_EVENT is an event id")
    });
    let save = std::env::var_os("DQ9_RECORD_SAVE").map(|p| std::fs::read(p).expect("the save"));
    let text = std::fs::read_to_string(path).expect("reading the record");
    let record = record::Record::parse(&text).expect("a dq9rec file");
    record
        .header
        .check_start(rom, save.as_deref())
        .expect("the record's ROM and save");
    let options = Options::from_record(&record.header, save, Layout::default(), true);
    let mut app = App::new_replay(options, Some(rom.to_vec()));
    app.start_replay(record);
    let stop = app.play_record(Some(event));
    assert_eq!(
        stop,
        headless::RecordStop::Event(event),
        "{}",
        app.record_stop_line(&stop)
    );
}
