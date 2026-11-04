//! `GPC2` archives (`*.gp2`): Level-5 packs of named, individually compressed files.
//! Layout in `docs/re/gpc2.md`; loader at `0x020d91ec` (`Gpc2_LoadIndex`).
//!
//! port: 0x020d91ec, 0x020d902c, 0x020d8fdc

use std::borrow::Cow;

use nds_rom::{Error, Result};

use crate::level5;

const HEADER_SIZE: usize = 0x14;
const ENTRY_SIZE: usize = 12;
/// Flag in the top nibble of the data-size word: members are stored without a Level-5
/// block header (seen in `pack_lv5/enemy.gp2`, `prm/actdt_*.gp2`).
const FLAG_RAW_MEMBERS: u32 = 1 << 28;

/// One archive member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// CRC-32 of `name`; the index is sorted by it for binary search.
    pub hash: u32,
    /// Absolute offset of the member inside the archive.
    pub offset: usize,
    /// Stored (possibly compressed) size in bytes.
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct Gpc2<'a> {
    data: &'a [u8],
    raw_members: bool,
    pub entries: Vec<Entry>,
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Level-5 block starting at a word offset.
fn block_at(data: &[u8], words: u16) -> Result<Cow<'_, [u8]>> {
    let start = usize::from(words) * 4;
    level5::decompress(
        data.get(start..)
            .ok_or(Error::Malformed("GPC2 block offset"))?,
    )
}

impl<'a> Gpc2<'a> {
    pub fn is_gpc2(data: &[u8]) -> bool {
        data.starts_with(b"GPC2")
    }

    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Malformed("GPC2 header truncated"));
        }
        if !Self::is_gpc2(data) {
            let found = [data[0], data[1], data[2], data[3]];
            return Err(Error::BadMagic {
                what: "GPC2",
                found,
            });
        }
        // All offsets/sizes below are in 32-bit words.
        let count = usize::from(u16_at(data, 4) & 0xFFF);
        let index_offset = u16_at(data, 6);
        let names_offset = u16_at(data, 8);
        let data_offset = usize::from(u16_at(data, 10)) * 4;
        let data_word = u32_at(data, 16);
        let data_size = (data_word & 0x0FFF_FFFF) as usize * 4;
        if data_offset + data_size != data.len() {
            return Err(Error::Malformed("GPC2 data region does not end the file"));
        }

        let index = block_at(data, index_offset)?;
        let names = block_at(data, names_offset)?;
        if index.len() < count * ENTRY_SIZE {
            return Err(Error::Malformed("GPC2 index shorter than its entry count"));
        }
        let entries = index.as_chunks::<ENTRY_SIZE>().0[..count]
            .iter()
            .map(|e| {
                let (hash, location, size) = (u32_at(e, 0), u32_at(e, 4), u32_at(e, 8));
                // The 16-bit name offset is split across the two top bytes.
                let name_offset = ((size >> 24) << 8 | location >> 24) as usize;
                let name = names
                    .get(name_offset..)
                    .and_then(|s| s.split(|&b| b == 0).next())
                    .ok_or(Error::Malformed("GPC2 name offset"))?;
                if crc32fast::hash(name) != hash {
                    return Err(Error::Malformed("GPC2 name hash mismatch"));
                }
                let entry = Entry {
                    name: name.iter().map(|&b| char::from(b)).collect(),
                    hash,
                    offset: data_offset + (location & 0x00FF_FFFF) as usize * 4,
                    size: (size & 0x00FF_FFFF) as usize,
                };
                if entry.offset + entry.size > data.len() {
                    return Err(Error::Malformed("GPC2 member past end of archive"));
                }
                Ok(entry)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            data,
            raw_members: data_word & FLAG_RAW_MEMBERS != 0,
            entries,
        })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        let hash = crc32fast::hash(name.as_bytes());
        self.entries
            .iter()
            .find(|e| e.hash == hash && e.name == name)
    }

    /// Stored bytes of a member, before decompression.
    pub fn stored(&self, entry: &Entry) -> &'a [u8] {
        &self.data[entry.offset..entry.offset + entry.size]
    }

    /// Member contents, decompressed.
    pub fn member(&self, entry: &Entry) -> Result<Cow<'a, [u8]>> {
        let stored = self.stored(entry);
        if self.raw_members {
            Ok(Cow::Borrowed(stored))
        } else {
            level5::decompress(stored)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Builds an archive with stored (method 0) index, names and members.
    fn build(members: &[(&str, &[u8])], raw_members: bool) -> Vec<u8> {
        let stored = |body: &[u8]| {
            let mut b = ((body.len() as u32) << 3).to_le_bytes().to_vec();
            b.extend(body);
            b.resize(b.len().next_multiple_of(4), 0);
            b
        };
        let mut names = Vec::new();
        let mut blobs = Vec::new();
        let mut index: Vec<(u32, u32, u32)> = Vec::new();
        for (name, body) in members {
            let name_offset = names.len() as u32;
            names.extend(name.as_bytes());
            names.push(0);
            let blob = if raw_members {
                body.to_vec()
            } else {
                stored(body)
            };
            let location = (blobs.len() / 4) as u32 | (name_offset & 0xFF) << 24;
            let size = blob.len() as u32 | (name_offset >> 8) << 24;
            index.push((crc32fast::hash(name.as_bytes()), location, size));
            blobs.extend(&blob);
            blobs.resize(blobs.len().next_multiple_of(4), 0);
        }
        index.sort_by_key(|e| e.0);
        let index: Vec<u8> = index
            .iter()
            .flat_map(|(h, l, s)| [h.to_le_bytes(), l.to_le_bytes(), s.to_le_bytes()].concat())
            .collect();
        let (index, names) = (stored(&index), stored(&names));
        let names_offset = HEADER_SIZE + index.len();
        let data_offset = names_offset + names.len();
        let mut out = b"GPC2".to_vec();
        out.extend((members.len() as u16).to_le_bytes());
        for words in [HEADER_SIZE / 4, names_offset / 4, data_offset / 4, 0, 0] {
            out.extend((words as u16).to_le_bytes());
        }
        let flags = if raw_members { FLAG_RAW_MEMBERS } else { 0 };
        out.extend(((blobs.len() / 4) as u32 | flags).to_le_bytes());
        out.extend(index);
        out.extend(names);
        out.extend(blobs);
        out
    }

    #[test]
    fn reads_named_members() {
        let data = build(&[("a.bin", b"alpha"), ("b.bin", b"bravo!")], false);
        let archive = Gpc2::parse(&data).unwrap();
        assert_eq!(archive.entries.len(), 2);
        let b = archive.find("b.bin").unwrap();
        assert_eq!(&*archive.member(b).unwrap(), b"bravo!");
        assert!(archive.find("c.bin").is_none());
    }

    #[test]
    fn raw_member_flag_skips_block_header() {
        let data = build(&[("x.nat", b"\x3f\x01\x02\x03")], true);
        let archive = Gpc2::parse(&data).unwrap();
        let x = &archive.entries[0];
        assert_eq!(&*archive.member(x).unwrap(), b"\x3f\x01\x02\x03");
    }

    #[test]
    fn rejects_hash_mismatch() {
        let mut data = build(&[("a.bin", b"alpha")], false);
        // Corrupt the first byte of the stored hash (index body starts after its header).
        data[HEADER_SIZE + 4] ^= 0xFF;
        assert!(Gpc2::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_archive() {
        let data = build(&[("a.bin", b"alpha")], false);
        assert!(Gpc2::parse(&data[..data.len() - 4]).is_err());
        assert!(Gpc2::parse(&data[..8]).is_err());
    }
}
