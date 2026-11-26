//! The ticks a fight's entry and return spend, phase by phase (`docs/re/battle-transition.md`).
//! Pure: the application drives one 60 Hz fixed step at a time and reads the phase back.
//!
//! Read from the European ROM: `SwirlTask_Init` (`ov_017::021b621c`) sets the swirl task's delay
//! (`+0xfd`) to 15 and its battle number (`+0xfe`); `SwirlTask_Update` (`ov_017::021b6290`)
//! state 0 zeroes the delay when the number is below 1, and its state 2 starts the swirl only
//! once the delay is spent. The task updates once per two frames (the game's 30 Hz loop), so 15
//! updates are [`DELAY_STEPS`] fixed steps. The freeze and the two load blacks are measured on
//! the original (core state `w-contact1`: 12 frozen frames after a roamer's contact, 94 black
//! frames while the battle stage loads, 95 while the field reloads), not read in a task: the
//! freeze is the blocking zone load of `ov_017::021b790c` state 0 (the `func_02015438` loop)
//! **[hypothesis]**, and the blacks are load times the code does not count.
//!
//! A preview (`--battle`, `?battle=`) has no field behind it: it skips the lead-in and the
//! measured blacks, as it always did (`docs/app/battle-scene.md`, § Transition).

use super::transition;

/// Frames the picture is frozen after a roamer's contact while the battle's data loads,
/// measured (`docs/re/battle-transition.md`, timeline: frames 5-17).
pub const FREEZE_STEPS: u32 = 12;

/// Updates a set battle's swirl task waits before its first update (`SwirlTask_Init` `+0xfd` =
/// 15, zeroed by `SwirlTask_Update` state 0 for a roamer), and the steps of each.
pub const DELAY_UPDATES: u32 = 15;
pub const DELAY_STEPS: u32 = DELAY_UPDATES * transition::UPDATE_TICKS;

/// Frames both screens stay black while the battle stage loads, measured (frames 55-148).
pub const LOAD_BLACK_STEPS: u32 = 94;

/// Steps of the way out: both screens to black over 15 ticks, the field reloading for 95 frames
/// (measured: frames 1043-1057 and 1057-1152), then the bottom screen's `SetMainBrightness(0,
/// 30)` over 30.
pub const FADE_OUT_STEPS: u32 = 15;
pub const RETURN_BLACK_STEPS: u32 = 95;
pub const RETURN_FADE_STEPS: u32 = 30;

/// Steps of the scene's fade-in on the black: `0x021d9d18`'s count was not read, and the
/// measured brightness log changes over frames 149-161 (~13 frames, 7 levels at the 30 Hz
/// pairing); the port spends 15 fixed steps, the count `docs/app/battle-scene.md` records.
pub const SCENE_FADE_STEPS: u32 = 15;

/// How a fight's entry reaches the swirl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lead {
    /// A preview (`--battle`, `?battle=`): the swirl's clock is already done and no stage is
    /// loaded from a field, so neither the lead-in nor the measured blacks are spent.
    #[default]
    Preview,
    /// A roamer's contact (`start_encounter`): the picture freezes while the battle's data
    /// loads (12 frames).
    Contact,
    /// A set battle (`Request::Battle`, number ≥ 1): the swirl task waits its 15 updates
    /// (`SwirlTask_Update` state 2).
    SetBattle,
}

impl Lead {
    /// The steps this lead-in spends before the swirl's first step.
    pub const fn steps(self) -> u32 {
        match self {
            Self::Preview => 0,
            Self::Contact => FREEZE_STEPS,
            Self::SetBattle => DELAY_STEPS,
        }
    }

    /// Whether the entry spends the measured load black after the swirl.
    pub const fn holds_black(self) -> bool {
        !matches!(self, Self::Preview)
    }

    /// The phases of this entry, in order (`docs/app/battle-scene.md`, § Transition). The
    /// application drives the same phases through `BattleChange`; this is the table the tests
    /// check it against.
    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn phases(self) -> &'static [Phase] {
        match self {
            Self::Preview => &PREVIEW,
            Self::Contact => &CONTACT,
            Self::SetBattle => &SET_BATTLE,
        }
    }
}

const PREVIEW: [Phase; 2] = [Phase::Swirl, Phase::Scene];
const CONTACT: [Phase; 4] = [Phase::Freeze, Phase::Swirl, Phase::LoadBlack, Phase::Scene];
const SET_BATTLE: [Phase; 4] = [Phase::Delay, Phase::Swirl, Phase::LoadBlack, Phase::Scene];

/// One phase of the transition, entry first, then the way back. The application's state
/// machine (`BattleChange`) is this table in order; the table is what its tests check.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The picture is frozen while the battle's data loads.
    Freeze,
    /// The swirl task waits its 15 updates.
    Delay,
    /// The swirl's own clock (`battle/transition.rs`).
    Swirl,
    /// Both screens black while the battle stage loads.
    LoadBlack,
    /// The scene's fade-in.
    Scene,
    /// Both screens to black.
    FadeOut,
    /// The field reloading behind the black.
    ReturnBlack,
    /// The field's fade-in.
    ReturnFade,
}

impl Phase {
    /// The fixed steps this phase spends.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn steps(self) -> u32 {
        match self {
            Self::Freeze => FREEZE_STEPS,
            Self::Delay => DELAY_STEPS,
            Self::Swirl => transition::entry_ticks(),
            Self::LoadBlack => LOAD_BLACK_STEPS,
            Self::Scene => SCENE_FADE_STEPS,
            Self::FadeOut => FADE_OUT_STEPS,
            Self::ReturnBlack => RETURN_BLACK_STEPS,
            Self::ReturnFade => RETURN_FADE_STEPS,
        }
    }
}

/// The steps one phase of `phases` and everything before it takes.
#[cfg_attr(not(test), allow(dead_code))]
pub fn steps_before(phases: &[Phase], phase: Phase) -> u32 {
    let mut steps = 0;
    for current in phases {
        if *current == phase {
            return steps;
        }
        steps += current.steps();
    }
    steps
}

/// Steps from the contact to the fight's first shown step: the entry's every phase.
#[cfg_attr(not(test), allow(dead_code))]
pub fn entry_steps(lead: Lead) -> u32 {
    lead.phases().iter().map(|phase| phase.steps()).sum()
}

/// The way back to the field, in order.
pub const fn leave() -> &'static [Phase; 3] {
    &[Phase::FadeOut, Phase::ReturnBlack, Phase::ReturnFade]
}

/// Steps of the way back, the fight's own step included from the black: [`leave`].
pub fn leave_steps() -> u32 {
    leave().iter().map(|phase| phase.steps()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry's phases, in order, each entered once, none skipped, and their step counts:
    /// 12 (the freeze), 30 (the delay's 15 updates of two frames), 38 (the swirl), 94 (the load
    /// black) and 15 (the scene's fade-in).
    #[test]
    fn the_entry_phases_are_in_order_and_spend_their_measured_ticks() {
        assert_eq!(
            Lead::Contact.phases(),
            &[Phase::Freeze, Phase::Swirl, Phase::LoadBlack, Phase::Scene]
        );
        assert_eq!(
            Lead::SetBattle.phases(),
            &[Phase::Delay, Phase::Swirl, Phase::LoadBlack, Phase::Scene]
        );
        assert_eq!(Lead::Preview.phases(), &[Phase::Swirl, Phase::Scene]);
        for lead in [Lead::Preview, Lead::Contact, Lead::SetBattle] {
            let phases = lead.phases();
            for (index, phase) in phases.iter().enumerate() {
                assert_eq!(
                    phases.iter().filter(|p| *p == phase).count(),
                    1,
                    "{lead:?}: {phase:?} twice"
                );
                let before = phases[..index].iter().map(|p| p.steps()).sum::<u32>();
                assert_eq!(steps_before(phases, *phase), before, "{lead:?} {phase:?}");
            }
            assert_eq!(
                entry_steps(lead),
                phases.iter().map(|p| p.steps()).sum::<u32>()
            );
        }
        assert_eq!(Lead::Contact.steps(), 12);
        assert_eq!(Lead::SetBattle.steps(), 30);
        assert_eq!(Lead::Preview.steps(), 0);
        assert_eq!(Phase::Swirl.steps(), 38);
        assert_eq!(entry_steps(Lead::Preview), 53);
        assert_eq!(entry_steps(Lead::Contact), 159);
        assert_eq!(entry_steps(Lead::SetBattle), 177);
        assert!(Lead::Contact.holds_black() && Lead::SetBattle.holds_black());
        assert!(!Lead::Preview.holds_black(), "a preview loads nothing");
    }

    /// The way back: 15 to black, 95 black while the field reloads, 30 up.
    #[test]
    fn the_way_back_holds_the_black_ninety_five_steps() {
        assert_eq!(
            leave(),
            &[Phase::FadeOut, Phase::ReturnBlack, Phase::ReturnFade]
        );
        assert_eq!(Phase::FadeOut.steps(), 15);
        assert_eq!(Phase::ReturnBlack.steps(), 95);
        assert_eq!(Phase::ReturnFade.steps(), 30);
        assert_eq!(leave_steps(), 140);
        assert_eq!(
            steps_before(leave(), Phase::ReturnFade),
            15 + 95,
            "the field fades up once its reload is spent"
        );
    }
}
