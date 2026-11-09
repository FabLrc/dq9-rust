//! melonDS savestate (`MELN`, melonDS 1.x) reader: the emulated memories, to use a running
//! game as an oracle for the reimplementation (`docs/re/melonds-state.md`).
//!
//! The container layout (sections, and where the memories sit inside `NDSG` and `CP15`) is
//! the one documented by ndskit (MIT, `melonds.py`); it is not copied from melonDS (GPL).
//! Checked on real states: resident code equals the ROM's, and [`SAVE_COPIES`] equals the
//! loaded save.

mod copies;
mod memory;

pub use copies::{MemberCheck, PARTY_MEMBER, PARTY_MEMBER_SIZE, SAVE_COPIES, SaveCopy};
pub use memory::{Checkpoint, Memory, NpcBody, NpcEntry, NpcPlacement, TrackState};

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
                Memory::from_parts(
                    &data[main_at..main_at + MAIN_RAM_SIZE],
                    &data[tcm_at..tcm_at + ITCM_SIZE],
                    &data[tcm_at + ITCM_SIZE..tcm_at + ITCM_SIZE + DTCM_SIZE],
                )
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

#[cfg(test)]
mod tests {
    use super::copies::{ITEM_REMOVED_ON_LOAD, MEMBER_COPIES, MEMBER_ID, member_fields};
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
