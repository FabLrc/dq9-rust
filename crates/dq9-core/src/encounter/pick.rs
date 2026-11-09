//! The draws of the roaming monsters: which zone and monster spawn, the surprise roll, the
//! company that joins a battle. Two generators: the C library's `rand` ([`CRand`], through
//! `func_02032380` and the wander code) and the world's [`Random`] (`GetBTRandom`, through
//! `NextRandomMax`), as the game uses them.
//!
//! port: 0x02032380, 0x02073ed4, 0x02073fec, 0x0209bdd8, 0x0209afe4, ov_017::021970a0

use super::tables::{BattleRoamer, BattleZone, RoamerDef, ZoneDef};
use crate::battle::{CRand, Random};

/// `func_02032370` (`0x02032380`), the game's `rand_below(n)`: `(int)(n × ((rand() − 1) /
/// 32767.0))` in doubles, truncating — so a draw of 0 gives 0, and `n` is never reached. It
/// draws even when `n` is 1 or 0.
pub fn rand_below(rng: &mut CRand, n: i32) -> i32 {
    let draw = rng.rand() as i32;
    (f64::from(n) * (f64::from(draw - 1) / 32767.0)) as i32
}

/// The phase `GameState::GetTimeOfDay` returns: **night 0, morning 1, day 2, evening 3**
/// (`docs/re/lighting.md`). Night is the night phase alone.
pub fn is_night(time_of_day: u8) -> bool {
    time_of_day == 0
}

/// `func_02073ec4` (`0x02073ed4`): the zone a spawn at a node with region mask `mask` uses. A
/// zone is eligible when its kind fits the hour (0 by day, 1 by night, 2 always); among them
/// those whose mask is 0 or meets `mask` are preferred, and one is drawn with
/// [`rand_below`]; with none of those, one of the eligible.
pub fn spawn_zone<'a>(
    zones: &'a [ZoneDef],
    night: bool,
    mask: u32,
    rng: &mut CRand,
) -> Option<&'a ZoneDef> {
    let mut eligible: Vec<&ZoneDef> = Vec::new();
    let mut matching: Vec<&ZoneDef> = Vec::new();
    for zone in zones {
        let kind = zone.kind();
        if (kind == 0 && night) || (kind == 1 && !night) {
            continue;
        }
        eligible.push(zone);
        if zone.mask() == 0 || zone.mask() & mask != 0 {
            matching.push(zone);
        }
    }
    let pool = if matching.is_empty() {
        &eligible
    } else {
        &matching
    };
    if pool.is_empty() {
        return None;
    }
    Some(pool[rand_below(rng, pool.len() as i32) as usize])
}

/// `func_0209bdc8` (`0x0209bdd8`), the battle's zone: the first zone whose kind fits the hour
/// (1 by night, 0 otherwise, or 2) **and** whose mask meets `ground`, the region mask of the
/// ground under the party. No draw.
pub fn battle_zone(zones: &[ZoneDef], night: bool, ground: u32) -> Option<&ZoneDef> {
    zones.iter().find(|zone| {
        let kind = zone.kind();
        let fits = (kind == 1 && night) || (kind == 0 && !night) || kind == 2;
        fits && ground & zone.mask() != 0
    })
}

/// `func_02073fdc` (`0x02073fec`): the monster of a zone, weighted by bits 12–14 of each word;
/// `None` when the draw falls past the weights (a zone with none).
pub fn roamer_of<'a>(zone: &'a ZoneDef, rng: &mut CRand) -> Option<&'a RoamerDef> {
    let total: i32 = zone.roamers.iter().map(|r| r.weight() as i32).sum();
    let draw = rand_below(rng, total);
    let mut cumulative = 0;
    zone.roamers.iter().find(|roamer| {
        cumulative += roamer.weight() as i32;
        draw < cumulative
    })
}

/// `func_ov017_021970a0`: the surprise roll. `facing` is 0 face to face, 1 when the monster's
/// back is turned (the party's chance gains 10 and the monsters cannot surprise), 2 when the
/// party's is (the monsters' gains 10 and the party cannot). `deftness` is the highest among
/// the party who can act. The chance is `2 + deftness / 20 (+ 10)` in a hundred, in `float`.
/// Returns 1 (the party surprises the monsters), 2 (they surprise it) or 0; the draws are
/// [`rand_below`]`(100)`, the party's first.
pub fn surprise_roll(facing: u8, deftness: u32, rng: &mut CRand) -> u8 {
    let bonus = |hit: bool| if hit { 10.0f32 } else { 0.0 };
    if facing != 2 {
        let chance = (2.0f32 + 0.05f32 * deftness as f32 + bonus(facing == 1)) as i32;
        if rand_below(rng, 100) < chance {
            return 1;
        }
    }
    if facing != 1 {
        let chance = (2.0f32 + bonus(facing == 2)) as i32;
        if rand_below(rng, 100) < chance {
            return 2;
        }
    }
    0
}

/// `func_ov017_021b8a34`: the roamer's record in its own zone may name a scripted company
/// (`eventbattle.bin`) with a chance in a hundred; the draw is taken whenever the monster is
/// found, whatever the chance.
pub fn preset_company(zone: Option<&BattleZone>, monster: u16, rng: &mut CRand) -> Option<u8> {
    let roamer: &BattleRoamer = zone?
        .roamers
        .iter()
        .find(|r| (r.word & 0xfff) as u16 == monster)?;
    if rand_below(rng, 100) < i32::from(roamer.chance) {
        Some(roamer.preset)
    } else {
        None
    }
}

/// The three slots of a company: a monster and how many.
pub type Company = [(u16, i16); 3];

/// The permutations `0x020e8e3c` holds, indexed by `NextRandomMax(6)`.
const PERMUTATIONS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

fn weight(word: u32) -> i32 {
    ((word & 0x7fff) >> 12) as i32
}

fn least(word: u32) -> i32 {
    ((word & 0x3_ffff) >> 15) as i32
}

fn most(word: u32) -> i32 {
    ((word & 0x1f_ffff) >> 18) as i32
}

/// A company word picked by weight (`NextRandomMax(total)`, first whose running total reaches
/// the draw + 1; the last when none does) with its count `least + NextRandomMax(most − least +
/// 1)`.
fn pick_company(words: &[u32], world: &mut Random) -> (u16, i16) {
    let total: i32 = words.iter().map(|&w| weight(w)).sum();
    let draw = world.next_max(total);
    let mut cumulative = 0;
    let mut chosen = words.last().copied().unwrap_or(0);
    for &word in words {
        cumulative += weight(word);
        if draw < cumulative {
            chosen = word;
            break;
        }
    }
    let count = world.next_max(most(chosen) - least(chosen) + 1) + least(chosen);
    ((chosen & 0xfff) as u16, count as i16)
}

/// `func_0209afd4` (`0x0209afe4`): the company of a battle with `monster`, a roamer of
/// `own_zone` (its `encbtl` record), drawn from the company of `company_zone`. `party_limit`
/// is bit 10–12 of the field zone `company_zone` (5 when it has none). Draws on `world` in the
/// game's order: the touched monster's count, the number of kinds (one to three, by the
/// roamer's weights at bits 18–26), each other kind and its count, the shrink to the limit,
/// then the order (a coin for two kinds, one of six permutations for three).
///
/// Slots that stay at a count of 0 are absent.
pub fn draw_company(
    monster: u16,
    own_zone: Option<&BattleZone>,
    company_zone: Option<&BattleZone>,
    party_limit: i32,
    world: &mut Random,
) -> Company {
    let mut out: Company = [(monster, 0), (0, 0), (0, 0)];
    let roamer = own_zone.and_then(|z| {
        z.roamers
            .iter()
            .find(|r| (r.word & 0xfff) as u16 == monster)
    });
    let mut kinds = 1;
    match roamer {
        Some(roamer) if monster.wrapping_sub(0x26) >= 3 => {
            let w = roamer.word;
            out[0].1 = (world.next_max(most_of_roamer(w) - least_of_roamer(w) + 1)
                + least_of_roamer(w)) as i16;
            let (w1, w2, w3) = (
                ((w & 0x1f_ffff) >> 18) as i32,
                ((w & 0xff_ffff) >> 21) as i32,
                ((w & 0x7ff_ffff) >> 24) as i32,
            );
            let draw = world.next_max(w1 + w2 + w3) + 1;
            if w1 < draw {
                if w1 + w2 < draw {
                    if draw <= w1 + w2 + w3 {
                        kinds = 3;
                    }
                } else {
                    kinds = 2;
                }
            }
            if kinds != 1 {
                match company_zone.filter(|z| z.company.iter().map(|&c| weight(c)).sum::<i32>() > 0)
                {
                    None => kinds = 1,
                    Some(zone) => {
                        out[1] = pick_company(&zone.company, world);
                        if kinds != 2 {
                            out[2] = pick_company(&zone.company, world);
                        }
                    }
                }
            }
        }
        _ => out[0].1 = 1,
    }
    // Shrink to the party limit, a share at a time, from the first slot round.
    let mut excess = i32::from(out[0].1) + i32::from(out[1].1) + i32::from(out[2].1) - party_limit;
    if excess > 0 {
        let step = (excess / 3).max(1) as i16;
        let mut slot = 0;
        let mut idle = 0;
        while excess > 0 && idle < 3 {
            let old = out[slot].1;
            out[slot].1 = (old - step).max(0);
            if slot == 0 && out[0].1 == 0 {
                out[0].1 = 1;
            }
            let removed = i32::from(old) - i32::from(out[slot].1);
            idle = if removed == 0 { idle + 1 } else { 0 };
            excess -= removed;
            slot = (slot + 1) % 3;
        }
    }
    if kinds == 2 {
        if world.next_max(2) != 0 {
            out.swap(0, 1);
        }
    } else if kinds == 3 {
        let permutation = PERMUTATIONS[world.next_max(6) as usize];
        let original = out;
        for (from, &to) in permutation.iter().enumerate() {
            out[to] = original[from];
        }
    }
    out
}

/// A roamer's word of `encbtl` has its least and most at bits 12–14 and 15–17.
fn least_of_roamer(word: u32) -> i32 {
    ((word & 0x7fff) >> 12) as i32
}

fn most_of_roamer(word: u32) -> i32 {
    ((word & 0x3_ffff) >> 15) as i32
}
