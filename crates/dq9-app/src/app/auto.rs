//! `--headless --auto`: plays the story without a window, pressing the action key to read the
//! messages, until the player would get control (no event runs or waits) or the run is stuck,
//! and prints what the story did (`Journal`). With `--pilot`, a free control is not the end:
//! the autopilot has the conversation, or takes the exit, that moves the story on.

use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use dq9_core::Hero;
use dq9_core::field::{self, Event, Exit, Leader};
use dq9_core::scenario::Step;
use dq9_core::trigger::Request;

use super::{App, status};
use crate::talk::{Button, Partner};

/// Steps between two presses of the action key while a scene runs (a message needs its text
/// typed before the key advances it).
const PRESS_EVERY: u64 = 20;
/// Things the autopilot may do in one run.
const MAX_ACTIONS: u32 = 300;

/// Why [`App::play_auto`] stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// No event runs or waits and the autopilot, if any, has nothing left to try: the player
    /// has control on this map.
    Control { map: u16 },
    /// The step limit was reached with a scene still running.
    Limit,
    /// The game was never started (no ROM, or the first scene did not load).
    NotStarted,
}

/// What the autopilot remembers.
#[derive(Default)]
struct Pilot {
    /// Conversations had: `(room, npc, context, story digest)`; one is not had twice at the
    /// same point of the story.
    talked: BTreeSet<(u16, u16, u16, u64)>,
    /// Exits taken: `(room, destination, story digest)`.
    left: BTreeSet<(u16, u16, u64)>,
    /// The rooms the hero was in.
    visited: BTreeSet<u16>,
    /// The room and story digest the current map was built for.
    built: Option<(u16, u64)>,
    actions: u32,
}

impl App {
    /// A hash of where the story is: the progress of every track, their flags and the global
    /// flags.
    fn story_digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for track in &self.state.tracks {
            (track.chapter, track.step, track.substep).hash(&mut hasher);
            track.flags.hash(&mut hasher);
        }
        self.state.flags.hash(&mut hasher);
        hasher.finish()
    }

    /// Runs the game logic for at most `max_steps` fixed steps; stops at the first moment the
    /// player would get control after a scene (`Stop::Control`), or, with `pilot`, when the
    /// autopilot finds nothing that moves the story.
    pub fn play_auto(&mut self, max_steps: u64, pilot: bool) -> Stop {
        let mut started = false;
        let mut bot = Pilot::default();
        for _ in 0..max_steps {
            let busy = self.events.is_active() || self.events.pending_start().is_some();
            if (busy || self.talk.is_active()) && self.tick.is_multiple_of(PRESS_EVERY) {
                self.talk.press(Button::Confirm);
            }
            self.run_steps(1);
            let busy = self.events.is_active() || self.events.pending_start().is_some();
            started |= busy;
            if started && !busy && !self.talk.is_active() && !self.talk.confirm_pending() {
                let map = self.current.as_ref().map_or(0, |(e, _)| e.id);
                let track = &self.state.tracks[self.state.track];
                let story = (track.chapter, track.step, track.substep);
                let last = self.journal.steps.iter().rev().find_map(|(_, s)| match s {
                    Step::Control { map, story } => Some((*map, *story)),
                    _ => None,
                });
                // The pilot crosses rooms: one line per story point while it works, and one
                // for the place where it stops.
                if last.is_none_or(|(_, s)| s != story) {
                    self.journal.record(self.tick, Step::Control { map, story });
                }
                if pilot && self.pursue(&mut bot) {
                    continue;
                }
                if last.is_some_and(|(m, s)| s == story && m != map) {
                    self.journal.record(self.tick, Step::Control { map, story });
                }
                return Stop::Control { map };
            }
        }
        if started {
            Stop::Limit
        } else {
            Stop::NotStarted
        }
    }

    /// At a free control: has the conversation that moves the story most, else leaves by an exit
    /// not taken yet. Returns whether it started something.
    fn pursue(&mut self, bot: &mut Pilot) -> bool {
        let Some((entry, _)) = &self.current else {
            return false;
        };
        let room = entry.id;
        bot.visited.insert(room);
        let digest = self.story_digest();
        if bot.actions >= MAX_ACTIONS {
            return false;
        }
        // The cast and the triggers of the map follow the story: build it again when the story
        // moved since (the game reloads its triggers on progress, `Request::ReloadTriggers`).
        if bot.built != Some((room, digest)) {
            if let Some(hero) = &self.hero {
                let spawn = (hero.position(), hero.yaw());
                self.quiet_events = true;
                self.load_map(&room.to_string(), false, Some(spawn), None, false);
                self.quiet_events = false;
                // The same map again is not a step of the story.
                if matches!(self.journal.steps.last(), Some((_, Step::Map { id, .. })) if *id == room)
                {
                    self.journal.steps.pop();
                }
            }
            bot.built = Some((room, digest));
            if self.events.is_active() || self.events.pending_start().is_some() {
                return true;
            }
        }
        if let Some((partner, why)) = self.best_conversation(room, digest, bot) {
            bot.talked
                .insert((room, partner.npc, partner.context, digest));
            bot.actions += 1;
            let what = format!(
                "talks to NPC {} (context {}) on map {room}: {why}",
                partner.npc, partner.context
            );
            status(&what);
            self.journal.record(self.tick, Step::Autopilot(what));
            self.converse(&partner);
            return true;
        }
        let Some(field) = &self.field else {
            return false;
        };
        let exits = field.exits();
        // Unvisited rooms first, in the order of the map's exits.
        let next = exits
            .iter()
            .filter(|exit| {
                exit.destination
                    .is_some_and(|d| !bot.left.contains(&(room, d, digest)))
            })
            .min_by_key(|exit| exit.destination.is_some_and(|d| bot.visited.contains(&d)));
        if let Some(exit) = next
            && let Some(destination) = exit.destination
        {
            let exit = *exit;
            bot.left.insert((room, destination, digest));
            bot.actions += 1;
            let what = format!("leaves map {room} for {destination}");
            status(&what);
            self.journal.record(self.tick, Step::Autopilot(what));
            if !self.leave_by(&exit) {
                status("the exit did not lead anywhere");
            }
            return true;
        }
        false
    }

    /// The conversation of this map that moves the story most when had now (see
    /// `Talk::simulate`), with what it does.
    fn best_conversation(&self, room: u16, digest: u64, bot: &Pilot) -> Option<(Partner, String)> {
        let field = self.field.as_ref()?;
        let track = &self.state.tracks[self.state.track];
        let progress = (track.chapter, track.step, track.substep);
        let mut best: Option<(u32, Partner, String)> = None;
        for partner in self.talk.partners() {
            if bot
                .talked
                .contains(&(room, partner.npc, partner.context, digest))
            {
                continue;
            }
            let (events, after) = self.talk.simulate(&partner, field, &self.state);
            let after_track = &after.tracks[after.track];
            let mut score = 0;
            let mut what = Vec::new();
            if (after_track.chapter, after_track.step, after_track.substep) != progress {
                score += 4;
                what.push(format!(
                    "story {}.{}.{}",
                    after_track.chapter, after_track.step, after_track.substep
                ));
            }
            for event in &events {
                match event {
                    Event::Request(Request::Event(id)) => {
                        score += 3;
                        what.push(format!("event {id}"));
                    }
                    Event::Request(Request::Battle(n)) => {
                        score += 3;
                        what.push(format!("battle {n}"));
                    }
                    Event::MapChange(change) => {
                        score += 2;
                        what.push(format!("map {}", change.destination));
                    }
                    _ => {}
                }
            }
            if after.flags != self.state.flags || after_track.flags != track.flags {
                score += 1;
                what.push("flags".to_owned());
            }
            if score > best.as_ref().map_or(0, |(s, _, _)| *s) {
                best = Some((score, partner, what.join(", ")));
            }
        }
        best.map(|(_, partner, what)| (partner, what))
    }

    /// Stands the hero where the partner search finds the NPC and presses the action key.
    fn converse(&mut self, partner: &Partner) {
        self.hero = Some(Hero::at(partner.position, partner.yaw));
        self.camera.target = partner.position;
        self.talk.press(Button::Confirm);
    }

    /// Puts the hero in the exit and has him walk into it, as the field task sees it. Returns
    /// whether the map changed.
    fn leave_by(&mut self, exit: &Exit) -> bool {
        let yaw = if exit.heading == glam::Vec3::ZERO {
            0.0
        } else {
            field::yaw_of_angle(exit.heading.x.atan2(exit.heading.z))
        };
        self.hero = Some(Hero::at(exit.position, yaw));
        for _ in 0..8 {
            let leader = Leader {
                position: exit.position,
                yaw,
                input: exit.heading,
            };
            let Some(field) = &mut self.field else {
                return false;
            };
            let events = field.step(&leader, &mut self.state);
            if self.apply_events(events) {
                return true;
            }
        }
        false
    }

    /// The report of a run: the journal, and where and why it stopped.
    pub fn auto_report(&self, stop: &Stop) -> String {
        let mut out = String::new();
        out.push_str(&self.journal.to_string());
        let events = self.journal.events();
        out.push_str(&format!("events played: {events:?}\n"));
        match stop {
            Stop::Control { map } => out.push_str(&format!(
                "stopped: free control on map {map} after {:.1} s of game time\n",
                self.tick as f64 / 60.0
            )),
            Stop::Limit => out.push_str("stopped: step limit reached while a scene was running\n"),
            Stop::NotStarted => out.push_str("stopped: no scene started\n"),
        }
        for (i, t) in self.state.tracks.iter().enumerate() {
            out.push_str(&format!(
                "track {i}: {}.{}.{} flags {:?}\n",
                t.chapter, t.step, t.substep, t.flags
            ));
        }
        out.push_str(&format!("global flags: {:?}\n", self.state.flags));
        if let Some(hero) = &self.hero {
            out.push_str(&format!("hero at {:?}\n", hero.position()));
        }
        out
    }
}
