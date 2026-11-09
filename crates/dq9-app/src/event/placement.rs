//! Map placements the scripts hide, show or move (natives 574, 575, 584; trigger actions
//! 0x95/0x96) and the state the drawn instances keep for them.

use dq9_core::CollisionWorld;
use dq9_core::cutscene::PlacementChange;
use dq9_render::AnimatedScene;
use glam::{Mat4, Vec3};

use super::{EventPlayer, Stage};

/// The scene instances of placement `(block, id)`: a model placement may draw one instance
/// per model of its archive.
pub(super) fn placement_instances(scene: &dq9_render::Scene, block: u32, id: u16) -> Vec<usize> {
    scene
        .placements
        .iter()
        .filter(|&&(entry, placement, _)| entry == block && placement == id)
        .map(|&(_, _, instance)| instance)
        .collect()
}

/// A map placement a script hid or moved: its world before the first change and what it is
/// now.
#[derive(Debug, Clone, Copy)]
pub(super) struct Placement {
    /// Home world matrix of the placement, as built from the map (`0x02013168`).
    home: Mat4,
    /// Hidden by the script or by the chosen time of day (native 574, actions 0x95/0x96): the
    /// placement's own state.
    pub(super) hidden: bool,
    /// Hidden through an ancestor above it in the map's hierarchy, which the game hides as a
    /// whole (`02013390`); the placement's own state survives so a show of the ancestor does
    /// not re-show it when it was hidden by itself.
    pub(super) ancestor_hidden: bool,
    pub(super) position: Option<Vec3>,
    /// Rotation around the placement's own origin, radians (native 584).
    pub(super) angle: Option<f32>,
}

impl Placement {
    /// The state of a placement before a script changes it: its home transform and whether
    /// the chosen time of day hides it (a null world scale). Nothing above it is hidden when
    /// the state is first recorded, so the null scale is its own.
    pub(super) fn of(instance: &dq9_render::SceneInstance) -> Self {
        Self {
            home: instance.home,
            hidden: !instance.drawn(),
            ancestor_hidden: false,
            position: None,
            angle: None,
        }
    }

    /// The world matrix the scene draws: the home transform with the translation native 575
    /// last wrote, and a null scale while native 574 hides it or an ancestor hides it. Hiding,
    /// showing and moving are independent, so their order does not change the result.
    pub(super) fn world(&self) -> Mat4 {
        if self.hidden || self.ancestor_hidden {
            return Mat4::from_scale(Vec3::ZERO);
        }
        let mut world = self.home;
        if let Some(angle) = self.angle {
            // The door turns around the placement's own origin; the home translation and
            // scale stay (native 584 sets the record's angle, `02018310` `+0x34`).
            world *= Mat4::from_rotation_y(angle);
        }
        if let Some(position) = self.position {
            world.w_axis = position.extend(1.0);
        }
        world
    }
}

impl EventPlayer {
    /// Natives 574 and 575: the placements of the map the scripts hide, show or move. The game
    /// keeps these changes until the zone is loaded again. Native 584 turns a door placement
    /// around its own origin while its animation runs.
    pub(super) fn apply_placements(&mut self, stage: &mut Stage) {
        let doors: Vec<(i32, i32, f32)> = self
            .cutscene
            .doors()
            .iter()
            .map(|door| (door.block, door.placement, door.angle))
            .collect();
        for change in self.cutscene.take_placement_changes() {
            // The collision is the application's (it drains the queue); the drawn instances
            // follow the placement hierarchy it holds.
            self.collision_changes.push(change);
            self.apply_placement_change(stage.scene, stage.collision, change);
        }
        for (block, placement, angle) in doors {
            let instances: Vec<usize> = stage
                .scene
                .scene
                .placements
                .iter()
                .filter(|&&(entry, id, _)| {
                    i64::from(entry) == i64::from(block) && i32::from(id) == placement
                })
                .map(|&(_, _, instance)| instance)
                .collect();
            if instances.is_empty() {
                self.coverage
                    .miss(format!("door ({block}, {placement}) not in the map"));
                continue;
            }
            for instance in instances {
                let state = self
                    .bindings
                    .placements
                    .entry(instance)
                    .or_insert_with(|| Placement::of(&stage.scene.scene.instances[instance]));
                state.angle = Some(angle);
                let world = state.world();
                self.set_world(stage.scene, instance, world);
            }
        }
    }

    /// One placement change on the scene's instances; `hierarchy` is the collision's
    /// placement tree, which the drawn instances have no link to.
    pub(super) fn apply_placement_change(
        &mut self,
        scene: &mut AnimatedScene,
        hierarchy: &CollisionWorld,
        change: PlacementChange,
    ) {
        let (group, id) = match change {
            PlacementChange::Hidden { group, id, .. }
            | PlacementChange::Moved { group, id, .. } => (group, id),
        };
        let (block, placement) = (group as u32, id as u16);
        let root_instances = placement_instances(&scene.scene, block, placement);
        if root_instances.is_empty()
            && !scene
                .scene
                .undrawn_placements
                .iter()
                .any(|&(entry, part)| entry == block && part == placement)
        {
            // A placement in neither list is not in the loaded map; the game records that for
            // a later load (`func_0206ea8c`), which this runtime does not model, so it is
            // reported instead of pretended applied. A collision mesh with no drawn instance
            // may still have drawn descendants below it, which the hide follows.
            self.coverage.miss(format!(
                "placement ({group}, {id}) absent from the loaded map"
            ));
        }
        match change {
            PlacementChange::Hidden { hidden, .. } => {
                self.apply_placement_hidden(scene, hierarchy, block, placement, hidden);
            }
            PlacementChange::Moved { position, .. } => {
                for instance in root_instances {
                    // The placement at the map's own transform, even when the chosen time of
                    // day hid it: native 574 shows it again where the map put it.
                    let state = self
                        .bindings
                        .placements
                        .entry(instance)
                        .or_insert_with(|| Placement::of(&scene.scene.instances[instance]));
                    state.position = Some(Vec3::from_array(position));
                    let world = state.world();
                    self.set_world(scene, instance, world);
                }
            }
        }
    }

    /// Natives 574 and actions 0x95/0x96 on the drawn scene: hides or shows placement
    /// `(block, placement)` and every placement below it in the map's hierarchy, as the
    /// collision does. The instances keep their home transform (`Placement::home`), so a show
    /// puts each one back where the map built it; a descendant stays hidden while its own
    /// state or another hidden ancestor hides it.
    fn apply_placement_hidden(
        &mut self,
        scene: &mut AnimatedScene,
        hierarchy: &CollisionWorld,
        block: u32,
        placement: u16,
        hidden: bool,
    ) {
        let root = (block, placement);
        let mut subtree = hierarchy.placement_subtree(block, placement);
        if subtree.is_empty() {
            // The hierarchy knows no such placement: the exact instance alone follows.
            subtree.push(root);
        }
        if hidden {
            self.bindings.hidden_placements.insert(root);
        } else {
            self.bindings.hidden_placements.remove(&root);
        }
        for (block, placement) in subtree {
            let is_root = (block, placement) == root;
            // A hide reaches every descendant through the root; a show restores one only when
            // no other hidden ancestor holds it.
            let ancestor_hidden =
                (hidden && !is_root) || self.hidden_ancestor(hierarchy, block, placement);
            for instance in placement_instances(&scene.scene, block, placement) {
                let state = self
                    .bindings
                    .placements
                    .entry(instance)
                    .or_insert_with(|| Placement::of(&scene.scene.instances[instance]));
                if is_root {
                    state.hidden = hidden;
                }
                state.ancestor_hidden = ancestor_hidden;
                let world = state.world();
                self.set_world(scene, instance, world);
            }
        }
    }

    /// Whether a placement above `(block, placement)` in the map's hierarchy is hidden: the
    /// placement then stays hidden when a descendant hide is shown.
    fn hidden_ancestor(&self, hierarchy: &CollisionWorld, block: u32, placement: u16) -> bool {
        hierarchy
            .placement_path(block, placement)
            .iter()
            .rev()
            .skip(1)
            .any(|key| self.bindings.hidden_placements.contains(key))
    }
}
