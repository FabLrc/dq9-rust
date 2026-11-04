//! Invariants on a real DS save. Set `DQ9_SAVE` to a `.sav` file (no save is shipped with
//! the repository); the tests are skipped otherwise, like the real-ROM ones.

use dq9_formats::save::{self, Save};

fn real_save() -> Option<Vec<u8>> {
    let path = std::env::var_os("DQ9_SAVE")?;
    Some(std::fs::read(path).expect("DQ9_SAVE is readable"))
}

#[test]
fn real_save_checksums_recompute_and_rewrite_is_identical() {
    let Some(bytes) = real_save() else {
        eprintln!("DQ9_SAVE not set, skipping");
        return;
    };
    let mut save = Save::parse(&bytes).unwrap();
    for i in 0..save::SLOT_COUNT {
        let slot = save.slot(i).unwrap();
        assert!(slot.checksums_valid(), "slot {i}: {:?}", slot.checksums());
    }
    // Sealing recomputes the stored values: a valid save must not change.
    for i in 0..save::SLOT_COUNT {
        save.slot_mut(i).unwrap().seal();
    }
    assert_eq!(save.as_bytes(), &bytes[..]);
}

#[test]
fn real_save_fields_are_coherent() {
    let Some(bytes) = real_save() else {
        eprintln!("DQ9_SAVE not set, skipping");
        return;
    };
    let save = Save::parse(&bytes).unwrap();
    let slot = save
        .slot(save.current_slot().expect("a committed slot"))
        .unwrap();
    assert!(slot.exists());
    assert!(slot.party_count() >= 1 && slot.party_count() <= 4);
    let used = usize::from(slot.standby_count()) + usize::from(slot.party_count());
    assert!(used <= save::CHARACTER_COUNT);
    for i in 0..used {
        let c = slot.character(i).unwrap();
        let v = usize::from(c.vocation());
        assert!(v < save::VOCATION_COUNT);
        let level = c.level(v).unwrap();
        assert!((1..=99).contains(&level), "character {i} level {level}");
        assert!(c.name_raw().iter().any(|&b| b != 0));
    }
    let (_, minutes, seconds) = slot.play_time();
    assert!(minutes < 60 && seconds < 60);
}
