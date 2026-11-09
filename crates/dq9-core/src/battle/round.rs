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
    /// An action: the Attack, a spell, an item (an item is an action of the action table).
    Act {
        /// The action (from [`ActionTables::skill`](super::setup::ActionTables::skill)).
        skill: Skill,
        /// Whom it aims at.
        target: Target,
    },
    /// Defend: the guard level is not modelled ([`Unsupported::Defend`]).
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
    Party { skill: Skill, target: Target },
    Monster { action: u16, targets: Vec<Who> },
    Idle,
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
                Command::Defend => return Err(Unsupported::Defend.into()),
                Command::Act { skill, .. } => {
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
                        },
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
            let (skill, targets) = match &record.plan {
                Plan::Idle => continue,
                Plan::Party { skill, target } => {
                    self.rng.next_max(100);
                    (
                        skill.clone(),
                        self.party_targets(record.who, skill, *target)?,
                    )
                }
                Plan::Monster { action, targets } => {
                    let Who::Monster(j) = record.who else {
                        unreachable!()
                    };
                    let skill = self.skill(*action)?;
                    self.rng.next_max(100);
                    let settled = self.monster_targets(j, &skill, targets)?;
                    (skill, settled)
                }
            };
            self.resolve(record.who, &skill, targets, turn, &mut report)?;
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
            let agility = self.fighter(who).agility.clamp(0, 999) as f32;
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
    /// counter, [`resolve_action`], the HP applied, the kills registered, the end test and the
    /// actor's status tick.
    fn resolve(
        &mut self,
        who: Who,
        skill: &Skill,
        targets: Vec<Who>,
        turn: usize,
        report: &mut RoundReport,
    ) -> Result<(), BattleError> {
        let action = skill.action.clone().map_err(BattleError::from)?;
        let actor = self.fighter(who).clone();
        let victims: Vec<Fighter> = targets.iter().map(|&w| self.fighter(w).clone()).collect();
        let combo = self.step_combo(who, &action, &targets, turn)?;
        let outcome = resolve_action(&mut self.rng, &actor, &action, &victims, combo)?;
        let mut outcomes = Vec::new();
        for (&victim, o) in targets.iter().zip(&outcome.targets) {
            let before = self.fighter(victim).hp;
            self.fighter_mut(victim).hp = o.hp_after;
            if before > 0
                && o.hp_after <= 0
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
            // `func_ov000_0215858c`: the actor's status tick draws once whatever its state.
            None => {
                self.rng.next_max(100);
            }
        }
        Ok(())
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
