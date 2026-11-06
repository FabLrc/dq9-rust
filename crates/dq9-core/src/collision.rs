//! Map collision of the field: the `.col2` meshes of the visible placements, queried through
//! their spatial grid, and what the game does with the triangles it finds — the ground ray
//! that stands a character on the floor and the sphere that pushes it out of walls
//! (`docs/re/collision.md`, `docs/re/movement.md`). No I/O: the caller supplies the ROM files.
//!
//! A *piece* is one placed `.col2` (`Zone3D` instance: position and `y` angle of the placement
//! hierarchy). Queries take a world box, ask each piece for the triangles whose bounding box
//! meets it (`0x0204c040`), and work in the piece's frame: positions are shifted by the
//! piece's position and turned by minus its angle, vertices are the file's integers times
//! `2^shift` (`ov_017::02192b48`).
//!
//! Not modelled: the small solid colliders of the zone objects (`ov_017::02193290`), the
//! characters that push the leader (`ov_017::0218df50`, `0218dfd8`), the special-cased maps
//! `0x1131` and `10000`, the terrain record copied to the character, the grotto lookup of
//! `0x23b8`, and the vehicle branch of `0x02017da0`.
//!
//! port: 0x0204c040, 0x020310ac, 0x020312cc, 0x020315c8, 0x02031b94, 0x02031d30, 0x020320a0

use std::collections::HashMap;

use dq9_formats::map::{Collision, Map, PartKind, fix32_cos, fix32_sin};
use glam::Vec3;
use nds_rom::compress::decompress;
use nds_rom::{Error, Narc, Result};

use crate::fx::{self, Fx3};

/// Height above the ground the game stands a character at (`0x02018fcc` adds `0x199`).
pub const STAND_HEIGHT: i32 = 0x199;
/// The ground ray starts this far above the character's feet and ends [`RAY_DEPTH`] below.
const RAY_HEIGHT: i32 = 0x1800;
const RAY_DEPTH: i32 = 0xa000;
/// Slope limit between floors (normal `y` above) and walls (at most).
const FLOOR_NORMAL_Y: i32 = 0x800;
/// Triangles a piece hands back per query, and candidates it examines (`0x0204c040`).
const MAX_TRIANGLES: usize = 0x60;
const MAX_CANDIDATES: usize = 0xc0;
/// A character on the ground moves its height only when the floor is further than this.
const SNAP_HEIGHT: i32 = 0x28;
/// Offsets of the eight probe rays that tell a ledge from a fall (`0x021d62d8`, `0x021d62f8`).
const PROBE_X: [i32; 8] = [-0x400, 0, 0x400, 0, -0x2e1, 0x2e1, -0x2e1, 0x2e1];
const PROBE_Z: [i32; 8] = [0, -0x400, 0, 0x400, -0x2e1, -0x2e1, 0x2e1, 0x2e1];

/// A triangle of a piece in its own frame: three vertices and the face normal, `fx32`
/// (`0x30` bytes in the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tri {
    pub vertices: [Fx3; 3],
    pub normal: Fx3,
}

/// One placed collision mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// Index into [`CollisionWorld::meshes`].
    pub mesh: usize,
    /// World position of the mesh origin.
    pub position: Fx3,
    /// `y` angle (`fx32` radians).
    pub angle: i32,
}

/// The wall a character last touched (`Character+0xe4`: the triangle in world position, in
/// the piece's own orientation, and its normal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wall {
    pub vertices: [Fx3; 3],
    pub normal: Fx3,
}

/// What [`CollisionWorld::resolve`] reads and writes of a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body {
    /// `+0x44`.
    pub position: Fx3,
    /// `+0xac`: downward speed, increased by `0x51` per call up to `0x1000`.
    pub fall: i32,
    /// `Object3D` radius and height (`+0x64`, `+0x68`).
    pub radius: i32,
    pub height: i32,
    /// `+0xe4`: the wall touched by the last call.
    pub wall: Option<Wall>,
}

/// How a step is resolved (`mode` of `ov_017::0219344c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The character moved a normal distance: one pass.
    Whole,
    /// The character moved fast: two passes, each half of the way.
    Halves,
}

/// What a call found out about the character's footing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// There is floor within reach.
    pub grounded: bool,
    /// The character left the floor on this call (it was grounded, [`Body::fall`] was 0).
    pub started_falling: bool,
}

/// One placement of a block (`Instance`) and its parent, kept to rotate a door's subtree
/// (model placement and its collision child) together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Node {
    /// `Block::entry` the placement belongs to (a door's class).
    block: u32,
    /// `Instance::id` (a door's number).
    id: u16,
    /// Parent node in the same list.
    parent: Option<usize>,
    /// World transform before any door rotation.
    position: Fx3,
    angle: i32,
}

/// Collision meshes and their placements.
#[derive(Debug, Clone, Default)]
pub struct CollisionWorld {
    meshes: Vec<Collision>,
    pieces: Vec<Piece>,
    /// Placement hierarchy of every processed block, parents first.
    nodes: Vec<Node>,
    /// Node of each piece (`nodes` index).
    node_of_piece: Vec<usize>,
}

impl CollisionWorld {
    /// Loads each referenced `.col2` from the map's NARC resources and applies its placement.
    /// `file` resolves ROM paths; archive members are read here.
    pub fn build<'a>(map: &Map, file: impl Fn(&str) -> Option<&'a [u8]>) -> Result<Self> {
        let mut archives: HashMap<&str, Narc<'a>> = HashMap::new();
        let mut parsed: HashMap<(usize, usize), usize> = HashMap::new();
        let mut out = Self::default();
        // The placement hierarchy first: a door rotates its placement and every descendant
        // (the collision piece is usually a child of the model).
        let mut block_nodes: HashMap<usize, usize> = HashMap::new();
        for (block_index, block) in map.blocks.iter().enumerate() {
            if !block.processed {
                continue;
            }
            let base = out.nodes.len();
            block_nodes.insert(block_index, base);
            for instance in &block.instances {
                out.nodes.push(Node {
                    block: block.entry,
                    id: instance.id,
                    parent: instance.parent.map(|parent| base + parent),
                    position: instance.world.position,
                    angle: i32::from(instance.world.angle),
                });
            }
        }
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
            for (instance_index, instance) in block.instances.iter().enumerate() {
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
                let mesh = match parsed.get(&(block_index, part_index)) {
                    Some(&mesh) => mesh,
                    None => {
                        let raw = archive
                            .names
                            .files
                            .iter()
                            .find(|entry| entry.path.eq_ignore_ascii_case(member))
                            .ok_or(Error::Malformed("collision member missing"))?;
                        let unpacked = decompress(archive.file(raw.id)?)?;
                        out.meshes.push(Collision::parse(&unpacked)?);
                        parsed.insert((block_index, part_index), out.meshes.len() - 1);
                        out.meshes.len() - 1
                    }
                };
                let node = block_nodes[&block_index] + instance_index;
                out.pieces.push(Piece {
                    mesh,
                    position: instance.world.position,
                    angle: i32::from(instance.world.angle),
                });
                out.node_of_piece.push(node);
            }
        }
        Ok(out)
    }

    /// Builds a world from meshes and their placements (tools and tests).
    pub fn from_pieces(meshes: Vec<Collision>, pieces: Vec<Piece>) -> Self {
        Self {
            meshes,
            node_of_piece: vec![usize::MAX; pieces.len()],
            pieces,
            nodes: Vec::new(),
        }
    }

    /// Native 584 (`0x02018b44`, and `Field_TouchDoorways` for a doorway with no destination):
    /// rotates placement `placement` of block entry `block` and its whole subtree (model
    /// placement and collision child) by `angle`, a delta in `fx32` added to the placement's
    /// own rotation (`+0x1922` = +90°). Rotates the collision pieces in place, from the
    /// transforms read at build time, without re-parsing anything.
    /// Returns whether any collision piece moved.
    pub fn set_door_angle(&mut self, block: u32, placement: u16, angle: i32) -> bool {
        let Some(root) = self
            .nodes
            .iter()
            .position(|node| node.block == block && node.id == placement)
        else {
            return false;
        };
        let pivot = self.nodes[root].position;
        let (sin, cos) = (fix32_sin(angle), fix32_cos(angle));
        let mut moved = false;
        for index in 0..self.pieces.len() {
            let mut node = self.node_of_piece[index];
            let mut inside = false;
            while node != usize::MAX {
                if node == root {
                    inside = true;
                    break;
                }
                node = self
                    .nodes
                    .get(node)
                    .and_then(|n| n.parent)
                    .unwrap_or(usize::MAX);
            }
            if !inside {
                continue;
            }
            let Some(base) = self.nodes.get(self.node_of_piece[index]) else {
                continue;
            };
            let (base_position, base_angle) = (base.position, base.angle);
            let relative = fx::sub(base_position, pivot);
            // The map walk's rotation (`MTX_RotY33_` on a row vector, `0x02013168`).
            let rotated = [
                ((i64::from(relative[0]) * i64::from(cos)
                    + i64::from(relative[2]) * i64::from(sin))
                    >> 12) as i32,
                relative[1],
                ((i64::from(relative[2]) * i64::from(cos)
                    - i64::from(relative[0]) * i64::from(sin))
                    >> 12) as i32,
            ];
            self.pieces[index].position = [
                rotated[0].wrapping_add(pivot[0]),
                rotated[1].wrapping_add(pivot[1]),
                rotated[2].wrapping_add(pivot[2]),
            ];
            self.pieces[index].angle = crate::fx::reduce_angle(base_angle.wrapping_add(angle));
            moved = true;
        }
        moved
    }

    pub fn meshes(&self) -> &[Collision] {
        &self.meshes
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The triangles of `piece` whose box meets the world box `[hi, lo]`, in the piece's frame
    /// (`0x02017da0` without the vehicle branch, `0x0204c040`, then the scaling of
    /// `ov_017::02192b48`).
    fn triangles(&self, piece: &Piece, hi: Fx3, lo: Fx3) -> Vec<Tri> {
        let mesh = &self.meshes[piece.mesh];
        let hi = fx::sub(hi, piece.position);
        let lo = fx::sub(lo, piece.position);
        let shift = mesh.shift;
        query(mesh, hi, lo)
            .into_iter()
            .map(|index| {
                let t = &mesh.triangles[index];
                Tri {
                    // `0x02031198` then `Vector3fixMultiplyScalar(·, 1 << shift)`.
                    vertices: t.vertices.map(|v| v.map(|c| i32::from(c) << shift)),
                    normal: t.normal.map(i32::from),
                }
            })
            .collect()
    }

    /// `Zone3D_GroundHeight` (`0x02018fcc`): the height a character at `position` stands at —
    /// the highest floor found by a ray from one unit above to ten below, plus
    /// [`STAND_HEIGHT`]; the last piece with a floor wins, as in the game. The floor's height
    /// is taken in the piece's frame, without its position **[hypothèse]** (the pieces of
    /// ground have no height offset).
    pub fn ground(&self, position: Fx3) -> Option<i32> {
        let hi = [
            position[0] + 0x800,
            position[1] + 0x1000,
            position[2] + 0x800,
        ];
        let lo = [
            position[0] - 0x800,
            position[1] - RAY_DEPTH,
            position[2] - 0x800,
        ];
        let mut found = None;
        for piece in &self.pieces {
            let tris = self.triangles(piece, hi, lo);
            if tris.is_empty() {
                continue;
            }
            let local = fx::sub(position, piece.position);
            let start = [local[0], local[1] + 0x1000, local[2]];
            let end = [local[0], local[1] - RAY_DEPTH, local[2]];
            if let Some((_, hit)) = ground_ray(&tris, start, end) {
                found = Some(hit[1] + STAND_HEIGHT);
            }
        }
        found
    }

    /// Every floor height of the vertical column `(x, z)` between `low` and `high`, in world
    /// units: the per-piece ground rays, so a ramp above a terrace is not hidden by the
    /// "last piece wins" rule of [`CollisionWorld::ground`] (the movement resolve takes the
    /// highest floor, and the grid must see it).
    pub fn floors(&self, x: f32, z: f32, low: f32, high: f32) -> Vec<f32> {
        let to_fx = |v: f32| (v * 4096.0) as i32;
        let (x_fx, z_fx) = (to_fx(x), to_fx(z));
        let hi_world = [x_fx, to_fx(high + 1.0), z_fx];
        let lo_world = [x_fx, to_fx(low - 1.0), z_fx];
        let box_hi = [x_fx + 0x800, to_fx(high + 1.0), z_fx + 0x800];
        let box_lo = [x_fx - 0x800, to_fx(low - 1.0), z_fx - 0x800];
        let mut out: Vec<f32> = Vec::new();
        for piece in &self.pieces {
            let tris = self.triangles(piece, box_hi, box_lo);
            if tris.is_empty() {
                continue;
            }
            let turn = rotation_y(piece.angle);
            let a = turn_vector(fx::sub(hi_world, piece.position), &turn);
            let b = turn_vector(fx::sub(lo_world, piece.position), &turn);
            for t in &tris {
                if t.normal[1] <= FLOOR_NORMAL_Y {
                    continue;
                }
                let Ray::Hit { weights, .. } = ray_triangle(a, b, t) else {
                    continue;
                };
                let height = fx::mul(t.vertices[1][1], weights[1])
                    .wrapping_add(fx::mul(t.vertices[0][1], weights[0]))
                    .wrapping_add(fx::mul(t.vertices[2][1], weights[2]))
                    .wrapping_add(STAND_HEIGHT)
                    .wrapping_add(piece.position[1]);
                let world = height as f32 / 4096.0;
                if world >= low - 0.5
                    && world <= high + 0.5
                    && !out.iter().any(|seen| (*seen - world).abs() < 0.02)
                {
                    out.push(world);
                }
            }
        }
        out.sort_by(|a, b| a.total_cmp(b));
        out
    }

    /// [`CollisionWorld::ground`] in world units.
    pub fn ground_height(&self, x: f32, z: f32, y: f32) -> Option<f32> {
        let to_fx = |v: f32| (v * 4096.0) as i32;
        self.ground([to_fx(x), to_fx(y), to_fx(z)])
            .map(|ground| ground as f32 / 4096.0)
    }

    /// Approximate default start at the walkable bounds' center, snapped to its nearest
    /// face. This is not a save-game spawn or a doorway arrival.
    pub fn spawn(&self) -> Option<Vec3> {
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        let mut floors: Vec<Vec3> = Vec::new();
        for piece in &self.pieces {
            let mesh = &self.meshes[piece.mesh];
            let (sin, cos) = (angle_f32(piece.angle)).sin_cos();
            let origin = piece.position.map(|c| c as f32 / 4096.0);
            let world = |v: [i16; 3]| {
                let [x, y, z] = v.map(|c| (i32::from(c) << mesh.shift) as f32 / 4096.0);
                Vec3::new(
                    origin[0] + cos * x + sin * z,
                    origin[1] + y,
                    origin[2] - sin * x + cos * z,
                )
            };
            for t in &mesh.triangles {
                if i32::from(t.normal[1]) < FLOOR_NORMAL_Y {
                    continue;
                }
                let vs = t.vertices.map(world);
                for v in vs {
                    low.x = low.x.min(v.x);
                    low.z = low.z.min(v.z);
                    high.x = high.x.max(v.x);
                    high.z = high.z.max(v.z);
                }
                floors.push((vs[0] + vs[1] + vs[2]) / 3.0);
            }
        }
        if !low.x.is_finite() {
            return None;
        }
        let center = Vec3::new((low.x + high.x) * 0.5, 0.0, (low.z + high.z) * 0.5);
        let nearest = floors.iter().min_by(|a, b| {
            let da = (a.x - center.x).powi(2) + (a.z - center.z).powi(2);
            let db = (b.x - center.x).powi(2) + (b.z - center.z).powi(2);
            da.total_cmp(&db)
        })?;
        self.ground_height(nearest.x, nearest.z, nearest.y + 0.1)
            .map(|y| Vec3::new(nearest.x, y, nearest.z))
    }

    /// `Character_CollideWithMap` (`ov_017::0219344c`, the map part): moves `body` out of the
    /// walls and onto the floor after it was displaced from `previous`. `Halves` resolves the
    /// move in two halves (the game does so when a character travels more than `0x333` in a
    /// call). `body.fall` is increased, then cleared when the character stands on a floor;
    /// `body.wall` receives the wall touched.
    pub fn resolve(&self, body: &mut Body, previous: Fx3, mode: Mode) -> Outcome {
        let fall_before = body.fall;
        // `+0xac` is a `short`; the game caps it at `0x1000`.
        body.fall = i32::from((body.fall as i16).wrapping_add(0x51)).min(0x1000);
        body.wall = None;
        let start_y = body.position[1];
        let mut floor = start_y - RAY_DEPTH;
        let mut grounded = true;
        let mut step = [0; 3];
        let passes = match mode {
            Mode::Whole => 1,
            Mode::Halves => {
                step = fx::scale(fx::sub(body.position, previous), 0x800);
                body.position = fx::add(previous, step);
                2
            }
        };
        for pass in 0..passes {
            let before = body.position;
            if pass == 1 {
                body.position = fx::add(body.position, step);
            }
            let wall_previous = if mode == Mode::Halves {
                before
            } else {
                previous
            };
            let hi = [
                body.position[0] + body.radius / 2,
                body.position[1] + body.height,
                body.position[2] + body.radius / 2,
            ];
            let lo = [
                body.position[0] - body.radius / 2,
                body.position[1] - RAY_DEPTH,
                body.position[2] - body.radius / 2,
            ];
            for piece in &self.pieces {
                let tris = self.triangles(piece, hi, lo);
                if !tris.is_empty() {
                    self.resolve_piece(
                        piece,
                        &tris,
                        body,
                        start_y,
                        &mut floor,
                        &mut grounded,
                        wall_previous,
                    );
                }
            }
        }
        // The game starts the falling state when the floor is lost on the first frame.
        let started_falling = !grounded && fall_before == 0;
        if grounded {
            body.fall = 0;
        }
        Outcome {
            grounded,
            started_falling,
        }
    }

    /// `Character_ResolveTriangles` (`ov_017::02192b48`): the floor under the character, then
    /// the walls around it, within one piece. `start_y` is the height at the start of the
    /// call and `floor` the highest floor found so far.
    #[allow(clippy::too_many_arguments)]
    fn resolve_piece(
        &self,
        piece: &Piece,
        tris: &[Tri],
        body: &mut Body,
        start_y: i32,
        floor: &mut i32,
        grounded: &mut bool,
        previous: Fx3,
    ) {
        let turn = rotation_y(piece.angle);
        let local = turn_vector(fx::sub(body.position, piece.position), &turn);
        let start = [local[0], local[1] + RAY_HEIGHT, local[2]];
        let end = [local[0], local[1] - RAY_DEPTH, local[2]];
        if let Some((_, hit)) = ground_ray(tris, start, end) {
            let height = hit[1] + STAND_HEIGHT + piece.position[1];
            if *floor < height {
                let drop = start_y - height;
                if drop < 0x800 {
                    if height < start_y || drop.abs() < 0x1000 {
                        *grounded = true;
                        body.position[1] = start_y;
                        *floor = height;
                        if drop.abs() > SNAP_HEIGHT {
                            body.position[1] = height;
                        }
                        body.fall = 0;
                    }
                } else {
                    // The floor is far below: standing needs a floor within reach around.
                    let mut ledge = true;
                    for i in 0..8 {
                        let a = [start[0] + PROBE_X[i], start[1], start[2] + 2 * PROBE_Z[i]];
                        let b = [end[0] + PROBE_X[i], end[1], end[2]];
                        if let Some((_, h)) = ground_ray(tris, a, b) {
                            let h = h[1] + STAND_HEIGHT + piece.position[1];
                            if h < start_y && start_y - h < 0x800 {
                                ledge = false;
                                break;
                            }
                        }
                    }
                    if ledge {
                        *grounded = false;
                    }
                }
            }
        }
        let center = [local[0], local[1] + 0x800, local[2]];
        let (pushed, wall) = push_from_walls(tris, center, 0x800, Some(previous));
        if let Some(index) = wall
            && let Some(back) = inverse_matrix(&turn)
        {
            let world = turn_vector(pushed, &back);
            body.position[0] = world[0] + piece.position[0];
            body.position[2] = world[2] + piece.position[2];
            let t = &tris[index];
            body.wall = Some(Wall {
                vertices: t.vertices.map(|v| fx::add(v, piece.position)),
                normal: t.normal,
            });
        }
    }
}

/// `2π - angle` as the game feeds its sine/cosine routines, whose result truncates to `fx32`
/// (`ov_017::02192b48`): the rotation matrix `MTX_RotY33_(sin, cos)` that takes a world
/// offset into the frame of a piece turned by `angle`. The argument is `0x6487 - angle`, one
/// unit short of a turn **[hypothèse]**: the SDK routines are evaluated in floating point
/// here.
fn rotation_y(angle: i32) -> [i32; 9] {
    let argument = f64::from(0x6487 - angle) / 4096.0;
    let sin = (argument.sin() * 4096.0) as i32;
    let cos = (argument.cos() * 4096.0) as i32;
    [cos, 0, -sin, 0, 0x1000, 0, sin, 0, cos]
}

/// `MTX_MultVec33` (`0x020c17d4`).
fn turn_vector(v: Fx3, m: &[i32; 9]) -> Fx3 {
    let row = |a: i32, b: i32, c: i32| {
        ((i64::from(a) * i64::from(v[0])
            + i64::from(b) * i64::from(v[1])
            + i64::from(c) * i64::from(v[2]))
            >> 12) as i32
    };
    [
        row(m[0], m[3], m[6]),
        row(m[1], m[4], m[7]),
        row(m[2], m[5], m[8]),
    ]
}

/// `MTX_Inverse33` (`0x020c12c4`), cofactors and one reciprocal of the determinant; `None`
/// for a singular matrix.
fn inverse_matrix(m: &[i32; 9]) -> Option<[i32; 9]> {
    let p = |i: usize| i64::from(m[i]);
    let round = |v: i64| ((v + 0x800) >> 12) as i32;
    let floor = |v: i64| (v >> 12) as i32;
    let c1 = round(p(3) * p(8) - p(5) * p(6));
    let c2 = round(p(4) * p(8) - p(5) * p(7));
    let c3 = round(p(3) * p(7) - p(4) * p(6));
    let det = round(p(0) * i64::from(c2) - p(1) * i64::from(c1) + p(2) * i64::from(c3));
    if det == 0 {
        return None;
    }
    let inverse = i64::from(fx::div(0x1000, det));
    let scale = |v: i32| floor(inverse * i64::from(v));
    Some([
        scale(c2),
        -scale(floor(p(1) * p(8) - p(2) * p(7))),
        scale(floor(p(1) * p(5) - p(2) * p(4))),
        -scale(c1),
        scale(floor(p(0) * p(8) - p(2) * p(6))),
        -scale(floor(p(0) * p(5) - p(2) * p(3))),
        scale(c3),
        -scale(floor(p(0) * p(7) - p(1) * p(6))),
        scale(floor(p(0) * p(4) - p(1) * p(3))),
    ])
}

fn angle_f32(angle: i32) -> f32 {
    angle as f32 / 4096.0
}

/// Whether two boxes `[max x y z, min x y z]` meet, edges included (`0x020310ac`).
fn boxes_meet(a: &[i32; 6], b: &[i32; 6]) -> bool {
    !(a[0] < b[3] || a[1] < b[4] || a[2] < b[5] || b[0] < a[3] || b[1] < a[4]) && a[5] <= b[2]
}

/// `0x0204c040`: the indices of the triangles of `mesh` whose box meets the box `[hi, lo]`
/// (in the mesh's frame): the grid cells around the box centre, then each triangle's
/// precomputed box (the six nibbles of its attribute word).
fn query(mesh: &Collision, hi: Fx3, lo: Fx3) -> Vec<usize> {
    let shift = mesh.shift;
    let max = [
        i32::from(mesh.bounds[3]) << shift,
        i32::from(mesh.bounds[4]) << shift,
        i32::from(mesh.bounds[5]) << shift,
    ];
    let min = [
        i32::from(mesh.bounds[0]) << shift,
        i32::from(mesh.bounds[1]) << shift,
        i32::from(mesh.bounds[2]) << shift,
    ];
    let query_box = [hi[0], hi[1], hi[2], lo[0], lo[1], lo[2]];
    let mesh_box = [max[0], max[1], max[2], min[0], min[1], min[2]];
    if !boxes_meet(&query_box, &mesh_box) {
        return Vec::new();
    }
    let q = query_box.map(|c| c >> shift);
    let cells = candidate_cells(mesh, &q);
    let mut seen: Vec<usize> = Vec::new();
    let mut found = Vec::new();
    'cells: for cell in cells {
        let Some(range) = mesh.cells.get(cell) else {
            continue;
        };
        for &index in &mesh.cell_indices[range.clone()] {
            let index = usize::from(index);
            if seen.contains(&index) {
                continue;
            }
            seen.push(index);
            let t = &mesh.triangles[index];
            if triangle_box_meets(t, &q) {
                found.push(index);
                if found.len() == MAX_TRIANGLES {
                    break 'cells;
                }
            }
            if seen.len() == MAX_CANDIDATES {
                break 'cells;
            }
        }
    }
    found
}

/// Whether the box of triangle `t` (attribute nibbles pick the extreme coordinates among its
/// nine) meets the query box `q` (`[max, min]`, in file units).
fn triangle_box_meets(t: &dq9_formats::map::Triangle, q: &[i32; 6]) -> bool {
    let coords: [i32; 12] = [
        t.vertices[0][0],
        t.vertices[0][1],
        t.vertices[0][2],
        t.vertices[1][0],
        t.vertices[1][1],
        t.vertices[1][2],
        t.vertices[2][0],
        t.vertices[2][1],
        t.vertices[2][2],
        t.normal[0],
        t.normal[1],
        t.normal[2],
    ]
    .map(i32::from);
    let a = t.attributes;
    let nibble = |i: u32| {
        coords
            .get(((a >> (4 * i)) & 0xf) as usize)
            .copied()
            .unwrap_or(0)
    };
    // Nibbles: min x, min y, min z, max x, max y, max z.
    let tri_box = [
        nibble(3),
        nibble(4),
        nibble(5),
        nibble(0),
        nibble(1),
        nibble(2),
    ];
    boxes_meet(&tri_box, q)
}

/// The grid cells to scan around the query box `q` (file units, `[max, min]`): all of them
/// for a mesh of at most 7 cells, else the cell under the centre and its neighbours — the
/// rows alternate by half a cell (`0x0204c040`).
fn candidate_cells(mesh: &Collision, q: &[i32; 6]) -> Vec<usize> {
    let gx = mesh.grid_x as i32;
    let gz = mesh.grid_z as i32;
    let count = gx * gz + gz / 2;
    if count <= 7 {
        return (0..count as usize).collect();
    }
    let size = i32::from(mesh.cell_size);
    let zc = (q[2] + q[5]) / 2;
    let row = fx::div(zc - i32::from(mesh.bounds[2]), size) / 4096;
    if row < 0 || row >= gz {
        return Vec::new();
    }
    let xc = (q[0] + q[3]) / 2;
    let min_x = i32::from(mesh.bounds[0]);
    let even = row % 2 == 0;
    let col = if even {
        fx::div(xc - min_x, size) / 4096
    } else {
        fx::div(xc - min_x + size / 2, size) / 4096
    };
    let columns = if even { gx } else { gx + 1 };
    if col < 0 || col >= columns {
        return Vec::new();
    }
    let base = row * gx + col + row / 2;
    let mut cells = vec![base];
    if even {
        if row > 0 {
            cells.extend([base - gx, base - gx - 1]);
        }
        if row < gz - 1 {
            cells.extend([base + gx, base + gx + 1]);
        }
        if col > 0 {
            cells.push(base - 1);
        }
        if col < gx - 1 {
            cells.push(base + 1);
        }
    } else {
        if row > 0 {
            if col > 0 {
                cells.push(base - gx - 1);
            }
            if col < gx {
                cells.push(base - gx);
            }
        }
        if row < gz - 1 {
            if col > 0 {
                cells.push(base + gx);
            }
            if col < gx {
                cells.push(base + gx + 1);
            }
        }
        if col > 0 {
            cells.push(base - 1);
        }
        if col < gx {
            cells.push(base + 1);
        }
    }
    cells.into_iter().map(|c| c as usize).collect()
}

/// Result of [`ray_triangle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ray {
    /// The segment does not cross the front of the triangle's plane.
    Miss,
    /// It crosses the plane outside the triangle.
    Outside,
    /// It crosses the triangle: barycentric weights of the three vertices and the fraction of
    /// the segment.
    Hit { weights: [i32; 3], t: i32 },
}

/// `0x02031b94`: the segment `p1 → p2` against a triangle, from its front side only.
fn ray_triangle(p1: Fx3, p2: Fx3, t: &Tri) -> Ray {
    let [v0, v1, v2] = t.vertices;
    let e1 = fx::sub(v1, v0);
    let e2 = fx::sub(v2, v0);
    let d = fx::sub(p1, p2);
    let n = fx::cross(e1, e2);
    let det = fx::dot(d, n);
    if det < 1 {
        return Ray::Miss;
    }
    let ap = fx::sub(p1, v0);
    let along = fx::dot(ap, n);
    if along < 0 || det < along {
        return Ray::Miss;
    }
    let e = fx::cross(d, ap);
    let b = fx::dot(e2, e);
    if b < 0 || det < b {
        return Ray::Outside;
    }
    let c = -fx::dot(e1, e);
    if c < 0 || det < b.wrapping_add(c) {
        return Ray::Outside;
    }
    let inverse = fx::div(0x1000, det);
    let (b, c) = (fx::mul(b, inverse), fx::mul(c, inverse));
    Ray::Hit {
        weights: [0x1000 - b - c, b, c],
        t: fx::mul(along, inverse),
    }
}

/// `0x020320a0`: where the segment `a → b` meets the plane `n · x = d`.
fn ray_plane(a: Fx3, b: Fx3, n: Fx3, d: i32) -> Option<Fx3> {
    let direction = fx::sub(b, a);
    let t = fx::div(d.wrapping_sub(fx::dot(n, a)), fx::dot(n, direction));
    if !(0..=0x1000).contains(&t) {
        return None;
    }
    Some(fx::add(a, fx::scale(direction, t)))
}

/// `0x020312cc`: among the floors (normal `y` above 0.5) the segment crosses, the one whose
/// interpolated height is highest, and where the segment meets its plane.
fn ground_ray(tris: &[Tri], a: Fx3, b: Fx3) -> Option<(usize, Fx3)> {
    let mut best: Option<(usize, i32)> = None;
    for (i, t) in tris.iter().enumerate() {
        if t.normal[1] <= FLOOR_NORMAL_Y {
            continue;
        }
        if let Ray::Hit { weights, .. } = ray_triangle(a, b, t) {
            let height = fx::mul(t.vertices[1][1], weights[1])
                .wrapping_add(fx::mul(t.vertices[0][1], weights[0]))
                .wrapping_add(fx::mul(t.vertices[2][1], weights[2]));
            if best.is_none_or(|(_, h)| h < height) {
                best = Some((i, height));
            }
        }
    }
    let (index, _) = best?;
    let [v0, v1, v2] = tris[index].vertices;
    let plane = fx::cross(fx::sub(v1, v0), fx::sub(v2, v0));
    let point = ray_plane(a, b, plane, fx::dot(plane, v0))?;
    Some((index, point))
}

/// `0x02031d30`: the point of the triangle `a b c` nearest to `p` (vertex, edge or inside).
fn closest_point(p: Fx3, a: Fx3, b: Fx3, c: Fx3) -> Fx3 {
    let ab = fx::sub(b, a);
    let ac = fx::sub(c, a);
    let ap = fx::sub(p, a);
    let d1 = fx::dot(ab, ap);
    let d2 = fx::dot(ac, ap);
    if d1 < 1 && d2 < 1 {
        return a;
    }
    let bp = fx::sub(p, b);
    let d3 = fx::dot(ab, bp);
    let d4 = fx::dot(ac, bp);
    if d3 >= 0 && d3 >= d4 {
        return b;
    }
    let edge = |a: Fx3, v: Fx3, t: i32| fx::add(a, fx::scale(v, t));
    if fx::mul(d1, d4).wrapping_sub(fx::mul(d3, d2)) < 1 && d1 >= 0 && d3 < 1 {
        return edge(a, ab, fx::div(d1, d1 - d3));
    }
    let cp = fx::sub(p, c);
    let d5 = fx::dot(ab, cp);
    let d6 = fx::dot(ac, cp);
    if d6 >= 0 && d6 >= d5 {
        return c;
    }
    if fx::mul(d2, d5).wrapping_sub(fx::mul(d1, d6)) < 1 && d2 >= 0 && d6 < 1 {
        return edge(a, ac, fx::div(d2, d2 - d6));
    }
    let vc = d4.wrapping_sub(d3);
    let vb = d5.wrapping_sub(d6);
    if fx::mul(d3, d6).wrapping_sub(fx::mul(d5, d4)) < 1 && vc >= 0 && vb >= 0 {
        return edge(b, fx::sub(c, b), fx::div(vc, vc + vb));
    }
    p
}

/// `0x020315c8`: pushes a sphere of `radius` centred at `center` out of the non-floor
/// triangles (normal `y` at most 0.5): a first pass for faces the sphere's axis crosses, a
/// second for edges and corners it only grazes. `previous` (the position before the move)
/// lets a corner trap send the sphere back. Returns the new centre and the triangle that
/// moved it.
fn push_from_walls(
    tris: &[Tri],
    center: Fx3,
    radius: i32,
    previous: Option<Fx3>,
) -> (Fx3, Option<usize>) {
    let radius_sq = fx::mul(radius, radius);
    let mut hit: Option<usize> = None;
    let mut out = center;
    let mut last = center;
    let mut last_normal = [0; 3];
    for pass in 0..2 {
        let mut faces = 0;
        for (i, t) in tris.iter().enumerate() {
            let before = out;
            let mut moved = false;
            let mut face = false;
            if t.normal[1] <= FLOOR_NORMAL_Y {
                let end = fx::add(out, fx::scale(t.normal, -radius));
                match ray_triangle(out, end, t) {
                    Ray::Hit { t: along, .. } if pass == 0 => {
                        let on_plane = fx::add(out, fx::scale(fx::sub(end, out), along));
                        out = fx::add(on_plane, fx::scale(t.normal, radius));
                        moved = true;
                        face = true;
                        faces += 1;
                        hit = Some(i);
                    }
                    Ray::Outside if pass > 0 => {
                        let near = closest_point(out, t.vertices[0], t.vertices[1], t.vertices[2]);
                        let d = fx::sub(out, near);
                        let d2 = fx::mul(d[0], d[0])
                            .wrapping_add(fx::mul(d[1], d[1]))
                            .wrapping_add(fx::mul(d[2], d[2]));
                        if d2 < radius_sq {
                            let k = fx::div(radius, fx::sqrt(d2));
                            out = fx::add(near, fx::scale(d, k));
                            moved = true;
                            hit.get_or_insert(i);
                        }
                    }
                    _ => {}
                }
            }
            if !moved {
                continue;
            }
            if let Some(previous) = previous
                && fx::distance(out, last) > 4
            {
                let alignment = fx::dot(t.normal, last_normal);
                if alignment < 0xfd7 && alignment > 0x800 {
                    let behind = [previous[0], center[1], previous[2]];
                    let e = fx::sub(behind, last);
                    let f = fx::sub(behind, out);
                    if fx::dot(e, f) < fx::distance(e, f).wrapping_mul(0x4cc) {
                        let g = fx::sub(behind, center);
                        if fx::dot(e, g) < 0 || fx::dot(f, g) < 0 {
                            out[0] = previous[0];
                            out[2] = previous[2];
                            return (out, hit);
                        }
                    }
                }
            }
            if face || faces == 0 {
                last_normal = t.normal;
            }
            last = before;
        }
    }
    (out, hit)
}

#[cfg(test)]
mod tests;
