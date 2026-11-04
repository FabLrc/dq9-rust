//! `dq9re talk`: decode an NPC dialogue file and show which line the game picks for a given
//! state: the map's talk triggers first (`dq9_formats::trigger`, `docs/re/trigger.md`), then
//! the line selection (`dq9_formats::dialogue`, `docs/re/dialogue.md`).

use anyhow::{Context, Result, bail};
use clap::Args;
use dq9_formats::dialogue::{self, Kind, Line, Session, TalkState};
use dq9_formats::trigger::{self, Op, World};
use dq9_formats::{Gpc2, Table, Value};
use nds_rom::{Narc, Rom};

use crate::text::escape;

/// Chapter letters of the dialogue archives (`Scenario_ChapterLetter`).
const CHAPTER_LETTERS: &[u8] = b"ABCDEFGHIJSTKLMNOPQ";

#[derive(Args)]
pub struct TalkArgs {
    /// Archive inside the ROM, e.g. `data/scenario/M02M0.gp2` (map M02, chapter letter M).
    archive: String,
    /// Member name, e.g. `018_en.bin` (NPC 18).
    member: String,
    /// Scenario step (save +0x5cb4).
    #[arg(long, default_value_t = 1)]
    step: i32,
    /// Save +0x5cb8.
    #[arg(long, default_value_t = 0)]
    substep: i32,
    /// Talk-zone id the hero stands in (0 outside zones, 80…).
    #[arg(long, default_value_t = 0)]
    zone: i32,
    /// Times already talked to this NPC (both counters).
    #[arg(long, default_value_t = 0)]
    talks: i32,
    #[arg(long)]
    night: bool,
    /// Quest save nibble, `QUEST=NIBBLE` (state in bits 0-1: 1 available, 2 accepted,
    /// 3 done); repeatable.
    #[arg(long = "quest", value_parser = parse_pair)]
    quests: Vec<(i32, i32)>,
    /// Story flag set; repeatable.
    #[arg(long = "flag")]
    flags: Vec<i32>,
    /// Item count, `ITEM=COUNT`; repeatable.
    #[arg(long = "item", value_parser = parse_pair)]
    items: Vec<(i32, i32)>,
    /// Highest party level.
    #[arg(long, default_value_t = 1)]
    level: i32,
    /// Quests in the quest log (8 = full).
    #[arg(long, default_value_t = 0)]
    quest_log: u8,
    /// Wireless session active.
    #[arg(long)]
    session: bool,
    /// Local console is a session guest.
    #[arg(long)]
    guest: bool,
    /// Local peer flag (session +0x1038).
    #[arg(long)]
    peer_flag: bool,
    /// Clear the hero word (hero +0x130 → +4), enabling kinds 5/6. Set by default: the
    /// kind 5 lines read as a visitor's view [assumption, see docs/re/dialogue.md].
    #[arg(long)]
    hero_flag_clear: bool,
    /// Room of the NPC; defaults to the first room placing it in `<map>place.bin`.
    #[arg(long)]
    room: Option<i32>,
    /// Ignore the map's triggers: the talk context is the zone.
    #[arg(long)]
    no_triggers: bool,
}

fn parse_pair(s: &str) -> Result<(i32, i32)> {
    let Some((a, b)) = s.split_once('=') else {
        bail!("expected A=B");
    };
    Ok((a.parse()?, b.parse()?))
}

/// The state given on the command line, as both selection inputs.
struct Preview<'a> {
    args: &'a TalkArgs,
    npc: u32,
}

fn lookup(pairs: &[(i32, i32)], key: i32) -> i32 {
    pairs
        .iter()
        .find(|&&(k, _)| k == key)
        .map_or(0, |&(_, v)| v)
}

impl Preview<'_> {
    fn session(&self) -> Session {
        Session {
            active: self.args.session,
            guest: self.args.guest,
            peer_flag: self.args.peer_flag,
            hero_flag: !self.args.hero_flag_clear,
        }
    }
}

impl World for Preview<'_> {
    fn flag(&self, index: i32) -> bool {
        self.args.flags.contains(&index)
    }
    fn player_flag(&self, _: u8, _: i32) -> bool {
        false
    }
    fn quest(&self, quest: i32) -> u8 {
        lookup(&self.args.quests, quest) as u8
    }
    fn daytime(&self) -> bool {
        !self.args.night
    }
    fn session(&self) -> Session {
        Preview::session(self)
    }
    fn substep(&self) -> i32 {
        self.args.substep
    }
    fn event(&self, i: usize) -> u32 {
        match i {
            0 => self.npc,
            5 => self.args.zone as u32,
            _ => 0,
        }
    }
    fn item_count(&self, id: i32) -> i32 {
        lookup(&self.args.items, id)
    }
    fn max_level(&self) -> i32 {
        self.args.level
    }
    fn quest_log_len(&self) -> u8 {
        self.args.quest_log
    }
    fn quest_log_flag(&self, _: i32) -> bool {
        false
    }
    fn quest_known(&self, _: i32) -> bool {
        false
    }
}

fn describe(line: &Line) -> String {
    let night = match line.night {
        Some(true) => " night",
        _ => "",
    };
    let cond = match line.kind {
        Some(Kind::Quest | Kind::HostQuest) => {
            let state = match line.b {
                -1 => "state 0",
                0 => "state 2",
                1 => "bit 2",
                2 => "state 3",
                3 => "state 1",
                4 => "bit 3 & state 0",
                _ => "any",
            };
            format!("quest {} {state}", line.a)
        }
        Some(_) if line.a < 0 => "any step".to_owned(),
        Some(_) => format!("step {}..={}", line.a, line.b),
        None => return "ignored kind".to_owned(),
    };
    let (group, test, n) = (line.slot / 80, line.slot % 80 / 16, line.slot % 16);
    let slot = match test {
        0 => format!("talks >= {n}"),
        1 => format!("talks(b) >= {n}"),
        2 | 3 => format!("context == {}", line.slot),
        _ => format!("substep best {n}"),
    };
    format!(
        "{:<12} {cond:<22} group {group} {slot}{night}",
        format!("{:?}", line.kind.expect("handled above"))
    )
}

fn ops(ops: &[Op]) -> String {
    let parts: Vec<String> = ops
        .iter()
        .map(|op| {
            let operands: Vec<String> = op
                .operands
                .iter()
                .map(|v| match v {
                    Value::Int(i) => format!("{i:#x}"),
                    Value::Float(f) => format!("{f}f"),
                    Value::Str(s) => format!("{:?}", s.map(escape)),
                })
                .collect();
            format!("{:#04x}({}){operands:?}", op.code, op.arg)
        })
        .collect();
    parts.join(" ")
}

/// First room placing `npc` (`<map>place.bin`, kind 3 `[room, npc, x, y, z, angle]`).
fn npc_room(rom: &Rom, map: &str, npc: u32) -> Result<Option<i32>> {
    let narc = Narc::parse(rom.file_by_path(&format!("data/scenario/{map}.npc"))?)?;
    for i in 0..u16::try_from(narc.len())? {
        if narc.name(i).is_some_and(|n| n.ends_with("place.bin")) {
            let table = Table::parse(narc.file(i)?)?;
            return Ok(table.records.iter().find_map(|r| match r.values[..] {
                [Value::Int(room), Value::Int(n), ..] if r.kind == 3 && n == npc => {
                    Some(room as i32)
                }
                _ => None,
            }));
        }
    }
    Ok(None)
}

/// Talk context after the map's talk triggers (event 0), or `None` if a trigger takes the
/// conversation over without the talk action.
fn trigger_context(
    rom: &Rom,
    args: &TalkArgs,
    preview: &Preview,
    map: &str,
    chapter: i32,
) -> Result<Option<i32>> {
    let room = match args.room {
        Some(room) => room,
        None => npc_room(rom, map, preview.npc)?
            .with_context(|| format!("NPC {} not placed in {map}; pass --room", preview.npc))?,
    };
    let data = rom.file_by_path(&format!("data/scenario/trigger{map}.bin"))?;
    let table = Table::parse(data).context("trigger table")?;
    let triggers: Vec<_> = trigger::parse(&table)?
        .into_iter()
        .filter(|t| t.loaded(room, chapter, args.step))
        .collect();
    println!(
        "room {room}, chapter {chapter}, step {}: {} triggers loaded",
        args.step,
        triggers.len()
    );
    match trigger::first_match(&triggers, 0, preview) {
        Err(trigger::Unsupported(code)) => {
            bail!("trigger condition {code:#x} not reimplemented; use --no-triggers")
        }
        Ok(None) => {
            println!("no talk trigger: context = zone {}", args.zone);
            Ok(Some(args.zone))
        }
        Ok(Some(t)) => {
            println!(
                "trigger: if {} then {}",
                ops(&t.conditions),
                ops(&t.actions)
            );
            Ok(match t.talk() {
                Some((npc, context)) if u32::from(npc) == preview.npc => Some(context),
                _ => None,
            })
        }
    }
}

pub fn run(rom: &Rom, args: &TalkArgs) -> Result<()> {
    let stem = args
        .archive
        .rsplit('/')
        .next()
        .and_then(|n| n.strip_suffix("0.gp2"))
        .filter(|s| s.len() == 4)
        .context("archive must be data/scenario/<map><letter>0.gp2")?;
    let (map3, letter) = stem.split_at(3);
    let map = if map3.starts_with('F') { "F" } else { map3 };
    let chapter = CHAPTER_LETTERS
        .iter()
        .position(|&c| c == letter.as_bytes()[0])
        .context("unknown chapter letter")? as i32
        + 1;
    let npc: u32 = args
        .member
        .get(..3)
        .and_then(|n| n.parse().ok())
        .context("member must be NNN_<lang>.bin")?;

    let archive = Gpc2::parse(rom.file_by_path(&args.archive)?).context("parsing archive")?;
    let entry = archive
        .find(&args.member)
        .with_context(|| format!("no member {}", args.member))?;
    let data = archive.member(entry)?;
    let table = Table::parse(&data).context("not a record table")?;
    let lines = dialogue::parse(&table)?;

    let preview = Preview { args, npc };
    let context = if args.no_triggers {
        Some(args.zone)
    } else {
        trigger_context(rom, args, &preview, map, chapter)?
    };
    let Some(context) = context else {
        println!("the trigger replaces the conversation");
        return Ok(());
    };
    let quests = |q: i32| preview.quest(q);
    let state = TalkState {
        step: args.step,
        substep: args.substep,
        context,
        talk_count: [args.talks, args.talks],
        daytime: !args.night,
        session: preview.session(),
        quest: &quests,
    };
    let chosen = dialogue::select(&lines, &state);
    for (i, line) in lines.iter().enumerate() {
        let mark = if chosen == Some(i) { "=>" } else { "  " };
        let text = line.text.map(escape).unwrap_or_default();
        let preview: String = text.chars().take(70).collect();
        println!("{mark}{i:3} {}  {preview}", describe(line));
    }
    match chosen {
        Some(i) => println!("\ncontext {context}, chosen: record {i}"),
        None => println!("\ncontext {context}: no line for this state"),
    }
    Ok(())
}
