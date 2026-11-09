//! The reader of an SSEQ command stream: fixed-width and variable-length arguments.

#[derive(Clone, Copy)]
pub(super) enum Over {
    None,
    Rand,
    Var,
}

#[derive(Clone, Copy)]
pub(super) enum Arg {
    U8,
    S8,
    U16,
    S16,
    Vlq,
}

pub(super) struct Cursor<'a> {
    pub(super) stream: &'a [u8],
    pub(super) pos: usize,
}

impl Cursor<'_> {
    pub(super) fn u8(&mut self) -> Option<u8> {
        let b = *self.stream.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    pub(super) fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.u8()?) | u16::from(self.u8()?) << 8)
    }

    pub(super) fn u24(&mut self) -> Option<usize> {
        Some(usize::from(self.u8()?) | usize::from(self.u8()?) << 8 | usize::from(self.u8()?) << 16)
    }

    /// Big-endian 7-bit groups, high bit set on all but the last.
    pub(super) fn vlq(&mut self) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            value = value << 7 | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
}
