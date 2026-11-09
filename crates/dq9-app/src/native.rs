//! Native entry point: command line, ROM file, window or offscreen screenshot.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::screens::{self, Layout};
use anyhow::{Context as _, Result, bail};
use clap::Parser;
use dq9_core::state::DayClock;
use dq9_core::{Hero, Input};
use dq9_render::{Camera, Environment, Gait, Panel, Renderer};
use glam::Vec3;
use winit::event_loop::EventLoop;

use crate::app::headless::PAD_NAMES;
use crate::app::{App, Options, UserEvent};
use crate::event::EventPlayer;
use crate::talk::{self, Talk};
use crate::{gpu, lighting, load, report, walk};

#[derive(Parser)]
#[command(version, about = "Shows a Dragon Quest IX map (see docs/app.md).")]
struct Cli {
    /// ROM image. Defaults to the only `*.nds` file in `rom/` or the current directory.
    #[arg(long, env = "DQ9_ROM")]
    rom: Option<PathBuf>,
    /// Explicit map code (`M01`) or id (`1100`); overrides the map from `--save`.
    #[arg(long)]
    map: Option<String>,
    /// DS save image; selects its current map when `--map` is absent.
    #[arg(long)]
    save: Option<PathBuf>,
    /// Time of day: `night`, `morning`, `day`, `evening` (or 0-3, `LightingManager+0x98`);
    /// defaults to the save's clock phase, else the night.
    #[arg(long, value_parser = lighting::parse_time)]
    time: Option<u32>,
    /// Second of the 420-second day (night 0, morning 180, day 210, evening 390); its phase
    /// replaces `--time`, and the last two seconds of a phase fade into the next.
    #[arg(long)]
    clock: Option<f32>,
    /// Story chapter (the prologue is 1.1.1).
    #[arg(long)]
    chapter: Option<u8>,
    /// Story step.
    #[arg(long)]
    step: Option<u8>,
    /// Story substep.
    #[arg(long)]
    substep: Option<u8>,
    /// Use nighttime cast placements (independent of --time's map variant).
    #[arg(long)]
    night: bool,
    /// Replaces the map's fog (screenshots only, to compare with a DS capture): `OFFSET,SHIFT,
    /// COLOR,D0,...,D31` with the `FOG_OFFSET` and `FOG_COLOR` register values (decimal or
    /// `0x` hexadecimal), the `DISP3DCNT` depth shift and the 32 `FOG_TABLE` densities.
    #[arg(long, value_parser = parse_fog)]
    fog: Option<dq9_render::lighting::Fog>,
    /// No music (and no sound device is opened); `M` in the window unmutes.
    #[arg(long)]
    mute: bool,
    /// Which DS screens are shown: `stacked` (default), `side` or `top` (`screens.rs`).
    #[arg(long, default_value = "stacked", value_parser = Layout::parse)]
    layout: Layout,
    /// Render one frame offscreen into this PNG and exit, without a window.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Evaluate exactly N fixed animation updates before taking the screenshot.
    #[arg(long, default_value_t = 0)]
    frame: u32,
    /// Screenshot size, `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1024x768", value_parser = parse_size)]
    size: (u32, u32),
    /// Walk without a window: a script of `+x:1.5,-z:3,idle:0.5` (world axes, seconds), logging
    /// the maps and positions passed, including the doors crossed (`docs/app.md`).
    #[arg(long, requires = "map", allow_hyphen_values = true)]
    walk: Option<String>,
    /// Start of `--walk` or `--screenshot` as `x,z` (ground height found, facing +z), instead
    /// of the map's default.
    #[arg(long, value_parser = parse_pair, allow_hyphen_values = true)]
    start: Option<(f32, f32)>,
    /// Camera position `x,y,z` (world units); with `--target`, replaces the framing view.
    #[arg(long, value_parser = parse_vec3, requires = "target", allow_hyphen_values = true)]
    eye: Option<Vec3>,
    /// With `--screenshot`: inputs played before the capture, as `--walk` (`+x:1.5,-z:3,idle:1`)
    /// plus the buttons `a` (action: talk, advance), `b` (cancel), `up` and `down`; each button
    /// is one step. `+x:2,a,idle:3` walks east, talks to the NPC in front and types 3 seconds.
    #[arg(long, requires = "screenshot", allow_hyphen_values = true)]
    input: Option<String>,
    /// Point looked at, `x,y,z`.
    #[arg(long, value_parser = parse_vec3, requires = "eye", allow_hyphen_values = true)]
    target: Option<Vec3>,
    /// Start a scripted event (`data/event/evNNNNN.gp2`, `docs/re/stb.md`) on the map its
    /// event-list record names. With `--screenshot`, plays `--frame N` fixed steps of it.
    #[arg(long)]
    event: Option<u16>,
    /// Start the opening scene of a story stage `MAJOR.MINOR` (`1.1`: the prologue's first
    /// cutscene) and use the stage as the story point.
    #[arg(long, value_parser = parse_stage, conflicts_with = "event")]
    stage: Option<(u32, u32)>,
    /// With an event and `--screenshot`: steps at which the action key is pressed to read
    /// the messages, `300,900`.
    #[arg(long, value_delimiter = ',')]
    press: Vec<u32>,
    /// With an event and `--screenshot`: also presses the action key every N steps.
    #[arg(long)]
    press_every: Option<u32>,
    /// With an event: runs the game logic (triggers, map changes, events) for at most N
    /// fixed steps without a window, pressing the action key every `--press-every` steps,
    /// and logs it.
    #[arg(long)]
    headless_steps: Option<u32>,
    /// With `--headless-steps`: mixes the sound offline (no device) and writes it to this WAV
    /// file: the music, the jingles and the effects the game logic asked for.
    #[arg(long)]
    sound_wav: Option<PathBuf>,
    /// Plays every scene of the story stages `MAJOR.MINOR-MINOR` (`1.1-3`) without a window
    /// and prints which events play fully, partially or not at all.
    #[arg(long, value_parser = parse_stage_range)]
    events_report: Option<(u32, u32, u32)>,
    /// Start a new game: default hero, story 1.1.1, the opening scene of the prologue
    /// (`ev21500`), then the story chains on its own (triggers, scripted battles, maps).
    #[arg(
        long,
        conflicts_with_all = ["event", "stage", "map", "save", "walk", "events_report", "screenshot"]
    )]
    new_game: bool,
    /// Shows a set battle of `data/event/eventbattle.bin` (`--battle 2`: the Hexagoon) on the
    /// record's stage with its monsters, instead of a map (`docs/app.md`, § Battle scene).
    #[arg(
        long,
        conflicts_with_all = [
            "event", "stage", "map", "save", "walk", "events_report", "new_game", "battle_stage"
        ]
    )]
    battle: Option<u16>,
    /// Shows a battle on this stage map (a map id such as `30116`, "F01 - Field").
    #[arg(
        long,
        requires = "monsters",
        conflicts_with_all = [
            "event", "stage", "map", "save", "walk", "events_report", "new_game", "battle"
        ]
    )]
    battle_stage: Option<u16>,
    /// Monsters of `--battle-stage`, `number[xcount]` comma-separated (`1x3,7x1`).
    #[arg(
        long,
        requires = "battle_stage",
        value_delimiter = ',',
        value_parser = parse_monsters
    )]
    monsters: Option<Vec<(u16, u16)>>,
    /// Run the game logic without a window (with `--new-game`, `--event` or `--stage`); needs
    /// `--auto` or `--headless-steps`.
    #[arg(long)]
    headless: bool,
    /// With `--headless`: press the action key to read every message, run until the player
    /// would get control (or `--headless-steps` steps, default 200000), and print what the
    /// story did.
    #[arg(long, requires = "headless")]
    auto: bool,
    /// With `--auto`: at each free control, have the conversation (or take the exit) that moves
    /// the story on, instead of stopping: a player's stand-in (docs/app.md, § New game).
    #[arg(long, requires = "auto")]
    pilot: bool,
    /// With `--auto`: writes the story state at every checkpoint as JSONL (the field names of
    /// `dq9re state --json`), for the melonDS comparison.
    #[arg(long, requires = "auto")]
    checkpoints: Option<PathBuf>,
    /// With `--auto`: writes the pad and action key of every fixed step as JSONL, for replay.
    #[arg(long, requires = "auto")]
    trace_inputs: Option<PathBuf>,
    /// Replays a `--trace-inputs` file without the pilot: each line feeds the pad and the
    /// action key of one fixed step (`--new-game --headless --replay inputs.jsonl`).
    #[arg(long, requires = "headless", conflicts_with = "auto")]
    replay: Option<PathBuf>,
}

impl Cli {
    fn story(&self) -> dq9_formats::cast::Story {
        let mut story = dq9_formats::cast::Story {
            chapter: self.chapter.unwrap_or(1),
            step: self.step.unwrap_or(1),
            substep: self.substep.unwrap_or(1),
            daytime: !self.night,
            ..dq9_formats::cast::Story::default()
        };
        if let Some((major, minor)) = self.stage {
            story.chapter = major as u8;
            story.step = minor as u8;
        }
        story
    }

    /// The event to start: `--event`, or the opening scene of `--stage`.
    fn event_id(&self, rom: &[u8]) -> Result<Option<u16>> {
        if let Some(event) = self.event {
            return Ok(Some(event));
        }
        let Some((major, minor)) = self.stage else {
            return Ok(None);
        };
        let rom = nds_rom::Rom::parse(rom).context("parsing the ROM")?;
        match dq9_core::script::load::stage_opening(&rom, major, minor) {
            Some(event) => Ok(Some(event)),
            None => bail!("no opening scene at story stage {major}.{minor}"),
        }
    }

    /// The battle the options ask for: a set battle (`--battle N`), or a stage and its
    /// monsters (`--battle-stage`/`--monsters`).
    fn battle_request(&self) -> Option<crate::battle::Request> {
        if let Some(record) = self.battle {
            return Some(crate::battle::Request {
                record: Some(record),
                ..Default::default()
            });
        }
        let stage = self.battle_stage?;
        Some(crate::battle::Request {
            record: None,
            stage,
            monsters: self.monsters.clone().unwrap_or_default(),
            track: None,
        })
    }
}

/// `MAJOR.MINOR-MINOR`: a story stage and the last of its steps.
fn parse_stage_range(s: &str) -> Result<(u32, u32, u32), String> {
    let (first, last) = s.split_once('-').ok_or("expected MAJOR.MINOR-MINOR")?;
    let (major, minor) = parse_stage(first)?;
    Ok((
        major,
        minor,
        last.parse().map_err(|e| format!("{last:?}: {e}"))?,
    ))
}

/// `MAJOR.MINOR` of a story stage.
fn parse_stage(s: &str) -> Result<(u32, u32), String> {
    let (major, minor) = s.split_once('.').ok_or("expected MAJOR.MINOR")?;
    Ok((
        major.parse().map_err(|e| format!("{major:?}: {e}"))?,
        minor.parse().map_err(|e| format!("{minor:?}: {e}"))?,
    ))
}

/// `number[xcount]` of `--monsters`: the monster number of `mon_data`, and how many of it
/// stand on the stage (one without `x`).
fn parse_monsters(s: &str) -> Result<(u16, u16), String> {
    let (number, count) = s.split_once('x').unwrap_or((s, "1"));
    let number = number
        .trim()
        .parse()
        .map_err(|e| format!("{number:?}: {e}"))?;
    let count: u16 = count
        .trim()
        .parse()
        .map_err(|e| format!("{count:?}: {e}"))?;
    if count == 0 {
        return Err("a count of zero".into());
    }
    Ok((number, count))
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let parse = |v: &str| match v.parse::<u32>() {
        Ok(n) if (1..=8192).contains(&n) => Ok(n),
        _ => Err(format!("bad size {v:?}")),
    };
    Ok((parse(w)?, parse(h)?))
}

fn parse_pair(s: &str) -> Result<(f32, f32), String> {
    let (x, z) = s.split_once(',').ok_or("expected X,Z")?;
    Ok((
        x.trim().parse().map_err(|e| format!("{e}"))?,
        z.trim().parse().map_err(|e| format!("{e}"))?,
    ))
}

fn parse_fog(s: &str) -> Result<dq9_render::lighting::Fog, String> {
    let values: Vec<u32> = s
        .split(',')
        .map(|v| {
            let v = v.trim();
            match v.strip_prefix("0x") {
                Some(hex) => u32::from_str_radix(hex, 16),
                None => v.parse(),
            }
            .map_err(|e| format!("{v:?}: {e}"))
        })
        .collect::<Result<_, _>>()?;
    if values.len() != 35 {
        return Err("expected OFFSET,SHIFT,COLOR and 32 densities".into());
    }
    let mut table = [0u8; 32];
    for (slot, v) in table.iter_mut().zip(&values[3..]) {
        *slot = (*v & 0x7f) as u8;
    }
    Ok(dq9_render::lighting::Fog {
        color: dq9_render::lighting::rgb5(values[2] as u16),
        alpha_only: false,
        offset: (values[0] & 0x7fff) as u16,
        shift: (values[1] & 0xf) as u8,
        table,
    })
}

fn parse_vec3(s: &str) -> Result<Vec3, String> {
    let v: Vec<f32> = s
        .split(',')
        .map(|c| c.trim().parse::<f32>().map_err(|e| format!("{c:?}: {e}")))
        .collect::<Result<_, _>>()?;
    match v.as_slice() {
        &[x, y, z] => Ok(Vec3::new(x, y, z)),
        _ => Err("expected x,y,z".into()),
    }
}

fn find_rom() -> Result<PathBuf> {
    for dir in [Path::new("rom"), Path::new(".")] {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let roms: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nds")))
            .collect();
        match roms.as_slice() {
            [one] => return Ok(one.clone()),
            [] => {}
            _ => bail!("several *.nds in {}; pass --rom", dir.display()),
        }
    }
    bail!("no *.nds found in rom/ or the current directory; pass --rom or set DQ9_ROM")
}

/// Reads a `--trace-inputs` JSONL file: the pad bits (bit 0 up, 1 down, 2 left, 3 right) and
/// the action key of each fixed step, in order.
fn read_inputs(path: &std::path::Path) -> anyhow::Result<Vec<(u8, bool)>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut frames = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let pad = line
            .split("\"pad\":[")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .unwrap_or_default();
        let mut bits = 0u8;
        for name in pad.split(',') {
            let name = name.trim().trim_matches('"');
            for (bit, known) in PAD_NAMES {
                if known == name {
                    bits |= bit;
                }
            }
        }
        frames.push((bits, line.contains("\"a\":true")));
    }
    Ok(frames)
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = match &cli.rom {
        Some(p) => p.clone(),
        None => find_rom()?,
    };
    let rom = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let save = cli
        .save
        .as_ref()
        .map(|p| fs::read(p).with_context(|| format!("reading save {}", p.display())))
        .transpose()?;
    if let Some(out) = &cli.screenshot {
        return screenshot(&cli, &rom, save.as_deref(), out);
    }
    if let Some(script) = &cli.walk {
        let script = walk::parse_script(script)?;
        // The real application's fixed step, so a walk uses the same Hero/Field/door path as
        // the window (the old Walker dropped Request::OpenDoor and stayed blocked).
        let options = Options {
            map: cli.map.clone(),
            save,
            time: cli.time,
            clock: cli.clock,
            layout: screens::Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: cli.story(),
            story_override: [None; 3],
            new_game: false,
            battle: None,
        };
        let mut app = App::new_headless(options, Some(rom));
        app.walk_pose().context("cannot walk: no map loaded")?;
        if let Some((x, z)) = cli.start {
            app.place_hero(x, z)?;
        }
        let pose = |app: &App| match app.walk_pose() {
            Some((id, code, at, yaw)) => format!(
                "map {id} {code} hero ({:.3}, {:.3}, {:.3}) yaw {:.3}",
                at.x, at.y, at.z, yaw
            ),
            None => "no map".to_owned(),
        };
        println!("start: {}", pose(&app));
        for (index, part) in script.iter().enumerate() {
            let frames = (part.seconds / crate::app::STEP).round() as u32;
            println!("step {index}: {:?} for {frames} frames", part.direction);
            for _ in 0..frames {
                app.walk_step(part.direction);
            }
            println!("after step {index}: {}", pose(&app));
        }
        return Ok(());
    }
    if let Some((major, first, last)) = cli.events_report {
        return report::report(&rom, major, first..=last, &cli.story());
    }
    let event = cli.event_id(&rom)?;
    let options = Options {
        record_sound: cli.sound_wav.is_some() && cli.headless_steps.is_some(),
        trace_inputs: cli.trace_inputs.is_some(),
        map: cli.map.clone(),
        save,
        time: cli.time,
        clock: cli.clock,
        mute: cli.mute,
        event,
        layout: cli.layout,
        story: cli.story(),
        story_override: [cli.chapter, cli.step, cli.substep],
        new_game: cli.new_game,
        battle: cli.battle_request(),
    };
    // Headless runs have no window, GPU task or `UserEvent`: do not make them depend on
    // winit accepting an event loop. The visible window keeps its real proxy.
    let headless = cli.headless || cli.headless_steps.is_some();
    let event_loop = if headless {
        None
    } else {
        Some(EventLoop::<UserEvent>::with_user_event().build()?)
    };
    let mut app = match &event_loop {
        Some(event_loop) => App::new(event_loop.create_proxy(), options, Some(rom)),
        None => App::new_headless(options, Some(rom)),
    };
    if let Some(path) = &cli.replay {
        let frames = read_inputs(path)?;
        for (bits, confirm) in &frames {
            app.replay_step(*bits, *confirm);
        }
        println!("replayed {} fixed steps", frames.len());
        if let Some(path) = &cli.checkpoints {
            std::fs::write(path, app.checkpoints_jsonl())
                .with_context(|| format!("writing {}", path.display()))?;
        }
        return Ok(());
    }
    if cli.auto {
        let stop = app.play_auto(u64::from(cli.headless_steps.unwrap_or(200_000)), cli.pilot);
        print!("{}", app.auto_report(&stop));
        if let Some(battle) = app.last_battle() {
            println!(
                "battle scene built: stage {} ({} fighters)",
                battle.stage, battle.fighters
            );
        }
        if let Some(path) = &cli.checkpoints {
            std::fs::write(path, app.checkpoints_jsonl())
                .with_context(|| format!("writing {}", path.display()))?;
        }
        if let Some(path) = &cli.trace_inputs {
            std::fs::write(path, app.input_trace_jsonl())
                .with_context(|| format!("writing {}", path.display()))?;
        }
        return Ok(());
    }
    if cli.headless || cli.headless_steps.is_some() {
        app.play_headless(cli.headless_steps.unwrap_or(200_000), cli.press_every);
        if let Some(battle) = app.last_battle() {
            println!(
                "battle scene built: stage {} ({} fighters)",
                battle.stage, battle.fighters
            );
        }
        if let (Some(path), Some((rate, samples))) = (&cli.sound_wav, app.take_recording()) {
            write_sound(path, rate, &samples)?;
        }
        return Ok(());
    }
    if let (Some(eye), Some(target)) = (cli.eye, cli.target) {
        app.look_at(eye, target);
    }
    if let Some(event_loop) = event_loop {
        event_loop.run_app(&mut app)?;
    }
    Ok(())
}

/// What `--screenshot` prepared before rendering: the loaded map, the hero after the inputs,
/// the day clock, and the event playback's effects on the frame.
struct Prepared {
    loaded: load::LoadedMap,
    hero: Hero,
    clock: DayClock,
    event: Option<u16>,
    script: Option<Vec<walk::Walk>>,
    talk: Talk,
    hidden: bool,
    swapped: bool,
    event_environment: Option<Environment>,
}

/// Loads the map of a `--screenshot`, places the hero (`--start`), plays `--input`, and logs
/// the pose and the map summary.
fn prepare(cli: &Cli, rom: &[u8], save: Option<&[u8]>) -> Result<Prepared> {
    let event = cli.event_id(rom)?;
    let place = event.and_then(|e| crate::event::place(rom, e));
    let start = load::start(cli.map.as_deref(), save)?;
    let clock = lighting::day_clock(start.state.as_ref().map(|s| s.clock), cli.time, cli.clock);
    let time = clock.phase().index();
    // An event happens on the map its record names, the hero at its start.
    let (map, spawn) = match place {
        Some(place) => (place.map.to_string(), crate::event::spawn(&place)),
        None => (start.map.clone(), None),
    };
    let mut trigger_state = load::trigger_state(
        start.state.as_ref(),
        &cli.story(),
        [cli.chapter, cli.step, cli.substep],
    );
    let story = if start.state.is_some() {
        load::story_from_state(
            &cli.story(),
            &trigger_state,
            map.parse()
                .ok()
                .or_else(|| start.state.as_ref().map(|s| s.map)),
        )
    } else {
        cli.story()
    };
    let mut loaded = load::load(
        rom,
        &map,
        time,
        &story,
        spawn,
        start.state.as_ref(),
        &start.look,
    )?;
    let script = cli.walk.as_deref().map(walk::parse_script).transpose()?;
    let mut hero = loaded.hero;
    if let Some(at) = cli.start {
        hero = walk::start_hero(&loaded.collision, &start.map, at)?;
    }
    let mut talk = Talk::default();
    talk.set_identity(start.identity);
    if let Some(input) = &cli.input {
        // The inputs start where `--start` puts the hero and leave it where they end.
        loaded.hero = hero;
        talk.set_rom(rom);
        let steps = talk::parse_inputs(input)?;
        talk::play(&steps, &mut loaded, rom, &mut trigger_state, &mut talk)?;
        hero = loaded.hero;
    }
    println!(
        "hero: ({:.3}, {:.3}, {:.3}) yaw {:.3} ({})",
        hero.position().x,
        hero.position().y,
        hero.position().z,
        hero.yaw(),
        if loaded.placed {
            "from the save"
        } else {
            "computed"
        }
    );
    println!("{}", loaded.summary());
    Ok(Prepared {
        loaded,
        hero,
        clock,
        event,
        script,
        talk,
        hidden: false,
        swapped: false,
        event_environment: None,
    })
}

/// Plays the cutscene of `--event` over `--frame` steps, or the `--walk` preview steps, and
/// leaves the hero instance at the pose the playback ended on.
fn play_event_or_walk(
    cli: &Cli,
    rom: &[u8],
    prepared: &mut Prepared,
    camera: &mut Camera,
) -> Result<()> {
    let Prepared {
        loaded,
        hero,
        clock,
        event,
        script,
        talk,
        hidden,
        swapped,
        event_environment,
    } = prepared;
    // The same fixed steps as the window: the hero follows the `--walk` script (standing once
    // it has ended, or without one) and plays the motion its speed asks for; an event plays
    // its script instead.
    if let Some(event) = *event {
        talk.set_rom(rom);
        let state = dq9_core::field::initial_state(&cli.story());
        let mut player = EventPlayer::new();
        player.queue(event);
        let played = report::play(
            rom,
            loaded,
            hero,
            camera,
            talk,
            &mut player,
            &state,
            Some(cli.frame),
            &cli.press,
            cli.press_every,
        )?;
        if let Some(next) = played.elsewhere {
            println!("event {next} is on another map: the screenshot stops there");
        }
        println!("events started: {:?}", player.started());
        println!("waiting: {}", player.waiting());
        println!(
            "events: {} not executed: {:?}; natives without effect: {:?}",
            player.coverage().events,
            player.coverage().missing,
            player.unimplemented()
        );
        *hidden = player.hero_hidden();
        *swapped = player.screen_swapped();
        for command in player.take_clock_commands() {
            lighting::apply_clock_command(clock, command);
        }
        *event_environment = player.environment(loaded.lighting.as_ref(), *clock);
        // An explicit `--eye`/`--target` looks at the scene the script left, whatever its
        // camera did (to inspect the actors).
        if let (Some(eye), Some(target)) = (cli.eye, cli.target) {
            camera.look_at(eye, target);
        }
    } else {
        // Preview path only (`--screenshot`, no `App`, no field task): one `Hero::step` per
        // requested frame at `ticks = 1`, for inspecting poses and the animation clock. It
        // deliberately does not implement the game's 30 Hz field cadence; gameplay (window,
        // Web, pilot, replay) runs through `App::run_steps`, which gates it.
        for frame in 0..cli.frame {
            let direction = script
                .as_deref()
                .map_or(Vec3::ZERO, |s| walk::direction_at(s, frame));
            let before = hero.position();
            hero.step(&loaded.collision, Input::from_direction(direction), 0, 1);
            if cli.eye.is_none() || cli.target.is_none() {
                camera.follow_hero(hero.position());
            }
            let moved = hero.position() - before;
            let speed = Vec3::new(moved.x, 0.0, moved.z).length() / crate::app::STEP;
            loaded.runtime.set_hero_gait(Gait::from_speed(speed));
            loaded.runtime.step(camera)?;
        }
    }
    let world = if *hidden {
        glam::Mat4::from_scale(Vec3::ZERO)
    } else {
        loaded
            .runtime
            .hero_world(hero.position(), dq9_core::field::angle_of_yaw(hero.yaw()))
    };
    loaded.runtime.scene.instances[loaded.hero_instance].world = world;
    if script.is_some() || cli.start.is_some() {
        println!(
            "hero after {} steps: ({:.3}, {:.3}, {:.3}) yaw {:.3}, motion {:?}",
            cli.frame,
            hero.position().x,
            hero.position().y,
            hero.position().z,
            hero.yaw(),
            loaded.runtime.hero_gait()
        );
    }
    loaded.runtime.face_camera(camera)?;
    Ok(())
}

/// Renders the prepared scene offscreen, writes it as PNG and prints the camera it used.
fn write_png(
    cli: &Cli,
    prepared: &mut Prepared,
    camera: &Camera,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    out: &Path,
) -> Result<()> {
    let Prepared {
        loaded,
        clock,
        talk,
        swapped,
        event_environment,
        ..
    } = prepared;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(device, queue, format, &loaded.runtime.scene);
    renderer.environment = event_environment
        .take()
        .unwrap_or_else(|| lighting::environment(loaded.lighting.as_ref(), *clock));
    if let Some(fog) = cli.fog {
        renderer.environment.fog = Some(fog);
    }
    let (w, h) = cli.size;
    let shown = screens::screens(cli.layout, w, h);
    let field = shown.field(*swapped);
    let panel = Panel::new(device, format);
    let rgba = renderer
        .render_to_rgba_with(device, queue, (w, h), field, camera, |encoder, view| {
            if let Some(overlay) = shown.overlay(*swapped) {
                panel.draw(encoder, view, overlay);
            }
            talk.draw(device, queue, format, encoder, view, field);
        })
        .context("reading the frame back")?;
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    save_png(out, cli.size, &rgba)?;
    println!(
        "camera: eye {:.2},{:.2},{:.2} target {:.2},{:.2},{:.2}",
        camera.eye().x,
        camera.eye().y,
        camera.eye().z,
        camera.target.x,
        camera.target.y,
        camera.target.z
    );
    println!("wrote {}", out.display());
    Ok(())
}

/// Writes RGBA8 rows as a PNG, creating the directory.
fn save_png(out: &Path, (w, h): (u32, u32), rgba: &[u8]) -> Result<()> {
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let file = File::create(out).with_context(|| format!("creating {}", out.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Renders one frame of a battle scene offscreen and writes it as PNG: what the window shows
/// after the transition, without one (`--battle 2 --screenshot …`, `docs/app.md`,
/// § "Battle scene").
fn battle_screenshot(
    cli: &Cli,
    rom: &[u8],
    request: crate::battle::Request,
    out: &Path,
) -> Result<()> {
    let clock = lighting::day_clock(None, cli.time, cli.clock);
    let time = clock.phase().index();
    let resolved = request.resolve(rom)?;
    let mut scene = crate::battle::BattleScene::build(
        rom,
        &resolved,
        time,
        clock,
        &cli.story(),
        &dq9_formats::hero::HeroLook::new_game(),
    )?;
    println!(
        "battle{}: stage {} {} ({}), {:?} / {:?}, {} fighters, {} triangles",
        resolved
            .number
            .map_or_else(String::new, |n| format!(" {n}")),
        scene.stage(),
        scene.entry().code,
        scene.label(),
        scene.shot(),
        scene.formation(),
        scene.fighters().len(),
        scene.runtime().scene.triangle_count()
    );
    for placed in scene.fighters() {
        let Some(fighter) = scene.fighter(placed.fighter) else {
            continue;
        };
        let screen = scene
            .fighter_screen_pos(fighter.fighter)
            .map(|at| format!(" screen ({:.0}, {:.0})", at.x, at.y))
            .unwrap_or_default();
        println!(
            "  {:?} {} at ({:.3}, {:.3}, {:.3}) heading {:.3}{screen}",
            fighter.fighter,
            fighter
                .number
                .map_or_else(|| "hero".to_owned(), |n| format!("monster {n}")),
            fighter.position.x,
            fighter.position.y,
            fighter.position.z,
            fighter.heading
        );
    }
    for _ in 0..cli.frame {
        scene.step()?;
    }
    let mut camera = *scene.camera();
    if let (Some(eye), Some(target)) = (cli.eye, cli.target) {
        camera.look_at(eye, target);
    }
    let (device, queue) = pollster::block_on(async {
        let instance = gpu::instance().await;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .context("no GPU adapter")?;
        let info = adapter.get_info();
        println!("adapter: {} ({:?})", info.name, info.backend);
        gpu::device(&adapter).await
    })?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&device, &queue, format, &scene.runtime().scene);
    renderer.environment = scene.environment.clone();
    if let Some(fog) = cli.fog {
        renderer.environment.fog = Some(fog);
    }
    let shown = screens::screens(cli.layout, cli.size.0, cli.size.1);
    let field = shown.field(false);
    let panel = Panel::new(&device, format);
    let rgba = renderer
        .render_to_rgba_with(
            &device,
            &queue,
            cli.size,
            field,
            &camera,
            |encoder, view| {
                if let Some(overlay) = shown.overlay(false) {
                    panel.draw(encoder, view, overlay);
                }
            },
        )
        .context("reading the frame back")?;
    save_png(out, cli.size, &rgba)?;
    println!(
        "camera: eye {:.2},{:.2},{:.2} target {:.2},{:.2},{:.2} half-angle {:.1}°",
        camera.eye().x,
        camera.eye().y,
        camera.eye().z,
        camera.target.x,
        camera.target.y,
        camera.target.z,
        camera.fovy.to_degrees() * 0.5
    );
    println!("wrote {}", out.display());
    Ok(())
}

/// Renders one frame of the map offscreen and writes it as PNG.
fn screenshot(cli: &Cli, rom: &[u8], save: Option<&[u8]>, out: &Path) -> Result<()> {
    if let Some(request) = cli.battle_request() {
        return battle_screenshot(cli, rom, request, out);
    }
    let mut prepared = prepare(cli, rom, save)?;
    let (device, queue) = pollster::block_on(async {
        let instance = gpu::instance().await;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .context("no GPU adapter")?;
        let info = adapter.get_info();
        println!("adapter: {} ({:?})", info.name, info.backend);
        gpu::device(&adapter).await
    })?;
    let mut camera = Camera::field(
        prepared.hero.position(),
        if prepared.loaded.placed {
            prepared.hero.yaw()
        } else {
            0.0
        },
    );
    if let (Some(eye), Some(target)) = (cli.eye, cli.target) {
        camera.look_at(eye, target);
    }
    camera.far = camera.far.max(camera.distance * 4.0);
    prepared.loaded.runtime.face_camera(&camera)?;
    play_event_or_walk(cli, rom, &mut prepared, &mut camera)?;
    write_png(cli, &mut prepared, &camera, &device, &queue, out)
}

/// Writes the recorded sound as a WAV file and reports its length and level.
fn write_sound(path: &Path, rate: u32, samples: &[f32]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, dq9_audio::wav(samples, rate))
        .with_context(|| format!("writing {}", path.display()))?;
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    println!(
        "sound: {:.2} s at {rate} Hz, peak {peak:.3}, written to {}",
        samples.len() as f64 / 2.0 / f64::from(rate),
        path.display()
    );
    Ok(())
}
