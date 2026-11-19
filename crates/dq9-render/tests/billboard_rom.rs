//! Scripted effect models turn to the camera like a map's billboards (`RenderCommand_7`,
//! `docs/re/g3d.md` § Billboards). Skipped without a ROM.
//!
//! The observable the test asserts is the one the original has: a model drawn under a
//! billboard command is **screen-aligned** around its pivot, so its shape seen from the camera
//! does not depend on where the camera stands, and a quad of it faces the eye.

use std::f32::consts::TAU;

use dq9_formats::Table;
use dq9_formats::chr::Chr;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList};
use dq9_render::{AnimatedScene, Camera};
use glam::{Mat3, Mat4, Vec3};
use nds_rom::Rom;
use nds_rom::g3d::G3dFile;

/// Effect archives a new game's story plays between the observatory and Angel Falls
/// (`docs/re/characters.md` § Open: effect models and billboards).
const EFFECTS: [&str; 8] = [
    "ev010000010",
    "ev010000300",
    "ev010000310",
    "ev010100400",
    "ev010100500",
    "ev014200000",
    "ev180003410",
    "ev999990100",
];

/// Where the effect stands in the test: a position, a yaw and a scale of its own, so that the
/// inverse of the instance's world is not the identity.
fn world() -> Mat4 {
    Mat4::from_translation(Vec3::new(12.0, 3.0, -7.0))
        * Mat4::from_rotation_y(0.7)
        * Mat4::from_scale(Vec3::splat(1.5))
}

fn scene(rom: &Rom) -> AnimatedScene {
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let record = list.records.iter().find(|r| r.code == b"M01M12").unwrap();
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    let map = Map::assemble(record, &table, &list, |p| map::rom_file(rom, p), &ctx).unwrap();
    let story = dq9_formats::cast::Story::default();
    AnimatedScene::build(&map, record.id, record.code, &story, |p| {
        map::rom_file(rom, p)
    })
    .unwrap()
}

/// Eight yaws at two pitches.
fn cameras(target: Vec3) -> Vec<Camera> {
    let mut out = Vec::new();
    for pitch in [0.0, 0.5] {
        for i in 0..8 {
            out.push(Camera {
                yaw: i as f32 * TAU / 8.0 + 0.1,
                pitch,
                distance: 40.0,
                target,
                ..Camera::default()
            });
        }
    }
    out
}

/// What one camera sees of an actor: for each billboard command, its vertices in the camera's
/// axes relative to the first of them, and the largest triangle's normal against the eye.
struct Seen {
    shapes: Vec<Vec<Vec3>>,
    /// `|normal · eye|` of the largest triangle drawn under a billboard command.
    facing: f32,
}

fn see(scene: &mut AnimatedScene, actor: usize, camera: &Camera) -> Seen {
    scene.face_camera(camera).unwrap();
    let instance = scene.actor_instance(actor).unwrap();
    let world = scene.scene.instances[instance].world;
    let model = scene.scene.instances[instance].model;
    let view = Mat3::from_mat4(camera.view());
    let eye = view.transpose() * Vec3::Z;
    let vertices = &scene.scene.models[model].vertices;
    let at = |i: u32| world.transform_point3(Vec3::from_array(vertices[i as usize].position));
    let records = &scene.scene.billboards[&model];
    let mut shapes = Vec::new();
    let mut under = std::collections::HashSet::new();
    for record in records {
        let first = at(record.vertices[0].0);
        shapes.push(
            record
                .vertices
                .iter()
                .map(|&(i, _)| view * (at(i) - first))
                .collect(),
        );
        under.extend(record.vertices.iter().map(|&(i, _)| i));
    }
    let mut best = (0.0f32, 0.0f32);
    for tri in scene.scene.models[model].indices.as_chunks::<3>().0 {
        if !tri.iter().all(|i| under.contains(i)) {
            continue;
        }
        let n = (at(tri[1]) - at(tri[0])).cross(at(tri[2]) - at(tri[0]));
        if n.length() > best.0 {
            best = (n.length(), n.normalize().dot(eye).abs());
        }
    }
    Seen {
        shapes,
        facing: best.1,
    }
}

/// Every camera sees the same shape, and the largest quad faces the eye within 3°.
fn assert_turns_to_the_camera(scene: &mut AnimatedScene, actor: usize, label: &str, facing: bool) {
    let instance = scene.actor_instance(actor).unwrap();
    scene.scene.instances[instance].world = world();
    let target = world().transform_point3(Vec3::ZERO);
    let cameras = cameras(target);
    let model = scene.scene.instances[instance].model;
    let upright: Vec<bool> = scene.scene.billboards[&model]
        .iter()
        .map(|b| b.y_only)
        .collect();
    // A flat billboard (`0x07`) looks the same from every camera. One about the object's up
    // axis (`0x08`) does not: the image of that axis in the camera changes with the camera.
    let reference = see(scene, actor, &cameras[0]);
    assert!(
        !reference.shapes.is_empty(),
        "{label}: no billboard command kept"
    );
    for (n, camera) in cameras.iter().enumerate() {
        let seen = see(scene, actor, camera);
        for (record, (a, b)) in reference.shapes.iter().zip(&seen.shapes).enumerate() {
            if upright[record] {
                assert!(b.iter().all(|v| v.is_finite()), "{label}: camera {n}");
                continue;
            }
            let extent = a.iter().map(|v| v.length()).fold(1.0, f32::max);
            for (v, (a, b)) in a.iter().zip(b).enumerate() {
                assert!(
                    (*a - *b).length() <= 2e-3 * extent,
                    "{label}: camera {n}, command {record}, vertex {v}: {a:?} against {b:?}"
                );
            }
        }
        if facing {
            assert!(
                seen.facing >= (3.0f32).to_radians().cos(),
                "{label}: camera {n}: the largest quad faces the eye by {:.3}",
                seen.facing
            );
        }
    }
}

/// Each effect model of the new game's first scenes, at rest and after its first motion has
/// played for a while.
#[test]
fn effect_models_face_the_camera() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut scene = scene(&rom);
    let camera = Camera::default();
    for name in EFFECTS {
        let chr = map::rom_file(&rom, &format!("data/effect/{name}.chr")).unwrap();
        let actor = scene.add_actor(chr, &[]).unwrap();
        assert!(scene.actor_instance(actor).is_some(), "{name}");
        assert_turns_to_the_camera(&mut scene, actor, &format!("{name} at rest"), true);
        assert!(scene.play_default_motion(actor), "{name}");
        for tick in [1, 40, 250] {
            for _ in 0..(if tick == 1 { 1 } else { tick - 1 }) {
                scene.step(&camera).unwrap();
            }
            assert_turns_to_the_camera(&mut scene, actor, &format!("{name} tick {tick}"), true);
        }
    }
}

/// Every character or effect archive the scripts can load (`data/effect`, `data/event_lv5`,
/// `data/chara_sub`) whose model carries a billboard command: whatever the model, the camera
/// sees the same shape from every side once it is added as an actor.
#[test]
fn every_scripted_model_with_a_billboard_turns_to_the_camera() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut scene = scene(&rom);
    let mut models = 0;
    for entry in &rom.files.files {
        let path = entry.path.to_ascii_lowercase();
        let scripted = ["data/effect/", "data/event_lv5/", "data/chara_sub/"]
            .iter()
            .any(|dir| path.starts_with(dir));
        if !scripted || !path.ends_with(".chr") {
            continue;
        }
        let data = rom.file(entry.id).unwrap();
        let Some(model) = Chr::parse(data).unwrap().model_data().unwrap() else {
            continue;
        };
        let file = G3dFile::parse(model).unwrap();
        if file.models.first().is_none_or(|m| m.billboards.is_empty()) {
            continue;
        }
        let actor = scene.add_actor(data, &[]).unwrap();
        if scene.actor_instance(actor).is_none() {
            continue;
        }
        assert_turns_to_the_camera(&mut scene, actor, &entry.path, false);
        models += 1;
    }
    println!("{models} scripted models with a billboard command turn to the camera");
    assert!(models > 100, "{models}");
}

/// The 21510 sparkle (`ev010100400`): the billboard command sits before the four quads of the
/// star, which hang from the matrix it rewrote through `MTX_STORE`, so every vertex of the model
/// is drawn under a command (before the fix only the 3 vertices of a dummy mesh were).
#[test]
fn the_sparkle_of_21510_is_billboarded_whole() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut scene = scene(&rom);
    let chr = map::rom_file(&rom, "data/effect/ev010100400.chr").unwrap();
    let model = Chr::parse(chr).unwrap().model_data().unwrap().unwrap();
    let file = G3dFile::parse(model).unwrap();
    assert_eq!(file.models[0].billboards.len(), 1);
    let actor = scene.add_actor(chr, &[]).unwrap();
    let model = scene
        .actor_instance(actor)
        .map(|i| scene.scene.instances[i].model);
    let model = model.unwrap();
    let under: usize = scene.scene.billboards[&model]
        .iter()
        .map(|b| b.vertices.len())
        .sum();
    assert_eq!(under, scene.scene.models[model].vertices.len());
}

/// An animation moves the nodes a billboard command is under: the command's pivot and axes
/// are those of the pose it runs in, not of the rest pose.
#[test]
fn a_billboard_pivot_follows_the_animation() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut scene = scene(&rom);
    let chr = map::rom_file(&rom, "data/effect/ev180003410.chr").unwrap();
    let actor = scene.add_actor(chr, &[]).unwrap();
    let model = scene
        .actor_instance(actor)
        .map(|i| scene.scene.instances[i].model);
    let model = model.unwrap();
    let rest: Vec<Vec3> = scene.scene.billboards[&model]
        .iter()
        .map(|b| b.pivot)
        .collect();
    assert!(scene.play_default_motion(actor));
    let camera = Camera::default();
    for _ in 0..60 {
        scene.step(&camera).unwrap();
    }
    let moved = scene.scene.billboards[&model]
        .iter()
        .zip(&rest)
        .filter(|(b, rest)| (b.pivot - **rest).length() > 1e-3)
        .count();
    assert!(moved > 0, "no pivot moved with the animation");
}
