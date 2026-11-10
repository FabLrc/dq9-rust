//! The route mode of the pilot (`--route <event>`): what a player does that the story does not
//! do for him. The story pilot (`auto.rs`) follows what the scenario offers; a fight it cannot
//! win stops it. Here the roaming monsters of the fields are on, and the party
//!
//! * after a lost fight, scripted or not, does nothing else for the story until it is stronger
//!   than it was when it lost, and meanwhile fights the roamers of the field,
//! * heals when a member is low: at the inn when the story opens one, else with the medicinal
//!   herbs it buys at the village shop and uses from the field menu,
//! * then goes back where it fell and tries again.
//!
//! Nothing is injected: levels and gold come from fights, healing from the inn's price or the
//! shop's, and the screens are the application's own, driven by presses. Nothing is hand-set:
//! the pilot knows no level the Hexagoon asks for; it knows only that it lost at a given
//! strength and that it must be stronger to try again. The route ends when the event it was
//! given ends. Without the flag none of this runs and the headless traces keep replaying
//! (roaming stays off).

use std::collections::BTreeSet;

use dq9_core::encounter::roamer::state as roamer_state;
use dq9_core::scenario::Step;
use dq9_core::talk::Target;
use dq9_core::trigger::track_of_room;
use dq9_formats::dialogue::{self, Session, TalkState};
use dq9_formats::{Gpc2, Table};
use glam::Vec3;
use nds_rom::Rom;

use super::{Goal, Pilot, flat, hop_towards};
use crate::app::{App, status};
use crate::talk::{Button, ServiceTag, service_tag};

/// Chapter letters of the dialogue archives (`Scenario_ChapterLetter`, `ov_017::0218d2c4`).
const CHAPTER_LETTERS: &[u8] = b"ABCDEFGHIJSTKLMNOPQ";
/// Roamer fights after which the grind is called off: the route has not got stronger in a
/// reasonable number of them.
const GRIND_CAP: u32 = 4_000;
/// A member is low when its HP is under this many fifths of its maximum.
const LOW_FIFTHS: u32 = 3;
/// Frames a hunt is kept before the nearest roamer is looked for again.
pub(super) const HUNT_FRAMES: u32 = 90;
/// How far from the party a roaming goal may lie, in units.
const ROAM_NEAR: f32 = 12.0;
const ROAM_FAR: f32 = 40.0;
/// Presses at a facility screen before the pilot leaves it.
const SERVICE_PRESSES: u32 = 60;
/// Talks to a keeper that may open no screen before the party gives up on it.
const MAX_VISIT_TRIES: u32 = 4;
/// Looks for something to fight that may find nothing (each waits half a second) before the
/// route stops.
const MAX_IDLE: u32 = 200;
/// Frames between two looks for a roamer during a roaming walk.
const ROAM_LOOK: u32 = 45;
/// The medicinal herb (`docs/re/facility.md`: the first of the village shop's 18 items).
const MEDICINAL_HERB: u16 = 22000;
/// How many herbs a shopping trip buys, as far as the purse goes.
const HERB_STOCK: u32 = 6;

/// How strong the party is, for comparing it with itself: the levels of its members and what
/// the roamers' step reads of its equipment (`App::roaming_party_strength`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Strength {
    levels: u32,
    strength: u32,
    deftness: u32,
}

impl Strength {
    /// Whether the party is stronger than `before`: a level more, or better equipment.
    fn exceeds(&self, before: &Self) -> bool {
        self.levels > before.levels
            || self.strength > before.strength
            || self.deftness > before.deftness
    }
}

/// A facility the pilot goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Facility {
    Inn,
    Shop,
}

impl Facility {
    fn opened_by(self, tag: ServiceTag) -> bool {
        match self {
            Self::Inn => matches!(tag, ServiceTag::Inn(_)),
            Self::Shop => matches!(tag, ServiceTag::Shop(_)),
        }
    }
}

/// What the pilot wants of the facility screen that opens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Intent {
    #[default]
    None,
    /// Stay at the inn: Confirm picks the first row, Stay.
    Rest,
    /// Buy herbs: Confirm buys the item under the cursor, which starts on the first row.
    Shop,
}

/// Where a purchase is: pressing to buy, or pressing to dismiss what the shop answered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Purchase {
    #[default]
    Buy,
    Dismiss,
    Leave,
}

/// The counters of the fights the route fought.
#[derive(Debug, Default, Clone, Copy)]
struct Fights {
    won: u32,
    lost: u32,
    fled: u32,
    experience: u32,
    gold: u32,
}

/// What the route mode keeps while it plays.
#[derive(Default)]
pub(super) struct Route {
    /// The event whose end stops the run.
    pub(super) until: u16,
    /// Set when that event ended.
    pub(super) reached: bool,
    /// The party's strength when it last lost a fight, until it is stronger than that.
    blocked: Option<Strength>,
    /// The map where the story was interrupted, where it goes on once the party is stronger.
    resume: Option<u16>,
    intent: Intent,
    purchase: Purchase,
    presses: u32,
    /// The gold the party had before the last press at a facility.
    gold_before: u32,
    /// The herbs in the bag when the pilot went to the shop.
    visit_herbs: u32,
    /// The gold the inn and the shop turned the party away at: no new try before it has more.
    inn_poor_at: Option<u32>,
    shop_poor_at: Option<u32>,
    /// Roaming contacts counted so far, to see a new fight.
    contacts: u32,
    /// Level shown last, to report level-ups.
    level: u8,
    /// Frames the current hunt or roam has lasted.
    pub(super) age: u32,
    /// The state of the walk generator (`--seed` and nothing else).
    rng: u64,
    /// Points a walk could not reach, in half units.
    bad: BTreeSet<(i32, i32)>,
    /// The room `bad` belongs to.
    bad_room: u16,
    /// Looks for something to fight, one after the other, that found nothing.
    idle: u32,
    /// Talks to a keeper that did not open a screen, one after the other.
    tries: u32,
    /// Herbs the field menu would not take, one after the other.
    herb_fails: u32,
    fights: Fights,
    rests: u32,
    herbs_bought: u32,
    herbs_used: u32,
    /// Set battles lost, with the level and the rounds: what the real fight needed.
    attempts: Vec<String>,
}

impl Route {
    pub(super) fn new(until: u16, seed: u64) -> Self {
        Self {
            until,
            rng: seed ^ 0x9e37_79b9_7f4a_7c15,
            ..Self::default()
        }
    }

    /// The generator of the roaming goals: a 64-bit LCG, so a run depends on the seed only.
    fn next(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.rng >> 33
    }

    /// Remembers a point a walk could not reach.
    pub(super) fn avoid(&mut self, point: Vec3) {
        self.bad.insert(half_units(point));
    }

    /// Whether the party lost and is not stronger yet: the story's offers wait.
    pub(super) fn waiting(&self) -> bool {
        self.blocked.is_some()
    }

    /// Tests: where the story resumes once the party is stronger.
    #[cfg(test)]
    pub(super) fn resume_room(&self) -> Option<u16> {
        self.resume
    }

    /// Tests: the party lost at `levels` levels in all and must be stronger to go on.
    #[cfg(test)]
    pub(super) fn lost_at(&mut self, levels: u32) {
        self.blocked = Some(Strength {
            levels,
            strength: u32::MAX,
            deftness: u32::MAX,
        });
    }

    /// Whether the pilot has fought more roamers than the grind allows.
    pub(super) fn capped(&self) -> bool {
        self.fights.won + self.fights.lost + self.fights.fled >= GRIND_CAP
    }

    fn poor_at(&mut self, kind: Facility) -> &mut Option<u32> {
        match kind {
            Facility::Inn => &mut self.inn_poor_at,
            Facility::Shop => &mut self.shop_poor_at,
        }
    }
}

fn half_units(point: Vec3) -> (i32, i32) {
    (
        (point.x * 2.0).round() as i32,
        (point.z * 2.0).round() as i32,
    )
}

/// A roamer state worth walking to: not one that is leaving, fighting or beaten.
fn huntable(state: u8) -> bool {
    !matches!(
        state,
        roamer_state::FLEE
            | roamer_state::VANISH
            | roamer_state::TOUCHED
            | roamer_state::FOUGHT
            | roamer_state::DEFEATED
    )
}

impl App {
    /// The party's strength now.
    fn route_strength(&mut self) -> Strength {
        let levels = self
            .party()
            .members
            .iter()
            .map(|member| u32::from(member.level()))
            .sum();
        let (strength, deftness) = self.roaming_party_strength();
        Strength {
            levels,
            strength,
            deftness,
        }
    }

    /// Whether a living member is low.
    fn route_low(&self) -> bool {
        self.party()
            .members
            .iter()
            .any(|m| m.hp > 0 && u32::from(m.hp) * 5 < u32::from(m.max_hp) * LOW_FIFTHS)
    }

    /// The code of map `id`.
    fn map_code(&self, id: u16) -> Option<&str> {
        self.maps
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.code.as_str())
    }

    /// Whether map `id` is a field (`F..`: the maps whose scenario file is the shared `F`,
    /// `dq9_core::field::trigger_path`), where monsters roam in the open.
    fn route_is_field(&self, id: u16) -> bool {
        self.map_code(id).is_some_and(|code| code.starts_with('F'))
    }

    /// What one step of the route mode saw: the journal steps since `from` (the scripted
    /// fights, a wipe-out, the end of the event the route runs to) and the roamer fight the
    /// step settled. A fight lost, scripted or not, sets the bar the party must get over
    /// before the story goes on, and the party comes round on the revival map; once it is
    /// stronger it goes back to where the story was interrupted.
    pub(super) fn route_observe(&mut self, bot: &mut Pilot, from: usize) {
        let Some(route) = bot.mode.as_mut() else {
            return;
        };
        let steps: Vec<Step> = self.journal.steps[from..]
            .iter()
            .map(|(_, step)| step.clone())
            .collect();
        let (mut scripted, mut scripted_lost, mut wiped_in) = (false, false, None);
        for step in steps {
            match step {
                Step::Battle {
                    number,
                    won,
                    fought,
                    ..
                } => {
                    // The level the party had when it went in: `route.level` is the last one
                    // seen, before this step's level-ups. A win shows what it left; a loss ends
                    // in the wipe-out that gives the HP back.
                    let level = route.level;
                    let left = self
                        .party()
                        .members
                        .first()
                        .map(|m| format!(", HP {}/{} left", m.hp, m.max_hp))
                        .unwrap_or_default();
                    let line = match &fought {
                        Some(f) => format!(
                            "set battle {number} {} at Lv {level}: {} rounds, {} experience, {} gold{}",
                            if won { "won" } else { "lost" },
                            f.rounds,
                            f.experience,
                            f.gold,
                            if won { left.as_str() } else { "" }
                        ),
                        None => format!("set battle {number} {}", if won { "won" } else { "lost" }),
                    };
                    route.attempts.push(line.clone());
                    self.journal
                        .record(self.tick, Step::Autopilot(format!("route: {line}")));
                    scripted = true;
                    scripted_lost |= !won;
                }
                Step::EventEnded(event) if event == route.until => route.reached = true,
                Step::MapChange {
                    from: Some(room),
                    cause,
                    ..
                } if cause == "wipe-out" => wiped_in = Some(room),
                _ => {}
            }
        }
        // A roamer's contact settles its fight inside the step that made it. A fight lost on
        // the spot ends in a wipe-out, whose map load starts the contact count again.
        let contacts = self.roaming_contacts();
        let touched = contacts > route.contacts;
        route.contacts = contacts;
        if touched || (wiped_in.is_some() && !scripted) {
            // The party may be hurt: choose again instead of walking on.
            if matches!(bot.goal, Some(Goal::Hunt { .. } | Goal::Roam { .. })) {
                bot.goal = None;
            }
            if let Some(fight) = self.last_fight().filter(|f| f.number.is_none()) {
                match fight.result {
                    dq9_core::battle::BattleResult::Won => route.fights.won += 1,
                    dq9_core::battle::BattleResult::Fled => route.fights.fled += 1,
                    dq9_core::battle::BattleResult::Lost => route.fights.lost += 1,
                }
                route.fights.experience += fight.experience;
                route.fights.gold += fight.gold;
            }
        }
        // Where the story goes on: the room of a fall outside a field, else the first fall.
        if let Some(room) = wiped_in {
            if route.resume.is_none() || !self.route_is_field_room(room) {
                route.resume = Some(room);
            }
        } else if scripted_lost && route.resume.is_none() {
            route.resume = bot.room;
        }
        if scripted_lost || wiped_in.is_some() {
            let strength = self.route_strength();
            if let Some(route) = bot.mode.as_mut() {
                route.blocked = Some(strength);
            }
            let what = format!(
                "route: lost at Lv {} (strength {}/{}); the story waits until the party is stronger",
                strength.levels, strength.strength, strength.deftness
            );
            self.journal.record(self.tick, Step::Autopilot(what));
        }
        let level = self.party().members.first().map_or(0, |m| m.level());
        if let Some(route) = bot.mode.as_mut()
            && route.level != level
        {
            route.level = level;
            let member = &self.party().members[0];
            let what = format!(
                "route: Lv {level}, experience {}, HP {}/{}, gold {}, {} fights ({} won, {} lost, {} fled)",
                member.current_experience(),
                member.hp,
                member.max_hp,
                self.party().gold,
                route.fights.won + route.fights.lost + route.fights.fled,
                route.fights.won,
                route.fights.lost,
                route.fights.fled
            );
            self.journal.record(self.tick, Step::Autopilot(what));
        }
    }

    /// [`App::route_is_field`] for the room of a fall.
    fn route_is_field_room(&self, room: u16) -> bool {
        self.route_is_field(room)
    }

    /// The closing line of a route run: levels, gold and the fights it took.
    pub(super) fn route_summary(&mut self, bot: &Pilot) {
        let Some(route) = bot.mode.as_ref() else {
            return;
        };
        let member = &self.party().members[0];
        let mut lines = vec![format!(
            "route: Lv {} experience {} HP {}/{} gold {}; roamer fights {} won, {} lost, {} fled \
             ({} experience, {} gold); {} stays at the inn, {} herbs bought, {} used",
            member.level(),
            member.current_experience(),
            member.hp,
            member.max_hp,
            self.party().gold,
            route.fights.won,
            route.fights.lost,
            route.fights.fled,
            route.fights.experience,
            route.fights.gold,
            route.rests,
            route.herbs_bought,
            route.herbs_used
        )];
        lines.extend(route.attempts.iter().map(|a| format!("route: {a}")));
        for line in lines {
            self.journal.record(self.tick, Step::Autopilot(line));
        }
    }

    /// The first of `contexts` for which NPC `npc` of `room` offers `kind` at this point of the
    /// story: the line the game selects (`dialogue::select` on the room's track step and
    /// substep, the time of day and how many times the NPC was talked to, as `Talk::start`)
    /// opens that facility (`<INN=n>`, `<SHOP=n>`). Ivor of Angel Falls only chats from 2.1 to
    /// 2.6: his inn lines start at step 7 (`dq9re talk data/scenario/M01B0.gp2 007_en.bin
    /// --step 4`), and the shop keeper's (NPC 19) are there all along.
    pub(super) fn route_offers(
        &self,
        kind: Facility,
        room: u16,
        npc: u16,
        contexts: impl IntoIterator<Item = i32>,
    ) -> Option<i32> {
        let code = self.map_code(room)?;
        let (Some(family), Some(bytes)) = (code.get(..3), self.rom.as_deref()) else {
            return None;
        };
        let track = &self.state.tracks[track_of_room(i32::from(room))];
        let letter = CHAPTER_LETTERS.get(usize::from(track.chapter).wrapping_sub(1))?;
        let rom = Rom::parse(bytes).ok()?;
        let file = rom
            .file_by_path(&format!(
                "data/scenario/{family}{}0.gp2",
                char::from(*letter)
            ))
            .ok()?;
        let archive = Gpc2::parse(file).ok()?;
        let entry = archive.find(&format!("{npc:03}_en.bin"))?;
        let data = archive.member(entry).ok()?;
        let table = Table::parse(&data).ok()?;
        let lines = dialogue::parse(&table).ok()?;
        let quests = |quest: i32| self.state.quests.get(&quest).copied().unwrap_or(0);
        contexts.into_iter().find(|&context| {
            let talk = TalkState {
                step: i32::from(track.step),
                substep: i32::from(track.substep),
                context,
                talk_count: self.state.talk_counts.get(npc),
                daytime: self.state.time_of_day != 0,
                session: Session {
                    hero_alive: true,
                    ..Session::default()
                },
                quest: &quests,
            };
            dialogue::select(&lines, &talk)
                .and_then(|line| lines[line].text)
                .and_then(service_tag)
                .is_some_and(|tag| kind.opened_by(tag))
        })
    }

    /// The room and the NPC of a facility at this point of the story: of the rooms the pilot
    /// knows, one whose cast (`dq9_formats::cast::load`, as the map loads it for the story
    /// point and the time of day) holds an NPC who offers it.
    pub(super) fn route_facility(&self, bot: &Pilot, kind: Facility) -> Option<(u16, u16)> {
        let mut rooms: BTreeSet<u16> = bot.links.keys().copied().collect();
        rooms.extend(bot.links.values().flatten().copied());
        let rom = Rom::parse(self.rom.as_deref()?).ok()?;
        for room in rooms {
            let Some(code) = self.map_code(room) else {
                continue;
            };
            let story = crate::load::story_from_state(&self.options.story, &self.state, Some(room));
            let Some(bytes) =
                dq9_formats::map::rom_file(&rom, &dq9_formats::cast::archive_path(code.as_bytes()))
            else {
                continue;
            };
            let Ok(cast) = dq9_formats::cast::load(bytes, room, &story) else {
                continue;
            };
            if let Some(npc) = cast
                .npcs
                .iter()
                .map(|npc| npc.member.id)
                .find(|npc| self.route_offers(kind, room, *npc, 0..=255).is_some())
            {
                return Some((room, npc));
            }
        }
        None
    }

    /// The pilot is in `room`: what the route mode keeps per room starts again.
    pub(super) fn route_enter(&self, bot: &mut Pilot, room: u16) {
        if let Some(route) = bot.mode.as_mut()
            && route.bad_room != room
        {
            // The points a walk failed to reach belong to the room they were chosen in.
            route.bad.clear();
            route.bad_room = room;
        }
    }

    /// The route mode's turn at a free control, before the story's own choices: heal when a
    /// member is low, else fight for strength while the story waits. `true`: a walk or a
    /// wait was started.
    pub(super) fn route_pursue(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
    ) -> bool {
        self.route_heal(bot, room, digest, from) || self.route_grind(bot, room, digest, from)
    }

    /// Whether the story waits: the party lost and is not stronger yet. When it is, the wait
    /// ends, and the pilot is told where to go back to.
    pub(super) fn route_waits(&mut self, bot: &mut Pilot) -> bool {
        let Some(before) = bot.mode.as_ref().and_then(|r| r.blocked) else {
            return false;
        };
        let now = self.route_strength();
        if !now.exceeds(&before) {
            return true;
        }
        if let Some(route) = bot.mode.as_mut() {
            route.blocked = None;
        }
        // The conversation that led to the lost fight is worth another try: the budgets of the
        // story's talks start again.
        bot.talks.clear();
        bot.talked.clear();
        let what = format!(
            "route: stronger (strength {}/{}/{} against {}/{}/{}), the story goes on",
            now.levels, now.strength, now.deftness, before.levels, before.strength, before.deftness
        );
        self.journal.record(self.tick, Step::Autopilot(what));
        false
    }

    /// Walks the way to room `target` through the rooms the pilot has seen: the first step of
    /// it. `false` when the pilot is there, or does not know the way. A floor can have two
    /// stairs up and one way on (D01M02): when no exit of this room leads on, the entrance the
    /// pilot came in by is booked as a dead end for this trip and it goes back out by it, so the
    /// next try takes the other one.
    pub(super) fn route_travel(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
        target: u16,
    ) -> bool {
        if room == target {
            return false;
        }
        let Some(hop) = hop_towards(&bot.links, room, |next| next == target) else {
            return false;
        };
        if self.walk_back(bot, room, digest, from, hop) {
            return true;
        }
        let Some((before, entered, key)) = bot.arrived_by.take() else {
            return false;
        };
        if entered != room || before == hop {
            return false;
        }
        bot.dead_ends.insert((before, entered, key, digest));
        let what = format!("route: no way on from map {room} to {hop}: back out to {before}");
        status(&what);
        self.journal.record(self.tick, Step::Autopilot(what));
        self.walk_back(bot, room, digest, from, before)
    }

    /// Heals when a member is low: at the inn when the story has one open, else a medicinal herb
    /// from the bag (instantly: the field menu holds the field while it is open), else a trip
    /// to the shop for herbs. `true` when a walk was started.
    pub(super) fn route_heal(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
    ) -> bool {
        if !self.route_low() {
            return false;
        }
        let gold = self.party().gold;
        let poor = |bot: &mut Pilot, kind| {
            bot.mode
                .as_mut()
                .is_some_and(|route| *route.poor_at(kind) == Some(gold))
        };
        if gold > 0
            && !poor(bot, Facility::Inn)
            && let Some((inn, keeper)) = self.route_facility(bot, Facility::Inn)
        {
            return self.route_visit(bot, room, digest, from, Facility::Inn, inn, keeper);
        }
        if self.route_use_herb(bot) {
            return false;
        }
        if self.party().bag.count(MEDICINAL_HERB) == 0
            && gold > 0
            && !poor(bot, Facility::Shop)
            && let Some((shop, keeper)) = self.route_facility(bot, Facility::Shop)
        {
            return self.route_visit(bot, room, digest, from, Facility::Shop, shop, keeper);
        }
        false
    }

    /// Uses a medicinal herb from the bag on the member who is lowest, through the field menu
    /// (Items, the herb's row, the target): `true` when one was spent.
    fn route_use_herb(&mut self, bot: &mut Pilot) -> bool {
        if bot.mode.as_ref().is_some_and(|route| route.herb_fails >= 3) {
            return false;
        }
        let party = self.party();
        let herbs = party.bag.count(MEDICINAL_HERB);
        let Some(row) = party
            .bag
            .slots()
            .iter()
            .filter_map(|stack| stack.item)
            .position(|item| item == MEDICINAL_HERB)
        else {
            return false;
        };
        let Some((target, _)) = party
            .members
            .iter()
            .enumerate()
            .filter(|(_, m)| m.hp > 0)
            .min_by_key(|(_, m)| u32::from(m.hp) * 100 / u32::from(m.max_hp.max(1)))
        else {
            return false;
        };
        let before: Vec<u16> = party.members.iter().map(|m| m.hp).collect();
        if !self.open_field_menu() {
            return false;
        }
        // Menu → Items (the cursor starts on it) → the herb's row → its target list.
        self.service_button(Button::Confirm);
        for _ in 0..row {
            self.service_button(Button::Down);
        }
        self.service_button(Button::Confirm);
        for _ in 0..target {
            self.service_button(Button::Down);
        }
        self.service_button(Button::Confirm);
        self.services.close();
        let spent = self.party().bag.count(MEDICINAL_HERB) < herbs;
        let Some(route) = bot.mode.as_mut() else {
            return spent;
        };
        if spent {
            route.herb_fails = 0;
            route.herbs_used += 1;
            let after: Vec<u16> = self.party().members.iter().map(|m| m.hp).collect();
            let what = format!(
                "route: uses a medicinal herb: HP {before:?} -> {after:?}, {} left",
                self.party().bag.count(MEDICINAL_HERB)
            );
            status(&what);
            self.journal.record(self.tick, Step::Autopilot(what));
        } else {
            route.herb_fails += 1;
            status("route: the field menu took no herb");
        }
        spent
    }

    /// Walks to the keeper of `kind` in `target_room` and talks to him: the screen he opens is
    /// the service driver's. `true` when a walk was started.
    #[allow(clippy::too_many_arguments)]
    fn route_visit(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
        kind: Facility,
        target_room: u16,
        keeper: u16,
    ) -> bool {
        if room != target_room {
            return self.route_travel(bot, room, digest, from, target_room);
        }
        // The keeper's partners (one a talk zone, one the NPC alone): the game's talk trigger
        // gives the real context, so any spot where the partner search picks him will do.
        let partners: Vec<_> = self
            .talk
            .partners()
            .into_iter()
            .filter(|p| p.npc == keeper)
            .collect();
        if partners.is_empty() {
            return false;
        }
        let gold = self.party().gold;
        let herbs_in_bag = self.party().bag.count(MEDICINAL_HERB);
        // The keeper must answer: a conversation that opens no screen is tried a few times.
        let tries = bot.mode.as_mut().map_or(0, |r| {
            r.tries += 1;
            r.tries
        });
        if tries > MAX_VISIT_TRIES {
            if let Some(r) = bot.mode.as_mut() {
                *r.poor_at(kind) = Some(gold);
                r.tries = 0;
            }
            status(&format!(
                "route: the keeper of the {kind:?} opens no screen"
            ));
            return false;
        }
        let Some((partner, position, route)) = partners.into_iter().find_map(|partner| {
            self.talk_spot(bot, room, from, &partner)
                .map(|(position, route)| (partner, position, route))
        }) else {
            return false;
        };
        if let Some(r) = bot.mode.as_mut() {
            r.intent = match kind {
                Facility::Inn => Intent::Rest,
                Facility::Shop => Intent::Shop,
            };
            r.purchase = Purchase::Buy;
            r.presses = 0;
            r.gold_before = gold;
            r.visit_herbs = herbs_in_bag;
        }
        let what = format!(
            "route: talks to the keeper of the {kind:?} (NPC {keeper}) of map {room}, HP {:?}, gold {gold}",
            self.party()
                .members
                .iter()
                .map(|m| (m.hp, m.max_hp))
                .collect::<Vec<_>>(),
        );
        status(&what);
        self.journal.record(self.tick, Step::Autopilot(what));
        let target = Target {
            npc: partner.npc,
            context: partner.context,
        };
        self.start_goal(
            bot,
            room,
            digest,
            route,
            Goal::Talk {
                target,
                position,
                yaw: partner.yaw,
            },
        );
        true
    }

    /// Presses the facility screen the pilot opened, one press every few frames. At the inn,
    /// Confirm picks Stay and what follows it. At the shop, Confirm buys the first row, the
    /// medicinal herb, and again dismisses the answer, until the stock is there or the purse is
    /// empty; then Cancel leaves. Any other screen is left. Bounded, so a refusal ends.
    pub(super) fn route_service(&mut self, bot: &mut Pilot) {
        let Some(route) = bot.mode.as_mut() else {
            return;
        };
        route.tries = 0;
        if !self.tick.is_multiple_of(3) {
            return;
        }
        route.presses += 1;
        let gold = self.party().gold;
        let herbs = self.party().bag.count(MEDICINAL_HERB);
        let button = match route.intent {
            Intent::Rest if route.presses <= SERVICE_PRESSES => Button::Confirm,
            Intent::Shop if route.presses <= SERVICE_PRESSES => match route.purchase {
                Purchase::Buy => {
                    route.gold_before = gold;
                    route.purchase = Purchase::Dismiss;
                    Button::Confirm
                }
                Purchase::Dismiss => {
                    // What the shop answered is read from the purse: a purchase takes gold.
                    let bought = gold < route.gold_before;
                    route.purchase = if bought && herbs < HERB_STOCK {
                        Purchase::Buy
                    } else {
                        Purchase::Leave
                    };
                    if !bought {
                        route.shop_poor_at = Some(gold);
                    }
                    Button::Confirm
                }
                Purchase::Leave => Button::Cancel,
            },
            _ => Button::Cancel,
        };
        self.service_button(button);
        if self.services.is_open() {
            return;
        }
        let gold_now = self.party().gold;
        let herbs_now = self.party().bag.count(MEDICINAL_HERB);
        let Some(route) = bot.mode.as_mut() else {
            return;
        };
        match route.intent {
            Intent::Rest => {
                if gold_now >= route.gold_before {
                    route.inn_poor_at = Some(gold_now);
                    status("route: the inn turned the party away");
                } else {
                    route.rests += 1;
                }
            }
            Intent::Shop => {
                route.herbs_bought += herbs_now.saturating_sub(route.visit_herbs);
            }
            Intent::None => {}
        }
        route.intent = Intent::None;
    }

    /// Grinds while the story waits: in a field, walks to the nearest roamer (or somewhere the
    /// field's monsters will find the party); elsewhere, walks to the nearest known field.
    pub(super) fn route_grind(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        from: Vec3,
    ) -> bool {
        if !self.route_waits(bot) {
            return self.route_resume(bot, room, digest, from);
        }
        if self.route_is_field(room) {
            if self.route_hunt(bot, room, digest, from) {
                if let Some(route) = bot.mode.as_mut() {
                    route.idle = 0;
                }
                return true;
            }
            // Nothing to walk to yet: wait a moment for the field to show a monster, and give
            // up when it never does.
            let idle = bot.mode.as_mut().map_or(u32::MAX, |route| {
                route.idle += 1;
                route.idle
            });
            if idle > MAX_IDLE {
                return false;
            }
            bot.wait = 30;
            return true;
        }
        let Some(hop) = hop_towards(&bot.links, room, |next| self.route_is_field(next)) else {
            return false;
        };
        self.walk_back(bot, room, digest, from, hop)
    }

    /// Goes back to where the story was interrupted once the party is stronger. There, what the
    /// pilot did in that room is forgotten, so its exits and talks are tried again.
    fn route_resume(&mut self, bot: &mut Pilot, room: u16, digest: u64, from: Vec3) -> bool {
        let Some(target) = bot.mode.as_ref().and_then(|r| r.resume) else {
            return false;
        };
        if room == target {
            if let Some(route) = bot.mode.as_mut() {
                route.resume = None;
            }
            bot.left.retain(|(r, _, _)| *r != room);
            bot.tried_exits.retain(|(r, _, _, _)| *r != room);
            bot.evaluated.remove(&room);
            bot.entered.retain(|(r, _, _)| *r != room);
            return false;
        }
        if self.route_travel(bot, room, digest, from, target) {
            return true;
        }
        // No known way: the story's own exploration finds it.
        if let Some(route) = bot.mode.as_mut() {
            route.resume = None;
        }
        false
    }

    /// The nearest roamer worth a fight that a route reaches, else a roaming goal.
    fn route_hunt(&mut self, bot: &mut Pilot, room: u16, digest: u64, from: Vec3) -> bool {
        let avoid = self.warp_avoid(bot, room, from, None);
        let mut roamers: Vec<(f32, Vec3)> = self
            .roaming_roamers()
            .into_iter()
            .filter(|(_, roamer)| huntable(roamer.state))
            .map(|(_, roamer)| {
                let at = Vec3::new(
                    roamer.position[0] as f32,
                    roamer.position[1] as f32,
                    roamer.position[2] as f32,
                ) / 4096.0;
                (flat(at - from).length(), at)
            })
            .collect();
        roamers.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, at) in roamers.into_iter().take(4) {
            let Some(point) = bot
                .grids
                .get(&room)
                .and_then(|grid| grid.nearest_node(at, 2.0).map(|node| grid.center(node)))
            else {
                continue;
            };
            if bot
                .mode
                .as_ref()
                .is_some_and(|r| r.bad.contains(&half_units(point)))
            {
                continue;
            }
            if let Some(route) = self.route(bot, room, from, point, &avoid) {
                self.route_walk(bot, room, digest, route, Goal::Hunt { point });
                return true;
            }
        }
        // Nobody in sight: somewhere the field's monsters spawn near the party.
        let Some(grid) = bot.grids.get(&room) else {
            return false;
        };
        let blocked = super::blocked_nodes(bot, room, &avoid);
        let mut spots: Vec<Vec3> = grid
            .reachable(from, &blocked)
            .into_iter()
            .filter(|at| {
                let d = flat(*at - from).length();
                (ROAM_NEAR..=ROAM_FAR).contains(&d)
            })
            .filter(|at| {
                !bot.mode
                    .as_ref()
                    .is_some_and(|r| r.bad.contains(&half_units(*at)))
            })
            .collect();
        if spots.is_empty() {
            return false;
        }
        spots.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
        let pick = bot.mode.as_mut().map_or(0, Route::next) as usize % spots.len();
        let point = spots[pick];
        if let Some(route) = self.route(bot, room, from, point, &avoid) {
            self.route_walk(bot, room, digest, route, Goal::Roam { point });
            return true;
        }
        if let Some(route) = bot.mode.as_mut() {
            route.avoid(point);
        }
        false
    }

    fn route_walk(
        &mut self,
        bot: &mut Pilot,
        room: u16,
        digest: u64,
        route: Vec<Vec3>,
        goal: Goal,
    ) {
        if let Some(r) = bot.mode.as_mut() {
            r.age = 0;
        }
        self.start_goal(bot, room, digest, route, goal);
    }

    /// Whether a hunt or a roaming walk still stands: a hunt lasts a few seconds, then the
    /// nearest roamer is looked for again; a roaming walk gives way when a roamer shows up.
    pub(super) fn route_keeps(&self, bot: &mut Pilot, goal: Goal) -> bool {
        let Some(route) = bot.mode.as_mut() else {
            return false;
        };
        route.age += 1;
        match goal {
            Goal::Hunt { .. } => route.age <= HUNT_FRAMES,
            Goal::Roam { .. } => {
                !route.age.is_multiple_of(ROAM_LOOK)
                    || !self
                        .roaming_roamers()
                        .iter()
                        .any(|(_, roamer)| huntable(roamer.state))
            }
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_party_is_stronger_by_a_level_or_by_its_gear_only() {
        let before = Strength {
            levels: 3,
            strength: 20,
            deftness: 10,
        };
        assert!(!before.exceeds(&before));
        assert!(
            Strength {
                levels: 4,
                ..before
            }
            .exceeds(&before)
        );
        assert!(
            Strength {
                strength: 21,
                ..before
            }
            .exceeds(&before)
        );
        assert!(
            !Strength {
                levels: 2,
                strength: 19,
                deftness: 9
            }
            .exceeds(&before)
        );
    }

    #[test]
    fn the_roaming_walks_depend_on_the_seed_alone() {
        let draw = |seed| {
            let mut route = Route::new(2750, seed);
            (0..5).map(|_| route.next()).collect::<Vec<_>>()
        };
        assert_eq!(draw(7), draw(7));
        assert_ne!(draw(7), draw(8));
    }
}
