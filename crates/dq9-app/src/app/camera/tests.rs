//! The field camera (real-ROM regressions).

use crate::app::App;
use crate::app::test_support::map_options;
use dq9_core::field::{Event, Leader, MapChange};
use dq9_formats::Table;
use dq9_formats::map::ZoneInfo;
use dq9_formats::maplist::{self, MapList};
use glam::Vec3;
use nds_rom::Rom;
use winit::keyboard::KeyCode;

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

/// What `ov_017::0219e384` gives the camera of map `id`: `(+0xc byte, home yaw)` from the map
/// list, read here without the code under test.
fn record(rom: &[u8], id: u16) -> (u8, u16) {
    let rom = Rom::parse(rom).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let byte_c = list.records.iter().find(|r| r.id == id).unwrap().byte_c;
    let mut zone = ZoneInfo::default();
    zone.load_map_list(&table, id).unwrap();
    (byte_c, zone.world_angle)
}

/// The framing of the four `0x020a27dc`.. entries as raw words: `(dy, distance, half fov)`.
fn framing_words(byte_c: u8) -> (f32, f32, f32) {
    match (byte_c >> 5) & 3 {
        0 => (31129.0, 57344.0, 15.0),
        1 => (24576.0, 45056.0, 15.0),
        2 => (35635.0, 58982.0, 25.0),
        _ => (50790.0, 58982.0, 25.0),
    }
}

/// The map changes the real exits of `map` make: the leader walks into each `0x72` warp (one
/// step inside the volume) and each doorway (towards it, until its approach counter fires).
/// Each change comes with the app that issued it, the camera turned away from anything a
/// load could leave by chance.
fn crossings(rom: &[u8], map: &str) -> Vec<(App, MapChange)> {
    let exits = App::new_replay(map_options(map, (1, 2, 1, true)), Some(rom.to_vec()))
        .field
        .as_ref()
        .unwrap()
        .exits();
    let mut out = Vec::new();
    for exit in exits {
        let mut app = App::new_replay(map_options(map, (1, 2, 1, true)), Some(rom.to_vec()));
        let leader = Leader {
            position: exit.position,
            yaw: 0.0,
            input: exit.heading,
        };
        let mut events = Vec::new();
        for _ in 0..8 {
            events = app.field.as_mut().unwrap().step(&leader, &mut app.state);
            if !events.is_empty() {
                break;
            }
        }
        let Some(change) = events.iter().find_map(|e| match e {
            Event::MapChange(change) => Some(*change),
            _ => None,
        }) else {
            continue;
        };
        // A hero who has turned the camera by hand and stands in a high orbit: none of it
        // may survive the map change.
        app.view.camera.yaw = 2.0;
        app.view.camera.pitch = 0.1;
        app.view.camera.distance = 30.0;
        assert!(app.apply_events(events), "{map}: the map changes");
        out.push((app, change));
    }
    out
}

/// A map change gives the camera the destination's yaw, elevation, distance and field of view
/// (`ov_017::0219e384`), whatever the camera was and wherever the hero comes out: warps and
/// doorways of four maps, ending in at least three different destinations and both common
/// framings.
#[test]
fn a_map_change_sets_the_destinations_camera() {
    let Some(rom) = testrom::load() else { return };
    let mut destinations = std::collections::BTreeSet::new();
    let mut framings = std::collections::BTreeSet::new();
    for map in ["C01", "X01M05", "D01M02", "1101"] {
        for (app, change) in crossings(rom, map) {
            let id = app.current.as_ref().map(|(entry, _)| entry.id).unwrap();
            assert_eq!(id, change.destination, "{map}: the destination loads");
            let (byte_c, world) = record(rom, id);
            let (dy, distance, half_fov) = framing_words(byte_c);
            let camera = app.view.camera;
            let home = (-(world as i16 as f32) / 4096.0).rem_euclid(std::f32::consts::TAU);
            let what = format!("{map} -> {id} (+0xc {byte_c:02x}, angle {world}, {change:?})");
            assert!(
                (camera.yaw - home).abs() < 1e-4,
                "yaw: {what}: {}",
                camera.yaw
            );
            assert!(
                (camera.pitch - (dy / distance).asin()).abs() < 1e-4,
                "pitch: {what}: {}",
                camera.pitch
            );
            assert!(
                (camera.distance - distance / 4096.0).abs() < 1e-3,
                "distance: {what}: {}",
                camera.distance
            );
            assert!(
                (camera.fovy - 2.0 * half_fov.to_radians()).abs() < 1e-4,
                "fov: {what}: {}",
                camera.fovy
            );
            assert!((app.view.camera_home - home).abs() < 1e-4, "home: {what}");
            // The map follows the hero too.
            let hero = app.hero.as_ref().unwrap().position();
            assert!(
                (camera.target - (hero + Vec3::Y * 0.75)).length() < 1e-3,
                "{what}"
            );
            destinations.insert(id);
            framings.insert((byte_c >> 5) & 3);
        }
    }
    assert!(destinations.len() >= 10, "destinations: {destinations:?}");
    assert!(framings.len() >= 2, "framings: {framings:?}");
    // The Angel Falls exterior is the one map whose home yaw is not 0.
    assert!(
        destinations.contains(&1100),
        "the exterior is reached: {destinations:?}"
    );
}

/// `M01`'s home yaw is `Zone3D+0x40` = 1228 (0.3 rad): the map is entered at that yaw, the
/// shoulder buttons turn from it and both together bring the camera back to it, not to 0.
#[test]
fn the_exterior_camera_returns_to_its_own_home_yaw() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_replay(map_options("M01", (1, 2, 1, true)), Some(rom.to_vec()));
    let home = (-1228.0f32 / 4096.0).rem_euclid(std::f32::consts::TAU);
    assert!(
        (app.view.camera.yaw - home).abs() < 1e-4,
        "{}",
        app.view.camera.yaw
    );
    // `L` for half a second, then `L` + `R`.
    app.controls.held.insert(KeyCode::KeyQ);
    for _ in 0..30 {
        app.turn_camera();
    }
    assert!(
        (app.view.camera.yaw - home).abs() > 0.2,
        "L turned the camera"
    );
    app.controls.held.insert(KeyCode::KeyE);
    for _ in 0..240 {
        app.turn_camera();
    }
    app.controls.held.clear();
    for _ in 0..240 {
        app.turn_camera();
    }
    assert!(
        (app.view.camera.yaw - home).abs() < 0.01,
        "back at {home}, not {}",
        app.view.camera.yaw
    );
}

/// A reload of the same map (the time of day changing) is not a map change: the camera the
/// player turned stays as it is, and so does the yaw the buttons return to.
#[test]
fn a_same_map_reload_keeps_the_camera() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_replay(map_options("C01", (1, 2, 1, true)), Some(rom.to_vec()));
    app.view.camera.yaw = 1.0;
    app.view.camera.distance = 20.0;
    let home = app.view.camera_home;
    assert!(app.load("C01", false, None, None));
    assert_eq!(app.view.camera.yaw, 1.0);
    assert_eq!(app.view.camera.distance, 20.0);
    assert_eq!(app.view.camera_home, home);
}
