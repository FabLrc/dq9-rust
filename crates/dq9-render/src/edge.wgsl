// Edge marking (DISP3DCNT bit 5): the nearer pixel of a boundary between two polygon ids takes
// the edge colour (GBATEK "DS 3D Toon, Edge, Fog, Alpha-Blending, Anti-Aliasing"; melonDS
// `ScanlineFinalPass`: a pixel whose 4-neighbour has another id and a greater depth). The DS
// does it on its 256x192 grid, so the band is one DS pixel wide: `scale` target pixels here,
// counted inwards from the silhouette, the last one partly covered when the scale is fractional.
// With the DS's anti-aliasing on (bit 4, set in every melonDS state) the marked pixel keeps only
// `opacity` of the edge colour over the pixel it covers; the pipeline blends by alpha.

struct Edge {
    // rgb: edge colour, w: size of one DS pixel in target pixels (the 3D screen's height / 192).
    color: vec4<f32>,
    // x: opacity of the edge colour over the marked pixel.
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> edge: Edge;
@group(0) @binding(1) var ids: texture_2d<u32>;
@group(0) @binding(2) var depth: texture_depth_2d;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn farther(q: vec2<i32>, size: vec2<i32>, id: u32, z: f32) -> bool {
    if q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y {
        return false;
    }
    return textureLoad(ids, q, 0).x != id && z < textureLoad(depth, q, 0);
}

// How much of the pixel `p` the edge covers because of the boundary found along `dir`: the band
// is `scale` pixels wide, so a pixel `t - 1` pixels inside the silhouette gets what is left of
// it after those (1 for the first `floor(scale)`, the remainder for the next one).
fn band(p: vec2<i32>, dir: vec2<i32>, size: vec2<i32>, id: u32, z: f32, scale: f32) -> f32 {
    let steps = i32(ceil(scale));
    for (var t = 1; t <= steps; t = t + 1) {
        if farther(p + dir * t, size, id, z) {
            return clamp(scale + 1.0 - f32(t), 0.0, 1.0);
        }
    }
    return 0.0;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(ids));
    let p = vec2<i32>(pos.xy);
    let id = textureLoad(ids, p, 0).x;
    let z = textureLoad(depth, p, 0);
    if z >= 1.0 {
        discard;
    }
    let scale = max(edge.color.w, 1.0);
    let covered = max(
        max(
            band(p, vec2<i32>(1, 0), size, id, z, scale),
            band(p, vec2<i32>(-1, 0), size, id, z, scale),
        ),
        max(
            band(p, vec2<i32>(0, 1), size, id, z, scale),
            band(p, vec2<i32>(0, -1), size, id, z, scale),
        ),
    );
    if covered <= 0.0 {
        discard;
    }
    return vec4<f32>(edge.color.rgb, covered * edge.params.x);
}
