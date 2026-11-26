//! The battle scene: a stage map, the fighters on its grid, their idle motions and the
//! battle camera (`docs/app/battle-scene.md`, § "Battle scene"; wiki *Battle-Stages*).
//!
//! The stage is an ordinary map of the map list (30116 "F01 - Field", 30215 "D01 -
//! Hexagoon"…), assembled and lit like a field, but the party and the monsters stand where
//! the battle set-up puts them, not where the map places anything: the game centres the
//! fight on the stage's origin, the party on `+z` facing π and the monsters on `−z` facing 0,
//! and starts everyone on the grid (`ov_000::02164d74`; the row is only used by a few action
//! scripts, so it is asked for explicitly).
//!
//! What is faithful: the grid slot formula (`ov_000::0216f74c`) and the party's and monsters'
//! slot tables (`0x02183108` four rows, `0x02183118` eight), the
//! row's places and turns (`ov_000::021675a0`, `ov_000::021677fc`, `0x02183158`,
//! `0x02167b5c`), the camera's extents from that row, the side framing of the battle camera
//! (`ov_000::0216d600`), the opening's pulled-back start and its eased approach into that
//! framing (`ov_000::0216118c`, `ov_000::0216f0d0`), the fixed shots of the big monster kinds
//! (`data_ov026_021de87c`, `ov_026::021d8aac`) and the eye clamp applied after every command
//! (`ov_000::0216f2b8`). What is a **[hypothesis]**: the body widths the row and the extents
//! use (`mon_data` radius ×4 where the original reads `Object3D_GetRadius`), the `0x115`
//! record's variant and the `kind 0` default of the shot table.
//!
//! The scene owns its runtime: the application swaps it in for the field's scene and gives
//! the field back when the battle is over (`App::enter_battle`, `App::restore_field`).

// Part of the scene's API is the native CLI's and the tests' (positions, camera, screen
// coordinates); the Web build compiles it for the next wave's battle commands, which is why
// some of it is not called there yet.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::f32::consts::PI;

use anyhow::{Context as _, Result};
use dq9_core::battle::Surprise;
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
use motion::{End, Script, Scripts, Timeline};

pub mod data;
pub mod entry;
pub mod fight;
pub mod motion;
pub mod narrate;
mod shots;
#[cfg(test)]
mod tests;
pub mod transition;
pub mod ui;

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

/// The sine and cosine of both half-angles on the game's fx12 grid, as `func_ov000_0216d600`
/// carries them: `0x424`/`0x424 + 0xb50` = sin/cos 15° at `iRam0216d8f4`, `0x5fe`/`0xed6` at
/// `iRam0216d8f8`/`iRam0216d8fc` (read on the EU ROM, `build/evidence/v4-battle-intro/
/// camera-tables.txt`).
const SIN_NARROW: i32 = 0x424;
const COS_NARROW: i32 = 0x424 + 0xb50;
const SIN_WIDE: i32 = 0x5fe;
const COS_WIDE: i32 = 0xed6;

/// The opening's approach: the side shot's own spherical framing is kept at `0xf33` (3891/4096
/// = 0.95) and the rest moved **per 30 Hz update** (`uRam02161260`, `func_ov000_0216f0d0`; the
/// battle scene iterates at 30 Hz, `ov_017::0218b688`'s `CalculateDeltaTime` giving 2 VBlanks
/// per update); one eased frame is therefore every second 60 Hz fixed step
/// (`transition::UPDATE_TICKS`). The step gives up once every difference is under `0x28`.
const INTRO_GAIN: i32 = 0x1000 - 0xf33;
const INTRO_SNAP: i32 = 0x28;

/// The opening pulls the eye back by these from the side shot's framing (`func_ov000_0216118c`:
/// `local_24 = y + 0x800`, `local_20 = z + 0x3000`, the camera's own spherical height and
/// distance in fx12).
const INTRO_PITCH: i32 = 0x800;
const INTRO_DISTANCE: i32 = 0x3000;

/// The game's angle unit: fx12 radians, a full turn `0x6488` (2π × 4096) and a half turn
/// `0x3244` (ROM `0x02030d30`/`0x02030ce0`, `fix32SignedAngleDistance`).
const ANGLE_HALF_TURN: i32 = 0x3244;
const ANGLE_TURN: i32 = 0x6488;

/// Which side the party is on (`+z`) and which the monsters are on (`−z`).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Party,
    Monsters,
}

impl Side {
    /// The sign the camera's `z` takes when it looks at this side (`func_ov000_0216d600`:
    /// side 0 is its `param_2 == 0` branch, which negates both `z`, and its extents are the
    /// party's at `+0x228`; side 1 is the monsters', at `+0x230`).
    const fn camera_sign(self) -> f32 {
        match self {
            Side::Monsters => 1.0,
            Side::Party => -1.0,
        }
    }
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

/// Where the fighters stand (`ov_000::02167dd8` for the grid, `ov_000::02167e6c` for the row).
///
/// The battle's set-up places everyone on the grid (`ov_000::02164d74`); the row places are
/// computed with it (`ov_000::021675a0`, `ov_000::021677fc`) and a few action scripts ask for
/// them (`ov_000::02167e6c` sets the flag). `BattleScene::set_formation` is the entry the next
/// wave's scripts use.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Formation {
    /// The 9 by 9 staggered grid every battle starts on.
    #[default]
    Grid,
    /// The row an action script asks for: the party turned at its ends, the monsters side by
    /// side by their widths (the five narrow kinds at 0.7).
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
    /// Who has the jump at the start; `None` is an even start.
    pub surprise: Option<Surprise>,
}

/// A [`Request`] with the record read: what the scene needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub stage: u16,
    pub monsters: Vec<(u16, u16)>,
    pub track: u8,
    pub number: Option<u16>,
    pub record: Option<BattleRecord>,
    /// Who has the jump at the start (the roaming monsters' contact).
    pub surprise: Surprise,
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
            surprise: self.surprise.unwrap_or(Surprise::None),
        })
    }
}

/// A fighter where the scene put it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedFighter {
    pub fighter: Fighter,
    /// Monster number (`mon_data`), `None` for the hero.
    pub number: Option<u16>,
    /// The monster's kind (`mon_data +0x10`, shared by its story versions), `None` for the
    /// hero: the fixed shots of `data_ov026_021de87c` are keyed by it (`func_ov000_0215fc8c`).
    pub kind: Option<u16>,
    pub position: Vec3,
    /// Game angle (0 faces `+z`).
    pub heading: f32,
    /// Hidden by the defeat fade (`remove`).
    pub alive: bool,
}

/// How the battle camera frames a side (`func_ov000_0216d600`). `side` is the side **looked
/// at**: the eye stands on the opposite one, so `Side::Monsters` places it on the party's `+z`
/// (the game's side 1, `param_2 == 1`, extents at `+0x230`, no `z` negation).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    /// The opening (`func_ov000_0216118c`): the wide 22° shot on the monsters, pulled back by
    /// 0.5 and 3.0 and eased into that framing at `0.95` a 30 Hz update.
    Opening,
    /// The normal side shot (15° half-angle), looking at `side`.
    Side(Side),
}

/// A framing in the game's spherical parameters (`camera +0x70`/`+0x74`/`+0x78`, written by
/// `func_0202eab8` and interpolated by `func_ov000_0216f0d0`): the yaw in fx12 radians, the
/// eye's height above the target in fx12 world units, and the eye's distance from the target
/// in fx12 world units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Framing {
    yaw: i32,
    pitch: i32,
    distance: i32,
}

/// The opening's easing (`func_ov000_0216f0d0`): where the eye is and the side shot's framing
/// it eases toward, both around the same target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Approach {
    current: Framing,
    target: Framing,
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
    /// The opening's easing while it runs (`None` once it settled, or for any other shot).
    approach: Option<Approach>,
    /// Which 60 Hz step of the 30 Hz update the running opening is on (the easing moves on
    /// every second one).
    approach_phase: bool,
    fighters: Vec<PlacedFighter>,
    /// Scene instance and actor of each fighter, in [`fighters`](Self::fighters) order.
    instances: Vec<usize>,
    actors: Vec<Option<usize>>,
    /// Drawn height of each fighter's body, for the screen position.
    heights: Vec<f32>,
    /// The body radius each fighter's row place and lunge use (`mon_data +0x0C` in 1024ths for
    /// a monster — the object's own radius, `Object3D_SetRadius(record[+0x0C] << 2)` at
    /// `arm9 0x020377d4`, set by `func_020484f8`/`func_02048850`; 0.25 for the hero, whose
    /// model radius is not read).
    widths: Vec<f32>,
    /// The length of each fighter's motions, from its model archives, aligned with
    /// [`fighters`](Self::fighters).
    spans: Vec<Vec<(String, u32)>>,
    /// Each monster's action script (`<code>.bact`), aligned with `fighters`.
    bacts: Vec<Option<Script>>,
    /// The hero's, the fallback and the spell scripts.
    scripts: Scripts,
    /// The hero's motion packs as read (`mp0200b`, the `be` set, `mp0200bm`), for the spans
    /// and a weapon's set later.
    hero_packs: Vec<Vec<u8>>,
    /// The weapon set the hero's `be` pack currently is (0 bare-handed; 1 the new game's
    /// copper sword). `set_hero_weapon` is a no-op for the set already loaded.
    hero_set: u8,
    /// The action each fighter is playing, if any: a blow, a reaction, a death or a flight.
    timelines: Vec<Option<Timeline>>,
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
        let mut spans = Vec::new();
        let mut bacts: Vec<Option<Script>> = Vec::new();
        let mut timelines: Vec<Option<Timeline>> = Vec::new();

        let hero_slot = party_slots(1)[0];
        let position = grid_position(hero_slot) + Vec3::Y * FIGHTER_HEIGHT;
        let heading = PI;
        loaded.hero = dq9_core::Hero::at(position, dq9_core::field::yaw_of_angle(heading));
        loaded.runtime.scene.instances[loaded.hero_instance].world =
            loaded.runtime.hero_world(position, heading);
        loaded.runtime.play_hero("stand", false);
        // The hero's own battle packs: `mp0200b` (reactions: `damage`, `death`, `guard`,
        // `escape`), `mp0200be` (the blows, bare-handed until the party's weapon is known) and
        // `mp0200bm` (casting). The weapon chooses the `be` set (`0x02072c9c`).
        let mut hero_archives = Vec::new();
        for pack in ["mp0200b.chr", "mp0200be.chr", "mp0200bm.chr"] {
            match motion::hero_pack(rom, pack) {
                Ok(Some(bytes)) => {
                    if let Err(e) = loaded.runtime.add_hero_package(&bytes) {
                        crate::app::status(&format!("battle motion: hero {pack}: {e:#}"));
                    }
                    hero_archives.push(bytes);
                }
                Ok(None) => {
                    crate::app::status(&format!("battle motion: the ROM has no {pack}"));
                }
                Err(e) => crate::app::status(&format!("battle motion: hero {pack}: {e:#}")),
            }
        }
        // The new game's hero carries the copper sword: `mp0201`, swords (item 20004's set in
        // `itemdt_w`). A fight of the application replaces it with the party's weapon
        // (`set_hero_weapon`); a capture of a new-game battle keeps this one.
        let default_pack = motion::attack_pack(motion::NEW_GAME_WEAPON_SET);
        if let Ok(Some(bytes)) = motion::hero_pack(rom, &default_pack) {
            if let Err(e) = loaded.runtime.add_hero_package(&bytes) {
                crate::app::status(&format!("battle motion: hero {default_pack}: {e:#}"));
            }
            hero_archives.push(bytes);
        }
        crate::app::status(&format!(
            "battle motion: hero set {} ({default_pack}), the new game's copper sword, until the \
             party's weapon is set",
            motion::NEW_GAME_WEAPON_SET
        ));
        let hero_spans = motion::motion_spans(
            &hero_archives
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<&[u8]>>(),
        );
        fighters.push(PlacedFighter {
            fighter: Fighter::party(0),
            number: None,
            kind: None,
            position,
            heading,
            alive: true,
        });
        instances.push(loaded.hero_instance);
        actors.push(None);
        heights.push(1.5);
        // The hero's battle body radius: the ROM's objects take it from the model
        // (`Object3D_SetRadius`), and the port has no hero record to read it from; 0.25 is the
        // model's radius the scene used before the monsters' came from `mon_data`
        // **[hypothesis]**.
        widths.push(0.25);
        spans.push(hero_spans);
        bacts.push(None);
        timelines.push(None);

        let flat = flatten(&resolved.monsters);
        if flat.len() > MONSTER_SLOTS {
            crate::app::status(&format!(
                "battle: {} monsters, the battle's slots hold {MONSTER_SLOTS}; the rest have \
                 no place (`0x02183118`, `func_ov000_0215eb1c`)",
                flat.len()
            ));
        }
        let slots = monster_slots(flat.len());
        for (rank, (&number, slot)) in flat.iter().zip(&slots).enumerate() {
            let record = names
                .find(number)
                .with_context(|| format!("monster {number} has no mon_data record"))?;
            let code = String::from_utf8_lossy(record.code);
            // The battle body's `.cchr` holds only `appear`, `attack0a`, `run` and `stand`: the
            // blows, reactions and death are the `.cmot` beside it, and the action script the
            // `.bact` (wiki *Monsters*, "Monster models").
            let archive = motion::monster_members(rom, &code)
                .with_context(|| format!("monster {number} ({code}) battle archive"))?;
            let (chr, cmot, bact) = (&archive.body, &archive.motions, &archive.script);
            let packages = cmot.iter().map(Vec::as_slice).collect::<Vec<&[u8]>>();
            let actor = loaded
                .runtime
                .add_actor(chr, &packages)
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
            let archives = [chr.as_slice(), cmot.as_deref().unwrap_or(&[])];
            fighters.push(PlacedFighter {
                fighter: Fighter::monster(rank),
                number: Some(number),
                kind: Some(record.kind),
                position,
                heading,
                alive: true,
            });
            instances.push(instance);
            actors.push(Some(actor));
            heights.push((f32::from(record.height) / 4096.0).max(0.5));
            // The row's width: the body's radius ×4 (`+0x0C`, 1024ths).
            widths.push((f32::from(record.radius) / 1024.0).max(0.05));
            spans.push(motion::motion_spans(&archives));
            bacts.push(
                bact.as_deref()
                    .and_then(|bytes| match Script::parse(bytes) {
                        Ok(script) => Some(script),
                        Err(e) => {
                            crate::app::status(&format!(
                                "battle motion: monster {number} ({code}) script: {e:#}"
                            ));
                            None
                        }
                    }),
            );
            timelines.push(None);
        }

        let scripts = match Scripts::load(rom) {
            Ok(scripts) => scripts,
            Err(e) => {
                crate::app::status(&format!("battle motion: action scripts: {e:#}"));
                Scripts::default()
            }
        };
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
            approach: None,
            approach_phase: false,
            fighters,
            instances,
            actors,
            heights,
            widths,
            spans,
            bacts,
            scripts,
            hero_packs: hero_archives,
            hero_set: motion::NEW_GAME_WEAPON_SET,
            timelines,
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

    /// The stage's animated scene, to add the transition's swirl to (tests and captures).
    #[cfg(test)]
    pub(crate) fn runtime_mut(&mut self) -> &mut AnimatedScene {
        &mut self.runtime
    }

    /// Uploads geometry and fighter worlds together. Hidden fighters must be included:
    /// their zero scale removes a defeated/fled model from an already uploaded scene.
    pub fn upload(
        &mut self,
        renderer: &mut dq9_render::Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<()> {
        self.runtime.face_camera(&self.camera)?;
        self.runtime
            .upload(renderer, device, queue)
            .context("uploading the battle animations")?;
        for &instance in &self.instances {
            let world = self.runtime.scene.instances[instance].world;
            anyhow::ensure!(
                renderer.update_instance_world(queue, instance, world),
                "battle GPU instance {instance} is missing"
            );
        }
        Ok(())
    }

    /// One fixed step of the scene's animations at the battle camera, and of the opening's
    /// eased approach while it runs (`func_ov000_0216d464`).
    pub fn step(&mut self) -> Result<()> {
        self.step_camera();
        self.runtime
            .step(&self.camera)
            .context("advancing the battle animations")?;
        self.present();
        Ok(())
    }

    /// Gives the hero the motion set of the weapon it carries (`itemdt_w`, `0x02072c9c`):
    /// `None` is bare-handed (`mp0200`). The application calls it once the party is known;
    /// the scene starts on the new game's copper sword.
    pub fn set_hero_weapon(&mut self, rom: &[u8], weapon: Option<u16>) -> Result<()> {
        let set = weapon
            .and_then(|item| motion::weapon_set(rom, item))
            .unwrap_or(0);
        if set == self.hero_set {
            return Ok(());
        }
        let pack = motion::attack_pack(set);
        let Some(bytes) = motion::hero_pack(rom, &pack)? else {
            crate::app::status(&format!("battle motion: the ROM has no {pack}"));
            return Ok(());
        };
        self.runtime.add_hero_package(&bytes)?;
        self.hero_packs.push(bytes);
        self.hero_set = set;
        crate::app::status(&format!("battle motion: hero set {set} ({pack})"));
        self.spans[0] = motion::motion_spans(
            &self
                .hero_packs
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<&[u8]>>(),
        );
        Ok(())
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

    /// A monster that joined the fight (a call for backup): its model is added on the next
    /// free place of the grid and the monsters' places are taken again for the new count.
    /// `Fighter::monster(n)` of the next index is its fighter.
    pub fn add_monster(&mut self, rom: &[u8], number: u16) -> Result<()> {
        let files = Rom::parse(rom).context("parsing the ROM")?;
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
        let record = names
            .find(number)
            .with_context(|| format!("monster {number} has no mon_data record"))?;
        let code = String::from_utf8_lossy(record.code);
        let archive = motion::monster_members(rom, &code)
            .with_context(|| format!("monster {number} ({code}) battle archive"))?;
        let (chr, cmot, bact) = (&archive.body, &archive.motions, &archive.script);
        let packages = cmot.iter().map(Vec::as_slice).collect::<Vec<&[u8]>>();
        let actor = self
            .runtime
            .add_actor(chr, &packages)
            .with_context(|| format!("monster {number} ({code}): model"))?;
        let instance = self
            .runtime
            .actor_instance(actor)
            .with_context(|| format!("monster {number} ({code}): no polygons"))?;
        // Those called for appear (`60`): `appear`, held on its last frame, then their idle
        // when it is over is the scene's call, not the script's (a name the archive lacks is
        // a no-op in the game).
        if !self.runtime.play_actor(actor, "appear", true, 1.0) {
            self.runtime.play_actor(actor, "stand", false, 1.0);
        }
        let rank = self
            .fighters
            .iter()
            .filter(|f| f.fighter.side == Side::Monsters)
            .count();
        self.fighters.push(PlacedFighter {
            fighter: Fighter::monster(rank),
            number: Some(number),
            kind: Some(record.kind),
            position: Vec3::ZERO,
            heading: 0.0,
            alive: true,
        });
        self.instances.push(instance);
        self.actors.push(Some(actor));
        self.heights
            .push((f32::from(record.height) / 4096.0).max(0.5));
        self.widths
            .push((f32::from(record.radius) / 1024.0).max(0.05));
        let archives = [chr.as_slice(), cmot.as_deref().unwrap_or(&[])];
        self.spans.push(motion::motion_spans(&archives));
        self.bacts
            .push(bact.as_deref().and_then(|bytes| Script::parse(bytes).ok()));
        self.timelines.push(None);
        self.place();
        if self.approach.is_some() {
            self.refresh_approach();
        } else {
            self.set_shot(self.shot);
        }
        Ok(())
    }

    /// The defeat fade: the fighter is hidden and `alive` goes false. Its slot stays (the
    /// game keeps it for the death effect and the results). Called once: a model that is
    /// already gone is not hidden again.
    #[allow(dead_code)]
    pub fn remove(&mut self, fighter: Fighter) -> bool {
        let Some(index) = self.index_of(fighter) else {
            return false;
        };
        if !self.fighters[index].alive {
            return false;
        }
        if let Some(instance) = self.instances.get(index)
            && let Some(entry) = self.runtime.scene.instances.get_mut(*instance)
        {
            entry.world = Mat4::from_scale(Vec3::ZERO);
        }
        self.fighters[index].alive = false;
        true
    }

    /// Plays `action` of `actor` on `targets`: the motion the actor's script names, its lunge
    /// and the moment the blow lands, at which the `damaged` targets play their `damage`
    /// motion. A fighter that has fallen, or is falling, plays nothing.
    ///
    /// The script is the actor's own (a monster's `.bact`, the hero's `mp0200.bact`), else the
    /// action's own file (`sp%03d.bact`), else `default.bact`; nothing at all when none has a
    /// motion, as the game skips a script it cannot load. `gap` is the scripts' edge-to-edge
    /// distance: the middles less the two bodies' half-widths.
    pub fn act(&mut self, actor: Fighter, action: u16, targets: &[Fighter], damaged: &[Fighter]) {
        let Some(index) = self.index_of(actor) else {
            return;
        };
        if !self.fighters[index].alive {
            return;
        }
        if self.timelines[index]
            .as_ref()
            .is_some_and(|timeline| timeline.end == End::Hide)
        {
            // Falling: its death is playing; no later action may animate it.
            return;
        }
        let first = targets
            .iter()
            .chain(damaged)
            .find_map(|target| self.index_of(*target));
        let (target, radius, gap) = match first {
            Some(target) => {
                let width = self.widths[index];
                let reach = self.widths[target];
                let distance = (self.fighters[target].position - self.fighters[index].position)
                    .xz()
                    .length();
                (
                    self.fighters[target].position,
                    reach,
                    (distance - width - reach).max(0.0),
                )
            }
            None => (self.fighters[index].position, 0.0, 0.0),
        };
        let blow = match self.actors[index] {
            None => self
                .scripts
                .hero(action, gap)
                .or_else(|| self.scripts.special(action))
                .or_else(|| self.scripts.fallback(action, gap)),
            Some(_) => {
                let own = self.bacts[index]
                    .as_ref()
                    .and_then(|script| script.section(action))
                    .and_then(|ops| motion::plan(ops, gap));
                own.or_else(|| self.scripts.special(action))
                    .or_else(|| self.scripts.fallback(action, gap))
            }
        };
        let Some(blow) = blow else {
            crate::app::status(&format!(
                "battle motion: action {action} has no motion for {:?}",
                self.fighters[index].fighter
            ));
            return;
        };
        // The script's `magic` falls back to `magic1`, then `magic_in`, when the model lacks
        // it (`3`'s player).
        let mut blow = blow;
        if motion::span_of(&self.spans[index], &blow.motion).is_none() && blow.motion == "magic" {
            for fallback in ["magic1", "magic_in"] {
                if motion::span_of(&self.spans[index], fallback).is_some() {
                    blow.motion = fallback.to_owned();
                    break;
                }
            }
        }
        let ticks = match motion::span_of(&self.spans[index], &blow.motion) {
            Some(ticks) => ticks,
            None => {
                crate::app::status(&format!(
                    "battle motion: {:?} has no motion {:?}",
                    self.fighters[index].fighter, blow.motion
                ));
                motion::FALLBACK_MOTION_TICKS
            }
        };
        // The previous gesture of this fighter ends first: settling it puts the fighter back
        // on its place and plays its idle, which must not replace the motion started below.
        if let Some(old) = self.timelines[index].take() {
            self.settle(index, &old);
        }
        let played = match self.actors[index] {
            None => self.runtime.play_hero(&blow.motion, true),
            Some(actor) => self.runtime.play_actor(actor, &blow.motion, true, 1.0),
        };
        if !played && self.actors[index].is_some() {
            crate::app::status(&format!(
                "battle motion: {:?} cannot play {:?}",
                self.fighters[index].fighter, blow.motion
            ));
        }
        let heading = self.fighters[index].heading;
        let mut timeline = Timeline::blow(
            &blow,
            ticks,
            self.fighters[index].position,
            heading,
            self.widths[index],
            target,
            radius,
        );
        timeline.damaged = damaged
            .iter()
            .filter_map(|target| self.index_of(*target))
            .filter(|&target| self.fighters[target].alive)
            .collect();
        let playing = self
            .animation(self.fighters[index].fighter)
            .map(|(name, _)| name.to_owned());
        crate::app::status(&format!(
            "battle motion: {:?} {:?} (playing {playing:?}), {ticks} ticks, hit {}%, lunge {}, \
             gap {gap:.2}",
            self.fighters[index].fighter,
            timeline.motion,
            blow.hit * 100.0,
            timeline.lunge.is_some(),
        ));
        self.timelines[index] = Some(timeline);
    }

    /// The flight of `fighter`: the `escape` motion, then hidden. `47 7 0 500` is the script's
    /// alpha fade, which the scene shows as the model going away once its motion is over.
    pub fn flee(&mut self, fighter: Fighter) {
        let Some(index) = self.index_of(fighter) else {
            return;
        };
        if !self.fighters[index].alive {
            return;
        }
        if self.timelines[index]
            .as_ref()
            .is_some_and(|timeline| timeline.end == End::Hide)
        {
            return;
        }
        if let Some(old) = self.timelines[index].take() {
            self.settle(index, &old);
        }
        let Some(ticks) = motion::span_of(&self.spans[index], "escape") else {
            self.remove(fighter);
            return;
        };
        match self.actors[index] {
            None => {
                self.runtime.play_hero("escape", true);
            }
            Some(actor) => {
                self.runtime.play_actor(actor, "escape", true, 1.0);
            }
        }
        let heading = self.fighters[index].heading;
        self.timelines[index] = Some(Timeline::reaction(
            "escape".to_owned(),
            End::Hide,
            ticks,
            self.fighters[index].position,
            heading,
        ));
    }

    /// The defeat fade of `fighter`: the `death` motion, then hidden once. A model without a
    /// death motion goes at once, as the game's own skip does.
    pub fn kill(&mut self, fighter: Fighter) {
        let Some(index) = self.index_of(fighter) else {
            return;
        };
        if !self.fighters[index].alive {
            return;
        }
        if self.timelines[index]
            .as_ref()
            .is_some_and(|timeline| timeline.end == End::Hide)
        {
            return;
        }
        if let Some(old) = self.timelines[index].take() {
            self.settle(index, &old);
        }
        let Some(ticks) = motion::span_of(&self.spans[index], "death") else {
            self.remove(fighter);
            return;
        };
        match self.actors[index] {
            None => {
                self.runtime.play_hero("death", true);
            }
            Some(actor) => {
                self.runtime.play_actor(actor, "death", true, 1.0);
            }
        }
        let heading = self.fighters[index].heading;
        self.timelines[index] = Some(Timeline::reaction(
            "death".to_owned(),
            End::Hide,
            ticks,
            self.fighters[index].position,
            heading,
        ));
    }

    /// A monster `caller` called `number` for backup: the caller's `call` motion, the monster
    /// added to the scene playing `appear` (the default script's `347`/`242`).
    pub fn summon(&mut self, rom: &[u8], caller: Fighter, number: u16) -> Result<()> {
        self.add_monster(rom, number)?;
        let Some(index) = self.index_of(caller) else {
            return Ok(());
        };
        if let Some(old) = self.timelines[index].take() {
            self.settle(index, &old);
        }
        let Some(ticks) = motion::span_of(&self.spans[index], "call") else {
            return Ok(());
        };
        if let Some(actor) = self.actors[index] {
            self.runtime.play_actor(actor, "call", true, 1.0);
        }
        let heading = self.fighters[index].heading;
        self.timelines[index] = Some(Timeline::reaction(
            "call".to_owned(),
            End::Idle,
            ticks,
            self.fighters[index].position,
            heading,
        ));
        Ok(())
    }

    /// Where `fighter` is drawn now: its blow's lunge, or its place. The native report and the
    /// tests read it; nothing in the window does.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn drawn_position(&self, fighter: Fighter) -> Option<Vec3> {
        let index = self.index_of(fighter)?;
        if !self.fighters[index].alive {
            return None;
        }
        match &self.timelines[index] {
            Some(timeline) if timeline.end != End::Hide => Some(timeline.position()),
            _ => Some(self.fighters[index].position),
        }
    }

    /// The motion `fighter` is playing and how far, for the report and the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn gesture(&self, fighter: Fighter) -> Option<(&str, u32)> {
        let index = self.index_of(fighter)?;
        let timeline = self.timelines[index].as_ref()?;
        Some((timeline.motion.as_str(), timeline.age))
    }

    /// The motion `fighter`'s model is actually playing, and its frame: what the renderer
    /// holds, which a timeline alone does not prove (a settled idle replaces a clip).
    pub fn animation(&self, fighter: Fighter) -> Option<(&str, i32)> {
        let index = self.index_of(fighter)?;
        match self.actors[index] {
            None => self.runtime.hero_animation(),
            Some(actor) => self.runtime.actor_animation(actor),
        }
    }

    /// Whether `fighter` is playing its death or its flight (it is hidden when it ends).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn falling(&self, fighter: Fighter) -> bool {
        self.index_of(fighter)
            .and_then(|index| self.timelines[index].as_ref())
            .is_some_and(|timeline| timeline.end == End::Hide)
    }

    /// Whether the scene is still presenting something: a blow before its landing, a
    /// reaction, a death or a flight. The windows wait for it, as the game's reaction queue
    /// does (`67`: no target dying, no effect playing).
    pub fn presenting(&self) -> bool {
        self.timelines.iter().any(Option::is_some)
    }

    /// Whether a blow is still on its way to its landing point.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pending_hit(&self) -> bool {
        self.timelines
            .iter()
            .flatten()
            .any(|timeline| !timeline.reacted && timeline.hit != u32::MAX)
    }

    /// Ends every presentation at once: every fighter back at its place or hidden, nothing
    /// left playing. The end of a fight and the return to the field.
    pub fn clear_gestures(&mut self) {
        for index in 0..self.timelines.len() {
            if let Some(timeline) = self.timelines[index].take() {
                self.settle(index, &timeline);
            }
        }
    }

    /// One step of the presentation: the motion clock, the blow's lunge and hit, the end of a
    /// reaction, a death or a flight.
    fn present(&mut self) {
        for index in 0..self.timelines.len() {
            let Some(mut timeline) = self.timelines[index].take() else {
                continue;
            };
            let (hit, done) = timeline.advance();
            if hit {
                for &target in &timeline.damaged {
                    self.react(target);
                }
            }
            if done {
                self.settle(index, &timeline);
            } else {
                if timeline.end == End::Restore {
                    let (position, face) = (timeline.position(), timeline.face);
                    self.place_world(index, position, face);
                }
                self.timelines[index] = Some(timeline);
            }
        }
    }

    /// A `damage` reaction of `index`, unless it is already playing something of its own.
    fn react(&mut self, index: usize) {
        if !self.fighters[index].alive || self.timelines[index].is_some() {
            return;
        }
        let Some(ticks) = motion::span_of(&self.spans[index], "damage") else {
            return;
        };
        match self.actors[index] {
            None => {
                self.runtime.play_hero("damage", true);
            }
            Some(actor) => {
                self.runtime.play_actor(actor, "damage", true, 1.0);
            }
        }
        let heading = self.fighters[index].heading;
        self.timelines[index] = Some(Timeline::reaction(
            "damage".to_owned(),
            End::Idle,
            ticks,
            self.fighters[index].position,
            heading,
        ));
    }

    /// What a finished timeline leaves: the fighter at its place and idle, or hidden.
    fn settle(&mut self, index: usize, timeline: &Timeline) {
        match timeline.end {
            End::Hide => {
                let fighter = self.fighters[index].fighter;
                if self.remove(fighter) {
                    crate::app::status(&format!(
                        "battle motion: {fighter:?} {:?} ended, hidden",
                        timeline.motion
                    ));
                }
            }
            End::Restore | End::Idle => {
                if !self.fighters[index].alive {
                    return;
                }
                let (position, heading) =
                    (self.fighters[index].position, self.fighters[index].heading);
                self.place_world(index, position, heading);
                match self.actors[index] {
                    None => {
                        self.runtime.play_hero("stand", false);
                    }
                    Some(actor) => {
                        self.runtime.play_actor(actor, "stand", false, 1.0);
                    }
                }
                if timeline.end == End::Restore {
                    crate::app::status(&format!(
                        "battle motion: {:?} back at ({:.3}, {:.3})",
                        self.fighters[index].fighter, position.x, position.z
                    ));
                }
            }
        }
    }

    /// Writes a fighter's drawn place (`hero_world`, `monster_world`).
    fn place_world(&mut self, index: usize, position: Vec3, heading: f32) {
        let Some(instance) = self.instances.get(index).copied() else {
            return;
        };
        let world = match self.actors[index] {
            None => self.runtime.hero_world(position, heading),
            Some(_) => monster_world(position, heading),
        };
        if let Some(entry) = self.runtime.scene.instances.get_mut(instance) {
            entry.world = world;
        }
    }

    /// Frames `shot` and clamps the eye (`ov_000::0216d600`, `ov_000::0216f2b8`), starting the
    /// opening's eased approach on [`Shot::Opening`] (`ov_000::0216118c`).
    pub fn set_shot(&mut self, shot: Shot) {
        self.shot = shot;
        let (eye, target, half) = self.shot_placement(shot);
        self.camera.look_at(eye, target);
        self.camera.fovy = 2.0 * half.to_radians();
        self.approach = None;
        if shot == Shot::Opening {
            self.start_opening();
        } else {
            self.clamp_camera();
        }
    }

    /// The eye, target and half-angle `func_ov000_0216d600` computes for `shot`, before its
    /// eye clamp: the framed side's extents, the 2.2/1.5 divisions, the 2.5 less and the floor,
    /// the target's height `depth/2` (at least 1.2) with the eye held to 2, and the kind's own
    /// shot when the table has one.
    fn shot_placement(&self, shot: Shot) -> (Vec3, Vec3, f32) {
        let (framed, wide) = match shot {
            Shot::Opening => (Side::Monsters, true),
            Shot::Side(side) => (side, false),
        };
        let (half, sin, cos) = if wide {
            (WIDE_HALF_ANGLE, SIN_WIDE, COS_WIDE)
        } else {
            (HALF_ANGLE, SIN_NARROW, COS_NARROW)
        };
        let (width, depth) = self.side_extents(framed);
        let distance = shot_distance(width, depth, sin, cos, wide);
        let height = (depth / 2).max(0x1333);
        let target = Vec3::new(0.0, fx12(height), 0.0);
        let eye = Vec3::new(0.0, fx12(height.min(0x2000)), fx12(distance));
        let sign = framed.camera_sign();
        // A monster kind with a shot of its own (`data_ov026_021de87c` through
        // `func_ov026_021d8aac`, applied by `param_8` of `func_ov000_0216d600`): the table
        // gives the eye and the target outright, in the side-1 convention the sign flips. Only
        // the opening passes `param_8` (the camera commands of `ov_025::021e3c80` pass 0).
        let kind = if shot == Shot::Opening {
            self.kind_shot()
        } else {
            None
        };
        if let Some((kind_eye, kind_target)) = kind {
            let target = Vec3::new(kind_target.x, kind_target.y, kind_target.z * sign);
            let eye = Vec3::new(kind_eye.x, kind_eye.y, kind_eye.z * sign);
            (eye, target, half)
        } else {
            (Vec3::new(eye.x, eye.y, eye.z * sign), target, half)
        }
    }

    /// Re-frames a running opening after the set changed (a call for backup widens the row and
    /// raises the camera's extents, `ov_000::021677fc`), without restarting its easing.
    fn refresh_approach(&mut self) {
        let (eye, target, _) = self.shot_placement(self.shot);
        if let Some(approach) = &mut self.approach {
            approach.target = framing(target, eye);
        }
    }

    /// The opening's first frame (`func_ov000_0216118c`): the side shot is framed, then the eye
    /// is read off the clamped camera as spherical parameters, pulled back by
    /// [`INTRO_PITCH`]/[`INTRO_DISTANCE`], and eased back over the following steps
    /// (`func_ov000_0216f0d0`, gain [`INTRO_GAIN`] per step, [`INTRO_SNAP`] to settle).
    fn start_opening(&mut self) {
        let target = self.camera.target;
        let settle = framing(target, self.camera.eye());
        let mut pulled = settle;
        pulled.pitch += INTRO_PITCH;
        pulled.distance += INTRO_DISTANCE;
        let eye = place(target, pulled);
        // `func_ov000_0216f2b8` runs on the same frame as the set-up and clamps the eye.
        let eye = clamp_eye(eye);
        self.camera.look_at(eye, target);
        self.approach = Some(Approach {
            current: framing(target, eye),
            target: settle,
        });
    }

    /// One fixed step of the opening's easing (`func_ov000_0216f0d0` then
    /// `func_ov000_0216f2b8`, in that order in `func_ov000_0216d464`), on the battle's 30 Hz
    /// update: the ease moves on every second 60 Hz fixed step.
    fn step_camera(&mut self) {
        if self.approach.is_none() {
            return;
        }
        self.approach_phase = !self.approach_phase;
        if self.approach_phase {
            return;
        }
        let Some(mut approach) = self.approach else {
            return;
        };
        let target = self.camera.target;
        let (mut eye, settled) = approach_step(&mut approach, target);
        let clamped = clamp_eye(eye);
        if clamped != eye {
            // `func_0202eab8` rewrites the spherical parameters from the clamped eye.
            approach.current = framing(target, clamped);
            eye = clamped;
        }
        self.camera.look_at(eye, target);
        self.approach = (!settled).then_some(approach);
    }

    /// The camera's own clamp after a command (`func_ov000_0216f2b8`): 17 from the stage's
    /// centre in the ground plane, 5 above it.
    fn clamp_camera(&mut self) {
        let eye = clamp_eye(self.camera.eye());
        self.camera.look_at(eye, self.camera.target);
    }

    /// Asks for the row formation (`ov_000::02167e6c`), or for the grid the battle starts on
    /// (`ov_000::02167dd8`). A few action scripts are its callers in the game; no action is
    /// played in this batch, so it is the next wave's entry point.
    #[allow(dead_code)]
    pub fn set_formation(&mut self, formation: Formation) {
        if self.formation == formation {
            return;
        }
        self.formation = formation;
        self.place();
        if self.approach.is_some() {
            self.refresh_approach();
        } else {
            self.set_shot(self.shot);
        }
    }

    /// Puts the fighters where the formation says and refreshes their drawn transforms. A
    /// fighter the game's tables have no place for (a ninth monster, a fifth party member)
    /// keeps the position the scene built it at: the original's tables hold four and eight
    /// (`0x02183108`, `0x02183118`) and its loops stop there.
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
        let kinds: Vec<Option<u16>> = self.fighters[party_count..]
            .iter()
            .map(|placed| placed.number)
            .collect();
        for index in 0..self.fighters.len() {
            let fighter = self.fighters[index].fighter;
            let placed = match (self.formation, fighter.side) {
                (Formation::Grid, Side::Party) => party_slots
                    .get(fighter.index)
                    .map(|slot| (grid_position(*slot), PI)),
                (Formation::Grid, Side::Monsters) => monster_slots
                    .get(fighter.index)
                    .map(|slot| (grid_position(*slot), 0.0)),
                (Formation::Row, Side::Party) => Some(row_party(fighter.index, party_count)),
                (Formation::Row, Side::Monsters) => row_monster(&widths, &kinds, fighter.index),
            };
            let Some((position, heading)) = placed else {
                continue;
            };
            let position = position + Vec3::Y * FIGHTER_HEIGHT;
            self.fighters[index].position = position;
            self.fighters[index].heading = heading;
            if !self.fighters[index].alive {
                // A fallen fighter stays hidden (`remove`).
                continue;
            }
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

    /// The width and depth of one side, on the fx12 grid, as `func_ov000::021675a0` (party) and
    /// `func_ov000::021677fc` (monsters) store them at `camera +0x228`/`+0x22c` and
    /// `+0x230`/`+0x234`: the party's row width `0x1000` plus `0x1800` per extra member and a
    /// depth of `0x1800`; the monsters' row width (each body's radius, narrowed for the five
    /// kinds when more than one fights) and the tallest body plus `0x800`.
    ///
    /// The original computes these from the row even when the battle stands on the grid
    /// (`ov_000::02164d74` calls `021677fc` before `02167cd4`), so the framing does not follow
    /// the drawn places.
    fn side_extents(&self, side: Side) -> (i32, i32) {
        match side {
            Side::Party => {
                let count = self
                    .fighters
                    .iter()
                    .filter(|f| f.fighter.side == Side::Party)
                    .count()
                    .clamp(1, 4);
                #[allow(clippy::cast_possible_wrap)]
                let width = 0x1000 + (count as i32 - 1) * 0x1800;
                (width, 0x1800)
            }
            Side::Monsters => {
                let count = self
                    .fighters
                    .iter()
                    .filter(|f| f.fighter.side == Side::Monsters)
                    .count()
                    .min(MONSTER_SLOTS);
                if count == 0 {
                    // The camera object's own defaults (`func_ov000_0216d2d0`).
                    return (0x1000, 0x1000);
                }
                let party = self.fighters.len()
                    - self
                        .fighters
                        .iter()
                        .filter(|f| f.fighter.side == Side::Monsters)
                        .count();
                let mut total = 0;
                let mut tallest = 0;
                for index in 0..count {
                    let number = self.fighters[party + index].number;
                    let body = to_fx12(self.widths[party + index]);
                    total += (body * narrow_factor(number, count) + 0x800) >> 12;
                    tallest = tallest.max(to_fx12(self.heights[party + index]));
                }
                // `func_ov000_021677fc` adds the row's gap once per interval to the width it
                // stores at `+0x230` (the camera frames the row, gaps included).
                (
                    total + row_gap(total, count) * (count as i32 - 1),
                    tallest + 0x800,
                )
            }
        }
    }

    /// The shot of the first monster kind the table at `data_ov026_021de87c` names
    /// (`func_ov026_021d8aac`: the records in order, the first kind present wins). `None`
    /// without a named kind: the framing is then the calculated side shot.
    ///
    /// The table's `0x115` record has three variants chosen by `func_ov026_021d8aac`'s second
    /// argument (`battle +0x8e20` modulo three, a battle counter the port does not carry): the
    /// first is taken. Its other default, the `kind 0` record applied when
    /// `func_ov000_0215fc60(battle)` answers 2, needs the battle record's own type and is not
    /// modelled.
    fn kind_shot(&self) -> Option<(Vec3, Vec3)> {
        let mut kinds: Vec<u16> = self
            .fighters
            .iter()
            .filter(|f| f.fighter.side == Side::Monsters)
            .filter_map(|f| f.kind)
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        shots::kind_shot(&kinds).map(|(eye, target)| {
            (
                Vec3::new(fx12(eye[0]), fx12(eye[1]), fx12(eye[2])),
                Vec3::new(fx12(target[0]), fx12(target[1]), fx12(target[2])),
            )
        })
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

/// One 60 Hz step of a fight on its scene: the window (the pad and its cues), unless the scene
/// is still presenting the previous command. While a blow has not landed, a reaction, a death
/// or a flight is playing, the windows do not advance and `pad` is dropped, as the game ignores
/// input during an action. The generator and the round are untouched: only the narrative's
/// clock waits. Returns `(advanced, joined)`, `joined` when a call for backup added a model.
pub fn advance_fight(
    fight: &mut fight::Fight,
    scene: &mut BattleScene,
    world: &mut fight::World,
    pad: fight::Pad,
    rom: Option<&[u8]>,
) -> (bool, bool) {
    if scene.presenting() {
        return (false, false);
    }
    fight.step(pad, world);
    let mut joined = false;
    for cue in fight.take_cues() {
        joined |= apply_cue(scene, cue, rom);
    }
    (true, joined)
}

/// Plays one cue on the scene: the action's motion and lunge, the target's reaction, the
/// death fade and a flight, and the model a call for backup adds. Returns whether the scene
/// gained a model (its renderer then has to be built again).
pub fn apply_cue(scene: &mut BattleScene, cue: narrate::Cue, rom: Option<&[u8]>) -> bool {
    match cue {
        narrate::Cue::Remove(fighter) => scene.kill(fighter),
        narrate::Cue::Escape(fighter) => scene.flee(fighter),
        narrate::Cue::Act {
            actor,
            action,
            targets,
            damaged,
        } => scene.act(actor, action, &targets, &damaged),
        narrate::Cue::Summon { caller, number } => match rom {
            Some(rom) => match scene.summon(rom, caller, number) {
                Ok(()) => return true,
                Err(e) => crate::app::status(&format!("battle: backup {number}: {e:#}")),
            },
            None => crate::app::status(&format!(
                "battle: backup {number} needs the ROM (this caller has none)"
            )),
        },
        narrate::Cue::Hp(..) | narrate::Cue::Mp(..) | narrate::Cue::Popup { .. } => {}
    }
    false
}

/// One step of the opening's easing (`func_ov000_0216f0d0`): the framing keeps `0xf33` of each
/// difference (moves `0xcd` of it, rounded on the fx12 grid) and snaps to the target once every
/// difference is under [`INTRO_SNAP`]. Returns the eye it places and whether it settled.
fn approach_step(approach: &mut Approach, target: Vec3) -> (Vec3, bool) {
    let d_yaw = signed_angle(approach.current.yaw, approach.target.yaw);
    let d_pitch = approach.target.pitch - approach.current.pitch;
    let d_distance = approach.target.distance - approach.current.distance;
    let settled =
        d_yaw.abs() < INTRO_SNAP && d_pitch.abs() < INTRO_SNAP && d_distance.abs() < INTRO_SNAP;
    if settled {
        approach.current = approach.target;
    } else {
        approach.current.yaw = reduce_angle(approach.current.yaw + fix_mul(d_yaw, INTRO_GAIN));
        approach.current.pitch += fix_mul(d_pitch, INTRO_GAIN);
        approach.current.distance += fix_mul(d_distance, INTRO_GAIN);
    }
    (place(target, approach.current), settled)
}

/// The camera's eye clamp (`ov_000::0216f2b8`): at most [`EYE_REACH`] from the stage's centre in
/// the ground plane (the original normalises `(eye.x, 0, eye.z)`, so the origin, not the
/// target), and at most [`EYE_HEIGHT`] above the stage.
fn clamp_eye(eye: Vec3) -> Vec3 {
    let mut eye = eye;
    let flat = Vec2::new(eye.x, eye.z);
    let reach = flat.length();
    if reach > EYE_REACH {
        let scale = EYE_REACH / reach;
        eye.x *= scale;
        eye.z *= scale;
    }
    eye.y = eye.y.min(EYE_HEIGHT);
    eye
}

/// An fx12 value as world units.
fn fx12(value: i32) -> f32 {
    value as f32 / 4096.0
}

/// A world length on the game's fx12 grid, rounded (`func_ov000_021677fc`'s `+ 0x800 >> 12`).
fn to_fx12(value: f32) -> i32 {
    (value * 4096.0).round() as i32
}

/// `(a × b + 0x800) >> 12`: the original's fx12 multiply-and-round
/// (`func_ov000_0216f0d0`, `func_ov000_021677fc`).
fn fix_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b) + 0x800) >> 12) as i32
}

/// The angle from `from` to `to` on the short way round, in fx12 (`fix32SignedAngleDistance`).
fn signed_angle(from: i32, to: i32) -> i32 {
    let mut difference = (to - from) % ANGLE_TURN;
    if difference > ANGLE_HALF_TURN {
        difference -= ANGLE_TURN;
    } else if difference < -ANGLE_HALF_TURN {
        difference += ANGLE_TURN;
    }
    difference
}

/// An angle reduced to `0..2π` in fx12 (`fix32ReduceAngle0To2Pi`).
fn reduce_angle(angle: i32) -> i32 {
    ((angle % ANGLE_TURN) + ANGLE_TURN) % ANGLE_TURN
}

/// The camera's spherical parameters of `eye` around `target` (`func_0202eab8`): the yaw, the
/// eye's height above the target, and its distance, both on the fx12 grid.
fn framing(target: Vec3, eye: Vec3) -> Framing {
    let offset = eye - target;
    let yaw = reduce_angle(to_fx12(offset.x.atan2(offset.z)));
    Framing {
        yaw,
        pitch: to_fx12(offset.y),
        distance: to_fx12(offset.length()),
    }
}

/// The eye a framing places around `target` (`func_0202e5d8`): `target` plus the horizontal
/// reach `√(distance² − pitch²)` turned by the yaw.
fn place(target: Vec3, framing: Framing) -> Vec3 {
    let pitch = i64::from(framing.pitch);
    let distance = i64::from(framing.distance);
    let flat = ((distance * distance - pitch * pitch).max(0) as f64).sqrt();
    let yaw = fx12(framing.yaw);
    Vec3::new(
        target.x + yaw.sin() * flat as f32 / 4096.0,
        target.y + fx12(framing.pitch),
        target.z + yaw.cos() * flat as f32 / 4096.0,
    )
}

/// The camera's distance for one side (`func_ov000_0216d600`): the larger of the width's and
/// the depth's fx12 divisions by `2.2 sin` and `1.5 sin`, less 2.5, floored at 3.0 (wide) or
/// 6.5. `sin`/`cos` are the half-angle's, on the fx12 grid, and the divisors are
/// `_ffix(2.2 × sin)`/`_ffix(1.5 × sin)` as the original converts them.
fn shot_distance(width: i32, depth: i32, sin: i32, cos: i32, wide: bool) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let divisor_width = (2.2 * f64::from(sin)) as i32;
    #[allow(clippy::cast_possible_truncation)]
    let divisor_depth = (1.5 * f64::from(sin)) as i32;
    let by_width = (i64::from((width * cos + 0x800) >> 12) << 12) / i64::from(divisor_width);
    let by_depth = (i64::from((depth * cos + 0x800) >> 12) << 12) / i64::from(divisor_depth);
    let distance = by_width.max(by_depth) as i32 - 0x2800;
    distance.max(if wide { 0x3000 } else { 0x6800 })
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

/// The monster grid table's own size: eight rows of eight bytes at `0x02183118`, and the
/// battle's combatant slots past it (`0x8de0 + slot × 4`) hold eight too
/// (`func_ov000_0215eb1c(battle, slots, 8, 0)`). There is no ninth place.
pub const MONSTER_SLOTS: usize = 8;

/// The party's grid slots by its size, from the table at `0x02183108` (wiki *Battle-Stages*).
/// The table is four byte rows; a fifth member has no place (the party holds four).
///
/// `func_ov000_021675a0` forces the count to one when `func_ov000_0215fc60(battle)` answers
/// (the battle's own creature at `+0x81fe` is 800 or 801), a battle type the port does not
/// carry: the party's width always takes the real member count.
pub fn party_slots(count: usize) -> Vec<usize> {
    const TABLE: [&[usize]; 4] = [&[58], &[57, 59], &[48, 58, 50], &[56, 66, 67, 60]];
    let count = count.clamp(1, TABLE.len());
    TABLE[count - 1].to_vec()
}

/// The monsters' grid slots by their size, from the table at `0x02183118`: eight byte rows of
/// eight (`func_ov000_02167cd4` reads `0x02183118 + (count - 1) × 8`, its count capped at 8).
/// A set with more than eight gets the eight rows: the game has no place for a ninth.
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
    let count = count.clamp(1, TABLE.len());
    TABLE[count - 1].to_vec()
}

/// The turn of each party member in the row, in fx12 radians: `func_ov000_021675a0` reads the
/// row of `iRam0216769c + (count - 1) * 0x10` at `0x02183158` (four `int`s per party size). A
/// one-hero party turns to `0x323d` (the ROM's own π, 0.0017 rad below a float π); the ends of
/// a larger row reach ±`0x333`/4096 = ±0.2 (`0x02183158`, read on the EU ROM).
const PARTY_ROW_TURNS: [&[i32]; 4] = [
    &[0x323d],
    &[0x335c, 0x311e],
    &[0x3570, 0x323d, 0x2f0a],
    &[0x3570, 0x335c, 0x311e, 0x2f0a],
];

/// The row's `x` spacing (`(count - 1) * 0x1800` fx12) and its `z` (`0x2800`, +2.5).
const ROW_SPACING: f32 = 0x1800 as f32 / 4096.0;
const ROW_PARTY_Z: f32 = 0x2800 as f32 / 4096.0;
const ROW_MONSTER_Z: f32 = -0x2800 as f32 / 4096.0;

/// The five monster numbers the row narrows when more than one fights
/// (`func_ov000_021681f8`, whose `param_1` is the combatant's number at `+2`): `0xbd`, `0xbe`,
/// `0xbf`, `0x110` and the literal it compares against (`0x155`) — the drackals and Rover, the
/// wide bodies. Every other number keeps its full width (`0x1000`).
const NARROWED_NUMBERS: [u16; 5] = [0xbd, 0xbe, 0xbf, 0x110, 0x155];

/// The narrowed width, `0xb33`/`0x1000` (`func_ov000_021681f8`'s own return).
const NARROWED_WIDTH: i32 = 0xb33;

/// The row's gap (`func_ov000_02167b5c`): at least `0x199` and at most `0xb33` fx12, chosen so
/// that the row spans `0x4000` plus one minimum gap per interval when it fits.
const ROW_GAP_MIN: i32 = 0x199;
const ROW_GAP_MAX: i32 = 0xb33;
const ROW_TARGET: i32 = 0x4000;

/// The factor `func_ov000_021681f8` gives a body of monster `number` in a row of `count`
/// monsters, in fx12: `0xb33` for the five wide numbers, `0x1000` otherwise, and `0x1000` for
/// every one of them when it fights alone.
fn narrow_factor(number: Option<u16>, count: usize) -> i32 {
    if count > 1 && number.is_some_and(|number| NARROWED_NUMBERS.contains(&number)) {
        NARROWED_WIDTH
    } else {
        0x1000
    }
}

/// A body's fx12 row width: `(radius × factor + 0x800) >> 12` (`func_ov000_021677fc`).
fn row_width(width: f32, number: Option<u16>, count: usize) -> i32 {
    let radius = to_fx12(width).max(0);
    (radius * narrow_factor(number, count) + 0x800) >> 12
}

/// The row's gap, `func_ov000_02167b5c`: `0x4000` plus `0x199` per interval, spread over the
/// intervals when the bodies leave room, else `0x199` or `0xb33`.
fn row_gap(total: i32, count: usize) -> i32 {
    let intervals = count as i32 - 1;
    if intervals < 1 {
        return 0;
    }
    let target = intervals * ROW_GAP_MIN + ROW_TARGET;
    if target > intervals * ROW_GAP_MAX + total {
        ROW_GAP_MAX
    } else if target < total {
        ROW_GAP_MIN
    } else {
        (target - total) / intervals
    }
}

/// The row the party stands in: `z` +2.5, [`ROW_SPACING`] apart and centred, turned by
/// [`PARTY_ROW_TURNS`] (`func_ov000_021675a0`).
///
/// The original walks the party from `+(count−1)·0x1800/2` down by `0x1800` and reads the turn
/// table forward, so member 0 (the hero) is the **+x** end and takes the row's first turn (for
/// four members `0x3570` = π + 0x333/4096): both ends turn inward. The party holds four; a
/// fifth has no turn in the table and keeps a straight heading.
fn row_party(index: usize, count: usize) -> (Vec3, f32) {
    let x = (count.saturating_sub(1) as f32) * ROW_SPACING * 0.5 - index as f32 * ROW_SPACING;
    let turn = PARTY_ROW_TURNS
        .get(count.saturating_sub(1))
        .and_then(|row| row.get(index))
        .map_or(0.0, |angle| *angle as f32 / 4096.0);
    (Vec3::new(x, 0.0, ROW_PARTY_Z), turn)
}

/// The row the monsters stand in: `z` −2.5, facing 0, side by side by their radii (the five
/// wide numbers at [`NARROWED_WIDTH`]), with the gap of `func_ov000_02167b5c`. `numbers` is
/// each monster's number, in `widths` order (`func_ov000_021677fc`, which reads the
/// combatant's own number at `+2`). The original walks the battle's eight combatant slots, so
/// a ninth monster has no place and gets `None`.
fn row_monster(widths: &[f32], numbers: &[Option<u16>], index: usize) -> Option<(Vec3, f32)> {
    let count = widths.len().min(MONSTER_SLOTS);
    if index >= count {
        return None;
    }
    let widths: Vec<i32> = widths[..count]
        .iter()
        .enumerate()
        .map(|(i, width)| row_width(*width, numbers.get(i).copied().flatten(), count))
        .collect();
    let total: i32 = widths.iter().sum();
    let intervals = count as i32 - 1;
    let gap = row_gap(total, count);
    let mut x = -(total + gap * intervals) / 2;
    for (i, width) in widths.iter().enumerate() {
        // The half the original places with is the integer half of the fx12 width.
        let half = width / 2;
        x += half;
        if i == index {
            return Some((Vec3::new(fx12(x), 0.0, ROW_MONSTER_Z), 0.0));
        }
        x += half + gap;
    }
    None
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
