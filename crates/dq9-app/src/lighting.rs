//! Glue between the game's lighting model (`dq9_core::lighting`) and the renderer: the `.bats`
//! script of a map, the day clock the options select, and the renderer's [`Environment`].

use dq9_core::cutscene::{ClockCommand, SceneLight, SceneTime};
use dq9_core::lighting::{DayState, Lighting, settle_basic_entry};
use dq9_core::state::{DayClock, TimeOfDay};
use dq9_formats::lighting::{LightingInfo, LightingMode};
use dq9_formats::map::Map;
use dq9_render::Environment;
use dq9_render::lighting::{Background, Fog, Light, Tint, rgb5};
use nds_rom::Narc;

/// The map's lighting script, when its file exists and parses. `file` returns a ROM file by
/// path.
pub fn script<'a>(map: &Map, file: impl Fn(&str) -> Option<&'a [u8]>) -> Option<LightingInfo> {
    let (lighting, found) = map.lighting.as_ref()?;
    if !found {
        return None;
    }
    let narc = Narc::parse(file(&lighting.archive)?).ok()?;
    let entry = narc
        .names
        .files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case(&lighting.member))?;
    let data = nds_rom::compress::decompress(narc.file(entry.id).ok()?).ok()?;
    LightingInfo::parse(&data).ok()
}

/// Phase of an index 0 to 3 (larger values wrap).
pub fn phase(index: u32) -> TimeOfDay {
    match index % 4 {
        0 => TimeOfDay::Night,
        1 => TimeOfDay::Morning,
        2 => TimeOfDay::Day,
        _ => TimeOfDay::Evening,
    }
}

/// Applies one clock command of a running script (579, 588, 589, 808): the day clock starts or
/// stops, or jumps to a phase.
pub fn apply_clock_command(clock: &mut DayClock, command: ClockCommand) {
    match command {
        ClockCommand::Running(running) => clock.set_running(running),
        ClockCommand::SetPhase(index) => clock.set_phase(phase(index as u32)),
    }
}

/// The day clock of a start: the save's (else the start of the night), its phase replaced by
/// `--time`, then its second by `--clock`.
pub fn day_clock(saved: Option<DayClock>, time: Option<u32>, clock: Option<f32>) -> DayClock {
    let mut c = saved.unwrap_or_else(|| DayClock::new(0.0, true));
    if let Some(time) = time {
        c.set_phase(phase(time));
    }
    if let Some(seconds) = clock {
        c = DayClock::new(seconds, c.is_running());
    }
    c
}

/// A phase name (`night`, `morning`, `day`, `evening`) or index, as an index 0 to 3.
pub fn parse_time(text: &str) -> Result<u32, String> {
    let text = text.trim();
    if let Ok(n) = text.parse::<u32>() {
        return if n < 4 {
            Ok(n)
        } else {
            Err(format!("time {n} is not in 0-3"))
        };
    }
    ["night", "morning", "day", "evening"]
        .iter()
        .position(|n| n.eq_ignore_ascii_case(text))
        .map(|i| i as u32)
        .ok_or_else(|| format!("expected night, morning, day, evening or 0-3, got {text:?}"))
}

/// What the game shows for the zone at `clock` (a basic zone entered near the end of a phase
/// moves the clock on, `ProcessZoneChange`), as the renderer's environment.
pub fn environment(info: Option<&LightingInfo>, clock: DayClock) -> Environment {
    scene_environment(info, clock, &SceneLight::default())
}

/// [`environment`] as a running script shapes it: the time of day it forces (588, 589), the
/// lighting index (548), the light scale fade (578) and the fog switch (804).
pub fn scene_environment(
    info: Option<&LightingInfo>,
    mut clock: DayClock,
    light: &SceneLight,
) -> Environment {
    let day = match light.time {
        SceneTime::Clock => {
            if info.is_some_and(|i| i.mode == LightingMode::Basic) {
                settle_basic_entry(&mut clock);
            }
            DayState::from_clock(&clock)
        }
        SceneTime::Forced(phase) => DayState::at(self::phase(phase as u32)),
        SceneTime::Saved => DayState::at(clock.phase()),
    };
    let lighting = Lighting::select(info, day, light.index_override, light.scale());
    let mut environment = convert(&lighting);
    if !light.fog {
        environment.fog = None;
    }
    environment.cover = light.cover();
    environment
}

fn unit(color: u16) -> [f32; 3] {
    rgb5(color).map(|c| f32::from(c) / 31.0)
}

/// `Lighting` to the renderer's terms.
pub fn convert(l: &Lighting) -> Environment {
    Environment {
        lights: l.lights.map(|light| Light {
            direction: light.direction(),
            color: unit(light.color),
        }),
        ambient: (l.mode == LightingMode::Advanced).then(|| unit(l.ambient)),
        model_diffuse: unit(l.model_diffuse),
        sprite_diffuse: unit(l.sprite_diffuse),
        tint: l.tint.map(|t| Tint {
            mix: t.mix,
            brightness: t.brightness,
            contrast: t.contrast,
        }),
        fog: l.fog.enabled.then(|| Fog {
            color: rgb5(l.fog.color),
            alpha_only: l.fog.kind != 0,
            offset: l.fog.offset,
            shift: l.fog.depth_shift,
            table: l.fog.table,
        }),
        background: Background {
            outer: rgb5(l.background),
            horizon: rgb5(l.horizon),
            center_offset: l.gradient_center_offset,
        },
        // The game turns edge marking on at start-up (bit 5 of DISP3DCNT in every melonDS state).
        edge: Some(rgb5(l.edge)),
        cover: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_options_accept_names_and_indices() {
        assert_eq!(parse_time("evening"), Ok(3));
        assert_eq!(parse_time("2"), Ok(2));
        assert!(parse_time("dusk").is_err());
    }

    #[test]
    fn explicit_clock_overrides_the_phase() {
        let c = day_clock(None, Some(0), Some(200.0));
        assert_eq!(c.phase(), TimeOfDay::Morning);
        assert_eq!(day_clock(None, Some(3), None).seconds(), 390.0);
        let saved = DayClock::new(100.0, true);
        assert_eq!(day_clock(Some(saved), None, None).seconds(), 100.0);
    }

    #[test]
    fn no_script_gives_black_lights_and_no_fog() {
        let env = environment(None, day_clock(None, Some(2), None));
        assert!(env.fog.is_none() && env.ambient.is_none());
        assert_eq!(env.lights[0].color, [0.0; 3]);
    }
}
