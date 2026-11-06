//! Nitro sound archives (`.sdat`, `data/sound/`): the container (SYMB names, INFO records,
//! FAT file table) and the files inside it a sequencer needs: sequences (SSEQ), instrument
//! banks (SBNK) and wave archives (SWAR, whose waves are SWAV without a file header).
//! Sequence archives (SSAR, sound effects) and streams (STRM) are not read here.
//!
//! Layout and evidence in `docs/re/sdat.md`. The structures are the SDK's data, read from the
//! published formats (wiki SDAT page, GBATEK "DS Files" and "DS Sound"); the SDK's sound code
//! is platform and is replaced by `dq9-audio`.
//!
//! Every reader is bounds-checked and returns [`Error`] on malformed data.

use nds_rom::{Error, Result};

const SDAT_MAGIC: [u8; 4] = *b"SDAT";
/// Header of every file inside the archive: stamp, byte order mark, version, size, header
/// size, block count, then the `DATA` (or `SYMB`, `INFO`...) block.
const FILE_HEADER: usize = 0x10;
/// Marks an unused bank wave-archive slot.
pub const NO_WAVE_ARCHIVE: u16 = 0xffff;

fn slice(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    offset
        .checked_add(len)
        .and_then(|end| data.get(offset..end))
        .ok_or(Error::OutOfBounds {
            offset,
            len,
            size: data.len(),
        })
}

fn u8_at(data: &[u8], offset: usize) -> Result<u8> {
    Ok(slice(data, offset, 1)?[0])
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    let b = slice(data, offset, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let b = slice(data, offset, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn magic(data: &[u8], offset: usize, expected: [u8; 4], what: &'static str) -> Result<()> {
    let found: [u8; 4] = slice(data, offset, 4)?.try_into().expect("4 bytes");
    if found == expected {
        Ok(())
    } else {
        Err(Error::BadMagic { what, found })
    }
}

/// One SSEQ record of INFO (`+0x00` file, `+0x04` bank, `+0x06` volume...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceInfo {
    /// FAT index of the SSEQ file.
    pub file: u16,
    /// Index in the bank list.
    pub bank: u16,
    /// Sequence volume, 0 to 127.
    pub volume: u8,
    /// Priority of the sequence's notes for the hardware channels (`+0x07`).
    pub channel_priority: u8,
    /// Priority of the sequence among those of its player (`+0x08`).
    pub player_priority: u8,
    /// Index in the PLAYER list (`+0x09`).
    pub player: u8,
}

/// One SBNK record of INFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankInfo {
    pub file: u16,
    /// Wave archive indices for the instruments' four slots; [`NO_WAVE_ARCHIVE`] when unused.
    pub wave_archives: [u16; 4],
}

/// One SWAR record of INFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveArchiveInfo {
    pub file: u16,
}

/// Positions of the eight record lists, in SYMB and INFO alike.
pub const KIND_SEQUENCE: usize = 0;
pub const KIND_SEQUENCE_ARCHIVE: usize = 1;
pub const KIND_BANK: usize = 2;
pub const KIND_WAVE_ARCHIVE: usize = 3;
pub const KIND_PLAYER: usize = 4;
pub const KIND_GROUP: usize = 5;
pub const KIND_STREAM_PLAYER: usize = 6;
pub const KIND_STREAM: usize = 7;
const KINDS: usize = 8;

/// One SSAR record of INFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceArchiveInfo {
    /// FAT index of the SSAR file.
    pub file: u16,
}

/// One PLAYER record of INFO: how many sequences a player runs at once, and which hardware
/// channels its notes may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerInfo {
    /// Sequences that may play at the same time on this player.
    pub max_sequences: u16,
    /// Hardware channels the player may take, one bit each.
    pub channels: u16,
    /// Bytes of heap the player reserves for its tracks.
    pub heap: u32,
}

impl PlayerInfo {
    fn parse(record: &[u8]) -> Result<Self> {
        Ok(Self {
            max_sequences: u16_at(record, 0)?,
            channels: u16_at(record, 2)?,
            heap: u32_at(record, 4)?,
        })
    }
}

/// What a group entry loads: `+0x00` type (0 sequence, 1 bank, 2 wave archive, 3 sequence
/// archive), `+0x01` load flags, `+0x04` index in the list of that type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupEntry {
    pub kind: u8,
    /// Bit 0: the sequence file itself; bit 1: its bank; bit 2: its wave archives.
    pub flags: u8,
    pub index: u32,
}

/// The entries of a GROUP record: a count, then 8-byte entries.
fn group_entries(record: &[u8]) -> Result<Vec<GroupEntry>> {
    let count = u32_at(record, 0)? as usize;
    let raw = slice(
        record,
        4,
        count.checked_mul(8).ok_or(Error::Malformed("count"))?,
    )?;
    Ok(raw
        .as_chunks::<8>()
        .0
        .iter()
        .map(|e| GroupEntry {
            kind: e[0],
            flags: e[1],
            index: u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
        })
        .collect())
}

/// A parsed archive: the file table, the typed sequence, bank, wave-archive, sequence-archive,
/// player and group records, and the raw lists with their names when the archive has a SYMB
/// block.
#[derive(Debug, Clone)]
pub struct Sdat<'a> {
    data: &'a [u8],
    /// `(offset, size)` of each file, absolute in the archive.
    files: Vec<(u32, u32)>,
    sequences: Vec<Option<SequenceInfo>>,
    banks: Vec<Option<BankInfo>>,
    wave_archives: Vec<Option<WaveArchiveInfo>>,
    sequence_archives: Vec<Option<SequenceArchiveInfo>>,
    players: Vec<Option<PlayerInfo>>,
    groups: Vec<Option<Vec<GroupEntry>>>,
    /// The INFO record of every list entry.
    lists: Vec<Vec<Option<&'a [u8]>>>,
    /// The SYMB name of every list entry.
    names: Vec<Vec<Option<&'a str>>>,
}

/// NUL-terminated string at `offset` of `block`, when it is UTF-8.
fn cstr(block: &[u8], offset: usize) -> Option<&str> {
    let tail = block.get(offset..)?;
    let end = tail.iter().position(|&b| b == 0)?;
    std::str::from_utf8(&tail[..end]).ok()
}

/// The `index`-th list of the SYMB or INFO `block`: its entries' offsets (0 = empty slot),
/// relative to the block.
fn list(block: &[u8], index: usize) -> Result<Vec<u32>> {
    let at = u32_at(block, 8 + 4 * index)? as usize;
    if at == 0 {
        return Ok(Vec::new());
    }
    let count = u32_at(block, at)? as usize;
    // Each entry is 4 bytes: a count past the block is malformed, not an allocation.
    let entries = slice(
        block,
        at + 4,
        count.checked_mul(4).ok_or(Error::Malformed("count"))?,
    )?;
    Ok(entries
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect())
}

impl<'a> Sdat<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        magic(data, 0, SDAT_MAGIC, "SDAT")?;
        let block = |at: usize, stamp: [u8; 4], what: &'static str| -> Result<&'a [u8]> {
            let offset = u32_at(data, at)? as usize;
            let size = u32_at(data, at + 4)? as usize;
            let block = slice(data, offset, size)?;
            magic(block, 0, stamp, what)?;
            Ok(block)
        };
        let symb = block(0x10, *b"SYMB", "SDAT SYMB");
        let info = block(0x18, *b"INFO", "SDAT INFO")?;
        let fat = block(0x20, *b"FAT ", "SDAT FAT")?;

        let count = u32_at(fat, 8)? as usize;
        let table = slice(
            fat,
            12,
            count.checked_mul(16).ok_or(Error::Malformed("count"))?,
        )?;
        let files = table
            .as_chunks::<16>()
            .0
            .iter()
            .map(|e| {
                (
                    u32::from_le_bytes([e[0], e[1], e[2], e[3]]),
                    u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
                )
            })
            .collect();

        let lists = (0..KINDS)
            .map(|kind| -> Result<Vec<Option<&'a [u8]>>> {
                list(info, kind)?
                    .into_iter()
                    .map(|o| match o {
                        0 => Ok(None),
                        o => Ok(Some(info.get(o as usize..).ok_or(Error::OutOfBounds {
                            offset: o as usize,
                            len: 1,
                            size: info.len(),
                        })?)),
                    })
                    .collect()
            })
            .collect::<Result<Vec<_>>>()?;
        let sequences = lists[KIND_SEQUENCE]
            .iter()
            .map(|r| {
                r.map(|r| {
                    Ok(SequenceInfo {
                        file: u16_at(r, 0)?,
                        bank: u16_at(r, 4)?,
                        volume: u8_at(r, 6)?,
                        channel_priority: u8_at(r, 7)?,
                        player_priority: u8_at(r, 8)?,
                        player: u8_at(r, 9)?,
                    })
                })
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        let banks = lists[KIND_BANK]
            .iter()
            .map(|r| {
                r.map(|r| {
                    let mut wave_archives = [NO_WAVE_ARCHIVE; 4];
                    for (i, w) in wave_archives.iter_mut().enumerate() {
                        *w = u16_at(r, 4 + 2 * i)?;
                    }
                    Ok(BankInfo {
                        file: u16_at(r, 0)?,
                        wave_archives,
                    })
                })
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        let wave_archives = lists[KIND_WAVE_ARCHIVE]
            .iter()
            .map(|r| {
                r.map(|r| {
                    Ok(WaveArchiveInfo {
                        file: u16_at(r, 0)?,
                    })
                })
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        let sequence_archives = lists[KIND_SEQUENCE_ARCHIVE]
            .iter()
            .map(|r| {
                r.map(|r| {
                    Ok(SequenceArchiveInfo {
                        file: u16_at(r, 0)?,
                    })
                })
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        let players = lists[KIND_PLAYER]
            .iter()
            .map(|r| r.map(PlayerInfo::parse).transpose())
            .collect::<Result<Vec<_>>>()?;
        let groups = lists[KIND_GROUP]
            .iter()
            .map(|r| r.map(group_entries).transpose())
            .collect::<Result<Vec<_>>>()?;

        // Names are a convenience: a damaged SYMB block loses them, not the archive.
        let names = (0..KINDS)
            .map(|kind| match symb.as_ref() {
                Ok(s) => list(s, kind)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|o| {
                        if o == 0 {
                            return None;
                        }
                        // A sequence archive's entry is {name, names of its sequences}.
                        let at = if kind == KIND_SEQUENCE_ARCHIVE {
                            u32_at(s, o as usize).ok()?
                        } else {
                            o
                        };
                        cstr(s, at as usize)
                    })
                    .collect(),
                Err(_) => Vec::new(),
            })
            .collect();

        Ok(Self {
            data,
            files,
            sequences,
            banks,
            wave_archives,
            sequence_archives,
            players,
            groups,
            lists,
            names,
        })
    }

    /// Size of the sequence list (empty slots included).
    pub fn sequence_count(&self) -> usize {
        self.sequences.len()
    }

    /// The sequence record at `index`; `None` for an empty slot or a missing index.
    pub fn sequence_info(&self, index: usize) -> Option<&SequenceInfo> {
        self.sequences.get(index)?.as_ref()
    }

    /// The sequence's name (`BG_001`, `ME_005`...).
    pub fn sequence_name(&self, index: usize) -> Option<&'a str> {
        self.name(KIND_SEQUENCE, index)
    }

    /// Index of the sequence called `name`.
    pub fn find_sequence(&self, name: &str) -> Option<usize> {
        self.names[KIND_SEQUENCE]
            .iter()
            .position(|n| *n == Some(name))
    }

    /// Index of the sequence archive called `name`.
    pub fn find_sequence_archive(&self, name: &str) -> Option<usize> {
        self.names[KIND_SEQUENCE_ARCHIVE]
            .iter()
            .position(|n| *n == Some(name))
    }

    /// The raw INFO record of `kind` (`KIND_*`) at `index`: its bytes run to the end of the
    /// block, records do not declare their length. `None` for an empty slot.
    pub fn record(&self, kind: usize, index: usize) -> Option<&'a [u8]> {
        self.lists.get(kind)?.get(index).copied().flatten()
    }

    /// Size of the list of `kind` (empty slots included).
    pub fn list_len(&self, kind: usize) -> usize {
        self.lists.get(kind).map_or(0, Vec::len)
    }

    /// The SYMB name of the record of `kind` at `index`.
    pub fn name(&self, kind: usize, index: usize) -> Option<&'a str> {
        self.names.get(kind)?.get(index).copied().flatten()
    }

    /// Size of the sequence-archive list (empty slots included).
    pub fn sequence_archive_count(&self) -> usize {
        self.sequence_archives.len()
    }

    /// The sequence archive (SSAR) `index`: its record and parsed file.
    pub fn sequence_archive(&self, index: usize) -> Result<(SequenceArchiveInfo, Ssar<'a>)> {
        let info = *self
            .sequence_archives
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::NotFound(format!("SDAT sequence archive {index}")))?;
        Ok((info, Ssar::parse(self.file(info.file)?)?))
    }

    /// The player record `index`: the polyphony limit of a group of sequences.
    pub fn player(&self, index: usize) -> Option<PlayerInfo> {
        self.players.get(index).copied().flatten()
    }

    /// The group record `index`: what a load request brings into memory together.
    pub fn group(&self, index: usize) -> Option<&[GroupEntry]> {
        self.groups.get(index)?.as_deref()
    }

    /// Bytes of the file with this FAT index.
    pub fn file(&self, id: u16) -> Result<&'a [u8]> {
        let &(offset, size) = self
            .files
            .get(usize::from(id))
            .ok_or_else(|| Error::NotFound(format!("SDAT file {id}")))?;
        slice(self.data, offset as usize, size as usize)
    }

    /// The sequence `index`: its record and parsed file.
    pub fn sequence(&self, index: usize) -> Result<(SequenceInfo, Sseq<'a>)> {
        let info = *self
            .sequence_info(index)
            .ok_or_else(|| Error::NotFound(format!("SDAT sequence {index}")))?;
        Ok((info, Sseq::parse(self.file(info.file)?)?))
    }

    /// The bank record at `index`.
    pub fn bank_info(&self, index: usize) -> Result<&BankInfo> {
        self.banks
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::NotFound(format!("SDAT bank {index}")))
    }

    /// Parses bank `index`.
    pub fn bank(&self, index: usize) -> Result<Sbnk> {
        Sbnk::parse(self.file(self.bank_info(index)?.file)?)
    }

    /// Parses wave archive `index`.
    pub fn wave_archive(&self, index: usize) -> Result<Swar<'a>> {
        let info = self
            .wave_archives
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::NotFound(format!("SDAT wave archive {index}")))?;
        Swar::parse(self.file(info.file)?)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(v: &mut Vec<u8>, x: u16) {
        v.extend(x.to_le_bytes());
    }
    fn put32(v: &mut Vec<u8>, x: u32) {
        v.extend(x.to_le_bytes());
    }

    /// A tiny archive: sequence 1 (file 0) -> bank 0 (file 1) -> wave archive 0 (file 2).
    fn archive() -> Vec<u8> {
        let sseq = {
            let mut f = b"SSEQ".to_vec();
            f.extend([0xff, 0xfe, 0, 1]);
            put32(&mut f, 0x1f);
            put16(&mut f, 0x10);
            put16(&mut f, 1);
            f.extend(b"DATA");
            put32(&mut f, 0xf);
            put32(&mut f, 0x1c);
            f.extend([0xff, 0xff, 0xff]);
            f
        };
        let sbnk = {
            let mut f = b"SBNK".to_vec();
            f.extend([0xff, 0xfe, 0, 1]);
            put32(&mut f, 0x6c);
            put16(&mut f, 0x10);
            put16(&mut f, 1);
            f.extend(b"DATA");
            put32(&mut f, 0x5c);
            f.extend([0; 32]);
            put32(&mut f, 2);
            // 0: single PCM note at 0x44; 1: key split at 0x4e.
            f.extend([1, 0x44, 0, 0]);
            f.extend([17, 0x4e, 0, 0]);
            assert_eq!(f.len(), 0x44);
            put16(&mut f, 3);
            put16(&mut f, 0);
            f.extend([60, 127, 127, 100, 127, 64]);
            assert_eq!(f.len(), 0x4e);
            f.extend([59, 0, 0, 0, 0, 0, 0, 0]);
            put16(&mut f, 1);
            f.extend([0, 0, 0, 0, 48, 127, 127, 127, 127, 64]);
            f
        };
        let swar = {
            let mut f = b"SWAR".to_vec();
            f.extend([0xff, 0xfe, 0, 1]);
            put32(&mut f, 0x54);
            put16(&mut f, 0x10);
            put16(&mut f, 1);
            f.extend(b"DATA");
            put32(&mut f, 0x44);
            f.extend([0; 32]);
            put32(&mut f, 1);
            put32(&mut f, 0x40);
            // One looping PCM8 wave, 4 samples, loop at word 0.
            f.extend([0, 1]);
            put16(&mut f, 8000);
            put16(&mut f, 2094);
            put16(&mut f, 0);
            put32(&mut f, 1);
            f.extend([0x10, 0x20, 0x80, 0x7f]);
            f
        };
        let files = [sseq, sbnk, swar];
        let (symb_len, info_len) = (0x60usize, 0x80usize);
        let fat_len = 12 + 16 * files.len();
        let mut out = b"SDAT".to_vec();
        out.extend([0xff, 0xfe, 0, 1]);
        put32(&mut out, 0); // size, patched below
        put16(&mut out, 0x40);
        put16(&mut out, 4);
        let symb_at = 0x40;
        let info_at = symb_at + symb_len;
        let fat_at = info_at + info_len;
        let file_at = fat_at + fat_len;
        for (at, len) in [(symb_at, symb_len), (info_at, info_len), (fat_at, fat_len)] {
            put32(&mut out, at as u32);
            put32(&mut out, len as u32);
        }
        let files_len: usize = files.iter().map(Vec::len).sum();
        put32(&mut out, file_at as u32);
        put32(&mut out, (8 + files_len) as u32);
        out.resize(symb_at, 0);
        // SYMB: sequence list (3 slots, name of slot 1), other lists empty.
        let mut symb = b"SYMB".to_vec();
        put32(&mut symb, symb_len as u32);
        put32(&mut symb, 0x28);
        for _ in 1..8 {
            put32(&mut symb, 0);
        }
        put32(&mut symb, 3);
        put32(&mut symb, 0);
        put32(&mut symb, 0x40);
        put32(&mut symb, 0);
        symb.resize(0x40, 0);
        symb.extend(b"BG_TEST\0");
        symb.resize(symb_len, 0);
        out.extend(symb);
        // INFO: sequence list at 0x28 (slot 1 -> record at 0x50), bank list at 0x34 (record at
        // 0x5c), wave archive list at 0x3c (record at 0x68).
        let mut info = b"INFO".to_vec();
        put32(&mut info, info_len as u32);
        for list in [0x28, 0, 0x34, 0x3c, 0, 0, 0, 0] {
            put32(&mut info, list);
        }
        assert_eq!(info.len(), 0x28);
        put32(&mut info, 2);
        put32(&mut info, 0);
        put32(&mut info, 0x50);
        put32(&mut info, 1);
        put32(&mut info, 0x5c);
        put32(&mut info, 1);
        put32(&mut info, 0x68);
        assert_eq!(info.len(), 0x44);
        info.resize(0x50, 0);
        put16(&mut info, 0); // file
        put16(&mut info, 0);
        put16(&mut info, 0); // bank
        info.extend([100, 0, 0, 64, 0, 0]);
        assert_eq!(info.len(), 0x5c);
        put16(&mut info, 1); // bank record: file 1
        put16(&mut info, 0);
        put16(&mut info, 0); // wave archive 0
        for _ in 0..3 {
            put16(&mut info, NO_WAVE_ARCHIVE);
        }
        assert_eq!(info.len(), 0x68);
        put16(&mut info, 2); // wave archive record: file 2
        info.resize(info_len, 0);
        out.extend(info);
        // FAT.
        out.extend(b"FAT ");
        put32(&mut out, fat_len as u32);
        put32(&mut out, files.len() as u32);
        let mut at = file_at + 8;
        for f in &files {
            put32(&mut out, at as u32);
            put32(&mut out, f.len() as u32);
            put32(&mut out, 0);
            put32(&mut out, 0);
            at += f.len();
        }
        out.extend(b"FILE");
        put32(&mut out, (8 + files_len) as u32);
        for f in &files {
            out.extend(f);
        }
        let size = out.len() as u32;
        out[8..12].copy_from_slice(&size.to_le_bytes());
        out
    }

    #[test]
    fn container_resolves_the_chain() {
        let bytes = archive();
        let sdat = Sdat::parse(&bytes).unwrap();
        assert_eq!(sdat.sequence_count(), 2);
        assert!(sdat.sequence_info(0).is_none());
        assert_eq!(sdat.sequence_name(1), Some("BG_TEST"));
        assert_eq!(sdat.find_sequence("BG_TEST"), Some(1));
        let (info, sseq) = sdat.sequence(1).unwrap();
        assert_eq!((info.bank, info.volume, info.player), (0, 100, 64));
        assert_eq!(sseq.stream, [0xff, 0xff, 0xff]);
        let bank = sdat.bank(usize::from(info.bank)).unwrap();
        let swar = sdat.wave_archive(0).unwrap();
        assert_eq!(swar.waves.len(), 1);
        let samples = swar.waves[0].decode().unwrap();
        assert_eq!(samples.samples, [0x1000, 0x2000, -0x8000, 0x7f00]);
        assert_eq!(samples.loop_start, Some(0));
        let Instrument::Single(note) = &bank.instruments[0] else {
            panic!("single instrument expected");
        };
        assert_eq!((note.wave, note.base_key, note.pan), (3, 60, 64));
        assert!(matches!(bank.instruments[1], Instrument::Split { .. }));
        assert!(bank.instruments[1].note(60).is_none());
        assert_eq!(bank.instruments[1].note(59).map(|n| n.base_key), Some(48));
    }

    #[test]
    fn truncations_are_errors_not_panics() {
        let bytes = archive();
        for len in 0..bytes.len() {
            if let Ok(sdat) = Sdat::parse(&bytes[..len]) {
                let _ = sdat.sequence(1);
                let _ = sdat.bank(0).map(|_| ());
                let _ = sdat.wave_archive(0).map(|s| s.waves.len());
            }
        }
        let mut bad = bytes;
        bad[0] = b'X';
        assert!(Sdat::parse(&bad).is_err());
    }

    #[test]
    fn adpcm_follows_the_ima_algorithm() {
        // Header: value 0, index 0; nibbles 7 then 0xf then 0.
        let data = [0, 0, 0, 0, 0xf7, 0x00, 0, 0, 0, 0, 0, 0];
        let wave = Wave {
            format: WaveFormat::ImaAdpcm,
            looping: true,
            rate: 8000,
            timer: 2094,
            loop_start: 2,
            length: 0,
            data: &data,
        };
        let s = wave.decode().unwrap();
        // Step 7: diff 0 + 1 + 3 + 7 = 11; index 0 + 8 = 8 (step 16): -(2 + 4 + 8 + 16) = -30
        // from 11 gives -19; index 8 + 8 = 16, then nibble 0: step 34 >> 3 = 4 added.
        assert_eq!(&s.samples[..3], [11, -19, -15]);
        assert_eq!(s.loop_start, Some(8));
    }
}
