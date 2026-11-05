//! Scene invariants on the user's ROM (skipped without it).

use std::collections::{BTreeMap, HashSet};

use dq9_formats::Table;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList, MapRecord};
use dq9_render::Scene;
use dq9_render::scene::PolygonMode;
use nds_rom::Rom;

const IDENTITY: [[f32; 3]; 2] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

fn assemble(rom: &Rom, table: &Table, record: &MapRecord) -> Map {
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    Map::assemble(record, table, |p| map::rom_file(rom, p), &ctx).unwrap()
}

#[test]
fn sample_maps() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    for code in ["M01", "C01M09", "F01"] {
        let record = list.records.iter().find(|r| r.code == code.as_bytes());
        let m = assemble(&rom, &table, record.unwrap());
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
        // Every placement shown at this time is a model instance.
        for inst in &s.instances {
            assert!(!m.blocks[inst.block].instances[inst.instance].hidden);
        }
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
        let m = assemble(&rom, &table, record);
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
        let map = assemble(&rom, &table, record);
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
