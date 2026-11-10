// Cinematic post-processing over the finished field: FXAA-lite, screen-space contact shadows
// read from the depth buffer, a threshold bloom, filmic tonemapping, colour grading, vignette
// and dithering. The DS renderer keeps its own look; this pass is only used by the wide 16:9
// presentation (`Renderer::new_with(..., true)`, `docs/app.md`).

struct Post {
    // x, y: scene size in pixels; z, w: their inverses.
    size: vec4<f32>,
    // x, y: the scene's origin in the target; z, w: unused.
    origin: vec4<f32>,
    // x: exposure, y: bloom, z: tonemap mix, w: vignette.
    params: vec4<f32>,
    // x: saturation, y: contrast, z: shadow tint, w: highlight warmth.
    grade: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> post: Post;

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// FXAA's edge direction, kept to the five-tap form (enough at 1080p on the soft DS textures).
fn antialias(uv: vec2<f32>, c: vec3<f32>) -> vec3<f32> {
    let texel = post.size.zw;
    let north = luma(textureSampleLevel(scene, samp, uv + vec2<f32>(0.0, -texel.y), 0.0).rgb);
    let south = luma(textureSampleLevel(scene, samp, uv + vec2<f32>(0.0, texel.y), 0.0).rgb);
    let east = luma(textureSampleLevel(scene, samp, uv + vec2<f32>(texel.x, 0.0), 0.0).rgb);
    let west = luma(textureSampleLevel(scene, samp, uv + vec2<f32>(-texel.x, 0.0), 0.0).rgb);
    let center = luma(c);
    let dir = vec2<f32>(north - south, east - west);
    let reduce = max((north + south + east + west) * 0.03125, 1.0 / 128.0);
    let rcp = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    let step = clamp(dir * rcp, vec2<f32>(-8.0), vec2<f32>(8.0)) * texel;
    let a = textureSampleLevel(scene, samp, uv + step * (1.0 / 3.0 - 0.5), 0.0).rgb;
    let b = textureSampleLevel(scene, samp, uv + step * (2.0 / 3.0 - 0.5), 0.0).rgb;
    let filtered = (a + b) * 0.25 + c * 0.5;
    // Keep flat areas untouched: filtering is visible only where the four neighbours differ.
    let edges = max(max(north, south), max(east, west)) - min(min(north, south), min(east, west));
    return mix(c, filtered, smoothstep(0.02, 0.18, edges));
}

// Screen-space contact shadows: a nearer neighbour in the depth buffer darkens the pixel.
// Not a full ambient-occlusion solution, but it gives models and foliage their weight.
fn occlusion(p: vec2<i32>, near: f32) -> f32 {
    let maxp = vec2<i32>(post.size.xy) - 1;
    let offsets = array<vec2<i32>, 8>(
        vec2<i32>(4, 0), vec2<i32>(-4, 0), vec2<i32>(0, 4), vec2<i32>(0, -4),
        vec2<i32>(3, 3), vec2<i32>(-3, 3), vec2<i32>(3, -3), vec2<i32>(-3, -3),
    );
    var hits = 0.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
        let q = clamp(p + offsets[i], vec2<i32>(0), maxp);
        let d = textureLoad(depth, q, 0);
        if d < near - 0.0008 {
            hits = hits + 1.0;
        }
    }
    return hits / 8.0;
}

// Threshold bloom: eight taps on two rings, enough to bleed highlights without a mip chain.
fn bloom(uv: vec2<f32>) -> vec3<f32> {
    let texel = post.size.zw;
    let offsets = array<vec2<f32>, 8>(
        vec2<f32>(1.5, 0.0), vec2<f32>(-1.5, 0.0), vec2<f32>(0.0, 1.5), vec2<f32>(0.0, -1.5),
        vec2<f32>(3.5, 2.0), vec2<f32>(-3.5, 2.0), vec2<f32>(3.5, -2.0), vec2<f32>(-3.5, -2.0),
    );
    let weights = array<f32, 8>(1.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 0.5);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
        let s = textureSampleLevel(scene, samp, uv + offsets[i] * texel, 0.0).rgb;
        sum = sum + max(s - vec3<f32>(0.72), vec3<f32>(0.0)) * weights[i];
        total = total + weights[i];
    }
    return sum / total;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let local = pos.xy - post.origin.xy;
    let uv = local * post.size.zw;
    let p = vec2<i32>(local);
    var c = textureSampleLevel(scene, samp, uv, 0.0).rgb;
    c = antialias(uv, c);

    // Contact shadows keep the interface and the sky clean: only pixels the depth buffer wrote.
    let d = textureLoad(depth, p, 0);
    if d < 1.0 {
        let ao = occlusion(p, d);
        c = c * (1.0 - ao * 0.32);
    }

    c = c + bloom(uv) * post.params.y;

    // Filmic curve (Narkowicz's ACES approximation), mixed with the DS colours rather than
    // replacing them: the result must still read as Dragon Quest IX.
    let exposed = c * post.params.x;
    let mapped = clamp(
        (exposed * (2.51 * exposed + 0.03)) / (exposed * (2.43 * exposed + 0.59) + 0.14),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    c = mix(c, mapped, post.params.z);

    // Colour grade: cool shadows, warm highlights, a little saturation and contrast.
    let l = luma(c);
    c = c + vec3<f32>(0.72, 0.86, 1.0) * (1.0 - smoothstep(0.0, 0.55, l)) * post.grade.z;
    c = c + vec3<f32>(1.0, 0.92, 0.78) * smoothstep(0.55, 1.0, l) * post.grade.w;
    c = mix(vec3<f32>(luma(c)), c, post.grade.x);
    c = (c - vec3<f32>(0.5)) * post.grade.y + vec3<f32>(0.5);

    // Vignette, then dither to hide the 8-bit steps on the broad sky gradients.
    let aspect = vec2<f32>(post.size.x * post.size.w, 1.0);
    let radius = length((uv - vec2<f32>(0.5)) * aspect);
    c = c * (1.0 - smoothstep(0.55, 1.05, radius) * post.params.w);
    c = c + (hash(pos.xy) - 0.5) / 255.0;
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
