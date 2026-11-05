//! Deterministic field state, movement and collision. No platform I/O.

pub mod camera;
pub mod collision;
pub mod field;
pub mod fx;
pub mod lighting;
pub mod movement;
pub mod script;
pub mod state;
pub mod talk;
pub mod trigger;

#[cfg(test)]
mod testing;

pub use collision::CollisionWorld;
pub use movement::{Hero, Input};
