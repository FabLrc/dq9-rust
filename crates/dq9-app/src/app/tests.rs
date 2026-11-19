//! The step loop: the field cadence (real-ROM regression).

use crate::app::App;
use crate::app::test_support::map_options;
use dq9_core::Hero;
use glam::Vec3;

/// The original updates the field on one VBlank out of two: this walks a flat, straight
/// segment of `X01M05` from the capture's start pose and asserts the exact raw angle and
/// fx position at every VBlank. Expected values are the verified samples of
/// `build/v01-20261107/review/cadence-30hz-field-review.md` §7 (original
/// `original-prep.jsonl` and `Hero::step(ticks = 2)` model): even steps hold the previous
/// pose, odd steps turn by `0x324 × 2` and accelerate/move by two ticks.
#[test]
fn field_cadence_holds_the_pose_and_matches_the_original_every_vblank() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(map_options("X01M05", (1, 1, 1, true)), Some(rom.to_vec()));
    assert_eq!(app.tick, 0, "a fresh app starts on phase 0");
    assert!(
        !app.events.is_active() && app.events.pending_start().is_none(),
        "the flat segment must not be interrupted by an event"
    );
    let y = app
        .collision
        .ground_height(1.756, -3.041, 3.0)
        .expect("ground under the capture's start pose");
    // App yaw π is raw angle 0, facing +z at speed 0: the original's prepared state.
    app.hero = Some(Hero::at(Vec3::new(1.756, y, -3.041), std::f32::consts::PI));
    let hero = app.hero.expect("the map placed a hero");
    assert_eq!(hero.angle(), 0);
    assert_eq!(hero.speed(), 0);
    assert_eq!(hero.position_fx(), [7192, (y * 4096.0) as i32, -12455]);

    // `Hero::at` truncates -3.041 × 4096; the original's exact fx is not observable.
    const BASE_Z: i32 = -12455;
    const BASE_X: i32 = 7192;
    const ANGLES: [i32; 10] = [0, 1608, 3216, 4824, 6432, 8040, 9648, 11256, 12864, 12868];
    const Z_OFFSETS: [i32; 10] = [0, 0, -160, -454, -846, -1338, -1928, -2616, -3402, -4188];

    let mut held = app.hero.expect("hero").position_fx();
    let mut held_angle = 0;
    for step in 1..=19u64 {
        app.walk_step(Vec3::NEG_Z);
        assert_eq!(app.tick, step);
        let hero = app.hero.expect("the hero stays placed");
        let position = hero.position_fx();
        if step % 2 == 0 {
            // The intermediate VBlank holds the pose byte for byte; the camera still
            // follows it (per VBlank, not per game update).
            assert_eq!(hero.angle(), held_angle, "angle held on step {step}");
            assert_eq!(position, held, "position held on step {step}");
            assert_eq!(
                app.view.camera.target.z,
                hero.position().z,
                "camera follows the held pose on step {step}"
            );
            continue;
        }
        let sample = (step as usize - 1) / 2;
        assert_eq!(hero.angle(), ANGLES[sample], "raw angle on step {step}");
        assert!(
            (position[2] - (BASE_Z + Z_OFFSETS[sample])).abs() <= 2,
            "z fx on step {step}: {} (expected {} ± 2)",
            position[2],
            BASE_Z + Z_OFFSETS[sample]
        );
        assert!(
            (position[0] - BASE_X).abs() <= 1,
            "x fx on step {step}: {} (expected {BASE_X} ± 1)",
            position[0]
        );
        held = position;
        held_angle = hero.angle();
    }
    assert_eq!(held_angle, 12868, "the turn clamps on the target");
    assert!(!app.events.is_active());
}
