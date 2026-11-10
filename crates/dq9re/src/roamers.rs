//! The roaming monsters of a melonDS state (`docs/re/encounter.md`, "Claims checked").
//!
//! The game keeps four map slots of `0x318` bytes at [`SLOT_ARRAY`] (`func_02027ca4`); a slot's
//! low two flag bits name its group, and the group's twelve monster objects are the
//! `GameState` objects `0x70 + 12 × group + i` (`func_02028a54`). A free object has a negative
//! monster number at `+2`. Nothing here is registered in `main.rs`: `tests/encounter_rom.rs`
//! includes this file to read states, and it compiles against `dq9-formats` alone.

use dq9_formats::melonds::{GAME_STATE, Memory, ZONE_3D};
use nds_rom::Result;

/// The map slot array (`func_02027ca4`, EU `0x02027cb4`).
pub const SLOT_ARRAY: u32 = 0x020f_dd44;
/// Bytes of one map slot.
pub const SLOT_SIZE: u32 = 0x318;
/// Map slots the game has.
pub const SLOT_COUNT: u32 = 4;
/// Index of the first roamer object in the `GameState` table.
pub const FIRST_OBJECT: u32 = 0x70;
/// Roamer objects per map slot.
pub const PER_SLOT: u32 = 12;
/// Bytes of one monster object.
pub const OBJECT_SIZE: usize = 0x198;

/// One map slot (`+0` map id, `+2` flags, `+6` spawned count, `+8` spawn timer in ms,
/// `+0xc` low byte: the slot has a spawn graph).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSlot {
    pub index: u32,
    pub map: u16,
    pub flags: u16,
    pub spawned: u16,
    pub timer_ms: u32,
    pub has_graph: bool,
}

impl MapSlot {
    /// The group of twelve objects the slot owns (flag bits 0–1).
    pub fn group(&self) -> u32 {
        u32::from(self.flags & 3)
    }

    /// Flag bit 2: the slot is in use.
    pub fn in_use(&self) -> bool {
        self.flags & 4 != 0
    }
}

/// One monster object, as the game keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roamer {
    /// Position in the slot's group, 0–11.
    pub slot: u32,
    /// Address of the object.
    pub object: u32,
    /// `+2`: the monster's number.
    pub monster: u16,
    /// `+0x44`: position, `fx32`.
    pub position: [i32; 3],
    /// `+0x54`: model angle, `fx32` radians.
    pub angle: i32,
    /// `+0x130`: the state (`encounter::roamer::state`).
    pub state: u8,
    /// `+0x14c`: milliseconds in the state.
    pub state_ms: i32,
    /// `+0x154`: milliseconds awake.
    pub awake_ms: i32,
    /// `+0xb8`: the graph node it stands on (an index).
    pub node: u16,
    /// `+0x164`: that node's id.
    pub node_id: u16,
    /// `+0x168`: the `encfld` zone it was drawn from.
    pub zone: u16,
    /// `+0x138..+0x13c`: wander, walk class, vision.
    pub wander: u8,
    pub walk: u8,
    pub vision: u8,
}

/// The day clock (`GameState+0x3cc`, `+0x3dc`; `docs/re/lighting.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayClock {
    pub seconds: f32,
    /// Night 0, morning 1, day 2, evening 3.
    pub phase: u32,
}

/// The four map slots.
pub fn map_slots(mem: &Memory<'_>) -> Result<Vec<MapSlot>> {
    (0..SLOT_COUNT)
        .map(|index| {
            let b = mem.read(SLOT_ARRAY + SLOT_SIZE * index, 0x10)?;
            Ok(MapSlot {
                index,
                map: u16::from_le_bytes([b[0], b[1]]),
                flags: u16::from_le_bytes([b[2], b[3]]),
                spawned: u16::from_le_bytes([b[6], b[7]]),
                timer_ms: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
                has_graph: b[12] != 0,
            })
        })
        .collect()
}

/// The slot in use for the current map (`Zone3D+0`), if any.
pub fn current_slot(mem: &Memory<'_>) -> Result<Option<MapSlot>> {
    let (map, _) = mem.current_map()?;
    Ok(map_slots(mem)?
        .into_iter()
        .find(|s| s.in_use() && s.map == map))
}

/// The roamers of `group`, in slot order (free objects skipped).
pub fn roamers(mem: &Memory<'_>, group: u32) -> Result<Vec<Roamer>> {
    let mut out = Vec::new();
    for i in 0..PER_SLOT {
        let Some(object) = mem.object(FIRST_OBJECT + PER_SLOT * group + i)? else {
            continue;
        };
        let b = mem.read(object, OBJECT_SIZE)?;
        let s16 = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
        let i32_at = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let monster = s16(2);
        if monster < 0 {
            continue;
        }
        out.push(Roamer {
            slot: i,
            object,
            monster: monster as u16,
            position: [i32_at(0x44), i32_at(0x48), i32_at(0x4c)],
            angle: i32_at(0x54),
            state: b[0x130],
            state_ms: i32_at(0x14c),
            awake_ms: i32_at(0x154),
            node: s16(0xb8) as u16,
            node_id: s16(0x164) as u16,
            zone: s16(0x168) as u16,
            wander: b[0x138],
            walk: b[0x139],
            vision: b[0x13a],
        });
    }
    Ok(out)
}

/// The day clock of the state.
pub fn day_clock(mem: &Memory<'_>) -> Result<DayClock> {
    Ok(DayClock {
        seconds: f32::from_bits(mem.u32(GAME_STATE + 0x3cc)?),
        phase: mem.u32(GAME_STATE + 0x3dc)?,
    })
}

/// A text report: map, clock, the slot and one line per roamer.
pub fn report(mem: &Memory<'_>) -> Result<String> {
    use std::fmt::Write;
    let (map, area) = mem.current_map()?;
    let clock = day_clock(mem)?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "vblank {} map {map} area {area} clock {:.3} s phase {} (zone3d {ZONE_3D:#x})",
        mem.vblank_count()?,
        clock.seconds,
        clock.phase
    );
    let Some(slot) = current_slot(mem)? else {
        out.push_str("no map slot in use for this map\n");
        return Ok(out);
    };
    let list = roamers(mem, slot.group())?;
    let _ = writeln!(
        out,
        "slot {} group {} spawned {} timer {} ms graph {} roamers {}",
        slot.index,
        slot.group(),
        slot.spawned,
        slot.timer_ms,
        slot.has_graph,
        list.len()
    );
    let _ = writeln!(
        out,
        "slot object   mon zone node(id) state state_ms awake_ms   x        y        z      angle"
    );
    for r in &list {
        let _ = writeln!(
            out,
            "{:>4} {:#010x} {:>4} {:>4} {:>4}({:>3}) {:>5} {:>8} {:>8} {:>8.3} {:>8.3} {:>8.3} {:>6}",
            r.slot,
            r.object,
            r.monster,
            r.zone,
            r.node,
            r.node_id,
            r.state,
            r.state_ms,
            r.awake_ms,
            f64::from(r.position[0]) / 4096.0,
            f64::from(r.position[1]) / 4096.0,
            f64::from(r.position[2]) / 4096.0,
            r.angle
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::melonds::MAIN_RAM_SIZE;

    const BASE: u32 = 0x0200_0000;

    fn put(ram: &mut [u8], addr: u32, bytes: &[u8]) {
        let at = (addr - BASE) as usize;
        ram[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// One slot on map 20002 owning group 0, object 0x70 a roamer, 0x71 free.
    fn synthetic() -> Vec<u8> {
        let mut ram = vec![0u8; MAIN_RAM_SIZE];
        put(&mut ram, ZONE_3D, &[0x22, 0x4e, 0, 0, 1, 0]);
        put(&mut ram, SLOT_ARRAY, &20002u16.to_le_bytes());
        put(&mut ram, SLOT_ARRAY + 2, &0xcu16.to_le_bytes());
        put(&mut ram, SLOT_ARRAY + 6, &1u16.to_le_bytes());
        put(&mut ram, SLOT_ARRAY + 8, &1500u32.to_le_bytes());
        put(&mut ram, SLOT_ARRAY + 0xc, &[1]);
        let first = 0x0230_0000u32;
        for i in 0..PER_SLOT {
            let object = first + i * OBJECT_SIZE as u32;
            put(
                &mut ram,
                GAME_STATE + 8 + 4 * (FIRST_OBJECT + i),
                &object.to_le_bytes(),
            );
            put(&mut ram, object + 2, &(-1i16).to_le_bytes());
        }
        put(&mut ram, first + 2, &28i16.to_le_bytes());
        put(&mut ram, first + 0x44, &(-41 * 4096i32).to_le_bytes());
        put(&mut ram, first + 0x130, &[8]);
        put(&mut ram, first + 0x14c, &1001i32.to_le_bytes());
        put(&mut ram, first + 0xb8, &6i16.to_le_bytes());
        put(&mut ram, first + 0x164, &21i16.to_le_bytes());
        put(&mut ram, first + 0x168, &2i16.to_le_bytes());
        put(&mut ram, GAME_STATE + 0x3cc, &45.5f32.to_le_bytes());
        put(&mut ram, GAME_STATE + 0x3dc, &1u32.to_le_bytes());
        ram
    }

    #[test]
    fn reads_the_slot_and_only_the_used_objects() {
        let ram = synthetic();
        let mem = Memory::from_main_ram(&ram).unwrap();
        let slot = current_slot(&mem).unwrap().unwrap();
        assert_eq!(
            (slot.map, slot.group(), slot.spawned, slot.timer_ms),
            (20002, 0, 1, 1500)
        );
        let list = roamers(&mem, slot.group()).unwrap();
        assert_eq!(list.len(), 1);
        let r = list[0];
        assert_eq!(
            (r.slot, r.monster, r.zone, r.node, r.node_id),
            (0, 28, 2, 6, 21)
        );
        assert_eq!((r.state, r.state_ms, r.position[0]), (8, 1001, -41 * 4096));
        let clock = day_clock(&mem).unwrap();
        assert_eq!((clock.seconds, clock.phase), (45.5, 1));
        assert!(report(&mem).unwrap().contains("roamers 1"));
    }

    #[test]
    fn a_map_without_a_slot_reports_it() {
        let mut ram = synthetic();
        put(&mut ram, ZONE_3D, &[0x01, 0, 0, 0, 0, 0]);
        let mem = Memory::from_main_ram(&ram).unwrap();
        assert_eq!(current_slot(&mem).unwrap(), None);
        assert!(report(&mem).unwrap().contains("no map slot"));
    }
}
