//! melonDS savestate (`MELN`, melonDS 1.x) reader: the emulated memories, to use a running
//! game as an oracle for the reimplementation (`docs/re/melonds-state.md`).
//!
//! The container layout (sections, and where the memories sit inside `NDSG` and `CP15`) is
//! the one documented by ndskit (MIT, `melonds.py`); it is not copied from melonDS (GPL).
//! No real state has been read yet: the tests use synthetic states.

use nds_rom::{Error, Result};

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
}
