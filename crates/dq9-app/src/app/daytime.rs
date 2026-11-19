//! Time of day: the day clock, the lighting an event or an inn changes.

use super::App;
use crate::lighting;
use dq9_core::state::TimeOfDay;

impl App {
    /// What the running events did to the lighting and to the day clock (natives 578 to 589,
    /// 548, 549, 804, 808): the renderer's environment follows, the clock obeys.
    pub(super) fn apply_event_lighting(&mut self) {
        for command in self.events.take_clock_commands() {
            lighting::apply_clock_command(&mut self.day, command);
            // The application's time index is the clock's phase (set with it everywhere).
            self.time = self.day.phase().index();
        }
        if let Some(environment) = self
            .events
            .environment(self.lighting_info.as_ref(), self.day)
        {
            if let Some(renderer) = &mut self.renderer {
                renderer.environment = environment.clone();
            }
            self.environment = environment;
        }
    }

    /// Shows the current map at another time of day.
    pub(super) fn set_time(&mut self, phase: TimeOfDay) {
        if self.battle_is_active() {
            return;
        }
        self.day.set_phase(phase);
        self.time = phase.index();
        if let Some((entry, _)) = &self.current {
            let code = entry.code.clone();
            self.load(&code, false, None, None);
        }
    }

    /// Action 0xb1 (`0x6e`) of a trigger: the field changed the time of day, and the
    /// application's clock and lighting follow before anything else in the same event list.
    /// Fired before a `MapChange`, it makes the destination map load at the new time.
    ///
    /// `--time`/`--clock` keep the priority for the oracle captures. The map is **not**
    /// reloaded: `set_time` would rebuild the scene and replay its entry events; a same-map
    /// change only moves the environment.
    pub(super) fn sync_day_phase(&mut self) {
        if self.options.time.is_some() || self.options.clock.is_some() {
            return;
        }
        if self.state.time_of_day == self.day.phase().index() {
            return;
        }
        self.day.set_phase(lighting::phase(self.state.time_of_day));
        self.time = self.day.phase().index();
        self.environment = lighting::environment(self.lighting_info.as_ref(), self.day);
        if let Some(renderer) = &mut self.renderer {
            renderer.environment = self.environment.clone();
        }
    }

    /// The inn moved the day clock: the application's time, lighting and the trigger state's
    /// `time_of_day` follow (`GameState::SetTimeOfDay`, `0x02010374`).
    pub(super) fn sync_clock_from_day(&mut self) {
        self.time = self.day.phase().index();
        self.state.time_of_day = self.time;
        self.environment = lighting::environment(self.lighting_info.as_ref(), self.day);
        if let Some(renderer) = &mut self.renderer {
            renderer.environment = self.environment.clone();
        }
    }
}
