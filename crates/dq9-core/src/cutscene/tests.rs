use super::*;
use crate::script::event::ActorCommand as A;

fn no_ground(_: f32, _: f32, _: f32) -> Option<f32> {
    None
}

fn actor(actor: i32, command: A) -> Event {
    Event::Actor { actor, command }
}

/// A scene with the hero declared as cast member 0 and a model-less cast member 1.
fn scene() -> Cutscene {
    let mut c = Cutscene::new();
    c.apply(&Event::DeclareCast {
        mode: 0,
        path: None,
        reference: None,
        attach: 0,
        option: None,
        character: None,
    });
    c.apply(&Event::DeclareCast {
        mode: 2,
        path: Some("chara_sub/s000.chr".into()),
        reference: None,
        attach: 1,
        option: None,
        character: None,
    });
    c
}

fn run(c: &mut Cutscene, ticks: u32) -> Vec<Signal> {
    (0..ticks).flat_map(|_| c.tick(&no_ground)).collect()
}

#[test]
fn declared_cast_binds_slot_and_actor_of_the_same_number() {
    let c = scene();
    assert_eq!(c.slot_target(0), Some(Target::Hero));
    assert_eq!(c.slot_target(1), Some(Target::Cast(1)));
    assert_eq!(c.actor_target(1), Some(Target::Cast(1)));
    assert_eq!(
        c.node(Target::Cast(1)).unwrap().model.as_deref(),
        Some("chara_sub/s000.chr")
    );
}

#[test]
fn timed_move_lasts_twice_its_duration_in_ticks() {
    let mut c = scene();
    c.apply(&actor(0, A::SetPosition([0.0, 0.0, 0.0])));
    c.apply(&actor(
        0,
        A::MoveTo {
            position: [10.0, 0.0, 0.0],
            duration: 5,
            arrival_animation: Some("stand".into()),
        },
    ));
    // The position is set and the move starts in the first tick: 5 of its 10 ticks.
    run(&mut c, 5);
    let x = c.node(Target::Hero).unwrap().pose.position.x;
    assert!((x - 5.0).abs() < 1e-4, "{x}");
    assert!(c.node(Target::Hero).unwrap().animation.is_none());
    let signals = run(&mut c, 5);
    assert_eq!(signals, [Signal::ActorIdle(0)]);
    let node = c.node(Target::Hero).unwrap();
    assert_eq!(node.pose.position.x, 10.0);
    // The arrival animation starts when the move ends.
    assert_eq!(node.animation.as_ref().unwrap().name, "stand");
}

#[test]
fn an_actor_is_busy_until_every_command_ran_and_one_report_follows() {
    let mut c = scene();
    c.apply(&actor(1, A::SetPosition([1.0, 2.0, 3.0])));
    c.apply(&actor(1, A::SetRotation([0.0, 1.0, 0.0])));
    assert_eq!(c.tick(&no_ground), [Signal::ActorIdle(1)]);
    assert!(run(&mut c, 10).is_empty());
    let pose = c.node(Target::Cast(1)).unwrap().pose;
    assert_eq!(pose.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(pose.rotation.y, 1.0);
}

#[test]
fn rotation_follows_the_requested_direction() {
    let mut c = scene();
    c.apply(&actor(0, A::SetRotation([0.0, 0.5, 0.0])));
    // Direction 0 increases the angle even when the short way is the other one.
    c.apply(&actor(
        0,
        A::RotateTo {
            rotation: [0.0, 0.25, 0.0],
            duration: 10,
            direction: 0,
        },
    ));
    run(&mut c, 10);
    let y = c.node(Target::Hero).unwrap().pose.rotation.y;
    assert!(y > 0.5 + (TAU - 0.25) / 2.0 - 0.01, "{y}");
    run(&mut c, 10);
    let y = c.node(Target::Hero).unwrap().pose.rotation.y;
    assert!((y - (0.5 + TAU - 0.25)).abs() < 1e-3, "{y}");
}

#[test]
fn turning_towards_an_actor_uses_the_seeded_poses() {
    let mut c = scene();
    c.seed(Target::Hero, Pose::default());
    c.apply(&actor(1, A::SetPosition([4.0, 0.0, 0.0])));
    run(&mut c, 1);
    c.apply(&actor(
        0,
        A::TurnTowards {
            target: 1,
            duration: 0,
            direction: None,
        },
    ));
    run(&mut c, 2);
    // +x is a quarter turn from +z.
    let y = c.node(Target::Hero).unwrap().pose.rotation.y;
    assert!((y - PI / 2.0).abs() < 1e-5, "{y}");
}

#[test]
fn a_queued_animation_waits_for_the_current_one() {
    let mut c = scene();
    c.apply(&actor(
        1,
        A::Animate {
            name: "walk".into(),
            flags: 1,
            speed: 1.0,
        },
    ));
    c.apply(&actor(
        1,
        A::QueueAnimation {
            name: "stand".into(),
            flags: 0,
            speed: 1.0,
        },
    ));
    run(&mut c, 3);
    let walk = c.node(Target::Cast(1)).unwrap().animation.clone().unwrap();
    assert_eq!(walk.name, "walk");
    c.animation_finished(Target::Cast(1), walk.serial);
    let signals = run(&mut c, 2);
    assert_eq!(signals, [Signal::ActorIdle(1)]);
    assert_eq!(
        c.node(Target::Cast(1))
            .unwrap()
            .animation
            .as_ref()
            .unwrap()
            .name,
        "stand"
    );
}

#[test]
fn a_report_for_an_old_animation_is_ignored() {
    let mut c = scene();
    c.apply(&actor(
        1,
        A::Animate {
            name: "walk".into(),
            flags: 1,
            speed: 1.0,
        },
    ));
    run(&mut c, 2);
    let old = c
        .node(Target::Cast(1))
        .unwrap()
        .animation
        .as_ref()
        .unwrap()
        .serial;
    c.apply(&actor(
        1,
        A::Animate {
            name: "run".into(),
            flags: 1,
            speed: 1.0,
        },
    ));
    run(&mut c, 2);
    c.animation_finished(Target::Cast(1), old);
    assert!(
        !c.node(Target::Cast(1))
            .unwrap()
            .animation
            .as_ref()
            .unwrap()
            .finished
    );
}

fn fade(screen: Screen, level: i32, ticks: i32, lock: Lock) -> Event {
    Event::Fade {
        screen,
        level,
        ticks,
        lock,
    }
}

#[test]
fn fades_run_in_ticks_and_a_locked_engine_ignores_plain_fades() {
    let mut c = Cutscene::new();
    c.apply(&fade(Screen::Both, -16, 16, Lock::Lock));
    run(&mut c, 8);
    assert!((c.fades().main.level + 8.0).abs() < 1e-4);
    // A plain fade does nothing while locked; the unlocking one goes through.
    c.apply(&fade(Screen::Both, 0, 0, Lock::Keep));
    assert!((c.fades().main.level + 8.0).abs() < 1e-4);
    c.apply(&fade(Screen::Main, 0, 4, Lock::Unlock));
    run(&mut c, 4);
    assert_eq!(c.fades().main.level, 0.0);
    assert!(!c.fades().main.locked);
    // The other engine is still locked and still at its level.
    assert!(c.fades().sub.locked);
    assert!((c.fades().sub.level + 12.0).abs() < 1e-4);
}

#[test]
fn restart_keeps_the_brightness_and_forgets_the_scene() {
    let mut c = scene();
    c.apply(&fade(Screen::Both, -16, 0, Lock::Keep));
    c.restart();
    assert_eq!(c.fades().main.level, -16.0);
    assert_eq!(c.slot_target(0), None);
    c.clear_fades();
    assert_eq!(c.fades().main.level, 0.0);
}

#[test]
fn orbit_camera_places_the_eye_around_the_target() {
    let mut c = Cutscene::new();
    c.apply(&Event::Camera(CameraCommand::SetTarget([1.0, 2.0, 3.0])));
    c.apply(&Event::Camera(CameraCommand::SetOrbit {
        azimuth: 0.0,
        height: 3.0,
        distance: 5.0,
    }));
    assert_eq!(c.tick(&no_ground), [Signal::CameraIdle]);
    let view = c.camera();
    // Distance 5 with height 3: 4 units away horizontally, towards +z at azimuth 0.
    assert!(
        (view.eye - Vec3::new(1.0, 5.0, 7.0)).length() < 1e-5,
        "{:?}",
        view.eye
    );
    assert_eq!(view.target, Vec3::new(1.0, 2.0, 3.0));
}

#[test]
fn camera_moves_end_with_a_report_and_follow_an_actor() {
    let mut c = scene();
    c.apply(&Event::Camera(CameraCommand::SetPosition([0.0, 0.0, 0.0])));
    c.apply(&Event::Camera(CameraCommand::MoveBoth {
        position: [8.0, 0.0, 0.0],
        target: [0.0, 0.0, 8.0],
        duration: 4,
        snap: true,
    }));
    let signals = run(&mut c, 9);
    assert_eq!(signals, [Signal::CameraIdle]);
    let view = c.camera();
    assert_eq!(view.eye, Vec3::new(8.0, 0.0, 0.0));
    assert_eq!(view.target, Vec3::new(0.0, 0.0, 8.0));
    c.apply(&actor(1, A::SetPosition([5.0, 1.0, 5.0])));
    c.apply(&Event::Camera(CameraCommand::FollowActor {
        actor: 1,
        offset: [0.0, 2.0, 0.0],
    }));
    run(&mut c, 2);
    assert_eq!(c.camera().target, Vec3::new(5.0, 3.0, 5.0));
}

#[test]
fn the_bone_camera_is_saved_and_restored() {
    let mut c = Cutscene::new();
    c.apply(&Event::Camera(CameraCommand::SetPosition([1.0, 1.0, 1.0])));
    run(&mut c, 2);
    c.apply(&Event::Camera(CameraCommand::BoneCamera {
        slot: 3,
        eye: "eye".into(),
        target: "lookat".into(),
    }));
    assert_eq!(c.camera().bones.as_ref().unwrap().slot, 3);
    c.apply(&Event::Camera(CameraCommand::RestoreSaved));
    let view = c.camera();
    assert!(view.bones.is_none());
    assert_eq!(view.eye, Vec3::ONE);
}

#[test]
fn bound_models_and_visibility_follow_the_natives() {
    let mut c = Cutscene::new();
    c.apply(&Event::LoadModel {
        path: "chara_sub/s070.chr".into(),
        character: -11,
        selector: 1,
    });
    c.apply(&Event::CloneModel {
        source: -11,
        destination: -12,
    });
    c.apply(&Event::BindSlot {
        slot: 11,
        character: -11,
        kind: 1,
    });
    c.apply(&Event::LoadModelArchive {
        path: "event_lv5/ev01000s070.chr".into(),
        character: -11,
        tag: 3,
    });
    assert_eq!(c.model_of(-12), Some("chara_sub/s070.chr"));
    assert_eq!(c.slot_target(11), Some(Target::Model(-11)));
    assert_eq!(c.node(Target::Model(-11)).unwrap().packages.len(), 1);
    c.apply(&Event::SlotVisible {
        slot: 11,
        visible: false,
    });
    assert!(!c.node(Target::Model(-11)).unwrap().visible);
    c.apply(&Event::ReleaseModel { character: -11 });
    assert_eq!(c.node(Target::Model(-11)).unwrap().model, None);
}

#[test]
fn scripts_polling_see_the_animation_and_pose_of_their_actors() {
    let mut c = scene();
    c.apply(&actor(1, A::SetPosition([1.0, 2.0, 3.0])));
    c.apply(&actor(
        1,
        A::Animate {
            name: "walk".into(),
            flags: 1,
            speed: 1.0,
        },
    ));
    run(&mut c, 2);
    let mut runtime = Runtime::default();
    c.sync_runtime(&mut runtime);
    assert!(runtime.animating[1]);
    assert_eq!(runtime.pose[1].0, [1.0, 2.0, 3.0]);
}

#[test]
fn a_path_walks_through_its_points() {
    let mut c = scene();
    c.apply(&actor(1, A::PathReset));
    c.apply(&actor(1, A::PathSpeed(2)));
    for p in [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 0.0, 4.0]] {
        c.apply(&actor(1, A::PathPoint(p)));
    }
    c.apply(&actor(1, A::PathRun));
    let signals = run(&mut c, 40);
    assert_eq!(signals, [Signal::ActorIdle(1)]);
    let pose = c.node(Target::Cast(1)).unwrap().pose;
    assert!((pose.position - Vec3::new(4.0, 0.0, 4.0)).length() < 1e-4);
}
