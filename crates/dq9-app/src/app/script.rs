//! Script events in the application: starting an event (its map and models), one fixed step of the
//! event task and its outcomes, the sound step a script may poll.

use super::App;
use super::status;
use crate::event::Outcome;
use crate::event::Stage;
use crate::event::apply_collision_changes;
use crate::record::Input as Logged;
use dq9_core::scenario::Step;
use dq9_core::trigger::Raw;

impl App {
    /// An event is next in line: loads its map when it is elsewhere, then has the player add
    /// the models its script needs.
    fn begin_event(&mut self, event: u16) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let place = crate::event::place(rom, event);
        let here = self.current.as_ref().map(|(entry, _)| entry.id);
        if let Some(place) = place
            && Some(place.map) != here
        {
            let spawn = crate::event::spawn(&place);
            status(&format!("event {event} happens on map {}", place.map));
            self.quiet_events = true;
            let loaded = self.load_map(&place.map.to_string(), true, spawn, None, false);
            self.quiet_events = false;
            if !loaded {
                status(&format!("event {event}: map {} did not load", place.map));
            }
        }
        let (Some(rom), Some(scene), Some(hero)) = (
            self.rom.as_deref(),
            self.runtime.as_mut(),
            self.hero.as_mut(),
        ) else {
            return;
        };
        let mut stage = Stage {
            rom,
            world: &self.state,
            scene,
            hero,
            collision: &self.collision,
            camera: &mut self.view.camera,
            talk: &mut self.talk,
        };
        let prepared = self.events.prepare(rom, event, &mut stage);
        if prepared {
            // `EventTask_Update` (`021bc77c`): the opening scenes set `540/770 + list rank +
            // 0x38e` on their first non-loading update, from the `EventScript` already loaded.
            if let Some(flag) = self.events.prepared_scene_flag() {
                self.state.flags.insert(i32::from(flag));
            }
            self.upload();
        }
        // `ov_001::02154f24`: an event whose list record has bit 0 or 2 in its flags records
        // the cast (`0x0203e0b0`) for the end of the event to give it back.
        if place.is_some_and(|place| place.keeps_cast())
            && let (Some((entry, _)), Some(scene)) = (&self.current, self.runtime.as_ref())
        {
            self.events.keep_cast(scene, entry.id);
        }
    }

    /// One fixed step while an event runs: the player is locked out and the script drives
    /// the actors, the camera and the message window. Returns `false` when the scene broke.
    pub(super) fn step_event(&mut self) -> bool {
        if let Some(event) = self.events.pending_start() {
            self.begin_event(event);
        }
        let outcomes = {
            let (Some(rom), Some(scene), Some(hero)) = (
                self.rom.as_deref(),
                self.runtime.as_mut(),
                self.hero.as_mut(),
            ) else {
                return true;
            };
            let mut stage = Stage {
                rom,
                world: &self.state,
                scene,
                hero,
                collision: &self.collision,
                camera: &mut self.view.camera,
                talk: &mut self.talk,
            };
            self.events.step(&mut stage)
        };
        self.drain_placement_changes();
        self.talk.step();
        self.apply_event_lighting();
        for outcome in outcomes {
            match outcome {
                Outcome::Started(id) => self.journal.record(self.tick, Step::EventStarted(id)),
                Outcome::Ended(id) => {
                    self.journal.record(self.tick, Step::EventEnded(id));
                    // `EventTask_Update` (`ov_017::021bc77c`) rebuilds the cast an event kept
                    // aside (`ov_017::021a2fa0`, task `+0x90` or `+0x50 & 4`) before it fires
                    // trigger event 11.
                    if self
                        .rom
                        .as_deref()
                        .and_then(|rom| crate::event::place(rom, id))
                        .is_some_and(|place| place.keeps_cast())
                    {
                        self.reload_cast();
                    }
                    // The game fires trigger event 11 when an event ends (`EventTask_Update`).
                    if let Some(field) = &mut self.field {
                        let events = field.event_ended(id, &mut self.state);
                        self.apply_events(events);
                    }
                }
                Outcome::Sound(sound) => {
                    if let Some(rom) = self.rom.as_deref() {
                        self.audio.script_sound(rom, &sound);
                    }
                }
                Outcome::SetQuestSignal(value) => {
                    // Native 559: quest manager `+0x490`, read by trigger condition 0x1f
                    // (`Raw::Quest490`). Applied in outcome order, before the `Ended` that
                    // runs event 11 of the same event.
                    self.state.raw.insert(Raw::Quest490, u32::from(value));
                }
                Outcome::DoorSound { block, placement } => {
                    self.play_door_sound(block, placement);
                }
                Outcome::BattleTransition => self.battle_transition(),
                Outcome::Finished => {}
            }
        }
        if let Some(runtime) = &mut self.runtime
            && let Err(e) = runtime.step(&self.view.camera)
        {
            status(&format!("animation update failed: {e}"));
            self.runtime = None;
            self.renderer = None;
            return false;
        }
        true
    }

    /// One fixed step of the sound: the waits of a jingle, which a script may be polling.
    pub(super) fn step_sound(&mut self) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        self.audio.step(rom);
        // The sound device decides when a jingle ends, by wall time: that end is an input of
        // the logic. A session records it; a replay takes the recorded one, whatever its own
        // (silent) audio says. Read before the talk sounds, as the step always did: a sound
        // that asks for a jingle ends it in the next step.
        let mut ended = self.audio.take_jingle_ended();
        for sound in self.talk.take_sounds() {
            self.audio.talk_sound(rom, sound);
        }
        if self.recording.replay.is_some() {
            ended = std::mem::take(&mut self.recording.jingle_ended);
        } else if ended {
            // The step begun by `step_begin` has already been counted.
            self.log_input_at(self.steps - 1, Logged::JingleEnded);
        }
        if ended {
            self.events.jingle_ended();
        }
    }

    /// Natives 574/575 applied by a running event: the scene bindings were updated by the
    /// event player, and the collision follows here (the collision world belongs to the app).
    pub(super) fn drain_placement_changes(&mut self) {
        let changes = self.events.take_collision_changes();
        apply_collision_changes(&mut self.collision, changes);
    }
}

#[cfg(all(test, windows))]
mod tests;
