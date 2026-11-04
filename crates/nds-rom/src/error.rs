use std::fmt;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A read of `len` bytes at `offset` fell outside a buffer of `size` bytes.
    OutOfBounds {
        offset: usize,
        len: usize,
        size: usize,
    },
    /// A structure did not start with the expected magic.
    BadMagic { what: &'static str, found: [u8; 4] },
    /// Structurally invalid data.
    Malformed(&'static str),
    /// No file or directory with this path / id.
    NotFound(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { offset, len, size } => write!(
                f,
                "read of {len:#x} bytes at {offset:#x} exceeds buffer of {size:#x} bytes"
            ),
            Self::BadMagic { what, found } => {
                write!(f, "bad {what} magic: {:02x?}", found)
            }
            Self::Malformed(why) => write!(f, "malformed data: {why}"),
            Self::NotFound(what) => write!(f, "not found: {what}"),
        }
    }
}

impl std::error::Error for Error {}
