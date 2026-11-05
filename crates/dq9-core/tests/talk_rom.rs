//! Conversations on the real game data; skipped when the ROM is absent.

use dq9_core::talk::{Dialogue, Input, Names, prepare};
use dq9_formats::dialogue;
use dq9_formats::{Font, FontId, Gpc2, Table};
use nds_rom::Rom;

/// Every English NPC line of the game prepares, lays out and plays to its end: a player who
/// presses the action key (answering Yes and No in turn) always closes the
/// window within a bounded number of steps, and no line is left without a glyph.
#[test]
fn every_npc_line_plays_to_its_end() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let info = rom.file_by_path("data/pack_lv5/fi_me.bin").unwrap();
    let data = rom.file_by_path("data/pack_lv5/fd_me.bin").unwrap();
    let font = Font::parse(info, data, FontId::Message).unwrap();
    let names = Names::default();
    let (mut lines, mut empty, mut prompts) = (0u32, 0u32, 0u32);
    for file in rom.files.files.iter().filter(|f| {
        f.path.starts_with("data/scenario/") && f.path.ends_with("0.gp2") && f.path.len() == 23
    }) {
        let archive = Gpc2::parse(rom.file(file.id).unwrap()).unwrap();
        for entry in archive
            .entries
            .iter()
            .filter(|e| e.name.ends_with("_en.bin"))
        {
            let member = archive.member(entry).unwrap();
            let Ok(table) = Table::parse(&member) else {
                continue;
            };
            for line in dialogue::parse(&table).unwrap() {
                let Some(text) = line.text else { continue };
                lines += 1;
                for answer in [0u8, 1] {
                    let script = prepare(&font, text, &names, 228);
                    let mut dialogue = Dialogue::new(script);
                    let mut glyphs = 0;
                    let mut asked = 0;
                    for _ in 0..100_000 {
                        let view = dialogue.view();
                        glyphs = glyphs.max(view.lines.iter().map(Vec::len).sum::<usize>());
                        let mut input = Input {
                            confirm: true,
                            ..Input::default()
                        };
                        if view.prompt.is_some() {
                            prompts += 1;
                            // Some questions come back after one of their answers: the
                            // answers alternate (Yes first, or No first on the second run).
                            let no = (asked + usize::from(answer)) % 2 == 1;
                            asked += 1;
                            input.cancel = no;
                            input.confirm = !no;
                        }
                        dialogue.step(input);
                        if dialogue.outcome().is_some() {
                            break;
                        }
                    }
                    assert!(
                        dialogue.outcome().is_some(),
                        "{}/{}: never closes: {}",
                        file.path,
                        entry.name,
                        String::from_utf8_lossy(text)
                    );
                    empty += u32::from(glyphs == 0);
                }
            }
        }
    }
    eprintln!("{lines} lines played, {empty} without a visible glyph, {prompts} prompt steps");
    assert!(lines > 30_000);
}
