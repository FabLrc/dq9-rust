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

// === The church's Confession (`ov_003::02159d0c`), on the real ROM ===

/// A blank container with a valid normal slot 0 and no mirror: what `--save` opens.
fn container() -> Vec<u8> {
    use dq9_formats::save::{FILE_SIZE, MAGIC, Save};
    let mut data = vec![0u8; FILE_SIZE];
    data[..MAGIC.len()].copy_from_slice(MAGIC);
    data[21] = 1; // slot 0 exists; byte 20 = 0: a normal save, not an intermission
    let mut save = Save::parse(&data).unwrap();
    save.slot_mut(0).unwrap().seal();
    save.as_bytes().to_vec()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dq9-confession-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An application standing on a real map with the container as `--save`.
fn app_with_save(rom: &[u8], bytes: Option<Vec<u8>>) -> crate::app::App {
    let options = crate::app::Options {
        map: Some("M01M05".into()),
        save: bytes,
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
    crate::app::App::new_headless(options, Some(rom.to_vec()))
}

/// Opens `<CHURCH=1>` and chooses the first row, which `str_ch0` makes the Confession.
fn open_confession(app: &mut crate::app::App) {
    assert!(app.open_service(crate::talk::ServiceTag::Church(1)));
    app.service_button(Button::Confirm);
    assert_eq!(
        app.services.confession_line(),
        Some(1010),
        "the Confession greets with the European str_ch0's 1010"
    );
}

/// Yes writes the adventure log: the file `--save-out` names reloads to the state the run
/// holds, the `--save` container is untouched, and the flow goes 100 → 1011 → 1013 → 1090.
#[test]
fn confessing_writes_the_adventure_log_and_reloads_the_state() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let dir = temp_dir("writes");
    let input = dir.join("in.sav");
    let out = dir.join("out.sav");
    let bytes = container();
    std::fs::write(&input, &bytes).unwrap();
    let _ = std::fs::remove_file(&out);

    let mut app = app_with_save(rom, Some(bytes.clone()));
    app.set_save_files(Some(input.clone()), Some(out.clone()));
    let expected = app.save_state().expect("a state to write").0;
    open_confession(&mut app);

    // Yes: the flow shows line 100 while the application writes.
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(100), "the saving line");
    assert!(out.exists(), "the adventure log was written");
    assert_eq!(
        std::fs::read(&input).unwrap(),
        bytes,
        "the --save container is never modified"
    );
    let reloaded = dq9_core::state::GameState::from_save(
        &dq9_formats::save::Save::parse(&std::fs::read(&out).unwrap()).unwrap(),
    )
    .expect("the written container reloads");
    assert_eq!(
        reloaded, expected,
        "the log reloads to the state the run held"
    );

    // 100 dismissed: 1011 says the log was recorded, a key asks 1013.
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(1011));
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(1013));
    // "Do you intend to continue?" Yes: 1090, then the visit ends and the game goes on.
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(1090));
    app.service_button(Button::Confirm);
    assert!(!app.services.is_open(), "the church closed");
    assert!(
        !app.session_over,
        "continuing the adventure does not end it"
    );
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&input);
}

/// "No" records nothing and ends the adventure: the greeting's No shows 1012, the offer to
/// continue's No shows 1015 (it opens on No), its Yes shows 1014 then 110, and the port ends
/// the session.
#[test]
fn refusing_the_log_writes_nothing_and_the_rest_ends_the_session() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let dir = temp_dir("rest");
    let input = dir.join("in.sav");
    let out = dir.join("out.sav");
    let bytes = container();
    std::fs::write(&input, &bytes).unwrap();
    let _ = std::fs::remove_file(&out);

    let mut app = app_with_save(rom, Some(bytes.clone()));
    app.set_save_files(Some(input.clone()), Some(out.clone()));
    open_confession(&mut app);
    // Greeting: No (row 1) → 1012.
    app.service_button(Button::Down);
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(1012));
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(1013));
    // "Continue your adventure?" No → 1015 (the rest without recording).
    app.service_button(Button::Down);
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(1015));
    // 1015 opens on No (the window's `param_2` is 1): Up is the Yes, ending the adventure.
    app.service_button(Button::Up);
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(1014));
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(110));
    app.service_button(Button::Confirm);
    assert!(!app.services.is_open());
    assert!(
        app.session_over,
        "110 is the original's last line; the port ends the session there"
    );
    assert!(!out.exists(), "nothing was written");
    assert_eq!(
        std::fs::read(&input).unwrap(),
        bytes,
        "the input is untouched"
    );
    let _ = std::fs::remove_file(&input);
}

/// The same flow's other branch: 1015's No (its initial row) shows 1016, which is itself the
/// question again (state 5 re-enters state 1) — and its Yes writes the log.
#[test]
fn a_refused_rest_asks_the_confession_again() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let dir = temp_dir("again");
    let input = dir.join("in.sav");
    let out = dir.join("out.sav");
    std::fs::write(&input, container()).unwrap();
    let _ = std::fs::remove_file(&out);
    let mut app = app_with_save(rom, Some(container()));
    app.set_save_files(Some(input.clone()), Some(out.clone()));
    open_confession(&mut app);
    app.service_button(Button::Down);
    app.service_button(Button::Confirm); // 1012
    app.service_button(Button::Confirm); // 1013
    app.service_button(Button::Down);
    app.service_button(Button::Confirm); // 1015
    app.service_button(Button::Confirm); // its initial row is No → 1016
    assert_eq!(app.services.confession_line(), Some(1016));
    // 1016 is the question, not a page: Yes writes the log (100), no extra key first.
    app.service_button(Button::Confirm);
    assert_eq!(app.services.confession_line(), Some(100));
    assert!(out.exists(), "1016's Yes wrote the log");
    assert!(!app.session_over);
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&input);
}

/// The whole sequence, as the ROM's state 3 walks it: recorded then continued, recorded then
/// rested, and refused without recording. The lines are printed for `confession.log`.
#[test]
fn the_confession_walks_the_roms_line_sequence() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let dir = temp_dir("sequence");
    let input = dir.join("in.sav");
    let out = dir.join("out.sav");
    let bytes = container();
    std::fs::write(&input, &bytes).unwrap();
    let _ = std::fs::remove_file(&out);

    let walk = |buttons: &[(Button, u16)], app: &mut crate::app::App| {
        open_confession(app);
        let mut seen = vec![app.services.confession_line().expect("a confession line")];
        for &(button, expected) in buttons {
            app.service_button(button);
            let line = app.services.confession_line();
            if let Some(line) = line
                && seen.last() != Some(&line)
            {
                seen.push(line);
            }
            eprintln!("confession: {button:?} -> {line:?}");
            assert_eq!(
                line.or(seen.last().copied()),
                Some(expected),
                "after {button:?}, walked {seen:?}"
            );
        }
        seen
    };

    let mut app = app_with_save(rom, Some(bytes.clone()));
    app.set_save_files(Some(input.clone()), Some(out.clone()));
    // 1010 Yes → 100 (the write) → 1011 → 1013; Yes → 1090, and the visit is over.
    let recorded = walk(
        &[
            (Button::Confirm, 100),
            (Button::Confirm, 1011),
            (Button::Confirm, 1013),
            (Button::Confirm, 1090),
        ],
        &mut app,
    );
    app.service_button(Button::Confirm);
    assert!(!app.services.is_open(), "1090 ends the visit");
    assert!(!app.session_over);
    // 1010 Yes → 100 → 1011 → 1013; No, and since the log was recorded (`0x59c`) it goes
    // straight to 1014, then 110, which ends the session.
    let rested = walk(
        &[
            (Button::Confirm, 100),
            (Button::Confirm, 1011),
            (Button::Confirm, 1013),
            (Button::Down, 1013),
            (Button::Confirm, 1014),
            (Button::Confirm, 110),
        ],
        &mut app,
    );
    assert_eq!(
        rested,
        [1010, 100, 1011, 1013, 1014, 110],
        "a recorded log skips the rest's 1015"
    );
    app.service_button(Button::Confirm);
    assert!(app.session_over);

    // Nothing recorded: 1010 No → 1012 → 1013 No → 1015 (on No) → 1016 (on Yes: No is Up) →
    // 1012 → 1013 Yes → 1090.
    let mut app = app_with_save(rom, Some(bytes.clone()));
    app.set_save_files(Some(input.clone()), Some(out.clone()));
    let refused = walk(
        &[
            (Button::Down, 1010),
            (Button::Confirm, 1012),
            (Button::Confirm, 1013),
            (Button::Down, 1013),
            (Button::Confirm, 1015),
            (Button::Confirm, 1016),
            (Button::Down, 1016),
            (Button::Confirm, 1012),
            (Button::Confirm, 1013),
            (Button::Confirm, 1090),
        ],
        &mut app,
    );
    assert_eq!(
        recorded,
        [1010, 100, 1011, 1013, 1090],
        "the recorded-then-continued path"
    );
    assert_eq!(
        refused,
        [1010, 1012, 1013, 1015, 1016, 1012, 1013, 1090],
        "the refused path"
    );
    eprintln!("confession lines, recorded then continued: {recorded:?}");
    eprintln!("confession lines, recorded then rested:    {rested:?}");
    eprintln!("confession lines, refused:                 {refused:?}");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&input);
}

/// Without `--save` the port has no container: the Confession says so and writes nothing.
#[test]
fn without_a_container_the_confession_refuses() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let dir = temp_dir("refused");
    let out = dir.join("out.sav");
    let _ = std::fs::remove_file(&out);
    let mut app = app_with_save(rom, None);
    app.set_save_files(None, Some(out.clone()));
    open_confession(&mut app);
    app.service_button(Button::Confirm); // Yes: the write refuses
    assert_eq!(app.services.confession_line(), Some(100));
    app.service_button(Button::Confirm); // the reason replaces the saving line
    let reason = app.services.notice_text().expect("a refusal line");
    let reason = String::from_utf8_lossy(&reason);
    assert!(reason.contains("--save"), "{reason}");
    assert!(!out.exists(), "nothing was written");
    app.service_button(Button::Confirm);
    assert!(!app.services.is_open(), "the visit ends");
    assert!(!app.session_over, "a refusal is not the rest");
}

/// Without `--save-out` the container itself is replaced, like the DS writing its own slot.
#[test]
fn confessing_without_save_out_writes_the_container_itself() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let dir = temp_dir("container");
    let input = dir.join("dq9.sav");
    let bytes = container();
    std::fs::write(&input, &bytes).unwrap();
    let mut app = app_with_save(rom, Some(bytes.clone()));
    app.set_save_files(Some(input.clone()), None);
    let expected = app.save_state().expect("a state").0;
    open_confession(&mut app);
    app.service_button(Button::Confirm); // Yes
    let written = std::fs::read(&input).unwrap();
    assert_ne!(written, bytes, "the container was written");
    let reloaded =
        dq9_core::state::GameState::from_save(&dq9_formats::save::Save::parse(&written).unwrap())
            .expect("the container still reloads");
    assert_eq!(reloaded, expected);
    let _ = std::fs::remove_file(&input);
}

/// A save taken on the DS (`DQ9_SAVE`), confessed over in the application: the written file
/// reloads to the state the run held, and only the updated slot's modelled ranges, its
/// checksums and its mirror copy move.
#[test]
fn a_real_save_confessed_over_changes_only_the_slot_it_updates() {
    use dq9_formats::save::{Checksum, SLOT_SIZE};
    let (Some(rom), Some(path)) = (testrom::load(), std::env::var_os("DQ9_SAVE")) else {
        eprintln!("skipping: no ROM or DQ9_SAVE");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let dir = temp_dir("real");
    let input = dir.join("in.sav");
    let out = dir.join("out.sav");
    std::fs::write(&input, &bytes).unwrap();
    let _ = std::fs::remove_file(&out);
    let mut app = app_with_save(rom, Some(bytes.clone()));
    app.set_save_files(Some(input.clone()), Some(out.clone()));
    let (state, index) = app.save_state().expect("a state to write");
    open_confession(&mut app);
    app.service_button(Button::Confirm); // Yes
    assert_eq!(app.services.confession_line(), Some(100));
    let written = std::fs::read(&out).expect("the Confession wrote the log");
    let reloaded =
        dq9_core::state::GameState::from_save(&dq9_formats::save::Save::parse(&written).unwrap())
            .expect("the written save reloads");
    assert_eq!(reloaded, state, "the log reloads to the state the run held");
    assert_eq!(std::fs::read(&input).unwrap(), bytes, "--save untouched");

    // The binding's own ranges on a copy of the input, plus the three checksum words.
    let mut probe = dq9_formats::save::Save::parse(&bytes).unwrap();
    let ranges = {
        let mut slot = probe.slot_mut(index).unwrap();
        state.write_into(&mut slot).unwrap()
    };
    let checksums: Vec<std::ops::Range<usize>> = Checksum::ALL
        .iter()
        .map(|which| which.stored_at()..which.stored_at() + 4)
        .collect();
    let mut moved = 0;
    for (at, (before, after)) in bytes.iter().zip(&written).enumerate() {
        if before == after {
            continue;
        }
        moved += 1;
        let in_slot = at % SLOT_SIZE;
        let modelled = |in_slot: usize| {
            ranges
                .iter()
                .chain(&checksums)
                .any(|range| range.contains(&in_slot))
        };
        // Slot 0's ranges and checksums, and slot 1 as the mirror of slot 0's `+0x10`.
        let mirrored = at >= SLOT_SIZE && in_slot >= 0x10 && modelled(in_slot);
        assert!(
            mirrored || (at < SLOT_SIZE && modelled(in_slot)),
            "byte {in_slot} of slot {} moved outside the written ranges",
            at / SLOT_SIZE
        );
    }
    assert!(moved > 0, "the confession changed nothing");
    // The mirror is required, not merely permitted: slot 1 from `+0x10` equals slot 0's, as
    // `Save_WriteStep` state 2 writes for a normal save (deleting the mirror must fail here).
    if index == 0 {
        // `Save::mirror_normal_slot` copies slot 0's `+0x10..0x6ff4` at slot 1's `+0x10`.
        assert_eq!(
            &written[SLOT_SIZE + 0x10..SLOT_SIZE + 0x6ff4],
            &written[0x10..0x6ff4],
            "slot 1 is slot 0's mirror"
        );
    }
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&input);
}

/// X is the answer window's cancel (`-2`), which the original folds into "No" everywhere
/// (`iVar6 == -2 || iVar6 == 1`): at the greeting it shows 1012, it does not go back to the menu.
#[test]
fn cancelling_a_confession_question_answers_no() {
    let Some(rom) = testrom::load() else {
        eprintln!("skipping: DQ9_ROM missing");
        return;
    };
    let mut app = app_with_save(rom, Some(container()));
    open_confession(&mut app);
    app.service_button(Button::Cancel);
    assert_eq!(app.services.confession_line(), Some(1012));
    assert!(app.services.is_open(), "the visit goes on with 1013");
}
