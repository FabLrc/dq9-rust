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

use std::collections::HashMap;

use dq9_formats::map::{Collision, Map, PartKind, fix32_cos, fix32_sin};
use glam::Vec3;
use nds_rom::compress::decompress;
use nds_rom::{Error, Narc, Result};

use crate::fx::{self, Fx3};

mod geometry;

use self::geometry::{
    Ray, angle_f32, ground_ray, inverse_matrix, push_from_walls, query, ray_triangle, rotation_y,
    turn_vector,
};
#[cfg(test)]
use self::geometry::{candidate_cells, closest_point};

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
    /// Hidden by the chosen time of day (`Instance::hidden`) or by a runtime action
    /// (0x95/0x96, native 574). A hidden node hides its subtree from collision queries.
    hidden: bool,
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
    /// Counts the placement changes that alter the collidable geometry, so callers can drop
    /// cached queries (the pilot's walkable grid, `dq9-app`).
    revision: u64,
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
                    hidden: instance.hidden,
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
            revision: 0,
        }
    }

    /// Native 584 (`0x02018b44`, and `Field_TouchDoorways` for a doorway with no destination):
    /// rotates placement `placement` of block entry `block` and its whole subtree (model
    /// placement and collision child) by `angle`, a delta in `fx32` added to the placement's
    /// own rotation (`+0x1922` = +90°). Rotates the collision pieces in place, from the
    /// transforms read at build time, without re-parsing anything.
    /// Returns whether any collision piece moved.
    pub fn set_door_angle(&mut self, block: u32, placement: u16, angle: i32) -> bool {
        let Some(root) = self.node_of(block, placement) else {
            return false;
        };
        let pivot = self.nodes[root].position;
        let (sin, cos) = (fix32_sin(angle), fix32_cos(angle));
        let mut moved = false;
        for index in 0..self.pieces.len() {
            if !self.piece_under(index, root) {
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

    /// Trigger actions 0x95/0x96 (`Trig_RunAction` cases 0x31/0x32) and natives 574/575
    /// (`ov_001::02163dec`, `02013390`) set or clear bit 2 of placement `placement`'s `+2`:
    /// its node and subtree become hidden or shown. A hidden placement's collision is not
    /// queried (`Zone3D_FindPlacement` `0x02019518` names it by block entry and instance id).
    /// Returns whether the placement changed.
    pub fn set_placement_hidden(&mut self, block: u32, placement: u16, hidden: bool) -> bool {
        let Some(root) = self.node_of(block, placement) else {
            return false;
        };
        if self.nodes[root].hidden == hidden {
            return false;
        }
        self.nodes[root].hidden = hidden;
        if (0..self.pieces.len()).any(|index| self.piece_under(index, root)) {
            self.revision = self.revision.wrapping_add(1);
        }
        true
    }

    /// Native 575 (`ov_001::02163e50`): the placement's translation (`+8`, `+0xc`, `+0x10`)
    /// becomes `position` (world `fx32`). The node and its collision subtree move by the
    /// delta; a later door rotation still pivots on the node. Returns whether a piece moved.
    pub fn set_placement_position(&mut self, block: u32, placement: u16, position: Fx3) -> bool {
        let Some(root) = self.node_of(block, placement) else {
            return false;
        };
        let delta = fx::sub(position, self.nodes[root].position);
        if delta == [0; 3] {
            return false;
        }
        self.nodes[root].position = position;
        let mut moved = false;
        for index in 0..self.nodes.len() {
            if index != root && self.node_under(index, root) {
                self.nodes[index].position = fx::add(self.nodes[index].position, delta);
            }
        }
        for index in 0..self.pieces.len() {
            if !self.piece_under(index, root) {
                continue;
            }
            self.pieces[index].position = fx::add(self.pieces[index].position, delta);
            moved = true;
        }
        if moved {
            self.revision = self.revision.wrapping_add(1);
        }
        moved
    }

    /// Counts the runtime changes that alter the collidable geometry (placement visibility,
    /// actions 0x95/0x96, natives 574/575). Callers that cache queries compare it, as the
    /// pilot does with its walkable grid; door rotations have their own finished signal.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The node of placement `(block, id)`, the first in the map's order like
    /// [`CollisionWorld::set_placement_hidden`].
    fn node_of(&self, block: u32, id: u16) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.block == block && node.id == id)
    }

    /// The placements of the subtree rooted at `(block, id)`, root first: the `(block entry,
    /// instance id)` of the placement and of every descendant in the map's placement
    /// hierarchy (`Instance::parent`). The drawn scene has no parent link, so this is how the
    /// application hides or shows the same placements as the collision (natives 574, actions
    /// 0x95/0x96: [`CollisionWorld::set_placement_hidden`]). Empty when the map has no such
    /// placement (a world built by [`CollisionWorld::from_pieces`] has no hierarchy).
    pub fn placement_subtree(&self, block: u32, id: u16) -> Vec<(u32, u16)> {
        let Some(root) = self.node_of(block, id) else {
            return Vec::new();
        };
        let mut subtree = vec![(self.nodes[root].block, self.nodes[root].id)];
        subtree.extend(
            (0..self.nodes.len())
                .filter(|&index| index != root && self.node_under(index, root))
                .map(|index| (self.nodes[index].block, self.nodes[index].id)),
        );
        subtree
    }

    /// The placement hierarchy above `(block, id)` and the placement itself, root first: the
    /// strict ancestors are every entry but the last. Empty when the map has no such
    /// placement. The walk is capped at the number of placements so a malformed map with a
    /// parent cycle returns a cut path instead of looping.
    pub fn placement_path(&self, block: u32, id: u16) -> Vec<(u32, u16)> {
        let Some(mut node) = self.node_of(block, id) else {
            return Vec::new();
        };
        let mut path = Vec::new();
        while node != usize::MAX && path.len() <= self.nodes.len() {
            let Some(n) = self.nodes.get(node) else {
                break;
            };
            path.push((n.block, n.id));
            node = n.parent.unwrap_or(usize::MAX);
        }
        path.reverse();
        path
    }

    /// Whether the placement node of `piece` is `root` or one of its descendants.
    fn piece_under(&self, piece: usize, root: usize) -> bool {
        self.node_under(self.node_of_piece[piece], root)
    }

    /// Whether `node` is `root` or one of its descendants.
    fn node_under(&self, mut node: usize, root: usize) -> bool {
        // A valid chain is shorter than the node list; the cap makes a malformed map with a
        // parent cycle return instead of looping.
        for _ in 0..=self.nodes.len() {
            if node == usize::MAX {
                return false;
            }
            if node == root {
                return true;
            }
            node = self
                .nodes
                .get(node)
                .and_then(|n| n.parent)
                .unwrap_or(usize::MAX);
        }
        false
    }

    /// Whether the piece's placement node or one of its ancestors is hidden (time of day or
    /// a runtime action).
    fn piece_hidden(&self, piece: usize) -> bool {
        let mut node = self.node_of_piece[piece];
        for _ in 0..=self.nodes.len() {
            let Some(n) = self.nodes.get(node) else {
                return false;
            };
            if n.hidden {
                return true;
            }
            node = n.parent.unwrap_or(usize::MAX);
        }
        false
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
    /// is taken in the piece's frame, without its position **[hypothesis]** (the pieces of
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
        for (index, piece) in self.pieces.iter().enumerate() {
            if self.piece_hidden(index) {
                continue;
            }
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
        for (index, piece) in self.pieces.iter().enumerate() {
            if self.piece_hidden(index) {
                continue;
            }
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
        for (index, piece) in self.pieces.iter().enumerate() {
            if self.piece_hidden(index) {
                continue;
            }
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
            for (index, piece) in self.pieces.iter().enumerate() {
                if self.piece_hidden(index) {
                    continue;
                }
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

#[cfg(test)]
mod placement_visibility;

#[cfg(test)]
mod tests;
