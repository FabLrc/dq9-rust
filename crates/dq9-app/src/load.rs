//! Map loading from the ROM bytes (the ROM was read by the platform layer): map list, map
//! assembly (`dq9_formats::map`), then the render scene.

use anyhow::{Context as _, Result};
use dq9_core::{CollisionWorld, Hero};
use dq9_formats::Table;
use dq9_formats::cast::Story;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList, MapRecord};
use dq9_formats::save::Save;
use dq9_render::AnimatedScene;
use glam::Vec3;
use nds_rom::Rom;

/// Chooses an explicit map, otherwise the map id in a normal save, then the prologue map.
pub fn starting_map(map: Option<&str>, save: Option<&[u8]>) -> Result<String> {
    if let Some(map) = map {
        return Ok(map.to_owned());
    }
    let Some(bytes) = save else {
        return Ok("M01".into());
    };
    let save = Save::parse(bytes).context("parsing the save")?;
    let index = save
        .current_slot()
        .context("save has no valid normal slot")?;
    let slot = save.slot(index).context("save slot is missing")?;
    Ok(slot.area().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::save::{FILE_SIZE, MAGIC};

    fn save_with_area(area: u16) -> Vec<u8> {
        let mut bytes = vec![0; FILE_SIZE];
        bytes[..MAGIC.len()].copy_from_slice(MAGIC);
        bytes[21] = 1;
        bytes[11476..11478].copy_from_slice(&area.to_le_bytes());
        let mut save = Save::parse(&bytes).unwrap();
        save.slot_mut(0).unwrap().seal();
        save.as_bytes().to_vec()
    }

    #[test]
    fn saved_map_uses_first_valid_normal_slot_area() {
        assert_eq!(
            starting_map(None, Some(&save_with_area(109))).unwrap(),
            "109"
        );
    }

    #[test]
    fn explicit_map_overrides_save_and_bad_save_is_rejected() {
        assert_eq!(starting_map(Some("C01M09"), Some(&[])).unwrap(), "C01M09");
        assert!(starting_map(None, Some(&[])).is_err());
    }

    #[test]
    fn no_save_defaults_to_prologue_map() {
        assert_eq!(starting_map(None, None).unwrap(), "M01");
    }
}
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
    pub collision: CollisionWorld,
    pub hero: Hero,
    pub hero_instance: usize,
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
    let collision = CollisionWorld::build(&assembled, |p| map::rom_file(&rom, p))
        .with_context(|| format!("loading collision for {key}"))?;
    let hero = Hero::new(collision.spawn().unwrap_or(Vec3::ZERO));
    let mut runtime = AnimatedScene::build(&assembled, record.id, record.code, story, |p| {
        map::rom_file(&rom, p)
    })
    .with_context(|| format!("loading animations and cast for {key}"))?;
    let hero_instance = runtime.add_hero_marker(hero.position);
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
        collision,
        hero,
        hero_instance,
    })
}
