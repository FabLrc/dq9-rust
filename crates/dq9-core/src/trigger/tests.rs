use super::condition::eval;
use super::*;
use dq9_formats::table::Table;
use dq9_formats::trigger::{Op, parse};
use std::collections::BTreeSet;

/// Talking to `npc` with quest 11 in state `quest11`.
fn w(quest11: u8, npc: u32) -> State {
    let mut s = State::default();
    s.quests.insert(11, quest11);
    s.event[0] = npc;
    s
}

fn word(code: u16, arg: u16) -> u32 {
    u32::from(code) << 16 | u32::from(arg)
}

/// Builds a table with one record per word list (header fields first).
fn table(records: &[&[u32]]) -> Vec<u8> {
    let mut body = Vec::new();
    for words in records {
        body.extend_from_slice(&1u16.to_le_bytes());
        body.push(words.len() as u8);
        let type_bytes = words.len().div_ceil(4);
        body.extend(std::iter::repeat_n(0x55u8, type_bytes)); // all ints
        while body.len() % 4 != 0 {
            body.push(0xff);
        }
        for w in *words {
            body.extend_from_slice(&w.to_le_bytes());
        }
    }
    let pool = 0x10 + body.len();
    let mut data = Vec::new();
    for h in [records.len() as u32, pool as u32, 0, 0] {
        data.extend_from_slice(&h.to_le_bytes());
    }
    data.extend(body);
    data
}

#[test]
fn compiles_words_and_selects_by_conditions() {
    let offer = [
        1200,
        3,
        1,
        3,
        9,
        0,
        word(6, 18),    // talking to NPC 18
        word(0x37, 18), // NPC 18 and quest 11 in state 1 ("offered")
        0x000b_0003,
        word(0x76, 18), // talk to 18 with context 192
        0x00c0_0000,
    ];
    let fallback = [
        1200,
        3,
        1,
        3,
        9,
        0,
        word(6, 18),
        word(0x76, 18),
        0x00c1_0000,
    ];
    let data = table(&[&offer, &fallback]);
    let table = Table::parse(&data).unwrap();
    let triggers = parse(&table).unwrap();
    assert_eq!(triggers[0].conditions.len(), 2);
    assert_eq!(triggers[0].actions.len(), 1);
    assert!(triggers[0].loaded(1200, 3, 5));
    assert!(!triggers[0].loaded(1200, 4, 1));
    assert!(!triggers[0].loaded(1201, 3, 5));

    let t = first_match(&triggers, 0, &w(1, 18)).unwrap().unwrap();
    assert_eq!(t.talk(), Some((18, 192)));
    let t = first_match(&triggers, 0, &w(2, 18)).unwrap().unwrap();
    assert_eq!(t.talk(), Some((18, 193)));
    assert_eq!(first_match(&triggers, 0, &w(1, 19)), Ok(None));
}

#[test]
fn unsupported_conditions_only_fail_when_reached() {
    let data = table(&[&[1, 1, 1, 1, 1, 0, word(6, 5), word(0x45, 0), 0]]);
    let table = Table::parse(&data).unwrap();
    let triggers = parse(&table).unwrap();
    assert_eq!(first_match(&triggers, 0, &w(0, 4)), Ok(None));
    assert_eq!(first_match(&triggers, 0, &w(0, 5)), Err(Unsupported(0x45)));
}

/// 0x72 raises the item's compendium flag (`func_0207cc18`) and 0x6b the session flag.
#[test]
fn item_and_session_actions_set_their_flags() {
    // One category member with item 22159 (benevolessence) → compendium 1093.
    let mut member = vec![0u8; 12 + 32];
    member[..2].copy_from_slice(&1u16.to_le_bytes());
    member[12 + 0x18..12 + 0x1a].copy_from_slice(&22159u16.to_le_bytes());
    let compendium_word = 1093u32 << 12;
    member[12 + 0xc..12 + 0x10].copy_from_slice(&compendium_word.to_le_bytes());
    let table = dq9_formats::item::ItemTable::from_members(&[&member]).unwrap();

    let data = one(&[word(0x6b, 0), word(0x72, 22159)]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    let mut s = w(0, 3);
    s.compendium = table;
    let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
    assert!(
        s.flags
            .contains(&i32::from(crate::state::SESSION_HOST_FLAG))
    );
    assert!(!s.flags.contains(&4283), "the give is deferred");
    assert_eq!(run_deferred(&fired.deferred, &mut s), []);
    assert_eq!(s.items.get(&22159), Some(&1));
    assert!(s.flags.contains(&4283), "0xc76 + compendium 1093");
    // An item absent from the table is still held, without a flag.
    let data = one(&[word(0x72, 22158)]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
    assert_eq!(run_deferred(&fired.deferred, &mut s), []);
    assert_eq!(s.items.get(&22158), Some(&1));
    assert!(!s.flags.contains(&4282));
}

/// 0x6b is not 0x6c: it must not mark a doorway or stop the list.
#[test]
fn session_action_does_not_stop_the_list() {
    let data = one(&[word(0x6b, 0), word(0x64, 9)]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    let mut s = w(0, 3);
    let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
    assert!(fired.completed);
    assert!(s.flags.contains(&9));
}

/// 0x32: node 1 (arg, op0, op1 high) remembers the members it accepts in a mask that
/// lives for the whole event; node 2 (op1 low, op2, op3 low) tests the remembered ones.
#[test]
fn equipment_mask_spans_triggers() {
    let mut a = Member {
        hp: 1,
        ..Member::default()
    };
    a.slots[0] = 100;
    a.slots[7] = 7;
    let mut b = a;
    b.slots[0] = 200;
    b.slots[7] = 8;
    let state = State {
        party: vec![a, b],
        ..State::default()
    };
    let head = [1, 1, 1, 1, 1, 0];
    // Slot 0 == 200 (b) and slot 7 == 7 (a): fails, but b stays in the mask.
    let first = [&head[..], &[word(0x32, 200), 0, 7, 0, 0]].concat();
    // Slot 0 == 999 (nobody) and slot 7 == 8: holds through b, remembered above.
    let second = [&head[..], &[word(0x32, 999), 0, 8, 0, 0]].concat();
    let data = table(&[&first, &second]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    assert_eq!(
        first_match(&triggers, 0, &state).unwrap(),
        Some(&triggers[1])
    );
    let data = table(&[&second]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    assert_eq!(first_match(&triggers, 0, &state), Ok(None));
}

#[test]
fn angles_are_compiled_like_the_game() {
    assert_eq!(degrees_to_angle(0), 0);
    assert_eq!(degrees_to_angle(135), 9650);
    // Brought back once into 0..=2π.
    assert_eq!(degrees_to_angle(400), degrees_to_angle(40));
    assert_eq!(degrees_to_angle(-90), degrees_to_angle(270));
}

/// 0x29 is false either way when the NPC is not on the map.
#[test]
fn zone_condition_needs_the_npc() {
    let op = |arg| Op {
        code: 0x29,
        arg,
        operands: vec![],
    };
    let mut s = w(0, 7);
    let mut mask = 0;
    assert_eq!(eval(&op(0), &s, &mut mask), Some(false));
    assert_eq!(eval(&op(1), &s, &mut mask), Some(false));
    s.zones.insert(7, false);
    assert_eq!(eval(&op(0), &s, &mut mask), Some(true));
    s.zones.insert(7, true);
    assert_eq!(eval(&op(1), &s, &mut mask), Some(true));
}

fn one(words: &[u32]) -> Vec<u8> {
    let mut record = vec![1, 1, 1, 9, 9, 0, word(6, 3)];
    record.extend_from_slice(words);
    table(&[&record])
}

/// An action that stops the list drops what was already deferred.
#[test]
fn stopping_drops_deferred_actions() {
    let data = one(&[word(0x64, 5), word(0x76, 3), 0x00c0_0000, word(0x6f, 0)]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    let mut s = w(0, 3);
    let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
    assert!(!fired.completed);
    assert!(s.flags.contains(&5), "actions before the stop ran");
    let data = one(&[word(0x76, 3), 0x00c0_0000]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
    assert!(fired.completed);
    assert_eq!(
        run_deferred(&fired.deferred, &mut s),
        [Request::Talk {
            npc: 3,
            context: 192
        }]
    );
}

/// Progress only moves forward; a new chapter clears both flag sets of the track, a new
/// step only set 1.
#[test]
fn progress_moves_forward_and_scopes_track_flags() {
    let mut s = w(0, 3);
    s.track = 2;
    s.tracks[2] = Track {
        chapter: 4,
        step: 2,
        substep: 0,
        flags: [[7].into(), [8].into()],
    };
    let advance = |c, st, b| one(&[word(0x84, 0), c, st, b]);
    let run = |data: &[u8], s: &mut State| {
        let triggers = parse(&Table::parse(data).unwrap()).unwrap();
        let fired = fire(&triggers, 0, s).unwrap().unwrap();
        run_deferred(&fired.deferred, s)
    };
    // Same chapter, next step: set 1 cleared.
    assert_eq!(run(&advance(4, 3, 0), &mut s), [Request::ReloadTriggers]);
    assert_eq!(s.tracks[2].flags, [[7].into(), BTreeSet::new()]);
    // Backwards: no change, but the game reloads the triggers all the same.
    assert_eq!(run(&advance(4, 1, 0), &mut s), [Request::ReloadTriggers]);
    assert_eq!(s.tracks[2].step, 3);
    // Other tracks are untouched.
    assert_eq!(s.tracks[0], Track::default());
    assert_eq!(track_of_room(201), 3);
}

/// A new step or chapter clears every conversation counter (`Track_Advance` calls
/// `0x0206ec04`); a new substep alone, or a move backwards, leaves them.
#[test]
fn a_new_step_clears_the_talk_counters() {
    let mut s = w(0, 3);
    s.tracks[0] = Track {
        chapter: 4,
        step: 2,
        substep: 0,
        ..Track::default()
    };
    let run = |data: &[u8], s: &mut State| {
        let triggers = parse(&Table::parse(data).unwrap()).unwrap();
        let fired = fire(&triggers, 0, s).unwrap().unwrap();
        run_deferred(&fired.deferred, s)
    };
    let advance = |c, st, b| one(&[word(0x84, 0), c, st, b]);
    s.talk_counts.record(7);
    s.talk_counts.record(9);
    // Same step, next substep: kept.
    run(&advance(4, 2, 1), &mut s);
    assert_eq!((s.tracks[0].step, s.tracks[0].substep), (2, 1));
    assert_eq!(s.talk_counts.get(7), [1, 1]);
    // Backwards: kept.
    run(&advance(4, 1, 5), &mut s);
    assert_eq!(s.talk_counts.get(9), [1, 1]);
    // Next step: cleared.
    run(&advance(4, 3, 0), &mut s);
    assert_eq!(s.talk_counts.get(7), [0, 0]);
    assert_eq!(s.talk_counts.get(9), [0, 0]);
    // Next chapter: cleared too.
    s.talk_counts.record(7);
    run(&advance(5, 1, 0), &mut s);
    assert_eq!(s.talk_counts.get(7), [0, 0]);
}

/// Action 0x6a clears both counters of one NPC (`Trig_RunAction` case 6).
#[test]
fn action_0x6a_clears_one_npc_s_talk_counters() {
    let mut s = w(0, 3);
    s.talk_counts.record(7);
    s.talk_counts.record(9);
    let data = one(&[word(0x6a, 7)]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    fire(&triggers, 0, &mut s).unwrap().unwrap();
    assert_eq!(s.talk_counts.get(7), [0, 0]);
    assert_eq!(s.talk_counts.get(9), [1, 1]);
}

/// Action 0x8d queues the cast rebuild when its argument is not 0 (`Trig_RunDeferred`,
/// tested after the loop), after the trigger reload of the same list.
#[test]
fn action_0x8d_asks_for_the_cast_rebuild_after_the_trigger_reload() {
    let run = |data: &[u8]| {
        let mut s = w(0, 3);
        let triggers = parse(&Table::parse(data).unwrap()).unwrap();
        let fired = fire(&triggers, 0, &mut s).unwrap().unwrap();
        run_deferred(&fired.deferred, &mut s)
    };
    assert_eq!(run(&one(&[word(0x8d, 1)])), [Request::ReloadCast]);
    assert_eq!(run(&one(&[word(0x8d, 0)])), []);
    assert_eq!(
        run(&one(&[word(0x8d, 1), word(0x84, 0), 1, 2, 1])),
        [Request::ReloadTriggers, Request::ReloadCast]
    );
}

/// Actions 0x95/0x96 (`Trig_RunAction` cases 0x31/0x32, `0x02061c14`) hide or show a map
/// object immediately, so a fired field trigger emits them in list order, before the
/// deferred requests. The packed fields are the game's: block = argument low byte,
/// placement = high half of operand 0.
#[test]
fn field_triggers_emit_placement_hide_and_show() {
    use crate::field::{Event, Field};
    use dq9_formats::map::Regions;

    let record = [
        4101,
        1,
        1,
        1,
        1,
        11,
        word(0x95, 0x1234), // hide: block entry 0x34
        0x0010_0000,        // placement 16
        word(0x96, 0x01ff), // show: block entry 0xff
        0x0007_0000,        // placement 7 (high half, like the game's operand)
        word(0x77, 5000),   // deferred: start event 5000
    ];
    let regions = Regions {
        warps: Vec::new(),
        zones: Vec::new(),
    };
    let mut field = Field::new(4101, &regions, Some(table(&[&record]))).unwrap();
    let mut state = State::default();
    (
        state.tracks[0].chapter,
        state.tracks[0].step,
        state.tracks[0].substep,
    ) = (1, 1, 2);
    assert_eq!(
        field.event_ended(1, &mut state),
        vec![
            Event::Request(Request::SetPlacementHidden {
                block: 0x34,
                placement: 16,
                hidden: true,
            }),
            Event::Request(Request::SetPlacementHidden {
                block: 0xff,
                placement: 7,
                hidden: false,
            }),
            Event::Request(Request::Event(5000)),
        ]
    );
}

#[test]
fn gold_is_capped() {
    let data = one(&[word(0x8b, 0), 9_999_000, word(0x8b, 0), 5000]);
    let triggers = parse(&Table::parse(&data).unwrap()).unwrap();
    let mut s = w(0, 3);
    fire(&triggers, 0, &mut s).unwrap();
    assert_eq!(s.gold, GOLD_MAX);
}
