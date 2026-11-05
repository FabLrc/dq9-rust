//! Event scripts on the real game data; skipped when the ROM is absent.

use std::collections::BTreeSet;

use dq9_core::script::{Director, Event, Frame, Text, load};
use dq9_core::trigger::{Request, State};
use nds_rom::Rom;

/// Natives the prologue calls that are not implemented (`docs/re/stb-natives.md`: roles
/// unknown). A native moving in or out of this list means the notes need an update.
const UNIMPLEMENTED: [i32; 24] = [
    238, 328, 409, 411, 413, 541, 542, 545, 546, 547, 552, 554, 559, 560, 561, 571, 574, 575, 581,
    582, 584, 712, 809, 815,
];

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

/// The 30 scripts of the prologue of Angel Falls (stages 1.1 to 1.3) run to the end: no
/// interpreter fault, every message exists in the event's text table, and the natives left
/// unimplemented are the documented ones.
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
    let unimplemented: BTreeSet<i32> = director.unimplemented().keys().copied().collect();
    assert_eq!(unimplemented, BTreeSet::from(UNIMPLEMENTED));
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
