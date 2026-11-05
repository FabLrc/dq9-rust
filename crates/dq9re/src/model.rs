//! `dq9re model`: export the textures (PNG) and models (Wavefront OBJ) of G3D files.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result, bail};
use nds_rom::g3d::tex::decode_texture;
use nds_rom::g3d::{G3dFile, Geometry, Model, Name, Palette, Tex0, Texture};
use nds_rom::{Narc, Rom, compress};

/// Decompresses `raw` when it is not a G3D file but a Nintendo compressed stream.
pub(crate) fn unpack(raw: &[u8]) -> Result<Cow<'_, [u8]>> {
    if G3dFile::is_g3d(raw) {
        return Ok(Cow::Borrowed(raw));
    }
    Ok(Cow::Owned(compress::decompress(raw)?))
}

/// Picks the palette of a texture: the one paired with it by a material, else `<name>_pl`,
/// else the only palette.
pub(crate) fn palette_for<'t, 'a>(
    tex0: &'t Tex0<'a>,
    texture: &Texture,
    models: &[Model],
) -> Option<&'t Palette<'a>> {
    if !texture.params.format().needs_palette() {
        return None;
    }
    let paired = models
        .iter()
        .flat_map(|m| &m.materials)
        .filter(|m| m.texture_name == Some(texture.name))
        .find_map(|m| m.palette_name.and_then(|p| tex0.palette(&p)));
    paired
        .or_else(|| {
            let mut pl = texture.name.bytes().to_vec();
            pl.extend(b"_pl");
            tex0.palettes.iter().find(|p| p.name.bytes() == pl)
        })
        .or(match tex0.palettes.as_slice() {
            [only] => Some(only),
            _ => None,
        })
}

fn file_name(name: &Name) -> String {
    name.to_string()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn run(rom: &Rom, path: &str, member: Option<&str>, out: &Path) -> Result<()> {
    let data = rom.file_by_path(path)?;
    let mut files: Vec<(String, Cow<[u8]>)> = Vec::new();
    if Narc::is_narc(data) {
        let narc = Narc::parse(data).with_context(|| format!("{path} as NARC"))?;
        for i in 0..u16::try_from(narc.len())? {
            let name = narc
                .name(i)
                .map_or_else(|| format!("{i:04}"), str::to_owned);
            if member.is_some_and(|m| m != name)
                || !(name.ends_with(".nsbmd") || name.ends_with(".nsbtx"))
            {
                continue;
            }
            let raw = narc.file(i)?;
            files.push((name, unpack(raw)?));
        }
    } else {
        files.push((
            path.rsplit('/').next().unwrap_or(path).to_owned(),
            unpack(data)?,
        ));
    }
    if files.is_empty() {
        bail!("no .nsbmd/.nsbtx member selected in {path}");
    }
    fs::create_dir_all(out)?;
    for (name, bytes) in &files {
        let g3d = match G3dFile::parse(bytes) {
            Ok(g) => g,
            Err(e) => {
                println!("{name}: not parsed ({e})");
                continue;
            }
        };
        println!("{name}: {:?}", g3d.kind);
        if let Some(tex0) = &g3d.tex {
            export_textures(tex0, &g3d.models, out)?;
        }
        for model in &g3d.models {
            export_model(model, out)?;
        }
    }
    Ok(())
}

fn export_textures(tex0: &Tex0, models: &[Model], out: &Path) -> Result<()> {
    for t in &tex0.textures {
        let pal = palette_for(tex0, t, models);
        let (w, h) = (t.params.width(), t.params.height());
        println!(
            "  texture {:16} {w:4}x{h:<4} {:?} palette {}",
            t.name.to_string(),
            t.params.format(),
            pal.map_or("-".to_owned(), |p| p.name.to_string())
        );
        let rgba = match decode_texture(t, pal) {
            Ok(rgba) => rgba,
            Err(e) => {
                println!("    not decoded: {e}");
                continue;
            }
        };
        let dest = out.join(format!("{}.png", file_name(&t.name)));
        let file = File::create(&dest).with_context(|| format!("creating {}", dest.display()))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgba)?;
    }
    for p in &tex0.palettes {
        println!("  palette {:16} @{:#06x}", p.name.to_string(), p.offset);
    }
    Ok(())
}

fn export_model(model: &Model, out: &Path) -> Result<()> {
    let geometry = Geometry::build(model).with_context(|| format!("model {}", model.name))?;
    let base = file_name(&model.name);
    let mut obj = format!("# {} (dq9re model)\nmtllib {base}.mtl\n", model.name);
    let mut mtl = String::new();
    for m in &model.materials {
        let d = m.diffuse();
        writeln!(
            mtl,
            "newmtl {}\nKd {} {} {}",
            file_name(&m.name),
            d[0],
            d[1],
            d[2]
        )?;
        writeln!(mtl, "d {}", f32::from(m.alpha()) / 31.0)?;
        if let Some(t) = m.texture_name {
            writeln!(mtl, "map_Kd {}.png", file_name(&t))?;
        }
    }
    let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in &geometry.vertices {
        let p = v.position;
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
        writeln!(
            obj,
            "v {} {} {} {} {} {}",
            p[0], p[1], p[2], v.color[0], v.color[1], v.color[2]
        )?;
        writeln!(obj, "vt {} {}", v.texcoord[0], 1.0 - v.texcoord[1])?;
    }
    for call in &geometry.draw_calls {
        let mat = &model.materials[usize::from(call.material_idx)];
        let mesh = &model.meshes[usize::from(call.mesh_idx)];
        writeln!(
            obj,
            "o {}\nusemtl {}",
            file_name(&mesh.name),
            file_name(&mat.name)
        )?;
        for &[a, b, c] in geometry.indices[call.index_range.clone()]
            .as_chunks::<3>()
            .0
        {
            let [a, b, c] = [a + 1, b + 1, c + 1];
            writeln!(obj, "f {a}/{a} {b}/{b} {c}/{c}")?;
        }
    }
    fs::write(out.join(format!("{base}.obj")), obj)?;
    fs::write(out.join(format!("{base}.mtl")), mtl)?;
    println!(
        "  model {}: {} objects, {} materials, {} meshes, up_scale {}, {} vertices, {} \
         triangles, bbox {:?}..{:?}",
        model.name,
        model.objects.len(),
        model.materials.len(),
        model.meshes.len(),
        model.up_scale.to_f32(),
        geometry.vertices.len(),
        geometry.indices.len() / 3,
        min,
        max
    );
    Ok(())
}
