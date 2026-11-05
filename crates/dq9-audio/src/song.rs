//! A sequence ready to play: its command stream, its bank's instruments and the decoded
//! waves they use.

use std::collections::HashMap;
use std::sync::Arc;

use dq9_formats::sdat::{Instrument, NO_WAVE_ARCHIVE, Samples, Sdat};
use nds_rom::{Error, Result};

/// A loaded sequence. Immutable and shared: any number of [`crate::Player`]s can play it.
#[derive(Debug)]
pub struct Song {
    /// Index in the archive's sequence list.
    pub index: usize,
    /// Name in the archive (`BG_005`).
    pub name: Option<String>,
    pub(crate) stream: Vec<u8>,
    /// The sequence's own volume (INFO record), 0 to 127.
    pub(crate) volume: u8,
    pub(crate) instruments: Vec<Instrument>,
    /// Decoded waves by (wave-archive slot of the bank, wave index).
    pub(crate) waves: HashMap<(u16, u16), Arc<Samples>>,
}

impl Song {
    /// Bytes of the command stream.
    pub fn stream_len(&self) -> usize {
        self.stream.len()
    }

    /// Loads sequence `index` of `sdat` with its bank and the waves its instruments use.
    pub fn load(sdat: &Sdat<'_>, index: usize) -> Result<Self> {
        let (info, sseq) = sdat.sequence(index)?;
        let bank_info = *sdat.bank_info(usize::from(info.bank))?;
        let bank = sdat.bank(usize::from(info.bank))?;
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
        Ok(Self {
            index,
            name: sdat.sequence_name(index).map(str::to_owned),
            stream: sseq.stream.to_vec(),
            volume: info.volume,
            instruments: bank.instruments,
            waves,
        })
    }
}
