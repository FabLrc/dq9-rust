//! Native-only drivers of [`App`](super::App): `--walk`, `--replay`, the checkpoint and input
//! traces (`--trace-inputs`) and the windowless `--headless-steps` run. The Web build never
//! compiles this module.

#[cfg(not(target_arch = "wasm32"))]
use dq9_core::field;
use dq9_core::scenario::Step;
use glam::Vec3;

use super::{App, status};
use crate::talk::Button;

/// How a replayed record ended (`--play-record`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordStop {
    /// The event asked for ended.
    Event(u16),
    /// The record's last step ran.
    End,
    /// A digest differed: the first one, with its step and field.
    Mismatch(String),
}

/// The d-pad bit and its JSONL name, in the order `--trace-inputs` writes them.
pub(crate) const PAD_NAMES: [(u8, &str); 4] = [(1, "up"), (2, "down"), (4, "left"), (8, "right")];

/// Writes a flag set as a JSON number array, without a per-flag allocation.
fn write_flag_list(out: &mut String, flags: &std::collections::BTreeSet<i32>) {
    use std::fmt::Write as _;
    for (index, flag) in flags.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{flag}");
    }
}

impl App {
    /// One fixed step of a native `--walk` script: the world direction is fed exactly as the
    /// pilot feeds it, through the same `Hero::step`/`Field::step`/door path as the window.
    pub(crate) fn walk_step(&mut self, direction: Vec3) {
        self.pilot_dir = Some(direction);
        self.run_steps(1);
    }

    /// Places the hero at `(x, z)` on the current map (`--start`), on the collision's ground.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn place_hero(&mut self, x: f32, z: f32) -> anyhow::Result<()> {
        let code = self
            .current
            .as_ref()
            .map(|(entry, _)| entry.code.as_str())
            .unwrap_or_default();
        let hero = crate::walk::start_hero(&self.collision, code, (x, z))?;
        self.hero = Some(hero);
        if let (Some(runtime), Some(index)) = (&mut self.runtime, self.hero_instance) {
            let world = runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()));
            if let Some(instance) = runtime.scene.instances.get_mut(index) {
                instance.world = world;
            }
        }
        Ok(())
    }

    /// The loaded map, its code and the hero's pose, for the `--walk` logs.
    pub(crate) fn walk_pose(&self) -> Option<(u16, &str, Vec3, f32)> {
        let (entry, _) = self.current.as_ref()?;
        let hero = self.hero?;
        Some((entry.id, entry.code.as_str(), hero.position(), hero.yaw()))
    }

    /// One fixed step of a recorded input trace (`--trace-inputs`, replayed): the pad keys and
    /// the action key are fed exactly as recorded, and the journal checkpoints are captured.
    pub(crate) fn replay_step(&mut self, bits: u8, confirm: bool) {
        self.recording.replay_pad = Some(bits);
        if confirm {
            self.talk.press(Button::Confirm);
        }
        self.observe_step();
    }

    /// One fixed step with the journal's checkpoints and the control points captured, as the
    /// pilot's runs do: what both `--replay` and a replayed human record (`--play-record`)
    /// compare.
    pub(crate) fn observe_step(&mut self) {
        let recorded = self.journal.steps.len();
        self.run_steps(1);
        self.capture_journal(recorded);
        // The pilot's control points are part of the proof: record them the same way. The
        // cast task of action 0x8d holds the field like an event.
        let busy = self.events.is_active()
            || self.events.pending_start().is_some()
            || self.cast_reload
            || self.battle_is_active();
        if !busy && !self.talk.is_active() && !self.talk.confirm_pending() {
            let map = self.current.as_ref().map_or(0, |(entry, _)| entry.id);
            let track = &self.state.tracks[self.state.track];
            let story = (track.chapter, track.step, track.substep);
            let last = self
                .journal
                .steps
                .iter()
                .rev()
                .find_map(|(_, step)| match step {
                    Step::Control { story, .. } => Some(*story),
                    _ => None,
                });
            if last.is_none_or(|seen| seen != story) {
                self.journal.record(self.tick, Step::Control { map, story });
                self.capture_checkpoint("control");
            }
        }
    }

    /// Records the story state for the oracle (`dq9re state --json` field names): one JSON
    /// object per checkpoint, in the order the journal recorded them.
    pub(super) fn capture_checkpoint(&mut self, kind: &'static str) {
        use std::fmt::Write as _;
        let map = self.current.as_ref().map_or(0, |(entry, _)| entry.id);
        let area = self
            .current
            .as_ref()
            .and_then(|(entry, _)| entry.code.get(..3))
            .and_then(|family| self.maps.iter().find(|map| map.code == family))
            .map_or(0, |map| map.id);
        let daytime = self.day.phase().is_daytime();
        let mut out = String::new();
        let _ = write!(
            out,
            "{{\"tick\":{},\"seconds\":{:.2},\"kind\":\"{kind}\",\"vblank\":{},\"map\":{},\"area\":{},\"track\":{},\"daytime\":{},",
            self.tick,
            self.tick as f64 / 60.0,
            self.tick,
            map,
            area,
            self.state.track,
            daytime,
        );
        out.push_str("\"tracks\":[");
        for (index, track) in self.state.tracks.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"chapter\":{},\"step\":{},\"substep\":{},\"chapter_flags\":[",
                track.chapter, track.step, track.substep
            );
            write_flag_list(&mut out, &track.flags[0]);
            out.push_str("],\"step_flags\":[");
            write_flag_list(&mut out, &track.flags[1]);
            out.push_str("]}");
        }
        out.push_str("],\"flags\":[");
        write_flag_list(&mut out, &self.state.flags);
        out.push(']');
        out.push_str(",\"quests\":{");
        for (index, (quest, state)) in self.state.quests.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "\"{quest}\":{state}");
        }
        out.push('}');
        out.push_str(",\"party\":[");
        for (index, member) in self.party.members.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"level\":{},\"hp\":{},\"max_hp\":{},\"mp\":{},\"max_mp\":{},\"experience\":{}}}",
                member.level(),
                member.hp,
                member.max_hp,
                member.mp,
                member.max_mp,
                member.current_experience()
            );
        }
        let _ = write!(out, "],\"gold\":{}", self.party.gold);
        let event = self
            .events
            .current()
            .map_or_else(|| "null".to_owned(), |event| event.to_string());
        let hero = self.hero.map_or([0.0; 4], |hero| {
            [
                hero.position().x,
                hero.position().y,
                hero.position().z,
                hero.yaw(),
            ]
        });
        let _ = write!(
            out,
            ",\"event\":{event},\"hero\":{{\"x\":{:.3},\"y\":{:.3},\"z\":{:.3},\"yaw\":{:.3}}}}}",
            hero[0], hero[1], hero[2], hero[3]
        );
        self.checkpoints.push(out);
    }

    /// The checkpoints captured so far, one JSON object per line.
    pub(crate) fn checkpoints_jsonl(&self) -> String {
        let mut out = self.checkpoints.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }

    /// The recorded pad and action key, one JSON object per fixed step.
    pub(crate) fn input_trace_jsonl(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (frame, (bits, confirm)) in self.recording.input_trace.iter().enumerate() {
            let _ = write!(out, "{{\"frame\":{frame},\"pad\":[");
            let mut first = true;
            for (bit, name) in PAD_NAMES {
                if bits & bit == 0 {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                let _ = write!(out, "\"{name}\"");
            }
            let _ = writeln!(out, "],\"a\":{confirm}}}");
        }
        out
    }

    /// Plays the game logic without a window for at most `steps` fixed steps, until the
    /// events that were started are over, pressing the action key every `press_every` steps
    /// (`--headless-steps`, `docs/app.md`). The log shows every event, map change and trigger.
    pub(crate) fn play_headless(&mut self, steps: u32, press_every: Option<u32>) {
        let mut started = false;
        for step in 0..steps {
            if press_every.is_some_and(|n| step > 0 && step % n == 0) {
                self.talk.press(Button::Confirm);
            }
            self.run_steps(1);
            let busy = self.events.is_active()
                || self.events.pending_start().is_some()
                || self.battle_is_active();
            started |= busy;
            if started && !busy {
                status(&format!("events over after {step} steps"));
                return;
            }
        }
        status(&format!("stopped after {steps} steps"));
    }

    /// What the sound mixed during the steps so far, with its sample rate, when the
    /// application records it (`Options::record_sound`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_recording(&mut self) -> Option<(u32, Vec<f32>)> {
        self.audio
            .take_recording()
            .map(|samples| (crate::audio::RECORD_RATE, samples))
    }

    /// Whether the replayed record has steps left and no digest has differed.
    fn replay_running(&self) -> bool {
        self.recording
            .replay
            .as_ref()
            .is_some_and(|replay| self.steps < replay.end() && replay.failure().is_none())
    }

    /// After the record's last step: the inputs stamped at the end, then the closing digest.
    fn finish_replay(&mut self) {
        let step = self.steps;
        self.replay_inputs(step);
        self.replay_check(step);
    }

    /// How the replay stands: a mismatch, else the expected event, else the end.
    fn replay_outcome(&self, hit: Option<u16>) -> RecordStop {
        match self
            .recording
            .replay
            .as_ref()
            .and_then(|replay| replay.failure())
        {
            Some(why) => RecordStop::Mismatch(why.to_owned()),
            None => hit.map_or(RecordStop::End, RecordStop::Event),
        }
    }

    /// The window's frame of a replayed record: `due` fixed steps of it, the checkpoints
    /// captured as the headless replay captures them, and the outcome said once at the end.
    pub(super) fn replay_frame(&mut self, due: u32) {
        for _ in 0..due {
            if !self.replay_running() {
                break;
            }
            self.observe_step();
        }
        if self.replay_running()
            || !self
                .recording
                .replay
                .as_mut()
                .is_some_and(|r| r.first_report())
        {
            return;
        }
        self.finish_replay();
        status(&self.record_stop_line(&self.replay_outcome(None)));
    }

    /// Plays the loaded record without a window, to its end or to the end of `expect`, and
    /// returns how it stopped. Every step is the window's: the inputs come through
    /// `App::key`, the digests are checked as they fall due.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn play_record(&mut self, expect: Option<u16>) -> RecordStop {
        self.checkpoints.clear();
        let mut hit = None;
        while hit.is_none() && self.replay_running() {
            let recorded = self.journal.steps.len();
            self.observe_step();
            hit = expect.filter(|event| {
                self.journal.steps[recorded..]
                    .iter()
                    .any(|(_, step)| matches!(step, Step::EventEnded(id) if id == event))
            });
        }
        if hit.is_none() {
            self.finish_replay();
        }
        self.replay_outcome(hit)
    }

    /// The line that says where a replayed record stopped (`stopped: event N ended ...`).
    pub(crate) fn record_stop_line(&self, stop: &RecordStop) -> String {
        let seconds = self.tick as f64 / 60.0;
        match stop {
            RecordStop::Event(event) => format!(
                "stopped: event {event} ended after {seconds:.1} s of game time ({} steps)",
                self.steps
            ),
            RecordStop::End => {
                let map = self.current.as_ref().map_or(0, |(entry, _)| entry.id);
                let track = &self.state.tracks[self.state.track];
                format!(
                    "stopped: record ended after {} steps ({seconds:.1} s of game time), map {map}, story {}.{}.{}",
                    self.steps, track.chapter, track.step, track.substep
                )
            }
            RecordStop::Mismatch(why) => format!("stopped: {why}"),
        }
    }

    /// The report of a replayed record: the journal, where it stopped, the digests checked
    /// and the party.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn record_report(&self, stop: &RecordStop) -> String {
        let mut out = self.journal.to_string();
        out.push_str(&format!("events played: {:?}\n", self.journal.events()));
        out.push_str(&self.record_stop_line(stop));
        out.push('\n');
        let checked = self
            .recording
            .replay
            .as_ref()
            .map_or(0, |replay| replay.checked());
        out.push_str(&format!("digests: {checked} checked\n"));
        out.push_str(&self.party_report());
        out
    }
}
