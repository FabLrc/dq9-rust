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

use nds_rom::bytes::{magic_at, slice, u8_at, u16_at, u32_at};
use nds_rom::{Error, Result};

mod bank;
mod sequence;
mod wave;

pub use bank::{Instrument, Note, NoteKind, Sbnk};
pub use sequence::{Ssar, SsarEntry, Sseq};
pub use wave::{Samples, Swar, Wave, WaveFormat};

const SDAT_MAGIC: [u8; 4] = *b"SDAT";
/// Header of every file inside the archive: stamp, byte order mark, version, size, header
/// size, block count, then the `DATA` (or `SYMB`, `INFO`...) block.
pub(super) const FILE_HEADER: usize = 0x10;
/// Marks an unused bank wave-archive slot.
pub const NO_WAVE_ARCHIVE: u16 = 0xffff;

pub(super) fn magic(
    data: &[u8],
    offset: usize,
    expected: [u8; 4],
    what: &'static str,
) -> Result<()> {
    let found = magic_at(data, offset)?;
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
