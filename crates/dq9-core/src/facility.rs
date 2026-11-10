//! What the party meets in a town: the inn, the church, the shops and the treasure containers.
//!
//! Pure rules over [`crate::party::Party`], [`crate::state::DayClock`] and the readers of
//! [`dq9_formats::facility`]; no I/O and no UI. The links between them are the talk tags
//! (wiki DQIX *Text events*): a talk line ending in `<INN=n>` opens the inn of file `n − 1`,
//! `<CHURCH=n>` the church of file `n − 1` and `<SHOP=n>` the shop numbered *n*; the message
//! numbers each rule returns are those of that file, which the caller prints.
//!
//! The rules are reimplemented from the game's own code (overlay 3 for the inn and the
//! church, overlay 17 for the treasure) and from the wiki; every one cites its original and
//! marks what is inferred. The notes are in `docs/re/facility.md`.

pub mod church;
pub mod inn;
pub mod shop;
pub mod treasure;
