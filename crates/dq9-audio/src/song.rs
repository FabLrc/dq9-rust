//! A sequence ready to play: its command stream, its bank's instruments and the decoded
//! waves they use. Music sequences (SSEQ) and the sound effects of a sequence archive (SSAR)
//! load the same way; the effects of an archive share their stream, bank and waves.

use std::collections::HashMap;
use std::sync::Arc;

use dq9_formats::sdat::{Instrument, NO_WAVE_ARCHIVE, Samples, Sdat};
use nds_rom::{Error, Result};

type Waves = HashMap<(u16, u16), Arc<Samples>>;

/// A loaded sequence. Immutable and shared: any number of [`crate::Player`]s can play it.
#[derive(Debug)]
pub struct Song {
    /// Index in the archive's sequence list, or in the sequence archive.
    pub index: usize,
    /// Name in the archive (`BG_005`); effects have none.
    pub name: Option<String>,
    /// Index in the PLAYER list of the archive: the group of sequences it competes with.
    pub player: u8,
    /// Priority among the sequences of its player: the higher wins.
    pub priority: u8,
    pub(crate) stream: Arc<[u8]>,
    /// Where the first track starts in the stream (0 for a sequence, the entry's offset in
    /// a sequence archive).
    pub(crate) start: usize,
    /// The sequence's own volume (INFO record or SSAR entry), 0 to 127.
    pub(crate) volume: u8,
    pub(crate) instruments: Arc<[Instrument]>,
    /// Decoded waves by (wave-archive slot of the bank, wave index).
    pub(crate) waves: Arc<Waves>,
}

/// The bank of a sequence with the waves its instruments use, decoded.
struct Kit {
    instruments: Arc<[Instrument]>,
    waves: Arc<Waves>,
}

fn kit(sdat: &Sdat<'_>, bank: u16) -> Result<Kit> {
    let bank_info = *sdat.bank_info(usize::from(bank))?;
    let bank = sdat.bank(usize::from(bank))?;
    let archives: Vec<_> = bank_info
        .wave_archives
        .iter()
        .map(|&a| {
            (a != NO_WAVE_ARCHIVE)
                .then(|| sdat.wave_archive(usize::from(a)))
                .transpose()
        })
        .collect::<Result<_>>()?;

    let mut waves = HashMap::new();
    for instrument in &bank.instruments {
        let notes = match instrument {
            Instrument::Empty => &[][..],
            Instrument::Single(note) => std::slice::from_ref(note),
            Instrument::Drums { notes, .. } | Instrument::Split { notes, .. } => notes,
        };
        for note in notes {
            use dq9_formats::sdat::NoteKind::{Direct, Pcm};
            if !matches!(note.kind, Pcm | Direct) {
                continue;
            }
            let key = (note.archive, note.wave);
            if waves.contains_key(&key) {
                continue;
            }
            let wave = archives
                .get(usize::from(note.archive))
                .and_then(Option::as_ref)
                .and_then(|swar| swar.waves.get(usize::from(note.wave)))
                .ok_or(Error::Malformed("note names a wave the bank does not have"))?;
            waves.insert(key, Arc::new(wave.decode()?));
        }
    }
    Ok(Kit {
        instruments: bank.instruments.into(),
        waves: Arc::new(waves),
    })
}

impl Song {
    /// Bytes of the command stream.
    pub fn stream_len(&self) -> usize {
        self.stream.len()
    }

    /// Loads sequence `index` of `sdat` with its bank and the waves its instruments use.
    pub fn load(sdat: &Sdat<'_>, index: usize) -> Result<Self> {
        let (info, sseq) = sdat.sequence(index)?;
        let kit = kit(sdat, info.bank)?;
        Ok(Self {
            index,
            name: sdat.sequence_name(index).map(str::to_owned),
            player: info.player,
            priority: info.player_priority,
            stream: sseq.stream.into(),
            start: 0,
            volume: info.volume,
            instruments: kit.instruments,
            waves: kit.waves,
        })
    }

    /// Loads every effect of sequence archive `archive` of `sdat` (`None` for an empty slot),
    /// decoding the waves of each bank once.
    pub fn load_effects(sdat: &Sdat<'_>, archive: usize) -> Result<Vec<Option<Self>>> {
        let (_, ssar) = sdat.sequence_archive(archive)?;
        let stream: Arc<[u8]> = ssar.stream.into();
        let mut kits: HashMap<u16, Kit> = HashMap::new();
        let mut effects = Vec::with_capacity(ssar.entries.len());
        for (index, entry) in ssar.entries.iter().enumerate() {
            let Some(offset) = entry.offset else {
                effects.push(None);
                continue;
            };
            if offset as usize >= stream.len() {
                return Err(Error::Malformed("SSAR entry starts past its stream"));
            }
            if !kits.contains_key(&entry.bank) {
                kits.insert(entry.bank, kit(sdat, entry.bank)?);
            }
            let kit = &kits[&entry.bank];
            effects.push(Some(Self {
                index,
                name: None,
                player: entry.player,
                priority: entry.player_priority,
                stream: Arc::clone(&stream),
                start: offset as usize,
                volume: entry.volume,
                instruments: Arc::clone(&kit.instruments),
                waves: Arc::clone(&kit.waves),
            }));
        }
        Ok(effects)
    }
}

#[cfg(test)]
impl Song {
    /// A sequence with no instrument, to test what does not depend on sound.
    pub(crate) fn synthetic(player: u8, priority: u8, stream: &[u8]) -> Self {
        Self {
            index: 0,
            name: None,
            player,
            priority,
            stream: stream.into(),
            start: 0,
            volume: 127,
            instruments: Arc::from(Vec::new()),
            waves: Arc::new(HashMap::new()),
        }
    }
}
