//! wgpu instance, adapter, device and window surface.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use winit::window::Window;

/// Device, queue and the configured surface of the window.
#[derive(Debug)]
pub struct Gpu {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
}

/// Instance with every backend of the platform; on the web, WebGPU when the browser has it,
/// else WebGL 2.
pub async fn instance() -> wgpu::Instance {
    wgpu::util::new_instance_with_webgpu_detection(
        wgpu::InstanceDescriptor::new_without_display_handle(),
    )
    .await
}

/// A device for `adapter`, within WebGL 2 limits so the same code runs in every browser.
pub async fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("dq9"),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            ..Default::default()
        })
        .await
        .context("requesting a GPU device")
}

impl Gpu {
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let instance = instance().await;
        let surface = instance
            .create_surface(window.clone())
            .context("creating the window surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .context("no GPU adapter")?;
        let (device, queue) = device(&adapter).await?;
        let size = window.inner_size();
        #[cfg(target_arch = "wasm32")]
        let size = crate::web::canvas_size()
            .map(|(width, height)| winit::dpi::PhysicalSize::new(width, height))
            .unwrap_or(size);
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface not supported by the adapter")?;
        // DS colours are display values: prefer a format without sRGB encoding.
        if let Some(&format) = caps.formats.iter().find(|f| !f.is_srgb()) {
            config.format = format;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        Ok(Self {
            surface,
            device,
            queue,
            config,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        #[cfg(target_arch = "wasm32")]
        let (width, height) = crate::web::canvas_size().unwrap_or((width, height));
        self.resize_physical(width, height);
    }

    fn resize_physical(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    #[cfg(target_arch = "wasm32")]
    pub fn sync_canvas_size(&mut self) {
        if let Some((width, height)) = crate::web::canvas_size()
            && (width, height) != (self.config.width, self.config.height)
        {
            self.resize_physical(width, height);
        }
    }
}
