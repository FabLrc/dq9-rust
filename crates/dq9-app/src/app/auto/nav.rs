//! The walkable grid the autopilot plans its routes on: [`NavGrid`] probes the map's own
//! collision around the places it must reach, keeps every floor of a column, and joins two
//! nodes only when a short walk of the hero's own movement crosses between them.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

use dq9_core::{Hero, Input};
use glam::Vec3;

use super::flat;

/// Floors a grid cell may carry: the game's maps stack walkways over halls.
pub(super) const MAX_FLOORS: usize = 8;

/// A grid of walkable ground around the places a map offers, built with the game's own
/// collision. A cell keeps every floor the map's ground ray finds, so a walkway above a hall is
/// a separate layer and the path may climb the stairs that join them.
pub(super) struct NavGrid {
    origin: Vec3,
    cell: f32,
    width: usize,
    height: usize,
    /// `MAX_FLOORS` heights per cell, lowest first; only `levels[cell]` are set.
    floor: Vec<f32>,
    levels: Vec<u8>,
    /// Per node, one bit per [`STEPS`] direction: the move is not blocked by a wall. The
    /// probe is directional because a ledge lets the hero step down but not up.
    pass: Vec<u8>,
    /// Highest and lowest ground the grid looked for.
    high: f32,
    low: f32,
}

/// Whether the hero may move from `a` to `b`, as the game would move it: a short walk from
/// `a` in that direction must make progress (turn, acceleration, wall slide and floor snap of
/// [`Hero::step`]). A thin wall between two free cell centres, and a ledge crossable one way
/// only, both show here.
fn edge_pass(world: &dq9_core::CollisionWorld, a: Vec3, b: Vec3) -> bool {
    let direction = flat(b - a).normalize_or_zero();
    if direction == Vec3::ZERO {
        return false;
    }
    let yaw = direction.x.atan2(-direction.z);
    let mut hero = Hero::at(a, yaw);
    let pad = Input::from_direction(direction);
    // Walk until the neighbour cell is reached in **3D**: a flat move that merely slides
    // under a higher terrace must not count, and the hero's height must actually reach the
    // neighbour's floor.
    for _ in 0..24 {
        if (hero.position() - b).length() < 0.2 {
            return true;
        }
        hero.step(world, pad, 0, 1);
    }
    (hero.position() - b).length() < 0.3
}

/// The floors of one column: the game's ground ray from `high`, then below the floor it found,
/// until the map's lowest ground or [`MAX_FLOORS`].
fn column_floors(
    world: &dq9_core::CollisionWorld,
    x: f32,
    z: f32,
    high: f32,
    low: f32,
) -> Vec<f32> {
    // Every floor the collision has in this column, not only the piece the game's single
    // ground query keeps: a ramp above a terrace must be visible to the grid. Standing is
    // decided by the directional edge probes (the hero's own walk), not by a static sphere
    // fit: on a ramp edge the sphere is pushed even though the hero climbs it.
    world
        .floors(x, z, low, high)
        .into_iter()
        .take(MAX_FLOORS)
        .collect()
}

/// Direction of the eight d-pad headings on the grid.
const STEPS: [(i32, i32, u32); 8] = [
    (1, 0, 10),
    (-1, 0, 10),
    (0, 1, 10),
    (0, -1, 10),
    (1, 1, 14),
    (1, -1, 14),
    (-1, 1, 14),
    (-1, -1, 14),
];

/// Height the hero may gain or lose between two adjacent cells. A route must stay on one
/// surface: ramps and stairs change the floor continuously (well under a unit per 0.5 cell),
/// so a bigger jump is a stacked walkway, not a step. With 1.25 the pilot planned a route on
/// the lower floor and then pressed at it from the ramp above (X01M01, y 6.05 over 4.25).
const STEP_HEIGHT: f32 = 0.75;

/// The two orthogonal directions that flank a diagonal one (no corner cutting between two
/// walls that the diagonal alone would slip through).
fn diagonal_sides(direction: usize) -> Option<[usize; 2]> {
    Some(match direction {
        4 => [0, 2],
        5 => [0, 3],
        6 => [1, 2],
        7 => [1, 3],
        _ => return None,
    })
}

/// Cell index of a grid coordinate, or `None` outside the grid.
fn cell_at(x: i32, z: i32, width: usize, height: usize) -> Option<usize> {
    (x >= 0 && z >= 0 && (x as usize) < width && (z as usize) < height)
        .then(|| z as usize * width + x as usize)
}

impl NavGrid {
    /// Cells over the box of `points` (margin 6), at 0.5 unit or coarser on large maps.
    pub(super) fn build(
        world: &dq9_core::CollisionWorld,
        points: &[Vec3],
        base_y: f32,
    ) -> Option<Self> {
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for point in points {
            low = low.min(*point);
            high = high.max(*point);
        }
        if !low.is_finite() {
            return None;
        }
        let high_y = points.iter().map(|p| p.y).fold(base_y, f32::max) + 2.0;
        let low_y = points.iter().map(|p| p.y).fold(base_y, f32::min) - 8.0;
        let margin = 6.0;
        low -= Vec3::splat(margin);
        high += Vec3::splat(margin);
        let mut spacing = 0.5;
        loop {
            let width = ((high.x - low.x) / spacing).ceil() as usize + 1;
            let height = ((high.z - low.z) / spacing).ceil() as usize + 1;
            if width.saturating_mul(height) <= 200_000 {
                break;
            }
            spacing *= 2.0;
        }
        let origin = Vec3::new(low.x, 0.0, low.z);
        let width = ((high.x - low.x) / spacing).ceil() as usize + 1;
        let height = ((high.z - low.z) / spacing).ceil() as usize + 1;
        let cells = width * height;
        let at = |cell: usize, level: usize, floor: &[f32]| {
            let index = cell * MAX_FLOORS + level;
            Vec3::new(
                origin.x + (cell % width) as f32 * spacing,
                floor[index],
                origin.z + (cell / width) as f32 * spacing,
            )
        };
        let mut floor = vec![0.0; cells * MAX_FLOORS];
        let mut levels = vec![0u8; cells];
        for iz in 0..height {
            for ix in 0..width {
                let x = origin.x + ix as f32 * spacing;
                let z = origin.z + iz as f32 * spacing;
                let found = column_floors(world, x, z, high_y, low_y);
                let mut kept = 0usize;
                for ground in found {
                    if kept < MAX_FLOORS {
                        floor[(iz * width + ix) * MAX_FLOORS + kept] = ground;
                        kept += 1;
                    }
                }
                levels[iz * width + ix] = kept as u8;
            }
        }
        // Directional edges: a cell centre may be free while a thin wall stands between it
        // and its neighbour, and a ledge is crossable one way only.
        let mut pass = vec![0u8; cells * MAX_FLOORS];
        for iz in 0..height {
            for ix in 0..width {
                let cell = iz * width + ix;
                for level in 0..usize::from(levels[cell]) {
                    let node = cell * MAX_FLOORS + level;
                    for (direction, (dx, dz, _)) in STEPS.iter().enumerate() {
                        let (nx, nz) = (ix as i32 + dx, iz as i32 + dz);
                        let Some(next_cell) = cell_at(nx, nz, width, height) else {
                            continue;
                        };
                        for next_level in 0..usize::from(levels[next_cell]) {
                            let next = next_cell * MAX_FLOORS + next_level;
                            if (floor[next] - floor[node]).abs() <= STEP_HEIGHT
                                && edge_pass(
                                    world,
                                    at(cell, level, &floor),
                                    at(next_cell, next_level, &floor),
                                )
                            {
                                pass[node] |= 1 << direction;
                            }
                        }
                    }
                }
            }
        }
        Some(Self {
            origin,
            cell: spacing,
            width,
            height,
            floor,
            levels,
            pass,
            high: high_y,
            low: low_y,
        })
    }

    pub(super) fn contains(&self, points: &[Vec3]) -> bool {
        points.iter().all(|p| {
            let ix = (p.x - self.origin.x) / self.cell;
            let iz = (p.z - self.origin.z) / self.cell;
            ix >= 0.0
                && iz >= 0.0
                && ix <= (self.width - 1) as f32
                && iz <= (self.height - 1) as f32
                && p.y >= self.low + 1.0
                && p.y <= self.high + 1.0
        })
    }

    pub(super) fn cell(&self, node: usize) -> usize {
        node / MAX_FLOORS
    }

    fn floor_of(&self, node: usize) -> f32 {
        self.floor[node]
    }

    fn levels_of(&self, cell: usize) -> u8 {
        self.levels[cell]
    }

    fn center(&self, node: usize) -> Vec3 {
        let cell = self.cell(node);
        let ix = cell % self.width;
        let iz = cell / self.width;
        Vec3::new(
            self.origin.x + ix as f32 * self.cell,
            self.floor_of(node),
            self.origin.z + iz as f32 * self.cell,
        )
    }

    /// The node nearest to `point` within `radius` world units, preferring its floor.
    pub(super) fn nearest_node(&self, point: Vec3, radius: f32) -> Option<usize> {
        let cx = ((point.x - self.origin.x) / self.cell).round() as i32;
        let cz = ((point.z - self.origin.z) / self.cell).round() as i32;
        let ring = (radius / self.cell).ceil() as i32;
        let mut best: Option<(f32, usize)> = None;
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                let Some(cell) = cell_at(cx + dx, cz + dz, self.width, self.height) else {
                    continue;
                };
                for level in 0..usize::from(self.levels_of(cell)) {
                    let node = cell * MAX_FLOORS + level;
                    let at = self.center(node);
                    let horizontal = (at.x - point.x).powi(2) + (at.z - point.z).powi(2);
                    if horizontal > radius * radius {
                        continue;
                    }
                    let score = horizontal + 4.0 * (at.y - point.y).powi(2);
                    if best.is_none_or(|(b, _)| score < b) {
                        best = Some((score, node));
                    }
                }
            }
        }
        best.map(|(_, node)| node)
    }

    /// Walkable cell centres within `radius` of `point`, nearest first.
    pub(super) fn cells_near(&self, point: Vec3, radius: f32) -> Vec<Vec3> {
        let cx = ((point.x - self.origin.x) / self.cell).round() as i32;
        let cz = ((point.z - self.origin.z) / self.cell).round() as i32;
        let ring = (radius / self.cell).ceil() as i32;
        let mut cells = Vec::new();
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                let Some(cell) = cell_at(cx + dx, cz + dz, self.width, self.height) else {
                    continue;
                };
                for level in 0..usize::from(self.levels_of(cell)) {
                    let at = self.center(cell * MAX_FLOORS + level);
                    let horizontal = (at.x - point.x).powi(2) + (at.z - point.z).powi(2);
                    if horizontal <= radius * radius {
                        cells.push((horizontal + (at.y - point.y).powi(2), at));
                    }
                }
            }
        }
        cells.sort_by(|a, b| a.0.total_cmp(&b.0));
        cells.into_iter().map(|(_, at)| at).collect()
    }

    /// Whether the two points' cells are joined when edges are ignored (debug: a false here
    /// means the walkable area itself is split, not the directional probes).
    pub(super) fn cells_connected(&self, from: Vec3, to: Vec3) -> bool {
        let (Some(start), Some(goal)) = (
            self.nearest_node(from, 2.0).map(|n| self.cell(n)),
            self.nearest_node(to, 3.0).map(|n| self.cell(n)),
        ) else {
            return false;
        };
        let mut seen = vec![false; self.levels.len()];
        let mut queue = std::collections::VecDeque::from([start]);
        seen[start] = true;
        while let Some(cell) = queue.pop_front() {
            if cell == goal {
                return true;
            }
            let (ix, iz) = (cell % self.width, cell / self.width);
            for (dx, dz, _) in STEPS {
                let (nx, nz) = (ix as i32 + dx, iz as i32 + dz);
                let Some(next) = cell_at(nx, nz, self.width, self.height) else {
                    continue;
                };
                if self.levels_of(next) > 0 && !seen[next] {
                    seen[next] = true;
                    queue.push_back(next);
                }
            }
        }
        false
    }

    /// A walkable route from `from` to `to` (cell centres, then `to`), by A* over the floors.
    pub(super) fn plan(
        &self,
        from: Vec3,
        to: Vec3,
        blocked: &BTreeSet<usize>,
    ) -> Option<Vec<Vec3>> {
        let start = self.nearest_node(from, 2.0)?;
        let goal = self.nearest_node(to, 3.0)?;
        // Do not accept a goal cell that is merely near the target: a wall or a chasm between
        // them must fail here, so the caller tries another door or exit.
        let goal_at = self.center(goal);
        if flat(goal_at - to).length() > 0.75 || (goal_at.y - to.y).abs() > 1.2 {
            return None;
        }
        if start == goal {
            return Some(vec![to]);
        }
        let nodes = self.floor.len();
        let mut cost = vec![u32::MAX; nodes];
        let mut came = vec![usize::MAX; nodes];
        cost[start] = 0;
        let goal_cell = self.cell(goal);
        let (gx, gz) = (goal_cell % self.width, goal_cell / self.width);
        let heuristic = |node: usize| {
            let cell = self.cell(node);
            let (x, z) = (cell % self.width, cell / self.width);
            let (dx, dz) = (x.abs_diff(gx), z.abs_diff(gz));
            10 * dx.max(dz) as u32 + 4 * dx.min(dz) as u32
        };
        let mut queue = BinaryHeap::new();
        queue.push(Reverse((heuristic(start), 0u32, start)));
        while let Some(Reverse((_, spent, node))) = queue.pop() {
            if spent > cost[node] {
                continue;
            }
            if node == goal {
                let mut route = vec![to];
                let mut at = node;
                while at != start {
                    route.push(self.center(at));
                    at = came[at];
                }
                route.reverse();
                return Some(route);
            }
            let cell = self.cell(node);
            let (ix, iz) = (cell % self.width, cell / self.width);
            let here = self.floor_of(node);
            let pass = self.pass[node];
            for (direction, (dx, dz, step)) in STEPS.iter().enumerate() {
                if pass & (1 << direction) == 0 {
                    continue;
                }
                if let Some([a, b]) = diagonal_sides(direction)
                    && pass & (1 << a) == 0
                    && pass & (1 << b) == 0
                {
                    continue;
                }
                let Some(next_cell) =
                    cell_at(ix as i32 + dx, iz as i32 + dz, self.width, self.height)
                else {
                    continue;
                };
                for level in 0..usize::from(self.levels_of(next_cell)) {
                    let next = next_cell * MAX_FLOORS + level;
                    if blocked.contains(&next) {
                        continue;
                    }
                    if (self.floor_of(next) - here).abs() > STEP_HEIGHT {
                        continue;
                    }
                    let next_cost = spent.saturating_add(*step);
                    if next_cost < cost[next] {
                        cost[next] = next_cost;
                        came[next] = node;
                        queue.push(Reverse((next_cost + heuristic(next), next_cost, next)));
                    }
                }
            }
        }
        if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
            let mut reachable = 0;
            let (mut min_x, mut max_x, mut min_z, mut max_z) =
                (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
            let mut far = goal;
            let mut far_z = f32::MIN;
            for (node, spent) in cost.iter().enumerate() {
                if *spent == u32::MAX {
                    continue;
                }
                reachable += 1;
                let at = self.center(node);
                min_x = min_x.min(at.x);
                max_x = max_x.max(at.x);
                min_z = min_z.min(at.z);
                max_z = max_z.max(at.z);
                min_y = min_y.min(at.y);
                max_y = max_y.max(at.y);
                if at.z > far_z {
                    far_z = at.z;
                    far = node;
                }
            }
            let far_at = self.center(far);
            let goal_cell = self.cell(goal);
            let mut levels = String::new();
            for level in 0..usize::from(self.levels_of(goal_cell)) {
                let node = goal_cell * MAX_FLOORS + level;
                levels.push_str(&format!(
                    "{level}:{:.2}{} ",
                    self.floor_of(node),
                    if cost[node] != u32::MAX { "*" } else { "" }
                ));
            }
            eprintln!(
                "pilot: no route {from:?} -> {to:?} (start {start}, goal {goal} floors {levels}, {reachable} nodes, x {min_x:.1}..{max_x:.1}, y {min_y:.1}..{max_y:.1}, z {min_z:.1}..{max_z:.1}, farthest z at {far_at:?})"
            );
        }
        None
    }
}
