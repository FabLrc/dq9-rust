//! Modelled trigger state, condition evaluation and execution: what the game does when a
//! trigger fires (`Trig_FindMatch`, `Trig_RunAction`, `Trig_RunDeferred`, `Trig_Fire`) on
//! the state the engine models. Rule parsing lives in `dq9_formats::trigger`; layout and
//! evidence in `docs/re/trigger.md`.

mod action;
mod condition;
mod world;

pub use action::{Fired, Flow, GOLD_MAX, Request, fire, run_action, run_deferred};
pub(crate) use action::{fixed, high};
pub use condition::{UNMODELLED, Unsupported, degrees_to_angle, first_match};
pub use world::{
    Member, QuestLog, QuestLogEntry, Raw, State, TRACKS, Track, VOCATIONS, World, track_of_room,
};

#[cfg(test)]
mod tests;
