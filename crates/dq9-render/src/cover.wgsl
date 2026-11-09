// The screen cover of natives 102/103 (`func_0207051c`, `0x0207052c`): one translucent polygon
// over the whole 3D screen, alpha `level / 31`, drawn after everything else.

struct Cover {
    // rgb: colour, a: alpha.
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> cover: Cover;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return cover.color;
}
