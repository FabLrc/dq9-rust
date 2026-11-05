//! Deterministic field state and collision queries. No platform I/O.
use std::collections::HashMap;

use dq9_formats::map::{Collision, Map, PartKind};
use glam::{Mat4, Vec3};
use nds_rom::compress::decompress;
use nds_rom::{Error, Narc, Result};

pub mod field;
pub mod lighting;
pub mod state;
pub mod trigger;

const GROUND_RISE: f32 = 1.0;
const GROUND_DROP: f32 = 10.0;
const GROUND_EPSILON: f32 = 0.1;
const WALL_NORMAL_Y: f32 = 0.2;

/// One world-space collision face, before identifying whether it is floor or a wall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub vertices: [Vec3; 3],
    pub normal: Vec3,
}

/// Collision faces assembled from the map's visible collision placements.
#[derive(Debug, Clone, Default)]
pub struct CollisionWorld {
    faces: Vec<Face>,
}

impl CollisionWorld {
    /// Loads each referenced `.col2` from the map's NARC resources and applies its placement.
    /// `file` resolves ROM paths; archive members are read here.
    pub fn build<'a>(map: &Map, file: impl Fn(&str) -> Option<&'a [u8]>) -> Result<Self> {
        let mut archives: HashMap<&str, Narc<'a>> = HashMap::new();
        let mut parsed: HashMap<(usize, usize), Collision> = HashMap::new();
        let mut out = Self::default();
        for (block_index, block) in map.blocks.iter().enumerate() {
            if !block.processed {
                continue;
            }
            if !archives.contains_key(block.archive.as_str()) {
                let bytes =
                    file(&block.archive).ok_or(Error::Malformed("collision archive missing"))?;
                archives.insert(block.archive.as_str(), Narc::parse(bytes)?);
            }
            let archive = archives
                .get(block.archive.as_str())
                .ok_or(Error::Malformed("collision archive not cached"))?;
            for instance in &block.instances {
                if instance.hidden {
                    continue;
                }
                let Some(part_index) = instance.part else {
                    continue;
                };
                let Some(part) = block.parts.get(part_index) else {
                    continue;
                };
                if part.kind != PartKind::Collision {
                    continue;
                }
                let Some(member) = part.file.as_deref() else {
                    continue;
                };
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    parsed.entry((block_index, part_index))
                {
                    let raw = archive
                        .names
                        .files
                        .iter()
                        .find(|entry| entry.path.eq_ignore_ascii_case(member))
                        .ok_or(Error::Malformed("collision member missing"))?;
                    let compressed = archive.file(raw.id)?;
                    let unpacked = decompress(compressed)?;
                    entry.insert(Collision::parse(&unpacked)?);
                }
                let mesh = parsed
                    .get(&(block_index, part_index))
                    .ok_or(Error::Malformed("collision mesh not cached"))?;
                let transform = world_matrix(&instance.world);
                let unit_scale = 2f32.powi(mesh.shift as i32) / 4096.0;
                for tri in &mesh.triangles {
                    let vertices = tri.vertices.map(|v| {
                        transform.transform_point3(Vec3::new(
                            f32::from(v[0]) * unit_scale,
                            f32::from(v[1]) * unit_scale,
                            f32::from(v[2]) * unit_scale,
                        ))
                    });
                    let cross = (vertices[1] - vertices[0]).cross(vertices[2] - vertices[0]);
                    let normal = if cross.length_squared() > f32::EPSILON {
                        cross.normalize()
                    } else {
                        Vec3::ZERO
                    };
                    out.faces.push(Face { vertices, normal });
                }
            }
        }
        Ok(out)
    }

    /// Builds a synthetic or already-world-space collision set, useful for deterministic tools.
    pub fn from_faces(faces: Vec<Face>) -> Self {
        Self { faces }
    }

    /// Highest upward-facing triangle containing `(x, z)` within the game's vertical query
    /// window: one unit above through ten units below the caller's current height.
    pub fn ground_height(&self, x: f32, z: f32, reference_y: f32) -> Option<f32> {
        let low = reference_y - GROUND_DROP;
        let high = reference_y + GROUND_RISE;
        let mut best = None;
        for face in &self.faces {
            if face.normal.y < 0.5 || !contains_xz(face.vertices, x, z) {
                continue;
            }
            let a = face.vertices[0];
            let y = a.y - (face.normal.x * (x - a.x) + face.normal.z * (z - a.z)) / face.normal.y;
            if y >= low && y <= high && best.is_none_or(|old| y > old) {
                best = Some(y);
            }
        }
        best
    }

    /// A wall blocks a horizontal segment crossing its face while the character's body overlaps.
    fn blocked(&self, from: Vec3, to: Vec3) -> bool {
        self.faces.iter().any(|face| {
            if face.normal.y.abs() >= WALL_NORMAL_Y || face.normal == Vec3::ZERO {
                return false;
            }
            let direction = to - from;
            let denominator = face.normal.dot(direction);
            if denominator.abs() < 1e-6 {
                return false;
            }
            let t = face.normal.dot(face.vertices[0] - from) / denominator;
            if !(0.0..=1.0).contains(&t) {
                return false;
            }
            let point = from.lerp(to, t) + Vec3::Y * 0.75;
            point_in_triangle(point, face.vertices)
        })
    }

    /// Approximate default start at the walkable bounds' center, snapped to its nearest face.
    /// This is not a save-game spawn or a doorway arrival.
    pub fn spawn(&self) -> Option<Vec3> {
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for face in &self.faces {
            if face.normal.y < 0.5 {
                continue;
            }
            for vertex in face.vertices {
                low.x = low.x.min(vertex.x);
                low.z = low.z.min(vertex.z);
                high.x = high.x.max(vertex.x);
                high.z = high.z.max(vertex.z);
            }
        }
        if !low.x.is_finite() {
            return None;
        }
        let center = Vec3::new((low.x + high.x) * 0.5, 0.0, (low.z + high.z) * 0.5);
        let face = self
            .faces
            .iter()
            .filter(|face| face.normal.y >= 0.5)
            .min_by(|a, b| {
                let centroid =
                    |face: &Face| (face.vertices[0] + face.vertices[1] + face.vertices[2]) / 3.0;
                let a_center = centroid(a);
                let b_center = centroid(b);
                let da = (a_center.x - center.x).powi(2) + (a_center.z - center.z).powi(2);
                let db = (b_center.x - center.x).powi(2) + (b_center.z - center.z).powi(2);
                da.total_cmp(&db)
            })?;
        let p = (face.vertices[0] + face.vertices[1] + face.vertices[2]) / 3.0;
        self.ground_height(p.x, p.z, p.y + GROUND_EPSILON)
            .map(|y| Vec3::new(p.x, y + GROUND_EPSILON, p.z))
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }
}

/// Player transform updated at a fixed timestep from explicit world-relative input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hero {
    pub position: Vec3,
    pub yaw: f32,
    /// Field walking speed, units per second. The game routine derives this from actor state;
    /// 4 units/s is a provisional value pending frame-aligned differential measurements.
    pub speed: f32,
}

impl Hero {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            yaw: 0.0,
            speed: 4.0,
        }
    }

    /// Moves in a normalized horizontal world direction and snaps to the nearest valid ground.
    /// Diagonal input is capped to the same speed as axial input. Wall collision slides on axes.
    pub fn step(&mut self, world: &CollisionWorld, direction: Vec3, dt: f32) {
        let direction = Vec3::new(direction.x, 0.0, direction.z);
        let direction = direction.normalize_or_zero();
        if direction == Vec3::ZERO || dt <= 0.0 {
            return;
        }
        self.yaw = direction.x.atan2(-direction.z);
        let movement = direction * (self.speed * dt);
        let mut candidate = self.position + movement;
        if world.blocked(self.position, candidate) {
            let x_only = Vec3::new(
                self.position.x + movement.x,
                self.position.y,
                self.position.z,
            );
            let z_only = Vec3::new(
                self.position.x,
                self.position.y,
                self.position.z + movement.z,
            );
            candidate = if !world.blocked(self.position, x_only) {
                x_only
            } else if !world.blocked(self.position, z_only) {
                z_only
            } else {
                self.position
            };
        }
        if let Some(y) = world.ground_height(candidate.x, candidate.z, self.position.y) {
            self.position = Vec3::new(candidate.x, y + GROUND_EPSILON, candidate.z);
        }
    }
}

fn world_matrix(transform: &dq9_formats::map::Transform) -> Mat4 {
    Mat4::from_translation(Vec3::from_array(
        transform.position.map(|v| v as f32 / 4096.0),
    )) * Mat4::from_rotation_y(f32::from(transform.angle) / 4096.0)
        * Mat4::from_scale(Vec3::from_array(transform.scale.map(|v| v as f32 / 4096.0)))
}

fn contains_xz([a, b, c]: [Vec3; 3], x: f32, z: f32) -> bool {
    let v0 = Vec3::new(c.x - a.x, 0.0, c.z - a.z);
    let v1 = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
    let v2 = Vec3::new(x - a.x, 0.0, z - a.z);
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d02 = v0.dot(v2);
    let d11 = v1.dot(v1);
    let d12 = v1.dot(v2);
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() <= f32::EPSILON {
        return false;
    }
    let u = (d11 * d02 - d01 * d12) / denom;
    let v = (d00 * d12 - d01 * d02) / denom;
    u >= -1e-5 && v >= -1e-5 && u + v <= 1.0 + 1e-5
}

fn point_in_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> bool {
    let v0 = c - a;
    let v1 = b - a;
    let v2 = p - a;
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d02 = v0.dot(v2);
    let d11 = v1.dot(v1);
    let d12 = v1.dot(v2);
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() <= f32::EPSILON {
        return false;
    }
    let u = (d11 * d02 - d01 * d12) / denom;
    let v = (d00 * d12 - d01 * d02) / denom;
    u >= -1e-5 && v >= -1e-5 && u + v <= 1.0 + 1e-5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor() -> Face {
        Face {
            vertices: [
                Vec3::new(-5.0, 0.0, -5.0),
                Vec3::new(5.0, 0.0, -5.0),
                Vec3::new(0.0, 0.0, 5.0),
            ],
            normal: Vec3::Y,
        }
    }

    #[test]
    fn ground_query_chooses_highest_surface_in_vertical_window() {
        let mut upper = floor();
        for v in &mut upper.vertices {
            v.y = 1.0;
        }
        let world = CollisionWorld::from_faces(vec![floor(), upper]);
        assert_eq!(world.ground_height(0.0, 0.0, 0.0), Some(1.0));
        assert_eq!(world.ground_height(0.0, 0.0, 12.0), None);
        assert_eq!(world.ground_height(8.0, 0.0, 0.0), None);
    }

    #[test]
    fn spawn_prefers_the_center_of_walkable_collision_bounds() {
        let far = Face {
            vertices: [
                Vec3::new(20.0, 0.0, -0.5),
                Vec3::new(21.0, 0.0, -0.5),
                Vec3::new(20.0, 0.0, 0.5),
            ],
            normal: Vec3::Y,
        };
        let world = CollisionWorld::from_faces(vec![far, floor()]);
        let spawn = world.spawn().unwrap();
        assert!(spawn.x.abs() < 1.0);
        assert!(spawn.z.abs() < 2.0);
        assert!((spawn.y - GROUND_EPSILON).abs() < 1e-5);
    }

    #[test]
    fn movement_caps_diagonal_speed_and_snaps_to_slope() {
        let world = CollisionWorld::from_faces(vec![Face {
            vertices: [
                Vec3::new(-10.0, 0.0, -10.0),
                Vec3::new(10.0, 2.0, -10.0),
                Vec3::new(-10.0, 0.0, 10.0),
            ],
            normal: Vec3::new(-0.1, 1.0, 0.0).normalize(),
        }]);
        let mut hero = Hero::new(Vec3::new(-2.0, 0.1, 0.0));
        hero.step(&world, Vec3::new(1.0, 0.0, 1.0), 0.25);
        assert!((hero.position.x - (-2.0 + 2.0f32.sqrt() * 0.25 * 4.0 / 2.0)).abs() < 1e-4);
        assert!((hero.position.y - (hero.position.x * 0.1 + 1.1)).abs() < 1e-4);
        assert!((hero.position.z - 0.70710677).abs() < 1e-4);
    }

    #[test]
    fn vertical_faces_block_and_allow_axis_sliding() {
        let wall = Face {
            vertices: [
                Vec3::new(1.0, 0.0, -2.0),
                Vec3::new(1.0, 2.0, 2.0),
                Vec3::new(1.0, 0.0, 2.0),
            ],
            normal: Vec3::X,
        };
        let world = CollisionWorld::from_faces(vec![floor(), wall]);
        let mut hero = Hero::new(Vec3::new(0.0, 0.1, 0.0));
        hero.step(&world, Vec3::new(1.0, 0.0, 1.0), 0.5);
        assert!(hero.position.x < 1.0);
        assert!(hero.position.z > 0.0);
    }
}
