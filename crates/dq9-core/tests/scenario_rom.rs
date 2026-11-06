//! What chains the scenes of the prologue, on the real game data; skipped when the ROM is
//! absent (`docs/re/trigger.md`, § Combats scriptés).

use dq9_core::field::{self, Event, Field};
use dq9_core::scenario;
use dq9_core::trigger::Request;
use dq9_formats::map::Regions;
use dq9_formats::table::Table;
use dq9_formats::trigger;
use nds_rom::Rom;

const BATTLE_ACTION: u16 = 0x78;
const BATTLE_WON_EVENT: i32 = 15;
/// Trigger condition 0xc: word 6 of the event (the battle number) equals the argument.
const BATTLE_CONDITION: u16 = 0xc;

/// Every scripted battle a trigger starts (action 0x78) is a record of `eventbattle.bin`, and
/// the map's triggers know how to go on once it is won (wiki: 65 of 66 records).
#[test]
fn every_scripted_battle_has_a_record_and_a_victory_trigger() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut started, mut without_victory) = (0, Vec::new());
    for file in &rom.files.files {
        if !file.path.starts_with("data/scenario/trigger") {
            continue;
        }
        let data = rom.file_by_path(&file.path).unwrap();
        let triggers = trigger::parse(&Table::parse(data).unwrap()).unwrap();
        for t in &triggers {
            for action in t.actions.iter().filter(|a| a.code == BATTLE_ACTION) {
                started += 1;
                let number = action.arg;
                assert!(
                    scenario::battle(&rom, number).is_some(),
                    "{}: battle {number} has no record",
                    file.path
                );
                let won = triggers.iter().any(|w| {
                    w.event == BATTLE_WON_EVENT
                        && w.room == t.room
                        && w.conditions
                            .iter()
                            .any(|c| c.code == BATTLE_CONDITION && c.arg == number)
                });
                if !won {
                    without_victory.push((file.path.clone(), number));
                }
            }
        }
    }
    assert!(started >= 60, "{started} battles started");
    assert!(
        without_victory.len() <= 1,
        "no victory trigger for {without_victory:?}"
    );
}

/// The end of `ev21507` starts battle 26 and its victory starts `ev21520` (what the game does
/// after the opening scenes), with the real triggers of `M01M12` / `F01M01`.
#[test]
fn the_opening_battle_leads_to_the_next_scene() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let story = dq9_formats::cast::Story {
        chapter: 1,
        step: 1,
        substep: 1,
        daytime: true,
        ..Default::default()
    };
    let file = |code: &str| {
        rom.file_by_path(&field::trigger_path(code).unwrap())
            .unwrap()
            .to_vec()
    };
    let mut state = field::initial_state(&story);
    // Room 1112 is `M01M12`; its trigger file is the one of `M01`.
    let mut opening = Field::new(1112, &Regions::default(), Some(file("M01M12"))).unwrap();
    assert_eq!(
        opening.event_ended(21507, &mut state),
        [Event::Request(Request::Battle(26))]
    );
    assert_eq!(
        opening.battle_ended(26, true, &mut state),
        [Event::Request(Request::Event(21520))]
    );
    let record = scenario::battle(&rom, 26).unwrap();
    assert_eq!(record.monsters, [(290, 1), (292, 1), (290, 1)]);
}
