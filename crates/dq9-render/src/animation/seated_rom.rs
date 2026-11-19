//! A seated NPC of the ROM against the camera (`0x0203f54c`, `docs/app/rendering.md` § NPCs).
//! Skipped without a ROM.
//!
//! The sleeper of Stornway Castle's cell (`C01M15`, NPC 41, `n019a_s.spr`, placement angle
//! 3850, flag `0x800`) is drawn tilted back and rolled, never as a plain billboard and never
//! leaning towards the eye.

use std::f32::consts::TAU;

use dq9_formats::Table;
use dq9_formats::cast::Story;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList};
use glam::Vec3;
use nds_rom::Rom;

use super::AnimatedScene;
use crate::Camera;

fn cell(rom: &Rom) -> AnimatedScene {
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let record = list.records.iter().find(|r| r.code == b"C01M15").unwrap();
    let ctx = Context {
        time_of_day: 2,
        grotto_environ: 1,
    };
    let map = Map::assemble(record, &table, &list, |p| map::rom_file(rom, p), &ctx).unwrap();
    let story = Story {
        chapter: 1,
        step: 1,
        substep: 1,
        daytime: true,
        ..Story::default()
    };
    AnimatedScene::build(&map, record.id, record.code, &story, |p| {
        map::rom_file(rom, p)
    })
    .unwrap()
}

/// The top edge of the quad, the bottom edge, and the unit normal, from the model's four
/// vertices (top-left, top-right, bottom-right, bottom-left).
fn quad(scene: &AnimatedScene, model: usize) -> (Vec3, Vec3, Vec3) {
    let v = &scene.scene.models[model].vertices;
    let at = |i: usize| Vec3::from_array(v[i].position);
    let normal = (at(1) - at(0)).cross(at(3) - at(0)).normalize();
    ((at(0) + at(1)) * 0.5, (at(2) + at(3)) * 0.5, normal)
}

#[test]
fn a_sleeper_lies_back_from_every_camera_and_is_flat_to_the_eye_from_the_side() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut scene = cell(&rom);
    let sprite = scene
        .sprites
        .iter()
        .position(|s| s.seated)
        .expect("the cell has a seated sprite");
    let (model, angle) = (scene.sprites[sprite].model, scene.sprites[sprite].angle);
    assert_eq!(angle, 3850, "the placement's angle");
    // The camera angle the sprite code reads is `-yaw`: yaw `-angle` sees the NPC head on
    // (θ = 0, the most tilt), yaw `π/2 - angle` side on (θ = π/2, none).
    let facing = -(angle as f32) / 4096.0;
    for pitch in [0.3, 0.55] {
        for k in 0..16 {
            let camera = Camera {
                yaw: facing + k as f32 * TAU / 16.0,
                pitch,
                ..Camera::default()
            };
            scene.face_camera(&camera).unwrap();
            let (top, bottom, normal) = quad(&scene, model);
            let eye = -camera.forward();
            let lean = (top - bottom).normalize().dot(camera.forward());
            assert!(
                lean >= -1e-4,
                "yaw step {k} pitch {pitch}: the top leans to the eye ({lean})"
            );
            if k == 0 {
                // Head on: lies back by half a radian, the normal tilts up by as much.
                assert!(lean > 0.45, "head on, the quad lies back ({lean})");
                assert!(
                    (normal.dot(eye).abs() - 0.5f32.cos()).abs() < 2e-2,
                    "the normal is half a radian off the eye ({})",
                    normal.dot(eye)
                );
            }
            if k == 4 || k == 12 {
                // Side on: cos θ = 0, no tilt, no roll: a plain billboard.
                assert!(lean.abs() < 1e-3, "side on, upright ({lean})");
                assert!(
                    (normal.dot(eye).abs() - 1.0).abs() < 1e-3,
                    "side on, the quad faces the eye"
                );
            }
        }
    }
}
