// 2D overlay: textured, tinted quads in screen pixels from one atlas. The canvas size is a
// uniform so the same atlas can address a 256x192 DS screen or a 960x540 HUD.

struct Screen {
    // x, y: canvas size; z, w: unused padding.
    size: vec4<f32>,
}

struct Vertex {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct Varying {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
@group(0) @binding(2) var<uniform> screen: Screen;

@vertex
fn vs_main(v: Vertex) -> Varying {
    var out: Varying;
    // Canvas pixels to clip space, y down.
    out.clip = vec4<f32>(
        v.pos.x / screen.size.x * 2.0 - 1.0,
        1.0 - v.pos.y / screen.size.y * 2.0,
        0.0,
        1.0,
    );
    out.uv = v.uv;
    out.color = v.color;
    return out;
}

@fragment
fn fs_main(v: Varying) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, v.uv);
    return texel * v.color;
}
