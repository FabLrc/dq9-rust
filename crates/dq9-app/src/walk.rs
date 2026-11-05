//! Headless walk: the field of the application without a window, driven by a script of
//! inputs, logging the maps and positions it passes through (`dq9-app --walk`, `docs/app.md`).
//!
//! It runs the same fixed steps as the window: `Hero::step` on the map's collision, then
//! `Field::step` with the leader's pose, then the map changes the field returns, loading the
//! destination at its arrival pose and running its load and arrival triggers.

use anyhow::{Result, bail};
use dq9_core::Hero;
use dq9_core::field::{self, Event, Field, Leader, MapChange};
use dq9_core::trigger::State;
use dq9_formats::cast::Story;
use glam::Vec3;

use crate::app::STEP;
use crate::load::{self, MapEntry};

/// One part of a script: walk in a world direction for some seconds, or stand still.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walk {
    pub direction: Vec3,
    pub seconds: f32,
}

/// Parses `+x:1.5,-z:3,+x-z:0.5,idle:0.5`: one or two signed world axes (`x` east, `z` south,
/// `-z` north; two make a diagonal) or `idle`, a colon and a duration in seconds.
pub fn parse_script(text: &str) -> Result<Vec<Walk>> {
    text.split(',')
        .map(|part| {
            let Some((what, seconds)) = part.trim().split_once(':') else {
                bail!("walk step {part:?} is not `<direction>:<seconds>`");
            };
            let direction = if what == "idle" {
                Vec3::ZERO
            } else {
                parse_axes(what)?
            };
            let seconds: f32 = seconds.parse()?;
            if seconds.is_nan() || seconds < 0.0 {
                bail!("negative duration in {part:?}");
            }
            Ok(Walk { direction, seconds })
        })
        .collect()
}

/// `+x`, `-z`, `+x-z`…: each axis at most once.
fn parse_axes(text: &str) -> Result<Vec3> {
    let mut direction = Vec3::ZERO;
    let mut seen = [false; 2];
    let bytes = text.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) || bytes.len() > 4 {
        bail!("unknown direction {text:?}: signed axes such as +x, -z, +x-z, or idle");
    }
    for pair in bytes.chunks(2) {
        let sign = match pair[0] {
            b'+' => 1.0,
            b'-' => -1.0,
            _ => bail!("unknown direction {text:?}: signed axes such as +x, -z, +x-z, or idle"),
        };
        let axis = match pair[1] {
            b'x' => 0,
            b'z' => 1,
            _ => bail!("unknown direction {text:?}: signed axes such as +x, -z, +x-z, or idle"),
        };
        if std::mem::replace(&mut seen[axis], true) {
            bail!("axis repeated in {text:?}");
        }
        direction[axis * 2] += sign;
    }
    Ok(direction)
}

/// The simulation state of one walk.
pub struct Walker<'r> {
    rom: &'r [u8],
    time: u32,
    story: Story,
    pub entry: MapEntry,
    collision: dq9_core::CollisionWorld,
    pub hero: Hero,
    field: Field,
    state: State,
    /// Map changes taken, as `(from, to)` ids.
    pub changes: Vec<(u16, u16)>,
    chain: u8,
}

impl<'r> Walker<'r> {
    /// Loads `key`; `start` is an `x, z` position (ground height found, facing +z).
    pub fn new(
        rom: &'r [u8],
        key: &str,
        time: u32,
        story: &Story,
        start: Option<(f32, f32)>,
    ) -> Result<Self> {
        let loaded = load::load(rom, key, time, story, None)?;
        let collision = loaded.collision;
        let mut hero = loaded.hero;
        if let Some((x, z)) = start {
            let Some(y) = collision.ground_height(x, z, 3.0) else {
                bail!("no ground at ({x}, {z}) on map {key}");
            };
            hero = Hero::new(Vec3::new(x, y + 0.1, z));
            hero.yaw = field::yaw_of_angle(0.0);
        }
        let mut walker = Self {
            rom,
            time,
            story: story.clone(),
            entry: loaded.entry,
            collision,
            hero,
            field: loaded.field,
            state: field::initial_state(story),
            changes: Vec::new(),
            chain: 0,
        };
        let events = walker.field.enter(&mut walker.state, false);
        walker.apply(events)?;
        Ok(walker)
    }

    fn log_position(&self, why: &str) {
        let p = self.hero.position;
        println!(
            "{why}: map {} {} hero ({:.3}, {:.3}, {:.3}) yaw {:.3}",
            self.entry.id, self.entry.code, p.x, p.y, p.z, self.hero.yaw
        );
    }

    /// One fixed step walking in `direction`. Returns whether the map changed.
    pub fn step(&mut self, direction: Vec3) -> Result<bool> {
        self.hero.step(&self.collision, direction, STEP);
        let leader = Leader {
            position: self.hero.position,
            yaw: self.hero.yaw,
            input: direction.normalize_or_zero(),
        };
        let events = self.field.step(&leader, &mut self.state);
        self.apply(events)
    }

    fn apply(&mut self, events: Vec<Event>) -> Result<bool> {
        let mut changed = false;
        for event in events {
            match event {
                Event::MapChange(change) if !changed => changed = self.change(&change)?,
                Event::MapChange(change) => println!("  map change ignored: {change:?}"),
                Event::Request(request) => {
                    println!("  trigger request not supported before J3: {request:?}");
                }
                Event::Unsupported(code) => println!("  trigger condition {code:#x} not modelled"),
            }
        }
        Ok(changed)
    }

    fn change(&mut self, change: &MapChange) -> Result<bool> {
        if self.chain >= 4 {
            bail!("map change chain cut at {change:?}");
        }
        let from = self.entry.id;
        let p = self.hero.position;
        println!(
            "map change {from} -> {}: {:?} with the hero at ({:.3}, {:.3}, {:.3}), arrival {:?} angle {:?}",
            change.destination, change.cause, p.x, p.y, p.z, change.position, change.angle
        );
        let spawn = change
            .position
            .map(|p| (p, change.angle.map_or(0.0, field::yaw_of_angle)));
        let loaded = load::load(
            self.rom,
            &change.destination.to_string(),
            self.time,
            &self.story,
            spawn,
        )?;
        self.entry = loaded.entry;
        self.collision = loaded.collision;
        self.hero = loaded.hero;
        self.field = loaded.field;
        self.changes.push((from, change.destination));
        self.log_position("arrived");
        self.chain += 1;
        let events = self.field.enter(&mut self.state, true);
        let result = self.apply(events);
        self.chain -= 1;
        result?;
        Ok(true)
    }

    /// Runs a script, logging the position at the start of each part and at the end.
    pub fn run(&mut self, script: &[Walk]) -> Result<()> {
        self.log_position("start");
        for (i, part) in script.iter().enumerate() {
            let frames = (part.seconds / STEP).round() as u32;
            println!("step {i}: {:?} for {frames} frames", part.direction);
            for _ in 0..frames {
                self.step(part.direction)?;
            }
            self.log_position(&format!("after step {i}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_parse_signed_axes_and_idle() {
        let script = parse_script("+x:1.5, -z:3,idle:0.25").unwrap();
        assert_eq!(
            script,
            [
                Walk {
                    direction: Vec3::X,
                    seconds: 1.5
                },
                Walk {
                    direction: Vec3::NEG_Z,
                    seconds: 3.0
                },
                Walk {
                    direction: Vec3::ZERO,
                    seconds: 0.25
                },
            ]
        );
        let diagonal = parse_script("+x-z:1").unwrap();
        assert_eq!(diagonal[0].direction, Vec3::new(1.0, 0.0, -1.0));
        assert!(parse_script("+x+x:1").is_err());
        assert!(parse_script("up:1").is_err());
        assert!(parse_script("+x").is_err());
        assert!(parse_script("+x:-1").is_err());
        assert!(parse_script("+x:nan").is_err());
    }
}
