//! The roaming monsters of a field or dungeon map and the encounter they start: where they
//! spawn, how they wander and chase, when the party touches one, the surprise roll, the company
//! that joins and the stage the ground names (`docs/re/encounter.md`). No I/O: the caller hands
//! over the ROM tables, the map's spawn graph and the collision world.
//!
//! The pieces, in the order the game runs them each update (30 Hz, `Field::step`'s cadence,
//! 33 ms, 50 ms when the frame's update runs long): the spawn tick of the map slot
//! (`ov_017::021b5648` loads, `0x020733e8` ticks), each roamer's state machine (`0x02076b68`),
//! the far roamers released (`ov_017::021a1ad0`), then the encounter check
//! (`ov_017::02196430`).

pub mod graph;
pub mod pick;
pub mod roamer;
pub mod tables;

use std::sync::Arc;

use crate::battle::{CRand, Random};
use crate::collision::CollisionWorld;
use crate::fx::{self, Fx3, fix32_cos, fix32_sin};
use graph::RoamGraph;
use pick::Company;
use roamer::{OPAQUE, Roamer, state};
use tables::{EncounterTables, ZoneDef};

/// Roamers a map holds (`func_02028a54`: twelve objects per slot).
pub const SLOTS: usize = 12;
/// The stage a battle falls back to when the ground names none the map list has
/// (`ov_000::02166880`, `docs/re/encounter.md`).
pub const FALLBACK_STAGE: u16 = 30116;
/// Monsters 38, 39 and 40 never give a surprise round (the three bytes at `0x021d6438`).
const NO_SURPRISE: std::ops::RangeInclusive<u16> = 0x26..=0x28;
/// The map's spawn timer ticks in milliseconds; a spawn waits at least a second.
const SPAWN_PERIOD_MS: i32 = 1000;

/// Who moves first (`[battle + 0xe49]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// The party surprises the monsters.
    PartyFirst,
    /// The monsters surprise the party.
    MonstersFirst,
    /// No surprise.
    Even,
}

/// The encounter request the battle flow sets a fight up from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    /// The slot of the roamer touched.
    pub roamer: usize,
    /// Its monster number.
    pub monster: u16,
    /// The company: monster and how many, in battle order, the touched monster's kind among
    /// them. Counts are at least 1.
    pub company: Vec<(u16, u8)>,
    /// The battle stage (a map id of `maplist9.bin`).
    pub stage: u16,
    pub opening: Opening,
    /// Other roamers pulled into the fight. **[unknown]** how the game fills this list: the
    /// encounter check stops at the first roamer touched, and no code that adds another was
    /// found, so it is always empty here.
    pub others: Vec<usize>,
}

/// What the encounter check asks of the map list and the scripted battles.
pub trait Surroundings {
    /// The region class of a stage in `maplist9.bin` (the byte at `+0xe` of its record), `None`
    /// when the list has no such map.
    fn stage_class(&self, stage: u16) -> Option<u8>;

    /// The company of scripted battle `preset` (`eventbattle.bin`), when it is loaded.
    fn event_company(&self, _preset: u8) -> Option<Company> {
        None
    }
}

/// `func_02099a68`: the region mask of a stage's class: bits 2–5 of the byte give a number `v`
/// and the mask is `1 << v`, 0 for `v` = 8.
pub fn ground_mask(class: u8) -> u32 {
    match (class & 0x3f) >> 2 {
        8 => 0,
        v => 1 << v,
    }
}

/// What one update needs to know of the party and the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepInput {
    /// The leader: position (`+0x44`), model angle (`+0x54`) and collision radius.
    pub position: Fx3,
    pub angle: i32,
    pub radius: i32,
    /// The leader is walking (a roamer that reached it waits while it stands).
    pub moving: bool,
    /// The leader's attack plus defence, what a monster weighs against its own.
    pub strength: u32,
    /// The highest deftness among the party who can act (the surprise roll).
    pub deftness: u32,
    /// `GameState::GetTimeOfDay`: night 0, morning 1, day 2, evening 3.
    pub time_of_day: u8,
    /// Milliseconds and frames since the last update: 33 and 2 at 30 Hz; the game's
    /// `GetTrueDeltaTime` gave 50 ms on some updates (`docs/re/encounter.md`, "Claims
    /// checked", (c)).
    pub delta_ms: i32,
    pub ticks: i32,
}

impl StepInput {
    /// The leader at `position` facing `angle`, standing, in daylight, 30 Hz.
    pub fn at(position: Fx3, angle: i32) -> Self {
        Self {
            position,
            angle,
            radius: crate::movement::RADIUS,
            moving: false,
            strength: 0,
            deftness: 0,
            time_of_day: 2,
            delta_ms: 33,
            ticks: 2,
        }
    }
}

/// The roamers of one map.
#[derive(Debug, Clone)]
pub struct Roamers {
    map: u16,
    tables: Arc<EncounterTables>,
    zones: Vec<ZoneDef>,
    graph: Option<RoamGraph>,
    slots: [Option<Roamer>; SLOTS],
    /// `+8` of the map slot: milliseconds since the last spawn.
    timer: i32,
    /// The party has taken a step since the map was entered: until then nothing spawns, the
    /// slot's timer still running (`docs/re/encounter.md`, "Claims checked", (b); the cause is
    /// **[hypothesis]**: on a fresh arrival nothing spawned for 40 s, the first spawn came two
    /// frames after the first step).
    stepped: bool,
    next_id: u16,
    hero_node: Option<usize>,
    /// Ids of roamers the encounter check skips (`+0x44b8`, four of them).
    recent: [u16; 4],
    crand: CRand,
    world: Random,
}

impl Roamers {
    /// The roamers of `map`: its zones from `tables`, its spawn `graph` (`None` for a map with no
    /// `P` file, which spawns nothing) and the two generators. A map with no `encfld` group
    /// spawns nothing either.
    pub fn new(
        map: u16,
        tables: Arc<EncounterTables>,
        graph: Option<RoamGraph>,
        crand: CRand,
        world: Random,
    ) -> Self {
        let zones = tables
            .zones(map)
            .map(|m| m.zones.clone())
            .unwrap_or_default();
        Self {
            map,
            tables,
            zones,
            graph,
            slots: std::array::from_fn(|_| None),
            timer: 0,
            stepped: false,
            next_id: 1,
            hero_node: None,
            recent: [0; 4],
            crand,
            world,
        }
    }

    pub fn map(&self) -> u16 {
        self.map
    }

    /// The roamers on the map, with their slot.
    pub fn roamers(&self) -> impl Iterator<Item = (usize, &Roamer)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_ref().map(|r| (i, r)))
    }

    pub fn roamer(&self, slot: usize) -> Option<&Roamer> {
        self.slots.get(slot).and_then(Option::as_ref)
    }

    /// The generators, to hand back to the battle (`.0` the C `rand`, `.1` the world's).
    pub fn generators(&self) -> (&CRand, &Random) {
        (&self.crand, &self.world)
    }

    /// Ids the encounter check ignores (`func_ov017_0219735c`).
    pub fn set_recent(&mut self, ids: [u16; 4]) {
        self.recent = ids;
    }

    /// Removes a roamer (it was defeated or fled the map).
    pub fn remove(&mut self, slot: usize) {
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = None;
        }
    }

    fn occupied(&self, id: u8, except: Option<usize>) -> bool {
        let Some(graph) = &self.graph else {
            return false;
        };
        self.slots.iter().enumerate().any(|(i, r)| {
            Some(i) != except
                && r.as_ref()
                    .is_some_and(|r| graph.nodes.get(r.node).is_some_and(|n| n.id == id))
        })
    }

    /// `func_02031118` over the roamers: whether any lies in the box `[lo, hi]`.
    fn any_in_box(&self, lo: Fx3, hi: Fx3) -> usize {
        self.slots
            .iter()
            .flatten()
            .filter(|r| (0..3).all(|a| r.position[a] >= lo[a] && r.position[a] <= hi[a]))
            .count()
    }

    /// One update: spawn, move, release the far ones, check for a touch. Returns the encounter
    /// the party walked into, if any.
    pub fn step(
        &mut self,
        input: &StepInput,
        world: &CollisionWorld,
        around: &dyn Surroundings,
    ) -> Option<Contact> {
        self.hero_node = self.graph.as_ref().and_then(|g| g.nearest(input.position));
        self.spawn(input, world);
        for slot in 0..SLOTS {
            self.update(slot, input, world);
        }
        self.release_far(input);
        self.touch(input, world, around)
    }

    /// `func_020733d8` for a party of one.
    fn spawn(&mut self, input: &StepInput, world: &CollisionWorld) {
        let Some(graph) = self.graph.clone() else {
            return;
        };
        if self.zones.is_empty() {
            return;
        }
        let before = self.timer;
        self.timer += input.delta_ms;
        // A fresh arrival spawned nothing until the party's first step; the timer kept running
        // (40 s at least, `docs/re/encounter.md`, "Claims checked", (b)).
        self.stepped |= input.moving;
        if !self.stepped {
            return;
        }
        if before + input.delta_ms < SPAWN_PERIOD_MS {
            return;
        }
        let Some(free) = self.slots.iter().position(Option::is_none) else {
            return;
        };
        let hero = input.position;
        // Roamers near the party: three is enough.
        let near = self.any_in_box(
            [hero[0] - 0xf000, hero[1] - 0x800, hero[2] - 0xf000],
            [hero[0] + 0xf000, hero[1] + 0x1800, hero[2] + 0xf000],
        );
        if near >= 3 {
            return;
        }
        let Some(hero_node) = self.hero_node else {
            return;
        };
        if graph.nodes[hero_node].links.is_empty() {
            return;
        }
        let slots = &self.slots;
        let taken = |id: u8| {
            slots
                .iter()
                .flatten()
                .any(|r| graph.nodes.get(r.node).is_some_and(|n| n.id == id))
        };
        let crand = &mut self.crand;
        let Some(plan) = plan_spawn(&graph, hero, input.angle, hero_node, &taken, &mut |n| {
            (crand.rand() as usize) % n
        }) else {
            return;
        };
        let chosen = plan.chosen;
        let mut position = plan.position;
        if fx::distance(hero, position) < 0x3800 {
            return;
        }
        if self.any_in_box(
            [
                position[0] - 0x3000,
                position[1] - 0x5000,
                position[2] - 0x3000,
            ],
            [
                position[0] + 0x3000,
                position[1] + 0x5000,
                position[2] + 0x3000,
            ],
        ) > 0
        {
            return;
        }
        let target = &graph.nodes[chosen];
        if target.flags & 2 != 0 {
            return;
        }
        let night = pick::is_night(input.time_of_day);
        let Some(zone) =
            pick::spawn_zone(&self.zones, night, u32::from(target.mask), &mut self.crand)
        else {
            return;
        };
        if (7 - zone.delay()) * SPAWN_PERIOD_MS > self.timer {
            return;
        }
        let zone_id = zone.id;
        let Some(roamer) = pick::roamer_of(zone, &mut self.crand).copied() else {
            return;
        };
        let Some(field) = self.tables.monster(roamer.monster()).copied() else {
            return;
        };
        let direction = fx::normalize(fx::sub(hero, position));
        let angle = fx::atan2(direction[0], direction[2]);
        if let Some(height) = world.ground(position) {
            position[1] = height;
        }
        let id = self.fresh_id();
        let mut spawned = Roamer::spawn(
            roamer.monster(),
            id,
            zone_id,
            field,
            i32::from(roamer.scale),
            position,
            angle,
            chosen,
            target.id,
        );
        spawned.enter(state::APPEAR, hero);
        self.slots[free] = Some(spawned);
        self.timer = 0;
    }

    /// `func_ov017_021a1944`: an id no roamer of the map has.
    fn fresh_id(&mut self) -> u16 {
        loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if self.slots.iter().flatten().all(|r| r.id != id) {
                return id;
            }
        }
    }

    /// `func_ov017_021a1ad0`: a roamer more than 25 units from the party on any axis goes
    /// (the box is `±0x19000`); the ones in a fight stay.
    fn release_far(&mut self, input: &StepInput) {
        let hero = input.position;
        for entry in &mut self.slots {
            let far = entry.as_ref().is_some_and(|r| {
                r.state != state::TOUCHED
                    && r.state != state::FOUGHT
                    && (0..3).any(|a| (r.position[a] - hero[a]).abs() > 0x19000)
            });
            if far {
                *entry = None;
            }
        }
    }

    /// `func_02078e60`: where a roamer on node `r.node` goes next, and whether that node was
    /// taken when it first stood on it.
    fn wander_choose(&mut self, slot: usize, r: &mut Roamer) -> (u8, bool) {
        let Some(graph) = self.graph.clone() else {
            return (0, false);
        };
        let Some(node) = graph.nodes.get(r.node) else {
            return (0, false);
        };
        if node.links.is_empty() {
            return (0, false);
        }
        if !r.started {
            let taken = self.occupied(node.id, Some(slot));
            if !taken {
                r.started = true;
            }
            return (node.id, taken);
        }
        let zone = self.zones.iter().find(|z| z.id == r.zone);
        let mut open: Vec<usize> = Vec::new();
        let mut blocked = false;
        if let Some(zone) = zone {
            for &link in &node.links {
                let l = &graph.nodes[link];
                if l.mask == 0 || u32::from(l.mask) & zone.mask() != 0 {
                    if self.occupied(l.id, None) {
                        blocked = true;
                    } else {
                        open.push(link);
                    }
                }
            }
        }
        let own = graph.index_of(node.id);
        if blocked && open.is_empty() {
            if let Some(own) = own {
                r.node = own;
            }
            return (node.id, false);
        }
        let picked = (!open.is_empty()).then(|| open[(self.crand.rand() as usize) % open.len()]);
        match picked {
            Some(link) => {
                let id = graph.nodes[link].id;
                if let Some(index) = graph.index_of(id) {
                    r.node = index;
                }
                (id, false)
            }
            None => {
                if let Some(own) = own {
                    r.node = own;
                }
                (node.id, false)
            }
        }
    }

    /// `func_02076ccc` into WALK (`0x02076fec`) or BLINK (`0x02077510`): the target is the
    /// destination node. Fails (stays) when the node is missing or, to walk, under a unit away.
    fn enter_travel(&self, r: &mut Roamer, to: u8, hero: Fx3) -> bool {
        if r.state == to {
            return true;
        }
        let Some(graph) = &self.graph else {
            return false;
        };
        let Some(index) = graph.index_of(r.destination) else {
            return false;
        };
        let target = graph.nodes[index].fx();
        if to == state::WALK {
            if fx::mag(fx::sub(target, r.position)) < 0x1000 {
                return false;
            }
            let direction = fx::normalize(fx::sub(target, r.position));
            r.target = target;
            r.heading = fx::atan2(direction[0], direction[2]);
            r.started = true;
        } else {
            r.target = target;
        }
        r.enter(to, hero)
    }

    /// One roamer's update: the timers, the notice test, the state's update
    /// (`func_02076b58`).
    fn update(&mut self, slot: usize, input: &StepInput, world: &CollisionWorld) {
        let Some(mut r) = self.slots[slot].take() else {
            return;
        };
        let hero = input.position;
        r.state_ms += input.delta_ms as u32;
        if r.state != state::APPEAR {
            r.awake_ms += input.delta_ms as u32;
        }
        // `func_020790ac`: standing or walking, awake for two seconds, it looks for the party.
        if matches!(r.state, state::IDLE | state::WALK) && r.awake_ms > 1999 && !r.arrived {
            let seen = match r.field.vision {
                1 => r.sees(hero, 0x3800, true),
                2 => r.sees(hero, 0x7800, true),
                3 => r.sees(hero, 0x3800, false),
                4 => r.sees(hero, 0x7800, false),
                _ => false,
            };
            if seen {
                r.enter(state::NOTICE, hero);
            }
        }
        let touch = r.touch_radius() + (input.radius >> 1);
        match r.state {
            state::APPEAR => {
                if r.state_ms > 1000 {
                    r.walking = false;
                    r.enter(state::IDLE, hero);
                }
            }
            state::IDLE => self.idle(slot, &mut r, input),
            state::WALK => {
                let blocked = self.moving(&mut r, world, input, false);
                if blocked {
                    r.enter(state::IDLE, hero);
                    r.awake_ms = 0;
                    r.blocked = true;
                } else {
                    let flat = r.flat_distance(r.target);
                    let toward =
                        fx::normalize(fx::sub(r.target, [r.position[0], 0, r.position[2]]));
                    r.heading = fx::atan2(toward[0], toward[2]);
                    if flat < 0x1000 {
                        let mut at = r.position;
                        if let Some(graph) = &self.graph
                            && let Some(node) = graph.nodes.get(r.node)
                        {
                            at[1] = node.fx()[1];
                        }
                        if let Some(height) = world.ground(at) {
                            at[1] = height;
                        }
                        r.place(at);
                        r.enter(state::IDLE, hero);
                    }
                }
            }
            state::BLINK => {
                if r.alpha == 0 && r.fade.is_none() {
                    r.place(r.target);
                    r.fade = Some((OPAQUE, 250));
                }
                r.fade_step(input.delta_ms);
                if r.alpha == OPAQUE && r.fade.is_none() && r.state_ms > 0 {
                    r.enter(state::IDLE, hero);
                }
            }
            state::CHASE | state::DASH => {
                if r.state == state::CHASE {
                    let toward = fx::normalize(fx::sub(hero, r.position));
                    r.heading = fx::atan2(toward[0], toward[2]);
                } else {
                    let toward = fx::normalize(fx::sub(
                        [r.target[0], 0, r.target[2]],
                        [r.position[0], 0, r.position[2]],
                    ));
                    r.heading = fx::atan2(toward[0], toward[2]);
                }
                let blocked = self.moving(&mut r, world, input, true);
                let to_hero = fx::distance(hero, r.position);
                if blocked {
                    r.enter(state::IDLE, hero);
                    r.awake_ms = 0;
                } else if (to_hero != 0 && to_hero < touch)
                    || (r.previous_state_is_chase() && to_hero > 0x7800)
                {
                    r.enter(state::IDLE, hero);
                    r.awake_ms = 0;
                    r.arrived = true;
                } else if !r.previous_state_is_chase() && (r.flat_distance(r.target) < 0x1800) {
                    r.enter(state::IDLE, hero);
                    r.awake_ms = 0;
                }
            }
            state::FLEE => {
                let blocked = self.moving(&mut r, world, input, true);
                if blocked {
                    r.enter(state::IDLE, hero);
                    r.awake_ms = 0;
                } else {
                    let flat = fx::normalize(fx::sub(
                        [r.target[0], 0, r.target[2]],
                        [r.position[0], 0, r.position[2]],
                    ));
                    r.heading = fx::atan2(flat[0], flat[2]);
                    if r.flat_distance(r.target) < 0x1800 {
                        let mut at = r.position;
                        if let Some(height) = world.ground(at) {
                            at[1] = height;
                        }
                        r.place(at);
                        r.enter(state::IDLE, hero);
                        r.awake_ms = 0;
                    }
                }
            }
            state::VANISH => {
                let blocked = self.moving(&mut r, world, input, true);
                if r.alpha == 0 {
                    return;
                }
                if r.alpha == OPAQUE {
                    let gap = fx::distance([r.position[0], 0, r.position[2]], r.target);
                    if gap < 0x2000 || blocked {
                        r.fade = Some((0, 125));
                    }
                }
                r.fade_step(input.delta_ms);
            }
            state::DEFEATED => {
                r.fade_step(input.delta_ms);
                if r.alpha == 0 {
                    return;
                }
            }
            state::NOTICE => self.decide(&mut r, input),
            state::STARTLED => self.startled(slot, &mut r, input),
            _ => {}
        }
        r.fade_step(
            if matches!(r.state, state::BLINK | state::VANISH | state::DEFEATED) {
                0
            } else {
                input.delta_ms
            },
        );
        self.slots[slot] = Some(r);
    }

    /// Moves `r` one update and says whether it ran into a wall it was heading for
    /// (**[hypothesis]**: the wall's normal faces away from the heading by more than a right
    /// angle; the game's test at `0x020322c0` was not ported).
    fn moving(
        &self,
        r: &mut Roamer,
        world: &CollisionWorld,
        input: &StepInput,
        _run: bool,
    ) -> bool {
        r.advance(world, input.ticks).is_some_and(|wall| {
            let n = fx::normalize([wall.normal[0], 0, wall.normal[2]]);
            let bearing = fx::atan2(n[0], n[2]);
            fx::abs(fx::signed_angle_distance(bearing, r.heading)) > 0x1922
        })
    }

    /// `func_02076df4`, the standing state: after four seconds it picks its next move.
    fn idle(&mut self, slot: usize, r: &mut Roamer, input: &StepInput) {
        let hero = input.position;
        if r.blocked || r.state_ms < 0xfa1 {
            return;
        }
        if r.arrived && !input.moving {
            return;
        }
        let first = !r.started;
        let old = r.destination;
        r.arrived = false;
        match r.field.wander {
            0 => {}
            2 => {
                r.started = true;
                let (id, _) = self.wander_choose(slot, r);
                r.destination = id;
                if old != id || first {
                    self.enter_travel(r, state::BLINK, hero);
                }
            }
            1 | 3 | 4 | 5 => {
                let (id, taken) = if matches!(r.previous, state::CHASE | state::DASH | state::FLEE)
                {
                    let nearest = self.graph.as_ref().and_then(|g| g.nearest(r.position));
                    if let (Some(index), Some(graph)) = (nearest, &self.graph) {
                        r.node = index;
                        (graph.nodes[index].id, false)
                    } else {
                        (0, false)
                    }
                } else {
                    self.wander_choose(slot, r)
                };
                r.destination = id;
                if first || taken {
                    if (old != id || first) && !taken {
                        self.enter_travel(r, state::WALK, hero);
                    }
                } else {
                    self.enter_travel(r, state::WALK, hero);
                }
            }
            _ => {}
        }
    }

    /// `0x020789a8`: 226 ms after noticing the party the roamer decides. A party whose attack
    /// plus defence is more than three times its own makes it flee (unless its level byte is
    /// 99), otherwise it fights.
    fn decide(&mut self, r: &mut Roamer, input: &StepInput) {
        if r.state_ms <= 0xe1 {
            return;
        }
        let hero = input.position;
        let power = (u32::from(r.field.attack_power) + u32::from(r.field.defence_power)) * 3;
        let next = if power < input.strength && r.field.level != 99 {
            match r.field.flee {
                0 => Some(state::STARTLED),
                1 => Some(state::FLEE),
                2 => Some(state::VANISH),
                3 => Some(if self.crand.rand() & 1 == 0 {
                    state::FLEE
                } else {
                    state::VANISH
                }),
                _ => None,
            }
        } else {
            match r.field.attack {
                0 => Some(state::DASH),
                1 => Some(state::CHASE),
                2 => Some(if self.crand.rand() & 1 == 0 {
                    state::FLEE
                } else {
                    state::VANISH
                }),
                _ => None,
            }
        };
        if let Some(next) = next {
            r.enter(next, hero);
        }
    }

    /// `0x02078d94`: after three seconds of standing startled it resumes wandering.
    fn startled(&mut self, slot: usize, r: &mut Roamer, input: &StepInput) {
        let hero = input.position;
        let wander = r.field.wander;
        if r.state_ms <= 0xbb8 {
            return;
        }
        match wander {
            0 => {
                r.enter(state::IDLE, hero);
            }
            2 => {
                let (id, _) = self.wander_choose(slot, r);
                r.destination = id;
                self.enter_travel(r, state::BLINK, hero);
            }
            1 | 3 | 4 | 5 if !self.enter_travel(r, state::WALK, hero) => {
                let (id, _) = self.wander_choose(slot, r);
                r.destination = id;
                self.enter_travel(r, state::WALK, hero);
            }
            _ => {}
        }
    }

    /// `func_ov017_02196430` for a party of one walking: the first roamer, in slot order, close
    /// enough to be touched, fully visible and past its appear state.
    fn touch(
        &mut self,
        input: &StepInput,
        world: &CollisionWorld,
        around: &dyn Surroundings,
    ) -> Option<Contact> {
        let hero = input.position;
        let slot = (0..SLOTS).find(|&i| {
            self.slots[i].as_ref().is_some_and(|r| {
                !self.recent.contains(&r.id)
                    && r.alpha == OPAQUE
                    && !matches!(r.state, state::APPEAR | state::TOUCHED | state::FOUGHT)
                    && touches(r.field.size, r.scale, r.position, hero, input.radius)
            })
        })?;
        let hero_state = self.slots[slot].as_ref()?.clone();
        // The roamer stops and faces the party.
        if let Some(r) = self.slots[slot].as_mut() {
            r.enter(state::TOUCHED, hero);
        }
        let facing = if Roamer::angle_to(hero_state.position, hero_state.angle, hero) as f32
            > Roamer::BACK_TURNED
        {
            1
        } else if Roamer::angle_to(hero, input.angle, hero_state.position) as f32
            > Roamer::BACK_TURNED
        {
            2
        } else {
            0
        };
        let roll = pick::surprise_roll(facing, input.deftness, &mut self.crand);
        let monster = hero_state.number;
        let opening = match roll {
            _ if NO_SURPRISE.contains(&monster) => Opening::Even,
            1 => Opening::PartyFirst,
            2 => Opening::MonstersFirst,
            _ => Opening::Even,
        };
        let ground = stage_under(world, hero);
        let class = around.stage_class(ground);
        let stage = if ground == 30000 || class.is_none() {
            FALLBACK_STAGE
        } else {
            ground
        };
        let night = pick::is_night(input.time_of_day);
        let mask = class.map_or(0, ground_mask);
        let zone = pick::battle_zone(&self.zones, night, mask).map_or(hero_state.zone, |z| z.id);
        let own = self.tables.battle_zone(hero_state.zone);
        let preset = pick::preset_company(own, monster, &mut self.crand)
            .and_then(|p| around.event_company(p));
        let company = preset.unwrap_or_else(|| {
            let limit = self
                .zones
                .iter()
                .find(|z| z.id == zone)
                .map_or(5, ZoneDef::party_limit);
            pick::draw_company(
                monster,
                own,
                self.tables.battle_zone(zone),
                limit,
                &mut self.world,
            )
        });
        Some(Contact {
            roamer: slot,
            monster,
            company: company
                .iter()
                .filter(|&&(m, n)| m != 0 && n > 0)
                .map(|&(m, n)| (m, n as u8))
                .collect(),
            stage,
            opening,
            others: Vec::new(),
        })
    }
}

impl Roamer {
    fn previous_state_is_chase(&self) -> bool {
        self.state == state::CHASE
    }
}

/// The touch radius of a roamer of `size` (value 4 × 4096) spawned at model `scale`:
/// `(size / 2) × (scale × 15)` in `fx32`, rounded (`func_02077a20`).
pub fn touch_reach(size: i16, scale: i32) -> i32 {
    let product = i64::from(i32::from(size) >> 1) * i64::from(scale * 15);
    ((product + 0x800) >> 12) as i32
}

/// `func_ov017_02196fc8` with a margin of 0: the party's centre is within a roamer's
/// [`touch_reach`] plus half its own radius, on each axis and in distance, and not on it.
pub fn touches(size: i16, scale: i32, roamer: Fx3, hero: Fx3, hero_radius: i32) -> bool {
    let reach = touch_reach(size, scale) + (hero_radius >> 1);
    if (hero[0] - roamer[0]).abs() > reach || (hero[2] - roamer[2]).abs() > reach {
        return false;
    }
    let distance = fx::distance(hero, roamer);
    distance <= reach && distance != 0
}

/// The stage of the ground under `position`: the record of the floor under it in the last piece
/// that has one (the game keeps it at `Character+0x114`), read as `30000 + 100a + 10b + c`
/// (`func_0204bd7c`); 30000 when there is none.
pub fn stage_under(world: &CollisionWorld, position: Fx3) -> u16 {
    let mut record = None;
    for piece in world.pieces() {
        let local = fx::sub(position, piece.position);
        if let Some(r) = world.meshes()[piece.mesh].ground_record_at(local, 0x1800, 0xa000) {
            record = Some(r.stage());
        }
    }
    record.unwrap_or(30000)
}

/// Where a spawn goes: the node picked and the point brought onto the path to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPlan {
    /// The node picked (an index of the graph).
    pub chosen: usize,
    /// Every node it was picked among, in the game's order.
    pub candidates: Vec<usize>,
    /// Whether one candidate lay ahead of the party (no draw); otherwise `draw` picked.
    pub ahead: bool,
    /// The spawn point before the ground height (`func_020733d8`, `0x02073740`–`0x02073a80`).
    pub position: Fx3,
}

/// The nodes a spawn may go to, in the game's order (`func_020733d8`'s list): the free nodes
/// linked to the party's node and, when the party's own node is more than 5.0 away and free,
/// that node. `taken` says whether a visible roamer stands on the node of an id.
pub fn spawn_candidates(
    graph: &RoamGraph,
    hero: Fx3,
    hero_node: usize,
    taken: &dyn Fn(u8) -> bool,
) -> Vec<usize> {
    let Some(node) = graph.nodes.get(hero_node) else {
        return Vec::new();
    };
    let mut candidates: Vec<usize> = node
        .links
        .iter()
        .copied()
        .filter(|&l| !taken(graph.nodes[l].id))
        .collect();
    if fx::distance(node.fx(), hero) > 0x5000
        && !candidates.iter().any(|&c| graph.nodes[c].id == node.id)
        && !taken(node.id)
    {
        candidates.push(hero_node);
    }
    candidates
}

/// The node choice and the spawn point of `func_020733d8` for a party of one at `hero`
/// facing `angle`, its node `hero_node`. `taken` says whether a visible roamer stands on the
/// node of an id; `draw(n)` is the `rand() % n` the game takes when no candidate lies ahead.
/// `None` where the game gives up (no candidate, or a leader more than `0x9fff` from the path).
pub fn plan_spawn(
    graph: &RoamGraph,
    hero: Fx3,
    angle: i32,
    hero_node: usize,
    taken: &dyn Fn(u8) -> bool,
    draw: &mut dyn FnMut(usize) -> usize,
) -> Option<SpawnPlan> {
    let node = graph.nodes.get(hero_node)?;
    let facing = [fix32_sin(angle), 0, fix32_cos(angle)];
    let candidates = spawn_candidates(graph, hero, hero_node, taken);
    // The one most ahead of the party, else any.
    let mut ahead: Option<(i32, usize)> = None;
    for &c in &candidates {
        let direction = fx::normalize(fx::sub(graph.nodes[c].fx(), hero));
        let dot = fx::dot(facing, direction);
        if dot > 0 && ahead.is_none_or(|(best, _)| best < dot) {
            ahead = Some((dot, c));
        }
    }
    let chosen = match ahead {
        Some((_, c)) => c,
        None if candidates.is_empty() => return None,
        None => candidates[draw(candidates.len())],
    };
    let mut position = graph.nodes[chosen].fx();
    // Far from the party: bring the spawn onto the segment, within ten units of it.
    if chosen != hero_node && fx::distance(position, hero) > 0xa000 {
        let (distance, closest) = segment_distance(position, node.fx(), hero);
        if distance > 0 {
            if distance > 0x9fff {
                return None;
            }
            let square = ((i64::from(distance) * i64::from(distance) + 0x800) >> 12) as i32;
            let ratio = fx::div(square, 0x64000);
            let chord = fx::mul(fx::sqrt(0x1000 - ratio), 0xa000);
            if chord > 0x5000 {
                let toward = fx::sub(position, closest);
                let length = fx::mag(toward);
                let step = fx::scale(fx::normalize(toward), length.min(chord));
                position = fx::add(closest, step);
            }
        }
    }
    Some(SpawnPlan {
        chosen,
        candidates,
        ahead: ahead.is_some(),
        position,
    })
}

/// `func_02031468`: the distance from `point` to the segment `a`–`b` and the nearest point on
/// it.
fn segment_distance(a: Fx3, b: Fx3, point: Fx3) -> (i32, Fx3) {
    let length = fx::distance(a, b);
    let distance = fx::distance(a, point);
    if length == 0 {
        return (distance, a);
    }
    if distance == 0 {
        return (0, a);
    }
    let along = fx::scale(fx::sub(b, a), fx::div(0x1000, length));
    let toward = fx::scale(fx::sub(point, a), fx::div(0x1000, distance));
    let dot = fx::dot(along, toward);
    if dot < 1 {
        return (distance, a);
    }
    let projected = fx::mul(distance, dot);
    if length < projected {
        return (fx::distance(point, b), b);
    }
    let nearest = fx::add(fx::scale(along, projected), a);
    (fx::distance(point, nearest), nearest)
}

#[cfg(test)]
mod tests;
