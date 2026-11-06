//! From an event number (action 0x77 of a trigger) to its script: the archive
//! `data/event/evNNNNN.gp2` or `data/evspt_lv5/evNNNNN.gp2` and the entry numbers of the
//! event lists (`docs/re/stb.md`, § Fichiers). No I/O: the files come from a [`Files`].

use std::collections::BTreeMap;
use std::fmt;

use dq9_formats::{Gpc2, Table, Value};
use nds_rom::Rom;

use super::Entries;

/// The three event lists, searched in this order for the record of an event number: type 102
/// records `[stage, …, room, event, archive, name, script, entries 100/200/300, …]`.
const LISTS: [&str; 3] = [
    "data/event/eventlist6.bin",
    "data/event/evl_quest.bin",
    "data/evspt_lv5/eventlist_lv5.bin",
];
const DIRECTORIES: [&str; 2] = ["data/event", "data/evspt_lv5"];

/// Read access to the game's files by path.
pub trait Files {
    fn file(&self, path: &str) -> Option<&[u8]>;
}

impl Files for Rom<'_> {
    fn file(&self, path: &str) -> Option<&[u8]> {
        self.file_by_path(path).ok()
    }
}

/// Why an event could not be loaded.
#[derive(Debug)]
pub enum LoadError {
    /// No archive for this event number.
    NoScript(u16),
    /// The archive has no `.stb` member.
    NoMember(String),
    Format(nds_rom::Error),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoScript(event) => write!(f, "no script for event {event}"),
            Self::NoMember(path) => write!(f, "{path} holds no .stb script"),
            Self::Format(e) => write!(f, "event script: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<nds_rom::Error> for LoadError {
    fn from(e: nds_rom::Error) -> Self {
        Self::Format(e)
    }
}

/// An event's script and texts, as read from the game files.
#[derive(Debug, Clone, PartialEq)]
pub struct EventScript {
    pub event: u16,
    /// Archive name, `evNNNNN`.
    pub name: String,
    pub stb: Vec<u8>,
    pub entries: Entries,
    /// Message number → text (`evNNNNN_<lg>.bin`); empty when the language has no table.
    pub texts: BTreeMap<i32, String>,
}

/// Script name and entries of `event` from the event lists.
fn from_lists(files: &dyn Files, event: u16) -> Option<(String, Entries)> {
    event_record(files, event).map(|(name, entries, _)| (name, entries))
}

/// The record of `event`, with its rank among the kind-102 records of the list that holds it.
fn event_record(files: &dyn Files, event: u16) -> Option<(String, Entries, u16)> {
    LISTS.iter().find_map(|list| {
        let table = Table::parse(files.file(list)?).ok()?;
        let mut rank = 0u16;
        for record in table.records.iter().filter(|r| r.kind == 102) {
            let before = rank;
            rank = rank.saturating_add(1);
            let Some(v) = record.values.get(..12) else {
                continue;
            };
            let (Value::Int(id), Value::Str(Some(script))) = (v[5], v[8]) else {
                continue;
            };
            if id != u32::from(event) {
                continue;
            }
            let entry = |i: usize| match v[i] {
                Value::Int(n) => Some(n as i32),
                _ => None,
            };
            let name = String::from_utf8_lossy(script);
            let stem = name.strip_suffix(".stb").unwrap_or(&name).to_owned();
            let entries = Entries {
                main: entry(9)?,
                prepare: entry(10)?,
                finish: entry(11)?,
            };
            return Some((stem, entries, before));
        }
        None
    })
}

/// First scene-flag base, set at boot by `Flags_SetStoryBases` (`0x02071698`, called from
/// `main`) for event ids in [`SCENE_FLAG_IDS`].
pub const SCENE_FLAG_BASE: u16 = 0x21c;
/// Second scene-flag base, for event ids outside [`SCENE_FLAG_IDS`] (`0x02071698`).
pub const SCENE_FLAG_BASE_HIGH: u16 = 0x302;
/// Added by `EventTask_Update` (`ov_017::021bc77c`) when it turns the context's scene index
/// (`ctx+0x52`) into a global flag.
pub const SCENE_FLAG_OFFSET: u16 = 0x38e;
/// Event ids whose scene flag uses [`SCENE_FLAG_BASE`] (`0x02071584`).
const SCENE_FLAG_IDS: std::ops::Range<u32> = 0x5208..0x9c40;

/// Global flag the game sets when the scene of `event` starts running
/// (`EventTask_Update`, `ov_017::021bc77c`): `base + rank`, where the rank is the number of
/// kind-102 records before the event's record in its list. The task writes the rank in
/// `ctx+0x46` from the `Tbl_Run` counter (`EventRecord_Build`, `0x02071218`) and
/// `EventRecord_Parse` (`0x02071498`) adds the base; the same value is read back from
/// `ctx+0x52` on real melonDS states (21500 → 540 → flag 1450, …, 21596 → 551 → 1461;
/// `docs/re/trigger.md`, § « Drapeaux globaux posés à l'ouverture »).
pub fn scene_flag(files: &dyn Files, event: u16) -> Option<u16> {
    let (_, _, rank) = event_record(files, event)?;
    let base = if SCENE_FLAG_IDS.contains(&u32::from(event)) {
        SCENE_FLAG_BASE
    } else {
        SCENE_FLAG_BASE_HIGH
    };
    base.checked_add(rank)?.checked_add(SCENE_FLAG_OFFSET)
}

/// Loads the script of `event` with the texts of language `lang` (`en`, `fr`, …).
pub fn load(files: &dyn Files, event: u16, lang: &str) -> Result<EventScript, LoadError> {
    let (name, entries) =
        from_lists(files, event).unwrap_or_else(|| (format!("ev{event:05}"), Entries::default()));
    let path = DIRECTORIES
        .iter()
        .map(|dir| format!("{dir}/{name}.gp2"))
        .find(|p| files.file(p).is_some())
        .ok_or(LoadError::NoScript(event))?;
    let archive = Gpc2::parse(files.file(&path).ok_or(LoadError::NoScript(event))?)?;
    let stb = archive
        .entries
        .iter()
        .find(|e| e.name.ends_with(".stb"))
        .ok_or_else(|| LoadError::NoMember(path.clone()))?;
    let stb = archive.member(stb)?.into_owned();
    let texts = match archive.find(&format!("{name}_{lang}.bin")) {
        Some(entry) => {
            let data = archive.member(entry)?;
            Table::parse(&data)?
                .records
                .iter()
                .filter_map(|r| match r.values[..] {
                    [Value::Int(id), Value::Str(Some(text)), ..] => {
                        Some((id as i32, String::from_utf8_lossy(text).into_owned()))
                    }
                    _ => None,
                })
                .collect()
        }
        None => BTreeMap::new(),
    };
    Ok(EventScript {
        event,
        name,
        stb,
        entries,
        texts,
    })
}

/// The event lists that give a scene's story stage (`stb.md`); `evl_quest.bin` holds quests.
const STAGE_LISTS: [&str; 2] = [LISTS[0], LISTS[2]];

/// `(event number, script name)` of the scenes at story stage `major.minor` for every
/// `minor` of `minors` (values 0 and 1 of the type 102 records), sorted.
pub fn stage_events(
    files: &dyn Files,
    major: u32,
    minors: std::ops::RangeInclusive<u32>,
) -> Vec<(u16, String)> {
    let mut events = Vec::new();
    for list in STAGE_LISTS {
        let Some(table) = files.file(list).and_then(|d| Table::parse(d).ok()) else {
            continue;
        };
        for record in table.records.iter().filter(|r| r.kind == 102) {
            let Some(
                &[
                    Value::Int(a),
                    Value::Int(b),
                    _,
                    _,
                    _,
                    Value::Int(event),
                    _,
                    _,
                    Value::Str(Some(script)),
                ],
            ) = record.values.get(..9)
            else {
                continue;
            };
            if a == major && minors.contains(&b) {
                let name = String::from_utf8_lossy(script);
                let stem = name.strip_suffix(".stb").unwrap_or(&name).to_owned();
                if let Ok(event) = u16::try_from(event) {
                    events.push((event, stem));
                }
            }
        }
    }
    events.sort();
    events.dedup();
    events
}

/// Where an event happens, from its event-list record (`stb.md`, § Fichiers).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventPlace {
    /// Map id (record value 4).
    pub map: u16,
    /// Position and angle of the record (values 12 to 15; all zero when the event has none,
    /// the script then places everyone itself) **[hypothèse]**: the hero's start.
    pub position: Option<([f32; 3], f32)>,
}

/// The map and start of `event`, from the event lists.
pub fn place(files: &dyn Files, event: u16) -> Option<EventPlace> {
    LISTS.iter().find_map(|list| {
        let table = Table::parse(files.file(list)?).ok()?;
        table
            .records
            .iter()
            .filter(|r| r.kind == 102)
            .find_map(|r| {
                let v = r.values.get(..16)?;
                let (Value::Int(map), Value::Int(id)) = (v[4], v[5]) else {
                    return None;
                };
                if id != u32::from(event) {
                    return None;
                }
                let number = |i: usize| match v[i] {
                    Value::Float(f) => f,
                    _ => 0.0,
                };
                let at = [number(12), number(13), number(14)];
                let angle = number(15);
                let position = (at != [0.0; 3] || angle != 0.0).then_some((at, angle));
                Some(EventPlace {
                    map: u16::try_from(map).ok()?,
                    position,
                })
            })
    })
}

/// The first scene of the story stage `major.minor` in `eventlist_lv5.bin`, in list order:
/// the opening cutscene of the stage (21500 for 1.1).
pub fn stage_opening(files: &dyn Files, major: u32, minor: u32) -> Option<u16> {
    let table = Table::parse(files.file(LISTS[2])?).ok()?;
    table
        .records
        .iter()
        .filter(|r| r.kind == 102)
        .find_map(|r| match r.values.get(..6)? {
            &[Value::Int(a), Value::Int(b), _, _, _, Value::Int(event)]
                if a == major && b == minor =>
            {
                u16::try_from(event).ok()
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// One `eventlist_lv5.bin` with the given event ids in file order: kind 102 records with
    /// `[stage major, stage minor, 0, 0, 0, event, 0, 0, "evNNNNN.stb", 100, 200, 300]`.
    fn list(events: &[(u16, u32)]) -> Vec<u8> {
        let mut pool = Vec::new();
        let mut records = Vec::new();
        for &(event, major) in events {
            let script = format!("ev{event:05}.stb");
            let at = pool.len() as u32;
            pool.extend_from_slice(script.as_bytes());
            pool.push(0);
            let values = [major, 1, 0, 0, 0, u32::from(event), 0, 0, at, 100, 200, 300];
            records.push((kind_header(), values));
        }
        build(&records, &pool)
    }

    fn kind_header() -> Vec<u8> {
        // 12 fields: 0x55, 0x55, 0x54 (string at index 8), then two 0xFF padding bytes.
        vec![102, 0, 12, 0x55, 0x55, 0x54, 0xFF, 0xFF]
    }

    fn build(records: &[(Vec<u8>, [u32; 12])], pool: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(records.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 12]); // pool offset and size, string count: patched below
        for (header, values) in records {
            out.extend_from_slice(header);
            for value in values {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        let pool_offset = out.len() as u32;
        out.extend_from_slice(pool);
        out[4..8].copy_from_slice(&pool_offset.to_le_bytes());
        out[8..12].copy_from_slice(&(pool.len() as u32).to_le_bytes());
        out[12..16].copy_from_slice(&(records.len() as u32).to_le_bytes());
        out
    }

    struct Map(HashMap<String, Vec<u8>>);

    impl Files for Map {
        fn file(&self, path: &str) -> Option<&[u8]> {
            self.0.get(path).map(Vec::as_slice)
        }
    }

    fn files(events: &[(u16, u32)]) -> Map {
        Map(HashMap::from([(LISTS[2].to_owned(), list(events))]))
    }

    #[test]
    fn scene_flags_follow_the_event_list_rank() {
        let files = files(&[(21500, 1), (21507, 1), (21520, 1), (21596, 1)]);
        assert_eq!(scene_flag(&files, 21500), Some(1450));
        assert_eq!(scene_flag(&files, 21507), Some(1451));
        assert_eq!(scene_flag(&files, 21520), Some(1452));
        // Rank 3 among the kind-102 records, not the event number.
        assert_eq!(scene_flag(&files, 21596), Some(1453));
        assert_eq!(scene_flag(&files, 1), None);
    }

    #[test]
    fn scene_flag_uses_the_other_base_below_the_rom_range() {
        let files = files(&[(1160, 1), (1600, 1)]);
        assert_eq!(
            scene_flag(&files, 1160),
            Some(SCENE_FLAG_BASE_HIGH + SCENE_FLAG_OFFSET)
        );
        assert_eq!(
            scene_flag(&files, 1600),
            Some(SCENE_FLAG_BASE_HIGH + SCENE_FLAG_OFFSET + 1)
        );
    }
}
