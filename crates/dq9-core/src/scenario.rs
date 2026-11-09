//! The story's flow from one scene to the next, and its journal. What chains the scenes is the
//! game's own mechanisms, already modelled elsewhere: the end of an event fires trigger event
//! 11 ([`crate::field::Field::event_ended`]), whose actions start the next event (0x77), move the
//! story forward (0x84), change the map (0x79…) or start a scripted battle (0x78,
//! [`crate::trigger::Request::Battle`]); the end of that battle fires trigger event 15 or 16
//! ([`crate::field::Field::battle_ended`]). Evidence in `docs/re/trigger.md`, § "Scripted
//! battles" and `docs/re/stb.md`, § "Prologue chaining".
//!
//! This module holds what is left: the record of a scripted battle ([`BattleRecord`]), the
//! stand-in for the battle system until J4 ([`stand_in`]), and the [`Journal`] the headless
//! report prints.

use std::fmt;

use dq9_formats::table::{Table, Value};

use crate::script::load::Files;
use crate::trigger::{State, TRACKS};

/// `data/event/eventbattle.bin`: one record per scripted battle (`docs/re/trigger.md`).
pub const BATTLE_LIST: &str = "data/event/eventbattle.bin";

/// A scripted battle as `eventbattle.bin` lists it: the record whose first value is the number
/// action 0x78 names. Values: `[number, monster, count, monster, count, monster, count, track,
/// stage]`; an empty slot has monster `0xffffffff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleRecord {
    pub number: u32,
    /// `(monster number, count)` of the filled slots (`monster.bin` order).
    pub monsters: Vec<(u32, u32)>,
    /// Battle music (wiki, EU **[hypothesis]**).
    pub track: u32,
    /// Map of the arena, 0 for the one the ground names (wiki, EU **[hypothesis]**).
    pub stage: u32,
}

/// The record of scripted battle `number`.
pub fn battle(files: &dyn Files, number: u16) -> Option<BattleRecord> {
    let table = Table::parse(files.file(BATTLE_LIST)?).ok()?;
    table.records.iter().find_map(|record| {
        let v = record.values.get(..9)?;
        let int = |i: usize| match v[i] {
            Value::Int(n) => Some(n),
            _ => None,
        };
        if int(0)? != u32::from(number) {
            return None;
        }
        let monsters = (0..3)
            .filter_map(|slot| {
                let monster = int(1 + 2 * slot)?;
                (monster != u32::MAX).then_some((monster, int(2 + 2 * slot)?))
            })
            .collect();
        Some(BattleRecord {
            number: u32::from(number),
            monsters,
            track: int(7)?,
            stage: int(8)?,
        })
    })
}

/// How a scripted battle ends while there is no battle system (J4): the party wins at once.
/// The game would play the fight and then fire trigger event 15 (won) or 16 (lost); the story
/// continues exactly as after a win. A stand-in, not a model of a fight: it exists so that
/// the scenes that follow a scripted battle can be reached.
pub fn stand_in(_battle: u16) -> bool {
    true
}

/// One thing the story did, for the report of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A map was loaded (`id`, code).
    Map {
        id: u16,
        code: String,
    },
    /// The map changed through a door, a warp or a trigger.
    MapChange {
        from: Option<u16>,
        to: u16,
        cause: String,
    },
    EventStarted(u16),
    EventEnded(u16),
    /// A scripted battle was requested and settled by [`stand_in`].
    Battle {
        number: u16,
        record: Option<BattleRecord>,
        won: bool,
    },
    /// The progress of a scenario track moved.
    Progress {
        track: usize,
        from: (u8, u8, u8),
        to: (u8, u8, u8),
    },
    /// A trigger request nothing carries out.
    Unhandled(String),
    /// The player has control: no event runs and none waits.
    Control {
        map: u16,
        story: (u8, u8, u8),
    },
    /// What the autopilot of a headless run did at a free control (a conversation, a door).
    Autopilot(String),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let story = |(c, s, b): (u8, u8, u8)| format!("{c}.{s}.{b}");
        match self {
            Self::Map { id, code } => write!(f, "map {id} {code}"),
            Self::MapChange { from, to, cause } => match from {
                Some(from) => write!(f, "map change {from} -> {to} ({cause})"),
                None => write!(f, "map change -> {to} ({cause})"),
            },
            Self::EventStarted(event) => write!(f, "event {event} starts"),
            Self::EventEnded(event) => write!(f, "event {event} ends"),
            Self::Battle {
                number,
                record,
                won,
            } => {
                write!(f, "scripted battle {number}")?;
                if let Some(r) = record {
                    let monsters: Vec<_> =
                        r.monsters.iter().map(|(m, n)| format!("{m}x{n}")).collect();
                    write!(f, " [monsters {}, stage {}]", monsters.join(" "), r.stage)?;
                }
                write!(
                    f,
                    ": {} (stand-in, no battle system yet)",
                    if *won { "won" } else { "lost" }
                )
            }
            Self::Progress { track, from, to } => {
                write!(f, "story track {track}: {} -> {}", story(*from), story(*to))
            }
            Self::Unhandled(what) => write!(f, "not carried out: {what}"),
            Self::Control { map, story: s } => {
                write!(
                    f,
                    "the player has control on map {map} (story {})",
                    story(*s)
                )
            }
            Self::Autopilot(what) => write!(f, "autopilot: {what}"),
        }
    }
}

/// The steps of a run, each with the fixed step (1/60 s) it happened at.
#[derive(Debug, Default, Clone)]
pub struct Journal {
    pub steps: Vec<(u64, Step)>,
    seen: [(u8, u8, u8); TRACKS],
}

impl Journal {
    pub fn record(&mut self, tick: u64, step: Step) {
        self.steps.push((tick, step));
    }

    /// Notes the scenario tracks that moved since the last call.
    pub fn note_progress(&mut self, tick: u64, state: &State) {
        for (track, (seen, now)) in self.seen.iter_mut().zip(&state.tracks).enumerate() {
            let now = (now.chapter, now.step, now.substep);
            if *seen != now {
                // The first call only learns where the story starts.
                if *seen != (0, 0, 0) {
                    self.steps.push((
                        tick,
                        Step::Progress {
                            track,
                            from: *seen,
                            to: now,
                        },
                    ));
                }
                *seen = now;
            }
        }
    }

    /// The events that started, in order.
    pub fn events(&self) -> Vec<u16> {
        self.steps
            .iter()
            .filter_map(|(_, s)| match s {
                Step::EventStarted(e) => Some(*e),
                _ => None,
            })
            .collect()
    }
}

impl fmt::Display for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (tick, step) in &self.steps {
            writeln!(f, "{:>7.2}s  {step}", *tick as f64 / 60.0)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trigger::Track;

    #[test]
    fn progress_is_noted_once_the_start_is_known() {
        let mut state = State::default();
        state.tracks[0] = Track {
            chapter: 1,
            step: 1,
            substep: 1,
            ..Track::default()
        };
        let mut journal = Journal::default();
        journal.note_progress(0, &state);
        assert!(journal.steps.is_empty());
        state.tracks[0].step = 2;
        journal.note_progress(60, &state);
        journal.note_progress(61, &state);
        assert_eq!(
            journal.steps,
            [(
                60,
                Step::Progress {
                    track: 0,
                    from: (1, 1, 1),
                    to: (1, 2, 1)
                }
            )]
        );
    }

    #[test]
    fn a_battle_is_read_from_its_record() {
        struct One(Vec<u8>);
        impl Files for One {
            fn file(&self, path: &str) -> Option<&[u8]> {
                (path == BATTLE_LIST).then_some(&self.0)
            }
        }
        // One record `[26, 290, 1, 292, 1, empty, 0, 23, 30116]`, as the game's tables lay it out:
        // header word, type bytes, then the nine values.
        let mut data = Vec::new();
        for word in [1u32, 0x10 + 8 + 36, 0, 0] {
            data.extend(word.to_le_bytes());
        }
        data.extend([0x64, 0x00, 9, 0x55, 0x55, 0x01, 0xff, 0xff]);
        for value in [26u32, 290, 1, 292, 1, u32::MAX, 0, 23, 30116] {
            data.extend(value.to_le_bytes());
        }
        let record = battle(&One(data), 26).expect("record");
        assert_eq!(record.monsters, [(290, 1), (292, 1)]);
        assert_eq!((record.track, record.stage), (23, 30116));
    }
}
