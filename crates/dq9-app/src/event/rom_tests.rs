use super::placement::placement_instances;
use super::*;
use crate::load::{self, LoadedMap};
use crate::report;
use crate::talk::{Button, Talk};
use dq9_core::field::initial_state;
use dq9_formats::cast::Story;
use dq9_formats::hero::HeroLook;

/// The story point and look the CLI and the other ROM tests use by default: `1.1.1`,
/// daytime (the events below were traced at that point).
fn pinned_story() -> Story {
    Story {
        chapter: 1,
        step: 1,
        substep: 1,
        daytime: true,
        ..Story::default()
    }
}

/// The map of `event` at time 0, with the story and the look the other ROM tests use.
fn load_event_map(rom: &[u8], event: u16) -> (LoadedMap, Camera, Talk) {
    let place = crate::event::place(rom, event).expect("event record");
    let spawn = crate::event::spawn(&place);
    let loaded = load::load(
        rom,
        &place.map.to_string(),
        0,
        &pinned_story(),
        spawn,
        None,
        &HeroLook::new_game(),
    )
    .expect("map of the event");
    let mut camera = Camera::default();
    if let Some(bounds) = loaded.runtime.scene.bounds() {
        camera.frame(bounds);
    }
    let mut talk = Talk::default();
    talk.set_rom(rom);
    (loaded, camera, talk)
}

/// One fixed step as the application runs it (`App::step_event`): the pending event's
/// scene is prepared, the script and the cutscene advance, the collision takes the
/// placement changes and the scene animates. `press` reads the messages of the script.
#[allow(clippy::too_many_arguments)]
fn frame(
    rom: &[u8],
    player: &mut EventPlayer,
    loaded: &mut LoadedMap,
    hero: &mut Hero,
    camera: &mut Camera,
    talk: &mut Talk,
    state: &State,
    press: bool,
) {
    if press {
        talk.press(Button::Confirm);
    }
    report::step_frame(rom, player, loaded, hero, camera, talk, state, false)
        .expect("scene animation");
}

/// Event 24590 of D04M02 hides placement `(0, 26)` at its update 2 and shows it at update
/// 257. The placement has twenty drawn children (ids 0 to 24, parts 12 to 34) that no
/// action of the script names: the image must follow the collision, which hides the whole
/// subtree (`Zone3D_FindPlacement` `0x02019518`, `02013390`).
#[test]
fn a_scripted_hide_takes_the_drawn_children_and_the_show_restores_them() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let state = initial_state(&pinned_story());
    let (mut loaded, mut camera, mut talk) = load_event_map(rom, 24590);
    let mut hero = loaded.hero;
    let mut player = EventPlayer::new();
    player.queue(24590);

    let subtree = loaded.collision.placement_subtree(0, 26);
    assert!(
        subtree.len() > 1,
        "placement (0, 26) has no drawn hierarchy: {subtree:?}"
    );
    let root = placement_instances(&loaded.runtime.scene, 0, 26);
    assert!(!root.is_empty(), "placement (0, 26) is not drawn");
    let instances: Vec<usize> = subtree
        .iter()
        .flat_map(|&(block, id)| placement_instances(&loaded.runtime.scene, block, id))
        .collect();
    assert!(
        instances.len() > root.len(),
        "placement (0, 26) has no drawn child: {subtree:?}"
    );
    let root_instances: BTreeSet<usize> = root.iter().copied().collect();
    assert!(
        root.iter()
            .any(|&instance| loaded.runtime.scene.instances[instance].drawn()),
        "placement (0, 26) is hidden at the chosen time of day"
    );
    assert!(
        instances.iter().any(|&instance| {
            !root_instances.contains(&instance) && loaded.runtime.scene.instances[instance].drawn()
        }),
        "no drawn child of placement (0, 26): {subtree:?}"
    );
    let original: Vec<Mat4> = instances
        .iter()
        .map(|&instance| loaded.runtime.scene.instances[instance].world)
        .collect();
    let homes: Vec<Mat4> = instances
        .iter()
        .map(|&instance| loaded.runtime.scene.instances[instance].home)
        .collect();
    let drawn_before: Vec<bool> = instances
        .iter()
        .map(|&instance| loaded.runtime.scene.instances[instance].drawn())
        .collect();
    // A model of the same block that no action of the script names: neither the hide nor
    // the show of (0, 26) may touch it.
    let sibling = placement_instances(&loaded.runtime.scene, 0, 2);
    assert!(
        sibling
            .iter()
            .any(|&instance| loaded.runtime.scene.instances[instance].drawn()),
        "the unrelated placement (0, 2) is not drawn"
    );
    let sibling_worlds: Vec<Mat4> = sibling
        .iter()
        .map(|&instance| loaded.runtime.scene.instances[instance].world)
        .collect();

    // The script's own hide: its update 2 runs at the fourth tick.
    let mut hidden = false;
    for tick in 0..120 {
        frame(
            rom,
            &mut player,
            &mut loaded,
            &mut hero,
            &mut camera,
            &mut talk,
            &state,
            tick % 30 == 0,
        );
        if root
            .iter()
            .all(|&instance| !loaded.runtime.scene.instances[instance].drawn())
        {
            hidden = true;
            break;
        }
    }
    assert!(hidden, "event 24590 did not hide placement (0, 26)");
    for &instance in &instances {
        assert!(
            !loaded.runtime.scene.instances[instance].drawn(),
            "instance {instance} below the hidden placement (0, 26) stayed drawn"
        );
    }
    for (&instance, &world) in sibling.iter().zip(&sibling_worlds) {
        assert_eq!(
            loaded.runtime.scene.instances[instance].world, world,
            "the hide of (0, 26) changed the unrelated instance {instance}"
        );
    }

    // The script's own show, at its update 257, puts every one of them back.
    let mut shown = false;
    for tick in 0..4_000 {
        frame(
            rom,
            &mut player,
            &mut loaded,
            &mut hero,
            &mut camera,
            &mut talk,
            &state,
            tick % 30 == 0,
        );
        if root
            .iter()
            .any(|&instance| loaded.runtime.scene.instances[instance].drawn())
        {
            shown = true;
            break;
        }
    }
    assert!(shown, "event 24590 did not show placement (0, 26) again");
    for (index, &instance) in instances.iter().enumerate() {
        let drawn = loaded.runtime.scene.instances[instance].drawn();
        assert_eq!(
            drawn, drawn_before[index],
            "instance {instance} after the show"
        );
        assert_eq!(
            loaded.runtime.scene.instances[instance].world, original[index],
            "the show of (0, 26) changed the world of instance {instance}"
        );
        if drawn {
            assert_eq!(
                loaded.runtime.scene.instances[instance].world, homes[index],
                "instance {instance} did not come back on its home transform"
            );
        }
    }
    for (&instance, &world) in sibling.iter().zip(&sibling_worlds) {
        assert_eq!(
            loaded.runtime.scene.instances[instance].world, world,
            "the show of (0, 26) changed the unrelated instance {instance}"
        );
    }

    // The event ends; its actor instances are gone, the map's placements stay.
    for tick in 0..2_000 {
        if !player.is_active() && player.pending_start().is_none() {
            break;
        }
        frame(
            rom,
            &mut player,
            &mut loaded,
            &mut hero,
            &mut camera,
            &mut talk,
            &state,
            tick % 30 == 0,
        );
    }
    assert!(!player.is_active(), "event 24590 did not end");

    // The trigger path (actions 0x95/0x96) on the same placement: the cascade hides the
    // subtree and touches nothing else, and the show restores every instance exactly.
    let before: Vec<Mat4> = loaded
        .runtime
        .scene
        .instances
        .iter()
        .map(|instance| instance.world)
        .collect();
    let drawn: Vec<bool> = loaded
        .runtime
        .scene
        .instances
        .iter()
        .map(dq9_render::SceneInstance::drawn)
        .collect();
    let subtree_instances: BTreeSet<usize> = instances.iter().copied().collect();
    player.set_placement(
        &mut loaded.runtime,
        &mut loaded.collision,
        PlacementChange::Hidden {
            group: 0,
            id: 26,
            hidden: true,
        },
    );
    for &instance in &instances {
        assert!(
            !loaded.runtime.scene.instances[instance].drawn(),
            "action 0x95 left instance {instance} of the subtree drawn"
        );
    }
    for (instance, &world) in before.iter().enumerate() {
        if subtree_instances.contains(&instance) {
            continue;
        }
        assert_eq!(
            loaded.runtime.scene.instances[instance].world, world,
            "hiding placement (0, 26) changed unrelated instance {instance}"
        );
        assert_eq!(
            loaded.runtime.scene.instances[instance].drawn(),
            drawn[instance],
            "hiding placement (0, 26) hid unrelated instance {instance}"
        );
    }

    // A child hidden by itself stays hidden when its hidden parent is shown again.
    let child = subtree
        .iter()
        .copied()
        .find(|&(block, id)| {
            (block, id) != (0, 26)
                && !placement_instances(&loaded.runtime.scene, block, id).is_empty()
        })
        .expect("a drawn child of placement (0, 26)");
    let child_instances = placement_instances(&loaded.runtime.scene, child.0, child.1);
    player.set_placement(
        &mut loaded.runtime,
        &mut loaded.collision,
        PlacementChange::Hidden {
            group: child.0 as i32,
            id: child.1 as i32,
            hidden: true,
        },
    );
    player.set_placement(
        &mut loaded.runtime,
        &mut loaded.collision,
        PlacementChange::Hidden {
            group: 0,
            id: 26,
            hidden: false,
        },
    );
    for &instance in &child_instances {
        assert!(
            !loaded.runtime.scene.instances[instance].drawn(),
            "the show of (0, 26) re-showed instance {instance} of {child:?}, hidden by itself"
        );
    }
    for &instance in &root {
        assert!(
            loaded.runtime.scene.instances[instance].drawn(),
            "the show of (0, 26) left its own instance {instance} hidden"
        );
    }
    player.set_placement(
        &mut loaded.runtime,
        &mut loaded.collision,
        PlacementChange::Hidden {
            group: child.0 as i32,
            id: child.1 as i32,
            hidden: false,
        },
    );
    for (instance, &world) in before.iter().enumerate() {
        assert_eq!(
            loaded.runtime.scene.instances[instance].world, world,
            "the hide/show cycle changed instance {instance}"
        );
    }

    // A show of the child while its parent stays hidden does not bring it back; the
    // parent's show does.
    player.set_placement(
        &mut loaded.runtime,
        &mut loaded.collision,
        PlacementChange::Hidden {
            group: 0,
            id: 26,
            hidden: true,
        },
    );
    player.set_placement(
        &mut loaded.runtime,
        &mut loaded.collision,
        PlacementChange::Hidden {
            group: child.0 as i32,
            id: child.1 as i32,
            hidden: false,
        },
    );
    for &instance in &child_instances {
        assert!(
            !loaded.runtime.scene.instances[instance].drawn(),
            "instance {instance} of {child:?} showed under its hidden parent (0, 26)"
        );
    }
    player.set_placement(
        &mut loaded.runtime,
        &mut loaded.collision,
        PlacementChange::Hidden {
            group: 0,
            id: 26,
            hidden: false,
        },
    );
    for (instance, &world) in before.iter().enumerate() {
        assert_eq!(
            loaded.runtime.scene.instances[instance].world, world,
            "the second hide/show cycle changed instance {instance}"
        );
    }
}

/// A chained event on another map must not apply the left map's scene indices to the new
/// scene. Event 9210 (M09M02, 7 instances, one NPC) -> 9220 (M09M04, 22) leaves the saved
/// home of that NPC at index 5, inside the new scene; event 9710 (M05, 35 instances, 15
/// NPCs) -> 9713 (O00, 31) leaves their homes at indices 19 to 33, inside it too. The
/// application loads the new map, calls `map_loaded` and prepares the event.
#[test]
fn a_chained_event_on_another_map_ignores_the_previous_map_bindings() {
    let Some(rom) = testrom::load() else {
        return;
    };
    let story = pinned_story();
    let state = initial_state(&story);
    for &(event, next) in &[(9210u16, 9220u16), (9710, 9713)] {
        let (mut loaded, mut camera, mut talk) = load_event_map(rom, event);
        let cast: Vec<usize> = loaded
            .runtime
            .scene
            .instances
            .iter()
            .enumerate()
            .filter(|(_, instance)| instance.block == usize::MAX && instance.instance != usize::MAX)
            .map(|(index, _)| index)
            .collect();
        let mut hero = loaded.hero;
        let mut player = EventPlayer::new();
        player.queue(event);
        let played = report::play(
            rom,
            &mut loaded,
            &mut hero,
            &mut camera,
            &mut talk,
            &mut player,
            &state,
            None,
            &[],
            Some(60),
        )
        .expect("playback of the event");
        assert_eq!(
            played.elsewhere,
            Some(next),
            "event {event} must chain to {next} on another map"
        );
        assert_eq!(player.pending_start(), Some(next));
        assert!(
            !cast.is_empty(),
            "event {event}: the map has no NPC whose home could carry over"
        );

        let place = crate::event::place(rom, next).expect("place of the chained event");
        let spawn = crate::event::spawn(&place);
        let mut there = load::load(
            rom,
            &place.map.to_string(),
            0,
            &story,
            spawn,
            None,
            &HeroLook::new_game(),
        )
        .expect("map of the chained event");
        // A stale home index must reach the new scene, or the test would prove nothing.
        assert!(
            cast.iter()
                .any(|&index| index < there.runtime.scene.instances.len()),
            "event {event}: none of the {} NPC homes reaches the {} instances of {}",
            cast.len(),
            there.runtime.scene.instances.len(),
            there.entry.code
        );
        let before: Vec<(Mat4, Mat4, bool)> = there
            .runtime
            .scene
            .instances
            .iter()
            .map(|instance| (instance.world, instance.home, instance.drawn()))
            .collect();
        let mut there_hero = there.hero;
        player.map_loaded(&camera);
        {
            let mut stage = Stage {
                rom,
                world: &state,
                scene: &mut there.runtime,
                hero: &mut there_hero,
                collision: &there.collision,
                camera: &mut camera,
                talk: &mut talk,
            };
            player.prepare(rom, next, &mut stage);
        }
        for (index, &(world, home, drawn)) in before.iter().enumerate() {
            let instance = &there.runtime.scene.instances[index];
            assert_eq!(
                instance.world, world,
                "instance {index} of {} moved by the bindings of event {event}",
                there.entry.code
            );
            assert_eq!(
                instance.home, home,
                "instance {index} of {} changed home transform after event {event}",
                there.entry.code
            );
            assert_eq!(
                instance.drawn(),
                drawn,
                "instance {index} of {} hid on the bindings of event {event}",
                there.entry.code
            );
        }
    }
}
