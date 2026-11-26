//! The WGSL sources are part of the renderer's contract: parse and validate them without a GPU
//! so a shader typo fails the normal test run (CI has no adapter).

const SHADERS: [(&str, &str); 8] = [
    ("shader", include_str!("../src/shader.wgsl")),
    ("edge", include_str!("../src/edge.wgsl")),
    ("background", include_str!("../src/background.wgsl")),
    ("cover", include_str!("../src/cover.wgsl")),
    ("panel", include_str!("../src/panel.wgsl")),
    ("ui", include_str!("../src/ui.wgsl")),
    ("post", include_str!("../src/post.wgsl")),
    ("map_screen", include_str!("../src/map_screen/shader.wgsl")),
];

#[test]
fn every_shader_parses_and_validates() {
    for (name, source) in SHADERS {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{name}.wgsl: {}", e.emit_to_string(source)));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}.wgsl: {e:?}"));
    }
}

/// The post pass switches its effects by bits of `post.features.x`: the shader's constants and
/// the Rust mask (`EnhancedOptions::post_mask`) are two representations of the same contract,
/// so a bit moved on one side must fail here.
#[test]
fn the_post_shader_reads_the_option_mask_bits() {
    let source = include_str!("../src/post.wgsl");
    for (name, bit) in dq9_render::PostBits::WGSL_NAMES {
        let declaration = format!("const {name}: u32 = {}u;", bit.0);
        assert!(
            source.contains(&declaration),
            "post.wgsl does not declare `{declaration}`"
        );
    }
    assert!(source.contains("post.features.x"), "the mask uniform");
}

/// The map screen selects its texture by a divergent layer condition. WGSL forbids
/// `textureSample` in non-uniform control flow; the browser's validator (Dawn/Tint) rejects the
/// module, wgpu drops the pass and the whole canvas stays black, while naga on the native side
/// accepts it (`v3-web-screens`: `textureSample must only be called from uniform control flow`).
/// naga's validator does not catch it, so the source rule is pinned here: explicit-LOD sampling
/// (`textureSampleLevel`) is the allowed form in divergent flow.
#[test]
fn the_map_screen_shader_samples_without_uniform_control_flow_requirements() {
    let source = include_str!("../src/map_screen/shader.wgsl");
    assert!(
        !source.contains("textureSample("),
        "map_screen/shader.wgsl uses textureSample in divergent flow; use textureSampleLevel"
    );
    assert!(
        source.contains("textureSampleLevel("),
        "map_screen/shader.wgsl no longer samples with an explicit LOD"
    );
}
