//! Headless playback of events (native only): the loop shared by `--screenshot --event` and
//! the coverage report `--events-report` (`docs/app.md`, § Événements).

use std::collections::HashSet;
use std::ops::RangeInclusive;

use anyhow::{Context as _, Result};
use dq9_core::Hero;
use dq9_core::script::Sound;
use dq9_core::script::load::stage_events;
use dq9_core::trigger::State;
use dq9_formats::cast::Story;
use dq9_render::Camera;
use nds_rom::Rom;

use crate::event::{EventPlayer, Outcome, Stage};
use crate::load::{self, LoadedMap};
use crate::talk::{Button, Talk};

/// Steps after which an event is declared stuck.
const LIMIT: u32 = 400_000;

/// How a playback went.
#[derive(Debug, Default)]
pub struct Played {
    pub steps: u32,
    /// The events ran to their end.
    pub finished: bool,
    /// An event of the chain is on another map (the playback stops there).
    pub elsewhere: Option<u16>,
}

/// Plays the queued events of `player` on `loaded`'s map with the fixed steps of the window,
/// for `steps` steps, or until they are over. The action key is pressed at the steps of
/// `press` and every `press_every` steps, which reads the messages.
#[allow(clippy::too_many_arguments)]
pub fn play(
    rom: &[u8],
    loaded: &mut LoadedMap,
    hero: &mut Hero,
    camera: &mut Camera,
    talk: &mut Talk,
    player: &mut EventPlayer,
    state: &State,
    steps: Option<u32>,
    press: &[u32],
    press_every: Option<u32>,
) -> Result<Played> {
    let mut played = Played::default();
    for frame in 0..steps.unwrap_or(LIMIT) {
        if press.contains(&frame) || press_every.is_some_and(|n| frame > 0 && frame % n == 0) {
            talk.press(Button::Confirm);
        }
        let mut stage = Stage {
            rom,
            world: state,
            scene: &mut loaded.runtime,
            hero,
            collision: &loaded.collision,
            camera,
            talk,
        };
        if let Some(next) = player.pending_start() {
            if crate::event::place(rom, next).is_some_and(|p| p.map != loaded.entry.id) {
                played.elsewhere = Some(next);
                break;
            }
            player.prepare(rom, next, &mut stage);
        }
        for outcome in player.step(&mut stage) {
            // Nothing is heard here: a jingle is over as soon as it is asked for.
            if matches!(outcome, Outcome::Sound(Sound::Jingle(_))) {
                player.jingle_ended();
            }
        }
        talk.step();
        loaded.runtime.step(camera)?;
        played.steps = frame + 1;
        if steps.is_none() && !player.is_active() {
            played.finished = true;
            break;
        }
    }
    Ok(played)
}

/// What the report does not count against an event (`docs/re/stb-natives.md`, § « Sans effet
/// dans le rendu »): 559 and 815 (a flag nothing reads, an integrity probe), 560 (a subsystem
/// test), 571 (a bit of `Object3D::flags_` nothing reads); 581 and 582 (which NPCs the shadows
/// are drawn for) and 809 (shows two objects of the hero's angel form), whose targets the
/// renderer does not draw **[hypothèse]**; and the motions a model has no name for (a no-op in
/// the game). A script that changes a collision placement (574, 575) **is** counted: the walls
/// and the floor change even though no image shows them.
const NO_VISIBLE_EFFECT: [&str; 8] = [
    "native 559",
    "native 560",
    "native 571",
    "native 581",
    "native 582",
    "native 809",
    "native 815",
    crate::event::ABSENT_MOTION,
];

/// How much of an event is played, from what the player could not execute.
fn verdict(played: &Played, started: &[u16], missing: &[(String, u32)]) -> &'static str {
    if started.is_empty() || missing.iter().any(|(k, _)| k.starts_with("event ")) {
        return "not played";
    }
    if !played.finished {
        return "stopped";
    }
    // The screen swap is a real layout effect: the field and the message window change screen
    // (`POWCNT1` bit 15). It stays reported until the layout applies it.
    let partial = missing
        .iter()
        .any(|(k, _)| !NO_VISIBLE_EFFECT.contains(&k.as_str()));
    if partial { "partial" } else { "complete" }
}

/// Plays every scene of the stages `major.minor` for the `minors` and prints what ran and
/// what did not.
pub fn report(rom: &[u8], major: u32, minors: RangeInclusive<u32>, story: &Story) -> Result<()> {
    let files = Rom::parse(rom).context("parsing the ROM")?;
    let events = stage_events(&files, major, minors);
    let mut done: HashSet<u16> = HashSet::new();
    println!("event(s) | map | steps | result | not executed (count)");
    for (event, name) in events {
        if done.contains(&event) {
            continue;
        }
        let Some(place) = crate::event::place(rom, event) else {
            println!("{event} ({name}) | ? | 0 | not played | no event-list record");
            continue;
        };
        let state = dq9_core::field::initial_state(story);
        let spawn = place.position.map(|(p, angle)| {
            (
                glam::Vec3::from_array(p),
                dq9_core::field::yaw_of_angle(angle),
            )
        });
        let loaded = load::load(
            rom,
            &place.map.to_string(),
            0,
            story,
            spawn,
            None,
            &dq9_formats::hero::HeroLook::new_game(),
        );
        let mut loaded = match loaded {
            Ok(l) => l,
            Err(e) => {
                println!(
                    "{event} ({name}) | {} | 0 | not played | map: {e:#}",
                    place.map
                );
                continue;
            }
        };
        let mut hero = loaded.hero;
        let mut camera = Camera::default();
        if let Some(bounds) = loaded.runtime.scene.bounds() {
            camera.frame(bounds);
        }
        let mut talk = Talk::default();
        talk.set_rom(rom);
        let mut player = EventPlayer::new();
        player.queue(event);
        let mut played = Played::default();
        let mut maps = vec![loaded.entry.code.clone()];
        loop {
            let part = play(
                rom,
                &mut loaded,
                &mut hero,
                &mut camera,
                &mut talk,
                &mut player,
                &state,
                None,
                &[],
                Some(100),
            )?;
            played.steps += part.steps;
            played.finished = part.finished;
            played.elsewhere = part.elsewhere;
            // A chained event on another map: the game loads the map, the report does too.
            let Some(next) = part.elsewhere else { break };
            let Some(there) = crate::event::place(rom, next) else {
                break;
            };
            let spawn = there.position.map(|(p, angle)| {
                (
                    glam::Vec3::from_array(p),
                    dq9_core::field::yaw_of_angle(angle),
                )
            });
            match load::load(
                rom,
                &there.map.to_string(),
                0,
                story,
                spawn,
                None,
                &dq9_formats::hero::HeroLook::new_game(),
            ) {
                Ok(l) => {
                    loaded = l;
                    hero = loaded.hero;
                    maps.push(loaded.entry.code.clone());
                    played.elsewhere = None;
                    // The new zone has its own placements: the changes of the previous map
                    // are gone with it (natives 574, 575 last until the zone loads again).
                    player.map_loaded(&camera);
                }
                Err(_) => break,
            }
        }
        let started = player.started().to_vec();
        done.extend(started.iter().copied());
        let missing: Vec<(String, u32)> = player
            .coverage()
            .missing
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let chain = started
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join("+");
        let verdict = verdict(&played, &started, &missing);
        let list = missing
            .iter()
            .map(|(k, n)| format!("{k} ×{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let elsewhere = played
            .elsewhere
            .map(|e| format!(" (chained event {e} is on another map)"))
            .unwrap_or_default();
        println!(
            "{chain} ({name}) | {} | {} | {verdict}{elsewhere} | {list}",
            maps.join("+"),
            played.steps
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native 574 shows a map placement the chosen time of day hid: its instance exists from
    /// the start (null scale, home transform), the scene brings it on screen, and the coverage
    /// no longer reports "hidden placement shown or moved" (the old build dropped it).
    #[test]
    fn a_scene_shows_a_placement_hidden_at_the_chosen_time() {
        let Some(rom) = testrom::load() else { return };
        let story = Story::default();
        let place = crate::event::place(rom, 1330).unwrap();
        let spawn = place.position.map(|(p, angle)| {
            (
                glam::Vec3::from_array(p),
                dq9_core::field::yaw_of_angle(angle),
            )
        });
        let mut loaded = load::load(
            rom,
            &place.map.to_string(),
            0,
            &story,
            spawn,
            None,
            &dq9_formats::hero::HeroLook::new_game(),
        )
        .unwrap();
        let hidden: Vec<(usize, glam::Mat4)> = loaded
            .runtime
            .scene
            .instances
            .iter()
            .enumerate()
            .filter(|(_, i)| i.block != usize::MAX && !i.drawn())
            .map(|(index, i)| (index, i.home))
            .collect();
        assert!(
            !hidden.is_empty(),
            "{} at time 0 keeps no placement hidden",
            place.map
        );
        let mut hero = loaded.hero;
        let mut camera = Camera::default();
        let mut talk = Talk::default();
        talk.set_rom(rom);
        let mut player = EventPlayer::new();
        player.queue(1330);
        let state = dq9_core::field::initial_state(&story);
        let mut shown = false;
        for frame in 0..400_000 {
            if frame > 0 && frame % 100 == 0 {
                talk.press(Button::Confirm);
            }
            let mut stage = Stage {
                rom,
                world: &state,
                scene: &mut loaded.runtime,
                hero: &mut hero,
                collision: &loaded.collision,
                camera: &mut camera,
                talk: &mut talk,
            };
            if let Some(next) = player.pending_start() {
                player.prepare(rom, next, &mut stage);
            }
            for outcome in player.step(&mut stage) {
                // Nothing is heard here: a jingle is over as soon as it is asked for.
                if matches!(outcome, Outcome::Sound(Sound::Jingle(_))) {
                    player.jingle_ended();
                }
            }
            talk.step();
            loaded.runtime.step(&camera).unwrap();
            for &(index, home) in &hidden {
                let instance = &loaded.runtime.scene.instances[index];
                assert_eq!(instance.home, home, "a show lost the home transform");
                shown |= instance.drawn();
            }
            if !player.is_active() {
                break;
            }
        }
        assert!(shown, "574 never showed a placement hidden at time 0");
        assert!(
            !player
                .coverage()
                .missing
                .keys()
                .any(|k| k == "hidden placement shown or moved"),
            "{:?}",
            player.coverage().missing
        );
    }

    /// The first scene of the observatory runs to its end on the real data: its messages are
    /// read with the action key, the camera is the script's and the hero is back to normal.
    #[test]
    fn a_short_scene_plays_to_its_end() {
        let Some(rom) = testrom::load() else { return };
        let story = Story::default();
        let place = crate::event::place(rom, 1130).unwrap();
        let spawn = place.position.map(|(p, angle)| {
            (
                glam::Vec3::from_array(p),
                dq9_core::field::yaw_of_angle(angle),
            )
        });
        let mut loaded = load::load(
            rom,
            &place.map.to_string(),
            0,
            &story,
            spawn,
            None,
            &dq9_formats::hero::HeroLook::new_game(),
        )
        .unwrap();
        let mut hero = loaded.hero;
        let mut camera = Camera::default();
        let mut before = camera;
        let mut talk = Talk::default();
        talk.set_rom(rom);
        let mut player = EventPlayer::new();
        player.queue(1130);
        let state = dq9_core::field::initial_state(&story);
        let played = play(
            rom,
            &mut loaded,
            &mut hero,
            &mut camera,
            &mut talk,
            &mut player,
            &state,
            None,
            &[],
            Some(100),
        )
        .unwrap();
        assert!(played.finished && played.elsewhere.is_none());
        assert_eq!(player.started(), [1130]);
        // The cutscene's own camera was used, and given back at the end.
        before.follow_hero(hero.position());
        assert_eq!(camera, before);
        assert!(!player.hero_hidden());
        // Nothing the script needed was missing: only the effects the notes list.
        assert!(
            player
                .coverage()
                .missing
                .keys()
                .all(|k| !k.starts_with("event "))
        );
    }
}
