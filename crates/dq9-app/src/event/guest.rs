//! The party's guest (the "attached NPC": Aquila, Ivor…): who it is and what it looks like,
//! for the scripts' cast mode 10 (`docs/re/characters.md`, § "The guest").
//!
//! The game keeps the guest outside the party list (character `0xce`, `ov_017::021c1e6c`). Two
//! trigger actions change it: `0xcd` (`Trig_RunAction` case `0x69`) looks up record `arg` of
//! `data/bin/attnpc.gp2` (`020d3d78`: by index, `0x24` bytes each) and starts the guest task
//! (`ov_017::021913d0`); `0xcc` (case `0x68`, `ov_017::02191484`) dismisses it. The trigger state
//! does not hold them, so the player replays them from the data when an event ends.

use dq9_formats::trigger::parse;
use dq9_formats::{Gpc2, Table, Value};
use nds_rom::Rom;

/// Trigger action `0xcc`: dismiss the guest.
const DISMISS: u16 = 0xcc;
/// Trigger action `0xcd`: the guest is record `arg` of the attached-NPC table.
const ADD: u16 = 0xcd;
/// Condition `0x8`: word 2 of the trigger parameters (the event that ended) equals `arg`.
const EVENT_IS: u16 = 0x8;
/// Trigger event 11: an event ended (`EventTask_Update`).
const EVENT_ENDED: i32 = 11;

/// What the end of one event does to the guest (the rule that answers trigger event 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rule {
    /// The event whose end triggers the rule.
    pub(super) event: u16,
    /// First `(chapter, step)` at which the rule is loaded.
    pub(super) from: (i32, i32),
    /// `None` dismisses the guest, `Some(record)` makes record `record` of the attached-NPC
    /// table the guest.
    pub(super) change: Option<u8>,
}

/// Every rule of the scenario triggers that changes the guest, in file order (the last change
/// of a rule wins).
pub(super) fn rules(rom: &[u8]) -> Vec<Rule> {
    let mut found = Vec::new();
    let Ok(rom) = Rom::parse(rom) else {
        return found;
    };
    for file in rom
        .files
        .files
        .iter()
        .filter(|f| f.path.starts_with("data/scenario/trigger") && f.path.ends_with(".bin"))
    {
        let Ok(data) = rom.file(file.id) else {
            continue;
        };
        let Ok(table) = Table::parse(data) else {
            continue;
        };
        let Ok(triggers) = parse(&table) else {
            continue;
        };
        for trigger in triggers.iter().filter(|t| t.event == EVENT_ENDED) {
            let Some(event) = trigger
                .conditions
                .iter()
                .find(|c| c.code == EVENT_IS)
                .map(|c| c.arg)
            else {
                continue;
            };
            let change = trigger.actions.iter().rev().find_map(|a| match a.code {
                DISMISS => Some(None),
                ADD => Some(u8::try_from(a.arg).ok()),
                _ => None,
            });
            if let Some(change) = change {
                found.push(Rule {
                    event,
                    from: trigger.from,
                    change,
                });
            }
        }
    }
    found
}

/// The guest a scene that plays at story step `at` starts with, for the report, which plays
/// each scene on its own: the one the latest rule loaded at or before `at` introduced, whatever
/// dismissed it since. The events of a step are not played in their number order (2430 to 2450
/// come before 2400, whose end dismisses Ivor) and nothing in the data orders them
/// **[hypothesis]**.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn introduced_by(rules: &[Rule], at: (i32, i32)) -> Option<u8> {
    rules
        .iter()
        .filter(|r| r.from <= at)
        .filter_map(|r| Some((r.from, r.event, r.change?)))
        .max()
        .map(|(_, _, record)| record)
}

/// The model of guest `record` (`ov_017::021c1e6c`: `data/chara_sub/s%03dn.chr`, the number in
/// field 1 of the record), relative to `data/`.
pub(super) fn model(rom: &[u8], record: u8) -> Option<String> {
    let rom = Rom::parse(rom).ok()?;
    let archive = Gpc2::parse(rom.file_by_path("data/bin/attnpc.gp2").ok()?).ok()?;
    let entry = archive.find("attnpc_en.bin")?;
    let data = archive.member(entry).ok()?;
    let table = Table::parse(&data).ok()?;
    let values = &table.records.get(usize::from(record))?.values;
    let Some(&Value::Int(number)) = values.get(1) else {
        return None;
    };
    Some(format!("chara_sub/s{number:03}n.chr"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ivor (record 1) is dressed with `s017n`, the guest the chapter 2 scenes talk with; the
    /// end of 2210 makes him the guest and the end of 22591 dismisses him.
    #[test]
    fn the_guest_of_chapter_2_is_ivor_from_the_end_of_2210() {
        let Some(rom) = testrom::load() else { return };
        assert_eq!(model(rom, 1).as_deref(), Some("chara_sub/s017n.chr"));
        let rules = rules(rom);
        let change = |event| rules.iter().find(|r| r.event == event).map(|r| r.change);
        assert_eq!(change(2210), Some(Some(1)));
        assert_eq!(change(22591), Some(None));
        assert_eq!(introduced_by(&rules, (2, 3)), Some(1));
        assert_eq!(introduced_by(&rules, (1, 4)), None);
    }
}
