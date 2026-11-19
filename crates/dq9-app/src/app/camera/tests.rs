//! The field camera (real-ROM regression).

use crate::app::App;
use crate::app::test_support::map_options;

/// `ov_017::0219e234`: the map record's kind and flags decide whether the shoulder buttons turn the
/// field camera. F02 (kind 0) keeps it fixed, as the original does in melonDS; Angel Falls'
/// church (`C01M09`) turns it.
#[test]
fn the_field_camera_lock_follows_the_map_record() {
    let Some(rom) = testrom::load() else { return };
    let locked = |map: &str| {
        App::new_replay(map_options(map, (1, 2, 1, true)), Some(rom.to_vec()))
            .view
            .field_camera
            .is_locked()
    };
    assert!(locked("F02"), "F02's camera does not turn");
    assert!(!locked("C01M09"), "the church's camera turns");
}
