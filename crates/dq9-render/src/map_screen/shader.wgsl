// The field's map screen: textured quads of the 256x192 top LCD, from the backdrop, the map
// picture and the party sprite sheet (three textures selected by the vertex's layer, bottom to
// top: 0 backdrop, 1 picture, 2 sprites).

struct Vertex {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) layer: f32,
}

struct Varying {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) layer: f32,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var sprites: texture_2d<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;
@group(0) @binding(3) var backdrop: texture_2d<f32>;

@vertex
fn vs_main(v: Vertex) -> Varying {
    var out: Varying;
    // Screen pixels to clip space, y down (the DS screen is 256x192).
    out.clip = vec4<f32>(v.pos.x / 128.0 - 1.0, 1.0 - v.pos.y / 96.0, 0.0, 1.0);
    out.uv = v.uv;
    out.layer = v.layer;
    return out;
}

// The layer select is divergent, so the explicit LOD form: WGSL forbids `textureSample` in
// non-uniform control flow and the browser's validator (Dawn/Tint) enforces it, while naga on
// the native side accepts it — the whole Web pass was dropped with a black canvas until this
// used `textureSampleLevel` (`crates/dq9-render/tests/shaders.rs` guards it).
@fragment
fn fs_main(v: Varying) -> @location(0) vec4<f32> {
    if v.layer > 1.5 {
        return textureSampleLevel(sprites, sprite_sampler, v.uv, 0.0);
    }
    if v.layer > 0.5 {
        return textureSampleLevel(picture, sprite_sampler, v.uv, 0.0);
    }
    return textureSampleLevel(backdrop, sprite_sampler, v.uv, 0.0);
}
