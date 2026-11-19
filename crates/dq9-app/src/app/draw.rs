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
use dq9_core::field;
use dq9_render::Camera;
use dq9_render::CameraMode;
use dq9_render::EnhancedOptions;
use dq9_render::Panel;
use dq9_render::Renderer;
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
