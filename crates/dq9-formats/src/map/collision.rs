//! Parser for EU map collision meshes (`.col2`).
//!
//! Layout and field evidence: [`docs/re/collision.md`](../../../../docs/re/collision.md).

use std::ops::Range;

use nds_rom::bytes::{u16_at, u32_at};
use nds_rom::{Error, Result};

const HEADER_SIZE: usize = 0x3c;
const TRIANGLE_SIZE: usize = 28;
const RECORD_SIZE: usize = 8;

/// A triangle in the coordinate units stored in the file (before applying `shift`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    pub vertices: [[i16; 3]; 3],
    /// Signed 1.3.12 components; `-4096` is `-1.0`.
    pub normal: [i16; 3],
    /// Meaning is not established beyond the inferred top-seven-bit ground-record index.
    pub attributes: u32,
}

/// One EU `.col2` ground record. Only the first two fields have documented uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroundRecord {
    pub map_digits: u16,
    pub flags: u16,
    pub unknown: [u16; 2],
}

/// Parsed map collision geometry and its validated spatial index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    pub shift: u32,
    /// Stored integer bounds: min x/y/z then max x/y/z.
    pub bounds: [i16; 6],
    pub cell_size: u16,
    pub grid_x: u32,
    pub grid_z: u32,
    pub triangles: Vec<Triangle>,
    /// Cell ranges into [`Self::cell_indices`].
    pub cells: Vec<Range<usize>>,
    /// Concatenated triangle indices for all cells.
    pub cell_indices: Vec<u16>,
    pub ground_records: Vec<GroundRecord>,
}

impl Collision {
    /// Parse a complete EU `.col2` file, rejecting truncated or inconsistent tables.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::Malformed("truncated COL2 header"));
        }
        if u32_at(bytes, 0)? != 3 {
            return Err(Error::Malformed("unsupported COL2 version"));
        }
        let shift = u32_at(bytes, 4)?;
        if shift > 5 {
            return Err(Error::Malformed("invalid COL2 shift"));
        }
        let mut bounds = [0; 6];
        for (i, value) in bounds.iter_mut().enumerate() {
            *value = i16_at(bytes, 8 + i * 2)?;
        }
        for axis in 0..3 {
            if bounds[axis] > bounds[axis + 3] {
                return Err(Error::Malformed("inverted COL2 bounds"));
            }
        }
        let triangle_count = usize_at(u32_at(bytes, 0x14)?)?;
        let cell_size = u16_at(bytes, 0x18)?;
        let grid_x = u32_at(bytes, 0x1c)?;
        let grid_z = u32_at(bytes, 0x20)?;
        if cell_size == 0 || !cell_size.is_power_of_two() || grid_x == 0 || grid_z == 0 {
            return Err(Error::Malformed("invalid COL2 grid dimensions"));
        }
        let triangle_offset = usize_at(u32_at(bytes, 0x24)?)?;
        let count_offset = usize_at(u32_at(bytes, 0x28)?)?;
        let start_offset = usize_at(u32_at(bytes, 0x2c)?)?;
        let index_offset = usize_at(u32_at(bytes, 0x30)?)?;
        let record_count = usize_at(u32_at(bytes, 0x34)?)?;
        let record_offset = usize_at(u32_at(bytes, 0x38)?)?;

        let rows = usize_at(grid_z)?;
        let columns = usize_at(grid_x)?;
        let cell_count = columns
            .checked_mul(rows)
            .and_then(|n| n.checked_add(rows / 2))
            .ok_or(Error::Malformed("COL2 grid size overflow"))?;
        let triangle_bytes = triangle_count
            .checked_mul(TRIANGLE_SIZE)
            .ok_or(Error::Malformed("COL2 triangle size overflow"))?;
        let grid_count_end = checked_end(count_offset, cell_count, bytes.len())?;
        let grid_start_end = checked_end(
            start_offset,
            cell_count
                .checked_mul(2)
                .ok_or(Error::Malformed("COL2 grid size overflow"))?,
            bytes.len(),
        )?;
        let record_bytes = record_count
            .checked_mul(RECORD_SIZE)
            .ok_or(Error::Malformed("COL2 records size overflow"))?;

        checked_end(
            index_offset,
            bytes.len().saturating_sub(index_offset),
            bytes.len(),
        )?;
        if record_offset < index_offset {
            return Err(Error::Malformed("invalid COL2 index-table range"));
        }
        if (record_offset - index_offset) % 2 != 0 {
            return Err(Error::Malformed("odd COL2 triangle-index section size"));
        }
        let record_end = checked_end(record_offset, record_bytes, bytes.len())?;
        if record_end != bytes.len()
            || triangle_offset < HEADER_SIZE
            || count_offset < triangle_offset + triangle_bytes
            || start_offset < grid_count_end
            || index_offset < grid_start_end
            || record_offset < index_offset
        {
            return Err(Error::Malformed("invalid COL2 section layout"));
        }
        let counts = &bytes[count_offset..grid_count_end];
        let mut starts = Vec::with_capacity(cell_count);
        for i in 0..cell_count {
            starts.push(u16_at(bytes, start_offset + 2 * i)? as usize);
        }
        let (pairs, _) = bytes[index_offset..record_offset].as_chunks::<2>();
        let mut indices = Vec::with_capacity(pairs.len());
        for pair in pairs {
            indices.push(u16::from_le_bytes(*pair));
        }
        let mut cells = Vec::with_capacity(cell_count);
        let mut expected_start = 0usize;
        for (i, &count) in counts.iter().enumerate() {
            let start = starts[i];
            let end = start
                .checked_add(usize::from(count))
                .ok_or(Error::Malformed("COL2 cell index overflow"))?;
            if start != expected_start || end > indices.len() {
                return Err(Error::Malformed("non-contiguous COL2 cell index table"));
            }
            if indices[start..end]
                .iter()
                .any(|&index| usize::from(index) >= triangle_count)
            {
                return Err(Error::Malformed("COL2 cell references missing triangle"));
            }
            cells.push(start..end);
            expected_start = end;
        }
        if indices.len().saturating_sub(expected_start) > 1
            || indices[expected_start..].iter().any(|&index| index != 0)
        {
            return Err(Error::Malformed("unexpected COL2 index padding"));
        }

        indices.truncate(expected_start);
        let mut triangles = Vec::with_capacity(triangle_count);
        for i in 0..triangle_count {
            let at = triangle_offset + i * TRIANGLE_SIZE;
            let mut vertices = [[0; 3]; 3];
            for (vertex_index, vertex) in vertices.iter_mut().enumerate() {
                for (axis, value) in vertex.iter_mut().enumerate() {
                    *value = i16_at(bytes, at + vertex_index * 6 + axis * 2)?;
                }
            }
            let mut normal = [0; 3];
            for (axis, value) in normal.iter_mut().enumerate() {
                *value = i16_at(bytes, at + 0x12 + axis * 2)?;
            }
            triangles.push(Triangle {
                vertices,
                normal,
                attributes: u32_at(bytes, at + 0x18)?,
            });
        }
        let mut ground_records = Vec::with_capacity(record_count);
        for i in 0..record_count {
            let at = record_offset + i * RECORD_SIZE;
            ground_records.push(GroundRecord {
                map_digits: u16_at(bytes, at)?,
                flags: u16_at(bytes, at + 2)?,
                unknown: [u16_at(bytes, at + 4)?, u16_at(bytes, at + 6)?],
            });
        }
        Ok(Self {
            shift,
            bounds,
            cell_size,
            grid_x,
            grid_z,
            triangles,
            cells,
            cell_indices: indices,
            ground_records,
        })
    }
}

fn checked_end(offset: usize, len: usize, size: usize) -> Result<usize> {
    let end = offset
        .checked_add(len)
        .ok_or(Error::Malformed("COL2 section size overflow"))?;
    if end > size {
        return Err(Error::OutOfBounds { offset, len, size });
    }
    Ok(end)
}

fn usize_at(value: u32) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::Malformed("COL2 value exceeds address space"))
}

fn i16_at(bytes: &[u8], at: usize) -> Result<i16> {
    Ok(u16_at(bytes, at)? as i16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; HEADER_SIZE + TRIANGLE_SIZE + 14];
        put_u32(&mut bytes, 0, 3);
        put_u32(&mut bytes, 4, 1);
        for (i, value) in [-2i16, 0, -2, 2, 0, 2].into_iter().enumerate() {
            put_u16(&mut bytes, 8 + i * 2, value as u16);
        }
        put_u32(&mut bytes, 0x14, 1);
        put_u16(&mut bytes, 0x18, 4);
        put_u32(&mut bytes, 0x1c, 1);
        put_u32(&mut bytes, 0x20, 1);
        put_u32(&mut bytes, 0x24, HEADER_SIZE as u32);
        put_u32(&mut bytes, 0x28, (HEADER_SIZE + TRIANGLE_SIZE) as u32);
        put_u32(&mut bytes, 0x2c, (HEADER_SIZE + TRIANGLE_SIZE + 2) as u32);
        put_u32(&mut bytes, 0x30, (HEADER_SIZE + TRIANGLE_SIZE + 4) as u32);
        put_u32(&mut bytes, 0x34, 1);
        put_u32(&mut bytes, 0x38, (HEADER_SIZE + TRIANGLE_SIZE + 6) as u32);
        let tri = HEADER_SIZE;
        for (i, value) in [-2i16, 0, -2, 2, 0, -2, 0, 0, 2, 0, 4096, 0]
            .into_iter()
            .enumerate()
        {
            put_u16(&mut bytes, tri + i * 2, value as u16);
        }
        put_u32(&mut bytes, tri + 0x18, 0x1234);
        put_u16(&mut bytes, HEADER_SIZE + TRIANGLE_SIZE, 1);
        put_u16(&mut bytes, HEADER_SIZE + TRIANGLE_SIZE + 2, 0);
        put_u16(&mut bytes, HEADER_SIZE + TRIANGLE_SIZE + 4, 0);
        put_u16(&mut bytes, HEADER_SIZE + TRIANGLE_SIZE + 6, 23254);
        bytes
    }

    fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn parses_geometry_index_and_ground_record() {
        let mesh = Collision::parse(&fixture()).unwrap();
        assert_eq!(mesh.shift, 1);
        assert_eq!(mesh.bounds, [-2, 0, -2, 2, 0, 2]);
        assert_eq!(mesh.triangles[0].vertices[1], [2, 0, -2]);
        assert_eq!(mesh.triangles[0].normal, [0, 4096, 0]);
        assert_eq!(mesh.triangles[0].attributes, 0x1234);
        assert_eq!(mesh.cells.len(), 1);
        assert_eq!(mesh.cells[0], 0..1);
        assert_eq!(mesh.cell_indices, [0]);
        assert_eq!(mesh.ground_records[0].map_digits, 23254);
    }

    #[test]
    fn rejects_truncated_and_inconsistent_collision_files() {
        assert!(Collision::parse(&fixture()[..20]).is_err());
        let mut bad = fixture();
        put_u32(&mut bad, 0x14, u32::MAX);
        assert!(Collision::parse(&bad).is_err());
        let mut bad = fixture();
        put_u16(&mut bad, HEADER_SIZE + TRIANGLE_SIZE + 2, 1);
        assert!(Collision::parse(&bad).is_err());
    }
}
