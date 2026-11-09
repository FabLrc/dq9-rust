//! Wave archives (SWAR): wave encodings and IMA-ADPCM decoding.

use super::{FILE_HEADER, magic};
use nds_rom::bytes::{slice, u8_at, u16_at, u32_at};
use nds_rom::{Error, Result};

/// Sample encodings of a wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveFormat {
    Pcm8,
    Pcm16,
    ImaAdpcm,
}

/// One wave of a wave archive: its 12-byte info block and encoded samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wave<'a> {
    pub format: WaveFormat,
    pub looping: bool,
    pub rate: u16,
    /// ARM7 timer value: `16756991 / rate`.
    pub timer: u16,
    /// Loop start in 32-bit words (ADPCM: counting its 4-byte header).
    pub loop_start: u16,
    /// Length after the loop start, in words.
    pub length: u32,
    pub data: &'a [u8],
}

/// A wave decoded to 16-bit samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Samples {
    pub samples: Vec<i16>,
    /// Sample the playback returns to at the end, when the wave loops.
    pub loop_start: Option<usize>,
    pub rate: u16,
    pub timer: u16,
}

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX_TABLE: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

impl Wave<'_> {
    /// Decodes the samples (GBATEK "DS Sound Notes" for IMA-ADPCM: header, then two nibbles a
    /// byte, low nibble first; value clamped to ±0x7fff, index to 0..=88).
    pub fn decode(&self) -> Result<Samples> {
        let total_words = usize::from(self.loop_start) + self.length as usize;
        let (samples, loop_start) = match self.format {
            WaveFormat::Pcm8 => (
                self.data.iter().map(|&b| i16::from(b as i8) << 8).collect(),
                usize::from(self.loop_start) * 4,
            ),
            WaveFormat::Pcm16 => (
                self.data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| i16::from_le_bytes(*c))
                    .collect(),
                usize::from(self.loop_start) * 2,
            ),
            WaveFormat::ImaAdpcm => {
                let header = slice(self.data, 0, 4)?;
                let mut value = i32::from(i16::from_le_bytes([header[0], header[1]]));
                let mut index =
                    i32::from(u16::from_le_bytes([header[2], header[3]]) & 0x7f).min(88);
                let mut out = Vec::with_capacity((self.data.len() - 4) * 2);
                for &byte in &self.data[4..] {
                    for nibble in [byte & 0xf, byte >> 4] {
                        let step = STEP_TABLE[index as usize];
                        let mut diff = step >> 3;
                        if nibble & 1 != 0 {
                            diff += step >> 2;
                        }
                        if nibble & 2 != 0 {
                            diff += step >> 1;
                        }
                        if nibble & 4 != 0 {
                            diff += step;
                        }
                        value += if nibble & 8 != 0 { -diff } else { diff };
                        value = value.clamp(-0x7fff, 0x7fff);
                        index = (index + INDEX_TABLE[usize::from(nibble & 7)]).clamp(0, 88);
                        out.push(value as i16);
                    }
                }
                // The loop start counts the header word as eight samples.
                (out, (usize::from(self.loop_start) * 8).saturating_sub(8))
            }
        };
        if total_words == 0 || samples.is_empty() {
            return Err(Error::Malformed("empty wave"));
        }
        Ok(Samples {
            loop_start: (self.looping && loop_start < samples.len()).then_some(loop_start),
            samples,
            rate: self.rate,
            timer: self.timer,
        })
    }
}

/// A parsed wave archive.
#[derive(Debug, Clone)]
pub struct Swar<'a> {
    pub waves: Vec<Wave<'a>>,
}

impl<'a> Swar<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self> {
        magic(file, 0, *b"SWAR", "SWAR")?;
        magic(file, FILE_HEADER, *b"DATA", "SWAR DATA")?;
        let count = u32_at(file, 0x38)? as usize;
        let offsets = slice(
            file,
            0x3c,
            count.checked_mul(4).ok_or(Error::Malformed("count"))?,
        )?;
        let mut waves = Vec::with_capacity(count);
        for o in offsets.as_chunks::<4>().0 {
            let at = u32::from_le_bytes(*o) as usize;
            if at == 0 {
                // An empty slot: keep the numbering with a silent placeholder.
                waves.push(Wave {
                    format: WaveFormat::Pcm8,
                    looping: false,
                    rate: 0,
                    timer: 0,
                    loop_start: 0,
                    length: 0,
                    data: &[],
                });
                continue;
            }
            let format = match u8_at(file, at)? {
                0 => WaveFormat::Pcm8,
                1 => WaveFormat::Pcm16,
                2 => WaveFormat::ImaAdpcm,
                _ => return Err(Error::Malformed("unknown wave format")),
            };
            let loop_start = u16_at(file, at + 6)?;
            let length = u32_at(file, at + 8)?;
            let words = usize::from(loop_start)
                .checked_add(length as usize)
                .ok_or(Error::Malformed("wave length"))?;
            waves.push(Wave {
                format,
                looping: u8_at(file, at + 1)? != 0,
                rate: u16_at(file, at + 2)?,
                timer: u16_at(file, at + 4)?,
                loop_start,
                length,
                data: slice(file, at + 12, words * 4)?,
            });
        }
        Ok(Self { waves })
    }
}
