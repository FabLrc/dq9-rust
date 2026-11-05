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
    LISTS.iter().find_map(|list| {
        let table = Table::parse(files.file(list)?).ok()?;
        table
            .records
            .iter()
            .filter(|r| r.kind == 102)
            .find_map(|r| {
                let v = r.values.get(..12)?;
                let (Value::Int(id), Value::Str(Some(script))) = (v[5], v[8]) else {
                    return None;
                };
                let entry = |i: usize| match v[i] {
                    Value::Int(n) => Some(n as i32),
                    _ => None,
                };
                if id != u32::from(event) {
                    return None;
                }
                let name = String::from_utf8_lossy(script);
                let stem = name.strip_suffix(".stb").unwrap_or(&name).to_owned();
                let entries = Entries {
                    main: entry(9)?,
                    prepare: entry(10)?,
                    finish: entry(11)?,
                };
                Some((stem, entries))
            })
    })
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
