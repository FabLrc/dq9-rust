//! Invariants checked against the real game image; skipped when the ROM is absent.

use nds_rom::{Narc, Rom};

#[test]
fn real_rom_invariants() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    assert!(rom.header.crc_ok());

    // BLZ correctness: the autoload list sits right after the autoload payload, which only
    // lines up if the ARM9 decompressed to exactly the right bytes.
    let arm9 = rom.arm9().unwrap();
    let autoloads = arm9.autoloads().unwrap();
    assert!(!autoloads.is_empty());
    let payload_end: u32 =
        arm9.params.autoload_start + autoloads.iter().map(|a| a.size).sum::<u32>();
    assert_eq!(payload_end, arm9.params.autoload_list_start);

    // Every overlay decompresses to exactly its declared RAM size.
    for ov in rom.overlays().unwrap() {
        let data = rom.overlay_data(&ov).unwrap();
        assert_eq!(data.len(), ov.ram_size as usize, "overlay {}", ov.id);
    }

    // Every named file is in bounds, and every NARC parses with in-bounds members.
    for f in &rom.files.files {
        let data = rom.file(f.id).unwrap();
        if Narc::is_narc(data) {
            let narc = Narc::parse(data).unwrap_or_else(|e| panic!("{}: {e}", f.path));
            for i in 0..narc.len() as u16 {
                narc.file(i)
                    .unwrap_or_else(|e| panic!("{} member {i}: {e}", f.path));
            }
        }
    }
}

/// Calls `f` with every `.nsbmd`/`.nsbtx`/`.nsbtp` member of the map archives (`data/map/*.ambl`,
/// `*.amdj`), decompressed.
fn for_each_map_g3d(rom: &Rom, mut f: impl FnMut(&str, &nds_rom::g3d::G3dFile)) {
    use nds_rom::compress;
    use nds_rom::g3d::G3dFile;

    for file in &rom.files.files {
        let p = &file.path;
        if !(p.starts_with("data/map/") && (p.ends_with(".ambl") || p.ends_with(".amdj"))) {
            continue;
        }
        let narc = Narc::parse(rom.file(file.id).unwrap()).unwrap();
        for i in 0..narc.len() as u16 {
            let name = narc.name(i).unwrap_or_default();
            if ![".nsbmd", ".nsbtx", ".nsbtp"]
                .iter()
                .any(|e| name.ends_with(e))
            {
                continue;
            }
            let ctx = format!("{p} {name}");
            let raw = narc.file(i).unwrap();
            let data = if G3dFile::is_g3d(raw) {
                raw.to_vec()
            } else {
                compress::decompress(raw).unwrap_or_else(|e| panic!("{ctx}: {e}"))
            };
            let g3d = G3dFile::parse(&data).unwrap_or_else(|e| panic!("{ctx}: {e}"));
            f(&ctx, &g3d);
        }
    }
}

/// Every G3D model and texture file of the map archives parses, every texture decodes with
/// its palette and every model's display lists run into triangles.
#[test]
fn map_g3d_files() {
    use nds_rom::g3d::tex::decode_texture;
    use nds_rom::g3d::{FileKind, Fx12, Geometry, Name, RenderOp, TexFormat};
    use std::collections::{BTreeMap, BTreeSet};

    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();

    // Pass 1: models, and the texture -> palette pairs their materials and pattern
    // animations declare.
    let (mut bmd, mut btx, mut btp, mut patterns) = (0, 0, 0, 0);
    let (mut models, mut meshes, mut materials) = (0, 0, 0);
    let (mut triangles, mut vertices) = (0, 0);
    let mut outside_bbox = BTreeSet::new();
    let mut pairs: BTreeMap<Name, BTreeSet<Name>> = BTreeMap::new();
    for_each_map_g3d(&rom, |ctx, g3d| {
        match g3d.kind {
            FileKind::Model => bmd += 1,
            FileKind::Texture => btx += 1,
            FileKind::TexPatternAnim => btp += 1,
            other => panic!("{ctx}: {other:?}"),
        }
        // Map models never embed their textures.
        assert!(g3d.kind != FileKind::Model || (g3d.models.len() == 1 && g3d.tex.is_none()));
        for m in &g3d.models {
            // posScale is applied by the ScaleUp render command, present iff it is not 1.
            let scales = m.render_ops.contains(&RenderOp::ScaleUp);
            assert_eq!(scales, m.up_scale != Fx12::ONE, "{ctx}");
            let g = Geometry::build(m).unwrap_or_else(|e| panic!("{ctx} {}: {e}", m.name));
            models += 1;
            meshes += m.meshes.len();
            materials += m.materials.len();
            triangles += g.indices.len() / 3;
            vertices += g.vertices.len();
            // The header box (scaled by box_pos_scale) bounds the rest-pose vertices, up to
            // its fx16 rounding and our f32 matrix products.
            let s = m.box_pos_scale.to_f32();
            let eps = s * 2.0 / 4096.0 + 0.05;
            let lo = m.bbox_origin.map(|v| v.to_f32() * s - eps);
            let hi: [f32; 3] =
                std::array::from_fn(|i| lo[i] + m.bbox_size[i].to_f32() * s + 2.0 * eps);
            let inside = |p: [f32; 3]| (0..3).all(|i| p[i] >= lo[i] && p[i] <= hi[i]);
            if !g.vertices.iter().all(|v| inside(v.position)) {
                outside_bbox.insert(m.name.to_string());
            }
            for mat in &m.materials {
                if let (Some(t), Some(p)) = (mat.texture_name, mat.palette_name) {
                    pairs.entry(t).or_default().insert(p);
                }
            }
        }
        for pat in &g3d.patterns {
            patterns += 1;
            for key in pat.tracks.iter().flat_map(|t| &t.keyframes) {
                let t = pat.texture_names[usize::from(key.texture_idx)];
                if let Some(p) = key.palette() {
                    pairs
                        .entry(t)
                        .or_default()
                        .insert(pat.palette_names[usize::from(p)]);
                }
            }
        }
    });

    // Pass 2: textures, with the palette named `<texture>_pl` or one paired by a material.
    let (mut textures, mut palettes, mut texels) = (0, 0, 0);
    let mut unresolved = Vec::new();
    let mut formats: BTreeMap<String, usize> = BTreeMap::new();
    for_each_map_g3d(&rom, |ctx, g3d| {
        let Some(tex0) = &g3d.tex else { return };
        palettes += tex0.palettes.len();
        for t in &tex0.textures {
            let format = t.params.format();
            *formats.entry(format!("{format:?}")).or_default() += 1;
            let pal = if format.needs_palette() {
                let mut pl = t.name.bytes().to_vec();
                pl.extend(b"_pl");
                let pal = tex0
                    .palettes
                    .iter()
                    .find(|p| p.name.bytes() == pl)
                    .or_else(|| {
                        let paired = pairs.get(&t.name)?;
                        tex0.palettes.iter().find(|p| paired.contains(&p.name))
                    });
                if pal.is_none() {
                    unresolved.push(t.name.to_string());
                }
                // Unpaired textures still decode, with an arbitrary palette.
                pal.or(tex0.palettes.first())
            } else {
                None
            };
            let rgba = decode_texture(t, pal).unwrap_or_else(|e| panic!("{ctx}: {e}"));
            assert_eq!(rgba.len() as u32, t.params.width() * t.params.height() * 4);
            assert_ne!(format, TexFormat::None);
            textures += 1;
            texels += rgba.len() / 4;
        }
    });

    assert_eq!((bmd, btx, btp, patterns), (4328, 737, 525, 525));
    assert_eq!((models, meshes, materials), (4328, 36632, 31297));
    assert_eq!((vertices, triangles), (1365561, 748435));
    assert_eq!((textures, palettes, texels), (22827, 22822, 69462528));
    let formats: Vec<_> = formats.into_iter().collect();
    let expected = [("A3I5", 3498), ("A5I3", 3445), ("Direct", 5)]
        .into_iter()
        .chain([("Palette16", 15532), ("Palette256", 331), ("Palette4", 16)]);
    assert_eq!(
        formats,
        expected.map(|(f, n)| (f.to_owned(), n)).collect::<Vec<_>>()
    );
    // Only one texture is neither `<name>_pl` nor paired by a material or pattern animation.
    assert_eq!(unresolved, ["c04m01gds28"]);
    // Every model fits its header box once the Softimage scaling rule is applied (M10M0600
    // and S14M01E1 nest scaled objects; a plain `T R S` puts them outside).
    assert!(outside_bbox.is_empty(), "{outside_bbox:?}");
}

/// Every animation file (`.nsbca`, `.nsbta`, `.nsbma`, `.nsbtp`) of the map archives and of the
/// character archives (`*.chr`) decodes, and every frame of every track evaluates; the joint
/// animations also pose the models of their own archive.
#[test]
fn animation_files() {
    use nds_rom::compress;
    use nds_rom::g3d::{Fx12, G3dFile, Geometry};
    use std::collections::BTreeMap;

    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut files: BTreeMap<String, usize> = BTreeMap::new();
    let (mut joints, mut srts, mut mats, mut pats) = (0, 0, 0, 0);
    let (mut tracks, mut evaluations, mut smooth, mut remapped) = (0, 0, 0, 0);
    let mut rules: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    let (mut posed, mut posed_vertices) = (0, 0);
    for file in &rom.files.files {
        let p = &file.path;
        let map = p.starts_with("data/map/") && (p.ends_with(".ambl") || p.ends_with(".amdj"));
        if !(map || p.ends_with(".chr")) {
            continue;
        }
        let narc = Narc::parse(rom.file(file.id).unwrap()).unwrap();
        let mut members = Vec::new();
        for i in 0..narc.len() as u16 {
            let name = narc.name(i).unwrap_or_default().to_owned();
            let Some(ext) = [".nsbca", ".nsbta", ".nsbma", ".nsbtp", ".nsbmd"]
                .into_iter()
                .find(|e| name.ends_with(e))
            else {
                continue;
            };
            let raw = narc.file(i).unwrap();
            let data = if G3dFile::is_g3d(raw) {
                raw.to_vec()
            } else {
                compress::decompress(raw).unwrap_or_else(|e| panic!("{p} {name}: {e}"))
            };
            if ext != ".nsbmd" {
                let kind = if map { "map" } else { "chr" };
                *files.entry(format!("{kind} {ext}")).or_default() += 1;
            }
            members.push((name, data));
        }
        let parsed: Vec<_> = members
            .iter()
            .map(|(name, data)| {
                let g = G3dFile::parse(data).unwrap_or_else(|e| panic!("{p} {name}: {e}"));
                (name, g)
            })
            .collect();
        let models: Vec<_> = parsed.iter().flat_map(|(_, g)| &g.models).collect();
        for (name, g) in &parsed {
            let ctx = format!("{p} {name}");
            for a in &g.joint_animations {
                joints += 1;
                tracks += a.tracks.len();
                for (i, t) in a.tracks.iter().enumerate() {
                    remapped += usize::from(t.data_index() != i);
                    for f in 0..i32::from(a.frame_count) {
                        let mut times = vec![f << 12];
                        if a.interpolates() {
                            times.push((f << 12) | 0x800);
                            smooth += 1;
                        }
                        for time in times {
                            a.track_transform(i, &Default::default(), Fx12(time))
                                .unwrap_or_else(|e| panic!("{ctx} track {i} t {time:#x}: {e}"));
                            evaluations += 1;
                        }
                    }
                }
                for m in &models {
                    if a.tracks.len() > m.objects.len() {
                        continue;
                    }
                    *rules.entry((m.scaling_rule, m.tex_mtx_mode)).or_default() += 1;
                    let pose = a.pose(m, a.frame_count / 2).unwrap();
                    let geo = Geometry::build_posed(m, &pose)
                        .unwrap_or_else(|e| panic!("{ctx} on {}: {e}", m.name));
                    assert!(
                        geo.vertices
                            .iter()
                            .all(|v| v.position.iter().all(|x| x.is_finite()))
                    );
                    posed += 1;
                    posed_vertices += geo.vertices.len();
                }
            }
            for a in &g.tex_srt_animations {
                srts += 1;
                tracks += a.tracks.len();
                for i in 0..a.tracks.len() {
                    for f in 0..a.frame_count {
                        a.srt(i, f)
                            .unwrap_or_else(|e| panic!("{ctx} track {i} frame {f}: {e}"));
                        evaluations += 1;
                    }
                }
                for m in &models {
                    for mi in 0..m.materials.len() {
                        a.matrix(m, mi, 0).unwrap();
                    }
                }
            }
            for a in &g.material_animations {
                mats += 1;
                tracks += a.tracks.len();
                for i in 0..a.tracks.len() {
                    for f in 0..a.frame_count {
                        a.colors(i, f)
                            .unwrap_or_else(|e| panic!("{ctx} track {i} frame {f}: {e}"));
                        evaluations += 1;
                    }
                }
            }
            for a in &g.patterns {
                pats += 1;
                tracks += a.tracks.len();
                for i in 0..a.tracks.len() {
                    for f in 0..a.num_frames {
                        assert!(a.at(i, f).is_some(), "{ctx} track {i} frame {f}");
                        evaluations += 1;
                    }
                }
            }
        }
    }
    let files: Vec<_> = files.into_iter().collect();
    let expected = [
        ("chr .nsbca", 5347),
        ("chr .nsbma", 1594),
        ("chr .nsbta", 692),
    ]
    .into_iter()
    .chain([("chr .nsbtp", 45), ("map .nsbca", 299), ("map .nsbma", 512)])
    .chain([("map .nsbta", 873), ("map .nsbtp", 525)]);
    assert_eq!(
        files,
        expected.map(|(f, n)| (f.to_owned(), n)).collect::<Vec<_>>()
    );
    // One animation per file.
    assert_eq!((joints, srts, mats, pats), (5646, 1565, 2106, 570));
    assert_eq!((tracks, evaluations, smooth), (98684, 6732887, 1671958));
    // Track `i` always drives object `i`.
    assert_eq!(remapped, 0);
    // Joint animations posing a model of their own archive (`.chr` with model and motions),
    // by (scaling rule, texture matrix mode): Softimage scaling, XSI texture matrices.
    assert_eq!((posed, posed_vertices), (2965, 891520));
    assert_eq!(rules, BTreeMap::from([((0, 3), 10), ((2, 3), 2955)]));
}
