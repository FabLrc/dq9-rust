//! The battle transition and scene: fighters on the grid, the swirl over the field, the scripted
//! battle's way in and out (real-ROM regressions).

use crate::app::App;
use crate::app::Options;
use crate::app::test_support::{entering_at_the_end, fighting, map_options};
use crate::battle::BattleScene;
use crate::screens::Layout;
use crate::talk::Button;
use dq9_core::battle::BattleResult;
use dq9_core::scenario::Step;
use dq9_core::state::DayClock;
use dq9_formats::hero::HeroLook;
use glam::Vec3;

/// The battle set-up puts the party on `+z` and the monsters on `−z`, on the 9 by 9 grid
/// (`ov_000::02164d74`, `ov_000::0216f74c`), and the camera keeps its eye within 17 and under
/// 5 (`ov_000::0216f2b8`).
#[test]
fn a_three_slime_battle_places_the_fighters_on_the_grid() {
    use crate::battle::{Fighter, Formation, Shot, Side};

    let Some(rom) = testrom::load() else { return };
    let clock = DayClock::new(0.0, true);
    let request = crate::battle::Request {
        record: None,
        stage: crate::battle::DEFAULT_STAGE,
        monsters: vec![(1, 3)],
        track: None,
        surprise: None,
    };
    let resolved = request.resolve(rom).unwrap();
    let mut scene = BattleScene::build(
        rom,
        &resolved,
        clock.phase().index(),
        clock,
        &dq9_formats::cast::Story::default(),
        &HeroLook::new_game(),
    )
    .unwrap();
    assert_eq!(scene.stage(), 30116);
    assert_eq!(scene.formation(), Formation::Grid);

    // One hero: slot 58 (column 4, row 6) → (0, 4.5), facing π.
    let hero = *scene.fighter(Fighter::party(0)).unwrap();
    assert!(hero.position.x.abs() < 1e-3, "{:?}", hero.position);
    assert!((hero.position.z - 4.5).abs() < 1e-3, "{:?}", hero.position);
    assert!((hero.heading - std::f32::consts::PI).abs() < 1e-6);
    // Three slimes: slots 29, 22, 32, facing 0.
    let expected = [(-3.897, -2.25), (0.0, -4.5), (3.897, -2.25)];
    for (index, (x, z)) in expected.into_iter().enumerate() {
        let monster = *scene.fighter(Fighter::monster(index)).unwrap();
        assert!(
            (monster.position.x - x).abs() < 1e-3,
            "{:?}",
            monster.position
        );
        assert!(
            (monster.position.z - z).abs() < 1e-3,
            "{:?}",
            monster.position
        );
        assert_eq!(monster.heading, 0.0);
        assert_eq!(monster.number, Some(1));
    }
    assert_eq!(scene.fighters().len(), 4);

    // The eye never reaches further than 17 nor higher than 5; the wide opening frames from
    // 22°, so the clamp bites on the normal side shot (15°).
    let eye = scene.camera().eye();
    let offset = eye - scene.camera().target;
    let reach = glam::Vec2::new(offset.x, offset.z).length();
    assert!(reach <= 17.0 + 1e-3, "the eye reaches {reach}");
    assert!(eye.y <= 5.0 + 1e-3, "the eye is {:.3} high", eye.y);
    assert!(
        (scene.camera().fovy.to_degrees() - 44.0).abs() < 1e-3,
        "the opening is the wide shot"
    );
    // The normal side shot and the monsters' side turn the framing round.
    scene.set_shot(Shot::Side(Side::Monsters));
    let offset = scene.camera().eye() - scene.camera().target;
    let reach = glam::Vec2::new(offset.x, offset.z).length();
    assert!(
        (reach - 17.0).abs() < 1e-3,
        "the eye is clamped at 17: {reach}"
    );
    assert!(scene.camera().eye().z < 0.0);
    assert!((scene.camera().fovy.to_degrees() - 30.0).abs() < 1e-3);

    // The idle of a slime is `stand`; the defeat fade hides it and takes its screen position.
    assert!(scene.play(Fighter::monster(0), "stand"));
    let at = scene.fighter_screen_pos(Fighter::party(0)).unwrap();
    assert!(
        (0.0..=256.0).contains(&at.x) && (0.0..=192.0).contains(&at.y),
        "{at:?}"
    );
    assert!(scene.remove(Fighter::monster(1)));
    assert!(scene.fighter_screen_pos(Fighter::monster(1)).is_none());
}

/// `eventbattle.bin` index 2 is the Hexagoon alone (monster 300): its set battle loads the
/// stage the record names with the hero facing it.
#[test]
fn the_hexagoon_set_battle_stands_on_its_stage() {
    use crate::battle::{Fighter, Side};

    let Some(rom) = testrom::load() else { return };
    let request = crate::battle::Request {
        record: Some(2),
        ..Default::default()
    };
    let resolved = request.resolve(rom).unwrap();
    assert_eq!(resolved.number, Some(2));
    assert_eq!(resolved.monsters, vec![(300, 1)]);
    assert_ne!(resolved.stage, 0);
    let clock = DayClock::new(0.0, true);
    let mut scene = BattleScene::build(
        rom,
        &resolved,
        clock.phase().index(),
        clock,
        &dq9_formats::cast::Story::default(),
        &HeroLook::new_game(),
    )
    .unwrap();
    assert_eq!(scene.stage(), resolved.stage);
    assert_eq!(scene.entry().id, resolved.stage);
    assert_eq!(scene.fighters().len(), 2);
    let monster = *scene.fighter(Fighter::monster(0)).unwrap();
    assert_eq!(monster.number, Some(300));
    assert!(monster.position.x.abs() < 1e-3 && (monster.position.z + 4.5).abs() < 1e-3);
    let hero = *scene.fighter(Fighter::party(0)).unwrap();
    assert!((hero.position.z - 4.5).abs() < 1e-3);
    // The hexagoon's model archive has its idle.
    assert!(scene.play(Fighter::monster(0), "stand"));
    assert!(
        scene.camera().eye().z > 0.0,
        "the opening looks from the party's side"
    );
    let at = scene.fighter_screen_pos(Fighter::monster(0)).unwrap();
    assert!(
        (0.0..=256.0).contains(&at.x) && (0.0..=192.0).contains(&at.y),
        "{at:?}"
    );
    let _ = Side::Party;
}

/// Scripted battle 26 (`0x78(26)` after `21507`) shows its scene — stage 30116, monsters 290,
/// 292, 290 — and gives the field back at the same map and pose.
#[test]
fn scripted_battle_26_enters_and_leaves_the_scene_and_gives_the_field_back() {
    let Some(rom) = testrom::load() else { return };
    let options = Options {
        map: None,
        save: None,
        time: None,
        clock: None,
        layout: Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: 1,
            step: 1,
            substep: 1,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: true,
        battle: None,
        play: Default::default(),
    };
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    let mut before: Option<(u16, Vec3)> = None;
    let mut settled = false;
    for frame in 0..40_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        let previous = app
            .current
            .as_ref()
            .zip(app.hero)
            .map(|((entry, _), hero)| (entry.id, hero.position()));
        if previous.is_some() {
            before = previous;
        }
        app.run_steps(1);
        if app.journal.steps.iter().any(|(_, step)| {
            matches!(
                step,
                Step::Battle {
                    number: 26,
                    record: Some(_),
                    won: true,
                    fought: None
                }
            )
        }) {
            settled = true;
            break;
        }
    }
    assert!(settled, "battle 26 must be settled by the stand-in");
    // The headless run builds the scene and settles it at once (no fixed step is spent on
    // it), so the timeline is the story's; the window shows the transition itself.
    let shown = app.last_battle().expect("battle 26's scene was built");
    assert_eq!(shown.stage, 30116, "battle 26 is fought on the field stage");
    assert_eq!(shown.fighters, 4, "the hero and three monsters");
    let (map, pose) = before.expect("the field before the battle");
    let now = app.current.as_ref().map(|(entry, _)| entry.id).unwrap();
    assert_eq!(now, map, "the battle gives the same map back");
    let hero = app.hero.expect("the hero is back");
    assert!(
        (hero.position() - pose).length() < 1e-3,
        "{:?} vs {pose:?}",
        hero.position()
    );
    assert!(!app.battle_is_active());
    assert!(!app.events.is_active() || app.events.pending_start().is_some());
}

/// The window's path without a window: the transition builds the scene, the fight reads the
/// pad the keyboard fills (`confirm` held), the scene is left, the field is back and the party
/// is paid. `proxy` is only what makes a run fight by itself; the phases are the same.
#[test]
fn the_window_path_plays_a_battle_from_the_pad() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(
        fighting(map_options("1100", (1, 2, 1, true)), 9),
        Some(rom.to_vec()),
    );
    let map = app.current.as_ref().map(|(e, _)| e.id);
    let request = crate::battle::Request {
        stage: 0,
        monsters: vec![(1, 2)],
        surprise: Some(dq9_core::battle::Surprise::None),
        ..Default::default()
    };
    let resolved = request.resolve(rom).unwrap();
    app.encounter.battle_change = Some(entering_at_the_end(resolved));
    let mut seen_menu = false;
    let mut seen_message = false;
    for _ in 0..20_000 {
        if !app.battle_is_active() {
            break;
        }
        if let Some(fight) = &app.combat.fight {
            let view = fight.view();
            seen_menu |= view.menu.is_some();
            seen_message |= view.message.is_some();
        }
        app.combat.battle_pad.confirm = true;
        app.run_steps(1);
    }
    assert!(!app.battle_is_active(), "the battle must end");
    assert!(
        seen_menu && seen_message,
        "the fight showed its menu and windows"
    );
    let last = app.last_fight().expect("the fight is summarised");
    assert_eq!(last.result, BattleResult::Won);
    assert_eq!(app.party().gold, last.gold);
    assert!(app.combat.fight.is_none() && app.battle().is_none());
    assert_eq!(
        app.current.as_ref().map(|(e, _)| e.id),
        map,
        "the field is back"
    );
}

/// The encounter transition the window draws (roaming contact and native 547 alike): the
/// swirl's clock rolls the field camera −8° and narrows its half field of view by 0.4333° per
/// 30 Hz update, blackens both screens from its 18th step and builds the battle scene when it
/// is done and the cover is full (step 38), then `SwirlEffect_Release` gives the camera back
/// (roll 0, 15°) and takes the disc out of the field's scene
/// (`docs/re/battle-transition.md`).
#[test]
fn the_swirl_rolls_the_field_camera_before_the_battle_scene() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_replay(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    let camera = app.view.camera;
    assert_eq!(camera.roll, 0.0, "the field camera starts level");
    assert!((camera.fovy.to_degrees() - 30.0).abs() < 1e-3);
    let disc = app
        .encounter
        .swirl
        .expect("the field scene carries the swirl");
    let drawn = |app: &App| {
        app.runtime
            .as_ref()
            .expect("the field is drawn")
            .scene
            .instances[disc.instance()]
        .drawn()
    };
    assert!(!drawn(&app), "the disc starts hidden");
    let request = crate::battle::Request {
        stage: 0,
        monsters: vec![(1, 2)],
        surprise: Some(dq9_core::battle::Surprise::None),
        ..Default::default()
    };
    app.start_battle(request, false, false);
    // The field is still the scene on screen: the swirl runs over it.
    app.run_steps(1);
    app.draw_swirl(app.swirl_clock());
    assert!(
        app.battle().is_none(),
        "the field is drawn during the swirl"
    );
    assert!(drawn(&app), "the swirl is placed over the field");
    assert!(
        (app.view.camera.roll.to_degrees() + 8.0).abs() < 1e-3,
        "the first update rolls −8°, {}",
        app.view.camera.roll.to_degrees()
    );
    assert!((app.view.camera.fovy.to_degrees() / 2.0 - 14.567).abs() < 1e-2);
    assert_eq!(app.encounter.battle_cover, 0.0);
    let clock = app.swirl_clock().expect("the clock is still running");
    assert!(
        (clock.model_frame() - 2.5).abs() < 1e-4,
        "frame 1 + 3 × 0.5"
    );
    // The fade starts after the counter passes 15: the 18th step.
    app.run_steps(16);
    assert_eq!(
        app.encounter.battle_cover, 0.0,
        "both screens are still lit"
    );
    app.run_steps(1);
    assert!(app.encounter.battle_cover > 0.0 && app.encounter.battle_cover < 0.5);
    app.run_steps(19);
    assert!(app.encounter.battle_cover > 0.9 && app.encounter.battle_cover < 1.0);
    assert!(app.battle().is_none(), "the scene waits for the black");
    // The 38th step: done and covered, the scene is built and the camera given back.
    app.run_steps(1);
    assert!(app.battle().is_some(), "the scene is built on the black");
    assert_eq!(app.encounter.battle_cover, 1.0);
    assert_eq!(app.view.camera.roll, 0.0);
    assert!((app.view.camera.fovy.to_degrees() - 30.0).abs() < 1e-3);
    let field = app
        .encounter
        .saved_field
        .as_ref()
        .and_then(|saved| saved.runtime.as_ref())
        .expect("the field is set aside");
    assert!(
        !field.scene.instances[disc.instance()].drawn(),
        "the released disc must not stay in the field's scene"
    );
}

/// The same transition rendered offscreen on the real map: the field rolls and narrows, the
/// swirl's rings cover it, both screens end black. Frames at steps 1, 10, 20, 30 and 38 go to
/// `DQ9_EVIDENCE_DIR` when it is set (they are the window's own scene, clock and camera).
#[test]
fn the_swirl_is_drawn_over_the_field() {
    use dq9_render::{Renderer, Viewport};

    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_replay(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    let disc = app
        .encounter
        .swirl
        .expect("the field scene carries the swirl");
    let request = crate::battle::Request {
        stage: 0,
        monsters: vec![(1, 2)],
        surprise: Some(dq9_core::battle::Surprise::None),
        ..Default::default()
    };
    app.start_battle(request, false, false);
    let (device, queue) = pollster::block_on(async {
        let instance = crate::gpu::instance().await;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("a GPU adapter for the swirl capture");
        crate::gpu::device(&adapter).await.unwrap()
    });
    let mut renderer = Renderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &app.runtime.as_ref().expect("a map is loaded").scene,
    );
    renderer.environment = app.environment.clone();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 256,
        height: 192,
    };
    let frame = |app: &mut App, renderer: &mut Renderer, show_disc: bool| {
        // `App::redraw`'s field path: the disc placed by the clock, the scene's geometry, the
        // camera the clock drives and the cover both screens share.
        app.draw_swirl(app.swirl_clock());
        if let Some(runtime) = app.runtime.as_mut() {
            runtime
                .upload(renderer, &device, &queue)
                .expect("uploading the field");
        }
        let world = app
            .runtime
            .as_ref()
            .or_else(|| {
                app.encounter
                    .saved_field
                    .as_ref()
                    .and_then(|saved| saved.runtime.as_ref())
            })
            .expect("the field scene")
            .scene
            .instances[disc.instance()]
        .world;
        let world = if show_disc {
            world
        } else {
            glam::Mat4::from_scale(Vec3::ZERO)
        };
        renderer.update_instance_world(&queue, disc.instance(), world);
        let cover = (app.encounter.battle_cover > 0.0).then_some([
            0.0,
            0.0,
            0.0,
            app.encounter.battle_cover,
        ]);
        renderer.environment.cover = cover;
        renderer
            .render_to_rgba_with(
                &device,
                &queue,
                (256, 192),
                viewport,
                &app.view.camera,
                |_, _| {},
            )
            .unwrap()
    };
    let evidence = std::env::var_os("DQ9_EVIDENCE_DIR").map(std::path::PathBuf::from);
    let mut covered = 0usize;
    let mut last = Vec::new();
    for step in 1..=38u32 {
        app.run_steps(1);
        let with = frame(&mut app, &mut renderer, true);
        if step == 20 {
            // The disc's own pixels: the same frame with it out of the picture.
            let without = frame(&mut app, &mut renderer, false);
            covered = with
                .as_chunks::<4>()
                .0
                .iter()
                .zip(without.as_chunks::<4>().0)
                .filter(|(a, b)| a != b)
                .count();
        }
        if let Some(dir) = &evidence
            && [1, 10, 20, 30, 38].contains(&step)
        {
            std::fs::create_dir_all(dir).unwrap();
            let file =
                std::fs::File::create(dir.join(format!("swirl-field-step{step:02}.png"))).unwrap();
            let mut encoder = png::Encoder::new(file, 256, 192);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&with)
                .unwrap();
        }
        last = with;
    }
    assert!(
        covered > 5_000,
        "the rings cover a good part of the picture at step 20 ({covered} pixels differ)"
    );
    assert!(
        last.as_chunks::<4>().0.iter().all(|p| p[..3] == [0, 0, 0]),
        "both screens are black when the scene is built"
    );
}

/// A monster that joins the fight (a call for backup) is drawn on the next place of the grid,
/// the monsters take their places again for the new count, and a fallen one stays hidden.
#[test]
fn a_monster_that_joins_the_fight_is_placed_and_the_fallen_stay_hidden() {
    use crate::battle::Fighter;

    let Some(rom) = testrom::load() else { return };
    let clock = DayClock::new(0.0, true);
    let resolved = crate::battle::Request {
        stage: crate::battle::DEFAULT_STAGE,
        monsters: vec![(1, 2)],
        ..Default::default()
    }
    .resolve(rom)
    .unwrap();
    let mut scene = BattleScene::build(
        rom,
        &resolved,
        clock.phase().index(),
        clock,
        &dq9_formats::cast::Story::default(),
        &HeroLook::new_game(),
    )
    .unwrap();
    assert!(scene.remove(Fighter::monster(0)));
    scene.add_monster(rom, 43).unwrap();
    assert_eq!(scene.fighters().len(), 4);
    let joined = *scene.fighter(Fighter::monster(2)).unwrap();
    assert_eq!(joined.number, Some(43));
    assert!(joined.alive && scene.fighter_screen_pos(Fighter::monster(2)).is_some());
    assert!(
        scene.fighter_screen_pos(Fighter::monster(0)).is_none(),
        "the fallen monster is still hidden"
    );
    // Three monsters stand on the three-monster slots (29, 22, 32: x −3.897, 0, 3.897).
    let xs: Vec<f32> = (0..3)
        .map(|i| scene.fighter(Fighter::monster(i)).unwrap().position.x)
        .collect();
    assert!(
        (xs[0] + 3.897).abs() < 1e-3 && xs[1].abs() < 1e-3 && (xs[2] - 3.897).abs() < 1e-3,
        "{xs:?}"
    );
}
