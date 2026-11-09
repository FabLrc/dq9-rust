//! The SSEQ sequencer and the software mixer.
//!
//! Timing follows the DS sound driver: it runs 192 times a second; each run adds the tempo
//! (BPM) to a counter and every 240 it plays one *tick* (48 ticks to a quarter note).
//! Voices (envelopes, pitch, volume and pan) are refreshed on every run and the samples
//! between two runs are mixed with the voice gains ramped linearly, so a volume change
//! never clicks.
//!
//! The commands are those of the SSEQ format (wiki SDAT page, GBATEK, Gota7's Nitro Studio 2
//! specification), read as kode54's SSEQPlayer and fincs's FSS read them (both WTFPL).

use std::sync::Arc;

use dq9_formats::sdat::{Instrument, Note, NoteKind, Samples};

use crate::Song;
use crate::tables::{self, AMPL_MIN};

/// Driver updates per second.
const UPDATE_HZ: f64 = 192.0;
/// Sample rate of a wave whose timer is `1`: the ARM7 clock over two.
const TIMER_HZ: f64 = 16_756_991.0;
/// Headroom of the mix. The DS mixer adds up to 16 channels and clips at full scale, which
/// the loudest sequences reach (peaks to twice full scale); the port keeps their balance and
/// lowers the whole mix by 4.4 dB, the limiter catching the rest.
const MIX_GAIN: f32 = 0.6;
const MAX_TRACKS: usize = 16;
const MAX_VOICES: usize = 32;
const MAX_STACK: usize = 8;
/// Commands a track may run in one tick before it is considered stuck in a loop without a
/// wait.
const COMMAND_BUDGET: u32 = 100_000;

#[derive(Debug, Clone, Copy)]
enum Frame {
    Call(usize),
    Loop { start: usize, count: u8 },
}

#[derive(Debug, Clone)]
struct Track {
    active: bool,
    pos: usize,
    wait: u32,
    stack: Vec<Frame>,
    program: u32,
    volume: u8,
    expression: u8,
    pan: u8,
    pan_range: u8,
    transpose: i8,
    bend: i8,
    bend_range: u8,
    note_wait: bool,
    tie: bool,
    tie_voice: Option<u32>,
    porta_on: bool,
    porta_key: u8,
    porta_time: u8,
    mod_depth: u8,
    mod_speed: u8,
    mod_type: u8,
    mod_range: u8,
    mod_delay: u16,
    sweep: i16,
    /// Attack, decay, sustain, release overriding the instrument's.
    envelope: [Option<u8>; 4],
    cond: bool,
    /// Times an infinite loop jumped back.
    loops: u32,
}

impl Track {
    fn new(pos: usize, active: bool) -> Self {
        Self {
            active,
            pos,
            wait: 0,
            stack: Vec::new(),
            program: 0,
            volume: 127,
            expression: 127,
            pan: 64,
            pan_range: 127,
            transpose: 0,
            bend: 0,
            bend_range: 2,
            note_wait: true,
            tie: false,
            tie_voice: None,
            porta_on: false,
            porta_key: 60,
            porta_time: 0,
            mod_depth: 0,
            mod_speed: 16,
            mod_type: 0,
            mod_range: 1,
            mod_delay: 0,
            sweep: 0,
            envelope: [None; 4],
            cond: false,
            loops: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug)]
enum Source {
    Pcm {
        samples: Arc<Samples>,
        pos: f64,
        /// Output samples a step of the wave advances at its own pitch.
        native: f64,
    },
    Psg {
        duty: f32,
        phase: f64,
    },
    Noise {
        lfsr: u16,
        phase: f64,
        value: f32,
    },
}

#[derive(Debug)]
struct Voice {
    id: u32,
    track: usize,
    key: u8,
    velocity: u8,
    /// Key at which the wave plays at its own rate (PCM) or the PSG's middle C.
    base_key: u8,
    note_pan: i32,
    source: Source,
    stage: Stage,
    ampl: i32,
    sustain_level: i32,
    attack_rate: i32,
    decay_rate: i32,
    release_rate: i32,
    /// Ticks until the key goes up; `None` while held.
    remaining: Option<u32>,
    sweep_pitch: i32,
    sweep_len: u32,
    sweep_pos: u32,
    mod_counter: u32,
    mod_delay_left: u32,
    step: f64,
    gain: [f32; 2],
    target: [f32; 2],
    fresh: bool,
    done: bool,
}

/// Plays a [`Song`]: fill buffers with [`Player::render`].
#[derive(Debug)]
pub struct Player {
    song: Arc<Song>,
    rate: f64,
    tracks: Vec<Track>,
    voices: Vec<Voice>,
    next_voice: u32,
    tempo: u16,
    tempo_count: u32,
    master: u8,
    vars: [i16; 32],
    rng: u32,
    /// Output frames until the next driver update.
    until_update: f64,
    frames: u64,
    /// Host gain (1.0 = as the game).
    volume: f32,
    volume_target: f32,
    /// Volume change per output frame while ramping.
    volume_step: f32,
    paused: bool,
    fade_left: u64,
    fade_total: u64,
    peak: f32,
}

impl Player {
    /// Starts `song` from the beginning, mixing at `sample_rate` Hz.
    pub fn new(song: Arc<Song>, sample_rate: u32) -> Self {
        let mut tracks = vec![Track::new(song.start, true)];
        tracks.reserve(MAX_TRACKS - 1);
        Self {
            song,
            rate: f64::from(sample_rate.max(1)),
            tracks,
            voices: Vec::new(),
            next_voice: 0,
            tempo: 120,
            tempo_count: 0,
            master: 127,
            vars: [0; 32],
            rng: 0x2545_f491,
            until_update: 0.0,
            frames: 0,
            volume: 1.0,
            volume_target: 1.0,
            volume_step: 0.0,
            paused: false,
            fade_left: 0,
            fade_total: 0,
            peak: 0.0,
        }
    }

    /// The song being played.
    pub fn song(&self) -> &Arc<Song> {
        &self.song
    }

    /// Current tempo in beats per minute.
    pub fn tempo(&self) -> u16 {
        self.tempo
    }

    /// Tracks the sequence has opened so far.
    pub fn track_count(&self) -> usize {
        self.tracks
            .iter()
            .filter(|t| t.pos != 0 || t.active)
            .count()
            .max(1)
    }

    /// Highest absolute sample of the mix so far, at the scale of the DS mixer (1.0 is where
    /// the hardware clips), before headroom and limiter.
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Seconds of audio rendered so far.
    pub fn elapsed(&self) -> f64 {
        self.frames as f64 / self.rate
    }

    /// Full passes of the sequence's infinite loops: the fewest any still-running track has
    /// made (0 until every running track has looped once).
    pub fn loops(&self) -> u32 {
        self.tracks
            .iter()
            .filter(|t| t.active)
            .map(|t| t.loops)
            .min()
            .unwrap_or(u32::MAX)
    }

    /// Host volume, 0.0 to 1.0, at once.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.volume_target = self.volume;
    }

    /// Host volume now.
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Moves the host volume to `volume` (0.0 to 1.0) in a straight line over `seconds`.
    pub fn ramp_volume(&mut self, volume: f32, seconds: f32) {
        self.volume_target = volume.clamp(0.0, 1.0);
        let frames = (f64::from(seconds.max(0.0)) * self.rate).max(1.0);
        self.volume_step = ((self.volume_target - self.volume).abs() as f64 / frames) as f32;
    }

    /// Holds the song where it is, silent, or lets it go on.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Fades the sound to silence over `seconds` of audio; the song is then finished.
    pub fn fade_out(&mut self, seconds: f32) {
        let frames = (f64::from(seconds.max(0.0)) * self.rate) as u64;
        self.fade_total = frames.max(1);
        self.fade_left = self.fade_total;
    }

    /// Whether every track ended and every voice died out, or a fade ran out.
    pub fn finished(&self) -> bool {
        (self.fade_total > 0 && self.fade_left == 0)
            || (self.tracks.iter().all(|t| !t.active) && self.voices.is_empty())
    }

    /// Mixes the next `out.len() / 2` frames into `out` (interleaved stereo, -1.0 to 1.0),
    /// overwriting it. Silence once [`Player::finished`].
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if self.paused {
            return;
        }
        let total = out.len() / 2;
        let mut done = 0;
        while done < total {
            if self.until_update <= 0.0 {
                self.update();
                self.until_update += self.rate / UPDATE_HZ;
            }
            let n = (total - done).min(self.until_update.ceil().max(1.0) as usize);
            let block = &mut out[done * 2..(done + n) * 2];
            for voice in &mut self.voices {
                voice.mix(block, n);
            }
            self.until_update -= n as f64;
            done += n;
        }
        self.frames += total as u64;
        self.finish_block(out);
    }

    /// Host volume, fade and a soft limiter over the mixed block.
    fn finish_block(&mut self, out: &mut [f32]) {
        let total = self.fade_total;
        for (i, frame) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            if self.volume != self.volume_target {
                let step = self.volume_step;
                self.volume = if self.volume < self.volume_target {
                    (self.volume + step).min(self.volume_target)
                } else {
                    (self.volume - step).max(self.volume_target)
                };
            }
            let fade = if total == 0 {
                1.0
            } else {
                self.fade_left.saturating_sub(i as u64) as f32 / total as f32
            };
            for s in frame {
                self.peak = self.peak.max(s.abs());
                *s = soft_limit(*s * MIX_GAIN * self.volume * fade);
            }
        }
        self.fade_left = self.fade_left.saturating_sub((out.len() / 2) as u64);
    }

    /// One driver update: tempo ticks, then every voice's envelope, pitch and gains.
    fn update(&mut self) {
        self.tempo_count += u32::from(self.tempo);
        while self.tempo_count >= 240 {
            self.tempo_count -= 240;
            self.tick();
        }
        let (tracks, master, seq_volume, rate) =
            (&self.tracks, self.master, self.song.volume, self.rate);
        for voice in &mut self.voices {
            voice.update(&tracks[voice.track], master, seq_volume, rate);
        }
        self.voices.retain(|v| !v.done);
    }

    /// One sequence tick: note lengths, then each track's commands.
    fn tick(&mut self) {
        for voice in &mut self.voices {
            if let Some(left) = &mut voice.remaining {
                *left = left.saturating_sub(1);
                if *left == 0 {
                    voice.remaining = None;
                    voice.key_off();
                }
            }
        }
        let mut t = 0;
        while t < self.tracks.len() {
            if self.tracks[t].active {
                self.tracks[t].wait = self.tracks[t].wait.saturating_sub(1);
                self.run_track(t);
            }
            t += 1;
        }
        // Once every track has ended, notes held without a length are let go, unless their
        // wave plays once: it ends by itself (the sound effects are written that way: a key,
        // a length of 0 and the end of the track) **[hypothesis, docs/re/sdat.md]**.
        if self.tracks.iter().all(|t| !t.active) {
            for voice in &mut self.voices {
                if voice.remaining.is_none() && voice.loops() {
                    voice.key_off();
                }
            }
        }
    }

    fn run_track(&mut self, t: usize) {
        let song = Arc::clone(&self.song);
        let mut track = std::mem::replace(&mut self.tracks[t], Track::new(0, false));
        let mut budget = COMMAND_BUDGET;
        while track.active && track.wait == 0 {
            budget -= 1;
            if budget == 0 {
                track.active = false;
                break;
            }
            let mut cursor = Cursor {
                stream: &song.stream,
                pos: track.pos,
            };
            let ok = self.exec(&mut cursor, t, &mut track);
            track.pos = cursor.pos;
            if ok.is_none() {
                track.active = false;
            }
        }
        self.tracks[t] = track;
    }

    fn random(&mut self, min: i32, max: i32) -> i32 {
        // xorshift32.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
        lo + (self.rng % (hi - lo + 1) as u32) as i32
    }

    /// The last argument of a command: as written, or replaced by a random value (`A0`) or a
    /// variable (`A1`).
    fn last_arg(&mut self, cur: &mut Cursor<'_>, over: Over, kind: Arg) -> Option<i32> {
        match over {
            Over::None => match kind {
                Arg::U8 => cur.u8().map(i32::from),
                Arg::S8 => cur.u8().map(|v| i32::from(v as i8)),
                Arg::U16 => cur.u16().map(i32::from),
                Arg::S16 => cur.u16().map(|v| i32::from(v as i16)),
                Arg::Vlq => cur.vlq().map(|v| v as i32),
            },
            Over::Rand => {
                let min = i32::from(cur.u16()? as i16);
                let max = i32::from(cur.u16()? as i16);
                Some(self.random(min, max))
            }
            Over::Var => {
                let var = usize::from(cur.u8()?);
                Some(i32::from(*self.vars.get(var)?))
            }
        }
    }

    /// Reads and runs one command of track `t`. `None` ends the track: end command, stream
    /// overrun or an unknown command.
    fn exec(&mut self, cur: &mut Cursor<'_>, t: usize, tr: &mut Track) -> Option<()> {
        let mut op = cur.u8()?;
        let mut skip = false;
        let mut over = Over::None;
        loop {
            match op {
                0xa0 => over = Over::Rand,
                0xa1 => over = Over::Var,
                0xa2 => skip = !tr.cond,
                _ => break,
            }
            op = cur.u8()?;
        }
        match op {
            0x00..=0x7f => {
                let velocity = cur.u8()?;
                let duration = self.last_arg(cur, over, Arg::Vlq)? as u32;
                if !skip {
                    self.note_on(t, tr, op, velocity, duration);
                    if tr.note_wait {
                        tr.wait = duration;
                    }
                }
            }
            0x80 => {
                let wait = self.last_arg(cur, over, Arg::Vlq)? as u32;
                if !skip {
                    tr.wait = wait;
                }
            }
            0x81 => {
                let program = self.last_arg(cur, over, Arg::Vlq)? as u32;
                if !skip {
                    tr.program = program;
                }
            }
            0x93 => {
                let number = usize::from(cur.u8()?);
                let offset = cur.u24()?;
                if !skip && number < MAX_TRACKS && number != t {
                    if self.tracks.len() <= number {
                        self.tracks.resize(number + 1, Track::new(0, false));
                    }
                    self.tracks[number] = Track::new(offset, true);
                }
            }
            0x94 => {
                let offset = cur.u24()?;
                if !skip {
                    if offset < cur.pos {
                        tr.loops += 1;
                    }
                    cur.pos = offset;
                }
            }
            0x95 => {
                let offset = cur.u24()?;
                if !skip && tr.stack.len() < MAX_STACK {
                    tr.stack.push(Frame::Call(cur.pos));
                    cur.pos = offset;
                }
            }
            0xb0..=0xbd => {
                let var = usize::from(cur.u8()?);
                let value = self.last_arg(cur, over, Arg::S16)?;
                if !skip {
                    self.var_op(op, var, value, tr);
                }
            }
            0xd4 => {
                let count = self.last_arg(cur, over, Arg::U8)?;
                if !skip && tr.stack.len() < MAX_STACK {
                    tr.stack.push(Frame::Loop {
                        start: cur.pos,
                        count: count as u8,
                    });
                }
            }
            0xc0..=0xd6 | 0xe0 | 0xe1 | 0xe3 => {
                let kind = match op {
                    0xc3 | 0xc4 => Arg::S8,
                    0xe0 | 0xe1 => Arg::U16,
                    0xe3 => Arg::S16,
                    _ => Arg::U8,
                };
                if op == 0xd6 {
                    cur.u8()?; // print variable: no output here
                    return Some(());
                }
                let v = self.last_arg(cur, over, kind)?;
                if !skip {
                    self.set_param(op, v, tr);
                }
            }
            0xfc => {
                if !skip && let Some(Frame::Loop { start, count }) = tr.stack.last().copied() {
                    if count == 0 {
                        tr.loops += 1;
                        cur.pos = start;
                    } else if count == 1 {
                        tr.stack.pop();
                    } else {
                        *tr.stack.last_mut()? = Frame::Loop {
                            start,
                            count: count - 1,
                        };
                        cur.pos = start;
                    }
                }
            }
            0xfd => {
                if !skip && let Some(Frame::Call(ret)) = tr.stack.last().copied() {
                    tr.stack.pop();
                    cur.pos = ret;
                }
            }
            0xfe => {
                cur.u16()?; // track allocation mask: tracks are opened as needed
            }
            0xff => return None,
            _ => return None,
        }
        Some(())
    }

    fn set_param(&mut self, op: u8, v: i32, tr: &mut Track) {
        let byte = v.clamp(0, 255) as u8;
        match op {
            0xc0 => tr.pan = byte.min(127),
            0xc1 => tr.volume = byte.min(127),
            0xc2 => self.master = byte.min(127),
            0xc3 => tr.transpose = v.clamp(-128, 127) as i8,
            0xc4 => tr.bend = v.clamp(-128, 127) as i8,
            0xc5 => tr.bend_range = byte,
            0xc6 => {} // priority: voices are not stolen by priority here
            0xc7 => tr.note_wait = byte != 0,
            0xc8 => {
                tr.tie = byte != 0;
                tr.tie_voice = None;
            }
            0xc9 => {
                tr.porta_key = byte.min(127);
                tr.porta_on = true;
            }
            0xca => tr.mod_depth = byte.min(127),
            0xcb => tr.mod_speed = byte,
            0xcc => tr.mod_type = byte,
            0xcd => tr.mod_range = byte,
            0xce => tr.porta_on = byte != 0,
            0xcf => tr.porta_time = byte,
            0xd0..=0xd3 => tr.envelope[usize::from(op - 0xd0)] = Some(byte.min(127)),
            0xd5 => tr.expression = byte.min(127),
            0xe0 => tr.mod_delay = v as u16,
            0xe1 => self.tempo = (v as u16).max(1),
            0xe3 => tr.sweep = v as i16,
            _ => {}
        }
    }

    fn var_op(&mut self, op: u8, var: usize, value: i32, tr: &mut Track) {
        let Some(slot) = self.vars.get(var).copied() else {
            return;
        };
        let cur = i32::from(slot);
        let set = |vars: &mut [i16; 32], v: i32| vars[var] = v as i16;
        match op {
            0xb0 => set(&mut self.vars, value),
            0xb1 => set(&mut self.vars, cur.wrapping_add(value)),
            0xb2 => set(&mut self.vars, cur.wrapping_sub(value)),
            0xb3 => set(&mut self.vars, cur.wrapping_mul(value)),
            0xb4 => {
                if value != 0 {
                    set(&mut self.vars, cur / value);
                }
            }
            0xb5 => set(
                &mut self.vars,
                if value >= 0 {
                    cur.wrapping_shl(value as u32 & 31)
                } else {
                    cur >> ((-value) as u32 & 31)
                },
            ),
            0xb6 => {
                let r = if value >= 0 {
                    self.random(0, value)
                } else {
                    self.random(value, 0)
                };
                set(&mut self.vars, r);
            }
            0xb8 => tr.cond = cur == value,
            0xb9 => tr.cond = cur >= value,
            0xba => tr.cond = cur > value,
            0xbb => tr.cond = cur <= value,
            0xbc => tr.cond = cur < value,
            0xbd => tr.cond = cur != value,
            _ => {}
        }
    }

    fn note_on(&mut self, t: usize, tr: &mut Track, key: u8, velocity: u8, duration: u32) {
        let key = (i32::from(key) + i32::from(tr.transpose)).clamp(0, 127) as u8;
        let porta_from = tr.porta_key;
        tr.porta_key = key;
        // Tied notes continue the track's last voice instead of starting a new one.
        if tr.tie
            && let Some(id) = tr.tie_voice
            && let Some(voice) = self
                .voices
                .iter_mut()
                .find(|v| v.id == id && v.stage != Stage::Release && !v.done)
        {
            voice.key = key;
            voice.remaining = Some(duration.max(1));
            return;
        }
        let Some(note) = self
            .song
            .instruments
            .get(tr.program as usize)
            .and_then(|i: &Instrument| i.note(key))
            .copied()
        else {
            return;
        };
        let Some(source) = self.source(&note) else {
            return;
        };
        let env = |i: usize, own: u8| tr.envelope[i].unwrap_or(own.min(127));
        let (attack, decay, sustain, release) = (
            env(0, note.attack),
            env(1, note.decay),
            env(2, note.sustain),
            env(3, note.release),
        );
        let (sweep_pitch, sweep_len) = if tr.porta_on {
            let sweep = (i32::from(porta_from) - i32::from(key)) * 64;
            let len = if tr.porta_time == 0 {
                // The glide lasts the note: its ticks in driver updates.
                (u64::from(duration) * 240 / u64::from(self.tempo.max(1))) as u32
            } else {
                ((u32::from(tr.porta_time).pow(2) * sweep.unsigned_abs()) >> 11).max(1)
            };
            (sweep, len)
        } else if tr.sweep != 0 {
            let len = (u64::from(duration) * 240 / u64::from(self.tempo.max(1))) as u32;
            (i32::from(tr.sweep), len)
        } else {
            (0, 0)
        };
        if self.voices.len() >= MAX_VOICES {
            self.steal_voice();
        }
        let id = self.next_voice;
        self.next_voice = self.next_voice.wrapping_add(1);
        let mut voice = Voice {
            id,
            track: t,
            key,
            velocity,
            base_key: if note.kind == NoteKind::Pcm || note.kind == NoteKind::Direct {
                note.base_key
            } else {
                60
            },
            note_pan: i32::from(note.pan) - 64,
            source,
            stage: Stage::Attack,
            ampl: AMPL_MIN,
            sustain_level: tables::decibel_square(sustain) << 7,
            attack_rate: tables::attack_rate(attack),
            decay_rate: tables::fall_rate(decay),
            release_rate: tables::fall_rate(release),
            // A length of 0 holds the note until the track or the sequence ends.
            remaining: (duration > 0).then_some(duration),
            sweep_pitch,
            sweep_len,
            sweep_pos: 0,
            mod_counter: 0,
            mod_delay_left: u32::from(tr.mod_delay),
            step: 0.0,
            gain: [0.0; 2],
            target: [0.0; 2],
            fresh: true,
            done: false,
        };
        if tr.tie {
            tr.tie_voice = Some(id);
            voice.remaining = Some(duration.max(1));
        }
        self.voices.push(voice);
    }

    fn source(&self, note: &Note) -> Option<Source> {
        match note.kind {
            NoteKind::Pcm | NoteKind::Direct => {
                let samples = Arc::clone(self.song.waves.get(&(note.archive, note.wave))?);
                if samples.timer == 0 {
                    return None;
                }
                let native = TIMER_HZ / f64::from(samples.timer) / self.rate;
                Some(Source::Pcm {
                    samples,
                    pos: 0.0,
                    native,
                })
            }
            NoteKind::Psg => Some(Source::Psg {
                duty: (f32::from(note.wave.min(6)) + 1.0) / 8.0,
                phase: 0.0,
            }),
            NoteKind::Noise => Some(Source::Noise {
                lfsr: 0x7fff,
                phase: 0.0,
                value: 1.0,
            }),
        }
    }

    /// Makes room for a voice: the quietest one in release, else the oldest.
    fn steal_voice(&mut self) {
        let pick = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.stage == Stage::Release)
            .min_by_key(|(_, v)| v.ampl)
            .map(|(i, _)| i)
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.id)
                    .map(|(i, _)| i)
            });
        if let Some(i) = pick {
            self.voices.swap_remove(i);
        }
    }
}

fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh())
    }
}

impl Voice {
    /// Whether the wave goes on forever (a loop, PSG, noise) rather than ending by itself.
    fn loops(&self) -> bool {
        match &self.source {
            Source::Pcm { samples, .. } => samples.loop_start.is_some(),
            Source::Psg { .. } | Source::Noise { .. } => true,
        }
    }

    fn key_off(&mut self) {
        if self.stage != Stage::Release {
            self.stage = Stage::Release;
        }
    }

    /// Steps the envelope, the sweep and the modulation, and sets the gains and the pitch step
    /// until the next update.
    fn update(&mut self, track: &Track, master: u8, seq_volume: u8, rate: f64) {
        match self.stage {
            Stage::Attack => {
                self.ampl = (i64::from(self.ampl) * i64::from(self.attack_rate) / 256) as i32;
                if self.ampl >= 0 {
                    self.ampl = 0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.ampl -= self.decay_rate;
                if self.ampl <= self.sustain_level {
                    self.ampl = self.sustain_level;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {}
            Stage::Release => {
                self.ampl -= self.release_rate;
                if self.ampl <= AMPL_MIN {
                    self.done = true;
                    return;
                }
            }
        }

        // Pitch in 1/64 semitones relative to the voice's base key.
        let mut pitch = (i32::from(self.key) - i32::from(self.base_key)) * 64;
        pitch += (i32::from(track.bend) * i32::from(track.bend_range)) >> 1;
        if self.sweep_len > 0 && self.sweep_pos < self.sweep_len {
            let left = i64::from(self.sweep_len - self.sweep_pos);
            pitch += (i64::from(self.sweep_pitch) * left / i64::from(self.sweep_len)) as i32;
            self.sweep_pos += 1;
        }
        let mut volume_db = 0;
        let mut pan = i32::from(track.pan);
        if track.mod_depth > 0 {
            if self.mod_delay_left > 0 {
                self.mod_delay_left -= 1;
            } else {
                self.mod_counter = (self.mod_counter + (u32::from(track.mod_speed) << 6)) % 0x8000;
            }
            let wave = tables::sine(self.mod_counter >> 8)
                * i32::from(track.mod_range)
                * i32::from(track.mod_depth);
            match track.mod_type {
                0 => pitch += wave >> 8,
                1 => volume_db += ((wave >> 8) * 60) >> 6,
                _ => pan += wave >> 8,
            }
        }

        let (left, right) = {
            let pan_range = i32::from(track.pan_range);
            let pan = ((pan - 64) * pan_range / 127 + 64 + self.note_pan).clamp(0, 127);
            ((127 - pan) as f32 / 127.0, pan as f32 / 127.0)
        };
        let db = tables::decibel_square(self.velocity)
            + tables::decibel_square(track.volume)
            + tables::decibel_square(track.expression)
            + tables::decibel_square(seq_volume)
            + tables::decibel_square(master)
            + volume_db
            + (self.ampl >> 7);
        let amplitude = tables::gain(db);
        self.target = [amplitude * left, amplitude * right];
        if self.fresh {
            self.gain = self.target;
            self.fresh = false;
        }

        let ratio = (f64::from(pitch) / (12.0 * 64.0)).exp2();
        self.step = match &self.source {
            Source::Pcm { native, .. } => native * ratio,
            // The PSG and the noise run on a timer of 0x1f46 at middle C: 8 steps to a PSG
            // cycle, one LFSR shift per step for the noise.
            Source::Psg { .. } => TIMER_HZ / f64::from(0x1f46u32) / 8.0 * ratio / rate,
            Source::Noise { .. } => TIMER_HZ / f64::from(0x1f46u32) * ratio / rate,
        }
        .min(64.0);
    }

    /// Adds `frames` frames of the voice to the interleaved block.
    fn mix(&mut self, block: &mut [f32], frames: usize) {
        let [l0, r0] = self.gain;
        let [l1, r1] = self.target;
        let (dl, dr) = ((l1 - l0) / frames as f32, (r1 - r0) / frames as f32);
        let step = self.step;
        for (i, frame) in block.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let sample = match &mut self.source {
                Source::Pcm { samples, pos, .. } => {
                    let data = &samples.samples;
                    let at = *pos as usize;
                    let Some(&s0) = data.get(at) else {
                        self.done = true;
                        break;
                    };
                    let next = match data.get(at + 1) {
                        Some(&s) => s,
                        None => samples
                            .loop_start
                            .and_then(|l| data.get(l))
                            .copied()
                            .unwrap_or(s0),
                    };
                    let frac = (*pos - at as f64) as f32;
                    let s = f32::from(s0) + (f32::from(next) - f32::from(s0)) * frac;
                    *pos += step;
                    if *pos >= data.len() as f64 {
                        match samples.loop_start {
                            Some(l) => {
                                let span = (data.len() - l) as f64;
                                *pos = l as f64 + (*pos - l as f64) % span;
                            }
                            None => self.done = true,
                        }
                    }
                    s / 32768.0
                }
                Source::Psg { duty, phase } => {
                    *phase = (*phase + step).fract();
                    if (*phase as f32) < *duty { 1.0 } else { -1.0 }
                }
                Source::Noise { lfsr, phase, value } => {
                    *phase += step;
                    while *phase >= 1.0 {
                        *phase -= 1.0;
                        if *lfsr & 1 != 0 {
                            *lfsr = (*lfsr >> 1) ^ 0x6000;
                            *value = -1.0;
                        } else {
                            *lfsr >>= 1;
                            *value = 1.0;
                        }
                    }
                    *value
                }
            };
            let k = (i + 1) as f32;
            frame[0] += sample * (l0 + dl * k);
            frame[1] += sample * (r0 + dr * k);
            if self.done {
                break;
            }
        }
        self.gain = self.target;
    }
}

#[derive(Clone, Copy)]
enum Over {
    None,
    Rand,
    Var,
}

#[derive(Clone, Copy)]
enum Arg {
    U8,
    S8,
    U16,
    S16,
    Vlq,
}

struct Cursor<'a> {
    stream: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.stream.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.u8()?) | u16::from(self.u8()?) << 8)
    }

    fn u24(&mut self) -> Option<usize> {
        Some(usize::from(self.u8()?) | usize::from(self.u8()?) << 8 | usize::from(self.u8()?) << 16)
    }

    /// Big-endian 7-bit groups, high bit set on all but the last.
    fn vlq(&mut self) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            value = value << 7 | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use dq9_formats::sdat::{Instrument, Note, NoteKind};

    use super::*;

    const RATE: u32 = 48_000;

    /// A bank of one instrument playing a looping sine of 32 samples a cycle at 32728.5 Hz
    /// (timer 512), which sounds at 1022.8 Hz on key 69.
    fn song(stream: &[u8], envelope: [u8; 4]) -> Arc<Song> {
        let samples: Vec<i16> = (0..64)
            .map(|i| ((i as f64 / 32.0 * std::f64::consts::TAU).sin() * 16000.0) as i16)
            .collect();
        let mut waves = HashMap::new();
        waves.insert(
            (0, 0),
            Arc::new(Samples {
                samples,
                loop_start: Some(0),
                rate: 32728,
                timer: 512,
            }),
        );
        let [attack, decay, sustain, release] = envelope;
        Arc::new(Song {
            index: 0,
            name: None,
            player: 0,
            priority: 0,
            stream: stream.into(),
            start: 0,
            volume: 127,
            instruments: vec![Instrument::Single(Note {
                kind: NoteKind::Pcm,
                wave: 0,
                archive: 0,
                base_key: 69,
                attack,
                decay,
                sustain,
                release,
                pan: 64,
            })]
            .into(),
            waves: Arc::new(waves),
        })
    }

    const INSTANT: [u8; 4] = [127, 127, 127, 127];

    fn render(song: Arc<Song>, seconds: f32) -> Vec<f32> {
        let mut player = Player::new(song, RATE);
        let mut out = vec![0.0; (seconds * RATE as f32) as usize * 2];
        player.render(&mut out);
        out
    }

    /// Frequency of the left channel from rising zero crossings in `[from, to)` frames.
    fn frequency(out: &[f32], from: usize, to: usize) -> f64 {
        let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        let crossings: Vec<usize> = (from.max(1)..to)
            .filter(|&i| left[i - 1] < 0.0 && left[i] >= 0.0)
            .collect();
        let cycles = (crossings.len() - 1) as f64;
        cycles * f64::from(RATE) / (crossings[crossings.len() - 1] - crossings[0]) as f64
    }

    #[test]
    fn notes_sound_at_their_pitch() {
        // Key 69 then key 81 (an octave up), 96 ticks each at 120 BPM (1 s).
        let out = render(
            song(&[0xe1, 120, 0, 69, 127, 96, 81, 127, 96, 0xff], INSTANT),
            2.0,
        );
        let low = frequency(&out, 4_800, 43_000);
        let high = frequency(&out, 52_800, 91_000);
        assert!((low - 1022.8).abs() < 5.0, "{low}");
        assert!((high / low - 2.0).abs() < 0.01, "{high} / {low}");
    }

    #[test]
    fn tempo_sets_the_note_length() {
        // 48 ticks, one quarter note: half a second at 120 BPM, a quarter at 240.
        for (bpm, expected) in [(120u8, 0.5), (240, 0.25)] {
            let out = render(song(&[0xe1, bpm, 0, 69, 127, 48, 0xff], INSTANT), 1.0);
            let last = out
                .as_chunks::<2>()
                .0
                .iter()
                .rposition(|f| f[0].abs() > 0.01)
                .unwrap();
            let seconds = last as f64 / f64::from(RATE);
            assert!((seconds - expected).abs() < 0.015, "{bpm} BPM: {seconds}");
        }
    }

    #[test]
    fn loops_repeat_their_body_and_end() {
        // Three notes of 12 ticks: 36 ticks, 0.375 s at 120 BPM.
        let stream = [0xe1, 120, 0, 0xd4, 3, 69, 127, 12, 0xfc, 0xff];
        let out = render(song(&stream, INSTANT), 1.0);
        let last = out
            .as_chunks::<2>()
            .0
            .iter()
            .rposition(|f| f[0].abs() > 0.01)
            .unwrap();
        assert!((last as f64 / f64::from(RATE) - 0.375).abs() < 0.015);
    }

    #[test]
    fn infinite_loops_are_counted_and_never_finish() {
        let mut player = Player::new(
            song(&[0xe1, 120, 0, 69, 127, 12, 0x94, 3, 0, 0], INSTANT),
            RATE,
        );
        let mut out = vec![0.0; RATE as usize * 2];
        player.render(&mut out);
        // 12 ticks = 0.125 s per pass: about 8 passes a second.
        assert!((7..=9).contains(&player.loops()), "{}", player.loops());
        assert!(!player.finished());
    }

    #[test]
    fn pan_and_volume_commands_place_the_sound() {
        // Track pan hard left; then centre with a quarter of the volume.
        let left = render(song(&[0xc0, 0, 69, 127, 24, 0xff], INSTANT), 0.2);
        let (l, r): (f32, f32) = left
            .as_chunks::<2>()
            .0
            .iter()
            .fold((0.0, 0.0), |(l, r), f| {
                (l.max(f[0].abs()), r.max(f[1].abs()))
            });
        assert!(l > 0.1 && r < 1e-4, "{l} {r}");
        let loud = render(song(&[69, 127, 24, 0xff], INSTANT), 0.2);
        let quiet = render(song(&[0xc1, 64, 69, 127, 24, 0xff], INSTANT), 0.2);
        let peak = |v: &[f32]| v.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(
            peak(&quiet) < peak(&loud) * 0.4,
            "{} {}",
            peak(&quiet),
            peak(&loud)
        );
    }

    #[test]
    fn release_and_envelope_shape_the_tail() {
        // A slow release keeps sounding after the note ended; the instant one does not.
        let stream = [0xe1, 120, 0, 69, 127, 24, 0xff];
        let tail = |release| {
            let out = render(song(&stream, [127, 127, 127, release]), 1.0);
            out.as_chunks::<2>()
                .0
                .iter()
                .rposition(|f| f[0].abs() > 0.01)
                .unwrap() as f64
                / f64::from(RATE)
        };
        assert!(tail(60) > tail(127) + 0.3, "{} {}", tail(60), tail(127));
    }

    #[test]
    fn garbage_streams_end_without_panic() {
        let mut state = 0x1234_5678u32;
        for _ in 0..200 {
            let stream: Vec<u8> = (0..64)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let mut player = Player::new(song(&stream, INSTANT), RATE);
            let mut out = vec![0.0; 4_800 * 2];
            for _ in 0..20 {
                player.render(&mut out);
            }
            assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        }
    }

    #[test]
    fn a_fade_finishes_the_player() {
        let mut player = Player::new(song(&[69, 127, 0, 0x94, 0, 0, 0], INSTANT), RATE);
        player.fade_out(0.5);
        let mut out = vec![0.0; RATE as usize * 2];
        player.render(&mut out);
        assert!(player.finished());
        assert!(out[out.len() - 100..].iter().all(|s| s.abs() < 1e-6));
    }
}
