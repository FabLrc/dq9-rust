//! DS geometry engine display lists (packed GPU commands), as stored in G3D meshes.
//!
//! Ported from apicula `nds::gpu_cmds`, extended to every command of GBATEK "DS 3D Geometry
//! Commands" so that matrix and material commands are reported instead of rejected. Values
//! stay in their hardware fixed-point encodings.

use super::fx::{Fx12, bits, sext};
use crate::bytes::u32_at;
use crate::{Error, Result};

/// One decoded geometry command.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuCmd {
    Nop,
    /// `MTX_MODE` (0 projection, 1 position, 2 position & vector, 3 texture).
    MatrixMode(u32),
    /// `MTX_PUSH`.
    Push,
    /// `MTX_POP` by a signed count.
    Pop(i32),
    /// `MTX_STORE` to stack slot.
    Store(u32),
    /// `MTX_RESTORE` from stack slot.
    Restore(u32),
    /// `MTX_IDENTITY`.
    Identity,
    /// `MTX_LOAD_4x4`, `MTX_LOAD_4x3`, `MTX_MULT_4x4`, `MTX_MULT_4x3`, `MTX_MULT_3x3`: the
    /// entries in DS order (row-major for row vectors).
    LoadMatrix(Vec<Fx12>),
    MultMatrix(Vec<Fx12>),
    /// `MTX_SCALE`.
    Scale([Fx12; 3]),
    /// `MTX_TRANS`.
    Translate([Fx12; 3]),
    /// `COLOR`, RGB555.
    Color(u16),
    /// `NORMAL`, three signed 1.0.9 values (raw /512).
    Normal([i16; 3]),
    /// `TEXCOORD`, signed 1.11.4 texel coordinates (raw /16).
    TexCoord([i16; 2]),
    /// Any of the `VTX_*` commands, resolved to an absolute position (relative forms use the
    /// previous vertex).
    Vertex([Fx12; 3]),
    /// `POLYGON_ATTR`, `TEXIMAGE_PARAM`, `PLTT_BASE`, `DIF_AMB`, `SPE_EMI`, `LIGHT_VECTOR`,
    /// `LIGHT_COLOR`, `SHININESS`, `SWAP_BUFFERS`, `VIEWPORT`, tests: opcode and parameters.
    Other {
        opcode: u8,
        params: Vec<u32>,
    },
    /// `BEGIN_VTXS`: 0 triangles, 1 quads, 2 triangle strip, 3 quad strip.
    Begin(u32),
    /// `END_VTXS`.
    End,
}

/// Number of 32-bit parameters of a geometry command, `None` for invalid opcodes.
pub fn param_count(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0x00 | 0x11 | 0x15 | 0x41 => 0,
        0x10 | 0x12 | 0x13 | 0x14 => 1,
        0x16 | 0x18 => 16,
        0x17 | 0x19 => 12,
        0x1A => 9,
        0x1B | 0x1C => 3,
        0x20..=0x22 | 0x24..=0x2B | 0x30..=0x33 | 0x40 | 0x50 | 0x60 | 0x72 => 1,
        0x23 | 0x71 => 2,
        0x34 => 32,
        0x70 => 3,
        _ => return None,
    })
}

/// Iterator over the commands of a packed display list: each word holds four opcodes, then
/// come the parameters of those four commands in order. Stops after the first error.
#[derive(Debug, Clone)]
pub struct DisplayList<'a> {
    data: &'a [u8],
    pos: usize,
    opcodes: [u8; 4],
    pending: usize,
    vertex: [Fx12; 3],
    done: bool,
}

impl<'a> DisplayList<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            opcodes: [0; 4],
            pending: 0,
            vertex: [Fx12::ZERO; 3],
            done: false,
        }
    }

    fn step(&mut self) -> Result<Option<GpuCmd>> {
        if self.pending == 0 {
            if self.pos >= self.data.len() {
                return Ok(None);
            }
            self.opcodes = u32_at(self.data, self.pos)?.to_le_bytes();
            self.pos += 4;
            self.pending = 4;
        }
        let opcode = self.opcodes[4 - self.pending];
        self.pending -= 1;
        let count = param_count(opcode).ok_or(Error::Malformed("unknown GPU command"))?;
        let mut params = Vec::with_capacity(count);
        for _ in 0..count {
            params.push(u32_at(self.data, self.pos)?);
            self.pos += 4;
        }
        let p = |i: usize| params[i];
        let fx16 = |v: u32| Fx12(sext(v, 16));
        let cmd = match opcode {
            0x00 => GpuCmd::Nop,
            0x10 => GpuCmd::MatrixMode(p(0) & 3),
            0x11 => GpuCmd::Push,
            0x12 => GpuCmd::Pop(sext(p(0), 6)),
            0x13 => GpuCmd::Store(p(0) & 31),
            0x14 => GpuCmd::Restore(p(0) & 31),
            0x15 => GpuCmd::Identity,
            0x16 | 0x17 => GpuCmd::LoadMatrix(params.iter().map(|&v| Fx12(v as i32)).collect()),
            0x18..=0x1A => GpuCmd::MultMatrix(params.iter().map(|&v| Fx12(v as i32)).collect()),
            0x1B => GpuCmd::Scale([0, 1, 2].map(|i| Fx12(p(i) as i32))),
            0x1C => GpuCmd::Translate([0, 1, 2].map(|i| Fx12(p(i) as i32))),
            0x20 => GpuCmd::Color(bits(p(0), 0, 15) as u16),
            0x21 => GpuCmd::Normal([0, 10, 20].map(|lo| sext(p(0) >> lo, 10) as i16)),
            0x22 => GpuCmd::TexCoord([0, 16].map(|lo| sext(p(0) >> lo, 16) as i16)),
            0x23 => self.vertex([fx16(p(0)), fx16(p(0) >> 16), fx16(p(1))]),
            // VTX_10: 1.3.6.
            0x24 => self.vertex([0, 10, 20].map(|lo| Fx12(sext(p(0) >> lo, 10) << 6))),
            0x25 => self.vertex([fx16(p(0)), fx16(p(0) >> 16), self.vertex[2]]),
            0x26 => self.vertex([fx16(p(0)), self.vertex[1], fx16(p(0) >> 16)]),
            0x27 => self.vertex([self.vertex[0], fx16(p(0)), fx16(p(0) >> 16)]),
            // VTX_DIFF: signed 10-bit offsets in units of 1/4096 (1.0.9 scaled by 1/8).
            0x28 => {
                let d = [0, 10, 20].map(|lo| sext(p(0) >> lo, 10));
                let v = self.vertex;
                self.vertex(std::array::from_fn(|i| Fx12(v[i].0.wrapping_add(d[i]))))
            }
            0x40 => GpuCmd::Begin(p(0) & 3),
            0x41 => GpuCmd::End,
            _ => GpuCmd::Other { opcode, params },
        };
        Ok(Some(cmd))
    }

    fn vertex(&mut self, v: [Fx12; 3]) -> GpuCmd {
        self.vertex = v;
        GpuCmd::Vertex(v)
    }
}

impl Iterator for DisplayList<'_> {
    type Item = Result<GpuCmd>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let r = self.step().transpose();
        if !matches!(r, Some(Ok(_))) {
            self.done = true;
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(w: &[u32]) -> Vec<u8> {
        w.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn decodes_packed_commands() {
        let dl = words(&[
            u32::from_le_bytes([0x40, 0x23, 0x28, 0x22]),
            0,
            0x2000_1000,
            0xFFFF_F000,
            (0x3FF << 20) | 1,
            (0xFFF0 << 16) | 0x20,
            u32::from_le_bytes([0x24, 0x21, 0x41, 0x00]),
            (0x200 << 20) | 1,
            0x1FF,
        ]);
        let cmds: Vec<_> = DisplayList::new(&dl).collect::<Result<_>>().unwrap();
        assert_eq!(
            cmds,
            vec![
                GpuCmd::Begin(0),
                GpuCmd::Vertex([Fx12(0x1000), Fx12(0x2000), Fx12(-0x1000)]),
                GpuCmd::Vertex([Fx12(0x1001), Fx12(0x2000), Fx12(-0x1001)]),
                GpuCmd::TexCoord([0x20, -16]),
                GpuCmd::Vertex([Fx12(64), Fx12(0), Fx12(-512 << 6)]),
                GpuCmd::Normal([511, 0, 0]),
                GpuCmd::End,
                GpuCmd::Nop,
            ]
        );
    }

    #[test]
    fn errors_stop_iteration() {
        let dl = words(&[u32::from_le_bytes([0x23, 0, 0, 0]), 0]);
        let mut it = DisplayList::new(&dl);
        assert!(it.next().unwrap().is_err());
        assert!(it.next().is_none());
        let dl = words(&[0xFF]);
        assert!(DisplayList::new(&dl).any(|c| c.is_err()));
        assert!(DisplayList::new(&[1, 2]).any(|c| c.is_err()));
    }
}
