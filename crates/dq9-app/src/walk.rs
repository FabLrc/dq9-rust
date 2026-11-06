//! Headless walk: the field of the application without a window, driven by a script of
//! inputs, logging the maps and positions it passes through (`dq9-app --walk`, `docs/app.md`).
//!
//! It runs the same fixed steps as the window: `Hero::step` (the pad keys the direction presses) on the map's collision, then
//! `Field::step` with the leader's pose, then the map changes the field returns, loading the
//! destination at its arrival pose and running its load and arrival triggers.

use anyhow::{Result, bail};
use dq9_core::Hero;
use dq9_core::field;
use glam::Vec3;

use crate::app::STEP;

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

/// The hero at the ground of `(x, z)`, facing +z (`--start`).
pub fn start_hero(
    collision: &dq9_core::CollisionWorld,
    key: &str,
    (x, z): (f32, f32),
) -> Result<Hero> {
    let Some(y) = collision.ground_height(x, z, 3.0) else {
        bail!("no ground at ({x}, {z}) on map {key}");
    };
    Ok(Hero::at(Vec3::new(x, y, z), field::yaw_of_angle(0.0)))
}

/// The direction the script walks at fixed step `frame` (`Vec3::ZERO` once it has ended), with
/// the parts lasting `(seconds / STEP)` rounded frames as [`Walker::run`] plays them.
pub fn direction_at(script: &[Walk], frame: u32) -> Vec3 {
    let mut start = 0;
    for part in script {
        let end = start + (part.seconds / STEP).round() as u32;
        if frame < end {
            return part.direction;
        }
        start = end;
    }
    Vec3::ZERO
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
