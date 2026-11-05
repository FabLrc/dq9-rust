//! `dq9re talk`: decode an NPC dialogue file and show which line the game picks for a given
//! state: the map's talk triggers first (`dq9_core::trigger`, `docs/re/trigger.md`), then
//! the line selection (`dq9_formats::dialogue`, `docs/re/dialogue.md`).

use anyhow::{Context, Result, bail};
use clap::Args;
use dq9_core::trigger::{self, Member, QuestLogEntry, Request, State};
use dq9_formats::dialogue::{self, Kind, Line, Session, TalkState};
use dq9_formats::trigger::{self as rules, Op};
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
    /// Leader's level (in the current vocation).
    #[arg(long, default_value_t = 1)]
    level: u16,
    /// Leader's (and hero's) vocation: 0 Guardian, 1 Warrior … 12 Ranger.
    #[arg(long, default_value_t = 0)]
    vocation: u8,
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
    /// The hero has fallen (HP 0, hero +0x130 → +4) [hypothesis, docs/re/trigger.md]: another
    /// member speaks; enables dialogue kinds 5/6.
    #[arg(long)]
    hero_down: bool,
    /// The leader stands in one of the NPC's talk zones (condition 0x29).
    #[arg(long)]
    in_zone: bool,
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

impl TalkArgs {
    fn session(&self) -> Session {
        Session {
            active: self.session,
            guest: self.guest,
            peer_flag: self.peer_flag,
            hero_alive: !self.hero_down,
        }
    }

    /// The game state given on the command line, talking to `npc`: a party of one (the
    /// hero), the given quests, flags, items and quest-log size.
    fn state(&self, npc: u32) -> State {
        let mut hero = Member {
            vocation: self.vocation,
            hp: u16::from(!self.hero_down),
            ..Member::default()
        };
        if let Some(level) = hero.levels.get_mut(usize::from(self.vocation)) {
            *level = self.level;
        }
        let mut s = State {
            flags: self.flags.iter().copied().collect(),
            quests: self.quests.iter().map(|&(q, v)| (q, v as u8)).collect(),
            time_of_day: u32::from(!self.night),
            session: self.session(),
            items: self.items.iter().copied().collect(),
            party: vec![hero],
            characters: [(0, hero)].into(),
            zones: [(npc, self.in_zone)].into(),
            ..State::default()
        };
        // Entries for no real quest (id 0x1ff): only the count matters here.
        let filler = QuestLogEntry {
            word: 0x1ff,
            ..QuestLogEntry::default()
        };
        s.quest_log.entries = vec![filler; usize::from(self.quest_log)];
        s.event[0] = npc;
        s.event[5] = self.zone as u32;
        s
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
            let name = rules::action_name(op.code).map_or(String::new(), |n| format!("{n}:"));
            format!("{name}{:#04x}({}){operands:?}", op.code, op.arg)
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

/// Talk context after the map's talk triggers (event 0): the trigger's actions run on
/// `world`, then its deferred actions; `None` if no talk with this NPC results.
fn trigger_context(
    rom: &Rom,
    args: &TalkArgs,
    world: &mut State,
    npc: u32,
    map: &str,
    chapter: i32,
) -> Result<Option<i32>> {
    let room = match args.room {
        Some(room) => room,
        None => npc_room(rom, map, npc)?
            .with_context(|| format!("NPC {npc} not placed in {map}; pass --room"))?,
    };
    // The room selects the scenario track that holds the progress and its flags.
    world.track = trigger::track_of_room(room);
    let track = &mut world.tracks[world.track];
    (track.chapter, track.step, track.substep) =
        (chapter as u8, args.step as u8, args.substep as u8);
    let data = rom.file_by_path(&format!("data/scenario/trigger{map}.bin"))?;
    let table = Table::parse(data).context("trigger table")?;
    let triggers: Vec<_> = rules::parse(&table)?
        .into_iter()
        .filter(|t| t.loaded(room, chapter, args.step))
        .collect();
    println!(
        "room {room} (track {}), chapter {chapter}, step {}: {} triggers loaded",
        world.track,
        args.step,
        triggers.len()
    );
    match trigger::fire(&triggers, 0, world) {
        Err(trigger::Unsupported(code)) => {
            bail!("trigger condition {code:#x} not reimplemented; use --no-triggers")
        }
        Ok(None) => {
            println!("no talk trigger: context = zone {}", args.zone);
            Ok(Some(args.zone))
        }
        Ok(Some(fired)) => {
            let t = fired.trigger;
            println!(
                "trigger: if {} then {}",
                ops(&t.conditions),
                ops(&t.actions)
            );
            if !fired.completed {
                println!("the action list stopped: deferred actions are dropped");
                return Ok(None);
            }
            let requests = trigger::run_deferred(&fired.deferred, world);
            println!("requests: {requests:?}");
            Ok(requests.iter().find_map(|r| match *r {
                Request::Talk { npc: n, context } if u32::from(n) == npc => {
                    Some(i32::from(context))
                }
                _ => None,
            }))
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

    let mut world = args.state(npc);
    let context = if args.no_triggers {
        Some(args.zone)
    } else {
        trigger_context(rom, args, &mut world, npc, map, chapter)?
    };
    let Some(context) = context else {
        println!("the trigger replaces the conversation");
        return Ok(());
    };
    let quests = |q: i32| world.quests.get(&q).copied().unwrap_or(0);
    let state = TalkState {
        step: args.step,
        substep: args.substep,
        context,
        talk_count: [args.talks, args.talks],
        daytime: !args.night,
        session: args.session(),
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
