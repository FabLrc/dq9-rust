//! Visible roaming monsters of the field: the model instances a map's encounter graph
//! spawns, the 30 Hz step that moves them through the real collision world, and the contact
//! that hands a fight to the application (`docs/re/encounter.md`).
//!
//! The rules are `dq9_core::encounter`'s; this module is the application wiring. It keeps
//! the map's tables, spawn graph, [`Roamers`] state and one pool of field models per monster
//! number, all created at map load so that a spawn never rebuilds the scene. A headless run
//! without a window is left untouched (its recorded traces keep replaying: the roaming path
//! is opt-in, `App::set_roaming_enabled`), the window and the Web always have it.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use dq9_core::battle::BattleResult;
use dq9_core::encounter::{
    Contact, FALLBACK_STAGE, Opening, Roamers, SLOTS, StepInput, Surroundings, graph::RoamGraph,
    pick::Company, roamer::Roamer, tables::EncounterTables,
};
use dq9_core::field;
use dq9_formats::Gpc2;
use dq9_formats::battle::equipment::BattleParams;
use dq9_formats::battle::monster::Names;
use dq9_formats::item::ItemStats;
use dq9_formats::level::VOCATION_COUNT;
use dq9_formats::maplist::{self, MapList};
use dq9_formats::table::Table;
use dq9_render::animation::AnimatedScene;
use glam::{Mat4, Vec3};
use nds_rom::{Narc, Rom};

use super::App;
use crate::app::status;
use crate::battle::fight::World;

/// The motion an actor plays, chosen from the roamer's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Stand,
    Walk,
    Run,
    Appear,
}

impl Motion {
    /// The archive's motions, the model's own names first; a chapter-2 body carries only
    /// `stand`, `run`, `walk` and `appear` (`docs/app.md`, § "Battle scene").
    fn names(self) -> &'static [&'static str] {
        match self {
            Motion::Stand => &["stand", "appear", "run", "walk"],
            Motion::Walk => &["walk", "run", "stand", "appear"],
            Motion::Run => &["run", "walk", "stand", "appear"],
            Motion::Appear => &["appear", "stand", "run", "walk"],
        }
    }

    fn once(self) -> bool {
        self == Motion::Appear
    }

    /// The motion a roamer's state plays (`docs/re/encounter.md`, (c)).
    fn of(state: u8) -> Self {
        use dq9_core::encounter::roamer::state;
        match state {
            state::APPEAR => Motion::Appear,
            state::WALK => Motion::Walk,
            state::CHASE | state::DASH | state::FLEE => Motion::Run,
            _ => Motion::Stand,
        }
    }
}

/// One field model added to the scene: its number, actor and instance.
#[derive(Debug, Clone, Copy)]
struct FieldActor {
    number: u16,
    actor: usize,
    instance: usize,
}

/// The actor a roamer slot drives.
#[derive(Debug, Clone, Copy)]
struct Bound {
    number: u16,
    /// Index in [`Roaming::actors`] (the pool entry), not the scene instance.
    model: usize,
    actor: usize,
    instance: usize,
    motion: Option<Motion>,
}

/// The equipment tables a party member's battle numbers need (`Character::fighter`).
#[derive(Debug, Clone)]
struct Equipment {
    items: ItemStats,
    params: BattleParams,
}

/// The map's region classes and the scripted companies of `eventbattle.bin`, the contact's
/// [`Surroundings`].
#[derive(Debug, Default)]
struct Classes {
    /// Map id → `byte_e` (the region class at `+0xe`).
    stage: HashMap<u16, u8>,
    /// `eventbattle.bin` number → its monsters and counts.
    companies: HashMap<u8, Company>,
}

impl Surroundings for Classes {
    fn stage_class(&self, stage: u16) -> Option<u8> {
        self.stage.get(&stage).copied()
    }

    fn event_company(&self, preset: u8) -> Option<Company> {
        self.companies.get(&preset).copied()
    }
}

/// The roaming monsters of the loaded map and their field models.
#[derive(Debug, Default)]
pub(super) struct Roaming {
    /// Set for the window and the Web, and by the native behaviour tests.
    enabled: bool,
    map: Option<u16>,
    /// `encfld`/`encbtl`/`fld_mondata`, read once from the ROM.
    tables: Option<Arc<EncounterTables>>,
    /// Monster number → its `mon_data` code (`z000a`).
    codes: HashMap<u16, String>,
    classes: Classes,
    graphs: HashMap<u16, Option<RoamGraph>>,
    roamers: Option<Roamers>,
    /// One pool of free field models per monster number, plus every model added.
    pools: BTreeMap<u16, Vec<usize>>,
    actors: Vec<FieldActor>,
    slots: [Option<Bound>; SLOTS],
    /// The roamer slot the battle in play came from.
    pending: Option<usize>,
    /// Equipment stats and battle params for `Character::fighter` (loaded once).
    equipment: Option<Equipment>,
    /// Contacts started, for the tests and diagnostics.
    contacts: u32,
    /// Numbers whose model could not be added: logged once.
    missing: std::collections::HashSet<u16>,
}

impl Roaming {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            slots: std::array::from_fn(|_| None),
            ..Self::default()
        }
    }

    /// Forgets the map and every model, keeping the cached ROM tables.
    fn forget(&mut self) {
        self.map = None;
        self.roamers = None;
        self.pools.clear();
        self.actors.clear();
        self.slots = std::array::from_fn(|_| None);
        self.pending = None;
        self.contacts = 0;
        self.missing.clear();
    }

    /// Forgets the field models only: a same-map reload keeps the [`Roamers`] but not the
    /// indices of the scene that was replaced.
    fn clear_models(&mut self) {
        self.pools.clear();
        self.actors.clear();
        self.slots = std::array::from_fn(|_| None);
        self.pending = None;
        self.missing.clear();
    }

    /// A new ROM, save or new game: the tables belong to the old ROM.
    pub(super) fn reset_rom(&mut self) {
        self.forget();
        self.tables = None;
        self.codes.clear();
        self.classes = Classes::default();
        self.graphs.clear();
        self.equipment = None;
    }

    fn tables(&mut self, rom: &[u8]) -> Result<Arc<EncounterTables>> {
        if let Some(tables) = &self.tables {
            return Ok(Arc::clone(tables));
        }
        let parsed = Rom::parse(rom).context("parsing the ROM")?;
        let file = |path: &str| parsed.file_by_path(path).with_context(|| path.to_owned());
        let names_archive =
            Gpc2::parse(file("data/prm/mon_data.gp2")?).context("parsing data/prm/mon_data.gp2")?;
        let entry = names_archive
            .find("mon_data_en.nat")
            .context("mon_data_en.nat is missing")?;
        let names_bytes = names_archive
            .member(entry)
            .context("reading mon_data_en.nat")?;
        let names = Names::parse(&names_bytes).context("parsing mon_data_en.nat")?;
        for record in &names.monsters {
            self.codes.insert(
                record.number,
                String::from_utf8_lossy(record.code).into_owned(),
            );
        }
        let tables = EncounterTables::parse(
            file("data/prm/encfld.bin")?,
            file("data/prm/encbtl.bin")?,
            file("data/prm/fld_mondata.bin")?,
            &names,
        )
        .context("parsing the encounter tables")?;
        let table = Table::parse(file(maplist::PATH)?).context("parsing the map list")?;
        let list = MapList::parse(&table).context("reading the map list")?;
        let stage = list
            .records
            .iter()
            .map(|record| (record.id, record.byte_e))
            .collect();
        // `eventbattle.bin` readers for every scripted company an `encbtl` roamer can name.
        let mut companies = HashMap::new();
        for preset in tables
            .battle
            .iter()
            .flat_map(|zone| zone.roamers.iter())
            .map(|roamer| roamer.preset)
        {
            if companies.contains_key(&preset) {
                continue;
            }
            let Some(record) = dq9_core::scenario::battle(&parsed, u16::from(preset)) else {
                continue;
            };
            let mut company: Company = [(0, 0), (0, 0), (0, 0)];
            for (slot, &(monster, count)) in record.monsters.iter().take(3).enumerate() {
                company[slot] = (monster as u16, count.min(i16::MAX as u32) as i16);
            }
            companies.insert(preset, company);
        }
        self.classes = Classes { stage, companies };
        let item_members = ["a", "b", "d", "h", "l", "s", "u", "w"]
            .iter()
            .map(|c| {
                gp2_member(
                    &parsed,
                    &format!("data/prm/itemdt_{c}.gp2"),
                    &format!("itemdt_{c}_en.nat"),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let refs: Vec<&[u8]> = item_members.iter().map(Vec::as_slice).collect();
        let items = ItemStats::from_members(&refs).context("parsing the equipment stats")?;
        let params = BattleParams::parse(file("data/prm/itembtlprm.nat")?)
            .context("parsing itembtlprm.nat")?;
        self.equipment = Some(Equipment { items, params });
        let tables = Arc::new(tables);
        self.tables = Some(Arc::clone(&tables));
        Ok(tables)
    }

    /// The spawn graph of a map: `<code>P00.bin` for a code of five characters or less, else
    /// the code with its fourth character replaced by `P` (`ov_017::021b498c`).
    fn graph(&mut self, rom: &[u8], map: u16, code: &str) -> Option<RoamGraph> {
        if let Some(graph) = self.graphs.get(&map) {
            return graph.clone();
        }
        let graph = (|| {
            let name = graph_member(code)?;
            let parsed = Rom::parse(rom).ok()?;
            let archive = Gpc2::parse(parsed.file_by_path("data/pack_lv5/path.gp2").ok()?).ok()?;
            let entry = archive.find(&name)?;
            let member = archive.member(entry).ok()?;
            let narc = Narc::parse(&member).ok()?;
            RoamGraph::parse(narc.file(0).ok()?).ok()
        })();
        self.graphs.insert(map, graph.clone());
        graph
    }

    /// Adds one field model of monster `number` to `runtime`; `None` when its archive is
    /// missing or malformed. The model is the monster's field body (`<code>_f.mon`), never
    /// another monster's.
    fn add_model(&mut self, rom: &[u8], number: u16, runtime: &mut AnimatedScene) -> Option<usize> {
        let code = self.codes.get(&number)?;
        let path = format!("enemy/{code}_f.mon");
        let Some(bytes) = crate::event::scan::monster_archive_bytes(rom, &path) else {
            if self.missing.insert(number) {
                status(&format!("roamers: {path} is not in enemy.gp2"));
            }
            return None;
        };
        let actor = match runtime.add_actor(&bytes, &[]) {
            Ok(actor) => actor,
            Err(e) => {
                if self.missing.insert(number) {
                    status(&format!("roamers: {path}: {e:#}"));
                }
                return None;
            }
        };
        let Some(instance) = runtime.actor_instance(actor) else {
            if self.missing.insert(number) {
                status(&format!("roamers: {path}: no polygons"));
            }
            return None;
        };
        let index = self.actors.len();
        self.actors.push(FieldActor {
            number,
            actor,
            instance,
        });
        runtime.scene.instances[instance].world = Mat4::from_scale(Vec3::ZERO);
        runtime.scene.instances[instance].home = Mat4::from_scale(Vec3::ZERO);
        Some(index)
    }

    /// One free model of `number`, adding one when the pool is empty.
    fn take_model(
        &mut self,
        rom: &[u8],
        number: u16,
        runtime: &mut AnimatedScene,
    ) -> Option<usize> {
        if let Some(pool) = self.pools.get_mut(&number)
            && let Some(index) = pool.pop()
        {
            return Some(index);
        }
        self.add_model(rom, number, runtime)
    }

    fn release(&mut self, index: usize, runtime: Option<&mut AnimatedScene>) {
        let Some(field) = self.actors.get(index).copied() else {
            return;
        };
        if let Some(runtime) = runtime
            && let Some(instance) = runtime.scene.instances.get_mut(field.instance)
        {
            instance.world = Mat4::from_scale(Vec3::ZERO);
            instance.home = instance.world;
        }
        self.pools.entry(field.number).or_default().push(index);
    }

    /// Loads `map` (id and code): a real map change starts a fresh [`Roamers`], a reload of
    /// the same map keeps its state. The field models are built again because the scene is a
    /// new one; they are never carried across a rebuild by index.
    fn enter(
        &mut self,
        rom: &[u8],
        map: u16,
        code: &str,
        runtime: &mut AnimatedScene,
        world: &World,
    ) {
        let same_map = self.map == Some(map) && self.roamers.is_some();
        if same_map {
            self.clear_models();
        } else {
            self.forget();
        }
        let tables = match self.tables(rom) {
            Ok(tables) => tables,
            Err(e) => {
                status(&format!("roamers: {e:#}"));
                return;
            }
        };
        let graph = self.graph(rom, map, code);
        if tables.zones(map).is_none() && graph.is_none() {
            // A map without encounters (S01M01, D01M01, D01M05): nothing to load.
            self.map = Some(map);
            return;
        }
        if graph.is_none() {
            status(&format!("roamers: {map} has no spawn graph"));
        }
        if !same_map {
            self.roamers = Some(Roamers::new(
                map,
                Arc::clone(&tables),
                graph.clone(),
                world.crand.clone(),
                world.random.clone(),
            ));
        }
        self.map = Some(map);
        // Every monster the map's zones can spawn gets one field model now, so the common
        // spawns add no geometry and need no scene rebuild.
        let mut numbers: Vec<u16> = tables
            .zones(map)
            .into_iter()
            .flat_map(|zones| zones.zones.iter())
            .flat_map(|zone| zone.roamers.iter())
            .map(|roamer| roamer.monster())
            .collect();
        numbers.sort_unstable();
        numbers.dedup();
        for number in numbers {
            if let Some(index) = self.add_model(rom, number, runtime) {
                self.pools.entry(number).or_default().push(index);
            }
        }
        status(&format!(
            "roamers: map {map} {}, {} field model(s)",
            graph.map_or(0, |graph| graph.nodes.len()),
            self.actors.len()
        ));
    }

    /// One 30 Hz update of the roamers. The scene follows in [`Self::sync`].
    fn step(
        &mut self,
        input: &StepInput,
        world: &dq9_core::collision::CollisionWorld,
    ) -> Option<Contact> {
        let contact = {
            let Self {
                roamers, classes, ..
            } = self;
            let roamers = roamers.as_mut()?;
            roamers.step(input, world, classes)
        };
        if contact.is_some() {
            self.contacts += 1;
        }
        // A defeated roamer leaves once its 125 ms fade has run out (state 11); the slot can
        // then spawn another. The array is fixed: twelve slots, no per-update allocation.
        if let Some(roamers) = &mut self.roamers {
            let mut done = [false; SLOTS];
            for (slot, finished) in done.iter_mut().enumerate() {
                *finished = roamers.roamer(slot).is_some_and(|roamer| {
                    roamer.state == dq9_core::encounter::roamer::state::DEFEATED
                        && roamer.alpha == 0
                });
            }
            for (slot, finished) in done.into_iter().enumerate() {
                if finished {
                    roamers.remove(slot);
                }
            }
        }
        contact
    }

    /// Binds the roamers' slots to field models, places them in the world, plays the motion
    /// their state asks and applies the alpha. Returns whether a model was added (the caller
    /// uploads the scene again only then).
    fn sync(&mut self, rom: &[u8], runtime: Option<&mut AnimatedScene>) -> bool {
        let Some(runtime) = runtime else {
            return false;
        };
        let before = self.actors.len();
        for slot in 0..SLOTS {
            let roamer = self
                .roamers
                .as_ref()
                .and_then(|roamers| roamers.roamer(slot))
                .cloned();
            let roamer = roamer.as_ref();
            let bound = self.slots[slot];
            match (roamer, bound) {
                (Some(roamer), bound) if bound.is_some_and(|b| b.number == roamer.number) => {}
                (Some(_), _) => {
                    if let Some(bound) = bound {
                        self.release(bound.model, Some(runtime));
                        self.slots[slot] = None;
                    }
                }
                (None, Some(bound)) => {
                    self.release(bound.model, Some(runtime));
                    self.slots[slot] = None;
                }
                (None, None) => {}
            }
            let Some(roamer) = roamer else {
                continue;
            };
            if self.slots[slot].is_none() {
                let Some(index) = self.take_model(rom, roamer.number, runtime) else {
                    continue;
                };
                let Some(field_actor) = self.actors.get(index).copied() else {
                    continue;
                };
                self.slots[slot] = Some(Bound {
                    number: roamer.number,
                    model: index,
                    actor: field_actor.actor,
                    instance: field_actor.instance,
                    motion: None,
                });
            }
            let Some(bound) = self.slots[slot].as_mut() else {
                continue;
            };
            let position = Vec3::new(
                roamer.position[0] as f32 / 4096.0,
                roamer.position[1] as f32 / 4096.0,
                roamer.position[2] as f32 / 4096.0,
            );
            let angle = roamer.angle as f32 / 4096.0;
            let scale = roamer.scale as f32 / 4096.0;
            let world = if roamer.alpha > 0 {
                Mat4::from_translation(position)
                    * Mat4::from_rotation_y(angle)
                    * Mat4::from_scale(Vec3::splat(scale))
            } else {
                Mat4::from_scale(Vec3::ZERO)
            };
            if let Some(instance) = runtime.scene.instances.get_mut(bound.instance) {
                instance.world = world;
                let model = instance.model;
                // `GetInheritedAlpha`: the roamer's fade, applied to the model's polygons.
                for material in &mut runtime.scene.models[model].materials {
                    material.alpha = roamer.alpha.clamp(0, 31) as u8;
                }
            }
            let motion = Motion::of(roamer.state);
            if bound.motion != Some(motion) {
                for name in motion.names() {
                    if runtime.play_actor(bound.actor, name, motion.once(), 1.0) {
                        bound.motion = Some(motion);
                        break;
                    }
                }
            }
        }
        self.actors.len() > before
    }

    /// Every pooled field model's world matrix (released ones carry a zero scale), for the
    /// renderer's per-frame instance update. All actors are returned: a slot released this
    /// step must reach the renderer with its zero matrix or the old model stays drawn.
    pub(super) fn instance_worlds<'a>(
        &'a self,
        runtime: &'a AnimatedScene,
    ) -> impl Iterator<Item = (usize, Mat4)> + 'a {
        self.actors.iter().filter_map(move |field| {
            let instance = runtime.scene.instances.get(field.instance)?;
            Some((field.instance, instance.world))
        })
    }

    /// The world matrix of slot `slot`'s model, if it has one.
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    fn slot_world(&self, slot: usize, runtime: &AnimatedScene) -> Option<Mat4> {
        let bound = self.slots.get(slot)?.as_ref()?;
        runtime.scene.instances.get(bound.instance).map(|i| i.world)
    }

    /// The battle in play came from `slot`; its settlement resumes or fades that roamer.
    pub(super) fn set_pending(&mut self, slot: usize) {
        self.pending = Some(slot);
    }

    /// The end of the battle that started from a contact: a won roamer fades out (the slot is
    /// removed once the fade ends), a fled one stands startled and resumes wandering; the
    /// recent list keeps it from touching the party again immediately. **[hypothesis]** for
    /// the fled/lost states (see `Roamers::settle`).
    fn settle(&mut self, result: Option<BattleResult>, hero: dq9_core::fx::Fx3) {
        let Some(slot) = self.pending.take() else {
            return;
        };
        let won = !matches!(result, Some(BattleResult::Fled | BattleResult::Lost));
        if let Some(roamers) = &mut self.roamers {
            roamers.settle(slot, won, hero);
        }
    }

    /// The field advanced the generators; the application's `World` follows, so a battle
    /// started by a script (not a contact) begins on the field's current stream.
    fn export_generators(&self, world: &mut World) {
        if let Some(roamers) = &self.roamers {
            let (crand, random) = roamers.generators();
            world.crand = crand.clone();
            world.random = random.clone();
        }
    }

    /// The battle advanced the world's generators; the field continues from that state (the
    /// return leg of the transfer `begin_encounter` makes). Called after every battle, a
    /// scripted one included, so the field's stream is never replayed from before it.
    fn adopt_generators(&mut self, world: &World) {
        if let Some(roamers) = &mut self.roamers {
            roamers.set_generators(world.crand.clone(), world.random.clone());
        }
    }
}

/// One member of a GPC2 archive, decompressed (`data/prm/itemdt_*.gp2`).
fn gp2_member(rom: &Rom<'_>, path: &str, name: &str) -> Result<Vec<u8>> {
    let archive = Gpc2::parse(rom.file_by_path(path).with_context(|| path.to_owned())?)
        .with_context(|| format!("parsing {path}"))?;
    let entry = archive
        .find(name)
        .with_context(|| format!("{name} is not in {path}"))?;
    Ok(archive
        .member(entry)
        .with_context(|| format!("reading {name} of {path}"))?
        .into_owned())
}

/// `<code>P00.bin` for a code of five characters or less; otherwise the code with its
/// fourth character replaced by `P` (`D01M02` → `D01P02.bin`).
fn graph_member(code: &str) -> Option<String> {
    let bytes = code.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if bytes.len() <= 5 {
        return Some(format!("{code}P00.bin"));
    }
    let mut replaced = bytes.to_vec();
    replaced[3] = b'P';
    Some(format!("{}.bin", String::from_utf8_lossy(&replaced)))
}

impl App {
    /// Turns the roaming monsters on (the window and the Web) or off (a headless run keeps
    /// its recorded traces). Enabling after a map loaded builds its field models.
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(super) fn set_roaming_enabled(&mut self, enabled: bool) {
        if self.roaming.enabled == enabled {
            return;
        }
        self.roaming.enabled = enabled;
        if !enabled {
            self.roaming.forget();
            return;
        }
        self.enter_roaming();
    }

    /// The current map's tables, graph, [`Roamers`] and field models. Called after a map load
    /// (any scene rebuild drops the old model indices) and when roaming is switched on.
    pub(super) fn enter_roaming(&mut self) {
        if !self.roaming.enabled {
            return;
        }
        // The world's generators seed the roamers' draws; a `--seed` therefore reaches the
        // spawns and the companies, not only the battles.
        if !self.world_seeded {
            self.ensure_world();
        }
        let (Some(rom), Some((entry, _))) = (self.rom.as_deref(), self.current.as_ref()) else {
            return;
        };
        let (map, code) = (entry.id, entry.code.clone());
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        self.roaming.enter(rom, map, &code, runtime, &self.world);
    }

    /// Keeps the roamers across a battle: the field's scene is the same object when it comes
    /// back, so the model indices stay valid and only the step is paused.
    pub(super) fn step_roaming(&mut self, moving: bool) -> Option<Contact> {
        if !self.roaming.enabled {
            return None;
        }
        let Some(hero) = &self.hero else {
            return None;
        };
        let (position, angle) = (
            field::to_fx(hero.position()),
            (field::angle_of_yaw(hero.yaw()) * 4096.0) as i32,
        );
        let (strength, deftness) = self.party_strength();
        let input = StepInput {
            position,
            angle,
            radius: dq9_core::movement::RADIUS,
            moving,
            strength,
            deftness,
            time_of_day: self.state.time_of_day as u8,
            delta_ms: 33,
            ticks: 2,
        };
        let contact = self.roaming.step(&input, &self.collision);
        // The field's draws are the live stream: a scripted battle that starts next uses this
        // state, and `settle_roaming` hands it back after the battle.
        self.roaming.export_generators(&mut self.world);
        self.world_seeded = true;
        let added = {
            let (Some(rom), runtime) = (self.rom.as_deref(), self.runtime.as_mut()) else {
                return contact;
            };
            self.roaming.sync(rom, runtime)
        };
        if added {
            // A new monster model is new scene geometry: the renderer is built from it once.
            self.upload();
        }
        contact
    }

    /// The leader's attack plus defence and the highest deftness among living members
    /// (`StepInput`), from `Character::fighter` with the cached `ItemStats`/`BattleParams`
    /// (level table + worn equipment). With no stats loaded, the level table's attributes.
    fn party_strength(&mut self) -> (u32, u32) {
        self.ensure_party_levels();
        let Some(equipment) = self.roaming.equipment.as_ref() else {
            let leader = self.party.leader().map_or((0u32, 0u32), |member| {
                (
                    u32::from(member.stats.strength) + u32::from(member.stats.resilience),
                    u32::from(member.stats.deftness),
                )
            });
            let deftness = self
                .party
                .members
                .iter()
                .map(|member| u32::from(member.stats.deftness))
                .max()
                .unwrap_or(0);
            return (leader.0, deftness.max(leader.1));
        };
        let mut leader = None;
        let mut deftness = 0u32;
        for member in self.party.members.iter().filter(|member| member.hp > 0) {
            let fighter = member.fighter(&equipment.items, &equipment.params);
            if leader.is_none() {
                leader = Some((fighter.attack + fighter.defence).max(0) as u32);
            }
            deftness = deftness.max(fighter.deftness.max(0) as u32);
        }
        (leader.unwrap_or(0), deftness)
    }

    /// Applies the level tables before the first contact, so the notice decision reads the
    /// party's real attributes (a new game has none until a level table fills them).
    fn ensure_party_levels(&mut self) {
        if self.party_ready {
            return;
        }
        if let Some(rom) = self.rom.as_deref()
            && let Ok(parsed) = Rom::parse(rom)
        {
            let files: Vec<&[u8]> = (0..VOCATION_COUNT)
                .filter_map(|v| parsed.file_by_path(&format!("data/prm/level{v}.bin")).ok())
                .collect();
            if let Ok(files) = <[&[u8]; VOCATION_COUNT]>::try_from(files)
                && let Ok(levels) = dq9_formats::level::LevelTables::parse(files)
            {
                self.party.apply_level_tables(&levels);
            }
        }
        self.party_ready = true;
    }

    /// A contact starts the fight: the company, the stage and the opening are the core's
    /// (`dq9_core::encounter::Contact`), never re-drawn here.
    pub(super) fn begin_encounter(&mut self, contact: Contact) {
        let groups = if contact.company.is_empty() {
            vec![(contact.monster, 1)]
        } else {
            contact
                .company
                .iter()
                .map(|&(monster, count)| (monster, u16::from(count)))
                .collect()
        };
        let opening = match contact.opening {
            Opening::PartyFirst => dq9_core::battle::Surprise::Party,
            Opening::MonstersFirst => dq9_core::battle::Surprise::Monsters,
            Opening::Even => dq9_core::battle::Surprise::None,
        };
        status(&format!(
            "roamers: contact with monster {} (slot {}), stage {}, company {:?}, {:?}",
            contact.monster, contact.roamer, contact.stage, groups, opening
        ));
        // The battle draws on the same generators the roamers used (the `CRand` of drops and
        // the world's), so a contact does not reset the sequence.
        let state = self.roaming.roamers.as_ref().map(|roamers| {
            let (crand, world) = roamers.generators();
            World {
                random: world.clone(),
                crand: crand.clone(),
            }
        });
        if let Some(world) = state {
            self.world = world;
            self.world_seeded = true;
        }
        self.roaming.set_pending(contact.roamer);
        let stage = if contact.stage == 0 {
            FALLBACK_STAGE
        } else {
            contact.stage
        };
        self.start_encounter(groups, stage, opening);
        // No scene could be built (a missing stage or model): the roamer must not stay stuck
        // in its touched state.
        if !self.battle_is_active() && self.fight.is_none() {
            let hero = self
                .hero
                .as_ref()
                .map_or([0; 3], |hero| field::to_fx(hero.position()));
            self.roaming.settle(Some(BattleResult::Fled), hero);
        }
    }

    /// The end of a battle tells the roaming layer once; the party, the drops and the story
    /// were already settled by `App::settle_battle`. The battle's generators are handed back
    /// to the field (a scripted battle too, so its stream is not replayed), and the contacted
    /// roamer gets its outcome (fade out on a win, startled resumption on a flight or loss).
    pub(super) fn settle_roaming(&mut self, result: Option<BattleResult>) {
        let hero = self
            .hero
            .as_ref()
            .map_or([0; 3], |hero| field::to_fx(hero.position()));
        self.roaming.settle(result, hero);
        self.roaming.adopt_generators(&self.world);
    }

    /// Roamers on the field, with their slot (tests and diagnostics).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_roamers(&self) -> Vec<(usize, Roamer)> {
        self.roaming
            .roamers
            .as_ref()
            .map_or_else(Vec::new, |roamers| {
                roamers
                    .roamers()
                    .map(|(slot, roamer)| (slot, roamer.clone()))
                    .collect()
            })
    }

    /// The world matrix of a slot's field model (tests: visibility).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_slot_world(&self, slot: usize) -> Option<Mat4> {
        let runtime = self.runtime.as_ref()?;
        self.roaming.slot_world(slot, runtime)
    }

    /// The scene instance currently bound to a slot (tests: the model released by a fade).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_slot_instance(&self, slot: usize) -> Option<usize> {
        self.roaming
            .slots
            .get(slot)?
            .as_ref()
            .map(|bound| bound.instance)
    }

    /// The scripted company of an `eventbattle.bin` number, as the contact sees it (tests).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_event_company(&self, preset: u8) -> Option<Company> {
        self.roaming.classes.companies.get(&preset).copied()
    }

    /// The preset numbers whose companies the map tables name (tests).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_presets(&self) -> Vec<u8> {
        let mut presets: Vec<u8> = self
            .roaming
            .tables
            .as_ref()
            .map(|tables| {
                tables
                    .battle
                    .iter()
                    .flat_map(|zone| zone.roamers.iter())
                    .map(|roamer| roamer.preset)
                    .collect()
            })
            .unwrap_or_default();
        presets.sort_unstable();
        presets.dedup();
        presets
    }

    /// The equipment-aware strength and deftness the roamer step reads (tests).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_party_strength(&mut self) -> (u32, u32) {
        self.party_strength()
    }

    /// The roamers' generators (tests: the battle and the field share one stream).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_world(&self) -> Option<World> {
        self.roaming.roamers.as_ref().map(|roamers| {
            let (crand, random) = roamers.generators();
            World {
                crand: crand.clone(),
                random: random.clone(),
            }
        })
    }

    /// Contacts started so far (tests).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_contacts(&self) -> u32 {
        self.roaming.contacts
    }

    /// The number of field models built for the loaded map (tests).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_model_count(&self) -> usize {
        self.roaming.actors.len()
    }

    /// The map whose roamers are loaded (tests).
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(crate) fn roaming_map(&self) -> Option<u16> {
        self.roaming.map
    }
}
