//! DS save file (`.sav`, 64 KiB EEPROM image): two slots of 32 KiB, each guarded by three
//! CRC-32 checksums. Layout, evidence and open questions in `docs/re/save.md`.
//!
//! Offsets come from the public layout of the DQIX save editor (read, not copied) and are
//! checked on a real save: all six checksums recompute, and the typed fields below read
//! plausible values. Everything not listed here stays reachable through [`Slot::bytes`].
//!
//! The game's own save code is not ported yet (it cites no original function): this module
//! is the file format, not the `GameState` serialization.

use core::ops::Range;

use nds_rom::{Error, Result};

/// Size of the whole EEPROM image.
pub const FILE_SIZE: usize = 0x10000;
/// Size of one slot.
pub const SLOT_SIZE: usize = 0x8000;
/// Number of slots in the image.
pub const SLOT_COUNT: usize = 2;

/// Index of the **suspend** (intermission) slot: `Save_LoadSlot` (`0x020aaf94`) receives
/// `intermission` 1 for it and `Save_WriteStep` (`0x020aad2c`) writes it at `0x8010`, while a
/// normal save goes to slot 0. The field menu's Quick Save is its only writer
/// (`ov_002::02169efc`, `docs/re/save.md` § "The suspend slot").
pub const SUSPEND_SLOT: usize = 1;

/// First 15 bytes of the file, followed by a zero byte. Only at offset 0 of the image.
pub const MAGIC: &[u8; 15] = b"DRAGON QUEST IX";

const INTERMISSION_FLAG: usize = 20;
const EXISTS_FLAG: usize = 21;
const VERSION: usize = 24;
/// Header A: CRC (4 bytes) then the 0x10 bytes it covers, at `slot+0x10`
/// (`Save_WriteHeaderA`, `0x020aba6c`).
const HEADER_A: usize = 16;
const HEADER_A_LEN: usize = 0x14;
/// Chapter, step and substep words, then flag data (`Save_Serialize`, block `0x2d30`).
const PROGRESS: usize = 11568;
/// Event block: `PROGRESS + 16`, 0x334 bytes (`Save_Serialize`, `func_020a99d0`).
const EVENT_FLAGS: usize = PROGRESS + 16;
/// Size of the saved event block (`func_020a99d0`).
pub const EVENT_FLAGS_SIZE: usize = 0x334;
const TRACK_COUNT: usize = 5;
const TRACK_SIZE: usize = 0x1c;
const GLOBAL_FLAGS: usize = TRACK_COUNT * TRACK_SIZE;
/// Size of the global flag array (4,512 bits, `Flags_Test` indices; `+0x8c` of the block).
pub const GLOBAL_FLAGS_SIZE: usize = 0x234;
const TRACK_SELECTOR: usize = 0x332;
/// Quest nibbles at `+0x2cc`: two per byte, low nibble first, 204 quests.
const QUEST_STATES: usize = 0x2cc;
/// Number of quests in the event block (`+0x2cc`, two quartets per byte).
pub const QUEST_COUNT: usize = 0xcc;

/// Characters stored per slot (hero, standby and party members, 8 at the inn + 4 in party
/// + 1 spare in the editor's reading).
pub const CHARACTER_COUNT: usize = 13;
/// Number of vocations, Guardian included.
pub const VOCATION_COUNT: usize = 13;
/// Length of a name, in the game's own encoding.
pub const NAME_LEN: usize = 10;
/// Seeds per vocation: strength, agility, resilience, deftness, charm, magical mending,
/// magical might, max HP, max MP (in that order, 10 bits each).
pub const SEEDS_PER_VOCATION: usize = 9;

const CHARACTER_OFFSET: usize = 136;
const CHARACTER_SIZE: usize = 572;
const STANDBY_COUNT: usize = 7572;
const PARTY_ORDER: usize = 7573;
const PARTY_COUNT: usize = 7577;
const HELD_ITEMS: usize = 7578;
const HELD_ITEMS_STRIDE: usize = 18;
const AREA: usize = 11476;
const WARP_MAP: usize = 27104;
const WARP_ANGLE: usize = 27106;
const WARP_POSITION: usize = 27108;
const DAY_TIMER: usize = 32;
const GOLD_ON_HAND: usize = 11448;
const GOLD_IN_BANK: usize = 11452;
const MINI_MEDALS: usize = 11460;
const SPECIAL_GUESTS: usize = 11528;
const VISITED_LOCATIONS: usize = 11788;
const PARTY_TRICKS: usize = 12108;
const UNLOCKED_VOCATIONS: usize = 12276;
const PLAY_TIME: usize = 16024;

// Offsets inside a character record (572 bytes), shared by the readers and the setters. The
// evidence for them is `docs/re/save.md` § "Fields typed by `dq9_formats::save`".
const NAME: usize = 320;
const VOCATION: usize = 80;
const LEVELS: usize = 2;
const REVOCATIONS: usize = 15;
const EXPERIENCE: usize = 28;
const SEEDS: usize = 88;
/// Three words of three 10-bit seeds per vocation.
const SEED_STRIDE: usize = 12;
const SKILL_POINTS: usize = 244;
const PROFICIENCY_FLAGS: usize = 282;
const FACE: usize = 356;
const HAIR_STYLE: usize = 358;
const APPEARANCE: usize = 372;
const HAIR_COLOR: usize = 373;
const BODY: usize = 376;

/// Bytes `Save_WriteStep` state 2 copies from slot 0 to slot 1 on a normal save (`0x6fe4`
/// at `0x10` / `0x8010`), i.e. the slot minus its first 16 bytes.
const MIRROR_LEN: usize = 0x6fe4;

/// The bag inside the `Main` block: 12 header bytes (two runtime pointers, the capacity
/// `u16` at `+8`, then two more), the ids (`u16`) and their quantities (`u8`). Same block as
/// `dq9_core::party::Bag` reads (`docs/re/party.md`); the offsets live here because this
/// module owns the file layout.
const BAG: usize = 7652;
const BAG_SIZE: usize = 0x1d4;
const BAG_HEADER: usize = 12;

/// The record's derived stat block (`+0x14c`): fourteen 10-bit fields, three per word, of
/// which 7 and 8 are the maximum HP and MP `PartyMember_LoadRecord` writes into the member
/// (`dq9_core::party`). The base attributes are fields 0 to 6.
const STAT_BLOCK: usize = 0x14c;
const MAX_HP_FIELD: usize = 7;
const MAX_MP_FIELD: usize = 8;

/// Value of an empty item word.
pub const NO_ITEM: u16 = 0xffff;

/// Where an intermission save resumes (`Save_UnpackWarp` copies it to `Save_Warp`, with the
/// "use this position" byte `+7` set; `ov_017::0219c598` then reads it instead of the map's
/// start point).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warp {
    /// Map id (`Save_Warp+0`).
    pub map: u16,
    /// Heading of the party, `fx32` radians (`Save_Warp+0x1c`, signed halfword).
    pub angle: i16,
    /// World position, `fx32` (`Save_Warp+0x10`).
    pub position: [i32; 3],
}

/// The blocks `Save_Serialize` (`0x020a9ec8`) writes, in file order. Together they tile the
/// range covered by checksum B exactly (checked by a test). Sizes come from the copy lengths
/// in `Save_Serialize` / `Save_LoadSlot`; the notes on each are in `docs/re/save.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// Characters, standby/party lists, held items, bag, gold, area (`Save_UnpackMain`).
    Main,
    /// Four progress words, then the event block (see [`EventFlags`]).
    Progress,
    /// `QuestLog` state (`0xa0` bytes).
    QuestLog,
    /// `QuestLog+0x178` (`0x330` bytes).
    QuestTimers,
    /// One byte, then 400 bytes to `GameState+0x5cdc` (`Save_UnpackHints`).
    Hints,
    /// `GameState+0x75f0`.
    Monsters,
    /// `GameState+0x7ac0`.
    Alchemy,
    /// `GameState+0x7504`.
    Accolades,
    /// `Zone3D+0x840` state and five fields (`Save_UnpackZone`).
    Zone,
    /// Two 512-byte messages and a word (`Save_UnpackDqvcMessages`).
    DqvcMessages,
    /// 99 treasure maps of 0x1c bytes after a 2-byte header (`Save_UnpackTreasureMaps`).
    TreasureMaps,
    /// `GameState+0x569c`.
    Profile,
    /// Warp destination, only restored from a suspend save (`Save_UnpackWarp`).
    Warp,
    /// `GameState+0x71fc`.
    Dqvc,
    /// `GameState+0x5e6c`.
    Misc,
    /// Suspend-only data (`Save_UnpackSuspend`); a short list when read by a normal load.
    Suspend,
}

impl Block {
    pub const ALL: [Self; 16] = [
        Self::Main,
        Self::Progress,
        Self::QuestLog,
        Self::QuestTimers,
        Self::Hints,
        Self::Monsters,
        Self::Alchemy,
        Self::Accolades,
        Self::Zone,
        Self::DqvcMessages,
        Self::TreasureMaps,
        Self::Profile,
        Self::Warp,
        Self::Dqvc,
        Self::Misc,
        Self::Suspend,
    ];

    /// Offset inside the slot.
    pub const fn start(self) -> usize {
        match self {
            Self::Main => 136,
            Self::Progress => 11568,
            Self::QuestLog => 12404,
            Self::QuestTimers => 12564,
            Self::Hints => 13380,
            Self::Monsters => 13784,
            Self::Alchemy => 15016,
            Self::Accolades => 15964,
            Self::Zone => 16200,
            Self::DqvcMessages => 23176,
            Self::TreasureMaps => 24204,
            Self::Profile => 26980,
            Self::Warp => 27104,
            Self::Dqvc => 27132,
            Self::Misc => 27840,
            Self::Suspend => 28112,
        }
    }

    /// Size in bytes: up to the next block (the serializer leaves no gap between them; the
    /// few padding bytes belong to the block before).
    pub const fn len(self) -> usize {
        let next = match self {
            Self::Main => 11568,
            Self::Progress => 12404,
            Self::QuestLog => 12564,
            Self::QuestTimers => 13380,
            Self::Hints => 13784,
            Self::Monsters => 15016,
            Self::Alchemy => 15964,
            Self::Accolades => 16200,
            Self::Zone => 23176,
            Self::DqvcMessages => 24204,
            Self::TreasureMaps => 26980,
            Self::Profile => 27104,
            Self::Warp => 27132,
            Self::Dqvc => 27840,
            Self::Misc => 28112,
            Self::Suspend => 28644,
        };
        next - self.start()
    }

    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub const fn range(self) -> core::ops::Range<usize> {
        self.start()..self.start() + self.len()
    }
}

/// One of the three CRC-32 checksums of a slot. Each covers a byte range and is stored
/// just before it (`A`, then `C`, then `B`, in file order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checksum {
    /// Stored at 16, covers `20..36` (flags and header words).
    A,
    /// Stored at 36, covers `40..132`.
    C,
    /// Stored at 132, covers `136..28644` (characters, bag, records, guests…).
    B,
}

impl Checksum {
    pub const ALL: [Self; 3] = [Self::A, Self::C, Self::B];

    /// Offset of the stored little-endian CRC.
    pub const fn stored_at(self) -> usize {
        match self {
            Self::A => 16,
            Self::C => 36,
            Self::B => 132,
        }
    }

    /// Covered byte range, relative to the slot.
    pub const fn range(self) -> core::ops::Range<usize> {
        match self {
            Self::A => 20..36,
            Self::C => 40..132,
            Self::B => 136..28644,
        }
    }
}

/// A stored checksum next to the one the data gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChecksumStatus {
    pub which: Checksum,
    pub stored: u32,
    pub computed: u32,
}

impl ChecksumStatus {
    pub fn is_valid(&self) -> bool {
        self.stored == self.computed
    }
}

/// What a slot header says, in the order `Save_SlotStatus` (`0x020ab9d0`) tests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotStatus {
    /// The header CRC does not match (the game's status 2).
    BadHeaderChecksum,
    /// The empty flag is clear (status 3).
    Empty,
    /// An intermission (suspend) save (status 4).
    Intermission,
    /// A normal save (status 0).
    Valid,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn put_u16(b: &mut [u8], at: usize, value: u16) {
    b[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(b: &mut [u8], at: usize, value: u32) {
    b[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// A whole save image. Parsing keeps the bytes as they are, so [`Save::as_bytes`] always
/// gives back the input until a slot is edited and [`SlotMut::seal`]ed.
#[derive(Clone, PartialEq, Eq)]
pub struct Save {
    data: Vec<u8>,
}

impl core::fmt::Debug for Save {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Save")
            .field("len", &self.data.len())
            .finish()
    }
}

impl Save {
    /// Accepts exactly [`FILE_SIZE`] bytes. Checksums are not required to be valid (a slot
    /// being written may not be): ask [`Slot::checksums`].
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() != FILE_SIZE {
            return Err(Error::Malformed("save image is not 64 KiB"));
        }
        Ok(Self {
            data: data.to_vec(),
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Slot `index` (0 or 1), or `None` past [`SLOT_COUNT`].
    pub fn slot(&self, index: usize) -> Option<Slot<'_>> {
        let start = index
            .checked_mul(SLOT_SIZE)
            .filter(|_| index < SLOT_COUNT)?;
        Some(Slot(&self.data[start..start + SLOT_SIZE]))
    }

    pub fn slot_mut(&mut self, index: usize) -> Option<SlotMut<'_>> {
        let start = index
            .checked_mul(SLOT_SIZE)
            .filter(|_| index < SLOT_COUNT)?;
        Some(SlotMut(&mut self.data[start..start + SLOT_SIZE]))
    }

    /// Whether the file starts with [`MAGIC`] (`Save_CheckMagic`, `0x020ac920`, tests only
    /// the first 16 bytes of the whole EEPROM, not of each slot).
    pub fn has_magic(&self) -> bool {
        self.data[..MAGIC.len()] == MAGIC[..] && self.data[MAGIC.len()] == 0
    }

    /// First slot whose header says [`SlotStatus::Valid`] and whose three checksums hold:
    /// the normal adventure log. It is the slot the title screen's Continue reads
    /// (`ov_004::0216d504` passes 0, and 0 for the intermission flag, to `Save_LoadSlot`).
    pub fn current_slot(&self) -> Option<usize> {
        (0..SLOT_COUNT).find(|&i| {
            self.slot(i)
                .is_some_and(|s| s.status() == SlotStatus::Valid && s.checksums_valid())
        })
    }

    /// The slot a start-up resumes: the normal log ([`Save::current_slot`]) when the container
    /// has one, else the suspend save — [`SUSPEND_SLOT`], when its header says
    /// [`SlotStatus::Intermission`] and its three checksums hold.
    ///
    /// `Save_LoadSlot` (`0x020aaf94`) decides nothing: its only selector is the `intermission`
    /// argument (its second argument is 0 in every caller and unused in the body), which picks
    /// slot 1 (header `0x8010`, blocks `0x8084`) or slot 0 (`0x84`). Its callers are the title
    /// screen's Continue (`ov_004::0216d504`: intermission 0) and its quick-save entry
    /// (`ov_004::0216e490`: intermission 1). The port has no title menu and folds the two
    /// entries into one start-up, so the Continue slot wins and the suspend save is the
    /// fallback; the title screen itself ranks a present suspend save first
    /// (`docs/re/save.md`, § "The suspend slot").
    ///
    /// Note that slot 1 is `Valid` on a cartridge that has only seen normal saves: it holds the
    /// mirror copy of slot 0, an adventure log, and [`Save::current_slot`] may take it when
    /// slot 0 is empty or unreadable (`Save_WriteStep` state 2 writes it, `0x020aad2c`).
    pub fn resume_slot(&self) -> Option<usize> {
        self.current_slot().or_else(|| {
            (0..SLOT_COUNT).find(|&index| {
                self.slot(index).is_some_and(|slot| {
                    slot.status() == SlotStatus::Intermission && slot.checksums_valid()
                })
            })
        })
    }

    /// Rewrite the mirror copy of slot 0, as `Save_WriteStep` (`0x020aad2c`) state 2 does for
    /// a **normal** save: slot 0 is written at `0x10`, then the same image again at `0x8010`,
    /// so the first 16 bytes of slot 1 (where no header lives) are left alone. Returns
    /// `false` when slot 0 holds no normal save (empty or intermission), which the game does
    /// not copy. Evidence: the demo save, whose slot 1 equals slot 0 except those 16 bytes.
    pub fn mirror_normal_slot(&mut self) -> bool {
        let header = Slot(&self.data[..SLOT_SIZE]);
        if !header.exists() || header.is_intermission() {
            return false;
        }
        self.data.copy_within(16..16 + MIRROR_LEN, SLOT_SIZE + 16);
        true
    }

    /// Prepares the image a **normal** save serializes into and returns the slot it writes,
    /// as `Save_WriteStep` (`0x020aad2c`) does before `Save_Serialize` (`0x020a9ec8`).
    ///
    /// State 0 loads slot 1's bytes (`0x8000`, `0x6fe4` bytes) into the buffer that states 1
    /// and 2 write back from `+0x10` to slot 0's `0x10` and slot 1's `0x8010`; every block
    /// serialization does not rewrite therefore keeps slot 1's bytes. A container that has
    /// an adventure log keeps today's target, [`Save::current_slot`]: the write starts from
    /// that slot's own bytes (states 1/2 then mirror them). A container that has **only** a
    /// suspend save — a Quick Save resumed before any Confession — has no log, and the
    /// original's normal save writes slot 0 whatever the slots hold (state 1's literal
    /// destination `0x10`): slot 0 is seeded from slot 1's image, so the new log carries the
    /// suspend slot's opaque blocks (quest log, monsters, grotto maps, zone), which are that
    /// same live state, and slot 1 becomes the mirror copy afterwards.
    ///
    /// `None` when there is neither an adventure log nor a sealed intermission slot to take
    /// the opaque blocks from: a fresh container's blocks are not modelled.
    pub fn prepare_normal_write(&mut self) -> Option<usize> {
        if let Some(index) = self.current_slot() {
            return Some(index);
        }
        let seedable = self.slot(SUSPEND_SLOT).is_some_and(|slot| {
            slot.status() == SlotStatus::Intermission && slot.checksums_valid()
        });
        if !seedable {
            return None;
        }
        // State 0's read at `0x8000` and state 1's write from `+0x10` to `0x10`: the two
        // images are aligned from header A on, and a slot's first 16 bytes are never written.
        self.data
            .copy_within(SLOT_SIZE + 16..SLOT_SIZE + 16 + MIRROR_LEN, 16);
        Some(0)
    }
}

/// Read-only view of one 32 KiB slot.
#[derive(Clone, Copy)]
pub struct Slot<'a>(&'a [u8]);

impl core::fmt::Debug for Slot<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Slot")
            .field("status", &self.status())
            .finish()
    }
}

impl<'a> Slot<'a> {
    pub fn bytes(&self) -> &'a [u8] {
        self.0
    }

    pub fn checksums(&self) -> [ChecksumStatus; 3] {
        Checksum::ALL.map(|which| ChecksumStatus {
            which,
            stored: u32_at(self.0, which.stored_at()),
            computed: crc32fast::hash(&self.0[which.range()]),
        })
    }

    pub fn checksums_valid(&self) -> bool {
        self.checksums().iter().all(ChecksumStatus::is_valid)
    }

    /// Byte 20 of the header (`slot+0x14`). `Save_SlotStatus` (`0x020ab9d0`) answers
    /// "intermission save" when it is 1; the game sets it from the `intermission` argument
    /// of `Save_Serialize` (`0x020a9ec8`).
    pub fn is_intermission(&self) -> bool {
        self.0[INTERMISSION_FLAG] != 0
    }

    /// Byte 21 of the header (`slot+0x15`): zero means an empty slot for `Save_SlotStatus`.
    pub fn exists(&self) -> bool {
        self.0[EXISTS_FLAG] & 1 != 0
    }

    /// Format version string at `slot+0x18` (`b"save013"` for this game; the game rejects
    /// `b"save011"` in `Save_ReadHeaderA`, `0x020a9508`).
    pub fn version(&self) -> &'a [u8] {
        let raw = &self.0[VERSION..VERSION + 8];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        &raw[..end]
    }

    /// Mirrors `Save_SlotStatus` (`0x020ab9d0`) on the header, minus its I/O and the
    /// held-key test: header CRC, then the empty flag, then the intermission flag. The two
    /// flags are read as booleans here (`exists` = byte 21 bit 0, `is_intermission` = byte 20
    /// non-zero) where the game compares them to 0 and 1 exactly; a save it writes only holds
    /// those two values.
    pub fn status(&self) -> SlotStatus {
        let crc = crc32fast::hash(&self.0[Checksum::A.range()]);
        if crc != u32_at(self.0, Checksum::A.stored_at()) {
            SlotStatus::BadHeaderChecksum
        } else if !self.exists() {
            SlotStatus::Empty
        } else if self.is_intermission() {
            SlotStatus::Intermission
        } else {
            SlotStatus::Valid
        }
    }

    /// Scenario chapter (1..=19), `Save_Chapter` in memory (`0x020107ac`).
    pub fn chapter(&self) -> u32 {
        u32_at(self.0, PROGRESS)
    }

    /// Scenario step, `Save_ScenarioStep` (`0x020107e0`).
    pub fn scenario_step(&self) -> u32 {
        u32_at(self.0, PROGRESS + 4)
    }

    /// Scenario substep, `Save_ScenarioSubstep` (`0x02010814`).
    pub fn scenario_substep(&self) -> u32 {
        u32_at(self.0, PROGRESS + 8)
    }

    /// Scenario tracks and event flags (`Save_Serialize` block `0x2d30`, minus the four
    /// leading words), a copy of the quest manager's state (`docs/re/trigger.md`).
    pub fn event_flags(&self) -> EventFlags<'a> {
        EventFlags(&self.0[EVENT_FLAGS..EVENT_FLAGS + EVENT_FLAGS_SIZE])
    }

    /// Raw bytes of one serializer block (`Save_Serialize`).
    pub fn block(&self, block: Block) -> &'a [u8] {
        &self.0[block.range()]
    }

    /// Characters kept at the inn (not in the party).
    pub fn standby_count(&self) -> u8 {
        self.0[STANDBY_COUNT]
    }

    /// Characters in the party (at most 4).
    pub fn party_count(&self) -> u8 {
        self.0[PARTY_COUNT]
    }

    /// Party order: entry `i` is the id that `Save_UnpackMain` (`0x020a95b4`) gives to the
    /// party member object (`+0x568`) of character `standby_count + i` (0 for the hero on
    /// the demo save). Truncated to 4.
    pub fn party_order(&self) -> &'a [u8] {
        let n = usize::from(self.party_count()).min(4);
        &self.0[PARTY_ORDER..PARTY_ORDER + n]
    }

    /// Character record `index` (< [`CHARACTER_COUNT`]). The hero is the first character of
    /// the party, i.e. index [`Slot::standby_count`].
    pub fn character(&self, index: usize) -> Option<Character<'a>> {
        (index < CHARACTER_COUNT).then(|| {
            let at = CHARACTER_OFFSET + index * CHARACTER_SIZE;
            Character(&self.0[at..at + CHARACTER_SIZE])
        })
    }

    /// The 8 item words held by party member `member` (0..4), [`NO_ITEM`] when empty.
    pub fn held_items(&self, member: usize) -> Option<[u16; 8]> {
        (member < 4).then(|| {
            let at = HELD_ITEMS + member * HELD_ITEMS_STRIDE;
            core::array::from_fn(|i| u16_at(self.0, at + 2 * i))
        })
    }

    pub fn gold_on_hand(&self) -> u32 {
        u32_at(self.0, GOLD_ON_HAND)
    }

    pub fn gold_in_bank(&self) -> u32 {
        u32_at(self.0, GOLD_IN_BANK)
    }

    /// Mini medals handed in.
    pub fn mini_medals(&self) -> u32 {
        u32_at(self.0, MINI_MEDALS)
    }

    /// `(hours, minutes, seconds)` of play time.
    pub fn play_time(&self) -> (u16, u8, u8) {
        (
            u16_at(self.0, PLAY_TIME),
            self.0[PLAY_TIME + 2],
            self.0[PLAY_TIME + 3],
        )
    }

    /// Area word of a normal save (`+11476`) or, for an intermission save, of the one at
    /// `+27104`. Both are map ids of the map list (109 = `C01M09`).
    pub fn area(&self) -> u16 {
        u16_at(
            self.0,
            if self.is_intermission() {
                WARP_MAP
            } else {
                AREA
            },
        )
    }

    /// Destination block of an intermission save (`Save_UnpackWarp`, `0x020a9d88`): the game
    /// stands the party there instead of at the map's start point. `None` on a normal save,
    /// whose block holds stale bytes the loader never reads.
    pub fn warp(&self) -> Option<Warp> {
        self.is_intermission().then(|| Warp {
            map: u16_at(self.0, WARP_MAP),
            angle: u16_at(self.0, WARP_ANGLE) as i16,
            position: core::array::from_fn(|i| u32_at(self.0, WARP_POSITION + 4 * i) as i32),
        })
    }

    /// Time of the in-game day in seconds (`f32` in header A, `slot+32`): `Save_LoadSlot`
    /// (`0x020aaf94`) hands it to `GameState::SetDayTimer`. The day lasts 420 s.
    pub fn day_timer(&self) -> f32 {
        f32::from_bits(u32_at(self.0, DAY_TIMER))
    }

    /// Raw bit set of visited locations (bit 0 unused). **[unknown]** bit meanings beyond
    /// the editor's list (bits 1..=18 name towns); the real save sets other bits too.
    pub fn visited_locations(&self) -> u32 {
        u32_at(self.0, VISITED_LOCATIONS)
    }

    /// Raw bit set of recruited special guests (bit 0 unused; bits 1..=22).
    pub fn special_guests(&self) -> u32 {
        u32_at(self.0, SPECIAL_GUESTS)
    }

    /// Raw bit set of learned party tricks (bits 0 and 1 unused).
    pub fn party_tricks(&self) -> u32 {
        u32_at(self.0, PARTY_TRICKS)
    }

    /// Raw bit set of unlocked vocations: the editor's pattern puts Gladiator,
    /// Armamentalist, Paladin, Sage, Luminary and Ranger at bits 6..=11.
    pub fn unlocked_vocations(&self) -> u16 {
        u16_at(self.0, UNLOCKED_VOCATIONS)
    }
}

/// One character record (572 bytes).
#[derive(Clone, Copy)]
pub struct Character<'a>(&'a [u8]);

impl core::fmt::Debug for Character<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Character")
            .field("vocation", &self.vocation())
            .finish()
    }
}

/// Equipment slots of a character, in file order within [`Character::equipment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipSlot {
    Torso,
    Legs,
    Arms,
    Feet,
    Head,
    Weapon,
    Shield,
    Accessory,
}

impl EquipSlot {
    pub const ALL: [Self; 8] = [
        Self::Torso,
        Self::Legs,
        Self::Arms,
        Self::Feet,
        Self::Head,
        Self::Weapon,
        Self::Shield,
        Self::Accessory,
    ];

    const fn offset(self) -> usize {
        match self {
            Self::Torso => 352,
            Self::Legs => 354,
            Self::Arms => 360,
            Self::Feet => 362,
            Self::Head => 364,
            Self::Weapon => 366,
            Self::Shield => 368,
            Self::Accessory => 370,
        }
    }
}

impl<'a> Character<'a> {
    /// The 572 bytes of the record.
    pub fn bytes(&self) -> &'a [u8] {
        self.0
    }

    /// Name as stored: one byte per character, each the index of a glyph of the message
    /// font (`fi_me.bin`), zero-terminated and padded with zeros.
    pub fn name_raw(&self) -> &'a [u8] {
        &self.0[NAME..NAME + NAME_LEN]
    }

    /// Name as text: every byte up to the first zero is looked up in the message font's
    /// glyph table (`<'e>` for é…); an index outside the table gives `?`.
    pub fn name(&self, font: &crate::Font<'_>) -> String {
        let mut out = String::new();
        for &b in self.name_raw().iter().take_while(|&&b| b != 0) {
            match font.glyphs.get(usize::from(b)) {
                Some(g) => out.push_str(&String::from_utf8_lossy(g.text)),
                None => out.push('?'),
            }
        }
        out
    }

    /// Current vocation index (0 = Guardian … 12 = Ranger).
    pub fn vocation(&self) -> u8 {
        self.0[VOCATION]
    }

    /// Level in vocation `v` (0 when never taken; Guardian is vocation 0).
    pub fn level(&self, v: usize) -> Option<u8> {
        (v < VOCATION_COUNT).then(|| self.0[LEVELS + v])
    }

    pub fn revocations(&self, v: usize) -> Option<u8> {
        (v < VOCATION_COUNT).then(|| self.0[REVOCATIONS + v])
    }

    pub fn experience(&self, v: usize) -> Option<u32> {
        (v < VOCATION_COUNT).then(|| u32_at(self.0, EXPERIENCE + 4 * v))
    }

    /// Seed `s` (see [`SEEDS_PER_VOCATION`] for the order) used on vocation `v`: three
    /// 10-bit values per 32-bit word, three words per vocation.
    pub fn seed(&self, v: usize, s: usize) -> Option<u16> {
        (v < VOCATION_COUNT && s < SEEDS_PER_VOCATION).then(|| {
            let word = u32_at(self.0, SEEDS + SEED_STRIDE * v + 4 * (s / 3));
            ((word >> (10 * (s % 3))) & 0x3ff) as u16
        })
    }

    /// Skill points not yet spent.
    pub fn unallocated_skill_points(&self) -> u16 {
        u16_at(self.0, SKILL_POINTS)
    }

    /// 36 bytes of proficiency flags (`+282`); also holds the Zoom and Egg On bits
    /// **[hypothesis]** per the editor.
    pub fn proficiency_flags(&self) -> &'a [u8] {
        &self.0[PROFICIENCY_FLAGS..PROFICIENCY_FLAGS + 36]
    }

    pub fn equipment(&self, slot: EquipSlot) -> u16 {
        u16_at(self.0, slot.offset())
    }

    pub fn face(&self) -> u8 {
        self.0[FACE]
    }

    pub fn hair_style(&self) -> u8 {
        self.0[HAIR_STYLE]
    }

    /// Face as an item id (`+356`, 9020..=9033: slot h2 of `CharaParts_GetPartNumbers`,
    /// `docs/re/characters.md` § "The hero").
    pub fn face_item(&self) -> u16 {
        u16_at(self.0, FACE)
    }

    /// Hair as an item id (`+358`, 9000..=9013: slot h3).
    pub fn hair_item(&self) -> u16 {
        u16_at(self.0, HAIR_STYLE)
    }

    /// Bit 0 of byte 372 (0 or 1; **[unknown]** which is which).
    pub fn gender(&self) -> u8 {
        self.0[APPEARANCE] & 1
    }

    pub fn skin_color(&self) -> u8 {
        (self.0[APPEARANCE] >> 1) & 7
    }

    pub fn eye_color(&self) -> u8 {
        self.0[APPEARANCE] >> 4
    }

    pub fn hair_color(&self) -> u8 {
        self.0[HAIR_COLOR] & 0xf
    }

    /// Body type width and height words.
    pub fn body(&self) -> (u16, u16) {
        (u16_at(self.0, BODY), u16_at(self.0, BODY + 2))
    }
}

/// Editing view of one slot. Each setter leaves checksums stale until [`SlotMut::seal`].
///
/// Every setter returns the slot-relative byte range it wrote, so that a caller can show the
/// change and a test can assert that nothing else moved.
pub struct SlotMut<'a>(&'a mut [u8]);

impl core::fmt::Debug for SlotMut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SlotMut")
    }
}

impl SlotMut<'_> {
    /// Read view of the same bytes, for the fields a write keeps as they are (the standby
    /// count, a record outside the party, the opaque parts of a block).
    pub fn as_slot(&self) -> Slot<'_> {
        Slot(self.0)
    }

    /// Byte 20 of the header (`slot+0x14`), read back by [`Slot::is_intermission`].
    pub fn set_intermission(&mut self, intermission: bool) -> Range<usize> {
        self.0[INTERMISSION_FLAG] = u8::from(intermission);
        INTERMISSION_FLAG..INTERMISSION_FLAG + 1
    }

    /// Writes an empty header A, as `Save_WriteHeaderA` (`0x020aba6c`) does: the 0x14 bytes at
    /// `slot+0x10` are zeroed (CRC A, the two flags, the day timer) and the version string is
    /// kept. [`Save_SlotStatus`](Slot::status) then reads the slot as [`SlotStatus::Empty`];
    /// the blocks, header C and their checksums stay as they were.
    ///
    /// That is the original's deletion of the quick save log: the title screen's quick-save
    /// entry clears slot 1 once `Save_LoadSlot` (`0x020aaf94`) has read it
    /// (`ov_004::0216e490`), a second step only clears it (`ov_004::0216e414`), and a new game
    /// clears both slots (`func_020abb74`). Like every setter it leaves CRC A stale until
    /// [`SlotMut::seal`].
    pub fn clear_header(&mut self) -> Range<usize> {
        let mut version = [0u8; 8];
        version.copy_from_slice(&self.0[VERSION..VERSION + 8]);
        self.0[HEADER_A..HEADER_A + HEADER_A_LEN].fill(0);
        self.0[VERSION..VERSION + 8].copy_from_slice(&version);
        HEADER_A..HEADER_A + HEADER_A_LEN
    }

    /// Byte 21 of the header (`slot+0x15`), read back by [`Slot::exists`].
    pub fn set_exists(&mut self, exists: bool) -> Range<usize> {
        self.0[EXISTS_FLAG] = u8::from(exists);
        EXISTS_FLAG..EXISTS_FLAG + 1
    }

    /// Speed of the in-game day, as [`Slot::day_timer`] reads it.
    pub fn set_day_timer(&mut self, seconds: f32) -> Range<usize> {
        put_u32(self.0, DAY_TIMER, seconds.to_bits());
        DAY_TIMER..DAY_TIMER + 4
    }

    /// `(hours, minutes, seconds)`, as [`Slot::play_time`] reads them.
    pub fn set_play_time(&mut self, hours: u16, minutes: u8, seconds: u8) -> Range<usize> {
        put_u16(self.0, PLAY_TIME, hours);
        self.0[PLAY_TIME + 2] = minutes;
        self.0[PLAY_TIME + 3] = seconds;
        PLAY_TIME..PLAY_TIME + 4
    }

    /// The word [`Slot::area`] reads: `+11476` on a normal save, the warp map on an
    /// intermission save (the reader picks by the header, so does the setter).
    pub fn set_area(&mut self, map: u16) -> Range<usize> {
        let at = if self.0[INTERMISSION_FLAG] != 0 {
            WARP_MAP
        } else {
            AREA
        };
        put_u16(self.0, at, map);
        at..at + 2
    }

    /// The whole [`Warp`] block of an intermission save.
    pub fn set_warp(&mut self, warp: Warp) -> Range<usize> {
        put_u16(self.0, WARP_MAP, warp.map);
        put_u16(self.0, WARP_ANGLE, warp.angle as u16);
        for (i, value) in warp.position.into_iter().enumerate() {
            put_u32(self.0, WARP_POSITION + 4 * i, value as u32);
        }
        WARP_MAP..WARP_POSITION + 12
    }

    pub fn set_standby_count(&mut self, count: u8) -> Range<usize> {
        self.0[STANDBY_COUNT] = count;
        STANDBY_COUNT..STANDBY_COUNT + 1
    }

    pub fn set_party_count(&mut self, count: u8) -> Range<usize> {
        self.0[PARTY_COUNT] = count;
        PARTY_COUNT..PARTY_COUNT + 1
    }

    /// Writes the first `order.len()` (at most 4) entries of [`Slot::party_order`].
    pub fn set_party_order(&mut self, order: &[u8]) -> Range<usize> {
        let n = order.len().min(4);
        self.0[PARTY_ORDER..PARTY_ORDER + n].copy_from_slice(&order[..n]);
        PARTY_ORDER..PARTY_ORDER + n
    }

    /// One of the 8 item words [`Slot::held_items`] reads. `false` past member 3.
    pub fn set_held_item(&mut self, member: usize, slot: usize, item: u16) -> Option<Range<usize>> {
        if member >= 4 || slot >= 8 {
            return None;
        }
        let at = HELD_ITEMS + member * HELD_ITEMS_STRIDE + 2 * slot;
        put_u16(self.0, at, item);
        Some(at..at + 2)
    }

    /// The 8 item words of one party member (`18` bytes are reserved, the last two untouched).
    pub fn set_held_items(&mut self, member: usize, items: &[u16; 8]) -> Option<Range<usize>> {
        if member >= 4 {
            return None;
        }
        let at = HELD_ITEMS + member * HELD_ITEMS_STRIDE;
        for (i, &item) in items.iter().enumerate() {
            put_u16(self.0, at + 2 * i, item);
        }
        Some(at..at + 16)
    }

    pub fn set_gold_on_hand(&mut self, gold: u32) -> Range<usize> {
        put_u32(self.0, GOLD_ON_HAND, gold);
        GOLD_ON_HAND..GOLD_ON_HAND + 4
    }

    /// The bag the party carries (`Main+0x1d5c`, `docs/re/party.md`): the capacity at `+8`,
    /// then the ids and quantities of its slots. An empty slot keeps the empty word it
    /// already had (`0` or [`NO_ITEM`]); the pointer words and the block's tail stay as they
    /// are. `None` when the slots do not fit the block (`12 + 3n > 0x1d4`).
    pub fn set_bag(&mut self, slots: &[(Option<u16>, u8)]) -> Option<Range<usize>> {
        let count = slots.len();
        if BAG_HEADER + 3 * count > BAG_SIZE {
            return None;
        }
        put_u16(self.0, BAG + 8, count as u16);
        let ids = BAG + BAG_HEADER;
        let quantities = ids + 2 * count;
        for (index, (item, quantity)) in slots.iter().enumerate() {
            let current = u16_at(self.0, ids + 2 * index);
            let word = match item {
                Some(id) => *id,
                None if current == NO_ITEM || current == 0 => current,
                None => NO_ITEM,
            };
            put_u16(self.0, ids + 2 * index, word);
            self.0[quantities + index] = *quantity;
        }
        Some(BAG + 8..BAG + BAG_HEADER + 3 * count)
    }

    pub fn set_gold_in_bank(&mut self, gold: u32) -> Range<usize> {
        put_u32(self.0, GOLD_IN_BANK, gold);
        GOLD_IN_BANK..GOLD_IN_BANK + 4
    }

    pub fn set_mini_medals(&mut self, medals: u32) -> Range<usize> {
        put_u32(self.0, MINI_MEDALS, medals);
        MINI_MEDALS..MINI_MEDALS + 4
    }

    pub fn set_visited_locations(&mut self, bits: u32) -> Range<usize> {
        put_u32(self.0, VISITED_LOCATIONS, bits);
        VISITED_LOCATIONS..VISITED_LOCATIONS + 4
    }

    pub fn set_special_guests(&mut self, bits: u32) -> Range<usize> {
        put_u32(self.0, SPECIAL_GUESTS, bits);
        SPECIAL_GUESTS..SPECIAL_GUESTS + 4
    }

    pub fn set_party_tricks(&mut self, bits: u32) -> Range<usize> {
        put_u32(self.0, PARTY_TRICKS, bits);
        PARTY_TRICKS..PARTY_TRICKS + 4
    }

    pub fn set_unlocked_vocations(&mut self, bits: u16) -> Range<usize> {
        put_u16(self.0, UNLOCKED_VOCATIONS, bits);
        UNLOCKED_VOCATIONS..UNLOCKED_VOCATIONS + 2
    }

    /// Chapter word ([`Slot::chapter`]).
    pub fn set_chapter(&mut self, chapter: u32) -> Range<usize> {
        put_u32(self.0, PROGRESS, chapter);
        PROGRESS..PROGRESS + 4
    }

    pub fn set_scenario_step(&mut self, step: u32) -> Range<usize> {
        put_u32(self.0, PROGRESS + 4, step);
        PROGRESS + 4..PROGRESS + 8
    }

    pub fn set_scenario_substep(&mut self, substep: u32) -> Range<usize> {
        put_u32(self.0, PROGRESS + 8, substep);
        PROGRESS + 8..PROGRESS + 12
    }

    /// Editing view of the event block [`Slot::event_flags`] reads.
    pub fn event_flags_mut(&mut self) -> EventFlagsMut<'_> {
        let range = EVENT_FLAGS..EVENT_FLAGS + EVENT_FLAGS_SIZE;
        EventFlagsMut {
            bytes: &mut self.0[range],
            base: EVENT_FLAGS,
        }
    }

    /// Editing view of character record `index` (< [`CHARACTER_COUNT`]).
    pub fn character_mut(&mut self, index: usize) -> Option<CharacterMut<'_>> {
        if index >= CHARACTER_COUNT {
            return None;
        }
        let base = CHARACTER_OFFSET + index * CHARACTER_SIZE;
        Some(CharacterMut {
            bytes: &mut self.0[base..base + CHARACTER_SIZE],
            base,
        })
    }

    /// Recompute the three checksums from the data.
    ///
    /// Contract: every setter above writes bytes only and leaves the checksums stale, so
    /// [`Slot::checksums`] reports the mismatch until this is called; the CLI writes sealed
    /// images. A no-op edit must **not** call it: sealing an already valid slot rewrites the
    /// stored CRC with the same value, so the bytes stay identical.
    pub fn seal(&mut self) {
        for which in Checksum::ALL {
            let crc = crc32fast::hash(&self.0[which.range()]);
            let at = which.stored_at();
            self.0[at..at + 4].copy_from_slice(&crc.to_le_bytes());
        }
    }
}

/// Editing view of the event block (0x334 bytes at `PROGRESS + 16`). Ranges are slot-relative.
pub struct EventFlagsMut<'a> {
    bytes: &'a mut [u8],
    base: usize,
}

impl core::fmt::Debug for EventFlagsMut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("EventFlagsMut")
    }
}

impl<'a> EventFlagsMut<'a> {
    /// Editing view of a standalone event block (an array of [`EVENT_FLAGS_SIZE`] bytes): the
    /// ranges are then relative to the block. For composing a block before it has a slot
    /// (`dq9_core::state::GameState::from_play`).
    pub fn new(bytes: &'a mut [u8]) -> Option<Self> {
        (bytes.len() == EVENT_FLAGS_SIZE).then_some(Self { bytes, base: 0 })
    }

    /// Byte `+0x332`: which track [`EventFlags::current_track`] reads.
    pub fn set_current_track(&mut self, track: u8) -> Range<usize> {
        self.bytes[TRACK_SELECTOR] = track;
        self.base + TRACK_SELECTOR..self.base + TRACK_SELECTOR + 1
    }

    /// Track `index` (0..[`TRACK_COUNT`]).
    pub fn track_mut(&mut self, index: usize) -> Option<TrackMut<'_>> {
        if index >= TRACK_COUNT {
            return None;
        }
        let at = index * TRACK_SIZE;
        let range = at..at + TRACK_SIZE;
        Some(TrackMut {
            bytes: &mut self.bytes[range],
            base: self.base + at,
        })
    }

    /// Global flag `index` as the scripts see it (`Flags_Test`, same shift as
    /// [`EventFlags::story_flag`] when `story` is set). `None` when the index falls outside
    /// the 0x234-byte array, or outside it after the `0x6fa` story shift.
    pub fn set_global_flag(
        &mut self,
        index: i32,
        value: bool,
        story: bool,
    ) -> Option<Range<usize>> {
        let index = if story && index >= 0x400 {
            index.saturating_add(0x6fa)
        } else {
            index
        };
        let index = usize::try_from(index).ok()?;
        if index >> 3 >= GLOBAL_FLAGS_SIZE {
            return None;
        }
        let at = GLOBAL_FLAGS + (index >> 3);
        let mask = 1u8 << (index & 7);
        if value {
            self.bytes[at] |= mask;
        } else {
            self.bytes[at] &= !mask;
        }
        Some(self.base + at..self.base + at + 1)
    }

    /// Quest nibble `index` (0..[`QUEST_COUNT`]), the counterpart of [`EventFlags::quest`]:
    /// two per byte at `+0x2cc`, low nibble first, four bits stored. `None` past the array.
    pub fn set_quest(&mut self, index: i32, value: u8) -> Option<Range<usize>> {
        let index = usize::try_from(index).ok()?;
        if index >= QUEST_COUNT {
            return None;
        }
        let at = QUEST_STATES + index / 2;
        let shift = (index & 1) * 4;
        self.bytes[at] = self.bytes[at] & !(0xf << shift) | (value & 0xf) << shift;
        Some(self.base + at..self.base + at + 1)
    }
}

/// Editing view of one scenario track (28 bytes). Ranges are slot-relative.
pub struct TrackMut<'a> {
    bytes: &'a mut [u8],
    base: usize,
}

impl core::fmt::Debug for TrackMut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("TrackMut")
    }
}

impl TrackMut<'_> {
    pub fn set_chapter(&mut self, chapter: u8) -> Range<usize> {
        self.bytes[0] = chapter;
        self.base..self.base + 1
    }

    pub fn set_step(&mut self, step: u8) -> Range<usize> {
        self.bytes[1] = step;
        self.base + 1..self.base + 2
    }

    pub fn set_substep(&mut self, substep: u8) -> Range<usize> {
        self.bytes[2] = substep;
        self.base + 2..self.base + 3
    }

    /// 13 chapter-local bytes, as [`Track::chapter_flags`] reads them (at most 13 copied).
    pub fn set_chapter_flags(&mut self, flags: &[u8]) -> Range<usize> {
        let n = flags.len().min(13);
        self.bytes[3..3 + n].copy_from_slice(&flags[..n]);
        self.base + 3..self.base + 3 + n
    }

    /// 12 step-local bytes, as [`Track::step_flags`] reads them (at most 12 copied).
    pub fn set_step_flags(&mut self, flags: &[u8]) -> Range<usize> {
        let n = flags.len().min(12);
        self.bytes[16..16 + n].copy_from_slice(&flags[..n]);
        self.base + 16..self.base + 16 + n
    }
}

/// Editing view of one character record (572 bytes). Ranges are slot-relative.
pub struct CharacterMut<'a> {
    bytes: &'a mut [u8],
    /// Offset of the record inside the slot.
    base: usize,
}

impl core::fmt::Debug for CharacterMut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("CharacterMut")
    }
}

impl CharacterMut<'_> {
    fn range(&self, at: usize, len: usize) -> Range<usize> {
        self.base + at..self.base + at + len
    }

    /// Name glyphs ([`Character::name_raw`]): at most [`NAME_LEN`] bytes, zero-padded.
    pub fn set_name_raw(&mut self, glyphs: &[u8]) -> Range<usize> {
        let n = glyphs.len().min(NAME_LEN);
        self.bytes[NAME..NAME + NAME_LEN].fill(0);
        self.bytes[NAME..NAME + n].copy_from_slice(&glyphs[..n]);
        self.range(NAME, NAME_LEN)
    }

    pub fn set_vocation(&mut self, vocation: u8) -> Range<usize> {
        self.bytes[VOCATION] = vocation;
        self.range(VOCATION, 1)
    }

    /// `false` for a vocation index at or past [`VOCATION_COUNT`].
    pub fn set_level(&mut self, v: usize, level: u8) -> Option<Range<usize>> {
        if v >= VOCATION_COUNT {
            return None;
        }
        self.bytes[LEVELS + v] = level;
        Some(self.range(LEVELS + v, 1))
    }

    pub fn set_revocations(&mut self, v: usize, count: u8) -> Option<Range<usize>> {
        if v >= VOCATION_COUNT {
            return None;
        }
        self.bytes[REVOCATIONS + v] = count;
        Some(self.range(REVOCATIONS + v, 1))
    }

    pub fn set_experience(&mut self, v: usize, experience: u32) -> Option<Range<usize>> {
        if v >= VOCATION_COUNT {
            return None;
        }
        let at = EXPERIENCE + 4 * v;
        put_u32(self.bytes, at, experience);
        Some(self.range(at, 4))
    }

    /// Writes the 10-bit seed `s` of vocation `v`, keeping its two neighbours.
    pub fn set_seed(&mut self, v: usize, s: usize, value: u16) -> Option<Range<usize>> {
        if v >= VOCATION_COUNT || s >= SEEDS_PER_VOCATION {
            return None;
        }
        let at = SEEDS + SEED_STRIDE * v + 4 * (s / 3);
        let shift = 10 * (s % 3);
        let word = u32_at(self.bytes, at) & !(0x3ff << shift) | (u32::from(value & 0x3ff) << shift);
        put_u32(self.bytes, at, word);
        Some(self.range(at, 4))
    }

    pub fn set_unallocated_skill_points(&mut self, points: u16) -> Range<usize> {
        put_u16(self.bytes, SKILL_POINTS, points);
        self.range(SKILL_POINTS, 2)
    }

    /// Maximum HP: 10-bit field 7 of the stat block at `+0x14c`, the value
    /// `Save_UnpackMain` writes into the member's HP (`dq9_core::party`'s `stat_field`).
    pub fn set_max_hp(&mut self, hp: u16) -> Range<usize> {
        self.set_stat_field(MAX_HP_FIELD, hp)
    }

    /// Maximum MP: field 8 of the same block.
    pub fn set_max_mp(&mut self, mp: u16) -> Range<usize> {
        self.set_stat_field(MAX_MP_FIELD, mp)
    }

    /// Writes one of the fourteen 10-bit fields, keeping the two neighbours of its word.
    fn set_stat_field(&mut self, field: usize, value: u16) -> Range<usize> {
        let at = STAT_BLOCK + 4 * (field / 3);
        let shift = 10 * (field % 3);
        let word = u32_at(self.bytes, at) & !(0x3ff << shift) | (u32::from(value & 0x3ff) << shift);
        put_u32(self.bytes, at, word);
        self.range(at, 4)
    }

    /// 36 proficiency bytes ([`Character::proficiency_flags`]), at most 36 copied.
    pub fn set_proficiency_flags(&mut self, flags: &[u8]) -> Range<usize> {
        let n = flags.len().min(36);
        self.bytes[PROFICIENCY_FLAGS..PROFICIENCY_FLAGS + n].copy_from_slice(&flags[..n]);
        self.range(PROFICIENCY_FLAGS, n)
    }

    pub fn set_equipment(&mut self, slot: EquipSlot, item: u16) -> Range<usize> {
        let at = slot.offset();
        put_u16(self.bytes, at, item);
        self.range(at, 2)
    }

    pub fn set_face(&mut self, face: u8) -> Range<usize> {
        self.bytes[FACE] = face;
        self.range(FACE, 1)
    }

    /// Face item id word ([`Character::face_item`], 9020..=9033), two bytes.
    pub fn set_face_item(&mut self, item: u16) -> Range<usize> {
        put_u16(self.bytes, FACE, item);
        self.range(FACE, 2)
    }

    pub fn set_hair_style(&mut self, style: u8) -> Range<usize> {
        self.bytes[HAIR_STYLE] = style;
        self.range(HAIR_STYLE, 1)
    }

    /// Hair item id word ([`Character::hair_item`], 9000..=9013), two bytes.
    pub fn set_hair_item(&mut self, item: u16) -> Range<usize> {
        put_u16(self.bytes, HAIR_STYLE, item);
        self.range(HAIR_STYLE, 2)
    }

    pub fn set_gender(&mut self, gender: u8) -> Range<usize> {
        self.bytes[APPEARANCE] = self.bytes[APPEARANCE] & !1 | (gender & 1);
        self.range(APPEARANCE, 1)
    }

    pub fn set_skin_color(&mut self, color: u8) -> Range<usize> {
        self.bytes[APPEARANCE] = self.bytes[APPEARANCE] & !0b1110 | (color & 7) << 1;
        self.range(APPEARANCE, 1)
    }

    pub fn set_eye_color(&mut self, color: u8) -> Range<usize> {
        self.bytes[APPEARANCE] = self.bytes[APPEARANCE] & 0x0f | (color & 0xf) << 4;
        self.range(APPEARANCE, 1)
    }

    pub fn set_hair_color(&mut self, color: u8) -> Range<usize> {
        self.bytes[HAIR_COLOR] = self.bytes[HAIR_COLOR] & 0xf0 | (color & 0xf);
        self.range(HAIR_COLOR, 1)
    }

    /// Body type width and height words, as [`Character::body`] reads them.
    pub fn set_body(&mut self, width: u16, height: u16) -> Range<usize> {
        put_u16(self.bytes, BODY, width);
        put_u16(self.bytes, BODY + 2, height);
        self.range(BODY, 4)
    }
}

/// Event state saved with a slot: five scenario tracks, the global flag array and a few
/// trailing blocks. `func_020a99d0` copies it in five pieces (`0x8c`, `0x234`, `0xc`,
/// `0x66` and 1 bytes); the block starts at offset 0 of the quest manager, so the manager's
/// offsets (`+0x8c` global flags, `+0x332` track selector) apply unchanged.
#[derive(Clone, Copy)]
pub struct EventFlags<'a>(&'a [u8]);

impl core::fmt::Debug for EventFlags<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventFlags")
            .field("current_track", &self.current_track())
            .finish_non_exhaustive()
    }
}

/// One scenario track: progress bytes and its two local flag sets.
#[derive(Clone, Copy, Debug)]
pub struct Track<'a>(&'a [u8]);

impl<'a> Track<'a> {
    pub fn chapter(&self) -> u8 {
        self.0[0]
    }
    pub fn step(&self) -> u8 {
        self.0[1]
    }
    pub fn substep(&self) -> u8 {
        self.0[2]
    }
    /// Chapter-local flags (13 bytes), cleared when the chapter changes.
    pub fn chapter_flags(&self) -> &'a [u8] {
        &self.0[3..16]
    }
    /// Step-local flags (12 bytes), cleared when the step changes.
    pub fn step_flags(&self) -> &'a [u8] {
        &self.0[16..28]
    }
}

/// Bit `index` of a byte array, least significant bit first (`Flags_Test`, `0x0206dfc0`).
/// Negative or out-of-range indices read as clear.
pub fn test_bit(bytes: &[u8], index: i32) -> bool {
    usize::try_from(index)
        .ok()
        .and_then(|i| bytes.get(i >> 3).map(|b| b >> (i & 7) & 1 != 0))
        .unwrap_or(false)
}

impl<'a> EventFlags<'a> {
    /// Wraps a copy of the event block (`EVENT_FLAGS_SIZE` = 0x334 bytes).
    pub fn from_bytes(bytes: &'a [u8]) -> Option<Self> {
        (bytes.len() == EVENT_FLAGS_SIZE).then_some(Self(bytes))
    }

    /// The raw block.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.0
    }

    /// Index of the current track (byte `+0x332` of the block), 0..5.
    pub fn current_track(&self) -> usize {
        usize::from(self.0[TRACK_SELECTOR])
    }

    /// Track `index` (0..[`TRACK_COUNT`]).
    pub fn track(&self, index: usize) -> Option<Track<'a>> {
        (index < TRACK_COUNT).then(|| Track(&self.0[index * TRACK_SIZE..][..TRACK_SIZE]))
    }

    /// The 564-byte global flag array.
    pub fn global_flags(&self) -> &'a [u8] {
        &self.0[GLOBAL_FLAGS..GLOBAL_FLAGS + GLOBAL_FLAGS_SIZE]
    }

    /// Global flag `index` as the scripts read it (`Flags_Test`).
    pub fn global_flag(&self, index: i32) -> bool {
        test_bit(self.global_flags(), index)
    }

    /// Quest state and flags: one nibble per quest at quest manager `+0x2cc`.
    pub fn quest(&self, index: i32) -> u8 {
        let Ok(index) = usize::try_from(index) else {
            return 0;
        };
        if index >= QUEST_COUNT {
            return 0;
        }
        (self.0[QUEST_STATES + index / 2] >> ((index & 1) * 4)) & 0xf
    }

    /// Story flag `index` (`Flags_TestExt`, `0x0206eba8`): indices from `0x400` are shifted
    /// by `0x6fa`.
    pub fn story_flag(&self, index: i32) -> bool {
        let index = if index < 0x400 {
            index
        } else {
            index.saturating_add(0x6fa)
        };
        self.global_flag(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Vec<u8> {
        let mut data = vec![0u8; FILE_SIZE];
        data[..15].copy_from_slice(MAGIC);
        data[EXISTS_FLAG] = 1;
        let mut save = Save::parse(&data).unwrap();
        save.slot_mut(0).unwrap().seal();
        save.as_bytes().to_vec()
    }

    #[test]
    fn rejects_wrong_size() {
        assert!(Save::parse(&[0; FILE_SIZE - 1]).is_err());
        assert!(Save::parse(&[0; FILE_SIZE + 1]).is_err());
    }

    #[test]
    fn parse_keeps_bytes() {
        let data = blank();
        assert_eq!(Save::parse(&data).unwrap().as_bytes(), &data[..]);
    }

    #[test]
    fn sealed_slot_is_valid_and_edits_invalidate() {
        let mut save = Save::parse(&blank()).unwrap();
        assert!(save.slot(0).unwrap().checksums_valid());
        assert_eq!(save.current_slot(), Some(0));

        save.slot_mut(0).unwrap().set_gold_on_hand(1234);
        let slot = save.slot(0).unwrap();
        assert_eq!(slot.gold_on_hand(), 1234);
        let bad: Vec<_> = slot
            .checksums()
            .into_iter()
            .filter(|c| !c.is_valid())
            .map(|c| c.which)
            .collect();
        assert_eq!(bad, [Checksum::B], "gold lies in the B range only");

        save.slot_mut(0).unwrap().seal();
        assert!(save.slot(0).unwrap().checksums_valid());
    }

    #[test]
    fn status_follows_header_flags() {
        let mut data = blank();
        let status = |data: &[u8]| Save::parse(data).unwrap().slot(0).unwrap().status();
        assert_eq!(status(&data), SlotStatus::Valid);

        data[EXISTS_FLAG] = 0;
        assert_eq!(
            status(&data),
            SlotStatus::BadHeaderChecksum,
            "flags are CRC-covered"
        );
        let mut save = Save::parse(&data).unwrap();
        save.slot_mut(0).unwrap().seal();
        assert_eq!(save.slot(0).unwrap().status(), SlotStatus::Empty);

        let mut data = blank();
        data[INTERMISSION_FLAG] = 1;
        let mut save = Save::parse(&data).unwrap();
        save.slot_mut(0).unwrap().seal();
        assert_eq!(save.slot(0).unwrap().status(), SlotStatus::Intermission);
        assert_eq!(save.current_slot(), None);
    }

    /// A start-up with no title menu resumes the normal log, else the suspend save; the
    /// suspend fallback needs an intermission header with all three checksums.
    #[test]
    fn a_resume_takes_the_normal_slot_then_the_suspend_one() {
        let mut save = Save::parse(&blank()).unwrap();
        assert_eq!(save.resume_slot(), Some(0), "the normal log");

        {
            let mut slot = save.slot_mut(SUSPEND_SLOT).unwrap();
            slot.set_intermission(true);
            slot.set_exists(true);
            slot.seal();
        }
        assert_eq!(
            save.slot(SUSPEND_SLOT).unwrap().status(),
            SlotStatus::Intermission
        );
        assert_eq!(save.resume_slot(), Some(0), "a normal log outranks it");

        // Empty normal slot: the suspend save is the fallback.
        {
            let mut slot = save.slot_mut(0).unwrap();
            slot.set_exists(false);
            slot.seal();
        }
        assert_eq!(save.current_slot(), None);
        assert_eq!(save.resume_slot(), Some(SUSPEND_SLOT));

        // A stale checksum makes it no resume point at all, as `Save_LoadSlot`'s CRC B test
        // rejects it.
        save.slot_mut(SUSPEND_SLOT).unwrap().set_day_timer(1.0);
        assert_eq!(save.resume_slot(), None);
    }

    /// The deletion of the quick save log (`Save_WriteHeaderA`, `0x020aba6c`): header A is
    /// zeroed with the version kept, so `Save_SlotStatus` reads the slot as empty and
    /// everything from header C on is untouched.
    #[test]
    fn clearing_a_header_empties_the_slot_and_keeps_its_blocks() {
        let mut save = Save::parse(&blank()).unwrap();
        {
            // A suspend-only container: no normal log, so the suspend save is the resume.
            let mut slot = save.slot_mut(0).unwrap();
            slot.set_exists(false);
            slot.seal();
        }
        {
            let mut slot = save.slot_mut(SUSPEND_SLOT).unwrap();
            slot.set_intermission(true);
            slot.set_exists(true);
            slot.set_day_timer(100.0);
            slot.set_gold_on_hand(77);
            slot.seal();
        }
        assert_eq!(save.resume_slot(), Some(SUSPEND_SLOT), "slot 0 is empty");
        let before = save.slot(SUSPEND_SLOT).unwrap();
        let version = before.version().to_vec();
        let from_header_c = before.bytes()[HEADER_A + HEADER_A_LEN..].to_vec();
        let ranges = {
            let mut slot = save.slot_mut(SUSPEND_SLOT).unwrap();
            let ranges = slot.clear_header();
            slot.seal();
            ranges
        };
        assert_eq!(ranges, HEADER_A..HEADER_A + HEADER_A_LEN);
        let after = save.slot(SUSPEND_SLOT).unwrap();
        assert_eq!(after.status(), SlotStatus::Empty);
        assert!(!after.exists() && !after.is_intermission());
        assert_eq!(after.day_timer(), 0.0, "the 0x10 covered bytes are zeroed");
        assert_eq!(after.version(), &version[..], "the version string stays");
        assert_eq!(
            &after.bytes()[HEADER_A + HEADER_A_LEN..],
            &from_header_c[..]
        );
        assert!(after.checksums_valid(), "the sealed header A recomputes");
        assert_eq!(after.gold_on_hand(), 77, "the blocks keep their bytes");
        assert_eq!(save.resume_slot(), None);
    }

    /// A normal save into a container that has only a suspend save: `Save_WriteStep`
    /// (`0x020aad2c`) state 0 loads slot 1's image into the buffer and state 1 writes it at
    /// slot 0's `0x10`, so the new log inherits slot 1's opaque blocks (here the bank and the
    /// medals, which `write_into` does not carry). Both slots' first 16 bytes stay put.
    #[test]
    fn a_normal_write_seeds_an_empty_slot_from_the_suspend_image() {
        let mut save = Save::parse(&blank()).unwrap();
        {
            let mut slot = save.slot_mut(0).unwrap();
            slot.set_exists(false);
            slot.seal();
        }
        {
            let mut slot = save.slot_mut(SUSPEND_SLOT).unwrap();
            slot.set_intermission(true);
            slot.set_exists(true);
            slot.set_gold_in_bank(777);
            slot.set_mini_medals(12);
            slot.seal();
        }
        let first16 = save.slot(0).unwrap().bytes()[..16].to_vec();
        assert_eq!(save.current_slot(), None);
        assert_eq!(save.prepare_normal_write(), Some(0));
        {
            let slot = save.slot(0).unwrap();
            assert_eq!(slot.status(), SlotStatus::Intermission, "slot 1's header");
            assert_eq!(slot.gold_in_bank(), 777, "the opaque block was seeded");
            assert_eq!(slot.mini_medals(), 12);
            assert_eq!(&slot.bytes()[..16], &first16[..], "the first 16 bytes");
        }
        // A caller prepares the same image for the state and for the write: idempotent.
        assert_eq!(save.prepare_normal_write(), Some(0));
        assert_eq!(save.slot(0).unwrap().gold_in_bank(), 777);
    }

    /// A container with an adventure log keeps today's target and its own bytes: the suspend
    /// image is not mixed into slot 0 (the mirror rewrites slot 1 afterwards).
    #[test]
    fn a_normal_write_keeps_the_adventure_log_and_does_not_seed_it() {
        let mut save = Save::parse(&blank()).unwrap();
        save.slot_mut(0).unwrap().set_gold_in_bank(111);
        save.slot_mut(0).unwrap().seal();
        {
            let mut slot = save.slot_mut(SUSPEND_SLOT).unwrap();
            slot.set_intermission(true);
            slot.set_exists(true);
            slot.set_gold_in_bank(777);
            slot.seal();
        }
        assert_eq!(save.prepare_normal_write(), Some(0));
        assert_eq!(save.slot(0).unwrap().status(), SlotStatus::Valid);
        assert_eq!(save.slot(0).unwrap().gold_in_bank(), 111);
    }

    /// A fresh container has no image to take the opaque blocks from, so there is no target.
    #[test]
    fn a_fresh_container_has_no_normal_write_target() {
        let mut save = Save::parse(&blank()).unwrap();
        {
            let mut slot = save.slot_mut(0).unwrap();
            slot.set_exists(false);
            slot.seal();
        }
        assert_eq!(save.current_slot(), None);
        assert_eq!(save.prepare_normal_write(), None);
    }

    #[test]
    fn magic_is_file_level() {
        let mut data = blank();
        assert!(Save::parse(&data).unwrap().has_magic());
        data[..16].fill(0xff);
        let save = Save::parse(&data).unwrap();
        assert!(!save.has_magic());
        assert_eq!(
            save.current_slot(),
            Some(0),
            "slot data does not depend on the magic"
        );
    }

    #[test]
    fn blocks_tile_checksum_b_exactly() {
        let b = Checksum::B.range();
        assert_eq!(Block::ALL[0].start(), b.start);
        for pair in Block::ALL.windows(2) {
            assert_eq!(pair[0].range().end, pair[1].start(), "{:?}", pair[0]);
        }
        assert_eq!(Block::ALL[Block::ALL.len() - 1].range().end, b.end);
        assert!(Block::ALL.iter().all(|b| !b.is_empty()));
        assert_eq!(Block::Progress.start(), PROGRESS);
        assert_eq!(Block::Progress.len(), 16 + EVENT_FLAGS_SIZE);
    }

    #[test]
    fn flags_are_lsb_first_and_story_flags_shift_past_0x400() {
        let mut data = blank();
        let base = EVENT_FLAGS + GLOBAL_FLAGS;
        data[base] = 0b0000_0010; // bit 1
        data[base + (0x400 + 0x6fa) / 8] |= 1 << ((0x400 + 0x6fa) % 8);
        data[EVENT_FLAGS + TRACK_SELECTOR] = 4;
        data[EVENT_FLAGS + 4 * TRACK_SIZE + 1] = 7;
        let save = Save::parse(&data).unwrap();
        let ev = save.slot(0).unwrap().event_flags();
        assert!(ev.global_flag(1) && !ev.global_flag(0) && !ev.global_flag(-1));
        assert!(ev.story_flag(0x400) && !ev.story_flag(0x3ff));
        assert!(!ev.global_flag(i32::MAX));
        assert_eq!(ev.current_track(), 4);
        assert_eq!(ev.track(4).unwrap().step(), 7);
        assert!(ev.track(5).is_none());
    }

    #[test]
    fn quest_nibbles_round_trip_through_the_setter() {
        let mut block = [0u8; EVENT_FLAGS_SIZE];
        {
            let mut flags = EventFlagsMut::new(&mut block).expect("a whole block");
            assert_eq!(flags.set_quest(2, 0xf), Some(0x2cd..0x2ce));
            assert_eq!(flags.set_quest(3, 0b1010), Some(0x2cd..0x2ce));
            assert_eq!(flags.set_quest(QUEST_COUNT as i32, 1), None);
            assert_eq!(flags.set_quest(-1, 1), None);
        }
        // Two per byte, low nibble first, four bits kept.
        assert_eq!((block[0x2cc], block[0x2cd]), (0x00, 0xaf));
        let flags = EventFlags::from_bytes(&block).unwrap();
        assert_eq!((flags.quest(2), flags.quest(3)), (0xf, 0b1010));
        assert_eq!(flags.quest(0), 0);
        assert!(EventFlagsMut::new(&mut [0u8; 4]).is_none());
    }

    #[test]
    fn progress_words_and_version_read_back() {
        let mut data = blank();
        data[VERSION..VERSION + 8].copy_from_slice(b"save013\0");
        data[PROGRESS..PROGRESS + 12].copy_from_slice(&[19, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0]);
        let save = Save::parse(&data).unwrap();
        let slot = save.slot(0).unwrap();
        assert_eq!(slot.version(), b"save013");
        assert_eq!(
            (
                slot.chapter(),
                slot.scenario_step(),
                slot.scenario_substep()
            ),
            (19, 2, 1)
        );
    }

    #[test]
    fn checksum_ranges_are_ordered_and_disjoint() {
        let mut end = 0;
        for which in [Checksum::A, Checksum::C, Checksum::B] {
            assert_eq!(which.stored_at() + 4, which.range().start);
            assert!(which.stored_at() >= end);
            end = which.range().end;
        }
    }

    #[test]
    fn seeds_unpack_three_per_word() {
        let mut data = blank();
        // vocation 2, word 1 holds seeds 3..=5: 1, 2, 3 (10 bits each).
        let at = CHARACTER_OFFSET + 88 + 12 * 2 + 4;
        data[at..at + 4].copy_from_slice(&(1u32 | 2 << 10 | 3 << 20).to_le_bytes());
        let save = Save::parse(&data).unwrap();
        let c = save.slot(0).unwrap().character(0).unwrap();
        assert_eq!(
            [c.seed(2, 3), c.seed(2, 4), c.seed(2, 5)],
            [Some(1), Some(2), Some(3)]
        );
        assert_eq!(c.seed(2, 9), None);
        assert_eq!(c.seed(13, 0), None);
    }

    #[test]
    fn indices_are_bounds_checked() {
        let save = Save::parse(&blank()).unwrap();
        assert!(save.slot(2).is_none());
        let slot = save.slot(0).unwrap();
        assert!(slot.character(CHARACTER_COUNT).is_none());
        assert!(slot.held_items(4).is_none());
        assert!(slot.party_order().is_empty());
    }

    #[test]
    fn last_character_stays_inside_the_checksummed_block() {
        let end = CHARACTER_OFFSET + CHARACTER_COUNT * CHARACTER_SIZE;
        assert!(end <= Checksum::B.range().end);
    }
}
