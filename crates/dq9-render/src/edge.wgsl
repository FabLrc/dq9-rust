// Edge marking (DISP3DCNT bit 5): a pixel whose neighbour has another polygon id and lies
// farther away takes the edge colour (melonDS `ScanlineFinalPass`, hardware behaviour).

struct Edge {
    // rgb: edge colour, w: neighbour distance in pixels (one DS pixel).
    color: vec4<f32>,
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

fn farther(p: vec2<i32>, q: vec2<i32>, size: vec2<i32>, id: u32, z: f32) -> bool {
    if q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y {
        return false;
    }
    return textureLoad(ids, q, 0).x != id && z < textureLoad(depth, q, 0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(ids));
    let p = vec2<i32>(pos.xy);
    let id = textureLoad(ids, p, 0).x;
    let z = textureLoad(depth, p, 0);
    let k = i32(edge.color.w);
    if z >= 1.0 {
        discard;
    }
    if farther(p, p + vec2<i32>(k, 0), size, id, z)
        || farther(p, p - vec2<i32>(k, 0), size, id, z)
        || farther(p, p + vec2<i32>(0, k), size, id, z)
        || farther(p, p - vec2<i32>(0, k), size, id, z) {
        return vec4<f32>(edge.color.rgb, 1.0);
    }
    discard;
    return vec4<f32>(0.0);
}
