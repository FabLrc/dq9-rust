//! Map loading and changes: the field of a map, its cast, its placements and the requests the
//! field's triggers make.

use super::App;
use super::camera::arrival_camera;
use super::services::containers::Containers;
use super::status;
use crate::lighting;
use crate::load;
use dq9_core::camera::FieldCamera;
use dq9_core::cutscene::PlacementChange;
use dq9_core::field;
use dq9_core::field::Cause;
use dq9_core::field::Event;
use dq9_core::field::MapChange;
use dq9_core::scenario::Step;
use dq9_core::state::GameState;
use dq9_core::trigger::Request;
use dq9_render::CameraMode;
use glam::Vec3;
use web_time::Instant;

/// Room whose map load re-applies the gate pair (trigger action `0xa9`): `func_020ae7d4`
/// compares the loaded map with the constant at `0x020ae88c`, `0xbd1b` = this id with its
/// bytes swapped, and returns on any other map.
const GATE_ROOM: u16 = 7101;

impl App {
    /// Loads map `key` at the current time of day; `frame` resets the camera onto it. `state`
    /// (a restored save) places the hero on its own map when there is no `spawn`.
    pub(super) fn load(
        &mut self,
        key: &str,
        frame: bool,
        spawn: Option<(Vec3, f32)>,
        state: Option<&GameState>,
    ) -> bool {
        self.load_map(key, frame, spawn, state, false)
    }

    /// [`App::load`]; `transition` is set when the hero arrives through a door or a warp,
    /// which runs the arrival triggers. Returns whether the map loaded.
    pub(super) fn load_map(
        &mut self,
        key: &str,
        frame: bool,
        spawn: Option<(Vec3, f32)>,
        state: Option<&GameState>,
        transition: bool,
    ) -> bool {
        let story = self.current_story(key);
        let Some(rom) = &self.rom else { return false };
        // A map load tears the talk and menu tasks down; a facility screen does not survive it.
        self.services.close();
        self.talk.abort_service();
        let started = Instant::now();
        let mut loaded = match load::load(rom, key, self.time, &story, spawn, state, &self.look) {
            Ok(l) => l,
            Err(e) => {
                status(&format!("{e:#}"));
                return false;
            }
        };
        let hero_start = loaded.hero.position();
        let summary = loaded.summary();
        self.environment = lighting::environment(loaded.lighting.as_ref(), self.day);
        self.lighting_info = loaded.lighting.clone();
        let previous = self.current.as_ref().map(|(entry, _)| entry.id);
        self.collision = loaded.collision;
        // The collision was rebuilt: even on the same map, every open door must be applied
        // again. A real map change also forgets the doors and their triggers.
        self.doors.door_angles.clear();
        if previous != Some(loaded.entry.id) {
            self.events.clear_doors();
            self.doors.doors_opened.clear();
        }
        // A same-map reload keeps the doors open (`events.doors()` survived), so the freshly
        // built collision must be turned to their current angles before anything (the pilot's
        // walkable grid, the first hero step) queries it. Otherwise the doors read as closed
        // until the next `step_doors`, and a grid built in that window never reopens.
        let doors: Vec<(i32, i32, i32)> = self
            .events
            .doors()
            .iter()
            .map(|door| (door.block, door.placement, door.angle_fx()))
            .collect();
        for (block, placement, angle) in doors {
            self.collision
                .set_door_angle(block as u32, placement as u16, angle);
            self.doors.door_angles.insert((block, placement), angle);
        }
        self.field = Some(loaded.field);
        // The map load builds its cast (`ov_017::0219e384` → `ov_017::021a2fa0`): a cast task
        // queued on the map that is left has nothing more to do.
        self.cast_reload = false;
        self.talk
            .enter_map(loaded.npcs, &mut self.state.talk_counts);
        self.hero = Some(loaded.hero);
        self.hero_instance = Some(loaded.hero_instance);
        let s = &loaded.runtime.scene;
        let milliseconds = started.elapsed().as_secs_f64() * 1000.0;
        // The scene line gets the load time, then the warnings; same bytes as `summary`.
        match summary.split_once('\n') {
            Some((line, warnings)) => {
                status(&format!("{line}, {milliseconds:.0} ms\n{warnings}"));
            }
            None => status(&format!("{summary}, {milliseconds:.0} ms")),
        }
        if frame || self.view.framed.is_none() {
            if self.view.camera.mode == CameraMode::Orbit {
                // The map load task sets the camera, not the hero's heading
                // (`ov_017::0219e384`; `docs/re/field-camera.md`).
                (self.view.camera, self.view.camera_home) =
                    arrival_camera(rom, hero_start, loaded.entry.byte_c, loaded.entry.id);
            } else {
                if let Some(bounds) = s.bounds() {
                    self.view.camera.frame(bounds);
                }
                self.view.camera.target = hero_start;
                if loaded.placed {
                    self.view.camera.yaw = loaded.hero.yaw();
                }
                self.view.camera_home = self.view.camera.yaw;
            }
            self.view.framed = s.bounds();
        } else {
            self.view.camera.follow_hero(hero_start);
        }
        // `ov_017::0219e234`: the map record's kind and flags may forbid the L/R turn.
        self.view.field_camera = FieldCamera::new(loaded.entry.byte_c);
        self.events.map_loaded(&self.view.camera);
        // `Cast_LoadMap` ends with `0x0203e2d4`: a snapshot kept on this map gives its NPCs
        // back, one kept elsewhere is dropped.
        for npc in self
            .events
            .cast_loaded(&mut loaded.runtime, loaded.entry.id)
        {
            self.talk.remove_npc(npc);
        }
        let flags = |flag: u16| self.state.flags.contains(&i32::from(flag));
        self.audio.set_map(rom, loaded.entry.id, self.time, &flags);
        self.journal.record(
            self.tick,
            Step::Map {
                id: loaded.entry.id,
                code: loaded.entry.code.clone(),
            },
        );
        // `0201ba78` (map visited): the original sets `2710 + map / 100` when the map loads,
        // before its triggers run.
        if let Some(flag) = dq9_core::state::map_visit_flag(loaded.entry.id) {
            self.state.flags.insert(i32::from(flag));
        }
        // The map's containers (`treasure.nsarc`): its `<map>.bin` and the three tables.
        self.containers = match &self.rom {
            Some(bytes) => match Containers::load(bytes, &loaded.entry.code) {
                Ok(containers) => {
                    if !containers.records().is_empty() {
                        status(&containers.summary());
                    }
                    containers
                }
                Err(e) => {
                    status(&format!("containers: {e:#}"));
                    Containers::default()
                }
            },
            None => Containers::default(),
        };
        self.current = Some((loaded.entry, loaded.label));
        // The encounter transition's swirl (`docs/re/battle-transition.md`): the field scene
        // carries it, hidden, so `redraw` can place it over the field. Only the field needs it
        // (a battle stage and a preview are loaded without one: `load::load` knows nothing of
        // the transition).
        let swirl = nds_rom::Rom::parse(rom)
            .ok()
            .and_then(|parsed| dq9_formats::map::rom_file(&parsed, dq9_render::swirl::CHR_PATH));
        self.encounter.swirl = match swirl {
            Some(chr) => match dq9_render::swirl::Swirl::add(&mut loaded.runtime, chr) {
                Ok(swirl) => swirl,
                Err(e) => {
                    status(&format!("swirl: {e:#}"));
                    None
                }
            },
            None => None,
        };
        if let Err(e) = loaded.runtime.face_camera(&self.view.camera) {
            status(&format!("facing cast: {e}"));
            return true;
        }
        self.runtime = Some(loaded.runtime);
        self.enter_roaming();
        self.upload();
        self.update_title();
        if let Some(field) = &mut self.field {
            let room = self.current.as_ref().map_or(0, |(entry, _)| entry.id);
            let mut events = field.enter(&mut self.state, transition);
            // `func_020ae53c`, the map's gimmick initialiser: the gate pair (trigger action
            // 0xa9) follows global flag 0x347 on every load, so a room re-entered after a
            // lost battle gets the open pair back. The application applies it because the
            // gate objects are its scene's placements, and only on the room the gimmick
            // belongs to: `func_020ae7d4` starts by comparing the loaded map with the
            // constant at `0x020ae88c`, `0xbd1b` — the room id **7101** with its bytes
            // swapped — and returns otherwise, so M01's and F01's own placements 13 and 15
            // are left alone.
            if room == GATE_ROOM {
                let gate = dq9_core::field::gate_pair(
                    self.state.flags.contains(&dq9_core::field::GATE_FLAG),
                );
                events.splice(0..0, gate.into_iter().map(Event::Request));
            }
            self.apply_events(events);
        }
        true
    }

    pub(super) fn change_map(&mut self, change: &MapChange) -> bool {
        /// Map changes a single step may chain before the loop is cut.
        const MAX_CHAIN: u8 = 4;
        if self.chain >= MAX_CHAIN {
            status(&format!("map change chain cut: {change:?}"));
            return false;
        }
        let from = self.current.as_ref().map(|(e, _)| e.id);
        self.journal.record(
            self.tick,
            Step::MapChange {
                from,
                to: change.destination,
                cause: format!("{:?}", change.cause),
            },
        );
        status(&format!(
            "map change {from:?} -> {} at {:?} facing {:?}: {:?}",
            change.destination, change.position, change.angle, change.cause
        ));
        let spawn = change
            .position
            .map(|p| (p, change.angle.map_or(0.0, field::yaw_of_angle)));
        self.chain += 1;
        let loaded = self.load_map(&change.destination.to_string(), true, spawn, None, true);
        self.chain -= 1;
        // Actions 0x85/0x8a encode the arrival event (`Save_Warp+0x20`, read by
        // `ov_017::0219e384` → `ov_017::0219fc88`, which puts it in the event task's list):
        // it starts once the destination map and its own entry triggers are in place. A
        // doorway or the `0x72` spawn entry is not an event, and the buffer holds no event
        // number 0 (the ROM has no `ev00000`).
        if loaded
            && change.cause == Cause::Trigger
            && let Some(entry) = change.entry
            && let Ok(entry) = u16::try_from(entry)
            && entry != 0
        {
            self.events.queue(entry);
        }
        loaded
    }

    pub(super) fn cycle_map(&mut self, delta: isize) {
        if self.battle_is_active() {
            return;
        }
        if self.maps.is_empty() {
            return;
        }
        let at = self
            .current
            .as_ref()
            .and_then(|(e, _)| self.maps.iter().position(|m| m == e))
            .unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(self.maps.len() as isize) as usize;
        let id = self.maps[next].id.to_string();
        self.load(&id, true, None, None);
    }

    /// `ov_017::021a2fa0`, the cast rebuild of task 0x30 (trigger action 0x8d,
    /// `ov_017::021c16cc`, run by the task queue `ov_017::021a3544`) and of the end of an
    /// event that kept the cast aside (`EventTask_Update`, `ov_017::021bc77c`): the cast is
    /// torn down (`ov_017::021a316c`) and loaded again with `Cast_LoadMap`
    /// (`ov_017::021a2c14`) for the current story point, on the same map, then the snapshot
    /// an event start took on this map gives back its NPCs' poses and removals
    /// (`0x0203e2d4`). The hero, the map's placements, doors, collision and triggers stay.
    /// The fade task 0x30 wraps around it (out over 15 frames, back in over 15) is not
    /// modelled: the cast changes at once.
    pub(super) fn reload_cast(&mut self) {
        let Some((entry, _)) = &self.current else {
            return;
        };
        let story = load::story_from_state(&self.options.story, &self.state, Some(entry.id));
        let (Some(rom), Some(runtime)) = (self.rom.as_deref(), self.runtime.as_mut()) else {
            return;
        };
        let npcs = match load::reload_cast(rom, runtime, entry.id, &entry.code, &story) {
            Ok(npcs) => npcs,
            Err(e) => {
                status(&format!("{e:#}"));
                return;
            }
        };
        status(&format!(
            "cast of {} rebuilt at {}.{}.{}: {} NPCs",
            entry.id,
            story.chapter,
            story.step,
            story.substep,
            npcs.len()
        ));
        self.talk.reload_npcs(npcs);
        for npc in self.events.cast_rebuilt(runtime, entry.id) {
            self.talk.remove_npc(npc);
        }
        if let Err(e) = runtime.face_camera(&self.view.camera) {
            status(&format!("facing cast: {e}"));
        }
        self.upload();
    }

    /// Action 0x7c: the NPC leaves the map. It stops being a talk target and its cast
    /// instance is hidden in the scene (the object's flag `0x8000`, `02013380`); the event
    /// player keeps it hidden through the following script updates.
    fn remove_npc(&mut self, npc: u16) {
        self.talk.remove_npc(npc);
        if let Some(scene) = self.runtime.as_mut() {
            self.events.remove_npc(scene, npc);
        }
    }

    /// Trigger actions 0x95/0x96 (`Trig_RunAction` cases 0x31/0x32): the placement `(block,
    /// id)` of the map is hidden or shown, for both the drawn instances and the collision.
    fn set_placement_hidden(&mut self, block: u16, placement: u16, hidden: bool) {
        let change = PlacementChange::Hidden {
            group: i32::from(block),
            id: i32::from(placement),
            hidden,
        };
        let Some(scene) = self.runtime.as_mut() else {
            self.collision
                .set_placement_hidden(u32::from(block), placement, hidden);
            return;
        };
        self.events
            .set_placement(scene, &mut self.collision, change);
    }

    /// Carries out what the field asks for: the first map change (loading its destination
    /// at the arrival pose), and logs the trigger requests that wait for J3. Returns whether
    /// the map changed.
    pub(super) fn apply_events(&mut self, events: Vec<Event>) -> bool {
        self.sync_day_phase();
        let mut changed = false;
        for event in events {
            match event {
                Event::MapChange(change) if !changed => {
                    changed = self.change_map(&change);
                }
                Event::MapChange(change) => {
                    status(&format!(
                        "map change ignored (already changing): {change:?}"
                    ));
                }
                Event::Request(Request::Event(id)) if !self.quiet_events => {
                    self.events.queue(id);
                }
                Event::Request(Request::Event(id)) => {
                    status(&format!("event {id} ignored while an event loads its map"));
                }
                Event::Request(Request::Battle(number)) => self.run_battle(number),
                Event::Request(Request::RemoveNpc(npc)) => self.remove_npc(npc),
                Event::Request(Request::ReloadCast) => self.cast_reload = true,
                Event::Request(Request::SetPlacementHidden {
                    block,
                    placement,
                    hidden,
                }) => self.set_placement_hidden(block, placement, hidden),
                Event::Request(Request::OpenDoor { block, placement }) => {
                    self.open_door(u32::from(block), placement)
                }
                Event::Request(Request::Talk { npc, context }) => self.talk_to(npc, context),
                Event::Request(request) => {
                    status(&format!(
                        "trigger request not supported before J3: {request:?}"
                    ));
                    self.journal
                        .record(self.tick, Step::Unhandled(format!("{request:?}")));
                }
                Event::Unsupported(code) => {
                    status(&format!("trigger condition {code:#x} is not modelled"));
                }
            }
        }
        changed
    }
}

#[cfg(all(test, windows))]
mod tests;
