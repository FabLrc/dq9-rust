//! `dq9re map`: the files a map loads and how the game assembles them, optionally checked
//! against the map loaded in a melonDS state, drawn from above, and flooded for reachability.

mod image;
mod reach;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use dq9_core::collision::CollisionWorld;
use dq9_formats::Table;
use dq9_formats::map::{self, Context, Map, PartKind};
use dq9_formats::maplist::{self, MapList, MapRecord};
use dq9_formats::melonds::{self, State};
use nds_rom::Rom;

/// `s_lightingManager` (`LightingManager::GetInstance`, `0x02050464`); the time of day is at
/// `+0x98`.
const LIGHTING_MANAGER: u32 = 0x0210_7930;

#[derive(clap::Args)]
pub struct MapArgs {
    /// Map code (`C01M09`) or id (`109`); defaults to the current map of `--state`.
    map: Option<String>,
    /// Time of day (`LightingManager+0x98`, 0-3); defaults to the state's, else 0.
    #[arg(long)]
    time: Option<u32>,
    /// Active grotto environment, for grotto maps.
    #[arg(long, default_value_t = 1)]
    grotto_environ: i32,
    /// melonDS state whose loaded map (`g_Zone3D+0x41c`) is compared with the assembly.
    #[arg(long)]
    state: Option<std::path::PathBuf>,
    /// Renders the assembled collision from above: floors by height, walls dark, warp and
    /// zone outlines with their destination.
    #[arg(long)]
    image: Option<PathBuf>,
    /// Pixels per world unit of `--image` and `--reach-image` (default 8).
    #[arg(long, default_value_t = 8)]
    scale: u32,
    /// Hero marker on the image, world units: `--at X,Z` is stood on the highest floor,
    /// `--at X,Y,Z` within one unit above a floor. Refused when no floor is found.
    #[arg(long, value_name = "X[,Y],Z")]
    at: Option<String>,
    /// Flood fill from `X,Z` (highest floor) or `X,Y,Z`, with the real hero step and 3D
    /// arrival; one line of statistics.
    #[arg(long, value_name = "X[,Y],Z")]
    reach: Option<String>,
    /// Target of `--reach`: the report says whether a hero reaches it.
    #[arg(long, value_name = "X[,Y],Z")]
    to: Option<String>,
    /// Writes the same view with the cells of `--reach` tinted green; needs `--reach`.
    #[arg(long)]
    reach_image: Option<PathBuf>,
}

fn text(s: &[u8]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

fn fx(v: i32) -> String {
    format!("{:.3}", f64::from(v) / 4096.0)
}

fn find<'a>(list: &'a MapList<'a>, key: &str) -> Result<&'a MapRecord<'a>> {
    let found = match key.parse::<u16>() {
        Ok(id) => list.find(id),
        Err(_) => list
            .records
            .iter()
            .find(|r| r.code.eq_ignore_ascii_case(key.as_bytes())),
    };
    found.with_context(|| format!("no map {key:?} in the map list"))
}

pub fn run(rom: &Rom, args: &MapArgs) -> Result<()> {
    if args.reach_image.is_some() && args.reach.is_none() {
        bail!("--reach-image needs --reach");
    }
    if args.to.is_some() && args.reach.is_none() {
        bail!("--to needs --reach");
    }
    let table = Table::parse(rom.file_by_path(maplist::PATH)?).context("parsing map list")?;
    let list = MapList::parse(&table)?;
    let state_bytes = match &args.state {
        Some(path) => Some(fs::read(path).with_context(|| format!("reading {}", path.display()))?),
        None => None,
    };
    let state = state_bytes.as_deref().map(State::parse).transpose()?;
    let record = match (&args.map, &state) {
        (Some(key), _) => find(&list, key)?,
        (None, Some(state)) => {
            let (id, _) = state.current_map()?;
            list.find(id)
                .with_context(|| format!("current map {id} not in the list"))?
        }
        (None, None) => bail!("give a map or --state"),
    };
    let time_of_day = match (args.time, &state) {
        (Some(t), _) => t,
        (None, Some(state)) => state.u32(LIGHTING_MANAGER + 0x98)?,
        (None, None) => 0,
    };
    let ctx = Context {
        time_of_day,
        grotto_environ: args.grotto_environ,
    };
    let map = Map::assemble(record, &table, &list, |p| map::rom_file(rom, p), &ctx)?;
    print(record, &map, &ctx);
    if args.image.is_some()
        || args.at.is_some()
        || args.reach.is_some()
        || args.reach_image.is_some()
    {
        run_collision(rom, &map, &list, args)?;
    }
    if let Some(state) = &state {
        let differences = compare(state, &map)?;
        if differences.is_empty() {
            println!("\nRAM: the loaded blocks match the assembly");
        } else {
            println!("\nRAM: {} differences", differences.len());
            for d in differences {
                println!("  {d}");
            }
        }
    }
    Ok(())
}

/// Settles a user point on the floor: `[X, Z]` probes the highest floor at that column,
/// `[X, Y, Z]` looks from `Y` one unit above to ten below. Nothing is projected onto a
/// guessed height: a point without a floor is an error.
fn grounded(world: &CollisionWorld, values: &[f32], what: &str) -> Result<[f32; 3]> {
    let (x, z) = (values[0], values[values.len() - 1]);
    let y = match values.len() {
        3 => values[1],
        _ => reach::highest_ground(world, x, z)
            .with_context(|| format!("{what} point ({x:.3}, ?, {z:.3})"))?,
    };
    let ground = world
        .ground([
            (x * 4096.0) as i32,
            (y * 4096.0) as i32,
            (z * 4096.0) as i32,
        ])
        .with_context(|| {
            format!(
                "{what} point ({x:.3}, {y:.3}, {z:.3}) is not on the floor: \
                 CollisionWorld::ground found nothing within one unit above or ten below"
            )
        })?;
    Ok([x, ground as f32 / 4096.0, z])
}

/// Parses `X,Z` or `X,Y,Z` (comma or whitespace separated) into world units.
fn point_values(text: &str, what: &str, min: usize, max: usize) -> Result<Vec<f32>> {
    let values: Vec<f32> = text
        .split([',', ' ', '\t'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<f32>()
                .with_context(|| format!("{what}: {part:?} is not a number"))
        })
        .collect::<Result<_>>()?;
    if values.len() < min || values.len() > max {
        bail!(
            "{what} needs {min} to {max} numbers (X[,Y],Z), got {}",
            values.len()
        );
    }
    Ok(values)
}

/// Builds the collision world of the assembled map and renders and/or floods it.
fn run_collision(rom: &Rom, map: &Map, list: &MapList<'_>, args: &MapArgs) -> Result<()> {
    let world = CollisionWorld::build(map, |p| map::rom_file(rom, p))
        .context("building the collision world")?;
    println!(
        "\ncollision: {} meshes, {} placed pieces",
        world.meshes().len(),
        world.pieces().len()
    );
    let mut codes = HashMap::new();
    for record in &list.records {
        if record.id != 0 {
            codes.insert(record.id, text(record.code));
        }
    }
    let start_record = map.block_list.start.position;
    let raw_start = start_record.map(|c| c as f32 / 4096.0);
    let start = match world.ground(start_record) {
        Some(ground) => Some([raw_start[0], ground as f32 / 4096.0, raw_start[2]]),
        None => {
            eprintln!("warning: the map's start record is not on the floor; drawn at its own Y");
            Some(raw_start)
        }
    };
    let at = args
        .at
        .as_deref()
        .map(|text| point_values(text, "--at", 2, 3))
        .transpose()?;
    let reach_arg = args
        .reach
        .as_deref()
        .map(|text| point_values(text, "--reach", 2, 3))
        .transpose()?;
    let to_arg = args
        .to
        .as_deref()
        .map(|text| point_values(text, "--to", 3, 3))
        .transpose()?;
    let hero = at
        .as_ref()
        .map(|values| grounded(&world, values, "--at"))
        .transpose()?;
    let target = to_arg
        .as_ref()
        .map(|values| [values[0], values[1], values[2]]);
    let reach = match &reach_arg {
        Some(values) => {
            let start = grounded(&world, values, "--reach")?;
            let start_fx = start.map(|c| (c * 4096.0) as i32);
            let target_fx = target.map(|c| c.map(|v| (v * 4096.0) as i32));
            let reach = reach::flood(&world, start_fx, target_fx).context("flood fill")?;
            println!(
                "reach from ({:.3}, {:.3}, {:.3}) stood on the floor: {} nodes, y {:.3}..{:.3} \
                 (step {:.2}, 3D arrival < {:.2}, climb <= {:.1}, fall <= {:.1})",
                reach.start.x,
                reach.start.y,
                reach.start.z,
                reach.cells.len(),
                reach.min_y,
                reach.max_y,
                reach::GRID_STEP,
                reach::ARRIVE_DISTANCE,
                reach::MAX_CLIMB,
                reach::MAX_FALL
            );
            if let Some(target) = reach.target {
                println!(
                    "target ({:.3}, {:.3}, {:.3}): {}",
                    target.x,
                    target.y,
                    target.z,
                    if reach.target_reached == Some(true) {
                        "reached"
                    } else {
                        "not reached"
                    }
                );
            }
            Some(reach)
        }
        None => None,
    };
    if let Some(path) = &args.image {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let rendered = image::render(
            &world,
            &map.regions,
            &codes,
            image::Markers {
                start,
                hero,
                target,
                reach: None,
            },
            args.scale,
            path,
        )?;
        println!(
            "image {}: {}x{} px, floor heights {:.3}..{:.3}{}",
            path.display(),
            rendered.width,
            rendered.height,
            rendered.min_y,
            rendered.max_y,
            if hero.is_some() { ", hero marked" } else { "" }
        );
    }
    if let Some(path) = &args.reach_image {
        let Some(reach) = &reach else {
            bail!("--reach-image needs --reach");
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let rendered = image::render(
            &world,
            &map.regions,
            &codes,
            image::Markers {
                start,
                hero,
                target,
                reach: Some(reach),
            },
            args.scale,
            path,
        )?;
        println!(
            "reach image {}: {}x{} px, {} cells tinted",
            path.display(),
            rendered.width,
            rendered.height,
            reach.cells.len()
        );
    }
    Ok(())
}

fn print(record: &MapRecord<'_>, map: &Map, ctx: &Context) {
    println!(
        "map {} {} ({} / {}), area {}, time of day {}",
        record.id,
        text(record.code),
        text(record.region.unwrap_or_default()),
        text(record.label.unwrap_or_default()),
        record.area,
        ctx.time_of_day
    );
    println!("files, in load order:");
    println!("  {}", maplist::PATH);
    let found = |f: bool| if f { "" } else { "  (missing)" };
    println!("  {}{}", map.files.ambl, found(map.ambl_found));
    if !map.ambl_found {
        return;
    }
    println!("    textures: {}", map.textures.join(" "));
    if let Some((lighting, ok)) = &map.lighting {
        println!("  {} : {}{}", lighting.archive, lighting.member, found(*ok));
    }
    for amdj in &map.files.amdj {
        println!("  {amdj}{}", found(map.amdj_loaded.contains(amdj)));
    }
    if let Some(effects) = &map.effects {
        println!("  {effects}");
    }
    let z = &map.zone;
    println!(
        "zone: lighting {:?}, effects {:?}, .dat name {:?}, +0x36 {:#x}, +0x3c {}, world angle \
         {}, position ({}, {})",
        text(&z.lighting),
        text(&z.effects),
        text(&z.dat_name),
        z.field_36,
        z.field_3c,
        z.world_angle,
        fx(z.world_position[0]),
        fx(z.world_position[1])
    );
    let s = &map.block_list.start;
    println!(
        "start point: ({}, {}, {}) angle {:#x} (fx32 {:?})",
        fx(s.position[0]),
        fx(s.position[1]),
        fx(s.position[2]),
        s.angle,
        s.position
    );
    println!(
        "block list ({} of {}):",
        map.block_list.entries.len(),
        map.block_list.capacity
    );
    for e in &map.block_list.entries {
        println!(
            "  {:3} {:10} offset ({}, {}, {})",
            e.id,
            text(&e.name),
            fx(e.offset[0]),
            fx(e.offset[1]),
            fx(e.offset[2])
        );
    }
    for block in &map.blocks {
        let m = &block.manifest;
        let links: Vec<_> = m.textures.iter().map(|t| text(t)).collect();
        println!(
            "block {}:{} (entry {}): {}/{} resources, {}/{} placements, texture links [{}]{}",
            block.archive.rsplit('/').next().unwrap_or_default(),
            block.member,
            block.entry,
            m.resources.len(),
            m.resource_capacity,
            m.placements.len(),
            m.placement_capacity,
            links.join(" "),
            if block.processed { "" } else { ", not built" }
        );
        for (i, part) in block.parts.iter().enumerate() {
            let r = m.resources.get(i);
            let mut line = format!(
                "  part {i:3} {:9} {:14} flags {:#04x}",
                format!("{:?}", part.kind).to_lowercase(),
                text(&part.name),
                r.map_or(0, |r| r.flags)
            );
            if let Some(file) = &part.file {
                let _ = write!(line, "  {file}");
            }
            for a in &part.animations {
                let _ = write!(line, " +{}", a.kind.extension());
            }
            if let Some(t) = part.texture_source {
                let _ = write!(line, "  anim textures {}", map.textures[t]);
            }
            println!("{line}");
        }
        for (i, inst) in block.instances.iter().enumerate() {
            let p = &m.placements[i];
            let w = &inst.world;
            println!(
                "  inst {i:3} id {:3} part {:>4} parent {:>4} flags {:#04x}{} pos ({}, {}, {}) \
                 angle {} scale ({}, {}, {})",
                inst.id,
                inst.part.map_or("-".into(), |p| p.to_string()),
                inst.parent.map_or("-".into(), |p| p.to_string()),
                p.flags,
                if inst.hidden { " hidden" } else { "" },
                fx(w.position[0]),
                fx(w.position[1]),
                fx(w.position[2]),
                w.angle,
                fx(w.scale[0]),
                fx(w.scale[1]),
                fx(w.scale[2]),
            );
        }
    }
    let kinds = |k: PartKind| {
        map.blocks
            .iter()
            .flat_map(|b| &b.parts)
            .filter(|p| p.kind == k)
            .count()
    };
    println!(
        "total: {} blocks, {} models, {} objects, {} collisions, {} instances",
        map.blocks.len(),
        kinds(PartKind::Model),
        kinds(PartKind::Object),
        kinds(PartKind::Collision),
        map.blocks.iter().map(|b| b.instances.len()).sum::<usize>()
    );
}

/// Walks `g_Zone3D+0x41c` (blocks, 0x58 bytes) and compares each with the assembly.
fn compare(state: &State<'_>, map: &Map) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let u16_at = |a: u32| -> Result<u16> {
        let b = state.read(a, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    };
    let c_str = |a: u32| -> Result<Vec<u8>> {
        let mut s = Vec::new();
        for i in 0..64 {
            match state.read(a + i, 1)?[0] {
                0 => break,
                b => s.push(b),
            }
        }
        Ok(s)
    };
    let mut block = state.u32(melonds::ZONE_3D + 0x41c)?;
    let mut index = 0;
    while block != 0 {
        let Some(ours) = map.blocks.get(index) else {
            out.push(format!("RAM has more than {index} blocks"));
            break;
        };
        let b = |what: &str| format!("block {index} ({}): {what}", ours.member);
        let m = &ours.manifest;
        if state.u32(block)? != ours.entry {
            out.push(b("entry id"));
        }
        let res = state.u32(block + 4)?;
        let res_count = state.u32(block + 8)? as usize;
        if res_count != m.resources.len() || state.u32(block + 0xc)? as usize != m.resource_capacity
        {
            out.push(b("resource count"));
        }
        for (i, r) in m.resources.iter().enumerate().take(res_count) {
            let a = res + 12 * i as u32;
            let raw = state.read(a, 4)?;
            let name = c_str(state.u32(a + 4)?)?;
            if (raw[0..2] != r.id.to_le_bytes()) || raw[2] != r.flags || raw[3] != r.textures {
                out.push(b(&format!("resource {i} fields")));
            }
            if name != r.name {
                out.push(b(&format!("resource {i} name {:?}", text(&name))));
            }
        }
        let links = usize::from(state.read(block + 0x3c, 1)?[0]);
        if links != m.textures.len() {
            out.push(b("texture link count"));
        }
        for (i, t) in m.textures.iter().enumerate().take(links) {
            if &c_str(state.u32(block + 0x1c + 4 * i as u32)?)? != t {
                out.push(b(&format!("texture link {i}")));
            }
        }
        let pl = state.u32(block + 0x10)?;
        if state.u32(block + 0x14)? as usize != m.placements.len() {
            out.push(b("placement count"));
        }
        for (i, p) in m.placements.iter().enumerate() {
            let raw = state.read(pl + 32 * i as u32, 32)?;
            let mut ours = Vec::with_capacity(32);
            for v in [p.instance, p.resource as u16, p.parent as u16, p.flags] {
                ours.extend(v.to_le_bytes());
            }
            for v in p.translation {
                ours.extend(v.to_le_bytes());
            }
            for v in p.rotation.into_iter().chain(p.scale) {
                ours.extend(v.to_le_bytes());
            }
            if raw != ours.as_slice() {
                out.push(b(&format!("placement {i} bytes")));
            }
        }
        let parts = state.u32(block + 0x40)?;
        for (i, part) in ours.parts.iter().enumerate() {
            let a = parts + 0x18 * i as u32;
            let kind = match part.kind {
                PartKind::None => 0xffff,
                PartKind::Model => 0,
                PartKind::Collision => 1,
                PartKind::Object => 2,
            };
            if u16_at(a)? != kind {
                out.push(b(&format!("part {i} kind")));
            }
            if part.kind == PartKind::Model {
                // Animation slots: +8 nsbtp, +0xc nsbta, +0x10 nsbma, +0x14 nsbca.
                for (offset, ext) in [
                    (8, "nsbtp"),
                    (0xc, "nsbta"),
                    (0x10, "nsbma"),
                    (0x14, "nsbca"),
                ] {
                    let ram = state.u32(a + offset)? != 0;
                    let ours = part.animations.iter().any(|x| x.kind.extension() == ext);
                    if ram != ours {
                        out.push(b(&format!("part {i} {ext} slot")));
                    }
                }
            }
        }
        let nodes = state.u32(block + 0x44)?;
        let index_of =
            |ptr: u32, base: u32, size: u32| (ptr != 0).then(|| ((ptr - base) / size) as usize);
        for (i, inst) in ours.instances.iter().enumerate() {
            let a = nodes + 0x70 * i as u32;
            let world = &inst.world;
            let mut position = [0; 3];
            let mut scale = [0; 3];
            for c in 0..3 {
                position[c] = state.u32(a + 8 + 4 * c as u32)? as i32;
                scale[c] = state.u32(a + 0x14 + 4 * c as u32)? as i32;
            }
            let checks = [
                ("id", u16_at(a)? == inst.id),
                ("hidden", (u16_at(a + 2)? & 4 != 0) == inst.hidden),
                ("angle", u16_at(a + 6)? as i16 == world.angle),
                ("position", position == world.position),
                ("scale", scale == world.scale),
                (
                    "part",
                    index_of(state.u32(a + 0x24)?, parts, 0x18) == inst.part,
                ),
                (
                    "parent",
                    index_of(state.u32(a + 0x28)?, nodes, 0x70) == inst.parent,
                ),
            ];
            for (what, ok) in checks {
                if !ok {
                    out.push(b(&format!("instance {i} {what}")));
                }
            }
        }
        block = state.u32(block + 0x54)?;
        index += 1;
    }
    if index < map.blocks.len() {
        out.push(format!(
            "RAM has {index} blocks, the assembly {}",
            map.blocks.len()
        ));
    }
    Ok(out)
}
