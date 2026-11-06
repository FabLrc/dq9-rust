//! `--headless --auto`: plays the story without a window, pressing the action key to read the
//! messages, until the player would get control (no event runs or waits) or the run is stuck,
//! and prints what the story did (`Journal`). With `--pilot`, a free control is not the end:
//! the autopilot walks to the conversation, area or door that moves the story on.
//!
//! The pilot uses the game's own movement: every frame it feeds the d-pad a world direction
//! (the keys a player would hold), the hero turns and accelerates (`Hero::step`), the map's
//! spheres and floor push it (`CollisionWorld`), and only [`Field::step`] decides that a door,
//! an area or a talk zone was reached. It never moves the hero: routes come from a walkable
//! grid built by probing the map's collision with the hero's own radius, and the action key is
//! pressed with [`Talk::press`] once `Talk_NearestNpc` sees the intended NPC. What it walks
//! to is what the game offers: the standing points of the partner search, the areas of the
//! `.bmbl`, and the map's warps and doorways.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::hash::{DefaultHasher, Hash, Hasher};

use dq9_core::collision::{Body, Mode};
use dq9_core::field::{Event, Exit, Leader, exit_contains};
use dq9_core::movement::{HEIGHT, RADIUS};
use dq9_core::scenario::Step;
use dq9_core::talk::Target;
use dq9_core::trigger::Request;
use dq9_core::trigger::State;
use dq9_core::{Hero, Input};
use glam::Vec3;

use super::{App, status};
use crate::talk::{Button, Partner};

/// Steps between two presses of the action key while a scene runs (a message needs its text
/// typed before the key advances it).
const PRESS_EVERY: u64 = 20;
/// Things the autopilot may do in one run.
const MAX_ACTIONS: u32 = 300;
/// Distance under which a waypoint counts as reached.
const WAYPOINT_REACH: f32 = 0.55;
/// Frames of walking without progress before the route is planned again, then given up.
const REPLAN_FRAMES: u32 = 45;
const STUCK_FRAMES: u32 = 120;
/// Frames the pilot may spend turning to a partner before it gives up.
const TURN_FRAMES: u32 = 90;
/// Frames spent walking into a doorway before the exit counts as taken.
const DOOR_FRAMES: u32 = 20;

/// Why [`App::play_auto`] stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// No event runs or waits and the autopilot, if any, has nothing left to try: the player
    /// has control on this map.
    Control { map: u16 },
    /// A walking goal could not be reached.
    Stuck { map: u16, why: String },
    /// The step limit was reached with a scene still running.
    Limit,
    /// The game was never started (no ROM, or the first scene did not load).
    NotStarted,
}

/// One thing the pilot decided to do, with the point it walks to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Goal {
    /// Talk to `target` from `position`, facing roughly `yaw`.
    Talk {
        target: Target,
        position: Vec3,
        yaw: f32,
    },
    /// Step into area `index` at `point`.
    Area { index: usize, point: Vec3 },
    /// Leave by a warp or a doorway; `heading` is the direction to walk into it (zero for a
    /// warp, whose volume triggers by itself).
    Exit {
        destination: Option<u16>,
        position: Vec3,
        heading: Vec3,
        /// The doorway's `(class, number)` when this is an internal door (no destination).
        door: Option<(u8, u8)>,
    },
}

impl Goal {
    fn point(&self) -> Vec3 {
        match *self {
            Self::Talk { position, .. } => position,
            Self::Area { point, .. } => point,
            Self::Exit { position, .. } => position,
        }
    }
}

/// What the autopilot remembers.
#[derive(Default)]
struct Pilot {
    /// Conversations had: `(room, npc, context, story digest)`; one is not had twice at the
    /// same point of the story.
    talked: BTreeSet<(u16, u16, u16, u64)>,
    /// Exits taken: `(room, destination, story digest)`.
    left: BTreeSet<(u16, u16, u64)>,
    /// Areas walked into: `(room, area index, story digest)`.
    entered: BTreeSet<(u16, usize, u64)>,
    /// The story digest at which each room was last looked at for something to do.
    evaluated: BTreeMap<u16, u64>,
    /// The rooms each room leads to (seen so far).
    graph: BTreeMap<u16, Vec<u16>>,
    /// The rooms the hero was in.
    visited: BTreeSet<u16>,
    /// The room and story digest the current map was built for.
    built: Option<(u16, u64)>,
    actions: u32,
    /// The goal being walked to, and the room and story it was chosen for.
    goal: Option<Goal>,
    goal_room: u16,
    goal_digest: u64,
    route: Vec<Vec3>,
    route_at: usize,
    exit_frames: u32,
    turn_frames: u32,
    stall: u32,
    last_position: Option<Vec3>,
    /// Best distance to the current waypoint (progress, not mere jitter).
    best_waypoint: f32,
    replans: u32,
    stuck: Option<String>,
    /// Walkable grids by room (the collision does not change with the story).
    grids: BTreeMap<u16, NavGrid>,
    /// Doorways already opened: `(class, number)`.
    opened: BTreeSet<(u8, u8)>,
    /// Fixed steps to let a door finish opening before choosing again.
    wait: u32,
    /// Completed doors the walkable grid was built with.
    grid_doors: usize,
    /// Cells the walk could not leave: `(room, cell)`, learned when stuck.
    blocked: BTreeSet<(u16, usize)>,
}

/// Floors a grid cell may carry: the game's maps stack walkways over halls.
const MAX_FLOORS: usize = 8;

/// A grid of walkable ground around the places a map offers, built with the game's own
/// collision. A cell keeps every floor the map's ground ray finds, so a walkway above a hall is
/// a separate layer and the path may climb the stairs that join them.
struct NavGrid {
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

/// Whether the hero's sphere fits at a known floor (the map pushes it by less than five
/// centimetres).
fn fits(world: &dq9_core::CollisionWorld, x: f32, z: f32, ground: f32) -> bool {
    let to_fx = |v: f32| (v * 4096.0) as i32;
    let start = [to_fx(x), to_fx(ground), to_fx(z)];
    let mut body = Body {
        position: start,
        fall: 0,
        radius: RADIUS,
        height: HEIGHT,
        wall: None,
    };
    let outcome = world.resolve(&mut body, start, Mode::Whole);
    if !outcome.grounded {
        return false;
    }
    let dx = (body.position[0] - start[0]) as f32 / 4096.0;
    let dz = (body.position[2] - start[2]) as f32 / 4096.0;
    dx * dx + dz * dz < 0.0025
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
    // Walk until the neighbour cell is reached: a wedged move slides along the wall and
    // never arrives, however far it travels.
    for _ in 0..24 {
        if flat(hero.position() - b).length() < 0.12 {
            return true;
        }
        hero.step(world, pad, 0, 1);
    }
    flat(hero.position() - b).length() < 0.25
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
    let mut floors: Vec<f32> = Vec::new();
    let mut base = high;
    // The ground ray reaches ten units below its base: when nothing is in reach, lower the
    // base instead of giving up (a cell high on a tower must still see the floors below it).
    // Steps are close together, so probe just below each floor found rather than jumping.
    while base > low - 1.0 && floors.len() < MAX_FLOORS {
        match world.ground_height(x, z, base) {
            Some(ground) if ground >= low - 1.0 => {
                let lower = floors.last().is_none_or(|last| ground < last - 0.05);
                if lower {
                    floors.push(ground);
                    base = ground - 1.2;
                } else {
                    base -= 2.0;
                }
            }
            _ => base -= 8.0,
        }
    }
    floors.sort_by(|a, b| a.total_cmp(b));
    floors
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

/// Height the hero may gain between two adjacent cells (stairs and ramps).
const STEP_HEIGHT: f32 = 1.25;

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
    fn build(world: &dq9_core::CollisionWorld, points: &[Vec3], base_y: f32) -> Option<Self> {
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
                    if kept < MAX_FLOORS && fits(world, x, z, ground) {
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

    fn contains(&self, points: &[Vec3]) -> bool {
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

    fn cell(&self, node: usize) -> usize {
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

    fn cell_xy(&self, x: i32, z: i32) -> Option<usize> {
        (x >= 0 && z >= 0 && (x as usize) < self.width && (z as usize) < self.height)
            .then(|| z as usize * self.width + x as usize)
    }

    /// The node nearest to `point` within `radius` world units, preferring its floor.
    fn nearest_node(&self, point: Vec3, radius: f32) -> Option<usize> {
        let cx = ((point.x - self.origin.x) / self.cell).round() as i32;
        let cz = ((point.z - self.origin.z) / self.cell).round() as i32;
        let ring = (radius / self.cell).ceil() as i32;
        let mut best: Option<(f32, usize)> = None;
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                let Some(cell) = self.cell_xy(cx + dx, cz + dz) else {
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
    fn cells_near(&self, point: Vec3, radius: f32) -> Vec<Vec3> {
        let cx = ((point.x - self.origin.x) / self.cell).round() as i32;
        let cz = ((point.z - self.origin.z) / self.cell).round() as i32;
        let ring = (radius / self.cell).ceil() as i32;
        let mut cells = Vec::new();
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                let Some(cell) = self.cell_xy(cx + dx, cz + dz) else {
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
    fn cells_connected(&self, from: Vec3, to: Vec3) -> bool {
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
    fn plan(&self, from: Vec3, to: Vec3, blocked: &BTreeSet<usize>) -> Option<Vec<Vec3>> {
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
                let Some(next_cell) = self.cell_xy(ix as i32 + dx, iz as i32 + dz) else {
                    continue;
                };
                if blocked.contains(&next_cell) {
                    continue;
                }
                for level in 0..usize::from(self.levels_of(next_cell)) {
                    let next = next_cell * MAX_FLOORS + level;
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

/// The closed internal doorway (type 2, no destination) nearest to the straight path from
/// `from` to `target`: opening it may join the two sides of a wall. Doors already opened are
/// skipped.
fn door_towards(
    exits: &[Exit],
    from: Vec3,
    target: Vec3,
    opened: &BTreeSet<(u8, u8)>,
) -> Option<Exit> {
    let a = flat(from);
    let b = flat(target);
    let segment = b - a;
    let length_sq = segment.length_squared();
    let mut candidates: Vec<(f32, Exit)> = exits
        .iter()
        .filter(|exit| {
            exit.destination.is_none()
                && exit.door.is_some()
                && !opened.contains(&(exit.class, exit.number))
        })
        .map(|exit| {
            let point = flat(exit.position);
            let along = if length_sq > 0.0 {
                ((point - a).dot(segment) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let near = a + segment * along;
            ((point - near).length(), *exit)
        })
        .collect();
    candidates.sort_by(|x, y| x.0.total_cmp(&y.0));
    candidates
        .into_iter()
        .find(|(distance, _)| *distance < 6.0)
        .map(|(_, exit)| exit)
}

/// The world direction of a [`Hero`] yaw (0 faces `-z`).
fn facing(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, -yaw.cos())
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

impl App {
    /// A hash of where the story is: the progress of every track, their flags and the global
    /// flags.
    fn story_digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for track in &self.state.tracks {
            (track.chapter, track.step, track.substep).hash(&mut hasher);
            track.flags.hash(&mut hasher);
        }
        self.state.flags.hash(&mut hasher);
        hasher.finish()
    }

    /// Runs the game logic for at most `max_steps` fixed steps; stops at the first moment the
    /// player would get control after a scene (`Stop::Control`), or, with `pilot`, when the
    /// autopilot finds nothing that moves the story or cannot walk to it.
    pub fn play_auto(&mut self, max_steps: u64, pilot: bool) -> Stop {
        let mut started = false;
        let mut bot = Pilot::default();
        for _ in 0..max_steps {
            let busy = self.events.is_active() || self.events.pending_start().is_some();
            if (busy || self.talk.is_active()) && self.tick.is_multiple_of(PRESS_EVERY) {
                self.talk.press(Button::Confirm);
            }
            if pilot {
                self.pilot_frame(&mut bot);
                if let Some(why) = bot.stuck.take() {
                    let map = self.current.as_ref().map_or(0, |(e, _)| e.id);
                    self.journal.record(self.tick, Step::Unhandled(why.clone()));
                    return Stop::Stuck { map, why };
                }
            }
            self.run_steps(1);
            let busy = self.events.is_active() || self.events.pending_start().is_some();
            started |= busy;
            if started && !busy && !self.talk.is_active() && !self.talk.confirm_pending() {
                let map = self.current.as_ref().map_or(0, |(e, _)| e.id);
                let track = &self.state.tracks[self.state.track];
                let story = (track.chapter, track.step, track.substep);
                let last = self.journal.steps.iter().rev().find_map(|(_, s)| match s {
                    Step::Control { map, story } => Some((*map, *story)),
                    _ => None,
                });
                // The pilot crosses rooms: one line per story point while it works, and one
                // for the place where it stops.
                if last.is_none_or(|(_, s)| s != story) {
                    self.journal.record(self.tick, Step::Control { map, story });
                }
                if pilot {
                    if bot.goal.is_some() {
                        continue;
                    }
                    if bot.wait > 0 {
                        bot.wait -= 1;
                        continue;
                    }
                    if self.pursue(&mut bot) {
                        continue;
                    }
                }
                if last.is_some_and(|(m, s)| s == story && m != map) {
                    self.journal.record(self.tick, Step::Control { map, story });
                }
                return Stop::Control { map };
            }
        }
        if started {
            Stop::Limit
        } else {
            Stop::NotStarted
        }
    }

    /// Rebuilds the walkable grid of `room` when the places of this map are not all inside it.
    fn ensure_grid(&self, bot: &mut Pilot, room: u16, points: &[Vec3], base_y: f32) {
        if bot
            .grids
            .get(&room)
            .is_none_or(|grid| !grid.contains(points))
            && let Some(grid) = NavGrid::build(&self.collision, points, base_y)
        {
            bot.blocked.retain(|(at, _)| *at != room);
            bot.grids.insert(room, grid);
        }
    }

    /// A route through the grid of `room`.
    fn route(&self, bot: &Pilot, room: u16, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let blocked: BTreeSet<usize> = bot
            .blocked
            .iter()
            .filter(|(at, _)| *at == room)
            .map(|(_, cell)| *cell)
            .collect();
        bot.grids.get(&room)?.plan(from, to, &blocked)
    }

    /// At a free control: has the conversation, steps into the area or leaves by the exit that
    /// moves the story most when a real walk reaches it. Returns whether a goal was started.
    fn pursue(&mut self, bot: &mut Pilot) -> bool {
        let Some((entry, _)) = &self.current else {
            return false;
        };
        let room = entry.id;
        bot.visited.insert(room);
        if let Some(field) = &self.field {
            let exits = field.exits();
            let towards = exits
                .iter()
                .filter_map(|e| e.destination)
                .filter(|d| self.same_family(*d))
                .collect::<BTreeSet<_>>();
            bot.graph.insert(room, towards.into_iter().collect());
        }
        let digest = self.story_digest();
        if bot.actions >= MAX_ACTIONS {
            return false;
        }
        // The cast and the triggers of the map follow the story: build it again when the story
        // moved since (the game reloads its triggers on progress, `Request::ReloadTriggers`).
        if bot.built != Some((room, digest)) {
            if let Some(hero) = &self.hero {
                let spawn = (hero.position(), hero.yaw());
                self.quiet_events = true;
                self.load_map(&room.to_string(), false, Some(spawn), None, false);
                self.quiet_events = false;
                // The same map again is not a step of the story.
                if matches!(self.journal.steps.last(), Some((_, Step::Map { id, .. })) if *id == room)
                {
                    self.journal.steps.pop();
                }
            }
            bot.built = Some((room, digest));
            if self.events.is_active() || self.events.pending_start().is_some() {
                return true;
            }
        }
        let Some(hero) = self.hero else {
            return false;
        };
        let from = hero.position();
        let Some(field) = &self.field else {
            return false;
        };
        // A door that finished opening changed the collision: the walkable graph must be
        // probed again, or the grid keeps the closed door as a wall.
        let done = self
            .events
            .doors()
            .iter()
            .filter(|door| door.angle >= door.target)
            .count();
        if bot.grid_doors != done {
            bot.grids.remove(&room);
            bot.blocked.retain(|(at, _)| *at != room);
            bot.grid_doors = done;
        }
        let exits = field.exits();
        let areas = field.area_points();
        let partners = self.talk.partners();
        let mut points = vec![from];
        points.extend(exits.iter().map(|e| e.position));
        points.extend(areas.iter().copied());
        points.extend(partners.iter().map(|p| p.position));
        self.ensure_grid(bot, room, &points, from.y);

        // What the map offers, by how much it moves the story: conversations first on a tie.
        enum Offer {
            Talk(Partner),
            Area { index: usize, point: Vec3 },
        }
        let mut offers: Vec<(u32, u8, Offer)> = Vec::new();
        for partner in partners {
            if bot
                .talked
                .contains(&(room, partner.npc, partner.context, digest))
            {
                continue;
            }
            let (events, after) = self.talk.simulate(&partner, field, &self.state);
            let (score, _) = self.rate(&events, &after);
            if score > 0 {
                offers.push((score, 0, Offer::Talk(partner)));
            }
        }
        for (index, point) in areas.iter().enumerate() {
            if bot.entered.contains(&(room, index, digest)) {
                continue;
            }
            let (mut field, mut state) = (field.clone(), self.state.clone());
            let leader = Leader {
                position: *point,
                yaw: 0.0,
                input: Vec3::ZERO,
            };
            let events = field.step(&leader, &mut state);
            let (score, what) = self.rate(&events, &state);
            if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
                eprintln!(
                    "pilot: map {room} area {index} at {point:?}: score {score} {what} {events:?}"
                );
            }
            if score > 0 {
                offers.push((
                    score,
                    1,
                    Offer::Area {
                        index,
                        point: *point,
                    },
                ));
            }
        }
        offers.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut unreachable_target: Option<Vec3> = None;
        if std::env::var_os("DQ9_PILOT_DEBUG").is_some() && !offers.is_empty() {
            let list: Vec<String> = offers
                .iter()
                .map(|(score, _kind, offer)| match offer {
                    Offer::Talk(p) => format!("{score}:talk {}/{}", p.npc, p.context),
                    Offer::Area { index, .. } => format!("{score}:area {index}"),
                })
                .collect();
            eprintln!("pilot: map {room} offers [{}]", list.join(", "));
        }
        for (_, _, offer) in offers {
            match offer {
                Offer::Talk(partner) => {
                    let target = Target {
                        npc: partner.npc,
                        context: partner.context,
                    };
                    if let Some((position, route)) = self.talk_spot(bot, room, from, &partner) {
                        bot.talked
                            .insert((room, partner.npc, partner.context, digest));
                        bot.actions += 1;
                        let what = format!(
                            "walks to NPC {} (context {}) on map {room} at {position:?}",
                            partner.npc, partner.context
                        );
                        status(&what);
                        self.journal.record(self.tick, Step::Autopilot(what));
                        self.start_goal(
                            bot,
                            room,
                            digest,
                            route,
                            Goal::Talk {
                                target,
                                position,
                                yaw: partner.yaw,
                            },
                        );
                        return true;
                    }
                    unreachable_target.get_or_insert(partner.position);
                }
                Offer::Area { index, point } => {
                    let goal = (|| {
                        let spots = std::iter::once(point).chain(
                            bot.grids
                                .get(&room)?
                                .cells_near(point, 2.0)
                                .into_iter()
                                .filter(|at| field.area_contains(index, *at)),
                        );
                        for spot in spots {
                            if let Some(route) = self.route(bot, room, from, spot) {
                                return Some((spot, route));
                            }
                        }
                        None
                    })();
                    if goal.is_none() {
                        unreachable_target.get_or_insert(point);
                    }
                    if goal.is_none() && std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
                        let cells = bot.grids.get(&room).map(|grid| {
                            (
                                grid.cells_near(point, 2.0)
                                    .iter()
                                    .filter(|at| field.area_contains(index, **at))
                                    .count(),
                                grid.cells_connected(from, point),
                            )
                        });
                        eprintln!(
                            "pilot: area {index} on map {room} has no walkable spot (cells in area/connected: {cells:?}) from {from:?}"
                        );
                    }
                    if let Some((spot, route)) = goal {
                        bot.entered.insert((room, index, digest));
                        bot.actions += 1;
                        let what = format!("walks into area {index} of map {room} at {spot:?}");
                        status(&what);
                        self.journal.record(self.tick, Step::Autopilot(what));
                        self.start_goal(
                            bot,
                            room,
                            digest,
                            route,
                            Goal::Area { index, point: spot },
                        );
                        return true;
                    }
                }
            }
        }
        // A closed internal door stands between us and the place to reach: walk into it and
        // let the field open it (native 584), then choose again.
        if let Some(target) = unreachable_target
            && let Some(field) = &self.field
            && let Some(exit) = door_towards(&field.exits(), from, target, &bot.opened)
            && let Some((spot, route)) = self.exit_spot(bot, room, from, &exit)
        {
            bot.opened.insert((exit.class, exit.number));
            bot.actions += 1;
            let what = format!(
                "walks into the door {}/{} of map {room} at {spot:?}",
                exit.class, exit.number
            );
            status(&what);
            self.journal.record(self.tick, Step::Autopilot(what));
            self.start_goal(
                bot,
                room,
                digest,
                route,
                Goal::Exit {
                    destination: None,
                    position: spot,
                    heading: exit.heading,
                    door: Some((exit.class, exit.number)),
                },
            );
            return true;
        }
        // Nothing to say or step into here: take an exit not taken yet, unvisited first.
        let Some(field) = &self.field else {
            return false;
        };
        bot.evaluated.insert(room, digest);
        let exits = field.exits();
        let mut untried: Vec<Exit> = exits
            .iter()
            .filter(|exit| {
                exit.destination
                    .is_some_and(|d| self.same_family(d) && !bot.left.contains(&(room, d, digest)))
            })
            .copied()
            .collect();
        // Unvisited rooms first, in the order of the map's exits; every exit to a destination
        // is tried, since one may be on an unreachable ledge while another is walkable.
        untried.sort_by_key(|exit| exit.destination.is_some_and(|d| bot.visited.contains(&d)));
        for exit in untried {
            let Some(destination) = exit.destination else {
                continue;
            };
            if let Some((spot, route)) = self.exit_spot(bot, room, from, &exit) {
                bot.left.insert((room, destination, digest));
                bot.actions += 1;
                let what = format!("walks out of map {room} for {destination}");
                status(&what);
                self.journal.record(self.tick, Step::Autopilot(what));
                self.start_goal(
                    bot,
                    room,
                    digest,
                    route,
                    Goal::Exit {
                        destination: Some(destination),
                        position: spot,
                        heading: exit.heading,
                        door: None,
                    },
                );
                return true;
            }
        }
        // Nothing left here: head for the nearest room that has something left to try.
        let Some(hop) = self.next_hop(bot, room, digest) else {
            return false;
        };
        let Some(field) = &self.field else {
            return false;
        };
        for exit in field
            .exits()
            .into_iter()
            .filter(|e| e.destination == Some(hop))
        {
            let Some((spot, route)) = self.exit_spot(bot, room, from, &exit) else {
                continue;
            };
            bot.actions += 1;
            let what = format!("walks back through map {room} to {hop}");
            status(&what);
            self.journal.record(self.tick, Step::Autopilot(what));
            self.start_goal(
                bot,
                room,
                digest,
                route,
                Goal::Exit {
                    destination: Some(hop),
                    position: spot,
                    heading: exit.heading,
                    door: None,
                },
            );
            return true;
        }
        status(&format!("no walkable way back through map {room} to {hop}"));
        false
    }

    /// A reachable standing point of `partner`: the partner search's own point, else a walkable
    /// cell near it where `Talk_NearestNpc` still picks this NPC and context.
    fn talk_spot(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        partner: &Partner,
    ) -> Option<(Vec3, Vec<Vec3>)> {
        let target = Target {
            npc: partner.npc,
            context: partner.context,
        };
        let grid = bot.grids.get(&room)?;
        let cells = grid.cells_near(partner.position, 2.5);
        let mut same_floor = 0;
        for spot in cells {
            // Same floor as the NPC: the game's partner search is a 3D distance, ours reads
            // the placement heights, so the walk must end on the NPC's walkway.
            if (spot.y - partner.position.y).abs() > 1.0 {
                continue;
            }
            same_floor += 1;
            if self.talk.nearest(spot, partner.yaw) != Some(target) {
                continue;
            }
            if let Some(route) = self.route(bot, room, from, spot) {
                return Some((spot, route));
            }
        }
        if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
            eprintln!(
                "pilot: no talk spot for NPC {} (context {}) on map {room} at {:?} from {from:?}: {same_floor} cells on its floor",
                partner.npc, partner.context, partner.position
            );
        }
        None
    }

    /// A reachable approach point of `exit`: the warp itself, or the ground in front of a
    /// doorway; once there the pilot walks into it.
    fn exit_spot(
        &self,
        bot: &Pilot,
        room: u16,
        from: Vec3,
        exit: &Exit,
    ) -> Option<(Vec3, Vec<Vec3>)> {
        let grid = bot.grids.get(&room)?;
        // Inside the trigger volume (a warp above a hall is not reached by standing under it,
        // and a doorway only fires where the volume is).
        let inside: Vec<Vec3> = grid
            .cells_near(exit.position, 3.5)
            .into_iter()
            .filter(|spot| exit_contains(exit, *spot))
            .collect();
        let mut spots = inside;
        if exit.heading != Vec3::ZERO {
            // A doorway reacts to a hero walking against its facing: start in front of it when
            // no walkable cell lies inside.
            let heading = flat(exit.heading).normalize_or_zero();
            spots.extend(
                grid.cells_near(exit.position, 3.0)
                    .into_iter()
                    .filter(|spot| flat(*spot - exit.position).dot(heading) <= 0.2),
            );
        }
        let near = grid.cells_near(exit.position, 3.5);
        let count = near
            .iter()
            .filter(|spot| exit_contains(exit, **spot))
            .count();
        for spot in spots {
            if let Some(route) = self.route(bot, room, from, spot) {
                return Some((spot, route));
            }
        }
        status(&format!(
            "exit {exit:?}: {} walkable cells near, {count} inside the volume, from {from:?}",
            near.len()
        ));
        None
    }

    fn start_goal(&self, bot: &mut Pilot, room: u16, digest: u64, route: Vec<Vec3>, goal: Goal) {
        bot.goal = Some(goal);
        bot.goal_room = room;
        bot.goal_digest = digest;
        bot.route = route;
        bot.route_at = 0;
        bot.exit_frames = 0;
        bot.turn_frames = 0;
        bot.stall = 0;
        bot.last_position = None;
        bot.best_waypoint = f32::INFINITY;
        bot.replans = 0;
    }

    /// One fixed step of walking towards the pilot's goal: feeds the d-pad, presses the action
    /// key when the game's partner search sees the target, and clears the goal when it is
    /// reached (the same step lets the field fire the trigger).
    fn pilot_frame(&mut self, bot: &mut Pilot) {
        self.pilot_dir = None;
        let Some(goal) = bot.goal else {
            return;
        };
        let room = self.current.as_ref().map_or(0, |(e, _)| e.id);
        if room != bot.goal_room || self.story_digest() != bot.goal_digest {
            // A scene or a trigger moved the story while walking: choose again.
            bot.goal = None;
            return;
        }
        let Some(hero) = self.hero else {
            bot.goal = None;
            return;
        };
        let position = hero.position();
        // An area is entered as soon as the hero is inside it, not only at its centre.
        if let Goal::Area { index, .. } = goal
            && self
                .field
                .as_ref()
                .is_some_and(|field| field.area_contains(index, position))
        {
            bot.goal = None;
            return;
        }
        // Talk as soon as the game itself would: the action key reads the same partner search,
        // on the NPC's floor (see `talk_spot`).
        if let Goal::Talk {
            target,
            position: end,
            ..
        } = goal
            && (position.y - end.y).abs() < 1.0
            && self.talk.nearest(position, hero.yaw()) == Some(target)
        {
            status(&format!(
                "autopilot: presses the action key for NPC {} (context {})",
                target.npc, target.context
            ));
            self.talk.press(Button::Confirm);
            bot.goal = None;
            return;
        }
        // Stuck: no progress towards the next waypoint (jitter against a wall is not
        // progress, even though the position changes every frame).
        let next = bot.route.get(bot.route_at).copied().unwrap_or(goal.point());
        let distance = flat(position - next).length();
        let moved = bot
            .last_position
            .is_none_or(|last| flat(position - last).length() > 0.01);
        if moved && distance < bot.best_waypoint - 0.02 {
            bot.best_waypoint = distance;
            bot.stall = 0;
            bot.last_position = Some(position);
        } else {
            if moved {
                bot.last_position = Some(position);
            }
            bot.stall += 1;
        }
        if bot.stall > REPLAN_FRAMES && bot.replans < 8 {
            // The collision probe let this cell through but the walk wedges here: forbid
            // entering it and plan around it.
            if let Some(grid) = bot.grids.get(&room)
                && let Some(step) = bot.route.get(bot.route_at).copied()
                && let Some(node) = grid.nearest_node(step, 1.0)
                && bot.blocked.len() < 256
            {
                bot.blocked.insert((room, grid.cell(node)));
            }
            bot.route = self
                .route(bot, room, position, goal.point())
                .unwrap_or_default();
            bot.route_at = 0;
            bot.best_waypoint = f32::INFINITY;
            bot.replans += 1;
        }
        if bot.stall > STUCK_FRAMES {
            let next = bot.route.get(bot.route_at).copied();
            bot.stuck = Some(format!(
                "pilot stuck on map {room} at {position:?} walking to {:?} \
                 (waypoint {:?} of {} at {next:?})",
                goal.point(),
                bot.route_at,
                bot.route.len()
            ));
            bot.goal = None;
            return;
        }
        // Reached the end of the route (or the last cell): act.
        let end = goal.point();
        let mut at_end = flat(position - end).length() < WAYPOINT_REACH;
        if matches!(goal, Goal::Talk { .. }) {
            at_end &= (position.y - end.y).abs() < 1.0;
        }
        while bot.route_at < bot.route.len() {
            let waypoint = bot.route[bot.route_at];
            if flat(position - waypoint).length() < WAYPOINT_REACH {
                bot.route_at += 1;
                bot.best_waypoint = f32::INFINITY;
            } else {
                break;
            }
        }
        if !at_end && bot.route_at >= bot.route.len() && bot.replans < 8 {
            // The route stopped short (a floor the grid did not join when it was planned):
            // plan again from here before giving up.
            if let Some(route) = self.route(bot, room, position, end) {
                bot.route = route;
                bot.route_at = 0;
                bot.replans += 1;
            }
        }
        if !at_end && bot.route_at >= bot.route.len() && bot.replans >= 8 {
            status(&format!(
                "autopilot: cannot reach {end:?} from {position:?} on map {room}"
            ));
            match goal {
                Goal::Exit {
                    destination: Some(destination),
                    ..
                } => {
                    bot.left.insert((room, destination, bot.goal_digest));
                }
                Goal::Exit {
                    door: Some((class, number)),
                    ..
                } => {
                    if self.events.doors().iter().any(|door| {
                        door.block == i32::from(class) && door.placement == i32::from(number)
                    }) {
                        bot.wait = 40;
                    } else {
                        bot.opened.remove(&(class, number));
                    }
                }
                _ => {}
            };
            bot.goal = None;
            return;
        }
        if at_end || bot.route_at >= bot.route.len() {
            match goal {
                Goal::Talk { yaw, .. } => {
                    // Turn to face the NPC: the partner search weights the facing direction.
                    bot.turn_frames += 1;
                    if bot.turn_frames > TURN_FRAMES {
                        status(&format!(
                            "autopilot: cannot face NPC at {end:?} from {position:?}"
                        ));
                        bot.goal = None;
                        return;
                    }
                    self.pilot_dir = Some(facing(yaw));
                }
                Goal::Area { .. } => {
                    // Standing inside the area is enough: the field fires event 2 this step.
                    bot.goal = None;
                }
                Goal::Exit {
                    destination,
                    heading,
                    ..
                } => {
                    bot.exit_frames += 1;
                    if bot.exit_frames > DOOR_FRAMES {
                        match destination {
                            Some(destination) => {
                                status(&format!("autopilot: no way out at {end:?}"));
                                bot.left.insert((room, destination, bot.goal_digest));
                            }
                            // A door without destination only opens; let it finish.
                            None => bot.wait = 40,
                        }
                        bot.goal = None;
                        return;
                    }
                    self.pilot_dir = Some(heading);
                }
            }
            return;
        }
        // A dense grid path can put a waypoint at a corner the hero cannot reach exactly;
        // look one cell ahead once progress stalls, as a player would.
        let look = if bot.stall > 15 && bot.route_at + 1 < bot.route.len() {
            bot.route_at + 1
        } else {
            bot.route_at
        };
        let waypoint = bot.route[look];
        let direction = flat(waypoint - position).normalize_or_zero();
        // Wedged diagonally against a corner: press one axis at a time, as a player would,
        // alternating the axis when the first one does not move either.
        if bot.stall > 10 && direction.x != 0.0 && direction.z != 0.0 {
            let (major, minor) = if direction.x.abs() >= direction.z.abs() {
                (
                    Vec3::new(direction.x.signum(), 0.0, 0.0),
                    Vec3::new(0.0, 0.0, direction.z.signum()),
                )
            } else {
                (
                    Vec3::new(0.0, 0.0, direction.z.signum()),
                    Vec3::new(direction.x.signum(), 0.0, 0.0),
                )
            };
            self.pilot_dir = Some(if (bot.stall / 25).is_multiple_of(2) {
                major
            } else {
                minor
            });
        } else {
            self.pilot_dir = Some(direction);
        }
    }

    /// The first room on the shortest known way from `room` to a room the pilot has not looked
    /// at in this state of the story, or that still has an exit to try.
    fn next_hop(&self, bot: &Pilot, room: u16, digest: u64) -> Option<u16> {
        let pending = |r: u16| {
            bot.evaluated.get(&r) != Some(&digest)
                || bot
                    .graph
                    .get(&r)
                    .is_some_and(|exits| exits.iter().any(|d| !bot.left.contains(&(r, *d, digest))))
        };
        let mut parent = BTreeMap::new();
        let mut queue = std::collections::VecDeque::from([room]);
        while let Some(at) = queue.pop_front() {
            for &next in bot.graph.get(&at).into_iter().flatten() {
                if next == room || parent.contains_key(&next) {
                    continue;
                }
                parent.insert(next, at);
                if pending(next) {
                    let mut hop = next;
                    while parent[&hop] != room {
                        hop = parent[&hop];
                    }
                    return Some(hop);
                }
                queue.push_back(next);
            }
        }
        None
    }

    /// Whether map `id` is in the family of the current one (the first three letters of the
    /// codes: `X01`, `M01`): the pilot stays in the place the story is told in.
    fn same_family(&self, id: u16) -> bool {
        let family = |id: u16| {
            self.maps
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.code.get(..3).map(str::to_owned))
        };
        let here = self.current.as_ref().and_then(|(e, _)| family(e.id));
        here.is_some() && here == family(id)
    }

    /// How much a conversation or a step into an area moves the story: what it does to the
    /// progress, the events it starts, the map it changes and the flags it raises.
    fn rate(&self, events: &[Event], after: &State) -> (u32, String) {
        let track = &self.state.tracks[self.state.track];
        let after_track = &after.tracks[after.track];
        let mut score = 0;
        let mut what = Vec::new();
        if (track.chapter, track.step, track.substep)
            != (after_track.chapter, after_track.step, after_track.substep)
        {
            score += 4;
            what.push(format!(
                "story {}.{}.{}",
                after_track.chapter, after_track.step, after_track.substep
            ));
        }
        for event in events {
            match event {
                Event::Request(Request::Event(id)) => {
                    score += 3;
                    what.push(format!("event {id}"));
                }
                Event::Request(Request::Battle(n)) => {
                    score += 3;
                    what.push(format!("battle {n}"));
                }
                Event::MapChange(change) if self.same_family(change.destination) => {
                    score += 2;
                    what.push(format!("map {}", change.destination));
                }
                _ => {}
            }
        }
        if after.flags != self.state.flags || after_track.flags != track.flags {
            score += 1;
            what.push("flags".to_owned());
        }
        (score, what.join(", "))
    }

    /// The report of a run: the journal, and where and why it stopped.
    pub fn auto_report(&self, stop: &Stop) -> String {
        let mut out = String::new();
        out.push_str(&self.journal.to_string());
        let events = self.journal.events();
        out.push_str(&format!("events played: {events:?}\n"));
        match stop {
            Stop::Control { map } => out.push_str(&format!(
                "stopped: free control on map {map} after {:.1} s of game time\n",
                self.tick as f64 / 60.0
            )),
            Stop::Stuck { map, why } => out.push_str(&format!(
                "stopped: stuck on map {map} after {:.1} s of game time: {why}\n",
                self.tick as f64 / 60.0
            )),
            Stop::Limit => out.push_str("stopped: step limit reached while a scene was running\n"),
            Stop::NotStarted => out.push_str("stopped: no scene started\n"),
        }
        for (i, t) in self.state.tracks.iter().enumerate() {
            out.push_str(&format!(
                "track {i}: {}.{}.{} flags {:?}\n",
                t.chapter, t.step, t.substep, t.flags
            ));
        }
        out.push_str(&format!("global flags: {:?}\n", self.state.flags));
        if let Some(hero) = &self.hero {
            out.push_str(&format!("hero at {:?}\n", hero.position()));
        }
        out
    }
}
