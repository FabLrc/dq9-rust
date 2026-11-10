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
pub mod fight;
pub mod motion;
pub mod narrate;
#[cfg(test)]
mod tests;
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
            position,
            heading,
            alive: true,
        });
        instances.push(loaded.hero_instance);
        actors.push(None);
        heights.push(1.5);
        widths.push(1.0);
        spans.push(hero_spans);
        bacts.push(None);
        timelines.push(None);

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
                position,
                heading,
                alive: true,
            });
            instances.push(instance);
            actors.push(Some(actor));
            heights.push((f32::from(record.height) / 4096.0).max(0.5));
            // The row's width: the body's radius ×4 (`+0x0C`, 1024ths).
            widths.push((f32::from(record.radius) / 1024.0 * 4.0).max(0.5));
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

    /// One fixed step of the scene's animations at the battle camera.
    pub fn step(&mut self) -> Result<()> {
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
            position: Vec3::ZERO,
            heading: 0.0,
            alive: true,
        });
        self.instances.push(instance);
        self.actors.push(Some(actor));
        self.heights
            .push((f32::from(record.height) / 4096.0).max(0.5));
        self.widths
            .push((f32::from(record.radius) / 1024.0 * 4.0).max(0.5));
        let archives = [chr.as_slice(), cmot.as_deref().unwrap_or(&[])];
        self.spans.push(motion::motion_spans(&archives));
        self.bacts
            .push(bact.as_deref().and_then(|bytes| Script::parse(bytes).ok()));
        self.timelines.push(None);
        self.place();
        self.set_shot(self.shot);
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
                let width = self.widths[index] / 4.0;
                let reach = self.widths[target] / 4.0;
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
            self.widths[index] / 4.0,
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
