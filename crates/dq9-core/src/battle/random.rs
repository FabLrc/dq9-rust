//! The game's pseudo-random generators, bit for bit.
//!
//! [`Random`] is the 64-bit generator of `struct Random` (`src/Util/Random.cpp` of the DQIX
//! decompilation, Unlicense): `state = state × 0x5D588B656C078965 + 0x269EC3`, the draw being the
//! top 32 bits **after** the step. The game owns two of them (`docs/re/battle.md`, "Generators"):
//! the battle's, seeded when the battle is made, and the world's (`GetBTRandom`,
//! `0x020741e0`), which the field and a monster's HP draw from. They are two values of this type,
//! never a global; the caller says which one it hands to which function.
//!
//! [`CRand`] is the C library's `rand` (`0x02003d14`, 32-bit `× 0x41C64E6D + 0x3039`, the draw
//! its bits 16 to 30) that the drop roll and the surprise roll use.
//!
//! The float helpers keep the game's single-precision rounding: `NextRandomFloat01` divides in
//! `double` and narrows to `float`, `NextRandomMax` multiplies in `float`. A threshold computed in
//! doubles or whole numbers is off by one on a few draws (`docs/re/battle.md`).
//!
//! port: 0x020742d0, 0x0207430c, 0x0207435c, 0x02074398

/// `struct Random`'s generator (the battle's or the world's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Random {
    state: u64,
}

/// `lcg_multiplier` of `SeedRandom` (`0x02074248`).
const MULTIPLIER: u64 = 0x5D58_8B65_6C07_8965;
/// `lcg_increment` of `SeedRandom`.
const INCREMENT: u64 = 0x0026_9EC3;

impl Random {
    /// A generator in the given state (`SeedRandom`, `0x02074248`: the state is the seed).
    pub const fn new(state: u64) -> Self {
        Self { state }
    }

    /// The state the next draw steps from (`GetRandomStateHi` / `GetRandomStateLo`).
    pub const fn state(&self) -> u64 {
        self.state
    }

    /// `NextRandom` (`0x020742d0`): step, then the top 32 bits.
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        (self.state >> 32) as u32
    }

    /// `NextRandomFloat01` (`0x0207435c`): `(float)(NextRandom / (double)0xFFFFFFFF)`, so the
    /// largest draw gives exactly 1.0.
    pub fn next_float01(&mut self) -> f32 {
        (f64::from(self.next_u32()) / f64::from(u32::MAX)) as f32
    }

    /// `NextRandomMax` (`0x0207430c`): `(int)(max × float01)` held below `max`. A non-positive
    /// `max` returns 0 and **does not draw**.
    pub fn next_max(&mut self, max: i32) -> i32 {
        if max <= 0 {
            return 0;
        }
        let result = (max as f32 * self.next_float01()) as i32;
        if result >= max { max - 1 } else { result }
    }

    /// `NextRandomFloatBetween` (`0x02074398`): `(max − min) × float01 + min`, in `float`.
    pub fn next_float_between(&mut self, min: f32, max: f32) -> f32 {
        (max - min) * self.next_float01() + min
    }
}

/// A source of the float draws the damage formulas take, so that a caller can count them.
pub trait FloatDraw {
    /// `NextRandomFloatBetween`.
    fn float_between(&mut self, min: f32, max: f32) -> f32;
}

impl FloatDraw for Random {
    fn float_between(&mut self, min: f32, max: f32) -> f32 {
        self.next_float_between(min, max)
    }
}

/// The C library's `rand` / `srand` (`0x02003d14`, `0x02003d48`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CRand {
    seed: u32,
}

impl CRand {
    /// `srand(seed)`.
    pub const fn new(seed: u32) -> Self {
        Self { seed }
    }

    /// The seed the next draw steps from.
    pub const fn seed(&self) -> u32 {
        self.seed
    }

    /// `rand()`: `seed = seed × 0x41C64E6D + 0x3039`, the draw its bits 16 to 30.
    pub fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        (self.seed >> 16) & 0x7FFF
    }
}
