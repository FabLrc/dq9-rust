//! Trigger invariants on the real game data; skipped when the ROM is absent.

use dq9_core::trigger::{self, Member, State};
use dq9_formats::dialogue::{self, Session, TalkState};
use dq9_formats::trigger as rules;
use dq9_formats::{Gpc2, Table};
use nds_rom::Rom;

/// Every condition used by a scenario trigger file is reimplemented in `dq9-core`.
#[test]
fn trigger_conditions_are_modelled() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    for file in rom
        .files
        .files
        .iter()
        .filter(|f| f.path.starts_with("data/scenario/trigger") && f.path.ends_with(".bin"))
    {
        let table = Table::parse(rom.file(file.id).unwrap()).unwrap();
        for t in rules::parse(&table).unwrap_or_else(|e| panic!("{}: {e}", file.path)) {
            for c in &t.conditions {
                assert!(
                    !trigger::UNMODELLED.contains(&c.code),
                    "{}: condition {:#x}",
                    file.path,
                    c.code
                );
            }
        }
    }
}

/// George's quest offer goes through a talk trigger: with quest 11 available (state 1), it
/// forces context 202 (offer) from level 25, 203 ("if only someone strong enough…") below.
#[test]
fn talk_triggers_force_the_dialogue_context() {
    let george = |level: u16| {
        let mut leader = Member {
            hp: 1,
            ..Member::default()
        };
        leader.levels[0] = level;
        let mut s = State::default();
        s.quests.insert(11, 1);
        s.event[0] = 18;
        s.party = vec![leader];
        s.characters.insert(0, leader);
        s.time_of_day = 1;
        s
    };
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path("data/scenario/triggerM02.bin").unwrap()).unwrap();
    // Chapter 15 = letter M, room 1200 (George's room in M02place.bin).
    let triggers: Vec<_> = rules::parse(&table)
        .unwrap()
        .into_iter()
        .filter(|t| t.loaded(1200, 15, 1))
        .collect();
    let archive = Gpc2::parse(rom.file_by_path("data/scenario/M02M0.gp2").unwrap()).unwrap();
    let data = archive.member(archive.find("018_en.bin").unwrap()).unwrap();
    let table = Table::parse(&data).unwrap();
    let lines = dialogue::parse(&table).unwrap();
    let available = |q: i32| u8::from(q == 11);
    for (level, context, start) in [
        (25, 202, &b"*: Just lately I<1>ve had real trouble"[..]),
        (5, 203, &b"*: There<1>ve been so many monsters"[..]),
    ] {
        let world = george(level);
        let t = trigger::first_match(&triggers, 0, &world).unwrap().unwrap();
        assert_eq!(t.talk(), Some((18, context)));
        let talk = TalkState {
            step: 1,
            substep: 0,
            context,
            talk_count: [0, 0],
            daytime: true,
            session: Session {
                hero_alive: true,
                ..Session::default()
            },
            quest: &available,
        };
        let chosen = dialogue::select(&lines, &talk).unwrap();
        assert!(
            lines[chosen].text.unwrap().starts_with(start),
            "level {level}"
        );
    }
}
