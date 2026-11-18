//! The pacing rule checked against a capture of the original (`docs/re/dialogue.md`
//! § "Text speed"); skipped when the ROM is absent.

use dq9_formats::{Font, FontId, Gpc2, Table, Value};
use nds_rom::Rom;

use super::*;

/// The line of the capture: the third text of event 1600's table (`ev01600_en.bin`), typed by
/// the original's window from frame 118 to 177 of the capture with a five-letter hero name.
/// The stream the window read held 61 codes before its end code (glyphs, spaces and one
/// break); the port's units must match, whatever the font.
#[test]
fn a_captured_line_costs_the_units_the_original_spent() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let info = rom.file_by_path("data/pack_lv5/fi_me.bin").unwrap();
    let data = rom.file_by_path("data/pack_lv5/fd_me.bin").unwrap();
    let font = Font::parse(info, data, FontId::Message).unwrap();
    let archive = Gpc2::parse(rom.file_by_path("data/event/ev01600.gp2").unwrap()).unwrap();
    let member = archive
        .member(archive.find("ev01600_en.bin").unwrap())
        .unwrap();
    let table = Table::parse(&member).unwrap();
    let Some(Value::Str(Some(text))) = table.records[2]
        .values
        .iter()
        .find(|v| matches!(v, Value::Str(Some(_))))
    else {
        panic!("no text in the record");
    };
    let names = Names {
        hero: b"Hhhhh".to_vec(),
        hero_male: true,
    };
    let script = prepare(&font, text, &names, 228);
    let mut dialogue = Dialogue::for_event(script);
    let mut steps = 0;
    while !dialogue.view().arrow {
        dialogue.step(Input::default());
        steps += 1;
        assert!(steps < 1000, "the line never waits");
    }
    // The original's idx 0 to 61 is 61 units; the end code is read the step after the last.
    assert_eq!(steps, 62, "units of the captured line");
}
