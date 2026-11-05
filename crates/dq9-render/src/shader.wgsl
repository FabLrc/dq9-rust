// Map shader: DS-like texture/vertex colour combination without lighting (see docs/app.md).

struct Camera {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;

struct Material {
    // Rows of the 2x3 affine transform of the normalised texture coordinate.
    uv_s: vec4<f32>,
    uv_t: vec4<f32>,
    // x: polygon alpha (0..1), y: 1 for decal mode, 0 for modulation.
    params: vec4<f32>,
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
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let world = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    var out: VertexOut;
    out.clip = camera.view_proj * world * vec4<f32>(v.position, 1.0);
    let uv = vec3<f32>(v.texcoord, 1.0);
    out.uv = vec2<f32>(dot(material.uv_s.xyz, uv), dot(material.uv_t.xyz, uv));
    out.color = v.color;
    return out;
}

fn shade(in: VertexOut) -> vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    let alpha = material.params.x;
    if material.params.y > 0.5 {
        // Decal: texture over the vertex colour by texture alpha, polygon alpha kept.
        return vec4<f32>(mix(in.color, t.rgb, t.a), alpha);
    }
    // Modulation: texture times vertex colour, alphas multiplied.
    return vec4<f32>(in.color, alpha) * t;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(in);
    // The DS never draws pixels of alpha 0.
    if c.a < 0.5 / 31.0 {
        discard;
    }
    return c;
}

// Only the pixels of full alpha (31), which the DS draws as opaque.
@fragment
fn fs_opaque_texels(in: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(in);
    if c.a < 30.5 / 31.0 {
        discard;
    }
    return c;
}
