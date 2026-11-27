//! The story's flow in the application: a new game, and what chains the scenes that the
//! event task and the field do not already chain (scripted battles). The mechanisms are the
//! game's (`dq9_core::scenario`, `docs/re/trigger.md`, § Scripted battles).

use dq9_core::scenario;
use dq9_core::state::GameState;
use dq9_core::trigger::Member;

#[cfg(not(target_arch = "wasm32"))]
use super::character::Creation;
use super::{App, status};
use crate::lighting;
use crate::talk::HeroIdentity;
use dq9_formats::hero::Sex;

/// The hero the creation screen chose, as the new-game path applies it: the save's glyph
/// indices, the look the scene assembles and the sex (`HeroIdentity`).
#[derive(Clone)]
struct CreatedHero {
    name: [u8; dq9_formats::save::NAME_LEN],
    look: dq9_formats::hero::HeroLook,
    sex: Sex,
}

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
        self.start_new_game_with(None);
    }

    /// [`App::start_new_game`] with the hero the creation screen chose: the look, the party
    /// record and the message identity are in place **before** the opening map builds the
    /// hero's rig, so the first frame of the prologue already shows the created character.
    fn start_new_game_with(&mut self, created: Option<&CreatedHero>) {
        // A battle preview gives way to the new game.
        self.encounter.battle = None;
        self.combat.fight = None;
        self.encounter.battle_change = None;
        self.encounter.saved_field = None;
        self.encounter.battle_cover = 0.0;
        self.encounter.last_battle = None;
        self.options.battle = None;
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let game = GameState::new_game();
        self.party = game.party.clone();
        self.party_ready = false;
        self.combat.world_seeded = false;
        self.combat.revival_map = super::fight::NEW_GAME_REVIVAL_MAP;
        self.combat.fight = None;
        self.combat.battle_pad = Default::default();
        self.combat.last_fight = None;
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
        let look = created.map_or_else(dq9_formats::hero::HeroLook::new_game, |h| h.look.clone());
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
        self.look = look.clone();
        self.reset_session();
        // The created hero reaches the party record (the save write's source, `+0x140` and
        // `+0x174` bit 0) and the message identity after the reset, and before `load_map`
        // builds the rig from `self.look`.
        if let Some(created) = created {
            let name = created.name;
            if let Some(hero) = self.party.members.first_mut() {
                hero.name = name;
                hero.sex = created.sex;
                hero.look = look;
            }
            self.talk.set_identity(Some(HeroIdentity {
                name,
                male: created.sex == Sex::Man,
            }));
        }
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
        // The page's pilot session (`?pilot=1`) takes the new game from its first scene.
        self.start_session();
    }

    /// A new game from the creation screen: the new game with the created hero in the three
    /// places that consume him — the message identity (`HeroIdentity`, decoded from the save's
    /// glyph indices), the look the scene assembles (`HeroLook::for_sex`), and the party
    /// record the save write serialises (`Character::name` at `+0x140` and the `+0x174` bit 0,
    /// through `GameState::write_into`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn start_created_game(&mut self, creation: Creation) {
        let look = creation.look();
        let sex = creation.sex();
        let created = CreatedHero {
            name: creation.saved_name(),
            look: look.clone(),
            sex,
        };
        self.start_new_game_with(Some(&created));
        status(&format!(
            "new game: the created hero \"{}\" ({:?}), look face {} hair {}",
            String::from_utf8_lossy(&self.talk.hero_name()),
            sex,
            look.face_item,
            look.hair_item
        ));
    }

    /// The wake-up after a wipe-out (`func_ov017_0219bfb4`, mode 2): `chur_messet.bin` gives
    /// the revival map's voice; voice 3 says nothing, any other runs the church in mode 2,
    /// whose line is 1082 of `str_ch<voice - 1>`, before the party gets control. The purse
    /// was halved by `func_02010604`; **no line says so** — the ROM has no gold line here.
    pub(super) fn wipe_out_arrival(&mut self) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };
        let table = nds_rom::Rom::parse(rom)
            .map_err(|e| format!("{e:#}"))
            .and_then(|image| {
                image
                    .file_by_path("data/scenario/chur_messet.bin")
                    .map_err(|e| format!("{e:#}"))
            })
            .and_then(|data| {
                dq9_core::facility::church::VoiceTable::parse(data).map_err(|e| format!("{e:#}"))
            });
        let table = match table {
            Ok(table) => table,
            Err(e) => {
                status(&format!("wipe-out: chur_messet.bin: {e}"));
                return;
            }
        };
        let chapter = u32::from(self.state.tracks[self.state.track].chapter);
        let voice = table.voice(
            self.combat.revival_map,
            chapter,
            self.day.phase().is_daytime(),
        );
        if voice == dq9_core::facility::church::WIPE_OUT_SILENT {
            status(&format!(
                "wipe-out: map {}'s priest is silent at story major {chapter}",
                self.combat.revival_map
            ));
            return;
        }
        if let Err(e) = self.services.open_priest(rom, voice) {
            status(&format!("wipe-out: the priest's line: {e:#}"));
        }
    }

    /// Action 0x78: a scripted battle. The scene is shown through the transition (native
    /// 547's black screens and the record's track), then the fight is played on it
    /// (`app/fight.rs`; a headless run without `--fight` keeps the J4 stand-in,
    /// [`scenario::stand_in`]) and the end of the battle fires trigger event 15 or 16, which
    /// starts the scene that follows it.
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
                surprise: None,
            },
            true,
            false,
        );
    }
}
