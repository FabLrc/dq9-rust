//! Free geometry of the collision queries: rotation matrices, grid cells, ray/triangle tests
//! and the wall push, shared by [`super::CollisionWorld`].
//!
//! port: 0x0204c040, 0x020310ac, 0x020312cc, 0x020315c8, 0x02031b94, 0x02031d30, 0x020320a0

use dq9_formats::map::Collision;

use crate::fx::{self, Fx3};

use super::{FLOOR_NORMAL_Y, MAX_CANDIDATES, MAX_TRIANGLES, Tri};

/// `2π - angle` as the game feeds its sine/cosine routines, whose result truncates to `fx32`
/// (`ov_017::02192b48`): the rotation matrix `MTX_RotY33_(sin, cos)` that takes a world
/// offset into the frame of a piece turned by `angle`. The argument is `0x6487 - angle`, one
/// unit short of a turn **[hypothesis]**: the SDK routines are evaluated in floating point
/// here.
pub(super) fn rotation_y(angle: i32) -> [i32; 9] {
    let argument = f64::from(0x6487 - angle) / 4096.0;
    let sin = (argument.sin() * 4096.0) as i32;
    let cos = (argument.cos() * 4096.0) as i32;
    [cos, 0, -sin, 0, 0x1000, 0, sin, 0, cos]
}

/// `MTX_MultVec33` (`0x020c17d4`).
pub(super) fn turn_vector(v: Fx3, m: &[i32; 9]) -> Fx3 {
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
pub(super) fn inverse_matrix(m: &[i32; 9]) -> Option<[i32; 9]> {
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

pub(super) fn angle_f32(angle: i32) -> f32 {
    angle as f32 / 4096.0
}

/// Whether two boxes `[max x y z, min x y z]` meet, edges included (`0x020310ac`).
pub(super) fn boxes_meet(a: &[i32; 6], b: &[i32; 6]) -> bool {
    !(a[0] < b[3] || a[1] < b[4] || a[2] < b[5] || b[0] < a[3] || b[1] < a[4]) && a[5] <= b[2]
}

/// `0x0204c040`: the indices of the triangles of `mesh` whose box meets the box `[hi, lo]`
/// (in the mesh's frame): the grid cells around the box centre, then each triangle's
/// precomputed box (the six nibbles of its attribute word).
pub(super) fn query(mesh: &Collision, hi: Fx3, lo: Fx3) -> Vec<usize> {
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
pub(super) fn triangle_box_meets(t: &dq9_formats::map::Triangle, q: &[i32; 6]) -> bool {
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
pub(super) fn candidate_cells(mesh: &Collision, q: &[i32; 6]) -> Vec<usize> {
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
pub(super) enum Ray {
    /// The segment does not cross the front of the triangle's plane.
    Miss,
    /// It crosses the plane outside the triangle.
    Outside,
    /// It crosses the triangle: barycentric weights of the three vertices and the fraction of
    /// the segment.
    Hit { weights: [i32; 3], t: i32 },
}

/// `0x02031b94`: the segment `p1 → p2` against a triangle, from its front side only.
pub(super) fn ray_triangle(p1: Fx3, p2: Fx3, t: &Tri) -> Ray {
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
pub(super) fn ray_plane(a: Fx3, b: Fx3, n: Fx3, d: i32) -> Option<Fx3> {
    let direction = fx::sub(b, a);
    let t = fx::div(d.wrapping_sub(fx::dot(n, a)), fx::dot(n, direction));
    if !(0..=0x1000).contains(&t) {
        return None;
    }
    Some(fx::add(a, fx::scale(direction, t)))
}

/// `0x020312cc`: among the floors (normal `y` above 0.5) the segment crosses, the one whose
/// interpolated height is highest, and where the segment meets its plane.
pub(super) fn ground_ray(tris: &[Tri], a: Fx3, b: Fx3) -> Option<(usize, Fx3)> {
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
pub(super) fn closest_point(p: Fx3, a: Fx3, b: Fx3, c: Fx3) -> Fx3 {
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
pub(super) fn push_from_walls(
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
