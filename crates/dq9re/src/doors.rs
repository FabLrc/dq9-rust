//! `dq9re doors`: the region records (`0x72`, `0x73`/`0x74`) of a map's `.bmbl`, raw and
//! as `dq9_core::field` reads them.

use std::fs;

use anyhow::{Context as _, Result, bail};
use dq9_formats::Table;
use dq9_formats::map::{self, MapFiles, Regions};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::melonds::{GAME_STATE, State, ZONE_3D};
use dq9_formats::table::Value;
use nds_rom::{Narc, Rom, compress};

#[derive(clap::Args)]
pub struct DoorsArgs {
    /// Map code (`M01`) or id (`109`); defaults to the current map of `--state`.
    map: Option<String>,
    /// melonDS state whose loaded regions (`g_Zone3D+0x6c`) are compared with the parse; also
    /// prints the leader's pose and the pending warp.
    #[arg(long)]
    state: Option<std::path::PathBuf>,
    /// Print statistics of every map's region records instead.
    #[arg(long)]
    stats: bool,
    /// Also list the triggers of the map answering region events (2/5 areas, 3 arrival, 6,
    /// 17 doorway touched, 20 load), as loaded at `--chapter`/`--step`.
    #[arg(long)]
    triggers: bool,
    #[arg(long, default_value_t = 1)]
    chapter: i32,
    #[arg(long, default_value_t = 1)]
    step: i32,
}

fn value(v: &Value<'_>) -> String {
    match v {
        Value::Str(Some(s)) => format!("{:?}", String::from_utf8_lossy(s)),
        Value::Str(None) => "nil".into(),
        Value::Int(i) => format!("{}", *i as i32),
        Value::Float(f) => format!("{f}"),
    }
}

fn stats(rom: &Rom, list: &MapList<'_>) -> Result<()> {
    use std::collections::BTreeMap;
    let mut seen = std::collections::BTreeSet::new();
    let mut c: BTreeMap<String, usize> = BTreeMap::new();
    for record in &list.records {
        if record.id == 0 || record.code.is_empty() {
            continue;
        }
        let files = MapFiles::new(record.id, record.code, 1);
        if !seen.insert(files.ambl.clone()) {
            continue;
        }
        let Some(ambl) = map::rom_file(rom, &files.ambl) else {
            continue;
        };
        let narc = Narc::parse(ambl)?;
        for entry in narc
            .names
            .files
            .iter()
            .filter(|f| f.path.ends_with(".bmbl"))
        {
            let bmbl = compress::decompress(narc.file(entry.id)?)?;
            let table = Table::parse(&bmbl)?;
            let mut last73 = None;
            for r in &table.records {
                let v = &r.values;
                if (r.kind == 0x72 && v.len() < 16) || (r.kind != 0x72 && v.len() < 4) {
                    continue;
                }
                let mut add = |k: String| *c.entry(k).or_default() += 1;
                match r.kind {
                    0x72 => {
                        add(format!("72 len {}", v.len()));
                        add(format!("72 type(v7) {}", value(&v[7])));
                        add(format!("72 v9 {}", value(&v[9])));
                        add(format!("72 v10 {}", value(&v[10])));
                        add(format!("72 v15 {}", value(&v[15])));
                        add(format!("72 dest str {}", v[8].str().is_some()));
                        add(format!("72 size y {}", value(&v[4])));
                    }
                    0x73 => {
                        add(format!("73 len {} type {}", v.len(), value(&v[0])));
                        last73 = Some(v[0].int());
                    }
                    0x74 => {
                        let t = last73.take();
                        add(format!("74 after73 type {t:?} len {}", v.len()));
                        add(format!(
                            "74 t{t:?} v0 {} flags(v2) {} v3 {}",
                            value(&v[0]),
                            value(&v[2]),
                            value(&v[3])
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
    for (k, n) in c {
        println!("{n:6} {k}");
    }
    Ok(())
}

pub fn run(rom: &Rom, args: &DoorsArgs) -> Result<()> {
    let table = Table::parse(rom.file_by_path(maplist::PATH)?).context("parsing map list")?;
    let list = MapList::parse(&table)?;
    if args.stats {
        return stats(rom, &list);
    }
    let state_bytes = match &args.state {
        Some(path) => Some(fs::read(path).with_context(|| format!("reading {}", path.display()))?),
        None => None,
    };
    let state = state_bytes.as_deref().map(State::parse).transpose()?;
    let key = match (&args.map, &state) {
        (Some(key), _) => key.clone(),
        (None, Some(state)) => state.current_map()?.0.to_string(),
        (None, None) => bail!("give a map or --state"),
    };
    let record = match key.parse::<u16>() {
        Ok(id) => list.find(id),
        Err(_) => list
            .records
            .iter()
            .find(|r| r.code.eq_ignore_ascii_case(key.as_bytes())),
    }
    .with_context(|| format!("no map {key:?} in the map list"))?;
    let files = MapFiles::new(record.id, record.code, 1);
    let ambl = map::rom_file(rom, &files.ambl).with_context(|| files.ambl.clone())?;
    let narc = Narc::parse(ambl)?;
    let Some(entry) = narc.names.files.iter().find(|f| f.path.ends_with(".bmbl")) else {
        bail!("no .bmbl in {}", files.ambl);
    };
    let bmbl = compress::decompress(narc.file(entry.id)?)?;
    let table = Table::parse(&bmbl)?;
    let regions = map::parse_regions(&table, &list.records)?;
    println!(
        "{} {} {}: {} records",
        record.id,
        String::from_utf8_lossy(record.code),
        entry.path,
        table.records.len()
    );
    for (i, r) in table.records.iter().enumerate() {
        if !(0x72..=0x74).contains(&r.kind) {
            continue;
        }
        let values: Vec<String> = r.values.iter().map(value).collect();
        println!("#{i} {:#x} [{}] {}", r.kind, values.len(), values.join(" "));
    }
    if let Some(state) = &state {
        let position = leader(state)?;
        if let Some(position) = position {
            for (i, w) in regions.warps.iter().enumerate() {
                if dq9_core::field::warp_contains(w, position) {
                    println!("leader inside warp {i} (to {})", w.destination);
                }
            }
            for (i, z) in regions.zones.iter().enumerate() {
                if dq9_core::field::zone_contains(z, position) {
                    println!(
                        "leader inside zone {i} (type {}, class {}, number {})",
                        z.kind, z.class, z.number
                    );
                }
            }
        }
        let differences = compare(state, &regions)?;
        if differences.is_empty() {
            println!(
                "regions: {} warps and {} zones identical to RAM",
                regions.warps.len(),
                regions.zones.len()
            );
        }
        for d in &differences {
            println!("DIFF {d}");
        }
    }
    if args.triggers {
        let name = String::from_utf8_lossy(record.code);
        let prefix = if name.starts_with('F') {
            &name[..1]
        } else {
            &name[..3]
        };
        let path = format!("data/scenario/trigger{prefix}.bin");
        let data = rom.file_by_path(&path)?;
        let table = Table::parse(data)?;
        println!(
            "{path}, room {} at {}.{}:",
            record.id, args.chapter, args.step
        );
        for t in dq9_formats::trigger::parse(&table)? {
            if t.loaded(i32::from(record.id), args.chapter, args.step)
                && [2, 3, 5, 6, 17, 20].contains(&t.event)
            {
                println!(
                    "  event {}: if {} then {}",
                    t.event,
                    crate::talk::ops(&t.conditions),
                    crate::talk::ops(&t.actions)
                );
            }
        }
    }
    Ok(())
}

fn fx(v: i32) -> String {
    format!("{:.3}", f64::from(v) / 4096.0)
}

/// The leader's object (`GameState+0x397c` holds the first party member, `Chara_Get`):
/// position, angle and the pending warp (`Save_Warp`, `GameState+0x3f8`).
fn leader(state: &State) -> Result<Option<glam::Vec3>> {
    let first = u32::from(state.read(GAME_STATE + 0x397c, 1)?[0]);
    let Some(object) = state.object(first)? else {
        println!("no leader object (party member {first})");
        return Ok(None);
    };
    let p = state.object_position(object)?;
    let angle = state.u32(object + 0x54)? as i32;
    println!(
        "leader {first} at {object:#x}: position ({}, {}, {}), angle {} ({:.3} rad)",
        fx(p[0]),
        fx(p[1]),
        fx(p[2]),
        angle,
        f64::from(angle) / 4096.0
    );
    let warp = state.read(GAME_STATE + 0x3f8, 0x60)?;
    let u16_at = |o: usize| u16::from_le_bytes([warp[o], warp[o + 1]]);
    let i32_at = |o: usize| i32::from_le_bytes([warp[o], warp[o + 1], warp[o + 2], warp[o + 3]]);
    println!(
        "Save_Warp: room {}, flags +2 {} +7 {} +0xa {} +0xc {}, entry {}, position ({}, {}, {}), angle {}",
        u16_at(0),
        warp[2],
        warp[7],
        warp[10],
        warp[12],
        warp[0xb] as i8,
        fx(i32_at(0x10)),
        fx(i32_at(0x14)),
        fx(i32_at(0x18)),
        u16_at(0x1c) as i16
    );
    Ok(Some(glam::Vec3::from_array(p.map(|c| c as f32 / 4096.0))))
}

/// Compares the parsed regions with the arrays of `g_Zone3D+0x6c`: warps (`+0x18`, count
/// `+0x1c`, 0x70 bytes each) and zones (`+0x24`, count `+0x28`, 0x74 bytes each).
fn compare(state: &State, regions: &Regions) -> Result<Vec<String>> {
    let base = ZONE_3D + 0x6c;
    let mut out = Vec::new();
    let i32s = |addr: u32, n: u32| -> Result<Vec<i32>> {
        (0..n)
            .map(|i| Ok(state.u32(addr + 4 * i)? as i32))
            .collect()
    };
    let u16_at = |addr: u32| -> Result<u16> {
        let b = state.read(addr, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    };
    // Warps with a destination the map list does not know are not loaded here.
    let count = state.u32(base + 0x1c)? as usize;
    let array = state.u32(base + 0x18)?;
    if count != regions.warps.len() {
        out.push(format!(
            "warps: RAM {count}, parsed {}",
            regions.warps.len()
        ));
    }
    for (i, w) in regions.warps.iter().enumerate().take(count) {
        let a = array + 0x70 * i as u32;
        let v = &w.volume;
        let mut check = |name: &str, ram: Vec<i32>, ours: Vec<i32>| {
            if ram != ours {
                out.push(format!("warp {i} {name}: RAM {ram:?}, parsed {ours:?}"));
            }
        };
        check("kind", vec![state.u32(a)? as i32], vec![w.kind]);
        check("high", i32s(a + 4, 3)?, v.high.to_vec());
        check("low", i32s(a + 0x10, 3)?, v.low.to_vec());
        check(
            "destination",
            vec![state.u32(a + 0x1c)? as i32],
            vec![i32::from(w.destination)],
        );
        check("arrival", i32s(a + 0x20, 3)?, w.arrival.to_vec());
        check(
            "arrival angle",
            vec![i32::from(u16_at(a + 0x50)? as i16)],
            vec![i32::from(w.arrival_angle)],
        );
        check(
            "angle",
            vec![i32::from(u16_at(a + 0x52)? as i16)],
            vec![i32::from(v.angle)],
        );
        check(
            "radius²",
            vec![state.u32(a + 0x54)? as i32],
            vec![v.radius_sq],
        );
        check("center", i32s(a + 0x58, 3)?, v.center.to_vec());
    }
    let count = state.u32(base + 0x28)? as usize;
    let array = state.u32(base + 0x24)?;
    if count != regions.zones.len() {
        out.push(format!(
            "zones: RAM {count}, parsed {}",
            regions.zones.len()
        ));
    }
    for (i, z) in regions.zones.iter().enumerate().take(count) {
        let a = array + 0x74 * i as u32;
        let v = &z.volume;
        let mut check = |name: &str, ram: Vec<i32>, ours: Vec<i32>| {
            if ram != ours {
                out.push(format!("zone {i} {name}: RAM {ram:?}, parsed {ours:?}"));
            }
        };
        check("type", vec![state.u32(a + 4)? as i32], vec![z.kind as i32]);
        check("center", i32s(a + 8, 3)?, v.center.to_vec());
        check("size", i32s(a + 0x14, 3)?, v.size.to_vec());
        check(
            "facing",
            vec![i32::from(u16_at(a + 0x20)? as i16)],
            vec![i32::from(v.facing)],
        );
        check(
            "angle",
            vec![i32::from(u16_at(a + 0x22)? as i16)],
            vec![i32::from(v.angle)],
        );
        check(
            "radius²",
            vec![state.u32(a + 0x24)? as i32],
            vec![v.radius_sq],
        );
        let b = state.read(a + 0x2c, 4)?;
        check("class", vec![i32::from(b[0])], vec![i32::from(z.class)]);
        if z.kind == map::ZONE_DOORWAY {
            check("number", vec![i32::from(b[1])], vec![i32::from(z.number)]);
            let flags = u16::from_le_bytes([b[2], b[3]]) >> 4;
            check("flags", vec![i32::from(flags)], vec![i32::from(z.flags)]);
            if let Some(w) = z.warp {
                check(
                    "destination",
                    vec![i32::from(u16_at(a + 0x30)?)],
                    vec![i32::from(w.destination)],
                );
                check("arrival", i32s(a + 0x38, 3)?, w.arrival.to_vec());
                check(
                    "arrival angle",
                    vec![i32::from(u16_at(a + 0x68)? as i16)],
                    vec![i32::from(w.arrival_angle)],
                );
            }
        }
    }
    Ok(out)
}
