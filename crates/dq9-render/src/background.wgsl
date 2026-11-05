// Vertical gradient behind the scene (LightingManager::DrawBackgroundGradient, EU 0x02052420):
// top colour at the top edge, the horizon colour on row `center`, the bottom colour at the
// bottom edge, interpolated linearly down the screen.

struct Bg {
    top: vec4<f32>,
    mid: vec4<f32>,
    bottom: vec4<f32>,
    // x: horizon row from the top, 0..1.
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> bg: Bg;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) v: f32,
}

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var out: Out;
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.v = y;
    return out;
}

struct FragOut {
    @location(0) color: vec4<f32>,
    @location(1) id: u32,
}

@fragment
fn fs_main(in: Out) -> FragOut {
    let c = bg.params.x;
    var color: vec4<f32>;
    if in.v < c {
        color = mix(bg.top, bg.mid, in.v / max(c, 0.0001));
    } else {
        color = mix(bg.mid, bg.bottom, (in.v - c) / max(1.0 - c, 0.0001));
    }
    // The game draws it with polygon id 0.
    return FragOut(vec4<f32>(color.rgb, 1.0), 0u);
}
