//! Level-5 compressed blocks (see `docs/re/gpc2.md`).
//!
//! A block starts with a little-endian `u32`: bits 0-2 = method, bits 3-31 = decompressed
//! size. The body that follows uses the Nintendo BIOS encodings without their own header.

use std::borrow::Cow;

use nds_rom::compress::{Codec, decode};
use nds_rom::{Error, Result};

/// Decompression method of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Stored,
    Codec(Codec),
}

/// Reads the block header: `(method, decompressed size)`.
pub fn header(data: &[u8]) -> Result<(Method, usize)> {
    let raw = data
        .first_chunk::<4>()
        .map(|b| u32::from_le_bytes(*b))
        .ok_or(Error::Malformed("Level-5 block shorter than its header"))?;
    let method = match raw & 7 {
        0 => Method::Stored,
        1 => Method::Codec(Codec::Lz10),
        2 => Method::Codec(Codec::Huffman4),
        3 => Method::Codec(Codec::Huffman8),
        4 => Method::Codec(Codec::Rle),
        _ => return Err(Error::Malformed("unknown Level-5 compression method")),
    };
    Ok((method, (raw >> 3) as usize))
}

/// Decompresses a block; stored blocks are borrowed.
pub fn decompress(data: &[u8]) -> Result<Cow<'_, [u8]>> {
    let (method, size) = header(data)?;
    let body = &data[4..];
    match method {
        Method::Stored => body
            .get(..size)
            .map(Cow::Borrowed)
            .ok_or(Error::Malformed("stored Level-5 block past end")),
        Method::Codec(codec) => decode(codec, body, size).map(Cow::Owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(method: u32, size: u32, body: &[u8]) -> Vec<u8> {
        [&(size << 3 | method).to_le_bytes()[..], body].concat()
    }

    #[test]
    fn stored_block_is_borrowed_and_trimmed() {
        let data = block(0, 3, b"abcPAD");
        let out = decompress(&data).unwrap();
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(&*out, b"abc");
    }

    #[test]
    fn lz10_block_has_no_inner_header() {
        let data = block(1, 9, &[0b0001_0000, b'a', b'b', b'c', 0x30, 0x02]);
        assert_eq!(&*decompress(&data).unwrap(), b"abcabcabc");
    }

    #[test]
    fn rejects_unknown_method_and_truncation() {
        assert!(decompress(&block(5, 1, b"x")).is_err());
        assert!(decompress(&block(0, 10, b"short")).is_err());
        assert!(decompress(&[1, 2]).is_err());
    }
}
