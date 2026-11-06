//! Flood fill of the walkable cells of a map, for `dq9re map --reach`.
//!
//! The grid is `0.5` world unit wide and anchored at the start point. A cell is reached when
//! a [`Hero`] stands on it after walking from a reached neighbour with the real
//! [`Hero::step`] and [`CollisionWorld`], not by projecting triangles onto `x/z`: the arrival
//! is checked in 3D (less than `0.3` from the cell's floor), a climb is at most `1.0` and a
//! fall at most `10.0`. Nodes are keyed by `x/z` cell **and height**, so two floors above each
//! other stay separate components instead of merging in a top view.
//!
//! Not modelled: the game's camera (the fill walks with camera yaw 0, like the headless pilot),
//! characters pushing the hero, door animations, ladders, oriented-zone magnets and events.
//! Each edge starts with speed 0, so a long open run may be split into shorter grid walks that
//! the real hero would chain without stopping.

use std::collections::{HashMap, VecDeque};

use anyhow::{Context as _, Result};
use dq9_core::collision::CollisionWorld;
use dq9_core::fx::Fx3;
use dq9_core::movement::{Hero, Input};
use glam::Vec3;

use super::image::world_bounds;

/// Grid spacing in world units (`--reach` uses half a unit, not the 1-unit app grid).
pub const GRID_STEP: f32 = 0.5;
/// A cell is reached when the hero stands this close to it, in 3D.
pub const ARRIVE_DISTANCE: f32 = 0.3;
/// Highest step the fill accepts between two cells.
pub const MAX_CLIMB: f32 = 1.0;
/// Deepest drop the fill accepts between two cells.
pub const MAX_FALL: f32 = 10.0;
/// Ticks one cell-to-cell walk may take (0.5 unit needs about 11 at full speed).
const MAX_TICKS: i32 = 40;
/// The eight directions of the d-pad on the grid, `(x, z)`.
const DIRECTIONS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// One reached point: the floor's standing height at `(x, z)`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cell {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Cell {
    pub fn distance(self, other: Cell) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2) + (self.z - other.z).powi(2))
            .sqrt()
    }
}

/// Result of [`flood`].
#[derive(Debug, Clone, Default)]
pub struct Reach {
    /// Start point after [`CollisionWorld::ground`].
    pub start: Cell,
    /// Every reached cell, in breadth-first order.
    pub cells: Vec<Cell>,
    pub min_y: f32,
    pub max_y: f32,
    /// The `--to` target and whether a hero reached it.
    pub target: Option<Cell>,
    pub target_reached: Option<bool>,
}

fn fx3(v: Vec3) -> Fx3 {
    [v.x, v.y, v.z].map(|c| (c * 4096.0) as i32)
}

/// A reached point in world units, as the hero stands on it.
fn point(cell: Cell) -> Vec3 {
    Vec3::new(cell.x, cell.y, cell.z)
}

/// Walks from `start` to `target` with [`Hero::step`]; true when a tick stands within
/// [`ARRIVE_DISTANCE`] of `target` in 3D.
fn walks_to(world: &CollisionWorld, start: Cell, target: Cell) -> bool {
    let direction = Vec3::new(target.x - start.x, 0.0, target.z - start.z);
    if direction.length_squared() < 1e-8 {
        return start.distance(target) < ARRIVE_DISTANCE;
    }
    let heading = (direction.x.atan2(direction.z) * 4096.0) as i32;
    let mut hero = Hero::place(fx3(point(start)), heading);
    let input = Input::from_direction(direction.normalize());
    for _ in 0..MAX_TICKS {
        hero.step(world, input, 0, 1);
        let position = hero.position();
        if Vec3::new(
            position.x - target.x,
            position.y - target.y,
            position.z - target.z,
        )
        .length()
            < ARRIVE_DISTANCE
        {
            return true;
        }
    }
    false
}

/// Floods the cells reachable from `start` (a `fx32` position), checking `target` when given.
/// The start is stood on the floor with [`CollisionWorld::ground`]; a point with no floor
/// within the ray's reach is refused instead of being projected onto a guessed height.
pub fn flood(world: &CollisionWorld, start: Fx3, target: Option<Fx3>) -> Result<Reach> {
    let ground = world.ground(start).with_context(|| {
        format!(
            "no floor at ({:.3}, {:.3}, {:.3}); CollisionWorld::ground found nothing within \
             one unit above or ten below. Pass an explicit Y within reach of the floor",
            start[0] as f32 / 4096.0,
            start[1] as f32 / 4096.0,
            start[2] as f32 / 4096.0
        )
    })?;
    let start_cell = Cell {
        x: start[0] as f32 / 4096.0,
        y: ground as f32 / 4096.0,
        z: start[2] as f32 / 4096.0,
    };
    let bounds = world_bounds(world).context("the map has no collision geometry")?;
    let target = target.map(|t| Cell {
        x: t[0] as f32 / 4096.0,
        y: t[1] as f32 / 4096.0,
        z: t[2] as f32 / 4096.0,
    });
    let bucket = |y: f32| (y / GRID_STEP).floor() as i32;
    let key = |ix: i32, iz: i32, y: f32| (ix, iz, bucket(y));
    let mut visited: HashMap<(i32, i32, i32), f32> = HashMap::new();
    visited.insert(key(0, 0, start_cell.y), start_cell.y);
    let mut queue = VecDeque::new();
    queue.push_back(start_cell);
    let mut cells = Vec::new();
    let mut target_reached = target.map(|_| false);
    while let Some(cell) = queue.pop_front() {
        cells.push(cell);
        let index = |cell: Cell| {
            (
                ((cell.x - start_cell.x) / GRID_STEP).round() as i32,
                ((cell.z - start_cell.z) / GRID_STEP).round() as i32,
            )
        };
        let (ix, iz) = index(cell);
        if let Some(target) = target
            && target_reached == Some(false)
            && (cell.x - target.x).hypot(cell.z - target.z) <= GRID_STEP * 1.5
        {
            let climb = target.y - cell.y;
            if climb <= MAX_CLIMB && -climb <= MAX_FALL && walks_to(world, cell, target) {
                target_reached = Some(true);
            }
        }
        for (dx, dz) in DIRECTIONS {
            let (nx, nz) = (ix + dx, iz + dz);
            let x = start_cell.x + nx as f32 * GRID_STEP;
            let z = start_cell.z + nz as f32 * GRID_STEP;
            if x < bounds.min_x - GRID_STEP
                || x > bounds.max_x + GRID_STEP
                || z < bounds.min_z - GRID_STEP
                || z > bounds.max_z + GRID_STEP
            {
                continue;
            }
            let Some(ground) = world.ground([
                (x * 4096.0) as i32,
                (cell.y * 4096.0) as i32,
                (z * 4096.0) as i32,
            ]) else {
                continue;
            };
            let y = ground as f32 / 4096.0;
            let climb = y - cell.y;
            if climb > MAX_CLIMB || -climb > MAX_FALL {
                continue;
            }
            if visited.contains_key(&key(nx, nz, y)) {
                continue;
            }
            let next = Cell { x, y, z };
            if walks_to(world, cell, next) {
                visited.insert(key(nx, nz, y), y);
                queue.push_back(next);
            }
        }
    }
    let (min_y, max_y) = cells.iter().fold((f32::MAX, f32::MIN), |(min, max), cell| {
        (min.min(cell.y), max.max(cell.y))
    });
    if let Some(target) = target
        && target_reached == Some(false)
    {
        target_reached = Some(
            cells
                .iter()
                .any(|cell| cell.distance(target) < ARRIVE_DISTANCE),
        );
    }
    Ok(Reach {
        start: start_cell,
        cells,
        min_y,
        max_y,
        target,
        target_reached,
    })
}

/// The highest floor at `(x, z)`, for a `--reach X,Z` without `Y`: probes down from the
/// collision bounds' top in `RAY_DEPTH` steps, since [`CollisionWorld::ground`] only looks
/// one unit up and ten down from the height it is given.
pub fn highest_ground(world: &CollisionWorld, x: f32, z: f32) -> Result<f32> {
    let bounds = world_bounds(world).context("the map has no collision geometry")?;
    let mut y = bounds.max_y + 1.0;
    let low = bounds.min_y - MAX_FALL;
    while y > low {
        if let Some(ground) = world.ground([
            (x * 4096.0) as i32,
            (y * 4096.0) as i32,
            (z * 4096.0) as i32,
        ]) {
            return Ok(ground as f32 / 4096.0);
        }
        y -= MAX_FALL.max(10.0);
    }
    anyhow::bail!(
        "no floor at ({x:.3}, ?, {z:.3}) between y {:.3} and {:.3}",
        low,
        bounds.max_y + 1.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::image::fixtures::{floor, wall_x, world};

    fn start(x: f32, z: f32) -> Fx3 {
        [(x * 4096.0) as i32, 0, (z * 4096.0) as i32]
    }

    fn target(x: f32, y: f32, z: f32) -> Fx3 {
        [
            (x * 4096.0) as i32,
            (y * 4096.0) as i32,
            (z * 4096.0) as i32,
        ]
    }

    #[test]
    fn a_flat_floor_is_one_component() {
        let world = world(&floor(2560, 0));
        let reach = flood(&world, start(0.0, 0.0), None).unwrap();
        assert!(reach.cells.len() > 100, "{} cells", reach.cells.len());
        assert!(reach.min_y > 0.09 && reach.max_y < 0.11);
        assert!(
            reach
                .cells
                .iter()
                .all(|cell| cell.x.abs() <= 10.0 && cell.z.abs() <= 10.0)
        );
    }

    #[test]
    fn a_wall_stops_the_flood() {
        let world = world(
            &[
                floor(2560, 0).as_slice(),
                wall_x(512, -2560, 2560, 0, 2048).as_slice(),
            ]
            .concat(),
        );
        let reach = flood(&world, start(-3.0, 0.0), None).unwrap();
        assert!(!reach.cells.is_empty());
        assert!(
            reach.cells.iter().all(|cell| cell.x < 2.0),
            "crossed the wall at x = 2"
        );
        assert!(reach.cells.iter().any(|cell| cell.x < -2.0));
    }

    #[test]
    fn a_continuous_ramp_is_walked() {
        // A floor rising from y = 0 at x = -4 to y = 1 at x = 4 (file units: 1/256).
        let ramp = [
            [[-1024, 0, -1024], [-1024, 0, 1024], [1024, 256, -1024]],
            [[1024, 256, -1024], [-1024, 0, 1024], [1024, 256, 1024]],
        ];
        let world = world(&ramp);
        let reach = flood(&world, start(-3.5, 0.0), None).unwrap();
        assert!(reach.max_y > 0.8, "ramp not climbed: max y {}", reach.max_y);
        assert!(reach.cells.iter().any(|cell| cell.x > 3.0));
        assert!(reach.cells.iter().any(|cell| cell.x < 0.0));
    }

    #[test]
    fn stacked_floors_stay_two_components() {
        let both = [floor(2560, 0).as_slice(), floor(2560, 512).as_slice()].concat();
        let world = world(&both);
        let low = flood(&world, start(0.0, 0.0), None).unwrap();
        assert!(low.max_y < 1.0, "low flood reached y {}", low.max_y);
        let high = flood(&world, target(0.0, 2.0, 0.0), None).unwrap();
        assert!(high.min_y > 1.9, "high flood reached y {}", high.min_y);
        let high_target = target(0.0, 2.2, 0.0);
        let high = flood(&world, target(0.0, 2.0, 0.0), Some(high_target)).unwrap();
        assert_eq!(high.target_reached, Some(true));
        let low_high_target = flood(&world, start(0.0, 0.0), Some(high_target)).unwrap();
        assert_eq!(low_high_target.target_reached, Some(false));
    }

    #[test]
    fn an_ungrounded_point_is_refused() {
        let world = world(&floor(1280, 0));
        let error = flood(&world, start(20.0, 20.0), None).unwrap_err();
        assert!(format!("{error:#}").contains("no floor"));
    }
}
