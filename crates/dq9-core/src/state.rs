//! Game state, initialised from a DS save or from the new-game defaults. No I/O.
//!
//! Only what a save restores and the field needs first is modelled: current map, where the
//! party stands, scenario position, day clock and event flags. Where each value comes from is
//! in `docs/re/save.md`.
//!
//! port: 0x02010250, 0x02010298, 0x02010364, 0x0201036c, 0x02010374

use dq9_formats::map::StartPoint;
use dq9_formats::save::{EVENT_FLAGS_SIZE, EventFlags, Save, Slot};

use crate::party::Party;

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
    /// Neither slot is a valid normal save.
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
            Self::NoValidSlot => "the save has no valid normal slot",
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

    /// The state of the first valid normal slot (`Save::current_slot`).
    pub fn from_save(save: &Save) -> Result<Self, StateError> {
        let slot = save
            .current_slot()
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
