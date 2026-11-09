//! The battle kernel: one action resolved exactly as the game does, with the game's generators,
//! draw order and `f32` arithmetic. No I/O, no global state; the battle's generator and the
//! world's are two [`Random`] values the caller owns.
//!
//! Scope (batch `j4-battle-kernel`): the plain Attack of a party member or a monster and the
//! damage and healing spells met in chapter 2, against any number of targets, with no status, no
//! tension, no guard and no metal body. Everything else the original does inside
//! `ResolveAction` is refused with [`Unsupported`] rather than guessed. Evidence, the formulas
//! with their addresses, the differential test against BattleEmulator and the open points are in
//! `docs/re/battle.md`.
//!
//! - [`random`]: the 64-bit generator, its float helpers and the C library's `rand`;
//! - [`model`]: the plain input structs ([`Fighter`], [`Action`]) and [`Unsupported`];
//! - [`damage`]: critical rate, `CalculatePhysicalDamage`, the amount of a blow or a range;
//! - [`resolve`]: [`resolve_action`], the order of draws of one action;
//! - [`monster`]: a monster's HP draw.

pub mod damage;
pub mod model;
pub mod monster;
pub mod random;
pub mod resolve;

pub use model::{
    Action, ActionRange, Amount, ELEMENTS, Fighter, GRADE_RATES, Reach, ScaleStat, Side,
    Unsupported, grade_rate, validate,
};
pub use monster::init_monster_hp;
pub use random::{CRand, FloatDraw, Random};
pub use resolve::{ActionOutcome, TargetOutcome, resolve_action};

#[cfg(test)]
mod tests;
