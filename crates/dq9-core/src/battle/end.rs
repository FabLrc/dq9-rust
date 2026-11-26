//! How a battle ends: the result, and on a victory the experience, the gold and the drops.
//!
//! - **Experience and gold** accrue as each monster is beaten (`func_ov000_02155184`, `ov_000::
//!   02155184`: `[battle + 0x8e28] += record +0x08`, `[battle + 0x8e2c] += record +0x0C`, once per
//!   monster). The victory routine (`func_ov023_021edf54`, `ov_023::021edf54`) turns the sums into
//!   the totals the party shares: `(uint)((float)sum × [battle + 0x8e3c])` for experience and
//!   `× [battle + 0x8e40]` for gold. Both multipliers are taken to be 1.0 (**[hypothesis]**: only
//!   actions `0x200` and `0x20C` write them, and nothing read initialises them). Sharing the
//!   experience among the party is `Party::award`'s (`func_ov023_021f4098`).
//! - **Drops** (`func_ov023_021f454c`, `ov_023::021f454c`): once per *kind* of monster beaten
//!   (a monster number in a group), in the order the kinds were first beaten, the rare drop is
//!   rolled first and the ordinary one only if the rare did not land. Each roll is
//!   `func_02032370(oneIn) == 0` on the C library's `rand` (EU `0x02032380`:
//!   `(int)(oneIn × ((rand() − 1) / 32767.0))`, in `double`), `oneIn` from the word at
//!   `0x021fd888`: 1, 8, 16, 32, 64, 128, 256 and 0 for "never" (no draw). At most eight items
//!   are kept. The leader's pass is followed by one extra pass per party member whose
//!   live record carries the item-finding flag `0xA4` and whose battle counter is at least half
//!   the turns played; that pass's `oneIn` is `N × 100 ÷ level` (`0x021f49c4`, `0x021f4a8c`), so
//!   its odds are `level ÷ 100` of the leader's, and a "one in one" step (byte 0) draws nothing
//!   there (`0x021f49e0`, `0x021f4aa8`). See [`PartyMember::item_finder`].
//!
//! Nothing here is marked as ported: the victory routine is translated for the totals and the
//! drops, without the group flags that force a drop nor the treasure-map branch.

use super::random::CRand;
use super::round::{Battle, BattleError};
use crate::party::PARTY_MAX;

/// How a battle ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleResult {
    /// Every monster is down or gone.
    Won,
    /// Every party member is down (`[battle + 0x8e14] == 2`, which fires trigger event 16).
    Lost,
    /// The party got away.
    Fled,
}

/// What the party takes from a battle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleEnd {
    /// How it ended.
    pub result: BattleResult,
    /// The experience to share (0 unless won).
    pub experience: u32,
    /// The gold (0 unless won).
    pub gold: u32,
    /// The items dropped, in the order they were rolled (empty unless won).
    pub drops: Vec<u16>,
}

/// A kind of monster beaten: its number and group, and what it drops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kind {
    pub(super) number: u16,
    pub(super) group: usize,
    pub(super) drops: [u16; 2],
    pub(super) steps: [u8; 2],
}

/// The "one in" of each chance step (`0x021fd888`, read on the European ROM).
pub const ONE_IN: [i32; 8] = [1, 8, 16, 32, 64, 128, 256, 0];

/// The most items a victory keeps.
const MAX_DROPS: usize = 8;

/// `func_02032370(one_in) == 0`: the roll of a drop on the C library's generator.
pub fn drop_roll(rand: &mut CRand, one_in: i32) -> bool {
    let draw = f64::from(rand.rand() as i32 - 1) / 32767.0;
    (f64::from(one_in) * draw) as i32 == 0
}

impl Battle {
    /// `func_ov000_02155184`: a monster has been beaten. Its experience and gold join the sums,
    /// once, and its kind joins the list of kinds beaten (twelve at most).
    pub(super) fn register_kill(&mut self, j: usize) {
        let monster = &self.monsters[j];
        self.experience += monster.experience;
        self.gold += u32::from(monster.gold);
        let known = self
            .kills
            .iter()
            .any(|k| k.number == monster.number && k.group == monster.group);
        if !known && self.kills.len() < 12 {
            self.kills.push(Kind {
                number: monster.number,
                group: monster.group,
                drops: monster.drops,
                steps: monster.drop_steps,
            });
        }
    }

    /// Whether party slot `slot` grants an extra drop pass, and at which level
    /// (`ov_023::021f454c`): the member must exist, carry the item-finding flag
    /// ([`PartyMember::item_finder`]), and hold at least half the battle's counter
    /// (`0x02053e0c ÷ ctrl+0x58cc ≥ 0.5`, `0x021f4674`–`0x021f4690`). The counter is
    /// [`Battle::passes`], the divisor the turns played.
    ///
    /// The site also skips a member `func_02010088` answers nonzero for (`0x021f4664`): bit 0 of
    /// the live record's `+0xA8`, the same bit that freezes the battle counter
    /// (`PartyMember_AddBattleCounter`, `0x02053de0`). Its meaning is **[unknown]**
    /// (`docs/re/party.md`), and the port evaluates neither it nor the HP: a member that took
    /// the bit mid-battle with a half counter keeps its extra pass here, the one known
    /// divergence.
    fn drop_pass(&self, slot: usize) -> Option<u8> {
        let member = self.party.get(slot)?;
        let level = member.item_finder?;
        if u64::from(self.passes[slot]) * 2 < u64::from(self.round) {
            return None;
        }
        Some(level)
    }

    /// The end of a finished battle: the result, and on a victory the totals and the drops,
    /// rolled on `rand` (the C library's generator, which nothing in the battle seeds).
    ///
    /// # Errors
    /// [`BattleError::Invalid`] while the battle goes on.
    pub fn finish(&self, rand: &mut CRand) -> Result<BattleEnd, BattleError> {
        let result = self
            .result
            .ok_or(BattleError::Invalid("the battle is not over"))?;
        if result != BattleResult::Won {
            return Ok(BattleEnd {
                result,
                experience: 0,
                gold: 0,
                drops: Vec::new(),
            });
        }
        // `(uint)((float)sum × 1.0f)`.
        let experience = (self.experience as f32 * 1.0f32) as u32;
        let gold = (self.gold as f32 * 1.0f32) as u32;
        let mut drops = Vec::new();
        // One pass for the leader (`0x021f461c`–`0x021f46c0`: `psVar17[5] = 0xFF`) then one for
        // each party slot whose member has the item-finding flag, is standing and holds at least
        // half the battle's counter (`0x021f4628`–`0x021f46b4`). Every pass walks all the kinds
        // again, rare first (`+0x03` step, `+0x06` item), the ordinary one (`+0x02`/`+0x04`) only
        // when the rare did not land.
        'passes: for pass in 0..=PARTY_MAX {
            let scale = if pass == 0 {
                None
            } else {
                match self.drop_pass(pass - 1) {
                    Some(level) => Some(i32::from(level)),
                    None => continue,
                }
            };
            for kind in &self.kills {
                for slot in [1usize, 0] {
                    let base = ONE_IN[usize::from(kind.steps[slot] & 7)];
                    // `0x021f49c4`–`0x021f49e4` (rare) and `0x021f4a8c`–`0x021f4aa8` (common):
                    // `N × 100 ÷ level`, truncated. A step whose byte is 0 ("one in one") is
                    // forced to 0 on an extra pass (`moveq` after the division), so a certain
                    // drop is never granted again; a "never" step (index 7) stays 0 too.
                    let one_in = match scale {
                        None => base,
                        Some(_) if kind.steps[slot] == 0 => 0,
                        Some(level) => base * 100 / level,
                    };
                    if one_in > 0 && drop_roll(rand, one_in) {
                        drops.push(kind.drops[slot]);
                        if drops.len() >= MAX_DROPS {
                            break 'passes;
                        }
                        break;
                    }
                }
            }
        }
        Ok(BattleEnd {
            result,
            experience,
            gold,
            drops,
        })
    }
}
