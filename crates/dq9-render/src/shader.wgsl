// Map shader: DS-like texture/vertex colour combination, DS vertex lighting, basic-mode vertex
// colour transform and DS fog (see docs/app.md and docs/re/lighting.md).

struct Camera {
    view_proj: mat4x4<f32>,
}

// The zone's lighting for this frame (`EnvUniform` in lighting.rs).
struct Env {
    light_dir: array<vec4<f32>, 2>,
    light_color: array<vec4<f32>, 2>,
    // rgb: ambient colour of terrain models, w: 1 when given.
    ambient: vec4<f32>,
    model_diffuse: vec4<f32>,
    sprite_diffuse: vec4<f32>,
    // rgb: vertex colour multiplier, w: 1 when the transform is active.
    tint_mix: vec4<f32>,
    // x: brightness, y: contrast.
    tint_bc: vec4<f32>,
    // rgb: fog colour, w: 1 when fog changes alpha only.
    fog_color: vec4<f32>,
    // x: FOG_OFFSET in Z-buffer units, y: depth shift, z: 1 when fog is on.
    fog_params: vec4<f32>,
    // The 34 densities of the engine's table.
    fog_table: array<vec4<f32>, 9>,
    // The camera's +z axis in world space (towards the viewer).
    eye_back: vec4<f32>,
    // x: near, y: far of the game's projection (the DS depth buffer is computed with them).
    zplanes: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> env: Env;

struct Material {
    // Rows of the 2x3 affine transform of the normalised texture coordinate.
    uv_s: vec4<f32>,
    uv_t: vec4<f32>,
    // x: polygon alpha (0..1), y: 1 for decal mode, 0 for modulation, z: polygon id.
    params: vec4<f32>,
    // rgb: DIF_AMB diffuse, w: shade (0 unlit, 1 map, 2 sprite, 3 character).
    diffuse: vec4<f32>,
    // rgb: DIF_AMB ambient, w: light enable bits (POLYGON_ATTR 0-3).
    ambient: vec4<f32>,
    // rgb: specular, w: 1 for a terrain model (ambient from the zone).
    specular: vec4<f32>,
    // rgb: emission, w: 1 when POLYGON_ATTR bit 15 enables fog.
    emission: vec4<f32>,
}

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) texcoord: vec2<f32>,
    @location(2) color: vec3<f32>,
    // Instance world matrix, by column.
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) normal: vec3<f32>,
    @location(8) tint: f32,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
}

// 5-bit channels, truncated like the geometry engine.
fn quantize(c: vec3<f32>) -> vec3<f32> {
    return floor(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * 31.0 + 0.001) / 31.0;
}

// LightingManager::ColorTransformTintBrightnessContrast (EU 0x02050980).
fn tint_color(c: vec3<f32>) -> vec3<f32> {
    let v5 = floor(c * 31.0 + 0.5);
    let scaled = min(floor(v5 * env.tint_mix.xyz), vec3<f32>(31.0));
    var v = clamp(scaled / 31.0 + env.tint_bc.x, vec3<f32>(0.0), vec3<f32>(1.0));
    let contrast = env.tint_bc.y;
    let above = max(v + contrast, vec3<f32>(0.5));
    let below = min(v - contrast, vec3<f32>(0.5));
    v = select(below, above, v > vec3<f32>(0.5));
    v = clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
    return floor(v * 31.0 + 0.001) / 31.0;
}

// The DS lighting equation (GBATEK "DS 3D Lighting"; melonDS `CalculateLighting`): emission
// plus, for each enabled light, light colour times diffuse, ambient and specular terms.
fn light_vertex(n: vec3<f32>) -> vec3<f32> {
    var c = material.emission.xyz;
    let mask = u32(material.ambient.w + 0.5);
    var ambient = material.ambient.xyz;
    if material.specular.w > 0.5 && env.ambient.w > 0.5 {
        ambient = env.ambient.xyz;
    }
    let spec = material.specular.xyz;
    let has_spec = any(spec > vec3<f32>(0.0));
    for (var i = 0u; i < 2u; i = i + 1u) {
        if (mask & (1u << i)) == 0u {
            continue;
        }
        let l = env.light_dir[i].xyz;
        let lc = env.light_color[i].xyz;
        let d = -dot(l, n);
        var diffuse = 0.0;
        var shine = 0.0;
        if d > 0.0 {
            diffuse = min(d, 1.0);
            if has_spec {
                let s = d + dot(n, env.eye_back.xyz);
                let den = 1.0 - dot(l, env.eye_back.xyz);
                shine = clamp(2.0 * s * s / den - 1.0, 0.0, 1.0);
            }
        }
        c = c + lc * (material.diffuse.xyz * diffuse + ambient + spec * shine);
    }
    return quantize(c);
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let world = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    var out: VertexOut;
    out.clip = camera.view_proj * world * vec4<f32>(v.position, 1.0);
    let uv = vec3<f32>(v.texcoord, 1.0);
    out.uv = vec2<f32>(dot(material.uv_s.xyz, uv), dot(material.uv_t.xyz, uv));
    var color = v.color;
    let kind = u32(material.diffuse.w + 0.5);
    if kind == 1u {
        if dot(v.normal, v.normal) > 0.0 {
            // The vector matrix of the DS carries rotation only: take the scale out.
            let rot = mat3x3<f32>(
                normalize(v.m0.xyz), normalize(v.m1.xyz), normalize(v.m2.xyz)
            );
            color = light_vertex(normalize(rot * v.normal));
        } else if v.tint > 0.5 && env.tint_mix.w > 0.5 {
            color = tint_color(color);
        }
    } else if kind == 2u {
        color = color * env.sprite_diffuse.xyz;
    } else if kind == 3u {
        color = color * env.model_diffuse.xyz;
    }
    out.color = color;
    return out;
}

// DS fog: density from the 24-bit Z-buffer value (melonDS `CalculateFogDensity`, verified
// against hardware by its authors), blended with the fog colour in 128ths.
fn fog_density(depth: f32) -> f32 {
    let n = env.zplanes.x;
    let f = env.zplanes.y;
    let z_ndc = (f + n) / (f - n) - 2.0 * f * n / ((f - n) * depth);
    let z = u32(clamp(floor((z_ndc * 16384.0 + 16383.0) * 512.0), 0.0, 16777215.0));
    let offset = u32(env.fog_params.x);
    var id = 0u;
    var frac = 0u;
    if z >= offset {
        let zz = ((z - offset) >> 2u) << u32(env.fog_params.y);
        id = zz >> 17u;
        if id >= 32u {
            id = 32u;
        } else {
            frac = zz & 0x1ffffu;
        }
    }
    let d0 = u32(env.fog_table[id / 4u][id % 4u]);
    let d1 = u32(env.fog_table[(id + 1u) / 4u][(id + 1u) % 4u]);
    var density = (d0 * (0x20000u - frac) + d1 * frac) >> 17u;
    if density >= 127u {
        density = 128u;
    }
    return f32(density) / 128.0;
}

fn shade(in: VertexOut) -> vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    let alpha = material.params.x;
    var c: vec4<f32>;
    if material.params.y > 0.5 {
        // Decal: texture over the vertex colour by texture alpha, polygon alpha kept.
        c = vec4<f32>(mix(in.color, t.rgb, t.a), alpha);
    } else {
        // Modulation: texture times vertex colour, alphas multiplied.
        c = vec4<f32>(in.color, alpha) * t;
    }
    if env.fog_params.z > 0.5 && material.emission.w > 0.5 && env.fog_color.w < 0.5 {
        c = vec4<f32>(mix(c.rgb, env.fog_color.xyz, fog_density(1.0 / in.clip.w)), c.a);
    }
    return c;
}

struct FragOut {
    @location(0) color: vec4<f32>,
    // Polygon id (POLYGON_ATTR bits 24-29), for edge marking.
    @location(1) id: u32,
}

@fragment
fn fs_main(in: VertexOut) -> FragOut {
    let c = shade(in);
    // The DS never draws pixels of alpha 0.
    if c.a < 0.5 / 31.0 {
        discard;
    }
    return FragOut(c, u32(material.params.z + 0.5));
}

// Only the pixels of full alpha (31), which the DS draws as opaque.
@fragment
fn fs_opaque_texels(in: VertexOut) -> FragOut {
    let c = shade(in);
    if c.a < 30.5 / 31.0 {
        discard;
    }
    return FragOut(c, u32(material.params.z + 0.5));
}
