//! NPC dialogue files (`scenario/<map><chapter>0.gp2` / `NNN_<lang>.bin`): typed records and
//! the game's choice of the line an NPC says. Layout, evidence and the meaning of every
//! input in `docs/re/dialogue.md`.
//!
//! Reimplements `ov_017::021ba810` (`Talk_SelectLine`): the record table is walked once in
//! file order, each record goes through its kind's handler (`ov_017::021b9d00`…`021ba524`)
//! and the last accepted line wins, subject to the precedence flags the handlers raise.
//!
//! port: ov_017::021ba810, ov_017::021b9bcc, ov_017::021b9d00, ov_017::021b9e30
//! port: ov_017::021ba124, ov_017::021ba280, ov_017::021ba3dc, ov_017::021ba524
//! port: 0x02030b1c

use nds_rom::{Error, Result};

use crate::table::{Table, Value};

/// Record kind (the table's per-record `kind` word). Other kinds have no handler and are
/// ignored by the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 1: scenario-step range (`ov_017::021b9d00`), lowest priority.
    Step,
    /// 2: quest state (`ov_017::021b9e30`).
    Quest,
    /// 3: step range, wireless session active and the local peer flag clear
    /// (`ov_017::021ba124`).
    SessionStepA,
    /// 4: step range, wireless session active and the local peer flag set
    /// (`ov_017::021ba280`).
    SessionStepB,
    /// 5: step range, not a guest and hero word clear (`ov_017::021ba3dc`).
    HostStep,
    /// 6: quest state, same gate as [`Kind::HostStep`] (`ov_017::021ba524`).
    HostQuest,
}

impl Kind {
    pub fn from_raw(kind: u16) -> Option<Self> {
        Some(match kind {
            1 => Self::Step,
            2 => Self::Quest,
            3 => Self::SessionStepA,
            4 => Self::SessionStepB,
            5 => Self::HostStep,
            6 => Self::HostQuest,
            _ => return None,
        })
    }
}

/// One record: `[a, b, slot, text]` or `[a, b, night, slot, text]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Line<'a> {
    pub kind: Option<Kind>,
    /// Step kinds: lowest scenario step (negative = no range check). Quest kinds: quest id.
    pub a: i32,
    /// Step kinds: highest scenario step. Quest kinds: required quest condition
    /// ([`quest_condition_holds`]).
    pub b: i32,
    /// `None` for 4-field records; for 5-field records, whether the line is night-only.
    pub night: Option<bool>,
    /// Message slot: `slot / 80` must equal the talk context's group, `slot % 80 / 16` selects
    /// the test ([`slot_accepts`]).
    pub slot: i32,
    /// `None` when absent or not a string field: the game then skips the record.
    pub text: Option<&'a [u8]>,
}

/// Integer view of a field (`FUN_02030b1c`): floats are converted, strings read as 0.
fn int(value: &Value) -> i32 {
    match *value {
        Value::Int(v) => v as i32,
        // FUN_0200c5fc, float to int [assumed truncating, like the ARM EABI helper].
        Value::Float(f) => f as i32,
        Value::Str(_) => 0,
    }
}

/// Parses the records of a dialogue file. Records of unknown kinds are kept (with
/// `kind: None`) so that indices match the table.
pub fn parse<'a>(table: &Table<'a>) -> Result<Vec<Line<'a>>> {
    table
        .records
        .iter()
        .map(|record| {
            let v = &record.values;
            let kind = Kind::from_raw(record.kind);
            if v.len() < 4 {
                if kind.is_none() {
                    return Ok(Line {
                        kind,
                        a: 0,
                        b: 0,
                        night: None,
                        slot: 0,
                        text: None,
                    });
                }
                return Err(Error::Malformed("dialogue record with fewer than 4 fields"));
            }
            let (night, rest) = if v.len() < 5 {
                (None, &v[2..])
            } else {
                (Some(int(&v[2]) != 0), &v[3..])
            };
            Ok(Line {
                kind,
                a: int(&v[0]),
                b: int(&v[1]),
                night,
                slot: int(&rest[0]),
                text: match rest[1] {
                    Value::Str(s) => s,
                    _ => None,
                },
            })
        })
        .collect()
}

/// Wireless-session and hero state gating kinds 3 to 6.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Session {
    /// A session exists (`FUN_0202b7e8`).
    pub active: bool,
    /// Session active and the local console is not peer 0 (`FUN_0202c550`).
    pub guest: bool,
    /// Byte of the local peer at session `+0x1038` is non-zero (`FUN_0202c1b4`)
    /// **[meaning unknown]**.
    pub peer_flag: bool,
    /// Halfword at `hero->+0x130->+4` is non-zero, read as "the hero's HP is not 0"
    /// **[hypothesis, docs/re/dialogue.md]**; a fallen hero enables kinds 5/6.
    pub hero_alive: bool,
}

/// Game state read while choosing a line.
pub struct TalkState<'q> {
    /// Scenario step, save `+0x5cb4` (`FUN_020107e0`); tested by step ranges.
    pub step: i32,
    /// Save `+0x5cb8` (`FUN_02010814`); tested by group-4 slots.
    pub substep: i32,
    /// Talk context (talk task `+0x124`): id of the talk zone the hero stands in, 0 outside
    /// any zone, or a value forced by the caller (see `docs/re/dialogue.md`).
    pub context: i32,
    /// Per-NPC talk counters (nibbles, `FUN_0206ec2c` on two 128-byte arrays).
    pub talk_count: [i32; 2],
    /// Daytime (`FUN_020103c4`: save `+0x3dc` non-zero); night-only lines are skipped.
    pub daytime: bool,
    pub session: Session,
    /// Quest save nibble for quest `0..=0xcb` (save `+0x2cc`, 4 bits per quest):
    /// bits 0-1 state, bit 2 and bit 3 flags.
    pub quest: &'q dyn Fn(i32) -> u8,
}

impl std::fmt::Debug for TalkState<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TalkState")
            .field("step", &self.step)
            .field("substep", &self.substep)
            .field("context", &self.context)
            .field("talk_count", &self.talk_count)
            .field("daytime", &self.daytime)
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

const QUEST_COUNT: i32 = 0xcc;

impl TalkState<'_> {
    fn nibble(&self, quest: i32) -> u8 {
        if (0..QUEST_COUNT).contains(&quest) {
            (self.quest)(quest) & 0xf
        } else {
            0
        }
    }

    /// `FUN_0206e130`.
    fn quest_state(&self, quest: i32) -> u8 {
        self.nibble(quest) & 3
    }
}

/// Quest condition `b` of kinds 2/6 against the quest's save nibble. Values other than
/// -1..=4 impose no condition (the game's `switch` has no default case).
pub fn quest_condition_holds(b: i32, nibble: u8) -> bool {
    let state = nibble & 3;
    match b {
        -1 => state == 0,
        0 => state == 2,
        1 => nibble & 4 != 0,
        2 => state == 3,
        3 => state == 1,
        4 => nibble & 8 != 0 && state == 0,
        _ => true,
    }
}

/// Selection state of `Talk_SelectLine` (the globals at `ov_017::021d8438` and
/// `ov_017::021d7c54`).
struct Selector {
    /// A kind 3/4 line was chosen.
    session_line: bool,
    /// A kind 5 line was chosen.
    host_line: bool,
    /// A night-only record passed its night test: later 4-field records are skipped.
    night_passed: bool,
    /// Best group-4 slot value so far.
    best_substep: i32,
    /// Quest of the last chosen kind 2 / kind 6 line. Stored as a signed byte, so quests
    /// above 127 read back as "none" — a game quirk kept as is.
    quest_line: i8,
    host_quest_line: i8,
    chosen: Option<usize>,
}

/// Group/test of a message slot (`ov_017::021b9bcc`): returns whether the line is taken.
fn slot_accepts(sel: &mut Selector, state: &TalkState, slot: i32, index: usize) -> bool {
    if slot / 80 != state.context / 80 {
        return false;
    }
    let low = slot % 80;
    let (test, n) = (low / 16, low % 16);
    match test {
        0 if n > state.talk_count[0] => return false,
        1 if n > state.talk_count[1] => return false,
        2 | 3 if slot != state.context => return false,
        4 => {
            if n > state.substep || n < sel.best_substep {
                return false;
            }
            sel.best_substep = n;
        }
        // Negative slots: accepted without choosing a line.
        t if !(0..=4).contains(&t) => return true,
        _ => {}
    }
    sel.chosen = Some(index);
    true
}

impl Selector {
    /// Shared tail of every handler: night test, then the slot. `None` = record skipped.
    fn night_and_slot(&mut self, line: &Line, state: &TalkState, index: usize) -> Option<bool> {
        match line.night {
            None if self.night_passed => return None,
            None | Some(false) => {}
            Some(true) => {
                if state.daytime {
                    return None;
                }
                self.night_passed = true;
            }
        }
        line.text?;
        Some(slot_accepts(self, state, line.slot, index))
    }

    fn step_in_range(line: &Line, state: &TalkState) -> bool {
        line.a < 0 || (line.a <= state.step && state.step <= line.b)
    }

    /// Precedence between quests, then the quest condition (kinds 2 and 6).
    fn quest_passes(state: &TalkState, line: &Line, last: i8) -> bool {
        let (quest, last) = (line.a, i32::from(last));
        let current = state.quest_state(quest);
        let overrides = last == quest
            || last < 0
            || current == 1
            || current == 2
            || (state.quest_state(last) == 3 && current == 3 && quest >= last);
        overrides && quest_condition_holds(line.b, state.nibble(quest))
    }

    fn visit(&mut self, line: &Line, state: &TalkState, index: usize) {
        let Some(kind) = line.kind else { return };
        let session = state.session;
        match kind {
            Kind::Step => {
                if self.session_line
                    || self.quest_line >= 0
                    || self.host_line
                    || self.host_quest_line >= 0
                    || !Self::step_in_range(line, state)
                {
                    return;
                }
                self.night_and_slot(line, state, index);
            }
            Kind::SessionStepA | Kind::SessionStepB => {
                let wanted = kind == Kind::SessionStepB;
                if self.quest_line >= 0
                    || self.host_line
                    || self.host_quest_line >= 0
                    || !session.active
                    || session.peer_flag != wanted
                    || !Self::step_in_range(line, state)
                {
                    return;
                }
                if self.night_and_slot(line, state, index) == Some(true) {
                    self.session_line = true;
                }
            }
            Kind::HostStep => {
                if self.host_quest_line >= 0
                    || session.guest
                    || session.hero_alive
                    || !Self::step_in_range(line, state)
                {
                    return;
                }
                if self.night_and_slot(line, state, index) == Some(true) {
                    self.host_line = true;
                }
            }
            Kind::Quest => {
                // FUN_0206e32c: a guest only gets quests above 0xad.
                let allowed = !session.guest || line.a > 0xad;
                if self.host_line
                    || self.host_quest_line >= 0
                    || !allowed
                    || !Self::quest_passes(state, line, self.quest_line)
                {
                    return;
                }
                if self.night_and_slot(line, state, index) == Some(true) {
                    self.quest_line = line.a as i8;
                }
            }
            Kind::HostQuest => {
                if session.guest
                    || session.hero_alive
                    || !Self::quest_passes(state, line, self.host_quest_line)
                {
                    return;
                }
                if self.night_and_slot(line, state, index) == Some(true) {
                    self.host_quest_line = line.a as i8;
                }
            }
        }
    }
}

/// Index of the line the NPC says, or `None` (the game then aborts the conversation).
pub fn select(lines: &[Line], state: &TalkState) -> Option<usize> {
    let mut sel = Selector {
        session_line: false,
        host_line: false,
        night_passed: false,
        best_substep: 0,
        quest_line: -1,
        host_quest_line: -1,
        chosen: None,
    };
    for (index, line) in lines.iter().enumerate() {
        sel.visit(line, state, index);
    }
    sel.chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: Kind, a: i32, b: i32, night: Option<bool>, slot: i32) -> Line<'static> {
        Line {
            kind: Some(kind),
            a,
            b,
            night,
            slot,
            text: Some(b"x"),
        }
    }

    fn state(quest: &dyn Fn(i32) -> u8) -> TalkState<'_> {
        TalkState {
            step: 5,
            substep: 0,
            context: 0,
            talk_count: [0, 0],
            daytime: true,
            session: Session::default(),
            quest,
        }
    }

    #[test]
    fn later_step_line_wins_and_ranges_apply() {
        let lines = [
            line(Kind::Step, 1, 9, None, 0),
            line(Kind::Step, 5, 5, None, 0),
            line(Kind::Step, 6, 9, None, 0),
        ];
        assert_eq!(select(&lines, &state(&|_| 0)), Some(1));
    }

    #[test]
    fn slot_groups_and_tests() {
        let quests = |_| 0;
        let mut s = state(&quests);
        // Second talk (counter 0 = 1) unlocks slot 1; slot 16 needs counter 1 >= 0.
        s.talk_count = [1, 0];
        let lines = [
            line(Kind::Step, -1, 0, None, 1),
            line(Kind::Step, -1, 0, None, 2),
        ];
        assert_eq!(select(&lines, &s), Some(0));
        // Group 1 (talk zone 80) is invisible from context 0, and vice versa.
        let zone = [
            line(Kind::Step, -1, 0, None, 80),
            line(Kind::Step, -1, 0, None, 0),
        ];
        assert_eq!(select(&zone, &s), Some(1));
        s.context = 80;
        assert_eq!(select(&zone, &s), Some(0));
        // Tests 2/3 need the exact context; test 4 keeps the highest value <= substep.
        s.context = 0xc4;
        s.substep = 2;
        let exact = [
            line(Kind::Step, -1, 0, None, 0xc4),
            line(Kind::Step, -1, 0, None, 0xc5),
        ];
        assert_eq!(select(&exact, &s), Some(0));
        s.context = 0;
        let best = [
            line(Kind::Step, -1, 0, None, 0x42),
            line(Kind::Step, -1, 0, None, 0x41),
            line(Kind::Step, -1, 0, None, 0x43),
        ];
        assert_eq!(select(&best, &s), Some(0));
    }

    #[test]
    fn night_lines_override_and_shadow_later_day_lines() {
        let quests = |_| 0;
        let mut s = state(&quests);
        let lines = [
            line(Kind::Step, 1, 9, None, 0),
            line(Kind::Step, 1, 9, Some(true), 0),
            line(Kind::Step, 1, 9, None, 0),
        ];
        assert_eq!(select(&lines, &s), Some(2));
        s.daytime = false;
        assert_eq!(select(&lines, &s), Some(1));
    }

    #[test]
    fn quest_lines_take_precedence_over_step_lines() {
        // Quest 11 accepted (state 2); quest 12 not taken.
        let quests = |q| if q == 11 { 2 } else { 0 };
        let s = state(&quests);
        let lines = [
            line(Kind::Quest, 12, 3, None, 0),  // needs state 1
            line(Kind::Quest, 11, 0, None, 0),  // needs state 2: chosen
            line(Kind::Step, 1, 9, None, 0),    // blocked by the quest line
            line(Kind::Quest, 12, -1, None, 0), // other quest in state 0: blocked
        ];
        assert_eq!(select(&lines, &s), Some(1));
    }

    #[test]
    fn quest_conditions() {
        assert!(quest_condition_holds(-1, 0));
        assert!(quest_condition_holds(0, 2));
        assert!(quest_condition_holds(1, 4 | 2));
        assert!(quest_condition_holds(2, 3));
        assert!(quest_condition_holds(3, 1));
        assert!(quest_condition_holds(4, 8));
        assert!(!quest_condition_holds(4, 8 | 1));
        assert!(quest_condition_holds(7, 0));
    }

    #[test]
    fn session_kinds_follow_their_gates() {
        let quests = |_| 0;
        let mut s = state(&quests);
        let lines = [
            line(Kind::Step, 1, 9, None, 0),
            line(Kind::SessionStepB, 1, 9, None, 0),
            line(Kind::HostStep, 1, 9, None, 0),
        ];
        s.session.hero_alive = true;
        assert_eq!(select(&lines, &s), Some(0));
        s.session = Session {
            active: true,
            guest: true,
            peer_flag: true,
            hero_alive: false,
        };
        assert_eq!(select(&lines, &s), Some(1));
        s.session = Session::default();
        assert_eq!(select(&lines, &s), Some(2));
    }
}
