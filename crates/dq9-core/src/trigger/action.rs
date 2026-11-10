//! Action execution: what `Trig_RunAction`, `Trig_RunDeferred` and `Trig_Fire` do to the
//! modelled [`State`] and the requests they leave for the engine (`docs/re/trigger.md`).
//!
//! port: 0x02061c14, 0x02064540, 0x0206446c, 0x020649c0, 0x0206f82c, 0x020703d8, 0x0206e174
//! port: 0x0206e358, 0x0206eb74, 0x0206ec04, 0x0206ecf8

use std::collections::BTreeSet;

use dq9_formats::table::Value;
use dq9_formats::trigger::{Op, Trigger};

use super::condition::{Unsupported, first_match, low, quest_state};
use super::world::{QuestLogEntry, Raw, State, TRACKS};

/// What `Trig_RunActions` (`0x02064540`) does after an action.
#[derive(Debug, Clone, PartialEq)]
pub enum Flow<'a> {
    Next,
    /// Stop the list: the trigger's deferred actions are then not run.
    Stop,
    /// Queue a node for `Trig_RunDeferred` (`Trig_Defer`, `0x0206446c`); composite actions
    /// queue another code.
    Defer(Op<'a>),
}

/// Gold cap of actions 0x8b (`DAT_02063014`).
pub const GOLD_MAX: u32 = 9_999_999;

/// Values whose rank (17 to 31) action 0x8e marks with flag `0xbf1 + rank` (`0x020e87d0`).
const RANKED: [i16; 32] = [
    0, 3, 2, 4, 5, 6, 7, 8, 9, 10, 11, 18, 12, 13, 14, 15, 16, 17, 19, 1, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31,
];

/// High half of operand `i`, as the compiler stores it (`asr #16`, then `strh`). Public for
/// the field's immediate 0x95/0x96 requests, which pack the placement there.
pub(crate) fn high(op: &Op, i: usize) -> u16 {
    (op.operands[i].int() >> 16) as u16
}

fn set(flags: &mut BTreeSet<i32>, index: i32, on: bool) {
    if on {
        flags.insert(index);
    } else {
        flags.remove(&index);
    }
}

/// `Flags_TestExt` numbering, for writes (`FUN_0206eb74`).
pub(super) fn ext_index(index: i32) -> i32 {
    if index < 0x400 { index } else { index + 0x6fa }
}

/// Quest nibble write keeping the two flag bits (`FUN_0206e174`).
fn set_quest_state(state: &mut State, quest: i32, value: u8) {
    if (0..0xcc).contains(&quest) {
        let nibble = state.quests.entry(quest).or_default();
        *nibble = *nibble & 0xc | value & 3;
    }
}

impl State {
    fn log_entry(&mut self, quest: i32) -> Option<&mut QuestLogEntry> {
        self.quest_log
            .entries
            .iter_mut()
            .find(|e| (e.word & 0x1ff) as i32 == quest)
    }

    /// `FUN_020703d8`: move track `track` forward to (chapter, step, substep); never back.
    /// A new chapter clears both flag sets, a new step of the same chapter set 1; either
    /// also clears every conversation counter (`0x0206ec04`, both tables at
    /// `gameres+0x44c4`). A new substep alone clears nothing.
    fn advance(&mut self, track: usize, chapter: u8, step: u8, substep: u8) {
        let Some(t) = self.tracks.get_mut(track) else {
            return;
        };
        let key = |c: u8, s: u8, b: u8| u32::from(c) * 10000 + u32::from(s) * 100 + u32::from(b);
        if key(t.chapter, t.step, t.substep) >= key(chapter, step, substep) {
            return;
        }
        if t.chapter != chapter {
            t.flags = Default::default();
            self.talk_counts.clear();
        } else if t.step != step {
            t.flags[1].clear();
            self.talk_counts.clear();
        }
        (t.chapter, t.step, t.substep) = (chapter, step, substep);
    }

    /// Adds `count` copies of `item` and, on the first acquisition, sets its "item obtained"
    /// flag (`func_0207cc18`, `0x0207cc28`): `0xc76 + compendium index`, index 0 skipped.
    /// Equipped copies are modelled as one count, as condition 0x12/0x30 read them.
    pub fn obtain(&mut self, item: i32, count: i32) {
        if count <= 0 {
            return;
        }
        *self.items.entry(item).or_default() += count;
        let Ok(id) = u16::try_from(item) else {
            return;
        };
        if let Some(flag) = self
            .compendium
            .compendium(id)
            .and_then(crate::state::item_obtained_flag)
        {
            self.flags.insert(i32::from(flag));
        }
    }

    /// Bag first, then equipped copies (`FUN_02086d98`): modelled as one count.
    fn take(&mut self, item: i32, count: i32) {
        if let Some(held) = self.items.get_mut(&item) {
            *held = (*held - count.max(0)).max(0);
        }
    }
}

/// Runs one action as `Trig_RunAction` (`0x02061c14`, switch on `code - 100`) does on the
/// modelled state. Effects on what [`State`] does not hold (map objects, NPCs, menus,
/// sound, wireless synchronisation, hit points) are skipped; the returned flow is the
/// game's.
pub fn run_action<'a>(op: &Op<'a>, state: &mut State) -> Flow<'a> {
    let arg = i32::from(op.arg);
    let track = state.track;
    let defer = || Flow::Defer(op.clone());
    match op.code {
        0x64 | 0x65 => set(&mut state.flags, arg, op.code == 0x64),
        0x66..=0x69 => {
            let flags = &mut state.tracks[track].flags[usize::from(op.code >= 0x68)];
            set(flags, arg, op.code.is_multiple_of(2));
        }
        // 0x6b: the host of a session raises the session flag (`Trig_RunAction` case 7:
        // `Flags_Set(manager, manager+0x8c, *(u32*)0x02063008)`, that word being `0x113a`,
        // when the session predicate holds). The runtime has no wireless session: it is the
        // host of a solo game, so the predicate holds.
        0x6b => {
            state
                .flags
                .insert(i32::from(crate::state::SESSION_HOST_FLAG));
        }
        // Case 6: both counters of NPC `arg` cleared (`0x0206ecf8` on each table).
        0x6a => state.talk_counts.reset(op.arg),
        0x6e => {
            if op.arg < 4 {
                state.time_of_day = u32::from(op.arg);
            }
        }
        // No case in the dispatcher: the list stops.
        0x6f..=0x71 | 0x98 | 0x9a | 0xa3 => return Flow::Stop,
        // Items (0x72/0x73 take one copy: the dispatcher stores 1 as count), talk, events,
        // warps, NPC removal, progress: deferred.
        0x72..=0x80 if op.code != 0x7d && op.code != 0x7e && op.code != 0x7f => {
            return defer();
        }
        0x7d => {
            // FUN_020961c0: fails (and stops the list) with a full log or a quest that is not
            // on offer; the offer list is not modelled.
            if state.quest_log.entries.len() >= 8 {
                return Flow::Stop;
            }
            if state.log_entry(arg).is_none() {
                state.quest_log.entries.push(QuestLogEntry {
                    word: u32::from(op.arg & 0x1ff) | 0x800,
                    values: [0; 6],
                });
            }
            state.flags.insert(0x119d);
        }
        0x7f => {
            // Date stamp, state 3 (FUN_0206e110), removal from the log [hypothesis].
            set_quest_state(state, arg, 3);
            state
                .quest_log
                .entries
                .retain(|e| (e.word & 0x1ff) as i32 != arg);
        }
        0x81 => set_quest_state(state, arg, 1),
        0x82 | 0x83 => set(
            &mut state.flags,
            ext_index(i32::from(high(op, 0))),
            op.code == 0x82,
        ),
        0x84 => {
            // Leaving the track's chapter or step clears its flags right away.
            let t = &mut state.tracks[track];
            if u16::from(t.chapter) != low(op, 0) {
                t.flags = Default::default();
            }
            if u16::from(t.step) != low(op, 1) {
                t.flags[1].clear();
            }
            return defer();
        }
        0x85 | 0x89..=0x8a | 0x8d | 0x91 | 0x94 | 0xd6 | 0xe1 | 0xe2 => return defer(),
        0x86 | 0x87 => {
            let on = op.code == 0x86;
            match high(op, 0) {
                3 => set(&mut state.flags, arg, on),
                4 => set(&mut state.tracks[track].flags[0], arg, on),
                5 => set(&mut state.tracks[track].flags[1], arg, on),
                _ => {}
            }
        }
        0x8b => {
            state.gold = state
                .gold
                .saturating_add(op.operands[0].int())
                .min(GOLD_MAX)
        }
        0x8c => state.gold = state.gold.saturating_sub(op.operands[0].int()),
        0x8e => {
            let rank = RANKED
                .iter()
                .position(|&v| i32::from(v) == arg)
                .unwrap_or(0);
            if rank >= 17 {
                state.flags.insert(0xbf1 + rank as i32);
            }
        }
        0x90 => {
            if quest_state(state, arg) == 2
                && !state.session.guest
                && let Some(e) = state.log_entry(arg)
            {
                e.word = e.word & !0x3800 | u32::from(high(op, 0) & 7) << 11;
            }
        }
        0x92 | 0x93 => set(&mut state.flags, arg + 0x776, op.code == 0x92),
        0x9b => {
            // Track flag (set 1) then the event, deferred.
            state.tracks[track].flags[1].insert(i32::from(high(op, 0)));
            return defer();
        }
        0x9c | 0x9d => {
            // 0x9c completes the quest; 0x9d accepts (0), completes (2) or offers (3) it.
            let code = match (op.code, high(op, 0)) {
                (0x9c, _) => 0x7f,
                (_, 0) => 0x7d,
                (_, 2) => 0x7f,
                (_, 3) => 0x81,
                _ => return Flow::Stop,
            };
            let inner = Op {
                code,
                arg: op.arg,
                operands: Vec::new(),
            };
            run_action(&inner, state);
            return defer();
        }
        0x9e | 0x9f => {
            // Queue 0x75 (take) / 0x74 (give) with the same arg and count.
            return Flow::Defer(Op {
                code: if op.code == 0x9e { 0x75 } else { 0x74 },
                arg: op.arg,
                operands: op.operands.clone(),
            });
        }
        0xa0 => {
            state.flags.insert(arg + 0x113f);
        }
        0xa2 => set(&mut state.flags, 0x113b, arg == 0),
        0xaf => {
            state.flags.remove(&(arg + 0x1136));
        }
        0xb0 => {
            if let Some(e) = state.log_entry(arg) {
                e.word = e.word & !(1 << 17) | u32::from(high(op, 0) & 1) << 17;
            }
        }
        0xb1 => {
            if high(op, 0) < 4 {
                state.time_of_day = u32::from(high(op, 0));
            }
        }
        0xba => set(&mut state.flags, 0x1138, arg != 0),
        0xbb => {
            state.raw.insert(Raw::Quest2ca, 0);
        }
        0xbc => {
            // Quest-log value: 0 set, 1 add (capped at 0xffff), 2 subtract (floored at 0).
            let (field, value, mode) = (high(op, 0), low(op, 0), low(op, 1));
            let index = match field {
                4 => 0,
                5 => 1,
                6 => 2,
                0 | 1 | 7 => 3,
                2 | 8 => 4,
                3 | 9 => 5,
                _ => return Flow::Next,
            };
            if let Some(e) = state.log_entry(arg) {
                let v = &mut e.values[index];
                *v = match mode {
                    0 => value,
                    1 => v.saturating_add(value),
                    2 => v.saturating_sub(value),
                    _ => *v,
                };
            }
        }
        // Random quest-log values: unmodelled, but a missing quest stops the list.
        0xbe | 0xbf => {
            if state.log_entry(arg).is_none() {
                return Flow::Stop;
            }
        }
        0xca => {
            state.flags.insert(arg + 0x1198);
        }
        // 0xcc / 0xcd: the guest of the party. `Guest_Dismiss` (`ov_017::02191484`) clears
        // character slot 0xce; `Guest_Start` (`ov_017::021913d0`, only for a record number
        // above 0) queues the load task that registers it, which `Cast_Rebuild` re-queues
        // after `Cast_ClearNpcs` has cleared the slot. Condition 0x56 reads that slot.
        0xcc => state.dismiss_guest(),
        0xcd => {
            if arg > 0 {
                state.attach_guest();
            }
        }
        0xce => {
            state.flags.insert(0x79a);
        }
        0xd3 => {
            let bit = 1u32.checked_shl(u32::from(op.arg & 0xff)).unwrap_or(0);
            *state.raw.entry(Raw::Global238c).or_default() |= bit;
        }
        0xde => {
            state.raw.insert(Raw::Global27d0, 0);
        }
        0xdf => set(&mut state.flags, 0x799, arg != 0),
        0xe0 => set(&mut state.flags, 0x798, arg != 0),
        0xe6 => set(&mut state.flags, arg + 0xbea, high(op, 0) != 0),
        // Codes beyond the dispatcher's table (0xea and up).
        0xea.. => return Flow::Stop,
        _ => {}
    }
    Flow::Next
}

/// Something a deferred action asks the engine to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// 0x76: talk to NPC `npc` with this talk context.
    Talk { npc: u16, context: u16 },
    /// 0x77, 0x80, 0x9b: start event `id` (`data/event/evNNNNN.gp2` or the event lists).
    Event(u16),
    /// 0x78: start the scripted battle `n` (the `eventbattle.bin` record whose index is `n`). The
    /// game fills the battle task `+0x3718` (`Trig_RunDeferred` case 0x78, `0x0206f82c`: battle
    /// number in `+0x28`, `n + 0x8005` in `+0x24`); its end fires trigger event 15 (won) or 16
    /// (lost) with `n` in word 6 (`docs/re/trigger.md`, § Scripted battles).
    Battle(u16),
    /// 0x79/0x89/0xe1 (room) and 0x85/0x8a/0xe2 (room and entry): leave for `room` (save
    /// `+0x3f8`, read back by condition 0x1fb); 0x7a/0x7b give the position (fixed point,
    /// 1.0 = 4096) and direction.
    Warp {
        room: u16,
        entry: Option<u16>,
        position: Option<[i32; 3]>,
        direction: Option<i16>,
    },
    /// 0x7c: remove NPC `npc` from the map (its object gets flag 0x8000).
    RemoveNpc(u16),
    /// A type 2 doorway with no destination was touched: native 584 (`0x02018b44`, called by
    /// `Field_TouchDoorways`) rotates the placement `placement` of block entry `block` by
    /// +90° and opens the way. The application animates the rotation.
    OpenDoor { block: u16, placement: u16 },
    /// 0x95/0x96: hide (`0x95`) or show (`0x96`) the map object of block entry `block` and
    /// placement `placement` (`Trig_RunAction` cases 0x31/0x32, `0x02061c14`). `block` is the
    /// low byte of the argument and `placement` the high half of operand 0, as the game's
    /// pending record stores them (`func_0206ea8c(manager, arg & 0xff, (operand + 0x80) >>
    /// 16, hidden)`); `0x95(0, 0x100000)` is block 0, placement 16, hidden. Immediate: the
    /// game applies it while the list runs (`func_02013380`), not after it.
    SetPlacementHidden {
        block: u16,
        placement: u16,
        hidden: bool,
    },
    /// 0x84, 0x94, 0xd6 ran (`Trig_RunDeferred` sets its reload flag whether the track
    /// moved or not): `Trig_LoadMap` (`0x0206462c`) reloads the triggers of the current
    /// map at the track's story point, drops the areas actions 0x8f added (`+0x494`) and
    /// fires event 20 again (`Trig_LoadTable`, `0x02064584`). The cast is not rebuilt.
    ReloadTriggers,
    /// 0x8d with a non-zero argument (`Trig_RunDeferred` queues task 0x30, `+0x3b58` of
    /// the game resources, when it is not queued yet): `ov_017::021c16cc` fades out, then
    /// `ov_017::021a2fa0` tears the map's cast down and runs `Cast_LoadMap`
    /// (`ov_017::021a2c14`) again for the current story point, and fades back in. The map,
    /// the hero and the triggers stay.
    ReloadCast,
    /// A deferred action whose effect is not modelled.
    Other(u16),
}

/// Float operand as the compiler stores positions: times 4096, rounded half away from zero.
pub(crate) fn fixed(value: &Value<'_>) -> i32 {
    let f = value.float() * 4096.0;
    (if f >= 0.0 { f + 0.5 } else { f - 0.5 }) as i32
}

/// Runs the deferred actions of a fired trigger (`Trig_RunDeferred`, `0x0206f82c`): applies
/// item and progress changes to `state` and returns the engine requests in order.
pub fn run_deferred(deferred: &[Op], state: &mut State) -> Vec<Request> {
    let mut requests = Vec::new();
    let (mut warp, mut position, mut direction) = (None, None, None);
    let mut reload = false;
    // 0x8d: the last argument of the list decides (`uVar5 = arg`, tested after the loop).
    let mut cast = 0;
    for op in deferred {
        let arg = i32::from(op.arg);
        match op.code {
            0x72 => state.obtain(arg, 1),
            0x73 => state.take(arg, 1),
            0x74 => state.obtain(arg, i32::from(high(op, 0) as i8)),
            0x75 => state.take(arg, i32::from(high(op, 0))),
            0x76 => requests.push(Request::Talk {
                npc: op.arg,
                context: high(op, 0),
            }),
            0x78 => requests.push(Request::Battle(op.arg)),
            0x77 | 0x80 | 0x9b => requests.push(Request::Event(op.arg)),
            0x79 | 0x89 | 0xe1 => warp = Some((op.arg, None)),
            0x85 | 0x8a | 0xe2 => warp = Some((op.arg, Some(high(op, 0)))),
            0x7a => position = Some([0, 1, 2].map(|i| fixed(&op.operands[i]))),
            0x7b => direction = Some(fixed(&op.operands[0]) as i16),
            0x7c => requests.push(Request::RemoveNpc(op.arg)),
            0x84 | 0x94 | 0xd6 => {
                let [c, s, b] = [0, 1, 2].map(|i| low(op, i) as u8);
                let tracks = match op.code {
                    0x84 => state.track..state.track + 1,
                    0x94 => 0..TRACKS,
                    _ => usize::from(op.arg)..usize::from(op.arg) + 1,
                };
                for t in tracks {
                    state.advance(t, c, s, b);
                }
                reload = true;
            }
            0x8d => cast = op.arg,
            0x9c => {
                state.take(i32::from(high(op, 0) as i16), 1);
                state.obtain(i32::from(low(op, 0) as i16), 1);
            }
            0x9d => state.obtain(i32::from(low(op, 0) as i16), 1),
            code => requests.push(Request::Other(code)),
        }
    }
    if let Some((room, entry)) = warp {
        requests.push(Request::Warp {
            room,
            entry,
            position,
            direction,
        });
    }
    // After the list, in the game's order: the trigger reload, then the cast task.
    if reload {
        requests.push(Request::ReloadTriggers);
    }
    if cast != 0 {
        requests.push(Request::ReloadCast);
    }
    requests
}

/// A trigger that fired (`Trig_Fire`, `0x020649c0`).
#[derive(Debug, Clone, PartialEq)]
pub struct Fired<'t, 'a> {
    pub trigger: &'t Trigger<'a>,
    /// Whether the action list ran to its end; callers only run the deferred actions then.
    pub completed: bool,
    pub deferred: Vec<Op<'a>>,
}

/// Fires `event`: the first matching trigger's actions run on `state` until one stops the
/// list.
pub fn fire<'t, 'a>(
    triggers: &'t [Trigger<'a>],
    event: i32,
    state: &mut State,
) -> std::result::Result<Option<Fired<'t, 'a>>, Unsupported> {
    let Some(trigger) = first_match(triggers, event, state)? else {
        return Ok(None);
    };
    let mut deferred = Vec::new();
    let mut completed = true;
    for action in &trigger.actions {
        match run_action(action, state) {
            Flow::Next => {}
            Flow::Defer(op) => deferred.push(op),
            Flow::Stop => {
                completed = false;
                break;
            }
        }
    }
    Ok(Some(Fired {
        trigger,
        completed,
        deferred,
    }))
}
