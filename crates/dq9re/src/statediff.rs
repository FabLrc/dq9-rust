//! `dq9re state diff`: compare two oracle states, a melonDS savestate (`.ml1`) or a 4 MiB
//! main-RAM image (`docs/re/melonds-state.md`, § "Diff of two states").
//!
//! The game-level fields come first: map and area, current track and story point, hero pose,
//! the five tracks (progress and both flag sets), global flags and quests. Everything else is
//! summarized as main-RAM ranges (start, length, number of changed bytes); the event block is
//! left out because its changes are already named. Transient bytes (VBlank counter, timers,
//! heap) therefore never turn an identical checkpoint into a failure: the exit status follows
//! the named fields only.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, bail};
use dq9_formats::melonds::{
    Checkpoint, MAIN_RAM_BASE, MAIN_RAM_SIZE, Memory, QUEST_MANAGER, State,
};
use dq9_formats::save::EVENT_FLAGS_SIZE;
use serde::Serialize;

/// Equal bytes that still merge two changed runs into one range: a structure mostly
/// unchanged reads as one range instead of dozens.
const MERGE_GAP: usize = 16;
/// Ranges printed in text mode; `--json` keeps them all.
const TEXT_RANGES: usize = 200;

/// Which of the two local flag sets a track difference belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagSet {
    /// Set 0, cleared when the chapter changes.
    Chapter,
    /// Set 1, cleared when the step changes.
    Step,
}

/// Hero pose in world units (`1 / 4096` of the raw `fx32`), as `dq9re state --json` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Hero {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
}

impl Hero {
    fn from_raw(raw: RawHero) -> Self {
        let scale = |v: i32| round6(f64::from(v) / 4096.0);
        Self {
            x: scale(raw.x),
            y: scale(raw.y),
            z: scale(raw.z),
            yaw: scale(raw.yaw),
        }
    }
}

fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

/// The raw `Object3D` pose, compared exactly (one `fx32` unit is about 0.00024 world unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawHero {
    x: i32,
    y: i32,
    z: i32,
    yaw: i32,
}

/// One game-level field that differs between the two states.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "field", content = "change", rename_all = "snake_case")]
pub enum Difference {
    Map {
        a: u16,
        b: u16,
    },
    Area {
        a: u16,
        b: u16,
    },
    CurrentTrack {
        a: usize,
        b: usize,
    },
    /// `chapter.step.substep` of the current track.
    Story {
        a: (u8, u8, u8),
        b: (u8, u8, u8),
    },
    Daytime {
        a: bool,
        b: bool,
    },
    Hero {
        a: Option<Hero>,
        b: Option<Hero>,
    },
    TrackProgress {
        track: usize,
        a: (u8, u8, u8),
        b: (u8, u8, u8),
    },
    TrackFlags {
        track: usize,
        set: FlagSet,
        /// Bits set only in B.
        added: Vec<i32>,
        /// Bits set only in A.
        removed: Vec<i32>,
    },
    GlobalFlags {
        /// Flags set only in B.
        added: Vec<i32>,
        /// Flags set only in A.
        removed: Vec<i32>,
    },
    Quest {
        quest: i32,
        a: u8,
        b: u8,
    },
}

/// A run of main-RAM differences, padded with up to [`MERGE_GAP`] equal bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedRange {
    /// Absolute ARM9 address of the first byte of the range.
    pub start: u32,
    /// Length of the whole range, equal bytes included.
    pub len: usize,
    /// Number of bytes that differ inside the range.
    pub changed: usize,
}

/// The whole comparison, printed as text or as `--json`.
#[derive(Debug, Serialize)]
pub struct Report {
    pub a: String,
    pub b: String,
    /// `savestate` or `ram`.
    pub a_kind: &'static str,
    pub b_kind: &'static str,
    /// True when every game-level field matches, even if transient bytes differ.
    pub identical: bool,
    pub differences: Vec<Difference>,
    /// Main-RAM bytes that differ outside the event block.
    pub changed_bytes: usize,
    pub ranges: Vec<ChangedRange>,
}

/// Compares `a` and `b`, prints the report and returns whether their checkpoints match.
pub fn run(a: &Path, b: &Path, json: bool) -> Result<bool> {
    let a_bytes = fs::read(a).with_context(|| format!("reading {}", a.display()))?;
    let b_bytes = fs::read(b).with_context(|| format!("reading {}", b.display()))?;
    let (a_memory, a_kind) = load(&a_bytes).with_context(|| format!("reading {}", a.display()))?;
    let (b_memory, b_kind) = load(&b_bytes).with_context(|| format!("reading {}", b.display()))?;
    let report = compare(
        a.display().to_string(),
        b.display().to_string(),
        a_kind,
        b_kind,
        &a_memory,
        &b_memory,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(report.identical)
}

/// Opens either container; a file that is neither is an error, never a guess.
fn load(bytes: &[u8]) -> Result<(Memory<'_>, &'static str)> {
    if bytes.starts_with(b"MELN") {
        let state = State::parse(bytes).context("savestate")?;
        Ok((state.memory(), "savestate"))
    } else if bytes.len() == MAIN_RAM_SIZE {
        Ok((Memory::from_main_ram(bytes)?, "ram"))
    } else {
        bail!(
            "neither a MELN savestate nor a 4 MiB main-RAM image ({} bytes)",
            bytes.len()
        )
    }
}

fn compare(
    a: String,
    b: String,
    a_kind: &'static str,
    b_kind: &'static str,
    a_memory: &Memory,
    b_memory: &Memory,
) -> Result<Report> {
    let ca = a_memory.checkpoint()?;
    let cb = b_memory.checkpoint()?;
    let mut differences = Vec::new();
    if ca.map != cb.map {
        differences.push(Difference::Map {
            a: ca.map,
            b: cb.map,
        });
    }
    if ca.area != cb.area {
        differences.push(Difference::Area {
            a: ca.area,
            b: cb.area,
        });
    }
    if ca.current_track != cb.current_track {
        differences.push(Difference::CurrentTrack {
            a: ca.current_track,
            b: cb.current_track,
        });
    }
    let story_a = ca.scenario().unwrap_or_default();
    let story_b = cb.scenario().unwrap_or_default();
    if story_a != story_b {
        differences.push(Difference::Story {
            a: story_a,
            b: story_b,
        });
    }
    if ca.daytime != cb.daytime {
        differences.push(Difference::Daytime {
            a: ca.daytime,
            b: cb.daytime,
        });
    }
    let hero_a = raw_hero(a_memory)?;
    let hero_b = raw_hero(b_memory)?;
    if hero_a != hero_b {
        differences.push(Difference::Hero {
            a: hero_a.map(Hero::from_raw),
            b: hero_b.map(Hero::from_raw),
        });
    }
    for (track, (ta, tb)) in ca.tracks.iter().zip(&cb.tracks).enumerate() {
        let progress_a = (ta.chapter, ta.step, ta.substep);
        let progress_b = (tb.chapter, tb.step, tb.substep);
        if progress_a != progress_b {
            differences.push(Difference::TrackProgress {
                track,
                a: progress_a,
                b: progress_b,
            });
        }
        for (set, fa, fb) in [
            (FlagSet::Chapter, &ta.chapter_flags, &tb.chapter_flags),
            (FlagSet::Step, &ta.step_flags, &tb.step_flags),
        ] {
            let (added, removed) = set_diff(fa, fb);
            if !added.is_empty() || !removed.is_empty() {
                differences.push(Difference::TrackFlags {
                    track,
                    set,
                    added,
                    removed,
                });
            }
        }
    }
    let (added, removed) = set_diff(&ca.flags, &cb.flags);
    if !added.is_empty() || !removed.is_empty() {
        differences.push(Difference::GlobalFlags { added, removed });
    }
    for (quest, a, b) in quest_changes(&ca, &cb) {
        differences.push(Difference::Quest { quest, a, b });
    }
    let skip = (QUEST_MANAGER - MAIN_RAM_BASE) as usize
        ..(QUEST_MANAGER - MAIN_RAM_BASE) as usize + EVENT_FLAGS_SIZE;
    let (changed_bytes, ranges) = changed_ranges(a_memory.main_ram(), b_memory.main_ram(), skip);
    Ok(Report {
        a,
        b,
        a_kind,
        b_kind,
        identical: differences.is_empty(),
        differences,
        changed_bytes,
        ranges,
    })
}

/// Hero pose as the game stores it (`Object3D+0x44`, yaw at `+0x54`).
fn raw_hero(memory: &Memory) -> Result<Option<RawHero>> {
    let index = memory.protagonist_index()?;
    let Some(at) = memory.object(index)? else {
        return Ok(None);
    };
    let p = memory.object_position(at)?;
    Ok(Some(RawHero {
        x: p[0],
        y: p[1],
        z: p[2],
        yaw: memory.object_heading(at)?,
    }))
}

/// `(added, removed)`, both sorted; `added` is set only in `b`.
fn set_diff(a: &[i32], b: &[i32]) -> (Vec<i32>, Vec<i32>) {
    let a: BTreeSet<i32> = a.iter().copied().collect();
    let b: BTreeSet<i32> = b.iter().copied().collect();
    (
        b.difference(&a).copied().collect(),
        a.difference(&b).copied().collect(),
    )
}

/// Every quest whose nibble differs, missing quests read as 0.
fn quest_changes(a: &Checkpoint, b: &Checkpoint) -> Vec<(i32, u8, u8)> {
    let a: BTreeMap<i32, u8> = a.quests.iter().copied().collect();
    let b: BTreeMap<i32, u8> = b.quests.iter().copied().collect();
    a.keys()
        .chain(b.keys())
        .copied()
        .collect::<BTreeSet<i32>>()
        .into_iter()
        .filter_map(|quest| {
            let va = a.get(&quest).copied().unwrap_or(0);
            let vb = b.get(&quest).copied().unwrap_or(0);
            (va != vb).then_some((quest, va, vb))
        })
        .collect()
}

/// Main-RAM differences outside `skip`, merged when separated by at most [`MERGE_GAP`] equal
/// bytes; returns the exact number of changed bytes and the ranges in address order.
fn changed_ranges(a: &[u8], b: &[u8], skip: Range<usize>) -> (usize, Vec<ChangedRange>) {
    let len = a.len().min(b.len());
    let mut ranges = Vec::new();
    let mut total = 0;
    let mut at = 0;
    while at < len {
        if skip.contains(&at) || a[at] == b[at] {
            at += 1;
            continue;
        }
        let start = at;
        let mut last = at;
        let mut changed = 0;
        while at < len {
            if skip.contains(&at) {
                at += 1;
            } else if a[at] != b[at] {
                last = at;
                changed += 1;
                at += 1;
            } else if at - last > MERGE_GAP {
                break;
            } else {
                at += 1;
            }
        }
        total += changed;
        ranges.push(ChangedRange {
            start: MAIN_RAM_BASE + start as u32,
            len: last - start + 1,
            changed,
        });
    }
    (total, ranges)
}

fn print_report(report: &Report) {
    println!(
        "{} ({}) -> {} ({})",
        report.a, report.a_kind, report.b, report.b_kind
    );
    if report.differences.is_empty() {
        println!("game state: identical");
    } else {
        println!("game state: {} difference(s)", report.differences.len());
        for difference in &report.differences {
            println!("  {}", describe(difference));
        }
    }
    println!(
        "other main RAM: {} changed byte(s) in {} range(s) outside the event block",
        report.changed_bytes,
        report.ranges.len()
    );
    for range in report.ranges.iter().take(TEXT_RANGES) {
        println!(
            "  {:#010x} +{:<6} ({} changed)",
            range.start, range.len, range.changed
        );
    }
    if report.ranges.len() > TEXT_RANGES {
        println!(
            "  ... {} more range(s), use --json",
            report.ranges.len() - TEXT_RANGES
        );
    }
}

fn describe(difference: &Difference) -> String {
    match difference {
        Difference::Map { a, b } => format!("map {a} -> {b}"),
        Difference::Area { a, b } => format!("area {a} -> {b}"),
        Difference::CurrentTrack { a, b } => format!("current track {a} -> {b}"),
        Difference::Story { a, b } => {
            format!("story {}.{}.{} -> {}.{}.{}", a.0, a.1, a.2, b.0, b.1, b.2)
        }
        Difference::Daytime { a, b } => format!("daytime {a} -> {b}"),
        Difference::Hero { a, b } => format!("hero {} -> {}", show_hero(*a), show_hero(*b)),
        Difference::TrackProgress { track, a, b } => {
            format!(
                "track {track} progress {}.{}.{} -> {}.{}.{}",
                a.0, a.1, a.2, b.0, b.1, b.2
            )
        }
        Difference::TrackFlags {
            track,
            set,
            added,
            removed,
        } => {
            let set = match set {
                FlagSet::Chapter => "chapter_flags",
                FlagSet::Step => "step_flags",
            };
            format!("track {track} {set} added {added:?} removed {removed:?}")
        }
        Difference::GlobalFlags { added, removed } => {
            format!("global flags added {added:?} removed {removed:?}")
        }
        Difference::Quest { quest, a, b } => format!("quest {quest}: {a} -> {b}"),
    }
}

fn show_hero(hero: Option<Hero>) -> String {
    match hero {
        Some(h) => format!("({:.3}, {:.3}, {:.3}) yaw {:.3}", h.x, h.y, h.z, h.yaw),
        None => "absent".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::melonds::{GAME_STATE, VBLANK_COUNT, ZONE_3D};

    const BLOCK: usize = (QUEST_MANAGER - MAIN_RAM_BASE) as usize;
    const HERO_OBJECT: u32 = 0x0210_0000;

    fn put_u16(ram: &mut [u8], at: usize, value: u16) {
        ram[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(ram: &mut [u8], at: usize, value: u32) {
        ram[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// A 4 MiB main-RAM image at the first Observatory control: map 4105 area 4100, five
    /// tracks at 1.1.1, track 0 step flag 1, global flag 337, quest 12 at 1, hero at the
    /// known position and heading.
    fn ram_image() -> Vec<u8> {
        let mut ram = vec![0u8; MAIN_RAM_SIZE];
        put_u16(&mut ram, (ZONE_3D - MAIN_RAM_BASE) as usize, 4105);
        put_u16(&mut ram, (ZONE_3D - MAIN_RAM_BASE) as usize + 4, 4100);
        for track in 0..5 {
            ram[BLOCK + track * 0x1c] = 1;
            ram[BLOCK + track * 0x1c + 1] = 1;
            ram[BLOCK + track * 0x1c + 2] = 1;
        }
        ram[BLOCK + 0x10] = 1; // track 0 step flag 0
        ram[BLOCK + 0x8c + 337 / 8] |= 1 << (337 % 8);
        ram[BLOCK + 0x2cc + 12 / 2] = 1; // quest 12, low nibble
        put_u32(
            &mut ram,
            (GAME_STATE - MAIN_RAM_BASE) as usize + 8,
            HERO_OBJECT,
        );
        let object = HERO_OBJECT as usize - 0x0200_0000;
        put_u32(&mut ram, object + 0x44, 2703); // 0.659912...
        put_u32(&mut ram, object + 0x48, (-1024i32) as u32); // -0.25
        put_u32(&mut ram, object + 0x4c, (-14786i32) as u32); // -3.609863...
        put_u32(&mut ram, object + 0x54, 19906); // 4.859863...
        ram
    }

    fn memory(ram: &[u8]) -> Memory<'_> {
        Memory::from_main_ram(ram).expect("4 MiB image")
    }

    fn diff(a: &[u8], b: &[u8]) -> Report {
        compare(
            "a".to_owned(),
            "b".to_owned(),
            "ram",
            "ram",
            &memory(a),
            &memory(b),
        )
        .expect("checkpoint")
    }

    #[test]
    fn identical_images_have_no_difference() {
        let report = diff(&ram_image(), &ram_image());
        assert!(report.identical);
        assert!(report.differences.is_empty());
        assert_eq!(report.changed_bytes, 0);
        assert!(report.ranges.is_empty());
    }

    /// The VBlank counter differs between two captures of the same game moment; it is not a
    /// game-level field, so the checkpoint stays identical and only the range summary sees it.
    #[test]
    fn transient_bytes_do_not_break_identity() {
        let a = ram_image();
        let mut b = ram_image();
        // Main RAM is mirrored every 4 MiB: `0x027ffc3c` is offset `0x3ffc3c` of the dump.
        let at = (VBLANK_COUNT & 0x3f_ffff) as usize;
        put_u32(&mut b, at, 12345);
        let report = diff(&a, &b);
        assert!(report.identical);
        assert!(report.differences.is_empty());
        assert_eq!(report.changed_bytes, 2);
        assert_eq!(report.ranges.len(), 1);
        assert_eq!(report.ranges[0].start, MAIN_RAM_BASE + at as u32);
        assert_eq!(report.ranges[0].len, 2);
        assert_eq!(report.ranges[0].changed, 2);
    }

    #[test]
    fn detects_flag_pose_progress_and_quest_changes() {
        let a = ram_image();
        let mut b = ram_image();
        // Global flags: clear 337, add 1450.
        ram_clear_global(&mut b, 337);
        ram_set_global(&mut b, 1450);
        // Track 0: step flag 0 cleared, step flag 1 set; substep 1 -> 2.
        b[BLOCK + 0x10] = 2;
        b[BLOCK + 2] = 2;
        // Quest 12 1 -> 2.
        b[BLOCK + 0x2cc + 12 / 2] = 2;
        // Hero moves by one world unit and turns.
        let object = HERO_OBJECT as usize - 0x0200_0000;
        put_u32(&mut b, object + 0x44, 2703 + 4096);
        put_u32(&mut b, object + 0x54, 0);

        let report = diff(&a, &b);
        assert!(!report.identical);
        assert_eq!(
            report.differences,
            vec![
                Difference::Story {
                    a: (1, 1, 1),
                    b: (1, 1, 2),
                },
                Difference::Hero {
                    a: Some(Hero {
                        x: 0.659912,
                        y: -0.25,
                        z: -3.609863,
                        yaw: 4.859863,
                    }),
                    b: Some(Hero {
                        x: 1.659912,
                        y: -0.25,
                        z: -3.609863,
                        yaw: 0.0,
                    }),
                },
                Difference::TrackProgress {
                    track: 0,
                    a: (1, 1, 1),
                    b: (1, 1, 2),
                },
                Difference::TrackFlags {
                    track: 0,
                    set: FlagSet::Step,
                    added: vec![1],
                    removed: vec![0],
                },
                Difference::GlobalFlags {
                    added: vec![1450],
                    removed: vec![337],
                },
                Difference::Quest {
                    quest: 12,
                    a: 1,
                    b: 2,
                },
            ]
        );
        // The event block is fully described above, so no range points into it.
        assert!(report.changed_bytes > 0);
        let block = QUEST_MANAGER..QUEST_MANAGER + EVENT_FLAGS_SIZE as u32;
        assert!(
            report.ranges.iter().all(
                |r| !block.contains(&r.start) && !block.contains(&(r.start + r.len as u32 - 1))
            )
        );
    }

    #[test]
    fn detects_map_and_current_track_changes() {
        let a = ram_image();
        let mut b = ram_image();
        put_u16(&mut b, (ZONE_3D - MAIN_RAM_BASE) as usize, 1112);
        put_u16(&mut b, (ZONE_3D - MAIN_RAM_BASE) as usize + 4, 1100);
        b[BLOCK + 0x332] = 2;
        let report = diff(&a, &b);
        assert_eq!(
            report.differences,
            vec![
                Difference::Map { a: 4105, b: 1112 },
                Difference::Area { a: 4100, b: 1100 },
                Difference::CurrentTrack { a: 0, b: 2 },
            ]
        );
    }

    #[test]
    fn ranges_merge_small_gaps_and_keep_large_ones_apart() {
        let a = vec![0u8; 256];
        let mut b = a.clone();
        b[10] = 1;
        b[20] = 1;
        b[100] = 1;
        let (changed, ranges) = changed_ranges(&a, &b, 0..0);
        assert_eq!(changed, 3);
        assert_eq!(ranges.len(), 2);
        assert_eq!(
            ranges[0],
            ChangedRange {
                start: MAIN_RAM_BASE + 10,
                len: 11,
                changed: 2,
            }
        );
        assert_eq!(
            ranges[1],
            ChangedRange {
                start: MAIN_RAM_BASE + 100,
                len: 1,
                changed: 1,
            }
        );
    }

    #[test]
    fn invalid_inputs_are_rejected_without_panicking() {
        assert!(load(&[]).is_err());
        assert!(load(&[0u8; 3]).is_err());
        assert!(load(b"MELN").is_err());
        // A 4 MiB image is valid, whatever its content.
        assert!(load(&vec![0u8; MAIN_RAM_SIZE]).is_ok());
        assert!(load(&vec![0u8; MAIN_RAM_SIZE - 1]).is_err());
    }

    fn ram_set_global(ram: &mut [u8], flag: usize) {
        ram[BLOCK + 0x8c + flag / 8] |= 1 << (flag % 8);
    }

    fn ram_clear_global(ram: &mut [u8], flag: usize) {
        ram[BLOCK + 0x8c + flag / 8] &= !(1 << (flag % 8));
    }
}
