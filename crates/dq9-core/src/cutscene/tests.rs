use super::*;
use crate::script::event::{ActorCommand as A, Camera as CameraCommand};

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

#[test]
fn loaded_characters_have_the_size_of_the_map_and_clones_share_the_packages() {
    let mut c = Cutscene::new();
    for (character, selector) in [(-1, 0), (-2, 1)] {
        c.apply(&Event::LoadModel {
            path: format!("chara_sub/s{:03}.chr", -character),
            character,
            selector,
        });
    }
    // The selector only picks the allocator: every loaded model gets 0x10a.
    for character in [-1, -2] {
        assert_eq!(
            c.node(Target::Model(character)).unwrap().pose.scale,
            Vec3::splat(CHARACTER_SCALE)
        );
    }
    c.apply(&Event::CloneModel {
        source: -2,
        destination: -3,
    });
    // The clone is sized like a character and gets what the original is given later.
    assert_eq!(
        c.node(Target::Model(-3)).unwrap().pose.scale,
        Vec3::splat(CHARACTER_SCALE)
    );
    c.apply(&Event::LoadModelArchive {
        path: "event_lv5/ev01000s002.chr".into(),
        character: -2,
        tag: 3,
    });
    assert_eq!(c.node(Target::Model(-3)).unwrap().packages.len(), 1);
    assert_eq!(c.node(Target::Model(-1)).unwrap().packages.len(), 0);
}

#[test]
fn a_camera_reset_is_reported_idle_like_any_camera_command() {
    let mut c = Cutscene::new();
    c.apply(&Event::Camera(CameraCommand::Reset));
    assert_eq!(c.tick(&no_ground), [Signal::CameraIdle]);
    assert!(run(&mut c, 3).is_empty());
}

#[test]
fn shake_lasts_its_duration_and_a_negative_one_never_ends() {
    let mut c = Cutscene::new();
    c.apply(&Event::Camera(CameraCommand::Shake {
        amplitude: [1.0, 0.0, 0.0],
        duration: 3,
        target_only: false,
    }));
    let signals = run(&mut c, 12);
    assert_eq!(signals, [Signal::CameraIdle]);
    assert_eq!(c.camera().eye, Vec3::new(0.0, 5.0, 20.0));
    c.apply(&Event::Camera(CameraCommand::Shake {
        amplitude: [1.0, 0.0, 0.0],
        duration: -1,
        target_only: false,
    }));
    assert!(run(&mut c, 600).is_empty());
}

#[test]
fn an_attached_model_hangs_on_a_bone_of_its_parent_until_detached() {
    let mut c = scene();
    c.apply(&Event::LoadModel {
        path: "chara_sub/s069.chr".into(),
        character: -16,
        selector: 0,
    });
    c.apply(&Event::BindSlot {
        slot: 16,
        character: -16,
        kind: 1,
    });
    let attach = |bone: Option<&str>| Event::AttachBone {
        child: 16,
        parent: 0,
        bone: bone.map(str::to_owned),
    };
    c.apply(&attach(Some("head")));
    let node = c.node(Target::Model(-16)).unwrap();
    assert_eq!(node.attached, Some((Target::Hero, "head".to_owned())));
    // Without a name the bone of the previous attachment is kept (`02037868`).
    c.apply(&attach(None));
    assert_eq!(
        c.node(Target::Model(-16)).unwrap().attached,
        Some((Target::Hero, "head".to_owned()))
    );
    assert!(!c.ignored().contains_key("bone attachments"));
    c.apply(&Event::DetachBone { slot: 16 });
    assert_eq!(c.node(Target::Model(-16)).unwrap().attached, None);
    // A slot nothing is bound to cannot be attached: reported as not executed.
    c.apply(&Event::AttachBone {
        child: 30,
        parent: 0,
        bone: Some("head".into()),
    });
    assert_eq!(c.ignored().get("bone attachments"), Some(&1));
}

#[test]
fn a_character_id_loaded_again_is_a_new_object() {
    let mut c = scene();
    let load = |path: &str| Event::LoadModel {
        path: path.into(),
        character: -4,
        selector: 1,
    };
    c.apply(&load("chara_sub/s058.chr"));
    c.apply(&Event::BindSlot {
        slot: 4,
        character: -4,
        kind: 1,
    });
    c.apply(&actor(4, A::SetPosition([1.0, 2.0, 3.0])));
    c.apply(&Event::CharacterVisible {
        character: -4,
        visible: false,
    });
    c.apply(&load("chara_sub/s019f07.chr"));
    let node = c.node(Target::Model(-4)).unwrap();
    assert_eq!(node.pose.position, Vec3::ZERO);
    assert!(node.visible);
    assert_eq!(node.model.as_deref(), Some("chara_sub/s019f07.chr"));
    // A monster (233) is a new object too, at the character size whatever the script set.
    c.apply(&actor(4, A::Bind { slot: 4 }));
    c.apply(&actor(
        4,
        A::SetScale {
            scale: [1.5; 3],
            duration: 0,
        },
    ));
    run(&mut c, 1);
    assert_eq!(
        c.node(Target::Model(-4)).unwrap().pose.scale,
        Vec3::splat(1.5)
    );
    c.apply(&Event::LoadMonster {
        path: "enemy/z063a.mon".into(),
        character: -4,
    });
    let node = c.node(Target::Model(-4)).unwrap();
    assert_eq!(node.pose.scale, Vec3::splat(CHARACTER_SCALE));
    assert_eq!(node.pose.position, Vec3::ZERO);
}

#[test]
fn a_script_sprite_is_an_object_the_slots_of_type_two_designate() {
    let mut c = scene();
    c.apply(&Event::CreateSprite {
        name: "n013a".into(),
        handle: 0,
    });
    c.apply(&Event::BindSlot {
        slot: 17,
        character: 0,
        kind: 2,
    });
    let node = c.node(Target::Object(0)).unwrap();
    assert_eq!(node.model.as_deref(), Some("n013a.spr"));
    // 545 and 546 start and stop its walk like those of a character.
    c.apply(&Event::SlotMotion {
        slot: 17,
        moving: true,
    });
    assert_eq!(
        c.node(Target::Object(0))
            .unwrap()
            .animation
            .as_ref()
            .map(|a| a.name.as_str()),
        Some("run")
    );
    c.apply(&Event::DestroySprite { handle: 0 });
    assert_eq!(c.node(Target::Object(0)).unwrap().model, None);
    assert!(!c.ignored().contains_key("slot motion of a map object"));
}

#[test]
fn balloons_follow_the_script_and_end_with_it() {
    let mut c = scene();
    c.apply(&Event::Balloon {
        actor: 1,
        kind: 15,
        offset: [0, -10],
    });
    assert_eq!(
        c.balloon().map(|b| (b.sprite, b.offset)),
        Some((13, [-6, -86]))
    );
    c.apply(&Event::BalloonNudge { offset: [0, -3] });
    assert_eq!(c.balloon().unwrap().offset, [-6, -89]);
    c.apply(&Event::BalloonEnd);
    assert!(c.balloon().is_none());
    // The event over takes a balloon the script forgot down too.
    c.apply(&Event::Balloon {
        actor: 1,
        kind: 2,
        offset: [0, 0],
    });
    c.clear_fades();
    assert!(c.balloon().is_none());
}

#[test]
fn placement_natives_are_queued_for_the_application() {
    let mut c = scene();
    c.apply(&Event::Placement {
        group: 0,
        id: 5,
        hidden: true,
    });
    c.apply(&Event::PlacementMove {
        group: 0,
        id: 6,
        position: [1.0, 2.0, 3.0],
    });
    assert_eq!(
        c.take_placement_changes(),
        [
            PlacementChange::Hidden {
                group: 0,
                id: 5,
                hidden: true
            },
            PlacementChange::Moved {
                group: 0,
                id: 6,
                position: [1.0, 2.0, 3.0]
            }
        ]
    );
    assert!(c.take_placement_changes().is_empty());
}

#[test]
fn a_palette_operation_stays_on_its_model_until_reset_or_released() {
    let mut c = scene();
    let tint = |rgb| Event::Tint {
        slot: 0,
        rgb,
        mode: 0,
    };
    c.apply(&tint([8, 6, 16]));
    let op = c.node(Target::Hero).unwrap().tint.unwrap();
    assert_eq!((op.mode, op.rgb), (0, [8, 6, 16]));
    // Adding black restores the model (the scripts' reset).
    c.apply(&tint([0, 0, 0]));
    assert!(c.node(Target::Hero).unwrap().tint.is_none());
    c.apply(&Event::LoadModel {
        path: "chara_sub/s019.chr".into(),
        character: -3,
        selector: 0,
    });
    c.apply(&Event::BindSlot {
        slot: 3,
        character: -3,
        kind: 1,
    });
    c.apply(&Event::Tint {
        slot: 3,
        rgb: [8, 6, 16],
        mode: 2,
    });
    assert!(c.node(Target::Model(-3)).unwrap().tint.is_some());
    c.apply(&Event::ReleaseModel { character: -3 });
    assert!(c.node(Target::Model(-3)).unwrap().tint.is_none());
}

#[test]
fn the_camera_goes_back_to_the_field_pose_around_the_hero() {
    let mut c = scene();
    c.seed(
        Target::Hero,
        Pose {
            position: Vec3::new(10.0, 0.0, 10.0),
            ..Pose::default()
        },
    );
    c.seed_camera(Vec3::new(10.0, 5.0, 30.0), Vec3::new(10.0, 1.0, 10.0), 15.0);
    // The script moves the hero and the camera away, then asks for the field camera.
    c.apply(&actor(0, A::SetPosition([20.0, 0.0, 0.0])));
    c.apply(&Event::Camera(CameraCommand::SetPosition([0.0, 0.0, 0.0])));
    run(&mut c, 2);
    c.apply(&Event::CameraToField { duration: 1 });
    let signals = run(&mut c, 3);
    assert!(signals.contains(&Signal::CameraIdle));
    let view = c.camera();
    assert!(
        (view.eye - Vec3::new(20.0, 5.0, 20.0)).length() < 1e-3,
        "{:?}",
        view.eye
    );
    assert!((view.target - Vec3::new(20.0, 1.0, 0.0)).length() < 1e-3);
}

#[test]
fn the_three_bone_camera_is_saved_and_restored() {
    let mut c = Cutscene::new();
    c.apply(&Event::Camera(CameraCommand::SetPosition([1.0, 1.0, 1.0])));
    run(&mut c, 2);
    c.apply(&Event::Camera(CameraCommand::ThreeBoneCamera {
        source: -6,
        target: 0,
        eye: "eye".into(),
        lookat: "lookat".into(),
        foot: "foot".into(),
    }));
    let view = c.camera();
    assert!(view.bones.is_none());
    let bones = view.three_bone.as_ref().unwrap();
    assert_eq!((bones.source, bones.target), (-6, 0));
    assert_eq!(bones.foot, "foot");
    c.apply(&Event::Camera(CameraCommand::RestoreSaved));
    let view = c.camera();
    assert!(view.three_bone.is_none());
    assert_eq!(view.eye, Vec3::ONE);
}

#[test]
fn a_door_turns_a_quarter_turn_in_sixteen_updates() {
    let mut c = Cutscene::new();
    c.apply(&Event::OpenDoor {
        block: 0,
        placement: 1,
        sound: true,
        option: true,
    });
    let door = c.doors()[0];
    assert_eq!((door.block, door.placement), (0, 1));
    run(&mut c, 2 * 15);
    assert!(c.doors()[0].angle < Door::QUARTER_TURN);
    run(&mut c, 2);
    assert!((c.doors()[0].angle - Door::QUARTER_TURN).abs() < 1e-6);
    assert_eq!(c.doors()[0].angle_fx(), 6434);
    // The animation stops at its target.
    run(&mut c, 20);
    assert!((c.doors()[0].angle - Door::QUARTER_TURN).abs() < 1e-6);
    assert_eq!(c.doors()[0].angle_fx(), 6434);
}

#[test]
fn a_map_door_survives_a_new_event() {
    let mut c = Cutscene::new();
    assert!(c.open_door(0, 2, false));
    run(&mut c, 40);
    assert_eq!(c.doors()[0].angle_fx(), 6434);
    // A new event restarts the scene, not the map: the door stays open at its angle.
    c.restart();
    assert_eq!(c.doors().len(), 1);
    assert_eq!(c.doors()[0].angle_fx(), 6434);
    assert!(!c.open_door(0, 2, false));
}

#[test]
fn a_timed_caption_ramps_in_holds_and_ramps_out() {
    let mut c = Cutscene::new();
    c.apply(&Event::CaptionTime { ticks: 140 });
    c.apply(&Event::CaptionCentered);
    c.apply(&Event::CaptionSilent);
    assert_eq!(c.caption_alpha(), 0.0);
    run(&mut c, 30);
    assert!((c.caption_alpha() - 0.5).abs() < 1e-6);
    run(&mut c, 30);
    assert!((c.caption_alpha() - 1.0).abs() < 1e-6);
    run(&mut c, 140);
    assert!((c.caption_alpha() - 1.0).abs() < 1e-6);
    run(&mut c, 30);
    assert!((c.caption_alpha() - 0.5).abs() < 1e-6);
    run(&mut c, 30);
    assert_eq!(c.caption_alpha(), 0.0);
    let caption = c.caption();
    assert!(caption.centered && caption.silent && caption.time == Some(140));
    // A new message resets the caption fields.
    c.apply(&Event::Message {
        text: crate::script::Text::Id(100),
        flags: 0,
    });
    assert_eq!(c.caption_alpha(), 1.0);
    assert!(!c.caption().centered && !c.caption().silent);
}

#[test]
fn the_display_swap_survives_a_chained_event() {
    let mut c = Cutscene::new();
    assert!(!c.screen_swapped());
    c.apply(&Event::SwapScreens(true));
    assert!(c.screen_swapped());
    c.restart();
    assert!(c.screen_swapped());
    c.apply(&Event::SwapScreens(false));
    assert!(!c.screen_swapped());
}
