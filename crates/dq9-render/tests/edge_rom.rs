//! Edge marking is one DS pixel wide at every render size, and as light as the original's
//! (`docs/re/lighting.md` § "Edge marking"): a flat "character" in front of a flat "ground" with
//! another polygon id is rendered offscreen, and the band along its silhouette is measured in DS
//! pixels (1/192 of the 3D screen's height). Needs a GPU adapter (native only); skipped without
//! one, so it never runs in CI. No ROM data: the scene is synthetic.

use std::task::{Context, Poll, Waker};

use dq9_render::scene::{Cull, MaterialLight, PolygonMode, SceneDraw, Wrap};
use dq9_render::{
    Camera, EnhancedOptions, Renderer, Scene, SceneInstance, SceneMaterial, SceneModel,
    SceneVertex, Viewport,
};
use glam::{Mat4, Vec3};

/// Minimal executor: wgpu's native futures are ready after a few polls.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = future.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = block_on(wgpu::util::new_instance_with_webgpu_detection(
        wgpu::InstanceDescriptor::new_without_display_handle(),
    ));
    let adapter =
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits:
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .ok()
}

fn material(name: &str, polygon_id: u8) -> SceneMaterial {
    SceneMaterial {
        name: name.into(),
        texture_name: None,
        palette_name: None,
        texture: None,
        wrap: [Wrap::Clamp; 2],
        alpha: 31,
        cull: Cull::None,
        mode: PolygonMode::Modulate,
        depth_equal: false,
        translucent_depth_write: false,
        uv_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        light: MaterialLight {
            polygon_id,
            ..MaterialLight::default()
        },
    }
}

/// A quad facing the camera, `half` wide and high around the origin at depth `z`.
fn quad(half: [f32; 2], z: f32, grey: f32) -> [SceneVertex; 4] {
    [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, y]| SceneVertex {
        position: [x * half[0], y * half[1], z],
        color: [grey; 3],
        ..SceneVertex::default()
    })
}

/// A dark ground (polygon id 0, like the sky and the sprites) behind a white "character"
/// (polygon id 8, like the models of the Observatory's field state).
fn silhouette_scene() -> Scene {
    let mut vertices = Vec::new();
    vertices.extend(quad([40.0, 40.0], -5.0, 0.25));
    vertices.extend(quad([3.0, 4.0], 0.0, 1.0));
    let model = SceneModel {
        archive: String::new(),
        member: String::new(),
        name: "silhouette".into(),
        vertices,
        indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        materials: vec![material("ground", 0), material("character", 8)],
        draws: vec![
            SceneDraw {
                material: 0,
                indices: 0..6,
                center: [0.0, 0.0, -5.0],
            },
            SceneDraw {
                material: 1,
                indices: 6..12,
                center: [0.0; 3],
            },
        ],
        bounds: None,
        lit: false,
        tinted: false,
    };
    Scene {
        models: vec![model],
        instances: vec![SceneInstance {
            model: 0,
            world: Mat4::IDENTITY,
            home: Mat4::IDENTITY,
            block: usize::MAX,
            instance: usize::MAX,
        }],
        ..Scene::default()
    }
}

/// How much edge (black) covers each pixel inside the silhouette, from its left border inwards
/// along the middle row: `1 - pixel / 255` over the white of the character, until the white.
fn left_band(enhanced: Option<EnhancedOptions>, width: u32, height: u32) -> Vec<f32> {
    let (device, queue) = gpu().expect("checked by the caller");
    let mut renderer = Renderer::new_with(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &silhouette_scene(),
        enhanced,
    );
    renderer.environment.edge = Some([0, 0, 0]);
    let camera = Camera {
        yaw: 0.0,
        pitch: 0.0,
        distance: 30.0,
        target: Vec3::ZERO,
        ..Camera::default()
    };
    let rgba = renderer
        .render_to_rgba_with(
            &device,
            &queue,
            (width, height),
            Viewport::full(width, height),
            &camera,
            |_, _| {},
        )
        .expect("read back");
    let row = (height / 2) as usize * width as usize;
    let grey = |x: usize| rgba[4 * (row + x)];
    // The ground is 0.25 grey (64); the first other pixel is the character's left border.
    let first = (0..width as usize)
        .find(|&x| grey(x) != 64)
        .expect("the character is in view");
    assert!(first > 0, "the ground shows left of the character");
    (first..width as usize)
        .map(|x| 1.0 - f32::from(grey(x)) / 255.0)
        .take_while(|&a| a > 0.02)
        .collect()
}

/// The band's peak opacity and its width in DS pixels (ink over the peak opacity, per DS pixel).
fn measure(enhanced: Option<EnhancedOptions>, width: u32, height: u32) -> (f32, f32) {
    let band = left_band(enhanced, width, height);
    assert!(!band.is_empty(), "{width}x{height}: no edge marking drawn");
    let ds_pixel = height as f32 / 192.0;
    let peak = band.iter().copied().fold(0.0, f32::max);
    (peak, band.iter().sum::<f32>() / peak / ds_pixel)
}

#[test]
fn the_edge_is_one_ds_pixel_wide_at_every_size() {
    if gpu().is_none() {
        eprintln!("no GPU adapter: skipped");
        return;
    }
    // 1x, 1.5x and 4x the DS screen; 3D only (`enhanced: None`, the DS look).
    for (width, height) in [(256, 192), (384, 288), (1024, 768)] {
        let (_, ds_pixels) = measure(None, width, height);
        assert!(
            (ds_pixels - 1.0).abs() < 0.05,
            "{width}x{height}: the edge is {ds_pixels:.3} DS pixels wide, the DS draws 1"
        );
    }
}

#[test]
fn the_edge_is_as_light_as_the_anti_aliased_original() {
    if gpu().is_none() {
        eprintln!("no GPU adapter: skipped");
        return;
    }
    // melonDS `ScanlineFinalPass`: a marked pixel keeps 17/32 of the edge colour over what it
    // covers; measured on the original's capture: docs/re/lighting.md.
    let (peak, _) = measure(None, 1024, 768);
    assert!(
        (peak - 17.0 / 32.0).abs() < 0.02,
        "the edge pixel is {peak:.3} edge colour, the DS blends 17/32"
    );
}

#[test]
fn the_edge_accent_stays_one_solid_physical_pixel() {
    if gpu().is_none() {
        eprintln!("no GPU adapter: skipped");
        return;
    }
    let accent = EnhancedOptions {
        edge_accent: true,
        ..EnhancedOptions::NONE
    };
    for (width, height) in [(256, 192), (1024, 768)] {
        let band = left_band(Some(accent), width, height);
        assert_eq!(band.len(), 1, "{width}x{height}: {band:?}");
        assert!(band[0] > 0.98, "{width}x{height}: {band:?}");
    }
}
