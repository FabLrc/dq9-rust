//! The map's cast while a scene plays: which NPCs the original keeps off the screen
//! (`docs/re/characters.md`, § "The map's cast during a scene").
//!
//! The field draws the NPC table every frame (`ov_017::02195bbc` → `0x0203de94`: each entry
//! that action `0x7c` has not removed, then its shadow). Two things take the map's NPCs out of
//! a scene:
//!
//! - **The cast is torn down.** An event whose list record has bit 0 or bit 2 in value 21
//!   records the NPCs (`0x0203e0b0`) and empties the table when it starts (`ov_001::02154f24`,
//!   `ov_017::021a316c` → `0x0203cfc4`), and builds the cast again when it ends
//!   (`ov_017::021bc77c`): [`dq9_core::script::load::EventPlace::keeps_cast`].
//! - **Native 568's bit `0x20`.** When the scene is built, between its prepare entry and the
//!   next one (`ov_001::021539d8`), the mask native 568 accumulated sets bit 1 of the table's
//!   `+0x98`; the draw then skips every entry but those with flag `0x10000`, which
//!   `ov_001::021539d8` sets on the entries the script's cast binds (566 modes 3, 4 and 5) and
//!   `ov_001::02154974` clears when the scene is torn down, with `+0x98`.
//!
//! In both cases the NPCs the script binds are not kept away: mode 4 turns the model of the
//! NPC into one of the scene's characters, mode 5 flags the sprite. The NPCs are not touched,
//! only not drawn: this module hides their instances and puts them back, with the world they
//! had, when the scene is over.

use std::collections::{BTreeMap, BTreeSet};

use dq9_core::script::{Entries, Event};
use glam::{Mat4, Vec3};

use super::{EventPlayer, Stage};

/// The bit of native 568's mask whose effect is `NpcTable+0x98 |= 2` (`ov_001::021539d8`).
const BOUND_ONLY: i32 = 0x20;
/// The bits of the scene's flags word that native 568 writes: the 27 low ones, the five high
/// ones count the cast members declared (`E+0xf8`, `docs/re/stb-natives.md`).
const MASK: i32 = 0x7ff_ffff;

/// What a scene decides about the map's cast, from the events of its script, and the NPCs it
/// has set aside.
#[derive(Debug, Default)]
pub(super) struct SceneCast {
    /// The event's record tears the cast down when it starts.
    teardown: bool,
    /// Native 568's masks until the scene is built.
    flags: i32,
    /// The scene is built: the entry after the prepare entry started.
    built: bool,
    /// The map NPCs the cast declares by id (566 modes 4 and 5).
    bound: BTreeSet<i32>,
    /// The instances hidden for the scene, with the world each had.
    aside: BTreeMap<usize, Mat4>,
}

impl SceneCast {
    /// The cast of a scene whose event record tears the cast down when `teardown`.
    pub(super) fn new(teardown: bool) -> Self {
        Self {
            teardown,
            ..Self::default()
        }
    }

    /// Notes one event of the script. Returns whether it is the one that builds the scene.
    ///
    /// The game builds the scene once, when the prepare entry is over and its files are
    /// loaded (`ov_001::02154de4`); what the natives asked after that does not reach the
    /// construction, so native 568 and the cast declarations only count before it.
    pub(super) fn observe(&mut self, event: &Event) -> bool {
        if self.built {
            return false;
        }
        match event {
            Event::AddSceneFlags(mask) => self.flags |= mask & MASK,
            Event::DeclareCast {
                mode: 4 | 5,
                reference: Some(npc),
                ..
            } => {
                self.bound.insert(*npc);
            }
            Event::Phase { entry } if *entry != Entries::default().prepare => {
                self.built = true;
                return true;
            }
            _ => {}
        }
        false
    }

    /// The scene draws only the map NPCs its cast binds.
    fn bound_only(&self) -> bool {
        self.teardown || self.flags & BOUND_ONLY != 0
    }

    /// Whether the scene's cast binds map NPC `npc`.
    fn binds(&self, npc: u16) -> bool {
        self.bound.contains(&i32::from(npc))
    }

    /// Whether the scene has hidden the instance `index`.
    pub(super) fn is_aside(&self, index: usize) -> bool {
        self.aside.contains_key(&index)
    }
}

impl EventPlayer {
    /// The scene is built: hides the map NPCs it does not bind when it keeps them off the
    /// screen ([`SceneCast`]). An NPC that action `0x7c` removed is hidden already and stays
    /// so.
    pub(super) fn set_cast_aside(&mut self, stage: &mut Stage) {
        if !self.cast.bound_only() {
            return;
        }
        let scene = &stage.scene.scene;
        let hidden: Vec<(usize, Mat4)> = scene
            .instances
            .iter()
            .enumerate()
            // A map NPC: an instance of the cast (`block` unset) that is not the hero, an
            // actor or a sprite of a script (`instance` unset).
            .filter(|(_, i)| i.block == usize::MAX && i.instance != usize::MAX && i.drawn())
            .filter_map(|(index, i)| Some((index, u16::try_from(i.instance).ok()?, i.world)))
            .filter(|&(_, id, _)| !self.cast.binds(id) && !self.removed_npcs.contains(&id))
            .map(|(index, _, world)| (index, world))
            .collect();
        for (index, world) in hidden {
            self.cast.aside.insert(index, world);
            self.set_world(stage.scene, index, Mat4::from_scale(Vec3::ZERO));
        }
    }

    /// The scene is over (or another begins): the NPCs it hid are drawn where they stood.
    /// What a script did to the NPCs it bound stays: the object is the NPC's own.
    pub(super) fn bring_cast_back(&mut self, stage: &mut Stage) {
        for (index, world) in std::mem::take(&mut self.cast.aside) {
            // An NPC removed after it was hidden (action 0x7c during the scene) stays away.
            let removed = stage.scene.scene.instances.get(index).is_some_and(|i| {
                u16::try_from(i.instance).is_ok_and(|id| self.removed_npcs.contains(&id))
            });
            if !removed {
                self.set_world(stage.scene, index, world);
            }
        }
        self.cast = SceneCast::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(entry: i32) -> Event {
        Event::Phase { entry }
    }

    #[test]
    fn native_568_counts_only_before_the_scene_is_built() {
        let mut cast = SceneCast::new(false);
        assert!(!cast.observe(&phase(200)));
        assert!(!cast.observe(&Event::AddSceneFlags(0x14)));
        assert!(!cast.bound_only(), "0x14 has no 0x20");
        assert!(!cast.observe(&Event::AddSceneFlags(0x20 | 0x8000_0000u32 as i32)));
        assert_eq!(cast.flags, 0x34, "the five high bits are not the mask");
        assert!(cast.bound_only());
        assert!(cast.observe(&phase(100)), "the main entry builds the scene");
        // Asked in the main entry: the construction is over, nothing reads it.
        let mut late = SceneCast::new(false);
        assert!(!late.observe(&phase(200)));
        assert!(late.observe(&phase(100)));
        assert!(!late.observe(&Event::AddSceneFlags(0x34)));
        assert!(!late.bound_only());
    }

    #[test]
    fn the_cast_binds_its_map_npcs_by_cast_mode() {
        let declare = |mode, reference| Event::DeclareCast {
            mode,
            path: None,
            reference,
            attach: 1,
            option: None,
            character: None,
        };
        let mut cast = SceneCast::new(false);
        cast.observe(&declare(4, Some(99)));
        cast.observe(&declare(5, Some(133)));
        cast.observe(&declare(2, None));
        cast.observe(&declare(0, None));
        assert!(cast.binds(99) && cast.binds(133));
        assert!(!cast.binds(127));
        // A scene without a prepare entry builds at its first phase.
        let mut bare = SceneCast::new(true);
        assert!(bare.observe(&phase(100)));
        assert!(
            bare.bound_only(),
            "a torn-down cast keeps the NPCs away too"
        );
    }
}
