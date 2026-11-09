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
//!   are kept. The four extra passes of the item-finding ability are not modelled.
//!
//! Nothing here is marked as ported: the victory routine is translated for the totals and the
//! first pass of the drops only.

use super::random::CRand;
use super::round::{Battle, BattleError};

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
        'kinds: for kind in &self.kills {
            // Rare first (`+0x03` step, `+0x06` item), the ordinary one only if it did not land.
            for slot in [1usize, 0] {
                let one_in = ONE_IN[usize::from(kind.steps[slot] & 7)];
                if one_in > 0 && drop_roll(rand, one_in) {
                    drops.push(kind.drops[slot]);
                    if drops.len() >= MAX_DROPS {
                        break 'kinds;
                    }
                    break;
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
