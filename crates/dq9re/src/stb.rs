//! `dq9re stb`: disassemble an event script (`docs/re/stb.md`), with the texts of the event
//! (`<stem>_<lang>.bin`) next to the message natives.

use std::collections::{BTreeMap, BTreeSet};

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

/// Parses `MAJOR.MINOR` or `MAJOR.MIN-MAX` into `(major, min..=max)`.
fn parse_stage(text: &str) -> Result<(u32, std::ops::RangeInclusive<u32>)> {
    let (major, minor) = text.split_once('.').context("stage is MAJOR.MINOR[-MAX]")?;
    let (min, max) = minor.split_once('-').unwrap_or((minor, minor));
    Ok((major.parse()?, min.parse()?..=max.parse()?))
}

/// `(event number, script stem)` of the scenes whose stage (list values 0 and 1) matches.
pub fn stage_events(rom: &Rom, stage: &str) -> Result<Vec<(u16, String)>> {
    let (major, minors) = parse_stage(stage)?;
    Ok(dq9_core::script::load::stage_events(rom, major, minors))
}

/// Script archives of the scenes whose stage matches.
fn stage_scripts(rom: &Rom, stage: &str) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    for (_, stem) in stage_events(rom, stage)? {
        paths.push(
            ["data/event", "data/evspt_lv5"]
                .map(|dir| format!("{dir}/{stem}.gp2"))
                .into_iter()
                .find(|p| rom.file_by_path(p).is_ok())
                .with_context(|| format!("no archive for {stem}"))?,
        );
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// `dq9re stb-natives`: per native id, the calls and the scripts that make them.
pub fn natives(rom: &Rom, stage: Option<&str>, unnamed: bool) -> Result<()> {
    let paths = match stage {
        Some(stage) => stage_scripts(rom, stage)?,
        None => rom
            .files
            .files
            .iter()
            .filter(|f| f.path.ends_with(".gp2"))
            .map(|f| f.path.clone())
            .collect(),
    };
    let mut total: BTreeMap<i32, (usize, usize)> = BTreeMap::new();
    let mut scripts = 0;
    for path in &paths {
        let archive = Gpc2::parse(rom.file_by_path(path)?).with_context(|| path.clone())?;
        let Some(entry) = archive.entries.iter().find(|e| e.name.ends_with(".stb")) else {
            continue;
        };
        let data = archive.member(entry)?;
        let script = Script::parse(&data).with_context(|| path.clone())?;
        scripts += 1;
        let mut seen = BTreeSet::new();
        for (_, id) in script.functions.values().flat_map(Function::native_calls) {
            let id = id.with_context(|| format!("{path}: native id is not constant"))?;
            let (calls, files) = total.entry(id).or_default();
            *calls += 1;
            *files += usize::from(seen.insert(id));
        }
    }
    println!("{scripts} scripts, {} natives", total.len());
    println!("  id   calls scripts name");
    for (id, (calls, files)) in &total {
        let name = stb::native_name(*id);
        if !unnamed || name.is_none() {
            println!("{id:>4} {calls:>7} {files:>7} {}", name.unwrap_or("-"));
        }
    }
    Ok(())
}
