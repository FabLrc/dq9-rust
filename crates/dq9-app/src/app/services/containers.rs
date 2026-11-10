//! The field's treasure containers: the current map's records, the three random tables of
//! `data/scenario/treasure.nsarc`, and what an examine gives.
//!
//! The rules are `dq9_core::facility::treasure` (flags, locks, the draw and its tables); this
//! module is the application side: it reads `<map>.bin` from the archive, finds the container
//! the hero faces by its record's position (`treasure::nearest`), opens it with
//! `treasure::open_and_remember`, and applies the gift to the party.
//!
//! The draw is made here, at the open, from the caller's [`Random`]: [`crate::app::App`] passes
//! the **world** generator (`GetBTRandom`, `0x020741e0`), the generator the core rule names
//! (**[hypothesis]**: `func_02032370` is the C library's `rand` according to
//! `docs/re/encounter.md`; which one the cartridge drains is open). The screenshot preview
//! (`crate::talk`) has no world and seeds a generator with 0, logged as such.
//!
//! Not modelled, and reported instead:
//! - A **monster** row (the three mimics, numbers 38 to 40): [`Taken::Monster`] is logged and
//!   shown, no battle is started and nothing is given (the row opens nothing), and the
//!   container's flag is cleared so the unmodelled battle is not swallowed. The container
//!   battle's stage and event request are not read.
//! - A **locked** chest: [`Taken::Locked`] is shown; the keys (the ultimate, the thief's, the
//!   magic) are not modelled.
//! - A **cupboard**: its record has three values and no position; the cabinet join is
//!   **[hypothesis]** in `docs/re/facility.md`, so it is counted in [`Containers::unplaced`]
//!   and never reached.
//! - The container's **model**: the map's `.bmdj` has no placement at the treasure position
//!   (checked on `M01M07`, `M01M08` and `D01M01`); `LootableContainerManager` creates it at
//!   run time. The port draws none, so there is no placement to hide and the flag alone stops
//!   a second open.
//! - The original's **message text**: the window shows the placeholder lines of
//!   [`Taken::text`], not the event script's words.
//! - A bag with **no room**: the item is reported as not stored; whether the cartridge keeps
//!   the container closed is not read (the flag stays set).

use anyhow::{Context as _, Result};
use dq9_core::battle::Random;
use dq9_core::facility::treasure::{self, Open, Opened, Tables};
use dq9_core::party::Party;
use dq9_core::trigger::State;
use dq9_formats::facility::treasure::{Lock, RandomTable, RandomTreasure, Treasure, TreasureMap};
use glam::Vec3;
use nds_rom::{Narc, Rom};

/// The treasure archive: one `<map>.bin` per map, and the three random tables
/// (`docs/re/facility.md`, § Treasure).
pub const PATH: &str = "data/scenario/treasure.nsarc";

/// The current map's treasure records and the archive's tables.
#[derive(Debug, Default)]
pub struct Containers {
    records: Vec<Treasure>,
    tables: Option<Tables>,
    /// Records with no position (a cupboard's cabinet join): not reachable from a position.
    unplaced: usize,
}

impl Containers {
    /// Reads `<map>.bin` of `treasure.nsarc` and its three random tables. A map without a
    /// member has no containers (not an error); a malformed archive, member or table is.
    pub fn load(rom: &[u8], map: &str) -> Result<Self> {
        let rom = Rom::parse(rom).context("parsing the ROM")?;
        let data = rom
            .file_by_path(PATH)
            .with_context(|| format!("reading {PATH}"))?;
        let narc = Narc::parse(data).with_context(|| format!("parsing {PATH}"))?;
        let index = |name: &str| {
            (0..narc.len() as u16)
                .find(|&i| narc.name(i) == Some(name))
                .with_context(|| format!("{name} in {PATH}"))
        };
        let [chest, grotto, pot] = RandomTable::ALL.map(|which| {
            let name = which.member();
            let data = narc.file(index(name)?)?;
            RandomTreasure::parse(data).with_context(|| format!("{name} of {PATH}"))
        });
        let tables = Tables::new(chest?, grotto?, pot?);
        let member = TreasureMap::member(map);
        let records = match (0..narc.len() as u16).find(|&i| narc.name(i) == Some(member.as_str()))
        {
            Some(i) => TreasureMap::parse(narc.file(i)?)
                .with_context(|| format!("{member} of {PATH}"))?
                .treasures()
                .to_vec(),
            None => Vec::new(),
        };
        let unplaced = records.iter().filter(|t| t.position.is_none()).count();
        Ok(Self {
            records,
            tables: Some(tables),
            unplaced,
        })
    }

    /// The records of the loaded map, in file order.
    pub fn records(&self) -> &[Treasure] {
        &self.records
    }

    /// The records that carry no position (cupboards), never reachable by position.
    pub fn unplaced(&self) -> usize {
        self.unplaced
    }

    /// The container the hero faces, if any ([`treasure::nearest`]).
    pub fn nearest(&self, hero: Vec3, yaw: f32) -> Option<&Treasure> {
        treasure::nearest(&self.records, [hero.x, hero.y, hero.z], yaw)
    }

    /// Opens the container the hero faces: its flag is set on an open by
    /// [`treasure::open_and_remember`], and the draw comes from `random`. `None` when no
    /// container is reachable (or the tables failed to load: [`Containers::load`] would have
    /// returned the error).
    pub fn open(
        &self,
        hero: Vec3,
        yaw: f32,
        state: &mut State,
        random: &mut Random,
    ) -> Option<(Treasure, Open)> {
        let tables = self.tables.as_ref()?;
        let container = *self.nearest(hero, yaw)?;
        let result = treasure::open_and_remember(&mut state.flags, &container, tables, random);
        // A mimic row is a battle this build does not start; the container is left as it was
        // (its flag is cleared) rather than swallowing the open with no battle.
        if matches!(result, Open::Opened(Opened::Monster(_))) {
            state.flags.remove(&i32::from(container.flag()));
        }
        Some((container, result))
    }

    /// A line for the load log: what the map holds and what cannot be reached.
    pub fn summary(&self) -> String {
        format!(
            "containers: {} record(s), {} without a position (cupboard's cabinet not modelled)",
            self.records.len(),
            self.unplaced()
        )
    }
}

/// What an [`Open`] gives, before it is applied to the party.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    Item(u16),
    Gold(u16),
    /// A mimic: the battle request is not modelled.
    Monster(u16),
    Nothing,
    Locked(Lock),
    AlreadyOpen,
}

impl Taken {
    pub const fn of(open: Open) -> Self {
        match open {
            Open::Opened(Opened::Item(id)) => Self::Item(id),
            Open::Opened(Opened::Gold(gold)) => Self::Gold(gold),
            Open::Opened(Opened::Monster(monster)) => Self::Monster(monster),
            Open::Opened(Opened::Nothing) => Self::Nothing,
            Open::Locked(lock) => Self::Locked(lock),
            Open::AlreadyOpen => Self::AlreadyOpen,
        }
    }

    /// Applies the item or the gold to `party`; an item also raises its obtained flag
    /// ([`State::obtain`]). Returns whether the item found room in the bag (`false` only for
    /// [`Taken::Item`]).
    pub fn give(&self, party: &mut Party, state: &mut State) -> bool {
        match *self {
            Self::Gold(gold) => {
                party.gold = party.gold.saturating_add(u32::from(gold));
                true
            }
            Self::Item(id) => {
                let stored = party.bag.add(id);
                if stored {
                    state.obtain(i32::from(id), 1);
                }
                stored
            }
            _ => true,
        }
    }

    /// The dev message line (not the original's wording; the event scripts' text is not read).
    pub fn line(&self) -> String {
        match *self {
            Self::Item(id) => format!("item {id} taken"),
            Self::Gold(gold) => format!("{gold} gold taken"),
            Self::Monster(monster) => {
                format!("monster {monster}: a battle is not modelled, the container stays")
            }
            Self::Nothing => "nothing".to_owned(),
            Self::Locked(Lock::Thief) => "locked: a thief's lock, the keys are not modelled".into(),
            Self::Locked(Lock::Magic) => "locked: a magic lock, the keys are not modelled".into(),
            Self::Locked(Lock::None) => "locked: the record has no lock".to_owned(),
            Self::AlreadyOpen => "already opened".to_owned(),
        }
    }

    /// The placeholder window text (ASCII; the font has its glyphs).
    pub fn text(&self) -> Vec<u8> {
        match *self {
            Self::Item(id) => format!("Found item {id}."),
            Self::Gold(gold) => format!("Found {gold} gold."),
            Self::Monster(monster) => format!("Monster {monster}: no battle here."),
            Self::Nothing => "Nothing here.".to_owned(),
            Self::Locked(Lock::Thief) => "Locked: the thief's key is not modelled.".to_owned(),
            Self::Locked(Lock::Magic) => "Locked: the magic key is not modelled.".to_owned(),
            Self::Locked(Lock::None) => "Locked: this container refuses.".to_owned(),
            Self::AlreadyOpen => "Already opened.".to_owned(),
        }
        .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_core::field::yaw_of_angle;
    use dq9_formats::facility::treasure::Container;

    /// The European ROM, else the test skips itself.
    fn rom() -> Option<Vec<u8>> {
        testrom::load().map(<[u8]>::to_vec)
    }

    fn containers(rom: &[u8], map: &str) -> Containers {
        Containers::load(rom, map).unwrap_or_else(|e| panic!("{map}: {e:#}"))
    }

    /// The hero just south of `position` (facing +z, the field's angle 0).
    fn before(position: [f32; 3]) -> Vec3 {
        Vec3::new(position[0], position[1], position[2] - 0.9)
    }

    #[test]
    fn the_village_pots_load_and_the_nearest_one_opens_once_until_the_field_start() {
        let Some(rom) = rom() else { return };
        let containers = containers(&rom, "M01M04");
        assert_eq!(containers.records().len(), 2);
        assert_eq!(containers.unplaced(), 0);
        let pot = *containers.records().first().unwrap();
        assert_eq!(pot.container, Container::Pot);
        assert_eq!((pot.id, pot.value), (288, 6));
        let hero = before(pot.position.unwrap());
        assert_eq!(
            containers.nearest(hero, yaw_of_angle(0.0)).map(|t| t.id),
            Some(288)
        );

        let mut state = dq9_core::field::initial_state(&Default::default());
        let mut random = Random::new(7);
        let (opened, open) = containers
            .open(hero, yaw_of_angle(0.0), &mut state, &mut random)
            .expect("the pot is reachable");
        assert_eq!(opened.id, 288);
        let taken = Taken::of(open);
        assert!(matches!(
            taken,
            Taken::Item(_) | Taken::Gold(_) | Taken::Nothing
        ));
        assert!(state.flags.contains(&(0x79e + 288)));
        // A second examine in the same session refuses, and the flag says why.
        assert_eq!(
            containers
                .open(hero, yaw_of_angle(0.0), &mut state, &mut random)
                .map(|(_, open)| Taken::of(open)),
            Some(Taken::AlreadyOpen)
        );
        // The field's start clears the refill flag: the pot is full again.
        treasure::reset_field(&mut state.flags);
        assert!(!state.flags.contains(&(0x79e + 288)));
        assert!(matches!(
            containers
                .open(hero, yaw_of_angle(0.0), &mut state, &mut random)
                .map(|(_, open)| Taken::of(open)),
            Some(Taken::Item(_) | Taken::Gold(_) | Taken::Nothing)
        ));
    }

    #[test]
    fn a_red_chest_gives_its_item_and_stays_open_after_the_field_start() {
        let Some(rom) = rom() else { return };
        let containers = containers(&rom, "D01M01");
        let chest = *containers.records().first().unwrap();
        assert_eq!(chest.container, Container::RedChest);
        assert_eq!((chest.id, chest.value), (7, 21291));
        let hero = before(chest.position.unwrap());
        let mut state = dq9_core::field::initial_state(&Default::default());
        let mut random = Random::new(7);
        let (_, open) = containers
            .open(hero, yaw_of_angle(0.0), &mut state, &mut random)
            .expect("the chest is reachable");
        let taken = Taken::of(open);
        assert_eq!(taken, Taken::Item(21291));
        let mut party = Party::new_game();
        let before = party.bag.count(21291);
        assert!(taken.give(&mut party, &mut state));
        assert_eq!(party.bag.count(21291), before + 1);
        assert!(state.flags.contains(&(0x212 + 7)));
        treasure::reset_field(&mut state.flags);
        assert!(state.flags.contains(&(0x212 + 7)));
        assert_eq!(
            containers
                .open(hero, yaw_of_angle(0.0), &mut state, &mut random)
                .map(|(_, open)| Taken::of(open)),
            Some(Taken::AlreadyOpen)
        );
    }

    #[test]
    fn gold_is_added_to_the_purse() {
        let Some(rom) = rom() else { return };
        let containers = containers(&rom, "D01M03");
        let chest = *containers
            .records()
            .iter()
            .find(|t| t.id == 9)
            .expect("the 210-gold chest");
        assert_eq!(
            chest.contents,
            dq9_formats::facility::treasure::Contents::Gold
        );
        let hero = before(chest.position.unwrap());
        let mut state = dq9_core::field::initial_state(&Default::default());
        let mut random = Random::new(7);
        let (_, open) = containers
            .open(hero, yaw_of_angle(0.0), &mut state, &mut random)
            .expect("the chest is reachable");
        let taken = Taken::of(open);
        assert_eq!(taken, Taken::Gold(210));
        let mut party = Party::new_game();
        party.gold = 5;
        assert!(taken.give(&mut party, &mut state));
        assert_eq!(party.gold, 215);
    }

    #[test]
    fn a_locked_chest_refuses_and_sets_no_flag() {
        let Some(rom) = rom() else { return };
        // C01M16 holds one of the six thief-locked chests (docs/re/facility.md).
        let containers = containers(&rom, "C01M16");
        let locked = *containers
            .records()
            .iter()
            .find(|t| t.lock != Lock::None)
            .expect("a locked chest");
        let hero = before(locked.position.unwrap());
        let mut state = dq9_core::field::initial_state(&Default::default());
        let mut random = Random::new(7);
        let (_, open) = containers
            .open(hero, yaw_of_angle(0.0), &mut state, &mut random)
            .expect("the chest is reachable");
        assert_eq!(Taken::of(open), Taken::Locked(locked.lock));
        assert!(!state.flags.contains(&i32::from(locked.flag())));
    }

    #[test]
    fn a_rank_past_its_weights_gives_nothing_from_the_real_pot_table() {
        let Some(rom) = rom() else { return };
        let containers = containers(&rom, "M01M04");
        let tables = containers.tables.as_ref().expect("the tables");
        let pot = tables.table(RandomTable::Pot);
        // The real randTTT has short ranks: a roll of 99 past a rank's total draws nothing.
        let short = (1..=20u16)
            .find(|&rank| {
                treasure::draw(pot, rank, 0).is_some() && treasure::draw(pot, rank, 99).is_none()
            })
            .expect("a rank whose weights stop before 100");
        assert!(treasure::draw(pot, short, 99).is_none(), "rank {short}");
    }

    #[test]
    fn a_cupboard_has_no_position_and_is_counted_not_reached() {
        let Some(rom) = rom() else { return };
        let containers = containers(&rom, "M01M03");
        assert_eq!(containers.records().len(), 2);
        assert_eq!(containers.unplaced(), 2);
        assert!(
            containers
                .records()
                .iter()
                .all(|t| t.container == Container::Cupboard && t.position.is_none())
        );
        assert_eq!(
            containers.nearest(Vec3::ZERO, yaw_of_angle(0.0)),
            None,
            "no cupboard is reachable by a position"
        );
    }

    /// A tagged table whose records hold one integer each, no strings (as the core tests).
    fn table(records: &[(u16, u32)]) -> Vec<u8> {
        let mut body = Vec::new();
        for &(kind, value) in records {
            body.extend(kind.to_le_bytes());
            body.push(1);
            body.push(0x01);
            body.extend(value.to_le_bytes());
        }
        let pool = (0x10 + body.len()).next_multiple_of(16);
        let mut out = (records.len() as u32).to_le_bytes().to_vec();
        out.extend((pool as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(&body);
        out.resize(pool, 0xff);
        out
    }

    fn row(rank: u32, contents: u32, value: u32, weight: u32) -> u32 {
        rank << 26 | contents << 23 | value << 7 | weight
    }

    /// The chest table has one rank, all a cannibox: the unmodelled battle leaves the blue
    /// chest unopened and its flag clear.
    #[test]
    fn a_mimic_row_leaves_the_container_unopened() {
        use dq9_formats::facility::treasure::Contents;

        let chest = RandomTreasure::parse(&table(&[(0x6a, 1), (0x69, row(1, 3, 38, 100))]))
            .expect("a one-row chest table");
        let empty = || RandomTreasure::parse(&table(&[(0x6a, 0)])).expect("an empty table");
        let tables = Tables::new(chest, empty(), empty());
        let blue = Treasure {
            number: 0,
            id: 566,
            container: Container::BlueChest,
            contents: Contents::Nothing,
            lock: Lock::None,
            value: 1,
            position: Some([0.0, 0.0, 1.0]),
            facing: None,
            cabinet: None,
        };
        let containers = Containers {
            records: vec![blue],
            tables: Some(tables),
            unplaced: 0,
        };
        let mut state = dq9_core::field::initial_state(&Default::default());
        let mut random = Random::new(0);
        let (_, open) = containers
            .open(Vec3::ZERO, yaw_of_angle(0.0), &mut state, &mut random)
            .expect("the blue chest is reachable");
        assert_eq!(Taken::of(open), Taken::Monster(38));
        assert!(!state.flags.contains(&(0x79e + 566)));
    }

    /// A record's position is a **world** position, not fixed point: the map's collision floor
    /// at the record's (x, z) is at its y. `LootManager_CreateContainer` (EU `0x0207baa0`)
    /// scales the values itself — a float by `[0x0207bcd0]` = 4096.0, an integer by `<< 12` —
    /// so the reader must not divide them. A reader that did would move the record by an
    /// order of magnitude and fail the floor test.
    #[test]
    fn a_record_position_is_the_world_position_of_its_floor() {
        let Some(rom) = rom() else { return };
        let cases = [
            ("M01M08", 25), // blue chest, floats
            ("M01M07", 21), // pot, floats
            ("D01M01", 29), // red chest, floats
            ("M01M04", 18), // pot, floats
            ("C01M16", 72), // locked red chest, floats
        ];
        for (map, number) in cases {
            let mut app = self::app_on(&rom, map);
            let record = *app
                .containers
                .records()
                .iter()
                .find(|t| t.number == number)
                .unwrap_or_else(|| panic!("{map} #{number}"));
            let [x, y, z] = record.position.expect("a placed record");
            if map == "M01M08" {
                assert_eq!([x, y, z], [4.32, 1.17, -3.02], "the raw float coordinates");
            }
            app.place_hero(x, z)
                .unwrap_or_else(|e| panic!("{map} #{number}: {e:#}"));
            let floor = app.hero.expect("the hero").position().y;
            assert!(
                (floor - y).abs() < 0.02,
                "{map} #{number}: record y {y}, floor at its x,z {floor}"
            );
            // The examine join uses that same world position: a hero just before the record
            // finds a container in front of it.
            app.place_hero(x, z - 0.9)
                .expect("ground before the record");
            let hero = app.hero.expect("the hero");
            assert!(
                app.containers
                    .nearest(hero.position(), hero.yaw())
                    .is_some(),
                "{map} #{number}: no container under the hero"
            );
        }
    }

    fn app_on(rom: &[u8], map: &str) -> crate::app::App {
        use crate::app::Options;
        use crate::screens::Layout;

        let options = Options {
            map: Some(map.to_owned()),
            save: None,
            time: None,
            clock: None,
            layout: Layout::default(),
            mute: true,
            record_sound: false,
            trace_inputs: false,
            event: None,
            story: dq9_formats::cast::Story::default(),
            story_override: [None; 3],
            new_game: false,
            battle: None,
            play: Default::default(),
        };
        crate::app::App::new_headless(options, Some(rom.to_vec()))
    }

    /// The application's own path: the map load reads the containers, the action takes the
    /// gold, and the red chest's flag refuses the second examine.
    #[test]
    fn the_app_takes_the_gold_and_refuses_the_red_chest_again() {
        let Some(rom) = rom() else { return };
        let mut app = self::app_on(&rom, "D01M03");
        assert_eq!(
            app.containers.records().len(),
            2,
            "the map load reads D01M03.bin"
        );
        app.place_hero(0.01, 21.2).expect("ground before the chest");
        app.party.gold = 5;
        assert!(app.examine_container());
        assert_eq!(app.party.gold, 215);
        assert!(app.state.flags.contains(&(0x212 + 9)));
        // The same session's second examine is refused: the gold is not taken twice.
        assert!(app.examine_container());
        assert_eq!(app.party.gold, 215);
    }
}
