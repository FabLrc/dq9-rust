//! The story's flow in the application: a new game, and what chains the scenes that the
//! event task and the field do not already chain (scripted battles). The mechanisms are the
//! game's (`dq9_core::scenario`, `docs/re/trigger.md`, § Scripted battles).

use dq9_core::scenario;
use dq9_core::state::GameState;
use dq9_core::trigger::Member;

use super::{App, status};
use crate::lighting;

/// Map the original loads after character creation and before the first scene: `S14M05`
/// (id 6405, "Starflight Express / For Events"). Its load raises the S14 "map visited" flag
/// 2774; the prologue's own map (`M01M12`, 1112) follows ~100 frames later. Evidence: the
/// oracle RAM at frame 3762 holds map 6405 / area 6401, 2774 appears at frame 3764, and map
/// 1112 at frame 3864.
const OPENING_MAP: u16 = 6405;

impl App {
    /// Action 0x76 outside a conversation (an area, a doorway, the end of a scene): opens the
    /// window on the line of `npc` for `context`.
    pub(super) fn talk_to(&mut self, npc: u16, context: u16) {
        let (Some(rom), Some((entry, _))) = (&self.rom, &self.current) else {
            return;
        };
        if let Err(e) = self
            .talk
            .start(rom, &entry.code, &mut self.state, npc, context)
        {
            status(&format!("talk: {e:#}"));
        }
    }

    /// The destination track supplies the cast's story after a new game or a save load.
    pub(super) fn current_story(&self, key: &str) -> dq9_formats::cast::Story {
        if self.options.new_game || (self.options.save.is_some() && self.options.map.is_none()) {
            let id = key.parse::<u16>().ok();
            let room = self.maps.iter().find(|entry| {
                id.map_or_else(|| entry.code.eq_ignore_ascii_case(key), |id| entry.id == id)
            });
            crate::load::story_from_state(
                &self.options.story,
                &self.state,
                room.map(|entry| entry.id),
            )
        } else {
            self.options.story.clone()
        }
    }

    /// A new game: the state of `GameState::new_game` (scenario 1.1.1, the day's start, the
    /// default hero), then the first scene of story stage 1.1 (`stage_opening`, `ev21500`),
    /// which loads its own map; what follows is chained by the triggers.
    pub(super) fn start_new_game(&mut self) {
        // A battle preview gives way to the new game.
        self.battle = None;
        self.battle_change = None;
        self.saved_field = None;
        self.battle_cover = 0.0;
        self.last_battle = None;
        self.options.battle = None;
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let game = GameState::new_game();
        let scenario = game.scenario;
        let story = &mut self.options.story;
        (story.chapter, story.step, story.substep) = (
            scenario.chapter as u8,
            scenario.step as u8,
            scenario.substep as u8,
        );
        story.daytime = game.clock.phase().is_daytime();
        let opening = nds_rom::Rom::parse(rom).ok().and_then(|rom| {
            dq9_core::script::load::stage_opening(&rom, scenario.chapter, scenario.step)
        });
        let Some(opening) = opening else {
            status(&format!(
                "no opening scene at story stage {}.{}",
                scenario.chapter, scenario.step
            ));
            return;
        };
        self.state = dq9_core::field::initial_state(&self.options.story);
        self.state.compendium = self.item_table.clone();
        self.day = lighting::day_clock(Some(game.clock), self.options.time, self.options.clock);
        self.time = self.day.phase().index();
        // The hero as the conditions read him: alive, level 1 Guardian, in the opening outfit
        // (0x32/0x52 compare the worn items; the slots of the items are a **[hypothesis]**).
        let look = dq9_formats::hero::HeroLook::new_game();
        let mut hero = Member {
            hp: 1,
            levels: [1; dq9_core::trigger::VOCATIONS],
            ..Member::default()
        };
        for (slot, item) in [look.armour, look.legwear, look.footwear]
            .into_iter()
            .enumerate()
        {
            hero.slots[slot] = item.unwrap_or(0);
        }
        self.state.characters.insert(0, hero);
        self.state.party = vec![hero];
        // Character creation is not replayed. The game hands the hero the copper sword and
        // the celestial outfit when it confirms the character (`ov_009` table `0x0218636c`);
        // each item it equips raises its compendium flag (`func_0207cc18`, `0x0207cc28`).
        // The four flags appear together at frame 3740 of the oracle run.
        for item in [
            Some(dq9_core::state::NEW_GAME_WEAPON),
            look.armour,
            look.legwear,
            look.footwear,
        ]
        .into_iter()
        .flatten()
        {
            self.state.obtain(i32::from(item), 1);
        }
        self.look = dq9_formats::hero::HeroLook::new_game();
        self.reset_session();
        self.journal = Default::default();
        self.tick = 0;
        self.journal.note_progress(0, &self.state);
        // The original leaves character creation for the `S14M05` event map (RAM frame 3762:
        // map 6405, area 6401), whose load raises 2774 (`Flags_SetMapVisited`), then loads the
        // first scene's map (`M01M12`, 1112, frame 3863). The scene played on the train map is
        // not modelled, but the map load and its flag are.
        if !self.load_map(&OPENING_MAP.to_string(), true, None, None, false) {
            status(&format!("opening map {OPENING_MAP} did not load"));
        }
        status(&format!(
            "new game: story {}.{}.{}, first scene {opening}",
            scenario.chapter, scenario.step, scenario.substep
        ));
        self.events.queue(opening);
    }

    /// Action 0x78: a scripted battle. The scene is shown through the transition (native
    /// 547's black screens and the record's track), then the J4 stand-in settles it as won
    /// ([`scenario::stand_in`]) and the end of the battle fires trigger event 15, which starts
    /// the scene that follows it.
    pub(super) fn run_battle(&mut self, number: u16) {
        let record = self.rom.as_deref().and_then(|rom| {
            let rom = nds_rom::Rom::parse(rom).ok()?;
            scenario::battle(&rom, number)
        });
        if record.is_none() {
            status(&format!(
                "scripted battle {number}: no eventbattle.bin record"
            ));
        }
        // The record's stage and monsters; a record that names no stage falls back to the
        // ground's (30116, the field).
        self.start_battle(
            crate::battle::Request {
                record: Some(number),
                stage: 0,
                monsters: Vec::new(),
                track: None,
            },
            true,
            false,
        );
    }
}
