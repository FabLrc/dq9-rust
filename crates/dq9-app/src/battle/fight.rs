//! A fight played from start to end: the opening, the command entry of every member, the
//! rounds the engine resolves, what each says, and the end (`docs/app.md`, § "Battle").
//!
//! [`Fight`] is a pure state machine over `dq9_core::battle`: no scene, no GPU. It is advanced
//! by [`Fight::step`] (one 60 Hz step and the pad of that step); what it asks of the scene it
//! leaves as [`Cue`]s ([`Fight::take_cues`]) and what the screens show it describes in
//! [`Fight::view`]. The headless play ([`Fight::play_out`]) runs the same machine with an
//! automatic policy and no waiting.

use std::collections::VecDeque;

use anyhow::{Context as _, Result};
use dq9_core::battle::{
    Battle, BattleError, BattleResult, BattleSetup, CRand, Command, Event, PartyMember, Random,
    Skill, Surprise, Target, Who, validate,
};
use dq9_core::party::{LevelUp, Party};

use super::Fighter;
use super::data::Data;
use super::narrate::{Beat, Cue, Narrator, PopupKind, Roster, fighter_of, plain};

/// Ticks a damage number stays over its fighter (**[hypothesis]**).
pub const POPUP_TICKS: u32 = 45;
/// Ticks a window is kept before a key may close it (**[hypothesis]**: a key held through
/// the previous window must not skip the next).
const MIN_SHOWN: u32 = 10;
/// The most steps [`Fight::play_out`] takes (a safety net, not a rule).
const PLAY_OUT_LIMIT: u32 = 20_000;

/// The world's generators, kept by the application across battles: the monsters' HP and the
/// flight draw on [`random`](Self::random), the drops on the C library's [`crand`](Self::crand).
#[derive(Debug, Clone)]
pub struct World {
    pub random: Random,
    pub crand: CRand,
}

impl World {
    /// Both generators from one seed.
    pub fn seeded(seed: u64) -> Self {
        Self {
            random: Random::new(seed),
            // The C `rand` is a 32-bit LCG; fold the seed into it (never 0).
            crand: CRand::new((seed ^ (seed >> 32)) as u32 | 1),
        }
    }
}

/// The d-pad and buttons of one step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pad {
    pub confirm: bool,
    pub cancel: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

/// What a fight is asked for.
#[derive(Debug, Clone)]
pub struct Setup {
    /// `(monster number, count)` per group.
    pub groups: Vec<(u16, u16)>,
    /// A set battle: the table's HP and no flight.
    pub set_battle: bool,
    pub surprise: Surprise,
    /// The party's names as the font draws them (an empty one is replaced by `Hero`).
    pub names: Vec<Vec<u8>>,
}

/// A number or word floating over a fighter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Popup {
    pub fighter: Fighter,
    pub text: String,
    pub kind: PopupKind,
    pub age: u32,
}

/// How a finished fight left the party.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub result: BattleResult,
    pub rounds: u32,
    pub experience: u32,
    pub gold: u32,
    pub drops: Vec<u16>,
    pub level_ups: Vec<LevelUp>,
    /// The party with the battle's HP and MP written back and, on a victory, the award
    /// applied.
    pub party: Party,
}

/// One row of the status window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    pub name: Vec<u8>,
    pub level: u8,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
}

/// A menu the bottom screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuView {
    /// The rows: the label as the font draws it and whether the row can be chosen.
    pub rows: Vec<(Vec<u8>, bool)>,
    pub cursor: usize,
    /// The member being asked.
    pub member: usize,
    /// The rows name monsters: the scene marks the one under the cursor.
    pub targets: bool,
}

/// What the screens show of the fight now.
#[derive(Debug, Clone)]
pub struct View<'a> {
    pub members: Vec<MemberRow>,
    /// The message window's lines.
    pub message: Option<&'a [Vec<u8>]>,
    pub menu: Option<MenuView>,
    pub popups: &'a [Popup],
    /// The fighter the cursor of a target menu is on.
    pub marker: Option<Fighter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    Commands,
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Narrating(After),
    Commands,
    Done,
}

/// A command the menus are building.
#[derive(Debug, Clone)]
enum Pending {
    Attack,
    Spell(usize),
    Item(usize),
}

/// The held slot an item command was built from, kept until the engine reports the use.
#[derive(Debug, Clone, Copy)]
struct Reservation {
    slot: usize,
    id: u16,
    action: u16,
}

#[derive(Debug, Clone)]
enum Menu {
    Top(usize),
    Fight(usize),
    Spells(usize),
    Items(usize),
    Target {
        cursor: usize,
        pending: Pending,
        party: bool,
    },
}

/// A spell the member can cast: its action, name and cost.
#[derive(Debug, Clone)]
struct Spell {
    skill: Skill,
    name: Vec<u8>,
    cost: i32,
}

/// The words of the menus, read once.
#[derive(Debug, Clone, Default)]
struct Labels {
    fight: Vec<u8>,
    flee: Vec<u8>,
    examine: Vec<u8>,
    misc: Vec<u8>,
    attack: Vec<u8>,
    spells: Vec<u8>,
    defend: Vec<u8>,
    items: Vec<u8>,
}

/// What the end of the battle pays, until the closing windows are shown.
#[derive(Debug)]
struct Ending {
    result: BattleResult,
    experience: u32,
    gold: u32,
    drops: Vec<u16>,
    level_ups: Vec<LevelUp>,
}

/// A fight in play.
#[derive(Debug)]
pub struct Fight {
    data: Data,
    battle: Battle,
    roster: Roster,
    monster_names: Vec<Vec<u8>>,
    party: Party,
    labels: Labels,
    /// The held items that do something in battle, for the member being asked.
    item_rows: Vec<ItemRow>,
    attack: Skill,
    spells: Vec<Vec<Spell>>,
    set_battle: bool,
    phase: Phase,
    queue: VecDeque<Beat>,
    current: Option<(Beat, u32)>,
    menu: Menu,
    member: usize,
    chosen: Vec<Option<Command>>,
    /// The held slot each member's item command was built from, released when the command is
    /// changed and spent only by the engine's [`Event::ItemUsed`].
    reserved: Vec<Option<Reservation>>,
    /// HP and MP the status window shows now, and their maxima.
    shown: Vec<(i32, i32)>,
    maxima: Vec<(i32, i32)>,
    popups: Vec<Popup>,
    cues: Vec<Cue>,
    log: Vec<String>,
    outcome: Option<Outcome>,
    auto: bool,
    /// How the fight ends, known once the engine has a result; the outcome once the closing
    /// windows are shown.
    ending: Option<Ending>,
    /// The events of the last resolved round, for the tests that prove an actor was skipped
    /// before it could act.
    #[cfg(test)]
    last_events: Vec<Event>,
}

impl Fight {
    /// Builds the battle of `setup` against `party` from the ROM tables in `data`, the
    /// monsters' HP drawn from `world` unless it is a set battle.
    ///
    /// # Errors
    /// A record the tables lack, or a party the battle cannot hold.
    pub fn new(data: Data, party: &Party, setup: &Setup, world: &mut World) -> Result<Self> {
        let members: Vec<_> = party.members.iter().take(4).collect();
        anyhow::ensure!(!members.is_empty(), "the party is empty");
        let groups: Vec<(u16, u8)> = setup
            .groups
            .iter()
            .map(|&(number, count)| (number, count.clamp(1, 8) as u8))
            .collect();
        let tables = data.tables()?;
        let battle_setup = if setup.set_battle {
            BattleSetup::set_battle(&tables, &groups)
        } else {
            BattleSetup::encounter(&tables, &groups, &mut world.random)
        }
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("building the battle's monsters")?;
        let fighters: Vec<PartyMember> = members
            .iter()
            .map(|c| PartyMember::new(c.fighter(&data.items, &data.params)))
            .collect();
        let attack = tables
            .actions
            .skill(1)
            .context("the Attack action is missing")?;
        let spells: Vec<Vec<Spell>> = members
            .iter()
            .map(|c| {
                data.spells_known(c.vocation, c.level())
                    .into_iter()
                    .filter_map(|action| {
                        let skill = tables.actions.skill(action)?;
                        let lines = data.lines.get(&action)?;
                        Some(Spell {
                            skill,
                            name: lines.name.clone(),
                            cost: i32::from(lines.mp_cost),
                        })
                    })
                    .collect()
            })
            .collect();
        let monster_names: Vec<Vec<u8>> = {
            let names = data.monster_names()?;
            battle_setup
                .monsters
                .iter()
                .map(|m| {
                    names
                        .find(m.number)
                        .map_or_else(|| b"?".to_vec(), |r| r.name.to_vec())
                })
                .collect()
        };
        let roster = Roster {
            party: (0..members.len())
                .map(|i| {
                    setup
                        .names
                        .get(i)
                        .filter(|n| !n.is_empty())
                        .cloned()
                        .unwrap_or_else(|| b"Hero".to_vec())
                })
                .collect(),
            monsters: battle_setup.monsters.iter().map(|m| m.number).collect(),
            items: vec![None; members.len()],
        };
        let seed = (u64::from(world.random.next_u32()) << 32) | u64::from(world.random.next_u32());
        let battle = Battle::new(battle_setup, fighters, setup.surprise, seed)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let (labels, opening) = {
            let narrator = Narrator::new(&data, &roster)?;
            (
                Labels {
                    fight: narrator.label(30000),
                    flee: narrator.label(30001),
                    examine: narrator.label(30002),
                    misc: narrator.label(30003),
                    attack: narrator.label(30004),
                    spells: narrator.label(30005),
                    defend: narrator.label(30006),
                    items: narrator.label(30008),
                },
                narrator.opening(setup.surprise),
            )
        };
        let shown = battle
            .party()
            .iter()
            .map(|m| (m.fighter.hp, m.fighter.mp))
            .collect();
        let maxima = members
            .iter()
            .map(|c| (i32::from(c.max_hp), i32::from(c.max_mp)))
            .collect();
        let count = members.len();
        Ok(Self {
            data,
            battle,
            roster,
            monster_names,
            party: party.clone(),
            labels,
            item_rows: Vec::new(),
            attack,
            spells,
            set_battle: setup.set_battle,
            phase: Phase::Narrating(After::Commands),
            queue: opening.into(),
            current: None,
            menu: Menu::Top(0),
            member: 0,
            chosen: vec![None; count],
            reserved: vec![None; count],
            shown,
            maxima,
            popups: Vec::new(),
            cues: Vec::new(),
            log: Vec::new(),
            outcome: None,
            auto: false,
            ending: None,
            #[cfg(test)]
            last_events: Vec::new(),
        })
    }

    /// The battle the engine runs.
    #[cfg(test)]
    pub fn battle(&self) -> &Battle {
        &self.battle
    }

    /// The events of the last round the engine resolved (the tests prove a skipped actor).
    #[cfg(test)]
    pub fn last_events(&self) -> &[Event] {
        &self.last_events
    }

    /// The party as the fight holds it: held items after a round's consumption.
    #[cfg(test)]
    pub fn party(&self) -> &Party {
        &self.party
    }

    /// Every window shown so far, as plain text (lines joined by `\n`).
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Whether the fight is over and its [`Outcome`] ready.
    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// The outcome, once the fight is over.
    #[cfg(test)]
    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    /// Takes the outcome of a finished fight.
    pub fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }

    /// The scene effects asked since the last call.
    pub fn take_cues(&mut self) -> Vec<Cue> {
        std::mem::take(&mut self.cues)
    }

    /// Runs the fight to its end without waiting: the automatic policy, every window
    /// closing at once. The headless play of `--fight`.
    ///
    /// # Errors
    /// The fight did not end in [`PLAY_OUT_LIMIT`] steps.
    pub fn play_out(&mut self, world: &mut World) -> Result<&Outcome> {
        self.auto = true;
        for _ in 0..PLAY_OUT_LIMIT {
            if self.is_done() {
                break;
            }
            self.step(Pad::default(), world);
            self.cues.clear();
        }
        self.outcome
            .as_ref()
            .context("the fight did not end (the engine stalled)")
    }

    /// One fixed step with the pad of that step.
    pub fn step(&mut self, pad: Pad, world: &mut World) {
        for popup in &mut self.popups {
            popup.age += 1;
        }
        self.popups.retain(|p| p.age < POPUP_TICKS);
        match self.phase {
            Phase::Narrating(after) => self.narrate(after, pad),
            Phase::Commands => self.command(pad, world),
            Phase::Done => {}
        }
    }

    fn narrate(&mut self, after: After, pad: Pad) {
        if self.current.is_none() {
            match self.queue.pop_front() {
                Some(beat) => self.show(beat),
                None => {
                    match after {
                        After::Commands => self.begin_commands(),
                        After::Finish => self.finish(),
                    }
                    return;
                }
            }
        }
        let Some((beat, age)) = &mut self.current else {
            return;
        };
        *age += 1;
        let closes = if self.auto {
            true
        } else if beat.wait {
            pad.confirm && *age >= MIN_SHOWN
        } else {
            *age >= beat.hold || (pad.confirm && *age >= MIN_SHOWN)
        };
        if closes {
            self.current = None;
        }
    }

    /// A window opens: it is logged and its cues apply.
    fn show(&mut self, beat: Beat) {
        if !beat.lines.iter().all(Vec::is_empty) {
            self.log.push(beat.plain());
        }
        for cue in &beat.cues {
            match cue {
                Cue::Hp(i, hp) => {
                    if let Some(shown) = self.shown.get_mut(*i) {
                        shown.0 = (*hp).max(0);
                    }
                }
                Cue::Mp(i, mp) => {
                    if let Some(shown) = self.shown.get_mut(*i) {
                        shown.1 = (*mp).max(0);
                    }
                }
                Cue::Popup {
                    fighter,
                    text,
                    kind,
                } => self.popups.push(Popup {
                    fighter: *fighter,
                    text: text.clone(),
                    kind: *kind,
                    age: 0,
                }),
                Cue::Remove(..) | Cue::Summon(..) => {}
            }
        }
        self.cues.extend(beat.cues.iter().cloned());
        self.current = Some((beat, 0));
    }

    // ---- command entry ----

    fn begin_commands(&mut self) {
        self.phase = Phase::Commands;
        self.chosen = (0..self.battle.party().len())
            .map(|i| (!self.battle.alive(Who::Party(i))).then_some(Command::Skip))
            .collect();
        self.reserved = vec![None; self.chosen.len()];
        self.member = self.chosen.iter().position(Option::is_none).unwrap_or(0);
        self.menu = Menu::Top(0);
        self.roster.items = vec![None; self.chosen.len()];
        self.refresh_items();
        // The status window shows what the engine holds, whatever the cues said.
        for (i, member) in self.battle.party().iter().enumerate() {
            self.shown[i] = (member.fighter.hp.max(0), member.fighter.mp.max(0));
        }
    }

    fn standing_monsters(&self) -> Vec<usize> {
        (0..self.battle.monsters().len())
            .filter(|&j| self.battle.alive(Who::Monster(j)))
            .collect()
    }

    fn standing_party(&self) -> Vec<usize> {
        (0..self.battle.party().len())
            .filter(|&i| self.battle.alive(Who::Party(i)))
            .collect()
    }

    fn can_cast(&self, member: usize, spell: &Spell) -> bool {
        let mp = self.battle.party()[member].fighter.mp;
        self.modelled(member, &spell.skill)
            && if spell.cost >= 255 {
                mp > 0
            } else {
                mp >= spell.cost
            }
    }

    fn command(&mut self, pad: Pad, world: &mut World) {
        if self.auto {
            let target = self.standing_monsters().first().copied().unwrap_or(0);
            for i in 0..self.chosen.len() {
                if self.chosen[i].is_none() {
                    self.chosen[i] = Some(Command::Act {
                        skill: self.attack.clone(),
                        target: Target::Monster(target),
                    });
                }
            }
            self.run_round(world);
            return;
        }
        let member = self.member;
        let dir = i32::from(pad.down || pad.right) - i32::from(pad.up || pad.left);
        let monsters = self.standing_monsters();
        let party = self.standing_party();
        let menu = self.menu.clone();
        match menu {
            Menu::Top(cursor) => {
                let cursor = step_cursor(cursor, dir, 4);
                self.menu = Menu::Top(cursor);
                if pad.cancel {
                    self.previous_member();
                } else if pad.confirm {
                    match cursor {
                        0 => self.menu = Menu::Fight(0),
                        1 if !self.set_battle => {
                            self.chosen[member] = Some(Command::Flee);
                            self.next_member(world);
                        }
                        _ => {}
                    }
                }
            }
            Menu::Fight(cursor) => {
                let cursor = step_cursor(cursor, dir, 4);
                self.menu = Menu::Fight(cursor);
                if pad.cancel {
                    self.menu = Menu::Top(0);
                } else if pad.confirm {
                    match cursor {
                        0 => {
                            self.menu = Menu::Target {
                                cursor: 0,
                                pending: Pending::Attack,
                                party: false,
                            }
                        }
                        1 if !self.spells[member].is_empty() => self.menu = Menu::Spells(0),
                        2 => {
                            self.chosen[member] = Some(Command::Defend);
                            self.next_member(world);
                        }
                        3 if self.item_rows.iter().any(|r| r.enabled) => self.menu = Menu::Items(0),
                        _ => {}
                    }
                }
            }
            Menu::Spells(cursor) => {
                let count = self.spells[member].len().max(1);
                let cursor = step_cursor(cursor, dir, count);
                self.menu = Menu::Spells(cursor);
                if pad.cancel {
                    self.menu = Menu::Fight(1);
                } else if pad.confirm
                    && let Some(spell) = self.spells[member].get(cursor).cloned()
                    && self.can_cast(member, &spell)
                {
                    self.aim(member, spell.skill, Pending::Spell(cursor), world);
                }
            }
            Menu::Items(cursor) => {
                let count = self.item_rows.len().max(1);
                let cursor = step_cursor(cursor, dir, count);
                self.menu = Menu::Items(cursor);
                if pad.cancel {
                    self.menu = Menu::Fight(3);
                } else if pad.confirm
                    && let Some(row) = self.item_rows.get(cursor).cloned()
                    && row.enabled
                {
                    self.aim(member, row.skill, Pending::Item(cursor), world);
                }
            }
            Menu::Target {
                cursor,
                pending,
                party: own_side,
            } => {
                let rows = if own_side {
                    party.len()
                } else {
                    monsters.len()
                };
                let cursor = step_cursor(cursor, dir, rows.max(1));
                self.menu = Menu::Target {
                    cursor,
                    pending: pending.clone(),
                    party: own_side,
                };
                if pad.cancel {
                    self.menu = match pending {
                        Pending::Attack => Menu::Fight(0),
                        Pending::Spell(k) => Menu::Spells(k),
                        Pending::Item(k) => Menu::Items(k),
                    };
                } else if pad.confirm {
                    let target = if own_side {
                        party.get(cursor).map(|&i| Target::Party(i))
                    } else {
                        monsters.get(cursor).map(|&j| Target::Monster(j))
                    };
                    let skill = match &pending {
                        Pending::Attack => Some(self.attack.clone()),
                        Pending::Spell(k) => self.spells[member].get(*k).map(|s| s.skill.clone()),
                        Pending::Item(k) => self.item_rows.get(*k).map(|r| r.skill.clone()),
                    };
                    if let (Some(target), Some(skill)) = (target, skill) {
                        self.give(member, skill, &pending, target);
                        self.next_member(world);
                    }
                }
            }
        }
    }

    fn previous_member(&mut self) {
        let before = (0..self.member)
            .rev()
            .find(|&i| self.battle.alive(Who::Party(i)));
        if let Some(i) = before {
            self.chosen[i] = None;
            self.reserved[i] = None;
            self.roster.items[i] = None;
            self.member = i;
            self.menu = Menu::Top(0);
            self.refresh_items();
        }
    }

    fn next_member(&mut self, world: &mut World) {
        match self.chosen.iter().position(Option::is_none) {
            Some(next) => {
                self.member = next;
                self.menu = Menu::Top(0);
                self.refresh_items();
            }
            None => self.run_round(world),
        }
    }

    /// Monsters that joined the battle (a call for backup) are named and counted like the
    /// others from now on.
    fn sync_monsters(&mut self) {
        if self.battle.monsters().len() == self.roster.monsters.len() {
            return;
        }
        let Ok(names) = self.data.monster_names() else {
            return;
        };
        self.roster.monsters = self.battle.monsters().iter().map(|m| m.number).collect();
        self.monster_names = self
            .battle
            .monsters()
            .iter()
            .map(|m| {
                names
                    .find(m.number)
                    .map_or_else(|| b"?".to_vec(), |r| r.name.to_vec())
            })
            .collect();
    }

    /// Whether the engine models `skill` for `member` against a standing opponent (or the
    /// member, for a skill aimed at their own side): what the menus grey otherwise.
    fn modelled(&self, member: usize, skill: &Skill) -> bool {
        let Ok(action) = &skill.action else {
            return false;
        };
        let actor = self.battle.party()[member].fighter.clone();
        let target = if skill.aims == 2 {
            actor.clone()
        } else {
            match self.standing_monsters().first() {
                Some(&j) => self.battle.monsters()[j].fighter.clone(),
                None => return false,
            }
        };
        validate(&actor, action, std::slice::from_ref(&target)).is_ok()
    }

    /// The held items of the member whose menu opens that do something in battle: every
    /// non-empty slot of [`Character::held_items`](dq9_core::party::Character::held_items)
    /// whose item record names a battle action. The row keeps its slot so that the item used
    /// is the one chosen, duplicates included (the original's item command carries the held
    /// slot, `ov_017::021d0164`; the record's second action is what it resolves).
    fn refresh_items(&mut self) {
        self.item_rows.clear();
        let Ok(tables) = self.data.tables() else {
            return;
        };
        let Ok(names) = self.data.item_names() else {
            return;
        };
        let member = self.member.min(self.battle.party().len().saturating_sub(1));
        let Some(character) = self.party.members.get(member) else {
            return;
        };
        let held = character.held_items;
        for (slot, id) in held.iter().enumerate() {
            let Some(id) = *id else { continue };
            let Some(skill) = self
                .data
                .battle_action(id)
                .and_then(|action| tables.actions.skill(action))
            else {
                continue;
            };
            let label = names.find(id).map_or_else(Vec::new, |i| i.name.to_vec());
            let enabled = self.modelled(member, &skill);
            self.item_rows.push(ItemRow {
                slot,
                id,
                label,
                skill,
                enabled,
            });
        }
    }

    /// Settles the command of `member`: the skill goes to its target, or asks for one.
    fn aim(&mut self, member: usize, skill: Skill, pending: Pending, world: &mut World) {
        let own_side = skill.aims == 2;
        if matches!(skill.reach, 1 | 3) {
            let target = if own_side {
                Target::Party(member)
            } else {
                Target::Monster(self.standing_monsters().first().copied().unwrap_or(0))
            };
            self.give(member, skill, &pending, target);
            self.next_member(world);
        } else {
            self.menu = Menu::Target {
                cursor: 0,
                pending,
                party: own_side,
            };
        }
    }

    fn give(&mut self, member: usize, skill: Skill, pending: &Pending, target: Target) {
        self.chosen[member] = Some(match pending {
            Pending::Item(k) => {
                let row = self.item_rows.get(*k);
                self.reserved[member] = row.map(|r| Reservation {
                    slot: r.slot,
                    id: r.id,
                    action: skill.number,
                });
                self.roster.items[member] = row.map(|r| r.id);
                Command::Item { skill, target }
            }
            _ => {
                self.reserved[member] = None;
                self.roster.items[member] = None;
                Command::Act { skill, target }
            }
        });
    }

    /// Spends one held item: its slot becomes empty, then one pass shifts each hole one rank
    /// down (`func_0208386c`, EU `0x0208387c`: set the slot to `0xffff`, then for `i` in
    /// `slot..7` move `i + 1` into an empty `i`). The other slots, duplicates of the same id
    /// included, keep their order.
    fn consume_held(&mut self, member: usize, slot: usize) {
        let Some(character) = self.party.members.get_mut(member) else {
            return;
        };
        let items = &mut character.held_items;
        let Some(last) = items.len().checked_sub(1) else {
            return;
        };
        if slot > last {
            return;
        }
        items[slot] = None;
        for i in slot..last {
            if items[i].is_none() {
                items[i] = items[i + 1];
                items[i + 1] = None;
            }
        }
    }

    // ---- the round and the end ----

    fn run_round(&mut self, world: &mut World) {
        let commands: Vec<Command> = self
            .chosen
            .iter()
            .map(|c| c.clone().unwrap_or(Command::Skip))
            .collect();
        match self.battle.round(&commands, &mut world.random) {
            Ok(report) => {
                let finished = self.battle.result().is_some();
                self.sync_monsters();
                #[cfg(test)]
                {
                    self.last_events = report.events.clone();
                }
                // An item used is spent from the held slot its command was built from, once,
                // and only when the record says it is used up (`+0x08` bit 19). A member
                // skipped before its turn produces no `ItemUsed` and keeps the item; the bag
                // is never read or changed here. Each reservation is taken in place, so the
                // round allocates nothing for it.
                let mut spent = [(0usize, 0usize); 4];
                let mut spent_count = 0;
                for event in &report.events {
                    let Event::ItemUsed {
                        who: Who::Party(i),
                        action,
                    } = event
                    else {
                        continue;
                    };
                    let Some(reservation) = self.reserved.get_mut(*i).and_then(Option::take) else {
                        continue;
                    };
                    if reservation.action != *action {
                        self.reserved[*i] = Some(reservation);
                        continue;
                    }
                    if self
                        .data
                        .item_meta
                        .get(&reservation.id)
                        .is_some_and(|meta| meta.used_up)
                        && spent_count < spent.len()
                    {
                        spent[spent_count] = (*i, reservation.slot);
                        spent_count += 1;
                    }
                }
                for &(member, slot) in &spent[..spent_count] {
                    self.consume_held(member, slot);
                }
                let mut beats = match Narrator::new(&self.data, &self.roster) {
                    Ok(narrator) => narrator.round(&report),
                    Err(e) => {
                        crate::app::status(&format!("battle text: {e:#}"));
                        Vec::new()
                    }
                };
                let mp: Vec<Cue> = self
                    .battle
                    .party()
                    .iter()
                    .enumerate()
                    .map(|(i, m)| Cue::Mp(i, m.fighter.mp))
                    .collect();
                match beats.last_mut() {
                    Some(last) => last.cues.extend(mp),
                    None => self.cues.extend(mp),
                }
                if finished {
                    beats.extend(self.closing(world));
                }
                self.queue = beats.into();
                self.phase = Phase::Narrating(if finished {
                    After::Finish
                } else {
                    After::Commands
                });
            }
            Err(BattleError::Unsupported(u)) => {
                // The engine cannot resolve a rule of this round (a monster's way, say) and
                // leaves the battle as it was: the same round would fail again.
                crate::app::status(&format!("battle: not modelled, the fight stops: {u}"));
                self.stall();
            }
            Err(e) => {
                crate::app::status(&format!("battle: round refused: {e}"));
                self.begin_commands();
            }
        }
    }

    /// A rule the engine does not model ended the fight: the party leaves it as a flight
    /// would (**[hypothesis]**: the game has no such state).
    fn stall(&mut self) {
        self.queue.clear();
        self.current = None;
        self.ending = Some(Ending {
            result: BattleResult::Fled,
            experience: 0,
            gold: 0,
            drops: Vec::new(),
            level_ups: Vec::new(),
        });
        self.finish();
    }

    /// The engine's result is in: the closing windows and the ending the outcome will hold.
    /// The drops roll on the world's C generator.
    fn closing(&mut self, world: &mut World) -> Vec<Beat> {
        let result = self.battle.result().unwrap_or(BattleResult::Fled);
        self.write_back();
        let mut ending = Ending {
            result,
            experience: 0,
            gold: 0,
            drops: Vec::new(),
            level_ups: Vec::new(),
        };
        let mut beats = Vec::new();
        match result {
            BattleResult::Won => match self.battle.finish(&mut world.crand) {
                Ok(end) => {
                    ending.level_ups =
                        self.party
                            .award(end.experience, end.gold, &end.drops, &self.data.levels);
                    let shared = self.standing_party().len() > 1;
                    let drops: Vec<(u16, Option<usize>)> = end
                        .drops
                        .iter()
                        .map(|&item| {
                            let from = self
                                .battle
                                .monsters()
                                .iter()
                                .position(|m| m.fighter.hp <= 0 && m.drops.contains(&item));
                            (item, from)
                        })
                        .collect();
                    if let (Ok(narrator), Ok(items)) = (
                        Narrator::new(&self.data, &self.roster),
                        self.data.item_names(),
                    ) {
                        beats =
                            narrator.victory(shared, end.gold, &ending.level_ups, &drops, &items);
                    }
                    ending.experience = end.experience;
                    ending.gold = end.gold;
                    ending.drops = end.drops;
                }
                Err(e) => crate::app::status(&format!("battle: {e}")),
            },
            BattleResult::Lost => {
                if let Ok(narrator) = Narrator::new(&self.data, &self.roster) {
                    beats = narrator.wipe_out();
                }
            }
            BattleResult::Fled => {}
        }
        self.ending = Some(ending);
        beats
    }

    /// The windows are shown: the fight is over.
    fn finish(&mut self) {
        let ending = self.ending.take().unwrap_or(Ending {
            result: BattleResult::Fled,
            experience: 0,
            gold: 0,
            drops: Vec::new(),
            level_ups: Vec::new(),
        });
        self.outcome = Some(Outcome {
            result: ending.result,
            rounds: self.battle.round_number(),
            experience: ending.experience,
            gold: ending.gold,
            drops: ending.drops,
            level_ups: ending.level_ups,
            party: self.party.clone(),
        });
        self.phase = Phase::Done;
    }

    fn write_back(&mut self) {
        for (member, fighter) in self.party.members.iter_mut().zip(self.battle.party()) {
            member.set_hp(fighter.fighter.hp.max(0) as u16);
            member.set_mp(fighter.fighter.mp.max(0) as u16);
        }
    }

    // ---- what the screens show ----

    /// What the screens show now.
    pub fn view(&self) -> View<'_> {
        let members = self
            .party
            .members
            .iter()
            .enumerate()
            .take(self.shown.len())
            .map(|(i, c)| MemberRow {
                name: self.roster.party[i].clone(),
                level: c.level(),
                hp: self.shown[i].0,
                max_hp: self.maxima[i].0,
                mp: self.shown[i].1,
                max_mp: self.maxima[i].1,
            })
            .collect();
        let message = self.current.as_ref().map(|(beat, _)| beat.lines.as_slice());
        let (menu, marker) = if self.phase == Phase::Commands {
            self.menu_view()
        } else {
            (None, None)
        };
        View {
            members,
            message,
            menu,
            popups: &self.popups,
            marker,
        }
    }

    fn menu_view(&self) -> (Option<MenuView>, Option<Fighter>) {
        let member = self.member;
        let row = |label: &[u8], enabled: bool| (label.to_vec(), enabled);
        match &self.menu {
            Menu::Top(cursor) => (
                Some(MenuView {
                    rows: vec![
                        row(&self.labels.fight, true),
                        row(&self.labels.flee, !self.set_battle),
                        row(&self.labels.examine, false),
                        row(&self.labels.misc, false),
                    ],
                    cursor: *cursor,
                    member,
                    targets: false,
                }),
                None,
            ),
            Menu::Fight(cursor) => (
                Some(MenuView {
                    rows: vec![
                        row(&self.labels.attack, true),
                        row(&self.labels.spells, !self.spells[member].is_empty()),
                        row(&self.labels.defend, true),
                        row(&self.labels.items, self.item_rows.iter().any(|r| r.enabled)),
                    ],
                    cursor: *cursor,
                    member,
                    targets: false,
                }),
                None,
            ),
            Menu::Spells(cursor) => (
                Some(MenuView {
                    rows: self.spells[member]
                        .iter()
                        .map(|s| {
                            let mut label = s.name.clone();
                            label.extend_from_slice(format!("  {}", s.cost).as_bytes());
                            (label, self.can_cast(member, s))
                        })
                        .collect(),
                    cursor: *cursor,
                    member,
                    targets: false,
                }),
                None,
            ),
            Menu::Items(cursor) => (
                Some(MenuView {
                    rows: self
                        .item_rows
                        .iter()
                        .map(|r| (r.label.clone(), r.enabled))
                        .collect(),
                    cursor: *cursor,
                    member,
                    targets: false,
                }),
                None,
            ),
            Menu::Target {
                cursor,
                party: own_side,
                ..
            } => {
                let ids: Vec<Who> = if *own_side {
                    self.standing_party().into_iter().map(Who::Party).collect()
                } else {
                    self.standing_monsters()
                        .into_iter()
                        .map(Who::Monster)
                        .collect()
                };
                let rows = ids
                    .iter()
                    .map(|w| match w {
                        Who::Party(i) => row(&self.roster.party[*i], true),
                        Who::Monster(j) => row(&self.monster_names[*j], true),
                    })
                    .collect();
                (
                    Some(MenuView {
                        rows,
                        cursor: *cursor,
                        member,
                        targets: true,
                    }),
                    ids.get(*cursor).map(|&w| fighter_of(w)),
                )
            }
        }
    }
}

/// The cursor after `dir` rows in a list of `count`, wrapping.
fn step_cursor(cursor: usize, dir: i32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    (cursor as i32 + dir).rem_euclid(count as i32) as usize
}

/// A window's text as plain strings (for the tests).
pub fn lines_plain(lines: &[Vec<u8>]) -> String {
    plain(&lines.join(&b'\n'))
}

/// A held item of the acting member that does something in battle, as the Items menu lists it.
#[derive(Debug, Clone)]
struct ItemRow {
    /// The held slot the item occupies; the command must spend this one, not the id.
    slot: usize,
    id: u16,
    /// Name as the font draws it.
    label: Vec<u8>,
    skill: Skill,
    /// The engine models its action for this member.
    enabled: bool,
}
