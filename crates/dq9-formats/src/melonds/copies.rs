//! The byte ranges a normal save load copies into RAM, and the party member fields its load
//! computes (`docs/re/save.md`, § "In RAM").

use super::{GAME_STATE, QUEST_LOG, QUEST_MANAGER, ZONE_3D};

/// A byte range of a save slot that `Save_LoadSlot` (`0x020aaf94`) and the functions it
/// calls copy unchanged into RAM when a normal save is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveCopy {
    pub name: &'static str,
    /// Offset inside the slot (as [`crate::save::Block::start`]).
    pub slot_offset: usize,
    pub len: usize,
    pub ram: u32,
    /// The game changes these bytes after loading, so a difference is expected.
    pub runtime: bool,
}

const fn copy(name: &'static str, slot_offset: usize, len: usize, ram: u32) -> SaveCopy {
    SaveCopy {
        name,
        slot_offset,
        len,
        ram,
        runtime: false,
    }
}

const MAIN: usize = 136;

/// Plain copies of a normal load, in slot order. Left out: standby characters (variable
/// length, `GameState+0x3984`), party members (converted by `func_02082d6c`), the zone's five
/// extra fields and the suspend-only blocks. Sources: `Save_ReadHeaderC` (`0x020ac874`),
/// `Save_UnpackMain` (`0x020a95b4`), `Save_UnpackProgress`, `Save_CopyEvents`,
/// `Save_UnpackHints`, `Save_UnpackZone`, `Save_UnpackDqvcMessages`,
/// `Save_UnpackTreasureMaps` and the `memcpy` calls of `Save_LoadSlot`.
pub const SAVE_COPIES: &[SaveCopy] = &[
    copy("header C (with its CRC)", 36, 0x54, GAME_STATE + 0x6380),
    copy("bag", MAIN + 0x1d5c, 0x1d4, GAME_STATE + 0x2a04),
    copy(
        "party list (order, count)",
        MAIN + 0x1d0d,
        5,
        GAME_STATE + 0x397c,
    ),
    copy("Main +0x2b08", MAIN + 0x2b08, 0x128, GAME_STATE + 0x3808),
    copy("gold in bank", MAIN + 0x2c34, 4, GAME_STATE + 0x396c),
    copy("gold on hand", MAIN + 0x2c30, 4, GAME_STATE + 0x3970),
    copy("Main +0x2c38", MAIN + 0x2c38, 4, GAME_STATE + 0x3974),
    copy("mini medals", MAIN + 0x2c3c, 4, GAME_STATE + 0x3978),
    copy("Main +0x2c44", MAIN + 0x2c44, 7, GAME_STATE + 0x5691),
    copy("area", MAIN + 0x2c4c, 2, GAME_STATE + 0x7e74),
    copy("Main +0x2c5a", MAIN + 0x2c5a, 1, GAME_STATE + 0x3981),
    copy("Main +0x2c5b", MAIN + 0x2c5b, 1, GAME_STATE + 0x63dc),
    copy("Main +0x2c5c", MAIN + 0x2c5c, 2, ZONE_3D + 0x2786),
    copy("Main +0x2c5e", MAIN + 0x2c5e, 2, ZONE_3D + 0x2784),
    copy(
        "Main +0x2c60 (vector, word)",
        MAIN + 0x2c60,
        16,
        ZONE_3D + 0x2774,
    ),
    copy("Main +0x2c70", MAIN + 0x2c70, 4, ZONE_3D + 0x2794),
    copy("Main +0x2c74", MAIN + 0x2c74, 1, ZONE_3D + 0x2788),
    copy("Main +0x2c77", MAIN + 0x2c77, 1, ZONE_3D + 0x27d0),
    copy("Main +0x2c78", MAIN + 0x2c78, 6, GAME_STATE + 0x74fe),
    copy("Main +0x2c88", MAIN + 0x2c88, 0x1a, QUEST_LOG + 0x4a8),
    copy("progress words", 11568, 0x10, GAME_STATE + 0x5cb0),
    SaveCopy {
        runtime: true,
        ..copy("event block", 11584, 0x334, QUEST_MANAGER)
    },
    copy("quest log", 12404, 0xa0, QUEST_LOG),
    copy("quest timers", 12564, 0x330, QUEST_LOG + 0x178),
    SaveCopy {
        runtime: true,
        ..copy("hints", 13384, 400, GAME_STATE + 0x5cdc)
    },
    copy("monsters", 13784, 0x4d0, GAME_STATE + 0x75f0),
    copy("alchemy", 15016, 0x3b4, GAME_STATE + 0x7ac0),
    copy("accolades", 15964, 0xec, GAME_STATE + 0x7504),
    copy("zone", 16200, 0x1b30, ZONE_3D + 0x840),
    copy("DQVC message 1", 23176, 0x200, GAME_STATE + 0x5f80),
    copy("DQVC message 2", 23688, 0x200, GAME_STATE + 0x6180),
    copy("DQVC word", 24200, 4, GAME_STATE + 0x5f7c),
    copy("treasure maps", 24204, 0xad6, GAME_STATE + 0x64f4),
    copy("profile", 26980, 0x7c, GAME_STATE + 0x569c),
    copy("DQVC", 27132, 0x2c4, GAME_STATE + 0x71fc),
    copy("misc", 27840, 0x110, GAME_STATE + 0x5e6c),
];

/// First of the four party member objects (`GameState_PartyMember`, `0x020108e8`).
pub const PARTY_MEMBER: u32 = GAME_STATE + 0x474;
/// Distance between two party member objects.
pub const PARTY_MEMBER_SIZE: u32 = 0x964;
/// Party member object field holding the id of [`crate::save::Slot::party_order`].
pub(super) const MEMBER_ID: u32 = 0x568;
/// Item `Save_UnpackMain` takes out of the held items and the bag on every load
/// (`func_020838d4`, `Bag_Remove`).
pub(super) const ITEM_REMOVED_ON_LOAD: u16 = 0x5612;

/// Plain copies from a character record into its party member object made by
/// `func_02082d7c`: `(field, member offset, record offset, length)`.
pub(super) const MEMBER_COPIES: &[(&str, u32, usize, usize)] = &[
    ("experience", 0x138, 0x1c, 13 * 4),
    ("revocations", 0x186, 0x0f, 13),
    ("equipment and look", 0x488, 0x160, 0x1c),
    ("record +0x17c", 0x4a4, 0x17c, 0xc0),
    ("record +0xf6", 0x464, 0xf6, 27),
    ("skill points", 0x564, 0xf4, 2),
    ("seeds", 0x850, 0x58, 13 * 12),
    ("record +0x11a", 0x8ec, 0x11a, 0x24),
    ("record +0x111", 0x910, 0x111, 9),
    ("vocation", 0x950, 0x50, 4),
    ("record +0x54", 0x954, 0x54, 2),
];

/// One party member of a save compared with its object in RAM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberCheck {
    /// Position in the party.
    pub party_index: usize,
    /// Index of the record ([`crate::save::Slot::character`]).
    pub character: usize,
    /// Address of the member object whose id matches, if any.
    pub member: Option<u32>,
    /// Fields whose RAM value differs from what the load computes from the record.
    pub differing: Vec<&'static str>,
}

/// What `Save_UnpackMain` (`0x020a95b4`) and `func_02082d7c` put in a party member object,
/// besides [`MEMBER_COPIES`]: `(field, member offset, bytes)`.
pub(super) fn member_fields(c: &[u8], held: [u16; 8], id: u8) -> Vec<(&'static str, u32, Vec<u8>)> {
    let word = u32::from_le_bytes([c[0x154], c[0x155], c[0x156], c[0x157]]);
    let levels: Vec<u8> = c[2..15].iter().flat_map(|&l| [l, 0]).collect();
    // func_020838d4: the first copy of the removed item becomes empty, then one pass moves
    // each empty word after the next one.
    let mut items = held;
    if let Some(k) = items.iter().position(|&w| w == ITEM_REMOVED_ON_LOAD) {
        items[k] = crate::save::NO_ITEM;
        for j in 0..7 {
            if (items[j] as i16) < 0 {
                items[j] = items[j + 1];
                items[j + 1] = crate::save::NO_ITEM;
            }
        }
    }
    vec![
        (
            "held items",
            0x454,
            items.iter().flat_map(|w| w.to_le_bytes()).collect(),
        ),
        ("levels", 0x16c, levels),
        ("record +1 low bits", 0x56a, vec![c[1] & 0xf]),
        (
            "record +1 high bits, +0x13f",
            0x56b,
            vec![(c[1] >> 5) | (c[0x13f] & 7) << 4],
        ),
        ("member id", MEMBER_ID, u16::from(id).to_le_bytes().to_vec()),
        (
            "word +0x154 (bits 10..20)",
            0x1c,
            (((word & 0xf_ffff) >> 10) as u16).to_le_bytes().to_vec(),
        ),
        (
            "word +0x154 (bits 20..30)",
            0x1e,
            ((((word >> 16) & 0x3fff) >> 4) as u16)
                .to_le_bytes()
                .to_vec(),
        ),
        // The hero (id 0) always gets 5.
        (
            "record +0x13f >> 3",
            0x94c,
            u32::from(if id == 0 { 5 } else { c[0x13f] >> 3 })
                .to_le_bytes()
                .to_vec(),
        ),
    ]
}
