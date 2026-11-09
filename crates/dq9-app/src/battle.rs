//! The battle scene: a stage map, the fighters on its grid, their idle motions and the
//! battle camera (`docs/app.md`, § "Battle scene"; wiki *Battle-Stages*).
//!
//! The stage is an ordinary map of the map list (30116 "F01 - Field", 30215 "D01 -
//! Hexagoon"…), assembled and lit like a field, but the party and the monsters stand where
//! the battle set-up puts them, not where the map places anything: the game centres the
//! fight on the stage's origin, the party on `+z` facing π and the monsters on `−z` facing 0,
//! and starts everyone on the grid (`ov_000::02164d74`; the row is only used by a few action
//! scripts, so it is asked for explicitly).
//!
//! What is faithful: the grid slot formula (`ov_000::0216f74c`), the side framing of the
//! battle camera (`ov_000::0216d600`) and the eye clamp applied after every command
//! (`ov_000::0216f2b8`). What is a **[hypothesis]**: which of the two sides the opening
//! (`ov_000::0216118c`, "side 1") is, the eased approach of the opening, the row's spacing
//! and turn at the ends, and the fallback slot of a formation larger than eight.
//!
//! The scene owns its runtime: the application swaps it in for the field's scene and gives
//! the field back when the battle is over (`App::enter_battle`, `App::restore_field`).

// Part of the scene's API is the native CLI's and the tests' (positions, camera, screen
// coordinates); the Web build compiles it for the next wave's battle commands, which is why
// some of it is not called there yet.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::f32::consts::PI;

use anyhow::{Context as _, Result};
use dq9_core::scenario::{BattleRecord, battle as battle_record};
use dq9_formats::Gpc2;
use dq9_formats::battle::monster::Names;
use dq9_formats::cast::Story;
use dq9_formats::hero::HeroLook;
use dq9_render::camera::DS_ASPECT;
use dq9_render::{AnimatedScene, Camera, Environment};
use glam::{Mat4, Vec2, Vec3, Vec3Swizzles};
use nds_rom::Rom;

use crate::lighting;
use crate::load::{self, MapEntry};

/// Stage the game falls back to when the ground names none, or a map the list does not hold
/// (`func_ov000_02166880`, wiki *Battle-Stages*).
pub const DEFAULT_STAGE: u16 = 30116;

/// The ordinary battles' track: sequence 0x17 (`func_0209c480`, wiki *Battle-Stages*).
pub const BATTLE_TRACK: u8 = 0x17;

/// Every fighter's height above the stage, from the templates (`+0xcc` fx32; the stage has no
/// collision to put them on, wiki *Battle-Stages*).
const FIGHTER_HEIGHT: f32 = 0xcc as f32 / 4096.0;

/// Character models are drawn at `0x10a` (`NpcTable_Build`, `0x0203d038`; the monster's
/// `+0x12` is its battle size, read by the camera, not its scale).
const MODEL_SCALE: f32 = 266.0 / 4096.0;

/// The camera keeps the eye no further than 17 from its target and no higher than 5
/// (`ov_000::0216f2b8`).
const EYE_REACH: f32 = 17.0;
const EYE_HEIGHT: f32 = 5.0;

/// The battle camera's default half-angle (15°, `func_ov000_0216d370`) and the wide variant's
/// (22°, `func_ov000_0216d600`).
const HALF_ANGLE: f32 = 15.0;
const WIDE_HALF_ANGLE: f32 = 22.0;

/// Which side the party is on (`+z`) and which the monsters are on (`−z`).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Party,
    Monsters,
}

/// One fighter of the scene, by side and rank in it (`Fighter::party(0)` is the hero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fighter {
    pub side: Side,
    pub index: usize,
}

impl Fighter {
    pub const fn party(index: usize) -> Self {
        Self {
            side: Side::Party,
            index,
        }
    }

    pub const fn monster(index: usize) -> Self {
        Self {
            side: Side::Monsters,
            index,
        }
    }
}

/// Where the fighters stand (`ov_000::02167dd8` for the grid, `0x02167e6c` for the row).
///
/// The row is only asked for by action scripts (calling for help, some special attacks), which
/// this batch does not play; `BattleScene::set_formation` is the entry the next wave uses.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Formation {
    /// The 9 by 9 staggered grid every battle starts on.
    #[default]
    Grid,
    /// The row an action script asks for.
    Row,
}

/// What a battle is asked for: an `eventbattle.bin` record, or an explicit stage and monsters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Request {
    /// `eventbattle.bin` index (`--battle 2`: the Hexagoon); its stage, monsters and track
    /// come from the record.
    pub record: Option<u16>,
    /// Explicit stage map id; 0 asks the record's, else [`DEFAULT_STAGE`].
    pub stage: u16,
    /// `(monster number, count)` of an explicit stage.
    pub monsters: Vec<(u16, u16)>,
    /// Music sequence; `None` takes the record's, else [`BATTLE_TRACK`].
    pub track: Option<u8>,
}

/// A [`Request`] with the record read: what the scene needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub stage: u16,
    pub monsters: Vec<(u16, u16)>,
    pub track: u8,
    pub number: Option<u16>,
    pub record: Option<BattleRecord>,
}

impl Request {
    /// Reads the `eventbattle.bin` record the request names and settles the stage, the
    /// monsters and the track. A record that is missing is an error only when it was asked
    /// for explicitly.
    pub fn resolve(&self, rom: &[u8]) -> Result<Resolved> {
        let record = match self.record {
            Some(number) => {
                let files = Rom::parse(rom).context("parsing the ROM")?;
                Some(
                    battle_record(&files, number)
                        .with_context(|| format!("no battle {number} in eventbattle.bin"))?,
                )
            }
            None => None,
        };
        let from_record = record.as_ref().and_then(|r| u16::try_from(r.stage).ok());
        let stage = if self.stage != 0 {
            self.stage
        } else {
            from_record
                .filter(|&stage| stage != 0)
                .unwrap_or(DEFAULT_STAGE)
        };
        let monsters = match &record {
            Some(record) => record
                .monsters
                .iter()
                .filter_map(|&(number, count)| Some((u16::try_from(number).ok()?, count as u16)))
                .collect(),
            None => self.monsters.clone(),
        };
        let track = self
            .track
            .or_else(|| record.as_ref().map(|r| r.track as u8))
            .unwrap_or(BATTLE_TRACK);
        Ok(Resolved {
            stage,
            monsters,
            track,
            number: self.record,
            record,
        })
    }
}

/// A fighter where the scene put it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedFighter {
    pub fighter: Fighter,
    /// Monster number (`mon_data`), `None` for the hero.
    pub number: Option<u16>,
    pub position: Vec3,
    /// Game angle (0 faces `+z`).
    pub heading: f32,
    /// Hidden by the defeat fade (`remove`).
    pub alive: bool,
}

/// How the battle camera frames a side (`func_ov000_0216d600`). The action scripts pick a
/// side with their camera commands, so the variant is the next wave's.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    /// The opening: the wide side shot on the party's side (`func_ov000_0216118c`).
    Opening,
    /// The normal side shot (15° half-angle), from `side`.
    Side(Side),
}

/// The battle: the stage's scene, its lighting kept as the battle was asked for, the fighters
/// and the camera.
#[derive(Debug)]
pub struct BattleScene {
    stage: u16,
    entry: MapEntry,
    label: String,
    /// The stage's lighting script, its slot taken once at entry (wiki *Battle-Stages*).
    lighting: Option<dq9_formats::lighting::LightingInfo>,
    pub environment: Environment,
    camera: Camera,
    formation: Formation,
    shot: Shot,
    fighters: Vec<PlacedFighter>,
    /// Scene instance and actor of each fighter, in [`fighters`](Self::fighters) order.
    instances: Vec<usize>,
    actors: Vec<Option<usize>>,
    /// Drawn height of each fighter's body, for the screen position.
    heights: Vec<f32>,
    /// Row width of each fighter (`mon_data` radius ×4 for a monster, 1 for the hero).
    widths: Vec<f32>,
    runtime: AnimatedScene,
}

impl BattleScene {
    /// Builds the scene of `resolved`: loads the stage map (its own `.bats` lighting, the slot
    /// taken once at `clock`), places the party and the monsters on the grid and plays their
    /// idle motions. The party is the hero (party companions are not loaded yet,
    /// `docs/app.md`).
    pub fn build(
        rom: &[u8],
        resolved: &Resolved,
        time: u32,
        clock: dq9_core::state::DayClock,
        story: &Story,
        look: &HeroLook,
    ) -> Result<Self> {
        let key = resolved.stage.to_string();
        let mut loaded = load::load(rom, &key, time, story, None, None, look)
            .with_context(|| format!("loading battle stage {}", resolved.stage))?;
        let files = Rom::parse(rom).context("parsing the ROM")?;
        // The names member may be decompressed into a local buffer: read it for the whole
        // build, not beyond it.
        let names_archive = Gpc2::parse(
            files
                .file_by_path("data/prm/mon_data.gp2")
                .context("the ROM has no mon_data archive")?,
        )
        .context("parsing mon_data.gp2")?;
        let names_member = names_archive
            .entries
            .iter()
            .find(|e| e.name == "mon_data_en.nat")
            .and_then(|entry| names_archive.member(entry).ok())
            .context("no readable mon_data_en.nat member")?;
        let names = Names::parse(&names_member).context("parsing mon_data_en.nat")?;
        // The map's own hero stands at its start point; the battle puts it on the grid.
        let mut fighters = Vec::new();
        let mut instances = Vec::new();
        let mut actors = Vec::new();
        let mut heights = Vec::new();
        let mut widths = Vec::new();

        let hero_slot = party_slots(1)[0];
        let position = grid_position(hero_slot) + Vec3::Y * FIGHTER_HEIGHT;
        let heading = PI;
        loaded.hero = dq9_core::Hero::at(position, dq9_core::field::yaw_of_angle(heading));
        loaded.runtime.scene.instances[loaded.hero_instance].world =
            loaded.runtime.hero_world(position, heading);
        loaded.runtime.play_hero("stand", false);
        fighters.push(PlacedFighter {
            fighter: Fighter::party(0),
            number: None,
            position,
            heading,
            alive: true,
        });
        instances.push(loaded.hero_instance);
        actors.push(None);
        heights.push(1.5);
        widths.push(1.0);

        let flat = flatten(&resolved.monsters);
        if flat.len() > 8 {
            crate::app::status(&format!(
                "battle: {} monsters, the game's grid holds eight; the rest use free slots \
                 [hypothesis]",
                flat.len()
            ));
        }
        let slots = monster_slots(flat.len());
        for (rank, (&number, slot)) in flat.iter().zip(&slots).enumerate() {
            let record = names
                .find(number)
                .with_context(|| format!("monster {number} has no mon_data record"))?;
            let code = String::from_utf8_lossy(record.code);
            let chr = crate::event::scan::monster_archive_bytes(rom, &format!("enemy/{code}.mon"))
                .with_context(|| format!("monster {number} ({code}) has no body in enemy.gp2"))?;
            let actor = loaded
                .runtime
                .add_actor(&chr, &[])
                .with_context(|| format!("monster {number} ({code}): model"))?;
            let instance = loaded
                .runtime
                .actor_instance(actor)
                .with_context(|| format!("monster {number} ({code}): no polygons"))?;
            let position = grid_position(*slot) + Vec3::Y * FIGHTER_HEIGHT;
            let heading = 0.0;
            loaded.runtime.scene.instances[instance].world = monster_world(position, heading);
            loaded.runtime.scene.instances[instance].home = monster_world(position, heading);
            // Idle: `stand` (the slime's and the models' usual idle); `appear` then `run` when
            // the model has no such motion (a name the archive lacks is a no-op in the game).
            if !loaded.runtime.play_actor(actor, "stand", false, 1.0) {
                if loaded.runtime.play_actor(actor, "appear", true, 1.0) {
                    crate::app::status(&format!("monster {number} ({code}): no stand, appear"));
                } else if loaded.runtime.play_actor(actor, "run", false, 1.0) {
                    crate::app::status(&format!("monster {number} ({code}): no stand, run"));
                } else {
                    crate::app::status(&format!("monster {number} ({code}): no idle motion"));
                }
            }
            fighters.push(PlacedFighter {
                fighter: Fighter::monster(rank),
                number: Some(number),
                position,
                heading,
                alive: true,
            });
            instances.push(instance);
            actors.push(Some(actor));
            heights.push((f32::from(record.height) / 4096.0).max(0.5));
            // The row's width: the body's radius ×4 (`+0x0C`, 1024ths).
            widths.push((f32::from(record.radius) / 1024.0 * 4.0).max(0.5));
        }

        let lighting = loaded.lighting.clone();
        let environment = lighting::environment(lighting.as_ref(), clock);
        let mut scene = Self {
            stage: resolved.stage,
            entry: loaded.entry,
            label: loaded.label,
            lighting,
            environment,
            camera: Camera::default(),
            formation: Formation::Grid,
            shot: Shot::Opening,
            fighters,
            instances,
            actors,
            heights,
            widths,
            runtime: loaded.runtime,
        };
        scene.set_shot(Shot::Opening);
        Ok(scene)
    }

    /// The stage map and its label.
    pub fn entry(&self) -> &MapEntry {
        &self.entry
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn stage(&self) -> u16 {
        self.stage
    }

    pub fn lighting(&self) -> Option<&dq9_formats::lighting::LightingInfo> {
        self.lighting.as_ref()
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn formation(&self) -> Formation {
        self.formation
    }

    pub fn shot(&self) -> Shot {
        self.shot
    }

    pub fn fighters(&self) -> &[PlacedFighter] {
        &self.fighters
    }

    /// The fighter, wherever it is in [`fighters`](Self::fighters).
    pub fn fighter(&self, fighter: Fighter) -> Option<&PlacedFighter> {
        self.fighters.iter().find(|f| f.fighter == fighter)
    }

    pub fn runtime(&self) -> &AnimatedScene {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut AnimatedScene {
        &mut self.runtime
    }

    /// One fixed step of the scene's animations at the battle camera.
    pub fn step(&mut self) -> Result<()> {
        self.runtime
            .step(&self.camera)
            .context("advancing the battle animations")
    }

    /// Plays `motion` on `fighter`: the hero's own motion name, or a name of the monster's
    /// archive. Returns whether the model has it (the motion it played goes on otherwise).
    /// The action scripts of the next wave are the callers.
    #[allow(dead_code)]
    pub fn play(&mut self, fighter: Fighter, motion: &str) -> bool {
        let Some(index) = self.index_of(fighter) else {
            return false;
        };
        match self.actors[index] {
            None => self.runtime.play_hero(motion, false),
            Some(actor) => self.runtime.play_actor(actor, motion, false, 1.0),
        }
    }

    /// The defeat fade: the fighter is hidden and `alive` goes false. Its slot stays (the
    /// game keeps it for the death effect and the results). The action scripts of the next
    /// wave are the callers.
    #[allow(dead_code)]
    pub fn remove(&mut self, fighter: Fighter) -> bool {
        let Some(index) = self.index_of(fighter) else {
            return false;
        };
        if let Some(instance) = self.instances.get(index)
            && let Some(entry) = self.runtime.scene.instances.get_mut(*instance)
        {
            entry.world = Mat4::from_scale(Vec3::ZERO);
        }
        self.fighters[index].alive = false;
        true
    }

    /// Frames `shot` and clamps the eye (`ov_000::0216d600`, `ov_000::0216f2b8`).
    pub fn set_shot(&mut self, shot: Shot) {
        self.shot = shot;
        let (side, wide) = match shot {
            Shot::Opening => (Side::Party, true),
            Shot::Side(side) => (side, false),
        };
        let half = if wide { WIDE_HALF_ANGLE } else { HALF_ANGLE };
        let (width, depth) = self.extents();
        let (sin, cos) = half.to_radians().sin_cos();
        let mut distance = (width * cos / (sin * 2.2)).max(depth * cos / (sin * 1.5)) - 2.5;
        distance = distance.max(if wide { 3.0 } else { 6.5 });
        // "the eye at most 2" high, and half the depth at least 1.2.
        let height = (depth * 0.5).clamp(1.2, 2.0);
        let sign = match side {
            Side::Party => 1.0,
            Side::Monsters => -1.0,
        };
        let target = Vec3::new(0.0, height, 0.0);
        let eye = clamp_eye(target, Vec3::new(0.0, height, sign * distance));
        self.camera.look_at(eye, target);
        self.camera.fovy = 2.0 * half.to_radians();
    }

    /// Asks for the row formation (`0x02167e6c`), or for the grid the battle starts on
    /// (`0x02167dd8`). The charge's scripts are the only callers in the game; no action is
    /// played in this batch, so it is the next wave's entry point.
    #[allow(dead_code)]
    pub fn set_formation(&mut self, formation: Formation) {
        if self.formation == formation {
            return;
        }
        self.formation = formation;
        self.place();
        self.set_shot(self.shot);
    }

    /// Puts the fighters where the formation says and refreshes their drawn transforms.
    #[allow(dead_code)]
    fn place(&mut self) {
        let party_count = self
            .fighters
            .iter()
            .filter(|f| f.fighter.side == Side::Party)
            .count();
        let monster_count = self.fighters.len() - party_count;
        let party_slots = party_slots(party_count);
        let monster_slots = monster_slots(monster_count);
        let widths: Vec<f32> = self.widths[party_count..].to_vec();
        for index in 0..self.fighters.len() {
            let fighter = self.fighters[index].fighter;
            let (position, heading) = match (self.formation, fighter.side) {
                (Formation::Grid, Side::Party) => (grid_position(party_slots[fighter.index]), PI),
                (Formation::Grid, Side::Monsters) => {
                    (grid_position(monster_slots[fighter.index]), 0.0)
                }
                (Formation::Row, Side::Party) => row_party(fighter.index, party_count),
                (Formation::Row, Side::Monsters) => row_monster(&widths, fighter.index),
            };
            let position = position + Vec3::Y * FIGHTER_HEIGHT;
            self.fighters[index].position = position;
            self.fighters[index].heading = heading;
            let instance = self.instances[index];
            let world = match self.actors[index] {
                None => self.runtime.hero_world(position, heading),
                Some(_) => monster_world(position, heading),
            };
            if let Some(entry) = self.runtime.scene.instances.get_mut(instance) {
                entry.world = world;
                entry.home = world;
            }
        }
    }

    /// Width and depth of the fighters' formation, from the drawn places.
    fn extents(&self) -> (f32, f32) {
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for placed in &self.fighters {
            lo = lo.min(placed.position.xz());
            hi = hi.max(placed.position.xz());
        }
        if self.fighters.is_empty() {
            return (2.0, 2.0);
        }
        // The bodies' widths widen the formation beyond the places alone; the tallest body is
        // the monster row's height (`ov_000::0216d600`).
        let body = self.widths.iter().copied().fold(0.0f32, f32::max);
        let tallest = self.heights.iter().copied().fold(0.0f32, f32::max).max(1.5);
        (
            (hi.x - lo.x + body.max(1.0)).max(2.0),
            (hi.y - lo.y + 1.5).max(tallest + 0.5).max(2.0),
        )
    }

    fn index_of(&self, fighter: Fighter) -> Option<usize> {
        self.fighters.iter().position(|f| f.fighter == fighter)
    }

    /// Where `fighter`'s body is on the 256×192 top screen, in pixels (damage numbers, next
    /// wave). `None` behind the camera.
    pub fn fighter_screen_pos(&self, fighter: Fighter) -> Option<Vec2> {
        let index = self.index_of(fighter)?;
        if !self.fighters[index].alive {
            return None;
        }
        let head = self.fighters[index].position + Vec3::Y * self.heights[index] * 0.5;
        let clip = self.camera.view_proj(DS_ASPECT) * head.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new(
            (ndc.x + 1.0) * 0.5 * 256.0,
            (1.0 - ndc.y) * 0.5 * 192.0,
        ))
    }
}

/// Keeps the eye where `ov_000::0216f2b8` does: at most [`EYE_REACH`] from the target in the
/// ground plane, and at most [`EYE_HEIGHT`] above the stage.
fn clamp_eye(target: Vec3, eye: Vec3) -> Vec3 {
    let mut offset = eye - target;
    offset.y = offset.y.min(EYE_HEIGHT);
    let flat = Vec2::new(offset.x, offset.z);
    let reach = flat.length();
    if reach > EYE_REACH {
        let scale = EYE_REACH / reach;
        offset.x *= scale;
        offset.z *= scale;
    }
    target + offset
}

/// A grid slot's place (`ov_000::0216f74c`): column `slot mod 9`, row `slot div 9`;
/// x = 2.598 × column − 10.392, plus 1.299 on an odd row; z = 2.25 × row − 9.
///
/// port: ov_000::0216f74c
pub fn grid_position(slot: usize) -> Vec3 {
    let column = (slot % 9) as f32;
    let row = (slot / 9) as f32;
    let stagger = if (slot / 9) % 2 == 1 { 1.299 } else { 0.0 };
    Vec3::new(2.598 * column - 10.392 + stagger, 0.0, 2.25 * row - 9.0)
}

/// The party's grid slots by its size, from the table at `0x02183108` (wiki *Battle-Stages*).
pub fn party_slots(count: usize) -> Vec<usize> {
    const TABLE: [&[usize]; 4] = [&[58], &[57, 59], &[48, 58, 50], &[56, 66, 67, 60]];
    match TABLE.get(count.saturating_sub(1)) {
        Some(slots) => slots.to_vec(),
        // The party holds four: a fifth would be a scripting error; give it the grid's middle.
        None => (0..count).map(|i| 58 + i).collect(),
    }
}

/// The monsters' grid slots by their size, from the table at `0x02183118`.
pub fn monster_slots(count: usize) -> Vec<usize> {
    const TABLE: [&[usize]; 8] = [
        &[22],
        &[21, 23],
        &[29, 22, 32],
        &[20, 12, 13, 24],
        &[28, 11, 22, 14, 33],
        &[20, 11, 12, 13, 14, 24],
        &[20, 11, 12, 22, 13, 14, 24],
        &[19, 10, 11, 12, 13, 14, 15, 25],
    ];
    if let Some(slots) = TABLE.get(count.saturating_sub(1)) {
        return slots.to_vec();
    }
    // More than eight: the table's eight, then the free slots nearest the monsters' middle
    // **[hypothesis]**; the game's battles never ask for more (`eventbattle.bin`, counts up
    // to 8).
    let mut slots = TABLE[7].to_vec();
    let mut candidate = 0;
    while slots.len() < count {
        if !slots.contains(&candidate) {
            slots.push(candidate);
        }
        candidate += 1;
    }
    slots
}

/// The row the party stands in: `z` +2.5, 1.5 apart and centred, turned in at the ends
/// (`0x021675a0`, `0x021677fc`; the turns are read as ±0.2 **[hypothesis]**).
#[allow(dead_code)]
fn row_party(index: usize, count: usize) -> (Vec3, f32) {
    let x = index as f32 * 1.5 - (count.saturating_sub(1) as f32) * 0.75;
    let turn = if count > 1 {
        0.2 * (2.0 * index as f32 / (count - 1) as f32 - 1.0)
    } else {
        0.0
    };
    (Vec3::new(x, 0.0, 2.5), PI + turn)
}

/// The row the monsters stand in: `z` −2.5, facing 0, side by side by their widths (radius ×4,
/// gap 0.7, narrowed to keep the row within 4 + 0.1(n − 1), down to 0.1; `0x02167b5c`).
#[allow(dead_code)]
fn row_monster(widths: &[f32], index: usize) -> (Vec3, f32) {
    let count = widths.len();
    if count == 0 {
        return (Vec3::new(0.0, 0.0, -2.5), 0.0);
    }
    let total: f32 = widths.iter().sum();
    let gap = if count > 1 {
        let limit = 4.0 + 0.1 * (count as f32 - 1.0);
        ((limit - total) / (count as f32 - 1.0)).clamp(0.1, 0.7)
    } else {
        0.0
    };
    let mut x = -(total + gap * (count as f32 - 1.0)) * 0.5;
    for (i, width) in widths.iter().enumerate() {
        if i == index {
            return (Vec3::new(x + width * 0.5, 0.0, -2.5), 0.0);
        }
        x += width + gap;
    }
    (Vec3::new(0.0, 0.0, -2.5), 0.0)
}

fn monster_world(position: Vec3, heading: f32) -> Mat4 {
    Mat4::from_translation(position)
        * Mat4::from_rotation_y(heading)
        * Mat4::from_scale(Vec3::splat(MODEL_SCALE))
}

fn flatten(monsters: &[(u16, u16)]) -> Vec<u16> {
    let mut out = Vec::new();
    for &(number, count) in monsters {
        for _ in 0..count {
            out.push(number);
        }
    }
    out
}
