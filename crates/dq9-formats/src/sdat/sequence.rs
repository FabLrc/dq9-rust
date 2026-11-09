//! Sequences (SSEQ) and sequence archives (SSAR): the command streams.

use super::{FILE_HEADER, magic};
use nds_rom::bytes::{slice, u32_at};
use nds_rom::{Error, Result};

/// A sequence file: the command stream. Jump and call targets, and the track offsets, count
/// from its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sseq<'a> {
    pub stream: &'a [u8],
}

impl<'a> Sseq<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self> {
        magic(file, 0, *b"SSEQ", "SSEQ")?;
        magic(file, FILE_HEADER, *b"DATA", "SSEQ DATA")?;
        let size = u32_at(file, FILE_HEADER + 4)? as usize;
        let start = u32_at(file, FILE_HEADER + 8)? as usize;
        let end = FILE_HEADER.saturating_add(size).min(file.len());
        if start > end {
            return Err(Error::Malformed("SSEQ stream starts past its block"));
        }
        Ok(Self {
            stream: &file[start..end],
        })
    }
}

/// One sequence of a sequence archive (12 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsarEntry {
    /// Where the sequence starts in the archive's stream; `None` for an empty slot.
    pub offset: Option<u32>,
    pub bank: u16,
    pub volume: u8,
    pub channel_priority: u8,
    pub player_priority: u8,
    /// Index in the PLAYER list.
    pub player: u8,
}

/// A sequence archive (SSAR, the sound effects): many short sequences sharing one command
/// stream. Track and jump offsets count from the stream's start, as in a sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ssar<'a> {
    pub stream: &'a [u8],
    pub entries: Vec<SsarEntry>,
}

/// Offset marking an unused slot of a sequence archive.
const SSAR_EMPTY: u32 = 0xffff_ffff;

impl<'a> Ssar<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self> {
        magic(file, 0, *b"SSAR", "SSAR")?;
        magic(file, FILE_HEADER, *b"DATA", "SSAR DATA")?;
        let size = u32_at(file, FILE_HEADER + 4)? as usize;
        let start = u32_at(file, FILE_HEADER + 8)? as usize;
        let count = u32_at(file, FILE_HEADER + 12)? as usize;
        let end = FILE_HEADER.saturating_add(size).min(file.len());
        if start > end {
            return Err(Error::Malformed("SSAR stream starts past its block"));
        }
        let raw = slice(
            file,
            FILE_HEADER + 16,
            count.checked_mul(12).ok_or(Error::Malformed("count"))?,
        )?;
        let entries = raw
            .as_chunks::<12>()
            .0
            .iter()
            .map(|e| {
                let offset = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
                SsarEntry {
                    offset: (offset != SSAR_EMPTY).then_some(offset),
                    bank: u16::from_le_bytes([e[4], e[5]]),
                    volume: e[6],
                    channel_priority: e[7],
                    player_priority: e[8],
                    player: e[9],
                }
            })
            .collect();
        Ok(Self {
            stream: &file[start..end],
            entries,
        })
    }
}
