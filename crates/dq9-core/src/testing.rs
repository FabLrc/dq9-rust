//! Synthetic `.col2` meshes for the unit tests of the collision and movement modules.

use dq9_formats::map::{Collision, Triangle};

use crate::collision::{CollisionWorld, Piece};

/// One triangle in file units (`fx32 >> shift`).
pub type Tri3 = [[i16; 3]; 3];

/// The attribute word of a triangle: six nibbles naming which of its nine coordinates are
/// the smallest and largest `x`, `y` and `z` (`docs/re/collision.md`).
fn attributes(v: &Tri3) -> u32 {
    let coord = |i: usize| v[i / 3][i % 3];
    let pick = |axis: usize, want_max: bool| {
        (0..3)
            .map(|k| k * 3 + axis)
            .reduce(|a, b| {
                let better = if want_max {
                    coord(b) > coord(a)
                } else {
                    coord(b) < coord(a)
                };
                if better { b } else { a }
            })
            .unwrap() as u32
    };
    pick(0, false)
        | pick(1, false) << 4
        | pick(2, false) << 8
        | pick(0, true) << 12
        | pick(1, true) << 16
        | pick(2, true) << 20
}

/// The unit normal of a triangle as 1.3.12.
fn normal(v: &Tri3) -> [i16; 3] {
    let f = |i: usize, a: usize| f64::from(v[i][a]);
    let e1 = [f(1, 0) - f(0, 0), f(1, 1) - f(0, 1), f(1, 2) - f(0, 2)];
    let e2 = [f(2, 0) - f(0, 0), f(2, 1) - f(0, 1), f(2, 2) - f(0, 2)];
    let c = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let length = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    c.map(|x| (x / length * 4096.0).round() as i16)
}

/// A mesh holding `triangles` in every cell of a grid of `columns × rows` cells of
/// `cell_size` file units, starting at the mesh's minimum corner.
pub fn mesh(shift: u32, triangles: &[Tri3], columns: u32, rows: u32, cell_size: u16) -> Collision {
    let mut low = [i16::MAX; 3];
    let mut high = [i16::MIN; 3];
    for t in triangles {
        for v in t {
            for a in 0..3 {
                low[a] = low[a].min(v[a]);
                high[a] = high[a].max(v[a]);
            }
        }
    }
    let count = (columns * rows + rows / 2) as usize;
    let n = triangles.len();
    Collision {
        shift,
        bounds: [low[0], low[1], low[2], high[0], high[1], high[2]],
        cell_size,
        grid_x: columns,
        grid_z: rows,
        triangles: triangles
            .iter()
            .map(|t| Triangle {
                vertices: *t,
                normal: normal(t),
                attributes: attributes(t),
            })
            .collect(),
        cells: (0..count).map(|i| i * n..(i + 1) * n).collect(),
        cell_indices: (0..count).flat_map(|_| 0..n as u16).collect(),
        ground_records: Vec::new(),
    }
}

/// A world of one piece at the origin made of `triangles`, one cell, shift 4 (16 `fx32`
/// units per file unit: one world unit is 256 file units).
pub fn world(triangles: &[Tri3]) -> CollisionWorld {
    CollisionWorld::from_pieces(
        vec![mesh(4, triangles, 1, 1, 0x4000)],
        vec![Piece {
            mesh: 0,
            position: [0; 3],
            angle: 0,
        }],
    )
}

/// Two upward triangles covering the square `[-size, size]²` at height `y` (file units).
pub fn floor(size: i16, y: i16) -> [Tri3; 2] {
    [
        [[-size, y, -size], [-size, y, size], [size, y, -size]],
        [[size, y, -size], [-size, y, size], [size, y, size]],
    ]
}

/// Two triangles forming a vertical wall on the plane `x = at`, from `z0` to `z1` and
/// `y0` to `y1`, facing `-x` (towards a character at smaller `x`).
pub fn wall_x(at: i16, z0: i16, z1: i16, y0: i16, y1: i16) -> [Tri3; 2] {
    [
        [[at, y0, z0], [at, y0, z1], [at, y1, z0]],
        [[at, y0, z1], [at, y1, z1], [at, y1, z0]],
    ]
}
