//! Who stands where on a map: the cast list `<map>npc.bin` and the placement script
//! `<map>place.bin` of `data/scenario/<area>.npc`, run as the field's cast loader
//! (`ov_017::021a2c14`) runs them for one map and story point, then what each NPC is drawn
//! with (`0x0203d038`). Layout and evidence in `docs/re/characters.md`.
//!
//! The placement script is a record table whose kinds are opcodes (`g_PlaceHandlers`,
//! `0x020f0994`). Records 3, 4, 5, 14, 17 and 21 add a placement to their NPC's chain when
//! their condition holds (story span, time of day, flags, quests) and the map is this one, or
//! take the NPC away when the map is another; the others attach data (talk zones, wander
//! area, path, flags, model override…) to the placement the last counted record made. The
//! head of a chain is where the NPC stands.
//!
//! port: 0x0206c020, 0x0206c108, 0x0206c2d0, 0x0206c4f8, 0x0206c624, 0x0206c704, 0x0206c83c
//! port: 0x0206c948, 0x0206ca4c, 0x0206cae8, 0x0206cb58, 0x0206cbdc, 0x0206cf68, 0x0206d3e4
//! port: 0x0206d474, 0x0206d4f0, 0x0206d744, 0x0206d7f4, 0x0206d8f8, 0x0206d9e8, 0x0206da90
//! port: 0x0206db30, 0x0206db58, 0x0206dd78, 0x0206bf3c, 0x02064d58, 0x02064e3c, 0x020981f4
//! port: 0x02098278

use std::collections::{BTreeMap, BTreeSet};

use nds_rom::{Narc, Result};

use crate::chr::Chr;
use crate::map::{fix32_cos, fix32_sin};
use crate::table::{Fields, Table, fx32};

/// Story point and world state the placement script tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Story {
    /// `Save_Chapter`, `Save_ScenarioStep`, `Save_ScenarioSubstep` (current track).
    pub chapter: u8,
    pub step: u8,
    pub substep: u8,
    /// `Save_IsDaytime` (`0x020103c4`: save `+0x3dc` ≠ 0).
    pub daytime: bool,
    /// Global flags set (quest manager `+0x8c`, `Flags_Test`), by index.
    pub flags: BTreeSet<i32>,
    /// Quest nibbles (quest manager `+0x2cc`), by quest; absent = 0.
    pub quests: BTreeMap<i32, u8>,
}

impl Story {
    /// Progress weight `chapter × 1000 + step × 10 + substep`, as the spans compare it.
    pub fn now(&self) -> i32 {
        i32::from(self.substep) + i32::from(self.chapter) * 1000 + i32::from(self.step) * 10
    }

    fn nibble(&self, quest: i32) -> u8 {
        if (0..0xcc).contains(&quest) {
            self.quests.get(&quest).copied().unwrap_or(0)
        } else {
            0
        }
    }

    /// `Quest_State` (`0x0206e130`).
    pub fn quest_state(&self, quest: i32) -> u8 {
        self.nibble(quest) & 3
    }

    /// Records 14 and 21: quest condition `cond` on `quest` (`Quest_State`, `Quest_Bit2`,
    /// `Quest_Bit3`); unknown conditions hold.
    fn quest_condition(&self, quest: i32, cond: i32) -> bool {
        let state = self.quest_state(quest);
        match cond {
            0 => state == 2,
            1 => self.nibble(quest) & 4 != 0,
            2 => state == 3,
            3 => state == 1,
            4 => self.nibble(quest) & 8 != 0 && state == 0,
            -1 => state == 0,
            _ => true,
        }
    }

    /// Time-of-day word of a record: 0 by day only, 1 by night only, any other value always.
    fn time_matches(&self, time: i32) -> bool {
        match time {
            0 => self.daytime,
            1 => !self.daytime,
            _ => true,
        }
    }

    /// `Flags_Test` / `Flags_TestExt` (`0x0206eba8`: indices from `0x400` shifted by `0x6fa`).
    fn flag(&self, index: i32, extended: bool) -> bool {
        let index = if extended && index >= 0x400 {
            index + 0x6fa
        } else {
            index
        };
        self.flags.contains(&index)
    }
}

/// A talk zone (record 6): `Talk_NearestNpc` asks with `label` from inside the rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TalkZone {
    /// Fields 1-4 × 4096: x and z at most, then x and z at least, in the data.
    pub rect: [i32; 4],
    pub label: i32,
}

/// Placement node (`0x78` bytes, `0x0206bf3c`): one record's place for one NPC, with what
/// later records attached to it. Offsets are the node's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement<'a> {
    /// `+0`: NPC id, kept on 8 bits.
    pub npc: u8,
    /// `+2`: map id.
    pub map: u16,
    /// `+4..+6`, `+7..+9`: story span `(chapter, step, substep)` of records 5 and 21; record 4
    /// keeps its point in `from` and leaves `to` at 0; blocks keep both at 0.
    pub from: [u8; 3],
    pub to: [u8; 3],
    /// `+0xa` bits 0-1: time-of-day word (0 day, 1 night, 2 always).
    pub time: u8,
    /// `+0xa` bit 2: placed by flags (record 17).
    pub by_flags: bool,
    /// `+0xa` bits 3-5: record 18's value **[inconnu]**.
    pub state: u8,
    /// `+0xa` bit 6: placed by quest and span (record 21).
    pub by_quest_span: bool,
    /// `+0xb`: record 12's NPC **[inconnu : un PNJ lié]**.
    pub linked_npc: u8,
    /// `+0xc`: flags ORed by records 11, 15 (`0x800`, seated) and 16 (`0x2000`), copied into
    /// the NPC object's flags (`0x02040550`).
    pub flags: u32,
    /// `+0x10`: position (`fx32`, fields × 4096 truncated), before the ground snap.
    pub position: [i32; 3],
    /// `+0x1c`: facing, radians × 4096 truncated (`fx32` kept on 16 bits).
    pub angle: i16,
    /// `+0x1e`: optional last field: the model's initial motion index.
    pub motion: u8,
    /// `+0x1f`: movement mode set by records 7 (area), 8 (segment), 9 and 10 (paths).
    pub mode: u8,
    /// `+0x20`: record 22's colour (RGB555) added to the model's palette.
    pub tint: Option<u16>,
    /// `+0x22`: record 8's direction (`fx32` radians on 16 bits), or record 9's point count
    /// minus one.
    pub heading: i16,
    /// `+0x24..+0x30`: area of records 7 and 8 (`[x_max, z_max, x_min, z_min]`).
    pub area: [i32; 4],
    /// `+0x34`: points (`fx32`) of records 9 and 10, a closed loop.
    pub path: Vec<[i32; 3]>,
    /// `+0x38`: record 13's value × 4096, `0x7a` by default, given to the NPC object
    /// (`0x02040c0c`) **[hypothèse : vitesse]**.
    pub speed: i32,
    /// `+0x3c`: record 15's angle × 4096 (seated NPCs).
    pub seat_angle: i32,
    /// `+0x40`: talk zones (record 6), in file order.
    pub zones: Vec<TalkZone>,
    /// `+0x44`: value of the record counter when made (attaching records find it by this).
    pub order: u16,
    /// `+0x46`: quest of records 14 and 21, -1 otherwise.
    pub quest: i16,
    /// `+0x48`: record 16's vector (the `s069` object) **[inconnu]**.
    pub prop: [i32; 3],
    /// `+0x54`, `+0x58`: record 19's archive (relative to `data/`) and motion name.
    pub chr: Option<&'a [u8]>,
    pub motion_name: Option<&'a [u8]>,
    /// `+0x5c`: record 20's `[f2, 0, f3, f1]` × 4096 **[inconnu]**.
    pub extra: Option<[i32; 4]>,
    /// `+0x60`: record 18's optional value × 4096 **[inconnu]**.
    pub state_value: i32,
}

impl Placement<'_> {
    fn new(npc: u8, map: u16) -> Self {
        Self {
            npc,
            map,
            from: [0; 3],
            to: [0; 3],
            time: 0,
            by_flags: false,
            state: 0,
            by_quest_span: false,
            linked_npc: 0,
            flags: 0,
            position: [0; 3],
            angle: 0,
            motion: 0,
            mode: 0,
            tint: None,
            heading: 0,
            area: [0; 4],
            path: Vec::new(),
            speed: 0x7a,
            seat_angle: 0,
            zones: Vec::new(),
            order: 0,
            quest: -1,
            prop: [0; 3],
            chr: None,
            motion_name: None,
            extra: None,
            state_value: 0,
        }
    }

    /// Priority class of `0x0206db58` (lower first): 0 by flags, 1 by quest and span, 2 by
    /// quest, 3 a point or a span ending at step 0 but starting at a step, 4 a span, 5 a block.
    pub fn class(&self) -> u8 {
        if self.by_flags {
            0
        } else if self.by_quest_span {
            1
        } else if self.quest >= 0 {
            2
        } else if self.from[1] != 0 && self.to[1] == 0 {
            3
        } else if self.to[1] != 0 {
            4
        } else {
            5
        }
    }

    fn start(&self) -> u32 {
        u32::from(self.from[2]) + u32::from(self.from[0]) * 1000 + u32::from(self.from[1]) * 10
    }

    /// Whether `self`, inserted after `other`, goes before it in the chain (`0x0206db58`):
    /// lower class first; within classes 1 and 4 the earlier start, then the earlier record;
    /// within the others the earlier record.
    fn precedes(&self, other: &Self) -> bool {
        let (a, b) = (self.class(), other.class());
        if a != b {
            return a < b;
        }
        if (a == 4 || a == 1) && self.start() != other.start() {
            return self.start() < other.start();
        }
        self.order < other.order
    }
}

/// A `<map>npc.bin` record (kind 3, `0x02064d58`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastMember<'a> {
    /// Field 0: NPC id (dialogue files `NNN_<lang>.bin`, placements).
    pub id: u16,
    /// Field 1: 0 sprite, 1 none (examined thing), 2 model, 5 monster.
    pub kind: u8,
    /// Field 2: Japanese name (Shift-JIS, read and dropped by the game).
    pub japanese_name: Option<&'a [u8]>,
    /// Field 3: resource name (`n003a`, `s017`, `z015d`).
    pub name: Option<&'a [u8]>,
    /// Field 4 (`+3`) **[inconnu]**.
    pub byte3: u8,
}

impl CastMember<'_> {
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(self.name.unwrap_or_default()).into_owned()
    }
}

/// What an NPC is drawn with (`0x0203d038`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Appearance {
    /// Nothing: kind 1 or another kind, or a kind-0 member without a name.
    None,
    /// Kind 0: a sprite sheet `data/ani/<name>.spr`, or `<name>_s.spr` when the placement is
    /// seated (flag `0x800`) and that sheet exists.
    Sprite { sheet: String },
    /// Kind 2: a model archive `data/chara_sub/<name>.chr` (`LoadFromCCHROrCMOTArchive`).
    Model { chr: String },
    /// Kind 5: member `<name>.mon` of `data/pack_lv5/enemy.gp2`, whose first `.cchr` member
    /// (LZ-compressed) is loaded like a model.
    Monster { member: String },
}

/// One NPC of the cast: its record and the placement at the head of its chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Npc<'a> {
    pub member: CastMember<'a>,
    pub placement: Placement<'a>,
}

impl Npc<'_> {
    /// What the NPC is drawn with; `exists` answers whether a ROM file exists (for seated
    /// sprites).
    pub fn appearance(&self, exists: impl Fn(&str) -> bool) -> Appearance {
        let name = self.member.name_str();
        match self.member.kind {
            2 => Appearance::Model {
                chr: format!("data/chara_sub/{name}.chr"),
            },
            5 => Appearance::Monster {
                member: format!("{name}.mon"),
            },
            0 if !name.is_empty() => {
                let seated = format!("data/ani/{name}_s.spr");
                let sheet = if self.placement.flags & 0x800 != 0 && exists(&seated) {
                    seated
                } else {
                    format!("data/ani/{name}.spr")
                };
                Appearance::Sprite { sheet }
            }
            _ => Appearance::None,
        }
    }

    /// Whether the game snaps the NPC's height to the ground after loading (not for flags
    /// `0x800` and `0x1000`, `ov_017::021a2c14`).
    pub fn snapped_to_ground(&self) -> bool {
        self.placement.flags & 0x1800 == 0
    }
}

/// The NPCs created for one map, in the order of the game's NPC table (the cast list is
/// built by prepending: last record first).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cast<'a> {
    pub npcs: Vec<Npc<'a>>,
    /// Whether a record read a field no record had written: garbage in the game.
    pub read_unset: bool,
}

/// `data/scenario/<area>.npc` for a map code: its first three characters, `F` alone for
/// field maps (`ov_017::021a2c14`).
pub fn archive_path(code: &[u8]) -> String {
    let area = if code.first() == Some(&b'F') {
        &code[..1]
    } else {
        &code[..code.len().min(3)]
    };
    format!("data/scenario/{}.npc", String::from_utf8_lossy(area))
}

/// Script context (`0x02108cec`).
struct Run<'s, 'a> {
    story: &'s Story,
    map: u16,
    /// `+0xa`: counter of records 3, 4, 5, 14, 17 and 21.
    counter: u16,
    /// `+6`, `+8`, `+4`: NPC of the last record 14/21, quest of its last record 14 and 21.
    last_npc: i16,
    quest14: i16,
    quest21: i16,
    /// The NPCs' chains, head first (`+0x10`: list of chain heads).
    chains: BTreeMap<u8, Vec<Placement<'a>>>,
}

impl<'a> Run<'_, 'a> {
    /// `0x0206db30`: the chain of NPC `id` (compared with the 8-bit id of its nodes).
    fn chain_mut(&mut self, id: i32) -> Option<&mut Vec<Placement<'a>>> {
        u8::try_from(id)
            .ok()
            .and_then(|id| self.chains.get_mut(&id))
    }

    /// `0x0206dd78`: takes the NPC away (its whole chain).
    fn remove(&mut self, id: i32) {
        if let Ok(id) = u8::try_from(id) {
            self.chains.remove(&id);
        }
    }

    /// `0x0206db58`: inserts before the first node it precedes; a node that precedes none
    /// is dropped (except the first of its NPC).
    fn insert(&mut self, node: Placement<'a>) {
        match self.chains.get_mut(&node.npc) {
            None => {
                self.chains.insert(node.npc, vec![node]);
            }
            Some(chain) => {
                if let Some(i) = chain.iter().position(|other| node.precedes(other)) {
                    chain.insert(i, node);
                }
            }
        }
    }

    /// The node the attaching records find: NPC `id`'s node made by the current counter.
    fn current(&mut self, id: i32) -> Option<&mut Placement<'a>> {
        let counter = self.counter;
        self.chain_mut(id)?.iter_mut().find(|p| p.order == counter)
    }

    /// Common tail of the placing records: position, angle, optional motion byte, insert.
    fn place(&mut self, mut node: Placement<'a>, f: &Fields<'a>, at: usize, n: usize) {
        node.position = [
            fx32(f.float(at)),
            fx32(f.float(at + 1)),
            fx32(f.float(at + 2)),
        ];
        node.angle = fx32(f.float(at + 3)) as i16;
        if n > at + 4 {
            node.motion = f.int(at + 4) as u8;
        }
        node.order = self.counter;
        self.insert(node);
    }

    fn record(&mut self, kind: u16, f: &Fields<'a>) {
        let n = f.count();
        let story = self.story;
        match kind {
            // Block (0x0206c020): [map, npc, x, y, z, angle, (motion)].
            3 => {
                self.counter = self.counter.wrapping_add(1);
                let map = f.int(0);
                if map != i32::from(self.map) {
                    return;
                }
                let node = Placement::new(f.int(1) as u8, map as u16);
                if n < 3 {
                    self.remove(i32::from(node.npc));
                    return;
                }
                self.place(node, f, 2, n);
            }
            // Point (0x0206c108): [chapter, step, substep, time, map, npc, x…].
            4 => {
                self.counter = self.counter.wrapping_add(1);
                let point = [f.int(0), f.int(1), f.int(2)];
                if point
                    != [
                        i32::from(story.chapter),
                        i32::from(story.step),
                        i32::from(story.substep),
                    ]
                {
                    return;
                }
                let time = f.int(3);
                if !story.time_matches(time) {
                    return;
                }
                let (map, npc) = (f.int(4), f.int(5));
                if map != i32::from(self.map) {
                    self.remove(npc);
                    return;
                }
                let mut node = Placement::new(npc as u8, map as u16);
                node.from = point.map(|v| v as u8);
                node.time = time as u8 & 3;
                if n < 7 {
                    self.remove(npc);
                    return;
                }
                self.place(node, f, 6, n);
            }
            // Span (0x0206c2d0): [from ×3, to ×3, time, map, npc, x…].
            5 => {
                self.counter = self.counter.wrapping_add(1);
                let weight = |a: i32, b: i32, c: i32| c + a * 1000 + b * 10;
                let (from, to) = (
                    [f.int(0), f.int(1), f.int(2)],
                    [f.int(3), f.int(4), f.int(5)],
                );
                let now = story.now();
                if !(weight(from[0], from[1], from[2]) <= now && now <= weight(to[0], to[1], to[2]))
                {
                    return;
                }
                let time = f.int(6);
                if !story.time_matches(time) {
                    return;
                }
                let (map, npc) = (f.int(7), f.int(8));
                if map != i32::from(self.map) {
                    self.remove(npc);
                    return;
                }
                let mut node = Placement::new(npc as u8, map as u16);
                node.from = from.map(|v| v as u8);
                node.to = to.map(|v| v as u8);
                node.time = time as u8 & 3;
                if n < 10 {
                    self.remove(npc);
                    return;
                }
                self.place(node, f, 9, n);
            }
            // Talk zone (0x0206c4f8): [npc, f1, f2, f3, f4, label].
            6 => {
                let zone = TalkZone {
                    rect: [
                        fx32(f.float(1)),
                        fx32(f.float(2)),
                        fx32(f.float(3)),
                        fx32(f.float(4)),
                    ],
                    label: f.int(5),
                };
                if let Some(p) = self.current(f.int(0)) {
                    p.zones.push(zone);
                }
            }
            // Area (0x0206c624): [npc, x_max, z_max, x_min, z_min].
            7 => {
                let area = [1, 2, 3, 4].map(|i| fx32(f.float(i)));
                if let Some(p) = self.current(f.int(0)) {
                    p.area = area;
                    p.mode = 7;
                }
            }
            // Segment (0x0206c704): [npc, direction, distance] around the position.
            8 => {
                let heading = fx32(f.float(1)) as i16;
                let distance = i64::from(fx32(f.float(2)));
                if let Some(p) = self.current(f.int(0)) {
                    p.mode = 8;
                    p.heading = heading;
                    let scale = |t: i32| ((distance * i64::from(t) + 0x800) >> 12) as i32;
                    let dx = scale(fix32_sin(i32::from(heading)));
                    let dz = scale(fix32_cos(i32::from(heading)));
                    let [x, _, z] = p.position;
                    let mut area = [
                        x.wrapping_add(dx),
                        z.wrapping_add(dz),
                        x.wrapping_sub(dx),
                        z.wrapping_sub(dz),
                    ];
                    if area[0] < area[2] {
                        area.swap(0, 2);
                    }
                    if area[1] < area[3] {
                        area.swap(1, 3);
                    }
                    p.area = area;
                }
            }
            // Paths (0x0206c83c, 0x0206c948): [npc, x, y, z, …].
            9 | 10 => {
                if n == 0 || !(n - 1).is_multiple_of(3) {
                    return;
                }
                let points: Vec<[i32; 3]> = (0..(n - 1) / 3)
                    .map(|k| {
                        let at = 1 + 3 * k;
                        [
                            fx32(f.float(at)),
                            fx32(f.float(at + 1)),
                            fx32(f.float(at + 2)),
                        ]
                    })
                    .collect();
                if points.is_empty() {
                    return;
                }
                if let Some(p) = self.current(f.int(0)) {
                    p.mode = kind as u8;
                    if kind == 9 {
                        // A byte store into the low half of `+0x22`.
                        let low = (points.len() - 1) as u8;
                        p.heading = (p.heading as u16 & 0xff00 | u16::from(low)) as i16;
                    }
                    p.path = points;
                }
            }
            // Flags (0x0206ca4c): [npc, flags…].
            11 => {
                let bits = (1..n).fold(0u32, |acc, i| acc | f.int(i) as u32);
                if let Some(p) = self.current(f.int(0)) {
                    p.flags |= bits;
                }
            }
            // Linked NPC (0x0206cae8).
            12 => {
                let other = f.int(1) as u8;
                if let Some(p) = self.current(f.int(0)) {
                    p.linked_npc = other;
                }
            }
            // Speed (0x0206cb58).
            13 => {
                let v = fx32(f.float(1));
                if let Some(p) = self.current(f.int(0)) {
                    p.speed = v;
                }
            }
            // Quest (0x0206cbdc): [quest, condition, time, map, npc, x…].
            14 => {
                self.counter = self.counter.wrapping_add(1);
                let (quest, cond, time, map, npc) =
                    (f.int(0), f.int(1), f.int(2), f.int(3), f.int(4));
                if i32::from(self.last_npc) != npc {
                    self.last_npc = npc as i16;
                    self.quest14 = -1;
                    self.quest21 = -1;
                }
                if self.quest21 > 0 || self.superseded(i32::from(self.quest14), quest) {
                    return;
                }
                if !story.quest_condition(quest, cond) || !story.time_matches(time) {
                    return;
                }
                if map != i32::from(self.map) {
                    self.remove(npc);
                    return;
                }
                let mut node = Placement::new(npc as u8, map as u16);
                node.quest = quest as i16;
                node.time = time as u8 & 3;
                if n < 6 {
                    self.remove(npc);
                } else {
                    self.place(node, f, 5, n);
                }
                self.quest14 = quest as i16;
            }
            // Seated (0x0206d3e4): [npc, angle].
            15 => {
                let v = fx32(f.float(1));
                if let Some(p) = self.current(f.int(0)) {
                    p.seat_angle = v;
                    p.flags |= 0x800;
                }
            }
            // Object s069 (0x0206d474): [npc, x, y, z].
            16 => {
                let v = [fx32(f.float(1)), fx32(f.float(2)), fx32(f.float(3))];
                if let Some(p) = self.current(f.int(0)) {
                    p.flags |= 0x2000;
                    p.prop = v;
                }
            }
            // Flags condition (0x0206d4f0): [(flag, set)…, time, map, npc, x…].
            17 => self.by_flags(f, n),
            // State (0x0206d744): [npc, value, (float)].
            18 => {
                let state = f.int(1) as u8 & 7;
                let value = (n > 2).then(|| fx32(f.float(2)));
                if let Some(p) = self.current(f.int(0)) {
                    p.state = state;
                    if let Some(v) = value {
                        p.state_value = v;
                    }
                }
            }
            // Model override (0x0206d7f4): [npc, archive, motion]. A null archive makes the
            // game copy from address 0; it then fails to load, as `None` does here.
            19 => {
                let (chr, motion) = (f.str(1), f.str(2));
                if let Some(p) = self.current(f.int(0)) {
                    p.chr = chr;
                    p.motion_name = motion;
                }
            }
            // (0x0206d8f8): [npc, f1, f2, f3].
            20 => {
                let v = [fx32(f.float(2)), 0, fx32(f.float(3)), fx32(f.float(1))];
                if let Some(p) = self.current(f.int(0)) {
                    p.extra = Some(v);
                }
            }
            // Quest and span (0x0206cf68): [quest, cond, from ×3, to ×3, time, map, npc, x…].
            21 => self.by_quest_span(f, n),
            // Tint (0x0206d9e8): [npc, r, g, b].
            22 => {
                let colour = (f.int(1) as u16) | (f.int(2) as u16) << 5 | (f.int(3) as u16) << 10;
                if let Some(p) = self.current(f.int(0)) {
                    p.tint = Some(colour);
                }
            }
            _ => {}
        }
    }

    /// Records 14 and 21 keep the highest-numbered completed quest: a completed `quest` is
    /// skipped after a completed `last` above it.
    fn superseded(&self, last: i32, quest: i32) -> bool {
        last != quest
            && last >= 0
            && self.story.quest_state(quest) == 3
            && self.story.quest_state(last) == 3
            && quest < last
    }

    fn by_flags(&mut self, f: &Fields<'a>, n: usize) {
        if n < 9 {
            return;
        }
        self.counter = self.counter.wrapping_add(1);
        let even = n.is_multiple_of(2);
        let pairs = (n - if even { 8 } else { 7 }) / 2;
        for k in 0..pairs {
            let (flag, set) = (f.int(2 * k), f.int(2 * k + 1));
            let held = match flag >> 16 {
                1 => self.story.flag(flag & 0xffff, false),
                2 => self.story.flag(flag & 0xffff, true),
                _ => return,
            };
            if held != (set != 0) {
                return;
            }
        }
        let at = 2 * pairs;
        let time = f.int(at);
        if !self.story.time_matches(time) {
            return;
        }
        let (map, npc) = (f.int(at + 1), f.int(at + 2));
        if map != i32::from(self.map) {
            self.remove(npc);
            return;
        }
        let mut node = Placement::new(npc as u8, map as u16);
        node.by_flags = true;
        // The time word is not stored by this record.
        if n - 2 * pairs - usize::from(even) < 4 {
            self.remove(npc);
            return;
        }
        let pos = at + 3;
        node.position = [
            fx32(f.float(pos)),
            fx32(f.float(pos + 1)),
            fx32(f.float(pos + 2)),
        ];
        node.angle = fx32(f.float(pos + 3)) as i16;
        if even {
            node.motion = f.int(pos + 4) as u8;
        }
        node.order = self.counter;
        self.insert(node);
    }

    fn by_quest_span(&mut self, f: &Fields<'a>, n: usize) {
        self.counter = self.counter.wrapping_add(1);
        let (quest, cond) = (f.int(0), f.int(1));
        let from = [f.int(2), f.int(3), f.int(4)];
        let to = [f.int(5), f.int(6), f.int(7)];
        let (time, map, npc) = (f.int(8), f.int(9), f.int(10));
        if i32::from(self.last_npc) != npc {
            self.last_npc = npc as i16;
            self.quest14 = -1;
            self.quest21 = -1;
        }
        if self.superseded(i32::from(self.quest21), quest) {
            return;
        }
        if !self.story.quest_condition(quest, cond) {
            return;
        }
        let weight = |v: [i32; 3]| v[2] + v[0] * 1000 + v[1] * 10;
        let now = self.story.now();
        if !(weight(from) <= now && now <= weight(to)) || !self.story.time_matches(time) {
            return;
        }
        if map != i32::from(self.map) {
            self.remove(npc);
            return;
        }
        let mut node = Placement::new(npc as u8, map as u16);
        node.quest = quest as i16;
        node.from = from.map(|v| v as u8);
        node.to = to.map(|v| v as u8);
        node.time = time as u8 & 3;
        if n < 12 {
            self.remove(npc);
        } else {
            node.by_quest_span = true;
            node.position = [fx32(f.float(11)), fx32(f.float(12)), fx32(f.float(13))];
            node.angle = fx32(f.float(14)) as i16;
            if n > 15 {
                node.motion = f.int(15) as u8;
            }
            node.order = self.counter;
            self.insert(node);
        }
        self.quest21 = quest as i16;
    }
}

/// Runs one `place.bin` (`0x0206da90`): the chains of the NPCs placed on `map`.
pub fn run_placements<'a>(
    table: &Table<'a>,
    map: u16,
    story: &Story,
) -> (BTreeMap<u8, Vec<Placement<'a>>>, bool) {
    let mut run = Run {
        story,
        map,
        counter: 0,
        last_npc: -1,
        quest14: -1,
        quest21: -1,
        chains: BTreeMap::new(),
    };
    let mut fields = Fields::default();
    for record in &table.records {
        fields.load(record);
        run.record(record.kind, &fields);
    }
    (run.chains, fields.read_unset())
}

/// Map ids whose NPCs 1 to 63 are visitors placed by other code (`0x020981f4`, `0x02098278`,
/// used by `0x02064d58`): the npc.bin records skip them.
pub fn is_visitor(map: u16, npc: i32) -> bool {
    (50101..=50523).contains(&map) && (1..=0x3f).contains(&npc)
}

/// Runs one `npc.bin` (`0x02064e3c`) against the placed NPCs, prepending to `members`.
pub fn read_members<'a>(
    table: &Table<'a>,
    map: u16,
    chains: &BTreeMap<u8, Vec<Placement<'a>>>,
    members: &mut Vec<CastMember<'a>>,
) {
    let mut fields = Fields::default();
    for record in table.records.iter().filter(|r| r.kind == 3) {
        fields.load(record);
        let id = fields.int(0);
        if is_visitor(map, id) || !u8::try_from(id).is_ok_and(|id| chains.contains_key(&id)) {
            continue;
        }
        members.insert(
            0,
            CastMember {
                id: id as u16,
                kind: fields.int(1) as u8,
                japanese_name: fields.str(2),
                name: fields.str(3),
                byte3: fields.int(4) as u8,
            },
        );
    }
}

/// The cast of `map` at `story` from its area's `.npc` archive, as `ov_017::021a2c14` loads
/// it: every member named `*place.bin` runs on a fresh list (the last one counts), then every
/// `*npc.bin` adds the placed NPCs. Visitors of `R0*` maps (`0x02064e98`) are not modelled.
pub fn load<'a>(npc_archive: &'a [u8], map: u16, story: &Story) -> Result<Cast<'a>> {
    let narc = Narc::parse(npc_archive)?;
    let names: Vec<(u16, String)> = (0..narc.len())
        .filter_map(|i| u16::try_from(i).ok())
        .map(|i| (i, narc.name(i).unwrap_or_default().to_string()))
        .collect();
    let (mut chains, mut read_unset) = (BTreeMap::new(), false);
    for (i, _) in names.iter().filter(|(_, n)| n.contains("place.bin")) {
        let table = Table::parse(narc.file(*i)?)?;
        let (c, unset) = run_placements(&table, map, story);
        chains = c;
        read_unset |= unset;
    }
    let mut members = Vec::new();
    for (i, _) in names.iter().filter(|(_, n)| n.contains("npc.bin")) {
        let table = Table::parse(narc.file(*i)?)?;
        read_members(&table, map, &chains, &mut members);
    }
    let npcs = members
        .into_iter()
        .filter_map(|member| {
            let chain = chains.get(&(member.id as u8))?;
            Some(Npc {
                member,
                placement: chain.first()?.clone(),
            })
        })
        .collect();
    Ok(Cast { npcs, read_unset })
}

/// The motion a model NPC starts with (`0x0203d038`), as `(package, motion index)`: package
/// 0 is its own archive, 1 the archive of record 19 (`data/<chr>`, appended after it).
/// First `Object3D::MaybeSetBCFGAnimation` (`0x020370b0`) with the placement's motion byte
/// on its own table, then, when record 19 names a motion,
/// `Object3D::MaybeSetRegularAnimation` (`0x02036e44`): the first package whose table has the
/// name with animation files. `None` without a model or a valid motion.
pub fn initial_motion(
    placement: &Placement<'_>,
    own: &Chr<'_>,
    extra: Option<&Chr<'_>>,
) -> Option<(usize, usize)> {
    own.model?;
    let mut current = own
        .motion_table
        .as_ref()
        .filter(|(_, t)| usize::from(placement.motion) < t.motions.len())
        .map(|_| (0, usize::from(placement.motion)));
    if let Some(name) = placement.motion_name.filter(|n| !n.is_empty()) {
        let found = [Some(own), extra]
            .into_iter()
            .enumerate()
            .filter_map(|(k, chr)| Some((k, chr?)))
            .find_map(|(k, chr)| {
                let i = chr.motion_index(name)?;
                (!chr.motions().get(i)?.files.is_empty()).then_some((k, i))
            });
        if found.is_some() {
            current = found;
        }
    }
    current
}

/// Name of the stand animation a sprite NPC shows (`0x0203cbe8`, default standing case:
/// not seated, no special mode): eight sectors of the NPC's angle minus the camera's
/// (`fx32` radians), table `0x020e7948` into the names at `0x020efd00`.
pub fn sprite_stand(npc_angle: i32, camera_angle: i32) -> &'static str {
    const TURN: i32 = 0x6488;
    const SECTOR: [&str; 8] = [
        "stand_down",
        "stand_r_down",
        "stand_right",
        "stand_r_up",
        "stand_up",
        "stand_l_up",
        "stand_left",
        "stand_l_down",
    ];
    let rel = (npc_angle.wrapping_sub(camera_angle)).rem_euclid(TURN);
    // FX_Div((rel + 2π) × 8 + π, 2π) >> 12 & 7.
    let num = i64::from((rel + TURN) * 8 + 0x3244);
    let q = (((num << 32) / i64::from(TURN)) + 0x80000) >> 20;
    SECTOR[(q >> 12 & 7) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Record, Value};

    use Value::{Float as F, Int as I};

    fn s(x: &str) -> Value<'_> {
        Value::Str(Some(x.as_bytes()))
    }

    fn table(records: Vec<(u16, Vec<Value<'static>>)>) -> Table<'static> {
        Table {
            records: records
                .into_iter()
                .map(|(kind, values)| Record { kind, values })
                .collect(),
            string_count: 0,
        }
    }

    fn pos(x: f32, y: f32, z: f32, a: f32) -> Vec<Value<'static>> {
        vec![F(x), F(y), F(z), F(a)]
    }

    fn with(head: Vec<Value<'static>>, tail: Vec<Value<'static>>) -> Vec<Value<'static>> {
        [head, tail].concat()
    }

    fn story(chapter: u8, step: u8, substep: u8, daytime: bool) -> Story {
        Story {
            chapter,
            step,
            substep,
            daytime,
            ..Story::default()
        }
    }

    #[test]
    fn spans_pick_the_story_point_and_other_maps_take_away() {
        let t = table(vec![
            (3, with(vec![I(1100), I(1)], pos(1.0, 0.0, 2.0, 1.5))),
            (
                5,
                with(
                    vec![I(1), I(2), I(1), I(1), I(2), I(2), I(2), I(1100), I(1)],
                    pos(3.0, 0.0, 4.0, 0.0),
                ),
            ),
            // At 1.3, NPC 1 is in another map: taken away from this one.
            (
                5,
                with(
                    vec![I(1), I(3), I(1), I(1), I(3), I(2), I(1), I(1106), I(1)],
                    pos(0.0, 0.0, 0.0, 0.0),
                ),
            ),
            (6, vec![I(1), F(1.0), F(2.0), F(-1.0), F(-2.0), I(80)]),
        ]);
        let (chains, unset) = run_placements(&t, 1100, &story(1, 1, 1, true));
        assert!(!unset);
        let head = &chains[&1][0];
        assert_eq!(
            (head.position, head.angle, head.class()),
            ([4096, 0, 8192], 6144, 5)
        );
        // The zone attaches to the last counted record's node: the 1.3 span, not made here.
        assert!(head.zones.is_empty());

        let (chains, _) = run_placements(&t, 1100, &story(1, 2, 2, true));
        let head = &chains[&1][0];
        assert_eq!(
            (head.position[0], head.class(), head.to),
            (12288, 4, [1, 2, 2])
        );
        // The block is still in the chain, behind the span.
        assert_eq!(chains[&1].len(), 2);

        // Night only: does not hold by day, the NPC stays.
        let (chains, _) = run_placements(&t, 1100, &story(1, 3, 1, true));
        assert_eq!(chains[&1][0].class(), 5);
        let (chains, _) = run_placements(&t, 1100, &story(1, 3, 1, false));
        assert!(!chains.contains_key(&1));
    }

    #[test]
    fn short_records_read_the_previous_fields() {
        let t = table(vec![
            (3, with(vec![I(1100), I(7)], pos(1.0, 0.0, 2.0, 0.0))),
            (6, vec![I(7), F(1.0), F(2.0), F(-1.0), F(-2.0), I(80)]),
            // No label: field 5 is still 80.
            (6, vec![I(7), F(5.0), F(6.0), F(4.0), F(3.0)]),
            // Fewer than 3 fields: taken away.
            (3, vec![I(1100), I(8)]),
        ]);
        let (chains, unset) = run_placements(&t, 1100, &Story::default());
        assert!(!unset);
        let zones = &chains[&7][0].zones;
        assert_eq!(zones[1].label, 80);
        assert_eq!(zones[1].rect, [20480, 24576, 16384, 12288]);
        assert!(!chains.contains_key(&8));
        // A first record reading past its fields reads what the game has as garbage.
        let t = table(vec![(6, vec![I(7)])]);
        assert!(run_placements(&t, 1100, &Story::default()).1);
    }

    #[test]
    fn classes_order_flags_quests_and_spans() {
        let mut st = story(2, 1, 1, true);
        st.flags.insert(0x13b);
        st.quests.insert(4, 3);
        st.quests.insert(9, 3);
        let t = table(vec![
            (3, with(vec![I(100), I(3)], pos(1.0, 0.0, 0.0, 0.0))),
            (
                5,
                with(
                    vec![I(1), I(1), I(1), I(9), I(9), I(9), I(2), I(100), I(3)],
                    pos(2.0, 0.0, 0.0, 0.0),
                ),
            ),
            (
                14,
                with(
                    vec![I(9), I(2), I(2), I(100), I(3)],
                    pos(3.0, 0.0, 0.0, 0.0),
                ),
            ),
            // Quest 4 completed after quest 9 for the same NPC: superseded.
            (
                14,
                with(
                    vec![I(4), I(2), I(2), I(100), I(3)],
                    pos(4.0, 0.0, 0.0, 0.0),
                ),
            ),
            (
                17,
                with(
                    vec![I(0x1013b), I(1), I(2), I(100), I(3)],
                    pos(5.0, 0.0, 0.0, 0.0),
                ),
            ),
            (11, vec![I(3), I(64), I(4096)]),
            (15, vec![I(3), F(1.0)]),
        ]);
        let (chains, _) = run_placements(&t, 100, &st);
        let chain = &chains[&3];
        let xs: Vec<i32> = chain.iter().map(|p| p.position[0] >> 12).collect();
        assert_eq!(xs, [5, 3, 2, 1]);
        assert_eq!(chain[0].flags, 64 | 4096 | 0x800);
        assert_eq!(chain[0].seat_angle, 4096);
        // Flag clear: the flag record does not count.
        st.flags.clear();
        let (chains, _) = run_placements(&t, 100, &st);
        assert_eq!(chains[&3][0].position[0] >> 12, 3);
    }

    #[test]
    fn attached_records() {
        let t = table(vec![
            (3, with(vec![I(1), I(2)], pos(1.0, 0.0, 1.0, 0.0))),
            (8, vec![I(2), F(0.0), F(1.0)]),
            (13, vec![I(2), F(0.05)]),
            (19, vec![I(2), s("event_lv5/x.chr"), s("st_lp")]),
            (22, vec![I(2), I(8), I(6), I(16)]),
            (3, with(vec![I(1), I(4)], pos(0.0, 0.0, 0.0, 0.0))),
            (
                10,
                vec![I(4), F(1.0), F(0.0), F(2.0), F(3.0), F(0.0), F(4.0)],
            ),
            (7, vec![I(9), F(1.0), F(1.0), F(0.0), F(0.0)]),
        ]);
        let (chains, _) = run_placements(&t, 1, &Story::default());
        let p = &chains[&2][0];
        // Segment along +z of length 1 around (1, 1).
        assert_eq!((p.mode, p.area), (8, [4096, 8192, 4096, 0]));
        assert_eq!(p.speed, 204);
        assert_eq!(
            (p.chr, p.motion_name),
            (Some(&b"event_lv5/x.chr"[..]), Some(&b"st_lp"[..]))
        );
        assert_eq!(p.tint, Some(8 | 6 << 5 | 16 << 10));
        let q = &chains[&4][0];
        assert_eq!((q.mode, q.path.len()), (10, 2));
        assert_eq!(q.path[1], [12288, 0, 16384]);
    }

    #[test]
    fn members_need_a_placement_and_come_last_first() {
        let place = table(vec![
            (3, with(vec![I(5), I(1)], pos(0.0, 0.0, 0.0, 0.0))),
            (3, with(vec![I(5), I(2)], pos(0.0, 0.0, 0.0, 0.0))),
        ]);
        let npc = table(vec![
            (3, vec![I(1), I(0), Value::Str(None), s("n003a"), I(0)]),
            (3, vec![I(3), I(2), Value::Str(None), s("s017"), I(0)]),
            (3, vec![I(2), I(2), Value::Str(None), s("s017"), I(2)]),
        ]);
        let (chains, _) = run_placements(&place, 5, &Story::default());
        let mut members = Vec::new();
        read_members(&npc, 5, &chains, &mut members);
        let ids: Vec<u16> = members.iter().map(|m| m.id).collect();
        assert_eq!(ids, [2, 1]);
        let npc = Npc {
            member: members[0],
            placement: chains[&2][0].clone(),
        };
        assert_eq!(
            npc.appearance(|_| false),
            Appearance::Model {
                chr: "data/chara_sub/s017.chr".into()
            }
        );
        let mut seated = Npc {
            member: members[1],
            placement: chains[&1][0].clone(),
        };
        seated.placement.flags |= 0x800;
        assert_eq!(
            seated.appearance(|p| p == "data/ani/n003a_s.spr"),
            Appearance::Sprite {
                sheet: "data/ani/n003a_s.spr".into()
            }
        );
        assert!(!seated.snapped_to_ground());
        assert!(is_visitor(50101, 1) && !is_visitor(50101, 64) && !is_visitor(1100, 1));
    }

    #[test]
    fn area_archives_and_sprite_directions() {
        assert_eq!(archive_path(b"M01"), "data/scenario/M01.npc");
        assert_eq!(archive_path(b"C01M09"), "data/scenario/C01.npc");
        assert_eq!(archive_path(b"F07M00"), "data/scenario/F.npc");
        // Facing the camera: front view; turned by π: back view.
        assert_eq!(sprite_stand(0, 0), "stand_down");
        assert_eq!(sprite_stand(0x3244, 0), "stand_up");
        assert_eq!(sprite_stand(0x1922, 0), "stand_right");
        assert_eq!(sprite_stand(-0x1922, 0), "stand_left");
    }
}
