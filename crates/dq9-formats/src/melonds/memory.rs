//! The ARM9-visible memories of a running game and the readers built on them
//! (`docs/re/melonds-state.md`, § "Neutral state").

use nds_rom::{Error, Result};

use super::copies::{
    MEMBER_COPIES, MEMBER_ID, MemberCheck, PARTY_MEMBER, PARTY_MEMBER_SIZE, SAVE_COPIES, SaveCopy,
    member_fields,
};
use super::{
    DTCM_BASE, DTCM_SIZE, GAME_STATE, MAIN_RAM_BASE, MAIN_RAM_SIZE, NPC_SLOTS, NPC_TABLE,
    OBJECT_COUNT, QUEST_MANAGER, VBLANK_COUNT, ZONE_3D,
};
use crate::cast::Story;
use crate::maplist::MapList;
use crate::save::{EVENT_FLAGS_SIZE, EventFlags, Slot};

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

/// Indices of the bits set in `bytes`.
fn set_bits(bytes: &[u8]) -> impl Iterator<Item = i32> + '_ {
    (0..bytes.len() * 8)
        .filter(move |&i| bytes[i / 8] >> (i % 8) & 1 != 0)
        .map(|i| i as i32)
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

impl<'a> Memory<'a> {
    /// The memories of a parsed savestate: main RAM and both TCMs.
    pub(super) fn from_parts(main: &'a [u8], itcm: &'a [u8], dtcm: &'a [u8]) -> Self {
        Self {
            main,
            itcm: Some(itcm),
            dtcm: Some(dtcm),
        }
    }

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
        self.i32x3(object + 0x44)
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
            flags: set_bits(flags).collect(),
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
        let tracks: Vec<TrackState> = (0..)
            .map_while(|i| events.track(i))
            .map(|t| TrackState {
                chapter: t.chapter(),
                step: t.step(),
                substep: t.substep(),
                chapter_flags: set_bits(t.chapter_flags()).collect(),
                step_flags: set_bits(t.step_flags()).collect(),
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
            flags: set_bits(events.global_flags()).collect(),
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
                let a = self.object_heading(o)?;
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
