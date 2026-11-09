//! A flat panel drawn inside a viewport, keeping what the target holds elsewhere. The app uses
//! it as the placeholder of the bottom DS screen, whose content is not yet rendered.

use crate::Viewport;

/// Draws the flat panel colour of [`Panel::draw`] into a target of one colour format.
#[derive(Debug)]
pub struct Panel {
    pipeline: wgpu::RenderPipeline,
}

impl Panel {
    /// Creates the pipeline for targets of `color_format`.
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dq9 panel shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("panel.wgsl").into()),
        });
        let pipeline = crate::gpu::fullscreen_pipeline(
            device,
            "panel",
            &shader,
            &[],
            None,
            &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        );
        Self { pipeline }
    }

    /// Fills `viewport` of `target` with the panel colour; the rest of `target` is kept.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        viewport: Viewport,
    ) {
        let mut pass = crate::gpu::load_pass(encoder, "panel", target);
        pass.set_viewport(
            viewport.x as f32,
            viewport.y as f32,
            viewport.width as f32,
            viewport.height as f32,
            0.0,
            1.0,
        );
        pass.set_pipeline(&self.pipeline);
        pass.draw(0..3, 0..1);
    }
}
