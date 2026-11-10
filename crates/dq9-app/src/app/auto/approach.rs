//! The last metres into a warp's trigger volume, searched with the hero's own movement.
//!
//! The walkable grid joins cells half a unit apart, so a passage narrower than a cell is
//! invisible to it, while the hero's sphere slides along walls and goes through (`M01M10`: the
//! stair that leads to the only warp is a lane 0.2 wide between two walls, entered through a
//! gap of a few centimetres beside the room's east wall). [`search`] tries the eight d-pad
//! directions with [`Hero::step`] itself, two ticks per update as the field runs it, and
//! returns the presses, one per update, that bring the hero's **own pose** (height included:
//! the volume is a box with a floor and a ceiling) into the trigger. The movement is
//! deterministic, so feeding the same presses from the same state walks the same way.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use dq9_core::{CollisionWorld, Hero, Input};
use dq9_formats::map::WarpVolume;
use glam::Vec3;

/// Updates one move of the search holds a direction for: a third of a unit at top speed.
const HOLD: u32 = 3;
/// Moves taken from the open list before the search gives up.
const EXPANSIONS: usize = 10_000;
/// What the hero covers in one update at top speed (`0x189` per tick, two ticks).
const REACH_PER_UPDATE: f32 = 0.192;
/// Side of the squares that keep one state each, per floor.
const SPOT: f32 = 0.125;

/// The eight d-pad directions, as the pilot feeds them to the hero.
const PRESSES: [Vec3; 8] = [
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(-1.0, 0.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::new(1.0, 0.0, 1.0),
    Vec3::new(1.0, 0.0, -1.0),
    Vec3::new(-1.0, 0.0, 1.0),
    Vec3::new(-1.0, 0.0, -1.0),
];

/// The point of the volume's footprint nearest to `at`, at the height of `at`: the box of a
/// `0x72` warp turned about its centre by the volume's angle (`ov_017::0219ca88`'s test, the
/// rotation of `func_020321e0`). Its height is not part of the answer: the hero's own pose
/// decides whether he is inside.
pub(super) fn nearest(volume: &WarpVolume, at: Vec3) -> Vec3 {
    let world = |v: [i32; 3]| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) / 4096.0;
    let (high, low, center) = (world(volume.high), world(volume.low), world(volume.center));
    let (sin, cos) = (f32::from(volume.angle) / 4096.0).sin_cos();
    let d = at - center;
    // The game turns the point back about the centre before it tests the box.
    let (x, z) = (
        center.x + d.x * cos - d.z * sin,
        center.z + d.x * sin + d.z * cos,
    );
    let (x, z) = (
        x.max(low.x).min(high.x) - center.x,
        z.max(low.z).min(high.z) - center.z,
    );
    Vec3::new(
        center.x + x * cos + z * sin,
        at.y,
        center.z - x * sin + z * cos,
    )
}

/// The presses (one per field update) that take a hero standing as `start` into a pose for
/// which `holds` is true, by A* over the states his own movement reaches: each move holds one
/// d-pad direction for [`HOLD`] updates. A move that enters `forbidden` (another warp, which
/// would carry him elsewhere) is dropped. `None` when no walk reaches the volume within the
/// search budget.
pub(super) fn search(
    world: &CollisionWorld,
    start: Hero,
    volume: &WarpVolume,
    holds: impl Fn(Vec3) -> bool,
    forbidden: impl Fn(Vec3) -> bool,
) -> Option<Vec<Vec3>> {
    struct State {
        hero: Hero,
        parent: usize,
        press: usize,
    }
    let cost_to_go = |at: Vec3| {
        let near = nearest(volume, at);
        let (dx, dz) = (near.x - at.x, near.z - at.z);
        ((dx * dx + dz * dz).sqrt() / REACH_PER_UPDATE) as u32
    };
    // One state per spot and floor **and last press**: a hero at rest and one running through
    // the same square are not in the same state, and a first move from rest covers less than a
    // square.
    let spot = |at: Vec3, press: usize| {
        (
            (at.x / SPOT).round() as i32,
            (at.z / SPOT).round() as i32,
            (at.y * 2.0).round() as i32,
            press,
        )
    };
    let mut states = vec![State {
        hero: start,
        parent: usize::MAX,
        press: PRESSES.len(),
    }];
    let mut best = BTreeMap::from([(spot(start.position(), PRESSES.len()), 0u32)]);
    let mut open = BinaryHeap::from([Reverse((cost_to_go(start.position()), 0u32, 0usize))]);
    let mut expanded = 0;
    while let Some(Reverse((_, cost, index))) = open.pop() {
        let from = states[index].hero;
        if best
            .get(&spot(from.position(), states[index].press))
            .is_some_and(|known| *known < cost)
        {
            continue;
        }
        expanded += 1;
        if expanded > EXPANSIONS {
            return None;
        }
        for (press, direction) in PRESSES.iter().enumerate() {
            let pad = Input::from_direction(*direction);
            let mut hero = from;
            let mut blocked = false;
            for held in 1..=HOLD {
                hero.step(world, pad, 0, 2);
                let at = hero.position();
                if holds(at) {
                    let mut moves = vec![(press, held)];
                    let mut at = index;
                    while states[at].parent != usize::MAX {
                        moves.push((states[at].press, HOLD));
                        at = states[at].parent;
                    }
                    return Some(
                        moves
                            .into_iter()
                            .rev()
                            .flat_map(|(press, count)| {
                                std::iter::repeat_n(PRESSES[press], count as usize)
                            })
                            .collect(),
                    );
                }
                if forbidden(at) {
                    blocked = true;
                    break;
                }
            }
            // A press that did not move him at all (a wall straight ahead) leads nowhere.
            let moved = hero.position() - from.position();
            if blocked || moved.x.abs() + moved.z.abs() < 0.02 {
                continue;
            }
            let next_cost = cost + HOLD;
            let key = spot(hero.position(), press);
            if best.get(&key).is_some_and(|known| *known <= next_cost) {
                continue;
            }
            best.insert(key, next_cost);
            states.push(State {
                hero,
                parent: index,
                press,
            });
            open.push(Reverse((
                next_cost + cost_to_go(hero.position()),
                next_cost,
                states.len() - 1,
            )));
        }
    }
    if std::env::var_os("DQ9_PILOT_DEBUG").is_some() {
        let best_state = states
            .iter()
            .min_by_key(|state| cost_to_go(state.hero.position()))
            .map(|state| state.hero.position());
        eprintln!(
            "approach: no walk after {expanded} expansions, {} states, nearest {best_state:?}",
            states.len()
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A volume 4 wide (`x`) and 2 deep (`z`) turned by 0.5 rad about its centre `(10, 0, 20)`,
    /// with its heights from -1 to 1 (invented numbers, as the engine stores them: `fx32`).
    fn turned() -> WarpVolume {
        let fx = |v: f32| (v * 4096.0) as i32;
        WarpVolume {
            high: [fx(12.0), fx(1.0), fx(21.0)],
            low: [fx(8.0), fx(-1.0), fx(19.0)],
            center: [fx(10.0), 0, fx(20.0)],
            angle: (0.5f32 * 4096.0) as i16,
            radius_sq: fx(5.0),
        }
    }

    /// The nearest point of the footprint is a point of it, and no other point of the
    /// footprint is nearer: checked against a brute-force scan of the same box turned the
    /// way the game turns the tested point.
    #[test]
    fn the_nearest_point_of_a_turned_footprint_is_its_closest() {
        let volume = turned();
        let angle = f32::from(volume.angle) / 4096.0;
        let (sin, cos) = angle.sin_cos();
        let inside = |p: Vec3| {
            let (dx, dz) = (p.x - 10.0, p.z - 20.0);
            let (x, z) = (10.0 + dx * cos - dz * sin, 20.0 + dx * sin + dz * cos);
            (8.0..=12.0).contains(&x) && (19.0..=21.0).contains(&z)
        };
        for at in [
            Vec3::new(10.0, 0.3, 20.0),
            Vec3::new(14.0, 0.3, 20.0),
            Vec3::new(6.0, 0.3, 23.0),
            Vec3::new(10.0, 0.3, 16.0),
            Vec3::new(13.0, 0.3, 17.5),
        ] {
            let near = nearest(&volume, at);
            assert!(
                (near.y - at.y).abs() < 1e-6,
                "the height of the query is kept: {near:?}"
            );
            let reach = Vec3::new(near.x - at.x, 0.0, near.z - at.z).length();
            if inside(at) {
                assert!(reach < 1e-3, "a point inside is its own nearest: {at:?}");
                continue;
            }
            // A scan of the plane for the closest point that is inside the footprint.
            let mut closest = f32::MAX;
            let mut x = 4.0;
            while x <= 16.0 {
                let mut z = 14.0;
                while z <= 26.0 {
                    let p = Vec3::new(x, 0.0, z);
                    if inside(p) {
                        closest = closest.min(Vec3::new(p.x - at.x, 0.0, p.z - at.z).length());
                    }
                    z += 0.02;
                }
                x += 0.02;
            }
            assert!(
                (reach - closest).abs() < 0.05,
                "from {at:?} the nearest point is {reach} away, the scan found {closest}"
            );
        }
    }
}
