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

/// First 15 bytes of a committed slot (byte 15 is 0).
pub const MAGIC: &[u8; 15] = b"DRAGON QUEST IX";

const MAGIC_LEN: usize = 16;
const INTERMISSION_FLAG: usize = 20;
const EXISTS_FLAG: usize = 21;

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
const QUICK_SAVE_AREA: usize = 27104;
const QUICK_SAVE_POSITION: usize = 27108;
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

    /// First slot that carries the magic and whose three checksums hold.
    ///
    /// **[hypothesis]** this is the slot the game loads: on the one real save examined, slot
    /// 1 is a copy of slot 0 with the magic erased to `0xff`, as if the game finished a slot
    /// by writing its magic last.
    pub fn current_slot(&self) -> Option<usize> {
        (0..SLOT_COUNT).find(|&i| {
            self.slot(i)
                .is_some_and(|s| s.has_magic() && s.checksums_valid())
        })
    }
}

/// Read-only view of one 32 KiB slot.
#[derive(Clone, Copy)]
pub struct Slot<'a>(&'a [u8]);

impl core::fmt::Debug for Slot<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Slot")
            .field("magic", &self.has_magic())
            .finish()
    }
}

impl<'a> Slot<'a> {
    pub fn bytes(&self) -> &'a [u8] {
        self.0
    }

    pub fn has_magic(&self) -> bool {
        self.0[..MAGIC.len()] == MAGIC[..] && self.0[MAGIC_LEN - 1] == 0
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

    /// Byte 20: set on the "intermission" (quick) save written by the in-game suspend.
    /// **[hypothesis]** name from the editor and ImHex pattern; not checked in code.
    pub fn is_intermission(&self) -> bool {
        self.0[INTERMISSION_FLAG] != 0
    }

    /// Bit 0 of byte 21: the slot holds a save.
    pub fn exists(&self) -> bool {
        self.0[EXISTS_FLAG] & 1 != 0
    }

    /// Characters kept at the inn (not in the party).
    pub fn standby_count(&self) -> u8 {
        self.0[STANDBY_COUNT]
    }

    /// Characters in the party (at most 4).
    pub fn party_count(&self) -> u8 {
        self.0[PARTY_COUNT]
    }

    /// Party order: indices from the start of the party in [`Slot::character`] terms,
    /// shifted by [`Slot::standby_count`] **[hypothesis]**. Truncated to 4.
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
    /// `+27104`. **[unknown]** the numbering (map id or zone id).
    pub fn area(&self) -> u16 {
        u16_at(
            self.0,
            if self.is_intermission() {
                QUICK_SAVE_AREA
            } else {
                AREA
            },
        )
    }

    /// Three signed words of the intermission save position (`+27108`). **[unknown]**
    /// units and axis order; meaningful only when [`Slot::is_intermission`].
    pub fn intermission_position(&self) -> [i32; 3] {
        core::array::from_fn(|i| u32_at(self.0, QUICK_SAVE_POSITION + 4 * i) as i32)
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
    /// Name in the game's own encoding (not ASCII), padded with zeros.
    /// **[unknown]** the character table; see `docs/re/save.md`.
    pub fn name_raw(&self) -> &'a [u8] {
        &self.0[320..320 + NAME_LEN]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Vec<u8> {
        let mut data = vec![0u8; FILE_SIZE];
        data[..15].copy_from_slice(MAGIC);
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
    fn slot_without_magic_is_not_current() {
        let mut data = blank();
        data[..16].fill(0xff);
        let save = Save::parse(&data).unwrap();
        assert!(!save.slot(0).unwrap().has_magic());
        assert_eq!(save.current_slot(), None);
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
