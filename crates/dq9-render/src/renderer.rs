//! GPU side: uploads a [`Scene`] and draws it in two passes like the DS (opaque polygons,
//! then translucent ones), with nearest filtering, per-material culling, alpha and depth
//! state.
//!
//! Per-frame hooks for animations: [`Renderer::update_vertices`] re-uploads a model's posed
//! vertices (same topology), [`Renderer::update_draws`] refreshes its translucent sort centres,
//! [`Renderer::update_material`] updates a material (pattern and SRT animations) and
//! [`Renderer::update_instance_world`] moves one instance.

use std::collections::HashMap;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::gpu::{fullscreen_pipeline, load_pass, uniform_entry};
use crate::lighting::{EnvUniform, Environment, gradient};
use crate::scene::{
    Cull, PolygonMode, Scene, SceneDraw, SceneMaterial, SceneTexture, Shade, TexelTint, Wrap,
};

/// Depth buffer format.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Rectangle of the render target drawn into, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    /// The whole target.
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    uv_s: [f32; 4],
    uv_t: [f32; 4],
    params: [f32; 4],
    diffuse: [f32; 4],
    ambient: [f32; 4],
    specular: [f32; 4],
    emission: [f32; 4],
    tint_mul: [f32; 4],
    tint_add: [f32; 4],
}

/// Colours of the background gradient (`Bg` of `background.wgsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BackgroundUniform {
    top: [f32; 4],
    mid: [f32; 4],
    bottom: [f32; 4],
    params: [f32; 4],
}

/// The two passes of a frame, as on the DS: opaque polygons, then translucent ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    Opaque,
    Translucent,
}

const PASSES: [Pass; 2] = [Pass::Opaque, Pass::Translucent];

/// Pipeline state that varies between materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    cull: Option<wgpu::Face>,
    blend: bool,
    depth_write: bool,
    depth_equal: bool,
    /// Draws only the texels of full alpha (`fs_opaque_texels`).
    opaque_texels: bool,
}

struct GpuMaterial {
    material: SceneMaterial,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct GpuModel {
    vertices: wgpu::Buffer,
    vertex_count: usize,
    indices: wgpu::Buffer,
    materials: Vec<GpuMaterial>,
    draws: Vec<SceneDraw>,
    /// Pipeline key of each draw for the opaque and translucent passes; see
    /// `Renderer::ensure_pipelines`.
    keys: Vec<[Option<PipelineKey>; 2]>,
    /// Range of the instance buffer showing this model.
    instances: Range<u32>,
}

/// Format of the polygon id buffer edge marking reads.
const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Uint;

/// Depth and polygon id buffers for one target size.
struct Targets {
    size: (u32, u32),
    depth: wgpu::TextureView,
    /// The depth buffer read back by the edge pass.
    depth_sampled: wgpu::TextureView,
    id: wgpu::TextureView,
}

/// GPU resources of one scene.
pub struct Renderer {
    color_format: wgpu::TextureFormat,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    material_layout: wgpu::BindGroupLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    camera_buffer: wgpu::Buffer,
    env_buffer: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    background_buffer: wgpu::Buffer,
    background_group: wgpu::BindGroup,
    background_pipeline: wgpu::RenderPipeline,
    textures: Vec<wgpu::TextureView>,
    translucent_textures: Vec<bool>,
    white: wgpu::TextureView,
    samplers: HashMap<[Wrap; 2], wgpu::Sampler>,
    models: Vec<GpuModel>,
    /// Translucent draws of the current frame, sorted back to front.
    translucent_draws: Vec<(f32, usize, usize, u32)>,
    instance_buffer: Option<wgpu::Buffer>,
    /// World matrix of each instance, in instance buffer order.
    instance_worlds: Vec<Mat4>,
    /// Buffer slot for each original scene instance.
    instance_slots: Vec<usize>,
    targets: Option<Targets>,
    edge_buffer: wgpu::Buffer,
    edge_layout: wgpu::BindGroupLayout,
    edge_pipeline: wgpu::RenderPipeline,
    /// The zone's lighting, fog and background, applied at the next [`render`](Self::render).
    pub environment: Environment,
    /// Width of the edge marking in target pixels; `None`: one DS pixel (the viewport height
    /// over 192), as the DS draws it when its 256×192 screen is scaled up.
    pub edge_pixels: Option<u32>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("models", &self.models.len())
            .field("textures", &self.textures.len())
            .field("instances", &self.instance_worlds.len())
            .finish_non_exhaustive()
    }
}

fn address_mode(w: Wrap) -> wgpu::AddressMode {
    match w {
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
        Wrap::Repeat => wgpu::AddressMode::Repeat,
        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
    }
}

fn material_uniform(m: &SceneMaterial) -> MaterialUniform {
    let [s, t] = m.uv_transform;
    let decal = m.mode == PolygonMode::Decal && m.texture.is_some();
    let l = &m.light;
    let tint = l.texel_tint.unwrap_or(TexelTint {
        mul: [1.0; 3],
        add: [0.0; 3],
    });
    MaterialUniform {
        uv_s: [s[0], s[1], s[2], 0.0],
        uv_t: [t[0], t[1], t[2], 0.0],
        params: [
            f32::from(m.alpha) / 31.0,
            f32::from(u8::from(decal)),
            f32::from(l.polygon_id),
            0.0,
        ],
        diffuse: [
            l.diffuse[0],
            l.diffuse[1],
            l.diffuse[2],
            match l.shade {
                Shade::Unlit => 0.0,
                Shade::Map => 1.0,
                Shade::Sprite => 2.0,
                Shade::Character => 3.0,
            },
        ],
        ambient: [
            l.ambient[0],
            l.ambient[1],
            l.ambient[2],
            f32::from(l.lights),
        ],
        specular: [
            l.specular[0],
            l.specular[1],
            l.specular[2],
            f32::from(u8::from(l.terrain)),
        ],
        emission: [
            l.emission[0],
            l.emission[1],
            l.emission[2],
            f32::from(u8::from(l.fog)),
        ],
        tint_mul: [tint.mul[0], tint.mul[1], tint.mul[2], 0.0],
        tint_add: [tint.add[0], tint.add[1], tint.add[2], 0.0],
    }
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    t: &SceneTexture,
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: t.width,
        height: t.height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&t.name),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // DS colours are display values: no sRGB decoding.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &t.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * t.width),
            rows_per_image: Some(t.height),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Uniform buffer, bind group layout and pipeline of the edge marking pass.
fn edge_pipeline(
    device: &wgpu::Device,
    color_format: wgpu::TextureFormat,
) -> (wgpu::Buffer, wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("dq9 edge shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("edge.wgsl").into()),
    });
    let texture_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("edge"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
            texture_entry(1, wgpu::TextureSampleType::Uint),
            texture_entry(2, wgpu::TextureSampleType::Depth),
        ],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("edge"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pipeline = fullscreen_pipeline(
        device,
        "edge",
        &shader,
        &[Some(&layout)],
        None,
        &[Some(wgpu::ColorTargetState {
            format: color_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })],
    );
    (buffer, layout, pipeline)
}

/// Uniform buffer, bind group and pipeline of the background gradient.
fn background_pipeline(
    device: &wgpu::Device,
    color_format: wgpu::TextureFormat,
) -> (wgpu::Buffer, wgpu::BindGroup, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("dq9 background shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("background.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("background"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("background"),
        size: std::mem::size_of::<BackgroundUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("background"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    let pipeline = fullscreen_pipeline(
        device,
        "background",
        &shader,
        &[Some(&layout)],
        Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        &[
            Some(wgpu::ColorTargetState {
                format: color_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: ID_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
        ],
    );
    (buffer, group, pipeline)
}

impl Renderer {
    /// Uploads `scene` for drawing into targets of `color_format` (use a non-sRGB format: DS
    /// colours are display values).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color_format: wgpu::TextureFormat,
        scene: &Scene,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dq9 map shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX),
                uniform_entry(1, wgpu::ShaderStages::VERTEX_FRAGMENT),
            ],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map"),
            bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<Mat4>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let env_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("environment"),
            size: std::mem::size_of::<EnvUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: env_buffer.as_entire_binding(),
                },
            ],
        });
        let (background_buffer, background_group, background_pipeline) =
            background_pipeline(device, color_format);
        let white = upload_texture(
            device,
            queue,
            &SceneTexture {
                name: "white".into(),
                palette: None,
                source: 0,
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                translucent: false,
            },
        );
        let (edge_buffer, edge_layout, edge_pipeline) = edge_pipeline(device, color_format);
        let mut renderer = Self {
            color_format,
            shader,
            pipeline_layout,
            material_layout,
            pipelines: HashMap::new(),
            camera_buffer,
            env_buffer,
            camera_group,
            background_buffer,
            background_group,
            background_pipeline,
            textures: scene
                .textures
                .iter()
                .map(|t| upload_texture(device, queue, t))
                .collect(),
            translucent_textures: scene.textures.iter().map(|t| t.translucent).collect(),
            white,
            samplers: HashMap::new(),
            models: Vec::new(),
            translucent_draws: Vec::new(),
            instance_buffer: None,
            instance_worlds: Vec::new(),
            instance_slots: Vec::new(),
            targets: None,
            edge_buffer,
            edge_layout,
            edge_pipeline,
            environment: Environment::default(),
            edge_pixels: None,
        };
        // Instances grouped by model, to draw each model's instances in one call.
        let mut order: Vec<usize> = (0..scene.instances.len()).collect();
        order.sort_by_key(|&i| scene.instances[i].model);
        renderer.instance_worlds = order.iter().map(|&i| scene.instances[i].world).collect();
        renderer.instance_slots.resize(order.len(), 0);
        for (slot, &instance) in order.iter().enumerate() {
            renderer.instance_slots[instance] = slot;
        }
        let mut ranges = vec![0..0u32; scene.models.len()];
        for (slot, &i) in order.iter().enumerate() {
            let r = &mut ranges[scene.instances[i].model];
            let slot = slot as u32;
            if r.start == r.end {
                *r = slot..slot + 1;
            } else {
                r.end = slot + 1;
            }
        }
        if !renderer.instance_worlds.is_empty() {
            let raw: Vec<[[f32; 4]; 4]> = renderer
                .instance_worlds
                .iter()
                .map(Mat4::to_cols_array_2d)
                .collect();
            renderer.instance_buffer = Some(device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("instances"),
                    contents: bytemuck::cast_slice(&raw),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                },
            ));
        }
        for (model, instances) in scene.models.iter().zip(ranges) {
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&model.name),
                contents: bytemuck::cast_slice(&model.vertices),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            // An empty buffer is not allowed for binding: keep one dummy index.
            let indices: &[u32] = if model.indices.is_empty() {
                &[0]
            } else {
                &model.indices
            };
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&model.name),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let materials = model
                .materials
                .iter()
                .map(|m| renderer.gpu_material(device, m.clone()))
                .collect();
            renderer.models.push(GpuModel {
                vertices,
                vertex_count: model.vertices.len(),
                indices,
                materials,
                draws: model.draws.clone(),
                keys: Vec::new(),
                instances,
            });
        }
        for m in 0..renderer.models.len() {
            renderer.ensure_pipelines(device, m);
        }
        renderer
    }

    fn sampler(&mut self, device: &wgpu::Device, wrap: [Wrap; 2]) -> wgpu::Sampler {
        self.samplers
            .entry(wrap)
            .or_insert_with(|| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("material"),
                    address_mode_u: address_mode(wrap[0]),
                    address_mode_v: address_mode(wrap[1]),
                    mag_filter: wgpu::FilterMode::Nearest,
                    min_filter: wgpu::FilterMode::Nearest,
                    ..Default::default()
                })
            })
            .clone()
    }

    fn gpu_material(&mut self, device: &wgpu::Device, material: SceneMaterial) -> GpuMaterial {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&material.name),
            contents: bytemuck::bytes_of(&material_uniform(&material)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = self.material_group(device, &material, &uniform);
        GpuMaterial {
            material,
            uniform,
            bind_group,
        }
    }

    fn material_group(
        &mut self,
        device: &wgpu::Device,
        material: &SceneMaterial,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let sampler = self.sampler(device, material.wrap);
        let view = material
            .texture
            .and_then(|t| self.textures.get(t))
            .unwrap_or(&self.white);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&material.name),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        })
    }

    fn translucent(&self, m: &SceneMaterial) -> bool {
        m.alpha < 31
            || m.texture
                .is_some_and(|t| self.translucent_textures.get(t).copied().unwrap_or(false))
    }

    /// Pipeline state of draw `d` of model `m` in `pass`; `None` when it draws nothing there.
    ///
    /// On the DS, pixels of alpha 31 are opaque (they update depth) even in translucent
    /// polygons, while translucent pixels update depth only with `POLYGON_ATTR` bit 11. Here a
    /// polygon of full alpha whose texture has translucent texels draws its opaque texels in
    /// the opaque pass and the others, blended, in the translucent pass.
    fn key(&self, m: usize, d: usize, pass: Pass) -> Option<PipelineKey> {
        let model = &self.models[m];
        let material = &model.materials[model.draws[d].material].material;
        if !material.visible() {
            return None;
        }
        let translucent = self.translucent(material);
        let cull = match material.cull {
            Cull::Back => Some(wgpu::Face::Back),
            Cull::Front => Some(wgpu::Face::Front),
            Cull::None | Cull::All => None,
        };
        match pass {
            Pass::Opaque if translucent && material.alpha < 31 => None,
            Pass::Opaque => Some(PipelineKey {
                cull,
                blend: false,
                depth_write: true,
                depth_equal: material.depth_equal,
                opaque_texels: translucent,
            }),
            Pass::Translucent if !translucent => None,
            Pass::Translucent => Some(PipelineKey {
                cull,
                blend: true,
                depth_write: material.translucent_depth_write,
                depth_equal: material.depth_equal,
                opaque_texels: false,
            }),
        }
    }

    fn pipeline(&mut self, device: &wgpu::Device, key: PipelineKey) -> &wgpu::RenderPipeline {
        let (shader, layout, format) = (&self.shader, &self.pipeline_layout, self.color_format);
        self.pipelines.entry(key).or_insert_with(|| {
            let vertex_attributes = wgpu::vertex_attr_array![
                0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 7 => Float32x3, 8 => Float32
            ];
            let instance_attributes = wgpu::vertex_attr_array![
                3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4
            ];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("map"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<crate::SceneVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &vertex_attributes,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &instance_attributes,
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // Front faces are counter-clockwise, as in OpenGL (apicula's viewer).
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: key.cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(key.depth_write),
                    depth_compare: Some(if key.depth_equal {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::Less
                    }),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(if key.opaque_texels {
                        "fs_opaque_texels"
                    } else {
                        "fs_main"
                    }),
                    compilation_options: Default::default(),
                    targets: &[
                        Some(wgpu::ColorTargetState {
                            format,
                            blend: key.blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        }),
                        // Only opaque polygons give their id to edge marking.
                        Some(wgpu::ColorTargetState {
                            format: ID_FORMAT,
                            blend: None,
                            write_mask: if key.blend {
                                wgpu::ColorWrites::empty()
                            } else {
                                wgpu::ColorWrites::ALL
                            },
                        }),
                    ],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// Updates one world matrix in the instance buffer without rebuilding other GPU resources.
    pub fn update_instance_world(
        &mut self,
        queue: &wgpu::Queue,
        instance: usize,
        world: Mat4,
    ) -> bool {
        let Some(&slot) = self.instance_slots.get(instance) else {
            return false;
        };
        let Some(current) = self.instance_worlds.get_mut(slot) else {
            return false;
        };
        let Some(buffer) = &self.instance_buffer else {
            return false;
        };
        *current = world;
        let offset = (slot * std::mem::size_of::<[[f32; 4]; 4]>()) as u64;
        queue.write_buffer(
            buffer,
            offset,
            bytemuck::bytes_of(&world.to_cols_array_2d()),
        );
        true
    }

    /// Re-uploads the vertices of `model` (e.g. posed by an animation), which must keep its
    /// vertex count; returns `false` otherwise.
    pub fn update_vertices(
        &self,
        queue: &wgpu::Queue,
        model: usize,
        vertices: &[crate::SceneVertex],
    ) -> bool {
        let Some(m) = self.models.get(model) else {
            return false;
        };
        if vertices.len() != m.vertex_count {
            return false;
        }
        queue.write_buffer(&m.vertices, 0, bytemuck::cast_slice(vertices));
        true
    }

    /// Creates the pipelines the draws of `model` need and stores their keys.
    ///
    /// Invariant: every path that mutates a material calls this method (`new` and
    /// `update_material`), so `render` reads the keys instead of recomputing them.
    fn ensure_pipelines(&mut self, device: &wgpu::Device, model: usize) {
        let keys: Vec<[Option<PipelineKey>; 2]> = (0..self.models[model].draws.len())
            .map(|d| PASSES.map(|pass| self.key(model, d, pass)))
            .collect();
        for key in keys.iter().flatten().flatten() {
            self.pipeline(device, *key);
        }
        self.models[model].keys = keys;
    }

    /// Updates animated material state, including alpha/pass changes. Unchanged bindings
    /// and uniforms are retained; PAT swaps do not recreate textures.
    pub fn update_material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        model: usize,
        material: usize,
        value: &SceneMaterial,
    ) -> nds_rom::Result<()> {
        let gm = self
            .models
            .get(model)
            .and_then(|m| m.materials.get(material))
            .ok_or(nds_rom::Error::Malformed("animated GPU material missing"))?;
        if gm.material == *value {
            return Ok(());
        }
        if value.texture.is_some_and(|i| i >= self.textures.len()) {
            return Err(nds_rom::Error::Malformed("animated GPU texture missing"));
        }
        let rebind = gm.material.texture != value.texture || gm.material.wrap != value.wrap;
        let uniform = gm.uniform.clone();
        let group = rebind.then(|| self.material_group(device, value, &uniform));
        let gm = &mut self.models[model].materials[material];
        gm.material.clone_from(value);
        if let Some(group) = group {
            gm.bind_group = group;
        }
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&material_uniform(value)));
        self.ensure_pipelines(device, model);
        Ok(())
    }

    /// Refreshes translucent sorting centers after a pose without touching topology.
    pub fn update_draws(&mut self, model: usize, draws: &[SceneDraw]) {
        if let Some(m) = self.models.get_mut(model) {
            for (out, draw) in m.draws.iter_mut().zip(draws) {
                out.center = draw.center;
            }
        }
    }

    /// The depth and polygon id buffers for a target of `width` × `height` pixels.
    fn targets(&mut self, device: &wgpu::Device, width: u32, height: u32) -> &Targets {
        if self
            .targets
            .as_ref()
            .is_none_or(|t| t.size != (width, height))
        {
            let size = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let texture = |format, label| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            };
            let depth = texture(DEPTH_FORMAT, "depth");
            let id = texture(ID_FORMAT, "polygon ids");
            self.targets = Some(Targets {
                size: (width, height),
                depth: depth.create_view(&wgpu::TextureViewDescriptor::default()),
                depth_sampled: depth.create_view(&wgpu::TextureViewDescriptor {
                    aspect: wgpu::TextureAspect::DepthOnly,
                    ..Default::default()
                }),
                id: id.create_view(&wgpu::TextureViewDescriptor::default()),
            });
        }
        self.targets.as_ref().expect("targets were just created")
    }

    /// Records the frame into `encoder`: clears `target` (of `width` × `height` pixels) and
    /// draws the scene in `viewport` seen by `camera`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        (target, (width, height)): (&wgpu::TextureView, (u32, u32)),
        viewport: Viewport,
        camera: &Camera,
    ) {
        let aspect = viewport.width as f32 / viewport.height.max(1) as f32;
        let view_proj = camera.view_proj(aspect);
        queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&view_proj.to_cols_array()),
        );
        let env_uniform = EnvUniform::new(&self.environment, camera);
        queue.write_buffer(&self.env_buffer, 0, bytemuck::bytes_of(&env_uniform));
        let ([top, mid, bottom], center) = gradient(&self.environment.background, camera, aspect);
        let pad = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        queue.write_buffer(
            &self.background_buffer,
            0,
            bytemuck::bytes_of(&BackgroundUniform {
                top: pad(top),
                mid: pad(mid),
                bottom: pad(bottom),
                params: [center, 0.0, 0.0, 0.0],
            }),
        );
        let (depth, ids, depth_sampled) = {
            let t = self.targets(device, width, height);
            (t.depth.clone(), t.id.clone(), t.depth_sampled.clone())
        };
        // Translucent draws, back to front by the view depth of their centres.
        let view = camera.view();
        let mut translucent = std::mem::take(&mut self.translucent_draws);
        translucent.clear();
        for (mi, model) in self.models.iter().enumerate() {
            for (di, draw) in model.draws.iter().enumerate() {
                if model.keys[di][1].is_none() {
                    continue;
                }
                for slot in model.instances.clone() {
                    let world = self.instance_worlds[slot as usize];
                    let center = (view * world).transform_point3(Vec3::from_array(draw.center));
                    translucent.push((center.z, mi, di, slot));
                }
            }
        }
        translucent.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.translucent_draws = translucent;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("map"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &ids,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            viewport.x as f32,
            viewport.y as f32,
            viewport.width as f32,
            viewport.height as f32,
            0.0,
            1.0,
        );
        // The gradient behind everything (the game draws it as the first polygon of the frame).
        pass.set_pipeline(&self.background_pipeline);
        pass.set_bind_group(0, &self.background_group, &[]);
        pass.draw(0..3, 0..1);
        let Some(instances) = &self.instance_buffer else {
            return;
        };
        pass.set_bind_group(0, &self.camera_group, &[]);
        pass.set_vertex_buffer(1, instances.slice(..));
        // Opaque polygons, then the "depth equal" ones (decals) over them.
        for decals in [false, true] {
            for model in &self.models {
                if model.instances.is_empty() {
                    continue;
                }
                pass.set_vertex_buffer(0, model.vertices.slice(..));
                pass.set_index_buffer(model.indices.slice(..), wgpu::IndexFormat::Uint32);
                for (di, draw) in model.draws.iter().enumerate() {
                    let Some(key) = model.keys[di][0] else {
                        continue;
                    };
                    if key.depth_equal != decals {
                        continue;
                    }
                    pass.set_pipeline(&self.pipelines[&key]);
                    pass.set_bind_group(1, &model.materials[draw.material].bind_group, &[]);
                    pass.draw_indexed(draw.indices.clone(), 0, model.instances.clone());
                }
            }
        }
        for &(_, mi, di, slot) in &self.translucent_draws {
            let model = &self.models[mi];
            let draw = &model.draws[di];
            let gm = &model.materials[draw.material];
            let Some(key) = model.keys[di][1] else {
                continue;
            };
            pass.set_pipeline(&self.pipelines[&key]);
            pass.set_vertex_buffer(0, model.vertices.slice(..));
            pass.set_index_buffer(model.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_bind_group(1, &gm.bind_group, &[]);
            pass.draw_indexed(draw.indices.clone(), 0, slot..slot + 1);
        }
        drop(pass);
        if let Some(color) = self.environment.edge {
            // Edge marking of the 3D engine, one DS pixel wide.
            let step = self
                .edge_pixels
                .map_or((viewport.height as f32 / 192.0).round().max(1.0), |w| {
                    w.max(1) as f32
                });
            let wide = color.map(|c| {
                if c == 0 {
                    0.0
                } else {
                    f32::from(2 * c + 1) / 63.0
                }
            });
            queue.write_buffer(
                &self.edge_buffer,
                0,
                bytemuck::bytes_of(&[wide[0], wide[1], wide[2], step]),
            );
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("edge"),
                layout: &self.edge_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.edge_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&ids),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&depth_sampled),
                    },
                ],
            });
            let mut pass = load_pass(encoder, "edges", target);
            pass.set_scissor_rect(viewport.x, viewport.y, viewport.width, viewport.height);
            pass.set_pipeline(&self.edge_pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Renders one frame offscreen and reads it back as RGBA8 rows (top to bottom), with
    /// `overlay` drawing over the finished frame (the interface of [`crate::ui`]) before the
    /// read-back. Blocks on the GPU: native targets only (on the web, mapping completes from
    /// the event loop).
    pub fn render_to_rgba_with(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (width, height): (u32, u32),
        viewport: Viewport,
        camera: &Camera,
        overlay: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
    ) -> Result<Vec<u8>, wgpu::BufferAsyncError> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let row = (4 * width).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("offscreen"),
        });
        self.render(
            device,
            queue,
            &mut encoder,
            (&view, (width, height)),
            viewport,
            camera,
        );
        overlay(&mut encoder, &view);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        rx.recv().unwrap_or(Err(wgpu::BufferAsyncError))?;
        let data = slice
            .get_mapped_range()
            .map_err(|_| wgpu::BufferAsyncError)?;
        let mut out = Vec::with_capacity((4 * width * height) as usize);
        let bgra = matches!(
            self.color_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        for r in 0..height as usize {
            let start = r * row as usize;
            for px in data[start..start + 4 * width as usize].as_chunks::<4>().0 {
                if bgra {
                    out.extend([px[2], px[1], px[0], px[3]]);
                } else {
                    out.extend_from_slice(px);
                }
            }
        }
        drop(data);
        buffer.unmap();
        Ok(out)
    }
}
