//! How fast the message window types: the original's rate and the port's option.
//!
//! **Original** (measured and read, `docs/re/dialogue.md` § "Text speed"): the field calls
//! `MsgWin_Update` (`0x020659a0`) once per 33 ms update with `ticks = GameState::GetTickCount()`
//! = 2 (`ov_017::0218cbd4` passes it in `r1`); the update reveals `ticks` message units per
//! call (`+0x80 += ticks * 0x10`, carry `>> 4`). A unit is one 16-bit code of the compiled
//! stream: a glyph, a space (`0xFF19`), a line break (`0xFF18`) or a code that does not stop
//! the interpreter. At 60 Hz that is **one unit per step**, 60 units per second. No setting
//! scales it: the update reads no option, and the action key does nothing while it types.
//!
//! The speed here is a port option: `Normal` is the original's rate and the default.

/// Fixed steps per `MsgWin_Update` call of the field (`GetTickCount` = 2: 33 ms at 60 Hz).
pub const STEPS_PER_UPDATE: u32 = 2;

/// Quarter units in one unit: the carry of [`TextSpeed::units`] counts in quarters so that
/// the slow speed's half unit per step is exact.
const QUARTERS: u32 = 4;

/// The port's message speed option (`--text-speed`, `?textspeed=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextSpeed {
    /// Half the original's rate: 30 units per second.
    Slow,
    /// The original's rate: one unit per 60 Hz step, 60 units per second. The default.
    #[default]
    Normal,
    /// Twice the original's rate: 120 units per second.
    Fast,
}

impl TextSpeed {
    /// Parses a speed name (`slow`, `normal` or `fast`).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "slow" => Ok(Self::Slow),
            "normal" => Ok(Self::Normal),
            "fast" => Ok(Self::Fast),
            _ => Err(format!("expected slow, normal or fast, not `{s}`")),
        }
    }

    /// The name [`TextSpeed::parse`] accepts for this speed.
    pub fn name(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Normal => "normal",
            Self::Fast => "fast",
        }
    }

    /// Quarter units revealed per step.
    fn quarters(self) -> u32 {
        match self {
            Self::Slow => QUARTERS / 2,
            Self::Normal => QUARTERS,
            Self::Fast => QUARTERS * 2,
        }
    }

    /// Message units to reveal this step. `carry` holds the fraction left over between steps
    /// (start it at 0).
    pub fn units(self, carry: &mut u32) -> usize {
        *carry += self.quarters();
        let units = *carry / QUARTERS;
        *carry %= QUARTERS;
        units as usize
    }
}
