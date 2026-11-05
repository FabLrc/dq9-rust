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
