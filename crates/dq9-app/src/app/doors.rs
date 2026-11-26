//! Doorways: the shared door state, its sound and its collision.

use super::App;
use super::status;
use crate::event::Stage;

/// What the doors of the loaded map did to the collision.
pub(super) struct Doors {
    /// Doors whose opening already fired the arrival trigger.
    pub(super) doors_opened: std::collections::BTreeSet<(i32, i32)>,
    /// Door angle last applied to the collision, to avoid redoing it every tick.
    pub(super) door_angles: std::collections::BTreeMap<(i32, i32), i32>,
    /// The type-1 door task of the field was announced in the log.
    pub(super) task_announced: bool,
}

impl Doors {
    pub(super) fn new() -> Self {
        Self {
            doors_opened: std::collections::BTreeSet::new(),
            door_angles: std::collections::BTreeMap::new(),
            task_announced: false,
        }
    }
}

impl App {
    /// A doorway the field touched (native 584): the shared door state, the same the scripts
    /// use, and its own effect once.
    pub(super) fn open_door(&mut self, block: u32, placement: u16) {
        if !self.events.open_door(block as i32, placement as i32, false) {
            return;
        }
        self.play_door_sound(block as i32, placement as i32);
        status(&format!("door {block}/{placement} opens"));
    }

    /// Native 584's effect: `g_DoorSeTable[low quartet]`, base effects 200 to 204, when the
    /// map's doorway names that quartet.
    pub(super) fn play_door_sound(&mut self, block: i32, placement: i32) {
        let Some(low) = self
            .field
            .as_ref()
            .and_then(|field| field.door_sound(block as u8, placement as u8))
        else {
            status(&format!(
                "door {block}/{placement}: no sound in this map's doorway"
            ));
            return;
        };
        if let Some(rom) = self.rom.as_deref() {
            self.audio.play_base_effect(rom, u16::from(low) + 200);
        }
    }

    /// One fixed step of the doors: when no script runs, advance the shared angles and move
    /// the scene placements; then turn the collision subtree and fire the arrival trigger
    /// once a door reaches its target.
    pub(super) fn step_doors(&mut self) {
        // The type-1 door task (`ov_017::0219cd98`, `ov_017::0219e384`): fade out, the map
        // change at black, then fade in. It advances on every 60 Hz tick and holds the
        // field; `run_steps` feeds it no pad.
        let running = self
            .field
            .as_ref()
            .is_some_and(|field| field.door_task().is_some());
        self.controls.suppress = running;
        if running {
            // The fade holds the field: the action key may not open a container or a
            // conversation, and the pilot and a replayed pad are ignored
            // (`ov_017::0219e384` clears the leader's control byte).
            self.talk.discard_input();
            self.pilot_dir = None;
            self.recording.replay_pad = None;
        } else if let Some(field) = &mut self.field {
            // The next 30 Hz field step is the application's gameplay step; a type-1 warp
            // then needs the action key (`ov_017::0219aa7c`). The pilot presses it for its
            // doors so its route keeps entering them.
            field.enable_gameplay_step();
            if self.talk.confirm_pending() || self.pilot_dir.is_some() {
                field.note_confirm();
            }
        }
        let mut task_events = Vec::new();
        let mut task_log = None;
        let mut fade_in = None;
        if let Some(field) = &mut self.field
            && field.door_task().is_some()
        {
            let before = field.door_task().map(|task| task.phase());
            if let Some(event) = field.step_door() {
                if matches!(event, dq9_core::field::Event::MapChange(_)) {
                    // The map load replaces `Field`: carry the fade-in over to the new one.
                    fade_in = field.door_task().cloned();
                }
                task_events.push(event);
            }
            let after = field.door_task().map(|task| task.phase());
            if before != after {
                task_log = Some(match after {
                    Some(dq9_core::field::DoorPhase::FadeIn) => {
                        format!(
                            "door task: fade in {} ticks",
                            dq9_core::field::DOOR_FADE_IN_TICKS
                        )
                    }
                    Some(dq9_core::field::DoorPhase::FadeOut) => format!(
                        "door task: fade out {} ticks",
                        dq9_core::field::DOOR_FADE_OUT_TICKS
                    ),
                    None => "door task: done".to_string(),
                });
            }
        }
        let running = self
            .field
            .as_ref()
            .is_some_and(|field| field.door_task().is_some());
        if running && !self.doors.task_announced {
            task_log = Some(format!(
                "door task: warp confirmed, fade out {} ticks",
                dq9_core::field::DOOR_FADE_OUT_TICKS
            ));
            self.doors.task_announced = true;
        } else if !running {
            self.doors.task_announced = false;
        }
        if let Some(line) = task_log {
            status(&line);
        }
        if !task_events.is_empty() {
            self.apply_events(task_events);
            if let (Some(task), Some(field)) = (fade_in, &mut self.field) {
                field.set_door_task(task);
            }
        }
        // The task may have ended on this tick: give the pad back at once.
        self.controls.suppress = self
            .field
            .as_ref()
            .is_some_and(|field| field.door_task().is_some());
        if self.events.doors().is_empty() {
            return;
        }
        if !(self.events.pending_start().is_some() || self.events.is_active()) {
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
            self.events.tick_doors(&mut stage);
        }
        self.drain_placement_changes();
        let count = self.events.doors().len();
        let mut opened = false;
        for index in 0..count {
            let (block, placement, angle, done) = {
                let door = &self.events.doors()[index];
                (
                    door.block,
                    door.placement,
                    door.angle_fx(),
                    door.angle >= door.target,
                )
            };
            let key = (block, placement);
            if self.doors.door_angles.get(&key).copied() != Some(angle) {
                self.collision
                    .set_door_angle(block as u32, placement as u16, angle);
                self.doors.door_angles.insert(key, angle);
            }
            if done && self.doors.doors_opened.insert(key) {
                opened = true;
            }
        }
        if opened && let Some(field) = &mut self.field {
            let events = field.door_opened(&mut self.state);
            self.apply_events(events);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::app::test_support::map_options;
    use crate::talk::Button;
    use dq9_core::field::{DOOR_FADE_IN_TICKS, DOOR_FADE_OUT_TICKS, DoorPhase, Leader};
    use glam::Vec3;

    /// `C01`'s type-1 door to `C01M10` (`dq9re doors C01` record #7, `0x72` value 7 = 1):
    /// the A edge arms task 8, the fade-out runs 15 ticks, the map changes to 110 at black,
    /// the fade-in 20 ticks more. Walking into the volume without the key does nothing.
    #[test]
    fn the_confirm_edge_runs_the_type_one_door_task() {
        let Some(rom) = testrom::load() else { return };
        let mut app = App::new_headless(map_options("C01", (1, 2, 1, true)), Some(rom.to_vec()));
        assert_eq!(app.current.as_ref().unwrap().0.id, 100);
        let leader = Leader {
            position: Vec3::new(-13.624, -1.0, 20.546),
            yaw: 0.0,
            input: Vec3::ZERO,
        };
        // No confirm: the volume holds but the task never runs.
        app.step_doors();
        assert!(
            app.field
                .as_mut()
                .unwrap()
                .step(&leader, &mut app.state)
                .is_empty()
        );
        assert!(app.field.as_ref().unwrap().door_task().is_none());
        // The A edge of `step_doors` arms it; the next field step starts the fade-out.
        app.talk.press(Button::Confirm);
        app.step_doors();
        assert!(
            app.field
                .as_mut()
                .unwrap()
                .step(&leader, &mut app.state)
                .is_empty()
        );
        app.step_doors();
        assert!(
            app.field
                .as_mut()
                .unwrap()
                .step(&leader, &mut app.state)
                .is_empty()
        );
        assert_eq!(
            app.field.as_ref().unwrap().door_task().unwrap().phase(),
            DoorPhase::FadeOut
        );
        // The next 60 Hz tick sees the task and suppresses the pad.
        app.step_doors();
        assert!(
            app.controls.suppress,
            "the pad is ignored while the task runs"
        );
        for tick in 2..DOOR_FADE_OUT_TICKS {
            app.step_doors();
            assert_eq!(
                app.current.as_ref().unwrap().0.id,
                100,
                "the map changes at black, not at tick {tick}"
            );
        }
        app.step_doors();
        assert_eq!(app.current.as_ref().unwrap().0.id, 110);
        assert_eq!(
            app.field.as_ref().unwrap().door_task().unwrap().phase(),
            DoorPhase::FadeIn
        );
        for _ in 0..DOOR_FADE_IN_TICKS {
            app.step_doors();
        }
        assert!(app.field.as_ref().unwrap().door_task().is_none());
        assert!(
            !app.controls.suppress,
            "the pad comes back after the fade-in"
        );
    }
}
