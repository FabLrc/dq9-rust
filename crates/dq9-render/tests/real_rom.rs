//! Scene invariants on the user's ROM (skipped without it).

use std::collections::{BTreeMap, HashSet};

use dq9_formats::Table;
use dq9_formats::map::{self, Context, Map, PartKind};
use dq9_formats::maplist::{self, MapList, MapRecord};
use dq9_render::Scene;
use dq9_render::scene::{PolygonMode, world_matrix};
use glam::{Mat4, Vec3};
use nds_rom::Rom;

const IDENTITY: [[f32; 3]; 2] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

fn assemble(rom: &Rom, table: &Table, list: &MapList, record: &MapRecord) -> Map {
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    Map::assemble(record, table, list, |p| map::rom_file(rom, p), &ctx).unwrap()
}

#[test]
fn sample_maps() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    for code in ["M01", "C01M09", "F01"] {
        let record = list.records.iter().find(|r| r.code == code.as_bytes());
        let m = assemble(&rom, &table, &list, record.unwrap());
        let s = Scene::build(&m, |p| map::rom_file(&rom, p));
        let textured = s
            .models
            .iter()
            .flat_map(|m| &m.materials)
            .all(|m| m.texture.is_some());
        println!(
            "{code}: {} models, {} instances, {} textures, {} triangles",
            s.models.len(),
            s.instances.len(),
            s.textures.len(),
            s.triangle_count(),
        );
        assert!(s.warnings.is_empty(), "{code}: {:?}", s.warnings);
        assert!(!s.instances.is_empty() && textured && s.bounds().is_some());
        // A placement hidden at the chosen time of day is built too, with a null scale and
        // the transform the map gives it; a visible one draws that transform (natives 574,
        // 575 show or move the hidden ones).
        for inst in &s.instances {
            let placement = &m.blocks[inst.block].instances[inst.instance];
            assert_eq!(inst.home, world_matrix(&placement.world));
            assert_eq!(inst.drawn(), !placement.hidden);
            if placement.hidden {
                assert_eq!(inst.world, Mat4::from_scale(Vec3::ZERO));
            } else {
                assert_eq!(inst.world, inst.home);
            }
        }
    }
}

/// `Scene::build` keeps the placements the chosen time of day hides: each has an instance with
/// a null scale and its home transform, listed in `placements` so that natives 574 and 575 can
/// show or move it; only collision parts stay undrawn.
#[test]
fn hidden_placements_are_built_for_scripts() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let record = list.records.iter().find(|r| r.code == b"M01M06").unwrap();
    let m = assemble(&rom, &table, &list, record);
    let s = Scene::build(&m, |p| map::rom_file(&rom, p));
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert_eq!(s.placements.len(), s.instances.len());
    let mut hidden = 0;
    for &(entry, id, instance) in &s.placements {
        let block = m.blocks.iter().find(|b| b.entry == entry).unwrap();
        let placement = block.instances.iter().find(|i| i.id == id).unwrap();
        let inst = &s.instances[instance];
        assert_eq!(inst.home, world_matrix(&placement.world));
        if placement.hidden {
            hidden += 1;
            assert!(!inst.drawn());
            assert_eq!(inst.world, Mat4::from_scale(Vec3::ZERO));
        } else {
            assert_eq!(inst.world, inst.home);
            assert!(inst.drawn());
        }
    }
    assert!(hidden > 0, "M01M06 at time 0 builds no hidden placement");
    for &(entry, id) in &s.undrawn_placements {
        let block = m.blocks.iter().find(|b| b.entry == entry).unwrap();
        let placement = block.instances.iter().find(|i| i.id == id).unwrap();
        let part = placement.part.map(|p| block.parts[p].kind);
        assert_eq!(part, Some(PartKind::Collision));
    }
}

/// Every distinct set of map archives builds without a warning: models parse and run, and
/// every material texture is found and decoded.
#[test]
fn all_maps() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let mut seen = HashSet::new();
    let (mut scenes, mut models, mut instances, mut textures, mut triangles) = (0, 0, 0, 0, 0);
    let (mut materials, mut untextured, mut unbound, mut tex_matrices) = (0, 0, 0, 0);
    let mut modes = BTreeMap::new();
    let mut warnings = Vec::new();
    for record in list.records.iter().filter(|r| r.id != 0) {
        let m = assemble(&rom, &table, &list, record);
        if !seen.insert((m.files.ambl.to_ascii_lowercase(), m.files.amdj.clone())) {
            continue;
        }
        let s = Scene::build(&m, |p| map::rom_file(&rom, p));
        scenes += 1;
        models += s.models.len();
        instances += s.instances.len();
        textures += s.textures.len();
        triangles += s.triangle_count();
        for mat in s.models.iter().flat_map(|m| &m.materials) {
            materials += 1;
            untextured += usize::from(mat.texture_name.is_none());
            tex_matrices += usize::from(mat.uv_transform != IDENTITY);
            *modes.entry(format!("{:?}", mat.mode)).or_insert(0) += 1;
            assert!(mat.uv_transform.iter().flatten().all(|v| v.is_finite()));
            if mat.mode == PolygonMode::Shadow {
                assert!(!mat.visible());
            }
        }
        unbound += s.unbound_textures.len();
        warnings.extend(s.warnings.iter().map(|w| format!("{}: {w}", m.files.ambl)));
    }
    println!(
        "{scenes} archive sets: {models} models, {instances} instances, {textures} textures, \
         {triangles} triangles; {materials} materials, {untextured} without texture, \
         {unbound} textures unbound, {tex_matrices} with a texture matrix; modes {modes:?}"
    );
    assert!(
        warnings.is_empty(),
        "{} warnings: {warnings:#?}",
        warnings.len()
    );
}

#[test]
fn runtime_cast_and_animation_are_deterministic() {
    use dq9_formats::cast::{self, Appearance, Story};
    use dq9_render::{AnimatedScene, Camera};
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let story = Story {
        chapter: 1,
        step: 1,
        substep: 1,
        daytime: true,
        ..Story::default()
    };
    let camera = Camera::default();
    for code in ["M01", "C01M09", "F01", "M01M08"] {
        let mut story = if code == "M01M08" {
            Story {
                chapter: 3,
                ..story.clone()
            }
        } else {
            story.clone()
        };
        if code == "M01M08" {
            story.quests.insert(152, 3);
        }
        let record = list
            .records
            .iter()
            .find(|r| r.code == code.as_bytes())
            .unwrap();
        let map = assemble(&rom, &table, &list, record);
        let mut a = AnimatedScene::build(&map, record.id, record.code, &story, |p| {
            map::rom_file(&rom, p)
        })
        .unwrap();
        let mut b = AnimatedScene::build(&map, record.id, record.code, &story, |p| {
            map::rom_file(&rom, p)
        })
        .unwrap();
        let cast = cast::load(
            rom.file_by_path(&cast::archive_path(record.code)).unwrap(),
            record.id,
            &story,
        )
        .unwrap();
        let bodies = cast
            .npcs
            .iter()
            .filter(|n| n.appearance(|p| map::rom_file(&rom, p).is_some()) != Appearance::None)
            .count();
        let drawn = a
            .scene
            .instances
            .iter()
            .filter(|i| i.block == usize::MAX)
            .count();
        assert_eq!(drawn, bodies, "{code}");
        if code == "M01M08" {
            assert!(cast.npcs.iter().any(|n| matches!(
                n.appearance(|p| map::rom_file(&rom, p).is_some()),
                Appearance::Monster { .. }
            )));
        }
        let topology: Vec<_> = a.scene.models.iter().map(|m| m.indices.clone()).collect();
        let initial: Vec<_> = a.scene.models.iter().map(|m| m.vertices.clone()).collect();
        for _ in 0..60 {
            a.step(&camera).unwrap();
            b.step(&camera).unwrap();
        }
        a.face_camera(&camera).unwrap();
        b.face_camera(&camera).unwrap();
        assert_eq!(a.frame, 60);
        for (i, (am, bm)) in a.scene.models.iter().zip(&b.scene.models).enumerate() {
            assert_eq!(am.vertices, bm.vertices, "{code}: {}", am.name);
            assert_eq!(am.materials, bm.materials, "{code}: {}", am.name);
            assert_eq!(am.indices, topology[i]);
            assert!(
                am.vertices.iter().all(|v| v
                    .position
                    .iter()
                    .chain(&v.color)
                    .all(|x| x.is_finite()))
            );
        }
        if code == "M01" {
            assert!(
                a.scene
                    .models
                    .iter()
                    .zip(&initial)
                    .any(|(m, v)| m.vertices != *v),
                "map animation must change vertices"
            );
        }
    }
}

/// The hero model is assembled on the rig, hangs its head parts on the head bone and its motion
/// follows the gait; the same inputs give the same vertices.
#[test]
fn hero_model_follows_its_gait() {
    use dq9_formats::cast::Story;
    use dq9_formats::hero::HeroLook;
    use dq9_render::{AnimatedScene, Camera, Gait};
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let record = list.records.iter().find(|r| r.code == b"M01").unwrap();
    let map = assemble(&rom, &table, &list, record);
    let build = || {
        let mut scene =
            AnimatedScene::build(&map, record.id, record.code, &Story::default(), |p| {
                map::rom_file(&rom, p)
            })
            .unwrap();
        let instance = scene
            .add_hero(&HeroLook::new_game(), |p| map::rom_file(&rom, p))
            .unwrap();
        (scene, instance)
    };
    let (mut a, instance) = build();
    let (mut b, _) = build();
    let model = a.scene.instances[instance].model;
    // Body, legs, face, hair and halo in one model.
    assert!(a.scene.models[model].draws.len() >= 5);
    assert_eq!(a.hero_gait(), Some(Gait::Stand));
    let camera = Camera::default();
    let heights = |scene: &AnimatedScene| {
        let v = &scene.scene.models[model].vertices;
        let y = v.iter().map(|v| v.position[1]);
        (
            y.clone().fold(f32::MAX, f32::min),
            y.fold(f32::MIN, f32::max),
        )
    };
    let (low, high) = heights(&a);
    // A character of the rig is about 21 units tall (body 16.6, head 4); the halo tops it.
    assert!(
        low > -1.0 && (18.0..30.0).contains(&(high - low)),
        "{low} {high}"
    );
    let rest = a.scene.models[model].vertices.clone();
    for _ in 0..7 {
        a.step(&camera).unwrap();
        b.step(&camera).unwrap();
    }
    assert_eq!(
        a.scene.models[model].vertices,
        b.scene.models[model].vertices
    );
    let standing = a.scene.models[model].vertices.clone();
    assert_ne!(rest, standing, "stand moves the model");
    a.set_hero_gait(Gait::Run);
    b.set_hero_gait(Gait::Run);
    for _ in 0..7 {
        a.step(&camera).unwrap();
        b.step(&camera).unwrap();
    }
    assert_eq!(
        a.scene.models[model].vertices,
        b.scene.models[model].vertices
    );
    assert_ne!(standing, a.scene.models[model].vertices);
    // The same gait is not restarted by being chosen again.
    let running = a.scene.models[model].vertices.clone();
    a.set_hero_gait(Gait::Run);
    a.step(&camera).unwrap();
    assert_ne!(running, a.scene.models[model].vertices);
    assert_eq!(Gait::from_speed(0.0), Gait::Stand);
    assert_eq!(Gait::from_speed(4.0), Gait::Walk);
    assert_eq!(Gait::from_speed(8.0), Gait::Run);
}

/// A scripted actor (`add_actor`): a character archive's motions play by name, one-shot
/// motions end, and a model without polygons still exposes its bones (the camera rigs of the
/// opening cutscene).
#[test]
fn scripted_actors_play_named_motions_and_expose_bones() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let record = list.records.iter().find(|r| r.code == b"M01M12").unwrap();
    let map = assemble(&rom, &table, &list, record);
    let file = |p: &str| map::rom_file(&rom, p);
    let story = dq9_formats::cast::Story::default();
    let mut scene =
        dq9_render::AnimatedScene::build(&map, record.id, record.code, &story, file).unwrap();
    let camera = dq9_render::Camera::default();

    // A character with a package: `cut01_move` comes from the package, `stand` from the model.
    let chr = file("data/chara_sub/s070.chr").unwrap();
    let package = file("data/event_lv5/ev01000s070.chr").unwrap();
    let before = scene.scene.instances.len();
    let actor = scene.add_actor(chr, &[package]).unwrap();
    assert_eq!(scene.actor_instance(actor), Some(before));
    let motions = scene.actor_motions(actor);
    assert!(motions.contains(&"stand") && motions.contains(&"cut01_move"));
    assert!(!scene.play_actor(actor, "no such motion", false, 1.0));
    assert!(scene.play_actor(actor, "cut01_move", true, 1.0));
    assert_eq!(scene.actor_wraps(actor), 0);
    for _ in 0..600 {
        scene.step(&camera).unwrap();
    }
    // A one-shot motion has ended and holds its last pose.
    assert!(scene.actor_wraps(actor) > 0);
    let held = scene.scene.models[scene.scene.instances[before].model]
        .vertices
        .clone();
    scene.step(&camera).unwrap();
    assert_eq!(
        held,
        scene.scene.models[scene.scene.instances[before].model].vertices
    );
    assert!(scene.actor_bone(actor, "head").unwrap().is_some());
    assert!(scene.actor_bone(actor, "no such bone").unwrap().is_none());

    // The camera rig has no polygon: no instance, but bones that move with its motion.
    let rig = scene
        .add_actor(file("data/event_lv5/ev01000cc000.chr").unwrap(), &[])
        .unwrap();
    assert_eq!(scene.actor_instance(rig), None);
    assert!(scene.play_actor(rig, "cut_00", true, 1.0));
    let start = scene.actor_bone(rig, "eye").unwrap().unwrap();
    for _ in 0..200 {
        scene.step(&camera).unwrap();
    }
    let later = scene.actor_bone(rig, "eye").unwrap().unwrap();
    assert_ne!(start, later);
}
