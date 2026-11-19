//! The battle transition and the scene's phases: the swirl on the field, the scene swap, the way
//! back (`docs/app.md`, § "Battle scene"). The fight in play is `app/fight.rs`.

use super::App;
use super::status;
use crate::battle;
use crate::battle::BattleScene;
use crate::battle::fight::Outcome as FightOutcome;
use crate::battle::transition;
use crate::battle::transition::Swirl;
use crate::load;
use crate::load::MapEntry;
use dq9_core::CollisionWorld;
use dq9_core::Hero;
use dq9_core::field::Field;
use dq9_render::AnimatedScene;
use dq9_render::Camera;

/// Half field of view the field camera is given back after the encounter swirl
/// (`SwirlEffect_Release`, `0x02046f94`: roll 0, half field of view 15°;
/// `battle/transition.rs`, `docs/re/battle-transition.md`).
const FIELD_HALF_FOV: f32 = 15.0;

/// Steps the scene is shown before the stand-in settles it (there is no battle logic yet).
const BATTLE_HOLD: u32 = 90;

/// Steps the scripted transition holds black when no battle follows it.
const BATTLE_TRANSITION_HOLD: u32 = 90;

/// Steps from the black to the field coming back (state 4, `SetMainBrightness(0, 30)` is the
/// field's own; the fade is 15 here) and the total of the leave.
const BATTLE_LEAVE_AT: u32 = 15;

const BATTLE_LEAVE_DONE: u32 = 45;

/// A battle phase (`docs/app.md`, § "Battle scene"): native 547's transition, the scene,
/// and the way back to the field.
#[derive(Debug)]
pub(super) enum BattleChange {
    /// Native 547 with no battle asked for (yet): the swirl rolls and narrows the field, both
    /// screens go to black, the battle track on. `ticks` counts the black hold that follows.
    Transition { swirl: Swirl, ticks: u32 },
    /// The swirl runs while the field is still drawn; the scene is built when its clock is
    /// done and both screens are black.
    Entering {
        swirl: Swirl,
        request: battle::Resolved,
        auto_leave: bool,
    },
    /// The scene is shown; the animations run.
    Fighting {
        ticks: u32,
        request: battle::Resolved,
        auto_leave: bool,
    },
    /// Both screens to black, the field back, then up; the fight's outcome settles the
    /// story when the field is back.
    Leaving {
        ticks: u32,
        request: battle::Resolved,
        restored: bool,
        outcome: Option<FightOutcome>,
    },
}

/// A battle scene a headless run built: no fixed step is spent showing it (the stand-in
/// settles it at once), so the headless timeline stays the story's and recorded input traces
/// replay identically. The window and the Web show the transition itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShownBattle {
    pub stage: u16,
    pub fighters: usize,
}

/// What the field was and what it must be again when the battle is over (native 844: the
/// map's placements are kept as the scene left them across the battle). The map, its cast,
/// its doors, its collision and the party's pose are put back unchanged.
#[derive(Debug)]
pub(super) struct SavedField {
    current: Option<(MapEntry, String)>,
    runtime: Option<AnimatedScene>,
    collision: CollisionWorld,
    field: Option<Field>,
    hero: Option<Hero>,
    hero_instance: Option<usize>,
    environment: dq9_render::Environment,
    lighting: Option<dq9_formats::lighting::LightingInfo>,
    framed: Option<[glam::Vec3; 2]>,
    camera: Camera,
    camera_home: f32,
}

/// State of the battle transition and scene: the scene while a battle runs, its phase, the
/// field set aside and the swirl's cover.
pub(super) struct Encounter {
    /// The battle scene while a battle runs; the field's scene is set aside in
    /// [`saved_field`](Self::saved_field).
    pub(super) battle: Option<BattleScene>,
    /// The battle transition or scene phase; `None` at the field.
    pub(super) battle_change: Option<BattleChange>,
    /// The field's state across a battle (native 844).
    pub(super) saved_field: Option<SavedField>,
    /// Alpha of the black cover of the transition, 0 to 1.
    pub(super) battle_cover: f32,
    /// The field scene's swirl disc, hidden until the transition places it
    /// (`docs/re/battle-transition.md`).
    pub(super) swirl: Option<dq9_render::swirl::Swirl>,
    /// The last battle a headless run built and settled at once ([`ShownBattle`]).
    pub(super) last_battle: Option<ShownBattle>,
}

impl Encounter {
    pub(super) fn new() -> Self {
        Self {
            battle: None,
            battle_change: None,
            saved_field: None,
            battle_cover: 0.0,
            swirl: None,
            last_battle: None,
        }
    }
}

impl App {
    /// The battle scene, when one is shown (tests and the next wave's UI).
    pub fn battle(&self) -> Option<&BattleScene> {
        self.encounter.battle.as_ref()
    }

    /// Whether a battle transition or scene holds the game.
    pub fn battle_is_active(&self) -> bool {
        self.encounter.battle_change.is_some()
    }

    /// The last battle a headless run built and settled at once (window and Web runs do not
    /// set it: they show the scene).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn last_battle(&self) -> Option<ShownBattle> {
        self.encounter.last_battle
    }

    /// Native 547 executed by the event player: the scripted battle's transition. The fight
    /// itself is a trigger's `Request::Battle` (or [`App::run_battle`]); the transition blacks
    /// both screens and starts the battle track, and gives the field back after
    /// [`BATTLE_TRANSITION_HOLD`] steps when no battle follows.
    pub(crate) fn battle_transition(&mut self) {
        if self.battles_instant() {
            // Headless: no fixed step is spent on a presentation (the recorded traces replay
            // exactly, `scripts/parity.ps1`).
            status("battle transition (native 547): headless, no step spent");
            return;
        }
        if self.encounter.battle_change.is_some() && self.encounter.battle.is_none() {
            return;
        }
        if self.encounter.battle_change.is_some() {
            return;
        }
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        self.audio.set_sequence(rom, Some(battle::BATTLE_TRACK));
        status("battle transition (native 547): the swirl, both screens to black");
        self.encounter.battle_change = Some(BattleChange::Transition {
            swirl: Swirl::new(),
            ticks: 0,
        });
    }

    /// Starts a battle: the record (or the explicit stage and monsters) is read now, then the
    /// transition builds the scene once both screens are black and the fight starts on it.
    /// `auto_leave` settles it after [`BATTLE_HOLD`] steps when no fight could be built (the
    /// J4 stand-in); `immediate` skips a transition that has no field to fade from. A
    /// headless run keeps the stand-in unless `--fight` asks for the engine.
    pub(crate) fn start_battle(
        &mut self,
        request: battle::Request,
        auto_leave: bool,
        immediate: bool,
    ) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let resolved = match request.resolve(rom) {
            Ok(resolved) => resolved,
            Err(e) => {
                status(&format!("battle: {e:#}"));
                return;
            }
        };
        self.audio.set_sequence(rom, Some(resolved.track));
        status(&format!(
            "battle{}: stage {}, monsters {:?}, track {}",
            resolved
                .number
                .map_or_else(String::new, |n| format!(" {n}")),
            resolved.stage,
            resolved.monsters,
            resolved.track
        ));
        if self.battles_instant() {
            // Headless: the scene is built (and checked) but no fixed step is spent showing
            // it, so the story's timeline is exactly the one recorded traces describe. The
            // stand-in then settles it as before, or `--fight` plays it with the engine.
            if self.enter_battle(&resolved) {
                self.encounter.last_battle =
                    self.encounter.battle.as_ref().map(|scene| ShownBattle {
                        stage: scene.stage(),
                        fighters: scene.fighters().len(),
                    });
                self.restore_field();
            }
            self.set_battle_cover(0.0);
            if self.options.play.fight {
                self.play_headless_fight(&resolved);
            } else {
                self.settle_battle(&resolved, None);
            }
            return;
        }
        // A preview (`--battle`, `?battle=`) has no field to fade from: its clock starts at the
        // end of the swirl, so the first step builds the scene as before.
        let swirl = if immediate {
            let mut swirl = Swirl::new();
            for _ in 0..transition::entry_ticks() {
                swirl.step();
            }
            swirl
        } else {
            Swirl::new()
        };
        self.encounter.battle_change = Some(BattleChange::Entering {
            swirl,
            request: resolved,
            auto_leave,
        });
    }

    /// One fixed step of the battle phase. Called before the field when a transition or a
    /// battle runs: the player has no control and the field's tasks wait.
    pub(super) fn step_battle(&mut self) {
        let Some(mut change) = self.encounter.battle_change.take() else {
            return;
        };
        let mut finished = false;
        match &mut change {
            BattleChange::Transition { swirl, ticks } => {
                *ticks += 1;
                swirl.step();
                self.drive_swirl(swirl);
                if *ticks >= BATTLE_TRANSITION_HOLD + transition::FADE_TICKS {
                    // No battle followed: the field comes back, its own camera and no disc.
                    self.release_swirl();
                    self.set_battle_cover(0.0);
                    if let Some(map) = self.current.as_ref().map(|(entry, _)| entry.id) {
                        self.restore_field_music(map);
                    }
                    finished = true;
                }
            }
            BattleChange::Entering {
                swirl,
                request,
                auto_leave,
            } => {
                swirl.step();
                if swirl.done() && swirl.cover() >= 1.0 {
                    let request = request.clone();
                    let auto_leave = *auto_leave;
                    self.set_battle_cover(swirl.cover());
                    // `SwirlEffect_Release` before the field is set aside: native 844 gives
                    // the camera back as it was, and the disc must not stay in its scene.
                    self.release_swirl();
                    if self.enter_battle(&request) {
                        // The fight starts on the scene; one that cannot be built leaves the
                        // stand-in to settle the battle after its hold.
                        self.combat.fight = self.make_fight(&request);
                        self.combat.battle_pad = Default::default();
                        self.talk.discard_input();
                        change = BattleChange::Fighting {
                            ticks: 0,
                            request,
                            auto_leave,
                        };
                    } else {
                        // No scene: the story does not stop on it (the stand-in of J4).
                        self.set_battle_cover(0.0);
                        self.settle_battle(&request, None);
                        finished = true;
                    }
                } else {
                    self.drive_swirl(swirl);
                }
            }
            BattleChange::Fighting {
                ticks,
                request,
                auto_leave,
            } => {
                if let Some(scene) = &mut self.encounter.battle
                    && let Err(e) = scene.step()
                {
                    status(&format!("battle animation: {e:#}"));
                }
                // The screens come up over 15 frames (`0x021d9d18`).
                let alpha = (1.0 - *ticks as f32 / 15.0).max(0.0);
                *ticks += 1;
                self.set_battle_cover(alpha);
                if self.combat.fight.is_some() {
                    if let Some(outcome) = self.drive_fight() {
                        let request = request.clone();
                        change = BattleChange::Leaving {
                            ticks: 0,
                            request,
                            restored: false,
                            outcome: Some(outcome),
                        };
                    }
                } else if *auto_leave && *ticks >= BATTLE_HOLD {
                    let request = request.clone();
                    change = BattleChange::Leaving {
                        ticks: 0,
                        request,
                        restored: false,
                        outcome: None,
                    };
                }
            }
            BattleChange::Leaving {
                ticks,
                request,
                restored,
                outcome,
            } => {
                *ticks += 1;
                if *ticks < BATTLE_LEAVE_AT {
                    // Both screens go to black over 15 steps (`SetMainBrightness(-16, 15)`).
                    self.set_battle_cover(*ticks as f32 / BATTLE_LEAVE_AT as f32);
                } else {
                    if !*restored {
                        self.restore_field();
                        *restored = true;
                    }
                    let alpha = 1.0
                        - (*ticks - BATTLE_LEAVE_AT) as f32
                            / (BATTLE_LEAVE_DONE - BATTLE_LEAVE_AT) as f32;
                    self.set_battle_cover(alpha.max(0.0));
                }
                if *ticks >= BATTLE_LEAVE_DONE {
                    self.set_battle_cover(0.0);
                    let request = request.clone();
                    let outcome = outcome.take();
                    finished = true;
                    self.settle_battle(&request, outcome);
                }
            }
        }
        if !finished {
            self.encounter.battle_change = Some(change);
        }
    }

    /// One step of the swirl on the field (`SwirlEffect_Update`, `0x0204701c`): the camera's
    /// roll and half field of view, its black cover, then the release once both are done.
    fn drive_swirl(&mut self, swirl: &Swirl) {
        if swirl.done() && swirl.cover() >= 1.0 {
            self.release_swirl();
            return;
        }
        self.view.camera.roll = swirl.roll_degrees().to_radians();
        self.view.camera.fovy = 2.0 * swirl.half_fov_degrees().to_radians();
        self.set_battle_cover(swirl.cover());
    }

    /// `SwirlEffect_Release` (`0x02046f94`): roll 0, half field of view 15°, and the disc gone
    /// from the field's scene. The uploaded instance is moved too: `AnimatedScene::upload`
    /// does not carry instance worlds, so the renderer would keep the last placed one.
    fn release_swirl(&mut self) {
        self.view.camera.roll = 0.0;
        self.view.camera.fovy = 2.0 * FIELD_HALF_FOV.to_radians();
        self.draw_swirl(None);
    }

    /// The swirl's clock while a transition still draws it: `None` at the field, in a fight and
    /// once `SwirlEffect_Release` has run (the clock is done and the cover full).
    pub(super) fn swirl_clock(&self) -> Option<Swirl> {
        match &self.encounter.battle_change {
            Some(BattleChange::Entering { swirl, .. } | BattleChange::Transition { swirl, .. })
                if !(swirl.done() && swirl.cover() >= 1.0) =>
            {
                Some(*swirl)
            }
            _ => None,
        }
    }

    /// Poses and places the field scene's disc for this frame: `Some((instance, world))` while
    /// `clock` runs, or to hide one that is still placed. `None` when there is nothing to do.
    fn place_swirl(&mut self, clock: Option<Swirl>) -> Option<(usize, glam::Mat4)> {
        let disc = self.encounter.swirl?;
        let camera = self.view.camera;
        let runtime = self.runtime.as_mut()?;
        let visible = runtime.scene.instances[disc.instance()].drawn();
        let world = match clock {
            Some(clock) => {
                if let Err(e) = disc.pose(runtime, clock.model_frame()) {
                    status(&format!("swirl animation: {e}"));
                }
                disc.place(runtime, Some(&camera))
            }
            None if visible => disc.place(runtime, None),
            None => return None,
        };
        Some((disc.instance(), world))
    }

    /// [`place_swirl`](Self::place_swirl) and the renderer's copy of the world.
    pub(super) fn draw_swirl(&mut self, clock: Option<Swirl>) {
        let Some((instance, world)) = self.place_swirl(clock) else {
            return;
        };
        if let (Some(gpu), Some(renderer)) = (&self.gpu, &mut self.renderer) {
            renderer.update_instance_world(&gpu.queue, instance, world);
        }
    }

    /// The black cover of the transition: both screens (`SetMainBrightness`/`SetSubBrightness`).
    /// The renderer applies it to the 3D screen it draws; the panel covers the other one.
    fn set_battle_cover(&mut self, alpha: f32) {
        self.encounter.battle_cover = alpha.clamp(0.0, 1.0);
        let cover = (self.encounter.battle_cover > 0.0).then_some([
            0.0,
            0.0,
            0.0,
            self.encounter.battle_cover,
        ]);
        if self.encounter.battle.is_some() {
            if let Some(renderer) = &mut self.renderer {
                renderer.environment.cover = cover;
            }
        } else {
            self.environment.cover = cover;
            if let Some(renderer) = &mut self.renderer {
                renderer.environment.cover = cover;
            }
        }
    }

    /// Builds the scene of `request` and swaps it in for the field's.
    fn enter_battle(&mut self, request: &battle::Resolved) -> bool {
        let Some(rom) = self.rom.as_deref() else {
            return false;
        };
        let story = load::story_from_state(&self.options.story, &self.state, Some(request.stage));
        let built = BattleScene::build(rom, request, self.time, self.day, &story, &self.look);
        let scene = match built {
            Ok(scene) => scene,
            Err(e) => {
                status(&format!("battle: {e:#}"));
                return false;
            }
        };
        status(&format!(
            "battle {} on stage {} {}: {} fighters, {} triangles",
            request.number.map_or_else(|| "-".into(), |n| n.to_string()),
            request.stage,
            scene.label(),
            scene.fighters().len(),
            scene.runtime().scene.triangle_count()
        ));
        self.save_field();
        let entry = scene.entry().clone();
        let label = scene.label().to_owned();
        self.lighting_info = scene.lighting().cloned();
        self.current = Some((entry, label));
        self.runtime = None;
        self.encounter.battle = Some(scene);
        self.upload();
        self.update_title();
        true
    }

    /// Sets the field aside: native 844 records the placements so that the battle leaves them
    /// as they were. The scene, the collision, the doors and the party's pose are put back
    /// unchanged when the battle is over.
    pub(super) fn save_field(&mut self) {
        let mut environment = self.environment.clone();
        environment.cover = None;
        self.encounter.saved_field = Some(SavedField {
            current: self.current.take(),
            runtime: self.runtime.take(),
            collision: std::mem::take(&mut self.collision),
            field: self.field.take(),
            hero: self.hero,
            hero_instance: self.hero_instance,
            environment,
            lighting: self.lighting_info.take(),
            framed: self.view.framed,
            camera: self.view.camera,
            camera_home: self.view.camera_home,
        });
    }

    /// Gives the field back after the battle: the same map, cast, doors, collision and party
    /// pose (`0x02017a94` restores the placements 844 recorded; nothing here moved them).
    fn restore_field(&mut self) {
        let Some(saved) = self.encounter.saved_field.take() else {
            return;
        };
        // A standalone preview has no field to restore. Keep its stage and battle camera;
        // clearing only the CPU scene would leave its GPU geometry under the field camera.
        if saved.runtime.is_none() && saved.field.is_none() {
            return;
        }
        self.encounter.battle = None;
        self.current = saved.current;
        self.runtime = saved.runtime;
        self.collision = saved.collision;
        self.field = saved.field;
        self.hero = saved.hero;
        self.hero_instance = saved.hero_instance;
        self.environment = saved.environment;
        self.lighting_info = saved.lighting;
        self.view.framed = saved.framed;
        self.view.camera = saved.camera;
        self.view.camera_home = saved.camera_home;
        if let Some((entry, _)) = &self.current {
            let map = entry.id;
            self.restore_field_music(map);
        }
        self.upload();
        self.update_title();
    }

    /// The field's tune again (`func_0209c530`), from the map and the story's flags.
    fn restore_field_music(&mut self, map: u16) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let flags = |flag: u16| self.state.flags.contains(&i32::from(flag));
        self.audio.set_map(rom, map, self.time, &flags);
    }

    /// Draws the battle scene once: the animations at the battle camera, its light and the
    /// transition's cover.
    pub(super) fn draw_battle(&mut self) -> Camera {
        let Some(camera) = self.encounter.battle.as_ref().map(|scene| *scene.camera()) else {
            return self.view.camera;
        };
        let cover = (self.encounter.battle_cover > 0.0).then_some([
            0.0,
            0.0,
            0.0,
            self.encounter.battle_cover,
        ]);
        let (Some(gpu), Some(renderer), Some(battle)) =
            (&self.gpu, &mut self.renderer, &mut self.encounter.battle)
        else {
            return camera;
        };
        if let Err(e) = battle.upload(renderer, &gpu.device, &gpu.queue) {
            status(&format!("battle upload: {e:#}"));
        }
        renderer.environment = battle.environment.clone();
        renderer.environment.cover = cover;
        camera
    }
}

#[cfg(all(test, windows))]
mod tests;
