//! Native-only drivers of [`App`](super::App): `--walk`, `--replay`, the checkpoint and input
//! traces (`--trace-inputs`) and the windowless `--headless-steps` run. The Web build never
//! compiles this module.

use dq9_core::field;
use dq9_core::scenario::Step;
use glam::Vec3;

use super::{App, status};
use crate::talk::Button;

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
        let recorded = self.journal.steps.len();
        self.replay_pad = Some(bits);
        if confirm {
            self.talk.press(Button::Confirm);
        }
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
        for (frame, (bits, confirm)) in self.input_trace.iter().enumerate() {
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
    pub(crate) fn take_recording(&mut self) -> Option<(u32, Vec<f32>)> {
        self.audio
            .take_recording()
            .map(|samples| (crate::audio::RECORD_RATE, samples))
    }
}
