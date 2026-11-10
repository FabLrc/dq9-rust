//! The WGSL sources are part of the renderer's contract: parse and validate them without a GPU
//! so a shader typo fails the normal test run (CI has no adapter).

const SHADERS: [(&str, &str); 7] = [
    ("shader", include_str!("../src/shader.wgsl")),
    ("edge", include_str!("../src/edge.wgsl")),
    ("background", include_str!("../src/background.wgsl")),
    ("cover", include_str!("../src/cover.wgsl")),
    ("panel", include_str!("../src/panel.wgsl")),
    ("ui", include_str!("../src/ui.wgsl")),
    ("post", include_str!("../src/post.wgsl")),
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
