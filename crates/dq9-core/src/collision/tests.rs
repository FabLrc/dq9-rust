use super::*;
use crate::testing::{self, floor, wall_x, world};

/// World units to `fx32`.
fn u(v: f32) -> i32 {
    (v * 4096.0) as i32
}

/// The ground ray meets the plane through a 12-bit fraction of an 11-unit segment: heights
/// carry a few units of noise, like the game's.
fn near(got: i32, want: i32) -> bool {
    (got - want).abs() <= 8
}

fn body_at(x: i32, y: i32, z: i32) -> Body {
    Body {
        position: [x, y, z],
        fall: 0,
        radius: 0x1000,
        height: 0x1800,
        wall: None,
    }
}

/// Resolves a body that did not move.
fn settle(world: &CollisionWorld, body: &mut Body) -> Outcome {
    let position = body.position;
    world.resolve(body, position, Mode::Whole)
}

fn world_of(parts: &[&[testing::Tri3]]) -> CollisionWorld {
    let all: Vec<_> = parts.iter().flat_map(|p| p.iter().copied()).collect();
    world(&all)
}

#[test]
fn the_ground_ray_stands_a_character_on_the_floor() {
    let w = world(&floor(2560, 0));
    assert!(near(w.ground([0, 0x400, 0]).unwrap(), STAND_HEIGHT));
    // Within the vertical window: one unit above to ten below.
    assert!(near(
        w.ground([u(3.0), u(9.0), u(-3.0)]).unwrap(),
        STAND_HEIGHT
    ));
    assert_eq!(w.ground([0, u(10.5), 0]), None);
    // Past the mesh.
    assert_eq!(w.ground([u(11.0), 0, 0]), None);
    let height = w.ground_height(0.0, 0.0, 0.1).unwrap();
    assert!((height - STAND_HEIGHT as f32 / 4096.0).abs() < 0.003);
}

#[test]
fn the_highest_floor_within_reach_wins() {
    let w = world_of(&[&floor(2560, 0), &floor(1280, 512)]);
    assert!(near(
        w.ground([0, u(3.0), 0]).unwrap(),
        u(2.0) + STAND_HEIGHT
    ));
    // Outside the smaller floor only the lower one remains.
    assert!(near(w.ground([u(7.0), u(3.0), 0]).unwrap(), STAND_HEIGHT));
}

#[test]
fn a_character_near_the_floor_snaps_to_it() {
    let w = world(&floor(2560, 0));
    let mut body = body_at(0, STAND_HEIGHT + 0x400, 0);
    let outcome = settle(&w, &mut body);
    assert!(outcome.grounded);
    assert!(near(body.position[1], STAND_HEIGHT));
    assert_eq!(body.fall, 0);
    // A few units off the floor keep their height: the game only snaps beyond 0x28.
    let mut body = body_at(0, STAND_HEIGHT + 0x20, 0);
    settle(&w, &mut body);
    assert_eq!(body.position[1], STAND_HEIGHT + 0x20);
}

#[test]
fn a_character_far_above_the_floor_falls_with_gravity() {
    let w = world(&floor(2560, 0));
    let mut body = body_at(0, u(2.0), 0);
    let outcome = settle(&w, &mut body);
    assert!(!outcome.grounded);
    assert!(outcome.started_falling);
    assert_eq!(body.fall, 0x51);
    let outcome = settle(&w, &mut body);
    assert!(!outcome.started_falling);
    assert_eq!(body.fall, 0xa2);
    for _ in 0..100 {
        settle(&w, &mut body);
    }
    assert_eq!(body.fall, 0x1000);
}

#[test]
fn steps_up_are_taken_below_one_unit_only() {
    let low = world_of(&[&floor(2560, 0), &floor(1280, 128)]);
    let mut body = body_at(0, STAND_HEIGHT, 0);
    settle(&low, &mut body);
    assert!(
        near(body.position[1], STAND_HEIGHT + 0x800),
        "{:?}",
        body.position
    );
    let high = world_of(&[&floor(2560, 0), &floor(1280, 384)]);
    let mut body = body_at(0, STAND_HEIGHT, 0);
    settle(&high, &mut body);
    assert_eq!(body.position[1], STAND_HEIGHT);
}

#[test]
fn a_wall_pushes_the_sphere_back_by_its_radius() {
    let w = world_of(&[&floor(2560, 0), &wall_x(256, -512, 512, 0, 512)]);
    let mut body = body_at(u(0.8), STAND_HEIGHT, u(0.3));
    let before = body.position;
    w.resolve(&mut body, before, Mode::Whole);
    // The wall is at x = 1 and the sphere's radius 0.5.
    assert!((body.position[0] - 0x800).abs() <= 2, "{:?}", body.position);
    assert_eq!(body.position[1], before[1]);
    assert!((body.position[2] - before[2]).abs() <= 2);
    let wall = body.wall.expect("wall touched");
    assert!(wall.normal[0] < -0xf00 && wall.normal[2].abs() < 0x10);
    // Away from the wall nothing happens, and the wall is forgotten.
    let mut body = body_at(0, STAND_HEIGHT, 0);
    settle(&w, &mut body);
    assert_eq!(body.position, [0, STAND_HEIGHT, 0]);
    assert_eq!(body.wall, None);
}

#[test]
fn the_end_of_a_wall_pushes_by_distance() {
    let w = world_of(&[&floor(2560, 0), &wall_x(256, -512, 512, 0, 512)]);
    // 0.1 before the wall's plane and 0.2 past its end: 0.22 from the corner.
    let mut body = body_at(u(0.9), STAND_HEIGHT, u(2.2));
    settle(&w, &mut body);
    let (x, z) = (
        body.position[0] as f32 / 4096.0,
        body.position[2] as f32 / 4096.0,
    );
    let from_corner = ((x - 1.0).powi(2) + (z - 2.0).powi(2)).sqrt();
    assert!((from_corner - 0.5).abs() < 0.01, "{x} {z}");
    assert!(x < 0.9 && z > 2.2);
}

#[test]
fn a_rotated_piece_is_queried_in_its_own_frame() {
    // A floor over local x in [-4, 4], z in [-1, 1] units, turned a quarter: it now covers
    // world z in [-4, 4] and x in [-1, 1]. The box that picks its triangles is not turned
    // (as in the game), so the point (3, 0) still draws them in, but the ground ray, which
    // is, finds nothing there.
    let a = [[-1024, 0, -256], [-1024, 0, 256], [1024, 0, -256]];
    let b = [[1024, 0, -256], [-1024, 0, 256], [1024, 0, 256]];
    let piece = |angle| {
        CollisionWorld::from_pieces(
            vec![testing::mesh(4, &[a, b], 1, 1, 0x4000)],
            vec![Piece {
                mesh: 0,
                position: [0; 3],
                angle,
            }],
        )
    };
    let high = STAND_HEIGHT + 0x400;
    let mut straight = body_at(u(3.0), high, 0);
    settle(&piece(0), &mut straight);
    assert!(near(straight.position[1], STAND_HEIGHT));
    let mut turned = body_at(u(3.0), high, 0);
    settle(&piece(0x1922), &mut turned);
    assert_eq!(turned.position[1], high);
    // Where the turned floor is, x in [-1, 1], the box and the ray agree.
    let mut on = body_at(u(0.5), high, 0);
    settle(&piece(0x1922), &mut on);
    assert!(near(on.position[1], STAND_HEIGHT));
}

#[test]
fn the_grid_names_the_cells_around_the_centre() {
    // 4 × 4 cells of 100 units from the origin, half a cell further on odd rows.
    let corner = [[0, 0, 0], [400, 0, 0], [0, 0, 400]];
    let mesh = testing::mesh(0, &[corner], 4, 4, 100);
    let at = |x: i32, z: i32| candidate_cells(&mesh, &[x, 0, z, x, 0, z]);
    // Row 2 (even), column 1: its own cell and the ring around it.
    assert_eq!(at(150, 250), vec![10, 6, 5, 14, 15, 9, 11]);
    // Row 1 (odd) starts half a cell early: x = 150 is column 2.
    assert_eq!(at(150, 150), vec![6, 1, 2, 10, 11, 5, 7]);
    // Outside the grid.
    assert!(at(150, 450).is_empty());
    assert!(at(-150, 50).is_empty());
    // A small mesh is scanned whole.
    let small = testing::mesh(0, &[corner], 1, 1, 100);
    assert_eq!(candidate_cells(&small, &[0; 6]), vec![0]);
}

#[test]
fn a_query_only_returns_triangles_whose_box_meets_it() {
    let near = [[0, 0, 0], [100, 0, 0], [0, 0, 100]];
    let far = [[1000, 0, 1000], [1100, 0, 1000], [1000, 0, 1100]];
    let mesh = testing::mesh(4, &[near, far], 1, 1, 0x4000);
    // Boxes are in fx32: file units times 16.
    let around = |x: i32, z: i32| query(&mesh, [x + 8, 8, z + 8], [x - 8, -8, z - 8]);
    assert_eq!(around(20 * 16, 20 * 16), vec![0]);
    assert_eq!(around(1050 * 16, 1050 * 16), vec![1]);
    assert!(around(500 * 16, 500 * 16).is_empty());
}

#[test]
fn rays_meet_only_the_front_of_a_triangle() {
    let tri = Tri {
        vertices: [[0, 0, 0], [0, 0, u(2.0)], [u(2.0), 0, 0]],
        normal: [0, 0x1000, 0],
    };
    let down = ray_triangle([u(0.5), u(1.0), u(0.5)], [u(0.5), u(-1.0), u(0.5)], &tri);
    assert!(matches!(down, Ray::Hit { .. }));
    // From below, the triangle is seen from behind.
    let up = ray_triangle([u(0.5), u(-1.0), u(0.5)], [u(0.5), u(1.0), u(0.5)], &tri);
    assert_eq!(up, Ray::Miss);
    // Beside it the ray crosses the plane outside.
    let beside = ray_triangle([u(3.0), u(1.0), u(3.0)], [u(3.0), u(-1.0), u(3.0)], &tri);
    assert_eq!(beside, Ray::Outside);
    // Too short to reach the plane.
    let short = ray_triangle([u(0.5), u(2.0), u(0.5)], [u(0.5), u(1.0), u(0.5)], &tri);
    assert_eq!(short, Ray::Miss);
}

#[test]
fn the_closest_point_picks_the_vertex_edge_or_face() {
    let (a, b, c) = ([0, 0, 0], [u(2.0), 0, 0], [0, 0, u(2.0)]);
    assert_eq!(closest_point([u(-1.0), u(1.0), u(-1.0)], a, b, c), a);
    assert_eq!(closest_point([u(4.0), 0, u(-1.0)], a, b, c), b);
    assert_eq!(closest_point([u(-1.0), 0, u(4.0)], a, b, c), c);
    assert_eq!(
        closest_point([u(1.0), u(1.0), u(-1.0)], a, b, c),
        [u(1.0), 0, 0]
    );
    assert_eq!(
        closest_point([u(0.5), u(1.0), u(0.5)], a, b, c),
        [u(0.5), u(1.0), u(0.5)]
    );
    // Past the hypotenuse.
    let on = closest_point([u(2.0), 0, u(2.0)], a, b, c);
    assert!((on[0] - u(1.0)).abs() <= 2 && (on[2] - u(1.0)).abs() <= 2);
}

#[test]
fn the_rotation_matrix_inverts() {
    let turn = rotation_y(0x1922);
    let back = inverse_matrix(&turn).unwrap();
    let v = [u(3.0), u(1.0), u(-2.0)];
    let there_and_back = turn_vector(turn_vector(v, &turn), &back);
    for (got, want) in there_and_back.iter().zip(v) {
        assert!((got - want).abs() <= 4, "{there_and_back:?} vs {v:?}");
    }
}
