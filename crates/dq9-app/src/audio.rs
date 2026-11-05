//! Music output of the application: picks the sequence a map plays (`dq9_formats::bgm`),
//! loads it from `bgm.sdat` and hands it to `dq9_audio`, which a platform stream pulls
//! from: cpal natively, a WebAudio `ScriptProcessorNode` on the Web (the context is made
//! and resumed on the first user gesture, which browsers require). `--mute` (`?mute=1` on
//! the Web) opens no device at all; `M` mutes and unmutes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use dq9_audio::{Player, Song};
use dq9_formats::Table;
use dq9_formats::bgm::{self, Overrides};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::sdat::Sdat;
use nds_rom::Rom;

use crate::app::status;

/// Path of the music archive in the ROM filesystem.
const BGM_SDAT: &str = "data/sound/bgm.sdat";
/// Seconds an old track takes to fade out when the map changes its music.
const FADE_SECONDS: f32 = 0.4;
/// Songs kept decoded: the maps one walks between share few tracks.
const CACHE: usize = 6;

/// What the stream callback mixes: the song playing, the ones fading out, the mute.
struct Engine {
    rate: u32,
    muted: bool,
    /// Song to start at the next callback, at the stream's own rate.
    next: Option<Arc<Song>>,
    current: Option<Player>,
    fading: Vec<Player>,
    scratch: Vec<f32>,
}

impl Engine {
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if let Some(song) = self.next.take() {
            if let Some(mut old) = self.current.take() {
                old.fade_out(FADE_SECONDS);
                self.fading.push(old);
            }
            self.current = Some(Player::new(song, self.rate));
        }
        if self.muted {
            return;
        }
        self.scratch.resize(out.len(), 0.0);
        let scratch = &mut self.scratch;
        for player in self.current.iter_mut().chain(&mut self.fading) {
            player.render(scratch);
            for (o, s) in out.iter_mut().zip(scratch.iter()) {
                *o += *s;
            }
        }
        self.fading.retain(|p| !p.finished());
        if self.current.as_ref().is_some_and(Player::finished) {
            self.current = None;
        }
    }
}

type Shared = Arc<Mutex<Engine>>;

fn lock(engine: &Shared) -> MutexGuard<'_, Engine> {
    // A poisoned lock only means a callback panicked; the engine state is still usable.
    engine.lock().unwrap_or_else(|e| e.into_inner())
}

/// The application's music.
pub struct Audio {
    engine: Shared,
    backend: Option<backend::Backend>,
    /// The device could not be opened: not tried again.
    failed: bool,
    /// Sequence asked for by the current map (`None`: silence).
    wanted: Option<u8>,
    playing: Option<u8>,
    songs: HashMap<u8, Arc<Song>>,
    order: Vec<u8>,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("wanted", &self.wanted)
            .field("open", &self.backend.is_some())
            .finish_non_exhaustive()
    }
}

impl Audio {
    /// `mute` starts silent and does not touch the sound device until unmuted.
    pub fn new(mute: bool) -> Self {
        Self {
            engine: Arc::new(Mutex::new(Engine {
                rate: 48_000,
                muted: mute,
                next: None,
                current: None,
                fading: Vec::new(),
                scratch: Vec::new(),
            })),
            backend: None,
            failed: false,
            wanted: None,
            playing: None,
            songs: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn muted(&self) -> bool {
        lock(&self.engine).muted
    }

    /// Mutes or unmutes (the song keeps its place while muted).
    pub fn toggle_mute(&mut self) {
        let muted = {
            let mut engine = lock(&self.engine);
            engine.muted = !engine.muted;
            engine.muted
        };
        status(if muted { "music muted" } else { "music on" });
        if !muted {
            self.open();
        }
    }

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
        if self.playing == sequence {
            return;
        }
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
        self.playing = sequence;
        {
            let mut engine = lock(&self.engine);
            match song {
                Some(song) => engine.next = Some(song),
                None => {
                    if let Some(mut old) = engine.current.take() {
                        old.fade_out(FADE_SECONDS);
                        engine.fading.push(old);
                    }
                    engine.next = None;
                }
            }
        }
        if !self.muted() {
            self.open();
        }
    }

    fn song(&mut self, rom: &[u8], index: u8) -> Result<Arc<Song>, nds_rom::Error> {
        if let Some(song) = self.songs.get(&index) {
            return Ok(Arc::clone(song));
        }
        let rom = Rom::parse(rom)?;
        let sdat = Sdat::parse(rom.file_by_path(BGM_SDAT)?)?;
        let song = Arc::new(Song::load(&sdat, usize::from(index))?);
        if self.order.len() >= CACHE {
            let old = self.order.remove(0);
            self.songs.remove(&old);
        }
        self.order.push(index);
        self.songs.insert(index, Arc::clone(&song));
        Ok(song)
    }

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
            Ok(backend) => self.backend = Some(backend),
            Err(e) => {
                // On the Web a context made before any gesture is refused: try again at
                // the next one; natively there is nothing to wait for.
                self.failed = !cfg!(target_arch = "wasm32");
                status(&format!("no sound: {e}"));
            }
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
