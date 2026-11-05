//! Scenario triggers (`data/scenario/trigger<map>.bin`): condition/action rules fired by game
//! events (talking to an NPC, entering a room…). Layout and evidence in
//! `docs/re/trigger.md`. This module parses the rules only; the modelled state and their
//! execution are in `dq9_core::trigger`.
//!
//! Parsing reimplements `Trig_CompileWords` (`0x0205ec80`) and `Trig_LoadRecord`
//! (`0x0205f9dc`).
//!
//! port: 0x0205f9cc, 0x0205f9d4, 0x0205f9dc, 0x0205ec80

use nds_rom::{Error, Result};

use crate::table::{Table, Value};

/// One compiled instruction: the word `code << 16 | arg`, then its operand fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Op<'a> {
    pub code: u16,
    pub arg: u16,
    pub operands: Vec<Value<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trigger<'a> {
    /// Room id (as in `<map>place.bin`, e.g. 1200).
    pub room: i32,
    /// Inclusive (chapter, step) range in which the trigger is loaded.
    pub from: (i32, i32),
    pub to: (i32, i32),
    /// Event type the trigger answers (0 = talking to an NPC).
    pub event: i32,
    /// All must hold, evaluated in order with short-circuit.
    pub conditions: Vec<Op<'a>>,
    pub actions: Vec<Op<'a>>,
}

/// Codes 100..=499 are actions; the others conditions (`FUN_0205ec80`).
pub fn is_action(code: u16) -> bool {
    (100..=499).contains(&code)
}

/// Operand fields following a word, per code (`FUN_0205ec80` and its condition jump table
/// at `0x0205ed1c`). Codes not listed take none.
pub fn operand_count(code: u16) -> usize {
    if is_action(code) {
        match code {
            0x74 | 0x75 | 0x76 | 0x7b | 0x82 | 0x83 | 0x85 | 0x86 | 0x87 | 0x8a | 0x8b | 0x8c
            | 0x90 | 0x95 | 0x96 | 0x9b | 0x9c | 0x9d | 0x9e | 0x9f | 0xa7 | 0xa8 | 0xa9 | 0xab
            | 0xac | 0xae | 0xb0 | 0xb1 | 0xb3 | 0xb4 | 0xc3 | 0xc4 | 0xc8 | 0xc9 | 0xe2 | 0xe6
            | 0xe8 => 1,
            0x6c | 0x6d | 0x88 | 0x98 | 0x99 | 0x9a | 0xbc | 0xbe | 0xcf | 0xd5 | 0xd7 => 2,
            0x7a | 0x84 | 0x94 | 0xb2 | 0xd6 => 3,
            0x8f => 7,
            _ => 0,
        }
    } else {
        match code {
            0x20 | 0x21 | 0x2c | 0x2e | 0x2f | 0x30 | 0x31 | 0x34 | 0x37 | 0x3e | 0x3f | 0x42
            | 0x45 | 0x46 | 0x49 | 0x4a | 0x4b | 0x4c | 0x4f | 0x53 | 0x55 | 0x5a | 0x5b | 500 => 1,
            0x1d | 0x35 | 0x36 | 0x38 | 0x39 | 0x3a | 0x3b | 0x3c | 0x3d | 0x40 | 0x41 | 0x4e => 2,
            0x32 | 0x52 => 4,
            _ => 0,
        }
    }
}

/// Parses the rules of a trigger file. Only record kind 1 defines triggers; kinds 100 and
/// 101 have no-op handlers and other kinds none.
pub fn parse<'a>(table: &Table<'a>) -> Result<Vec<Trigger<'a>>> {
    let mut triggers = Vec::new();
    for record in table.records.iter().filter(|r| r.kind == 1) {
        let v = &record.values;
        if v.len() < 6 {
            return Err(Error::Malformed("trigger record with fewer than 6 fields"));
        }
        let mut trigger = Trigger {
            room: v[0].int() as i32,
            from: (v[1].int() as i32, v[2].int() as i32),
            to: (v[3].int() as i32, v[4].int() as i32),
            event: v[5].int() as i32,
            conditions: Vec::new(),
            actions: Vec::new(),
        };
        let mut words = &v[6..];
        while let Some((word, rest)) = words.split_first() {
            let word = word.int();
            let code = (word >> 16) as u16;
            let n = operand_count(code);
            let operands = rest
                .get(..n)
                .ok_or(Error::Malformed("trigger operand past the record end"))?
                .to_vec();
            let op = Op {
                code,
                arg: word as u16,
                operands,
            };
            if is_action(code) {
                trigger.actions.push(op);
            } else {
                trigger.conditions.push(op);
            }
            words = &rest[n..];
        }
        triggers.push(trigger);
    }
    Ok(triggers)
}

impl Trigger<'_> {
    /// Whether the trigger is loaded in this room at this point of the story
    /// (`FUN_0205f9dc`: `from <= chapter*1000+step <= to`).
    pub fn loaded(&self, room: i32, chapter: i32, step: i32) -> bool {
        let now = chapter * 1000 + step;
        room == self.room
            && self.from.0 * 1000 + self.from.1 <= now
            && now <= self.to.0 * 1000 + self.to.1
    }

    /// `(npc, talk context)` of a "talk to NPC" action (code 0x76): it starts the
    /// conversation with the context forced to the operand's high half.
    pub fn talk(&self) -> Option<(u16, i32)> {
        self.actions
            .iter()
            .find(|a| a.code == 0x76)
            .map(|a| (a.arg, i32::from((a.operands[0].int() >> 16) as i16)))
    }
}

/// Short name of an action code (`docs/re/trigger.md`), `None` when its role is unknown.
pub fn action_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0x64 => "set_flag",
        0x65 => "clear_flag",
        0x66 | 0x68 => "set_track_flag",
        0x67 | 0x69 => "clear_track_flag",
        0x6a => "reset_talk_counts",
        0x6c => "mark_object",
        0x6d => "remove_object",
        0x6e | 0xb1 => "set_time_of_day",
        0x72 | 0x74 => "give_item",
        0x73 | 0x75 | 0x9e => "take_item",
        0x76 => "talk",
        0x77 | 0x80 => "start_event",
        0x79 | 0x89 | 0xe1 => "warp",
        0x7a => "warp_position",
        0x7b => "warp_direction",
        0x7c => "remove_npc",
        0x7d => "accept_quest",
        0x7f => "complete_quest",
        0x81 => "offer_quest",
        0x82 => "set_flag_ext",
        0x83 => "clear_flag_ext",
        0x84 => "advance_track",
        0x85 | 0x8a | 0xe2 => "warp_entry",
        0x86 => "set_flag_in",
        0x87 => "clear_flag_in",
        0x88 => "unmark_object",
        0x8b => "give_gold",
        0x8c => "take_gold",
        0x8e => "mark_ranked",
        0x8f => "add_area",
        0x90 => "set_quest_stage",
        0x92 | 0xa0 | 0xca => "set_flag_offset",
        0x93 | 0xaf => "clear_flag_offset",
        0x94 => "advance_all_tracks",
        0x95 => "hide_object",
        0x96 => "show_object",
        0x9b => "track_flag_and_event",
        0x9c => "complete_quest_exchange",
        0x9d => "quest_and_reward",
        0x9f => "give_items",
        0xbc => "quest_log_value",
        0xcb => "revive_party",
        0xd3 => "set_bit_238c",
        0xd6 => "advance_track_n",
        0xe8 => "set_npc_talk_kind",
        0x6f..=0x71 | 0x98 | 0x9a | 0xa3 => "stop",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(code: u16, arg: u16) -> u32 {
        u32::from(code) << 16 | u32::from(arg)
    }

    /// Builds a table with one record per word list (header fields first).
    fn table(records: &[&[u32]]) -> Vec<u8> {
        let mut body = Vec::new();
        for words in records {
            body.extend_from_slice(&1u16.to_le_bytes());
            body.push(words.len() as u8);
            let type_bytes = words.len().div_ceil(4);
            body.extend(std::iter::repeat_n(0x55u8, type_bytes)); // all ints
            while body.len() % 4 != 0 {
                body.push(0xff);
            }
            for w in *words {
                body.extend_from_slice(&w.to_le_bytes());
            }
        }
        let pool = 0x10 + body.len();
        let mut data = Vec::new();
        for h in [records.len() as u32, pool as u32, 0, 0] {
            data.extend_from_slice(&h.to_le_bytes());
        }
        data.extend(body);
        data
    }

    #[test]
    fn truncated_operands_are_rejected() {
        let data = table(&[&[1200, 1, 1, 1, 1, 0, word(0x76, 1)]]);
        let table = Table::parse(&data).unwrap();
        assert!(parse(&table).is_err());
    }
}
