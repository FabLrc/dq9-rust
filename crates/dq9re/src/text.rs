//! `dq9re table` / `dq9re text`: inspect record tables and dump game text.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use dq9_formats::{Gpc2, Table, Value};
use nds_rom::Rom;

/// Printable rendering of raw string bytes: ASCII kept (the game writes line breaks as the
/// two characters `\n`), real newlines and non-ASCII bytes escaped.
fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\n' => out.push_str("<LF>"),
            0x20..0x7F => out.push(char::from(b)),
            _ => write!(out, "\\x{b:02x}").unwrap(),
        }
    }
    out
}

fn value(v: &Value) -> String {
    match v {
        Value::Str(Some(s)) => format!("\"{}\"", escape(s)),
        Value::Str(None) => "null".to_owned(),
        Value::Int(i) => format!("{i:#x}"),
        Value::Float(f) => format!("{f}f"),
    }
}

pub fn table(rom: &Rom, archive: &str, member: &str) -> Result<()> {
    let archive = Gpc2::parse(rom.file_by_path(archive)?).context("parsing archive")?;
    let entry = archive
        .find(member)
        .with_context(|| format!("no member {member}"))?;
    let data = archive.member(entry)?;
    let table = Table::parse(&data).context("not a record table")?;
    println!(
        "{} records, {} strings",
        table.records.len(),
        table.string_count
    );
    for (i, r) in table.records.iter().enumerate() {
        let values: Vec<String> = r.values.iter().map(value).collect();
        println!("{i:5} kind {:#06x}: {}", r.kind, values.join(", "));
    }
    Ok(())
}

pub fn dump(rom: &Rom, lang: &str, out: &Path) -> Result<()> {
    let suffix = format!("_{lang}.");
    let (mut files, mut strings) = (0, 0);
    for f in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let archive = Gpc2::parse(rom.file(f.id)?).with_context(|| f.path.clone())?;
        let mut text = String::new();
        // The index is sorted by name hash; sort by name for readable dumps.
        let mut entries: Vec<_> = archive
            .entries
            .iter()
            .filter(|e| e.name.contains(&suffix))
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            let data = archive.member(entry)?;
            let Ok(table) = Table::parse(&data) else {
                continue;
            };
            for (i, r) in table.records.iter().enumerate() {
                for v in &r.values {
                    if let Value::Str(Some(s)) = v {
                        writeln!(text, "{}#{i}: {}", entry.name, escape(s))?;
                        strings += 1;
                    }
                }
            }
        }
        if text.is_empty() {
            continue;
        }
        let dest = out
            .join(lang)
            .join(f.path.trim_end_matches(".gp2"))
            .with_extension("txt");
        fs::create_dir_all(dest.parent().expect("dest has a parent"))?;
        fs::write(&dest, text).with_context(|| format!("writing {}", dest.display()))?;
        files += 1;
    }
    println!(
        "{strings} strings in {files} files under {}",
        out.join(lang).display()
    );
    Ok(())
}
