//! Scenario triggers (`data/scenario/trigger<map>.bin`): condition/action rules fired by game
//! events (talking to an NPC, entering a room…). Layout and evidence in
//! `docs/re/trigger.md`.
//!
//! Loading reimplements `FUN_0205f9dc` (record handler) and `FUN_0205ec80` (word compiler),
//! matching `FUN_020644a0`, and condition evaluation the documented subset of
//! `FUN_0205fb04`.

use nds_rom::{Error, Result};

use crate::dialogue::Session;
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

/// Integer view of a field (`Tbl_FieldInt`).
fn int(value: &Value) -> u32 {
    match *value {
        Value::Int(v) => v,
        Value::Float(f) => f as i32 as u32,
        Value::Str(_) => 0,
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
            room: int(&v[0]) as i32,
            from: (int(&v[1]) as i32, int(&v[2]) as i32),
            to: (int(&v[3]) as i32, int(&v[4]) as i32),
            event: int(&v[5]) as i32,
            conditions: Vec::new(),
            actions: Vec::new(),
        };
        let mut words = &v[6..];
        while let Some((word, rest)) = words.split_first() {
            let word = int(word);
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
            .map(|a| (a.arg, i32::from((int(&a.operands[0]) >> 16) as i16)))
    }
}

/// Game state read by conditions.
pub trait World {
    /// Story flag `index` (bit array at quest manager `+0x8c`).
    fn flag(&self, index: i32) -> bool;
    /// Per-player flag of set 0 (`+3`) or 1 (`+0x10`), indexed by the local player
    /// (`+0x332`) **[meaning unknown]**.
    fn player_flag(&self, set: u8, index: i32) -> bool;
    /// Quest save nibble, as in [`crate::dialogue::TalkState::quest`].
    fn quest(&self, quest: i32) -> u8;
    fn daytime(&self) -> bool;
    fn session(&self) -> Session;
    /// Save `+0x5cb8`.
    fn substep(&self) -> i32;
    /// Word `i` of the event parameters; for a talk event, 0 = NPC id, 5 = talk-zone
    /// context.
    fn event(&self, i: usize) -> u32;
    /// Copies of item `id` held by the party and bag (`FUN_02086afc`).
    fn item_count(&self, id: i32) -> i32;
    /// Highest level among the active party members (condition 0x50 reads the level of
    /// each member's current vocation).
    fn max_level(&self) -> i32;
    /// Number of quests in the quest log (byte 0 of the log at `FUN_02094d7c`; 8 = full).
    fn quest_log_len(&self) -> u8;
    /// The quest's log entry has bit 15 set **[meaning unknown]** (`FUN_02096144`).
    fn quest_log_flag(&self, quest: i32) -> bool;
    /// Quest bit in the quest log's array at `+0x84` (`FUN_02096098`) **[meaning unknown,
    /// perhaps "already heard of"]**.
    fn quest_known(&self, quest: i32) -> bool;
}

/// High and low halves of an operand, the high one sign-extended like the game's `asr`.
fn halves(op: &Op, i: usize) -> (i32, i32) {
    let v = int(&op.operands[i]);
    (i32::from((v >> 16) as i16), i32::from(v as u16 as i16))
}

fn quest_allowed(world: &dyn World, quest: i32) -> bool {
    !world.session().guest || quest > 0xad
}

fn quest_state(world: &dyn World, quest: i32) -> u8 {
    if (0..0xcc).contains(&quest) {
        world.quest(quest) & 3
    } else {
        0
    }
}

/// `FUN_0206475c`: quest condition with the dialogue encoding (4 and unknown values fail).
fn quest_condition(world: &dyn World, quest: i32, cond: i32) -> bool {
    if !quest_allowed(world, quest) {
        return false;
    }
    let state = quest_state(world, quest);
    match cond {
        0 => state == 2,
        1 => (0..0xcc).contains(&quest) && world.quest(quest) & 4 != 0,
        2 => state == 3,
        3 => state == 1,
        5 | -1 => state == 0,
        _ => false,
    }
}

/// `FUN_0206eba8`: extended flag numbering.
fn flag_ext(world: &dyn World, index: i32) -> bool {
    world.flag(if index < 0x400 { index } else { index + 0x6fa })
}

/// Condition 0x17: wireless-session mode.
fn session_mode(world: &dyn World, mode: i32) -> bool {
    let s = world.session();
    match mode {
        0 => !s.active,
        1 => s.active,
        2 => !s.active || !s.peer_flag,
        3 => s.active && s.peer_flag,
        _ => false,
    }
}

/// Evaluates one condition; `None` for codes not reimplemented yet.
pub fn eval(op: &Op, world: &dyn World) -> Option<bool> {
    let arg = i32::from(op.arg);
    let npc = || world.event(0) == u32::from(op.arg);
    Some(match op.code {
        0 => world.flag(arg),
        1 => !world.flag(arg),
        2 => world.player_flag(0, arg),
        3 => !world.player_flag(0, arg),
        4 => world.player_flag(1, arg),
        5 => !world.player_flag(1, arg),
        6..=0xc => world.event(usize::from(op.code - 6)) == u32::from(op.arg),
        0x11 => world.daytime() != (arg == 1),
        0x12 => world.item_count(i32::from(op.arg as i16)) > 0,
        0x13 => world.item_count(i32::from(op.arg as i16)) < 1,
        0x14 => quest_state(world, arg) == 2 && quest_allowed(world, arg),
        0x15 => quest_condition(world, arg, 1),
        0x16 => quest_state(world, arg) == 3 && quest_allowed(world, arg),
        0x17 => session_mode(world, arg),
        0x19 => quest_state(world, arg) == 1 && quest_allowed(world, arg),
        0x1a => flag_ext(world, arg),
        0x1b => !flag_ext(world, arg),
        0x1c => quest_state(world, arg) == 0 && quest_allowed(world, arg),
        0x25 => world.quest_known(arg) && quest_allowed(world, arg),
        0x26 => !world.quest_known(arg) && quest_allowed(world, arg),
        0x30 => {
            world.item_count(i32::from(op.arg as i16)) >= i32::from(int(&op.operands[0]) as u16)
        }
        0x33 => world.quest_log_flag(arg),
        0x50 => world.max_level() >= arg,
        0x57 => {
            let len = world.quest_log_len();
            if arg == 0 { len < 8 } else { len == 8 }
        }
        0x1e => world.event(8) == u32::from(op.arg),
        0x23 => world.substep() == arg,
        0x24 => world.session().hero_flag == (arg == 0),
        0x27 => world.flag(arg + 0x776),
        0x28 => !world.flag(arg + 0x776),
        0x34 => {
            let (flag, mode) = halves(op, 0);
            npc() && !world.player_flag(1, flag) && session_mode(world, mode)
        }
        0x35..=0x3d => {
            let (quest, cond) = halves(op, 0);
            if !npc() || !quest_condition(world, quest, cond) {
                return Some(false);
            }
            if op.code == 0x37 {
                return Some(true);
            }
            let (a, b) = halves(op, 1);
            match op.code {
                0x38 => world.session().hero_flag == (a == 0),
                0x39 => flag_ext(world, a) && session_mode(world, b),
                0x3a => !flag_ext(world, a) && session_mode(world, b),
                0x3b => flag_ext(world, a),
                0x3c => !flag_ext(world, a),
                0x3d => session_mode(world, a),
                // 0x35: talk zone == a, then condition 0x10; 0x36: then condition 0x12.
                0x35 if world.event(5) != u32::from(a as u16) => false,
                _ => return None,
            }
        }
        0x3e | 0x3f => {
            let (flag, mode) = halves(op, 0);
            npc() && world.flag(flag) == (op.code == 0x3e) && session_mode(world, mode)
        }
        0x40 | 0x41 => {
            // Up to four flags: the arg and three operand halves; negative = unused.
            let (a, b) = halves(op, 0);
            let (c, _) = halves(op, 1);
            [i32::from(op.arg as i16), a, b, c]
                .into_iter()
                .filter(|&f| f >= 0)
                .all(|f| world.flag(f) == (op.code == 0x40))
        }
        0x43 => world.flag(arg + 0x1136),
        0x44 => !world.flag(arg + 0x1136),
        0x51 => world.session().active == (arg != 0),
        _ => return None,
    })
}

/// A condition the evaluator does not implement was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported(pub u16);

/// First loaded trigger of `event` whose conditions all hold (`FUN_020644a0`: triggers of
/// one event are tried in file order). An unimplemented condition is only an error if it is
/// actually reached.
pub fn first_match<'t, 'a>(
    triggers: &'t [Trigger<'a>],
    event: i32,
    world: &dyn World,
) -> std::result::Result<Option<&'t Trigger<'a>>, Unsupported> {
    for trigger in triggers.iter().filter(|t| t.event == event) {
        let mut holds = true;
        for cond in &trigger.conditions {
            match eval(cond, world) {
                Some(true) => {}
                Some(false) => {
                    holds = false;
                    break;
                }
                None => return Err(Unsupported(cond.code)),
            }
        }
        if holds {
            return Ok(Some(trigger));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct W {
        quest11: u8,
        npc: u32,
    }

    impl World for W {
        fn flag(&self, _: i32) -> bool {
            false
        }
        fn player_flag(&self, _: u8, _: i32) -> bool {
            false
        }
        fn quest(&self, q: i32) -> u8 {
            if q == 11 { self.quest11 } else { 0 }
        }
        fn daytime(&self) -> bool {
            true
        }
        fn session(&self) -> Session {
            Session::default()
        }
        fn substep(&self) -> i32 {
            0
        }
        fn event(&self, i: usize) -> u32 {
            if i == 0 { self.npc } else { 0 }
        }
        fn item_count(&self, _: i32) -> i32 {
            0
        }
        fn max_level(&self) -> i32 {
            1
        }
        fn quest_log_len(&self) -> u8 {
            0
        }
        fn quest_log_flag(&self, _: i32) -> bool {
            false
        }
        fn quest_known(&self, _: i32) -> bool {
            false
        }
    }

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
    fn compiles_words_and_selects_by_conditions() {
        let offer = [
            1200,
            3,
            1,
            3,
            9,
            0,
            word(6, 18),    // talking to NPC 18
            word(0x37, 18), // NPC 18 and quest 11 in state 1 ("offered")
            0x000b_0003,
            word(0x76, 18), // talk to 18 with context 192
            0x00c0_0000,
        ];
        let fallback = [
            1200,
            3,
            1,
            3,
            9,
            0,
            word(6, 18),
            word(0x76, 18),
            0x00c1_0000,
        ];
        let data = table(&[&offer, &fallback]);
        let table = Table::parse(&data).unwrap();
        let triggers = parse(&table).unwrap();
        assert_eq!(triggers[0].conditions.len(), 2);
        assert_eq!(triggers[0].actions.len(), 1);
        assert!(triggers[0].loaded(1200, 3, 5));
        assert!(!triggers[0].loaded(1200, 4, 1));
        assert!(!triggers[0].loaded(1201, 3, 5));

        let offered = W {
            quest11: 1,
            npc: 18,
        };
        let t = first_match(&triggers, 0, &offered).unwrap().unwrap();
        assert_eq!(t.talk(), Some((18, 192)));
        let accepted = W {
            quest11: 2,
            npc: 18,
        };
        let t = first_match(&triggers, 0, &accepted).unwrap().unwrap();
        assert_eq!(t.talk(), Some((18, 193)));
        let other = W {
            quest11: 1,
            npc: 19,
        };
        assert_eq!(first_match(&triggers, 0, &other), Ok(None));
    }

    #[test]
    fn truncated_operands_are_rejected() {
        let data = table(&[&[1200, 1, 1, 1, 1, 0, word(0x76, 1)]]);
        let table = Table::parse(&data).unwrap();
        assert!(parse(&table).is_err());
    }

    #[test]
    fn unsupported_conditions_only_fail_when_reached() {
        let data = table(&[&[1, 1, 1, 1, 1, 0, word(6, 5), word(0x10, 0)]]);
        let table = Table::parse(&data).unwrap();
        let triggers = parse(&table).unwrap();
        let w = W { quest11: 0, npc: 4 };
        assert_eq!(first_match(&triggers, 0, &w), Ok(None));
        let w = W { quest11: 0, npc: 5 };
        assert_eq!(first_match(&triggers, 0, &w), Err(Unsupported(0x10)));
    }
}
