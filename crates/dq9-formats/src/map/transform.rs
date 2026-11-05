//! Placement hierarchy and world transforms: the game links each placement's instance to
//! its parent (`0x020177e4`), then walks the tree from the first instance (`0x0201311c`,
//! `0x02013168`). Only the `y` rotation is propagated; scales multiply, translations turn
//! with the parent's angle but are not scaled.
//!
//! port: 0x02013028, 0x020130c0, 0x020130f0, 0x0201311c, 0x02013168, 0x02030c78, 0x02030cac,
//! port: 0x02030f40

use super::manifest::Placement;
use nds_rom::{Error, Result};

/// 2π in `fx32` (`FIX_2PI`).
const TWO_PI: i32 = 0x6488;

/// `fx32` position, `y` angle (`fx32` radians in `[0, 2π]`) and scale of an instance
/// (`Node+8`, `+6`, `+0x14`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transform {
    pub position: [i32; 3],
    pub angle: i16,
    pub scale: [i32; 3],
}

impl Default for Transform {
    /// An instance the walk does not reach keeps these (`0x02013028`).
    fn default() -> Self {
        Self {
            position: [0; 3],
            angle: 0,
            scale: [0x1000; 3],
        }
    }
}

/// Entry `i` (`0..4096`) of the SDK's sine/cosine table (`g_fixSinCosTable`,
/// `0x020e9460`): `round(4096 · sin(2πi/4096))`, `round(4096 · cos(2πi/4096))` (equal to the
/// ROM's table, checked by the real-ROM tests).
pub fn sin_cos_entry(i: usize) -> (i16, i16) {
    let a = i as f64 * std::f64::consts::TAU / 4096.0;
    (
        (a.sin() * 4096.0).round() as i16,
        (a.cos() * 4096.0).round() as i16,
    )
}

/// Table index of an `fx32` angle: its 16-bit fraction of a turn, over 16.
fn table_index(x: i32) -> usize {
    usize::from(((x << 16) / TWO_PI) as u16 >> 4)
}

/// `fix32sin` (`0x02030c78`).
pub fn fix32_sin(x: i32) -> i32 {
    i32::from(sin_cos_entry(table_index(x)).0)
}

/// `fix32cos` (`0x02030cac`).
pub fn fix32_cos(x: i32) -> i32 {
    i32::from(sin_cos_entry(table_index(x)).1)
}

/// `FX_Div` (Nitro SDK, hardware divider): `(a << 32) / b`, rounded to `fx32`.
fn fx_div(a: i32, b: i32) -> i32 {
    (((i64::from(a) << 32) / i64::from(b) + (1 << 19)) >> 20) as i32
}

/// `FIX32_MULTIPLY`: rounded `fx32` product.
fn fx_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b) + 0x800) >> 12) as i32
}

/// `fix32ReduceAngle0To2Pi` (`0x02030f40`): brings an `fx32` angle into `[0, 2π]` by whole
/// turns.
pub fn reduce_angle(angle: i32) -> i32 {
    if angle < 0 {
        let periods = (fx_div(angle.wrapping_neg(), TWO_PI) + 0x1000) & !0xfff;
        angle.wrapping_add(fx_mul(periods, TWO_PI))
    } else if angle > TWO_PI {
        let periods = fx_div(angle, TWO_PI) & !0xfff;
        angle.wrapping_sub(fx_mul(periods, TWO_PI))
    } else {
        angle
    }
}

/// One instance of the hierarchy (0x70 bytes in the game; only the fields used here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Node {
    /// `+0`: the placement's instance number.
    pub id: u16,
    /// `+0x28`: index of the parent instance.
    pub parent: Option<usize>,
    /// `+0x2c`: first child.
    first_child: Option<usize>,
    /// `+0x30`: next sibling.
    next_sibling: Option<usize>,
    /// `+6`, `+8`, `+0x14`: world transform.
    pub world: Transform,
    computed: bool,
}

/// Links one instance per placement and computes their world transforms (`0x020177e4`, then
/// `0x0201311c`). A placement with a negative parent hangs beside the first instance; one
/// naming a missing instance is reached by nothing and keeps [`Transform::default`]. Errors
/// where the game would loop forever (an instance reached twice).
pub(super) fn hierarchy(placements: &[Placement]) -> Result<Vec<Node>> {
    let mut nodes: Vec<Node> = placements
        .iter()
        .map(|p| Node {
            id: p.instance,
            parent: None,
            first_child: None,
            next_sibling: None,
            world: Transform::default(),
            computed: false,
        })
        .collect();
    for (i, p) in placements.iter().enumerate() {
        if p.parent < 0 {
            if i != 0 {
                append_sibling(&mut nodes, 0, i)?;
            }
        } else if let Some(j) = nodes
            .iter()
            .position(|n| i32::from(n.id) == i32::from(p.parent))
        {
            // `0x020130c0`.
            nodes[i].parent = Some(j);
            match nodes[j].first_child {
                None => nodes[j].first_child = Some(i),
                Some(first) => append_sibling(&mut nodes, first, i)?,
            }
        }
    }
    if !nodes.is_empty() {
        place(&mut nodes, placements, 0, &Transform::default(), 0)?;
    }
    Ok(nodes)
}

/// Appends `node` at the end of the sibling chain starting at `first` (`0x020130f0`).
fn append_sibling(nodes: &mut [Node], first: usize, node: usize) -> Result<()> {
    let mut last = first;
    for _ in 0..nodes.len() {
        match nodes[last].next_sibling {
            Some(next) => last = next,
            None => {
                nodes[last].next_sibling = Some(node);
                return Ok(());
            }
        }
    }
    Err(Error::Malformed("placement sibling chain loops"))
}

/// `0x02013168`: computes `first` and its siblings under `parent`, then their children.
fn place(
    nodes: &mut [Node],
    placements: &[Placement],
    first: usize,
    parent: &Transform,
    depth: usize,
) -> Result<()> {
    if depth > nodes.len() {
        return Err(Error::Malformed("placement hierarchy loops"));
    }
    let angle = i32::from(parent.angle);
    let (sin, cos) = (i64::from(fix32_sin(angle)), i64::from(fix32_cos(angle)));
    let compute = |nodes: &mut [Node], k: usize| -> Result<Transform> {
        let node = &mut nodes[k];
        if std::mem::replace(&mut node.computed, true) {
            return Err(Error::Malformed("placement instance reached twice"));
        }
        let p = &placements[k];
        let [x, y, z] = p.translation.map(i64::from);
        // MTX_RotY33_ then MTX_MultVec33 (row vector times matrix), then VEC_Add.
        let rotated = [(x * cos + z * sin) >> 12, y, (z * cos - x * sin) >> 12];
        let world = Transform {
            position: std::array::from_fn(|c| (rotated[c] as i32).wrapping_add(parent.position[c])),
            angle: reduce_angle(angle + i32::from(p.rotation[1])) as i16,
            scale: std::array::from_fn(|c| fx_mul(i32::from(p.scale[c]), parent.scale[c])),
        };
        node.world = world;
        Ok(world)
    };
    let world = compute(nodes, first)?;
    let mut next = nodes[first].next_sibling;
    while let Some(k) = next {
        let sibling = compute(nodes, k)?;
        if let Some(child) = nodes[k].first_child {
            place(nodes, placements, child, &sibling, depth + 1)?;
        }
        next = nodes[k].next_sibling;
    }
    if let Some(child) = nodes[first].first_child {
        place(nodes, placements, child, &world, depth + 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(instance: u16, parent: i16, translation: [i32; 3], rot_y: i16) -> Placement {
        Placement {
            instance,
            resource: 0,
            parent,
            flags: 0xf,
            translation,
            rotation: [0, rot_y, 0],
            scale: [0x1000, 0x2000, 0x800],
            name: None,
        }
    }

    #[test]
    fn trigonometry_matches_the_sdk() {
        assert_eq!(sin_cos_entry(0), (0, 4096));
        assert_eq!(sin_cos_entry(1), (6, 4096));
        assert_eq!(sin_cos_entry(1024), (4096, 0));
        // π/2 = 0x1922 is exactly a quarter turn (index 1024); 1 rad is index 651.
        assert_eq!((fix32_sin(0x1922), fix32_cos(0x1922)), (4096, 0));
        assert_eq!(fix32_sin(0x1000), i32::from(sin_cos_entry(651).0));
        assert_eq!(fix32_sin(-0x1922), -4096);
        assert_eq!(reduce_angle(-4096), TWO_PI - 4096);
        assert_eq!(reduce_angle(TWO_PI), TWO_PI);
        assert_eq!(reduce_angle(TWO_PI + 1), 1);
        assert_eq!(reduce_angle(3 * TWO_PI + 5), 5);
    }

    #[test]
    fn children_follow_their_parent() {
        let placements = [
            pl(0, -1, [0x1000, 0, 0], 0x1922),
            // Child of instance 0: turned by its angle, its scale multiplied.
            pl(5, 0, [0x1000, 0x10, 0], 0),
            // Beside instance 0.
            pl(7, -1, [0, 0, 0x3000], 0),
            // Grandchild through instance 5.
            pl(9, 5, [0, 0, 0x1000], 0),
            // Missing parent: unreached.
            pl(3, 42, [1, 2, 3], 0),
        ];
        let nodes = hierarchy(&placements).unwrap();
        let w = |i: usize| nodes[i].world;
        assert_eq!(w(0).position, [0x1000, 0, 0]);
        assert_eq!(w(0).angle, 0x1922);
        assert_eq!(w(0).scale, [0x1000, 0x2000, 0x800]);
        // x' = (x cos + z sin) >> 12 = 0, z' = (z cos - x sin) >> 12 = -0x1000.
        assert_eq!(w(1).position, [0x1000, 0x10, -0x1000]);
        assert_eq!(w(1).scale, [0x1000, 0x4000, 0x400]);
        assert_eq!(nodes[1].parent, Some(0));
        assert_eq!(w(2).position, [0, 0, 0x3000]);
        assert_eq!(w(3).position, [0x2000, 0x10, -0x1000]);
        assert_eq!(w(4), Transform::default());
        assert_eq!(nodes[4].parent, None);
    }

    #[test]
    fn loops_are_errors() {
        // Instance 0 is its own parent.
        assert!(hierarchy(&[pl(0, 0, [0; 3], 0)]).is_err());
        assert!(hierarchy(&[]).unwrap().is_empty());
    }
}
