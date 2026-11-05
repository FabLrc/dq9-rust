//! Reader for Nintendo DS ROM images.
//!
//! Everything operates on borrowed byte slices and performs no I/O, so the crate builds for
//! every target the game port runs on (including `wasm32-unknown-unknown`). Malformed input
//! yields [`Error`]; nothing here panics on untrusted data.

mod bytes;
pub mod compress;
mod error;
pub mod fnt;
pub mod g3d;
pub mod header;
pub mod narc;
pub mod rom;

pub use error::{Error, Result};
pub use fnt::{FileEntry, FileTable};
pub use header::Header;
pub use narc::Narc;
pub use rom::{Arm9, Autoload, ModuleParams, Overlay, Rom};
