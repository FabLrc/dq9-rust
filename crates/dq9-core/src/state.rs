//! Game state, initialised from a DS save or from the new-game defaults. No I/O.
//!
//! Only what a save restores and the field needs first is modelled: current map, where the
//! party stands, scenario position, day clock and event flags. Where each value comes from is
//! in `docs/re/save.md`.
//!
//! port: 0x02010250, 0x02010298, 0x02010364, 0x0201036c, 0x02010374

use core::ops::Range;
use std::collections::BTreeSet;

use dq9_formats::hero::{HeroLook, Sex};
use dq9_formats::map::StartPoint;
use dq9_formats::save::{
    EVENT_FLAGS_SIZE, EventFlags, EventFlagsMut, GLOBAL_FLAGS_SIZE, NO_ITEM, QUEST_COUNT, Save,
    Slot, SlotMut, Warp,
};

use crate::party::{HELD_ITEMS, PARTY_MAX, Party};

/// Length of the in-game day in seconds (`GameState+0x3d0`, 420.0 in RAM).
pub const DAY_LENGTH: f32 = 420.0;
/// Start of each phase in seconds (`0x020f33bc`, `+0xc`, `+0x10` in RAM: 390, 210, 180).
const EVENING_START: f32 = 390.0;
const DAY_START: f32 = 210.0;
const MORNING_START: f32 = 180.0;
/// Seconds of day per tick (`GameState+0x3d4`, `0x3c888889` in RAM: 1/60).
const SECONDS_PER_TICK: f32 = f32::from_bits(0x3c88_8889);

/// Map of the prologue, the default of a new game (id 1100 = `M01`, Angel Falls) **[hypothesis]**:
/// the application already starts there without a save, but the game's own new-game map has
/// not been read.
pub const NEW_GAME_MAP: u16 = 1100;

/// Phase of the day (`GameState+0x3dc`, `TimeOfDay` of the USA decompilation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Night = 0,
    Morning = 1,
    Day = 2,
    Evening = 3,
}

impl TimeOfDay {
    /// Phase of a clock time already reduced to `[0, DAY_LENGTH)` (`0x02010298`).
    fn at(seconds: f32) -> Self {
        if seconds >= EVENING_START {
            Self::Evening
        } else if seconds >= DAY_START {
            Self::Day
        } else if seconds >= MORNING_START {
            Self::Morning
        } else {
            Self::Night
        }
    }

    /// Clock time at which the phase begins (`0x02010374`).
    pub fn start(self) -> f32 {
        match self {
            Self::Night => 0.0,
            Self::Morning => MORNING_START,
            Self::Day => DAY_START,
            Self::Evening => EVENING_START,
        }
    }

    /// Index as the game stores it and the lighting expects it (0 to 3).
    pub fn index(self) -> u32 {
        self as u32
    }

    /// `Save_IsDaytime` (`0x020103c4`) is true for everything but the night phase.
    pub fn is_daytime(self) -> bool {
        self != Self::Night
    }
}

/// The day clock: `GameState+0x3cc` (seconds), `+0x3d8` (running) and the derived phase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayClock {
    seconds: f32,
    running: bool,
    phase: TimeOfDay,
}

impl DayClock {
    /// `GameState::SetDayTimer` (`0x02010298`): reduces `seconds` modulo the day and derives the
    /// phase. A not-finite or negative time is not reachable from the game's own writers; it
    /// is kept at the start of the night here instead of reproducing the console's float
    /// behaviour **[unknown]**.
    pub fn new(seconds: f32, running: bool) -> Self {
        let mut clock = Self {
            seconds: 0.0,
            running,
            phase: TimeOfDay::Night,
        };
        clock.set(seconds);
        clock
    }

    fn set(&mut self, seconds: f32) {
        let mut t = seconds;
        if !t.is_finite() || t < 0.0 {
            t = 0.0;
        }
        if t >= DAY_LENGTH {
            t -= (t / DAY_LENGTH).trunc() * DAY_LENGTH;
        }
        if t >= DAY_LENGTH {
            t -= DAY_LENGTH;
        }
        self.seconds = t;
        self.phase = TimeOfDay::at(t);
    }

    /// Seconds since the start of the day, in `[0, DAY_LENGTH)`.
    pub fn seconds(&self) -> f32 {
        self.seconds
    }

    pub fn phase(&self) -> TimeOfDay {
        self.phase
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// `GameState::SetDayTimerRunning` (`0x02010364`).
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }

    /// `GameState::SetTimeOfDay` (`0x02010374`): the clock goes to the start of `phase`.
    pub fn set_phase(&mut self, phase: TimeOfDay) {
        self.set(phase.start());
    }

    /// `GameState::AdvanceDayTimer` (`0x02010250`): `ticks` frames (the game caps them at 3)
    /// while the clock runs.
    pub fn advance(&mut self, ticks: u32) {
        if self.running {
            self.set(self.seconds + ticks as f32 * SECONDS_PER_TICK);
        }
    }
}

/// Scenario position (`GameState+0x5cb0..+0x5cb8`, `Save_Chapter` and its neighbours).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scenario {
    pub chapter: u32,
    pub step: u32,
    pub substep: u32,
}

/// Where the party stands, as the game keeps it in an object: `fx32` world position
/// (`Object3D+0x44`) and heading (`+0x54`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeroPose {
    pub position: [i32; 3],
    /// `fx32` radians in `[0, 2π]`.
    pub angle: i16,
}

impl HeroPose {
    /// Position in game units.
    pub fn position_f32(&self) -> [f32; 3] {
        self.position.map(|p| p as f32 / 4096.0)
    }

    /// Heading in radians.
    pub fn angle_radians(&self) -> f32 {
        f32::from(self.angle) / 4096.0
    }
}

impl From<StartPoint> for HeroPose {
    fn from(start: StartPoint) -> Self {
        Self {
            position: start.position,
            angle: start.angle,
        }
    }
}

/// Base of the "map visited" flags, set on map load by `Flags_SetMapVisited` (`0x0201ba78`,
/// called from `0x02015448`): flag = base + map id / 100 for `99 < id <= 9999`.
const MAP_VISIT_FLAG_BASE: u16 = 0xa96;

/// Global flag the game sets when a map is entered (`Flags_SetMapVisited`, `0x0201ba78`).
///
/// Only the branch that the prologue uses is reproduced: a map id in `100..=9999` sets
/// `0xa96 + id / 100` (2710 + tens), which gives 2721 for `M01M12` (1112), 2751 for `X01`
/// (4100), 2729 for the map id 1949 and 2774 for the `S14` group (6400). The constant 1949
/// at `0x0201bb84` is a **flag** set by `func_0201b588`, not a map id. Map ids
/// 20000..=29999 and those outside `99..=9999` take branches of the original that are not
/// established (`docs/re/trigger.md`, § "Global flags set at the opening"); this
/// returns `None` for them rather than guessing.
pub fn map_visit_flag(map: u16) -> Option<u16> {
    (99 < map && map <= 9999).then(|| MAP_VISIT_FLAG_BASE + map / 100)
}

/// Base of the "item obtained" flags (`func_0207cc18`, `0x0207cc28`, called by `Items_Give`
/// and the equip paths): when the first copy of an item enters the party, the game sets
/// `0xc76 + compendium index` in `g_QuestManager+0x8c`. The index is a field of the item's
/// record (`itemdt_<category>.gp2`, bits 12..22 of `+0xc`, [`dq9_formats::item::ItemTable`]);
/// index 0 has no compendium entry and sets no flag.
const ITEM_OBTAINED_FLAG_BASE: u16 = 0xc76;

/// Flag of the item whose compendium index is `compendium` (`func_0207cc18`): `None` for
/// index 0, which the original skips.
pub fn item_obtained_flag(compendium: u16) -> Option<u16> {
    (compendium != 0)
        .then(|| ITEM_OBTAINED_FLAG_BASE.checked_add(compendium))
        .flatten()
}

/// Flag action `0x6b` sets when the session predicate holds (`Trig_RunAction` case 7,
/// `0x02061c14`: `Flags_Set(manager, manager+0x8c, *(u32*)0x02063008)`, and that word is
/// `0x113a`). The runtime has no wireless session: it is the host of a solo game.
pub const SESSION_HOST_FLAG: u16 = 0x113a;

/// Flag `KeyManager::RefreshKeys` sets on the first frame a button is held
/// (`0x020122ac`): `Flags_Set(flag manager, +0x8c, 0x113e, 1)` when `currentKeys != 0`. The
/// runtime has no brightness/lid gate, which would always pass here.
pub const KEY_REFRESH_FLAG: u16 = 0x113e;

/// The palette colour the winged form adds to the hero's models: 5/5/5 `(8, 6, 16)`, the
/// literal `0x40c8` that `ov_017::021bdb78` (at `0x021bdba4`) stores at `Object3D+0x9c` and
/// passes to `0218fcf8`, and the colour every scripted winged scene gives native 238
/// (`238(slot, 8, 6, 16)` in `ev21500`, `ev21507`, `ev21593`, `ev21595`). The operation is mode
/// 0 (add, clamped at 31) when `Object3D+0x9e` is 0, its value without a fifth argument of 238
/// **[hypothesis]**: nothing writes it between a script's 238 and the field.
pub const WINGED_TINT: [i32; 3] = [8, 6, 16];

/// Whether the hero is in his winged (Celestrian) form in the field of `room` at story point
/// `chapter.step`: the state trigger action `0x97(1)` sets on arrival and `0x97(2)`/`0x97(0)`
/// clear (`Trig_RunAction` case `0x33`, `0x02061c14`: it starts the task `ov_017::021bdc34`,
/// which sets bit `0x40` of the leader's `+0x18c` and tints the palettes of his models with
/// [`WINGED_TINT`]). The rules that set it, all arrival rules (event 3) of `triggerM01.bin`
/// (`dq9re triggers M01`, `docs/re/mortal-veil.md`):
///
/// - `0x97(1)` in the rooms 1100 to 1110 (Angel Falls and its houses) from 1.2 to 1.3;
/// - `0x97(2)` in the Observatory rooms (4101, 4103 to 4107, 4109) from 1.1 to 1.4, and at the
///   end of `21593` / `21595` (the story then warps to the Observatory);
/// - `0x97(0)` on arrival in 1100 at 2.1.
///
/// Only a rule of those rooms and ranges changes the state, so it is a function of the room
/// and the story point: the maps in between (none exist in 1.2 and 1.3) are not read.
pub fn hero_winged(room: u16, chapter: i32, step: i32) -> bool {
    let story = chapter * 1000 + step;
    (1100..=1110).contains(&room) && (1002..=1003).contains(&story)
}

/// First weapon of a new game: the **copper sword**, item 20004. Character creation writes
/// it with the opening outfit from the table at `ov_009` `0x0218636c` (overlay file offset
/// `0x20cc`, u32 ids 20004, 20500, 20403, 20600, 20100; the u16 list at `0x0218ab78` repeats
/// 20004 with the outfit items). The item's compendium index is 1 (`itemdt_w`), which is the
/// `3191` flag of the original states.
pub const NEW_GAME_WEAPON: u16 = 20004;

/// Where the party is put when the map is entered (`ov_017::0219c598`, no explicit position).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The map's own start point (`.bmbl` record `0x6e`): normal save, new game.
    MapStart,
    /// A position carried by the state (`Save_Warp`, set by an intermission save).
    At(HeroPose),
}

/// Why no state could be built from a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    /// Neither slot is a valid normal save nor a readable suspend save.
    NoValidSlot,
    /// The slot's event block is not a whole block (cannot happen on a parsed save).
    BadEventBlock,
    /// The slot's party block is inconsistent (more than four members, a record or held
    /// items past the block, a bag that does not fit).
    BadParty,
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::NoValidSlot => "the save has no valid normal or suspend slot",
            Self::BadEventBlock => "the save's event block has the wrong size",
            Self::BadParty => "the save's party block is inconsistent",
        })
    }
}

impl std::error::Error for StateError {}

/// The state a save restores.
#[derive(Debug, Clone, PartialEq)]
pub struct GameState {
    /// Current map id (`GameState+0x7e74`; map list id, 109 = `C01M09`).
    pub map: u16,
    pub placement: Placement,
    pub scenario: Scenario,
    pub clock: DayClock,
    /// Members in party order (the leader first), gold and bag (`Save_UnpackMain`,
    /// `0x020a95b4`).
    pub party: Party,
    events: Vec<u8>,
}

impl GameState {
    /// A new game: prologue map, scenario 1/1/1 and the day's start, the clock running
    /// (`0x0200f3a4`; the clock's initial 210 s from the wiki's *Time-Of-Day*), and the hero
    /// character creation makes ([`Party::new_game`]). The event block is empty
    /// **[unknown]**: the game's initial tracks are not read.
    pub fn new_game() -> Self {
        Self {
            map: NEW_GAME_MAP,
            placement: Placement::MapStart,
            scenario: Scenario {
                chapter: 1,
                step: 1,
                substep: 1,
            },
            clock: DayClock::new(DAY_START, true),
            party: Party::new_game(),
            events: vec![0; EVENT_FLAGS_SIZE],
        }
    }

    /// The state of the slot a start-up resumes ([`Save::resume_slot`]): the normal adventure
    /// log when the container has one, else the **suspend** save, whose `Warp` block then puts
    /// the party at the pose it was written with ([`Placement::At`]).
    pub fn from_save(save: &Save) -> Result<Self, StateError> {
        let slot = save
            .resume_slot()
            .and_then(|i| save.slot(i))
            .ok_or(StateError::NoValidSlot)?;
        Self::from_slot(&slot)
    }

    /// What `Save_LoadSlot` (`0x020aaf94`) restores, for one slot: map and position
    /// (`Save_UnpackMain`, `Save_UnpackWarp`), scenario and event block (`Save_UnpackProgress`),
    /// day timer (header A). The clock runs after a load (`+0x3d8` is 1 in RAM; the flag is not
    /// saved).
    pub fn from_slot(slot: &Slot<'_>) -> Result<Self, StateError> {
        let events = slot.event_flags();
        let events = events.as_bytes().to_vec();
        if EventFlags::from_bytes(&events).is_none() {
            return Err(StateError::BadEventBlock);
        }
        Ok(Self {
            map: slot.area(),
            placement: slot.warp().map_or(Placement::MapStart, |w| {
                Placement::At(HeroPose {
                    position: w.position,
                    angle: w.angle,
                })
            }),
            scenario: Scenario {
                chapter: slot.chapter(),
                step: slot.scenario_step(),
                substep: slot.scenario_substep(),
            },
            clock: DayClock::new(slot.day_timer(), true),
            party: Party::from_slot(slot).map_err(|_| StateError::BadParty)?,
            events,
        })
    }

    /// Event tracks and flags (`g_QuestManager`'s saved block).
    pub fn events(&self) -> EventFlags<'_> {
        EventFlags::from_bytes(&self.events).expect("block size is checked on construction")
    }

    /// Where the party stands on entering the map (`ov_017::0219c598` without explicit
    /// position): the carried position, else the map's `start`. Not grounded: the game's
    /// per-frame update may later move the height onto the floor (`docs/re/save.md`).
    pub fn hero_pose(&self, start: StartPoint) -> HeroPose {
        match self.placement {
            Placement::At(pose) => pose,
            Placement::MapStart => start.into(),
        }
    }

    /// The state a played session holds, bound to a save's layout: the live trigger state
    /// supplies the event block's tracks, global flags, quest nibbles and track selector (the
    /// inverse of [`crate::field::saved_state`]); `map` and `clock` are the live map and day
    /// clock, `party` the live party (members, gold, bag). `base` is the state loaded from
    /// the input slot: it carries what a run does not model and what a write keeps, the
    /// placement (a normal save has none, `docs/re/save.md` § "Position on resume") and the
    /// opaque bytes of the event block (`+0x2c0`, 12 bytes).
    ///
    /// Global flags outside the saved array are dropped here: the array holds indices
    /// `0..0x11a0` (`Flags_Test`), and `live` may hold flags the save does not carry.
    pub fn from_play(
        live: &crate::trigger::State,
        base: &Self,
        map: u16,
        clock: DayClock,
        party: Party,
    ) -> Result<Self, StateError> {
        let mut events = base.events.clone();
        {
            let mut saved = EventFlagsMut::new(&mut events).ok_or(StateError::BadEventBlock)?;
            for (index, track) in live.tracks.iter().enumerate() {
                let Some(mut block) = saved.track_mut(index) else {
                    break;
                };
                block.set_chapter(track.chapter);
                block.set_step(track.step);
                block.set_substep(track.substep);
                block.set_chapter_flags(&flag_bits(&track.flags[0], 13));
                block.set_step_flags(&flag_bits(&track.flags[1], 12));
            }
            for index in 0..(GLOBAL_FLAGS_SIZE * 8) {
                let index = index as i32;
                saved.set_global_flag(index, live.flags.contains(&index), false);
            }
            for quest in 0..QUEST_COUNT as i32 {
                saved.set_quest(quest, live.quests.get(&quest).copied().unwrap_or(0));
            }
            saved.set_current_track(live.track.try_into().unwrap_or(0));
        }
        let track = live.tracks.get(live.track).unwrap_or(&live.tracks[0]);
        Ok(Self {
            map,
            placement: base.placement,
            scenario: Scenario {
                chapter: u32::from(track.chapter),
                step: u32::from(track.step),
                substep: u32::from(track.substep),
            },
            clock,
            party,
            events,
        })
    }

    /// Writes the fields this state carries into `slot`: the inverse of
    /// [`GameState::from_slot`], every field the reader fills going back at the same offset.
    /// The checksums stay stale ([`SlotMut::seal`]); the ranges returned are the slot-relative
    /// bytes written, sorted and deduplicated.
    ///
    /// Not written, because the state does not carry them: the standby count and the party
    /// order (member object ids, not record indices), the per-vocation seeds (`+0x58`), the
    /// record's `+0x14c` base attributes (fields 0 to 6 — the original writes them as a cache
    /// of the live attributes **after** seeds and equipment, which the state does not model,
    /// and its load recomputes them anyway), the bank, medals, visited places, special guests,
    /// party tricks, unlocked vocations, the version string, and every block the reader leaves
    /// opaque (`Block`'s content beyond the fields above, the 12 bytes at `+0x2c0` of the
    /// event block). The stat block's current HP and MP (fields 7/8) and maxima (9/10)
    /// **are** written. An empty equipment, held-item or bag place keeps the word it had when
    /// that word was empty too (`0` or [`NO_ITEM`]), so a write of unchanged state changes no
    /// byte.
    pub fn write_into(&self, slot: &mut SlotMut<'_>) -> Result<Vec<Range<usize>>, StateError> {
        let mut ranges = Vec::new();
        ranges.push(slot.set_area(self.map));
        if let Placement::At(pose) = self.placement {
            ranges.push(slot.set_warp(Warp {
                map: self.map,
                angle: pose.angle,
                position: pose.position,
            }));
        }
        ranges.push(slot.set_day_timer(self.clock.seconds()));
        ranges.push(slot.set_chapter(self.scenario.chapter));
        ranges.push(slot.set_scenario_step(self.scenario.step));
        ranges.push(slot.set_scenario_substep(self.scenario.substep));
        let events = self.events();
        {
            let mut flags = slot.event_flags_mut();
            for index in 0.. {
                let Some(track) = events.track(index) else {
                    break;
                };
                let Some(mut block) = flags.track_mut(index) else {
                    break;
                };
                ranges.push(block.set_chapter(track.chapter()));
                ranges.push(block.set_step(track.step()));
                ranges.push(block.set_substep(track.substep()));
                ranges.push(block.set_chapter_flags(track.chapter_flags()));
                ranges.push(block.set_step_flags(track.step_flags()));
            }
            for index in 0..(GLOBAL_FLAGS_SIZE * 8) {
                let index = index as i32;
                if let Some(range) = flags.set_global_flag(index, events.global_flag(index), false)
                {
                    ranges.push(range);
                }
            }
            for quest in 0..QUEST_COUNT as i32 {
                if let Some(range) = flags.set_quest(quest, events.quest(quest)) {
                    ranges.push(range);
                }
            }
            ranges.push(flags.set_current_track(events.current_track().try_into().unwrap_or(0)));
        }
        let count = self.party.members.len();
        if count > PARTY_MAX {
            return Err(StateError::BadParty);
        }
        ranges.push(slot.set_party_count(count as u8));
        let standby = usize::from(slot.as_slot().standby_count());
        for (index, member) in self.party.members.iter().enumerate() {
            let record = standby.checked_add(index).ok_or(StateError::BadParty)?;
            // An empty place keeps the word it already had when that word is empty too
            // (`0` and `NO_ITEM` both read as empty): a write of unchanged state must not
            // change the representation an input save used.
            let (current_equipment, current_held, current_look): (
                [u16; 8],
                [u16; HELD_ITEMS],
                Option<HeroLook>,
            ) = {
                let current = slot.as_slot();
                let character = current.character(record);
                (
                    core::array::from_fn(|i| {
                        character.map_or(0, |c| c.equipment(dq9_formats::save::EquipSlot::ALL[i]))
                    }),
                    current.held_items(index).unwrap_or([0; HELD_ITEMS]),
                    character.map(|c| HeroLook::from_character(&c)),
                )
            };
            {
                let mut saved = slot.character_mut(record).ok_or(StateError::BadParty)?;
                ranges.push(saved.set_name_raw(&member.name));
                ranges.push(saved.set_gender(u8::from(member.sex == Sex::Woman)));
                // The look, from the live member: `func_020830cc` (the writer of
                // `Save_Serialize`'s character records) copies the 28 bytes at the member
                // object's `+0x488` onto the record's `+0x160` for every active member, and
                // `PartyMember_LoadRecord` (`0x02082d7c`) reads them back on a load — face
                // and hair item words, the skin/eye/gender byte, the hair colour and the body
                // words. The gender bit above comes from the same character, so it is not
                // repeated here. A field the record already reads as is left alone, so an
                // unchanged save keeps its bytes (the look's hair colour is clamped to
                // 0..=9 on read and rewriting it would normalise an out-of-band byte).
                let look = &member.look;
                let face = current_look
                    .as_ref()
                    .is_none_or(|c| c.face_item != look.face_item);
                let hair = current_look
                    .as_ref()
                    .is_none_or(|c| c.hair_item != look.hair_item);
                let hair_color = current_look
                    .as_ref()
                    .is_none_or(|c| c.hair_color != look.hair_color);
                let skin = current_look
                    .as_ref()
                    .is_none_or(|c| c.skin_tone != look.skin_tone);
                let eyes = current_look
                    .as_ref()
                    .is_none_or(|c| c.eye_color != look.eye_color);
                let body = current_look.as_ref().is_none_or(|c| c.build != look.build);
                if face {
                    ranges.push(saved.set_face_item(look.face_item));
                }
                if hair {
                    ranges.push(saved.set_hair_item(look.hair_item));
                }
                if hair_color {
                    ranges.push(saved.set_hair_color(look.hair_color));
                }
                if skin {
                    ranges.push(saved.set_skin_color(look.skin_tone));
                }
                if eyes {
                    ranges.push(saved.set_eye_color(look.eye_color));
                }
                if body {
                    ranges.push(saved.set_body(look.build.0, look.build.1));
                }
                ranges.push(saved.set_vocation(member.vocation));
                for (vocation, &level) in member.levels.iter().enumerate() {
                    ranges.extend(saved.set_level(vocation, level));
                }
                for (vocation, &experience) in member.experience.iter().enumerate() {
                    ranges.extend(saved.set_experience(vocation, experience));
                }
                for (i, place) in dq9_formats::save::EquipSlot::ALL.into_iter().enumerate() {
                    let word = match member.equipment.get(place) {
                        Some(item) => item,
                        None if is_empty_word(current_equipment[i]) => continue,
                        None => NO_ITEM,
                    };
                    ranges.push(saved.set_equipment(place, word));
                }
                ranges.push(saved.set_unallocated_skill_points(member.skill_points));
                ranges.push(saved.set_proficiency_flags(&member.skill_panels));
                // The record carries the member's current HP and MP (fields 7/8 of
                // `+0x14c`) and its maxima (fields 9/10): `func_020830cc` writes
                // `+0x1c`/`+0x1e` and `+0x6c`/`+0x6e` there, and `PartyMember_LoadRecord`
                // reads 7 and 8 back, so a wounded member reloads wounded. A zero maximum
                // means "not established yet" (a new game's hero before the level tables):
                // the record's own value is then kept rather than zeroed, and so is the
                // current value it belongs to.
                if member.max_hp != 0 {
                    ranges.push(saved.set_hp(member.hp));
                    ranges.push(saved.set_max_hp(member.max_hp));
                }
                if member.max_mp != 0 {
                    ranges.push(saved.set_mp(member.mp));
                    ranges.push(saved.set_max_mp(member.max_mp));
                }
            }
            let held: [u16; HELD_ITEMS] =
                core::array::from_fn(|slot| match member.held_items[slot] {
                    Some(item) => item,
                    None if is_empty_word(current_held[slot]) => current_held[slot],
                    None => NO_ITEM,
                });
            ranges.extend(slot.set_held_items(index, &held));
        }
        // The bag is part of the state (`Party::from_slot` fills it) and the session mutates
        // it: containers, item use and battle drops all go through `Bag`.
        let bag: Vec<(Option<u16>, u8)> = self
            .party
            .bag
            .slots()
            .iter()
            .map(|stack| (stack.item, stack.quantity))
            .collect();
        ranges.push(slot.set_bag(&bag).ok_or(StateError::BadParty)?);
        ranges.push(slot.set_gold_on_hand(self.party.gold));
        ranges.sort_by_key(|range| (range.start, range.end));
        ranges.dedup();
        Ok(ranges)
    }

    /// Writes this state into `slot` as a **suspend** (intermission) save: the original's
    /// `Save_Serialize` (`0x020a9ec8`) with its `intermission` argument set by the field
    /// menu's Quick Save (`ov_002::02169efc`, `Save_WriteStep` state 1 at `0x8010`).
    ///
    /// The header's intermission and exists bytes are set, the modelled state goes at the
    /// offsets a normal save writes (the `Main` block's own map word included), and the
    /// `Warp` block carries `pose` — the position and heading `Save_UnpackWarp`
    /// (`0x020a9d88`) stands the party at on the next load. `pose` is the **live** pose of
    /// the leader, not this state's [`placement`](Self::placement): that one is where a
    /// previous load put the party, and a run moves away from it.
    ///
    /// The caller writes this into [`dq9_formats::save::SUSPEND_SLOT`]; the normal slot is
    /// left alone. Checksums stay stale ([`SlotMut::seal`]); the ranges returned are
    /// slot-relative, sorted and deduplicated.
    pub fn write_suspend(
        &self,
        slot: &mut SlotMut<'_>,
        pose: HeroPose,
    ) -> Result<Vec<Range<usize>>, StateError> {
        let mut state = self.clone();
        state.placement = Placement::At(pose);
        // `SlotMut::set_area` picks its offset from the header: a slot that already held a
        // suspend save must be addressed as a normal one first, so that the `Main` block's
        // own map word (`11476`) is written too, as `Save_Serialize` does for both kinds.
        let mut ranges = vec![slot.set_intermission(false)];
        ranges.extend(state.write_into(slot)?);
        ranges.push(slot.set_intermission(true));
        ranges.push(slot.set_exists(true));
        ranges.sort_by_key(|range| (range.start, range.end));
        ranges.dedup();
        Ok(ranges)
    }
}

/// The bytes of a flag set that a track holds: bit `index` of `bytes` bytes, least significant
/// bit first (`Flags_Test`'s convention, [`dq9_formats::save::test_bit`]).
fn flag_bits(set: &BTreeSet<i32>, bytes: usize) -> Vec<u8> {
    let mut out = vec![0u8; bytes];
    for &flag in set {
        if let Ok(index) = usize::try_from(flag)
            && index < bytes * 8
        {
            out[index / 8] |= 1 << (index % 8);
        }
    }
    out
}

/// Whether an item word is an empty place: the two representations the readers accept
/// ([`dq9_formats::save::NO_ITEM`] and 0).
fn is_empty_word(word: u16) -> bool {
    word == NO_ITEM || word == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::save::{FILE_SIZE, MAGIC};

    fn blank_save() -> Vec<u8> {
        let mut bytes = vec![0; FILE_SIZE];
        bytes[..MAGIC.len()].copy_from_slice(MAGIC);
        bytes[21] = 1;
        bytes
    }

    fn sealed(bytes: Vec<u8>) -> Save {
        let mut save = Save::parse(&bytes).unwrap();
        save.slot_mut(0).unwrap().seal();
        save
    }

    #[test]
    fn clock_phases_follow_the_thresholds() {
        let phase = |s| DayClock::new(s, true).phase();
        assert_eq!(phase(0.0), TimeOfDay::Night);
        assert_eq!(phase(179.9), TimeOfDay::Night);
        assert_eq!(phase(180.0), TimeOfDay::Morning);
        assert_eq!(phase(210.0), TimeOfDay::Day);
        assert_eq!(phase(389.9), TimeOfDay::Day);
        assert_eq!(phase(390.0), TimeOfDay::Evening);
        // The day wraps: 430 s is 10 s into the next day.
        assert_eq!(DayClock::new(430.0, true).seconds(), 10.0);
        assert_eq!(phase(430.0), TimeOfDay::Night);
    }

    #[test]
    fn clock_advances_only_while_running() {
        let mut clock = DayClock::new(389.0, true);
        clock.advance(60);
        assert_eq!(clock.phase(), TimeOfDay::Evening);
        clock.set_running(false);
        let before = clock.seconds();
        clock.advance(3);
        assert_eq!(clock.seconds(), before);
        clock.set_phase(TimeOfDay::Morning);
        assert_eq!(clock.seconds(), 180.0);
        assert!(TimeOfDay::Morning.is_daytime() && !TimeOfDay::Night.is_daytime());
    }

    #[test]
    fn normal_save_restores_map_scenario_clock_and_map_start() {
        let mut bytes = blank_save();
        bytes[11476..11478].copy_from_slice(&109u16.to_le_bytes());
        bytes[32..36].copy_from_slice(&42.5f32.to_le_bytes());
        bytes[11568..11572].copy_from_slice(&19u32.to_le_bytes());
        bytes[11572..11576].copy_from_slice(&2u32.to_le_bytes());
        bytes[11576..11580].copy_from_slice(&1u32.to_le_bytes());
        // Global flag 9 in the event block (`slot+11584`, flags at `+0x8c`).
        bytes[11584 + 0x8c + 1] = 0b10;
        let state = GameState::from_save(&sealed(bytes)).unwrap();
        assert_eq!(state.map, 109);
        assert_eq!(state.placement, Placement::MapStart);
        assert_eq!(
            state.scenario,
            Scenario {
                chapter: 19,
                step: 2,
                substep: 1
            }
        );
        assert_eq!(state.clock.seconds(), 42.5);
        assert_eq!(state.clock.phase(), TimeOfDay::Night);
        assert!(state.events().global_flag(9) && !state.events().global_flag(8));
        let start = StartPoint {
            position: [860, 491, -5652],
            angle: 12861,
        };
        assert_eq!(state.hero_pose(start), start.into());
    }

    #[test]
    fn intermission_save_carries_its_own_position() {
        let mut bytes = blank_save();
        bytes[20] = 1;
        bytes[27104..27106].copy_from_slice(&100u16.to_le_bytes());
        bytes[27106..27108].copy_from_slice(&(-5i16).to_le_bytes());
        for (i, v) in [4096i32, 8192, -4096].iter().enumerate() {
            bytes[27108 + 4 * i..][..4].copy_from_slice(&v.to_le_bytes());
        }
        let save = Save::parse(&bytes).unwrap();
        let state = GameState::from_slot(&save.slot(0).unwrap()).unwrap();
        assert_eq!(state.map, 100);
        let pose = HeroPose {
            position: [4096, 8192, -4096],
            angle: -5,
        };
        assert_eq!(state.placement, Placement::At(pose));
        assert_eq!(state.hero_pose(StartPoint::default()), pose);
        assert_eq!(pose.position_f32(), [1.0, 2.0, -1.0]);
    }

    #[test]
    fn save_without_a_valid_slot_is_rejected() {
        let save = Save::parse(&vec![0; FILE_SIZE]).unwrap();
        assert_eq!(GameState::from_save(&save), Err(StateError::NoValidSlot));
    }

    #[test]
    fn new_game_starts_at_the_day_start_on_the_map_start() {
        let state = GameState::new_game();
        assert_eq!(state.clock.phase(), TimeOfDay::Day);
        assert_eq!(state.placement, Placement::MapStart);
        assert_eq!(state.scenario.chapter, 1);
        assert!(!state.events().global_flag(0));
    }

    #[test]
    fn new_game_carries_the_hero_party() {
        let state = GameState::new_game();
        assert_eq!(state.party.members.len(), 1);
        let hero = state.party.leader().unwrap();
        assert_eq!(hero.vocation, crate::party::NOVICE_VOCATION);
        assert_eq!(hero.levels, [1; dq9_formats::save::VOCATION_COUNT]);
        assert_eq!(hero.experience, [0; dq9_formats::save::VOCATION_COUNT]);
        assert_eq!(state.party.gold, 0);
        assert!(state.party.bag.is_empty());
    }

    #[test]
    fn map_visit_flags_follow_the_map_load_rule() {
        // Prologue maps seen in the oracle states (`docs/re/trigger.md`).
        assert_eq!(map_visit_flag(1112), Some(2721));
        assert_eq!(map_visit_flag(4100), Some(2751));
        assert_eq!(map_visit_flag(6401), Some(2774));
        // The general branch is inclusive on both ends; 1949 is a map id here (the constant
        // 1949 of the original is a flag set by `func_0201b588`, not a map).
        assert_eq!(map_visit_flag(99), None);
        assert_eq!(map_visit_flag(100), Some(2711));
        assert_eq!(map_visit_flag(1949), Some(2729));
        assert_eq!(map_visit_flag(9999), Some(2809));
        assert_eq!(map_visit_flag(10000), None);
        // The flag is one per hundred map ids.
        assert_eq!(map_visit_flag(1100), map_visit_flag(1199));
        assert_eq!(map_visit_flag(100), Some(2711));
        // Branches of the original that are not reproduced.
        assert_eq!(map_visit_flag(99), None);
        assert_eq!(map_visit_flag(10000), None);
        assert_eq!(map_visit_flag(20101), None);
    }

    #[test]
    fn item_flags_follow_the_compendium_index() {
        // Compendium indexes of the original's first-control flags (`itemdt_*`, 2026-11-06).
        assert_eq!(item_obtained_flag(1), Some(3191));
        assert_eq!(item_obtained_flag(585), Some(3775));
        assert_eq!(item_obtained_flag(1093), Some(4283));
        assert_eq!(item_obtained_flag(945), Some(4135));
        // Index 0 has no compendium entry: `func_0207cc18` skips it.
        assert_eq!(item_obtained_flag(0), None);
        assert_eq!(item_obtained_flag(u16::MAX), None);
    }

    /// A normal save with every field the binding writes set to a distinct value.
    fn played_save() -> Vec<u8> {
        use dq9_formats::save::EquipSlot;
        let mut save = sealed(blank_save());
        {
            let mut slot = save.slot_mut(0).unwrap();
            slot.set_area(109);
            slot.set_day_timer(42.5);
            slot.set_chapter(19);
            slot.set_scenario_step(2);
            slot.set_scenario_substep(1);
            slot.set_party_count(1);
            slot.set_gold_on_hand(38_754);
            {
                let mut flags = slot.event_flags_mut();
                flags.set_global_flag(9, true, false);
                flags.set_global_flag(4511, true, false);
                flags.set_quest(3, 0b1010);
                flags.set_quest(0xcb, 1);
                flags.set_current_track(1);
                let mut track = flags.track_mut(1).unwrap();
                track.set_chapter(19);
                track.set_step(2);
                track.set_substep(1);
                track.set_chapter_flags(&[0x0f, 0]);
                track.set_step_flags(&[0x30]);
            }
            let mut hero = slot.character_mut(0).unwrap();
            hero.set_name_raw(&[18, 47, 48, 39]);
            hero.set_gender(1);
            hero.set_face_item(9_023);
            hero.set_hair_item(9_007);
            hero.set_hair_color(4);
            hero.set_skin_color(2);
            hero.set_eye_color(5);
            hero.set_body(3_600, 4_200);
            hero.set_vocation(6);
            hero.set_level(6, 42).unwrap();
            hero.set_experience(6, 1_234_567).unwrap();
            hero.set_equipment(EquipSlot::Weapon, 20_004);
            hero.set_equipment(EquipSlot::Head, 9_001);
            hero.set_unallocated_skill_points(12);
            hero.set_proficiency_flags(&[0b1, 0, 0b10]);
            slot.set_held_items(0, &[1, 2, 3, NO_ITEM, 5, 6, NO_ITEM, 8])
                .unwrap();
            slot.set_bag(&[(Some(1), 3), (Some(2), 1), (None, 0), (None, 0)])
                .unwrap();
            slot.seal();
        }
        let mut bytes = save.as_bytes().to_vec();
        // The per-vocation seeds (`+0x58`) and the base attributes (`+0x14c` fields 0 to 6)
        // have no writer: a pattern there proves the binding leaves them alone. Fields 7 to
        // 10 (current HP and MP, maxima) and 11 to 13 do round-trip as the record's own
        // values, so the pattern must come back byte for byte.
        for (i, byte) in bytes[136 + 0x58..136 + 0x94].iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(7) | 1;
        }
        for (i, byte) in bytes[136 + 0x14c..136 + 0x160].iter_mut().enumerate() {
            *byte = (i as u8) | 0x80;
        }
        let mut save = Save::parse(&bytes).unwrap();
        save.slot_mut(0).unwrap().seal();
        save.as_bytes().to_vec()
    }

    #[test]
    fn a_state_writes_back_onto_its_own_slot() {
        let bytes = played_save();
        let state = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let mut rewritten = Save::parse(&bytes).unwrap();
        let ranges = {
            let mut slot = rewritten.slot_mut(0).unwrap();
            let ranges = state.write_into(&mut slot).unwrap();
            slot.seal();
            ranges
        };
        assert!(!ranges.is_empty(), "the binding wrote nothing");
        assert_eq!(rewritten.as_bytes(), &bytes[..]);
    }

    /// The live member's look is what the record keeps: `func_020830cc` writes the 28 bytes
    /// at the member object's `+0x488` (face `+356`, hair `+358`, appearance `+372`, hair
    /// colour `+373`, body `+376`/`+378`), and a reload reads them back through
    /// `PartyMember_LoadRecord`.
    #[test]
    fn a_live_look_is_written_into_the_record_and_reloads() {
        let bytes = played_save();
        let mut state = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let look = &mut state.party.members[0].look;
        look.face_item = 9_021;
        look.hair_item = 9_013;
        look.hair_color = 7;
        look.skin_tone = 6;
        look.eye_color = 9;
        look.build = (3_900, 4_300);
        let mut written = Save::parse(&bytes).unwrap();
        {
            let mut slot = written.slot_mut(0).unwrap();
            state.write_into(&mut slot).unwrap();
            slot.seal();
        }
        // The record itself, not only the round trip.
        let before = Save::parse(&bytes).unwrap();
        let was = before.slot(0).unwrap().character(0).unwrap();
        let record = written.slot(0).unwrap().character(0).unwrap();
        assert_eq!(record.face_item(), 9_021, "+356");
        assert_eq!(record.hair_item(), 9_013, "+358");
        assert_eq!(record.hair_color(), 7, "+373");
        assert_eq!(record.skin_color(), 6, "+372 bits 1-3");
        assert_eq!(record.eye_color(), 9, "+372 bits 4-7");
        assert_eq!(record.body(), (3_900, 4_300), "+376/+378");
        assert_ne!(
            record.face_item(),
            was.face_item(),
            "the input's face was replaced"
        );
        assert_ne!(record.body(), was.body(), "the input's body was replaced");
        let reloaded = GameState::from_save(&written).unwrap();
        assert_eq!(reloaded.party.members[0].look, state.party.members[0].look);

        // A second write of the same session is the same image.
        let mut again = Save::parse(written.as_bytes()).unwrap();
        {
            let mut slot = again.slot_mut(0).unwrap();
            reloaded.write_into(&mut slot).unwrap();
            slot.seal();
        }
        assert_eq!(again.as_bytes(), written.as_bytes());
    }

    /// The demo container (`DQ9_SAVE`): a created look replaces every appearance field of
    /// its hero record, the other members' records keep their bytes, and the file reloads to
    /// the live look.
    #[test]
    fn a_real_saves_hero_look_is_replaced_by_the_live_one() {
        let Some(path) = std::env::var_os("DQ9_SAVE") else {
            eprintln!("skipped: no DQ9_SAVE");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let mut state = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let was = state.party.members[0].look.clone();
        let mut look = was.clone();
        look.face_item = 9_021;
        look.hair_item = 9_013;
        look.hair_color = 7;
        look.skin_tone = 6;
        look.eye_color = 9;
        look.build = (3_900, 4_300);
        state.party.members[0].look = look.clone();
        let mut written = Save::parse(&bytes).unwrap();
        let index = written.current_slot().unwrap();
        {
            let mut slot = written.slot_mut(index).unwrap();
            state.write_into(&mut slot).unwrap();
            slot.seal();
        }
        if index == 0 {
            written.mirror_normal_slot();
        }
        let reloaded = GameState::from_save(&written).unwrap();
        assert_eq!(reloaded.party.members[0].look, look);
        assert_ne!(
            look.face_item, was.face_item,
            "the fixture differs on the face"
        );
        let record = written.slot(index).unwrap().character(0).unwrap();
        assert_eq!(
            (
                record.face_item(),
                record.hair_item(),
                record.skin_color(),
                record.eye_color(),
                record.hair_color(),
                record.body()
            ),
            (9_021, 9_013, 6, 9, 7, (3_900, 4_300))
        );
        // The other members' looks were not touched: their records still read as before.
        let after = Save::parse(&bytes).unwrap();
        for i in 1..reloaded.party.members.len() {
            let old = after.slot(index).unwrap().character(i).unwrap();
            let new = written.slot(index).unwrap().character(i).unwrap();
            assert_eq!(
                HeroLook::from_character(&new),
                HeroLook::from_character(&old),
                "member {i}'s look"
            );
        }
    }

    #[test]
    fn an_intermission_state_writes_its_warp_and_map() {
        let mut bytes = blank_save();
        bytes[20] = 1;
        bytes[27104..27106].copy_from_slice(&100u16.to_le_bytes());
        bytes[27106..27108].copy_from_slice(&(-5i16).to_le_bytes());
        for (i, v) in [4096i32, 8192, -4096].iter().enumerate() {
            bytes[27108 + 4 * i..][..4].copy_from_slice(&v.to_le_bytes());
        }
        let save = sealed(bytes);
        let bytes = save.as_bytes().to_vec();
        let state = GameState::from_slot(&save.slot(0).unwrap()).unwrap();
        let mut rewritten = Save::parse(&bytes).unwrap();
        {
            let mut slot = rewritten.slot_mut(0).unwrap();
            state.write_into(&mut slot).unwrap();
            slot.seal();
        }
        assert_eq!(rewritten.as_bytes(), &bytes[..]);
    }

    /// The field menu's Quick Save (`ov_002::02169efc`, intermission 1 to `Save_WriteStep`):
    /// slot 1 gets the intermission header byte, the modelled state and the warp block of the
    /// **live** pose; slot 0 keeps every byte, and the file resumes at that pose.
    #[test]
    fn a_suspend_write_marks_slot_one_and_resumes_at_its_pose() {
        use dq9_formats::save::SlotStatus;
        let bytes = played_save();
        let state = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let pose = HeroPose {
            position: [4096, 8192, -4096],
            angle: 12_866,
        };
        let mut written = Save::parse(&bytes).unwrap();
        let ranges = {
            let mut slot = written.slot_mut(dq9_formats::save::SUSPEND_SLOT).unwrap();
            let ranges = state.write_suspend(&mut slot, pose).unwrap();
            slot.seal();
            ranges
        };
        for (at, what) in [(20, "intermission"), (21, "exists"), (27104, "warp")] {
            assert!(
                ranges.iter().any(|range| range.contains(&at)),
                "{what} ({at}) outside the written ranges"
            );
        }
        assert_eq!(
            written.as_bytes()[..dq9_formats::save::SLOT_SIZE],
            bytes[..dq9_formats::save::SLOT_SIZE],
            "the normal slot is untouched"
        );
        let slot = written.slot(1).unwrap();
        assert_eq!(slot.status(), SlotStatus::Intermission);
        assert!(slot.checksums_valid());
        assert_eq!(
            slot.warp(),
            Some(Warp {
                map: state.map,
                angle: pose.angle,
                position: pose.position,
            })
        );
        // A reader that does not look at the warp block (the editor, a normal-save read of
        // the bytes) finds the current map at the `Main` block's own word too.
        assert_eq!(
            u16::from_le_bytes([slot.bytes()[11476], slot.bytes()[11477]]),
            state.map
        );
        let resumed = GameState::from_slot(&slot).unwrap();
        assert_eq!(resumed.map, state.map);
        assert_eq!(resumed.scenario, state.scenario);
        assert_eq!(resumed.party, state.party);
        assert_eq!(resumed.clock.seconds(), state.clock.seconds());
        assert_eq!(resumed.events().as_bytes(), state.events().as_bytes());
        assert_eq!(resumed.placement, Placement::At(pose));
        assert_eq!(resumed.hero_pose(StartPoint::default()), pose);
    }

    /// Writing a slot that already holds a suspend save: the header's intermission byte is
    /// already 1, so `set_area` would aim at the warp block; the walk must still write the
    /// `Main` block's map word (11476) like `Save_Serialize` does.
    #[test]
    fn a_second_suspend_write_updates_the_block_map_of_an_intermission_slot() {
        let bytes = played_save();
        let state = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let mut written = Save::parse(&bytes).unwrap();
        let first = HeroPose {
            position: [1, 2, 3],
            angle: 4,
        };
        {
            let mut slot = written.slot_mut(dq9_formats::save::SUSPEND_SLOT).unwrap();
            state.write_suspend(&mut slot, first).unwrap();
            slot.seal();
        }
        let mut moved = state.clone();
        moved.map = 1112;
        let second = HeroPose {
            position: [5, 6, 7],
            angle: -9,
        };
        {
            let mut slot = written.slot_mut(dq9_formats::save::SUSPEND_SLOT).unwrap();
            moved.write_suspend(&mut slot, second).unwrap();
            slot.seal();
        }
        let slot = written.slot(dq9_formats::save::SUSPEND_SLOT).unwrap();
        assert_eq!(
            u16::from_le_bytes([slot.bytes()[11476], slot.bytes()[11477]]),
            1112
        );
        assert_eq!(
            slot.warp(),
            Some(Warp {
                map: 1112,
                angle: second.angle,
                position: second.position,
            })
        );
        assert_eq!(GameState::from_slot(&slot).unwrap().map, 1112);
    }

    /// A container with no normal slot resumes its **suspend** save at the `Warp` pose
    /// (`Save_UnpackWarp` `0x020a9d88` + `Party_Place` `ov_017::0219c598`), and deleting the
    /// log (`Save_WriteHeaderA(1)`, `0x020aba6c`) leaves nothing to resume.
    #[test]
    fn a_suspend_save_resumes_when_no_normal_slot_exists() {
        use dq9_formats::save::{SUSPEND_SLOT, SlotStatus};
        let bytes = played_save();
        let base = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let pose = HeroPose {
            position: [4096, 8192, -4096],
            angle: 12_866,
        };
        let mut written = Save::parse(&bytes).unwrap();
        {
            let mut slot = written.slot_mut(SUSPEND_SLOT).unwrap();
            base.write_suspend(&mut slot, pose).unwrap();
            slot.seal();
        }
        {
            // The container the original's quick save leaves behind when slot 0 has no log.
            let mut slot = written.slot_mut(0).unwrap();
            slot.set_exists(false);
            slot.seal();
        }
        assert_eq!(written.current_slot(), None);
        assert_eq!(written.resume_slot(), Some(SUSPEND_SLOT));
        let resumed = GameState::from_save(&written).unwrap();
        assert_eq!(resumed.map, base.map);
        assert_eq!(resumed.placement, Placement::At(pose));
        assert_eq!(resumed.hero_pose(StartPoint::default()), pose);
        assert_eq!(resumed.scenario, base.scenario);
        assert_eq!(resumed.party, base.party);
        assert_eq!(resumed.party.gold, 38_754);
        assert_eq!(resumed.clock.seconds(), base.clock.seconds());
        assert_eq!(resumed.events().as_bytes(), base.events().as_bytes());

        // The resume deletes the log: the slot reads as empty and the container is spent.
        {
            let mut slot = written.slot_mut(SUSPEND_SLOT).unwrap();
            slot.clear_header();
            slot.seal();
        }
        assert_eq!(
            written.slot(SUSPEND_SLOT).unwrap().status(),
            SlotStatus::Empty
        );
        assert_eq!(written.resume_slot(), None);
        assert_eq!(GameState::from_save(&written), Err(StateError::NoValidSlot));
    }

    /// Both slots hold a save: the normal adventure log wins, with the map's start point and
    /// not the suspend save's pose (the port's rule, `docs/re/save.md` § "The suspend slot").
    #[test]
    fn a_normal_slot_wins_over_a_suspend_save() {
        use dq9_formats::save::SUSPEND_SLOT;
        let bytes = played_save();
        let base = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let mut suspend = base.clone();
        suspend.map = base.map + 3;
        let pose = HeroPose {
            position: [1, 2, 3],
            angle: 4,
        };
        let mut written = Save::parse(&bytes).unwrap();
        {
            let mut slot = written.slot_mut(SUSPEND_SLOT).unwrap();
            suspend.write_suspend(&mut slot, pose).unwrap();
            slot.seal();
        }
        assert_eq!(written.resume_slot(), Some(0));
        let resumed = GameState::from_save(&written).unwrap();
        assert_eq!(resumed.map, base.map);
        assert_eq!(resumed.placement, Placement::MapStart);
        assert!(!resumed.events().as_bytes().is_empty());
    }

    #[test]
    fn a_mutation_stays_inside_the_written_ranges() {
        use dq9_formats::save::{Checksum, EquipSlot};
        let bytes = played_save();
        let mut state = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        state.map = 1100;
        state.scenario = Scenario {
            chapter: 2,
            step: 1,
            substep: 3,
        };
        state.clock = DayClock::new(300.0, true);
        state.party.gold = 1234;
        state.party.bag.add(3);
        let hero = &mut state.party.members[0];
        hero.name = [9, 8, 7, 0, 0, 0, 0, 0, 0, 0];
        hero.vocation = 1;
        hero.levels[1] = 12;
        hero.experience[1] = 9999;
        hero.equipment.set(EquipSlot::Weapon, Some(20_005));
        hero.look.face_item = 9_021;
        hero.look.hair_item = 9_013;
        hero.look.hair_color = 8;
        hero.look.skin_tone = 6;
        hero.look.eye_color = 2;
        hero.look.build = (3_900, 4_300);
        hero.max_hp = 111;
        hero.max_mp = 22;
        let mut save = Save::parse(&bytes).unwrap();
        let ranges = {
            let mut slot = save.slot_mut(0).unwrap();
            let ranges = state.write_into(&mut slot).unwrap();
            slot.seal();
            ranges
        };
        let checksums: Vec<Range<usize>> = Checksum::ALL
            .iter()
            .map(|which| which.stored_at()..which.stored_at() + 4)
            .collect();
        let out = save.as_bytes();
        let mut changed = 0;
        for (index, (before, after)) in bytes.iter().zip(out).enumerate() {
            if before == after {
                continue;
            }
            changed += 1;
            let in_slot = index % dq9_formats::save::SLOT_SIZE;
            assert!(
                ranges
                    .iter()
                    .chain(&checksums)
                    .any(|range| range.contains(&in_slot)),
                "byte {in_slot} of slot {} changed outside the written ranges",
                index / dq9_formats::save::SLOT_SIZE
            );
        }
        assert!(
            changed > 5,
            "the mutation changed nothing ({changed} bytes)"
        );
    }

    #[test]
    fn from_play_round_trips_the_live_state() {
        let base = GameState::from_save(&Save::parse(&played_save()).unwrap()).unwrap();
        let mut live = crate::field::saved_state(&base);
        live.flags.insert(4321);
        live.flags.remove(&9);
        live.tracks[2].chapter = 7;
        live.tracks[2].step = 4;
        live.tracks[2].substep = 2;
        live.tracks[2].flags[1].insert(5);
        live.quests.insert(7, 0b1001);
        live.track = 2;
        // The 12 opaque bytes of the block (quest manager `+0x2c0`) have no setter and no
        // reader: a base state keeps them, so the output must carry them unchanged.
        let mut base = base;
        base.events[0x2c0] = 0xab;
        let played = GameState::from_play(
            &live,
            &base,
            1112,
            DayClock::new(100.0, true),
            base.party.clone(),
        )
        .unwrap();
        assert_eq!(played.map, 1112);
        assert_eq!(played.events().as_bytes()[0x2c0], 0xab);
        assert_eq!(
            played.scenario,
            Scenario {
                chapter: 7,
                step: 4,
                substep: 2
            }
        );
        let back = crate::field::saved_state(&played);
        assert_eq!(back.flags, live.flags);
        assert_eq!(back.tracks, live.tracks);
        assert_eq!(back.quests, live.quests);
        assert_eq!(back.track, live.track);
    }

    #[test]
    fn a_party_past_the_character_table_is_refused() {
        let bytes = played_save();
        let mut state = GameState::from_save(&Save::parse(&bytes).unwrap()).unwrap();
        let extra = state.party.members[0].clone();
        state.party.members.push(extra);
        let mut save = Save::parse(&bytes).unwrap();
        save.slot_mut(0).unwrap().set_standby_count(12);
        let mut slot = save.slot_mut(0).unwrap();
        assert_eq!(state.write_into(&mut slot), Err(StateError::BadParty));
    }

    #[test]
    fn a_real_save_writes_back_byte_identical() {
        let Some(path) = std::env::var_os("DQ9_SAVE") else {
            eprintln!("skipped: no DQ9_SAVE");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let save = Save::parse(&bytes).unwrap();
        let state = GameState::from_save(&save).unwrap();
        let mut rewritten = Save::parse(&bytes).unwrap();
        let index = rewritten.current_slot().unwrap();
        {
            let mut slot = rewritten.slot_mut(index).unwrap();
            let ranges = state.write_into(&mut slot).unwrap();
            assert!(!ranges.is_empty());
            slot.seal();
        }
        assert_eq!(index, 0, "the fixture is a normal save in slot 0");
        rewritten.mirror_normal_slot();
        assert_eq!(rewritten.as_bytes(), &bytes[..]);
    }

    /// The demo save's party is wounded (the hero 481 of 693 HP). The record's stat block
    /// carries the current HP and MP (fields 7/8) and the maxima (9/10) — `func_020830cc`
    /// writes the member's `+0x1c`/`+0x1e` and `+0x6c`/`+0x6e` there, `PartyMember_LoadRecord`
    /// reads 7 and 8 back — so the load keeps the saved value, and the write puts it back:
    /// nothing heals on a round trip. A port that read the maxima from 7/8 would load 481/481.
    #[test]
    fn a_real_saves_wounded_party_reloads_wounded() {
        let Some(path) = std::env::var_os("DQ9_SAVE") else {
            eprintln!("skipped: no DQ9_SAVE");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let save = Save::parse(&bytes).unwrap();
        let index = save.current_slot().unwrap();
        let state = GameState::from_save(&save).unwrap();
        let slot = save.slot(index).unwrap();
        let standby = usize::from(slot.standby_count());
        for (i, member) in state.party.members.iter().enumerate() {
            let record = slot.character(standby + i).unwrap();
            assert_eq!(member.hp, record.stat_field(7).unwrap(), "member {i}: HP");
            assert_eq!(member.mp, record.stat_field(8).unwrap(), "member {i}: MP");
            assert_eq!(
                member.max_hp,
                record.stat_field(9).unwrap(),
                "member {i}: max HP"
            );
            assert_eq!(
                member.max_mp,
                record.stat_field(10).unwrap(),
                "member {i}: max MP"
            );
        }
        let wounded = state
            .party
            .members
            .iter()
            .filter(|member| member.hp < member.max_hp)
            .count();
        assert!(wounded > 0, "the fixture's party is wounded");
        // The binding writes those four fields back: the record is reproduced exactly.
        let mut rewritten = Save::parse(&bytes).unwrap();
        {
            let mut slot = rewritten.slot_mut(index).unwrap();
            state.write_into(&mut slot).unwrap();
            slot.seal();
        }
        rewritten.mirror_normal_slot();
        assert_eq!(rewritten.as_bytes(), &bytes[..]);
    }
}

#[cfg(test)]
mod winged_tests {
    use super::*;

    /// The boundaries of the gate: the first and last step with the rule, the neighbours
    /// without it, and the rooms around Angel Falls' own range.
    #[test]
    fn the_hero_is_winged_in_angel_falls_from_1_2_to_1_3_only() {
        for room in 1100..=1110 {
            assert!(!hero_winged(room, 1, 1), "room {room}: before the stage");
            assert!(hero_winged(room, 1, 2), "room {room}: arrival at 1.2");
            assert!(hero_winged(room, 1, 3), "room {room}: still at 1.3");
            assert!(!hero_winged(room, 1, 4), "room {room}: after the stage");
            assert!(!hero_winged(room, 2, 1), "room {room}: chapter 2");
        }
        // Chapter 2 step 2 and 3 are not 1.2/1.3 (`chapter * 1000 + step`).
        assert!(!hero_winged(1100, 2, 2) && !hero_winged(1100, 2, 3));
        assert!(!hero_winged(1099, 1, 2) && !hero_winged(1111, 1, 2));
        // The Observatory is never winged in the field.
        for room in [4101, 4103, 4104, 4105, 4106, 4107, 4109] {
            assert!(!hero_winged(room, 1, 2) && !hero_winged(room, 1, 3));
        }
    }

    /// The gate against the game's own rules: every arrival rule of `triggerM01.bin` and
    /// `triggerX01.bin` with action `0x97`, over its whole story range. `0x97(1)` must give
    /// the winged form, `0x97(2)` and `0x97(0)` must not. Skipped without the ROM.
    #[test]
    fn the_gate_agrees_with_every_arrival_rule_of_action_0x97() {
        let Some(bytes) = testrom::load() else { return };
        let rom = nds_rom::Rom::parse(bytes).unwrap();
        let mut checked = 0;
        for path in [
            "data/scenario/triggerM01.bin",
            "data/scenario/triggerX01.bin",
        ] {
            let table = dq9_formats::Table::parse(rom.file_by_path(path).unwrap()).unwrap();
            for rule in dq9_formats::trigger::parse(&table).unwrap() {
                let Some(action) = rule.actions.iter().find(|a| a.code == 0x97) else {
                    continue;
                };
                // The two end-of-event rules (event 11, 21593 and 21595) clear the form just
                // before the warp to the Observatory, whose arrival rules clear it too.
                if rule.event != 3 {
                    continue;
                }
                let room = u16::try_from(rule.room).unwrap();
                for story in rule.from.0 * 1000 + rule.from.1..=rule.to.0 * 1000 + rule.to.1 {
                    let (chapter, step) = (story / 1000, story % 1000);
                    if !(1..=99).contains(&step) {
                        continue;
                    }
                    assert_eq!(
                        hero_winged(room, chapter, step),
                        action.arg == 1,
                        "{path}: room {room} at {chapter}.{step} after 0x97({})",
                        action.arg
                    );
                }
                checked += 1;
            }
        }
        assert!(checked >= 20, "the rules were found ({checked})");
    }
}
