//! Modelled trigger state, condition evaluation and execution: what the game does when a
//! trigger fires (`Trig_FindMatch`, `Trig_RunAction`, `Trig_RunDeferred`, `Trig_Fire`) on
//! the state the engine models. Rule parsing lives in `dq9_formats::trigger`; layout and
//! evidence in `docs/re/trigger.md`.
//!
//! port: 0x0205fb04, 0x020644a0, 0x02061c14, 0x02064540, 0x0206446c, 0x020649c0
//! port: 0x0206f82c, 0x0206475c, 0x02064ba8, 0x020703d8, 0x0206e174, 0x0206e358
//! port: 0x0206eb74, 0x0206eba8, 0x0206ec04, 0x0206ecf8

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use dq9_formats::dialogue::Session;
use dq9_formats::table::Value;
use dq9_formats::trigger::{Op, Trigger};

/// Number of vocations (levels array at character data `+0x16c`).
pub const VOCATIONS: usize = 13;

/// A character as conditions read it: an entry of the save's character table
/// (`FUN_0200fd80`, ids `0..=0xe8`), its data block at `+0x150` and its status block at
/// `+0x130`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Member {
    /// Current vocation, data `+0x950` (0 Guardian, 1 Warrior … 12 Ranger, menu `str_sta`).
    pub vocation: u8,
    /// Level in each vocation, data `+0x16c`.
    pub levels: [u16; VOCATIONS],
    /// Status halfword `+4`, read as current HP **[hypothesis, `trigger.md`]**: 0 = fallen.
    pub hp: u16,
    /// Bit 0 of the status word **[unknown]**: a set bit excludes the member from
    /// conditions 0x31, 0x32/0x52 and 0x50 and flips 0x1f4.
    pub status_bit0: bool,
    /// Byte at data `+0x56b` **[unknown]**: low nibble tested by 0x56, high nibble by 0x1f4.
    pub tag: u8,
    /// Halfword `+0x18` of each 32-byte slot at data `+0x194`, compared by 0x32/0x52 with
    /// item ids **[hypothesis: equipment]**.
    pub slots: [u16; 11],
    /// Bit 18 of word `+8` of each slot, as a bit mask (counted by 0x2f) **[unknown]**.
    pub slot_bits: u16,
    /// Bits 10-19 of data `+4` **[unknown]**, subtracted by 0x2e.
    pub base: u16,
    /// Six packed 10-bit fields **[unknown]**: bits 10-19 and 20-29 of data `+0xc`, bits
    /// 0-9, 10-19 and 20-29 of `+0x10`, bits 0-9 of `+0x14` (0x2d, 0x2e).
    pub attrs: [u16; 6],
}

impl Member {
    /// Level in the current vocation.
    pub fn level(&self) -> u16 {
        self.level_in(i32::from(self.vocation))
    }

    pub fn level_in(&self, vocation: i32) -> u16 {
        usize::try_from(vocation)
            .ok()
            .and_then(|v| self.levels.get(v))
            .copied()
            .unwrap_or(0)
    }

    /// Counted by conditions 0x31 and 0x50: status bit 0 clear and HP not 0.
    fn standing(&self) -> bool {
        !self.status_bit0 && self.hp != 0
    }
}

/// One entry of the quest log (`QuestLog_Get`: up to 8 entries of 16 bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuestLogEntry {
    /// Word 0: quest id in bits 0-8; bits 9, 15 and 18 are tested by conditions 0x5c, 0x33
    /// and 0x46 **[unknown]**.
    pub word: u32,
    /// Halfwords at `+4` to `+0xe` (values compared by condition 0x4e).
    pub values: [u16; 6],
}

/// The quest log (`QuestLog_Get`, `0x02094d7c`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestLog {
    pub entries: Vec<QuestLogEntry>,
    /// Quests whose bit is set in the log's bit array at `+0x84` (conditions 0x25/0x26)
    /// **[unknown]**.
    pub marked: BTreeSet<i32>,
}

impl QuestLog {
    /// `QuestLog_Find`: the entry whose quest id (bits 0-8) is `quest`.
    pub fn find(&self, quest: i32) -> Option<&QuestLogEntry> {
        self.entries
            .iter()
            .find(|e| (e.word & 0x1ff) as i32 == quest)
    }
}

/// Values compared by conditions whose meaning is unknown, named after where the game
/// reads them. "Save" is the game state returned by `FUN_0200f398` (chapter at `+0x5cb0`…),
/// "global" the structure returned by `FUN_02012ff4`. Values are the raw bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Raw {
    /// 0x10, 0x35: word `+0x954` of `*(DAT_020421bc + 0x1c)` (`FUN_020421b0`,
    /// `FUN_020457f0`).
    Field954,
    /// 0x1f: signed byte at quest manager `+0x490`.
    Quest490,
    /// 0x49: signed halfword at quest manager `+0x2ca`.
    Quest2ca,
    /// 0x2b: bits 19-29 of save `+0x56a0`, sign-extended.
    Save56a0,
    /// 0x4a: halfword at save `+0x71dc`.
    Save71dc,
    /// 0x54: byte at save `+0x64de`.
    Save64de,
    /// 0x1f9: byte at save `+0x63dc` (bit 0 tested).
    Save63dc,
    /// 0x1fb: halfword at save `+0x3f8`, written with a room id by the deferred actions
    /// 0x79/0x89/0xe1.
    Save3f8,
    /// 0x5d: word at global `+0x238c` (bit array).
    Global238c,
    /// 0x5e: word at global `+0x237c`.
    Global237c,
    /// 0x5f: halfword at global `+0x27b4`.
    Global27b4,
    /// 0x62/0x63: byte at global `+0x27d0`.
    Global27d0,
}

/// Game state read by conditions (the engine's view; [`State`] is a plain-data
/// implementation).
pub trait World {
    /// Story flag `index` (bit array at quest manager `+0x8c`, `Flags_Test`).
    fn flag(&self, index: i32) -> bool;
    /// Flag `index` of set 0 or 1 of the current scenario track (see [`Track`]).
    fn track_flag(&self, set: u8, index: i32) -> bool;
    /// Quest save nibble, as in [`crate::dialogue::TalkState::quest`].
    fn quest(&self, quest: i32) -> u8;
    fn quest_log(&self) -> &QuestLog;
    /// Save `+0x3dc`: 0 at night (`Save_IsDaytime`); condition 0x1f5 compares it with 0, 2
    /// and 3.
    fn time_of_day(&self) -> u32;
    fn session(&self) -> Session;
    /// Substep of the current track, copied to save `+0x5cb8`.
    fn substep(&self) -> i32;
    /// Word `i` (0..12) of the event parameters; for a talk event, 0 = NPC id, 5 = talk-zone
    /// context.
    fn event(&self, i: usize) -> u32;
    /// Copies of item `id` held by the party and bag (`Party_ItemCount`).
    fn item_count(&self, id: i32) -> i32;
    /// Word at save `+0x3970` compared by 0x55 **[hypothesis: gold]**.
    fn gold(&self) -> u32;
    /// Party members in order (ids at save `+0x397c`, count at `+0x3980`); the first leads.
    fn party(&self) -> &[Member];
    /// Character `id` of the character table, for ids 0 to 3 (one per player of a session
    /// **[hypothesis]**; 0 = the hero) and `0xce` (condition 0x56).
    fn character(&self, id: usize) -> Option<Member>;
    /// Save `+0x3ac`: id of the local player's character (`FUN_0200fddc`).
    fn local_character(&self) -> usize;
    /// Whether the leader stands strictly inside one of the talk zones of NPC `npc`
    /// (`<map>place.bin` kind 6 rectangles); `None` if the NPC is not on the map.
    fn leader_in_zone_of(&self, npc: u32) -> Option<bool>;
    /// Today's `(month, day)` from the real-time clock (`FUN_020cf10c`).
    fn date(&self) -> (u32, u32);
    /// `(month, day)` stored in save `+0x569c` (bits 12-15, 16-20).
    fn saved_date(&self) -> (u32, u32);
    fn raw(&self, field: Raw) -> u32;
    /// Condition 0x1f6: byte at global `+0x23a1` (`FUN_02098ef8` on `+0x840`), cleared by
    /// the read **[unknown]**.
    fn take_signal(&self) -> bool;
}

/// Number of scenario tracks (`0x1c`-byte blocks at quest manager `+0`).
pub const TRACKS: usize = 5;

/// One scenario track: progress and its scratch flags. The track in use (`+0x332`) is
/// chosen by the room when a map is entered (`FUN_02064ba8`, [`track_of_room`]), and its
/// progress copied to save `+0x5cb0..+0x5cb8`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Track {
    pub chapter: u8,
    pub step: u8,
    pub substep: u8,
    /// Set 0 (`+3`, 13 bytes) is cleared when the track's chapter changes, set 1 (`+0x10`,
    /// 12 bytes) when its step changes (`FUN_0206e090`, `FUN_0206e0e0`).
    pub flags: [BTreeSet<i32>; 2],
}

/// Track of a room (`FUN_02064ba8`): 1 for X02/T01, 2 for M07, M08, S10, D07, 3 for C02 and
/// D08, 4 for M11 and D13 (rooms listed by the game), 0 elsewhere.
pub fn track_of_room(room: i32) -> usize {
    match room {
        4200..=4202 | 9000..=9008 => 1,
        1700..=1706 | 1800..=1808 | 6000..=6001 | 7700..=7709 => 2,
        200..=219 | 7802..=7809 => 3,
        2100..=2109 | 8301..=8303 => 4,
        _ => 0,
    }
}

/// A [`World`] made of plain values; every unset value is the default (no flag, no quest,
/// empty party, night…).
#[derive(Debug, Default, Clone)]
pub struct State {
    pub flags: BTreeSet<i32>,
    pub tracks: [Track; TRACKS],
    /// Current track (quest manager `+0x332`).
    pub track: usize,
    pub quests: BTreeMap<i32, u8>,
    pub quest_log: QuestLog,
    pub time_of_day: u32,
    pub session: Session,
    pub event: [u32; 12],
    pub items: BTreeMap<i32, i32>,
    /// Item id → compendium index, read from the ROM's item tables by the application
    /// ([`dq9_formats::item::ItemTable`]). The "item obtained" flag of
    /// [`State::obtain`] needs it; empty when no table was loaded.
    pub compendium: dq9_formats::item::ItemTable,
    pub gold: u32,
    pub party: Vec<Member>,
    pub characters: BTreeMap<usize, Member>,
    pub local_character: usize,
    /// NPC on the map → leader inside one of its talk zones.
    pub zones: BTreeMap<u32, bool>,
    pub date: (u32, u32),
    pub saved_date: (u32, u32),
    pub raw: BTreeMap<Raw, u32>,
    pub signal: Cell<bool>,
    /// Per-NPC conversation counters (the two tables at `gameres+0x44c4`): cleared by a new
    /// chapter or step ([`State::advance`]) and, NPC by NPC, by action 0x6a.
    pub talk_counts: crate::talk::TalkCounts,
}

impl World for State {
    fn flag(&self, index: i32) -> bool {
        self.flags.contains(&index)
    }
    fn track_flag(&self, set: u8, index: i32) -> bool {
        self.tracks[self.track].flags[usize::from(set & 1)].contains(&index)
    }
    fn quest(&self, quest: i32) -> u8 {
        self.quests.get(&quest).copied().unwrap_or(0)
    }
    fn quest_log(&self) -> &QuestLog {
        &self.quest_log
    }
    fn time_of_day(&self) -> u32 {
        self.time_of_day
    }
    fn session(&self) -> Session {
        self.session
    }
    fn substep(&self) -> i32 {
        i32::from(self.tracks[self.track].substep)
    }
    fn event(&self, i: usize) -> u32 {
        self.event.get(i).copied().unwrap_or(0)
    }
    fn item_count(&self, id: i32) -> i32 {
        self.items.get(&id).copied().unwrap_or(0)
    }
    fn gold(&self) -> u32 {
        self.gold
    }
    fn party(&self) -> &[Member] {
        &self.party
    }
    fn character(&self, id: usize) -> Option<Member> {
        self.characters.get(&id).copied()
    }
    fn local_character(&self) -> usize {
        self.local_character
    }
    fn leader_in_zone_of(&self, npc: u32) -> Option<bool> {
        self.zones.get(&npc).copied()
    }
    fn date(&self) -> (u32, u32) {
        self.date
    }
    fn saved_date(&self) -> (u32, u32) {
        self.saved_date
    }
    fn raw(&self, field: Raw) -> u32 {
        self.raw.get(&field).copied().unwrap_or(0)
    }
    fn take_signal(&self) -> bool {
        self.signal.replace(false)
    }
}

/// High and low halves of an operand, both sign-extended (the high one like the game's
/// `asr`; callers that store the low one unsigned use [`low`]).
fn halves(op: &Op, i: usize) -> (i32, i32) {
    let v = op.operands[i].int();
    (i32::from((v >> 16) as i16), i32::from(v as u16 as i16))
}

/// Low halfword of operand `i` (the compiler's `strh` of a single-field operand).
fn low(op: &Op, i: usize) -> u16 {
    op.operands[i].int() as u16
}

fn quest_allowed(world: &dyn World, quest: i32) -> bool {
    !world.session().guest || quest > 0xad
}

/// Quest nibble, 0 outside `0..0xcc` (the `Quest_*` accessors' range check).
fn quest_nibble(world: &dyn World, quest: i32) -> u8 {
    if (0..0xcc).contains(&quest) {
        world.quest(quest)
    } else {
        0
    }
}

fn quest_state(world: &dyn World, quest: i32) -> u8 {
    quest_nibble(world, quest) & 3
}

/// `Trig_QuestCondition`: quest condition with the dialogue encoding (4 and unknown values
/// fail).
fn quest_condition(world: &dyn World, quest: i32, cond: i32) -> bool {
    if !quest_allowed(world, quest) {
        return false;
    }
    let state = quest_state(world, quest);
    match cond {
        0 => state == 2,
        1 => quest_nibble(world, quest) & 4 != 0,
        2 => state == 3,
        3 => state == 1,
        5 | -1 => state == 0,
        _ => false,
    }
}

/// `Flags_TestExt`: extended flag numbering.
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

/// Condition 0x24 with `arg`: character 0's status halfword `+4` (HP) is 0 if `arg` ≠ 0,
/// not 0 if `arg` = 0.
fn hero_down(world: &dyn World, arg: i32) -> bool {
    let hp = world.character(0).map_or(0, |h| h.hp);
    (hp == 0) == (arg != 0)
}

/// Condition 0x12 with `item`.
fn holds_item(world: &dyn World, item: i32) -> bool {
    world.item_count(item) > 0
}

/// Byte `offset` of the event parameter block (conditions 0x20/0x21 read `+0x2a..+0x2e`).
fn event_byte(world: &dyn World, offset: usize) -> i8 {
    (world.event(offset / 4) >> (8 * (offset % 4))) as i8
}

/// Degrees to the 16-bit angle stored by the compiler for condition 0x1d: radians brought
/// once into `0..=2π`, times 4096, truncated (`Trig_CompileWords`, `0x0205ee24`; single
/// precision like the game's soft-float calls).
pub fn degrees_to_angle(degrees: i32) -> u16 {
    use std::f32::consts::PI;
    let mut r = PI * degrees as f32 / 180.0;
    if r > 2.0 * PI {
        r -= 2.0 * PI;
    } else if r < 0.0 {
        r += 2.0 * PI;
    }
    (r * 4096.0) as i32 as u16
}

/// Conditions 0x32/0x52 (equipment): one record word compiles into two nodes
/// (`0x0205ef88`). Node 1 = (arg, op0 high, op0 low, op1 high), each `< 1` = any, compared
/// with slots 0, 1, 5, 6; each candidate it accepts sets a bit of the mask at quest manager
/// `+0x492` (cleared once per event) and the node holds if the mask is not 0. Node 2 = (op1
/// low, op2 high, op2 low, op3 low), each `0` = any, compared with slots 7 to 10 of the
/// candidates of the mask. Candidates: the party (0x32) or the local character (0x52),
/// without status bit 0.
fn equipment(op: &Op, world: &dyn World, mask: &mut u8) -> bool {
    let w = |i: usize| op.operands[i].int();
    let (hi, lo) = (|v: u32| (v >> 16) as u16, |v: u32| v as u16);
    let first = [op.arg, hi(w(0)), lo(w(0)), hi(w(1))];
    let second = [lo(w(1)), hi(w(2)), lo(w(2)), lo(w(3))];
    let candidates: Vec<Member> = if op.code == 0x52 {
        world
            .character(world.local_character())
            .into_iter()
            .collect()
    } else {
        world.party().to_vec()
    };
    for (i, m) in candidates.iter().enumerate() {
        let accepted = first
            .iter()
            .zip([0, 1, 5, 6])
            .all(|(&want, s)| (want as i16) < 1 || want == m.slots[s]);
        if !m.status_bit0 && accepted {
            *mask |= 1 << i;
        }
    }
    *mask != 0
        && candidates.iter().enumerate().any(|(i, m)| {
            !m.status_bit0
                && *mask & (1 << i) != 0
                && second
                    .iter()
                    .zip(7..11)
                    .all(|(&want, s)| want == 0 || want == m.slots[s])
        })
}

/// Evaluates one condition word (`Trig_EvalCondition`, `0x0205fb04`); `None` for the codes
/// absent from the data whose state is not modelled. `mask` is the equipment mask of
/// conditions 0x32/0x52.
fn eval(op: &Op, world: &dyn World, mask: &mut u8) -> Option<bool> {
    let arg = i32::from(op.arg);
    let npc = || world.event(0) == u32::from(op.arg);
    let party = world.party();
    Some(match op.code {
        0 => world.flag(arg),
        1 => !world.flag(arg),
        2 => world.track_flag(0, arg),
        3 => !world.track_flag(0, arg),
        4 => world.track_flag(1, arg),
        5 => !world.track_flag(1, arg),
        6..=0xc => world.event(usize::from(op.code - 6)) == u32::from(op.arg),
        0xd..=0xf => {
            // FUN_020108a0: characters 0..=3 present.
            let n = (0..4).filter(|&i| world.character(i).is_some()).count() as i32;
            match op.code {
                0xd => arg <= n,
                0xe => n <= arg,
                _ => n == arg,
            }
        }
        0x10 => world.raw(Raw::Field954) == u32::from(op.arg),
        0x11 => (world.time_of_day() != 0) != (arg == 1),
        0x12 => holds_item(world, i32::from(op.arg as i16)),
        0x13 => !holds_item(world, i32::from(op.arg as i16)),
        0x14 => quest_state(world, arg) == 2 && quest_allowed(world, arg),
        0x15 => quest_condition(world, arg, 1),
        0x16 => quest_state(world, arg) == 3 && quest_allowed(world, arg),
        0x17 => session_mode(world, arg),
        0x19 => quest_state(world, arg) == 1 && quest_allowed(world, arg),
        0x1a => flag_ext(world, arg),
        0x1b => !flag_ext(world, arg),
        0x1c => quest_state(world, arg) == 0 && quest_allowed(world, arg),
        0x1d => {
            // Word 7 = (arg, operand 0) as (low, high) halves, low half of word 10 = the
            // angle of operand 1 (degrees).
            let word = world.event(7);
            word as u16 == op.arg
                && (word >> 16) as u16 == low(op, 0)
                && world.event(10) as u16 == degrees_to_angle(op.operands[1].int() as i32)
        }
        0x1e => world.event(8) == u32::from(op.arg),
        0x1f => i32::from(world.raw(Raw::Quest490) as i8) == i32::from(op.arg as i16),
        0x20 | 0x21 => {
            // The operand's bytes, most significant first, 0 = any. 0x20: in order against
            // the event bytes +0x2a..+0x2e; 0x21: each matches a distinct event byte.
            let want = op.operands[0].int().to_be_bytes().map(|b| b as i8);
            let have: [i8; 4] = std::array::from_fn(|i| event_byte(world, 0x2a + i));
            if op.code == 0x20 {
                want.iter().zip(have).all(|(&w, h)| w == 0 || w == h)
            } else {
                let mut used = [false; 4];
                want.iter().filter(|&&w| w != 0).all(|&w| {
                    let j = (0..4).find(|&j| !used[j] && have[j] == w);
                    j.inspect(|&j| used[j] = true).is_some()
                })
            }
        }
        0x22 => (party.len() >= 2) == (arg != 0),
        0x23 => world.substep() == arg,
        0x24 => hero_down(world, arg),
        0x25 => world.quest_log().marked.contains(&arg) && quest_allowed(world, arg),
        0x26 => !world.quest_log().marked.contains(&arg) && quest_allowed(world, arg),
        0x27 => world.flag(arg + 0x776),
        0x28 => !world.flag(arg + 0x776),
        0x29 => world.leader_in_zone_of(world.event(0)) == Some(arg != 0),
        0x2a => world
            .character(0)
            .is_some_and(|h| u16::from(h.vocation) == op.arg),
        0x2b => world.raw(Raw::Save56a0) == u32::from(op.arg),
        0x2d => world.character(0).is_some_and(|h| {
            let i = usize::from(op.arg);
            i < 6 && h.attrs.iter().all(|&v| v <= h.attrs[i])
        }),
        0x2e => {
            // Compiled as [code, arg, operand high = limit, operand low = mode].
            let (limit, at_most) = ((op.operands[0].int() >> 16) as i32, low(op, 0) != 0);
            world.character(0).is_some_and(|h| {
                h.attrs.get(usize::from(op.arg)).is_some_and(|&v| {
                    let diff = i32::from(v) - i32::from(h.base);
                    if at_most { diff <= limit } else { limit < diff }
                })
            })
        }
        0x2f => party.first().is_some_and(|m| {
            let (n, limit) = (m.slot_bits.count_ones() as i32, i32::from(low(op, 0)));
            if arg == 0 { n < limit } else { limit <= n }
        }),
        0x30 => world.item_count(i32::from(op.arg as i16)) >= i32::from(low(op, 0)),
        0x31 => party.iter().any(|m| {
            m.standing() && u16::from(m.vocation) == op.arg && low(op, 0) <= m.level_in(arg)
        }),
        0x32 | 0x52 => equipment(op, world, mask),
        0x33 => world
            .quest_log()
            .find(arg)
            .is_some_and(|e| e.word & 0x8000 != 0),
        0x34 => {
            let (flag, mode) = halves(op, 0);
            npc() && !world.track_flag(1, flag) && session_mode(world, mode)
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
                // Talk zone == a, then condition 0x10 with b.
                0x35 => {
                    world.event(5) == u32::from(a as u16)
                        && world.raw(Raw::Field954) == u32::from(b as u16)
                }
                0x36 => holds_item(world, a),
                0x38 => hero_down(world, a),
                0x39 => flag_ext(world, a) && session_mode(world, b),
                0x3a => !flag_ext(world, a) && session_mode(world, b),
                0x3b => flag_ext(world, a),
                0x3c => !flag_ext(world, a),
                _ => session_mode(world, a),
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
        0x42 => {
            // More than `low` copies of item `high` if arg ≠ 0, at most `low` otherwise.
            let (item, _) = halves(op, 0);
            (world.item_count(item) > i32::from(low(op, 0))) == (arg != 0)
        }
        0x43 => world.flag(arg + 0x1136),
        0x44 => !world.flag(arg + 0x1136),
        0x46 => world
            .quest_log()
            .find(arg)
            .is_some_and(|e| (e.word >> 18 & 1 != 0) == (low(op, 0) != 0)),
        0x49 => {
            let value = i32::from(world.raw(Raw::Quest2ca) as i16);
            (i32::from(low(op, 0)) <= value) == (arg != 0)
        }
        // The compiler drops the arg and keeps the operand's low half.
        0x4a => world.raw(Raw::Save71dc) as u16 == low(op, 0),
        0x4b | 0x4c => world.event(9) == u32::from(low(op, 0)),
        0x4d => world.flag(0x364) && world.flag(0x365),
        0x4e => {
            // [code, quest, field = operand 0 high, value = low, mode = operand 1 low]:
            // quest-log entry halfword compared with value.
            let (field, _) = halves(op, 0);
            let index = match field {
                4 => Some(0),
                5 => Some(1),
                6 => Some(2),
                0 | 1 | 7 => Some(3),
                2 | 8 => Some(4),
                3 | 9 => Some(5),
                _ => None,
            };
            let value = low(op, 0);
            match (world.quest_log().find(arg), index) {
                (Some(e), Some(i)) => match low(op, 1) {
                    0 => value <= e.values[i],
                    1 => e.values[i] < value,
                    2 => e.values[i] == value,
                    _ => false,
                },
                _ => false,
            }
        }
        0x50 => party
            .iter()
            .any(|m| m.standing() && i32::from(m.level()) >= arg),
        0x51 => world.session().active == (arg != 0),
        0x54 => world.raw(Raw::Save64de) as u8 as i32 == arg,
        0x55 => {
            let (gold, limit) = (world.gold(), op.operands[0].int());
            if arg == 0 {
                gold < limit
            } else {
                limit <= gold
            }
        }
        0x56 => {
            // The first of characters 0..=3 with a non-zero low tag nibble decides by its
            // HP; without one, whether character 0xce exists.
            let found = (0..4)
                .filter_map(|i| world.character(i))
                .find(|h| h.tag & 0xf != 0);
            let yes = match found {
                Some(h) => h.hp != 0,
                None => world.character(0xce).is_some(),
            };
            arg == i32::from(yes)
        }
        0x57 => {
            let len = world.quest_log().entries.len();
            if arg == 0 { len < 8 } else { len == 8 }
        }
        0x58 => arg < 0x49 && world.flag(arg + 0x33e),
        0x59 => arg >= 0x49 || !world.flag(arg + 0x33e),
        0x5a => world.date() == (u32::from(op.arg), u32::from(low(op, 0))),
        0x5c => world
            .quest_log()
            .find(arg)
            .is_some_and(|e| e.word & 0x200 == 0),
        0x5d => {
            let bit = 1u32.checked_shl(u32::from(op.arg & 0xff)).unwrap_or(0);
            world.raw(Raw::Global238c) & bit != 0
        }
        0x5e => world.raw(Raw::Global237c) == u32::from(op.arg),
        0x5f => i32::from(world.raw(Raw::Global27b4) as u16) == i32::from(op.arg as i16),
        0x60 => world
            .character(world.local_character())
            .is_some_and(|h| h.levels.iter().filter(|&&l| l > 0x62).count() as i32 >= arg),
        0x62 => i32::from(world.raw(Raw::Global27d0) as u8) <= arg,
        0x63 => arg <= i32::from(world.raw(Raw::Global27d0) as u8),
        0x1f4 => party
            .iter()
            .find(|m| m.tag >> 4 != 0 && i32::from(m.tag >> 4) == arg)
            .is_some_and(|m| low(op, 0) == u16::from(!m.status_bit0)),
        0x1f5 => world.time_of_day() == u32::from(op.arg),
        0x1f6 => world.take_signal(),
        0x1f7 => world.flag(0x798) == (arg != 0),
        0x1f8 => world.date() == world.saved_date(),
        0x1f9 => world.raw(Raw::Save63dc) & 1 != 0,
        0x1fa => quest_nibble(world, arg) & 8 != 0,
        0x1fb => world.raw(Raw::Save3f8) as u16 == op.arg,
        code if UNMODELLED.contains(&code) => return None,
        // 0x18, 0x47, 0x48, 0x61 and every code without a case: false.
        _ => false,
    })
}

/// Conditions absent from the data whose state is not modelled: skill bytes (0x2c), hero
/// name (0x45), the talk task (0x4f), a loaded file (0x53), the field manager (0x5b).
pub const UNMODELLED: [u16; 5] = [0x2c, 0x45, 0x4f, 0x53, 0x5b];

/// A condition the evaluator does not implement was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported(pub u16);

/// First loaded trigger of `event` whose conditions all hold (`Trig_FindMatch`,
/// `0x020644a0`: triggers of one event are tried in file order). An unimplemented
/// condition is only an error if it is actually reached.
///
/// Two quirks are kept: the equipment mask of 0x32/0x52 is cleared once per call, not per
/// trigger, and the "all hold" flag is not reset between triggers, so a trigger without
/// conditions only matches if it comes first (never the case in the data).
pub fn first_match<'t, 'a>(
    triggers: &'t [Trigger<'a>],
    event: i32,
    world: &dyn World,
) -> std::result::Result<Option<&'t Trigger<'a>>, Unsupported> {
    let mut mask = 0u8;
    let mut holds = true;
    for trigger in triggers.iter().filter(|t| t.event == event) {
        for cond in &trigger.conditions {
            match eval(cond, world, &mut mask) {
                Some(true) => holds = true,
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

/// What `Trig_RunActions` (`0x02064540`) does after an action.
#[derive(Debug, Clone, PartialEq)]
pub enum Flow<'a> {
    Next,
    /// Stop the list: the trigger's deferred actions are then not run.
    Stop,
    /// Queue a node for `Trig_RunDeferred` (`Trig_Defer`, `0x0206446c`); composite actions
    /// queue another code.
    Defer(Op<'a>),
}

/// Gold cap of actions 0x8b (`DAT_02063014`).
pub const GOLD_MAX: u32 = 9_999_999;

/// Values whose rank (17 to 31) action 0x8e marks with flag `0xbf1 + rank` (`0x020e87d0`).
const RANKED: [i16; 32] = [
    0, 3, 2, 4, 5, 6, 7, 8, 9, 10, 11, 18, 12, 13, 14, 15, 16, 17, 19, 1, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31,
];

/// High half of operand `i`, as the compiler stores it (`asr #16`, then `strh`). Public for
/// the field's immediate 0x95/0x96 requests, which pack the placement there.
pub(crate) fn high(op: &Op, i: usize) -> u16 {
    (op.operands[i].int() >> 16) as u16
}

fn set(flags: &mut BTreeSet<i32>, index: i32, on: bool) {
    if on {
        flags.insert(index);
    } else {
        flags.remove(&index);
    }
}

/// `Flags_TestExt` numbering, for writes (`FUN_0206eb74`).
fn ext_index(index: i32) -> i32 {
    if index < 0x400 { index } else { index + 0x6fa }
}

/// Quest nibble write keeping the two flag bits (`FUN_0206e174`).
fn set_quest_state(state: &mut State, quest: i32, value: u8) {
    if (0..0xcc).contains(&quest) {
        let nibble = state.quests.entry(quest).or_default();
        *nibble = *nibble & 0xc | value & 3;
    }
}

impl State {
    fn log_entry(&mut self, quest: i32) -> Option<&mut QuestLogEntry> {
        self.quest_log
            .entries
            .iter_mut()
            .find(|e| (e.word & 0x1ff) as i32 == quest)
    }

    /// `FUN_020703d8`: move track `track` forward to (chapter, step, substep); never back.
    /// A new chapter clears both flag sets, a new step of the same chapter set 1; either
    /// also clears every conversation counter (`0x0206ec04`, both tables at
    /// `gameres+0x44c4`). A new substep alone clears nothing.
    fn advance(&mut self, track: usize, chapter: u8, step: u8, substep: u8) {
        let Some(t) = self.tracks.get_mut(track) else {
            return;
        };
        let key = |c: u8, s: u8, b: u8| u32::from(c) * 10000 + u32::from(s) * 100 + u32::from(b);
        if key(t.chapter, t.step, t.substep) >= key(chapter, step, substep) {
            return;
        }
        if t.chapter != chapter {
            t.flags = Default::default();
            self.talk_counts.clear();
        } else if t.step != step {
            t.flags[1].clear();
            self.talk_counts.clear();
        }
        (t.chapter, t.step, t.substep) = (chapter, step, substep);
    }

    /// Adds `count` copies of `item` and, on the first acquisition, sets its "item obtained"
    /// flag (`func_0207cc18`, `0x0207cc28`): `0xc76 + compendium index`, index 0 skipped.
    /// Equipped copies are modelled as one count, as condition 0x12/0x30 read them.
    pub fn obtain(&mut self, item: i32, count: i32) {
        if count <= 0 {
            return;
        }
        *self.items.entry(item).or_default() += count;
        let Ok(id) = u16::try_from(item) else {
            return;
        };
        if let Some(flag) = self
            .compendium
            .compendium(id)
            .and_then(crate::state::item_obtained_flag)
        {
            self.flags.insert(i32::from(flag));
        }
    }

    /// Bag first, then equipped copies (`FUN_02086d98`): modelled as one count.
    fn take(&mut self, item: i32, count: i32) {
        if let Some(held) = self.items.get_mut(&item) {
            *held = (*held - count.max(0)).max(0);
        }
    }
}

/// Runs one action as `Trig_RunAction` (`0x02061c14`, switch on `code - 100`) does on the
/// modelled state. Effects on what [`State`] does not hold (map objects, NPCs, menus,
/// sound, wireless synchronisation, hit points) are skipped; the returned flow is the
/// game's.
pub fn run_action<'a>(op: &Op<'a>, state: &mut State) -> Flow<'a> {
    let arg = i32::from(op.arg);
    let track = state.track;
    let defer = || Flow::Defer(op.clone());
    match op.code {
        0x64 | 0x65 => set(&mut state.flags, arg, op.code == 0x64),
        0x66..=0x69 => {
            let flags = &mut state.tracks[track].flags[usize::from(op.code >= 0x68)];
            set(flags, arg, op.code.is_multiple_of(2));
        }
        // 0x6b: the host of a session raises the session flag (`Trig_RunAction` case 7:
        // `Flags_Set(manager, manager+0x8c, *(u32*)0x02063008)`, that word being `0x113a`,
        // when the session predicate holds). The runtime has no wireless session: it is the
        // host of a solo game, so the predicate holds.
        0x6b => {
            state
                .flags
                .insert(i32::from(crate::state::SESSION_HOST_FLAG));
        }
        // Case 6: both counters of NPC `arg` cleared (`0x0206ecf8` on each table).
        0x6a => state.talk_counts.reset(op.arg),
        0x6e => {
            if op.arg < 4 {
                state.time_of_day = u32::from(op.arg);
            }
        }
        // No case in the dispatcher: the list stops.
        0x6f..=0x71 | 0x98 | 0x9a | 0xa3 => return Flow::Stop,
        // Items (0x72/0x73 take one copy: the dispatcher stores 1 as count), talk, events,
        // warps, NPC removal, progress: deferred.
        0x72..=0x80 if op.code != 0x7d && op.code != 0x7e && op.code != 0x7f => {
            return defer();
        }
        0x7d => {
            // FUN_020961c0: fails (and stops the list) with a full log or a quest that is not
            // on offer; the offer list is not modelled.
            if state.quest_log.entries.len() >= 8 {
                return Flow::Stop;
            }
            if state.log_entry(arg).is_none() {
                state.quest_log.entries.push(QuestLogEntry {
                    word: u32::from(op.arg & 0x1ff) | 0x800,
                    values: [0; 6],
                });
            }
            state.flags.insert(0x119d);
        }
        0x7f => {
            // Date stamp, state 3 (FUN_0206e110), removal from the log [hypothesis].
            set_quest_state(state, arg, 3);
            state
                .quest_log
                .entries
                .retain(|e| (e.word & 0x1ff) as i32 != arg);
        }
        0x81 => set_quest_state(state, arg, 1),
        0x82 | 0x83 => set(
            &mut state.flags,
            ext_index(i32::from(high(op, 0))),
            op.code == 0x82,
        ),
        0x84 => {
            // Leaving the track's chapter or step clears its flags right away.
            let t = &mut state.tracks[track];
            if u16::from(t.chapter) != low(op, 0) {
                t.flags = Default::default();
            }
            if u16::from(t.step) != low(op, 1) {
                t.flags[1].clear();
            }
            return defer();
        }
        0x85 | 0x89..=0x8a | 0x8d | 0x91 | 0x94 | 0xd6 | 0xe1 | 0xe2 => return defer(),
        0x86 | 0x87 => {
            let on = op.code == 0x86;
            match high(op, 0) {
                3 => set(&mut state.flags, arg, on),
                4 => set(&mut state.tracks[track].flags[0], arg, on),
                5 => set(&mut state.tracks[track].flags[1], arg, on),
                _ => {}
            }
        }
        0x8b => {
            state.gold = state
                .gold
                .saturating_add(op.operands[0].int())
                .min(GOLD_MAX)
        }
        0x8c => state.gold = state.gold.saturating_sub(op.operands[0].int()),
        0x8e => {
            let rank = RANKED
                .iter()
                .position(|&v| i32::from(v) == arg)
                .unwrap_or(0);
            if rank >= 17 {
                state.flags.insert(0xbf1 + rank as i32);
            }
        }
        0x90 => {
            if quest_state(state, arg) == 2
                && !state.session.guest
                && let Some(e) = state.log_entry(arg)
            {
                e.word = e.word & !0x3800 | u32::from(high(op, 0) & 7) << 11;
            }
        }
        0x92 | 0x93 => set(&mut state.flags, arg + 0x776, op.code == 0x92),
        0x9b => {
            // Track flag (set 1) then the event, deferred.
            state.tracks[track].flags[1].insert(i32::from(high(op, 0)));
            return defer();
        }
        0x9c | 0x9d => {
            // 0x9c completes the quest; 0x9d accepts (0), completes (2) or offers (3) it.
            let code = match (op.code, high(op, 0)) {
                (0x9c, _) => 0x7f,
                (_, 0) => 0x7d,
                (_, 2) => 0x7f,
                (_, 3) => 0x81,
                _ => return Flow::Stop,
            };
            let inner = Op {
                code,
                arg: op.arg,
                operands: Vec::new(),
            };
            run_action(&inner, state);
            return defer();
        }
        0x9e | 0x9f => {
            // Queue 0x75 (take) / 0x74 (give) with the same arg and count.
            return Flow::Defer(Op {
                code: if op.code == 0x9e { 0x75 } else { 0x74 },
                arg: op.arg,
                operands: op.operands.clone(),
            });
        }
        0xa0 => {
            state.flags.insert(arg + 0x113f);
        }
        0xa2 => set(&mut state.flags, 0x113b, arg == 0),
        0xaf => {
            state.flags.remove(&(arg + 0x1136));
        }
        0xb0 => {
            if let Some(e) = state.log_entry(arg) {
                e.word = e.word & !(1 << 17) | u32::from(high(op, 0) & 1) << 17;
            }
        }
        0xb1 => {
            if high(op, 0) < 4 {
                state.time_of_day = u32::from(high(op, 0));
            }
        }
        0xba => set(&mut state.flags, 0x1138, arg != 0),
        0xbb => {
            state.raw.insert(Raw::Quest2ca, 0);
        }
        0xbc => {
            // Quest-log value: 0 set, 1 add (capped at 0xffff), 2 subtract (floored at 0).
            let (field, value, mode) = (high(op, 0), low(op, 0), low(op, 1));
            let index = match field {
                4 => 0,
                5 => 1,
                6 => 2,
                0 | 1 | 7 => 3,
                2 | 8 => 4,
                3 | 9 => 5,
                _ => return Flow::Next,
            };
            if let Some(e) = state.log_entry(arg) {
                let v = &mut e.values[index];
                *v = match mode {
                    0 => value,
                    1 => v.saturating_add(value),
                    2 => v.saturating_sub(value),
                    _ => *v,
                };
            }
        }
        // Random quest-log values: unmodelled, but a missing quest stops the list.
        0xbe | 0xbf => {
            if state.log_entry(arg).is_none() {
                return Flow::Stop;
            }
        }
        0xca => {
            state.flags.insert(arg + 0x1198);
        }
        0xce => {
            state.flags.insert(0x79a);
        }
        0xd3 => {
            let bit = 1u32.checked_shl(u32::from(op.arg & 0xff)).unwrap_or(0);
            *state.raw.entry(Raw::Global238c).or_default() |= bit;
        }
        0xde => {
            state.raw.insert(Raw::Global27d0, 0);
        }
        0xdf => set(&mut state.flags, 0x799, arg != 0),
        0xe0 => set(&mut state.flags, 0x798, arg != 0),
        0xe6 => set(&mut state.flags, arg + 0xbea, high(op, 0) != 0),
        // Codes beyond the dispatcher's table (0xea and up).
        0xea.. => return Flow::Stop,
        _ => {}
    }
    Flow::Next
}

/// Something a deferred action asks the engine to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// 0x76: talk to NPC `npc` with this talk context.
    Talk { npc: u16, context: u16 },
    /// 0x77, 0x80, 0x9b: start event `id` (`data/event/evNNNNN.gp2` or the event lists).
    Event(u16),
    /// 0x78: start the scripted battle `n` (the `eventbattle.bin` record whose index is `n`). The
    /// game fills the battle task `+0x3718` (`Trig_RunDeferred` case 0x78, `0x0206f82c`: battle
    /// number in `+0x28`, `n + 0x8005` in `+0x24`); its end fires trigger event 15 (won) or 16
    /// (lost) with `n` in word 6 (`docs/re/trigger.md`, § Combats scriptés).
    Battle(u16),
    /// 0x79/0x89/0xe1 (room) and 0x85/0x8a/0xe2 (room and entry): leave for `room` (save
    /// `+0x3f8`, read back by condition 0x1fb); 0x7a/0x7b give the position (fixed point,
    /// 1.0 = 4096) and direction.
    Warp {
        room: u16,
        entry: Option<u16>,
        position: Option<[i32; 3]>,
        direction: Option<i16>,
    },
    /// 0x7c: remove NPC `npc` from the map (its object gets flag 0x8000).
    RemoveNpc(u16),
    /// A type 2 doorway with no destination was touched: native 584 (`0x02018b44`, called by
    /// `Field_TouchDoorways`) rotates the placement `placement` of block entry `block` by
    /// +90° and opens the way. The application animates the rotation.
    OpenDoor { block: u16, placement: u16 },
    /// 0x95/0x96: hide (`0x95`) or show (`0x96`) the map object of block entry `block` and
    /// placement `placement` (`Trig_RunAction` cases 0x31/0x32, `0x02061c14`). `block` is the
    /// low byte of the argument and `placement` the high half of operand 0, as the game's
    /// pending record stores them (`func_0206ea8c(manager, arg & 0xff, (operand + 0x80) >>
    /// 16, hidden)`); `0x95(0, 0x100000)` is block 0, placement 16, hidden. Immediate: the
    /// game applies it while the list runs (`func_02013380`), not after it.
    SetPlacementHidden {
        block: u16,
        placement: u16,
        hidden: bool,
    },
    /// 0x84, 0x94, 0xd6 ran (`Trig_RunDeferred` sets its reload flag whether the track
    /// moved or not): `Trig_LoadMap` (`0x0206462c`) reloads the triggers of the current
    /// map at the track's story point, drops the areas actions 0x8f added (`+0x494`) and
    /// fires event 20 again (`Trig_LoadTable`, `0x02064584`). The cast is not rebuilt.
    ReloadTriggers,
    /// 0x8d with a non-zero argument (`Trig_RunDeferred` queues task 0x30, `+0x3b58` of
    /// the game resources, when it is not queued yet): `ov_017::021c16cc` fades out, then
    /// `ov_017::021a2fa0` tears the map's cast down and runs `Cast_LoadMap`
    /// (`ov_017::021a2c14`) again for the current story point, and fades back in. The map,
    /// the hero and the triggers stay.
    ReloadCast,
    /// A deferred action whose effect is not modelled.
    Other(u16),
}

/// Float operand as the compiler stores positions: times 4096, rounded half away from zero.
fn fixed(value: &Value) -> i32 {
    let f = match *value {
        Value::Float(f) => f,
        Value::Int(i) => i as i32 as f32,
        Value::Str(_) => 0.0,
    } * 4096.0;
    (if f >= 0.0 { f + 0.5 } else { f - 0.5 }) as i32
}

/// Runs the deferred actions of a fired trigger (`Trig_RunDeferred`, `0x0206f82c`): applies
/// item and progress changes to `state` and returns the engine requests in order.
pub fn run_deferred(deferred: &[Op], state: &mut State) -> Vec<Request> {
    let mut requests = Vec::new();
    let (mut warp, mut position, mut direction) = (None, None, None);
    let mut reload = false;
    // 0x8d: the last argument of the list decides (`uVar5 = arg`, tested after the loop).
    let mut cast = 0;
    for op in deferred {
        let arg = i32::from(op.arg);
        match op.code {
            0x72 => state.obtain(arg, 1),
            0x73 => state.take(arg, 1),
            0x74 => state.obtain(arg, i32::from(high(op, 0) as i8)),
            0x75 => state.take(arg, i32::from(high(op, 0))),
            0x76 => requests.push(Request::Talk {
                npc: op.arg,
                context: high(op, 0),
            }),
            0x78 => requests.push(Request::Battle(op.arg)),
            0x77 | 0x80 | 0x9b => requests.push(Request::Event(op.arg)),
            0x79 | 0x89 | 0xe1 => warp = Some((op.arg, None)),
            0x85 | 0x8a | 0xe2 => warp = Some((op.arg, Some(high(op, 0)))),
            0x7a => position = Some([0, 1, 2].map(|i| fixed(&op.operands[i]))),
            0x7b => direction = Some(fixed(&op.operands[0]) as i16),
            0x7c => requests.push(Request::RemoveNpc(op.arg)),
            0x84 | 0x94 | 0xd6 => {
                let [c, s, b] = [0, 1, 2].map(|i| low(op, i) as u8);
                let tracks = match op.code {
                    0x84 => state.track..state.track + 1,
                    0x94 => 0..TRACKS,
                    _ => usize::from(op.arg)..usize::from(op.arg) + 1,
                };
                for t in tracks {
                    state.advance(t, c, s, b);
                }
                reload = true;
            }
            0x8d => cast = op.arg,
            0x9c => {
                state.take(i32::from(high(op, 0) as i16), 1);
                state.obtain(i32::from(low(op, 0) as i16), 1);
            }
            0x9d => state.obtain(i32::from(low(op, 0) as i16), 1),
            code => requests.push(Request::Other(code)),
        }
    }
    if let Some((room, entry)) = warp {
        requests.push(Request::Warp {
            room,
            entry,
            position,
            direction,
        });
    }
    // After the list, in the game's order: the trigger reload, then the cast task.
    if reload {
        requests.push(Request::ReloadTriggers);
    }
    if cast != 0 {
        requests.push(Request::ReloadCast);
    }
    requests
}

/// A trigger that fired (`Trig_Fire`, `0x020649c0`).
#[derive(Debug, Clone, PartialEq)]
pub struct Fired<'t, 'a> {
    pub trigger: &'t Trigger<'a>,
    /// Whether the action list ran to its end; callers only run the deferred actions then.
    pub completed: bool,
    pub deferred: Vec<Op<'a>>,
}

/// Fires `event`: the first matching trigger's actions run on `state` until one stops the
/// list.
pub fn fire<'t, 'a>(
    triggers: &'t [Trigger<'a>],
    event: i32,
    state: &mut State,
) -> std::result::Result<Option<Fired<'t, 'a>>, Unsupported> {
    let Some(trigger) = first_match(triggers, event, state)? else {
        return Ok(None);
    };
    let mut deferred = Vec::new();
    let mut completed = true;
    for action in &trigger.actions {
        match run_action(action, state) {
            Flow::Next => {}
            Flow::Defer(op) => deferred.push(op),
            Flow::Stop => {
                completed = false;
                break;
            }
        }
    }
    Ok(Some(Fired {
        trigger,
        completed,
        deferred,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::table::Table;
    use dq9_formats::trigger::parse;

    /// Talking to `npc` with quest 11 in state `quest11`.
    fn w(quest11: u8, npc: u32) -> State {
        let mut s = State::default();
        s.quests.insert(11, quest11);
        s.event[0] = npc;
        s
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

        let t = first_match(&triggers, 0, &w(1, 18)).unwrap().unwrap();
        assert_eq!(t.talk(), Some((18, 192)));
        let t = first_match(&triggers, 0, &w(2, 18)).unwrap().unwrap();
        assert_eq!(t.talk(), Some((18, 193)));
        assert_eq!(first_match(&triggers, 0, &w(1, 19)), Ok(None));
    }

    #[test]
    fn unsupported_conditions_only_fail_when_reached() {
        let data = table(&[&[1, 1, 1, 1, 1, 0, word(6, 5), word(0x45, 0), 0]]);
        let table = Table::parse(&data).unwrap();
        let triggers = parse(&table).unwrap();
        assert_eq!(first_match(&triggers, 0, &w(0, 4)), Ok(None));
        assert_eq!(first_match(&triggers, 0, &w(0, 5)), Err(Unsupported(0x45)));
    }

    /// 0x72 raises the item's compendium flag (`func_0207cc18`) and 0x6b the session flag.
    #[test]
    fn item_and_session_actions_set_their_flags() {
        // One category member with item 22159 (benevolessence) → compendium 1093.
        let mut member = vec![0u8; 12 + 32];
        member[..2].copy_from_slice(&1u16.to_le_bytes());
        member[12 + 0x18..12 + 0x1a].copy_from_slice(&22159u16.to_le_bytes());
        let compendium_word = 1093u32 << 12;
        member[12 + 0xc..12 + 0x10].copy_from_slice(&compendium_word.to_le_bytes());
        let table = dq9_formats::item::ItemTable::from_members(&[&member]).unwrap();

        let data = one(&[word(0x6b, 0), word(0x72, 22159)]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        let mut s = w(0, 3);
        s.compendium = table;
        let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
        assert!(
            s.flags
                .contains(&i32::from(crate::state::SESSION_HOST_FLAG))
        );
        assert!(!s.flags.contains(&4283), "the give is deferred");
        assert_eq!(run_deferred(&fired.deferred, &mut s), []);
        assert_eq!(s.items.get(&22159), Some(&1));
        assert!(s.flags.contains(&4283), "0xc76 + compendium 1093");
        // An item absent from the table is still held, without a flag.
        let data = one(&[word(0x72, 22158)]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
        assert_eq!(run_deferred(&fired.deferred, &mut s), []);
        assert_eq!(s.items.get(&22158), Some(&1));
        assert!(!s.flags.contains(&4282));
    }

    /// 0x6b is not 0x6c: it must not mark a doorway or stop the list.
    #[test]
    fn session_action_does_not_stop_the_list() {
        let data = one(&[word(0x6b, 0), word(0x64, 9)]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        let mut s = w(0, 3);
        let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
        assert!(fired.completed);
        assert!(s.flags.contains(&9));
    }

    /// 0x32: node 1 (arg, op0, op1 high) remembers the members it accepts in a mask that
    /// lives for the whole event; node 2 (op1 low, op2, op3 low) tests the remembered ones.
    #[test]
    fn equipment_mask_spans_triggers() {
        let mut a = Member {
            hp: 1,
            ..Member::default()
        };
        a.slots[0] = 100;
        a.slots[7] = 7;
        let mut b = a;
        b.slots[0] = 200;
        b.slots[7] = 8;
        let state = State {
            party: vec![a, b],
            ..State::default()
        };
        let head = [1, 1, 1, 1, 1, 0];
        // Slot 0 == 200 (b) and slot 7 == 7 (a): fails, but b stays in the mask.
        let first = [&head[..], &[word(0x32, 200), 0, 7, 0, 0]].concat();
        // Slot 0 == 999 (nobody) and slot 7 == 8: holds through b, remembered above.
        let second = [&head[..], &[word(0x32, 999), 0, 8, 0, 0]].concat();
        let data = table(&[&first, &second]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        assert_eq!(
            first_match(&triggers, 0, &state).unwrap(),
            Some(&triggers[1])
        );
        let data = table(&[&second]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        assert_eq!(first_match(&triggers, 0, &state), Ok(None));
    }

    #[test]
    fn angles_are_compiled_like_the_game() {
        assert_eq!(degrees_to_angle(0), 0);
        assert_eq!(degrees_to_angle(135), 9650);
        // Brought back once into 0..=2π.
        assert_eq!(degrees_to_angle(400), degrees_to_angle(40));
        assert_eq!(degrees_to_angle(-90), degrees_to_angle(270));
    }

    /// 0x29 is false either way when the NPC is not on the map.
    #[test]
    fn zone_condition_needs_the_npc() {
        let op = |arg| Op {
            code: 0x29,
            arg,
            operands: vec![],
        };
        let mut s = w(0, 7);
        let mut mask = 0;
        assert_eq!(eval(&op(0), &s, &mut mask), Some(false));
        assert_eq!(eval(&op(1), &s, &mut mask), Some(false));
        s.zones.insert(7, false);
        assert_eq!(eval(&op(0), &s, &mut mask), Some(true));
        s.zones.insert(7, true);
        assert_eq!(eval(&op(1), &s, &mut mask), Some(true));
    }

    fn one(words: &[u32]) -> Vec<u8> {
        let mut record = vec![1, 1, 1, 9, 9, 0, word(6, 3)];
        record.extend_from_slice(words);
        table(&[&record])
    }

    /// An action that stops the list drops what was already deferred.
    #[test]
    fn stopping_drops_deferred_actions() {
        let data = one(&[word(0x64, 5), word(0x76, 3), 0x00c0_0000, word(0x6f, 0)]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        let mut s = w(0, 3);
        let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
        assert!(!fired.completed);
        assert!(s.flags.contains(&5), "actions before the stop ran");
        let data = one(&[word(0x76, 3), 0x00c0_0000]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
        assert!(fired.completed);
        assert_eq!(
            run_deferred(&fired.deferred, &mut s),
            [Request::Talk {
                npc: 3,
                context: 192
            }]
        );
    }

    /// Progress only moves forward; a new chapter clears both flag sets of the track, a new
    /// step only set 1.
    #[test]
    fn progress_moves_forward_and_scopes_track_flags() {
        let mut s = w(0, 3);
        s.track = 2;
        s.tracks[2] = Track {
            chapter: 4,
            step: 2,
            substep: 0,
            flags: [[7].into(), [8].into()],
        };
        let advance = |c, st, b| one(&[word(0x84, 0), c, st, b]);
        let run = |data: &[u8], s: &mut State| {
            let triggers = parse(&Table::parse(data).unwrap()).unwrap();
            let fired = fire(&triggers, 0, s).unwrap().unwrap();
            run_deferred(&fired.deferred, s)
        };
        // Same chapter, next step: set 1 cleared.
        assert_eq!(run(&advance(4, 3, 0), &mut s), [Request::ReloadTriggers]);
        assert_eq!(s.tracks[2].flags, [[7].into(), BTreeSet::new()]);
        // Backwards: no change, but the game reloads the triggers all the same.
        assert_eq!(run(&advance(4, 1, 0), &mut s), [Request::ReloadTriggers]);
        assert_eq!(s.tracks[2].step, 3);
        // Other tracks are untouched.
        assert_eq!(s.tracks[0], Track::default());
        assert_eq!(track_of_room(201), 3);
    }

    /// A new step or chapter clears every conversation counter (`Track_Advance` calls
    /// `0x0206ec04`); a new substep alone, or a move backwards, leaves them.
    #[test]
    fn a_new_step_clears_the_talk_counters() {
        let mut s = w(0, 3);
        s.tracks[0] = Track {
            chapter: 4,
            step: 2,
            substep: 0,
            ..Track::default()
        };
        let run = |data: &[u8], s: &mut State| {
            let triggers = parse(&Table::parse(data).unwrap()).unwrap();
            let fired = fire(&triggers, 0, s).unwrap().unwrap();
            run_deferred(&fired.deferred, s)
        };
        let advance = |c, st, b| one(&[word(0x84, 0), c, st, b]);
        s.talk_counts.record(7);
        s.talk_counts.record(9);
        // Same step, next substep: kept.
        run(&advance(4, 2, 1), &mut s);
        assert_eq!((s.tracks[0].step, s.tracks[0].substep), (2, 1));
        assert_eq!(s.talk_counts.get(7), [1, 1]);
        // Backwards: kept.
        run(&advance(4, 1, 5), &mut s);
        assert_eq!(s.talk_counts.get(9), [1, 1]);
        // Next step: cleared.
        run(&advance(4, 3, 0), &mut s);
        assert_eq!(s.talk_counts.get(7), [0, 0]);
        assert_eq!(s.talk_counts.get(9), [0, 0]);
        // Next chapter: cleared too.
        s.talk_counts.record(7);
        run(&advance(5, 1, 0), &mut s);
        assert_eq!(s.talk_counts.get(7), [0, 0]);
    }

    /// Action 0x6a clears both counters of one NPC (`Trig_RunAction` case 6).
    #[test]
    fn action_0x6a_clears_one_npc_s_talk_counters() {
        let mut s = w(0, 3);
        s.talk_counts.record(7);
        s.talk_counts.record(9);
        let data = one(&[word(0x6a, 7)]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        fire(&triggers, 0, &mut s).unwrap().unwrap();
        assert_eq!(s.talk_counts.get(7), [0, 0]);
        assert_eq!(s.talk_counts.get(9), [1, 1]);
    }

    /// Action 0x8d queues the cast rebuild when its argument is not 0 (`Trig_RunDeferred`,
    /// tested after the loop), after the trigger reload of the same list.
    #[test]
    fn action_0x8d_asks_for_the_cast_rebuild_after_the_trigger_reload() {
        let run = |data: &[u8]| {
            let mut s = w(0, 3);
            let triggers = parse(&Table::parse(data).unwrap()).unwrap();
            let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
            run_deferred(&fired.deferred, &mut s)
        };
        assert_eq!(run(&one(&[word(0x8d, 1)])), [Request::ReloadCast]);
        assert_eq!(run(&one(&[word(0x8d, 0)])), []);
        assert_eq!(
            run(&one(&[word(0x8d, 1), word(0x84, 0), 1, 2, 1])),
            [Request::ReloadTriggers, Request::ReloadCast]
        );
    }

    /// Actions 0x95/0x96 (`Trig_RunAction` cases 0x31/0x32, `0x02061c14`) hide or show a map
    /// object immediately, so a fired field trigger emits them in list order, before the
    /// deferred requests. The packed fields are the game's: block = argument low byte,
    /// placement = high half of operand 0.
    #[test]
    fn field_triggers_emit_placement_hide_and_show() {
        use crate::field::{Event, Field};
        use dq9_formats::map::Regions;

        let record = [
            4101,
            1,
            1,
            1,
            1,
            11,
            word(0x95, 0x1234), // hide: block entry 0x34
            0x0010_0000,        // placement 16
            word(0x96, 0x01ff), // show: block entry 0xff
            0x0007_0000,        // placement 7 (high half, like the game's operand)
            word(0x77, 5000),   // deferred: start event 5000
        ];
        let regions = Regions {
            warps: Vec::new(),
            zones: Vec::new(),
        };
        let mut field = Field::new(4101, &regions, Some(table(&[&record]))).unwrap();
        let mut state = State::default();
        (
            state.tracks[0].chapter,
            state.tracks[0].step,
            state.tracks[0].substep,
        ) = (1, 1, 2);
        assert_eq!(
            field.event_ended(1, &mut state),
            vec![
                Event::Request(Request::SetPlacementHidden {
                    block: 0x34,
                    placement: 16,
                    hidden: true,
                }),
                Event::Request(Request::SetPlacementHidden {
                    block: 0xff,
                    placement: 7,
                    hidden: false,
                }),
                Event::Request(Request::Event(5000)),
            ]
        );
    }

    #[test]
    fn gold_is_capped() {
        let data = one(&[word(0x8b, 0), 9_999_000, word(0x8b, 0), 5000]);
        let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
        let mut s = w(0, 3);
        fire(&triggers, 0, &mut s).unwrap();
        assert_eq!(s.gold, GOLD_MAX);
    }
}
