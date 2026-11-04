//! `dq9re stb`: disassemble an event script (`docs/re/stb.md`), with the texts of the event
//! (`<stem>_<lang>.bin`) next to the message natives.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use dq9_formats::stb::{self, Function, Instr, Scope, Script};
use dq9_formats::{Gpc2, Table, Value};
use nds_rom::Rom;

use crate::text::escape;

/// Message id → text, from the event's language table (`[id, text]` records).
fn messages(archive: &Gpc2, stem: &str, lang: &str) -> Result<BTreeMap<i32, String>> {
    let Some(entry) = archive.find(&format!("{stem}_{lang}.bin")) else {
        return Ok(BTreeMap::new());
    };
    let data = archive.member(entry)?;
    let table = Table::parse(&data).context("event text table")?;
    Ok(table
        .records
        .iter()
        .filter_map(|r| match r.values[..] {
            [Value::Int(id), Value::Str(Some(text)), ..] => Some((id as i32, escape(text))),
            _ => None,
        })
        .collect())
}

fn scope(scope: Scope, index: u32) -> String {
    match scope {
        Scope::Local => format!("local{index}"),
        Scope::LocalIndexed => format!("local{index}[pop]"),
        Scope::Indirect => format!("(*local{index})[pop]"),
        Scope::LocalFloat => format!("local{index}:f"),
        Scope::LocalFloatIndexed => format!("local{index}[pop]:f"),
        Scope::IndirectFloat => format!("(*local{index})[pop]:f"),
        Scope::Global => format!("global{index}"),
        Scope::GlobalFloat => format!("global{index}:f"),
    }
}

fn instr(script: &Script, instr: &Instr) -> String {
    match *instr {
        Instr::Load { index, scope: s } => format!("load {}", scope(s, index)),
        Instr::Address { index, scope: s } => format!("addr {}", scope(s, index)),
        Instr::Int(v) => format!("int {v}"),
        Instr::Float(v) => format!("float {v}"),
        Instr::Str(off) => format!(
            "str {:?}",
            script.string(off).map(escape).unwrap_or_default()
        ),
        Instr::Jump(t) => format!("jump {t:#x}"),
        Instr::JumpIfFalse { target, keep } => {
            format!(
                "jump_if_false {target:#x}{}",
                if keep { " keep" } else { "" }
            )
        }
        Instr::JumpIfTrue { target, keep } => {
            format!(
                "jump_if_true {target:#x}{}",
                if keep { " keep" } else { "" }
            )
        }
        Instr::Call { function, .. } => format!("call fn@{function:#x}"),
        Instr::Native { args } => format!("native/{args}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// Comment for a native call: its id, known name, constant arguments and, for messages,
/// the text.
fn native(function: &Function, at: usize, texts: &BTreeMap<i32, String>) -> String {
    let Some(args) = function.args(at) else {
        return String::new();
    };
    let show = |v: &Option<i32>| v.map_or("?".to_owned(), |v| v.to_string());
    let id = args.first().copied().flatten();
    let name = id.and_then(stb::native_name).unwrap_or("");
    let rest: Vec<String> = args.iter().skip(1).map(show).collect();
    let mut out = format!("; {} {name}({})", show(&args[0]), rest.join(", "));
    if let (Some(400), Some(Some(msg))) = (id, args.get(1))
        && let Some(text) = texts.get(msg)
    {
        let preview: String = text.chars().take(60).collect();
        out += &format!("  {preview:?}");
    }
    out
}

pub fn run(rom: &Rom, path: &str, lang: &str) -> Result<()> {
    let archive = Gpc2::parse(rom.file_by_path(path)?).context("parsing archive")?;
    let entry = archive
        .entries
        .iter()
        .find(|e| e.name.ends_with(".stb"))
        .context("no .stb member")?;
    let stem = entry.name.trim_end_matches(".stb").to_owned();
    let data = archive.member(entry)?;
    let script = Script::parse(&data).context("parsing script")?;
    let texts = messages(&archive, &stem, lang)?;

    println!(
        "{} ({} bytes), {} globals",
        entry.name,
        data.len(),
        script.globals
    );
    println!("default entry: fn@{:#x}", script.default_entry);
    for (id, record) in &script.entries {
        println!("entry {id}: fn@{record:#x}");
    }
    for function in script.functions.values() {
        println!(
            "\nfn@{:#x} (frame {}, params {})",
            function.record, function.frame, function.params
        );
        let mut last = None;
        for (i, (pc, ins)) in function.code.iter().enumerate() {
            if last.is_some_and(|l| l + 12 != *pc) {
                println!("  ...");
            }
            last = Some(*pc);
            let comment = match ins {
                Instr::Native { .. } => native(function, i, &texts),
                _ => String::new(),
            };
            println!("  {pc:#06x}  {:<34}{comment}", instr(&script, ins));
        }
    }
    Ok(())
}
