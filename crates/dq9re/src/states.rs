//! `dq9re states`: catalogue of oracle states, so that « which state is at 1.1.2 ? » is
//! answered without loading every file (`docs/re/melonds-state.md`, § « Catalogue »).
//!
//! `index` scans a directory of `.ml1` savestates and raw 4 MiB main-RAM images (as
//! `scripts/melondsds_host.py --dump-ram` writes), reads them through
//! [`dq9_formats::melonds::Memory`] and writes one JSON entry per readable file; a file that
//! is neither is reported with its error instead of failing the scan. `find` filters an
//! index by story point and map.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dq9_formats::melonds::{Checkpoint, MAIN_RAM_SIZE, Memory, State};
use serde::{Deserialize, Serialize};

/// One readable state of the catalogue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// File name, relative to the scanned directory.
    pub file: String,
    /// `savestate` for a `.ml1`, `ram` for a raw main-RAM image.
    pub kind: String,
    pub vblank: u32,
    pub map: u16,
    pub area: u16,
    /// Index of the current scenario track.
    pub track: usize,
    /// `chapter.step.substep` of the current track.
    pub story: String,
    /// Number of global flags set.
    pub flags: usize,
    /// Protagonist world position and heading, when the object exists.
    pub hero: Option<Hero>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Hero {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
}

impl Hero {
    /// The hero as the game stores it (`Object3D::position_` and `+0x54`, fx32).
    fn read(memory: &Memory) -> Result<Option<Self>> {
        let index = memory.protagonist_index()?;
        let Some(at) = memory.object(index)? else {
            return Ok(None);
        };
        let p = memory.object_position(at)?;
        let yaw = memory.object_heading(at)?;
        let scale = |v: i32| f64::from(v) / 4096.0;
        Ok(Some(Self {
            x: scale(p[0]),
            y: scale(p[1]),
            z: scale(p[2]),
            yaw: scale(yaw),
        }))
    }
}

/// A file the scan could not read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Invalid {
    pub file: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Index {
    /// Directory the entries are relative to.
    pub root: String,
    pub entries: Vec<Entry>,
    /// Files that are neither a savestate nor a 4 MiB image, or are truncated.
    pub invalid: Vec<Invalid>,
}

impl Entry {
    /// Reads one state file; `None` when it is neither `.ml1` nor a RAM image.
    fn read(path: &Path, name: &str) -> Result<Option<Self>> {
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let (kind, memory) = if bytes.starts_with(b"MELN") {
            let state = State::parse(&bytes).context("savestate")?;
            ("savestate".to_owned(), state.memory())
        } else if bytes.len() == MAIN_RAM_SIZE {
            (
                "ram".to_owned(),
                Memory::from_main_ram(&bytes).context("main RAM image")?,
            )
        } else {
            return Ok(None);
        };
        let checkpoint = memory.checkpoint().context("event block")?;
        Ok(Some(Self::from_checkpoint(
            name,
            kind,
            &checkpoint,
            Hero::read(&memory)?,
        )))
    }

    fn from_checkpoint(
        file: &str,
        kind: String,
        checkpoint: &Checkpoint,
        hero: Option<Hero>,
    ) -> Self {
        let (chapter, step, substep) = checkpoint.scenario().unwrap_or((0, 0, 0));
        Self {
            file: file.to_owned(),
            kind,
            vblank: checkpoint.vblank,
            map: checkpoint.map,
            area: checkpoint.area,
            track: checkpoint.current_track,
            story: format!("{chapter}.{step}.{substep}"),
            flags: checkpoint.flags.len(),
            hero,
        }
    }
}

/// Scans `dir` and returns the catalogue; unreadable files land in [`Index::invalid`].
pub fn scan(dir: &Path) -> Result<Index> {
    let mut names: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("listing {}", dir.display()))?;
    names.sort();
    let mut index = Index {
        root: dir.display().to_string(),
        entries: Vec::new(),
        invalid: Vec::new(),
    };
    for path in names {
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match Entry::read(&path, &name) {
            Ok(Some(entry)) => index.entries.push(entry),
            Ok(None) => index.invalid.push(Invalid {
                file: name,
                error: "not a savestate or a 4 MiB main-RAM image".to_owned(),
            }),
            Err(error) => index.invalid.push(Invalid {
                file: name,
                error: format!("{error:#}"),
            }),
        }
    }
    Ok(index)
}

pub fn run_index(dir: &Path, out: Option<&Path>) -> Result<()> {
    let index = scan(dir)?;
    let json = serde_json::to_string_pretty(&index)?;
    match out {
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::write(path, json + "\n").with_context(|| format!("writing {}", path.display()))?;
            println!(
                "{} states, {} invalid, {} -> {}",
                index.entries.len(),
                index.invalid.len(),
                dir.display(),
                path.display()
            );
        }
        None => println!("{json}"),
    }
    Ok(())
}

/// `CHAPTER.STEP.SUBSTEP` as the story filter spells it.
pub fn parse_story(text: &str) -> Result<String> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 3 {
        bail!("expected CHAPTER.STEP.SUBSTEP, got {text:?}");
    }
    let mut out = Vec::new();
    for part in parts {
        match part.parse::<u8>() {
            Ok(v) => out.push(v.to_string()),
            Err(_) => bail!("bad story part {part:?} in {text:?}"),
        }
    }
    Ok(out.join("."))
}

/// Whether an entry passes the `find` filters.
fn matches(entry: &Entry, story: Option<&str>, map: Option<u16>) -> bool {
    !story.is_some_and(|s| s != entry.story) && !map.is_some_and(|m| m != entry.map)
}

pub fn run_find(index: &Path, story: Option<&str>, map: Option<u16>) -> Result<()> {
    let text = fs::read_to_string(index).with_context(|| format!("reading {}", index.display()))?;
    let index: Index =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", index.display()))?;
    let story = story.map(parse_story).transpose()?;
    let mut shown = 0;
    for entry in &index.entries {
        if !matches(entry, story.as_deref(), map) {
            continue;
        }
        shown += 1;
        let hero = entry.hero.map_or("-".to_owned(), |h| {
            format!("({:.3}, {:.3}, {:.3})", h.x, h.y, h.z)
        });
        println!(
            "{:<9} {:>9} map {:>5} area {:>5} {:>24} flags {:<3} vblank {:<8} {}",
            entry.story,
            entry.kind,
            entry.map,
            entry.area,
            hero,
            entry.flags,
            entry.vblank,
            entry.file
        );
    }
    if shown == 0 {
        println!("no state matches in {}", index.root);
    } else {
        println!("{shown} state(s) in {}", index.root);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::melonds::{GAME_STATE, QUEST_MANAGER, VBLANK_COUNT, ZONE_3D};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn story_filter_needs_three_bytes() {
        assert_eq!(parse_story("1.1.2").unwrap(), "1.1.2");
        assert_eq!(parse_story("1.10.0").unwrap(), "1.10.0");
        assert!(parse_story("1.1").is_err());
        assert!(parse_story("1.1.2.3").is_err());
        assert!(parse_story("1.x.2").is_err());
        assert!(parse_story("1.1.256").is_err());
    }

    /// A 4 MiB main-RAM image with just the story fields the catalogue reads, and a savestate
    /// wrapper around the same bytes (the catalogue must read both).
    fn ram_image() -> Vec<u8> {
        let mut ram = vec![0u8; MAIN_RAM_SIZE];
        let put = |ram: &mut Vec<u8>, addr: u32, bytes: &[u8]| {
            let o = (addr & 0x3f_ffff) as usize;
            ram[o..o + bytes.len()].copy_from_slice(bytes);
        };
        put(&mut ram, VBLANK_COUNT, &777u32.to_le_bytes());
        put(&mut ram, ZONE_3D, &4101u16.to_le_bytes());
        put(&mut ram, ZONE_3D + 4, &4100u16.to_le_bytes());
        put(&mut ram, QUEST_MANAGER, &[1, 1, 2]);
        put(&mut ram, QUEST_MANAGER + 0x8c, &[0b101]);
        put(&mut ram, GAME_STATE + 0x3ac, &0u32.to_le_bytes());
        put(&mut ram, GAME_STATE + 8, &0x0220_0000u32.to_le_bytes());
        put(
            &mut ram,
            0x0220_0000 + 0x44,
            &[
                0x00, 0x10, 0, 0, 0x00, 0xf0, 0xff, 0xff, 0x00, 0x00, 0x01, 0x00,
            ],
        );
        ram
    }

    fn savestate(main: &[u8]) -> Vec<u8> {
        let mut out = b"MELN".to_vec();
        out.resize(0x10, 0);
        let mut ndsg = vec![0u8; 0x14 - 0x10];
        ndsg.extend_from_slice(main);
        out.extend_from_slice(b"NDSG");
        out.extend_from_slice(&((ndsg.len() + 16) as u32).to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&ndsg);
        let mut cp15 = vec![0u8; 0x1c - 0x10];
        cp15.extend_from_slice(&vec![0u8; 0x8000 + 0x4000]);
        out.extend_from_slice(b"CP15");
        out.extend_from_slice(&((cp15.len() + 16) as u32).to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&cp15);
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dq9-states-{name}-{}-{stamp}", std::process::id()))
    }

    #[test]
    fn scan_reads_states_and_reports_invalid_files() {
        let dir = scratch("scan");
        fs::create_dir_all(&dir).unwrap();
        let ram = ram_image();
        fs::write(dir.join("control.bin"), &ram).unwrap();
        fs::write(dir.join("control.ml1"), savestate(&ram)).unwrap();
        fs::write(dir.join("notes.txt"), b"not a state").unwrap();
        fs::write(dir.join("truncated.ml1"), &savestate(&ram)[..0x100]).unwrap();
        let index = scan(&dir).unwrap();
        assert_eq!(index.entries.len(), 2);
        assert_eq!(index.invalid.len(), 2);
        let by_name = |name: &str| index.entries.iter().find(|e| e.file == name).unwrap();
        let bin = by_name("control.bin");
        assert_eq!(bin.kind, "ram");
        assert_eq!((bin.map, bin.area, bin.track), (4101, 4100, 0));
        assert_eq!(
            (bin.story.as_str(), bin.flags, bin.vblank),
            ("1.1.2", 2, 777)
        );
        assert_eq!(
            bin.hero,
            Some(Hero {
                x: 1.0,
                y: -1.0,
                z: 16.0,
                yaw: 0.0
            })
        );
        let state = by_name("control.ml1");
        assert_eq!(state.kind, "savestate");
        assert_eq!(state.hero, bin.hero);
        assert_eq!(state.story, bin.story);
        assert!(index.invalid.iter().any(|i| i.file == "notes.txt"));
        assert!(index.invalid.iter().any(|i| i.file == "truncated.ml1"));
        assert!(matches(bin, Some("1.1.2"), None));
        assert!(matches(bin, Some("1.1.2"), Some(4101)));
        assert!(!matches(bin, Some("1.1.1"), None));
        assert!(!matches(bin, None, Some(4102)));
        assert!(matches(bin, None, None));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn find_rejects_a_catalogue_that_is_not_json() {
        let dir = scratch("bad");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.json");
        fs::write(&path, b"not json").unwrap();
        assert!(run_find(&path, None, None).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
