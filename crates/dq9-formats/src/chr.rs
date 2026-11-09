//! Character archives (`.chr`, NARC): a model, its motion table (`.bcfg`) and the animation
//! files of each motion, as the game's two archive loaders pick them. Layout and evidence in
//! `docs/re/characters.md`.
//!
//! - [`MotionTable`] is `BCFG::LoadFromScript` (`0x0203462c`) and its opcodes.
//! - [`Chr::motions`] is what `Object3D::LoadFromCCHROrCMOTArchive` (`0x02036814`) finds for
//!   each motion (the loader of NPC models and monsters); [`Chr::slots`] is the single
//!   animation package of `Object3D::InternalLoadFromCHRArchive` (`0x0203644c`). The G3D
//!   decoding itself is `nds_rom::g3d`'s.
//!
//! port: 0x020343b0..=0x020349d0

use nds_rom::{Narc, Result};

use crate::table::{Fields, Table, fx32};

/// Animation file kinds, in the order `LoadFromCCHROrCMOTArchive` tries their extensions for
/// a motion (`object3DsData.extensionLookup`, decomp `src/World/Object3D.cpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AnimKind {
    /// `BCA0` joint animation.
    Joint,
    /// `BMA0` material animation.
    Material,
    /// `BTP0` texture pattern animation.
    Pattern,
    /// `BTA0` texture SRT animation.
    TextureSrt,
}

impl AnimKind {
    pub const ALL: [AnimKind; 4] = [
        AnimKind::Joint,
        AnimKind::Material,
        AnimKind::Pattern,
        AnimKind::TextureSrt,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            AnimKind::Joint => "nsbca",
            AnimKind::Material => "nsbma",
            AnimKind::Pattern => "nsbtp",
            AnimKind::TextureSrt => "nsbta",
        }
    }

    /// Slot of the type-0 package (`InternalLoadFromCHRArchive`): nsbca 0, nsbma 1,
    /// nsbtp 2, nsbta 3.
    pub fn slot(self) -> usize {
        match self {
            AnimKind::Joint => 0,
            AnimKind::Material => 1,
            AnimKind::Pattern => 2,
            AnimKind::TextureSrt => 3,
        }
    }
}

/// `BCFG::SizeCEntry`, opcode `0x6c` **[unknown]**: `(value, time)` attached to a motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionMark {
    /// Third field (`Tbl_FieldInt`), kept on 16 bits.
    pub value: u16,
    /// Second field × 4096, kept on 16 bits.
    pub time: i16,
}

/// `BCFG::AltSizeCEntry`, opcode `0x6f`: a cue at a time of a motion (a sound effect
/// according to the decomp's naming, **[hypothesis]**).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionCue {
    /// Third field, kept on 16 bits.
    pub sound: u16,
    /// Second field × 4096 (`fx16`).
    pub time: i16,
}

/// `BCFG::AnimationRecord`, opcode `0x66`: a named stretch of the model's animation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Motion<'a> {
    /// Name (the game copies it into 16 bytes with `strcpy`).
    pub name: &'a [u8],
    /// First and last frame × 4096 (`fx32`).
    pub start: i32,
    pub end: i32,
    /// Playback speed × 4096; the clock advances `speed × delta` per update.
    pub speed: i32,
    /// `0x6c` entries, last inserted first (the game prepends to a list).
    pub marks: Vec<MotionMark>,
    /// `0x6f` entries, last inserted first.
    pub cues: Vec<MotionCue>,
}

impl Motion<'_> {
    /// Name as text (motion names are ASCII).
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(self.name).into_owned()
    }

    /// One update of this motion in a per-motion package, playing forward and looping
    /// (`Object3D::AdvanceAnimations_v1`, `0x02035188`, default case): `time` is the frame
    /// (`fx32`) in its animation file of `frames` frames, `delta` the animation clock
    /// (`GameState::CalculateDeltaTime`, `0x02010160`: 4096 per 17 ms of game time) and
    /// `playback` the object's own speed (4096 unless set). Returns the new time, wrapped
    /// into `[0, frames - 1)`.
    pub fn advance(&self, time: i32, delta: i32, playback: i32, frames: u16) -> i32 {
        let mul = |a: i32, b: i32| ((i64::from(a) * i64::from(b) + 0x800) >> 12) as i32;
        let duration = (i32::from(frames) << 12) - 0x1000;
        let time = time.wrapping_add(mul(mul(self.speed, delta), playback));
        if duration <= time {
            time - duration
        } else {
            time
        }
    }
}

/// A `.bcfg` motion table as `BCFG::LoadFromScript` (`0x0203462c`) builds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MotionTable<'a> {
    pub motions: Vec<Motion<'a>>,
    /// Bone the motions are rooted at (opcode `0x71`), `waist` when the table sets none;
    /// `None` only for an empty file, which the game does not run.
    pub root_bone: Option<&'a [u8]>,
}

impl<'a> MotionTable<'a> {
    /// Runs a `.bcfg` record table. Capacities follow the game: motions beyond the `0x64`
    /// count, and `0x6c`/`0x6f` entries beyond their `0x6a`/`0x6d` counts or naming an
    /// unknown motion, are dropped. Records of other kinds (`0x65`, `0x67`-`0x69`, `0x6b`,
    /// `0x6e`, `0x70`) do nothing.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let mut table = Self::default();
        if data.is_empty() {
            return Ok(table);
        }
        let records = Table::parse(data)?;
        let mut fields = Fields::default();
        let (mut capacity, mut marks, mut cues) = (0usize, (0usize, 0usize), (0usize, 0usize));
        for record in &records.records {
            fields.load(record);
            match record.kind {
                // BCFGScript_Opcode_64, BCFG::AllocateAnimationEntries.
                0x64 => {
                    capacity = usize::try_from(fields.int(0)).unwrap_or(0);
                    table.motions.clear();
                }
                // BCFGScript_Opcode_66, BCFG::InsertAnimationRecord.
                0x66 => {
                    if let Some(name) = fields.str(0)
                        && table.motions.len() < capacity
                    {
                        table.motions.push(Motion {
                            name,
                            start: fx32(fields.float(1)),
                            end: fx32(fields.float(2)),
                            speed: fx32(fields.float(3)),
                            marks: Vec::new(),
                            cues: Vec::new(),
                        });
                    }
                }
                // BCFGScript_Opcode_6a / 6d, BCFG::Allocate*SizeCEntries.
                0x6a => marks = (usize::try_from(fields.int(0)).unwrap_or(0), 0),
                0x6d => cues = (usize::try_from(fields.int(0)).unwrap_or(0), 0),
                // BCFGScript_Opcode_6c, BCFG::InsertSizeCEntry.
                0x6c => {
                    let index = table.search(fields.str(0));
                    let mark = MotionMark {
                        value: fields.int(2) as u16,
                        time: fx32(fields.float(1)) as i16,
                    };
                    if marks.1 < marks.0
                        && let Some(m) = index.and_then(|i| table.motions.get_mut(i))
                    {
                        m.marks.insert(0, mark);
                        marks.1 += 1;
                    }
                }
                // BCFGScript_Opcode_6f, BCFG::InsertAltSizeCEntry.
                0x6f => {
                    let index = table.search(fields.str(0));
                    let cue = MotionCue {
                        sound: fields.int(2) as u16,
                        time: fx32(fields.float(1)) as i16,
                    };
                    if cues.1 < cues.0
                        && let Some(m) = index.and_then(|i| table.motions.get_mut(i))
                    {
                        m.cues.insert(0, cue);
                        cues.1 += 1;
                    }
                }
                // BCFGScript_Opcode_71, BCFG::MaybeSetRootBoneName.
                0x71 => table.root_bone = fields.str(0),
                _ => {}
            }
        }
        table.root_bone.get_or_insert(b"waist");
        Ok(table)
    }

    /// `BCFG::SearchAnimationByName` (`0x02034778`): first motion with this name.
    pub fn search(&self, name: Option<&[u8]>) -> Option<usize> {
        let name = name?;
        self.motions.iter().position(|m| m.name == name)
    }
}

/// Role of a member of a `.chr` archive, from its name as the loaders test it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role<'n> {
    /// `.nsbmd`: the model (only the first counts).
    Model,
    /// `.bcfg`: the motion table (only the first counts for motions).
    MotionTable,
    /// `<motion>.<ext>`: an animation file of a motion.
    Animation { motion: &'n str, kind: AnimKind },
    /// Anything else (`.beff`, `.bact` effect and action scripts, read by other code).
    Other,
}

/// Role of a member name: `strstr` on the extensions, as both loaders do.
pub fn role(name: &str) -> Role<'_> {
    if name.contains(".nsbmd") {
        return Role::Model;
    }
    if name.contains(".bcfg") {
        return Role::MotionTable;
    }
    for kind in AnimKind::ALL {
        if let Some(motion) = name.strip_suffix(kind.extension())
            && let Some(motion) = motion.strip_suffix('.')
        {
            return Role::Animation { motion, kind };
        }
    }
    Role::Other
}

/// The animation files of one motion, found by exact member name `<motion>.<ext>` in
/// [`AnimKind::ALL`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionFiles<'a> {
    pub motion: Motion<'a>,
    /// `(kind, member index)` in the order the loader attaches them.
    pub files: Vec<(AnimKind, u16)>,
}

/// A parsed `.chr` archive.
#[derive(Debug, Clone)]
pub struct Chr<'a> {
    pub narc: Narc<'a>,
    /// First member whose name contains `.nsbmd`.
    pub model: Option<u16>,
    /// First member whose name contains `.bcfg`, and its table.
    pub motion_table: Option<(u16, MotionTable<'a>)>,
}

impl<'a> Chr<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let narc = Narc::parse(data)?;
        let first = |want: fn(&Role) -> bool| {
            (0..narc.len())
                .filter_map(|i| u16::try_from(i).ok())
                .find(|&i| narc.name(i).is_some_and(|n| want(&role(n))))
        };
        let model = first(|r| *r == Role::Model);
        let motion_table = match first(|r| *r == Role::MotionTable) {
            Some(i) => Some((i, MotionTable::parse(narc.file(i)?)?)),
            None => None,
        };
        Ok(Self {
            narc,
            model,
            motion_table,
        })
    }

    /// Bytes of the model member.
    pub fn model_data(&self) -> Result<Option<&'a [u8]>> {
        self.model.map(|i| self.narc.file(i)).transpose()
    }

    fn member(&self, name: &str) -> Option<u16> {
        (0..self.narc.len())
            .filter_map(|i| u16::try_from(i).ok())
            .find(|&i| self.narc.name(i) == Some(name))
    }

    /// Each motion of the table with its animation files, as `LoadFromCCHROrCMOTArchive`
    /// (`0x02036814`) attaches them; a motion without files plays nothing.
    pub fn motions(&self) -> Vec<MotionFiles<'a>> {
        let Some((_, table)) = &self.motion_table else {
            return Vec::new();
        };
        table
            .motions
            .iter()
            .map(|motion| {
                let name = String::from_utf8_lossy(motion.name);
                let files = AnimKind::ALL
                    .into_iter()
                    .filter_map(|kind| {
                        self.member(&format!("{name}.{}", kind.extension()))
                            .map(|i| (kind, i))
                    })
                    .collect();
                MotionFiles {
                    motion: motion.clone(),
                    files,
                }
            })
            .collect()
    }

    /// The four slots of the type-0 package of `InternalLoadFromCHRArchive` (`0x0203644c`):
    /// every member whose name contains an animation extension fills its kind's slot, the
    /// last one winning (extensions tested in the order nsbca, nsbma, nsbta, nsbtp).
    pub fn slots(&self) -> [Option<u16>; 4] {
        let mut slots = [None; 4];
        for i in (0..self.narc.len()).filter_map(|i| u16::try_from(i).ok()) {
            let Some(name) = self.narc.name(i) else {
                continue;
            };
            let kind = [
                AnimKind::Joint,
                AnimKind::Material,
                AnimKind::TextureSrt,
                AnimKind::Pattern,
            ]
            .into_iter()
            .find(|k| name.contains(&format!(".{}", k.extension())));
            if let Some(kind) = kind {
                slots[kind.slot()] = Some(i);
            }
        }
        slots
    }

    /// Index of motion `name` in the table.
    pub fn motion_index(&self, name: &[u8]) -> Option<usize> {
        self.motion_table.as_ref()?.1.search(Some(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Record, Value};
    use crate::test_util::{encode_table, narc};

    fn rec<'a>(kind: u16, values: Vec<Value<'a>>) -> Record<'a> {
        Record { kind, values }
    }

    fn s(x: &str) -> Value<'_> {
        Value::Str(Some(x.as_bytes()))
    }

    #[test]
    fn motion_table_follows_the_game_capacities() {
        use Value::{Float as F, Int as I};
        let data = encode_table(&[
            rec(0x64, vec![I(2)]),
            rec(0x66, vec![s("stand"), F(0.0), F(8.0), F(0.1)]),
            rec(0x65, vec![]),
            rec(0x66, vec![s("walk"), I(0), F(12.5), F(0.25)]),
            // Past the count of 2: dropped.
            rec(0x66, vec![s("run"), F(0.0), F(4.0), F(1.0)]),
            rec(0x6d, vec![I(2)]),
            rec(0x6f, vec![s("walk"), F(1.5), I(7)]),
            rec(0x6f, vec![s("walk"), F(3.0), I(8)]),
            // Over capacity, then an unknown motion.
            rec(0x6f, vec![s("walk"), F(4.0), I(9)]),
            // No 0x6a: no room for marks.
            rec(0x6c, vec![s("stand"), F(1.0), I(1)]),
            rec(0x70, vec![I(u32::MAX)]),
        ]);
        let t = MotionTable::parse(&data).unwrap();
        assert_eq!(t.motions.len(), 2);
        let walk = &t.motions[1];
        assert_eq!((walk.start, walk.end, walk.speed), (0, 51200, 1024));
        assert_eq!(t.motions[0].speed, 409, "0.1 × 4096 truncated");
        // Cues are prepended.
        assert_eq!(
            walk.cues,
            [
                MotionCue {
                    sound: 8,
                    time: 12288
                },
                MotionCue {
                    sound: 7,
                    time: 6144
                }
            ]
        );
        assert!(t.motions[0].marks.is_empty());
        assert_eq!(t.root_bone, Some(&b"waist"[..]));
        assert_eq!(t.search(Some(b"walk")), Some(1));
        assert_eq!(t.search(Some(b"run")), None);
    }

    #[test]
    fn root_bone_and_marks() {
        use Value::{Float as F, Int as I};
        let data = encode_table(&[
            rec(0x64, vec![I(1)]),
            rec(0x66, vec![s("attack1"), F(0.0), F(20.0), F(0.25)]),
            rec(0x6a, vec![I(1)]),
            rec(0x6c, vec![s("attack1"), F(10.0), I(3)]),
            rec(0x71, vec![s("chest")]),
        ]);
        let t = MotionTable::parse(&data).unwrap();
        // 10.0 × 4096 = 40960, kept on 16 bits.
        assert_eq!(
            t.motions[0].marks,
            [MotionMark {
                value: 3,
                time: -24576
            }]
        );
        assert_eq!(t.root_bone, Some(&b"chest"[..]));
        assert_eq!(MotionTable::parse(&[]).unwrap().root_bone, None);
    }

    #[test]
    fn motions_loop_over_their_file() {
        let m = Motion {
            name: b"walk",
            start: 0,
            end: 0,
            speed: 1024,
            marks: Vec::new(),
            cues: Vec::new(),
        };
        // 9 frames: time runs in [0, 8); one 17 ms tick at speed 0.25 is a quarter frame.
        assert_eq!(m.advance(0, 4096, 4096, 9), 1024);
        assert_eq!(m.advance(32000, 4096, 4096, 9), 256);
    }

    #[test]
    fn roles_and_motion_files() {
        use Value::{Float as F, Int as I};
        assert_eq!(role("s017.nsbmd"), Role::Model);
        assert_eq!(role("s017.bcfg"), Role::MotionTable);
        assert_eq!(
            role("walk.nsbca"),
            Role::Animation {
                motion: "walk",
                kind: AnimKind::Joint
            }
        );
        assert_eq!(role("a.beff"), Role::Other);
        let table = encode_table(&[
            rec(0x64, vec![I(2)]),
            rec(0x66, vec![s("0"), F(0.0), F(10.0), F(1.0)]),
            rec(0x66, vec![s("talk"), F(0.0), F(10.0), F(1.0)]),
        ]);
        let data = narc(&[
            ("0.nsbca", vec![1]),
            ("0.nsbma", vec![2]),
            ("x.bcfg", table),
            ("x.nsbmd", vec![3]),
            ("other.nsbca", vec![4]),
        ]);
        let chr = Chr::parse(&data).unwrap();
        assert_eq!(chr.model, Some(3));
        assert_eq!(chr.model_data().unwrap(), Some(&[3u8][..]));
        let motions = chr.motions();
        assert_eq!(
            motions[0].files,
            [(AnimKind::Joint, 0), (AnimKind::Material, 1)]
        );
        assert!(motions[1].files.is_empty());
        assert_eq!(chr.motion_index(b"talk"), Some(1));
        // Type 0: the last nsbca wins its slot.
        assert_eq!(chr.slots(), [Some(4), Some(1), None, None]);
    }
}
