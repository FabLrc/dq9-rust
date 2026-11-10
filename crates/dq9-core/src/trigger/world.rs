//! Game state the trigger conditions read: characters, quest log, scenario tracks and
//! [`State`], the plain-data implementation of [`World`] (`docs/re/trigger.md`).
//!
//! port: 0x02064ba8

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use dq9_formats::dialogue::Session;

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
    pub(super) fn standing(&self) -> bool {
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
    /// Character `id` of the character table (`Chara_Get`, `0x0200fd80`: 233 slots,
    /// `GameState + 8 + id × 4`, 0 = the hero), for ids 0 to 3 (one per player of a session
    /// **[hypothesis]**; 0 = the hero) and [`GUEST_CHARACTER`] (`0xce`), which condition
    /// 0x56 only tests for non-null.
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

/// Character-table slot of the field guest (`Chara_Get(0xce)`): the object `Guest_LoadTask`
/// (`ov_017::021c1e6c`) registers after trigger action `0xcd`, removed by `0xcc`
/// (`Guest_Dismiss`, `ov_017::02191484`) and cleared with `0xc9..0xce` by `Cast_ClearNpcs`
/// (`ov_017::021a316c`, the first step of `Cast_Rebuild`, which queues `Guest_Start` again
/// at `ov_017::021a2fa0+444`). Condition 0x56 tests only whether the slot is non-null
/// (`0x020617b4`), so what this table holds for the guest is an existence marker, not a
/// member: the object itself is a `0x2d4`-byte guest task, not a character model.
pub const GUEST_CHARACTER: usize = 0xce;

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

impl State {
    /// Trigger action `0xcd` (`Guest_Start`, `ov_017::021913d0`, record `> 0`): the guest
    /// task takes slot `0xce`, which condition 0x56 reads as "a guest is travelling with
    /// the party". The marker keeps the port's character table shaped like the game's; no
    /// member is invented for it (the object is not a character).
    pub fn attach_guest(&mut self) {
        self.characters.insert(GUEST_CHARACTER, Member::default());
    }

    /// Trigger action `0xcc` (`Guest_Dismiss`, `ov_017::02191484`): the guest leaves and its
    /// slot is cleared.
    pub fn dismiss_guest(&mut self) {
        self.characters.remove(&GUEST_CHARACTER);
    }

    /// Whether a guest holds slot `0xce` (condition 0x56's fallback).
    pub fn guest_present(&self) -> bool {
        self.characters.contains_key(&GUEST_CHARACTER)
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
