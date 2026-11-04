//! Nintendo compression formats.
//!
//! * [`decompress`]: streams with the standard 4-byte BIOS header (type byte + 24-bit size),
//!   dispatching to the [`Codec`] bodies: LZ10 (`0x10`), LZ11 (`0x11`), Huffman (`0x24` /
//!   `0x28`) and RLE (`0x30`).
//! * [`decode`]: a codec body without header, for containers that store the codec and size
//!   themselves (e.g. Level-5 archives).
//! * [`blz`]: "bottom LZ", decoded back-to-front in place, used for the ARM9 binary and
//!   overlays.

use crate::bytes::{u8_at, u32_at};
use crate::{Error, Result};

/// Largest decompressed size accepted from a stream header (guards against garbage headers).
pub const MAX_OUTPUT: usize = 64 << 20;

/// Body encodings shared by the BIOS decompression routines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Lz10,
    Lz11,
    /// Huffman with 4-bit symbols (low nibble emitted first).
    Huffman4,
    /// Huffman with 8-bit symbols.
    Huffman8,
    Rle,
}

/// Decompresses a stream with the standard BIOS header.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let header = u32_at(data, 0)?;
    let codec = match header as u8 {
        0x10 => Codec::Lz10,
        0x11 => Codec::Lz11,
        0x24 => Codec::Huffman4,
        0x28 => Codec::Huffman8,
        0x30 => Codec::Rle,
        _ => return Err(Error::Malformed("unknown compression type")),
    };
    let (size, body) = match header >> 8 {
        // Extended header: the real size follows.
        0 => (u32_at(data, 4)? as usize, &data[8..]),
        size => (size as usize, &data[4..]),
    };
    decode(codec, body, size)
}

/// Decodes a header-less `codec` body into exactly `size` bytes.
pub fn decode(codec: Codec, body: &[u8], size: usize) -> Result<Vec<u8>> {
    if size > MAX_OUTPUT {
        return Err(Error::Malformed("declared decompressed size too large"));
    }
    match codec {
        Codec::Lz10 | Codec::Lz11 => lz(body, size, codec == Codec::Lz11),
        Codec::Huffman4 => huffman(body, size, 4),
        Codec::Huffman8 => huffman(body, size, 8),
        Codec::Rle => rle(body, size),
    }
}

fn lz(data: &[u8], size: usize, lz11: bool) -> Result<Vec<u8>> {
    let mut pos = 0;
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
            let (len, disp) = if !lz11 {
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

/// Body: tree-size byte `t`, `(t + 1) * 2 - 1` bytes of tree (root at offset 1), then the
/// bitstream as little-endian `u32` words read MSB first.
///
/// Tree node byte: bits 0-5 = offset to the child pair at `(pos & !1) + offset * 2 + 2`;
/// bit 7 / bit 6 = left (`0`) / right (`1`) child is a leaf holding a symbol.
fn huffman(data: &[u8], size: usize, symbol_bits: u32) -> Result<Vec<u8>> {
    let tree_len = (usize::from(u8_at(data, 0)?) + 1) * 2;
    let corrupt = Error::Malformed("Huffman tree");
    if data.len() < tree_len {
        return Err(corrupt);
    }
    let mut out = Vec::with_capacity(size);
    let mut pending: Option<u8> = None;
    let mut node = 1;
    let mut word_pos = tree_len;
    while out.len() < size {
        let word = u32_at(data, word_pos)?;
        word_pos += 4;
        for i in (0..32).rev() {
            let bit = (word >> i & 1) as usize;
            let n = data[node];
            let child = (node & !1) + usize::from(n & 0x3F) * 2 + 2 + bit;
            if child >= tree_len {
                return Err(corrupt);
            }
            if n >> (7 - bit) & 1 == 0 {
                node = child;
                continue;
            }
            let symbol = data[child];
            node = 1;
            if symbol_bits == 8 {
                out.push(symbol);
            } else if let Some(low) = pending.take() {
                out.push(low | (symbol & 0xF) << 4);
            } else {
                pending = Some(symbol & 0xF);
            }
            if out.len() >= size {
                break;
            }
        }
    }
    Ok(out)
}

/// Flag byte: bit 7 set = run of `(f & 0x7F) + 3` copies of the next byte, otherwise
/// `(f & 0x7F) + 1` literal bytes.
fn rle(data: &[u8], size: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    let mut pos = 0;
    while out.len() < size {
        let flag = u8_at(data, pos)?;
        pos += 1;
        let room = size - out.len();
        if flag & 0x80 != 0 {
            let byte = u8_at(data, pos)?;
            pos += 1;
            out.resize(out.len() + (usize::from(flag & 0x7F) + 3).min(room), byte);
        } else {
            let len = usize::from(flag & 0x7F) + 1;
            let run = data
                .get(pos..pos + len)
                .ok_or(Error::Malformed("RLE literal past end"))?;
            pos += len;
            out.extend_from_slice(&run[..len.min(room)]);
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
        assert_eq!(decompress(&data).unwrap(), b"abcabcabc");
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
        let out = decompress(&data).unwrap();
        assert_eq!(out.len(), size);
        assert!(out.chunks(3).all(|c| c == &b"abc"[..c.len()]));
    }

    #[test]
    fn lz_rejects_reference_before_start() {
        assert!(decompress(&[0x10, 4, 0, 0, 0b1000_0000, 0x10, 0x00]).is_err());
    }

    #[test]
    fn lz_rejects_truncated_input() {
        assert!(decompress(&[0x10, 9, 0, 0, 0, b'a']).is_err());
    }

    /// Tree: root with two leaves, `0` -> `left`, `1` -> `right`; bitstream `0110`.
    fn two_leaf_huffman(header: u8, left: u8, right: u8, size: u8) -> Vec<u8> {
        let mut data = vec![header, size, 0, 0];
        data.extend([1, 0xC0, left, right]);
        data.extend(0x6000_0000u32.to_le_bytes());
        data
    }

    #[test]
    fn huffman8_decodes_symbols() {
        assert_eq!(
            decompress(&two_leaf_huffman(0x28, b'a', b'b', 4)).unwrap(),
            b"abba"
        );
    }

    #[test]
    fn huffman4_packs_low_nibble_first() {
        assert_eq!(
            decompress(&two_leaf_huffman(0x24, 1, 2, 2)).unwrap(),
            [0x21, 0x12]
        );
    }

    #[test]
    fn huffman_rejects_child_outside_tree() {
        let mut data = two_leaf_huffman(0x28, b'a', b'b', 4);
        data[5] = 0xC5; // child offset points past the 4-byte tree
        assert!(decompress(&data).is_err());
    }

    #[test]
    fn rle_runs_and_literals() {
        let data = [0x30, 7, 0, 0, 0x82, b'x', 0x01, b'a', b'b'];
        assert_eq!(decompress(&data).unwrap(), b"xxxxxab");
    }

    #[test]
    fn unknown_type_is_rejected() {
        assert!(decompress(&[0x40, 1, 0, 0, 0]).is_err());
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
