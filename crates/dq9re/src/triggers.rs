//! `dq9re triggers`: lists the rules of a `data/scenario/trigger<map>.bin` (`docs/re/trigger.md`),
//! to find what an event or a battle chains to.

use anyhow::{Context, Result};
use clap::Args;
use dq9_formats::table::Table;
use dq9_formats::trigger::{self, Op, action_name};
use nds_rom::Rom;

#[derive(Args)]
pub struct TriggersArgs {
    /// Trigger file: a map code (`M01`, `F`) or `data/scenario/trigger<code>.bin`.
    file: String,
    /// Only rules of this trigger event type (11 = end of an event, 15 = set battle won, 3 =
    /// arrival, 0 = talk…).
    #[arg(long)]
    event: Option<i32>,
    /// Only rules of this room (`maplist9.bin` id).
    #[arg(long)]
    room: Option<i32>,
    /// Only rules loaded at `CHAPTER.STEP` (`trigger.md`: chapter×1000+step range).
    #[arg(long, value_parser = parse_story)]
    story: Option<(i32, i32)>,
    /// Only rules with an operand or argument equal to this number (any condition or action).
    #[arg(long)]
    with: Option<u32>,
}

fn parse_story(s: &str) -> Result<(i32, i32), String> {
    let (c, st) = s.split_once('.').ok_or("expected CHAPTER.STEP")?;
    Ok((
        c.parse().map_err(|_| "bad chapter")?,
        st.parse().map_err(|_| "bad step")?,
    ))
}

fn show(op: &Op) -> String {
    let name = action_name(op.code).unwrap_or("?");
    let mut s = format!("{:#x}[{name}]({}", op.code, op.arg);
    for v in &op.operands {
        s.push_str(&format!(", {}", v.int()));
    }
    s.push(')');
    s
}

pub fn run(rom: &Rom, args: &TriggersArgs) -> Result<()> {
    let path = if args.file.contains('/') {
        args.file.clone()
    } else {
        format!("data/scenario/trigger{}.bin", args.file)
    };
    let data = rom.file_by_path(&path)?;
    let table = Table::parse(data).context("trigger table")?;
    let triggers = trigger::parse(&table)?;
    let mut shown = 0;
    for t in &triggers {
        if args.event.is_some_and(|e| e != t.event)
            || args.room.is_some_and(|r| r != t.room)
            || args.story.is_some_and(|(c, s)| !t.loaded(t.room, c, s))
        {
            continue;
        }
        if let Some(n) = args.with
            && !t.conditions.iter().chain(&t.actions).any(|op| {
                u32::from(op.arg) == n
                    || op
                        .operands
                        .iter()
                        .any(|v| v.int() == n || v.int() >> 16 == n || v.int() & 0xffff == n)
            })
        {
            continue;
        }
        shown += 1;
        let conditions: Vec<_> = t.conditions.iter().map(show).collect();
        let actions: Vec<_> = t.actions.iter().map(show).collect();
        println!(
            "room {} {}.{}..{}.{} event {}: if {} then {}",
            t.room,
            t.from.0,
            t.from.1,
            t.to.0,
            t.to.1,
            t.event,
            conditions.join(" "),
            actions.join(" ")
        );
    }
    println!("{shown} of {} triggers in {path}", triggers.len());
    Ok(())
}

/// `dq9re stage-events`: the scenes of a story stage range with the map each happens on.
pub fn stage_events(rom: &Rom, stage: &str) -> Result<()> {
    for (event, name) in crate::stb::stage_events(rom, stage)? {
        let place = dq9_core::script::load::place(rom, event);
        match place {
            Some(p) => println!("event {event:>5} {name:<12} map {}", p.map),
            None => println!("event {event:>5} {name:<12} map ?"),
        }
    }
    Ok(())
}
