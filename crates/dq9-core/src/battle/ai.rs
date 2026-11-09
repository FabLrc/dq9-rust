//! How a monster chooses its way and its targets, and how an action's targets are settled just
//! before it resolves.
//!
//! Every function names the game's own (`docs/re/battle.md`, "Monster AI"; EU addresses, the
//! overlay ones equal the USA's). The draws are made on the battle's generator in the order the
//! code makes them. What is not modelled is refused with [`Unsupported`], before anything is
//! kept: [`Battle::round`](super::Battle::round) works on a copy.
//!
//! Nothing here is marked as ported: these are partial models (no status, no tension, no grotto,
//! no co-op), not reimplementations of the functions they cite.

use super::model::Unsupported;
use super::round::{Battle, Who};
use super::setup::Skill;

/// The four weight tables of `monsterActionWeights` (EU `0x020e8cba`, USA `0x020e8caa`; the
/// bytes were read on the European ROM).
pub const WAY_WEIGHTS: [[u8; 6]; 4] = [
    [43, 42, 43, 43, 42, 43],
    [68, 58, 48, 38, 27, 17],
    [210, 29, 10, 4, 2, 1],
    [70, 70, 70, 16, 15, 15],
];

/// What `func_ov000_0215fbe0` returns by hit code, apart from the two codes that draw (3 and
/// 11): EU `0x02182a85`, read on the European ROM.
const HIT_TABLE: [u8; 12] = [0, 0, 0, 0, 2, 4, 0, 7, 3, 0, 1, 0];

impl Battle {
    /// `func_0208a91c` (EU `0x0208a92c`): the monster's way by its rule, then the targets of the
    /// way's action. Returns the action's number and its targets.
    ///
    /// When no way is usable the monster attacks with action 2 through the default pick
    /// (`func_ov000_02154a04`).
    pub(super) fn choose_way(&mut self, j: usize) -> Result<(u16, Vec<Who>), Unsupported> {
        let mode = self.monsters[j].ai_mode;
        if mode > 1 {
            return Err(Unsupported::AiMode(mode));
        }
        let found = match self.monsters[j].selector {
            0 => self.rule_weighted(j, 0)?,
            1 => self.rule_weighted(j, 1)?,
            2 => self.rule_weighted(j, 2)?,
            4 => self.rule_weighted(j, 3)?,
            3 => self.rule_in_turn(j)?,
            5 => self.rule_pair(j)?,
            6 => self.rule_first_then_rest(j)?,
            other => return Err(Unsupported::WaySelector(other)),
        };
        match found {
            Some(way) => Ok(way),
            None => {
                let skill = self.skill(2)?;
                let targets = self.generic_pick(Who::Monster(j), &skill)?;
                Ok((2, targets))
            }
        }
    }

    /// Rules 0, 1, 2 and 4 (`func_0208a370`, EU `0x0208a380`): `NextRandomMax(256) + 1` walked
    /// down the table's six weights, then the way tried, the ways before it down to the first,
    /// then those after it.
    fn rule_weighted(
        &mut self,
        j: usize,
        table: usize,
    ) -> Result<Option<(u16, Vec<Who>)>, Unsupported> {
        let mut roll = self.rng.next_max(256) + 1;
        let weights = &WAY_WEIGHTS[table];
        let mut slot = 0;
        while slot < 6 && i32::from(weights[slot]) < roll {
            roll -= i32::from(weights[slot]);
            slot += 1;
        }
        let slot = slot.min(5);
        let order = std::iter::once(slot)
            .chain((0..slot).rev())
            .chain(slot + 1..6);
        for s in order {
            if let Some(targets) = self.way_usable(j, s)? {
                return Ok(Some((self.monsters[j].ways[s], targets)));
            }
        }
        Ok(None)
    }

    /// Rule 3 (`func_0208a52c`, EU `0x0208a53c`): in turn, the way the count names, the count
    /// then `(count + 1) mod 6`; six tries.
    fn rule_in_turn(&mut self, j: usize) -> Result<Option<(u16, Vec<Who>)>, Unsupported> {
        for _ in 0..6 {
            let slot = usize::from(self.way_counts[j]);
            self.way_counts[j] = ((slot + 1) % 6) as u8;
            if let Some(targets) = self.way_usable(j, slot)? {
                return Ok(Some((self.monsters[j].ways[slot], targets)));
            }
        }
        Ok(None)
    }

    /// Rule 5 (`func_0208a5d8`, EU `0x0208a5e8`): the count taken mod 3; three times, `NextRandom`'s
    /// low bit `b` picks within the pair `2c`, `2c + 1`: way `2c + b` first, then `2c + (b xor 1)`.
    fn rule_pair(&mut self, j: usize) -> Result<Option<(u16, Vec<Who>)>, Unsupported> {
        self.way_counts[j] %= 3;
        for _ in 0..3 {
            let draw = self.rng.next_u32();
            let pair = usize::from(self.way_counts[j] & 0x7f) * 2;
            let first = pair + (draw & 1) as usize;
            let second = pair + ((draw & 1) ^ 1) as usize;
            self.way_counts[j] = (self.way_counts[j] + 1) % 3;
            for slot in [first, second] {
                if let Some(targets) = self.way_usable(j, slot)? {
                    return Ok(Some((self.monsters[j].ways[slot], targets)));
                }
            }
        }
        Ok(None)
    }

    /// Rule 6 (`func_0208a700`, EU `0x0208a710`): the count's low bit; twice, at 0 the first way
    /// alone, at 1 a start drawn by `NextRandomMax(5)` among ways 1 to 5 and on round them; the
    /// count flips.
    fn rule_first_then_rest(&mut self, j: usize) -> Result<Option<(u16, Vec<Who>)>, Unsupported> {
        self.way_counts[j] &= 1;
        for _ in 0..2 {
            let base = usize::from(self.way_counts[j] != 0);
            let span = if base == 1 { 5 } else { 1 };
            let mut start = self.rng.next_max(span as i32) as usize;
            self.way_counts[j] = (self.way_counts[j] + 1) & 1;
            for _ in 0..span {
                let slot = (base + start) & 0xff;
                if let Some(targets) = self.way_usable(j, slot)? {
                    return Ok(Some((self.monsters[j].ways[slot], targets)));
                }
                start = (start + 1) % span;
            }
        }
        Ok(None)
    }

    /// `func_0208a03c` (EU `0x0208a04c`) for AI modes 0 and 1: a way the monster's group may use
    /// once and has, or the flee way in the surprise round, is not usable; otherwise the way's
    /// targeting handler decides, and a usable way marks itself used in the group.
    ///
    /// The MP test of AI mode 2 and the legacy-boss test are not modelled (mode 2 is refused
    /// upstream).
    fn way_usable(&mut self, j: usize, slot: usize) -> Result<Option<Vec<Who>>, Unsupported> {
        let monster = &self.monsters[j];
        let way = monster.ways[slot];
        let (group, mode) = (monster.group, monster.ai_mode);
        if self.round == 0 && self.surprise == super::round::Surprise::Monsters && way == 0xE1 {
            return Ok(None);
        }
        let bit = 1u8 << slot;
        if monster.once_mask & bit != 0 && self.once_used[group] & bit != 0 {
            return Ok(None);
        }
        let skill = self.skill(way)?;
        let handler = match mode {
            1 => skill.handlers[0],
            _ => 0,
        };
        let handler = if handler > 0xa0 { 0 } else { handler };
        let targets = match handler {
            0 => self.generic_pick(Who::Monster(j), &skill)?,
            112 => match self.flee_handler(j) {
                Some(targets) => targets,
                None => return Ok(None),
            },
            other => return Err(Unsupported::TargetHandler(other)),
        };
        if skill.kind != 0x22 {
            self.once_used[group] |= bit;
        }
        Ok(Some(targets))
    }

    /// Targeting handler 112, Flee (`func_ov024_021f418c`): usable only when the mean of the
    /// party's attack and defence (all its members present, the fallen too, summed in a `short`)
    /// is at least three times the monster's own. The target is the monster itself.
    fn flee_handler(&self, j: usize) -> Option<Vec<Who>> {
        let present = self.party.len() as i32;
        if present < 1 {
            return None;
        }
        let sum = self.party.iter().fold(0i16, |acc, p| {
            acc.wrapping_add((p.fighter.attack + p.fighter.defence) as i16)
        });
        let mean = i32::from(sum) / present;
        let own = &self.monsters[j].fighter;
        let own = i32::from(((own.attack + own.defence) & 0xffff) as u16);
        (mean >= own * 3).then(|| vec![Who::Monster(j)])
    }

    /// `func_ov000_02154a04`: the default targets of an action by its side and reach, then the
    /// two draws of `func_ov000_0215fbe0` (see [`Battle::hit_draws`]).
    ///
    /// Aimed at the opponents: one of the standing party for reach 2 and 5 (the weighted pick),
    /// all of them for 3 and 4. Aimed at its own side: one standing monster by `NextRandomMax`
    /// for reach 2, the caster's group for 4, all for 3. Reach 1 is the caster.
    pub(super) fn generic_pick(
        &mut self,
        caster: Who,
        skill: &Skill,
    ) -> Result<Vec<Who>, Unsupported> {
        let party = self.party_standing();
        let monsters = self.monsters_standing();
        let targets = if skill.reach == 1 {
            vec![caster]
        } else {
            match (skill.aims, skill.reach) {
                (1, 2 | 5) => {
                    let Who::Monster(j) = caster else {
                        return Err(Unsupported::PartyReach(skill.reach));
                    };
                    self.weighted_pick(j, &party)?
                        .map(Who::Party)
                        .into_iter()
                        .collect()
                }
                (1, 3 | 4) => party.iter().map(|&i| Who::Party(i)).collect(),
                (2, 2) => {
                    let n = monsters.len() as i32;
                    let k = self.rng.next_max(n) as usize;
                    monsters
                        .get(k)
                        .map(|&i| Who::Monster(i))
                        .into_iter()
                        .collect()
                }
                (2, 4) => {
                    let Who::Monster(j) = caster else {
                        return Err(Unsupported::PartyReach(skill.reach));
                    };
                    let group = self.monsters[j].group;
                    monsters
                        .iter()
                        .filter(|&&i| self.monsters[i].group == group)
                        .map(|&i| Who::Monster(i))
                        .collect()
                }
                (2, 3) => monsters.iter().map(|&i| Who::Monster(i)).collect(),
                (_, reach) => return Err(Unsupported::Reach(reach)),
            }
        };
        self.hit_draws(skill.hit_code)?;
        Ok(targets)
    }

    /// `func_ov000_02154f30`: a monster's weighted pick among `candidates` (the standing party):
    /// 2 for the front line and 1 for the back line, `NextRandomMax(total) + 1` walked down the
    /// weights, a second draw `NextRandomMax(count)` should the walk end unspent.
    ///
    /// A monster that remembers its attackers adds 2 and 1 to the last two who struck it; what
    /// writes those is not read, so that case is refused against several candidates.
    fn weighted_pick(
        &mut self,
        j: usize,
        candidates: &[usize],
    ) -> Result<Option<usize>, Unsupported> {
        if candidates.is_empty() {
            return Ok(None);
        }
        if self.monsters[j].remembers && candidates.len() > 1 {
            return Err(Unsupported::TargetMemory);
        }
        let weights: Vec<i32> = candidates
            .iter()
            .map(|&i| if self.party[i].back_line { 1 } else { 2 })
            .collect();
        let total: i32 = weights.iter().sum();
        let mut roll = self.rng.next_max(total) + 1;
        for (&candidate, &weight) in candidates.iter().zip(&weights) {
            if roll <= weight {
                return Ok(Some(candidate));
            }
            roll -= weight;
        }
        let k = self.rng.next_max(candidates.len() as i32) as usize;
        Ok(Some(candidates[k]))
    }

    /// `func_ov000_0215fbe0`: **two draws every time** (`NextRandomBetween(3, 4)` and
    /// `NextRandomBetween(6, 8)`), then the table by hit code. A hit code that makes the action
    /// strike several times at random targets is refused.
    pub(super) fn hit_draws(&mut self, hit_code: u8) -> Result<(), Unsupported> {
        let three_or_four = self.rng.next_between(3, 4);
        let six_to_eight = self.rng.next_between(6, 8);
        let hits = match hit_code {
            3 => three_or_four,
            11 => six_to_eight,
            code => i32::from(HIT_TABLE.get(usize::from(code)).copied().unwrap_or(0)),
        };
        if hits > 0 {
            return Err(Unsupported::HitCode(hit_code));
        }
        Ok(())
    }

    /// The targets of a monster's action at the moment it resolves (`func_ov000_0215440c`): the
    /// ones chosen at the command, those still standing (a group or an everyone reach is rebuilt
    /// from the whole side); none left, a fresh default pick; otherwise the draws of
    /// [`Battle::hit_draws`].
    pub(super) fn monster_targets(
        &mut self,
        j: usize,
        skill: &Skill,
        stored: &[Who],
    ) -> Result<Vec<Who>, Unsupported> {
        let alive: Vec<Who> = if matches!(skill.reach, 3 | 4) {
            match skill.aims {
                1 => self.party_standing().into_iter().map(Who::Party).collect(),
                2 if skill.reach == 4 => {
                    let group = match stored.first() {
                        Some(Who::Monster(i)) => self.monsters[*i].group,
                        _ => self.monsters[j].group,
                    };
                    self.monsters_standing()
                        .into_iter()
                        .filter(|&i| self.monsters[i].group == group)
                        .map(Who::Monster)
                        .collect()
                }
                2 => self
                    .monsters_standing()
                    .into_iter()
                    .map(Who::Monster)
                    .collect(),
                _ => Vec::new(),
            }
        } else {
            stored.iter().copied().filter(|&w| self.alive(w)).collect()
        };
        if alive.is_empty() {
            return self.generic_pick(Who::Monster(j), skill);
        }
        self.hit_draws(skill.hit_code)?;
        Ok(alive)
    }

    /// The skill of a way (always loaded at setup); a number the setup did not load is refused.
    pub(super) fn skill(&self, number: u16) -> Result<Skill, Unsupported> {
        self.skills
            .get(&number)
            .cloned()
            .ok_or(Unsupported::ActionNumber(number))
    }
}
