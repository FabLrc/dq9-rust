// 2D overlay: textured, tinted quads in DS screen pixels (256x192) from one atlas.

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

@vertex
fn vs_main(v: Vertex) -> Varying {
    var out: Varying;
    // 256x192 screen pixels to clip space, y down.
    out.clip = vec4<f32>(v.pos.x / 128.0 - 1.0, 1.0 - v.pos.y / 96.0, 0.0, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    return out;
}

@fragment
fn fs_main(v: Varying) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, v.uv);
    return texel * v.color;
}
