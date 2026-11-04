//! Dragon Quest IX data formats (Level-5 engine).
//!
//! Like `nds-rom`, everything works on borrowed bytes with no I/O and returns
//! [`nds_rom::Error`] on malformed input. Format notes live in `docs/re/`.

pub mod dialogue;
pub mod font;
pub mod gpc2;
pub mod level5;
pub mod message;
pub mod save;
pub mod stb;
pub mod table;
pub mod trigger;

pub use font::{Font, FontId};
pub use gpc2::Gpc2;
pub use nds_rom::{Error, Result};
pub use table::{Record, Table, Value};
