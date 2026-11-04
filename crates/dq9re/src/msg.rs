//! `dq9re msg`: run one dialogue string through the reimplemented message pipeline and
//! render its pages like the message window would (single-colour, no window frame).

use std::path::Path;

use anyhow::{Context, Result, bail};
use dq9_formats::message::{self, Name, Token};
use dq9_formats::{FontId, Gpc2, Table, Value};
use nds_rom::Rom;

use crate::font::{Canvas, load};

/// Stand-in game state for previews: male hero called "Hero", alone, every condition
/// takes its first branch.
fn preview_name(name: Name) -> Vec<u8> {
    match name {
        Name::Substitution { tag, index } => match tag {
            "HERO" | "LEADER" | "ADDRESSEE" => b"Hero".to_vec(),
            _ => format!("[{tag}{index}]").into_bytes(),
        },
        Name::Inflected { entity, index, .. } => format!("[{entity:?}{index}]").into_bytes(),
        Name::Weapon => b"[weapon]".to_vec(),
        Name::Actor => b"[actor]".to_vec(),
    }
}

/// Full pipeline with the preview context.
pub fn prepare(font: &dq9_formats::Font, text: &[u8]) -> Vec<Token> {
    let text = message::expand_values(text, |i| format!("{}", i + 1).into_bytes());
    let text = message::normalize(font, &text);
    let text = message::eval_conditionals(&text, |_| 0);
    let text = message::expand_names(&text, preview_name);
    let text = message::apply_cap(font, &text);
    message::compile(&text)
}

pub fn run(rom: &Rom, archive: &str, member: &str, record: usize, out: &Path) -> Result<()> {
    let archive = Gpc2::parse(rom.file_by_path(archive)?).context("parsing archive")?;
    let entry = archive
        .find(member)
        .with_context(|| format!("no member {member}"))?;
    let data = archive.member(entry)?;
    let table = Table::parse(&data).context("not a record table")?;
    let record = table
        .records
        .get(record)
        .with_context(|| format!("no record {record}"))?;
    let Some(text) = record.values.iter().find_map(|v| match v {
        Value::Str(Some(s)) => Some(*s),
        _ => None,
    }) else {
        bail!("record has no string");
    };

    let font = load(rom, FontId::Message)?;
    let tokens = prepare(&font, text);
    // Pages split on <PAGE> (0xFF0D); lines on new-line tokens.
    let mut pages: Vec<Vec<Vec<u8>>> = vec![vec![Vec::new()]];
    for token in &tokens {
        match token {
            Token::Text(bytes) => pages.last_mut().unwrap().last_mut().unwrap().extend(bytes),
            Token::NewLine => pages.last_mut().unwrap().push(Vec::new()),
            Token::Control { code: 0xFF0D, .. } => pages.push(vec![Vec::new()]),
            Token::Control { code, arg } => {
                let arg = arg
                    .as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_default();
                println!("control {code:#06x} {arg}");
            }
        }
    }
    let line_height = i32::from(font.height) + 4;
    let width = pages
        .iter()
        .flatten()
        .map(|line| font.layout(line).1)
        .max()
        .unwrap_or(1)
        .max(1);
    let page_height = line_height * pages.iter().map(Vec::len).max().unwrap_or(1) as i32 + 6;
    let mut canvas = Canvas::new(
        width as usize + 8,
        (page_height * pages.len() as i32) as usize,
    );
    for (p, page) in pages.iter().enumerate() {
        for (l, line) in page.iter().enumerate() {
            println!("page {p} line {l}: {}", String::from_utf8_lossy(line));
            let y = p as i32 * page_height + l as i32 * line_height + 3;
            for (glyph, x) in font.layout(line).0 {
                canvas.glyph(&font, glyph, x + 4, y);
            }
        }
        canvas.hline(((p as i32 + 1) * page_height - 1) as usize);
    }
    canvas.save(out, 3)?;
    println!("wrote {}", out.display());
    Ok(())
}
