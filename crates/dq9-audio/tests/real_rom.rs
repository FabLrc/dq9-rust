//! Every music sequence of the real `bgm.sdat` plays; skipped without the ROM.

use std::sync::Arc;

use dq9_audio::{Player, Song};
use dq9_formats::sdat::Sdat;
use nds_rom::Rom;

#[test]
fn every_sequence_renders_finite_audio() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let sdat = Sdat::parse(rom.file_by_path("data/sound/bgm.sdat").unwrap()).unwrap();
    let mut played = 0;
    for index in 0..sdat.sequence_count() {
        if sdat.sequence_info(index).is_none() {
            continue;
        }
        let song = Arc::new(Song::load(&sdat, index).unwrap());
        let mut player = Player::new(song, 32_728);
        let mut out = vec![0.0f32; 2 * 32_728];
        let mut loud = false;
        for _ in 0..10 {
            player.render(&mut out);
            assert!(
                out.iter().all(|s| s.is_finite() && s.abs() < 1.0),
                "sequence {index}"
            );
            loud |= out.iter().any(|s| s.abs() > 0.01);
        }
        assert!(loud, "sequence {index} is silent for its first 10 s");
        played += 1;
    }
    assert!(played >= 60);
}

/// Every effect of the field's base archive (`se_norm.sdat`, archive 100) makes a sound and
/// ends by itself within a few seconds; the first filled entry of the others loads and plays.
#[test]
fn field_effects_sound_and_end() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let sdat = Sdat::parse(rom.file_by_path("data/sound/se_norm.sdat").unwrap()).unwrap();
    let effects = Song::load_effects(&sdat, 100).unwrap();
    let mut played = 0;
    for (index, song) in effects.into_iter().enumerate() {
        let Some(song) = song else { continue };
        let mut player = Player::new(Arc::new(song), 32_728);
        let mut out = vec![0.0f32; 2 * 32_728];
        let mut loud = false;
        let mut seconds = 0;
        while !player.finished() && seconds < 20 {
            player.render(&mut out);
            assert!(
                out.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
                "effect {index}"
            );
            loud |= out.iter().any(|s| s.abs() > 0.001);
            seconds += 1;
        }
        assert!(loud, "effect {index} is silent");
        played += 1;
    }
    let (_, ssar) = sdat.sequence_archive(100).unwrap();
    assert_eq!(
        played,
        ssar.entries.iter().filter(|e| e.offset.is_some()).count()
    );
    assert!(played >= 50);
}
