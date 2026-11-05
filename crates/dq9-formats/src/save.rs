//! DS save file (`.sav`, 64 KiB EEPROM image): two slots of 32 KiB, each guarded by three
//! CRC-32 checksums. Layout, evidence and open questions in `docs/re/save.md`.
//!
//! Offsets come from the public layout of the DQIX save editor (read, not copied) and are
//! checked on a real save: all six checksums recompute, and the typed fields below read
//! plausible values. Everything not listed here stays reachable through [`Slot::bytes`].
//!
//! The game's own save code is not ported yet (it cites no original function): this module
//! is the file format, not the `GameState` serialization.

use nds_rom::{Error, Result};

/// Size of the whole EEPROM image.
pub const FILE_SIZE: usize = 0x10000;
/// Size of one slot.
pub const SLOT_SIZE: usize = 0x8000;
/// Number of slots in the image.
pub const SLOT_COUNT: usize = 2;

/// First 15 bytes of the file, followed by a zero byte. Only at offset 0 of the image.
pub const MAGIC: &[u8; 15] = b"DRAGON QUEST IX";

const INTERMISSION_FLAG: usize = 20;
const EXISTS_FLAG: usize = 21;
const VERSION: usize = 24;
/// Chapter, step and substep words, then flag data (`Save_Serialize`, block `0x2d30`).
const PROGRESS: usize = 11568;
/// Event block: `PROGRESS + 16`, 0x334 bytes (`Save_Serialize`, `func_020a99d0`).
const EVENT_FLAGS: usize = PROGRESS + 16;
/// Size of the saved event block (`func_020a99d0`).
pub const EVENT_FLAGS_SIZE: usize = 0x334;
const TRACK_COUNT: usize = 5;
const TRACK_SIZE: usize = 0x1c;
const GLOBAL_FLAGS: usize = TRACK_COUNT * TRACK_SIZE;
const GLOBAL_FLAGS_SIZE: usize = 0x234;
const TRACK_SELECTOR: usize = 0x332;

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

    /// First slot whose header says [`SlotStatus::Valid`] and whose three checksums hold.
    ///
    /// **[hypothesis]** this is the slot a normal load takes; which slot the game loads is
    /// decided by its callers (`Save_LoadSlot` takes the slot as an argument).
    pub fn current_slot(&self) -> Option<usize> {
        (0..SLOT_COUNT).find(|&i| {
            self.slot(i)
                .is_some_and(|s| s.status() == SlotStatus::Valid && s.checksums_valid())
        })
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
    /// held-key test: header CRC, then the empty flag, then the intermission flag.
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
        &self.0[320..320 + NAME_LEN]
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
        self.0[80]
    }

    /// Level in vocation `v` (0 when never taken; Guardian is vocation 0).
    pub fn level(&self, v: usize) -> Option<u8> {
        (v < VOCATION_COUNT).then(|| self.0[2 + v])
    }

    pub fn revocations(&self, v: usize) -> Option<u8> {
        (v < VOCATION_COUNT).then(|| self.0[15 + v])
    }

    pub fn experience(&self, v: usize) -> Option<u32> {
        (v < VOCATION_COUNT).then(|| u32_at(self.0, 28 + 4 * v))
    }

    /// Seed `s` (see [`SEEDS_PER_VOCATION`] for the order) used on vocation `v`: three
    /// 10-bit values per 32-bit word, three words per vocation.
    pub fn seed(&self, v: usize, s: usize) -> Option<u16> {
        (v < VOCATION_COUNT && s < SEEDS_PER_VOCATION).then(|| {
            let word = u32_at(self.0, 88 + 12 * v + 4 * (s / 3));
            ((word >> (10 * (s % 3))) & 0x3ff) as u16
        })
    }

    /// Skill points not yet spent.
    pub fn unallocated_skill_points(&self) -> u16 {
        u16_at(self.0, 244)
    }

    /// 36 bytes of proficiency flags (`+282`); also holds the Zoom and Egg On bits
    /// **[hypothesis]** per the editor.
    pub fn proficiency_flags(&self) -> &'a [u8] {
        &self.0[282..282 + 36]
    }

    pub fn equipment(&self, slot: EquipSlot) -> u16 {
        u16_at(self.0, slot.offset())
    }

    pub fn face(&self) -> u8 {
        self.0[356]
    }

    pub fn hair_style(&self) -> u8 {
        self.0[358]
    }

    /// Face as an item id (`+356`, 9020..=9033: slot h2 of `CharaParts_GetPartNumbers`,
    /// `docs/re/characters.md` § « Le héros »).
    pub fn face_item(&self) -> u16 {
        u16_at(self.0, 356)
    }

    /// Hair as an item id (`+358`, 9000..=9013: slot h3).
    pub fn hair_item(&self) -> u16 {
        u16_at(self.0, 358)
    }

    /// Bit 0 of byte 372 (0 or 1; **[unknown]** which is which).
    pub fn gender(&self) -> u8 {
        self.0[372] & 1
    }

    pub fn skin_color(&self) -> u8 {
        (self.0[372] >> 1) & 7
    }

    pub fn eye_color(&self) -> u8 {
        self.0[372] >> 4
    }

    pub fn hair_color(&self) -> u8 {
        self.0[373] & 0xf
    }

    /// Body type width and height words.
    pub fn body(&self) -> (u16, u16) {
        (u16_at(self.0, 376), u16_at(self.0, 378))
    }
}

/// Editing view of one slot. Each setter leaves checksums stale until [`SlotMut::seal`].
pub struct SlotMut<'a>(&'a mut [u8]);

impl core::fmt::Debug for SlotMut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SlotMut")
    }
}

impl SlotMut<'_> {
    pub fn set_gold_on_hand(&mut self, gold: u32) {
        self.0[GOLD_ON_HAND..GOLD_ON_HAND + 4].copy_from_slice(&gold.to_le_bytes());
    }

    pub fn set_gold_in_bank(&mut self, gold: u32) {
        self.0[GOLD_IN_BANK..GOLD_IN_BANK + 4].copy_from_slice(&gold.to_le_bytes());
    }

    pub fn set_mini_medals(&mut self, medals: u32) {
        self.0[MINI_MEDALS..MINI_MEDALS + 4].copy_from_slice(&medals.to_le_bytes());
    }

    /// Recompute the three checksums from the data.
    pub fn seal(&mut self) {
        for which in Checksum::ALL {
            let crc = crc32fast::hash(&self.0[which.range()]);
            let at = which.stored_at();
            self.0[at..at + 4].copy_from_slice(&crc.to_le_bytes());
        }
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
