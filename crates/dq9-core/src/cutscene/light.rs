//! What a script does to the lighting of the scene: the light scale fade (native 578), the
//! scene's time-of-day profile (588, 589), the lighting index override (548, 549) and the 3D
//! fog switch (804). The application turns this state into the renderer's environment
//! (`dq9_app::lighting`, `dq9_core::lighting::Lighting::select`); requests that concern the
//! game's own clock (579, 808) are queued as [`ClockCommand`]s.
//!
//! Evidence: `docs/re/stb-natives.md` (handlers `ov_001::02164004` (578), `02164080` (579),
//! `02160338` / `021603b0` (548, 549), `021603d8` (588), `02160504` (589), `02161e8c` (804),
//! `02162148` (808)), `docs/re/lighting.md` and `LightingManager.cpp` of the USA decompilation
//! (`BeginFade`, `lightRGBScale_`).

use crate::script::event::Event;

/// Ticks per script update, the unit of the fade length of native 578 (`LightingManager`
/// compares the elapsed delta time to `duration × s16(02010218)` ≈ 33 ms, i.e. 2 ticks).
const TICKS_PER_UNIT: i32 = 2;

/// Which time of day the scene's lighting reads (`LightingManager+0x94` / `+0x98`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneTime {
    /// The game's day clock, copied every frame (`ov_017::0218d0ec`).
    #[default]
    Clock,
    /// A phase 0 (night) to 3 (evening) forced by native 588, at its start.
    Forced(usize),
    /// The saved phase, at its start (native 589).
    Saved,
}

/// A request of a script on the game's own clock, for the application to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockCommand {
    /// Native 579: the day timer runs (`true`) or stops.
    Running(bool),
    /// Native 808: `Save_SetTimeOfDay`, the clock goes to the start of phase 0 to 3.
    SetPhase(usize),
}

/// The scene lighting a script set up.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLight {
    /// Time of day of the scene (native 588, 589).
    pub time: SceneTime,
    /// `LightingManager+0x90` (native 548): 0 when unused.
    pub index_override: usize,
    /// `LightingManager+0x85` (native 804). The game starts with the fog on.
    pub fog: bool,
    scale: f32,
    from: f32,
    to: f32,
    total: u32,
    left: u32,
    clock: Vec<ClockCommand>,
}

impl Default for SceneLight {
    fn default() -> Self {
        Self {
            time: SceneTime::Clock,
            index_override: 0,
            fog: true,
            scale: 1.0,
            from: 1.0,
            to: 1.0,
            total: 0,
            left: 0,
            clock: Vec::new(),
        }
    }
}

impl SceneLight {
    /// The two lightings look the same (the pending clock commands are not part of it).
    pub fn same_look(&self, other: &Self) -> bool {
        self.time == other.time
            && self.index_override == other.index_override
            && self.fog == other.fog
            && self.scale == other.scale
    }

    /// `lightRGBScale_ / 4096`: the factor on the light and background colours.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Whether the scene differs from the one the field shows.
    pub fn is_default(&self) -> bool {
        self.time == SceneTime::Clock
            && self.index_override == 0
            && self.fog
            && self.scale == 1.0
            && self.left == 0
    }

    /// The commands on the game's clock made since the last call.
    pub fn take_clock_commands(&mut self) -> Vec<ClockCommand> {
        std::mem::take(&mut self.clock)
    }

    /// `LightingManager::BeginFade` (`0x02052894` of the USA decomp's `LightingManager.cpp`
    /// l.1164): an immediate change when the length is 0 or the target is the current scale.
    fn begin_fade(&mut self, target: f32, ticks: i32) {
        if ticks <= 0 || target == self.scale {
            self.scale = target;
            self.from = target;
            self.to = target;
            self.total = 0;
            self.left = 0;
        } else {
            self.from = self.scale;
            self.to = target;
            self.total = ticks as u32;
            self.left = ticks as u32;
        }
    }

    /// One tick: the linear part of the fade (`RecomputeAdvancedLighting`, l.912-930).
    pub(super) fn tick(&mut self) {
        if self.left == 0 {
            return;
        }
        self.left -= 1;
        if self.left == 0 {
            self.scale = self.to;
            self.from = self.to;
        } else {
            let done = (self.total - self.left) as f32 / self.total as f32;
            self.scale = self.from + (self.to - self.from) * done;
        }
    }

    /// Applies a script event; returns whether it concerned the lighting.
    pub(super) fn apply(&mut self, event: &Event) -> bool {
        match event {
            Event::LightingOverride(mode) => {
                self.index_override = mode.map_or(0, |m| usize::try_from(m).unwrap_or(0));
            }
            Event::LightIntensity {
                intensity,
                duration,
            } => {
                // The game keeps the scale as a signed 4.12 fixed point.
                let fixed = (intensity * 4096.0) as i32;
                let target =
                    f32::from(fixed.clamp(i16::MIN.into(), i16::MAX.into()) as i16) / 4096.0;
                self.begin_fade(target, duration.saturating_mul(TICKS_PER_UNIT));
            }
            Event::DayTimerStopped(stop) => self.clock.push(ClockCommand::Running(!stop)),
            Event::SceneTimeOfDay(phase) => {
                self.time = SceneTime::Forced(phase.rem_euclid(4) as usize);
            }
            Event::SyncSceneTimeOfDay => self.time = SceneTime::Saved,
            Event::SetTimeOfDay(phase) => {
                self.clock
                    .push(ClockCommand::SetPhase(phase.rem_euclid(4) as usize));
            }
            Event::Fog(enabled) => self.fog = *enabled,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intensity(light: &mut SceneLight, intensity: f32, duration: i32) {
        light.apply(&Event::LightIntensity {
            intensity,
            duration,
        });
    }

    #[test]
    fn fade_is_linear_and_ends_on_target() {
        let mut light = SceneLight::default();
        intensity(&mut light, 0.5, 5);
        assert_eq!(light.scale(), 1.0);
        for _ in 0..5 {
            light.tick();
        }
        assert!((light.scale() - 0.75).abs() < 1e-6);
        for _ in 0..5 {
            light.tick();
        }
        assert_eq!(light.scale(), 0.5);
        assert!(!light.is_default());
    }

    #[test]
    fn zero_length_is_immediate() {
        let mut light = SceneLight::default();
        intensity(&mut light, 0.25, 0);
        assert_eq!(light.scale(), 0.25);
        intensity(&mut light, 1.0, 0);
        assert!(light.is_default());
    }

    #[test]
    fn scene_time_fog_and_clock_requests() {
        let mut light = SceneLight::default();
        assert!(light.apply(&Event::SceneTimeOfDay(3)));
        assert_eq!(light.time, SceneTime::Forced(3));
        assert!(light.apply(&Event::SyncSceneTimeOfDay));
        assert_eq!(light.time, SceneTime::Saved);
        assert!(light.apply(&Event::Fog(false)));
        assert!(!light.fog);
        light.apply(&Event::DayTimerStopped(true));
        light.apply(&Event::SetTimeOfDay(2));
        assert_eq!(
            light.take_clock_commands(),
            [ClockCommand::Running(false), ClockCommand::SetPhase(2)]
        );
        assert!(light.take_clock_commands().is_empty());
        assert!(!light.apply(&Event::CloseMessage));
    }
}
