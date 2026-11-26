//! The battle transition and scene: fighters on the grid, the swirl over the field, the scripted
//! battle's way in and out (real-ROM regressions).

use super::BattleChange;
use crate::app::App;
use crate::app::Options;
use crate::app::test_support::{entering_at_the_end, fighting, map_options};
use crate::battle::BattleScene;
use crate::battle::entry::{self, Lead};
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

    // The opening is the wide 22° shot of `func_ov000_0216118c`, from the party's side
    // (`z` +), pulled back by 0.5 and 3.0 until its easing settles; the eye stays within 17
    // and under 5 (`func_ov000_0216f2b8`).
    let eye = scene.camera().eye();
    let offset = eye - scene.camera().target;
    let reach = glam::Vec2::new(offset.x, offset.z).length();
    assert!(reach <= 17.0 + 1e-3, "the eye reaches {reach}");
    assert!(eye.y <= 5.0 + 1e-3, "the eye is {:.3} high", eye.y);
    assert!(eye.z > 0.0, "the opening looks from the party's side");
    assert!(
        (scene.camera().fovy.to_degrees() - 44.0).abs() < 1e-3,
        "the opening is the wide shot"
    );
    // `Shot::Side` names the side looked at; the eye stands opposite. The normal 15° shot of
    // the monsters is the game's side 1 (`func_ov000_0216d600` `param_2 == 1`).
    scene.set_shot(Shot::Side(Side::Monsters));
    let offset = scene.camera().eye() - scene.camera().target;
    let reach = glam::Vec2::new(offset.x, offset.z).length();
    assert!(reach <= 17.0 + 1e-3, "the eye reaches {reach}");
    assert!(scene.camera().eye().z > 0.0);
    assert!((scene.camera().fovy.to_degrees() - 30.0).abs() < 1e-3);
    scene.set_shot(Shot::Side(Side::Party));
    assert!(scene.camera().eye().z < 0.0, "looking at the party");

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
    // A roamer's contact freezes the picture while the battle's data loads: no field step, no
    // swirl, no camera change (`battle/entry.rs`, 12 frames measured).
    for step in 1..=12 {
        app.run_steps(1);
        assert_eq!(app.view.camera.roll, 0.0, "step {step} of the freeze");
        assert_eq!(app.encounter.battle_cover, 0.0);
        assert!(app.swirl_clock().is_none(), "the swirl waits");
        assert!(!drawn(&app), "the disc is not placed yet");
    }
    // The 13th step: the swirl's first update, over the field.
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
    // The fade starts after the counter passes 15: the 18th step of the swirl.
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
    // The battle stage's own load holds the black for the measured 94 frames.
    app.run_steps(entry::LOAD_BLACK_STEPS);
    assert_eq!(app.encounter.battle_cover, 1.0);
    assert!(app.battle().is_some());
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

/// The entry's phases, tick for tick: a roamer's contact freezes 12 steps, the swirl takes 38,
/// the battle stage's load holds the black 94 and the scene fades in over 15; a set battle
/// waits its 15 updates (30 steps) instead of the freeze, and a preview skips both and the
/// load black (`battle/entry.rs`). No phase is skipped, and none of them is spent headless.
#[test]
fn the_entry_spends_each_phase_once_in_order() {
    let Some(rom) = testrom::load() else { return };

    fn phase(app: &App) -> &'static str {
        match &app.encounter.battle_change {
            Some(BattleChange::Preparing { .. }) => "lead-in",
            Some(BattleChange::Entering { .. }) => "swirl",
            Some(BattleChange::Loading { .. }) => "black",
            Some(BattleChange::Fighting { .. }) => "scene",
            Some(_) => "other",
            None => "field",
        }
    }

    /// The runs of one phase, in order: `(phase, steps)`.
    fn runs(app: &mut App, steps: u32) -> Vec<(&'static str, u32)> {
        let mut out: Vec<(&'static str, u32)> = Vec::new();
        for _ in 0..steps {
            let now = phase(app);
            match out.last_mut() {
                Some((last, count)) if *last == now => *count += 1,
                _ => out.push((now, 1)),
            }
            app.run_steps(1);
        }
        out
    }

    let request = |record: Option<u16>| crate::battle::Request {
        record,
        stage: 0,
        monsters: vec![(1, 2)],
        surprise: Some(dq9_core::battle::Surprise::None),
        ..Default::default()
    };

    // A roamer's contact: 12 frozen, the swirl, 94 black, 15 up.
    let mut app = App::new_replay(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    app.start_battle(request(None), false, false);
    assert_eq!(app.encounter.entry_lead, Lead::Contact);
    assert_eq!(
        runs(&mut app, entry::entry_steps(Lead::Contact)),
        vec![
            ("lead-in", entry::FREEZE_STEPS),
            ("swirl", entry::Phase::Swirl.steps()),
            ("black", entry::LOAD_BLACK_STEPS),
            ("scene", entry::SCENE_FADE_STEPS),
        ]
    );

    // A set battle (battle 2, the Hexagoon): the swirl task's 15 updates (30 steps) first.
    let mut app = App::new_replay(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    app.start_battle(request(Some(2)), true, false);
    assert_eq!(app.encounter.entry_lead, Lead::SetBattle);
    assert_eq!(
        runs(&mut app, entry::entry_steps(Lead::SetBattle)),
        vec![
            ("lead-in", entry::DELAY_STEPS),
            ("swirl", entry::Phase::Swirl.steps()),
            ("black", entry::LOAD_BLACK_STEPS),
            ("scene", entry::SCENE_FADE_STEPS),
        ]
    );

    // A preview skips the lead-in and the load black: its swirl starts done, so the scene is
    // built on the first step and only the fade-in is spent.
    let mut app = App::new_replay(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    app.start_battle(request(None), false, true);
    assert_eq!(app.encounter.entry_lead, Lead::Preview);
    assert_eq!(phase(&app), "swirl");
    app.run_steps(1);
    assert!(
        app.battle().is_some(),
        "the preview's scene is built at once"
    );
    assert_eq!(
        runs(&mut app, entry::SCENE_FADE_STEPS),
        vec![("scene", entry::SCENE_FADE_STEPS)]
    );

    // Headless: no presentation step at all (`tests/real_rom.rs` and `parity.ps1` pin the
    // rest); the scene is built and settled inside the same fixed step.
    let mut app = App::new_headless(
        fighting(map_options("1100", (1, 2, 1, true)), 7),
        Some(rom.to_vec()),
    );
    let before = app.tick;
    app.start_battle(request(None), false, false);
    assert!(app.last_battle().is_some(), "the fight is settled at once");
    assert!(app.encounter.battle_change.is_none(), "no phase is entered");
    assert_eq!(app.tick, before, "no step was spent");
}

/// The way back holds the black 95 steps while the field reloads, then fades up over 30
/// (`battle/entry.rs`): 15 + 95 + 30 = 140 steps from the last window.
#[test]
fn the_way_back_holds_the_black_ninety_five_steps() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_replay(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    let request = crate::battle::Request {
        stage: 0,
        monsters: vec![(1, 2)],
        surprise: Some(dq9_core::battle::Surprise::None),
        ..Default::default()
    };
    app.start_battle(request, false, false);
    // The scene, the lead-in, the swirl, the black and the fade-in done.
    app.run_steps(entry::entry_steps(Lead::Contact));
    assert!(app.battle().is_some());
    assert_eq!(app.encounter.battle_cover, 0.0);
    let resolved = match &app.encounter.battle_change {
        Some(BattleChange::Fighting { request, .. }) => request.clone(),
        other => panic!("the fight should be on its scene: {other:?}"),
    };
    app.encounter.battle_change = Some(BattleChange::Leaving {
        ticks: 0,
        request: resolved,
        restored: false,
        outcome: None,
    });
    app.run_steps(entry::FADE_OUT_STEPS);
    assert_eq!(app.encounter.battle_cover, 1.0, "both screens are black");
    // The field is restored on the black and reloads there (native 844's placements kept).
    assert!(app.battle().is_none(), "the field is back behind the black");
    app.run_steps(entry::RETURN_BLACK_STEPS - 1);
    assert_eq!(app.encounter.battle_cover, 1.0);
    app.run_steps(1);
    assert_eq!(
        app.encounter.battle_cover, 1.0,
        "the fade up starts after the load"
    );
    app.run_steps(entry::RETURN_FADE_STEPS);
    assert_eq!(app.encounter.battle_cover, 0.0, "the field is up");
    assert!(
        app.encounter.battle_change.is_none(),
        "the last step settles the fight"
    );
}

/// The same transition rendered offscreen on the real map: the field freezes for the contact's
/// load, rolls and narrows with the swirl, the rings cover it, both screens end black. Frames at
/// the swirl's steps 1, 10, 20, 30 and 38 go to `DQ9_EVIDENCE_DIR` when it is set (they are the
/// window's own scene, clock and camera).
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
    // The contact's 12 frozen frames first: the disc is not drawn before its first update.
    app.run_steps(entry::FREEZE_STEPS);
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

/// The row formation, placed from the ROM's tables: the one-hero party at `z` +2.5 turned to
/// the ROM's own π (`0x02183158`), the three slimes side by side at `z` −2.5 on the gap of
/// `func_ov000_02167b5c` and centred (`ov_000::021675a0`, `ov_000::021677fc`).
#[test]
fn the_row_formation_places_one_hero_and_three_monsters() {
    use crate::battle::{Fighter, Formation};

    let Some(rom) = testrom::load() else { return };
    let clock = DayClock::new(0.0, true);
    let resolved = crate::battle::Request {
        stage: crate::battle::DEFAULT_STAGE,
        monsters: vec![(1, 3)],
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
    scene.set_formation(Formation::Row);
    assert_eq!(scene.formation(), Formation::Row);
    let hero = scene.fighter(Fighter::party(0)).unwrap();
    assert!((hero.position.x).abs() < 1e-6, "the hero is centred");
    assert!((hero.position.z - 2.5).abs() < 1e-6, "at z +2.5");
    assert!(
        (hero.heading - 0x323d as f32 / 4096.0).abs() < 1e-6,
        "turned to the ROM's π, {}",
        hero.heading
    );
    let monsters: Vec<_> = (0..3)
        .map(|index| *scene.fighter(Fighter::monster(index)).unwrap())
        .collect();
    for monster in &monsters {
        assert!((monster.position.z + 2.5).abs() < 1e-6, "at z −2.5");
        assert_eq!(monster.heading, 0.0, "facing the party");
    }
    let xs: Vec<f32> = monsters.iter().map(|m| m.position.x).collect();
    assert!(xs[1].abs() < 1e-6, "the middle slime is centred: {xs:?}");
    assert!((xs[0] + xs[2]).abs() < 1e-6, "the row is symmetric: {xs:?}");
    // One gap between each pair (`func_ov000_02167b5c`), and a row, not the grid's slots.
    let step = xs[2] - xs[1];
    assert!((xs[1] - xs[0] - step).abs() < 1e-6, "one gap between them");
    assert!(
        (0.5..4.0).contains(&step),
        "the row's spacing, not the grid's 3.897: {xs:?}"
    );
    // The places the CLI's `--screenshot` prints for a battle, in the row (no `--screenshot`
    // flag asks for the row yet: `battle_screenshot` is outside this batch's scope).
    for placed in scene.fighters() {
        println!(
            "row {:?} {} at ({:.3}, {:.3}, {:.3}) heading {:.3}",
            placed.fighter,
            placed
                .number
                .map_or_else(|| "hero".to_owned(), |n| format!("monster {n}")),
            placed.position.x,
            placed.position.y,
            placed.position.z,
            placed.heading
        );
    }
}

/// The transition's boundaries rendered offscreen on the real map, written to
/// `DQ9_EVIDENCE_DIR` when it is set: the field before the contact, the roamer's 12 frozen
/// frames (byte-identical to the frame before: the picture does not move), both screens black
/// while the battle stage loads, the scene after its fade-in, the field back after the return's
/// 95 black frames, a set battle's own lead-in (the swirl task's 15 updates) and the Hexagoon's
/// stage (`battle/entry.rs`, `docs/re/battle-transition.md`).
#[test]
fn the_transition_boundaries_are_captured() {
    use dq9_render::{Renderer, Viewport};

    let Some(rom) = testrom::load() else { return };
    let evidence = std::env::var_os("DQ9_EVIDENCE_DIR").map(std::path::PathBuf::from);
    let (device, queue) = pollster::block_on(async {
        let instance = crate::gpu::instance().await;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("a GPU adapter for the transition capture");
        crate::gpu::device(&adapter).await.unwrap()
    });
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 256,
        height: 192,
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let shoot = |app: &mut App| -> Vec<u8> {
        let cover = app.encounter.battle_cover;
        let overlay = (cover > 0.0).then_some([0.0, 0.0, 0.0, cover]);
        if let Some(battle) = app.encounter.battle.as_mut() {
            let camera = *battle.camera();
            let mut renderer = Renderer::new(&device, &queue, format, &battle.runtime().scene);
            renderer.environment = battle.environment.clone();
            renderer.environment.cover = overlay;
            battle
                .upload(&mut renderer, &device, &queue)
                .expect("uploading the battle stage");
            renderer
                .render_to_rgba_with(&device, &queue, (256, 192), viewport, &camera, |_, _| {})
                .unwrap()
        } else {
            let disc = app.encounter.swirl.expect("the field carries the swirl");
            let camera = app.view.camera;
            let environment = app.environment.clone();
            let runtime = app.runtime.as_mut().expect("the field is on screen");
            let mut renderer = Renderer::new(&device, &queue, format, &runtime.scene);
            renderer.environment = environment;
            renderer.environment.cover = overlay;
            runtime.upload(&mut renderer, &device, &queue).unwrap();
            let world = runtime.scene.instances[disc.instance()].world;
            renderer.update_instance_world(&queue, disc.instance(), world);
            renderer
                .render_to_rgba_with(&device, &queue, (256, 192), viewport, &camera, |_, _| {})
                .unwrap()
        }
    };
    let save = |name: &str, rgba: &[u8]| {
        if let Some(dir) = &evidence {
            std::fs::create_dir_all(dir).unwrap();
            let file = std::fs::File::create(dir.join(name)).unwrap();
            let mut encoder = png::Encoder::new(file, 256, 192);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(rgba)
                .unwrap();
        }
    };
    let black = |rgba: &[u8]| rgba.as_chunks::<4>().0.iter().all(|p| p[..3] == [0, 0, 0]);

    let mut app = App::new_replay(map_options("1100", (1, 2, 1, true)), Some(rom.to_vec()));
    let request = |record: Option<u16>| crate::battle::Request {
        record,
        stage: 0,
        monsters: vec![(1, 2)],
        surprise: Some(dq9_core::battle::Surprise::None),
        ..Default::default()
    };

    // 00: the field, before the contact.
    let before = shoot(&mut app);
    save("entry-00-field.png", &before);

    // 01: the roamer's freeze, its last frame: the same picture, no disc, no cover.
    app.start_battle(request(None), false, false);
    app.run_steps(entry::FREEZE_STEPS - 1);
    let frozen = shoot(&mut app);
    save("entry-01-freeze.png", &frozen);
    assert_eq!(
        frozen, before,
        "the picture does not move during the freeze"
    );

    // 02: the load black, inside the 94 frames after the swirl.
    app.run_steps(1 + entry::Phase::Swirl.steps() + 40);
    assert_eq!(app.encounter.battle_cover, 1.0);
    assert!(
        app.battle().is_some(),
        "the stage is built behind the black"
    );
    let dark = shoot(&mut app);
    save("entry-02-black.png", &dark);
    assert!(black(&dark), "both screens are black while the stage loads");

    // 03: the scene, its fade-in done (the roamer's F01 stage).
    app.run_steps(entry::LOAD_BLACK_STEPS - 40 + entry::SCENE_FADE_STEPS);
    assert_eq!(app.encounter.battle_cover, 0.0);
    let scene = shoot(&mut app);
    save("entry-03-scene.png", &scene);
    assert!(!black(&scene), "the stage is shown");

    // 04: the field back, after the 15 + 95 + 30 steps of the way back.
    let resolved = match &app.encounter.battle_change {
        Some(BattleChange::Fighting { request, .. }) => request.clone(),
        other => panic!("the fight should be on its scene: {other:?}"),
    };
    app.encounter.battle_change = Some(BattleChange::Leaving {
        ticks: 0,
        request: resolved,
        restored: false,
        outcome: None,
    });
    app.run_steps(entry::leave_steps());
    assert!(app.battle().is_none() && app.encounter.battle_cover == 0.0);
    let back = shoot(&mut app);
    save("entry-04-back.png", &back);
    assert_eq!(back, before, "the same map and camera come back");

    // 05: a set battle's lead-in (battle 2, the Hexagoon): 15 updates of the swirl task.
    app.start_battle(request(Some(2)), true, false);
    assert_eq!(app.encounter.entry_lead, Lead::SetBattle);
    app.run_steps(entry::DELAY_STEPS / 2);
    assert!(app.battle().is_none() && app.encounter.battle_cover == 0.0);
    assert_eq!(app.view.camera.roll, 0.0, "the delay does not roll");
    let delay = shoot(&mut app);
    save("entry-05-set-battle-delay.png", &delay);
    assert_eq!(delay, before, "the field holds still during the delay");

    // 06: the Hexagoon's load black, after its swirl (`entry-02` is the same phase on a roamer).
    app.run_steps(entry::DELAY_STEPS / 2 + entry::Phase::Swirl.steps() + 40);
    assert_eq!(app.encounter.battle_cover, 1.0);
    assert!(
        app.battle().is_some(),
        "the Hexagoon's stage is built behind it"
    );
    let hexagoon_black = shoot(&mut app);
    save("entry-06-hexagoon-black.png", &hexagoon_black);
    assert!(black(&hexagoon_black));

    // 07: the Hexagoon's stage, after its own black and fade-in.
    app.run_steps(entry::LOAD_BLACK_STEPS - 40 + entry::SCENE_FADE_STEPS);
    assert!(app.battle().is_some() && app.encounter.battle_cover == 0.0);
    let hexagoon = shoot(&mut app);
    save("entry-07-hexagoon.png", &hexagoon);
    assert!(!black(&hexagoon), "the Hexagoon's stage is shown");
}
