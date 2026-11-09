//! Game state, initialised from a DS save or from the new-game defaults. No I/O.
//!
//! Only what a save restores and the field needs first is modelled: current map, where the
//! party stands, scenario position, day clock and event flags. Where each value comes from is
//! in `docs/re/save.md`.
//!
//! port: 0x02010250, 0x02010298, 0x02010364, 0x0201036c, 0x02010374

use dq9_formats::map::StartPoint;
use dq9_formats::save::{EVENT_FLAGS_SIZE, EventFlags, Save, Slot};

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
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::NoValidSlot => "the save has no valid normal slot",
            Self::BadEventBlock => "the save's event block has the wrong size",
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
    events: Vec<u8>,
}

impl GameState {
    /// A new game: prologue map, scenario 1/1/1 and the day's start, the clock running
    /// (`0x0200f3a4`; the clock's initial 210 s from the wiki's *Time-Of-Day*). The event
    /// block is empty **[unknown]**: the game's initial tracks are not read.
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
