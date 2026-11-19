//! The Web session: the pilot and the route mode driven from the page, one animation frame at a
//! time (`?newgame=1&pilot=1&fight=1&route=2750&seed=7&speed=20`, `docs/app/browser.md`,
//! § "Route mode in the browser").
//!
//! The session calls [`App::auto_step`], the very step of the native `--headless --auto` loop,
//! so the browser plays the same logic; only the clock differs. The fixed 60 Hz step stays the
//! unit: a frame runs `speed` of them (never a variable `dt`), stopping early only when the
//! frame's time budget is spent, so the number of steps per frame changes how long the run
//! takes and never what it does.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use web_time::Instant;

use super::{App, Pilot, Stop, route};
use crate::app::{STEP, status};

/// What the page asked for.
#[derive(Debug, Clone)]
pub(crate) struct SessionSpec {
    /// The event whose end stops the run (`route=2750`); without one the story pilot runs.
    pub route: Option<u16>,
    /// Fixed steps per animation frame; `None` plays in real time (60 steps per second).
    pub speed: Option<u32>,
    /// Wall time after which a frame stops running steps, in milliseconds.
    pub budget_ms: u32,
    /// Draws the frames (`render=0` runs the logic only: no frame is presented).
    pub render: bool,
    /// Shows the battles (the transition, the scene, the fight's windows) instead of settling
    /// them in the step that starts them like the native headless run (`battles=show`).
    pub show_battles: bool,
}

/// The session of a page: the request, and the run once the new game has started.
pub(crate) struct Session {
    spec: SessionSpec,
    run: Option<Run>,
}

/// A started run.
struct Run {
    bot: Box<Pilot>,
    started: bool,
    /// Checkpoints already handed to the page.
    published: usize,
    /// The step count at which the next progress line is due.
    next_progress: u64,
    /// Why the run ended, once it has.
    stop: Option<String>,
    begun: Instant,
}

/// Fixed steps between two progress lines (10 s of game time).
const PROGRESS_EVERY: u64 = 600;
/// Steps run between two looks at the wall clock.
const CLOCK_EVERY: u32 = 32;

impl Session {
    pub(crate) fn new(spec: SessionSpec) -> Self {
        Self { spec, run: None }
    }

    /// Whether the frames are drawn.
    pub(crate) fn renders(&self) -> bool {
        self.spec.render
    }

    /// Whether the battles are shown rather than settled in the step that starts them.
    pub(crate) fn shows_battles(&self) -> bool {
        self.spec.show_battles
    }

    /// Whether a run is going: the frame is the session's, not the keyboard's.
    pub(crate) fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|run| run.stop.is_none())
    }
}

impl App {
    /// Asks for a session on this page: the next new game becomes the pilot's run. The roaming
    /// monsters stay off until it starts, like the headless run that enables them itself.
    pub(crate) fn set_session(&mut self, spec: SessionSpec) {
        self.session = Some(Session::new(spec));
        self.set_roaming_enabled(false);
    }

    /// A route session as the tests of the application drive it: `speed` fixed steps per
    /// frame, battles shown or settled, nothing drawn.
    #[cfg(test)]
    pub(in crate::app) fn set_route_session(&mut self, event: u16, speed: u32, show: bool) {
        self.set_session(SessionSpec {
            route: Some(event),
            speed: Some(speed),
            budget_ms: 600_000,
            render: false,
            show_battles: show,
        });
    }

    /// The line a finished session stopped with (`stopped: event 2750 ended ...`).
    #[cfg(test)]
    pub(in crate::app) fn session_stop(&self) -> Option<String> {
        self.session.as_ref()?.run.as_ref()?.stop.clone()
    }

    /// Starts the run when the page asked for one: what `play_auto` and `play_route` do before
    /// their loop, once the new game's first scene is queued.
    pub(in crate::app) fn start_session(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let route = session.spec.route;
        if route.is_some() {
            // Off, then on: a second new game must build its first map's roamers again.
            self.set_roaming_enabled(false);
            self.set_roaming_enabled(true);
        }
        let seed = self.options.play.seed.unwrap_or(0);
        let bot = Pilot {
            mode: route.map(|event| route::Route::new(event, seed)),
            ..Pilot::default()
        };
        self.checkpoints.clear();
        self.accumulator = 0.0;
        self.last = Instant::now();
        let started = self.current.is_some();
        publish::reset();
        status(&format!(
            "session: pilot{} from a new game, seed {seed}{}",
            route.map_or_else(String::new, |event| format!(" and route to event {event}")),
            self.session
                .as_ref()
                .map_or(String::new(), |s| match s.spec.speed {
                    Some(n) => format!(", {n} fixed steps per frame"),
                    None => ", real time".to_owned(),
                }),
        ));
        if let Some(session) = &mut self.session {
            session.run = Some(Run {
                bot: Box::new(bot),
                started,
                published: 0,
                next_progress: 0,
                stop: None,
                begun: Instant::now(),
            });
        }
    }

    /// Whether a session is playing: `update` hands the frame to [`App::session_frame`].
    pub(in crate::app) fn session_running(&self) -> bool {
        self.session.as_ref().is_some_and(Session::running)
    }

    /// Whether the session wants its frames drawn: always without one, and once it is over.
    pub(in crate::app) fn session_renders(&self) -> bool {
        self.session
            .as_ref()
            .is_none_or(|session| session.renders() || !session.running())
    }

    /// Whether battles are settled in the step that starts them: a headless run, and a session
    /// that does not show them. A session decides on its own (`battles=show`), so a headless
    /// app can present its battles the way the browser does.
    pub(in crate::app) fn battles_instant(&self) -> bool {
        match &self.session {
            Some(session) => !session.shows_battles(),
            None => self.proxy.is_none() && !self.presented,
        }
    }

    /// One animation frame of the session: `speed` fixed steps (or the real time's), then the
    /// checkpoints and the progress for the page.
    pub(in crate::app) fn session_frame(&mut self) {
        // Only the run leaves the session while it steps: the request (`battles_instant`,
        // `session_renders`) must stay readable from inside the step.
        let Some((spec, mut run)) = self
            .session
            .as_mut()
            .and_then(|session| Some((session.spec.clone(), session.run.take()?)))
        else {
            return;
        };
        if run.stop.is_none() {
            self.run_session_frame(&spec, &mut run);
        }
        if let Some(session) = &mut self.session {
            session.run = Some(run);
        }
    }

    fn run_session_frame(&mut self, spec: &SessionSpec, run: &mut Run) {
        let now = Instant::now();
        let due = match spec.speed {
            Some(steps) => steps,
            None => {
                // Real time: the same accumulator as the keyboard's frame, a long pause clamped.
                self.accumulator += now.duration_since(self.last).as_secs_f32().min(0.25);
                let mut due = 0;
                while self.accumulator >= STEP {
                    self.accumulator -= STEP;
                    due += 1;
                }
                due
            }
        };
        self.last = now;
        let budget = std::time::Duration::from_millis(u64::from(spec.budget_ms));
        let mut stop = None;
        for done in 0..due {
            if done > 0 && done % CLOCK_EVERY == 0 && now.elapsed() >= budget {
                break;
            }
            stop = self.auto_step(&mut run.bot, true, &mut run.started);
            if stop.is_some() {
                break;
            }
        }
        for line in &self.checkpoints[run.published..] {
            publish::checkpoint(line);
        }
        run.published = self.checkpoints.len();
        if self.tick >= run.next_progress || stop.is_some() {
            run.next_progress = self.tick - self.tick % PROGRESS_EVERY + PROGRESS_EVERY;
            self.publish_progress(run, stop.is_some());
        }
        if let Some(stop) = stop {
            self.finish_session(run, &stop);
        }
    }

    /// The line the page shows while the run goes on.
    fn progress_line(&self, run: &Run) -> String {
        let map = self.current.as_ref().map_or_else(
            || "no map".to_owned(),
            |(entry, _)| format!("{} ({})", entry.code, entry.id),
        );
        let track = &self.state.tracks[self.state.track];
        let party = self.party().members.first().map_or_else(String::new, |m| {
            format!(
                ", Lv {} HP {}/{} gold {}",
                m.level(),
                m.hp,
                m.max_hp,
                self.party().gold
            )
        });
        let fights = run
            .bot
            .mode
            .as_ref()
            .map_or_else(String::new, |m| format!(", fights {}", m.fights_line()));
        format!(
            "session: {:.1} s of game time ({} steps), map {map}, story {}.{}.{}{party}{fights}",
            self.tick as f64 / 60.0,
            self.tick,
            track.chapter,
            track.step,
            track.substep
        )
    }

    fn publish_progress(&self, run: &Run, ending: bool) {
        let line = self.progress_line(run);
        if !ending {
            status(&line);
        }
        let attempts = run
            .bot
            .mode
            .as_ref()
            .map_or(&[][..], route::Route::attempts);
        publish::progress(
            &line,
            self.tick,
            run.begun.elapsed().as_secs_f64(),
            self.events.current(),
            attempts,
        );
    }

    /// The end of the run: the route's closing lines, the journal and the reason it stopped,
    /// to the console and the page.
    fn finish_session(&mut self, run: &mut Run, stop: &Stop) {
        self.route_summary(&run.bot);
        let report = self.auto_report(stop);
        let line = report
            .lines()
            .find(|line| line.starts_with("stopped:"))
            .unwrap_or("stopped")
            .to_owned();
        publish::report(&report, &line);
        status(&line);
        run.stop = Some(line);
    }

    /// The party's lines of the report on the Web (`party_report` is native-only): level,
    /// experience, gold, HP and MP.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn party_lines(&self) -> String {
        let mut out = String::new();
        for (i, member) in self.party().members.iter().enumerate() {
            out.push_str(&format!(
                "party {i}: Lv {} experience {} HP {}/{} MP {}/{} gold {}\n",
                member.level(),
                member.current_experience(),
                member.hp,
                member.max_hp,
                member.mp,
                member.max_mp,
                self.party().gold
            ));
        }
        out
    }
}

/// What the page can read: `window.dq9_session` holds the progress, the checkpoints (JSONL
/// lines, the native `--checkpoints` format) and, once it ended, the report.
#[cfg(target_arch = "wasm32")]
mod publish {
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    fn object() -> Option<Object> {
        let window = web_sys::window()?;
        let key = JsValue::from_str("dq9_session");
        let value = Reflect::get(&window, &key)
            .ok()
            .filter(JsValue::is_object)?;
        Some(value.into())
    }

    fn set(object: &Object, key: &str, value: impl Into<JsValue>) {
        let _ = Reflect::set(object, &JsValue::from_str(key), &value.into());
    }

    /// A fresh `window.dq9_session`.
    pub fn reset() {
        let Some(window) = web_sys::window() else {
            return;
        };
        let session = Object::new();
        set(&session, "checkpoints", Array::new());
        set(&session, "attempts", Array::new());
        set(&session, "done", false);
        set(&session, "stop", JsValue::NULL);
        set(&session, "report", JsValue::NULL);
        set(&session, "tick", 0.0);
        let _ = Reflect::set(&window, &JsValue::from_str("dq9_session"), &session);
    }

    pub fn checkpoint(line: &str) {
        let Some(session) = object() else {
            return;
        };
        if let Ok(list) = Reflect::get(&session, &JsValue::from_str("checkpoints"))
            && let Ok(list) = list.dyn_into::<Array>()
        {
            list.push(&JsValue::from_str(line));
        }
    }

    pub fn progress(line: &str, tick: u64, wall: f64, event: Option<u16>, attempts: &[String]) {
        let Some(session) = object() else {
            return;
        };
        crate::web::session_line(line);
        set(&session, "line", line);
        set(&session, "tick", tick as f64);
        set(&session, "seconds", tick as f64 / 60.0);
        set(&session, "wall", wall);
        set(
            &session,
            "event",
            event.map_or(JsValue::NULL, |e| JsValue::from(f64::from(e))),
        );
        let list = Array::new();
        for attempt in attempts {
            list.push(&JsValue::from_str(attempt));
        }
        set(&session, "attempts", list);
    }

    pub fn report(report: &str, stop: &str) {
        let Some(session) = object() else {
            return;
        };
        crate::web::session_line(stop);
        set(&session, "report", report);
        set(&session, "stop", stop);
        set(&session, "done", true);
        web_sys::console::log_1(&JsValue::from_str(report));
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod publish {
    pub fn reset() {}
    pub fn checkpoint(_: &str) {}
    pub fn progress(_: &str, _: u64, _: f64, _: Option<u16>, _: &[String]) {}
    pub fn report(_: &str, _: &str) {}
}
