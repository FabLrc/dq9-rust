//! Event scripts on the real game data; skipped when the ROM is absent.

use dq9_core::script::{Director, Event, Frame, Text, load};
use dq9_core::trigger::{Request, State};
use nds_rom::Rom;

/// Runs `event` to its end with an application that completes every request at once.
fn run(director: &mut Director, rom: &Rom, event: u16) -> Vec<Event> {
    let world = State::default();
    director.dispatch(vec![Request::Event(event)]);
    let mut trace = Vec::new();
    while director.is_active() {
        assert!(trace.len() < 200_000, "event {event} does not end");
        for e in director.step(rom, Frame::default(), &world) {
            if let Event::Message {
                text: Text::Id(id), ..
            } = &e
            {
                let texts = director.texts();
                assert!(texts.contains_key(id), "event {event}: no message {id}");
            }
            if let Some(signal) = e.instant_signal() {
                director.signal(signal);
            }
            trace.push(e);
        }
    }
    trace
}

/// The 30 scripts of the prologue of Angel Falls (stages 1.1 to 1.3) run to the end:
/// no interpreter fault, and every message exists in the event's text table.
#[test]
fn prologue_scripts_run_to_completion() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let events = load::stage_events(&rom, 1, 1..=3);
    assert_eq!(events.len(), 30);
    let mut director = Director::new("en");
    for &(event, _) in &events {
        let trace = run(&mut director, &rom, event);
        // An event that chains to another (native 538) ends with the last of the chain.
        assert_eq!(trace.first(), Some(&Event::Started { event }));
        assert!(matches!(trace.last(), Some(Event::Ended { .. })));
        assert!(trace.contains(&Event::Ended { event }));
        for e in &trace {
            match e {
                Event::Fault(f) => panic!("event {event}: {f}"),
                Event::LoadFailed { reason, .. } => panic!("event {event}: {reason}"),
                _ => {}
            }
        }
        assert!(
            trace.contains(&Event::Phase { entry: 100 }),
            "event {event} never ran its main entry"
        );
    }
}

/// A script is deterministic: the same event twice gives the same events.
#[test]
fn scripts_are_deterministic() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut first = Director::new("en");
    let mut second = Director::new("en");
    assert_eq!(run(&mut first, &rom, 1340), run(&mut second, &rom, 1340));
}

/// Where the opening scenes of the prologue happen, from the event lists: the first scene of
/// stage 1.1 and the maps of its chain (`docs/app.md`, § Events).
#[test]
fn prologue_events_know_their_map_and_the_stage_its_opening() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    assert_eq!(load::stage_opening(&rom, 1, 1), Some(21500));
    // `M01M12`, "Opening (Background)", without a start position of its own.
    let opening = load::place(&rom, 21500).unwrap();
    assert_eq!((opening.map, opening.position), (1112, None));
    // The observatory scene has a hero start: x, y, z and the angle of the record.
    let observatory = load::place(&rom, 1130).unwrap();
    assert_eq!(observatory.map, 4105);
    let (at, angle) = observatory.position.unwrap();
    for (got, want) in at.iter().zip([-16.6, 0.45, 2.25]) {
        assert!((got - want).abs() < 1e-3, "{at:?}");
    }
    assert!((angle - 4.71).abs() < 1e-3);
    assert_eq!(load::place(&rom, 3), None);
}

/// Record value 21 is the event task's `+0x50` (`EventRecord_Build`, `0x02071218`): bit 2 on
/// the opening scenes, bit 0 on 21510 and the 1.3 gift scene 21595, neither on the
/// prologue's own scenes (0x40 or 0x140), so only the first ones keep the cast aside.
#[test]
fn event_records_say_which_scenes_keep_the_cast_aside() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    for (event, flags, keeps) in [
        (21500, 0x4, true),
        (21507, 0x4, true),
        (21510, 0x1, true),
        (21595, 0x1, true),
        (21592, 0x0, false),
        (1130, 0x40, false),
        (1390, 0x40, false),
        (1140, 0x140, false),
    ] {
        let place = load::place(&rom, event).unwrap();
        assert_eq!(place.flags, flags, "record value 21 of event {event}");
        assert_eq!(place.keeps_cast(), keeps, "event {event}");
    }
}
