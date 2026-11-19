//! Recording a human session and replaying a record (`record.rs`, `docs/app/record.md`): the
//! input log, the state digest, the start of a fixed step and the page's recording commands.

use super::App;
#[cfg(target_arch = "wasm32")]
use super::options::Options;
use super::status;
use crate::record;
use crate::record::Input as Logged;
use crate::record::Recorder;
use crate::record::Replay;
use glam::Vec2;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
use winit::keyboard::KeyCode;

/// State of a recorded or replayed session.
pub(super) struct Recording {
    /// The human session being recorded (`--record`).
    pub(super) recorder: Option<Recorder>,
    /// The record being replayed (`--play-record`): it feeds the logic in place of the window.
    pub(super) replay: Option<Replay>,
    /// A recorded jingle end waits for the next sound step.
    pub(super) jingle_ended: bool,
    /// Pad bits a replay frame asks for, consumed by one fixed step.
    pub(super) replay_pad: Option<u8>,
    /// Pad bits and action key of every fixed step (`--trace-inputs`).
    pub(super) input_trace: Vec<(u8, bool)>,
    /// `?record=1` in the page: the next ROM read starts a recording of the session.
    #[cfg(target_arch = "wasm32")]
    pub(super) record_wanted: bool,
    /// A `.dq9rec` picked before the ROM (`?play-record=1`): replayed once the ROM is read.
    #[cfg(target_arch = "wasm32")]
    pub(super) pending_replay: Option<record::Record>,
}

impl Recording {
    pub(super) fn new() -> Self {
        Self {
            recorder: None,
            replay: None,
            jingle_ended: false,
            replay_pad: None,
            input_trace: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            record_wanted: false,
            #[cfg(target_arch = "wasm32")]
            pending_replay: None,
        }
    }
}

impl App {
    /// Records an input of the window at the step count it was handled at.
    pub(super) fn log_input(&mut self, input: Logged) {
        self.log_input_at(self.steps, input);
    }

    /// Records a key the logic reads. From the field Escape ends the session natively (not an
    /// input) and does nothing on the page: neither its press nor its release is recorded, so a
    /// record holds no unpaired line (`docs/app/record.md`).
    pub(super) fn log_key(&mut self, code: KeyCode, pressed: bool) {
        if code == KeyCode::Escape && self.combat.fight.is_none() && !self.services.is_open() {
            return;
        }
        self.log_input(Logged::Key { key: code, pressed });
    }

    pub(super) fn log_input_at(&mut self, step: u64, input: Logged) {
        if let Logged::Key { key, .. } = input
            && record::key_name(key).is_none()
        {
            return; // a key the logic never reads (mute, layout, ...)
        }
        if let Some(recorder) = &mut self.recording.recorder {
            recorder.input(step, input);
        }
    }

    /// An input of a replayed record, handled as the window handled it.
    fn apply_input(&mut self, input: Logged) {
        match input {
            Logged::Key { key, pressed } => {
                self.key(key, pressed);
            }
            Logged::FocusLost => self.controls.held.clear(),
            Logged::JingleEnded => self.recording.jingle_ended = true,
            Logged::Mouse { rotate, zoom } => {
                self.controls.rotate += Vec2::from(rotate);
                self.controls.zoom += zoom;
            }
        }
    }

    /// What a record's digest holds: where the story, the hero, the party and the generators
    /// are. A replay that drifts is caught at the first digest that differs.
    pub(crate) fn state_digest(&self) -> record::Digest {
        let map = self.current.as_ref().map_or(0, |(entry, _)| entry.id);
        let track = &self.state.tracks[self.state.track];
        let hero = self.hero.map_or_else(
            || "none".to_owned(),
            |hero| {
                let at = hero.position();
                format!(
                    "{:08x}:{:08x}:{:08x}:{:08x}",
                    at.x.to_bits(),
                    at.y.to_bits(),
                    at.z.to_bits(),
                    hero.yaw().to_bits()
                )
            },
        );
        let party = self
            .party
            .members
            .iter()
            .map(|m| format!("{}/{}/{}/{}", m.level(), m.hp, m.mp, m.current_experience()))
            .collect::<Vec<_>>()
            .join(",");
        let rng = record::fnv64(format!("{:?}", self.combat.world).as_bytes());
        let pair = |name: &str, value: String| (name.to_owned(), value);
        vec![
            pair("map", map.to_string()),
            pair(
                "story",
                format!("{}.{}.{}", track.chapter, track.step, track.substep),
            ),
            pair("tick", self.tick.to_string()),
            pair("hero", hero),
            pair(
                "party",
                if party.is_empty() {
                    "none".into()
                } else {
                    party
                },
            ),
            pair("gold", self.party.gold.to_string()),
            pair("rng", format!("{rng:016x}")),
        ]
    }

    /// The start of a fixed step: the replay's inputs of this step, the digest of the state
    /// the step starts from, then the step is counted.
    pub(super) fn step_begin(&mut self) {
        let step = self.steps;
        self.replay_inputs(step);
        if self.recording.recorder.is_some() && step.is_multiple_of(record::DIGEST_EVERY) {
            let digest = self.state_digest();
            if let Some(recorder) = &mut self.recording.recorder {
                recorder.digest(step, digest);
            }
        }
        // The page shows how much it has recorded (`docs/app/record.md`).
        #[cfg(target_arch = "wasm32")]
        if self.recording.recorder.is_some() && step.is_multiple_of(record::PROGRESS_EVERY) {
            let seed = self
                .recording
                .recorder
                .as_ref()
                .map_or(0, |r| r.header().seed);
            crate::web::recording_progress(seed, step);
        }
        self.replay_check(step);
        self.steps += 1;
    }

    /// Hands the replayed record's inputs due at `step` to the logic, as the window would.
    pub(crate) fn replay_inputs(&mut self, step: u64) {
        while let Some(input) = self
            .recording
            .replay
            .as_mut()
            .and_then(|r| r.next_input(step))
        {
            self.apply_input(input);
        }
    }

    /// Compares the state at `step` with the replayed record's digest, when one is due.
    pub(crate) fn replay_check(&mut self, step: u64) {
        if self
            .recording
            .replay
            .as_ref()
            .is_some_and(|r| r.digest_due(step))
        {
            let digest = self.state_digest();
            if let Some(replay) = &mut self.recording.replay {
                replay.check(step, &digest);
            }
        }
    }

    /// Starts recording this session (`--record`).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn start_recording(&mut self, recorder: Recorder) {
        self.recording.recorder = Some(recorder);
    }

    /// Ends the recording: the closing digest, then the record. `None` without a recording.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn finish_recording(&mut self) -> Option<record::Record> {
        let mut recorder = self.recording.recorder.take()?;
        recorder.digest(self.steps, self.state_digest());
        let record = recorder.finish(self.steps).clone();
        status(&format!("record: {} fixed steps", self.steps));
        Some(record)
    }

    /// Feeds the logic from `record` instead of the window (`--play-record`).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn start_replay(&mut self, record: record::Record) {
        self.recording.replay = Some(Replay::new(record));
    }

    /// Starts recording the page's session (`?record=1`), with the options it started from.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn start_page_recording(&mut self) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let Ok((code, version, crc)) = record::Header::rom_identity(rom) else {
            return;
        };
        // The page's loop runs steps before the ROM is read: the record's first step must be
        // the loaded session's first, as natively (`App::new` starts both counters at 0).
        self.restart_clock();
        let header = record::Header {
            rom: code,
            rom_version: version,
            rom_crc: crc,
            new_game: self.options.new_game,
            seed: self.options.play.seed.unwrap_or_default(),
            map: self.options.map.clone(),
            save: self.options.save.as_deref().map(record::Header::save_hash),
            time: self.options.time,
            clock: self.options.clock,
            chapter: self.options.story_override[0],
            step: self.options.story_override[1],
            substep: self.options.story_override[2],
            night: !self.options.story.daytime,
            stage: None,
            event: self.options.event,
            battles: "show".to_owned(),
        };
        self.recording.recorder = Some(Recorder::new(header));
    }

    /// The page's "Save recording": the record so far, ended at this step with a closing
    /// digest, handed to the page as a download. The recording goes on.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn save_page_record(&mut self) {
        let digest = self.state_digest();
        let snapshot = self
            .recording
            .recorder
            .as_ref()
            .map(|recorder| recorder.snapshot(self.steps, digest));
        let Some(record) = snapshot else {
            status("record: this session is not being recorded");
            return;
        };
        crate::web::offer_record(
            &format!("dq9-{}-{}.dq9rec", record.header.seed, record.end),
            &record.to_text(),
        );
    }

    /// Replays a `.dq9rec` picked in the page (`?play-record=1`): its start options replace the
    /// page's, and the session starts over so the record's steps are the first ones again.
    /// `rom` is the ROM the page queued with the record (picked before it); else the one the
    /// page already loaded is loaded again under the record's options.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn start_page_replay(&mut self, record: record::Record, rom: Option<Vec<u8>>) {
        let rom = rom.or_else(|| self.rom.take());
        if let Some(rom) = rom {
            if let Err(e) = record
                .header
                .check_start(&rom, self.options.save.as_deref())
            {
                // The page's own session goes on: its ROM is loaded, without a replay.
                status(&format!("record: {e}"));
                self.set_rom(rom);
                return;
            }
            self.adopt_record_options(&record.header);
            self.set_rom(rom);
        } else {
            self.adopt_record_options(&record.header);
        }
        let end = record.end;
        self.start_replay(record);
        status(&format!(
            "replay: {end} recorded steps; the keyboard only closes the window (M mutes, R changes the layout)"
        ));
    }

    /// The start options a record names replace the page's: the map it started on (or its new
    /// game), its seed, its time of day, its story point and its save. The session's clock
    /// starts over with them, so the record's step 0 is the session's.
    #[cfg(target_arch = "wasm32")]
    fn adopt_record_options(&mut self, header: &record::Header) {
        let (layout, mute) = (self.options.layout, self.options.mute);
        self.options = Options::from_record(header, self.options.save.take(), layout, mute);
        self.restart_clock();
        self.recording.replay = None;
    }

    /// Starts the fixed step count and the free-running tick over, between two frames: a
    /// record (made here or natively) counts its steps from the session's first one, while the
    /// page's loop has already run steps before the ROM was read.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn restart_clock(&mut self) {
        self.steps = 0;
        self.tick = 0;
        self.view.clock_ms = 0.0;
        self.accumulator = 0.0;
        self.last = Instant::now();
    }
}

#[cfg(all(test, windows))]
mod tests;
