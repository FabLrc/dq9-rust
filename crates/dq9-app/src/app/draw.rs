//! Drawing: the renderer's upload, the presentation switches (layout, screens, effects, text speed)
//! and the frame the window presents.

use super::App;
use super::fight;
use super::status;
use crate::hud::HudInfo;
use crate::hud::time_label;
use crate::screens;
use crate::screens::Layout;
use crate::screens::ScreenMode;
use dq9_core::cutscene::TexelOp;
use dq9_core::field;
use dq9_core::state::{WINGED_TINT, hero_winged};
use dq9_render::AnimatedScene;
use dq9_render::Camera;
use dq9_render::CameraMode;
use dq9_render::EnhancedOptions;
use dq9_render::Panel;
use dq9_render::Renderer;
use dq9_render::TexelTint;
use glam::Vec3;

/// How the frame is presented: where the DS screens go, the enhanced effects and the bottom
/// screen's placeholder.
pub(super) struct Presentation {
    /// Where the field and its message window are drawn (`--screens`, the page's "Screens"
    /// select, `?screens=`): the port's own assignment or the original's. Not in [`Options`]
    /// because the headless drivers and their tests never draw a frame.
    pub(super) screens: ScreenMode,
    /// The enhanced presentation's switches (`--fx`/`--no-fx`, the page's checkboxes): every
    /// effect of the 16:9 and reactive layouts, all on by default. Not in [`Options`] because
    /// the headless drivers never draw a frame.
    pub(super) fx: EnhancedOptions,
    /// Placeholder of the bottom DS screen, created with the surface format.
    pub(super) panel: Option<Panel>,
}

impl Presentation {
    pub(super) fn new() -> Self {
        Self {
            screens: ScreenMode::default(),
            fx: EnhancedOptions::ALL,
            panel: None,
        }
    }
}

impl App {
    /// The hero's Celestrian form in the field: while [`hero_winged`] holds for the map and the
    /// story point, his models' palettes get [`WINGED_TINT`] added, the operation the game
    /// applies with trigger action `0x97(1)` (`ov_017::021bdb78`, `0218fcf8`, `0218fd34`: the
    /// same palette operation as native 238, so the cutscenes' hero and the field's look alike),
    /// and he wears his wings and halo ([`WingedForm`]). An event leaves the hero's colours and
    /// attachments to its script (its end puts them back, and this puts the form back with
    /// them); rebuilding the hero (equipment) drops the tint and this restores it. The first time
    /// the form is wanted its two models join the scene, which rebuilds the renderer once.
    /// Draw-only: nothing of the logic reads it.
    pub(super) fn sync_winged(&mut self) {
        let track = &self.state.tracks[self.state.track];
        let winged = self.current.as_ref().is_some_and(|(entry, _)| {
            hero_winged(entry.id, i32::from(track.chapter), i32::from(track.step))
        });
        let pose = match (&self.hero, self.events.hero_hidden()) {
            (Some(hero), false) => Some((hero.position(), field::angle_of_yaw(hero.yaw()))),
            _ => None,
        };
        let (Some(runtime), Some(instance)) = (self.runtime.as_mut(), self.hero_instance) else {
            return;
        };
        let scripted = self.events.is_active();
        match Self::winged_in_scene(
            runtime,
            instance,
            (winged, scripted),
            pose,
            self.rom.as_deref(),
        ) {
            Ok(true) => self.upload(),
            Ok(false) => {}
            Err(e) => status(&format!("winged form: {e:#}")),
        }
    }

    /// [`App::sync_winged`] on a scene: the hero's tint and his wings and halo for `winged`
    /// (the gate) unless a script runs (`scripted`: it owns both). `pose` is the hero's position
    /// and object angle, `None` while he is hidden. Also what `--screenshot` draws. Returns
    /// whether the scene gained geometry and the renderer must be built again.
    pub(crate) fn winged_in_scene(
        runtime: &mut AnimatedScene,
        hero_instance: usize,
        (winged, scripted): (bool, bool),
        pose: Option<(Vec3, f32)>,
        rom: Option<&[u8]>,
    ) -> anyhow::Result<bool> {
        if !scripted {
            tint_hero_model(runtime, hero_instance, winged_tint(winged));
        }
        show_winged_form(runtime, winged && !scripted, pose, rom)
    }

    pub(super) fn upload(&mut self) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        let (scene, environment) = match (&self.encounter.battle, &self.runtime) {
            (Some(battle), _) => (&battle.runtime().scene, battle.environment.clone()),
            (None, Some(runtime)) => (&runtime.scene, self.environment.clone()),
            (None, None) => return,
        };
        self.renderer = Some(Renderer::new_with(
            &gpu.device,
            &gpu.queue,
            gpu.config.format,
            scene,
            self.enhanced(),
        ));
        if let Some(renderer) = &mut self.renderer {
            renderer.set_reactive(self.options.layout.is_reactive());
            renderer.environment = environment;
            renderer.environment.cover = (self.encounter.battle_cover > 0.0).then_some([
                0.0,
                0.0,
                0.0,
                self.encounter.battle_cover,
            ]);
        }
    }

    /// The enhanced presentation's switches, when the layout shows its single field.
    fn enhanced(&self) -> Option<EnhancedOptions> {
        self.options
            .layout
            .fills_window()
            .then_some(self.presentation.fx)
    }

    /// Switches effects of the enhanced presentation (`--fx`/`--no-fx`, the page's
    /// checkboxes). Only mipmaps need new textures: the other effects are read by the post
    /// pass or the frame every time it is drawn.
    pub fn set_fx(&mut self, fx: EnhancedOptions) {
        let mipmaps = fx.trilinear != self.presentation.fx.trilinear;
        self.presentation.fx = fx;
        if mipmaps {
            self.upload();
            return;
        }
        let enhanced = self.enhanced();
        if let Some(renderer) = &mut self.renderer {
            renderer.set_enhanced(enhanced);
        }
    }

    pub(super) fn update_title(&self) {
        let (Some(window), Some((entry, label))) = (&self.window, &self.current) else {
            return;
        };
        let mode = match self.view.camera.mode {
            CameraMode::Orbit => "orbit",
            CameraMode::Fly => "fly",
        };
        window.set_title(&format!(
            "DQ9 Rust: {} {} ({label}), time {}, {mode}",
            entry.id, entry.code, self.time
        ));
    }

    /// Changes the presentation; entering or leaving the enhanced layouts rebuilds the
    /// renderer because the enhanced mode changes texture filtering and adds the post pass.
    pub(super) fn set_layout(&mut self, layout: Layout) {
        if layout == self.options.layout {
            return;
        }
        let rebuild = layout.fills_window() != self.options.layout.fills_window();
        self.options.layout = layout;
        if rebuild {
            self.upload();
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.set_reactive(layout.is_reactive());
        }
    }

    /// Picks where the field and its message window are drawn (`--screens`, the page's
    /// "Screens" select, `?screens=`); a battle keeps the port's assignment.
    pub fn set_screens(&mut self, screens: ScreenMode) {
        self.presentation.screens = screens;
    }

    /// Sets how fast dialogue text types (`--text-speed`, `?textspeed=`); `Normal` is the
    /// original's rate (`docs/re/dialogue.md`).
    pub fn set_text_speed(&mut self, speed: dq9_core::talk::TextSpeed) {
        self.talk.set_text_speed(speed);
    }

    pub(super) fn redraw(&mut self) {
        self.update();
        if !self.session_renders() {
            return;
        }
        if self.battle().is_some() {
            let camera = self.draw_battle();
            self.present_frame(&camera, false, false);
            return;
        }
        // The encounter's swirl over the field: posed at the clock's model frame and pinned in
        // front of the rolling, narrowing field camera (`docs/re/battle-transition.md`).
        self.draw_swirl(self.swirl_clock());
        self.sync_winged();
        if let (Some(runtime), Some(gpu), Some(renderer)) =
            (&mut self.runtime, &self.gpu, &mut self.renderer)
            && let Err(e) = runtime
                .face_camera(&self.view.camera)
                .and_then(|()| runtime.upload(renderer, &gpu.device, &gpu.queue))
        {
            status(&format!("animation upload failed: {e}"));
            self.runtime = None;
            self.renderer = None;
            return;
        }
        if let (Some(gpu), Some(renderer), Some(runtime)) =
            (&self.gpu, &mut self.renderer, &self.runtime)
        {
            // Instances an event moved.
            for index in self.events.take_dirty() {
                if let Some(instance) = runtime.scene.instances.get(index) {
                    renderer.update_instance_world(&gpu.queue, index, instance.world);
                }
            }
            // The roaming monsters move each field step (at most twelve instances).
            for (index, world) in self.roaming.instance_worlds(runtime) {
                renderer.update_instance_world(&gpu.queue, index, world);
            }
            // The hero's wings and halo follow his bones each frame.
            if let Some(form) = WingedForm::find(runtime) {
                for index in [form.wings, form.halo] {
                    renderer.update_instance_world(
                        &gpu.queue,
                        index,
                        runtime.scene.instances[index].world,
                    );
                }
            }
        }
        if let (Some(gpu), Some(renderer), Some(runtime), Some(hero), Some(index)) = (
            &self.gpu,
            &mut self.renderer,
            &self.runtime,
            &self.hero,
            self.hero_instance,
        ) {
            let world = if self.events.hero_hidden() {
                glam::Mat4::from_scale(Vec3::ZERO)
            } else {
                runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()))
            };
            if !renderer.update_instance_world(&gpu.queue, index, world) {
                status("hero instance is unavailable");
            }
        }
        let camera = self.view.camera;
        let swapped = self.events.screen_swapped();
        self.present_frame(&camera, swapped, true);
    }

    /// Presents one frame of the active scene: the 3D screen, the panel that fills the freed
    /// screen, and the message window when the field draws one. `swapped` is the script's
    /// screen swap (natives 109/110); the battle keeps the default layout.
    fn present_frame(&mut self, camera: &Camera, swapped: bool, messages: bool) {
        let (Some(gpu), Some(renderer)) = (&mut self.gpu, &mut self.renderer) else {
            return;
        };
        #[cfg(target_arch = "wasm32")]
        gpu.sync_canvas_size();
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (w, h) = (gpu.config.width, gpu.config.height);
        let screens = screens::screens(self.options.layout, w, h);
        // 109/110 (`POWCNT1` bit 15) and `--screens` decide where the field and the message
        // window go. A fight keeps the port's assignment, which is the original's for a battle
        // (`re/bottom-screen.md` § 3), so the battle scene is never moved by `--screens`.
        let mode = if self.encounter.battle.is_some() {
            ScreenMode::Port
        } else {
            self.presentation.screens
        };
        let field = screens.field_for(mode, swapped);
        let panel = self
            .presentation
            .panel
            .get_or_insert_with(|| Panel::new(&gpu.device, gpu.config.format));
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        renderer.render(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            (&view, (w, h)),
            field,
            camera,
        );
        if let Some(overlay) = screens.overlay_for(mode, swapped) {
            panel.draw(&mut encoder, &view, overlay);
        }
        if messages && self.options.layout.fills_window() {
            if let Some((entry, label)) = &self.current {
                let info = HudInfo {
                    title: label,
                    code: &entry.code,
                    time: time_label(self.day.phase()),
                    party: Some(&self.party),
                    glass: self.presentation.fx.glass,
                };
                // The 16:9 layout keeps its 960 × 540 canvas; the reactive one lays the
                // interface out over the whole window, at the window's own aspect.
                let canvas = if self.options.layout.is_reactive() {
                    crate::hud::Canvas::for_window(field.width, field.height)
                } else {
                    crate::hud::Canvas::ENHANCED
                };
                self.talk.draw_hud(
                    &gpu.device,
                    &gpu.queue,
                    gpu.config.format,
                    &mut encoder,
                    &view,
                    field,
                    canvas,
                    &info,
                );
            }
        } else if messages {
            self.talk.draw(
                &gpu.device,
                &gpu.queue,
                gpu.config.format,
                &mut encoder,
                &view,
                field,
            );
        }
        if messages
            && self.services.is_open()
            && let Some(quads) = self
                .talk
                .with_ui(|atlas, font| self.services.quads(atlas, font, &self.party))
                .flatten()
        {
            // The menu takes the screen the field does not use; a one-screen layout lays it
            // over the field so that it stays visible.
            let region = screens.overlay_for(mode, swapped).unwrap_or(screens.top);
            self.talk.draw_screens(
                &gpu.device,
                &gpu.queue,
                gpu.config.format,
                &mut encoder,
                &view,
                &[(region, quads)],
            );
        }
        if self.encounter.battle_cover <= 0.0
            && let (Some(fight), Some(scene)) = (&self.combat.fight, &self.encounter.battle)
        {
            fight::draw_ui(
                &mut self.talk,
                fight,
                scene,
                gpu,
                &mut encoder,
                &view,
                &screens,
                self.tick as u32,
            );
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
    }
}

/// The palette operation the winged form gives the hero's models (`WINGED_TINT`, added), or
/// `None` outside the form: the same as native 238 (`TexelOp` mode 0).
pub(crate) fn winged_tint(winged: bool) -> Option<TexelTint> {
    winged
        .then(|| TexelOp::new(0, WINGED_TINT))
        .flatten()
        .map(|op| TexelTint {
            mul: op.mul(),
            add: op.add(),
        })
}

/// Gives the hero's drawn model `wanted` (`ov_017::0218fcf8`), when it does not carry it yet.
/// Rebuilding the hero (equipment) drops the tint and this restores it.
pub(crate) fn tint_hero_model(
    runtime: &mut AnimatedScene,
    instance: usize,
    wanted: Option<TexelTint>,
) {
    let Some(model) = runtime.scene.instances.get(instance).map(|i| i.model) else {
        return;
    };
    let drawn = runtime.scene.models[model]
        .materials
        .first()
        .and_then(|m| m.light.texel_tint);
    if drawn != wanted {
        runtime.tint_hero(wanted);
    }
}

/// The wings and the halo of the hero's winged form in the field: the two character archives
/// the task `0x32` loads (`ov_017::021bdc34`, the files at `0x021be080` and `0x021be084`:
/// `chara_sub/s023n2.chr`, `+0x4334` of `GameResources`, and `chara_sub/s069.chr`, `+0x4338`;
/// native 809, `ov_001::02162174`, shows and hides them by those two fields). The field's draw
/// of party member 0 (`ov_017::0218eafc`) places the wings on his bone slot 2 (`chest`,
/// `func_02053e10` names slot 0 `head`, 1 `waist`, 2 `chest`) and the halo on slot 0, then draws
/// each with its own object, after the equipment (halo) and the party's children (wings). Their
/// motion is `stand`, set once when the form is entered (`021bdc34`, `MaybeSetRegularAnimation`
/// with `0x021be094`); no gait changes it (`0218e674` only advances the wings' effects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WingedForm {
    /// Scene instance of the wings.
    pub(crate) wings: usize,
    /// Scene instance of the halo.
    pub(crate) halo: usize,
}

/// Scene model names the form's two models carry, which is how [`WingedForm::find`] knows a
/// scene already has them.
const WINGS_MODEL: &str = "winged form: wings";
const HALO_MODEL: &str = "winged form: halo";

impl WingedForm {
    /// The form's instances in `scene`, when it has been added.
    pub(crate) fn find(scene: &AnimatedScene) -> Option<Self> {
        let mut wings = None;
        let mut halo = None;
        for (index, instance) in scene.scene.instances.iter().enumerate() {
            match scene.scene.models[instance.model].name.as_str() {
                WINGS_MODEL => wings = Some(index),
                HALO_MODEL => halo = Some(index),
                _ => {}
            }
        }
        Some(Self {
            wings: wings?,
            halo: halo?,
        })
    }

    /// Adds the two models to `scene`, hidden, playing their `stand` motion. New geometry: the
    /// renderer must be built from the scene again.
    fn add(scene: &mut AnimatedScene, rom: &[u8]) -> anyhow::Result<Self> {
        use anyhow::Context;
        let image = nds_rom::Rom::parse(rom).context("parsing the ROM")?;
        let mut add = |path: &str, name: &str| -> anyhow::Result<usize> {
            let chr = dq9_formats::map::rom_file(&image, path)
                .with_context(|| format!("{path} is not in the ROM"))?;
            let actor = scene.add_actor(chr, &[]).with_context(|| path.to_owned())?;
            let instance = scene
                .actor_instance(actor)
                .with_context(|| format!("{path} has no polygons"))?;
            scene.play_default_motion(actor);
            let model = scene.scene.instances[instance].model;
            scene.scene.models[model].name = name.to_owned();
            Ok(instance)
        };
        let wings = add("data/chara_sub/s023n2.chr", WINGS_MODEL)?;
        let halo = add("data/chara_sub/s069.chr", HALO_MODEL)?;
        Ok(Self { wings, halo })
    }

    /// Hangs the wings and the halo on the hero's `chest` and `head` bones for his world pose
    /// `pose` (position, object angle), or hides them without one. The matrix is the hero's
    /// world times the bone's, the model's own transform being the identity: how the scripts
    /// hang a model on a bone (`Event::AttachBone`; the scripts reset the scale of the models
    /// they attach to 1 **[hypothesis]**: the drawing code of the field is not read that far).
    fn place(
        &self,
        scene: &mut AnimatedScene,
        pose: Option<(Vec3, f32)>,
        wings_tint: Option<TexelTint>,
    ) -> anyhow::Result<()> {
        let hidden = glam::Mat4::from_scale(Vec3::ZERO);
        let hang = |scene: &AnimatedScene, bone: &str| -> anyhow::Result<glam::Mat4> {
            let Some((position, angle)) = pose else {
                return Ok(hidden);
            };
            Ok(match scene.hero_bone(bone)? {
                Some(bone) => scene.hero_world(position, angle) * bone,
                None => hidden,
            })
        };
        let wings = hang(scene, "chest")?;
        let halo = hang(scene, "head")?;
        scene.scene.instances[self.wings].world = wings;
        scene.scene.instances[self.halo].world = halo;
        let model = scene.scene.instances[self.wings].model;
        for material in &mut scene.scene.models[model].materials {
            material.light.texel_tint = wings_tint;
        }
        Ok(())
    }
}

/// Brings the hero's wings and halo up to date in `scene`: while `winged` they hang on the
/// hero's bones (`pose`: his position and object angle, `None` when he is hidden), added first
/// if the scene has none (`rom` is where they come from); otherwise they are hidden. The wings
/// carry the form's palette operation like the hero's models (`0218fd34` includes slot `0xff`,
/// the wings, in the models it recolours; the halo is not in that list). Returns whether the
/// scene gained geometry, in which case the renderer must be rebuilt.
///
/// # Errors
/// A character archive missing or malformed; nothing is shown then.
pub(crate) fn show_winged_form(
    scene: &mut AnimatedScene,
    winged: bool,
    pose: Option<(Vec3, f32)>,
    rom: Option<&[u8]>,
) -> anyhow::Result<bool> {
    let (form, added) = match WingedForm::find(scene) {
        Some(form) => (form, false),
        None if winged => {
            let Some(rom) = rom else {
                return Ok(false);
            };
            (WingedForm::add(scene, rom)?, true)
        }
        None => return Ok(false),
    };
    let pose = pose.filter(|_| winged);
    form.place(scene, pose, winged_tint(pose.is_some()))?;
    Ok(added)
}

/// The hero's drawn look in the field, through the application (real-ROM regression).
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::app::Options;

    fn app_at(map: &str, story: (u8, u8, u8)) -> Option<App> {
        let rom = testrom::load()?;
        let options = Options {
            map: Some(map.into()),
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story {
                chapter: story.0,
                step: story.1,
                substep: story.2,
                daytime: true,
                ..Default::default()
            },
            story_override: [None; 3],
            new_game: false,
            battle: None,
            play: Default::default(),
        };
        Some(App::new_headless(options, Some(rom.to_vec())))
    }

    /// The palette operation on the hero's drawn model, as the renderer would upload it.
    fn drawn_tint(app: &App) -> Option<TexelTint> {
        let runtime = app.runtime.as_ref().expect("a map is loaded");
        let instance = app.hero_instance.expect("a hero instance");
        let model = runtime.scene.instances[instance].model;
        runtime.scene.models[model]
            .materials
            .iter()
            .map(|m| m.light.texel_tint)
            .reduce(|a, b| if a == b { a } else { None })
            .flatten()
    }

    /// Arriving at Angel Falls in 1.2.1 the hero is drawn in his winged form (the palette
    /// colour `(8, 6, 16)` added, `ov_017::021bdb78`); the same map before the stage (1.1) and
    /// after it (1.4) draws his own colours, and the Observatory never does.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_hero_arrives_at_angel_falls_winged_and_is_normal_before_and_after() {
        let winged = TexelTint {
            mul: [1.0; 3],
            add: [8.0 / 31.0, 6.0 / 31.0, 16.0 / 31.0],
        };
        let Some(mut app) = app_at("M01", (1, 2, 1)) else {
            eprintln!("skipping: DQ9_ROM missing");
            return;
        };
        assert_eq!(app.current.as_ref().map(|c| c.0.id), Some(1100));
        assert_eq!(
            drawn_tint(&app),
            None,
            "nothing is drawn before the first frame"
        );
        // The frame loop calls this before every upload; an event running leaves it alone.
        app.sync_winged();
        assert!(!app.events.is_active(), "no event is playing yet");
        assert_eq!(drawn_tint(&app), Some(winged), "1.2.1 arrival at 1100");
        for (step, expected) in [(3, Some(winged)), (4, None), (1, None)] {
            app.state.tracks[app.state.track].step = step;
            app.sync_winged();
            assert_eq!(drawn_tint(&app), expected, "step {step} at 1100");
        }
    }

    /// The wings and the halo the hero wears in the field, as the renderer would place them: the
    /// world matrix of each when it is drawn (`None` when the scene has none or hides it).
    fn worn(app: &App) -> Option<[glam::Mat4; 2]> {
        let runtime = app.runtime.as_ref()?;
        let form = WingedForm::find(runtime)?;
        let world = |i: usize| {
            let m = runtime.scene.instances[i].world;
            (m.determinant().abs() > 1e-6).then_some(m)
        };
        Some([world(form.wings)?, world(form.halo)?])
    }

    /// The Celestrian form draws wings and a halo on the hero: hung on his chest and head bones
    /// (`ov_017::0218eafc`), at 1.2 and 1.3 in Angel Falls, with the wings recoloured like him
    /// (`0218fd34` includes slot `0xff`); not at 1.1 or 1.4, and an event's script owns them.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_winged_hero_wears_wings_and_a_halo_only_in_his_form() {
        let Some(mut app) = app_at("M01", (1, 2, 1)) else {
            eprintln!("skipping: DQ9_ROM missing");
            return;
        };
        assert!(
            worn(&app).is_none(),
            "nothing is drawn before the first frame"
        );
        app.sync_winged();
        let hero = app.hero.expect("a hero").position();
        let [wings, halo] = worn(&app).expect("wings and halo at 1.2.1");
        // On the hero: the wings on his back, the halo above the head (a character is ~21 rig
        // units tall, scaled by `hero_scale`).
        let scale = app.runtime.as_ref().unwrap().hero_scale().y;
        let near = |m: glam::Mat4, at: Vec3| m.w_axis.truncate().distance(at) < 0.5;
        assert!(
            near(wings, hero + Vec3::Y * 11.0 * scale),
            "wings {wings:?}"
        );
        assert!(near(halo, hero + Vec3::Y * 19.0 * scale), "halo {halo:?}");
        let runtime = app.runtime.as_ref().unwrap();
        let form = WingedForm::find(runtime).expect("the form");
        let model = runtime.scene.instances[form.wings].model;
        assert!(
            runtime.scene.models[model]
                .materials
                .iter()
                .all(|m| m.light.texel_tint == winged_tint(true)),
            "wings carry the form's palette operation"
        );
        // The same frame again adds nothing.
        let count = runtime.scene.instances.len();
        app.sync_winged();
        assert_eq!(app.runtime.as_ref().unwrap().scene.instances.len(), count);
        for (step, shown) in [(3, true), (4, false), (1, false), (2, true)] {
            app.state.tracks[app.state.track].step = step;
            app.sync_winged();
            assert_eq!(worn(&app).is_some(), shown, "step {step} at 1100");
        }
    }

    /// A scene entered outside the form never gets the wings: nothing is loaded for it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_hero_outside_the_form_has_no_wings_in_the_scene() {
        for story in [(1, 1, 1), (1, 4, 1)] {
            let Some(mut app) = app_at("M01", story) else {
                eprintln!("skipping: DQ9_ROM missing");
                return;
            };
            app.sync_winged();
            let runtime = app.runtime.as_ref().unwrap();
            assert!(WingedForm::find(runtime).is_none(), "story {story:?}");
        }
    }
}
