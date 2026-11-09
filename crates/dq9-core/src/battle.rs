//! The battle kernel and the battle's flow, with the game's generators, draw order and `f32`
//! arithmetic. No I/O, no global state; the battle's generator and the world's are two
//! [`Random`] values (the battle owns its own, the caller owns the world's).
//!
//! The kernel ([`resolve`]) resolves one action exactly as the game does, for the plain Attack
//! of a party member or a monster and the damage and healing spells met in chapter 2, against any
//! number of targets, with no status, no tension, no guard and no metal body. Everything else the
//! original does inside `ResolveAction` is refused with [`Unsupported`] rather than guessed.
//!
//! On top of it (batch `j4-battle-flow`) [`Battle`] plays a whole battle headless: it is built
//! from ROM records ([`setup`]), runs round after round ([`round`], [`ai`]) and ends with a
//! [`BattleEnd`] ([`end`]). Evidence, the formulas with their addresses, the differential test
//! against BattleEmulator and the open points are in `docs/re/battle.md`.
//!
//! - [`random`]: the 64-bit generator, its float helpers and the C library's `rand`;
//! - [`model`]: the plain input structs ([`Fighter`], [`Action`]) and [`Unsupported`];
//! - [`damage`]: critical rate, `CalculatePhysicalDamage`, the amount of a blow or a range;
//! - [`resolve`]: [`resolve_action`], the order of draws of one action;
//! - [`monster`]: a monster's HP draw;
//! - [`setup`]: monsters and actions built from the ROM's records;
//! - [`round`]: the round state machine, flight, initiative, commands;
//! - [`ai`]: a monster's way and targets, the targets of every action;
//! - [`end`]: victory rewards, the result.

pub mod ai;
pub mod damage;
pub mod end;
pub mod model;
pub mod monster;
pub mod random;
pub mod resolve;
pub mod round;
pub mod setup;

pub use end::{BattleEnd, BattleResult, drop_roll};
pub use model::{
    Action, ActionRange, Amount, ELEMENTS, Fighter, GRADE_RATES, Reach, ScaleStat, Side,
    Unsupported, grade_rate, validate,
};
pub use monster::init_monster_hp;
pub use random::{CRand, FloatDraw, Random};
pub use resolve::{ActionOutcome, TargetOutcome, resolve_action};
pub use round::{
    Battle, BattleError, Command, Event, PartyMember, RoundReport, Surprise, Target, Who,
};
pub use setup::{ActionTables, BattleSetup, Monster, SetupError, Skill, Tables};

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod tests;
