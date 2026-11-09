use super::*;
use crate::testing::{self, floor, wall_x};

fn u(v: f32) -> i32 {
    (v * 4096.0) as i32
}

/// A floor (placement 0), a wall at world `x = 1` (placement 16, the tower gate) and a
/// second wall at `x = 2` (placement 7, unaffected), each a piece.
fn world() -> CollisionWorld {
    let floor_mesh = testing::mesh(4, &floor(2560, 0), 1, 1, 0x4000);
    let wall_mesh = testing::mesh(4, &wall_x(256, -512, 512, 0, 512), 1, 1, 0x4000);
    let other_mesh = testing::mesh(4, &wall_x(512, -512, 512, 0, 512), 1, 1, 0x4000);
    CollisionWorld {
        meshes: vec![floor_mesh, wall_mesh, other_mesh],
        pieces: vec![
            Piece {
                mesh: 0,
                position: [0; 3],
                angle: 0,
            },
            Piece {
                mesh: 1,
                position: [0; 3],
                angle: 0,
            },
            Piece {
                mesh: 2,
                position: [0; 3],
                angle: 0,
            },
        ],
        nodes: vec![
            Node {
                block: 0,
                id: 0,
                parent: None,
                position: [0; 3],
                angle: 0,
                hidden: false,
            },
            Node {
                block: 0,
                id: 16,
                parent: None,
                position: [0; 3],
                angle: 0,
                hidden: false,
            },
            Node {
                block: 0,
                id: 7,
                parent: None,
                position: [0; 3],
                angle: 0,
                hidden: false,
            },
        ],
        node_of_piece: vec![0, 1, 2],
        revision: 0,
    }
}

fn body() -> Body {
    Body {
        position: [u(0.8), STAND_HEIGHT, u(0.3)],
        fall: 0,
        radius: 0x1000,
        height: 0x1800,
        wall: None,
    }
}

#[test]
fn hiding_a_placement_removes_its_wall_and_showing_restores_it() {
    let mut world = world();
    let mut blocked = body();
    let start = blocked.position;
    world.resolve(&mut blocked, start, Mode::Whole);
    assert!(blocked.wall.is_some(), "{:?}", blocked.position);

    assert!(world.set_placement_hidden(0, 16, true));
    assert_eq!(world.revision(), 1);
    // The floor placement stays: only the named placement's subtree is hidden.
    assert!(world.ground([0, u(0.5), 0]).is_some());
    let mut free = body();
    world.resolve(&mut free, start, Mode::Whole);
    assert_eq!(free.position, start);
    assert_eq!(free.wall, None);

    assert!(world.set_placement_hidden(0, 16, false));
    assert_eq!(world.revision(), 2);
    let mut blocked_again = body();
    world.resolve(&mut blocked_again, start, Mode::Whole);
    assert!(blocked_again.wall.is_some());
    // A placement of another block entry is not what the name means.
    assert!(!world.set_placement_hidden(1, 16, true));
    assert_eq!(world.revision(), 2);
}

#[test]
fn hiding_the_gate_leaves_the_other_wall_blocking() {
    let mut world = world();
    let mut gate = body();
    let start = gate.position;
    world.resolve(&mut gate, start, Mode::Whole);
    assert!(gate.wall.is_some(), "placement 16 blocks");
    assert!(world.set_placement_hidden(0, 16, true));
    let mut free = body();
    world.resolve(&mut free, start, Mode::Whole);
    assert_eq!(free.position, start, "placement 16 no longer blocks");
    assert_eq!(free.wall, None);
    // Placement 7, at x = 2, is not the hidden one and still pushes to its radius.
    let other_start = [u(1.6), STAND_HEIGHT, u(0.3)];
    let mut other = Body {
        position: other_start,
        fall: 0,
        radius: 0x1000,
        height: 0x1800,
        wall: None,
    };
    world.resolve(&mut other, other_start, Mode::Whole);
    assert!(other.wall.is_some(), "the unaffected wall still blocks");
    assert!(
        (other.position[0] - u(1.5)).abs() <= 2,
        "{:?}",
        other.position
    );
}

#[test]
fn hiding_a_parent_hides_the_collision_of_its_descendants() {
    let mut world = world();
    world.nodes[1].parent = Some(0);
    assert!(world.set_placement_hidden(0, 0, true));
    assert!(world.ground([0, u(0.5), 0]).is_none());
    let mut free = body();
    let start = free.position;
    world.resolve(&mut free, start, Mode::Whole);
    assert!(free.wall.is_none());
    assert!(world.set_placement_hidden(0, 0, false));
    assert!(world.ground([0, u(0.5), 0]).is_some());
    let mut blocked = body();
    world.resolve(&mut blocked, start, Mode::Whole);
    assert!(blocked.wall.is_some());
}

#[test]
fn worlds_without_placement_nodes_cannot_be_hidden() {
    let mut world = testing::world(&floor(2560, 0));
    assert!(!world.set_placement_hidden(0, 16, true));
    assert_eq!(world.revision(), 0);
}

#[test]
fn moving_a_placement_translates_its_collision_subtree_and_bumps_revision() {
    let mut world = world();
    world.nodes[1].parent = Some(0);
    let wall_home = world.pieces[1].position;
    let delta = [u(2.0), 0, 0];
    assert!(world.set_placement_position(0, 0, delta));
    assert_eq!(world.pieces[0].position, delta);
    assert_eq!(world.pieces[1].position, fx::add(wall_home, delta));
    assert_eq!(world.revision(), 1);
    // Setting the same position again changes nothing.
    assert!(!world.set_placement_position(0, 0, delta));
    assert_eq!(world.revision(), 1);
    // The block entry is part of the name.
    assert!(!world.set_placement_position(1, 0, [0; 3]));
    assert_eq!(world.revision(), 1);
}

#[test]
fn rotating_a_moved_parent_keeps_the_descendant_translation() {
    let mut world = world();
    world.nodes[1].parent = Some(0);
    // Put the wall four units along +z of its parent, then move the parent two units +x.
    assert!(world.set_placement_position(0, 16, [0, 0, u(4.0)]));
    assert!(world.set_placement_position(0, 0, [u(2.0), 0, 0]));
    assert_eq!(world.nodes[1].position, [u(2.0), 0, u(4.0)]);
    // A quarter turn around the moved parent's origin: (0, 0, 4) relative becomes
    // (+4, 0, 0) in the map walk's rotation, so the piece sits at (6, 0, 0).
    assert!(world.set_door_angle(0, 0, 0x1922));
    assert_eq!(world.pieces[1].position, [u(6.0), 0, 0]);
}
