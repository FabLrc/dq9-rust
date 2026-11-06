//! SDAT invariants on the real archives (`docs/re/sdat.md`); skipped without the ROM.

use dq9_formats::Table;
use dq9_formats::bgm::{self, Overrides};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::sdat::{Instrument, NoteKind, Sdat, WaveFormat};
use nds_rom::Rom;

/// Every sequence of `bgm.sdat` that has a file resolves its bank and wave archives, and every
/// PCM note of the bank names a wave that exists and decodes.
#[test]
fn bgm_sequences_resolve_and_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let sdat = Sdat::parse(rom.file_by_path("data/sound/bgm.sdat").unwrap()).unwrap();
    let mut sequences = 0;
    let mut waves = 0;
    for index in 0..sdat.sequence_count() {
        let Some(info) = sdat.sequence_info(index) else {
            continue;
        };
        let (_, sseq) = sdat.sequence(index).unwrap();
        assert!(!sseq.stream.is_empty(), "sequence {index}");
        let bank = sdat.bank(usize::from(info.bank)).unwrap();
        let bank_info = sdat.bank_info(usize::from(info.bank)).unwrap();
        let archives: Vec<_> = bank_info
            .wave_archives
            .iter()
            .map(|&a| {
                (a != dq9_formats::sdat::NO_WAVE_ARCHIVE)
                    .then(|| sdat.wave_archive(a.into()).unwrap())
            })
            .collect();
        for instrument in &bank.instruments {
            let notes: Vec<_> = match instrument {
                Instrument::Empty => vec![],
                Instrument::Single(n) => vec![*n],
                Instrument::Drums { notes, .. } | Instrument::Split { notes, .. } => notes.clone(),
            };
            for note in notes.iter().filter(|n| n.kind == NoteKind::Pcm) {
                let swar = archives[usize::from(note.archive)]
                    .as_ref()
                    .unwrap_or_else(|| panic!("sequence {index}: archive slot {}", note.archive));
                let wave = swar
                    .waves
                    .get(usize::from(note.wave))
                    .unwrap_or_else(|| panic!("sequence {index}: wave {}", note.wave));
                assert_eq!(wave.format, WaveFormat::ImaAdpcm);
                wave.decode().unwrap();
                waves += 1;
            }
        }
        sequences += 1;
    }
    eprintln!("{sequences} sequences, {waves} PCM notes");
    assert!(sequences >= 60);
    assert!(sdat.find_sequence("BG_005").is_some());
}

/// The overrides parse, and every map of the list names a sequence that has a file (or none).
#[test]
fn every_map_music_has_a_sequence() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let sdat = Sdat::parse(rom.file_by_path("data/sound/bgm.sdat").unwrap()).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let over = Table::parse(rom.file_by_path(bgm::PATH).unwrap()).unwrap();
    let over = Overrides::parse(&over).unwrap();
    assert_eq!(over.entries.len(), 67);
    for record in &list.records {
        for time in 0..4 {
            for flags in [false, true] {
                if let Some(id) = bgm::sequence_for_map(&list, &over, record.id, time, |_| flags) {
                    assert!(
                        sdat.sequence_info(usize::from(id)).is_some(),
                        "map {} time {time}: sequence {id}",
                        record.id
                    );
                }
            }
        }
    }
    // Angel Falls (1100): sequence 5 by day, 6 at night.
    assert_eq!(
        bgm::sequence_for_map(&list, &over, 1100, 2, |_| false),
        Some(5)
    );
    assert_eq!(
        bgm::sequence_for_map(&list, &over, 1100, 0, |_| false),
        Some(6)
    );
}

/// The effect archives: every sequence archive parses, its entries start inside its stream and
/// name a bank that exists and a player of the archive, and the group of an archive loads that
/// archive.
#[test]
fn effect_archives_are_consistent() {
    use dq9_formats::sdat::{KIND_PLAYER, KIND_SEQUENCE_ARCHIVE};
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    for (path, expected) in [
        ("data/sound/se_norm.sdat", 279),
        ("data/sound/se_btl.sdat", 481),
    ] {
        let sdat = Sdat::parse(rom.file_by_path(path).unwrap()).unwrap();
        let mut archives = 0;
        for index in 0..sdat.sequence_archive_count() {
            let Ok((_, ssar)) = sdat.sequence_archive(index) else {
                assert!(
                    sdat.record(KIND_SEQUENCE_ARCHIVE, index).is_none(),
                    "{path} {index}"
                );
                continue;
            };
            archives += 1;
            for entry in &ssar.entries {
                let Some(offset) = entry.offset else { continue };
                assert!((offset as usize) < ssar.stream.len(), "{path} {index}");
                assert!(
                    sdat.bank_info(usize::from(entry.bank)).is_ok(),
                    "{path} {index}"
                );
                assert!(
                    sdat.player(usize::from(entry.player)).is_some(),
                    "{path} {index}"
                );
            }
            let group = sdat.group(index).expect("a group per archive");
            assert!(
                group
                    .iter()
                    .any(|e| e.kind == 3 && e.index as usize == index),
                "{path}: group {index} does not load its archive"
            );
        }
        assert_eq!(archives, expected, "{path}");
        assert!((0..sdat.list_len(KIND_PLAYER)).any(|i| sdat.player(i).is_some()));
    }
}
