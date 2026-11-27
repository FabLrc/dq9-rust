//! Native-only drivers of [`App`](super::App): `--walk`, `--replay`, the checkpoint and input
//! traces (`--trace-inputs`) and the windowless `--headless-steps` run. The Web build never
//! compiles this module.

#[cfg(not(target_arch = "wasm32"))]
use dq9_core::field;
use dq9_core::scenario::Step;
use glam::Vec3;

use super::{App, status};
use crate::talk::Button;
#[cfg(not(target_arch = "wasm32"))]
use dq9_core::state::{GameState, Placement};
#[cfg(not(target_arch = "wasm32"))]
use dq9_formats::save::Save;

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

    /// The state the session holds, bound to the layout of the slot [`App::write_save_out`]
    /// writes: [`GameState::from_play`] over the live trigger state, map, day clock and
    /// party, with the input save's own state as the base (its opaque block bytes). The
    /// placement is cleared to [`Placement::MapStart`]: a normal save carries no pose, so a
    /// state read from a suspend slot must not write its `Warp` block into a slot whose
    /// reader never takes one (`docs/re/save.md` § "Position on resume").
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn save_state(&self) -> anyhow::Result<(GameState, usize)> {
        use anyhow::Context as _;
        let input = self.options.save.as_deref().context(
            "--save-out needs --save <file>: a fresh container's opaque blocks (quest log, \
             monsters, grotto maps, zone) are not modelled, so they cannot be written",
        )?;
        let mut save = Save::parse(input).context("parsing the input save")?;
        let base = GameState::from_save(&save).context("restoring the input save")?;
        let index = save
            .prepare_normal_write()
            .context("the input save has no adventure log or suspend save to update")?;
        let map = self
            .current
            .as_ref()
            .map(|(entry, _)| entry.id)
            .context("no map is loaded: there is no state to write")?;
        let mut state =
            GameState::from_play(&self.state, &base, map, self.day, self.party.clone())?;
        state.placement = Placement::MapStart;
        Ok((state, index))
    }

    /// `--save-out`: writes the session's state as a normal save, the way `Save_WriteStep`
    /// (`0x020aad2c`) and `Save_Serialize` (`0x020a9ec8`) do (states 0–2): the target slot is
    /// [`Save::prepare_normal_write`]'s (the adventure log, else slot 0 seeded with the
    /// suspend save's image), header A is the normal one (`0x020a9ec8`: intermission 0,
    /// exists 1), the modelled fields go through [`GameState::write_into`], the slot is
    /// sealed and slot 0 is mirrored to slot 1. The whole container is written atomically and
    /// the input file is never touched. Returns the state written, so a caller can compare it
    /// with the reload.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn write_save_out(&self, out: &std::path::Path) -> anyhow::Result<GameState> {
        use anyhow::Context as _;
        let input = self.options.save.as_deref().context(
            "--save-out needs --save <file>: a fresh container's opaque blocks (quest log, \
             monsters, grotto maps, zone) are not modelled, so they cannot be written",
        )?;
        let (state, _) = self.save_state()?;
        let mut save = Save::parse(input).context("parsing the input save")?;
        let index = save
            .prepare_normal_write()
            .context("the input save has no adventure log or suspend save to update")?;
        let ranges = {
            let mut slot = save.slot_mut(index).context("slot out of range")?;
            // `Save_Serialize` writes header A for a normal save before the blocks. A slot
            // seeded from the suspend save still holds the intermission byte, which would
            // also make `SlotMut::set_area` address the `Warp` block instead of `Main`'s own
            // map word — the one a normal reader takes.
            let mut ranges = vec![slot.set_intermission(false), slot.set_exists(true)];
            ranges.extend(state.write_into(&mut slot)?);
            // A setter leaves the checksums stale: an image the game rejects is not a save.
            slot.seal();
            ranges.sort_by_key(|range| (range.start, range.end));
            ranges.dedup();
            ranges
        };
        let mirrored = index == 0 && save.mirror_normal_slot();
        write_atomically(out, save.as_bytes())?;
        let changed = input
            .iter()
            .zip(save.as_bytes())
            .filter(|(before, after)| before != after)
            .count();
        status(&format!(
            "save: wrote {} (slot {index}, map {}, story {}.{}.{}, {} member(s), {} gold, \
             {} byte(s) differ from the input{}; {} field range(s))",
            out.display(),
            state.map,
            state.scenario.chapter,
            state.scenario.step,
            state.scenario.substep,
            state.party.members.len(),
            state.party.gold,
            changed,
            if mirrored { ", slot 1 mirrored" } else { "" },
            ranges.len(),
        ));
        Ok(state)
    }

    /// The live pose of the leader, as the suspend save's `Warp` block wants it: the world
    /// position in `fx32` (`dq9_core::field::to_fx`, 4096 = 1.0) and the heading as the
    /// object angle of `Object3D+0x54` (`Save_Serialize` writes the low halfword of it, so the
    /// value is an `i16`).
    #[cfg(not(target_arch = "wasm32"))]
    fn suspend_pose(&self) -> anyhow::Result<dq9_core::state::HeroPose> {
        use anyhow::Context as _;
        let hero = self
            .hero
            .context("no hero is on the field: there is no position to suspend")?;
        Ok(dq9_core::state::HeroPose {
            position: field::to_fx(hero.position()),
            angle: (field::angle_of_yaw(hero.yaw()) * 4096.0) as i16,
        })
    }

    /// `--suspend-out`: writes the session's state into the input save's **suspend** slot,
    /// the original's Quick Save (`ov_002::02169efc` calls `Save_WriteStep`,
    /// `0x020aad2c`, with its intermission argument set, state 1 writing at `0x8010`): the
    /// modelled state, the intermission header byte and the live pose in the `Warp` block.
    /// The normal slot is left alone; the whole container goes through a temporary file and
    /// a rename, and the input file is never touched.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn write_suspend_out(&self, out: &std::path::Path) -> anyhow::Result<GameState> {
        use anyhow::Context as _;
        let input = self.options.save.as_deref().context(
            "--suspend-out needs --save <file>: a fresh container's opaque blocks (quest log, \
             monsters, grotto maps, zone) are not modelled, so they cannot be written",
        )?;
        let (state, _) = self.save_state()?;
        let pose = self.suspend_pose()?;
        let mut save = Save::parse(input).context("parsing the input save")?;
        let ranges = {
            let mut slot = save
                .slot_mut(dq9_formats::save::SUSPEND_SLOT)
                .context("the suspend slot is out of range")?;
            let ranges = state.write_suspend(&mut slot, pose)?;
            // A setter leaves the checksums stale: an image the game rejects is not a save.
            slot.seal();
            ranges
        };
        write_atomically(out, save.as_bytes())?;
        let changed = input
            .iter()
            .zip(save.as_bytes())
            .filter(|(before, after)| before != after)
            .count();
        status(&format!(
            "suspend: wrote {} (slot {}, map {}, story {}.{}.{}, {} member(s), {} gold, \
             position ({}, {}, {}) fx32, heading {}; {} byte(s) differ from the input; \
             {} field range(s))",
            out.display(),
            dq9_formats::save::SUSPEND_SLOT,
            state.map,
            state.scenario.chapter,
            state.scenario.step,
            state.scenario.substep,
            state.party.members.len(),
            state.party.gold,
            pose.position[0],
            pose.position[1],
            pose.position[2],
            pose.angle,
            changed,
            ranges.len(),
        ));
        Ok(state)
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

/// Writes `bytes` to `path` through a sibling temporary file and a rename: an interrupted
/// write cannot leave a truncated image under the requested name, and the input save (a
/// different path) is never touched.
#[cfg(not(target_arch = "wasm32"))]
fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let name = path
        .file_name()
        .with_context(|| format!("{} is not a file name", path.display()))?;
    let mut temp = path.to_path_buf();
    temp.set_file_name(format!("{}.tmp", name.to_string_lossy()));
    std::fs::write(&temp, bytes).with_context(|| format!("writing {}", temp.display()))?;
    std::fs::rename(&temp, path)
        .with_context(|| format!("renaming {} to {}", temp.display(), path.display()))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::story_of;
    use crate::load::MapEntry;
    use crate::screens::Layout;
    use dq9_core::state::DayClock;
    use dq9_formats::save::{FILE_SIZE, MAGIC, NO_ITEM};

    /// A normal save with a named hero, held items and one global flag.
    fn save_bytes() -> Vec<u8> {
        let mut data = vec![0u8; FILE_SIZE];
        data[..MAGIC.len()].copy_from_slice(MAGIC);
        data[21] = 1;
        let mut save = Save::parse(&data).unwrap();
        {
            let mut slot = save.slot_mut(0).unwrap();
            slot.set_area(109);
            slot.set_day_timer(42.5);
            slot.set_chapter(19);
            slot.set_scenario_step(2);
            slot.set_scenario_substep(1);
            slot.set_party_count(1);
            slot.set_gold_on_hand(38_754);
            {
                let mut flags = slot.event_flags_mut();
                flags.set_global_flag(9, true, false);
                // The progress words above are the active track's own copy.
                let mut track = flags.track_mut(0).unwrap();
                track.set_chapter(19);
                track.set_step(2);
                track.set_substep(1);
            }
            let mut hero = slot.character_mut(0).unwrap();
            hero.set_name_raw(&[18, 47, 48, 39]);
            hero.set_vocation(6);
            hero.set_level(6, 42).unwrap();
            hero.set_experience(6, 1234).unwrap();
            hero.set_equipment(dq9_formats::save::EquipSlot::Weapon, 20_004);
            slot.set_held_items(0, &[NO_ITEM; 8]).unwrap();
            slot.set_bag(&[(Some(1), 3), (None, 0)]).unwrap();
            slot.seal();
        }
        // The game writes a normal save twice (`Save_WriteStep` state 2): the input is a
        // mirrored image, like a save taken on the DS.
        save.mirror_normal_slot();
        save.as_bytes().to_vec()
    }

    fn options(save: Option<Vec<u8>>) -> crate::app::Options {
        crate::app::Options {
            map: None,
            save,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: story_of([None; 3], true, None),
            story_override: [None; 3],
            new_game: false,
            battle: None,
            play: Default::default(),
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("dq9-save-out-{name}-{}.sav", std::process::id()))
    }

    /// The state a run holds is what the written file reloads to, field for field.
    #[test]
    fn a_session_writes_the_state_it_holds() {
        let bytes = save_bytes();
        let base = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let mut app = App::new_headless(options(Some(bytes.clone())), None);
        // Without a ROM the save is not applied at start-up: put the run where a load would.
        app.party = base.party.clone();
        app.state = dq9_core::field::saved_state(&base);
        app.current = Some((
            MapEntry {
                id: 109,
                code: "C01M09".into(),
                byte_c: 0,
            },
            "C01M09".into(),
        ));
        app.day = DayClock::new(300.0, true);
        app.party.gold = 4321;
        app.party.bag.add(123);
        app.party.members[0].max_hp += 7;
        app.party.members[0].max_mp += 3;
        // The format restores a member at full HP and MP (`from_record`: `hp = max_hp`), so a
        // state that expects to reload equal must hold full values.
        app.party.members[0].hp = app.party.members[0].max_hp;
        app.party.members[0].mp = app.party.members[0].max_mp;
        app.state.flags.insert(11);
        app.state.tracks[0].chapter = 3;
        app.state.tracks[0].step = 2;
        let out = temp_path("held");
        let _ = std::fs::remove_file(&out);
        let written = app.write_save_out(&out).unwrap();
        let reloaded =
            GameState::from_save(&Save::parse(&std::fs::read(&out).unwrap()).unwrap()).unwrap();
        assert_eq!(reloaded, written);
        assert_eq!(reloaded.map, 109);
        assert_eq!(
            (
                reloaded.scenario.chapter,
                reloaded.scenario.step,
                reloaded.scenario.substep
            ),
            (3, 2, 1)
        );
        assert_eq!(reloaded.clock.seconds(), 300.0);
        assert_eq!(reloaded.party.gold, 4321);
        assert!(reloaded.events().global_flag(11) && reloaded.events().global_flag(9));
        let hero = &reloaded.party.members[0];
        assert_eq!(hero.name(), &[18, 47, 48, 39]);
        assert_eq!(hero.levels[6], 42);
        assert_eq!(hero.experience[6], 1234);
        assert_eq!(
            (hero.max_hp, hero.max_mp),
            (
                written.party.members[0].max_hp,
                written.party.members[0].max_mp
            )
        );
        assert_eq!(reloaded.party.bag.count(123), 1);
        assert!(reloaded.party.bag.count(123) == written.party.bag.count(123));
        // The temporary file of the atomic write is gone.
        assert!(!out.with_extension("sav.tmp").exists());
        let _ = std::fs::remove_file(&out);
    }

    /// Every byte the write changes lies in the checksummed slot block or a checksum word,
    /// and its mirror copy in slot 1: the opaque blocks elsewhere in the image do not move.
    #[test]
    fn a_written_save_changes_only_the_slot_it_updates() {
        use dq9_formats::save::{Checksum, SLOT_SIZE};
        let bytes = save_bytes();
        let mut app = App::new_headless(options(Some(bytes.clone())), None);
        let base = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        app.party = base.party.clone();
        app.state = dq9_core::field::saved_state(&base);
        app.current = Some((
            MapEntry {
                id: 1100,
                code: "M01M12".into(),
                byte_c: 0,
            },
            "M01M12".into(),
        ));
        app.party.gold = 7;
        app.party.bag.add(5);
        let (state, index) = app.save_state().unwrap();
        // What the binding says it owns, measured on a copy of the input (checksums stale).
        let mut probe = Save::parse(&bytes).unwrap();
        let ranges = {
            let mut slot = probe.slot_mut(index).unwrap();
            state.write_into(&mut slot).unwrap()
        };
        let out = temp_path("confined");
        let _ = std::fs::remove_file(&out);
        app.write_save_out(&out).unwrap();
        let written = std::fs::read(&out).unwrap();
        let checksums: Vec<std::ops::Range<usize>> = Checksum::ALL
            .iter()
            .map(|which| which.stored_at()..which.stored_at() + 4)
            .collect();
        let mut changed = 0;
        for (index, (before, after)) in bytes.iter().zip(&written).enumerate() {
            if before == after {
                continue;
            }
            changed += 1;
            let in_slot = index % SLOT_SIZE;
            // Slot 0's written ranges and checksums; slot 1 as the mirror of slot 0's `+0x10`,
            // which copies each byte to the same slot offset.
            let mirrored = index >= SLOT_SIZE
                && in_slot >= 0x10
                && ranges
                    .iter()
                    .chain(&checksums)
                    .any(|range| range.contains(&in_slot));
            assert!(
                mirrored
                    || (index < SLOT_SIZE
                        && ranges
                            .iter()
                            .chain(&checksums)
                            .any(|range| range.contains(&in_slot))),
                "byte {in_slot} of slot {} changed outside the written ranges",
                index / SLOT_SIZE
            );
        }
        assert!(changed > 0, "the mutation changed nothing");
        let _ = std::fs::remove_file(&out);
    }

    /// The Quick Save's write: the **live** pose goes into slot 1's `Warp` block with the
    /// intermission header, slot 0 keeps every byte, and the file resumes at that pose.
    #[test]
    fn a_suspend_write_keeps_the_normal_slot_and_resumes_at_the_live_pose() {
        use dq9_formats::save::{SLOT_SIZE, SlotStatus};
        let bytes = save_bytes();
        let base = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let mut app = App::new_headless(options(Some(bytes.clone())), None);
        app.party = base.party.clone();
        app.state = dq9_core::field::saved_state(&base);
        app.current = Some((
            MapEntry {
                id: 109,
                code: "C01M09".into(),
                byte_c: 0,
            },
            "C01M09".into(),
        ));
        app.day = DayClock::new(300.0, true);
        app.state.flags.insert(11);
        // The run walked away from where the input save put the party: the suspend save must
        // carry this pose, not the base state's placement.
        let hero = dq9_core::Hero::at(Vec3::new(1.5, 0.0, -2.25), 0.5);
        app.hero = Some(hero);
        let out = temp_path("suspend");
        let _ = std::fs::remove_file(&out);
        let written = app.write_suspend_out(&out).unwrap();
        let file = std::fs::read(&out).unwrap();
        assert_eq!(
            file[..SLOT_SIZE],
            bytes[..SLOT_SIZE],
            "the normal slot is untouched"
        );
        let save = Save::parse(&file).unwrap();
        let slot = save.slot(dq9_formats::save::SUSPEND_SLOT).unwrap();
        assert_eq!(slot.status(), SlotStatus::Intermission);
        assert!(slot.checksums_valid(), "the written slot is sealed");
        let pose = dq9_core::state::HeroPose {
            position: dq9_core::field::to_fx(hero.position()),
            angle: (dq9_core::field::angle_of_yaw(hero.yaw()) * 4096.0) as i16,
        };
        assert_eq!(
            slot.warp(),
            Some(dq9_formats::save::Warp {
                map: 109,
                angle: pose.angle,
                position: pose.position,
            })
        );
        let resumed = GameState::from_slot(&slot).unwrap();
        assert_eq!(resumed.map, 109);
        assert_eq!(resumed.scenario, written.scenario);
        assert_eq!(resumed.party, written.party);
        assert_eq!(resumed.clock.seconds(), 300.0);
        assert_eq!(resumed.events().as_bytes(), written.events().as_bytes());
        assert_eq!(resumed.placement, dq9_core::state::Placement::At(pose));
        assert!(!out.with_extension("sav.tmp").exists());
        let _ = std::fs::remove_file(&out);
    }

    /// A container that holds only the quick save log: slot 0's header A is empty (a resumed
    /// log's deletion, or a cartridge that never confessed) and slot 1 is the suspend save,
    /// with two markers in blocks `write_into` does not carry (the bank and the medals).
    fn suspend_only_bytes() -> Vec<u8> {
        let mut save = Save::parse(&save_bytes()).unwrap();
        {
            let mut slot = save.slot_mut(0).unwrap();
            slot.clear_header();
            slot.seal();
        }
        {
            let mut slot = save.slot_mut(dq9_formats::save::SUSPEND_SLOT).unwrap();
            slot.set_intermission(true);
            slot.set_warp(dq9_formats::save::Warp {
                map: 109,
                angle: 0,
                position: [0; 3],
            });
            slot.set_gold_in_bank(777);
            slot.set_mini_medals(12);
            slot.seal();
        }
        save.as_bytes().to_vec()
    }

    /// A headless [`App`] parked where the input save resumes (no ROM, so start-up does not
    /// apply the save itself): the state, party and map of its resumable slot.
    fn resumed_app(bytes: &[u8]) -> App {
        let base = GameState::from_save(&Save::parse(bytes).unwrap()).unwrap();
        let mut app = App::new_headless(options(Some(bytes.to_vec())), None);
        app.party = base.party.clone();
        app.state = dq9_core::field::saved_state(&base);
        app.current = Some((
            MapEntry {
                id: base.map,
                code: "C01M09".into(),
                byte_c: 0,
            },
            "C01M09".into(),
        ));
        app.day = DayClock::new(300.0, true);
        app
    }

    /// The batch's case: a suspend-only container, a run, `--save-out`. The write is a
    /// **normal** save into the empty slot 0 (`Save_WriteStep` state 1's destination), seeded
    /// from the suspend image (state 0) and mirrored to slot 1 (state 2); the reader takes
    /// slot 0 and restores the state the session held. A normal save carries no pose, so the
    /// party resumes at the map's start record, as the original's does.
    #[test]
    fn a_suspend_only_container_writes_a_normal_slot_that_reloads() {
        use dq9_formats::save::{SLOT_SIZE, SlotStatus};
        let bytes = suspend_only_bytes();
        let mut app = resumed_app(&bytes);
        app.party.gold = 4321;
        app.state.flags.insert(11);
        app.state.tracks[0].chapter = 3;
        app.state.tracks[0].step = 2;
        let out = temp_path("suspend-only");
        let _ = std::fs::remove_file(&out);
        let written = app.write_save_out(&out).unwrap();
        let file = std::fs::read(&out).unwrap();
        assert_eq!(
            file[..16],
            bytes[..16],
            "the first 16 bytes are never written"
        );
        assert_eq!(
            &file[27104..27132],
            &bytes[SLOT_SIZE + 27104..SLOT_SIZE + 27132],
            "a normal save does not write the Warp block"
        );
        let save = Save::parse(&file).unwrap();
        let slot = save.slot(0).unwrap();
        assert_eq!(slot.status(), SlotStatus::Valid, "a normal adventure log");
        assert!(slot.checksums_valid());
        assert_eq!(slot.warp(), None);
        assert_eq!(
            slot.gold_in_bank(),
            777,
            "the opaque block came from slot 1"
        );
        assert_eq!(slot.mini_medals(), 12);
        // `Save_WriteStep` state 2 copies slot 0's 0x10..0x6ff4 (`MIRROR_LEN`), not the whole
        // slot: the first 16 bytes on each side and the tail past it are never written.
        let mirror = 16..16 + 0x6fe4;
        assert_eq!(
            &save.slot(1).unwrap().bytes()[mirror.clone()],
            &slot.bytes()[mirror],
            "slot 1 is the mirror copy"
        );
        assert_eq!(save.slot(1).unwrap().status(), SlotStatus::Valid);
        let reloaded = GameState::from_save(&save).unwrap();
        assert_eq!(reloaded, written);
        assert_eq!(reloaded.map, 109);
        assert_eq!(reloaded.placement, Placement::MapStart);
        assert_eq!(reloaded.party.gold, 4321);
        assert_eq!(reloaded.clock.seconds(), 300.0);
        assert_eq!(
            (
                reloaded.scenario.chapter,
                reloaded.scenario.step,
                reloaded.scenario.substep
            ),
            (3, 2, 1)
        );
        assert!(reloaded.events().global_flag(11));
        // A second write of the same session is the same image.
        let again = temp_path("suspend-only-again");
        let _ = std::fs::remove_file(&again);
        app.write_save_out(&again).unwrap();
        assert_eq!(
            std::fs::read(&again).unwrap(),
            file,
            "a stable second write"
        );
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&again);
    }

    /// A container with both slots keeps today's behaviour: the normal log is the target and
    /// the suspend image is not mixed into it (slot 1 becomes the mirror, so its own markers
    /// are gone).
    #[test]
    fn a_normal_slot_is_updated_without_the_suspend_image() {
        let mut save = Save::parse(&save_bytes()).unwrap();
        save.slot_mut(0).unwrap().set_gold_in_bank(111);
        save.slot_mut(0).unwrap().seal();
        {
            let mut slot = save.slot_mut(dq9_formats::save::SUSPEND_SLOT).unwrap();
            slot.set_intermission(true);
            slot.set_gold_in_bank(777);
            slot.seal();
        }
        let bytes = save.as_bytes().to_vec();
        let mut app = resumed_app(&bytes);
        app.party.gold = 5;
        let out = temp_path("both-slots");
        let _ = std::fs::remove_file(&out);
        app.write_save_out(&out).unwrap();
        let save = Save::parse(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(save.slot(0).unwrap().gold_in_bank(), 111);
        assert_eq!(
            save.slot(1).unwrap().gold_in_bank(),
            111,
            "the mirror copy of slot 0"
        );
        assert_eq!(save.slot(0).unwrap().gold_on_hand(), 5);
        let _ = std::fs::remove_file(&out);
    }

    /// Without `--save` the suspend write refuses too: there is no container to mark.
    #[test]
    fn a_suspend_write_without_an_input_save_is_refused() {
        let mut app = App::new_headless(options(None), None);
        app.hero = Some(dq9_core::Hero::at(Vec3::ZERO, 0.0));
        let out = temp_path("suspend-refused");
        let error = app.write_suspend_out(&out).unwrap_err().to_string();
        assert!(error.contains("--suspend-out needs --save"), "{error}");
        assert!(!out.exists());
    }

    /// Without `--save` there is no container to update: the write refuses instead of
    /// inventing the opaque blocks of a fresh one.
    #[test]
    fn writing_without_an_input_save_is_refused() {
        let app = App::new_headless(options(None), None);
        let out = temp_path("refused");
        let error = app.write_save_out(&out).unwrap_err().to_string();
        assert!(error.contains("--save-out needs --save"), "{error}");
        assert!(!out.exists());
    }

    /// A real save, a real map and a played run: the written file reloads to the very state
    /// the run held (map, story, party with its derived HP/MP, gold, clock, flags).
    #[test]
    fn a_real_rom_headless_run_writes_a_save_that_reloads_equal() {
        let (Some(rom), Some(path)) = (testrom::load(), std::env::var_os("DQ9_SAVE")) else {
            eprintln!("skipping: no ROM or DQ9_SAVE");
            return;
        };
        let bytes = std::fs::read(&path).unwrap();
        let mut app = App::new_headless(options(Some(bytes.clone())), Some(rom.to_vec()));
        for _ in 0..30 {
            app.walk_step(Vec3::new(0.0, 0.0, 1.0));
        }
        // A played session mutates the party: gold, the bag and a member's stored maximum.
        app.party.bag.add(123);
        app.party.members[0].max_hp += 5;
        // A normal save restores a member at full HP: the current value is not stored.
        app.party.members[0].hp = app.party.members[0].max_hp;
        let (map, _, _, _) = app.walk_pose().expect("the save's map is loaded");
        let out = temp_path("rom");
        let _ = std::fs::remove_file(&out);
        let written = app.write_save_out(&out).unwrap();
        let reloaded =
            GameState::from_save(&Save::parse(&std::fs::read(&out).unwrap()).unwrap()).unwrap();
        assert_eq!(reloaded, written);
        assert_eq!(reloaded.map, map);
        assert_eq!(reloaded.party.members.len(), app.party.members.len());
        assert_eq!(reloaded.party.gold, app.party.gold);
        assert_eq!(reloaded.clock.seconds(), app.day.seconds());
        for (saved, live) in reloaded.party.members.iter().zip(&app.party.members) {
            assert_eq!(saved.name, live.name);
            assert_eq!(saved.vocation, live.vocation);
            assert_eq!(saved.levels, live.levels);
            assert_eq!(saved.experience, live.experience);
            assert_eq!(saved.equipment, live.equipment);
            // HP/MP are restored at full by the load; the record stores the maxima
            // (`+0x14c` fields 7/8), which the binding writes, so the reload must find the
            // run's own maxima.
            assert_eq!((saved.max_hp, saved.max_mp), (live.max_hp, live.max_mp));
        }
        assert_eq!(reloaded.party.bag.count(123), 1);
        // The input save is never modified.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let _ = std::fs::remove_file(&out);
    }
}
