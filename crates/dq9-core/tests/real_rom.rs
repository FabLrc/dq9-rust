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

/// What `check_save_start` found: the hero's RAM position and the pose the save decodes to.
struct StartCheck {
    ram: [i32; 3],
    pose: dq9_core::state::HeroPose,
}

/// Decodes the start of `save_bytes` (ROM start point of its map) and compares it with the
/// party in the melonDS state `state_bytes`, taken in the game resumed from that save: map,
/// scenario, heading, x and z, the day clock and its phase.
fn check_save_start(rom: &Rom, save_bytes: &[u8], state_bytes: &[u8]) -> StartCheck {
    use dq9_core::state::{GameState, Placement};
    use dq9_formats::map::{self, Context, Map};
    use dq9_formats::maplist::{self, MapList};
    use dq9_formats::melonds::State as RamState;
    use dq9_formats::save::Save;

    let table = Table::parse(rom.file_by_path(maplist::PATH).unwrap()).unwrap();
    let list = MapList::parse(&table).unwrap();
    let save = Save::parse(save_bytes).unwrap();
    let state = GameState::from_save(&save).unwrap();
    assert_eq!(state.placement, Placement::MapStart, "a normal save");
    let record = list.find(state.map).expect("the save's map exists");
    let ctx = Context {
        time_of_day: 0,
        grotto_environ: 1,
    };
    let assembled = Map::assemble(record, &table, &list, |p| map::rom_file(rom, p), &ctx).unwrap();
    let pose = state.hero_pose(assembled.block_list.start);

    let ram = RamState::parse(state_bytes).unwrap();
    assert_eq!(ram.current_map().unwrap().0, state.map);
    assert_eq!(
        ram.scenario().unwrap(),
        (
            state.scenario.chapter,
            state.scenario.step,
            state.scenario.substep
        )
    );
    // The hero is object `protagonist_index`; the rest of the party stands on the same point.
    let hero = ram
        .object(ram.protagonist_index().unwrap())
        .unwrap()
        .expect("the protagonist object exists");
    let at = ram.object_position(hero).unwrap();
    assert_eq!(at[0], pose.position[0], "x");
    assert_eq!(at[2], pose.position[2], "z");
    assert_eq!(ram.object_heading(hero).unwrap(), i32::from(pose.angle));
    // The day clock restored from header A is the one in RAM (`GameState+0x3cc`, `+0x3dc`).
    let seconds = f32::from_bits(ram.u32(0x020f_33d8 + 0x3cc).unwrap());
    assert_eq!(seconds, state.clock.seconds());
    assert_eq!(
        ram.u32(0x020f_33d8 + 0x3dc).unwrap(),
        state.clock.phase().index()
    );
    StartCheck { ram: at, pose }
}

fn env_bytes(name: &str) -> Option<Vec<u8>> {
    let path = std::env::var_os(name)?;
    Some(std::fs::read(path).unwrap())
}

/// The position a save puts the party at is the one in melonDS RAM after "Continue".
///
/// Needs the ROM, `DQ9_SAVE` (a normal save) and `DQ9_STATE` (a melonDS state of the game
/// resumed from it, `scripts/melonds-state.ps1`). The church floor is at the authored height:
/// y matches too.
#[test]
fn save_start_matches_melonds_ram() {
    let Some(bytes) = testrom::load() else { return };
    let (Some(save), Some(state)) = (env_bytes("DQ9_SAVE"), env_bytes("DQ9_STATE")) else {
        eprintln!("DQ9_SAVE / DQ9_STATE not set, skipping");
        return;
    };
    let rom = Rom::parse(bytes).unwrap();
    let found = check_save_start(&rom, &save, &state);
    assert_eq!(found.ram[1], found.pose.position[1], "y");
}

/// Same save with its map id patched to 100 (Stornway, `C01`) and resealed; `DQ9_STATE_MAP100`
/// is the melonDS state of the game resumed from that file. The start record's height is the
/// floor's, the game then stands the hero `0x199` above the ground it finds: higher, never
/// lower, and within a unit.
#[test]
fn patched_save_start_matches_melonds_ram() {
    use dq9_formats::save::Save;
    let Some(bytes) = testrom::load() else { return };
    let (Some(mut save), Some(state)) = (env_bytes("DQ9_SAVE"), env_bytes("DQ9_STATE_MAP100"))
    else {
        eprintln!("DQ9_SAVE / DQ9_STATE_MAP100 not set, skipping");
        return;
    };
    for i in 0..2 {
        save[i * 0x8000 + 11476..][..2].copy_from_slice(&100u16.to_le_bytes());
    }
    let mut patched = Save::parse(&save).unwrap();
    for i in 0..2 {
        patched.slot_mut(i).unwrap().seal();
    }
    let rom = Rom::parse(bytes).unwrap();
    let found = check_save_start(&rom, patched.as_bytes(), &state);
    let rise = found.ram[1] - found.pose.position[1];
    assert!((0..0x1000).contains(&rise), "y rose by {rise}");
}

/// The scene flag of the opening events, as read in the original's event context at the
/// first control (`build/orca/oracle/`, `docs/re/trigger.md` § « Drapeaux globaux posés à
/// l'ouverture »): base 540 plus the record's rank in `eventlist_lv5.bin`.
#[test]
fn scene_flags_match_the_oracle_checkpoints() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    for (event, flag) in [
        (21500, 1450),
        (21507, 1451),
        (21520, 1452),
        (21521, 1453),
        (21522, 1454),
        (21596, 1461),
    ] {
        assert_eq!(
            dq9_core::script::load::scene_flag(&rom, event),
            Some(flag),
            "event {event}"
        );
    }
}
