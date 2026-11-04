//! Bounds-checked little-endian readers.

use crate::{Error, Result};

pub(crate) fn slice(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    offset
        .checked_add(len)
        .and_then(|end| data.get(offset..end))
        .ok_or(Error::OutOfBounds {
            offset,
            len,
            size: data.len(),
        })
}

pub(crate) fn u8_at(data: &[u8], offset: usize) -> Result<u8> {
    Ok(slice(data, offset, 1)?[0])
}

pub(crate) fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    let b = slice(data, offset, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let b = slice(data, offset, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub(crate) fn magic_at(data: &[u8], offset: usize) -> Result<[u8; 4]> {
    let b = slice(data, offset, 4)?;
    Ok([b[0], b[1], b[2], b[3]])
}

/// Range `[start, end)` given as u32 file offsets, validated against `data`.
pub(crate) fn range(data: &[u8], start: u32, end: u32) -> Result<&[u8]> {
    let (start, end) = (start as usize, end as usize);
    if end < start {
        return Err(Error::Malformed("range end precedes start"));
    }
    slice(data, start, end - start)
}

/// CRC-16/MODBUS as used by the DS header and banner checksums.
pub(crate) fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_matches_reference_vector() {
        // CRC-16/MODBUS check value.
        assert_eq!(crc16(b"123456789"), 0x4B37);
    }

    #[test]
    fn slice_rejects_overflow() {
        assert!(slice(&[0; 4], usize::MAX, 2).is_err());
        assert!(slice(&[0; 4], 3, 2).is_err());
        assert_eq!(slice(&[1, 2, 3, 4], 2, 2).unwrap(), &[3, 4]);
    }
}
