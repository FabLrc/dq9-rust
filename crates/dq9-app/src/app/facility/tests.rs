//! The hero's look after a change of equipment, through the application (real-ROM regression).

use crate::talk::Button;

/// Fingerprint of the hero's posed geometry, observed from the runtime the renderer draws.
#[cfg(not(target_arch = "wasm32"))]
fn hero_shape(app: &crate::app::App) -> (usize, u64) {
    let runtime = app.runtime.as_ref().expect("a map is loaded");
    let vertices = runtime.hero_vertices().expect("the hero is in the scene");
    let hash = vertices.iter().fold(0u64, |hash, vertex| {
        let mut next = hash;
        for value in vertex.position {
            next = next
                .wrapping_mul(0x100000001b3)
                .wrapping_add(u64::from(value.to_bits()));
        }
        next
    });
    (vertices.len(), hash)
}

/// The field state a same-map reload would reset: the map's own load journal, the per-map
/// talk counters, a global flag set after the load, the world generator and the hero's pose.
/// The visible gear must change without touching any of them.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn changing_gear_rebuilds_the_hero_without_reloading_the_field() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let options = crate::app::Options {
        map: Some("M01M05".into()),
        save: None,
        time: None,
        clock: None,
        layout: crate::screens::Layout::default(),
        mute: true,
        record_sound: false,
        trace_inputs: false,
        event: None,
        story: dq9_formats::cast::Story {
            chapter: 2,
            step: 1,
            substep: 1,
            daytime: true,
            ..Default::default()
        },
        story_override: [None; 3],
        new_game: false,
        battle: None,
        play: Default::default(),
    };
    let mut app = crate::app::App::new_headless(options, Some(rom.to_vec()));
    let map = app.current.as_ref().expect("a map").0.id;
    // Live field state that a reload would replace: a talk counter and a global flag set
    // after the map was entered.
    app.state.talk_counts.record(7);
    app.state.flags.insert(0x7f01);
    let counts = app.state.talk_counts.get(7);
    let flags = app.state.flags.clone();
    let steps = app.journal.steps.len();
    let loads = app
        .journal
        .steps
        .iter()
        .filter(|(_, step)| matches!(step, dq9_core::scenario::Step::Map { id, .. } if *id == map))
        .count();
    let random = app.combat.world.random.state();
    let started = app.events.started().len();
    let pose = app.hero.expect("a hero").position();
    let yaw = app.hero.expect("a hero").yaw();
    let before = hero_shape(&app);

    // Change a visible piece through the real menu, then close it: Equip is the second row,
    // Torso the fourth slot, and Close the fifth menu row. The close calls `after_service`,
    // whose `look_dirty` path used to refresh through `load_map`.
    assert!(app.party.bag.add(13021), "a spare torso for the menu");
    assert!(app.open_field_menu());
    app.service_button(Button::Down);
    app.service_button(Button::Confirm);
    for _ in 0..3 {
        app.service_button(Button::Down);
    }
    app.service_button(Button::Confirm);
    app.service_button(Button::Confirm);
    app.service_button(Button::Cancel);
    // The equipment screen leaves its slot index as the menu cursor: Torso (3) → Close (4).
    app.service_button(Button::Down);
    app.service_button(Button::Confirm);
    assert!(!app.services.is_open(), "the menu closed");
    assert_eq!(
        app.party.members[0].equipment.torso,
        Some(13021),
        "the menu put the spare torso on"
    );

    assert_ne!(hero_shape(&app), before, "the drawn gear changed");
    assert_eq!(
        app.journal.steps.len(),
        steps,
        "no map or entry step was journaled"
    );
    assert_eq!(
        app.journal
            .steps
            .iter()
            .filter(
                |(_, step)| matches!(step, dq9_core::scenario::Step::Map { id, .. } if *id == map)
            )
            .count(),
        loads,
        "the map was not loaded again"
    );
    assert_eq!(app.state.talk_counts.get(7), counts, "talk counters kept");
    assert_eq!(app.state.flags, flags, "field flags kept");
    assert_eq!(
        app.combat.world.random.state(),
        random,
        "generator untouched"
    );
    assert_eq!(app.events.started().len(), started, "no event replayed");
    assert!(app.events.pending_start().is_none());
    let hero = app.hero.expect("a hero");
    assert_eq!(hero.position(), pose, "hero pose kept");
    assert_eq!(hero.yaw(), yaw, "hero facing kept");

    // Repeated alternating outfits: the visible geometry must follow each change while the
    // hero's model and instance slots and the scene's resource storage stay bounded. The old
    // rig is not left hidden beside the new one.
    let instance = app.hero_instance.expect("a hero instance");
    let hero_model = app.runtime.as_ref().unwrap().scene.instances[instance].model;
    let npc = app
        .runtime
        .as_ref()
        .unwrap()
        .npc_instance(7)
        .expect("the innkeeper is in the cast");
    let npc_world = app.runtime.as_ref().unwrap().scene.instances[npc].world;
    let other_texture_refs: Vec<Option<usize>> = app
        .runtime
        .as_ref()
        .unwrap()
        .scene
        .models
        .iter()
        .enumerate()
        .filter(|(model, _)| *model != hero_model)
        .flat_map(|(_, model)| model.materials.iter().map(|material| material.texture))
        .collect();
    let resource_counts = |app: &crate::app::App| {
        let scene = &app.runtime.as_ref().unwrap().scene;
        (
            scene.models.len(),
            scene.instances.len(),
            scene.textures.len(),
            scene
                .textures
                .iter()
                .map(|texture| texture.rgba.len())
                .sum::<usize>(),
        )
    };
    let sample = |app: &mut crate::app::App, torso: u16| {
        app.party.members[0].equipment.torso = Some(torso);
        app.refresh_hero_look();
        assert_eq!(
            app.hero_instance,
            Some(instance),
            "the hero instance is stable"
        );
        hero_shape(app)
    };
    let first = sample(&mut app, 13007);
    let first_storage = resource_counts(&app);
    let second = sample(&mut app, 13021);
    let second_storage = resource_counts(&app);
    sample(&mut app, 13007);
    sample(&mut app, 13021);
    let bounded = resource_counts(&app);
    let bytes_bound = first_storage.3.max(second_storage.3);
    assert_ne!(first, second, "the two outfits draw differently");
    for round in 0..20 {
        let torso = if round % 2 == 0 { 13007 } else { 13021 };
        let shape = sample(&mut app, torso);
        assert_eq!(
            shape,
            if torso == 13007 { first } else { second },
            "the same outfit draws the same geometry"
        );
        let storage = resource_counts(&app);
        assert_eq!(storage.0, bounded.0, "the scene model count stayed bounded");
        assert_eq!(
            storage.1, bounded.1,
            "the scene instance count stayed bounded"
        );
        assert_eq!(
            storage.2, bounded.2,
            "the scene texture count stayed bounded"
        );
        assert!(
            storage.3 <= bytes_bound,
            "texture storage {storage:?} stayed under {bytes_bound}"
        );
        let scene = &app.runtime.as_ref().unwrap().scene;
        assert_eq!(
            scene.instances[npc].world, npc_world,
            "the other actor kept its pose"
        );
        let refs: Vec<Option<usize>> = scene
            .models
            .iter()
            .enumerate()
            .filter(|(model, _)| *model != hero_model)
            .flat_map(|(_, model)| model.materials.iter().map(|material| material.texture))
            .collect();
        assert_eq!(refs, other_texture_refs, "other texture references kept");
    }

    // A weapon-only change is not drawn by the rig: no rebuild, no allocation.
    let shape = hero_shape(&app);
    let storage = resource_counts(&app);
    let pointer = app
        .runtime
        .as_ref()
        .unwrap()
        .hero_vertices()
        .unwrap()
        .as_ptr();
    app.party.members[0].equipment.weapon = Some(20005);
    app.refresh_hero_look();
    assert_eq!(hero_shape(&app), shape, "the drawn hero is unchanged");
    assert_eq!(resource_counts(&app), storage, "nothing was allocated");
    assert_eq!(
        app.runtime
            .as_ref()
            .unwrap()
            .hero_vertices()
            .unwrap()
            .as_ptr(),
        pointer,
        "the existing rig was kept"
    );

    // A failed replacement is transactional: the old rig, storage and instance survive.
    let shape = hero_shape(&app);
    let storage = resource_counts(&app);
    let look = app.look.clone();
    let runtime = app.runtime.as_mut().unwrap();
    assert!(
        runtime.replace_hero(&look, |_| None).is_err(),
        "a missing hero resource fails"
    );
    assert_eq!(hero_shape(&app), shape, "the old hero is still drawn");
    assert_eq!(resource_counts(&app), storage, "nothing was committed");
    assert_eq!(app.hero_instance, Some(instance));
    assert_eq!(
        app.journal.steps.len(),
        steps,
        "the repeated swaps did not reload or re-enter the field"
    );
    assert_eq!(app.state.talk_counts.get(7), counts, "talk counters kept");
    assert_eq!(app.state.flags, flags, "field flags kept");
    assert_eq!(
        app.combat.world.random.state(),
        random,
        "generator untouched"
    );
}
