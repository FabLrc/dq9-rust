//! melonDS savestate (`MELN`, melonDS 1.x) reader: the emulated memories, to use a running
//! game as an oracle for the reimplementation (`docs/re/melonds-state.md`).
//!
//! The container layout (sections, and where the memories sit inside `NDSG` and `CP15`) is
//! the one documented by ndskit (MIT, `melonds.py`); it is not copied from melonDS (GPL).
//! Checked on real states: resident code equals the ROM's, and [`SAVE_COPIES`] equals the
//! loaded save.

use nds_rom::{Error, Result};

use crate::save::Slot;

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
/// Quest manager whose start is the save's event block (`func_0205ec44` returns it).
pub const QUEST_MANAGER: u32 = 0x0210_8844;
/// Quest log (`QuestLog_Get`, `0x02094d7c`).
pub const QUEST_LOG: u32 = 0x0210_9418;
/// Current `Zone3D` object (`func_02012ff4` returns it).
pub const ZONE_3D: u32 = 0x020f_b3f0;

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

/// A parsed savestate, borrowing its bytes.
#[derive(Debug, Clone)]
pub struct State<'a> {
    data: &'a [u8],
    sections: Vec<Section>,
    ndsg: Section,
    cp15: Section,
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
        })
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Main RAM (4 MiB), from [`MAIN_RAM_BASE`].
    pub fn main_ram(&self) -> &'a [u8] {
        let at = self.ndsg.offset + NDSG_MAIN;
        &self.data[at..at + MAIN_RAM_SIZE]
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
                (self.dtcm(), (addr - DTCM_BASE) as usize)
            } else if (MAIN_RAM_BASE..0x0300_0000).contains(&addr) {
                (self.main_ram(), (addr & 0x3f_ffff) as usize)
            } else if addr < MAIN_RAM_BASE {
                (self.itcm(), (addr & 0x7fff) as usize)
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
}
