//! A voice of the mixer: its wave source, envelope, pitch and gains.

use std::sync::Arc;

use dq9_formats::sdat::Samples;

use super::{TIMER_HZ, Track};
use crate::tables::{self, AMPL_MIN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug)]
pub(super) enum Source {
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
pub(super) struct Voice {
    pub(super) id: u32,
    pub(super) track: usize,
    pub(super) key: u8,
    pub(super) velocity: u8,
    /// Key at which the wave plays at its own rate (PCM) or the PSG's middle C.
    pub(super) base_key: u8,
    pub(super) note_pan: i32,
    pub(super) source: Source,
    pub(super) stage: Stage,
    pub(super) ampl: i32,
    pub(super) sustain_level: i32,
    pub(super) attack_rate: i32,
    pub(super) decay_rate: i32,
    pub(super) release_rate: i32,
    /// Ticks until the key goes up; `None` while held.
    pub(super) remaining: Option<u32>,
    pub(super) sweep_pitch: i32,
    pub(super) sweep_len: u32,
    pub(super) sweep_pos: u32,
    pub(super) mod_counter: u32,
    pub(super) mod_delay_left: u32,
    pub(super) step: f64,
    pub(super) gain: [f32; 2],
    pub(super) target: [f32; 2],
    pub(super) fresh: bool,
    pub(super) done: bool,
}

impl Voice {
    /// Whether the wave goes on forever (a loop, PSG, noise) rather than ending by itself.
    pub(super) fn loops(&self) -> bool {
        match &self.source {
            Source::Pcm { samples, .. } => samples.loop_start.is_some(),
            Source::Psg { .. } | Source::Noise { .. } => true,
        }
    }

    pub(super) fn key_off(&mut self) {
        if self.stage != Stage::Release {
            self.stage = Stage::Release;
        }
    }

    /// Steps the envelope, the sweep and the modulation, and sets the gains and the pitch step
    /// until the next update.
    pub(super) fn update(&mut self, track: &Track, master: u8, seq_volume: u8, rate: f64) {
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
    pub(super) fn mix(&mut self, block: &mut [f32], frames: usize) {
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
