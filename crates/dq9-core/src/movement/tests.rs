use super::*;
use crate::collision::STAND_HEIGHT;
use crate::testing::{self, floor, wall_x, world};

fn u(v: f32) -> i32 {
    (v * 4096.0) as i32
}

const UP: Input = Input {
    up: true,
    down: false,
    left: false,
    right: false,
};
const RIGHT: Input = Input {
    up: false,
    down: false,
    left: false,
    right: true,
};
const NONE: Input = Input {
    up: false,
    down: false,
    left: false,
    right: false,
};

fn flat() -> CollisionWorld {
    world(&floor(2560, 0))
}

/// A hero on the floor at the origin facing north (`-z`, angle π).
fn hero() -> Hero {
    Hero::place([0, STAND_HEIGHT, 0], PI)
}

fn run(hero: &mut Hero, world: &CollisionWorld, input: Input, camera: i32, steps: usize) {
    for _ in 0..steps {
        hero.step(world, input, camera, 1);
    }
}

#[test]
fn the_d_pad_picks_a_direction_with_the_diagonals_first() {
    let pad = |up, down, left, right| Input {
        up,
        down,
        left,
        right,
    };
    assert_eq!(pad(true, false, false, false).heading_index(), Some(0));
    assert_eq!(pad(false, true, false, false).heading_index(), Some(1));
    assert_eq!(pad(false, false, true, false).heading_index(), Some(2));
    assert_eq!(pad(false, false, false, true).heading_index(), Some(3));
    assert_eq!(pad(true, false, true, false).heading_index(), Some(4));
    assert_eq!(pad(true, false, false, true).heading_index(), Some(5));
    assert_eq!(pad(false, true, true, false).heading_index(), Some(6));
    assert_eq!(pad(false, true, false, true).heading_index(), Some(7));
    // Opposite keys: up beats down, left beats right.
    assert_eq!(pad(true, true, false, false).heading_index(), Some(0));
    assert_eq!(pad(false, false, true, true).heading_index(), Some(2));
    assert_eq!(pad(true, true, true, true).heading_index(), Some(4));
    assert_eq!(pad(false, false, false, false).heading_index(), None);
}

#[test]
fn the_speed_ramps_up_and_the_key_acts_one_step_late() {
    let w = flat();
    let mut h = hero();
    let mut speeds = Vec::new();
    let mut z = Vec::new();
    for _ in 0..16 {
        h.step(&w, UP, 0, 1);
        speeds.push(h.speed());
        z.push(h.position_fx()[2]);
    }
    // The key is read after the motion: the first step only starts walking.
    assert_eq!(&speeds[..4], &[0, 0x28, 0x50, 0x78]);
    assert_eq!(z[0], 0);
    assert_eq!(z[1], 0);
    // Motion uses the speed of the step before: 0x28 on the third step.
    assert_eq!(z[2], -0x28);
    assert_eq!(z[3], -0x28 - 0x50);
    // Ten steps of 0x28 overshoot the top speed by 7, which the next step cuts back.
    assert_eq!(speeds[10], 0x190);
    assert_eq!(speeds[11], WALK_SPEED);
    assert_eq!(speeds[15], WALK_SPEED);
    assert_eq!(z[15] - z[14], -WALK_SPEED);
    assert!(h.is_walking());
    assert_eq!(h.position_fx()[0], 0);
}

#[test]
fn the_hero_turns_to_the_heading_before_it_can_walk() {
    let w = flat();
    // Facing south (0), walking north: half a turn of 0x324 per step takes 16 steps.
    let mut h = Hero::place([0, STAND_HEIGHT, 0], 0);
    let mut turned = 0;
    for step in 1..=40 {
        h.step(&w, UP, 0, 1);
        if turned == 0 && h.angle() == PI {
            turned = step;
        }
    }
    // The first step reads the key; the turn takes 16 steps of 0x324 plus a last one.
    assert_eq!(turned, 18);
    // While the model lags the hero already goes the way the key says, but slower: the top
    // speed is cut by the share of the half turn still to do.
    let mut h = Hero::place([0, STAND_HEIGHT, 0], 0);
    run(&mut h, &w, UP, 0, 6);
    let early = h.position_fx()[2];
    assert!(early < 0 && early > -0x200, "{early}");
    run(&mut h, &w, UP, 0, 60);
    assert!(h.position_fx()[2] < -u(3.0));
}

#[test]
fn a_diagonal_is_as_fast_as_an_axis() {
    let w = flat();
    let mut h = Hero::place([0, STAND_HEIGHT, 0], 0x25b3);
    let up_right = Input {
        up: true,
        right: true,
        ..NONE
    };
    run(&mut h, &w, up_right, 0, 30);
    let at = h.position_fx();
    h.step(&w, up_right, 0, 1);
    let step = fx::distance(at, h.position_fx());
    assert!((step - WALK_SPEED).abs() <= 2, "{step}");
    let moved = fx::sub(h.position_fx(), at);
    assert!(moved[0] > 0 && moved[2] < 0);
}

#[test]
fn the_camera_turns_the_d_pad() {
    let w = flat();
    // A camera yawed a quarter turn: up now walks west.
    let mut h = Hero::place([0, STAND_HEIGHT, 0], 0x4b66);
    run(&mut h, &w, UP, 0x1922, 40);
    let at = h.position_fx();
    assert!(at[0] < -u(1.0), "{at:?}");
    assert!(at[2].abs() < 16);
    assert_eq!(h.target_angle(), 0x4b66);
}

#[test]
fn releasing_the_key_stops_the_hero() {
    let w = flat();
    let mut h = hero();
    run(&mut h, &w, UP, 0, 30);
    assert_eq!(h.speed(), WALK_SPEED);
    let mut speeds = Vec::new();
    for _ in 0..12 {
        h.step(&w, NONE, 0, 1);
        speeds.push(h.speed());
    }
    assert!(!h.is_walking());
    // The key is read after the update: the first step after the release still runs at
    // speed, then the speed falls by 0x28 per step.
    assert_eq!(speeds[0], WALK_SPEED);
    assert_eq!(speeds[1], WALK_SPEED - 0x28);
    assert_eq!(*speeds.last().unwrap(), 0);
    let at = h.position_fx();
    run(&mut h, &w, NONE, 0, 10);
    assert_eq!(h.position_fx(), at);
}

#[test]
fn a_wall_stops_the_hero() {
    let w = world(&[floor(2560, 0), wall_x(512, -2560, 2560, 0, 1024)].concat());
    // The wall is at x = 2: the sphere stops half a unit before it.
    let mut h = Hero::place([0, STAND_HEIGHT, 0], 0x1922);
    run(&mut h, &w, RIGHT, 0, 200);
    let x = h.position_fx()[0];
    assert!((x - u(1.5)).abs() < 16, "{x}");
    assert!(h.wall().is_some());
    // Pressing on does not move it, nor does it turn it away.
    run(&mut h, &w, RIGHT, 0, 20);
    assert_eq!(h.position_fx()[0], x);
}

#[test]
fn the_hero_slides_along_a_wall_it_meets_at_an_angle() {
    let w = world(&[floor(2560, 0), wall_x(512, -2560, 2560, 0, 1024)].concat());
    let mut h = Hero::place([u(-2.0), STAND_HEIGHT, u(4.0)], 0x25b3);
    let up_right = Input {
        up: true,
        right: true,
        ..NONE
    };
    run(&mut h, &w, up_right, 0, 120);
    let at = h.position_fx();
    // It reached the wall (x just under 1.5) and went on north along it.
    assert!(at[0] > u(1.4) && at[0] < u(1.55), "{at:?}");
    assert!(at[2] < u(-1.0), "{at:?}");
    // It turns towards the wall's direction.
    assert!(fx::abs(fx::signed_angle_distance(h.angle(), PI)) < 0x400);
}

#[test]
fn the_hero_falls_off_a_ledge_and_lands_below() {
    // A floor for x < 0 and another three units lower for x >= 0.
    let high = [[-2560, 0, -2560], [-2560, 0, 2560], [0, 0, -2560]];
    let high2 = [[0, 0, -2560], [-2560, 0, 2560], [0, 0, 2560]];
    let low = [[0, -768, -2560], [0, -768, 2560], [2560, -768, -2560]];
    let low2 = [[2560, -768, -2560], [0, -768, 2560], [2560, -768, 2560]];
    let w = world(&[high, high2, low, low2]);
    let mut h = Hero::place([u(-1.0), STAND_HEIGHT, 0], 0x1922);
    let mut fell = false;
    for _ in 0..200 {
        h.step(&w, RIGHT, 0, 1);
        fell |= h.position_fx()[1] < STAND_HEIGHT - u(1.0);
    }
    assert!(fell);
    let at = h.position_fx();
    assert!(at[0] > 0);
    // The height only follows the floor beyond 0x28 units.
    assert!((at[1] - (-u(3.0) + STAND_HEIGHT)).abs() <= 0x30, "{at:?}");
}

#[test]
fn a_fast_move_is_resolved_in_halves() {
    // Three ticks at full speed travel 0x497, more than the 0x333 of a single pass.
    let w = flat();
    let mut h = hero();
    for _ in 0..20 {
        h.step(&w, UP, 0, 1);
    }
    let at = h.position_fx();
    h.step(&w, UP, 0, 3);
    // Each half rounds to whole units: at most one unit short.
    let moved = fx::distance(at, h.position_fx());
    assert!((WALK_SPEED * 3 - moved).abs() <= 1, "{moved}");
}

#[test]
fn a_pose_in_world_units_places_the_hero() {
    let h = Hero::at(Vec3::new(1.0, 2.0, -3.0), std::f32::consts::PI);
    assert_eq!(h.position_fx(), [0x1000, 0x2000, -0x3000]);
    // Application yaw π is game angle 0.
    assert_eq!(h.angle(), 0);
    assert!((h.yaw() - std::f32::consts::PI).abs() < 1e-3);
    let _ = testing::floor(1, 0);
}

#[test]
fn directions_of_the_scripts_press_the_matching_keys() {
    let keys = |x: f32, z: f32| Input::from_direction(Vec3::new(x, 0.0, z));
    assert_eq!(keys(0.0, -1.0), UP);
    assert_eq!(keys(1.0, 0.0), RIGHT);
    let diagonal = keys(
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    );
    assert!(diagonal.up && diagonal.right && !diagonal.down && !diagonal.left);
    assert_eq!(keys(0.0, 0.0), NONE);
}
