//! Roaming monsters on the field: spawning, contact, flight (real-ROM regressions).

use crate::app::App;
use crate::app::test_support::{a_winning_party, roaming_options};
use dq9_core::Hero;
use dq9_core::battle::BattleResult;
use glam::Vec3;

/// Walks the pilot in one direction for `steps` single fixed steps (the field task runs on
/// every other one), so the roamers see a stepped party.
fn walk_steps(app: &mut App, direction: Vec3, steps: usize) {
    for _ in 0..steps {
        app.pilot_dir = Some(direction);
        app.run_steps(1);
    }
    app.pilot_dir = None;
}

/// Waits (walking) for the first roamer, in `steps` single fixed steps.
fn wait_for_roamer(
    app: &mut App,
    steps: usize,
) -> Option<(usize, dq9_core::encounter::roamer::Roamer)> {
    for _ in 0..steps {
        app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
        app.run_steps(1);
        if let Some(found) = app.roaming_roamers().into_iter().next() {
            return Some(found);
        }
    }
    None
}

/// Natural spawn on F01: the map has encfld zones and a spawn graph, its monsters get field
/// models at load, and a roamer appears with a drawn model after the party steps.
#[test]
fn f01_spawns_a_visible_roamer_after_the_party_steps() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    assert_eq!(app.roaming_map(), Some(20001));
    // F01's three zones roam six monsters; every one has its `<code>_f.mon` field model.
    assert_eq!(app.roaming_model_count(), 6);
    let (slot, roamer) = wait_for_roamer(&mut app, 2_000).expect("F01 must spawn a roamer");
    assert_ne!(roamer.number, 0);
    assert!(
        roamer.position.iter().any(|coordinate| *coordinate != 0),
        "a real spawn point, not the origin: {:?}",
        roamer.position
    );
    let world = app
        .roaming_slot_world(slot)
        .expect("the roamer has a field model");
    assert!(world.x_axis.length() > 0.0, "the model is drawn: {world:?}");
    let at = world.w_axis;
    assert!(
        (at.x - roamer.position[0] as f32 / 4096.0).abs() < 0.01
            && (at.z - roamer.position[2] as f32 / 4096.0).abs() < 0.01,
        "the model stands at the roamer's position: {at:?} against {:?}",
        roamer.position
    );
}

#[test]
fn a_dialogue_pauses_roamers_until_it_closes() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    let (slot, first) = wait_for_roamer(&mut app, 2_000).expect("F01 spawns a roamer");
    app.pilot_dir = None;
    let before = app.roaming_roamers();
    let generators = (
        app.combat.world.crand.clone(),
        app.combat.world.random.clone(),
    );
    app.talk.show_event(b"A conversation.").unwrap();
    app.run_steps(120);
    assert!(app.talk.is_active());
    assert_eq!(
        app.roaming_roamers(),
        before,
        "no movement, spawn or contact while reading"
    );
    assert_eq!(
        (
            app.combat.world.crand.clone(),
            app.combat.world.random.clone()
        ),
        generators
    );
    assert_eq!(app.roaming_contacts(), 0);
    app.talk.close_event();
    app.run_steps(2);
    let (_, resumed) = app
        .roaming_roamers()
        .into_iter()
        .find(|(index, _)| *index == slot)
        .expect("the paused monster resumes in its slot");
    assert_eq!(resumed.id, first.id);
    assert!(
        resumed.state_ms > first.state_ms,
        "closing the dialogue resumes the field"
    );
}

/// Contact -> the existing encounter -> a played battle -> the same field, the defeated
/// roamer gone, the party paid once.
#[test]
fn a_contact_fights_on_the_same_field_and_removes_its_roamer_once() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    // Let the roamer leave its appear state (touches skips state 0).
    let mut target = None;
    for _ in 0..2_000 {
        app.pilot_dir = Some(Vec3::new(1.0, 0.0, 0.0));
        app.run_steps(1);
        target = app
            .roaming_roamers()
            .into_iter()
            .find(|(_, roamer)| roamer.state != dq9_core::encounter::roamer::state::APPEAR);
        if target.is_some() {
            break;
        }
    }
    let target_slot = target
        .map(|(slot, _)| slot)
        .expect("a roamer must be past its appear state");
    let map = app.current.as_ref().map(|(entry, _)| entry.id);
    let gold = app.party().gold;
    let mut fought = false;
    for _ in 0..8 {
        // Stand on it: the touch is `touches`, the very function the original tick uses.
        let Some(current) = app
            .roaming_roamers()
            .into_iter()
            .find(|(slot, _)| *slot == target_slot)
            .map(|(_, roamer)| roamer)
            .or_else(|| app.roaming_roamers().into_iter().next().map(|(_, r)| r))
        else {
            continue;
        };
        app.hero = Some(Hero::at(
            Vec3::new(
                current.position[0] as f32 / 4096.0 + 0.15,
                current.position[1] as f32 / 4096.0,
                current.position[2] as f32 / 4096.0,
            ),
            0.0,
        ));
        app.run_steps(1);
        if app.roaming_contacts() > 0 {
            fought = true;
            break;
        }
    }
    assert!(fought, "the hero must touch the roamer");
    assert_eq!(app.roaming_contacts(), 1);
    assert!(
        !app.battle_is_active(),
        "the encounter settled in the same step"
    );
    let last = app.last_fight().expect("the encounter was fought");
    assert_eq!(last.result, BattleResult::Won);
    assert!(
        last.gold > 0 && last.experience > 0,
        "the win pays: {last:?}"
    );
    assert_eq!(
        app.party().gold,
        last.gold,
        "the award went to the party once"
    );
    assert!(last.gold >= gold, "an encounter pays, it does not reset");
    assert_eq!(
        app.current.as_ref().map(|(entry, _)| entry.id),
        map,
        "the field is back, on the same map"
    );
    // The won monster is in its fade (state 11) when the battle settles; the field's next
    // steps finish it and release the model. The touch picks the first eligible slot, which
    // need not be the one the test stood on.
    let (contact_slot, contact_id) = {
        let (slot, roamer) = app
            .roaming_roamers()
            .into_iter()
            .find(|(_, roamer)| roamer.state == dq9_core::encounter::roamer::state::DEFEATED)
            .expect("the contacted roamer fades first");
        (slot, roamer.id)
    };
    let contact_instance = app
        .roaming_slot_instance(contact_slot)
        .expect("the contacted roamer has a field model");
    // The slot's spawn timer may already be near its second, so wait for the fade to end and
    // check the released model on the first step the old id is gone, before the slot can be
    // given another roamer.
    for _ in 0..40 {
        app.run_steps(1);
        if !app
            .roaming_roamers()
            .iter()
            .any(|(_, roamer)| roamer.id == contact_id)
        {
            break;
        }
    }
    assert!(
        !app.roaming_roamers()
            .iter()
            .any(|(_, roamer)| roamer.id == contact_id),
        "the defeated roamer left its slot after the fade"
    );
    let released = app
        .runtime
        .as_ref()
        .expect("the field is loaded")
        .scene
        .instances[contact_instance]
        .world;
    assert_eq!(
        released.x_axis.length(),
        0.0,
        "the released field model is hidden: {released:?}"
    );
    // Settled once: more steps do not fight it again or pay twice.
    let gold = app.party().gold;
    for _ in 0..120 {
        app.run_steps(1);
    }
    assert_eq!(app.roaming_contacts(), 1);
    assert_eq!(app.party().gold, gold, "no second award");
    assert!(
        !app.roaming_roamers()
            .iter()
            .any(|(_, roamer)| roamer.id == contact_id),
        "no respawn is the defeated roamer"
    );
}

/// Maps with no `encfld` group load no roamer and no field model: `S01M01`, `D01M01` and
/// `D01M05` are the maps the encounter note names.
#[test]
fn encounter_free_maps_spawn_no_roamers() {
    let Some(rom) = testrom::load() else { return };
    for (map, id) in [("S01M01", 5101), ("D01M01", 7101), ("D01M05", 7105)] {
        let mut app = App::new_headless(roaming_options(map), Some(rom.to_vec()));
        a_winning_party(&mut app);
        app.set_roaming_enabled(true);
        assert_eq!(app.roaming_map(), Some(id), "{map} has no encounter table");
        assert_eq!(app.roaming_model_count(), 0, "{map} has no field models");
        walk_steps(&mut app, Vec3::new(1.0, 0.0, 0.0), 80);
        assert!(
            app.roaming_roamers().is_empty(),
            "{map} must not spawn a roamer"
        );
        assert_eq!(app.roaming_contacts(), 0);
    }
}

/// A flight does not delete the live roamer: the settlement puts it in STARTLED, its model
/// stays drawn, and its id is recent so the party cannot touch it at once.
#[test]
fn a_flight_keeps_and_resumes_the_roamer() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    let (slot, found) = wait_for_roamer(&mut app, 2_000).expect("F01 spawns");
    let id = found.id;
    app.roaming.set_pending(slot);
    app.settle_roaming(Some(BattleResult::Fled));
    let roamer = app
        .roaming_roamers()
        .into_iter()
        .find(|(slot_now, _)| *slot_now == slot)
        .map(|(_, roamer)| roamer)
        .expect("a flight leaves the roamer alive");
    assert_eq!(roamer.id, id);
    assert_eq!(roamer.state, dq9_core::encounter::roamer::state::STARTLED);
    let instance = app
        .roaming_slot_instance(slot)
        .expect("the fled roamer keeps its model");
    for _ in 0..6 {
        app.run_steps(1);
    }
    assert_eq!(
        app.roaming_slot_instance(slot),
        Some(instance),
        "the model is still bound to the fled roamer"
    );
    assert!(
        app.runtime
            .as_ref()
            .expect("the field is loaded")
            .scene
            .instances[instance]
            .world
            .x_axis
            .length()
            > 0.0,
        "the fled roamer stays drawn"
    );
    assert!(app.roaming_roamers().iter().any(|(_, r)| r.id == id));
}
