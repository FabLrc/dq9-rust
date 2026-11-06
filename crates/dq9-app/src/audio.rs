//! Sound output of the application: the music of the map (`dq9_formats::bgm`), the jingles
//! and the sound effects the scripts and the field ask for, all played by the `dq9_audio`
//! mixer that a platform stream pulls from: cpal natively, a WebAudio `ScriptProcessorNode`
//! on the Web (the context is made and resumed on the first user gesture, which browsers
//! require). `--mute` (`?mute=1` on the Web) opens no device at all; `M` mutes and unmutes.
//!
//! What the game's sound manager does (`ov_001::02163564..02163a5c`, `0x0209c2f0..0x0209ca3c`,
//! `docs/re/sdat.md`): one music handle, one jingle handle, 16 effect handles, the base
//! sequence archive of `se_norm.sdat` and two archives a script loads.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use dq9_audio::{Handle, Mixer, Song};
use dq9_core::script::Sound;
use dq9_formats::Table;
use dq9_formats::bgm::{self, Overrides};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::sdat::{KIND_PLAYER, Sdat};
use nds_rom::Rom;

use crate::app::status;

/// Path of the music archive in the ROM filesystem.
const BGM_SDAT: &str = "data/sound/bgm.sdat";
/// The field's effects archive.
const SE_SDAT: &str = "data/sound/se_norm.sdat";
/// Sequence archive of `se_norm.sdat` mounted as the base of the field
/// (`ov_001::02163a5c`, native 734 mode 0: "ID 100").
const BASE_ARCHIVE: usize = 100;
/// Seconds an old track takes to fade out when the map changes its music.
const FADE_SECONDS: f32 = 0.4;
/// Songs kept decoded: the maps one walks between share few tracks.
const CACHE: usize = 6;
/// Effect archives kept decoded.
const EFFECT_CACHE: usize = 8;
/// Handles of the scripts' effects (natives 728, 732).
const HANDLES: usize = 16;
/// The game's durations count 1/60 s (`GetEffectiveDeltaTime` is in milliseconds for the
/// jingle's waits, the fades of the scripts are in frames **[hypothèse]**).
const TICK: f32 = 1.0 / 60.0;
/// Ticks the music takes to fade before a jingle (`0x0209c850`: 20 frames), the wait before
/// the jingle starts (800 ms), the wait after it ends (500 ms) and the fade back in (30
/// frames).
const JINGLE_FADE: u32 = 20;
const JINGLE_LEAD: u32 = 48;
const JINGLE_TAIL: u32 = 30;
const JINGLE_RETURN: u32 = 30;
/// Sample rate of a recording: the DS mixer's 32 728 Hz.
pub const RECORD_RATE: u32 = 32_728;

/// The sounds the field itself plays, by index in the base archive (`se_norm.sdat` archive
/// 100). Where each is found is in `docs/re/sdat.md`.
pub mod se {
    /// A typed character of the message window, by the voice a script selects (native 554):
    /// `0x020455f8`.
    pub const TEXT_BLIP: [u16; 3] = [10, 12, 11];
    /// `<EXC>` (`0xFF2E`) and `<QES>` (`0xFF2F`) of a message: `0x02065990`.
    pub const EXCLAMATION: u16 = 6;
    pub const QUESTION: u16 = 28;
    /// A choice (`<YESNO>`, `<UKEYAME>`...) opens: `0x02065990`.
    pub const CHOICE_OPEN: u16 = 5;
}

/// What the stream callback mixes.
struct Engine {
    rate: u32,
    muted: bool,
    mixer: Mixer,
}

impl Engine {
    fn fill(&mut self, out: &mut [f32]) {
        if self.muted {
            out.fill(0.0);
            return;
        }
        self.mixer.render(out);
    }
}

type Shared = Arc<Mutex<Engine>>;

fn lock(engine: &Shared) -> MutexGuard<'_, Engine> {
    // A poisoned lock only means a callback panicked; the engine state is still usable.
    engine.lock().unwrap_or_else(|e| e.into_inner())
}

/// The effects of a sequence archive, by entry.
type Effects = Arc<Vec<Option<Arc<Song>>>>;

/// Which of the game's two music paths is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Jingle {
    /// The music fades, then the jingle starts (native 720).
    Lead { sequence: u8, left: u32 },
    /// The jingle sounds; the music waits paused when `resume`.
    Playing { handle: Handle, resume: bool },
    /// The jingle ended; the music comes back after `left` ticks.
    Tail { left: u32, resume: bool },
}

/// The application's sound.
pub struct Audio {
    engine: Shared,
    backend: Option<backend::Backend>,
    /// The device could not be opened: not tried again.
    failed: bool,
    /// Sequence asked for by the current map (`None`: silence).
    wanted: Option<u8>,
    /// Sequence the music is on and its handle.
    playing: Option<(u8, Handle)>,
    /// Sequence a script selected (natives 713, 714).
    selected: Option<u8>,
    /// Native 735: the music ignores selecting, restarting and stopping.
    locked: bool,
    /// Host volume of the music, 0.0 to 1.0 (native 715).
    volume: f32,
    jingle: Option<Jingle>,
    /// The jingle played out since the last [`Audio::take_jingle_ended`].
    jingle_ended: bool,
    /// Text blip of the message window (native 554).
    voice: usize,
    songs: HashMap<u8, Arc<Song>>,
    order: Vec<u8>,
    /// Decoded effect archives of `se_norm.sdat`, oldest first.
    effects: Vec<(usize, Effects)>,
    /// Archives the scripts loaded: native 726 and native 730.
    loaded: [Option<Effects>; 2],
    /// Polyphony of the players of the archives read so far.
    limits: Vec<(u8, u16)>,
    /// The mix of the steps so far, when recording ([`Audio::record`]).
    recording: Option<Vec<f32>>,
    record_carry: f64,
    /// What the scripts' handles 0 to 15 sound.
    handles: [Option<Handle>; HANDLES],
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("wanted", &self.wanted)
            .field("open", &self.backend.is_some())
            .finish_non_exhaustive()
    }
}

/// The polyphony of the players of `sdat`: player index and sequences at once.
fn player_limits(sdat: &Sdat<'_>) -> Vec<(u8, u16)> {
    (0..sdat.list_len(KIND_PLAYER))
        .filter_map(|index| {
            let player = sdat.player(index)?;
            Some((u8::try_from(index).ok()?, player.max_sequences))
        })
        .collect()
}

impl Audio {
    /// `mute` starts silent and does not touch the sound device until unmuted.
    pub fn new(mute: bool) -> Self {
        Self {
            engine: Arc::new(Mutex::new(Engine {
                rate: 48_000,
                muted: mute,
                mixer: Mixer::new(48_000),
            })),
            backend: None,
            failed: false,
            wanted: None,
            playing: None,
            selected: None,
            locked: false,
            volume: 1.0,
            jingle: None,
            jingle_ended: false,
            voice: 0,
            songs: HashMap::new(),
            order: Vec::new(),
            effects: Vec::new(),
            loaded: [None, None],
            limits: Vec::new(),
            recording: None,
            record_carry: 0.0,
            handles: [None; HANDLES],
        }
    }

    pub fn muted(&self) -> bool {
        lock(&self.engine).muted
    }

    /// Whether anything is heard now: a device is open and the sound is not muted, or the
    /// sound is being recorded.
    fn heard(&self) -> bool {
        self.recording.is_some() || (self.backend.is_some() && !self.muted())
    }

    /// Like [`Audio::heard`], opening the device first if it is not and could be.
    fn audible(&mut self) -> bool {
        if !self.muted() {
            self.open();
        }
        self.heard()
    }

    /// Renders offline instead of to a device: every [`Audio::step`] mixes 1/60 s at
    /// [`RECORD_RATE`] into a buffer [`Audio::take_recording`] hands out (`--sound-wav`).
    pub fn record(&mut self) {
        self.failed = true;
        let mut engine = lock(&self.engine);
        engine.rate = RECORD_RATE;
        engine.muted = false;
        engine.mixer = Mixer::new(RECORD_RATE);
        drop(engine);
        self.recording = Some(Vec::new());
    }

    /// What was recorded since [`Audio::record`], as interleaved stereo at [`RECORD_RATE`].
    pub fn take_recording(&mut self) -> Option<Vec<f32>> {
        self.recording.take()
    }

    fn record_step(&mut self) {
        let Some(buffer) = &mut self.recording else {
            return;
        };
        self.record_carry += f64::from(RECORD_RATE) / 60.0;
        let frames = self.record_carry.floor();
        self.record_carry -= frames;
        let start = buffer.len();
        buffer.resize(start + frames as usize * 2, 0.0);
        lock(&self.engine).fill(&mut buffer[start..]);
    }

    /// Mutes or unmutes (the sounds keep their place while muted).
    pub fn toggle_mute(&mut self) {
        let muted = {
            let mut engine = lock(&self.engine);
            engine.muted = !engine.muted;
            engine.muted
        };
        status(if muted { "sound muted" } else { "sound on" });
        if !muted {
            self.open();
        }
    }

    /// Takes the polyphony limits of the players of `sdat` into the mixer.
    fn register_players(&mut self, sdat: &Sdat<'_>) {
        let mut engine = lock(&self.engine);
        for (player, max) in player_limits(sdat) {
            engine.mixer.set_player_limit(player, max);
            self.limits.retain(|&(p, _)| p != player);
            self.limits.push((player, max));
        }
    }

    // --- The music of the map ---------------------------------------------------------------

    /// Chooses the music of map `map` at time of day `time` (`bgm::sequence_for_map`; the
    /// story flags are the trigger state's) and starts it unless it is already the one
    /// playing: a map change inside a place keeps its tune.
    pub fn set_map(&mut self, rom: &[u8], map: u16, time: u32, flags: &dyn Fn(u16) -> bool) {
        let sequence = match sequence_for_map(rom, map, time, flags) {
            Ok(sequence) => sequence,
            Err(e) => {
                status(&format!("music: {e:#}"));
                return;
            }
        };
        self.wanted = sequence;
        if self.locked || self.playing.map(|(s, _)| s) == sequence {
            return;
        }
        self.start_music(rom, sequence, FADE_SECONDS);
    }

    /// Replaces the music by `sequence` (`None`: silence), the old one fading over `fade`
    /// seconds.
    fn start_music(&mut self, rom: &[u8], sequence: Option<u8>, fade: f32) {
        let song = match sequence {
            None => None,
            Some(index) => match self.song(rom, index) {
                Ok(song) => Some(song),
                Err(e) => {
                    status(&format!("music: sequence {index}: {e}"));
                    None
                }
            },
        };
        status(&format!(
            "music: {}",
            song.as_ref().map_or_else(
                || "none".to_owned(),
                |s| { format!("{} ({})", s.name.as_deref().unwrap_or("?"), s.index) }
            )
        ));
        let volume = self.volume;
        let mut engine = lock(&self.engine);
        if let Some((_, old)) = self.playing.take() {
            engine.mixer.stop(old, fade);
        }
        if let Some(song) = song
            && let Some(handle) = engine.mixer.play(Arc::clone(&song))
        {
            engine.mixer.set_volume(handle, volume);
            self.playing = Some((song.index as u8, handle));
        }
        drop(engine);
        if !self.muted() {
            self.open();
        }
    }

    /// The music stops, fading over `seconds`.
    fn stop_music(&mut self, seconds: f32) {
        if let Some((_, handle)) = self.playing.take() {
            lock(&self.engine).mixer.stop(handle, seconds);
        }
    }

    fn song(&mut self, rom: &[u8], index: u8) -> Result<Arc<Song>, nds_rom::Error> {
        if let Some(song) = self.songs.get(&index) {
            return Ok(Arc::clone(song));
        }
        let rom = Rom::parse(rom)?;
        let sdat = Sdat::parse(rom.file_by_path(BGM_SDAT)?)?;
        self.register_players(&sdat);
        let song = Arc::new(Song::load(&sdat, usize::from(index))?);
        if self.order.len() >= CACHE {
            let old = self.order.remove(0);
            self.songs.remove(&old);
        }
        self.order.push(index);
        self.songs.insert(index, Arc::clone(&song));
        Ok(song)
    }

    // --- Effects ----------------------------------------------------------------------------

    /// The decoded effects of sequence archive `archive` of `se_norm.sdat`.
    fn effects(&mut self, rom: &[u8], archive: usize) -> Option<Effects> {
        if let Some((_, effects)) = self.effects.iter().find(|(a, _)| *a == archive) {
            return Some(Arc::clone(effects));
        }
        let mut load = || -> Result<Effects, nds_rom::Error> {
            let rom = Rom::parse(rom)?;
            let sdat = Sdat::parse(rom.file_by_path(SE_SDAT)?)?;
            self.register_players(&sdat);
            Ok(Arc::new(
                Song::load_effects(&sdat, archive)?
                    .into_iter()
                    .map(|song| song.map(Arc::new))
                    .collect(),
            ))
        };
        match load() {
            Ok(effects) => {
                if self.effects.len() >= EFFECT_CACHE {
                    self.effects.remove(0);
                }
                self.effects.push((archive, Arc::clone(&effects)));
                Some(effects)
            }
            Err(e) => {
                status(&format!("sound effects {archive}: {e}"));
                None
            }
        }
    }

    /// Starts effect `index` of `effects`. `None` when nothing sounds: muted, no device, an
    /// empty slot or a full player.
    fn start_effect(&mut self, effects: &Effects, index: usize) -> Option<Handle> {
        if !self.audible() {
            return None;
        }
        let song = effects.get(index)?.as_ref()?;
        lock(&self.engine).mixer.play(Arc::clone(song))
    }

    /// Effect `index` of the base archive: what the field plays by itself (`se::*`).
    pub fn play_base_effect(&mut self, rom: &[u8], index: u16) {
        if !self.audible() {
            return;
        }
        if let Some(effects) = self.effects(rom, BASE_ARCHIVE) {
            self.start_effect(&effects, usize::from(index));
        }
    }

    /// The voice of the message window's blip (native 554).
    pub fn set_text_voice(&mut self, voice: i32) {
        self.voice = usize::try_from(voice)
            .unwrap_or(0)
            .min(se::TEXT_BLIP.len() - 1);
    }

    /// The message window made a sound.
    pub fn talk_sound(&mut self, rom: &[u8], sound: dq9_core::talk::Sound) {
        use dq9_core::talk::Sound;
        let index = match sound {
            Sound::Blip => se::TEXT_BLIP[self.voice],
            Sound::Exclamation => se::EXCLAMATION,
            Sound::Question => se::QUESTION,
            Sound::Choice => se::CHOICE_OPEN,
        };
        self.play_base_effect(rom, index);
    }

    // --- The scripts ------------------------------------------------------------------------

    /// Executes a sound native of an event script (`Sound`, natives 712 to 738).
    pub fn script_sound(&mut self, rom: &[u8], sound: &Sound) {
        match *sound {
            // 713: `Bgm_Select` then a stop (`ov_001::02163640`).
            Sound::SelectBgm(id) if !self.locked => {
                self.selected = u8::try_from(id).ok();
                if let Some(sequence) = self.selected
                    && self.playing.map(|(s, _)| s) != Some(sequence)
                {
                    self.start_music(rom, Some(sequence), 0.0);
                }
                self.stop_music(0.0);
            }
            // 714: the selected sequence from its start, at full volume
            // (`ov_001::021636a8`).
            Sound::RestartBgm if !self.locked => {
                self.volume = 1.0;
                self.start_music(rom, self.selected.or(self.wanted), 0.0);
            }
            Sound::BgmVolume { volume, duration } if !self.locked => {
                self.volume = volume.clamp(0, 127) as f32 / 127.0;
                if let Some((_, handle)) = self.playing {
                    let volume = self.volume;
                    lock(&self.engine).mixer.ramp_volume(
                        handle,
                        volume,
                        duration.max(0) as f32 * TICK,
                    );
                }
            }
            Sound::StopBgm { duration } if !self.locked => {
                self.stop_music(duration.max(0) as f32 * TICK);
            }
            // 737: the music stops, the sequence plays alone (`0x0209c6e8`).
            Sound::PlaySequence(id) => {
                self.stop_music(0.0);
                self.jingle = None;
                if let Ok(sequence) = u8::try_from(id) {
                    self.play_jingle(rom, sequence, false);
                }
            }
            // 738: everything stops, the volume is back to 127 (`0x0209c21c`).
            Sound::ResetBgm => {
                self.stop_music(0.0);
                self.jingle = None;
                self.selected = None;
                self.volume = 1.0;
            }
            // 720: the music pauses for the jingle and comes back (`0x0209c850`).
            Sound::Jingle(id) => self.request_jingle(id),
            // 736: `Bgm_SequenceForMap` then `Bgm_Select` (`ov_001::02163aec`).
            Sound::ZoneBgm if !self.locked => {
                self.selected = self.wanted;
                if self.playing.map(|(s, _)| s) != self.wanted {
                    self.start_music(rom, self.wanted, 0.0);
                }
            }
            Sound::BgmEnabled(enabled) => self.locked = !enabled,
            Sound::TextVoice(voice) => self.set_text_voice(voice),
            Sound::SelectBgm(_)
            | Sound::RestartBgm
            | Sound::BgmVolume { .. }
            | Sound::StopBgm { .. }
            | Sound::ZoneBgm => {}
            Sound::LoadEffects {
                archive, secondary, ..
            } => {
                let slot = usize::from(secondary);
                self.loaded[slot] = usize::try_from(archive)
                    .ok()
                    .and_then(|a| self.effects(rom, a));
            }
            // The groups are given back to the DS heap; the decoded archives stay cached.
            Sound::RestoreEffects { .. } => {}
            Sound::PlayBaseEffect { index } => {
                status(&format!("base sound effect {index}"));
                if let Ok(index) = u16::try_from(index) {
                    self.play_base_effect(rom, index);
                }
            }
            Sound::PlayEffect {
                index,
                handle,
                secondary,
            } => {
                let Some(effects) = self.loaded[usize::from(secondary)].clone() else {
                    status(&format!("sound effect {index}: no archive loaded"));
                    return;
                };
                let started = usize::try_from(index)
                    .ok()
                    .and_then(|i| self.start_effect(&effects, i));
                status(&format!(
                    "sound effect {index} on handle {handle}{}",
                    if started.is_some() { "" } else { " (silent)" }
                ));
                if let Some(slot) = usize::try_from(handle)
                    .ok()
                    .and_then(|h| self.handles.get_mut(h))
                {
                    *slot = started;
                }
            }
            Sound::StopEffect { handle, duration } => {
                let taken = usize::try_from(handle)
                    .ok()
                    .and_then(|h| self.handles.get_mut(h))
                    .and_then(Option::take);
                if let Some(handle) = taken {
                    lock(&self.engine)
                        .mixer
                        .stop(handle, duration.max(0) as f32 * TICK);
                }
            }
        }
    }

    /// Native 720: the music fades, the jingle plays, the music returns. Nothing is heard
    /// (muted, no device): the wait of the script ends at once.
    fn request_jingle(&mut self, id: i32) {
        let Ok(sequence) = u8::try_from(id) else {
            self.jingle_ended = true;
            return;
        };
        if !self.audible() {
            self.jingle_ended = true;
            return;
        }
        if let Some((_, handle)) = self.playing {
            let ramp = JINGLE_FADE as f32 * TICK;
            lock(&self.engine).mixer.ramp_volume(handle, 0.0, ramp);
        }
        self.jingle = Some(Jingle::Lead {
            sequence,
            left: JINGLE_LEAD,
        });
    }

    /// Starts `sequence` of the music archive as a jingle.
    fn play_jingle(&mut self, rom: &[u8], sequence: u8, resume: bool) {
        let handle = match self.song(rom, sequence) {
            Ok(song) => {
                status(&format!(
                    "jingle: {} ({sequence})",
                    song.name.as_deref().unwrap_or("?")
                ));
                self.audible()
                    .then(|| lock(&self.engine).mixer.play(song))
                    .flatten()
            }
            Err(e) => {
                status(&format!("jingle {sequence}: {e}"));
                None
            }
        };
        self.jingle = handle.map(|handle| Jingle::Playing { handle, resume });
        if self.jingle.is_none() {
            self.finish_jingle(resume);
        }
    }

    /// The jingle is over: the music resumes if it waited.
    fn finish_jingle(&mut self, resume: bool) {
        if resume && let Some((_, handle)) = self.playing {
            let volume = self.volume;
            let mut engine = lock(&self.engine);
            engine.mixer.set_paused(handle, false);
            engine
                .mixer
                .ramp_volume(handle, volume, JINGLE_RETURN as f32 * TICK);
        }
        self.jingle = None;
        self.jingle_ended = true;
    }

    /// One fixed step of 1/60 s: the waits of the jingle.
    pub fn step(&mut self, rom: &[u8]) {
        self.record_step();
        match self.jingle {
            Some(Jingle::Lead { sequence, left }) if left > 1 => {
                self.jingle = Some(Jingle::Lead {
                    sequence,
                    left: left - 1,
                });
            }
            Some(Jingle::Lead { sequence, .. }) => {
                let resume = self.playing.is_some();
                if let Some((_, handle)) = self.playing {
                    lock(&self.engine).mixer.set_paused(handle, true);
                }
                self.play_jingle(rom, sequence, resume);
            }
            Some(Jingle::Playing { handle, resume }) => {
                if !self.heard() || !lock(&self.engine).mixer.is_playing(handle) {
                    self.jingle = Some(Jingle::Tail {
                        left: JINGLE_TAIL,
                        resume,
                    });
                }
            }
            Some(Jingle::Tail { left, resume }) if left > 1 => {
                self.jingle = Some(Jingle::Tail {
                    left: left - 1,
                    resume,
                });
            }
            Some(Jingle::Tail { resume, .. }) => self.finish_jingle(resume),
            None => {}
        }
    }

    /// Whether a jingle asked by a script played out since the last call (`Signal::JingleEnded`).
    pub fn take_jingle_ended(&mut self) -> bool {
        std::mem::take(&mut self.jingle_ended)
    }

    // --- Device -----------------------------------------------------------------------------

    /// Called on every user gesture: the Web needs one to start (or resume) its audio
    /// context; natively it opens the device if it was not yet.
    pub fn gesture(&mut self) {
        if self.muted() {
            return;
        }
        self.open();
        if let Some(backend) = &self.backend {
            backend.resume();
        }
    }

    /// Opens the output stream once, when something has to be heard.
    fn open(&mut self) {
        if self.backend.is_some() || self.failed {
            return;
        }
        match backend::Backend::open(Arc::clone(&self.engine)) {
            Ok(backend) => {
                self.backend = Some(backend);
                self.adopt_device_rate();
            }
            Err(e) => {
                // On the Web a context made before any gesture is refused: try again at
                // the next one; natively there is nothing to wait for.
                self.failed = !cfg!(target_arch = "wasm32");
                status(&format!("no sound: {e}"));
            }
        }
    }

    /// The device sets the rate of the mix: the sequences started before it opened began at
    /// another one, so the music starts again at the right rate.
    fn adopt_device_rate(&mut self) {
        let mut engine = lock(&self.engine);
        if engine.mixer.rate() == engine.rate {
            return;
        }
        engine.mixer = Mixer::new(engine.rate);
        for &(player, max) in &self.limits {
            engine.mixer.set_player_limit(player, max);
        }
        self.handles = [None; HANDLES];
        self.jingle = None;
        let playing = self.playing.take().map(|(s, _)| s);
        if let Some(sequence) = playing
            && let Some(song) = self.songs.get(&sequence).cloned()
            && let Some(handle) = engine.mixer.play(song)
        {
            engine.mixer.set_volume(handle, self.volume);
            self.playing = Some((sequence, handle));
        }
    }
}

/// The sequence a map plays at a time of day, from the map list and the overrides.
fn sequence_for_map(
    rom: &[u8],
    map: u16,
    time: u32,
    flags: &dyn Fn(u16) -> bool,
) -> anyhow::Result<Option<u8>> {
    let rom = Rom::parse(rom)?;
    let list = MapList::parse(&Table::parse(rom.file_by_path(maplist::PATH)?)?)?;
    let overrides = Overrides::parse(&Table::parse(rom.file_by_path(bgm::PATH)?)?)?;
    Ok(bgm::sequence_for_map(&list, &overrides, map, time, flags))
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    use super::{Shared, lock};

    pub struct Backend {
        _stream: cpal::Stream,
    }

    impl Backend {
        pub fn open(engine: Shared) -> Result<Self, String> {
            let device = cpal::default_host()
                .default_output_device()
                .ok_or("no output device")?;
            let supported = device.default_output_config().map_err(|e| format!("{e}"))?;
            let format = supported.sample_format();
            let config = supported.config();
            lock(&engine).rate = config.sample_rate;
            let stream = match format {
                SampleFormat::F32 => build::<f32>(&device, config, engine),
                SampleFormat::I16 => build::<i16>(&device, config, engine),
                SampleFormat::U16 => build::<u16>(&device, config, engine),
                other => return Err(format!("unsupported sample format {other:?}")),
            }?;
            stream.play().map_err(|e| format!("{e}"))?;
            Ok(Self { _stream: stream })
        }

        /// Nothing to resume natively: a started stream plays.
        pub fn resume(&self) {}
    }

    /// Writes the engine's stereo mix into a device buffer of `channels` interleaved channels.
    fn spread(stereo: &[f32], out: &mut dyn FnMut(usize, f32), channels: usize) {
        for (i, frame) in stereo.as_chunks::<2>().0.iter().enumerate() {
            match channels {
                1 => out(i, (frame[0] + frame[1]) * 0.5),
                _ => {
                    out(i * channels, frame[0]);
                    out(i * channels + 1, frame[1]);
                    for c in 2..channels {
                        out(i * channels + c, 0.0);
                    }
                }
            }
        }
    }

    fn build<T: SizedSample + FromSample<f32> + Send + 'static>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        engine: Shared,
    ) -> Result<cpal::Stream, String> {
        let channels = usize::from(config.channels);
        let mut stereo: Vec<f32> = Vec::new();
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    stereo.resize(data.len() / channels * 2, 0.0);
                    lock(&engine).fill(&mut stereo);
                    spread(&stereo, &mut |i, v| data[i] = T::from_sample(v), channels);
                },
                |e| eprintln!("sound stream: {e}"),
                None,
            )
            .map_err(|e| format!("{e}"))
    }
}

#[cfg(target_arch = "wasm32")]
mod backend {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    use super::{Shared, lock};

    /// Frames a `ScriptProcessorNode` asks for at a time (about 85 ms at 48 kHz).
    const BUFFER: u32 = 4096;

    pub struct Backend {
        context: web_sys::AudioContext,
        _node: web_sys::ScriptProcessorNode,
        _callback: Closure<dyn FnMut(web_sys::AudioProcessingEvent)>,
    }

    impl Backend {
        pub fn open(engine: Shared) -> Result<Self, String> {
            let context = web_sys::AudioContext::new().map_err(|e| format!("{e:?}"))?;
            lock(&engine).rate = context.sample_rate() as u32;
            let node = context
                .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
                    BUFFER, 1, 2,
                )
                .map_err(|e| format!("{e:?}"))?;
            let mut stereo: Vec<f32> = Vec::new();
            let mut left: Vec<f32> = Vec::new();
            let mut right: Vec<f32> = Vec::new();
            let callback = Closure::<dyn FnMut(web_sys::AudioProcessingEvent)>::new(
                move |event: web_sys::AudioProcessingEvent| {
                    let Ok(buffer) = event.output_buffer() else {
                        return;
                    };
                    let frames = buffer.length() as usize;
                    stereo.resize(frames * 2, 0.0);
                    lock(&engine).fill(&mut stereo);
                    left.clear();
                    right.clear();
                    for frame in stereo.as_chunks::<2>().0.iter() {
                        left.push(frame[0]);
                        right.push(frame[1]);
                    }
                    let _ = buffer.copy_to_channel(&left, 0);
                    let _ = buffer.copy_to_channel(&right, 1);
                },
            );
            node.set_onaudioprocess(Some(callback.as_ref().unchecked_ref()));
            node.connect_with_audio_node(&context.destination())
                .map_err(|e| format!("{e:?}"))?;
            // Made outside a gesture, the context starts suspended: `resume` wakes it.
            let _ = context.resume();
            Ok(Self {
                context,
                _node: node,
                _callback: callback,
            })
        }

        pub fn resume(&self) {
            if self.context.state() == web_sys::AudioContextState::Suspended {
                let _ = self.context.resume();
            }
        }
    }
}
