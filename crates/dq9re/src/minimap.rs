//! `dq9re minimap`: the field's map screen data. `minimap.gp2`'s `.bmmp` layouts and `.obg`
//! pictures, the `obj_minimap` cells the party strip and the dots are drawn with, and the
//! position formula `pixel = position × scale − corner × 8`.

use std::borrow::Cow;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use dq9_formats::Gpc2;
use dq9_formats::minimap::{self, Layout, Pac, Picture};
use nds_rom::Rom;
use nds_rom::nitro2d::Ncer;

/// The archive the field's map screen loads (mode 0, `MapScreen_Load`'s `func_02020b98`).
pub const ARCHIVE: &str = "data/pack_lv5/minimap.gp2";
/// The loose Nitro 2D files the party strip and the dots come from.
const SPRITE_CELLS: &str = "data/ani/obj_minimap.NCER";
/// The sprite pack `obj_mm.pac`, which holds the same three files.
const SPRITE_PACK: &str = "data/ani/obj_mm.pac";

#[derive(clap::Args)]
pub struct MinimapArgs {
    /// Map code (`M01`) or a code the layouts list (`M01M01`); prints that layout.
    map: Option<String>,
    /// Write the layout's picture there as a PNG.
    #[arg(long, requires = "map")]
    out: Option<PathBuf>,
}

pub fn run(rom: &Rom<'_>, args: &MinimapArgs) -> Result<()> {
    let archive =
        Gpc2::parse(rom.file_by_path(ARCHIVE)?).with_context(|| format!("parsing {ARCHIVE}"))?;
    match &args.map {
        None => counts(rom, &archive),
        Some(map) => one(rom, &archive, map, args.out.as_deref()),
    }
}

fn members<'a, 'b>(archive: &'b Gpc2<'a>, suffix: &str) -> Vec<&'b dq9_formats::gpc2::Entry> {
    archive
        .entries
        .iter()
        .filter(|e| e.name.ends_with(suffix))
        .collect()
}

/// The counts over the cartridge's map screen data, every file parsed.
fn counts(rom: &Rom<'_>, archive: &Gpc2<'_>) -> Result<()> {
    let layouts = members(archive, ".bmmp");
    let pictures = members(archive, ".obg");
    let mut named = std::collections::BTreeSet::new();
    for entry in &layouts {
        let data = archive.member(entry)?;
        let layout = Layout::parse(&data)?;
        named.extend(layout.picture().map(str::to_owned));
    }
    let mut tiles = 0;
    let mut cells = 0;
    for entry in &pictures {
        let data = archive.member(entry)?;
        let picture = Picture::parse(&data)?;
        tiles += usize::from(picture.tile_count);
        cells += picture.cells.len();
    }
    println!("{ARCHIVE}: {} entries", archive.entries.len());
    println!("  layouts (`.bmmp`): {}", layouts.len());
    println!(
        "  pictures (`.obg`): {} ({} tiles, {cells} cells), {} named by a layout",
        pictures.len(),
        tiles,
        named.len()
    );

    let ncer = Ncer::parse(rom.file_by_path(SPRITE_CELLS)?)?;
    let strip = minimap::party_strip(&ncer.cells);
    print!("  {SPRITE_CELLS}: {} cells", ncer.cells.len());
    match strip {
        Some(strip) => println!(
            ", party strip (cell {}) {}×{} at ({}, {})",
            minimap::PARTY_PANEL_CELL,
            strip.width,
            strip.height,
            strip.x,
            strip.y
        ),
        None => println!(", no party strip"),
    }
    let dots = minimap::DOT_CELLS;
    println!(
        "    dots: cells {}..={}, one 8×8 part each",
        dots.start(),
        dots.end()
    );
    let pack = Pac::parse(rom.file_by_path(SPRITE_PACK)?)?;
    println!("  {SPRITE_PACK}: {} members", pack.members.len());
    for name in pack.names() {
        println!("    {name}");
    }
    Ok(())
}

/// Finds the layout of a map: `<map>.bmmp`, else the layout listing `map` among its codes.
fn find_layout<'a>(archive: &'a Gpc2<'a>, map: &str) -> Result<(String, Cow<'a, [u8]>)> {
    let direct = format!("{map}.bmmp");
    if let Some(entry) = archive.find(&direct) {
        return Ok((entry.name.clone(), archive.member(entry)?));
    }
    for entry in members(archive, ".bmmp") {
        let data = archive.member(entry)?;
        let layout = Layout::parse(&data)?;
        if layout
            .codes()
            .iter()
            .any(|(_, code)| code.eq_ignore_ascii_case(map))
        {
            return Ok((entry.name.clone(), data));
        }
    }
    bail!("no `.bmmp` layout for {map}")
}

/// One layout: its picture, scale, corner, maps and marks, and the party strip.
fn one(rom: &Rom<'_>, archive: &Gpc2<'_>, map: &str, out: Option<&Path>) -> Result<()> {
    let (name, data) = find_layout(archive, map)?;
    let layout = Layout::parse(&data)?;
    let picture = layout.picture().context("layout without a picture")?;
    let scale = layout.scale().context("layout without a scale")?;
    let (corner_x, corner_z) = layout.corner().context("layout without a corner")?;
    println!("{name}: picture {picture}");
    println!("  scale {scale} pixels per unit, corner ({corner_x}, {corner_z}) tiles");
    if let Some(backdrop) = layout.backdrop() {
        println!("  backdrop {backdrop}");
    }
    let codes: std::collections::BTreeMap<u32, &str> = layout.codes().into_iter().collect();
    let code = |id: u32| match codes.get(&id) {
        Some(code) => format!("{id} {code}"),
        None => format!("{id}"),
    };
    let maps: Vec<String> = layout.maps().into_iter().map(code).collect();
    println!("  maps: {}", maps.join(", "));
    let marks = layout.marks();
    println!("  marks: {}", marks.len());
    let picture_entry = archive
        .find(picture)
        .or_else(|| archive.find(&format!("{picture}.obg")))
        .ok_or_else(|| anyhow::anyhow!("no {picture} in {ARCHIVE}"))?;
    let picture_data = archive.member(picture_entry)?;
    let picture = Picture::parse(&picture_data)?;
    let image = picture.image();
    println!(
        "  picture {}×{} pixels ({}×{} cells)",
        image.width, image.height, picture.width, picture.height
    );
    for mark in &marks {
        let (x, z) = layout.pixel(mark.x, mark.z).unwrap_or((f32::NAN, f32::NAN));
        let maps: Vec<String> = mark.maps.iter().map(|&id| code(id)).collect();
        println!(
            "    ({:.2}, {:.2}) → pixel ({x:.1}, {z:.1}) for {}",
            mark.x,
            mark.z,
            maps.join(", ")
        );
    }
    let ncer = Ncer::parse(rom.file_by_path(SPRITE_CELLS)?)?;
    let strip = minimap::party_strip(&ncer.cells).context("no party strip")?;
    println!(
        "  party strip: cell {} of {SPRITE_CELLS}, {}×{} at ({}, {})",
        minimap::PARTY_PANEL_CELL,
        strip.width,
        strip.height,
        strip.x,
        strip.y
    );
    if let Some(out) = out {
        write_png(&image, out)?;
        let clear = image.rgba.iter().filter(|p| p[3] == 0).count();
        println!(
            "wrote {} ({}×{}, {clear} clear pixels)",
            out.display(),
            image.width,
            image.height
        );
    }
    Ok(())
}

fn write_png(image: &minimap::Image, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let file = fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(file),
        image.width as u32,
        image.height as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    let mut bytes = Vec::with_capacity(image.rgba.len() * 4);
    for pixel in &image.rgba {
        bytes.extend_from_slice(pixel);
    }
    writer.write_image_data(&bytes)?;
    Ok(())
}
