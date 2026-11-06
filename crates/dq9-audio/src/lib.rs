//! `dq9-audio`: plays the game's music. A software version of the DS sound driver's sequence
//! player: it reads an SSEQ command stream (`dq9_formats::sdat`), turns it into notes of the
//! bank's instruments (IMA-ADPCM waves, PSG and noise), shapes them with the DS envelope and
//! mixes stereo `f32` at any host rate. Like the other core crates it does no I/O and builds
//! for the Web. Format notes and evidence in `docs/re/sdat.md`.
//!
//! The Nintendo sound library the game links is platform code and is replaced, not
//! translated; the command semantics and the conversion tables follow the public
//! documentation and the WTFPL players it cites (SSEQPlayer by kode54, FSS by fincs).

mod mixer;
mod player;
mod song;
mod tables;

pub use mixer::{Handle, Mixer};
pub use player::Player;
pub use song::Song;
