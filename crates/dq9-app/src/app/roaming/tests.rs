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
    let mut contacted: Option<(usize, u16, usize)> = None;
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
        // The touch picks the first eligible slot, so the contacted roamer is the one the
        // settle takes out of the list, whichever the loop aimed at.
        let before: Vec<(usize, u16, Option<usize>)> = app
            .roaming_roamers()
            .into_iter()
            .map(|(slot, roamer)| (slot, roamer.id, app.roaming_slot_instance(slot)))
            .collect();
        app.run_steps(1);
        if app.roaming_contacts() > 0 {
            let after: Vec<u16> = app
                .roaming_roamers()
                .into_iter()
                .map(|(_, roamer)| roamer.id)
                .collect();
            contacted = before
                .into_iter()
                .find(|(_, id, _)| !after.contains(id))
                .map(|(slot, id, instance)| {
                    (
                        slot,
                        id,
                        instance.expect("the contacted roamer had a field model"),
                    )
                });
            break;
        }
    }
    let (_contact_slot, contact_id, contact_instance) =
        contacted.expect("the won monster leaves its slot at the battle's exit, not after a fade");
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
    // The battle's exit resets and hides a won monster's object (`ov_000::021688dc`), so the
    // roamer is already out of its slot when the fight settles, and the next field update's
    // `sync` releases its model; there is no state 11 and no fade.
    assert!(
        !app.roaming_roamers()
            .iter()
            .any(|(_, roamer)| roamer.id == contact_id),
        "the won monster left its slot at the battle's exit"
    );
    // The field runs every other fixed step: the next one syncs the released slot before its
    // spawn timer can give it another roamer.
    app.run_steps(2);
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

/// A flight leaves the monster's object untouched (`ov_000::021688dc` only resets the one the
/// party beat): it stands where it stood in `FOUGHT` (state 9, which the contact check skips)
/// and its model stays drawn; there is no startled resumption.
#[test]
fn a_flight_leaves_the_roamer_standing() {
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
        .expect("a flight leaves the roamer on the field");
    assert_eq!(roamer.id, id);
    assert_eq!(roamer.state, dq9_core::encounter::roamer::state::FOUGHT);
    assert_eq!(roamer.position, found.position, "it stays where it stood");
    let instance = app
        .roaming_slot_instance(slot)
        .expect("the fled roamer keeps its model");
    for _ in 0..120 {
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
    let still = app
        .roaming_roamers()
        .into_iter()
        .find(|(_, r)| r.id == id)
        .map(|(_, roamer)| roamer)
        .expect("no resumption removes it");
    assert_eq!(
        still.state,
        dq9_core::encounter::roamer::state::FOUGHT,
        "no STARTLED"
    );
    assert_eq!(still.position, found.position, "state 9 stands still");
    assert_eq!(app.roaming_contacts(), 0, "state 9 is skipped by the touch");
}

/// A wiped party's battle also ends with `ov_000::02163c80` at 0 (only the party's escape keeps
/// the monster), so the exit resets and hides it: the slot is released like a win.
#[test]
fn a_lost_fight_leaves_no_roamer() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    let (slot, found) = wait_for_roamer(&mut app, 2_000).expect("F01 spawns");
    app.roaming.set_pending(slot);
    app.settle_roaming(Some(BattleResult::Lost));
    assert!(
        app.roaming_roamers()
            .into_iter()
            .all(|(_, roamer)| roamer.id != found.id),
        "the lost fight's monster is hidden by the battle's exit"
    );
}

/// The original draws a roamer at its own heading: `Object3D::PopulateRenderConfigWorld`
/// (`0x0203575c`) builds `RotY(+0x54)`, and takes the camera's matrix only under flag `0x80000` of
/// `+0x6c`, which is clear on every live roamer of the original RAM images
/// (`docs/app/rendering.md` § Field monsters). The model's world matrix is `T(position) ·
/// RotY(heading) · S(scale)` whatever the camera's yaw, and the camera turning does not turn the
/// model: a change that made roamers face the eye would show here.
#[test]
fn a_roamers_model_keeps_its_own_heading_whatever_the_camera_yaw() {
    let Some(rom) = testrom::load() else { return };
    let mut app = App::new_headless(roaming_options("F01"), Some(rom.to_vec()));
    a_winning_party(&mut app);
    app.set_roaming_enabled(true);
    let (slot, spawned) = wait_for_roamer(&mut app, 2_000).expect("F01 must spawn a roamer");
    assert_ne!(spawned.angle, 0, "a spawn faces the party, not yaw 0");
    let mut relative_to_the_eye = Vec::new();
    for k in 0..8 {
        let yaw = k as f32 * std::f32::consts::TAU / 8.0;
        app.view.camera.yaw = yaw;
        app.run_steps(2);
        let turned = (app.view.camera.yaw - yaw).abs();
        assert!(
            turned.min(std::f32::consts::TAU - turned) < 1e-3,
            "the camera keeps the yaw the test gave it: {} against {yaw}",
            app.view.camera.yaw
        );
        let roamer = app
            .roaming_roamers()
            .into_iter()
            .find(|(now, _)| *now == slot)
            .map(|(_, roamer)| roamer)
            .expect("the roamer stays on the field");
        let world = app.roaming_slot_world(slot).expect("it has a model");
        let position = Vec3::new(
            roamer.position[0] as f32 / 4096.0,
            roamer.position[1] as f32 / 4096.0,
            roamer.position[2] as f32 / 4096.0,
        );
        let own = glam::Mat4::from_translation(position)
            * glam::Mat4::from_rotation_y(roamer.angle as f32 / 4096.0)
            * glam::Mat4::from_scale(Vec3::splat(roamer.scale as f32 / 4096.0));
        assert!(
            world.abs_diff_eq(own, 1e-4),
            "camera yaw step {k}: the model is T · RotY(heading) · S, not turned: {world:?} against {own:?}"
        );
        // The model's facing seen from the camera's side of the ground plane.
        let facing = world.z_axis.truncate().normalize();
        let to_eye = Vec3::new(-yaw.sin(), 0.0, yaw.cos());
        relative_to_the_eye.push(facing.cross(to_eye).y.atan2(facing.dot(to_eye)));
    }
    // A model that turned to the eye would hold one angle to it; this one's moves with the
    // camera, a step of 45° each time.
    for pair in relative_to_the_eye.windows(2) {
        let step = (pair[1] - pair[0]).rem_euclid(std::f32::consts::TAU);
        let step = step.min(std::f32::consts::TAU - step);
        assert!(
            (step - std::f32::consts::FRAC_PI_4).abs() < 0.05,
            "the camera turned 45°, the model did not: {relative_to_the_eye:?}"
        );
    }
}
