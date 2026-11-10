//! A battle as the original plays it, round after round, headless and deterministic.
//!
//! [`Battle`] holds the fighters, the battle's generator and the little the flow keeps from one
//! round to the next (the round counter, the flight attempts, the once-a-battle ways, the combo
//! counter, the monsters beaten). [`Battle::round`] takes the party's commands, the **world's**
//! generator (the flight draws from it, like a monster's HP) and plays one round:
//!
//! 1. **Flight** (`func_ov026_021dd3dc` → `func_ov000_0215f7a8`, EU same address): the members
//!    that chose Flee; success ends the battle with no round.
//! 2. **Initiative** (`ProcessCombatTurn`, `ov_000::0215d63c`): every standing combatant, the
//!    party first then the monsters group by group, is scored `agility × NextRandomFloatBetween(
//!    0.51, 1.0)`; the surprise round leaves some out; the scores are sorted, highest first.
//! 3. **Commands**, in that order: a monster chooses its way and targets
//!    ([`ai`](super::ai)), then draws `NextRandomMax(2)` for its extra actions
//!    (`func_ov000_0215f57c`); a party member's command is the caller's.
//! 4. **Turn classes** (`func_ov000_0215f1ac`): classes 1, 5 and 10 move to the front, class 32
//!    to the back.
//! 5. **Execution**, record by record: `NextRandomMax(100)` (`func_ov000_0215833c`, the actor's
//!    status roll), the targets settled ([`ai`](super::ai)), [`resolve_action`], the end test
//!    (`func_ov000_02157d3c`) and, while the battle goes on, `NextRandomMax(100)` again
//!    (`func_ov000_0215858c`, the actor's status tick).
//!
//! Every step that is not modelled returns [`BattleError::Unsupported`] and leaves the battle
//! exactly as it was before the round, generator included. What is a **[hypothesis]** is marked
//! where it is made and in `docs/re/battle.md`.
//!
//! Nothing here is marked as ported: `ProcessCombatTurn` is translated for a battle with no
//! status, no tension, no guard, no co-op action and no grotto.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use super::end::{BattleResult, Kind};
use super::model::{Fighter, Side, Unsupported};
use super::random::Random;
use super::resolve::{TargetOutcome, resolve_action};
use super::setup::{BattleSetup, MAX_GROUPS, Monster, SetupError, Skill};

/// A combatant: a party slot or a monster's index in [`Battle::monsters`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    /// A party member by slot (0 to 3).
    Party(usize),
    /// A monster by index.
    Monster(usize),
}

/// How the fight opens (`[battle + 0xe49]`, `func_ov000_0215d414`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surprise {
    /// An even start.
    None,
    /// 1: the party has the jump: the monsters sit the first round out.
    Party,
    /// 2: the monsters have the jump: the party sits it out.
    Monsters,
}

/// A party member in the battle.
#[derive(Clone, Debug, PartialEq)]
pub struct PartyMember {
    /// The battle numbers (built by the caller: `party::Character::fighter`).
    pub fighter: Fighter,
    /// The Back Line (`base + 0x3c` bit 30): weighs 1 in a monster's pick instead of 2.
    pub back_line: bool,
    /// Their weapon grants the first move to an action that carries `+0x10` bit 19
    /// (`func_02085180`, `0x02085190`: bit 6 of the weapon's `itembtlprm` word). **[unknown]**
    /// which weapons; false unless the caller knows.
    pub weapon_acts_first: bool,
}

impl PartyMember {
    /// A front-line member with no special weapon.
    pub fn new(fighter: Fighter) -> Self {
        Self {
            fighter,
            back_line: false,
            weapon_acts_first: false,
        }
    }
}

/// Whom a party command aims at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A monster by index; a group or everyone reach widens it to its group or all.
    Monster(usize),
    /// A party member by slot.
    Party(usize),
}

/// One party member's command for a round.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// An action: the Attack, a spell (an item is an action of the action table, see
    /// [`Command::Item`]).
    Act {
        /// The action (from [`ActionTables::skill`](super::setup::ActionTables::skill)).
        skill: Skill,
        /// Whom it aims at.
        target: Target,
    },
    /// Use an item: its action table entry, resolved like any action. The engine reports it
    /// ([`Event::ItemUsed`]); taking it out of the bag is the caller's (`dq9_core::party`).
    Item {
        /// The item's action (from the item table, through the caller).
        skill: Skill,
        /// Whom it aims at.
        target: Target,
    },
    /// Defend: the guard level goes to 1 for the round (`func_ov024_021e57c0`), cleared at the
    /// round's end (`func_ov000_0215e6e8`).
    Defend,
    /// Flee.
    Flee,
    /// For a member who is down.
    Skip,
}

/// Why a round was not played. Nothing changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleError {
    /// A rule that is not modelled or not established.
    Unsupported(Unsupported),
    /// The caller's commands do not fit the battle.
    Invalid(&'static str),
    /// The battle is over already.
    Over,
    /// Flight from a set battle: the setup forbids it and never lets go
    /// (`func_ov000_0215f7a8`, `[setup + 0x0C]` not negative).
    FleeForbidden,
}

impl fmt::Display for BattleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(u) => write!(f, "{u}"),
            Self::Invalid(why) => write!(f, "invalid command: {why}"),
            Self::Over => write!(f, "the battle is over"),
            Self::FleeForbidden => write!(f, "the party cannot flee from a set battle"),
        }
    }
}

impl Error for BattleError {}

impl From<Unsupported> for BattleError {
    fn from(u: Unsupported) -> Self {
        Self::Unsupported(u)
    }
}

/// What happened in a round, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The party tried to flee: the percentage drawn against, if there was a draw.
    Flee {
        /// Whether they got away.
        success: bool,
        /// The percentage the world's `NextRandomMax(100)` was held under.
        percent: Option<i32>,
    },
    /// The order of the round with each combatant's score.
    Initiative(Vec<(Who, f32)>),
    /// A monster chose its way.
    Chose {
        /// The monster.
        who: Who,
        /// The action's number.
        action: u16,
        /// Its targets as chosen at the command.
        targets: Vec<Who>,
    },
    /// An action resolved.
    Acted {
        /// The actor.
        who: Who,
        /// The action's number.
        action: u16,
        /// What happened to each target, in order.
        outcomes: Vec<(Who, TargetOutcome)>,
    },
    /// A party member defended: its guard level is 1 until the round's end.
    Defended(Who),
    /// An item's action resolved; the caller takes one of it out of the bag.
    ItemUsed {
        /// The user.
        who: Who,
        /// The item's action number.
        action: u16,
    },
    /// A monster called for backup (`func_ov024_021dc2a0`): another of its kind joined.
    Summoned {
        /// The caller.
        who: Who,
        /// The monster number that joined.
        number: u16,
    },
    /// A combatant did not act: fallen before its turn, or its flight failed.
    Skipped(Who),
    /// The battle ended.
    End(BattleResult),
}

/// One round's log.
#[derive(Clone, Debug, PartialEq)]
pub struct RoundReport {
    /// The round's number, 0 for the first.
    pub round: u32,
    /// What happened.
    pub events: Vec<Event>,
}

/// What a turn record does.
#[derive(Clone, Debug)]
enum Plan {
    Party {
        skill: Skill,
        target: Target,
        item: bool,
    },
    Monster {
        action: u16,
        targets: Vec<Who>,
    },
    Idle,
}

/// What a combatant's battle status holds of the chapter 2 actions (`[combatant + 0x138]` in the
/// original; `docs/re/battle.md`, "Statuses"). Levels are held to two either way; a count is the
/// passes the status still holds, a second count the wear-off rolls once it has run out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Defence level, `+0x58` bits 3–5, and its counts `+0x6f` and `+0x92` (Buff, Sap, Kasap).
    pub defence_level: i8,
    /// Passes the defence level still holds (`+0x6f`).
    pub defence_count: u8,
    /// Wear-off rolls left at defence (`+0x92`).
    pub defence_second: u8,
    /// Agility level, `+0x58` bits 6–8, and its counts `+0x70` and `+0x93` (Accelerate,
    /// Deceleratle).
    pub agility_level: i8,
    /// Passes the agility level still holds (`+0x70`).
    pub agility_count: u8,
    /// Wear-off rolls left at agility (`+0x93`).
    pub agility_second: u8,
    /// Passes the dazzle still holds (`+0x5f`), its wear-off `+0x82` (`func_02088854`).
    pub dazzle_count: u8,
    /// Wear-off rolls left at dazzle (`+0x82`).
    pub dazzle_second: u8,
    /// Guard level 0 to 3 (`+0x21`): 1 while defending, cleared at the round's end.
    pub guard: u8,
}

impl Status {
    /// The defence multiplier of a level (`CalculateDefenseBuffMultiplier`): 1 + 0.5 a level up,
    /// 0.5 at −1 and 0.25 at −2.
    fn defence_multiplier(&self) -> f32 {
        Self::level_multiplier(self.defence_level)
    }

    fn agility_multiplier(&self) -> f32 {
        Self::level_multiplier(self.agility_level)
    }

    fn level_multiplier(level: i8) -> f32 {
        if level >= 0 {
            1.0f32 + 0.5f32 * f32::from(level)
        } else {
            1.0f32 + (f32::from(level + 1) * 0.25f32 - 0.5f32)
        }
    }
}

/// The chance a wear-off roll clears a status with `second` rolls left (`0x02182ad4`:
/// 1.0, 0.875, 0.75, 0.625, read from the fourth roll down).
fn wear_off_chance(second: u8) -> f32 {
    match second {
        4 => 0.625,
        3 => 0.75,
        2 => 0.875,
        _ => 1.0,
    }
}

#[derive(Clone, Debug)]
struct Record {
    who: Who,
    plan: Plan,
}

/// The combo counter (`[battle + 0x8e82]`…`+0x8e83`): the side, action and target of the last
/// blow, the turn record it came from and the count.
#[derive(Clone, Debug, Default)]
struct Combo {
    side: Option<Side>,
    action: Option<u16>,
    target: Option<Who>,
    turn: Option<usize>,
    count: u8,
}

/// A battle in progress.
#[derive(Clone, Debug)]
pub struct Battle {
    pub(super) rng: Random,
    pub(super) party: Vec<PartyMember>,
    pub(super) monsters: Vec<Monster>,
    pub(super) party_status: Vec<Status>,
    pub(super) monster_status: Vec<Status>,
    /// The monsters that left through action `0xE1` (`func_ov000_021554f4`'s reason 2): gone, and
    /// worth no experience.
    pub(super) fled: Vec<bool>,
    pub(super) skills: BTreeMap<u16, Skill>,
    pub(super) set_battle: bool,
    pub(super) surprise: Surprise,
    pub(super) round: u32,
    flee_attempts: usize,
    pub(super) once_used: [u8; MAX_GROUPS],
    pub(super) way_counts: Vec<u8>,
    extras_called: Vec<bool>,
    combo: Combo,
    pub(super) kills: Vec<Kind>,
    pub(super) experience: u32,
    pub(super) gold: u32,
    pub(super) result: Option<BattleResult>,
}

/// The party's flight floors by attempt (`0x02182c04`, read on the European ROM): the fifth
/// always gets away.
const FLEE_FLOOR: [i32; 5] = [25, 50, 75, 100, 65535];

/// What the world's generator is held under by the surprise-round roll of a monster after the
/// first (`NextRandomMax(100) < 67`).
const SURPRISE_ROLL: i32 = 0x42;

impl Battle {
    /// A battle of `setup` against `party` (one to four members), the battle's generator seeded
    /// with `seed`.
    ///
    /// # Errors
    /// [`SetupError::Shape`] for a party of none or of more than four.
    pub fn new(
        setup: BattleSetup,
        party: Vec<PartyMember>,
        surprise: Surprise,
        seed: u64,
    ) -> Result<Self, SetupError> {
        if party.is_empty() || party.len() > 4 {
            return Err(SetupError::Shape);
        }
        let n = setup.monsters.len();
        Ok(Self {
            rng: Random::new(seed),
            party_status: vec![Status::default(); party.len()],
            monster_status: vec![Status::default(); n],
            fled: vec![false; n],
            party,
            monsters: setup.monsters,
            skills: setup.skills,
            set_battle: setup.set_battle,
            surprise,
            round: 0,
            flee_attempts: 0,
            once_used: [0; MAX_GROUPS],
            way_counts: vec![0; n],
            extras_called: vec![false; n],
            combo: Combo::default(),
            kills: Vec::new(),
            experience: 0,
            gold: 0,
            result: None,
        })
    }

    /// The party.
    pub fn party(&self) -> &[PartyMember] {
        &self.party
    }

    /// The monsters, group by group.
    pub fn monsters(&self) -> &[Monster] {
        &self.monsters
    }

    /// The number of rounds played.
    pub fn round_number(&self) -> u32 {
        self.round
    }

    /// How the battle ended, `None` while it goes on.
    pub fn result(&self) -> Option<BattleResult> {
        self.result
    }

    /// The battle generator's state (for chaining and for tests).
    pub fn generator_state(&self) -> u64 {
        self.rng.state()
    }

    /// Whether `who` is standing: HP above 0 and not gone.
    pub fn alive(&self, who: Who) -> bool {
        match who {
            Who::Party(i) => self.party[i].fighter.hp > 0,
            Who::Monster(i) => self.monsters[i].fighter.hp > 0,
        }
    }

    pub(super) fn party_standing(&self) -> Vec<usize> {
        (0..self.party.len())
            .filter(|&i| self.alive(Who::Party(i)))
            .collect()
    }

    pub(super) fn monsters_standing(&self) -> Vec<usize> {
        (0..self.monsters.len())
            .filter(|&i| self.alive(Who::Monster(i)))
            .collect()
    }

    fn fighter(&self, who: Who) -> &Fighter {
        match who {
            Who::Party(i) => &self.party[i].fighter,
            Who::Monster(i) => &self.monsters[i].fighter,
        }
    }

    /// The status of `who`.
    pub fn status(&self, who: Who) -> &Status {
        match who {
            Who::Party(i) => &self.party_status[i],
            Who::Monster(i) => &self.monster_status[i],
        }
    }

    fn status_mut(&mut self, who: Who) -> &mut Status {
        match who {
            Who::Party(i) => &mut self.party_status[i],
            Who::Monster(i) => &mut self.monster_status[i],
        }
    }

    /// Whether `who` left the battle through the monsters' flight.
    pub fn has_fled(&self, who: Who) -> bool {
        match who {
            Who::Party(_) => false,
            Who::Monster(i) => self.fled[i],
        }
    }

    /// The numbers the resolver reads for `who`: its base numbers with the defence and agility
    /// levels applied and `RoundUp`-ed (`UpdateCombatantDefence`, `…Agility`), its guard level
    /// and its dazzle.
    fn fighter_snapshot(&self, who: Who) -> Fighter {
        let mut fighter = self.fighter(who).clone();
        let status = self.status(who);
        fighter.guard_level = status.guard;
        fighter.dazzled = status.dazzle_count > 0;
        if status.defence_level != 0 {
            fighter.defence =
                super::damage::round_up(fighter.defence as f32 * status.defence_multiplier());
        }
        if status.agility_level != 0 {
            fighter.agility =
                super::damage::round_up(fighter.agility as f32 * status.agility_multiplier());
        }
        fighter
    }

    fn fighter_mut(&mut self, who: Who) -> &mut Fighter {
        match who {
            Who::Party(i) => &mut self.party[i].fighter,
            Who::Monster(i) => &mut self.monsters[i].fighter,
        }
    }

    /// Plays one round: `commands` holds one command per party slot ([`Command::Skip`] for a
    /// member who is down), `world` is the world's generator (the flight draws from it).
    ///
    /// # Errors
    /// [`BattleError`]; the battle is left exactly as it was.
    pub fn round(
        &mut self,
        commands: &[Command],
        world: &mut Random,
    ) -> Result<RoundReport, BattleError> {
        if self.result.is_some() {
            return Err(BattleError::Over);
        }
        if commands.len() != self.party.len() {
            return Err(BattleError::Invalid("one command per party slot"));
        }
        for (i, command) in commands.iter().enumerate() {
            let standing = self.alive(Who::Party(i));
            match command {
                Command::Skip if standing => {
                    return Err(BattleError::Invalid(
                        "a standing member must give a command",
                    ));
                }
                Command::Skip => {}
                _ if !standing => {
                    return Err(BattleError::Invalid("a fallen member can only be skipped"));
                }
                Command::Defend => {}
                Command::Act { skill, .. } | Command::Item { skill, .. } => {
                    let cost = skill.action.as_ref().map_or(0, |a| i32::from(a.mp_cost));
                    if cost < 255 && self.party[i].fighter.mp < cost {
                        return Err(BattleError::Invalid("not enough MP"));
                    }
                }
                Command::Flee => {}
            }
        }
        let mut attempt = self.clone();
        let mut world_attempt = world.clone();
        let report = attempt.play_round(commands, &mut world_attempt)?;
        *self = attempt;
        *world = world_attempt;
        Ok(report)
    }

    fn play_round(
        &mut self,
        commands: &[Command],
        world: &mut Random,
    ) -> Result<RoundReport, BattleError> {
        let mut report = RoundReport {
            round: self.round,
            events: Vec::new(),
        };

        // 1. Flight, at the command phase.
        let fleeing: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Command::Flee))
            .map(|(i, _)| i)
            .collect();
        let mut failed_flee = Vec::new();
        if !fleeing.is_empty() {
            if self.set_battle {
                return Err(BattleError::FleeForbidden);
            }
            let (success, percent) = self.try_flee(world, &fleeing);
            report.events.push(Event::Flee { success, percent });
            if success {
                self.result = Some(BattleResult::Fled);
                report.events.push(Event::End(BattleResult::Fled));
                self.round += 1;
                return Ok(report);
            }
            // [hypothesis]: a failed flight costs the fleeing members their turn and the round
            // goes on (the original shows the failure in the flight state, `ov_026::021d8ba0`
            // state 0xb, and what follows was not read).
            failed_flee = fleeing;
        }

        // 2. Initiative.
        let order = self.initiative();
        report.events.push(Event::Initiative(order.clone()));

        // 3. Commands, in the order of the round.
        let mut records = Vec::new();
        for &(who, _) in &order {
            match who {
                Who::Party(i) => {
                    let plan = match &commands[i] {
                        Command::Act { skill, target } => Plan::Party {
                            skill: skill.clone(),
                            target: *target,
                            item: false,
                        },
                        Command::Item { skill, target } => Plan::Party {
                            skill: skill.clone(),
                            target: *target,
                            item: true,
                        },
                        Command::Defend => {
                            // `func_ov024_021e57c0`: Defend is a stance taken up as the round
                            // begins; its guard level lasts until the round's end. No turn
                            // record, so no draw ([hypothesis] for the absence of draws: the
                            // command code was not read).
                            self.status_mut(who).guard = 1;
                            report.events.push(Event::Defended(who));
                            Plan::Idle
                        }
                        _ => Plan::Idle,
                    };
                    if failed_flee.contains(&i) {
                        report.events.push(Event::Skipped(who));
                    }
                    records.push(Record { who, plan });
                }
                Who::Monster(j) => {
                    let (action, targets) = self.choose_way(j)?;
                    report.events.push(Event::Chose {
                        who,
                        action,
                        targets: targets.clone(),
                    });
                    records.push(Record {
                        who,
                        plan: Plan::Monster { action, targets },
                    });
                    for _ in 0..self.extra_actions(j)? {
                        let (action, targets) = self.choose_way(j)?;
                        report.events.push(Event::Chose {
                            who,
                            action,
                            targets: targets.clone(),
                        });
                        records.push(Record {
                            who,
                            plan: Plan::Monster { action, targets },
                        });
                    }
                }
            }
        }

        // 4. Turn classes.
        self.reorder(&mut records);

        // 5. Execution.
        for (turn, record) in records.iter().enumerate() {
            if self.result.is_some() {
                break;
            }
            if !self.alive(record.who) {
                report.events.push(Event::Skipped(record.who));
                continue;
            }
            // `func_ov000_0215833c`: the actor's status die, `NextRandomMax(100)`, comes before
            // the targets are settled (which is `ResolveAction`'s head).
            // [hypothesis]: a party member under the original's tactics AI (`func_ov024_021f8f20`)
            // draws more here; the hero's tactic is taken to be Follow Orders (no draw).
            let (skill, targets, item) = match &record.plan {
                Plan::Idle => continue,
                Plan::Party {
                    skill,
                    target,
                    item,
                } => {
                    self.rng.next_max(100);
                    (
                        skill.clone(),
                        self.party_targets(record.who, skill, *target)?,
                        *item,
                    )
                }
                Plan::Monster { action, targets } => {
                    let Who::Monster(j) = record.who else {
                        unreachable!()
                    };
                    let skill = self.skill(*action)?;
                    self.rng.next_max(100);
                    let settled = self.monster_targets(j, &skill, targets)?;
                    (skill, settled, false)
                }
            };
            if item {
                report.events.push(Event::ItemUsed {
                    who: record.who,
                    action: skill.number,
                });
            }
            self.resolve(record.who, &skill, targets, turn, &mut report, world)?;
        }
        // `func_ov000_0215e6e8`: the round's end clears the guard level for everyone.
        for status in self.party_status.iter_mut().chain(&mut self.monster_status) {
            status.guard = 0;
        }
        self.round += 1;
        Ok(report)
    }

    /// `func_ov000_0215f7a8`: whether the members in `fleeing` get away, and the percentage the
    /// world's draw was held under. Steps in the code's order: the surprise round, no monster
    /// left to act, the party at least three times as strong, then a percentage.
    fn try_flee(&mut self, world: &mut Random, fleeing: &[usize]) -> (bool, Option<i32>) {
        if self.round == 0 && self.surprise == Surprise::Party {
            return (true, None);
        }
        let monsters = self.monsters_standing();
        if monsters.is_empty() {
            return (true, None);
        }
        let strength = |f: &Fighter| (f.attack + f.defence) & 0xffff;
        let monster_sum: i32 = monsters
            .iter()
            .map(|&j| strength(&self.monsters[j].fighter))
            .sum();
        let monster_mean = monster_sum / monsters.len() as i32;
        let party = self.party_standing();
        let party_sum: i32 = party
            .iter()
            .map(|&i| strength(&self.party[i].fighter))
            .sum();
        let party_mean = party_sum / party.len() as i32;
        if party_mean >= monster_mean * 3 {
            return (true, None);
        }
        let mut percent = 0;
        for &i in fleeing {
            let deftness = self.party[i].fighter.deftness & 0x3ff;
            percent = percent.max((deftness as f32 * 0.05f32) as i32 + 10);
        }
        let floor = FLEE_FLOOR[self.flee_attempts.min(FLEE_FLOOR.len() - 1)];
        percent = percent.max(floor);
        self.flee_attempts += 1;
        (world.next_max(100) < percent, Some(percent))
    }

    /// Initiative (`ProcessCombatTurn`, `ov_000::0215d63c`): the party then the monsters, each
    /// standing combatant scored `agility × NextRandomFloatBetween(0.51, 1.0)`, sorted highest
    /// first. In the first round a surprise leaves some out **without a draw**: the party sits
    /// out when the monsters have the jump and every monster after the first then rolls
    /// `NextRandomMax(100)` and sits out above 66; the monsters sit out when the party has it.
    /// Ties are broken by order **[hypothesis]**: the game's quicksort (`func_020749ac`) is not
    /// stable, but a tie needs two equal products of 32-bit draws.
    fn initiative(&mut self) -> Vec<(Who, f32)> {
        let everyone = self
            .party_standing()
            .into_iter()
            .map(Who::Party)
            .chain(self.monsters_standing().into_iter().map(Who::Monster));
        let everyone: Vec<Who> = everyone.collect();
        let mut scored: Vec<(Who, f32)> = Vec::new();
        for who in everyone {
            if self.round == 0 {
                let party = matches!(who, Who::Party(_));
                match self.surprise {
                    Surprise::Monsters => {
                        if party || (!scored.is_empty() && self.rng.next_max(100) > SURPRISE_ROLL) {
                            continue;
                        }
                    }
                    Surprise::Party if !party => continue,
                    _ => {}
                }
            }
            let agility = self.fighter_snapshot(who).agility.clamp(0, 999) as f32;
            let draw = self.rng.next_float_between(0.51, 1.0);
            scored.push((who, agility * draw));
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored
    }

    /// `func_ov000_0215f57c`: a monster's extra actions this round. It always draws
    /// `NextRandomMax(2)`; the class (`+0x10` bits 8–10) indexes `[0, coin, 1, 2, b, …]`
    /// (the bytes `0, 0, 1, 2` are the ROM's, EU `0x02182a6c`; `b` is the flag the function sets
    /// on its first call).
    fn extra_actions(&mut self, j: usize) -> Result<usize, Unsupported> {
        let coin = self.rng.next_max(2);
        let called = std::mem::replace(&mut self.extras_called[j], true);
        let table = [
            0,
            u8::from(coin != 0),
            1,
            2,
            u8::from(called),
            if called { 2 } else { 0 },
            u8::from(!called),
            if called { 0 } else { 2 },
        ];
        Ok(usize::from(
            table[usize::from(self.monsters[j].extra_class & 7)],
        ))
    }

    /// `func_ov000_0215f1ac`: a party member's action with `+0x10` bit 19 and a weapon that
    /// grants it goes to the front; then the classes 10, 5 and 1 each move to the front in that
    /// order (so 1 leads), each pass keeping the order inside the class; class 32 goes to the
    /// back, in order.
    fn reorder(&self, records: &mut Vec<Record>) {
        let class = |r: &Record| match &r.plan {
            Plan::Party { skill, .. } => skill.turn_class,
            Plan::Monster { action, .. } => self.skills.get(action).map_or(0, |s| s.turn_class),
            Plan::Idle => 0,
        };
        let first = |r: &Record| match (&r.plan, r.who) {
            (Plan::Party { skill, .. }, Who::Party(i)) => {
                skill.acts_first && self.party[i].weapon_acts_first
            }
            _ => false,
        };
        let to_front = |records: &mut Vec<Record>, keep: &dyn Fn(&Record) -> bool| {
            let (mut front, rest): (Vec<Record>, Vec<Record>) =
                records.drain(..).partition(|r| keep(r));
            front.extend(rest);
            *records = front;
        };
        to_front(records, &first);
        for c in [10u8, 5, 1] {
            to_front(records, &|r| class(r) == c);
        }
        let (back, mut rest): (Vec<Record>, Vec<Record>) =
            records.drain(..).partition(|r| class(r) == 32);
        rest.extend(back);
        *records = rest;
    }

    /// A party command's targets at the moment it resolves (`func_ov000_021540fc`): by the
    /// action's side and reach, one chosen target (re-chosen when it has fallen: another group if
    /// its group is empty, then a member of the group), a group, everyone, or the actor; then the
    /// draws of [`Battle::hit_draws`].
    fn party_targets(
        &mut self,
        who: Who,
        skill: &Skill,
        target: Target,
    ) -> Result<Vec<Who>, BattleError> {
        let Who::Party(actor) = who else {
            unreachable!()
        };
        let fighter = &self.party[actor].fighter;
        if skill.reach == 5 && fighter.weapon_strikes_several {
            return Err(Unsupported::MultiHitWeapon.into());
        }
        let targets: Vec<Who> = match (skill.aims, skill.reach) {
            (_, 1) => vec![who],
            (1, 2 | 5) => {
                let Target::Monster(chosen) = target else {
                    return Err(BattleError::Invalid("an attack aims at a monster"));
                };
                vec![Who::Monster(self.rechoose_monster(chosen))]
            }
            (1, 4) => {
                let Target::Monster(chosen) = target else {
                    return Err(BattleError::Invalid("an attack aims at a monster"));
                };
                let group = self.rechoose_group(self.monsters[chosen].group);
                self.monsters_standing()
                    .into_iter()
                    .filter(|&j| self.monsters[j].group == group)
                    .map(Who::Monster)
                    .collect()
            }
            (1, 3) => self
                .monsters_standing()
                .into_iter()
                .map(Who::Monster)
                .collect(),
            (2, 2 | 7 | 8) => {
                let Target::Party(chosen) = target else {
                    return Err(BattleError::Invalid("a heal aims at a party member"));
                };
                // `func_ov000_02153cc0`: a fallen target becomes the actor.
                vec![if self.alive(Who::Party(chosen)) {
                    Who::Party(chosen)
                } else {
                    who
                }]
            }
            (2, 3 | 4) => self.party_standing().into_iter().map(Who::Party).collect(),
            (_, reach) => return Err(Unsupported::PartyReach(reach).into()),
        };
        self.hit_draws(skill.hit_code)?;
        Ok(targets)
    }

    /// `func_ov000_02153aa4` (EU same): a chosen monster that has fallen is replaced. When its
    /// group has none standing a group that has one is drawn by `NextRandomMax`; then a standing
    /// member of the group is drawn by `NextRandomMax`.
    fn rechoose_monster(&mut self, chosen: usize) -> usize {
        if self.alive(Who::Monster(chosen)) {
            return chosen;
        }
        let group = self.rechoose_group_if_empty(self.monsters[chosen].group);
        let members: Vec<usize> = self
            .monsters_standing()
            .into_iter()
            .filter(|&j| self.monsters[j].group == group)
            .collect();
        let k = self.rng.next_max(members.len() as i32) as usize;
        members.get(k).copied().unwrap_or(chosen)
    }

    /// `func_ov000_02153c24`: a group with none standing is replaced by a drawn one that has.
    fn rechoose_group(&mut self, group: usize) -> usize {
        self.rechoose_group_if_empty(group)
    }

    fn rechoose_group_if_empty(&mut self, group: usize) -> usize {
        let standing = |b: &Self, g: usize| {
            b.monsters_standing()
                .iter()
                .any(|&j| b.monsters[j].group == g)
        };
        if standing(self, group) {
            return group;
        }
        let groups: Vec<usize> = (0..MAX_GROUPS).filter(|&g| standing(self, g)).collect();
        let k = self.rng.next_max(groups.len() as i32) as usize;
        groups.get(k).copied().unwrap_or(group)
    }

    /// Runs the kernel for one action and everything the original does around it: the combo
    /// counter, [`resolve_action`], the HP, MP, statuses and flight applied, the kills
    /// registered, the end test and the actor's status tick.
    #[allow(clippy::too_many_arguments)]
    fn resolve(
        &mut self,
        who: Who,
        skill: &Skill,
        targets: Vec<Who>,
        turn: usize,
        report: &mut RoundReport,
        world: &mut Random,
    ) -> Result<(), BattleError> {
        let action = skill.action.clone().map_err(BattleError::from)?;
        let actor = self.fighter_snapshot(who);
        let victims: Vec<Fighter> = targets.iter().map(|&w| self.fighter_snapshot(w)).collect();
        let combo = self.step_combo(who, &action, &targets, turn)?;
        let outcome = resolve_action(&mut self.rng, &actor, &action, &victims, combo)?;
        let mut outcomes = Vec::new();
        for (&victim, o) in targets.iter().zip(&outcome.targets) {
            let before = self.fighter(victim).hp;
            if o.fled {
                self.fighter_mut(victim).hp = 0;
                if let Who::Monster(j) = victim {
                    self.fled[j] = true;
                }
            } else {
                self.fighter_mut(victim).hp = o.hp_after;
                if o.mp_damage > 0 {
                    let mp = &mut self.fighter_mut(victim).mp;
                    *mp = (*mp - o.mp_damage).max(0);
                }
                self.apply_kind(
                    victim,
                    &action,
                    o.landed && !o.dodged && !o.blocked,
                    world,
                    report,
                )?;
            }
            if before > 0
                && o.hp_after <= 0
                && !o.fled
                && let Who::Monster(j) = victim
            {
                self.register_kill(j);
            }
            // `func_ov000_0215cd80`: a miss, a dodge, a block or a blow of less than 1 resets
            // the combo counter.
            if !o.landed || o.dodged || o.blocked || o.amount < 1 {
                self.combo = Combo {
                    target: self.combo.target,
                    ..Combo::default()
                };
            }
            outcomes.push((victim, o.clone()));
        }
        let mp = &mut self.fighter_mut(who).mp;
        *mp = (*mp - outcome.mp_spent).max(0);
        report.events.push(Event::Acted {
            who,
            action: skill.number,
            outcomes,
        });

        // `func_ov000_02157d3c`: the end test, the party's fall first.
        if self.party_standing().is_empty() {
            self.result = Some(BattleResult::Lost);
        } else if self.monsters_standing().is_empty() {
            self.result = Some(BattleResult::Won);
        }
        match self.result {
            Some(result) => report.events.push(Event::End(result)),
            // `func_ov000_0215858c`: the actor's status tick draws once whatever its state, then
            // the wear-off rolls of its statuses; `func_ov000_021599f4` then takes a pass off
            // their counts.
            None => {
                self.rng.next_max(100);
                self.tick_status(who);
                self.decrement_status(who);
            }
        }
        Ok(())
    }

    /// The kind handlers of the chapter 2 actions, once one target's pass is settled: 4 defence
    /// (`func_ov024_021db7c0`), 5 agility (`021db994`), 19 dazzle (`021dd534`), 12 a call for
    /// backup (`021dc2a0`). No draw of their own (`docs/re/battle.md`).
    fn apply_kind(
        &mut self,
        victim: Who,
        action: &super::model::Action,
        landed: bool,
        world: &mut Random,
        report: &mut RoundReport,
    ) -> Result<(), BattleError> {
        if !landed {
            return Ok(());
        }
        let step = action.stat_step.clamp(-2, 2) as i8;
        let standing = self.alive(victim);
        match action.kind {
            4 if standing => {
                let status = self.status_mut(victim);
                // `func_02087860`: nothing moves a level already at the action's bound.
                let movable = if step < 0 {
                    status.defence_level > -2
                } else {
                    status.defence_level < 2
                };
                if movable {
                    status.defence_level = (status.defence_level + step).clamp(-2, 2);
                    if status.defence_level == 0 {
                        status.defence_count = 0;
                        status.defence_second = 0;
                    } else {
                        status.defence_count = 6;
                        status.defence_second = 0;
                    }
                }
            }
            5 if standing => {
                let status = self.status_mut(victim);
                let movable = if step < 0 {
                    status.agility_level > -2
                } else {
                    status.agility_level < 2
                };
                if movable {
                    status.agility_level = (status.agility_level + step).clamp(-2, 2);
                    if status.agility_level == 0 {
                        status.agility_count = 0;
                        status.agility_second = 0;
                    } else {
                        status.agility_count = 6;
                        status.agility_second = 0;
                    }
                }
            }
            19 if standing => {
                // `func_02088854`: the flag and a count of four.
                let status = self.status_mut(victim);
                status.dazzle_count = 4;
                status.dazzle_second = 0;
            }
            12 => {
                // `func_ov024_021dc2a0`: the caller adds another of its kind while the battle
                // holds room for it (five monsters, eight for the one monster 266).
                let Who::Monster(j) = victim else {
                    return Ok(());
                };
                self.summon(j, world, report)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// A call for backup (`func_ov024_021dc2a0`): another of the caller's kind joins its group,
    /// with its HP drawn from the world's generator (`func_02089630`). The count the handler
    /// tests is the battle's monsters, five, or eight for the one whose number is `0x10a`.
    fn summon(
        &mut self,
        caller: usize,
        world: &mut Random,
        report: &mut RoundReport,
    ) -> Result<(), BattleError> {
        let limit = if self.monsters[caller].number == 266 {
            8
        } else {
            5
        };
        if self.monsters.len() >= limit {
            return Ok(());
        }
        let template = self.monsters[caller].clone();
        let hp = super::init_monster_hp(world, template.table_hp, false);
        let mut monster = template.clone();
        monster.fighter.hp = hp;
        monster.fighter.max_hp = hp;
        let number = monster.number;
        self.monsters.push(monster);
        self.monster_status.push(Status::default());
        self.fled.push(false);
        self.way_counts.push(0);
        self.extras_called.push(false);
        report.events.push(Event::Summoned {
            who: Who::Monster(caller),
            number,
        });
        Ok(())
    }

    /// `func_ov000_0215858c`'s wear-off: one roll per status with a second count running, cleared
    /// where the chance by the count is above the draw (`0x02182ad4`).
    fn tick_status(&mut self, who: Who) {
        let mut status = *self.status(who);
        // `func_ov000_0215858c`'s blocks run dazzle before attack, defence and agility.
        if status.dazzle_second > 0 {
            status.dazzle_second -= 1;
            let draw = self.rng.next_max(100) as f32 / 100.0;
            if wear_off_chance(status.dazzle_second) > draw {
                status.dazzle_count = 0;
                status.dazzle_second = 0;
            }
        }
        for (second, level, count) in [
            (
                &mut status.defence_second,
                &mut status.defence_level,
                &mut status.defence_count,
            ),
            (
                &mut status.agility_second,
                &mut status.agility_level,
                &mut status.agility_count,
            ),
        ] {
            if *second > 0 {
                *second -= 1;
                let draw = self.rng.next_max(100) as f32 / 100.0;
                if wear_off_chance(*second) > draw {
                    *level = 0;
                    *count = 0;
                    *second = 0;
                }
            }
        }
        *self.status_mut(who) = status;
    }

    /// `func_ov000_021599f4`: a pass off every count the holder holds; at 0 the second count
    /// starts at four. [hypothesis] the four of `0x02182efc` for defence, agility and dazzle.
    fn decrement_status(&mut self, who: Who) {
        let status = self.status_mut(who);
        for (count, second) in [
            (&mut status.defence_count, &mut status.defence_second),
            (&mut status.agility_count, &mut status.agility_second),
        ] {
            if *count > 0 {
                *count -= 1;
                if *count == 0 {
                    *second = 4;
                }
            }
        }
        if status.dazzle_count > 0 {
            status.dazzle_count -= 1;
            if status.dazzle_count == 0 {
                status.dazzle_second = 4;
            }
        }
    }

    /// `func_ov024_021ea584`: steps the combo counter for the action's target. An action that
    /// does not chain resets it; another side, action or target starts it again; the same
    /// from another turn record adds one. Several targets of a chaining action are refused (the
    /// kernel takes one counter).
    fn step_combo(
        &mut self,
        who: Who,
        action: &super::model::Action,
        targets: &[Who],
        turn: usize,
    ) -> Result<u8, Unsupported> {
        if !action.chains_combo {
            self.combo = Combo {
                target: self.combo.target,
                ..Combo::default()
            };
            return Ok(0);
        }
        if targets.len() > 1 {
            return Err(Unsupported::ComboMultiTarget);
        }
        let side = self.fighter(who).side;
        let target = targets.first().copied();
        let c = &mut self.combo;
        if c.side != Some(side) || c.action != Some(action.number) || c.target != target {
            *c = Combo {
                side: Some(side),
                action: Some(action.number),
                target,
                turn: Some(turn),
                count: 0,
            };
        } else if c.turn != Some(turn) {
            c.count = c.count.saturating_add(1);
            c.turn = Some(turn);
        }
        Ok(c.count.min(3))
    }
}
