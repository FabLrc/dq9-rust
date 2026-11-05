//! Native entry point: command line, ROM file, window or offscreen screenshot.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use clap::Parser;
use dq9_core::Input;
use dq9_render::camera::{DS_ASPECT, letterbox};
use dq9_render::{Camera, Gait, Renderer, Viewport};
use glam::Vec3;
use winit::event_loop::EventLoop;

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
    #[arg(long, default_value_t = 1)]
    chapter: u8,
    /// Story step.
    #[arg(long, default_value_t = 1)]
    step: u8,
    /// Story substep.
    #[arg(long, default_value_t = 1)]
    substep: u8,
    /// Use nighttime cast placements (independent of --time's map variant).
    #[arg(long)]
    night: bool,
    /// Replaces the map's fog (screenshots only, to compare with a DS capture): `OFFSET,SHIFT,
    /// COLOR,D0,...,D31` with the `FOG_OFFSET` and `FOG_COLOR` register values (decimal or
    /// `0x` hexadecimal), the `DISP3DCNT` depth shift and the 32 `FOG_TABLE` densities.
    #[arg(long, value_parser = parse_fog)]
    fog: Option<dq9_render::lighting::Fog>,
    /// Draw at the DS screen aspect (256×192), letterboxed.
    #[arg(long)]
    ds_aspect: bool,
    /// No music (and no sound device is opened); `M` in the window unmutes.
    #[arg(long)]
    mute: bool,
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
    /// Plays every scene of the story stages `MAJOR.MINOR-MINOR` (`1.1-3`) without a window
    /// and prints which events play fully, partially or not at all.
    #[arg(long, value_parser = parse_stage_range)]
    events_report: Option<(u32, u32, u32)>,
}

impl Cli {
    fn story(&self) -> dq9_formats::cast::Story {
        let mut story = dq9_formats::cast::Story {
            chapter: self.chapter,
            step: self.step,
            substep: self.substep,
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
        let map = load::start(cli.map.as_deref(), save.as_deref())?.map;
        let time = cli.time.unwrap_or(0);
        let mut walker = walk::Walker::new(&rom, &map, time, &cli.story(), cli.start)?;
        return walker.run(&script);
    }
    if let Some((major, first, last)) = cli.events_report {
        return report::report(&rom, major, first..=last, &cli.story());
    }
    let event = cli.event_id(&rom)?;
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let options = Options {
        map: cli.map.clone(),
        save,
        time: cli.time,
        clock: cli.clock,
        ds_aspect: cli.ds_aspect,
        mute: cli.mute,
        event,
        story: cli.story(),
    };
    let mut app = App::new(event_loop.create_proxy(), options, Some(rom));
    if let Some(steps) = cli.headless_steps {
        app.play_headless(steps, cli.press_every);
        return Ok(());
    }
    if let (Some(eye), Some(target)) = (cli.eye, cli.target) {
        app.look_at(eye, target);
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Renders one frame of the map offscreen and writes it as PNG.
fn screenshot(cli: &Cli, rom: &[u8], save: Option<&[u8]>, out: &Path) -> Result<()> {
    let event = cli.event_id(rom)?;
    let place = event.and_then(|e| crate::event::place(rom, e));
    let start = load::start(cli.map.as_deref(), save)?;
    let clock = lighting::day_clock(start.state.as_ref().map(|s| s.clock), cli.time, cli.clock);
    let time = clock.phase().index();
    // An event happens on the map its record names, the hero at its start.
    let (map, spawn) = match place {
        Some(place) => (
            place.map.to_string(),
            place
                .position
                .map(|(p, angle)| (Vec3::from_array(p), dq9_core::field::yaw_of_angle(angle))),
        ),
        None => (start.map.clone(), None),
    };
    let mut loaded = load::load(
        rom,
        &map,
        time,
        &cli.story(),
        spawn,
        start.state.as_ref(),
        &start.look,
    )?;
    let script = cli.walk.as_deref().map(walk::parse_script).transpose()?;
    let mut hidden = false;
    let mut hero = loaded.hero;
    if let Some(at) = cli.start {
        hero = walk::start_hero(&loaded.collision, &start.map, at)?;
    }
    let mut talk = Talk::default();
    if let Some(input) = &cli.input {
        // The inputs start where `--start` puts the hero and leave it where they end.
        loaded.hero = hero;
        talk.set_rom(rom);
        let steps = talk::parse_inputs(input)?;
        talk::play(&steps, &mut loaded, rom, &cli.story(), &mut talk)?;
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
    let s = &loaded.runtime.scene;
    println!(
        "{} {} ({}), time {}: {} models, {} textures, {} instances, {} triangles",
        loaded.entry.id,
        loaded.entry.code,
        loaded.label,
        loaded.time,
        s.models.len(),
        s.textures.len(),
        s.instances.len(),
        s.triangle_count()
    );
    for w in &s.warnings {
        println!("  warning: {w}");
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
    let mut camera = Camera::default();
    match (cli.eye, cli.target, s.bounds()) {
        (Some(eye), Some(target), _) => camera.look_at(eye, target),
        (_, _, Some(bounds)) => {
            camera.frame(bounds);
            if loaded.placed || cli.input.is_some() {
                camera.target = hero.position();
            }
        }
        _ => {}
    }
    camera.far = camera.far.max(camera.distance * 4.0);
    loaded.runtime.face_camera(&camera)?;
    // The same fixed steps as the window: the hero follows the `--walk` script (standing once
    // it has ended, or without one) and plays the motion its speed asks for; an event plays
    // its script instead.
    if let Some(event) = event {
        talk.set_rom(rom);
        let state = dq9_core::field::initial_state(&cli.story());
        let mut player = EventPlayer::new();
        player.queue(event);
        let played = report::play(
            rom,
            &mut loaded,
            &mut hero,
            &mut camera,
            &mut talk,
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
        hidden = player.hero_hidden();
    } else {
        for frame in 0..cli.frame {
            let direction = script
                .as_deref()
                .map_or(Vec3::ZERO, |s| walk::direction_at(s, frame));
            let before = hero.position();
            hero.step(&loaded.collision, Input::from_direction(direction), 0, 1);
            let moved = hero.position() - before;
            let speed = Vec3::new(moved.x, 0.0, moved.z).length() / crate::app::STEP;
            loaded.runtime.set_hero_gait(Gait::from_speed(speed));
            loaded.runtime.step(&camera)?;
        }
    }
    let world = if hidden {
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
    loaded.runtime.face_camera(&camera)?;
    let mut renderer = Renderer::new(&device, &queue, format, &loaded.runtime.scene);
    renderer.environment = lighting::environment(loaded.lighting.as_ref(), clock);
    // Events are framed for one DS screen.
    let ds_aspect = cli.ds_aspect || event.is_some();
    renderer.edge_pixels = (!ds_aspect).then_some(1);
    if let Some(fog) = cli.fog {
        renderer.environment.fog = Some(fog);
    }
    let (w, h) = cli.size;
    let viewport = if ds_aspect {
        let (x, y, width, height) = letterbox(w, h, DS_ASPECT);
        Viewport {
            x,
            y,
            width,
            height,
        }
    } else {
        Viewport::full(w, h)
    };
    let rgba = renderer
        .render_to_rgba_with(
            &device,
            &queue,
            (w, h),
            viewport,
            &camera,
            |encoder, view| {
                talk.draw(&device, &queue, format, encoder, view, (w, h));
            },
        )
        .context("reading the frame back")?;
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = File::create(out).with_context(|| format!("creating {}", out.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgba)?;
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
