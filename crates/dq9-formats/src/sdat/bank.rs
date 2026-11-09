//! Instrument banks (SBNK): how an instrument produces a note.

use super::{FILE_HEADER, magic};
use nds_rom::bytes::{slice, u8_at, u16_at, u32_at};
use nds_rom::{Error, Result};

/// How a note is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// A wave of one of the bank's wave archives.
    Pcm,
    /// A square wave; [`Note::wave`] is its duty (0 to 6 for 1/8 to 7/8).
    Psg,
    /// The noise channel.
    Noise,
    /// Type 4 (direct PCM) **[unknown]**: read like [`NoteKind::Pcm`].
    Direct,
}

/// One note definition of an instrument (10 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub kind: NoteKind,
    /// Wave index in the wave archive (PCM), or the duty (PSG).
    pub wave: u16,
    /// Which of the bank's four wave-archive slots holds the wave.
    pub archive: u16,
    /// MIDI key the wave plays at its own rate.
    pub base_key: u8,
    /// Raw envelope values (0 to 127; 255 = default in the SDK, read as the same maximum).
    pub attack: u8,
    pub decay: u8,
    pub sustain: u8,
    pub release: u8,
    /// 0 to 127, 64 centre.
    pub pan: u8,
}

/// An instrument of a bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instrument {
    Empty,
    /// One note for every key.
    Single(Note),
    /// One note per key from `low`.
    Drums {
        low: u8,
        notes: Vec<Note>,
    },
    /// One note per key region: region `i` ends at `ends[i]` (inclusive).
    Split {
        ends: Vec<u8>,
        notes: Vec<Note>,
    },
}

impl Instrument {
    /// The note definition played for `key`.
    pub fn note(&self, key: u8) -> Option<&Note> {
        match self {
            Instrument::Empty => None,
            Instrument::Single(n) => Some(n),
            Instrument::Drums { low, notes } => notes.get(usize::from(key.checked_sub(*low)?)),
            Instrument::Split { ends, notes } => {
                let region = ends.iter().position(|&end| key <= end)?;
                notes.get(region)
            }
        }
    }
}

/// A parsed instrument bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sbnk {
    pub instruments: Vec<Instrument>,
}

fn note_def(data: &[u8], at: usize, record_type: u16) -> Result<Option<Note>> {
    let kind = match record_type {
        1 => NoteKind::Pcm,
        2 => NoteKind::Psg,
        3 => NoteKind::Noise,
        4 => NoteKind::Direct,
        // 0 empty, 5 null: nothing plays.
        _ => return Ok(None),
    };
    Ok(Some(Note {
        kind,
        wave: u16_at(data, at)?,
        archive: u16_at(data, at + 2)?,
        base_key: u8_at(data, at + 4)?,
        attack: u8_at(data, at + 5)?,
        decay: u8_at(data, at + 6)?,
        sustain: u8_at(data, at + 7)?,
        release: u8_at(data, at + 8)?,
        pan: u8_at(data, at + 9)?,
    }))
}

impl Sbnk {
    pub fn parse(file: &[u8]) -> Result<Self> {
        magic(file, 0, *b"SBNK", "SBNK")?;
        magic(file, FILE_HEADER, *b"DATA", "SBNK DATA")?;
        let count = u32_at(file, 0x38)? as usize;
        let records = slice(
            file,
            0x3c,
            count.checked_mul(4).ok_or(Error::Malformed("count"))?,
        )?;
        let mut instruments = Vec::with_capacity(count);
        for r in records.as_chunks::<4>().0 {
            let kind = u16::from(r[0]);
            let at = usize::from(u16::from_le_bytes([r[1], r[2]]));
            instruments.push(match kind {
                0 => Instrument::Empty,
                1..=5 => note_def(file, at, kind)?.map_or(Instrument::Empty, Instrument::Single),
                16 => {
                    let (low, high) = (u8_at(file, at)?, u8_at(file, at + 1)?);
                    let keys = usize::from(high.saturating_sub(low)) + 1;
                    let mut notes = Vec::with_capacity(keys);
                    for i in 0..keys {
                        let e = at + 2 + 12 * i;
                        let note = note_def(file, e + 2, u16_at(file, e)?)?;
                        notes.push(note.ok_or(Error::Malformed("empty drum note"))?);
                    }
                    Instrument::Drums { low, notes }
                }
                17 => {
                    let ends = slice(file, at, 8)?;
                    let regions = ends.iter().take_while(|&&e| e != 0).count();
                    let mut notes = Vec::with_capacity(regions);
                    for i in 0..regions {
                        let e = at + 8 + 12 * i;
                        let note = note_def(file, e + 2, u16_at(file, e)?)?;
                        notes.push(note.ok_or(Error::Malformed("empty split note"))?);
                    }
                    Instrument::Split {
                        ends: ends[..regions].to_vec(),
                        notes,
                    }
                }
                _ => return Err(Error::Malformed("unknown instrument type")),
            });
        }
        Ok(Self { instruments })
    }
}
