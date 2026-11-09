//! The spawn graph of a map: points of its ground linked by edges, from the `<code>P<nn>.bin`
//! member of `data/pack_lv5/path.gp2` (`ov_017::021b498c` loads it, the table handlers
//! `0x02027cf4`…`0x02027fa0` read it, `0x02028214` builds the links). Roamers spawn on its
//! nodes and walk along its edges.
//!
//! port: 0x02027cf4, 0x02027dc0, 0x02027e54, 0x02027efc, 0x02028470

use dq9_formats::table::{Table, Value};
use nds_rom::{Error, Result};

use crate::fx::{self, Fx3};

/// One point (`0x10` bytes in the game).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The id the file gives it (a byte).
    pub id: u8,
    /// The region mask a zone must meet to spawn here (a zone's bits 13–20; 0 meets any).
    pub mask: u8,
    /// Bit 1 forbids spawning; no record sets it.
    pub flags: u8,
    /// World units (integers): the file's floats truncated.
    pub position: [i16; 3],
    /// The nodes it is linked to, as indices, in the order the edges list them.
    pub links: Vec<usize>,
}

impl Node {
    /// The position as `fx32`.
    pub fn fx(&self) -> Fx3 {
        self.position.map(|c| i32::from(c) << 12)
    }
}

/// The nodes of a map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoamGraph {
    pub nodes: Vec<Node>,
}

impl RoamGraph {
    /// Reads the table. Records: `0x68` the node capacity, `0x64` a node (`id`, `mask`, then the
    /// three coordinates), `0x65` an edge (two ids), `0x66` a chain (each id linked to the next
    /// one found) and `0x67` a run (`a` linked to `a+1`, … up to `b`).
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table or a node record too short.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let int = |v: &Value<'_>| v.int();
        let mut capacity = usize::MAX;
        let mut nodes: Vec<Node> = Vec::new();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let find = |nodes: &[Node], id: u32| nodes.iter().position(|n| u32::from(n.id) == id);
        for record in &table.records {
            let v = &record.values;
            match record.kind {
                0x68 if !v.is_empty() => capacity = int(&v[0]) as usize,
                0x64 => {
                    if v.len() < 5 {
                        return Err(Error::Malformed("roam node record too short"));
                    }
                    if nodes.len() < capacity.min(0xff) {
                        // `Parameter::ToFloat` × 4096, truncated, then `>> 12`.
                        let coord =
                            |value: &Value<'_>| ((value.float() * 4096.0) as i32 >> 12) as i16;
                        nodes.push(Node {
                            id: int(&v[0]) as u8,
                            mask: int(&v[1]) as u8,
                            flags: 0,
                            position: [coord(&v[2]), coord(&v[3]), coord(&v[4])],
                            links: Vec::new(),
                        });
                    }
                }
                0x65 if v.len() >= 2 => {
                    let (a, b) = (int(&v[0]), int(&v[1]));
                    if a != b
                        && let (Some(a), Some(b)) = (find(&nodes, a), find(&nodes, b))
                    {
                        edges.push((a, b));
                    }
                }
                0x66 => {
                    let mut previous = None;
                    for value in v {
                        let found = find(&nodes, int(value));
                        if let (Some(a), Some(b)) = (previous, found) {
                            edges.push((a, b));
                        }
                        if found.is_some() {
                            previous = found;
                        }
                    }
                }
                0x67 if v.len() >= 2 => {
                    let (first, last) = (int(&v[0]), int(&v[1]));
                    let mut previous = find(&nodes, first);
                    for id in first + 1..=last {
                        let found = find(&nodes, id);
                        if let (Some(a), Some(b)) = (previous, found) {
                            edges.push((a, b));
                        }
                        if found.is_some() {
                            previous = found;
                        }
                    }
                }
                _ => {}
            }
        }
        // `0x02028214`: a node's links are its edges' other ends, in edge order.
        for &(a, b) in &edges {
            nodes[a].links.push(b);
            nodes[b].links.push(a);
        }
        Ok(Self { nodes })
    }

    /// The index of the node with `id` (`0x020283fc`, the first).
    pub fn index_of(&self, id: u8) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// The node nearest to `position` (`0x02028470`): the first of equal distance, `None` for
    /// an empty graph.
    pub fn nearest(&self, position: Fx3) -> Option<usize> {
        let mut best: Option<(i32, usize)> = None;
        for (index, node) in self.nodes.iter().enumerate() {
            let d = fx::distance(position, node.fx());
            if best.is_none_or(|(least, _)| d < least) {
                best = Some((d, index));
            }
        }
        best.map(|(_, index)| index)
    }
}
