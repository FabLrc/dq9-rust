//! Level-5 packs (`.pac`): a chain of named members, each an `0x50`-byte head followed by
//! its data. Layout measured on the cartridge in `docs/re/minimap.md` (source: the DQIX
//! formats wiki, `Pac`; 463 of the 501 `.pac`-named files of our ROM are packs, 33 are
//! NARCs and five are the `tdata` tables).
//!
//! `/data/ani/obj_mm.pac` holds the mini-map's panel and dots as Nitro 2D members.

use std::borrow::Cow;

use nds_rom::bytes::{u16_at, u32_at};
use nds_rom::{Error, Result};

/// Bytes of a member's head: the name, then four words.
pub const HEAD_SIZE: usize = 0x50;
/// Longest name, NUL-padded inside the head.
pub const NAME_SIZE: usize = 0x40;

/// One named member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member<'a> {
    /// The name up to its NUL; bytes after it are leftovers and ignored. Names are UTF-8 on
    /// the cartridge but two packs carry Shift-JIS bytes, kept one character per byte.
    pub name: Cow<'a, str>,
    pub data: &'a [u8],
}

/// A parsed pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pac<'a> {
    pub members: Vec<Member<'a>>,
}

impl<'a> Pac<'a> {
    /// Parses the chain up to its end marker, which must end the file.
    ///
    /// # Errors
    /// [`Error::Malformed`] for a head that is neither an entry nor an end marker, a step
    /// that is not the head plus the data rounded up to sixteen, or a chain that ends
    /// before or past the data.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let mut members = Vec::new();
        let mut pos = 0;
        loop {
            let head = data
                .get(pos..pos + HEAD_SIZE)
                .ok_or(Error::Malformed("pac head past end"))?;
            let head_size = u32_at(head, 0x40)?;
            let data_size_word = u32_at(head, 0x44)?;
            let data_size = data_size_word as usize;
            let step_word = u32_at(head, 0x48)?;
            let step = step_word as usize;
            let unknown = u32_at(head, 0x4C)?;
            // End markers: a head of zero bytes, or `0xFFFFFFFF` data size and step (seven
            // effect packs, whose head size word stays 0x50).
            if (head_size == 0 && data_size == 0 && step == 0 && unknown == 0)
                || (data_size_word == u32::MAX && step_word == u32::MAX)
            {
                if pos + HEAD_SIZE != data.len() {
                    return Err(Error::Malformed("pac end marker does not end the file"));
                }
                return Ok(Self { members });
            }
            if head_size as usize != HEAD_SIZE {
                return Err(Error::Malformed("pac head size"));
            }
            if step != (HEAD_SIZE + data_size).next_multiple_of(16) {
                return Err(Error::Malformed("pac step"));
            }
            let name_end = head[..NAME_SIZE]
                .iter()
                .position(|&b| b == 0)
                .ok_or(Error::Malformed("pac unterminated name"))?;
            let name_bytes = &head[..name_end];
            let name = match core::str::from_utf8(name_bytes) {
                Ok(name) => Cow::Borrowed(name),
                // Shift-JIS names (`bg_qm.pac`, `bg_lv5.pac`): one character per byte.
                Err(_) => Cow::Owned(name_bytes.iter().map(|&b| char::from(b)).collect()),
            };
            let body = data
                .get(pos + HEAD_SIZE..pos + HEAD_SIZE + data_size)
                .ok_or(Error::Malformed("pac member past end"))?;
            members.push(Member { name, data: body });
            pos += step;
            if pos > data.len() {
                return Err(Error::Malformed("pac step past end"));
            }
            if pos == data.len() {
                return Err(Error::Malformed("pac without end marker"));
            }
        }
    }

    /// The member called `name`, if any.
    pub fn find(&self, name: &str) -> Option<&Member<'a>> {
        self.members.iter().find(|m| m.name.as_ref() == name)
    }

    /// Names of every member, in chain order.
    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.members.iter().map(|m| m.name.as_ref())
    }
}

/// `true` when `data` looks like a pack: a `0x50` head size at the first head's fourth word.
pub fn is_pac(data: &[u8]) -> bool {
    u16_at(data, 0).is_ok() && u32_at(data, 0x40).is_ok_and(|size| size as usize == HEAD_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(members: &[(&str, &[u8])], end: Option<u32>) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, body) in members {
            let step = (HEAD_SIZE + body.len()).next_multiple_of(16);
            let mut head = vec![0u8; HEAD_SIZE];
            head[..name.len()].copy_from_slice(name.as_bytes());
            head[0x40..0x44].copy_from_slice(&(HEAD_SIZE as u32).to_le_bytes());
            head[0x44..0x48].copy_from_slice(&(body.len() as u32).to_le_bytes());
            head[0x48..0x4C].copy_from_slice(&(step as u32).to_le_bytes());
            head[0x4C..0x50].copy_from_slice(&0x438C48u32.to_le_bytes());
            out.extend(head);
            out.extend(*body);
            out.resize(out.len().next_multiple_of(16), 0);
        }
        match end {
            None => out.extend([0u8; HEAD_SIZE]),
            Some(value) => {
                // The effect packs' marker: head size 0x50, data size and step 0xFFFFFFFF.
                let mut head = vec![0u8; HEAD_SIZE];
                head[0x40..0x44].copy_from_slice(&(HEAD_SIZE as u32).to_le_bytes());
                head[0x44..0x48].copy_from_slice(&value.to_le_bytes());
                head[0x48..0x4C].copy_from_slice(&value.to_le_bytes());
                out.extend(head);
            }
        }
        out
    }

    #[test]
    fn reads_named_members() {
        let data = build(&[("a.NCER", b"alpha"), ("b.NCGR", b"bravo")], None);
        let pac = Pac::parse(&data).unwrap();
        assert_eq!(pac.names().collect::<Vec<_>>(), ["a.NCER", "b.NCGR"]);
        assert_eq!(pac.find("b.NCGR").unwrap().data, b"bravo");
        assert!(pac.find("c").is_none());
    }

    #[test]
    fn reads_the_effect_end_marker() {
        let data = build(&[("x", b"one")], Some(u32::MAX));
        assert_eq!(Pac::parse(&data).unwrap().members.len(), 1);
    }

    #[test]
    fn a_shift_jis_name_keeps_its_bytes_one_char_each() {
        let mut data = build(&[("a", b"x")], None);
        // Byte 0x82, 0x8c, 'q', 't' is the start of `bg_qm.pac`'s name.
        data[0] = 0x82;
        data[1] = 0x8C;
        data[2] = b'q';
        let pac = Pac::parse(&data).unwrap();
        assert_eq!(
            pac.members[0].name.as_bytes(),
            &[0xC2, 0x82, 0xC2, 0x8C, b'q']
        );
    }

    #[test]
    fn rejects_broken_chains() {
        // No end marker.
        let mut none = build(&[("a", b"x")], None);
        none.truncate(none.len() - HEAD_SIZE);
        assert!(Pac::parse(&none).is_err());
        // End marker followed by data.
        let mut trailing = build(&[("a", b"x")], None);
        trailing.push(0);
        assert!(Pac::parse(&trailing).is_err());
        // A step that does not round the head plus the data.
        let mut step = build(&[("a", b"xy")], None);
        step[0x48] = 0x70;
        assert!(Pac::parse(&step).is_err());
        // A head size that is not 0x50.
        let mut head = build(&[("a", b"x")], None);
        head[0x40] = 0x40;
        assert!(Pac::parse(&head).is_err());
        assert!(Pac::parse(b"short").is_err());
    }

    #[test]
    fn detects_a_pack() {
        assert!(is_pac(&build(&[("a", b"x")], None)));
        assert!(!is_pac(b"RLCN\xfe\xff\x00\x01"));
    }
}
