//! `dq9re talk`: decode an NPC dialogue file and show which line the game picks for a given
//! state (`dq9_formats::dialogue`, `docs/re/dialogue.md`).

use anyhow::{Context, Result, bail};
use clap::Args;
use dq9_formats::dialogue::{self, Kind, Line, Session, TalkState};
use dq9_formats::{Gpc2, Table};
use nds_rom::Rom;

use crate::text::escape;

#[derive(Args)]
pub struct TalkArgs {
    /// Archive inside the ROM, e.g. `data/scenario/M02M0.gp2`.
    archive: String,
    /// Member name, e.g. `018_en.bin` (NPC 18).
    member: String,
    /// Scenario step (save +0x5cb4).
    #[arg(long, default_value_t = 1)]
    step: i32,
    /// Save +0x5cb8, tested by group-4 slots.
    #[arg(long, default_value_t = 0)]
    substep: i32,
    /// Talk context: talk-zone id (0 outside zones, 80…) or a value forced by a trigger.
    #[arg(long, default_value_t = 0)]
    context: i32,
    /// Times already talked to this NPC (both counters).
    #[arg(long, default_value_t = 0)]
    talks: i32,
    #[arg(long)]
    night: bool,
    /// Quest save nibble, `QUEST=NIBBLE` (state in bits 0-1: 1 offered, 2 accepted,
    /// 3 done); repeatable.
    #[arg(long = "quest", value_parser = parse_quest)]
    quests: Vec<(i32, u8)>,
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
}

fn parse_quest(s: &str) -> Result<(i32, u8)> {
    let Some((q, n)) = s.split_once('=') else {
        bail!("expected QUEST=NIBBLE");
    };
    Ok((q.parse()?, n.parse()?))
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

pub fn run(rom: &Rom, args: &TalkArgs) -> Result<()> {
    let archive = Gpc2::parse(rom.file_by_path(&args.archive)?).context("parsing archive")?;
    let entry = archive
        .find(&args.member)
        .with_context(|| format!("no member {}", args.member))?;
    let data = archive.member(entry)?;
    let table = Table::parse(&data).context("not a record table")?;
    let lines = dialogue::parse(&table)?;

    let quests = |q: i32| {
        args.quests
            .iter()
            .find(|&&(id, _)| id == q)
            .map_or(0, |&(_, n)| n)
    };
    let state = TalkState {
        step: args.step,
        substep: args.substep,
        context: args.context,
        talk_count: [args.talks, args.talks],
        daytime: !args.night,
        session: Session {
            active: args.session,
            guest: args.guest,
            peer_flag: args.peer_flag,
            hero_flag: !args.hero_flag_clear,
        },
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
        Some(i) => println!("\nchosen: record {i}"),
        None => println!("\nno line for this state"),
    }
    Ok(())
}
