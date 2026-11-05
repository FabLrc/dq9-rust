//! Map loading from the ROM bytes (the ROM was read by the platform layer): map list, map
//! assembly (`dq9_formats::map`), then the render scene.

use anyhow::{Context as _, Result};
use dq9_formats::Table;
use dq9_formats::cast::Story;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList, MapRecord};
use dq9_render::AnimatedScene;
use nds_rom::Rom;

/// One shippable map: id and code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntry {
    pub id: u16,
    pub code: String,
}

/// A map ready to upload.
#[derive(Debug)]
pub struct LoadedMap {
    pub entry: MapEntry,
    /// Developer label of the map list (slots 2 and 5), for the window title.
    pub label: String,
    pub time: u32,
    pub runtime: AnimatedScene,
}

fn text(s: &[u8]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

fn find<'a>(list: &'a MapList<'a>, key: &str) -> Option<&'a MapRecord<'a>> {
    match key.parse::<u16>() {
        Ok(id) => list.find(id),
        Err(_) => list
            .records
            .iter()
            .find(|r| r.code.eq_ignore_ascii_case(key.as_bytes())),
    }
}

/// The maps of the list (id ≠ 0, with a code), in list order.
pub fn maps(rom: &[u8]) -> Result<Vec<MapEntry>> {
    let rom = Rom::parse(rom).context("parsing the ROM")?;
    let table = Table::parse(rom.file_by_path(maplist::PATH)?).context("parsing the map list")?;
    let list = MapList::parse(&table)?;
    Ok(list
        .records
        .iter()
        .filter(|r| r.id != 0 && !r.code.is_empty())
        .map(|r| MapEntry {
            id: r.id,
            code: text(r.code),
        })
        .collect())
}

/// Assembles map `key` (code such as `M01`, or id) at time of day `time` and builds its
/// scene.
pub fn load(rom: &[u8], key: &str, time: u32, story: &Story) -> Result<LoadedMap> {
    let rom = Rom::parse(rom).context("parsing the ROM")?;
    let table = Table::parse(rom.file_by_path(maplist::PATH)?).context("parsing the map list")?;
    let list = MapList::parse(&table)?;
    let record = find(&list, key).with_context(|| format!("no map {key:?} in the map list"))?;
    let ctx = Context {
        time_of_day: time,
        grotto_environ: 1,
    };
    let assembled = Map::assemble(record, &table, |p| map::rom_file(&rom, p), &ctx)
        .with_context(|| format!("assembling map {key}"))?;
    let runtime = AnimatedScene::build(&assembled, record.id, record.code, story, |p| {
        map::rom_file(&rom, p)
    })
    .with_context(|| format!("loading animations and cast for {key}"))?;
    let label = [record.region, record.label]
        .into_iter()
        .flatten()
        .map(text)
        .collect::<Vec<_>>()
        .join(" / ");
    Ok(LoadedMap {
        entry: MapEntry {
            id: record.id,
            code: text(record.code),
        },
        label,
        time,
        runtime,
    })
}
