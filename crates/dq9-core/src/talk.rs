//! Conversations with NPCs: whom the hero talks to, how a line is prepared and laid out, and
//! the dialogue state machine driven by explicit inputs. No I/O; evidence and open points in
//! `docs/re/dialogue.md`.
//!
//! - [`nearest`] chooses the partner like `Talk_NearestNpc` (`ov_017::021a4e88`);
//! - [`prepare`] runs the message pipeline of a line (`dq9_formats::message`) with the
//!   application's names and builds a [`Script`]: glyphs placed on lines (word wrap),
//!   pages, waits and prompts;
//! - [`Dialogue`] plays a script one 60 Hz step at a time ([`Dialogue::step`]) and exposes
//!   what the window shows ([`Dialogue::view`]).
//!
//! No port marker: `Talk_NearestNpc` is modelled without the object flags and the
//! hero-busy tests, and the window state machine is rebuilt from the observed behaviour of
//! the message interpreter (`0x020659a0`) rather than translated.

mod counts;
mod dialogue;
mod layout;
mod partner;

pub use counts::TalkCounts;
pub use dialogue::{Dialogue, Input, Outcome, PromptView, SCROLL_STEPS, View};
pub use layout::{
    AnswerSound, BLIP_EVERY, DEFAULT_LINES, EndKind, Names, Op, Plate, PromptKind, Script, Sound,
    compile_message, layout, prepare,
};
pub use partner::{RANGE, REACH, Talkable, Target, nearest, zones_containing};

#[cfg(test)]
mod tests;
