//! Battle action scripts (`.bact`): the motion an action plays, the lunge that carries the
//! fighter in, and the point of the motion at which the blow lands (wiki
//! *Battle-Action-Scripts*, "How a script is read", "A blow"; `docs/re/battle-data.md`).
//!
//! A script is a [record table](dq9_formats::table) the game runs once: a record's tag is an
//! opcode and its values its parameters. This module reads only what the presentation needs —
//! sections keyed by action number, `3` (a motion), `5` (a lunge), `26` (a point of a motion),
//! `9`/`10` (a skip) and `47` (a fade) — and ignores the rest. The game plays a script to the
//! end; here a [`Script::plan`] returns the handful of facts the scene uses, so nothing is
//! interpreted per frame.
//!
//! Where a script lives (EU layout, read on the ROM):
//!
//! - `data/pack_lv5/enemy.gp2/<code>.mon/<code>.bact`: a monster's own actions;
//! - `data/pack_lv5/chara_mp.gp2/mp0200b.chr/mp0200.bact`: the hero's, whatever weapon he
//!   carries (the weapon chooses the motions, not the script);
//! - `data/bin/actdef.nsarc/default.bact`: defend, flee, a call for help, the fallbacks;
//! - `data/skill/act{spl,skl,etc}.nsarc/sp%03d.bact`: spells, skills and the other actions
//!   with a script of their own.
//!
//! The ROM's battle models carry no attack motion in the field body (`*_f.mon`), and a battle
//! body's `.cchr` holds only `appear`, `attack0a`, `run` and `stand`: the rest —
//! `attack1a`, `damage`, `death`, `call`, `escape`, `sake` — is the `.cmot` beside it.

use std::collections::HashMap;

use anyhow::{Context as _, Result};
use dq9_formats::Gpc2;
use dq9_formats::chr::Chr;
use dq9_formats::table::{Table, Value};
use glam::Vec3;
use nds_rom::{Narc, Rom};

/// The number of 60 Hz ticks a motion without a usable table is given: the hero's `attack1a`
/// is 72, the slime's 58% point 407 ms (**[hypothesis]**, only used when the motion's own
/// speed and frames are missing).
pub const FALLBACK_MOTION_TICKS: u32 = 40;

/// Ticks past a motion's end that its last pose is held before the fighter stands again
/// (**[hypothesis]**: the game's own state machine runs the next command at once).
const HOLD: u32 = 8;

/// One parameter of a script record, kept owned (a script is read once, then the bytes are not
/// needed).
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Int(i32),
    Float(f32),
    Str(String),
}

impl Val {
    fn int(&self) -> i32 {
        match self {
            Val::Int(v) => *v,
            Val::Float(f) => *f as i32,
            Val::Str(_) => 0,
        }
    }

    fn float(&self) -> f32 {
        match self {
            Val::Int(v) => *v as f32,
            Val::Float(f) => *f,
            Val::Str(_) => 0.0,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A fraction written either as a float (`0.61`) or in thousandths (`61`, `407`).
    fn fraction(&self) -> f32 {
        let value = self.float();
        if value > 1.5 { value / 1000.0 } else { value }
    }
}

/// One script record: its opcode and values.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    tag: u16,
    values: Vec<Val>,
}

/// A `.bact` file: its sections, each keyed by the action numbers it serves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Script {
    sections: Vec<(Vec<u16>, Vec<Op>)>,
}

impl Script {
    /// Reads the records of a script, grouping them under the last `1` or `16` that came
    /// before them (`0x02169bf0`, `0x0216a438`: a key of 1 brings 2 and 219 with it).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let table = Table::parse(bytes).context("parsing the action script")?;
        let mut script = Self::default();
        let mut current: Option<usize> = None;
        for record in &table.records {
            let keyed = match record.kind {
                1 => record.values.iter().map(|v| v.int() as u16).collect(),
                16 => vec![1, 2, 219],
                _ => {
                    if let Some(index) = current {
                        script.sections[index].1.push(op(record));
                    }
                    continue;
                }
            };
            // `1` with no value opens a section whose keys are not read: the spell archives'
            // first section plays whatever its keys (`sp000`-style scripts).
            script.sections.push((keyed, Vec::new()));
            current = Some(script.sections.len() - 1);
        }
        Ok(script)
    }

    /// The section keyed by `action`.
    pub fn section(&self, action: u16) -> Option<&[Op]> {
        self.sections
            .iter()
            .find(|(keys, _)| keys.contains(&action))
            .map(|(_, ops)| ops.as_slice())
    }

    /// The first section, whatever its keys (what a spell's own file plays).
    pub fn first(&self) -> Option<&[Op]> {
        self.sections.first().map(|(_, ops)| ops.as_slice())
    }
}

fn op(record: &dq9_formats::table::Record<'_>) -> Op {
    Op {
        tag: record.kind,
        values: record
            .values
            .iter()
            .map(|value| match value {
                Value::Int(v) => Val::Int(*v as i32),
                Value::Float(f) => Val::Float(*f),
                Value::Str(Some(s)) => Val::Str(String::from_utf8_lossy(s).into_owned()),
                Value::Str(None) => Val::Str(String::new()),
            })
            .collect(),
    }
}

/// The lunge of a blow: `5 from to gap`, the fractions of the motion it runs over and the gap
/// it leaves between the two bodies (`0x021e2ca4`, linear, no easing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lunge {
    pub from: f32,
    pub to: f32,
    pub gap: f32,
}

/// What one action's script asks of the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Blow {
    /// The motion the actor plays (`attack1a`, `magic`, `guard`, `call`, `escape`…).
    pub motion: String,
    /// The lunge that carries it in, when the script has one.
    pub lunge: Option<Lunge>,
    /// Where in the motion the blow lands, 0 to 1 (`26`), 1 when it waits for the end.
    pub hit: f32,
    /// The script fades the fighter out at the end (`47`), the flight's `escape`.
    pub fades: bool,
}

/// Runs a section far enough to know what the presentation does.
///
/// Only the skips this needs are evaluated: `0` (always, a goto) and the gap comparisons `1`
/// to `4`. A result-dependent skip (`7`, `8`, `10`, `11`, `100`…) is taken as not holding:
/// the scripts put those after the blow. Sections without a motion, or whose motion is a
/// camera's (`3 25`), have no plan.
pub fn plan(ops: &[Op], gap: f32) -> Option<Blow> {
    let mut motion: Option<String> = None;
    let mut lunge = None;
    let mut hit = None;
    let mut fades = false;
    let mut skip: Option<i32> = None;
    for op in ops {
        if let Some(id) = skip {
            // While a skip is open only its `10` plays; `11` included in what is passed over.
            // A reaction block is where the landing is, even when a result-dependent goto
            // jumps over it (the scene does not evaluate results).
            if op.tag == 61 {
                break;
            }
            if op.tag == 10 && op.values.first().is_none_or(|v| v.int() == id) {
                skip = None;
            }
            continue;
        }
        match op.tag {
            // A motion at the one acting (`7`), not a camera's (`25`). The flags, effect and
            // speed follow the name and are not read here.
            3 => {
                let who = op.values.first().map_or(7, Val::int);
                if who == 7
                    && let Some(name) = op.values.get(1).and_then(Val::text)
                {
                    motion = Some(name.to_owned());
                }
            }
            5 => {
                lunge = Some(Lunge {
                    from: op.values.first().map_or(0.0, Val::fraction),
                    to: op.values.get(1).map_or(1.0, Val::fraction),
                    gap: op.values.get(2).map_or(0.0, Val::float),
                });
            }
            // A point of the one acting's motion; no point means "to its end".
            26 => {
                if op.values.first().map_or(7, Val::int) == 7 {
                    hit = Some(op.values.get(1).map_or(1.0, Val::fraction));
                }
            }
            9 => {
                let id = op.values.first().map_or(0, Val::int);
                let kind = op.values.get(1).map_or(0, Val::int);
                let value = op.values.get(2).map_or(0.0, Val::float);
                let holds = match kind {
                    0 => true,
                    1 => gap < value,
                    2 => gap > value,
                    3 => gap <= value,
                    4 => gap >= value,
                    _ => false,
                };
                if holds {
                    skip = Some(id);
                }
            }
            11 => break,
            47 => fades = true,
            _ => {}
        }
    }
    Some(Blow {
        motion: motion?,
        lunge,
        hit: hit.unwrap_or(1.0).clamp(0.0, 1.0),
        fades,
    })
}

/// A motion's length in 60 Hz ticks: its frames over its speed, as the animation clock
/// advances (`MotionTable` `0x66` stores frames and speed ×4096).
pub fn motion_ticks(start: i32, end: i32, speed: i32) -> u32 {
    if speed <= 0 {
        return FALLBACK_MOTION_TICKS;
    }
    u32::try_from((i64::from(end) - i64::from(start)).max(0) / i64::from(speed))
        .unwrap_or(FALLBACK_MOTION_TICKS)
        .max(1)
}

/// The motions of an archive and their lengths, by name; a later package shadows an earlier
/// one, as [`AnimatedScene::play_actor`](dq9_render::AnimatedScene::play_actor) does.
pub fn motion_spans(archives: &[&[u8]]) -> Vec<(String, u32)> {
    let mut spans: Vec<(String, u32)> = Vec::new();
    for bytes in archives {
        let Ok(chr) = Chr::parse(bytes) else {
            continue;
        };
        for found in chr.motions() {
            if found.files.is_empty() {
                continue;
            }
            let motion = &found.motion;
            let span = (
                String::from_utf8_lossy(motion.name).into_owned(),
                motion_ticks(motion.start, motion.end, motion.speed),
            );
            match spans.iter_mut().find(|(name, _)| *name == span.0) {
                Some(existing) => existing.1 = span.1,
                None => spans.push(span),
            }
        }
    }
    spans
}

/// The length of a named motion, when the archive has it.
pub fn span_of(spans: &[(String, u32)], name: &str) -> Option<u32> {
    spans
        .iter()
        .find(|(motion, _)| motion == name)
        .map(|(_, ticks)| *ticks)
}

/// What a timeline does when its motion ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// Back to the fighter's place and idle (a blow).
    Restore,
    /// Idle where it stands (a reaction).
    Idle,
    /// Hidden once (a death or a flight).
    Hide,
}

/// One fighter's presentation over time, in 60 Hz ticks.
#[derive(Debug, Clone)]
pub struct Timeline {
    /// The motion name, for the report and the tests.
    pub motion: String,
    pub end: End,
    /// Where the fighter stood before the motion.
    pub base: Vec3,
    /// The lunge, a displacement along `direction` from `base`, if any.
    pub direction: Vec3,
    pub lunge: Option<Lunge>,
    pub length: f32,
    /// The facing the actor takes, the target's direction (the lunge turns it in,
    /// `0x021e2ca4`); otherwise the heading it had.
    pub face: f32,
    /// Scene indices of the fighters this motion makes react at its hit.
    pub damaged: Vec<usize>,
    pub ticks: u32,
    pub age: u32,
    pub hit: u32,
    pub reacted: bool,
}

impl Timeline {
    /// A blow of `blow` lasting `ticks` from `base` towards `target`.
    ///
    /// The lunge ends `gap` plus the two bodies' radii from the target's middle: the script's
    /// gap is edge to edge, and a body's radius is its `mon_data` radius (the width the scene
    /// stores is four radii). The hit is at its point of the motion; a blow that cannot step
    /// in keeps its place.
    pub fn blow(
        blow: &Blow,
        ticks: u32,
        base: Vec3,
        heading: f32,
        radius: f32,
        target: Vec3,
        target_radius: f32,
    ) -> Self {
        let offset = target - base;
        let flat = Vec3::new(offset.x, 0.0, offset.z);
        let distance = flat.length();
        let direction = if distance > 0.0 {
            flat / distance
        } else {
            Vec3::ZERO
        };
        let standoff = blow
            .lunge
            .map_or(0.0, |lunge| lunge.gap + radius + target_radius);
        let length = (distance - standoff).max(0.0);
        let face = if distance > 0.0 {
            direction.x.atan2(direction.z)
        } else {
            heading
        };
        Self {
            motion: blow.motion.clone(),
            end: if blow.fades { End::Hide } else { End::Restore },
            base,
            direction,
            lunge: blow.lunge,
            length,
            face,
            damaged: Vec::new(),
            ticks: ticks.max(1) + HOLD,
            age: 0,
            hit: (blow.hit * ticks.max(1) as f32).round() as u32,
            reacted: false,
        }
    }

    /// A reaction (`damage`) or a death (`death`) of `ticks` at `base`.
    pub fn reaction(motion: String, end: End, ticks: u32, base: Vec3, heading: f32) -> Self {
        Self {
            motion,
            end,
            base,
            direction: Vec3::ZERO,
            lunge: None,
            length: 0.0,
            face: heading,
            damaged: Vec::new(),
            ticks: ticks.max(1),
            age: 0,
            hit: u32::MAX,
            reacted: true,
        }
    }

    /// Where the fighter is this tick: held, eased along the lunge (linear, as
    /// `0x021eedb0`), then held again at its end.
    pub fn position(&self) -> Vec3 {
        let Some(lunge) = self.lunge else {
            return self.base;
        };
        let length = self.ticks as f32 - HOLD as f32;
        let from = lunge.from * length;
        let to = lunge.to * length;
        let progress = if self.age as f32 <= from {
            0.0
        } else if self.age as f32 >= to || to <= from {
            1.0
        } else {
            (self.age as f32 - from) / (to - from)
        };
        self.base + self.direction * (self.length * progress)
    }

    /// Whether the blow lands this tick (once), and whether the motion is over.
    pub fn advance(&mut self) -> (bool, bool) {
        let hit = !self.reacted && self.age >= self.hit;
        if hit {
            self.reacted = true;
        }
        self.age += 1;
        (hit, self.age >= self.ticks)
    }
}

/// The battle's action scripts and hero packages, read once when the scene is built.
#[derive(Debug, Default)]
pub struct Scripts {
    /// The hero's own script (`mp0200.bact`): its weapon chooses the motions, not the script.
    hero: Option<Script>,
    /// `default.bact`: Defend, Flee, a call for help and the fallbacks.
    default: Option<Script>,
    /// Spell and skill files by action number (`sp%03d.bact`).
    specials: HashMap<u16, Option<Script>>,
    /// ROM bytes, kept only to read a spell's script the first time it is played.
    skill_archives: Vec<Vec<u8>>,
}

impl Scripts {
    /// Reads the hero's and the default script and the three spell/skill archives.
    pub fn load(rom: &[u8]) -> Result<Self> {
        let files = Rom::parse(rom).context("parsing the ROM")?;
        let mut out = Self::default();
        let chara_mp = files
            .file_by_path("data/pack_lv5/chara_mp.gp2")
            .context("the ROM has no chara_mp archive")?;
        if let Ok(pack) = Gpc2::parse(chara_mp)
            && let Some(entry) = pack.find("mp0200b.chr")
            && let Ok(bytes) = pack.member(entry)
            && let Ok(chr) = Chr::parse(&bytes)
            && let Some(bact) = (0..chr.narc.len())
                .filter_map(|i| u16::try_from(i).ok())
                .find(|&i| chr.narc.name(i).is_some_and(|n| n.ends_with(".bact")))
            && let Ok(bytes) = chr.narc.file(bact)
        {
            out.hero = Script::parse(bytes).ok();
        }
        if let Ok(bytes) = files.file_by_path("data/bin/actdef.nsarc")
            && let Ok(narc) = Narc::parse(bytes)
            && let Some(entry) = narc.names.files.iter().find(|f| f.path == "default.bact")
            && let Ok(bytes) = narc.file(entry.id)
        {
            out.default = Script::parse(bytes).ok();
        }
        for path in [
            "data/skill/actspl.nsarc",
            "data/skill/actskl.nsarc",
            "data/skill/actetc.nsarc",
        ] {
            if let Ok(bytes) = files.file_by_path(path) {
                out.skill_archives.push(bytes.to_vec());
            }
        }
        Ok(out)
    }

    /// The hero's own section for `action` (the plain Attack is 1).
    pub fn hero(&self, action: u16, gap: f32) -> Option<Blow> {
        self.hero
            .as_ref()?
            .section(action)
            .and_then(|ops| plan(ops, gap))
    }

    /// The fallback script's section for `action` (`3` Defend, `225` Flee, a call for help).
    pub fn fallback(&self, action: u16, gap: f32) -> Option<Blow> {
        self.default
            .as_ref()?
            .section(action)
            .and_then(|ops| plan(ops, gap))
    }

    /// `sp%03d.bact` of `action`, if one exists.
    pub fn special(&mut self, action: u16) -> Option<Blow> {
        if !self.specials.contains_key(&action) {
            let name = format!("sp{action:03}.bact");
            let mut found = None;
            for bytes in &self.skill_archives {
                let Ok(narc) = Narc::parse(bytes) else {
                    continue;
                };
                if let Some(entry) = narc.names.files.iter().find(|f| f.path == name)
                    && let Ok(bytes) = narc.file(entry.id)
                {
                    found = Script::parse(bytes).ok();
                    break;
                }
            }
            self.specials.insert(action, found);
        }
        self.specials
            .get(&action)?
            .as_ref()?
            .first()
            .and_then(|ops| plan(ops, 0.0))
    }
}

/// The members of a monster's battle archive (`enemy.gp2/<code>.mon`).
#[derive(Debug, Clone)]
pub struct MonsterArchive {
    /// The body (`.cchr`): the model, `appear`, `attack0a`, `run` and `stand`, decompressed.
    pub body: Vec<u8>,
    /// The rest of the motions (`.cmot`), which the field body `_f.mon` does not have.
    pub motions: Option<Vec<u8>>,
    /// Its action script (`.bact`), plain.
    pub script: Option<Vec<u8>>,
}

/// Reads the three members of a monster's battle archive (`enemy.gp2/<code>.mon`).
pub fn monster_members(rom: &[u8], code: &str) -> Result<MonsterArchive> {
    let files = Rom::parse(rom).context("parsing the ROM")?;
    let enemies = Gpc2::parse(
        files
            .file_by_path("data/pack_lv5/enemy.gp2")
            .context("the ROM has no enemy archive")?,
    )
    .context("parsing enemy.gp2")?;
    let entry = enemies
        .find(&format!("{code}.mon"))
        .with_context(|| format!("no {code}.mon in enemy.gp2"))?;
    let mon = enemies.member(entry)?;
    let monster = Narc::parse(&mon).with_context(|| format!("{code}.mon is not a NARC"))?;
    let mut body = None;
    let mut motions = None;
    let mut script = None;
    for file in &monster.names.files {
        let Ok(bytes) = monster.file(file.id) else {
            continue;
        };
        if file.path.ends_with(".cchr") {
            body = Some(
                nds_rom::compress::decompress(bytes)
                    .with_context(|| format!("{code}: decompressing {}", file.path))?,
            );
        } else if file.path.ends_with(".cmot") {
            motions = Some(
                nds_rom::compress::decompress(bytes)
                    .with_context(|| format!("{code}: decompressing {}", file.path))?,
            );
        } else if file.path.ends_with(".bact") {
            script = Some(bytes.to_vec());
        }
    }
    Ok(MonsterArchive {
        body: body.with_context(|| format!("{code}.mon has no .cchr"))?,
        motions,
        script,
    })
}

/// The members of a hero motion pack (`chara_mp.gp2/mp0201be.chr`), or `None` when the pack
/// is absent.
pub fn hero_pack(rom: &[u8], name: &str) -> Result<Option<Vec<u8>>> {
    let files = Rom::parse(rom).context("parsing the ROM")?;
    let Ok(bytes) = files.file_by_path("data/pack_lv5/chara_mp.gp2") else {
        return Ok(None);
    };
    let packs = Gpc2::parse(bytes).context("parsing chara_mp.gp2")?;
    let Some(entry) = packs.find(name) else {
        return Ok(None);
    };
    Ok(Some(packs.member(entry)?.into_owned()))
}

/// The motion set a weapon gives its wielder: word 4, bits 12–19 of its record in
/// `itemdt_w` (`0x02072c9c`; 0 for no weapon, one value per weapon kind).
pub fn weapon_set(rom: &[u8], item: u16) -> Option<u8> {
    let files = Rom::parse(rom).ok()?;
    let bytes = files.file_by_path("data/prm/itemdt_w.gp2").ok()?;
    let archive = Gpc2::parse(bytes).ok()?;
    let entry = archive.find("itemdt_w_en.nat")?;
    let bytes = archive.member(entry).ok()?;
    let count = usize::from(u16::from_le_bytes([*bytes.first()?, *bytes.get(1)?]));
    let records = 12usize;
    for index in 0..count {
        let at = records.checked_add(index.checked_mul(32)?)?;
        let record = bytes.get(at..at + 32)?;
        let id = i16::from_le_bytes([record[0x18], record[0x19]]);
        if id as u16 == item {
            let word = u32::from_le_bytes([record[0x10], record[0x11], record[0x12], record[0x13]]);
            return Some(((word >> 12) & 0xff) as u8);
        }
    }
    None
}

/// The name of a weapon's battle motion pack: `mp%02d%02d` with the body 02 and the weapon's
/// set (`mp0200be` bare-handed, `mp0201be` with a sword).
pub fn attack_pack(set: u8) -> String {
    format!("mp02{set:02}be.chr")
}

/// The default weapon of a new game: the copper sword (item 20004, set 1).
pub const NEW_GAME_WEAPON_SET: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    fn op(tag: u16, values: Vec<Val>) -> Op {
        Op { tag, values }
    }

    /// The hero's `mp0200.bact` shape: the far blow, then the near one.
    fn hero_like() -> Vec<Op> {
        vec![
            op(9, vec![Val::Int(3), Val::Int(2), Val::Float(2.0)]),
            op(77, vec![Val::Float(0.75)]),
            op(10, vec![Val::Int(3)]),
            op(9, vec![Val::Int(2), Val::Int(2), Val::Float(2.0)]),
            op(3, vec![Val::Int(7), Val::Str("attack1b".into())]),
            op(26, vec![Val::Int(7), Val::Float(0.5)]),
            op(9, vec![Val::Int(1), Val::Int(0)]),
            op(10, vec![Val::Int(2)]),
            op(3, vec![Val::Int(7), Val::Str("attack1a".into())]),
            op(5, vec![Val::Int(60), Val::Int(330), Val::Float(0.25)]),
            op(26, vec![Val::Int(7), Val::Int(60)]),
            op(26, vec![Val::Int(7), Val::Float(0.61)]),
            op(10, vec![Val::Int(1)]),
            op(116, vec![Val::Float(0.1)]),
            op(61, vec![]),
        ]
    }

    /// Apart, the script takes the far blow: the running `attack1a`, its lunge and the 61%
    /// hit; within two, the near `attack1b` at half its motion.
    #[test]
    fn the_gap_picks_the_blow_the_script_picks() {
        let far = plan(&hero_like(), 9.0).unwrap();
        assert_eq!(far.motion, "attack1a");
        assert_eq!(
            far.lunge,
            Some(Lunge {
                from: 0.06,
                to: 0.33,
                gap: 0.25
            })
        );
        assert!((far.hit - 0.61).abs() < 1e-6);
        assert!(!far.fades);

        let near = plan(&hero_like(), 1.0).unwrap();
        assert_eq!(near.motion, "attack1b");
        assert_eq!(near.lunge, None);
        assert!((near.hit - 0.5).abs() < 1e-6);
    }

    /// A skip that does not hold keeps the branch; an unreadable result skip does not jump
    /// before the blow.
    #[test]
    fn skips_are_not_taken_when_their_condition_fails() {
        let ops = vec![
            op(9, vec![Val::Int(1), Val::Int(2), Val::Float(20.0)]),
            op(3, vec![Val::Int(7), Val::Str("attack1a".into())]),
            op(10, vec![Val::Int(1)]),
        ];
        assert_eq!(plan(&ops, 9.0).unwrap().motion, "attack1a");
        let ops = vec![
            op(9, vec![Val::Int(1), Val::Int(0), Val::Int(0)]),
            op(3, vec![Val::Int(7), Val::Str("attack1a".into())]),
            op(10, vec![Val::Int(1)]),
            op(3, vec![Val::Int(7), Val::Str("attack1b".into())]),
        ];
        assert_eq!(plan(&ops, 9.0).unwrap().motion, "attack1b");
    }

    /// A camera's motion is not the actor's, and a flight's fade is kept.
    #[test]
    fn only_the_actors_motion_and_its_fade_are_read() {
        let ops = vec![
            op(3, vec![Val::Int(25), Val::Str("cut_01".into())]),
            op(3, vec![Val::Int(7), Val::Str("escape".into())]),
            op(47, vec![Val::Int(7), Val::Int(0), Val::Int(500)]),
        ];
        let blow = plan(&ops, 0.0).unwrap();
        assert_eq!(blow.motion, "escape");
        assert!(blow.fades);
        assert_eq!(plan(&[], 0.0), None);
    }

    /// A motion's ticks are its frames over its speed; the lunge is linear and the fighter
    /// ends at its place once the motion and the hold are over.
    #[test]
    fn a_timeline_lunges_then_returns() {
        let blow = Blow {
            motion: "attack1a".into(),
            lunge: Some(Lunge {
                from: 0.25,
                to: 0.5,
                gap: 0.5,
            }),
            hit: 0.6,
            fades: false,
        };
        let mut timeline = Timeline::blow(
            &blow,
            40,
            Vec3::new(0.0, 0.0, 0.0),
            0.0,
            0.25,
            Vec3::new(0.0, 0.0, 10.0),
            0.25,
        );
        // 10 apart, 0.5 kept between them → 9 of travel.
        assert_eq!(timeline.position(), Vec3::ZERO);
        let mut hits = 0;
        let mut done = false;
        while !done {
            let (hit, end) = timeline.advance();
            hits += u32::from(hit);
            done = end;
        }
        assert_eq!(hits, 1);
        assert_eq!(timeline.age, 48);
        assert!(timeline.position().z > 0.0);
        // The hold is past the motion: the last pose stands until the timeline ends, then
        // the scene puts the fighter back at `base`.
        assert_eq!(
            Timeline::reaction("damage".into(), End::Idle, 5, Vec3::ZERO, 0.0).ticks,
            5
        );
    }

    #[test]
    fn ticks_follow_the_motion_table() {
        // attack1a: frames 1..19 at 0.25 → 18 frames × 4096 / 1024.
        assert_eq!(motion_ticks(1 << 12, 19 << 12, 0x400), 72);
        assert_eq!(motion_ticks(0, 0, 0), FALLBACK_MOTION_TICKS);
    }
}
