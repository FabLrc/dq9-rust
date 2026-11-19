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
}

impl Doors {
    pub(super) fn new() -> Self {
        Self {
            doors_opened: std::collections::BTreeSet::new(),
            door_angles: std::collections::BTreeMap::new(),
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
