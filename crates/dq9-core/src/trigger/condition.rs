//! Condition evaluation: `Trig_EvalCondition` and the search for the first trigger of an event
//! whose conditions all hold (`docs/re/trigger.md`).
//!
//! port: 0x0205fb04, 0x020644a0, 0x0206475c, 0x0206eba8

use dq9_formats::trigger::{Op, Trigger};

use super::action::ext_index;
use super::world::{Member, Raw, World};

/// High and low halves of an operand, both sign-extended (the high one like the game's
/// `asr`; callers that store the low one unsigned use [`low`]).
fn halves(op: &Op, i: usize) -> (i32, i32) {
    let v = op.operands[i].int();
    (i32::from((v >> 16) as i16), i32::from(v as u16 as i16))
}

/// Low halfword of operand `i` (the compiler's `strh` of a single-field operand).
pub(super) fn low(op: &Op, i: usize) -> u16 {
    op.operands[i].int() as u16
}

fn quest_allowed(world: &dyn World, quest: i32) -> bool {
    !world.session().guest || quest > 0xad
}

/// Quest nibble, 0 outside `0..0xcc` (the `Quest_*` accessors' range check).
fn quest_nibble(world: &dyn World, quest: i32) -> u8 {
    if (0..0xcc).contains(&quest) {
        world.quest(quest)
    } else {
        0
    }
}

pub(super) fn quest_state(world: &dyn World, quest: i32) -> u8 {
    quest_nibble(world, quest) & 3
}

/// `Trig_QuestCondition`: quest condition with the dialogue encoding (4 and unknown values
/// fail).
fn quest_condition(world: &dyn World, quest: i32, cond: i32) -> bool {
    if !quest_allowed(world, quest) {
        return false;
    }
    let state = quest_state(world, quest);
    match cond {
        0 => state == 2,
        1 => quest_nibble(world, quest) & 4 != 0,
        2 => state == 3,
        3 => state == 1,
        5 | -1 => state == 0,
        _ => false,
    }
}

/// `Flags_TestExt`: extended flag numbering.
fn flag_ext(world: &dyn World, index: i32) -> bool {
    world.flag(ext_index(index))
}

/// Condition 0x17: wireless-session mode.
fn session_mode(world: &dyn World, mode: i32) -> bool {
    let s = world.session();
    match mode {
        0 => !s.active,
        1 => s.active,
        2 => !s.active || !s.peer_flag,
        3 => s.active && s.peer_flag,
        _ => false,
    }
}

/// Condition 0x24 with `arg`: character 0's status halfword `+4` (HP) is 0 if `arg` ≠ 0,
/// not 0 if `arg` = 0.
fn hero_down(world: &dyn World, arg: i32) -> bool {
    let hp = world.character(0).map_or(0, |h| h.hp);
    (hp == 0) == (arg != 0)
}

/// Condition 0x12 with `item`.
fn holds_item(world: &dyn World, item: i32) -> bool {
    world.item_count(item) > 0
}

/// Byte `offset` of the event parameter block (conditions 0x20/0x21 read `+0x2a..+0x2e`).
fn event_byte(world: &dyn World, offset: usize) -> i8 {
    (world.event(offset / 4) >> (8 * (offset % 4))) as i8
}

/// Degrees to the 16-bit angle stored by the compiler for condition 0x1d: radians brought
/// once into `0..=2π`, times 4096, truncated (`Trig_CompileWords`, `0x0205ee24`; single
/// precision like the game's soft-float calls).
pub fn degrees_to_angle(degrees: i32) -> u16 {
    use std::f32::consts::PI;
    let mut r = PI * degrees as f32 / 180.0;
    if r > 2.0 * PI {
        r -= 2.0 * PI;
    } else if r < 0.0 {
        r += 2.0 * PI;
    }
    (r * 4096.0) as i32 as u16
}

/// Conditions 0x32/0x52 (equipment): one record word compiles into two nodes
/// (`0x0205ef88`). Node 1 = (arg, op0 high, op0 low, op1 high), each `< 1` = any, compared
/// with slots 0, 1, 5, 6; each candidate it accepts sets a bit of the mask at quest manager
/// `+0x492` (cleared once per event) and the node holds if the mask is not 0. Node 2 = (op1
/// low, op2 high, op2 low, op3 low), each `0` = any, compared with slots 7 to 10 of the
/// candidates of the mask. Candidates: the party (0x32) or the local character (0x52),
/// without status bit 0.
fn equipment(op: &Op, world: &dyn World, mask: &mut u8) -> bool {
    let w = |i: usize| op.operands[i].int();
    let (hi, lo) = (|v: u32| (v >> 16) as u16, |v: u32| v as u16);
    let first = [op.arg, hi(w(0)), lo(w(0)), hi(w(1))];
    let second = [lo(w(1)), hi(w(2)), lo(w(2)), lo(w(3))];
    let candidates: Vec<Member> = if op.code == 0x52 {
        world
            .character(world.local_character())
            .into_iter()
            .collect()
    } else {
        world.party().to_vec()
    };
    for (i, m) in candidates.iter().enumerate() {
        let accepted = first
            .iter()
            .zip([0, 1, 5, 6])
            .all(|(&want, s)| (want as i16) < 1 || want == m.slots[s]);
        if !m.status_bit0 && accepted {
            *mask |= 1 << i;
        }
    }
    *mask != 0
        && candidates.iter().enumerate().any(|(i, m)| {
            !m.status_bit0
                && *mask & (1 << i) != 0
                && second
                    .iter()
                    .zip(7..11)
                    .all(|(&want, s)| want == 0 || want == m.slots[s])
        })
}

/// Evaluates one condition word (`Trig_EvalCondition`, `0x0205fb04`); `None` for the codes
/// absent from the data whose state is not modelled. `mask` is the equipment mask of
/// conditions 0x32/0x52.
pub(super) fn eval(op: &Op, world: &dyn World, mask: &mut u8) -> Option<bool> {
    let arg = i32::from(op.arg);
    let npc = || world.event(0) == u32::from(op.arg);
    let party = world.party();
    Some(match op.code {
        0 => world.flag(arg),
        1 => !world.flag(arg),
        2 => world.track_flag(0, arg),
        3 => !world.track_flag(0, arg),
        4 => world.track_flag(1, arg),
        5 => !world.track_flag(1, arg),
        6..=0xc => world.event(usize::from(op.code - 6)) == u32::from(op.arg),
        0xd..=0xf => {
            // FUN_020108a0: characters 0..=3 present.
            let n = (0..4).filter(|&i| world.character(i).is_some()).count() as i32;
            match op.code {
                0xd => arg <= n,
                0xe => n <= arg,
                _ => n == arg,
            }
        }
        0x10 => world.raw(Raw::Field954) == u32::from(op.arg),
        0x11 => (world.time_of_day() != 0) != (arg == 1),
        0x12 => holds_item(world, i32::from(op.arg as i16)),
        0x13 => !holds_item(world, i32::from(op.arg as i16)),
        0x14 => quest_state(world, arg) == 2 && quest_allowed(world, arg),
        0x15 => quest_condition(world, arg, 1),
        0x16 => quest_state(world, arg) == 3 && quest_allowed(world, arg),
        0x17 => session_mode(world, arg),
        0x19 => quest_state(world, arg) == 1 && quest_allowed(world, arg),
        0x1a => flag_ext(world, arg),
        0x1b => !flag_ext(world, arg),
        0x1c => quest_state(world, arg) == 0 && quest_allowed(world, arg),
        0x1d => {
            // Word 7 = (arg, operand 0) as (low, high) halves, low half of word 10 = the
            // angle of operand 1 (degrees).
            let word = world.event(7);
            word as u16 == op.arg
                && (word >> 16) as u16 == low(op, 0)
                && world.event(10) as u16 == degrees_to_angle(op.operands[1].int() as i32)
        }
        0x1e => world.event(8) == u32::from(op.arg),
        0x1f => i32::from(world.raw(Raw::Quest490) as i8) == i32::from(op.arg as i16),
        0x20 | 0x21 => {
            // The operand's bytes, most significant first, 0 = any. 0x20: in order against
            // the event bytes +0x2a..+0x2e; 0x21: each matches a distinct event byte.
            let want = op.operands[0].int().to_be_bytes().map(|b| b as i8);
            let have: [i8; 4] = std::array::from_fn(|i| event_byte(world, 0x2a + i));
            if op.code == 0x20 {
                want.iter().zip(have).all(|(&w, h)| w == 0 || w == h)
            } else {
                let mut used = [false; 4];
                want.iter().filter(|&&w| w != 0).all(|&w| {
                    let j = (0..4).find(|&j| !used[j] && have[j] == w);
                    j.inspect(|&j| used[j] = true).is_some()
                })
            }
        }
        0x22 => (party.len() >= 2) == (arg != 0),
        0x23 => world.substep() == arg,
        0x24 => hero_down(world, arg),
        0x25 => world.quest_log().marked.contains(&arg) && quest_allowed(world, arg),
        0x26 => !world.quest_log().marked.contains(&arg) && quest_allowed(world, arg),
        0x27 => world.flag(arg + 0x776),
        0x28 => !world.flag(arg + 0x776),
        0x29 => world.leader_in_zone_of(world.event(0)) == Some(arg != 0),
        0x2a => world
            .character(0)
            .is_some_and(|h| u16::from(h.vocation) == op.arg),
        0x2b => world.raw(Raw::Save56a0) == u32::from(op.arg),
        0x2d => world.character(0).is_some_and(|h| {
            let i = usize::from(op.arg);
            i < 6 && h.attrs.iter().all(|&v| v <= h.attrs[i])
        }),
        0x2e => {
            // Compiled as [code, arg, operand high = limit, operand low = mode].
            let (limit, at_most) = ((op.operands[0].int() >> 16) as i32, low(op, 0) != 0);
            world.character(0).is_some_and(|h| {
                h.attrs.get(usize::from(op.arg)).is_some_and(|&v| {
                    let diff = i32::from(v) - i32::from(h.base);
                    if at_most { diff <= limit } else { limit < diff }
                })
            })
        }
        0x2f => party.first().is_some_and(|m| {
            let (n, limit) = (m.slot_bits.count_ones() as i32, i32::from(low(op, 0)));
            if arg == 0 { n < limit } else { limit <= n }
        }),
        0x30 => world.item_count(i32::from(op.arg as i16)) >= i32::from(low(op, 0)),
        0x31 => party.iter().any(|m| {
            m.standing() && u16::from(m.vocation) == op.arg && low(op, 0) <= m.level_in(arg)
        }),
        0x32 | 0x52 => equipment(op, world, mask),
        0x33 => world
            .quest_log()
            .find(arg)
            .is_some_and(|e| e.word & 0x8000 != 0),
        0x34 => {
            let (flag, mode) = halves(op, 0);
            npc() && !world.track_flag(1, flag) && session_mode(world, mode)
        }
        0x35..=0x3d => {
            let (quest, cond) = halves(op, 0);
            if !npc() || !quest_condition(world, quest, cond) {
                return Some(false);
            }
            if op.code == 0x37 {
                return Some(true);
            }
            let (a, b) = halves(op, 1);
            match op.code {
                // Talk zone == a, then condition 0x10 with b.
                0x35 => {
                    world.event(5) == u32::from(a as u16)
                        && world.raw(Raw::Field954) == u32::from(b as u16)
                }
                0x36 => holds_item(world, a),
                0x38 => hero_down(world, a),
                0x39 => flag_ext(world, a) && session_mode(world, b),
                0x3a => !flag_ext(world, a) && session_mode(world, b),
                0x3b => flag_ext(world, a),
                0x3c => !flag_ext(world, a),
                _ => session_mode(world, a),
            }
        }
        0x3e | 0x3f => {
            let (flag, mode) = halves(op, 0);
            npc() && world.flag(flag) == (op.code == 0x3e) && session_mode(world, mode)
        }
        0x40 | 0x41 => {
            // Up to four flags: the arg and three operand halves; negative = unused.
            let (a, b) = halves(op, 0);
            let (c, _) = halves(op, 1);
            [i32::from(op.arg as i16), a, b, c]
                .into_iter()
                .filter(|&f| f >= 0)
                .all(|f| world.flag(f) == (op.code == 0x40))
        }
        0x42 => {
            // More than `low` copies of item `high` if arg ≠ 0, at most `low` otherwise.
            let (item, _) = halves(op, 0);
            (world.item_count(item) > i32::from(low(op, 0))) == (arg != 0)
        }
        0x43 => world.flag(arg + 0x1136),
        0x44 => !world.flag(arg + 0x1136),
        0x46 => world
            .quest_log()
            .find(arg)
            .is_some_and(|e| (e.word >> 18 & 1 != 0) == (low(op, 0) != 0)),
        0x49 => {
            let value = i32::from(world.raw(Raw::Quest2ca) as i16);
            (i32::from(low(op, 0)) <= value) == (arg != 0)
        }
        // The compiler drops the arg and keeps the operand's low half.
        0x4a => world.raw(Raw::Save71dc) as u16 == low(op, 0),
        0x4b | 0x4c => world.event(9) == u32::from(low(op, 0)),
        0x4d => world.flag(0x364) && world.flag(0x365),
        0x4e => {
            // [code, quest, field = operand 0 high, value = low, mode = operand 1 low]:
            // quest-log entry halfword compared with value.
            let (field, _) = halves(op, 0);
            let index = match field {
                4 => Some(0),
                5 => Some(1),
                6 => Some(2),
                0 | 1 | 7 => Some(3),
                2 | 8 => Some(4),
                3 | 9 => Some(5),
                _ => None,
            };
            let value = low(op, 0);
            match (world.quest_log().find(arg), index) {
                (Some(e), Some(i)) => match low(op, 1) {
                    0 => value <= e.values[i],
                    1 => e.values[i] < value,
                    2 => e.values[i] == value,
                    _ => false,
                },
                _ => false,
            }
        }
        0x50 => party
            .iter()
            .any(|m| m.standing() && i32::from(m.level()) >= arg),
        0x51 => world.session().active == (arg != 0),
        0x54 => world.raw(Raw::Save64de) as u8 as i32 == arg,
        0x55 => {
            let (gold, limit) = (world.gold(), op.operands[0].int());
            if arg == 0 {
                gold < limit
            } else {
                limit <= gold
            }
        }
        0x56 => {
            // The first of characters 0..=3 with a non-zero low tag nibble decides by its
            // HP; without one, whether character 0xce exists.
            let found = (0..4)
                .filter_map(|i| world.character(i))
                .find(|h| h.tag & 0xf != 0);
            let yes = match found {
                Some(h) => h.hp != 0,
                None => world.character(0xce).is_some(),
            };
            arg == i32::from(yes)
        }
        0x57 => {
            let len = world.quest_log().entries.len();
            if arg == 0 { len < 8 } else { len == 8 }
        }
        0x58 => arg < 0x49 && world.flag(arg + 0x33e),
        0x59 => arg >= 0x49 || !world.flag(arg + 0x33e),
        0x5a => world.date() == (u32::from(op.arg), u32::from(low(op, 0))),
        0x5c => world
            .quest_log()
            .find(arg)
            .is_some_and(|e| e.word & 0x200 == 0),
        0x5d => {
            let bit = 1u32.checked_shl(u32::from(op.arg & 0xff)).unwrap_or(0);
            world.raw(Raw::Global238c) & bit != 0
        }
        0x5e => world.raw(Raw::Global237c) == u32::from(op.arg),
        0x5f => i32::from(world.raw(Raw::Global27b4) as u16) == i32::from(op.arg as i16),
        0x60 => world
            .character(world.local_character())
            .is_some_and(|h| h.levels.iter().filter(|&&l| l > 0x62).count() as i32 >= arg),
        0x62 => i32::from(world.raw(Raw::Global27d0) as u8) <= arg,
        0x63 => arg <= i32::from(world.raw(Raw::Global27d0) as u8),
        0x1f4 => party
            .iter()
            .find(|m| m.tag >> 4 != 0 && i32::from(m.tag >> 4) == arg)
            .is_some_and(|m| low(op, 0) == u16::from(!m.status_bit0)),
        0x1f5 => world.time_of_day() == u32::from(op.arg),
        0x1f6 => world.take_signal(),
        0x1f7 => world.flag(0x798) == (arg != 0),
        0x1f8 => world.date() == world.saved_date(),
        0x1f9 => world.raw(Raw::Save63dc) & 1 != 0,
        0x1fa => quest_nibble(world, arg) & 8 != 0,
        0x1fb => world.raw(Raw::Save3f8) as u16 == op.arg,
        code if UNMODELLED.contains(&code) => return None,
        // 0x18, 0x47, 0x48, 0x61 and every code without a case: false.
        _ => false,
    })
}

/// Conditions absent from the data whose state is not modelled: skill bytes (0x2c), hero
/// name (0x45), the talk task (0x4f), a loaded file (0x53), the field manager (0x5b).
pub const UNMODELLED: [u16; 5] = [0x2c, 0x45, 0x4f, 0x53, 0x5b];

/// A condition the evaluator does not implement was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported(pub u16);

/// First loaded trigger of `event` whose conditions all hold (`Trig_FindMatch`,
/// `0x020644a0`: triggers of one event are tried in file order). An unimplemented
/// condition is only an error if it is actually reached.
///
/// Two quirks are kept: the equipment mask of 0x32/0x52 is cleared once per call, not per
/// trigger, and the "all hold" flag is not reset between triggers, so a trigger without
/// conditions only matches if it comes first (never the case in the data).
pub fn first_match<'t, 'a>(
    triggers: &'t [Trigger<'a>],
    event: i32,
    world: &dyn World,
) -> std::result::Result<Option<&'t Trigger<'a>>, Unsupported> {
    let mut mask = 0u8;
    let mut holds = true;
    for trigger in triggers.iter().filter(|t| t.event == event) {
        for cond in &trigger.conditions {
            match eval(cond, world, &mut mask) {
                Some(true) => holds = true,
                Some(false) => {
                    holds = false;
                    break;
                }
                None => return Err(Unsupported(cond.code)),
            }
        }
        if holds {
            return Ok(Some(trigger));
        }
    }
    Ok(None)
}
