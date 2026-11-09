//! melonDS savestate (`MELN`, melonDS 1.x) reader: the emulated memories, to use a running
//! game as an oracle for the reimplementation (`docs/re/melonds-state.md`).
//!
//! The container layout (sections, and where the memories sit inside `NDSG` and `CP15`) is
//! the one documented by ndskit (MIT, `melonds.py`); it is not copied from melonDS (GPL).
//! Checked on real states: resident code equals the ROM's, and [`SAVE_COPIES`] equals the
//! loaded save.

use nds_rom::{Error, Result};

use crate::cast::Story;
use crate::maplist::MapList;
use crate::save::{EVENT_FLAGS_SIZE, EventFlags, Slot};

/// Start of main RAM in the ARM9 address space.
pub const MAIN_RAM_BASE: u32 = 0x0200_0000;
/// Main RAM size of a DS (the `NDSG` section reserves 16 MiB for the DSi).
pub const MAIN_RAM_SIZE: usize = 0x40_0000;
/// ITCM size, and where the game's code sees it.
pub const ITCM_SIZE: usize = 0x8000;
/// DTCM size.
pub const DTCM_SIZE: usize = 0x4000;
/// Default DTCM base used by the game (`Dtcm` autoload).
pub const DTCM_BASE: u32 = 0x027E_0000;
/// The SDK's VBlank counter (`0x027FFC3C`), to align a state with a run.
pub const VBLANK_COUNT: u32 = 0x027F_FC3C;
/// Address of the `GameState` singleton (`GameState::GetInstance` reads it from a literal pool).
pub const GAME_STATE: u32 = 0x020f_33d8;
/// Entries of the game object table at `GameState+8` (party, NPCs, monsters).
pub const OBJECT_COUNT: u32 = 0xe9;
/// Quest manager whose start is the save's event block (`func_0205ec44` returns it).
pub const QUEST_MANAGER: u32 = 0x0210_8844;
/// Quest log (`QuestLog_Get`, `0x02094d7c`).
pub const QUEST_LOG: u32 = 0x0210_9418;
/// Current `Zone3D` object (`func_02012ff4` returns it).
pub const ZONE_3D: u32 = 0x020f_b3f0;
/// The field's NPC table (`0x0203cf5c` returns it; `ov_017::021a2c14` fills it through
/// `0x0203d038`): `+0xc` 32 entry pointers, `+0x8c` count, `+0x90` cast list, `+0x94`
/// placement chains.
pub const NPC_TABLE: u32 = 0x0210_75f4;
/// Most entries the NPC table stores.
pub const NPC_SLOTS: u32 = 0x20;

/// What an NPC entry of the RAM table draws with: `+0x14` sprite (`0x9c` bytes), `+0x18`
/// model (`0xd8`-byte `Object3D`), `+0x1c` game object (a party member slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcBody {
    Sprite(u32),
    Model(u32),
    Object(u32),
}

/// One entry of the RAM NPC table (`0x20` bytes), with its cast record (`+8`, `0x14` bytes)
/// and placement node (`+0xc`, `0x78` bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcEntry {
    pub address: u32,
    /// `+0`: entry flags (the placement's `+0xc` ORed in).
    pub flags: u32,
    /// Cast record: id (`+0`), kind (`+2`), field 4 (`+3`), name (`+4`, 12 bytes).
    pub id: u16,
    pub kind: u8,
    pub byte3: u8,
    pub name: Vec<u8>,
    /// Placement node: NPC (`+0`), map (`+2`), position (`+0x10`), angle (`+0x1c`), motion
    /// byte (`+0x1e`), counter (`+0x44`), quest (`+0x46`); `None` without a node.
    pub placement: Option<NpcPlacement>,
    pub body: Option<NpcBody>,
    /// Position (`fx32`) and angle of the body: sprite `+4`/`+0x14`, `Object3D` `+0x44`/`+0x54`.
    pub position: Option<[i32; 3]>,
    pub angle: Option<i32>,
}

/// The RAM placement node fields [`NpcEntry`] compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcPlacement {
    pub npc: u8,
    pub map: u16,
    pub position: [i32; 3],
    pub angle: i16,
    pub motion: u8,
    pub order: u16,
    pub quest: i16,
    pub flags: u32,
}

/// One scenario track of the event block (`docs/re/save.md`, § "Event block"):
/// progress bytes and the flags set in each of its two sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackState {
    pub chapter: u8,
    pub step: u8,
    pub substep: u8,
    /// Set 0 (13 bytes), cleared when the chapter changes; flag indices set.
    pub chapter_flags: Vec<i32>,
    /// Set 1 (12 bytes), cleared when the step changes; flag indices set.
    pub step_flags: Vec<i32>,
}

/// The story state a differential run compares at a checkpoint: the whole event block
/// (tracks, global flags, quests), the current map and the frame counter. Built from the
/// same RAM layout the save's event block uses (`State::checkpoint`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// SDK VBlank counter (`0x027ffc3c`): the frame the checkpoint was taken on.
    pub vblank: u32,
    /// Current map id and area (`Zone3D+0`, `+4`).
    pub map: u16,
    pub area: u16,
    /// Index of the current scenario track (`quest manager +0x332`).
    pub current_track: usize,
    pub tracks: Vec<TrackState>,
    /// `Save_IsDaytime` (`GameState+0x3dc` ≠ 0).
    pub daytime: bool,
    /// Global flags set (`Flags_Test`, bit indices).
    pub flags: Vec<i32>,
    /// Quest nibbles that are non-zero (`quest manager +0x2cc`).
    pub quests: Vec<(i32, u8)>,
}

impl Checkpoint {
    /// Progress of the current track, `(chapter, step, substep)`; `None` only if the
    /// checkpoint was built with an out-of-range index (the reader rejects that).
    pub fn scenario(&self) -> Option<(u8, u8, u8)> {
        self.tracks
            .get(self.current_track)
            .map(|t| (t.chapter, t.step, t.substep))
    }
}

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
/// Party member object field holding the id of [`Slot::party_order`].
const MEMBER_ID: u32 = 0x568;
/// Item `Save_UnpackMain` takes out of the held items and the bag on every load
/// (`func_020838d4`, `Bag_Remove`).
const ITEM_REMOVED_ON_LOAD: u16 = 0x5612;

/// Plain copies from a character record into its party member object made by
/// `func_02082d7c`: `(field, member offset, record offset, length)`.
const MEMBER_COPIES: &[(&str, u32, usize, usize)] = &[
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
    /// Index of the record ([`Slot::character`]).
    pub character: usize,
    /// Address of the member object whose id matches, if any.
    pub member: Option<u32>,
    /// Fields whose RAM value differs from what the load computes from the record.
    pub differing: Vec<&'static str>,
}

/// What `Save_UnpackMain` (`0x020a95b4`) and `func_02082d7c` put in a party member object,
/// besides [`MEMBER_COPIES`]: `(field, member offset, bytes)`.
fn member_fields(c: &[u8], held: [u16; 8], id: u8) -> Vec<(&'static str, u32, Vec<u8>)> {
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

/// Section header: 4-byte name, 32-bit length (header included), 8 more bytes.
const SECTION_HEADER: usize = 16;
const FIRST_SECTION: usize = 0x10;
/// Where each memory starts inside its section.
const NDSG_MAIN: usize = 0x14;
const CP15_TCM: usize = 0x1c;
const SHARED_WRAM_SIZE: usize = 0x8000;
const ARM7_WRAM_SIZE: usize = 0x1_0000;
const WRAM_AFTER_MAIN: usize = 0x100_0000;

/// One section of the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub name: [u8; 4],
    /// Offset of the section header in the file.
    pub offset: usize,
    /// Length including the 16-byte header.
    pub len: usize,
}

/// The ARM9-visible memories of a running game, whatever holds them: a melonDS savestate
/// ([`State`]) or a raw main-RAM image ([`Memory::from_main_ram`], as `--dump-ram` writes).
/// Every address reader lives here once, so a `.ml1` and a 4 MiB dump are read by the same
/// code (`docs/re/melonds-state.md`, § "Neutral state").
#[derive(Debug, Clone, Copy)]
pub struct Memory<'a> {
    main: &'a [u8],
    itcm: Option<&'a [u8]>,
    dtcm: Option<&'a [u8]>,
}

/// A parsed savestate, borrowing its bytes.
#[derive(Debug, Clone)]
pub struct State<'a> {
    data: &'a [u8],
    sections: Vec<Section>,
    ndsg: Section,
    cp15: Section,
    memory: Memory<'a>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

impl<'a> State<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        match data.first_chunk::<4>() {
            Some(b"MELN") => {}
            Some(found) => {
                return Err(Error::BadMagic {
                    what: "melonDS savestate",
                    found: *found,
                });
            }
            None => return Err(Error::Malformed("savestate truncated")),
        }
        let mut sections = Vec::new();
        let mut at = FIRST_SECTION;
        while at + SECTION_HEADER <= data.len() {
            let len = u32_at(data, at + 4).unwrap_or(0) as usize;
            if len < SECTION_HEADER || at + len > data.len() {
                break;
            }
            let name = [data[at], data[at + 1], data[at + 2], data[at + 3]];
            sections.push(Section {
                name,
                offset: at,
                len,
            });
            at += len;
        }
        let find = |name: &[u8; 4], what: &'static str, min: usize| -> Result<Section> {
            let s = sections
                .iter()
                .find(|s| &s.name == name)
                .copied()
                .ok_or(Error::Malformed(what))?;
            if s.len < min {
                return Err(Error::Malformed(what));
            }
            Ok(s)
        };
        let ndsg = find(
            b"NDSG",
            "savestate has no NDSG section",
            NDSG_MAIN + MAIN_RAM_SIZE,
        )?;
        let cp15 = find(
            b"CP15",
            "savestate has no CP15 section",
            CP15_TCM + ITCM_SIZE + DTCM_SIZE,
        )?;
        Ok(Self {
            data,
            sections,
            ndsg,
            cp15,
            memory: {
                let main_at = ndsg.offset + NDSG_MAIN;
                let tcm_at = cp15.offset + CP15_TCM;
                Memory {
                    main: &data[main_at..main_at + MAIN_RAM_SIZE],
                    itcm: Some(&data[tcm_at..tcm_at + ITCM_SIZE]),
                    dtcm: Some(&data[tcm_at + ITCM_SIZE..tcm_at + ITCM_SIZE + DTCM_SIZE]),
                }
            },
        })
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The shared address readers over these memories.
    pub fn memory(&self) -> Memory<'a> {
        self.memory
    }

    /// Main RAM (4 MiB), from [`MAIN_RAM_BASE`].
    pub fn main_ram(&self) -> &'a [u8] {
        self.memory.main_ram()
    }

    /// Shared WRAM (32 KiB) when the section holds it, else `None`.
    pub fn shared_wram(&self) -> Option<&'a [u8]> {
        self.after_main(0, SHARED_WRAM_SIZE)
    }

    /// ARM7 private WRAM (64 KiB) when the section holds it, else `None`.
    pub fn arm7_wram(&self) -> Option<&'a [u8]> {
        self.after_main(SHARED_WRAM_SIZE, ARM7_WRAM_SIZE)
    }

    fn after_main(&self, skip: usize, len: usize) -> Option<&'a [u8]> {
        let at = self.ndsg.offset + NDSG_MAIN + WRAM_AFTER_MAIN + skip;
        let end = (self.ndsg.offset + self.ndsg.len).min(self.data.len());
        (at + len <= end).then(|| &self.data[at..at + len])
    }

    pub fn itcm(&self) -> &'a [u8] {
        let at = self.cp15.offset + CP15_TCM;
        &self.data[at..at + ITCM_SIZE]
    }

    pub fn dtcm(&self) -> &'a [u8] {
        let at = self.cp15.offset + CP15_TCM + ITCM_SIZE;
        &self.data[at..at + DTCM_SIZE]
    }
}

/// A savestate is a container: the story readers are those of its memories.
impl<'a> core::ops::Deref for State<'a> {
    type Target = Memory<'a>;

    fn deref(&self) -> &Self::Target {
        &self.memory
    }
}

impl<'a> Memory<'a> {
    /// A raw main-RAM image: `main` is the whole 4 MiB from [`MAIN_RAM_BASE`], and no TCM
    /// is available (reads into the ITCM or DTCM fail).
    pub fn from_main_ram(main: &'a [u8]) -> Result<Self> {
        if main.len() != MAIN_RAM_SIZE {
            return Err(Error::Malformed("main RAM image is not 4 MiB"));
        }
        Ok(Self {
            main,
            itcm: None,
            dtcm: None,
        })
    }

    /// Main RAM (4 MiB), from [`MAIN_RAM_BASE`].
    pub fn main_ram(&self) -> &'a [u8] {
        self.main
    }

    /// ITCM when the container holds it.
    pub fn itcm(&self) -> Option<&'a [u8]> {
        self.itcm
    }

    /// DTCM when the container holds it.
    pub fn dtcm(&self) -> Option<&'a [u8]> {
        self.dtcm
    }

    /// `len` bytes at an ARM9 address: main RAM (`0x02000000..0x03000000`, mirrored every
    /// 4 MiB), the DTCM at [`DTCM_BASE`] (checked first) or the ITCM (`< 0x02000000`,
    /// mirrored every 32 KiB). A read must stay inside one memory.
    pub fn read(&self, addr: u32, len: usize) -> Result<&'a [u8]> {
        let out_of_bounds = |size| Error::OutOfBounds {
            offset: addr as usize,
            len,
            size,
        };
        let (mem, offset): (&[u8], usize) =
            if (DTCM_BASE..DTCM_BASE + DTCM_SIZE as u32).contains(&addr) {
                (self.dtcm.unwrap_or(&[]), (addr - DTCM_BASE) as usize)
            } else if (MAIN_RAM_BASE..0x0300_0000).contains(&addr) {
                (self.main, (addr & 0x3f_ffff) as usize)
            } else if addr < MAIN_RAM_BASE {
                (self.itcm.unwrap_or(&[]), (addr & 0x7fff) as usize)
            } else {
                return Err(out_of_bounds(0));
            };
        offset
            .checked_add(len)
            .and_then(|end| mem.get(offset..end))
            .ok_or_else(|| out_of_bounds(mem.len()))
    }

    pub fn u32(&self, addr: u32) -> Result<u32> {
        let b = self.read(addr, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// The SDK's VBlank counter, the frame number to match a state with a run.
    pub fn vblank_count(&self) -> Result<u32> {
        self.u32(VBLANK_COUNT)
    }

    /// Scenario `(chapter, step, substep)` of the running game (`Save_Chapter`,
    /// `Save_ScenarioStep`, `Save_ScenarioSubstep`: `GameState+0x5cb0..0x5cb8`).
    pub fn scenario(&self) -> Result<(u32, u32, u32)> {
        Ok((
            self.u32(GAME_STATE + 0x5cb0)?,
            self.u32(GAME_STATE + 0x5cb4)?,
            self.u32(GAME_STATE + 0x5cb8)?,
        ))
    }

    /// Address of game object `index` (`GameState+8`, table of [`OBJECT_COUNT`] pointers read
    /// by `Chara_Get`, `0x0200fd80`), `None` for an empty entry or an index past the table.
    pub fn object(&self, index: u32) -> Result<Option<u32>> {
        if index >= OBJECT_COUNT {
            return Ok(None);
        }
        let ptr = self.u32(GAME_STATE + 8 + 4 * index)?;
        Ok((ptr != 0).then_some(ptr))
    }

    /// Index of the protagonist's object (`GameState+0x3ac`, read by
    /// `GameState::GetProtagonist`, `0x0200fddc`).
    pub fn protagonist_index(&self) -> Result<u32> {
        self.u32(GAME_STATE + 0x3ac)
    }

    /// World position of a game object: `Object3D::position_` (`+0x44`, three fx32 with
    /// 12 fractional bits), the start of every game object.
    pub fn object_position(&self, object: u32) -> Result<[i32; 3]> {
        let b = self.read(object + 0x44, 12)?;
        Ok(core::array::from_fn(|i| {
            i32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
        }))
    }

    /// Heading of a game object: `fx32` radians at `+0x54` (set from the angle given to
    /// `func_020399b0` when the party is placed, `ov_017::0219c598`).
    pub fn object_heading(&self, object: u32) -> Result<i32> {
        Ok(self.u32(object + 0x54)? as i32)
    }

    /// Current map id and its area (`Zone3D+0`, `+4`, set by `Zone3D::SwitchZone`,
    /// `0x0201384c`).
    pub fn current_map(&self) -> Result<(u16, u16)> {
        let b = self.read(ZONE_3D, 6)?;
        Ok((
            u16::from_le_bytes([b[0], b[1]]),
            u16::from_le_bytes([b[4], b[5]]),
        ))
    }

    /// Indices of the map records in RAM (`GameState+0x468`: pointer, count) that differ
    /// from `list`, ignoring the bytes the game leaves undefined (see
    /// [`crate::maplist::MapRecord::to_bytes`]); a count mismatch is an error.
    pub fn compare_map_list(&self, list: &MapList<'_>) -> Result<Vec<usize>> {
        let ptr = self.u32(GAME_STATE + 0x468)?;
        let count = self.u32(GAME_STATE + 0x46c)? as usize;
        if count != list.records.len() {
            return Err(Error::Malformed("map list count differs from RAM"));
        }
        let ram = self.read(ptr, count * 16)?;
        Ok(list
            .records
            .iter()
            .zip(ram.as_chunks::<16>().0)
            .enumerate()
            .filter(|(_, (record, ram))| {
                let mut ram = **ram;
                ram[4] &= 1;
                ram[5 + record.code.len()..0xc].fill(0);
                ram[0xf] = 0;
                record.to_bytes() != ram
            })
            .map(|(i, _)| i)
            .collect())
    }

    /// For each of [`SAVE_COPIES`], the offsets (relative to the copy) where RAM differs from
    /// `slot`: all empty when the state's game was loaded from that slot and the copies have
    /// not been changed since (except those marked `runtime`).
    pub fn compare_save(&self, slot: &Slot<'_>) -> Result<Vec<(SaveCopy, Vec<usize>)>> {
        let bytes = slot.bytes();
        SAVE_COPIES
            .iter()
            .map(|&c| {
                let saved = bytes
                    .get(c.slot_offset..c.slot_offset + c.len)
                    .ok_or(Error::Malformed("save slot shorter than its blocks"))?;
                let ram = self.read(c.ram, c.len)?;
                let diffs = (0..c.len).filter(|&i| saved[i] != ram[i]).collect();
                Ok((c, diffs))
            })
            .collect()
    }

    /// Each party member of `slot` compared with the member object that has its id: the
    /// fields `func_02082d7c` fills from the character record (the name, converted to text,
    /// and three flag bits are not compared).
    pub fn compare_party(&self, slot: &Slot<'_>) -> Result<Vec<MemberCheck>> {
        let standby = usize::from(slot.standby_count());
        let mut members = Vec::new();
        for k in 0..4 {
            let at = PARTY_MEMBER + k * PARTY_MEMBER_SIZE;
            members.push((at, self.read(at + MEMBER_ID, 2)?));
        }
        slot.party_order()
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let character = standby + i;
                let record = slot
                    .character(character)
                    .ok_or(Error::Malformed("party member past the character records"))?;
                let held = slot
                    .held_items(i)
                    .ok_or(Error::Malformed("party of more than 4"))?;
                let member = members
                    .iter()
                    .find(|(_, w)| u16::from_le_bytes([w[0], w[1]]) == u16::from(id))
                    .map(|&(at, _)| at);
                let mut differing = Vec::new();
                if let Some(at) = member {
                    let c = record.bytes();
                    for &(name, off, src, len) in MEMBER_COPIES {
                        if self.read(at + off, len)? != &c[src..src + len] {
                            differing.push(name);
                        }
                    }
                    for (name, off, bytes) in member_fields(c, held, id) {
                        if self.read(at + off, bytes.len())? != bytes.as_slice() {
                            differing.push(name);
                        }
                    }
                }
                Ok(MemberCheck {
                    party_index: i,
                    character,
                    member,
                    differing,
                })
            })
            .collect()
    }

    fn i32x3(&self, addr: u32) -> Result<[i32; 3]> {
        let b = self.read(addr, 12)?;
        Ok(core::array::from_fn(|i| {
            i32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
        }))
    }

    /// The story point the placement scripts test: track progress, `Save_IsDaytime`
    /// (`GameState+0x3dc`), global flags (quest manager `+0x8c`, `0x234` bytes) and quest
    /// nibbles (`+0x2cc`, `0xcc` quests).
    pub fn story(&self) -> Result<Story> {
        let (chapter, step, substep) = self.scenario()?;
        let flags = self.read(QUEST_MANAGER + 0x8c, 0x234)?;
        let nibbles = self.read(QUEST_MANAGER + 0x2cc, 0x66)?;
        Ok(Story {
            chapter: chapter as u8,
            step: step as u8,
            substep: substep as u8,
            daytime: self.u32(GAME_STATE + 0x3dc)? != 0,
            flags: (0..flags.len() * 8)
                .filter(|&i| flags[i / 8] >> (i % 8) & 1 != 0)
                .map(|i| i as i32)
                .collect(),
            quests: (0..0xcc)
                .map(|q| (q, nibbles[q as usize / 2] >> (4 * (q % 2)) & 0xf))
                .filter(|&(_, n)| n != 0)
                .collect(),
        })
    }

    /// The game's event block as `Save_UnpackEvents` leaves it in RAM: tracks, global flags,
    /// quest nibbles and the current track (`quest manager`, `0x334` bytes; `docs/re/save.md`,
    /// § "Event block").
    pub fn event_flags(&self) -> Result<EventFlags<'a>> {
        let bytes = self.read(QUEST_MANAGER, EVENT_FLAGS_SIZE)?;
        EventFlags::from_bytes(bytes).ok_or(Error::Malformed("event block shorter than 0x334"))
    }

    /// The checkpoint a differential run compares (`docs/re/melonds-state.md`, § "Differential
    /// oracle"): the whole event block, the current map and the frame counter.
    pub fn checkpoint(&self) -> Result<Checkpoint> {
        let events = self.event_flags()?;
        let bits = |bytes: &[u8]| -> Vec<i32> {
            (0..bytes.len() * 8)
                .filter(|&i| bytes[i / 8] >> (i % 8) & 1 != 0)
                .map(|i| i as i32)
                .collect()
        };
        let tracks: Vec<TrackState> = (0..)
            .map_while(|i| events.track(i))
            .map(|t| TrackState {
                chapter: t.chapter(),
                step: t.step(),
                substep: t.substep(),
                chapter_flags: bits(t.chapter_flags()),
                step_flags: bits(t.step_flags()),
            })
            .collect();
        let current_track = events.current_track();
        if current_track >= tracks.len() {
            return Err(Error::Malformed("current scenario track out of range"));
        }
        let (map, area) = self.current_map()?;
        Ok(Checkpoint {
            vblank: self.vblank_count()?,
            map,
            area,
            current_track,
            tracks,
            daytime: self.u32(GAME_STATE + 0x3dc)? != 0,
            flags: bits(events.global_flags()),
            quests: (0..0xcc)
                .map(|q| (q, events.quest(q)))
                .filter(|&(_, n)| n != 0)
                .collect(),
        })
    }

    /// The entries of the NPC table ([`NPC_TABLE`]), in table order.
    pub fn npcs(&self) -> Result<Vec<NpcEntry>> {
        let count = self.u32(NPC_TABLE + 0x8c)?.min(NPC_SLOTS);
        let mut out = Vec::new();
        for i in 0..count {
            let address = self.u32(NPC_TABLE + 0xc + 4 * i)?;
            if address == 0 {
                continue;
            }
            let flags = self.u32(address)?;
            let def = self.u32(address + 8)?;
            let (id, kind, byte3, name) = if def == 0 {
                (0, 0, 0, Vec::new())
            } else {
                let d = self.read(def, 0x10)?;
                let name = d[4..0x10].split(|&b| b == 0).next().unwrap_or(&[]).to_vec();
                (u16::from_le_bytes([d[0], d[1]]), d[2], d[3], name)
            };
            let node = self.u32(address + 0xc)?;
            let placement = if node == 0 {
                None
            } else {
                let n = self.read(node, 0x48)?;
                let h = |o: usize| u16::from_le_bytes([n[o], n[o + 1]]);
                Some(NpcPlacement {
                    npc: n[0],
                    map: h(2),
                    position: self.i32x3(node + 0x10)?,
                    angle: h(0x1c) as i16,
                    motion: n[0x1e],
                    order: h(0x44),
                    quest: h(0x46) as i16,
                    flags: self.u32(node + 0xc)?,
                })
            };
            let [sprite, model, object] =
                [0x14, 0x18, 0x1c].map(|o| self.u32(address + o).unwrap_or(0));
            let (body, position, angle) = if sprite != 0 {
                let a = self.u32(sprite + 0x14)? as i32;
                (
                    Some(NpcBody::Sprite(sprite)),
                    Some(self.i32x3(sprite + 4)?),
                    Some(a),
                )
            } else if model != 0 || object != 0 {
                let (o, body) = if model != 0 {
                    (model, NpcBody::Model(model))
                } else {
                    (object, NpcBody::Object(object))
                };
                let a = self.u32(o + 0x54)? as i32;
                (Some(body), Some(self.i32x3(o + 0x44)?), Some(a))
            } else {
                (None, None, None)
            };
            out.push(NpcEntry {
                address,
                flags,
                id,
                kind,
                byte3,
                name,
                placement,
                body,
                position,
                angle,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(out: &mut Vec<u8>, name: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(name);
        out.extend_from_slice(&((body.len() + SECTION_HEADER) as u32).to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(body);
    }

    /// Builds a state whose main RAM, ITCM and DTCM bytes are `f(address)`-like markers.
    fn synthetic() -> Vec<u8> {
        synthetic_with(|_| {})
    }

    /// Same, letting `edit` change main RAM (indexed by `addr & 0x3fffff`) first.
    fn synthetic_with(edit: impl FnOnce(&mut [u8])) -> Vec<u8> {
        let mut out = b"MELN".to_vec();
        out.resize(FIRST_SECTION, 0);
        let mut ndsg = vec![0u8; NDSG_MAIN - SECTION_HEADER];
        let mut main = vec![0u8; MAIN_RAM_SIZE];
        let put = |m: &mut Vec<u8>, addr: u32, v: u32| {
            let o = (addr & 0x3f_ffff) as usize;
            m[o..o + 4].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut main, GAME_STATE + 0x5cb0, 19);
        put(&mut main, GAME_STATE + 0x5cb4, 2);
        put(&mut main, GAME_STATE + 0x5cb8, 1);
        put(&mut main, VBLANK_COUNT, 1234);
        edit(&mut main);
        ndsg.extend_from_slice(&main);
        section(&mut out, b"ARM9", &[0; 20]);
        section(&mut out, b"NDSG", &ndsg);
        let mut cp15 = vec![0u8; CP15_TCM - SECTION_HEADER];
        let mut itcm = vec![0u8; ITCM_SIZE];
        itcm[0x10] = 0xaa;
        let mut dtcm = vec![0u8; DTCM_SIZE];
        dtcm[4] = 0xbb;
        cp15.extend_from_slice(&itcm);
        cp15.extend_from_slice(&dtcm);
        section(&mut out, b"CP15", &cp15);
        out
    }

    #[test]
    fn reads_scenario_and_frame_from_main_ram() {
        let data = synthetic();
        let st = State::parse(&data).unwrap();
        assert_eq!(st.scenario().unwrap(), (19, 2, 1));
        assert_eq!(st.vblank_count().unwrap(), 1234);
        assert_eq!(st.sections().len(), 3);
        assert_eq!(&st.sections()[1].name, b"NDSG");
    }

    #[test]
    fn tcm_and_mirrors_are_addressed() {
        let data = synthetic();
        let st = State::parse(&data).unwrap();
        assert_eq!(st.read(DTCM_BASE + 4, 1).unwrap(), [0xbb]);
        assert_eq!(st.read(0x10, 1).unwrap(), [0xaa]);
        assert_eq!(st.read(0x0100_8010, 1).unwrap(), [0xaa]);
        // main RAM mirrors every 4 MiB inside 0x02000000..0x03000000
        assert_eq!(
            st.u32(VBLANK_COUNT - 0x40_0000).unwrap(),
            st.u32(VBLANK_COUNT).unwrap()
        );
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        assert!(State::parse(b"").is_err());
        assert!(State::parse(b"NOPE0000000000000000").is_err());
        let data = synthetic();
        // truncated inside the NDSG section: the section is dropped, so parsing fails
        assert!(State::parse(&data[..0x1000]).is_err());
        let st = State::parse(&data).unwrap();
        assert!(st.read(0x0200_0000 + 0x3f_fffe, 4).is_err());
        assert!(st.read(0x0400_0000, 1).is_err());
        assert!(st.read(u32::MAX, usize::MAX).is_err());
        assert!(st.shared_wram().is_none() && st.arm7_wram().is_none());
    }

    #[test]
    fn compare_save_reports_each_copy_that_differs() {
        let mut file = vec![0u8; crate::save::FILE_SIZE];
        for (i, b) in file.iter_mut().enumerate() {
            *b = (i * 7 + 3) as u8;
        }
        let save = crate::save::Save::parse(&file).unwrap();
        let slot = save.slot(0).unwrap();
        let load = |main: &mut [u8]| {
            for c in SAVE_COPIES {
                let o = (c.ram & 0x3f_ffff) as usize;
                main[o..o + c.len]
                    .copy_from_slice(&slot.bytes()[c.slot_offset..c.slot_offset + c.len]);
            }
        };
        let data = synthetic_with(load);
        let st = State::parse(&data).unwrap();
        assert!(
            st.compare_save(&slot)
                .unwrap()
                .iter()
                .all(|(_, d)| d.is_empty())
        );

        // The game changes one byte of the monster records after loading.
        let monsters = SAVE_COPIES.iter().find(|c| c.name == "monsters").unwrap();
        let data = synthetic_with(|main| {
            load(main);
            main[(monsters.ram & 0x3f_ffff) as usize + 5] ^= 1;
        });
        let st = State::parse(&data).unwrap();
        for (c, diffs) in st.compare_save(&slot).unwrap() {
            let expected: &[usize] = if c == *monsters { &[5] } else { &[] };
            assert_eq!(diffs, expected, "{}", c.name);
        }
    }

    #[test]
    fn save_copies_do_not_overlap() {
        let mut by_slot: Vec<_> = SAVE_COPIES.iter().collect();
        by_slot.sort_by_key(|c| c.slot_offset);
        for w in by_slot.windows(2) {
            assert!(
                w[0].slot_offset + w[0].len <= w[1].slot_offset,
                "{}",
                w[1].name
            );
        }
        let mut by_ram: Vec<_> = SAVE_COPIES.iter().collect();
        by_ram.sort_by_key(|c| c.ram);
        for w in by_ram.windows(2) {
            assert!(w[0].ram + w[0].len as u32 <= w[1].ram, "{}", w[1].name);
        }
    }

    #[test]
    fn removed_item_leaves_a_gap_moved_by_one_pass_only() {
        let record = [0u8; 572];
        let held = [
            1,
            ITEM_REMOVED_ON_LOAD,
            2,
            0xffff,
            3,
            0xffff,
            0xffff,
            0xffff,
        ];
        let fields = member_fields(&record, held, 1);
        let (_, _, items) = fields.iter().find(|f| f.0 == "held items").unwrap();
        let words: Vec<u16> = items
            .chunks(2)
            .map(|w| u16::from_le_bytes([w[0], w[1]]))
            .collect();
        assert_eq!(words, [1, 2, 0xffff, 3, 0xffff, 0xffff, 0xffff, 0xffff]);
    }

    #[test]
    fn compare_party_finds_members_by_id() {
        let mut file = vec![0u8; crate::save::FILE_SIZE];
        for (i, b) in file.iter_mut().enumerate() {
            *b = (i * 13 + 5) as u8;
        }
        // One character at the inn, then a party of two whose ids are 0 and 1.
        file[7572] = 1;
        file[7573..7577].copy_from_slice(&[0, 1, 0xff, 0xff]);
        file[7577] = 2;
        let save = crate::save::Save::parse(&file).unwrap();
        let slot = save.slot(0).unwrap();
        // Member objects 2 and 0 hold ids 0 and 1; the other two hold no party id.
        let objects = [(2u32, 0usize), (0, 1)];
        let fill = |main: &mut [u8], change_level: bool| {
            for k in 0..4u32 {
                let at = ((PARTY_MEMBER + k * PARTY_MEMBER_SIZE + MEMBER_ID) & 0x3f_ffff) as usize;
                main[at..at + 2].copy_from_slice(&0x7fffu16.to_le_bytes());
            }
            for (k, i) in objects {
                let base = (PARTY_MEMBER + k * PARTY_MEMBER_SIZE) & 0x3f_ffff;
                let c = slot.character(1 + i).unwrap().bytes();
                let id = slot.party_order()[i];
                for &(_, off, src, len) in MEMBER_COPIES {
                    let o = (base + off) as usize;
                    main[o..o + len].copy_from_slice(&c[src..src + len]);
                }
                for (_, off, bytes) in member_fields(c, slot.held_items(i).unwrap(), id) {
                    let o = (base + off) as usize;
                    main[o..o + bytes.len()].copy_from_slice(&bytes);
                }
                if change_level && i == 1 {
                    main[(base + 0x16c + 2 * 3) as usize] ^= 1;
                }
            }
        };
        let data = synthetic_with(|m| fill(m, false));
        let checks = State::parse(&data).unwrap().compare_party(&slot).unwrap();
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].member, Some(PARTY_MEMBER + 2 * PARTY_MEMBER_SIZE));
        assert_eq!(checks[1].member, Some(PARTY_MEMBER));
        assert_eq!(checks[1].character, 2);
        assert!(checks.iter().all(|c| c.differing.is_empty()));

        let data = synthetic_with(|m| fill(m, true));
        let checks = State::parse(&data).unwrap().compare_party(&slot).unwrap();
        assert!(checks[0].differing.is_empty());
        assert_eq!(checks[1].differing, ["levels"]);
    }

    #[test]
    fn protagonist_position_follows_the_object_table() {
        let object = 0x022b_0000u32;
        let data = synthetic_with(|m| {
            let put = |m: &mut [u8], addr: u32, v: i32| {
                let o = (addr & 0x3f_ffff) as usize;
                m[o..o + 4].copy_from_slice(&v.to_le_bytes());
            };
            put(m, GAME_STATE + 8 + 4 * 2, object as i32);
            put(m, GAME_STATE + 0x3ac, 2);
            put(m, object + 0x44, -0x3_9000);
            put(m, object + 0x48, 0x199);
            put(m, object + 0x4c, 0x1_f800);
        });
        let st = State::parse(&data).unwrap();
        let index = st.protagonist_index().unwrap();
        assert_eq!(index, 2);
        let at = st.object(index).unwrap().unwrap();
        assert_eq!(at, object);
        assert_eq!(
            st.object_position(at).unwrap(),
            [-0x3_9000, 0x199, 0x1_f800]
        );
        assert_eq!(st.object(0).unwrap(), None);
        assert_eq!(st.object(OBJECT_COUNT).unwrap(), None);
    }

    #[test]
    fn checkpoint_reads_tracks_flags_and_map() {
        let data = synthetic_with(|m| {
            let mut put = |addr: u32, bytes: &[u8]| {
                let o = (addr & 0x3f_ffff) as usize;
                m[o..o + bytes.len()].copy_from_slice(bytes);
            };
            // Track 0: 1.2.3, chapter flag 0 set, step flag 1 set.
            put(QUEST_MANAGER, &[1, 2, 3]);
            put(QUEST_MANAGER + 3, &[0x01]);
            put(QUEST_MANAGER + 16, &[0x02]);
            put(QUEST_MANAGER + 0x332, &[0]);
            // Global flag 0x13f, quests 4 = 3 and 5 = 1.
            put(QUEST_MANAGER + 0x8c + 0x27, &[0x80]);
            put(QUEST_MANAGER + 0x2cc + 2, &[0x13]);
            // Map 1100, area 3, daytime.
            put(ZONE_3D, &[0x4c, 0x04, 0, 0, 3, 0]);
            put(GAME_STATE + 0x3dc, &1u32.to_le_bytes());
        });
        let st = State::parse(&data).unwrap();
        let c = st.checkpoint().unwrap();
        assert_eq!(c.scenario(), Some((1, 2, 3)));
        assert_eq!((c.map, c.area, c.current_track), (1100, 3, 0));
        assert!(c.daytime);
        assert_eq!(c.tracks.len(), 5);
        assert_eq!(c.tracks[0].chapter_flags, [0]);
        assert_eq!(c.tracks[0].step_flags, [1]);
        assert!(c.flags.contains(&0x13f));
        assert_eq!(c.quests, [(4, 3), (5, 1)]);
        assert_eq!(c.vblank, 1234);
    }

    #[test]
    fn checkpoint_rejects_an_out_of_range_track() {
        let data = synthetic_with(|m| {
            m[(QUEST_MANAGER + 0x332) as usize & 0x3f_ffff] = 5;
        });
        let st = State::parse(&data).unwrap();
        assert!(st.checkpoint().is_err());
    }

    #[test]
    fn npc_table_and_story_read_back() {
        let (entry, def, node, sprite): (u32, u32, u32, u32) =
            (0x0230_0000, 0x0230_0100, 0x0230_0200, 0x0230_0300);
        let data = synthetic_with(|m| {
            let mut put = |addr: u32, bytes: &[u8]| {
                let o = (addr & 0x3f_ffff) as usize;
                m[o..o + bytes.len()].copy_from_slice(bytes);
            };
            put(NPC_TABLE + 0x8c, &1u32.to_le_bytes());
            put(NPC_TABLE + 0xc, &entry.to_le_bytes());
            put(entry, &0x40u32.to_le_bytes());
            put(entry + 8, &def.to_le_bytes());
            put(entry + 0xc, &node.to_le_bytes());
            put(entry + 0x14, &sprite.to_le_bytes());
            put(def, &[25, 0, 0, 2, b'n', b'0', b'1', b'6', b'a', 0]);
            put(node, &[25, 0, 109, 0]);
            put(node + 0xc, &0x40u32.to_le_bytes());
            put(node + 0x10, &696i32.to_le_bytes());
            put(node + 0x1c, &778u16.to_le_bytes());
            put(node + 0x44, &[0x3d, 0, 0xff, 0xff]);
            put(sprite + 4, &696i32.to_le_bytes());
            put(sprite + 0x14, &778u32.to_le_bytes());
            put(GAME_STATE + 0x3dc, &1u32.to_le_bytes());
            put(QUEST_MANAGER + 0x8c + 0x27, &[0x80]);
            put(QUEST_MANAGER + 0x2cc + 2, &[0x13]);
        });
        let st = State::parse(&data).unwrap();
        let npcs = st.npcs().unwrap();
        assert_eq!(npcs.len(), 1);
        let n = &npcs[0];
        assert_eq!(
            (n.id, n.kind, n.byte3, n.name.as_slice()),
            (25, 0, 2, &b"n016a"[..])
        );
        let p = n.placement.unwrap();
        assert_eq!(
            (p.npc, p.map, p.angle, p.order, p.quest),
            (25, 109, 778, 0x3d, -1)
        );
        assert_eq!((p.position, p.flags), ([696, 0, 0], 0x40));
        assert_eq!(n.body, Some(NpcBody::Sprite(sprite)));
        assert_eq!((n.position, n.angle), (Some([696, 0, 0]), Some(778)));
        let story = st.story().unwrap();
        assert_eq!((story.chapter, story.step, story.substep), (19, 2, 1));
        assert!(story.daytime);
        assert_eq!(story.flags.iter().copied().collect::<Vec<_>>(), [0x13f]);
        assert_eq!(
            story
                .quests
                .iter()
                .map(|(&q, &n)| (q, n))
                .collect::<Vec<_>>(),
            [(4, 3), (5, 1)]
        );
    }

    /// The same main RAM as the savestate above, without the container: every story reader
    /// must give the same answer (`--dump-ram` + `dq9re state --ram`).
    #[test]
    fn main_ram_image_reads_like_a_savestate() {
        let object = 0x0220_0000u32;
        let data = synthetic_with(|m| {
            let mut put = |addr: u32, bytes: &[u8]| {
                let o = (addr & 0x3f_ffff) as usize;
                m[o..o + bytes.len()].copy_from_slice(bytes);
            };
            put(ZONE_3D, &4105u16.to_le_bytes());
            put(ZONE_3D + 4, &4100u16.to_le_bytes());
            put(QUEST_MANAGER + 0x8c + 0x27, &[0x80]);
            put(QUEST_MANAGER, &[1, 1, 1]);
            put(GAME_STATE + 0x3ac, &0u32.to_le_bytes());
            put(GAME_STATE + 8, &object.to_le_bytes());
            put(
                object + 0x44,
                &[
                    0x8f, 0x0a, 0, 0, 0x00, 0xfc, 0xff, 0xff, 0x3e, 0xc6, 0xff, 0xff,
                ],
            );
            put(object + 0x54, &0x3400u32.to_le_bytes());
        });
        let state = State::parse(&data).unwrap();
        let ram = state.main_ram().to_vec();
        let image = Memory::from_main_ram(&ram).unwrap();
        assert_eq!(image.checkpoint().unwrap(), state.checkpoint().unwrap());
        assert_eq!(image.story().unwrap(), state.story().unwrap());
        assert_eq!(image.current_map().unwrap(), (4105, 4100));
        assert_eq!(image.protagonist_index().unwrap(), 0);
        let at = image.object(0).unwrap().unwrap();
        assert_eq!(at, object);
        assert_eq!(image.object_position(at).unwrap(), [2703, -1024, -14786]);
        assert_eq!(image.object_heading(at).unwrap(), 0x3400);
        let c = image.checkpoint().unwrap();
        assert_eq!((c.map, c.area, c.vblank), (4105, 4100, 1234));
        assert_eq!(c.scenario(), Some((1, 1, 1)));
        assert_eq!(c.flags, [0x13f]);
        assert!(c.tracks[0].step_flags.is_empty());
        // Main RAM mirrors every 4 MiB; the TCMs are not part of a raw image.
        assert_eq!(
            image.u32(VBLANK_COUNT).unwrap(),
            image.u32(VBLANK_COUNT - 0x40_0000).unwrap()
        );
        assert!(image.read(DTCM_BASE, 1).is_err());
        assert!(image.read(0x10, 1).is_err());
        assert!(image.read(0x0200_0000 + 0x3f_fffe, 4).is_err());
    }

    #[test]
    fn main_ram_image_needs_the_full_four_mebibytes() {
        assert!(Memory::from_main_ram(&[]).is_err());
        assert!(Memory::from_main_ram(&vec![0u8; MAIN_RAM_SIZE - 1]).is_err());
        assert!(Memory::from_main_ram(&vec![0u8; MAIN_RAM_SIZE + 1]).is_err());
        assert!(Memory::from_main_ram(&vec![0u8; MAIN_RAM_SIZE]).is_ok());
    }
}
