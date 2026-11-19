//! The fight in play: battle outcomes, the stand-in, wipe-outs, generators and the party's
//! strength (real-ROM regressions).

use crate::app::App;
use crate::app::fight;
use crate::app::test_support::{
    a_winning_party, entering_at_the_end, fighting, map_options, roaming_options,
};
use crate::battle::fight::Pad;
use crate::talk::Button;
use dq9_core::battle::BattleResult;
use dq9_core::scenario::Step;
use glam::Vec3;

/// The journal's battle step for scripted battle `number`.
fn battle_step(app: &App, number: u16) -> Option<&Step> {
    app.journal
        .steps
        .iter()
        .map(|(_, step)| step)
        .find(|step| matches!(step, Step::Battle { number: n, .. } if *n == number))
}

/// `--fight`: scripted battle 26 of a new game is played by the engine instead of declared
/// won. It pays the party (`Party::award`, the gold), keeps the opening's monsters and says
/// the game's lines, and the story chains on as after the stand-in.
#[test]
fn battle_26_is_fought_from_a_new_game_and_pays_the_party() {
    let Some(rom) = testrom::load() else { return };
    let mut options = fighting(map_options("1100", (1, 1, 1, true)), 7);
    options.map = None;
    options.new_game = true;
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    assert_eq!(app.party().gold, 0);
    for frame in 0..40_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        if battle_step(&app, 26).is_some() {
            break;
        }
    }
    let Some(Step::Battle {
        won,
        fought: Some(fought),
        ..
    }) = battle_step(&app, 26).cloned()
    else {
        panic!("battle 26 must be fought: {:?}", battle_step(&app, 26));
    };
    assert!(won, "seed 7 wins battle 26");
    assert_eq!(fought.result, "Won");
    assert!(fought.experience > 0 && fought.gold > 0, "{fought:?}");
    // The party shows the award: one member takes the whole experience.
    let hero = &app.party().members[0];
    assert_eq!(hero.current_experience(), fought.experience);
    assert_eq!(app.party().gold, fought.gold);
    assert!(hero.hp > 0 && hero.hp <= hero.max_hp);
    let last = app.last_fight().expect("the fight is summarised");
    assert_eq!(last.number, Some(26));
    assert_eq!(
        &last.log[..2],
        ["Some slimes draw near!", "A cruelcumber draws near!"]
    );
    assert!(
        last.log.iter().any(|line| line.contains("gold coins!")),
        "{:?}",
        last.log
    );
    // The same scenes follow the battle as after the stand-in.
    assert!(!app.battle_is_active());
    assert!(!app.events.is_active() || app.events.pending_start().is_some());
}

/// A level-1 hero loses the Hexagoon (battle 2, a set battle): trigger event 16 of its room
/// runs (its action sets track flag 4), the fallen hero is raised, the purse is halved and
/// the party comes round in Angel Falls' church (map 1106, `GameState+0x5698` after a new
/// game).
#[test]
fn a_lost_hexagoon_fires_trigger_16_and_the_party_wakes_in_the_church() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(
        fighting(map_options("7105", (2, 4, 1, true)), 3),
        Some(rom.to_vec()),
    );
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(7105));
    app.party.gold = 101;
    let flags_before = app.state.tracks[app.state.track].flags.clone();
    app.run_battle(2);
    let Some(Step::Battle {
        won,
        fought: Some(fought),
        ..
    }) = battle_step(&app, 2).cloned()
    else {
        panic!("battle 2 must be fought: {:?}", battle_step(&app, 2));
    };
    assert!(!won);
    assert_eq!(fought.result, "Lost");
    assert_eq!((fought.experience, fought.gold), (0, 0));
    let flags = &app.state.tracks[app.state.track].flags;
    assert_ne!(flags, &flags_before, "trigger 16 ran: {flags:?}");
    assert!(flags.iter().any(|set| set.contains(&4)), "{flags:?}");
    assert_eq!(app.party().gold, 50, "the purse is halved, rounding down");
    let hero = &app.party().members[0];
    assert_eq!(hero.hp, hero.max_hp, "the fallen hero gets up");
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(1106));
    assert!(
        app.journal.steps.iter().any(
            |(_, s)| matches!(s, Step::MapChange { to: 1106, cause, .. } if cause == "wipe-out")
        )
    );
    // `chur_messet.bin` gives map 1106 voice 1 from story major 2 (`str_ch0`), and the church
    // in mode 2 says line 1082 before the party gets control.
    assert!(
        app.services.is_open(),
        "the priest waits on the revival map"
    );
    let line = app
        .services
        .priest_line()
        .expect("voice 1 says line 1082 of str_ch0");
    let line = String::from_utf8_lossy(line);
    assert!(line.contains("O Almighty One"), "{line}");
    assert!(line.contains("Amen!"), "{line}");
    // A key ends the visit: control comes back with the halved purse.
    app.service_button(Button::Confirm);
    assert!(!app.services.is_open(), "the priest's line is over");
    assert!(!app.talk.is_active() && !app.battle_is_active());
    assert_eq!(app.party().gold, 50, "the halved purse stands");
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(1106));
    app.run_steps(1);
    assert!(
        app.hero.is_some(),
        "the party has control on the church map"
    );
}

/// The same fall at story major 1: `chur_messet.bin` gives 1106 **voice 3**, which says
/// nothing, so no window opens and the party simply has control in the church.
#[test]
fn a_wipe_out_at_story_major_one_has_no_priest_line() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(
        fighting(map_options("7105", (1, 4, 1, true)), 3),
        Some(rom.to_vec()),
    );
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(7105));
    app.party.gold = 101;
    app.run_battle(2);
    assert_eq!(app.party().gold, 50, "the purse is halved all the same");
    assert_eq!(app.current.as_ref().map(|(e, _)| e.id), Some(1106));
    assert!(!app.services.is_open(), "voice 3 says nothing: no window");
    assert_eq!(app.services.priest_line(), None);
}

/// `start_encounter`, the roaming monsters' entry: an encounter has no battle number, so no
/// trigger fires and no step is journaled; the party is paid and the field is back.
#[test]
fn an_encounter_pays_the_party_and_fires_no_trigger() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(
        fighting(map_options("1100", (1, 2, 1, true)), 5),
        Some(rom.to_vec()),
    );
    let map = app.current.as_ref().map(|(e, _)| e.id);
    app.start_encounter(vec![(1, 2)], 0, dq9_core::battle::Surprise::None);
    let last = app.last_fight().expect("the encounter was fought").clone();
    assert_eq!(last.result, BattleResult::Won);
    assert_eq!((last.experience, last.gold), (4, 8), "two slimes");
    assert_eq!(app.party().gold, 8);
    assert!(
        !app.journal
            .steps
            .iter()
            .any(|(_, s)| matches!(s, Step::Battle { .. })),
        "no battle number, nothing to report to the triggers"
    );
    assert_eq!(
        app.current.as_ref().map(|(e, _)| e.id),
        map,
        "the field is back"
    );
    assert!(!app.battle_is_active());
}

/// Without `--fight` the stand-in still settles a headless battle at once and leaves the party
/// as it was: the recorded traces replay as before.
#[test]
fn the_stand_in_leaves_the_party_untouched() {
    let Some(rom) = testrom::load() else { return };
    let mut options = map_options("1100", (1, 2, 1, true));
    options.map = None;
    options.new_game = true;
    let mut app = App::new_headless(options, Some(rom.to_vec()));
    for frame in 0..40_000 {
        if frame % 20 == 0 {
            app.talk.press(Button::Confirm);
        }
        app.run_steps(1);
        if battle_step(&app, 26).is_some() {
            break;
        }
    }
    assert!(matches!(
        battle_step(&app, 26),
        Some(Step::Battle {
            won: true,
            fought: None,
            ..
        })
    ));
    assert_eq!(app.party().gold, 0);
    assert_eq!(app.party().members[0].current_experience(), 0);
    assert!(app.last_fight().is_none());
}

/// A battle preview (`--battle`, `?battle=`) is fought once: closing its reward windows must
/// not start it again (the Web defect observed after a won battle 26).
#[test]
fn a_battle_preview_is_fought_once_and_not_restarted() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("1100"), Some(rom.to_vec()));
    let request = crate::battle::Request {
        record: None,
        stage: 0,
        monsters: vec![(1, 2)],
        track: None,
        surprise: Some(dq9_core::battle::Surprise::None),
    };
    app.options.battle = Some(request.clone());
    // The preview has no field behind it, as `set_rom` sets it up.
    app.save_field();
    let resolved = request.resolve(rom).unwrap();
    let expected_stage = resolved.stage;
    app.encounter.battle_change = Some(entering_at_the_end(resolved));
    let mut fought = false;
    for _ in 0..20_000 {
        if !app.battle_is_active() {
            fought = true;
            break;
        }
        app.combat.battle_pad.confirm = true;
        app.run_steps(1);
    }
    assert!(fought, "the preview must end");
    let last = app.last_fight().expect("the preview was played").clone();
    assert!(!app.battle_is_active());
    assert_eq!(
        app.battle().map(|scene| scene.stage()),
        Some(expected_stage)
    );
    assert!(app.combat.fight.is_none());
    let gold = app.party().gold;
    for _ in 0..2_000 {
        app.run_steps(1);
    }
    assert_eq!(app.party().gold, gold, "the preview must not pay twice");
    assert!(
        !app.battle_is_active(),
        "the preview must not start again: {:?}",
        app.last_fight().map(|fight| fight.gold)
    );
    assert_eq!(app.last_fight().map(|fight| fight.gold), Some(last.gold));
    assert_eq!(
        app.battle().map(|scene| scene.stage()),
        Some(expected_stage),
        "the framed stage survives after the preview ends"
    );
}

/// Esc cancels in a fight (the pad owns it); the field's Esc is not touched.
#[test]
fn escape_is_cancel_in_a_fight() {
    let mut pad = Pad::default();
    fight::pad_press(&mut pad, winit::keyboard::KeyCode::Escape);
    assert!(pad.cancel && !pad.confirm);
}

/// A Hexagon floor with `encfld`/spawn graph loads the field models of its zones (`D01M02` to
/// `D01M04`); the map with no group loads none.
#[test]
fn hexagon_encounter_floors_load_their_field_models() {
    let Some(rom) = testrom::load() else { return };
    for (map, id) in [("D01M02", 7102), ("D01M03", 7103), ("D01M04", 7104)] {
        let mut app = App::new_headless(roaming_options(map), Some(rom.to_vec()));
        a_winning_party(&mut app);
        app.set_roaming_enabled(true);
        assert_eq!(app.roaming_map(), Some(id));
        assert!(
            app.roaming_model_count() > 0,
            "{map} must load its roamer field models"
        );
        let mut spawned = false;
        for _ in 0..2_000 {
            app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
            app.run_steps(1);
            if !app.roaming_roamers().is_empty() {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "{map} must spawn a roamer");
        let (slot, roamer) = app.roaming_roamers().into_iter().next().unwrap();
        assert!(
            app.roaming_slot_world(slot)
                .is_some_and(|world| world.x_axis.length() > 0.0),
            "{map}: the {:?} model is drawn",
            roamer.number
        );
    }
}

/// The leader's attack plus defence comes from `Character::fighter`, so worn equipment changes
/// the number the notice decision reads (not the level table alone).
#[test]
fn equipment_is_counted_in_the_party_strength() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    app.set_roaming_enabled(true);
    app.party.members[0].stats.strength = 10;
    app.party.members[0].stats.resilience = 10;
    app.party.members[0].hp = 100;
    app.party.members[0].max_hp = 100;
    app.party.members[0].equipment = Default::default();
    app.party_ready = true;
    let unarmed = app.roaming_party_strength();
    app.party.members[0].equipment.weapon = Some(dq9_core::state::NEW_GAME_WEAPON);
    let armed = app.roaming_party_strength();
    assert!(
        armed.0 > unarmed.0,
        "the copper sword adds attack: {unarmed:?} -> {armed:?}"
    );
    // A dead member is neither the leader's sum nor a deftness source.
    app.party.members[0].hp = 0;
    assert_eq!(app.roaming_party_strength(), (0, 0));
}

/// The scripted companies the `encbtl` roamer words name are read from `eventbattle.bin`, not
/// dropped: every found preset equals its record's monsters and counts.
#[test]
fn scripted_companies_come_from_eventbattle() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    let presets = app.roaming_presets();
    assert!(!presets.is_empty(), "the encbtl tables name presets");
    let parsed = nds_rom::Rom::parse(rom).unwrap();
    let mut found = 0;
    for preset in presets {
        let Some(company) = app.roaming_event_company(preset) else {
            continue;
        };
        let record = dq9_core::scenario::battle(&parsed, u16::from(preset))
            .unwrap_or_else(|| panic!("preset {preset} has no eventbattle record"));
        let mut expected = [(0u16, 0i16); 3];
        for (slot, &(monster, count)) in record.monsters.iter().take(3).enumerate() {
            expected[slot] = (monster as u16, count as i16);
        }
        assert_eq!(company, expected, "preset {preset}");
        found += 1;
    }
    assert!(found > 0, "at least one encbtl preset names a battle");
}

/// The two-way generator transfer: a battle's draws replace the field's stream (a scripted
/// battle has no contact pending), and the next field step hands the same state back, so no
/// draw is replayed and the state is not a constant copy.
#[test]
fn battle_generators_continue_the_field_stream() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
    app.run_steps(2);
    let before = app.roaming_world().expect("roamers loaded");
    // What a battle does to the world's generators.
    let mut advanced = before.clone();
    advanced.crand.rand();
    advanced.random.next_u32();
    app.combat.world = advanced.clone();
    // A scripted battle settles with no contacted roamer; the field must adopt the stream.
    app.settle_roaming(None);
    let adopted = app.roaming_world().expect("roamers loaded");
    assert_eq!(
        (adopted.crand.clone(), adopted.random.clone()),
        (advanced.crand.clone(), advanced.random.clone())
    );
    assert_ne!(
        (adopted.crand.clone(), adopted.random.clone()),
        (before.crand, before.random),
        "the transfer is a real advance, not a copy of the pre-battle state"
    );
    // The next field step exports the same stream again (field -> world).
    app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
    app.run_steps(2);
    let field = app.roaming_world().expect("roamers loaded");
    assert_eq!(
        (field.crand.clone(), field.random.clone()),
        (
            app.combat.world.crand.clone(),
            app.combat.world.random.clone()
        ),
        "the world and the field share one live stream"
    );
}

/// A map change seeds the new map's roamers from the live stream (the world the last field
/// step exported), not from a fresh seed: no pre-change draw is replayed.
#[test]
fn a_map_change_continues_the_same_generator_stream() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
    app.run_steps(2);
    // A battle (or a script) advances the world; the field adopts it, then the map changes.
    app.combat.world.crand.rand();
    app.combat.world.random.next_u32();
    let advanced = app.combat.world.clone();
    app.settle_roaming(None);
    assert!(app.load("D01M02", true, None, None), "D01M02 loads");
    let next = app.roaming_world().expect("the new map has roamers");
    assert_eq!(
        (next.crand.clone(), next.random.clone()),
        (advanced.crand, advanced.random),
        "the new map's roamers start on the live stream"
    );
}
