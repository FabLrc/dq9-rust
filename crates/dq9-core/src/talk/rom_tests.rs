//! The pacing rule checked against a capture of the original (`docs/re/dialogue.md`
//! § "Text speed") and the two turn tags against real English lines (§ "Turned to the hero");
//! skipped when the ROM is absent.

use dq9_formats::{Font, FontId, Gpc2, Table, Value};
use nds_rom::Rom;

use super::*;

/// The ROM's message font (`fi_me.bin` / `fd_me.bin`).
fn message_font<'a>(rom: &'a Rom<'a>) -> Font<'a> {
    let info = rom.file_by_path("data/pack_lv5/fi_me.bin").unwrap();
    let data = rom.file_by_path("data/pack_lv5/fd_me.bin").unwrap();
    Font::parse(info, data, FontId::Message).unwrap()
}

/// The first text of record `record` of the scenario member `member` of `archive`.
fn line(rom: &Rom, archive: &str, member: &str, record: usize) -> Vec<u8> {
    let arc = Gpc2::parse(rom.file_by_path(archive).unwrap()).unwrap();
    let bytes = arc.member(arc.find(member).unwrap()).unwrap();
    let table = Table::parse(&bytes).unwrap();
    let Some(Value::Str(Some(text))) = table.records[record]
        .values
        .iter()
        .find(|v| matches!(v, Value::Str(Some(_))))
    else {
        panic!("no text in record {record} of {member}");
    };
    text.to_vec()
}

fn press() -> Input {
    Input {
        confirm: true,
        advance: true,
        ..Input::default()
    }
}

/// M12J0's NPC 108, record 13 carries `<R_TURN>` in the middle of its second page
/// (`...<PAGE>//Mayor Doffish// <R_TURN>Oh! Hold it!...`): the window reports the release
/// there, with the page still typing, and the `<END>` that follows keeps that heading. 2,724
/// `<R_TURN>` tags sit on 2,698 lines of the English tables (`dq9re text --lang en`,
/// `build/evidence/v3-dialogue-rturn/text-counts.txt`, `4642587c8ebc`).
#[test]
fn a_real_line_releases_the_heading_mid_page() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let text = line(&rom, "data/scenario/M12J0.gp2", "108_en.bin", 13);
    assert!(text.windows(8).any(|w| w == b"<R_TURN>"));
    let font = message_font(&rom);
    let mut dialogue = Dialogue::new(prepare(&font, &text, &Names::default(), 228));
    let shown = |d: &Dialogue| d.view().lines.iter().map(Vec::len).sum::<usize>();
    let (mut steps, mut at_release, mut last) = (0, None, 0);
    while dialogue.outcome().is_none() {
        // The action key turns the first page; the end of the line takes it too.
        let input = if dialogue.waiting() {
            press()
        } else {
            Input::default()
        };
        dialogue.step(input);
        last = shown(&dialogue);
        if dialogue.take_turn_release() {
            assert!(at_release.is_none(), "one `<R_TURN>` in the line");
            at_release = Some((steps, last));
            assert_eq!(dialogue.outcome(), None, "the window still shows the page");
        }
        steps += 1;
        assert!(steps < 5_000, "the line ends");
    }
    let (step, typed) = at_release.expect("`<R_TURN>` released the heading");
    assert!(
        last > typed + 20,
        "the rest of the page typed on after step {step}: {typed} then {last} glyphs"
    );
}

/// M01A0's NPC 108, record 0 ends with `<END_R_TURN>` (`*: I am Aquila. ... <HERO>.`): the
/// heading comes back on the step the action key leaves the end wait, and the outcome carries
/// the end that asks for it (`EndKind::Turn`).
#[test]
fn a_real_line_gives_the_heading_back_at_its_end_r_turn() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let text = line(&rom, "data/scenario/M01A0.gp2", "108_en.bin", 0);
    assert!(text.windows(12).any(|w| w == b"<END_R_TURN>"));
    let font = message_font(&rom);
    let mut dialogue = Dialogue::new(prepare(&font, &text, &Names::default(), 228));
    let mut steps = 0;
    while !dialogue.waiting() {
        dialogue.step(Input::default());
        assert!(
            !dialogue.take_turn_release(),
            "nothing is given back while the text stays"
        );
        steps += 1;
        assert!(steps < 5_000, "the text waits at its end");
    }
    dialogue.step(press());
    assert!(
        dialogue.take_turn_release(),
        "the key that closes reports the release"
    );
    for _ in 0..CLOSE_STEPS {
        dialogue.step(Input::default());
    }
    assert_eq!(dialogue.outcome().map(|o| o.end), Some(EndKind::Turn));
}

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
    while !dialogue.waiting() {
        dialogue.step(Input::default());
        steps += 1;
        assert!(steps < 1000, "the line never waits");
    }
    // The original's idx 0 to 61 is 61 units; the end code is read the step after the last.
    assert_eq!(steps, 62, "units of the captured line");
}
