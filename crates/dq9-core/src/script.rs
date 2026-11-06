//! Event scripts (`evNNNNN.stb`): the interpreter of `ov_017` and the engine side of its
//! natives, as requests the application executes. Layout and evidence in `docs/re/stb.md`
//! and `docs/re/stb-natives.md`.
//!
//! * [`vm`]: the stack machine (`Stb_Run` and its helpers), independent of any native.
//! * [`natives`]: native number → [`Event`], and the state scripts poll.
//! * [`load`]: event number → archive, entries and texts.
//! * [`Script`]: one script run in the phases the game gives it; [`Director`]: the event
//!   task, started by the trigger requests ([`crate::trigger::Request::Event`]).
//!
//! Timing is the game's: [`Script::step`] is one update (`Stb_Resume` once). Everything a
//! script waits on is polled through a native (405 message, 204 actor, 301 camera, 840 tick
//! count): the script raises the flag when it queues work, the application lowers it with a
//! [`Signal`]. Fades have no completion native; scripts count updates instead.

pub mod event;
pub mod load;
pub mod natives;
#[cfg(test)]
mod testing;
pub mod vm;

use std::collections::{BTreeMap, VecDeque};

use nds_rom::Result;

pub use event::{ActorCommand, Camera, Event, Lock, Screen, Sound, Text};
pub use load::{EventScript, Files, LoadError, load};
pub use natives::{Frame, Runtime};
pub use vm::Fault;

use crate::field::Event as FieldEvent;
use crate::trigger::{Request, World};
use natives::Ctx;
use vm::{Program, Status, Vm};

/// Instructions one update may run before the script is stopped as stuck (a loop without
/// `yield` would hang the game).
const BUDGET: u32 = 1_000_000;

/// The entries an event runs, in the order the game does: `prepare` (fields `+0x4c`,
/// declares the cast), then the scene is built, `main` (`+0x4a`), `finish` (`+0x4e`). The
/// numbers are the event list's (`stb.md`: 200, 100, 300 in every record seen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entries {
    pub prepare: i32,
    pub main: i32,
    pub finish: i32,
}

impl Default for Entries {
    fn default() -> Self {
        Self {
            prepare: 200,
            main: 100,
            finish: 300,
        }
    }
}

/// What the application reports to the script: the completion of the work it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The last message was read and closed (native 405 reads 0 from now on).
    MessageClosed,
    /// The answer of a question (native 558).
    Answer(i32),
    /// The queues of an actor are empty (native 204).
    ActorIdle(i32),
    /// The camera queues are empty (native 301).
    CameraIdle,
    /// The preloaded files are in (native 507).
    Preloaded,
    /// The jingle asked by native 720 played out (native 725 reads 0 from now on).
    JingleEnded,
}

/// One script, run entry by entry.
#[derive(Debug, Clone)]
pub struct Script {
    program: Program,
    vm: Vm,
    pending: VecDeque<i32>,
    runtime: Runtime,
    fault: Option<Fault>,
    done: bool,
}

impl Script {
    /// A script from the bytes of its `.stb` file, to run `entries`.
    pub fn new(stb: Vec<u8>, entries: Entries) -> Result<Self> {
        let program = Program::parse(stb)?;
        Ok(Self {
            vm: Vm::new(&program),
            program,
            pending: [entries.prepare, entries.main, entries.finish].into(),
            runtime: Runtime::default(),
            fault: None,
            done: false,
        })
    }

    /// One update: runs the script until it yields, or until the entry ends (the next entry
    /// starts at the next update, which is where the game builds its scene). Returns what
    /// the natives asked meanwhile.
    pub fn step(&mut self, frame: Frame, world: &dyn World) -> Vec<Event> {
        let mut events = Vec::new();
        if self.done {
            return events;
        }
        if !self.vm.is_running() {
            loop {
                let Some(entry) = self.pending.pop_front() else {
                    self.done = true;
                    return events;
                };
                match self.vm.start(&self.program, entry) {
                    Ok(()) => {
                        events.push(Event::Phase { entry });
                        break;
                    }
                    // An entry the script does not have (`Stb_Start` returns -1): nothing runs.
                    Err(Fault {
                        kind: vm::FaultKind::NoEntry,
                        ..
                    }) => {}
                    Err(fault) => return self.stop(events, fault),
                }
            }
        }
        let mut ctx = Ctx {
            rt: &mut self.runtime,
            events: &mut events,
            world,
            frame,
        };
        match self.vm.run(&self.program, &mut ctx, BUDGET) {
            Ok(Status::Yielded) => {}
            Ok(Status::Finished) => self.done = self.pending.is_empty(),
            Err(fault) => return self.stop(events, fault),
        }
        events
    }

    fn stop(&mut self, mut events: Vec<Event>, fault: Fault) -> Vec<Event> {
        self.fault = Some(fault);
        self.done = true;
        events.push(Event::Fault(fault));
        events
    }

    /// The application reports a completion.
    pub fn signal(&mut self, signal: Signal) {
        let rt = &mut self.runtime;
        match signal {
            Signal::MessageClosed => rt.message_open = false,
            Signal::Answer(answer) => rt.answer = answer,
            Signal::ActorIdle(actor) => {
                if let Some(busy) = usize::try_from(actor)
                    .ok()
                    .and_then(|a| rt.actor_busy.get_mut(a))
                {
                    *busy = false;
                }
            }
            Signal::CameraIdle => rt.camera_busy = false,
            Signal::Preloaded => rt.preload_pending = false,
            Signal::JingleEnded => rt.jingle_playing = false,
        }
    }

    /// What the scripts poll, for the values only the application knows (actor poses,
    /// playing animations).
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    /// The whole script ran, or stopped on a fault.
    pub fn is_finished(&self) -> bool {
        self.done
    }

    pub fn fault(&self) -> Option<Fault> {
        self.fault
    }

    /// Event to chain to once this one ends (native 538), if any.
    pub fn next_event(&self) -> Option<u16> {
        Some(self.runtime.next_event).filter(|&e| e != 0)
    }

    /// Natives called but not implemented, with their call counts.
    pub fn unimplemented(&self) -> &BTreeMap<i32, u32> {
        &self.runtime.unimplemented
    }

    /// Entry numbers the script's header declares.
    pub fn entries(&self) -> impl Iterator<Item = i32> + '_ {
        self.program.entries()
    }
}

/// The event task (`EventTask_Update`, `ov_017::021bc77c`): runs the event a trigger
/// started, the next one when it ends or chains (native 538), and reports where each starts
/// and ends. The application fires trigger event 11 on [`Event::Ended`], as the game does.
#[derive(Debug)]
pub struct Director {
    lang: String,
    queue: VecDeque<u16>,
    current: Option<(u16, Script)>,
    texts: BTreeMap<i32, String>,
    unimplemented: BTreeMap<i32, u32>,
}

impl Director {
    /// `lang`: language of the texts (`en`, `fr`, …).
    pub fn new(lang: &str) -> Self {
        Self {
            lang: lang.to_owned(),
            queue: VecDeque::new(),
            current: None,
            texts: BTreeMap::new(),
            unimplemented: BTreeMap::new(),
        }
    }

    /// Natives called but not implemented since the director was made, with call counts.
    pub fn unimplemented(&self) -> &BTreeMap<i32, u32> {
        &self.unimplemented
    }

    /// Message table of the running event ([`Text::Id`] keys).
    pub fn texts(&self) -> &BTreeMap<i32, String> {
        &self.texts
    }

    /// Queues the events the requests start (actions 0x77, 0x80, 0x9b) and returns the others.
    pub fn dispatch(&mut self, requests: Vec<Request>) -> Vec<Request> {
        let mut rest = Vec::new();
        for request in requests {
            match request {
                Request::Event(event) => self.queue.push_back(event),
                other => rest.push(other),
            }
        }
        rest
    }

    /// The same for the events of the field task ([`crate::field::Field::step`]): takes the
    /// event starts, returns the other events.
    pub fn dispatch_field(&mut self, events: Vec<FieldEvent>) -> Vec<FieldEvent> {
        events
            .into_iter()
            .filter(|event| match event {
                FieldEvent::Request(Request::Event(id)) => {
                    self.queue.push_back(*id);
                    false
                }
                _ => true,
            })
            .collect()
    }

    /// The event the next update starts, when none runs: the application prepares its scene
    /// (map, models) before stepping.
    pub fn pending_start(&self) -> Option<u16> {
        match self.current {
            Some(_) => None,
            None => self.queue.front().copied(),
        }
    }

    /// Whether an event runs or waits.
    pub fn is_active(&self) -> bool {
        self.current.is_some() || !self.queue.is_empty()
    }

    /// The running event.
    pub fn current(&self) -> Option<(u16, &Script)> {
        self.current.as_ref().map(|(e, s)| (*e, s))
    }

    pub fn signal(&mut self, signal: Signal) {
        if let Some((_, script)) = &mut self.current {
            script.signal(signal);
        }
    }

    pub fn runtime_mut(&mut self) -> Option<&mut Runtime> {
        self.current.as_mut().map(|(_, s)| s.runtime_mut())
    }

    /// One update: starts the next event when none runs (a script that cannot be loaded
    /// gives [`Event::LoadFailed`] and is skipped), steps it, and ends it when done.
    pub fn step(&mut self, files: &dyn Files, frame: Frame, world: &dyn World) -> Vec<Event> {
        let mut events = Vec::new();
        while self.current.is_none() {
            let Some(event) = self.queue.pop_front() else {
                return events;
            };
            let started = load(files, event, &self.lang).and_then(|loaded| {
                let mut script = Script::new(loaded.stb, loaded.entries)?;
                script.runtime.messages = Some(loaded.texts.keys().copied().collect());
                self.texts = loaded.texts;
                Ok(script)
            });
            match started {
                Ok(script) => {
                    events.push(Event::Started { event });
                    self.current = Some((event, script));
                }
                Err(e) => events.push(Event::LoadFailed {
                    event,
                    reason: e.to_string(),
                }),
            }
        }
        let Some((event, script)) = &mut self.current else {
            return events;
        };
        let event = *event;
        events.extend(script.step(frame, world));
        if script.is_finished() {
            let next = script.next_event();
            for (id, calls) in script.unimplemented() {
                *self.unimplemented.entry(*id).or_default() += calls;
            }
            events.push(Event::Ended { event });
            self.current = None;
            // A chained event keeps the context, so it runs before the queued ones.
            if let Some(next) = next {
                self.queue.push_front(next);
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::testing::*;
    use super::*;
    use crate::trigger::State;

    fn script(funcs: &[Func], entries: &[(i32, usize)]) -> Script {
        Script::new(build(0, funcs, entries), Entries::default()).unwrap()
    }

    /// Steps until the script is done, answering nothing; the events of each update.
    fn run(script: &mut Script, world: &dyn World) -> Vec<Vec<Event>> {
        let mut updates = Vec::new();
        while !script.is_finished() {
            assert!(updates.len() < 1000, "script does not end");
            updates.push(script.step(Frame::default(), world));
        }
        updates
    }

    fn flat(updates: &[Vec<Event>]) -> Vec<&Event> {
        updates.iter().flatten().collect()
    }

    /// `400(value)`: shows message number `value`, which makes a variable observable.
    fn probe(value: T) -> Vec<T> {
        native(400, &[value])
    }

    fn message(id: i32) -> Event {
        Event::Message {
            text: Text::Id(id),
            flags: 0,
        }
    }

    #[test]
    fn entries_run_in_order_one_per_update_and_missing_ones_are_skipped() {
        let entry = |tag: i32| func(0, 0, [probe(int(tag)), vec![op(END)]].concat());
        // 100 and 200 exist; 300 does not.
        let mut s = script(&[entry(0), entry(1), entry(2)], &[(200, 1), (100, 2)]);
        let updates = run(&mut s, &State::default());
        assert_eq!(
            flat(&updates),
            [
                &Event::Phase { entry: 200 },
                &message(1),
                &Event::Phase { entry: 100 },
                &message(2),
            ]
        );
        assert_eq!(updates.len(), 3, "the empty last update ends the script");
        assert!(s.fault().is_none());
    }

    #[test]
    fn a_message_holds_the_script_until_the_application_closes_it() {
        // 400(100); while 405 == 1 { yield }; 401
        let wait = [
            probe(int(100)),
            native(405, &[addr(0)]),
            vec![
                var(0),
                int(1),
                T::Op(CMP, 0x28, 0),
                T::Jump(JF, 12, 0),
                op(YIELD),
                T::Jump(JUMP, 3, 0),
            ],
            native(401, &[]),
            vec![op(END)],
        ]
        .concat();
        let mut s = script(&[func(0, 0, vec![op(END)]), func(1, 0, wait)], &[(100, 1)]);
        let world = State::default();
        let first = s.step(Frame::default(), &world);
        assert_eq!(first, [Event::Phase { entry: 100 }, message(100)]);
        // Still open: the script polled 405 and yielded again.
        assert!(s.step(Frame::default(), &world).is_empty());
        assert!(s.step(Frame::default(), &world).is_empty());
        s.signal(Signal::MessageClosed);
        assert_eq!(s.step(Frame::default(), &world), [Event::CloseMessage]);
    }

    #[test]
    fn actors_stay_busy_until_the_application_reports_them_idle() {
        // 206(3, 1.0, 2.0, 3.0); 204(3, &local0); 400(local0)
        let code = [
            native(206, &[int(3), float(1.0), float(2.0), float(3.0)]),
            native(204, &[int(3), addr(0)]),
            probe(var(0)),
            vec![op(YIELD)],
            native(204, &[int(3), addr(0)]),
            probe(var(0)),
            vec![op(END)],
        ]
        .concat();
        let mut s = script(&[func(0, 0, vec![op(END)]), func(1, 0, code)], &[(100, 1)]);
        let world = State::default();
        let first = s.step(Frame::default(), &world);
        assert_eq!(
            first[1..],
            [
                Event::Actor {
                    actor: 3,
                    command: ActorCommand::SetPosition([1.0, 2.0, 3.0])
                },
                message(1)
            ]
        );
        s.signal(Signal::ActorIdle(3));
        assert_eq!(s.step(Frame::default(), &world), [message(0)]);
        assert_eq!(s.runtime_mut().pose[3].0, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn fades_decode_their_target_lock_and_screen() {
        let code = [
            native(100, &[int(10)]),
            native(101, &[int(20)]),
            native(107, &[int(30), int(16)]),
            native(117, &[int(40), int(16)]),
            native(114, &[int(50)]),
            vec![op(END)],
        ]
        .concat();
        let mut s = script(&[func(0, 0, vec![op(END)]), func(0, 0, code)], &[(100, 1)]);
        let events = s.step(Frame::default(), &State::default());
        let fade = |screen, level, ticks, lock| Event::Fade {
            screen,
            level,
            ticks,
            lock,
        };
        assert_eq!(
            events[1..],
            [
                fade(Screen::Both, 0, 10, Lock::Keep),
                fade(Screen::Both, -16, 20, Lock::Keep),
                fade(Screen::Main, 16, 30, Lock::Keep),
                fade(Screen::Both, 0, 40, Lock::Unlock),
                fade(Screen::Sub, -16, 50, Lock::Lock),
            ]
        );
    }

    #[test]
    fn unknown_natives_are_reported_once_and_counted() {
        let code = [
            native(571, &[int(1), int(0)]),
            native(571, &[int(2), int(0)]),
            native(584, &[int(0)]),
            // Established but without counterpart here, and ids with no handler: silent.
            native(502, &[int(1)]),
            native(999, &[]),
            vec![op(END)],
        ]
        .concat();
        let mut s = script(&[func(0, 0, vec![op(END)]), func(0, 0, code)], &[(100, 1)]);
        let events = s.step(Frame::default(), &State::default());
        assert_eq!(
            events[1..],
            [
                Event::Unimplemented { id: 571 },
                Event::Unimplemented { id: 584 }
            ]
        );
        assert_eq!(s.unimplemented().get(&571), Some(&2));
        assert_eq!(s.unimplemented().get(&999), None);
    }

    #[test]
    fn natives_read_the_update_and_the_world() {
        let code = [
            native(840, &[addr(0)]),
            probe(var(0)),
            native(600, &[int(77), addr(1)]),
            probe(var(1)),
            native(600, &[int(78), addr(1)]),
            probe(var(1)),
            native(596, &[int(12), addr(0), addr(1)]),
            probe(var(0)),
            probe(var(1)),
            vec![op(END)],
        ]
        .concat();
        let mut s = script(&[func(0, 0, vec![op(END)]), func(2, 0, code)], &[(100, 1)]);
        let mut world = State::default();
        world.flags.insert(77);
        let events = s.step(Frame { ticks: 3 }, &world);
        let ids: Vec<i32> = events
            .iter()
            .filter_map(|e| match e {
                Event::Message {
                    text: Text::Id(id), ..
                } => Some(*id),
                _ => None,
            })
            .collect();
        // Ticks, flag set, flag clear, then an undeclared actor: kind -1, state 2.
        assert_eq!(ids, [3, 1, 0, -1, 2]);
    }

    #[test]
    fn declared_cast_members_take_negative_ids_in_order() {
        let code = [
            native(566, &[int(4), int(100), int(1)]),
            native(566, &[int(8), int(2)]),
            native(566, &[int(5), int(105), int(3)]),
            native(566, &[int(11), int(12)]),
            native(596, &[int(12), addr(0), addr(1)]),
            probe(var(0)),
            vec![op(END)],
        ]
        .concat();
        let mut s = script(&[func(0, 0, vec![op(END)]), func(2, 0, code)], &[(100, 1)]);
        let events = s.step(Frame::default(), &State::default());
        let declared: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DeclareCast {
                    mode, character, ..
                } => Some((*mode, *character)),
                _ => None,
            })
            .collect();
        assert_eq!(
            declared,
            [(4, Some(-1)), (8, Some(-2)), (5, Some(105)), (11, None)]
        );
        assert_eq!(events.last(), Some(&message(2)), "mode 11 is category 2");
    }

    /// A script whose entry 100 chains to event `next` (native 538) when it is not 0.
    fn chain_file(next: i32) -> Vec<u8> {
        let code = [native(538, &[int(next)]), vec![op(END)]].concat();
        let stb = build(
            0,
            &[func(0, 0, vec![op(END)]), func(0, 0, code)],
            &[(100, 1)],
        );
        gp2(&[("ev00001.stb", &stb)])
    }

    struct Mem(HashMap<String, Vec<u8>>);

    impl Files for Mem {
        fn file(&self, path: &str) -> Option<&[u8]> {
            self.0.get(path).map(Vec::as_slice)
        }
    }

    #[test]
    fn a_message_missing_from_the_table_shows_nothing() {
        let code = [
            probe(int(1)),
            native(405, &[addr(0)]),
            probe(var(0)),
            probe(int(100)),
            vec![op(END)],
        ]
        .concat();
        let mut s = script(&[func(0, 0, vec![op(END)]), func(1, 0, code)], &[(100, 1)]);
        s.runtime_mut().messages = Some([100].into());
        let events = s.step(Frame::default(), &State::default());
        // Message 1 is absent: nothing shown, nothing to wait for (405 reads 0, shown as
        // message 0, which is absent as well); message 100 is shown.
        assert_eq!(events, [Event::Phase { entry: 100 }, message(100)]);
    }

    #[test]
    fn the_director_runs_queued_events_and_chains_before_them() {
        let archive = |name: &str| (format!("data/event/{name}.gp2"), chain_file(0));
        let mut files: HashMap<_, _> = ["ev00001", "ev00002", "ev00003", "ev00004"]
            .map(archive)
            .into();
        // Event 1 chains to 3; every archive holds `ev00001.stb`, the loader takes the
        // first `.stb` member whatever its name.
        files.insert("data/event/ev00001.gp2".into(), chain_file(3));
        let files = Mem(files);
        let mut director = Director::new("en");
        let rest = director.dispatch(vec![
            Request::Event(1),
            Request::ReloadTriggers,
            Request::Event(2),
        ]);
        assert_eq!(rest, [Request::ReloadTriggers]);
        // Event 9 has no archive; it comes from the field task.
        let rest = director.dispatch_field(vec![
            FieldEvent::Request(Request::Event(9)),
            FieldEvent::Unsupported(5),
        ]);
        assert_eq!(rest, [FieldEvent::Unsupported(5)]);
        let mut trace = Vec::new();
        let world = State::default();
        while director.is_active() {
            assert!(trace.len() < 100);
            trace.extend(director.step(&files, Frame::default(), &world));
        }
        let outline: Vec<_> = trace
            .iter()
            .filter_map(|e| match e {
                Event::Started { event } => Some(format!("start {event}")),
                Event::Ended { event } => Some(format!("end {event}")),
                Event::LoadFailed { event, .. } => Some(format!("failed {event}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            outline,
            [
                "start 1", "end 1", "start 3", "end 3", "start 2", "end 2", "failed 9"
            ]
        );
    }
}
