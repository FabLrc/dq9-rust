//! `dq9re state watch`: translate the per-frame watch log of the libretro host
//! (`scripts/melondsds_host.py --watch ADDR:LEN --watch-out FILE`) into game fields.
//!
//! The host writes one JSON object per changed byte:
//! `{"frame":N,"addr":"0x...","old":0,"new":1}`. An address inside the event block
//! (`g_QuestManager`, [`QUEST_MANAGER`]) is named as the track progress or flag bit, the
//! global flag, the quest nibble or the current-track byte it changes; anything else is
//! reported as a raw address. This is the read side of the memory tracer: a flag that
//! appears at a frame can be tied to the scene or the input of that frame without another
//! emulator run.
//!
//! The block layout is the save block one (`docs/re/save.md`, § « Bloc d'événements »):
//! five tracks of `0x1c` bytes (chapter, step, substep, 13 chapter-flag bytes, 12
//! step-flag bytes), `0x234` global-flag bytes, `0x66` quest-nibble bytes and the current
//! track at `+0x332`.

use std::fs;
use std::io::{BufRead, BufReader, Write as _};
use std::path::Path;

use anyhow::{Context as _, Result};
use dq9_formats::melonds::QUEST_MANAGER;
use dq9_formats::save::EVENT_FLAGS_SIZE;
use serde::{Deserialize, Serialize};

const TRACK_COUNT: usize = 5;
const TRACK_SIZE: usize = 0x1c;
const GLOBAL_OFFSET: usize = TRACK_COUNT * TRACK_SIZE;
const GLOBAL_SIZE: usize = 0x234;
const QUEST_OFFSET: usize = 0x2cc;
const SELECTOR_OFFSET: usize = 0x332;

/// One line of the host watch log.
#[derive(Debug, Deserialize)]
struct RawChange {
    frame: u64,
    addr: String,
    old: u8,
    new: u8,
}

/// A named change, as printed and serialized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A bit of the global flag array (`Flags_Set`, `Flags_Test`).
    GlobalFlag {
        frame: u64,
        flag: i32,
        old: bool,
        new: bool,
    },
    /// A bit of one of the two local flag sets of a track.
    TrackFlags {
        frame: u64,
        track: usize,
        set: &'static str,
        flag: i32,
        old: bool,
        new: bool,
    },
    /// Chapter, step or substep of a track.
    TrackProgress {
        frame: u64,
        track: usize,
        field: &'static str,
        old: u8,
        new: u8,
    },
    /// One nibble of the quest table.
    Quest {
        frame: u64,
        quest: i32,
        old: u8,
        new: u8,
    },
    /// The current-track selector (`+0x332`).
    CurrentTrack { frame: u64, old: u8, new: u8 },
    /// A watched byte outside the event block (timers, heap, VBlank counter).
    Ram {
        frame: u64,
        addr: u32,
        old: u8,
        new: u8,
    },
}

/// Parses `0x...` (or bare hexadecimal) into an address.
fn address(text: &str) -> Result<u32> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u32::from_str_radix(digits, 16)
        .with_context(|| format!("watch address {text:?} is not hexadecimal"))
}

/// Bits that differ between `old` and `new`, least significant first.
fn changed_bits(old: u8, new: u8) -> impl Iterator<Item = u32> {
    let mask = old ^ new;
    (0..8).filter(move |bit| mask & (1 << bit) != 0)
}

fn bit(old: u8, new: u8, bit: u32) -> (bool, bool) {
    ((old >> bit) & 1 != 0, (new >> bit) & 1 != 0)
}

/// Names one byte change at `offset` inside the event block. An offset outside the block is
/// not handled here.
fn name_in_block(frame: u64, offset: usize, old: u8, new: u8) -> Vec<Change> {
    if offset < GLOBAL_OFFSET {
        let track = offset / TRACK_SIZE;
        let within = offset % TRACK_SIZE;
        return match within {
            0..=2 => vec![Change::TrackProgress {
                frame,
                track,
                field: ["chapter", "step", "substep"][within],
                old,
                new,
            }],
            3..=15 => changed_bits(old, new)
                .map(|index| {
                    let (old, new) = bit(old, new, index);
                    Change::TrackFlags {
                        frame,
                        track,
                        set: "chapter_flags",
                        flag: (within as i32 - 3) * 8 + index as i32,
                        old,
                        new,
                    }
                })
                .collect(),
            16..=27 => changed_bits(old, new)
                .map(|index| {
                    let (old, new) = bit(old, new, index);
                    Change::TrackFlags {
                        frame,
                        track,
                        set: "step_flags",
                        flag: (within as i32 - 16) * 8 + index as i32,
                        old,
                        new,
                    }
                })
                .collect(),
            _ => vec![Change::Ram {
                frame,
                addr: (QUEST_MANAGER as usize + offset) as u32,
                old,
                new,
            }],
        };
    }
    if offset < GLOBAL_OFFSET + GLOBAL_SIZE {
        let base = offset - GLOBAL_OFFSET;
        return changed_bits(old, new)
            .map(|index| {
                let (old, new) = bit(old, new, index);
                Change::GlobalFlag {
                    frame,
                    flag: (base as i32) * 8 + index as i32,
                    old,
                    new,
                }
            })
            .collect();
    }
    if (QUEST_OFFSET..SELECTOR_OFFSET).contains(&offset) {
        let base = offset - QUEST_OFFSET;
        let mut out = Vec::new();
        for nibble in 0..2 {
            let shift = nibble * 4;
            let a = (old >> shift) & 0xf;
            let b = (new >> shift) & 0xf;
            if a != b {
                out.push(Change::Quest {
                    frame,
                    quest: (base as i32) * 2 + nibble,
                    old: a,
                    new: b,
                });
            }
        }
        return out;
    }
    if offset == SELECTOR_OFFSET {
        return vec![Change::CurrentTrack { frame, old, new }];
    }
    vec![Change::Ram {
        frame,
        addr: (QUEST_MANAGER as usize + offset) as u32,
        old,
        new,
    }]
}

/// Names a change at any address (inside the event block or not).
fn name(frame: u64, addr: u32, old: u8, new: u8) -> Vec<Change> {
    let base = QUEST_MANAGER;
    if addr >= base && addr < base + EVENT_FLAGS_SIZE as u32 {
        return name_in_block(frame, (addr - base) as usize, old, new);
    }
    vec![Change::Ram {
        frame,
        addr,
        old,
        new,
    }]
}

fn print_text(change: &Change) {
    match change {
        Change::GlobalFlag {
            frame,
            flag,
            old,
            new,
        } => println!(
            "frame {frame}: global flag {flag}: {} -> {}",
            u8::from(*old),
            u8::from(*new)
        ),
        Change::TrackFlags {
            frame,
            track,
            set,
            flag,
            old,
            new,
        } => println!(
            "frame {frame}: track {track} {set} flag {flag}: {} -> {}",
            u8::from(*old),
            u8::from(*new)
        ),
        Change::TrackProgress {
            frame,
            track,
            field,
            old,
            new,
        } => println!("frame {frame}: track {track} {field}: {old} -> {new}"),
        Change::Quest {
            frame,
            quest,
            old,
            new,
        } => println!("frame {frame}: quest {quest}: {old} -> {new}"),
        Change::CurrentTrack { frame, old, new } => {
            println!("frame {frame}: current track: {old} -> {new}")
        }
        Change::Ram {
            frame,
            addr,
            old,
            new,
        } => println!("frame {frame}: ram {addr:#010x}: {old:#04x} -> {new:#04x}"),
    }
}

/// Reads the host watch JSONL, names each change and prints it (or a JSON array).
pub fn run(path: &Path, json: bool) -> Result<()> {
    let file = fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut changes = Vec::new();
    for (number, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.with_context(|| format!("reading line {} of {}", number + 1, path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let raw: RawChange = serde_json::from_str(&line).with_context(|| {
            format!(
                "line {} of {} is not a watch object",
                number + 1,
                path.display()
            )
        })?;
        let addr = address(&raw.addr)?;
        changes.extend(name(raw.frame, addr, raw.old, raw.new));
    }
    if json {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        serde_json::to_writer_pretty(&mut out, &changes).context("writing the JSON array")?;
        writeln!(out).context("writing the JSON array")?;
    } else {
        for change in &changes {
            print_text(change);
        }
        println!("{} change(s)", changes.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(offset: usize) -> u32 {
        QUEST_MANAGER + offset as u32
    }

    #[test]
    fn global_flag_bits_are_named() {
        let changes = name(7, base(GLOBAL_OFFSET), 0, 0b101);
        assert_eq!(
            changes,
            vec![
                Change::GlobalFlag {
                    frame: 7,
                    flag: 0,
                    old: false,
                    new: true
                },
                Change::GlobalFlag {
                    frame: 7,
                    flag: 2,
                    old: false,
                    new: true
                },
            ]
        );
        let changes = name(8, base(GLOBAL_OFFSET + 1), 0xff, 0xfe);
        assert_eq!(
            changes,
            vec![Change::GlobalFlag {
                frame: 8,
                flag: 8,
                old: true,
                new: false
            }]
        );
    }

    #[test]
    fn track_progress_and_flags_are_named() {
        let track = 1;
        let changes = name(3, base(track * TRACK_SIZE), 1, 2);
        assert_eq!(
            changes,
            vec![Change::TrackProgress {
                frame: 3,
                track,
                field: "chapter",
                old: 1,
                new: 2
            }]
        );
        let changes = name(4, base(track * TRACK_SIZE + 16), 0, 0b10);
        assert_eq!(
            changes,
            vec![Change::TrackFlags {
                frame: 4,
                track,
                set: "step_flags",
                flag: 1,
                old: false,
                new: true
            }]
        );
        let changes = name(5, base(track * TRACK_SIZE + 3 + 1), 0, 0b1000);
        assert_eq!(
            changes,
            vec![Change::TrackFlags {
                frame: 5,
                track,
                set: "chapter_flags",
                flag: 11,
                old: false,
                new: true
            }]
        );
    }

    #[test]
    fn quests_and_current_track_are_named() {
        let changes = name(9, base(QUEST_OFFSET), 0x12, 0x34);
        assert_eq!(
            changes,
            vec![
                Change::Quest {
                    frame: 9,
                    quest: 0,
                    old: 2,
                    new: 4
                },
                Change::Quest {
                    frame: 9,
                    quest: 1,
                    old: 1,
                    new: 3
                },
            ]
        );
        let changes = name(10, base(SELECTOR_OFFSET), 0, 2);
        assert_eq!(
            changes,
            vec![Change::CurrentTrack {
                frame: 10,
                old: 0,
                new: 2
            }]
        );
    }

    #[test]
    fn outside_the_block_is_ram() {
        let changes = name(11, 0x027f_fc3c, 0, 1);
        assert_eq!(
            changes,
            vec![Change::Ram {
                frame: 11,
                addr: 0x027f_fc3c,
                old: 0,
                new: 1
            }]
        );
    }

    #[test]
    fn address_rejects_garbage() {
        assert!(address("0xzz").is_err());
        assert_eq!(address("0x02108844").unwrap(), QUEST_MANAGER);
        assert_eq!(address("2108844").unwrap(), QUEST_MANAGER);
    }
}
