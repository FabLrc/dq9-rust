//! Nintendo compression formats.
//!
//! * [`lz`]: forward LZ77 variants `0x10` (LZ10) and `0x11` (LZ11) used for asset files.
//! * [`blz`]: "bottom LZ", decoded back-to-front in place, used for the ARM9 binary and
//!   overlays.

use crate::bytes::{u8_at, u32_at};
use crate::{Error, Result};

/// Largest decompressed size accepted from a stream header (guards against garbage headers).
const MAX_OUTPUT: usize = 64 << 20;

/// Decompresses an LZ10/LZ11 stream (type byte `0x10` / `0x11`).
pub fn lz(data: &[u8]) -> Result<Vec<u8>> {
    let header = u32_at(data, 0)?;
    let kind = header as u8;
    let (mut size, mut pos) = ((header >> 8) as usize, 4);
    if size == 0 {
        // Extended header: real size follows.
        size = u32_at(data, 4)? as usize;
        pos = 8;
    }
    if !matches!(kind, 0x10 | 0x11) {
        return Err(Error::Malformed("not an LZ10/LZ11 stream"));
    }
    if size > MAX_OUTPUT {
        return Err(Error::Malformed("LZ declared size too large"));
    }
    let mut out = Vec::with_capacity(size);
    let mut next = || -> Result<usize> {
        let b = u8_at(data, pos)?;
        pos += 1;
        Ok(usize::from(b))
    };
    while out.len() < size {
        let flags = next()?;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags >> bit & 1 == 0 {
                out.push(next()? as u8);
                continue;
            }
            let b0 = next()?;
            let (len, disp) = if kind == 0x10 {
                let b1 = next()?;
                ((b0 >> 4) + 3, ((b0 & 0xF) << 8 | b1) + 1)
            } else {
                match b0 >> 4 {
                    0 => {
                        let (b1, b2) = (next()?, next()?);
                        (
                            ((b0 & 0xF) << 4 | b1 >> 4) + 0x11,
                            ((b1 & 0xF) << 8 | b2) + 1,
                        )
                    }
                    1 => {
                        let (b1, b2, b3) = (next()?, next()?, next()?);
                        let len = ((b0 & 0xF) << 12 | b1 << 4 | b2 >> 4) + 0x111;
                        (len, ((b2 & 0xF) << 8 | b3) + 1)
                    }
                    n => {
                        let b1 = next()?;
                        (n + 1, ((b0 & 0xF) << 8 | b1) + 1)
                    }
                }
            };
            let start = out
                .len()
                .checked_sub(disp)
                .ok_or(Error::Malformed("LZ back-reference before start"))?;
            // Byte-wise copy: source and destination may overlap.
            for i in 0..len.min(size - out.len()) {
                out.push(out[start + i]);
            }
        }
    }
    Ok(out)
}

/// Decompresses a BLZ ("bottom LZ") image such as a compressed overlay.
///
/// Footer (last 8 bytes): `u32 { enc_len: 24, header_len: 8 }`, `u32 extra_len`.
/// The first `len - enc_len` bytes are stored uncompressed.
pub fn blz(data: &[u8]) -> Result<Vec<u8>> {
    let len = data.len();
    let footer = u32_at(
        data,
        len.checked_sub(8)
            .ok_or(Error::Malformed("BLZ too short"))?,
    )?;
    let extra = u32_at(data, len - 4)? as usize;
    let header_len = (footer >> 24) as usize;
    let enc_len = (footer & 0x00FF_FFFF) as usize;
    if header_len < 8 || header_len > enc_len || enc_len > len {
        return Err(Error::Malformed("BLZ footer"));
    }
    if extra > MAX_OUTPUT {
        return Err(Error::Malformed("BLZ declared size too large"));
    }
    let mut buf = Vec::with_capacity(len + extra);
    buf.extend_from_slice(data);
    buf.resize(len + extra, 0);

    let stop = len - enc_len;
    let (mut r, mut w) = (len - header_len, len + extra);
    let corrupt = Error::Malformed("BLZ stream");
    while r > stop {
        r -= 1;
        let flags = buf[r];
        for bit in (0..8).rev() {
            if r <= stop {
                break;
            }
            if flags >> bit & 1 == 0 {
                r -= 1;
                w = w.checked_sub(1).ok_or(corrupt.clone())?;
                buf[w] = buf[r];
            } else {
                if r - stop < 2 {
                    return Err(corrupt);
                }
                r -= 2;
                let val = usize::from(u16::from_le_bytes([buf[r], buf[r + 1]]));
                let (count, disp) = ((val >> 12) + 3, (val & 0xFFF) + 3);
                for _ in 0..count {
                    w = w.checked_sub(1).ok_or(corrupt.clone())?;
                    buf[w] = *buf.get(w + disp).ok_or(corrupt.clone())?;
                }
            }
            // Output must never overtake unread input.
            if w < r {
                return Err(corrupt);
            }
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz10_overlapping_reference() {
        let data = [0x10, 9, 0, 0, 0b0001_0000, b'a', b'b', b'c', 0x30, 0x02];
        assert_eq!(lz(&data).unwrap(), b"abcabcabc");
    }

    #[test]
    fn lz11_all_reference_forms() {
        // 3 literals, then: short ref (len 6), 3-byte ref (len 0x11), 4-byte ref (len 0x111).
        let mut data = vec![0x11];
        let size = 3 + 6 + 0x11 + 0x111;
        data.extend(&(size as u32).to_le_bytes()[..3]);
        data.extend([0b0001_1100, b'a', b'b', b'c']);
        data.extend([0x50, 0x02]); // len 6, disp 3
        data.extend([0x00, 0x00, 0x02]); // len 0x11, disp 3
        data.extend([0x10, 0x00, 0x00, 0x02]); // len 0x111, disp 3
        let out = lz(&data).unwrap();
        assert_eq!(out.len(), size);
        assert!(out.chunks(3).all(|c| c == &b"abc"[..c.len()]));
    }

    #[test]
    fn lz_rejects_reference_before_start() {
        assert!(lz(&[0x10, 4, 0, 0, 0b1000_0000, 0x10, 0x00]).is_err());
    }

    #[test]
    fn lz_rejects_truncated_input() {
        assert!(lz(&[0x10, 9, 0, 0, 0, b'a']).is_err());
    }

    #[test]
    fn blz_backward_decode() {
        // Prefix "XY" stored raw; region decodes to literals "abc" followed (backwards) by a
        // length-18, distance-3 copy.
        let mut data = b"XY".to_vec();
        data.extend([0x00, 0xF0, b'a', b'b', b'c', 0b0001_0000]);
        let enc_len = 2 + 3 + 1 + 8;
        data.extend(((8u32 << 24) | enc_len).to_le_bytes());
        data.extend(7u32.to_le_bytes()); // 23 output bytes - 16 input bytes
        let out = blz(&data).unwrap();
        assert_eq!(out, [&b"XY"[..], &b"abc".repeat(7)].concat());
    }

    #[test]
    fn blz_rejects_bad_footer() {
        assert!(blz(&[0; 4]).is_err());
        let mut data = vec![0u8; 8];
        data[..4].copy_from_slice(&((8u32 << 24) | 100).to_le_bytes());
        assert!(blz(&data).is_err());
    }
}
