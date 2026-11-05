//! Map loading from the ROM bytes (the ROM was read by the platform layer): map list, map
//! assembly (`dq9_formats::map`), then the render scene.

use anyhow::{Context as _, Result};
use dq9_core::field::{self, Field};
use dq9_core::state::GameState;
use dq9_core::talk::Talkable;
use dq9_core::{CollisionWorld, Hero};
use dq9_formats::Table;
use dq9_formats::cast::Story;
use dq9_formats::hero::HeroLook;
use dq9_formats::map::{self, Context, Map};
use dq9_formats::maplist::{self, MapList, MapRecord};
use dq9_formats::save::Save;
use dq9_render::AnimatedScene;
use glam::Vec3;
use nds_rom::Rom;

/// What the application starts from: a map key and, when it comes from a save, the state the
/// save restores.
#[derive(Debug, Clone)]
pub struct Start {
    pub map: String,
    /// `None` for an explicit map or without a save: the hero then starts where the
    /// collisions put it.
    pub state: Option<GameState>,
    /// The protagonist's look: the save's hero when there is a save, else a new game's.
    pub look: HeroLook,
}

/// The hero of the current slot of `save` (the first character of the party), if it reads.
fn save_look(bytes: &[u8]) -> Option<HeroLook> {
    let save = Save::parse(bytes).ok()?;
    let slot = save.slot(save.current_slot()?)?;
    let hero = slot.character(usize::from(slot.standby_count()))?;
    Some(HeroLook::from_character(&hero))
}

/// Chooses an explicit map (the save is then ignored: its position belongs to its own map),
/// otherwise the first valid normal slot of the save, then the prologue map.
pub fn start(map: Option<&str>, save: Option<&[u8]>) -> Result<Start> {
    let look = save.and_then(save_look).unwrap_or_else(HeroLook::new_game);
    if let Some(map) = map {
        return Ok(Start {
            map: map.to_owned(),
            state: None,
            look,
        });
    }
    let Some(bytes) = save else {
        return Ok(Start {
            map: "M01".into(),
            state: None,
            look,
        });
    };
    let save = Save::parse(bytes).context("parsing the save")?;
    let state = GameState::from_save(&save).context("restoring the save")?;
    Ok(Start {
        map: state.map.to_string(),
        state: Some(state),
        look,
    })
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
        let start = start(None, Some(&save_with_area(109))).unwrap();
        assert_eq!(start.map, "109");
        assert_eq!(start.state.unwrap().map, 109);
    }

    #[test]
    fn explicit_map_overrides_save_and_bad_save_is_rejected() {
        let start = start(Some("C01M09"), Some(&[])).unwrap();
        assert_eq!(start.map, "C01M09");
        assert!(start.state.is_none());
        assert!(super::start(None, Some(&[])).is_err());
    }

    #[test]
    fn no_save_defaults_to_prologue_map() {
        let start = start(None, None).unwrap();
        assert_eq!(start.map, "M01");
        assert!(start.state.is_none());
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
    /// Warps, doorways and areas of the map with its trigger file.
    pub field: Field,
    /// The hero's pose is a game pose (doorway arrival or save), not the computed default.
    pub placed: bool,
    /// The map's `.bats` lighting script, when it has one.
    pub lighting: Option<dq9_formats::lighting::LightingInfo>,
    /// The NPCs the hero can talk to, at the story point.
    pub npcs: Vec<Talkable>,
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
/// scene. The hero stands at `spawn` (a doorway arrival), else where `state` puts the party
/// on this map (`GameState::hero_pose`), else at the point the collisions give.
pub fn load(
    rom: &[u8],
    key: &str,
    time: u32,
    story: &Story,
    spawn: Option<(Vec3, f32)>,
    state: Option<&GameState>,
    look: &HeroLook,
) -> Result<LoadedMap> {
    let rom = Rom::parse(rom).context("parsing the ROM")?;
    let table = Table::parse(rom.file_by_path(maplist::PATH)?).context("parsing the map list")?;
    let list = MapList::parse(&table)?;
    let record = find(&list, key).with_context(|| format!("no map {key:?} in the map list"))?;
    let ctx = Context {
        time_of_day: time,
        grotto_environ: 1,
    };
    let assembled = Map::assemble(record, &table, &list, |p| map::rom_file(&rom, p), &ctx)
        .with_context(|| format!("assembling map {key}"))?;
    let collision = CollisionWorld::build(&assembled, |p| map::rom_file(&rom, p))
        .with_context(|| format!("loading collision for {key}"))?;
    // A state only places the party on its own map; a map without start record (all zero)
    // keeps the computed point.
    let saved = state
        .filter(|s| s.map == record.id)
        .map(|s| s.hero_pose(assembled.block_list.start))
        .filter(|pose| pose.position != [0; 3]);
    let (position, yaw) = match (spawn, saved) {
        (Some(spawn), _) => spawn,
        (None, Some(pose)) => {
            let [x, y, z] = pose.position_f32();
            // The game stands the hero on the ground it finds (`ov_017::0219c598` places the
            // pose as is, the ground follows: `ground_height` includes the `0x199` it adds).
            let y = collision.ground_height(x, z, y).unwrap_or(y);
            (Vec3::new(x, y, z), pose.angle_radians())
        }
        (None, None) => (collision.spawn().unwrap_or(Vec3::ZERO), 0.0),
    };
    let placed = spawn.is_some() || saved.is_some();
    let lighting = crate::lighting::script(&assembled, |p| map::rom_file(&rom, p));
    let hero = Hero::at(position, yaw);
    let mut runtime = AnimatedScene::build(&assembled, record.id, record.code, story, |p| {
        map::rom_file(&rom, p)
    })
    .with_context(|| format!("loading animations and cast for {key}"))?;
    let hero_instance = runtime
        .add_hero(look, |p| map::rom_file(&rom, p))
        .context("building the hero model")?;
    runtime.scene.instances[hero_instance].world =
        runtime.hero_world(hero.position(), field::angle_of_yaw(hero.yaw()));
    let trigger_file = field::trigger_path(&text(record.code))
        .and_then(|path| map::rom_file(&rom, &path))
        .map(<[u8]>::to_vec);
    let field = Field::new(record.id, &assembled.regions, trigger_file)
        .with_context(|| format!("reading the trigger file of {key}"))?;
    let npcs = match map::rom_file(&rom, &dq9_formats::cast::archive_path(record.code)) {
        Some(bytes) => dq9_formats::cast::load(bytes, record.id, story)
            .with_context(|| format!("reading the cast of {key}"))?
            .npcs
            .iter()
            .map(Talkable::from_cast)
            .collect(),
        None => Vec::new(),
    };
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
        field,
        placed,
        hero_instance,
        lighting,
        npcs,
    })
}
