use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};

use dq9_formats::cast::{Npc, TalkZone};
use glam::Vec3;

use crate::field::{angle_of_yaw, to_fx};

// --- Who the hero talks to ------------------------------------------------------------------

/// Half-side of the square around an NPC where the hero can talk to it (`fx32`, 1.5).
pub const RANGE: i32 = 0x1800;
/// Distance under which a free (not in a talk zone) partner is accepted (`fx32`, 1.75).
pub const REACH: i32 = 0x1c00;
/// Added to the range and reach of an NPC with a record-20 offset (`+0x1000`, `+0x1400`).
const EXTRA_RANGE: i32 = 0x1000;
const EXTRA_REACH: i32 = 0x1400;

/// What the partner search needs of an NPC of the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Talkable {
    /// NPC id (`<map>npc.bin` field 0, the dialogue file number).
    pub id: u16,
    /// Cast kind: 0 sprite, 1 thing to examine (talked to only from a zone), 2 model…
    pub kind: u8,
    /// Position, `fx32`.
    pub position: [i32; 3],
    pub zones: Vec<TalkZone>,
    /// Record 20 of the placement script: `[dx, 0, dz, extra range]` (`fx32`), the offset of
    /// the talk centre and the extra range **[unknown: exact use]**.
    pub extra: Option<[i32; 4]>,
}

impl Talkable {
    pub fn from_cast(npc: &Npc<'_>) -> Self {
        Self {
            id: npc.member.id,
            kind: npc.member.kind,
            position: npc.placement.position,
            zones: npc.placement.zones.clone(),
            extra: npc.placement.extra,
        }
    }
}

/// A conversation partner: the NPC and the context the talk asks with (id of the talk zone
/// holding the hero, 0 outside any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub npc: u16,
    pub context: u16,
}

/// Talk zone test of `Talk_NearestNpc`: strictly inside the rectangle on x and z.
fn zone_holds(zone: &TalkZone, x: i32, z: i32) -> bool {
    let [x_max, z_max, x_min, z_min] = zone.rect;
    x < x_max && z < z_max && x > x_min && z > z_min
}

/// The first talk zone of `npc` that holds the hero.
fn zone_of(npc: &Talkable, hero: [i32; 3]) -> Option<&TalkZone> {
    npc.zones.iter().find(|z| zone_holds(z, hero[0], hero[2]))
}

/// For every NPC, whether the hero stands in one of its talk zones: the `leader_in_zone_of`
/// condition of the triggers (`crate::trigger::State::zones`).
pub fn zones_containing(npcs: &[Talkable], hero: Vec3) -> BTreeMap<u32, bool> {
    let hero = to_fx(hero);
    npcs.iter()
        .map(|n| (u32::from(n.id), zone_of(n, hero).is_some()))
        .collect()
}

/// Absolute angle between the hero's facing and the direction to a point (0 to π), like
/// `ov_017::021a4700` followed by `fix32abs`.
fn facing_error(hero: Vec3, yaw: f32, to: [i32; 3]) -> f32 {
    let (dx, dz) = (
        to[0] as f32 / 4096.0 - hero.x,
        to[2] as f32 / 4096.0 - hero.z,
    );
    let bearing = dx.atan2(dz);
    let d = (bearing - angle_of_yaw(yaw)).rem_euclid(TAU);
    if d > PI { TAU - d } else { d }
}

/// The partner the action key picks (`Talk_NearestNpc(1)`, `ov_017::021a4e88`).
///
/// An NPC is a candidate when a talk zone holds the hero, or when the hero is within 1.5
/// units on x and z of it (offset and enlarged by its record-20 values) and, for anything
/// but a thing to examine, nearer than 1.75 on the ground plane **[deviation: the game measures
/// the distance in 3D, here the NPC heights are those of the placements, not readjusted]**. Of the
/// candidates the one the hero faces most directly wins; a zone partner is accepted whatever
/// its angle, a free one only when strictly better than the best free one so far. The
/// object flags and the "hero busy" tests are not modelled.
pub fn nearest(npcs: &[Talkable], hero: Vec3, yaw: f32) -> Option<Target> {
    let hero_fx = to_fx(hero);
    let mut best = PI;
    let mut best_free = PI;
    let mut chosen = None;
    for npc in npcs {
        let zone = zone_of(npc, hero_fx);
        let (centre, range, reach) = match npc.extra {
            Some([dx, _, dz, extra]) => (
                [npc.position[0] + dx, npc.position[1], npc.position[2] + dz],
                extra + EXTRA_RANGE,
                extra + EXTRA_REACH,
            ),
            None => (npc.position, RANGE, REACH),
        };
        let near =
            (hero_fx[0] - centre[0]).abs() <= range && (hero_fx[2] - centre[2]).abs() <= range;
        if zone.is_none() && !near {
            continue;
        }
        let angle = facing_error(hero, yaw, npc.position);
        if angle > best {
            continue;
        }
        if let Some(zone) = zone {
            best = angle;
            chosen = Some(Target {
                npc: npc.id,
                context: zone.label as u16,
            });
        } else if npc.kind != 1 {
            let dx = (hero_fx[0] - centre[0]) as f32 / 4096.0;
            let dz = (hero_fx[2] - centre[2]) as f32 / 4096.0;
            let distance = dx.hypot(dz);
            if distance < reach as f32 / 4096.0 && angle < best_free {
                best = angle;
                best_free = angle;
                chosen = Some(Target {
                    npc: npc.id,
                    context: 0,
                });
            }
        }
    }
    chosen
}
