//! Treasure containers: what opening one gives, and what remembers it.
//!
//! Source: the treasure service `func_ov017_021adcb0` (`0x021ae528`–`0x021ae58c` for the
//! flags, `0x021ade84` for the locks), the readers of
//! [`dq9_formats::facility::treasure`], and the wiki's *Treasure* and *Getting around*
//! (§ Locked chests).
//!
//! A **red chest** holds what its record says: gold, an item or a monster, or nothing. It
//! remembers its opening **for ever** (flag `0x212 + id`); a **blue chest, pot, barrel or
//! cupboard** draws from a random table at a rank its record names (a blue chest from
//! `randTBox`, the others from `randTTT`) and is full again at every game start: the field's
//! start clears the 700 flags from `0x79e` (`func_ov017_0218b688`, `0x0218c180`).
//!
//! A locked chest opens for no one here: the game counts the three keys (the ultimate opens
//! any, a thief's lock the thief's or the magic key, a magic lock the magic key) and says 41
//! and 43; this module returns [`Open::Locked`] and leaves the unlocking to the caller.
//!
//! Two **open questions**, marked where they matter:
//!
//! - The cartridge draws a container's contents **when the map loads**, one draw a container
//!   (`LoadZoneContainers`), so a draw sits in the stream before the open; here the draw is
//!   made at the open. The distribution is the same, the generator's position is not.
//! - The wiki names `func_02032370(100)` for the draw, which `re/encounter.md` reads as the C
//!   library's `rand`; the batch's contract says the **world** generator. This module takes a
//!   [`Random`] (the world's, `GetBTRandom`, `0x020741e0`) and draws
//!   [`Random::next_max`]`(100)` from it. **[Hypothesis]**.
//!
//! On the field the container is found by its record's position ([`nearest`]), in **world
//! units** (the loader scales them to `fx32` itself: `LootManager_CreateContainer`, EU
//! `0x0207baa0`, `Int << 12` and `Float × 4096.0`; the floor at a record's `(x, z)` is at its
//! `y`, `docs/re/facility.md`). The cartridge has no placement for the container in the map's
//! `.bmdj` (checked on `M01M07`, `M01M08` and `D01M01`; `LootableContainerManager` makes
//! them), so the port matches the hero against the position with the talk partner's range and
//! facing. The liaison is the map plus the position, as `docs/re/facility.md` records.

use core::ops::Range;
use std::collections::BTreeSet;
use std::f32::consts::{PI, TAU};

use dq9_formats::facility::treasure::{
    CONTAINER_FLAG_BASE, CONTAINER_FLAG_COUNT, Contents, Lock, RandomRow, RandomTable,
    RandomTreasure, Treasure,
};

use crate::battle::Random;

/// The monsters that pose as chests, weakest first: cannibox, mimic, Pandora's box (numbers 38
/// to 40 of the monster list). A draw of one is a battle, which this build does not start.
pub const MIMICS: Range<u16> = 38..41;

/// Whether `monster` is one of the three chests that come alive.
pub fn is_mimic(monster: u16) -> bool {
    MIMICS.contains(&monster)
}

/// Half-side of the square around a container where the hero can examine it, in world units:
/// the talk partner's range (`Talk_NearestNpc`, `talk::RANGE` = 1.5). **[Hypothesis]**: the
/// cartridge reaches a container as a "thing to examine", whose zone the treasure record does
/// not carry; the port uses the talk partner's range and the facing test, on the ground plane
/// (as `talk::nearest` does).
pub const EXAMINE_REACH: f32 = 1.5;

/// The container the hero can examine: the nearest record with a position, inside
/// [`EXAMINE_REACH`] on x and z and in front of the hero (within a right angle), earlier in
/// file order on a tie. A cupboard's record has no position and is never chosen (the cabinet
/// join is **[hypothesis]**, not modelled).
pub fn nearest<'a>(treasures: &'a [Treasure], hero: [f32; 3], yaw: f32) -> Option<&'a Treasure> {
    let facing = crate::field::angle_of_yaw(yaw);
    let mut best: Option<(f32, &'a Treasure)> = None;
    for treasure in treasures {
        let Some([x, _, z]) = treasure.position else {
            continue;
        };
        let (dx, dz) = (x - hero[0], z - hero[2]);
        if dx.abs() > EXAMINE_REACH || dz.abs() > EXAMINE_REACH {
            continue;
        }
        let error = (dx.atan2(dz) - facing).rem_euclid(TAU);
        if error.min(TAU - error) > PI / 2.0 {
            continue;
        }
        let distance = dx.hypot(dz);
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, treasure));
        }
    }
    best.map(|(_, treasure)| treasure)
}

/// What an opened container gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    Item(u16),
    Gold(u16),
    /// A monster: a battle request, or one of the three mimics (numbers 38 to 40).
    Monster(u16),
    Nothing,
}

/// The outcome of opening a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    Opened(Opened),
    /// Its flag is already set: a red chest never refills, and within a session neither does
    /// another container.
    AlreadyOpen,
    /// A thief's or a magic lock: the caller's keys decide (not modelled here).
    Locked(Lock),
}

/// The three random tables, `randTBox`, `randTD` and `randTTT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tables {
    chest: RandomTreasure,
    grotto: RandomTreasure,
    pot: RandomTreasure,
}

impl Tables {
    pub fn new(chest: RandomTreasure, grotto: RandomTreasure, pot: RandomTreasure) -> Self {
        Self { chest, grotto, pot }
    }

    /// The table a container of `which` draws from.
    pub fn table(&self, which: RandomTable) -> &RandomTreasure {
        match which {
            RandomTable::Chest => &self.chest,
            RandomTable::Grotto => &self.grotto,
            RandomTable::Pot => &self.pot,
        }
    }
}

/// The draw `func_02032370(100)` makes, against `rank`'s rows in file order: a `roll` below a
/// row's cumulative weight takes that row, past their total nothing.
pub fn draw(table: &RandomTreasure, rank: u16, roll: u8) -> Option<&RandomRow> {
    let rank = u8::try_from(rank).ok()?;
    let mut total = 0u16;
    for row in table.rank(rank) {
        total += u16::from(row.weight);
        if u16::from(roll) < total {
            return Some(row);
        }
    }
    None
}

/// What the caller knows before opening: the flag's state and the world generator.
#[derive(Debug)]
pub struct OpenState<'a> {
    /// Whether the container's flag (its [`Treasure::flag`]) is set.
    pub opened: bool,
    /// The world generator (`GetBTRandom`, `0x020741e0`) the tables draw from.
    pub random: &'a mut Random,
}

impl<'a> OpenState<'a> {
    pub fn new(opened: bool, random: &'a mut Random) -> Self {
        Self { opened, random }
    }
}

/// Opens `container` and returns what it gives, without remembering it:
/// [`open_and_remember`] is the stateful entry (sets [`Treasure::flag`] on [`Open::Opened`]),
/// and [`reset_field`] clears the 700 refill flags when the field starts.
pub fn open(container: &Treasure, tables: &Tables, state: OpenState<'_>) -> Open {
    if state.opened {
        return Open::AlreadyOpen;
    }
    if container.lock != Lock::None {
        return Open::Locked(container.lock);
    }
    let Some(which) = container.container.table() else {
        return Open::Opened(from_contents(container.contents, container.value));
    };
    // A drawing container's record names its rank and leaves what it holds to the table.
    let roll = state.random.next_max(100) as u8;
    match draw(tables.table(which), container.value, roll) {
        Some(row) => Open::Opened(from_contents(row.contents, row.value)),
        // Past the rank's weights: their chance of nothing (`randTTT` sums to 20 to 50).
        None => Open::Opened(Opened::Nothing),
    }
}

/// Opens `container` with [`open`] and, when it opens, sets [`Treasure::flag`] in `flags` (the
/// trigger state's global flags). A locked container and an open one set nothing. The caller
/// clears the 700 refill flags with [`reset_field`] when the field starts.
pub fn open_and_remember(
    flags: &mut BTreeSet<i32>,
    container: &Treasure,
    tables: &Tables,
    random: &mut Random,
) -> Open {
    let opened = flags.contains(&i32::from(container.flag()));
    let result = open(container, tables, OpenState::new(opened, random));
    if matches!(result, Open::Opened(_)) {
        flags.insert(i32::from(container.flag()));
    }
    result
}

/// The gift of a contents kind and its value.
fn from_contents(contents: Contents, value: u16) -> Opened {
    match contents {
        Contents::Nothing => Opened::Nothing,
        Contents::Gold => Opened::Gold(value),
        Contents::Item => Opened::Item(value),
        Contents::Monster => Opened::Monster(value),
    }
}

/// The flags the field's start clears: `func_0206dfe8(bank, 0x79e, 0x2bc)` — exactly the
/// container ids 0 to 699, which no red chest uses (`docs/re/facility.md`).
pub fn field_reset() -> Range<u16> {
    CONTAINER_FLAG_BASE..CONTAINER_FLAG_BASE + CONTAINER_FLAG_COUNT
}

/// Clears the refill flags ([`field_reset`]) from `flags`: the field's start
/// (`func_ov017_0218b688`, `0x0218c180`). A red chest's `0x212 + id` flag is left alone.
pub fn reset_field(flags: &mut BTreeSet<i32>) {
    let range = field_reset();
    flags.retain(|&flag| !(i32::from(range.start)..i32::from(range.end)).contains(&flag));
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq9_formats::facility::treasure::Container;

    /// A tagged table whose records hold one integer each, no strings.
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

    /// `randTBox` rank 1: an item of 60 and a gold of 40, in file order.
    fn tables() -> Tables {
        let chest = RandomTreasure::parse(&table(&[
            (0x6a, 2),
            (0x69, row(1, 2, 22000, 60)),
            (0x69, row(1, 1, 210, 40)),
        ]))
        .unwrap();
        let grotto = RandomTreasure::parse(&table(&[(0x6a, 0)])).unwrap();
        // A pot's table, its rank summing to 40: past that, nothing.
        let pot =
            RandomTreasure::parse(&table(&[(0x6a, 1), (0x69, row(2, 2, 22004, 40))])).unwrap();
        Tables::new(chest, grotto, pot)
    }

    fn container(container: Container, contents: Contents, id: u16, value: u16) -> Treasure {
        Treasure {
            number: 0,
            id,
            container,
            contents,
            lock: Lock::None,
            value,
            position: None,
            facing: None,
            cabinet: None,
        }
    }

    #[test]
    fn a_draw_walks_the_ranks_rows_in_file_order() {
        let tables = tables();
        let chest = tables.table(RandomTable::Chest);
        assert_eq!(draw(chest, 1, 0).unwrap().value, 22000);
        assert_eq!(draw(chest, 1, 59).unwrap().value, 22000);
        assert_eq!(draw(chest, 1, 60).unwrap().value, 210);
        assert_eq!(draw(chest, 1, 99).unwrap().value, 210);
        assert_eq!(draw(chest, 2, 0), None);
        // The pot's rank comes to 40 of a hundred: past it, nothing.
        let pot = tables.table(RandomTable::Pot);
        assert_eq!(draw(pot, 2, 39).unwrap().value, 22004);
        assert_eq!(draw(pot, 2, 40), None);
        assert_eq!(draw(pot, 3, 0), None);
    }

    fn open_with(container: &Treasure, tables: &Tables, opened: bool, seed: u64) -> Open {
        let mut random = Random::new(seed);
        open(container, tables, OpenState::new(opened, &mut random))
    }

    fn placed(container: Container, id: u16, position: [f32; 3]) -> Treasure {
        Treasure {
            position: Some(position),
            ..self::container(container, Contents::Nothing, id, 1)
        }
    }

    #[test]
    fn the_container_under_the_hero_is_the_nearest_faced_record() {
        use crate::field::yaw_of_angle;
        let pot = placed(Container::Pot, 1, [-0.5, 0.16, 1.0]);
        let barrel = placed(Container::Barrel, 2, [1.2, 0.16, 0.8]);
        let outside = placed(Container::BlueChest, 3, [2.0, 0.0, 2.0]);
        let behind = placed(Container::RedChest, 4, [-0.2, 0.0, -1.0]);
        let cupboard = Treasure {
            cabinet: Some(0),
            ..self::container(Container::Cupboard, Contents::Nothing, 5, 16)
        };
        assert_eq!(cupboard.position, None);
        let records = [pot, barrel, outside, behind, cupboard];
        // The hero at the origin, facing the game's angle 0 (+z): the pot is nearest.
        assert_eq!(
            nearest(&records, [0.0; 3], yaw_of_angle(0.0)).map(|t| t.id),
            Some(1)
        );
        // Facing -z, only the chest behind is in front.
        assert_eq!(
            nearest(&records, [0.0; 3], yaw_of_angle(PI)).map(|t| t.id),
            Some(4)
        );
        // Facing +x: the barrel is in front, the pot and the chest are not.
        assert_eq!(
            nearest(&records, [0.0; 3], yaw_of_angle(PI / 2.0)).map(|t| t.id),
            Some(2)
        );
    }

    #[test]
    fn a_container_remembers_its_open_and_the_field_start_refills_it() {
        let tables = tables();
        let pot = self::container(Container::Pot, Contents::Nothing, 288, 2);
        let mut flags = BTreeSet::new();
        let mut random = Random::new(7);
        assert!(matches!(
            open_and_remember(&mut flags, &pot, &tables, &mut random),
            Open::Opened(_)
        ));
        assert!(flags.contains(&(0x79e + 288)));
        assert_eq!(
            open_and_remember(&mut flags, &pot, &tables, &mut random),
            Open::AlreadyOpen
        );
        reset_field(&mut flags);
        assert!(!flags.contains(&(0x79e + 288)));
        assert!(matches!(
            open_and_remember(&mut flags, &pot, &tables, &mut random),
            Open::Opened(_)
        ));

        // A red chest's flag is outside the cleared range and survives the field's start.
        let red = self::container(Container::RedChest, Contents::Item, 7, 21291);
        let mut flags = BTreeSet::new();
        assert_eq!(
            open_and_remember(&mut flags, &red, &tables, &mut random),
            Open::Opened(Opened::Item(21291))
        );
        assert!(flags.contains(&(0x212 + 7)));
        reset_field(&mut flags);
        assert!(flags.contains(&(0x212 + 7)));
        assert_eq!(
            open_and_remember(&mut flags, &red, &tables, &mut random),
            Open::AlreadyOpen
        );
    }

    #[test]
    fn a_locked_container_refuses_and_sets_no_flag() {
        let tables = tables();
        let mut locked = self::container(Container::RedChest, Contents::Item, 7, 21291);
        let mut flags = BTreeSet::new();
        let mut random = Random::new(7);
        for lock in [Lock::Thief, Lock::Magic] {
            locked.lock = lock;
            assert_eq!(
                open_and_remember(&mut flags, &locked, &tables, &mut random),
                Open::Locked(lock)
            );
            assert!(flags.is_empty());
        }
    }

    #[test]
    fn an_empty_rank_gives_nothing_and_still_sets_the_flag() {
        let tables = tables();
        // randTTT holds rank 2 only: rank 3 draws nothing and spends the roll.
        let pot = self::container(Container::Pot, Contents::Nothing, 300, 3);
        let mut flags = BTreeSet::new();
        let mut random = Random::new(7);
        assert_eq!(
            open_and_remember(&mut flags, &pot, &tables, &mut random),
            Open::Opened(Opened::Nothing)
        );
        assert!(flags.contains(&(0x79e + 300)));
    }

    #[test]
    fn the_field_start_clears_only_the_700_refill_flags() {
        let mut flags: BTreeSet<i32> = (0x79e..0x79e + 700).collect();
        flags.extend([0x212, 0x219, 0x79e + 700]);
        reset_field(&mut flags);
        assert_eq!(flags, BTreeSet::from([0x212, 0x219, 0x79e + 700]));
    }

    #[test]
    fn a_red_chest_gives_what_its_record_says() {
        let tables = tables();
        let item = container(Container::RedChest, Contents::Item, 3, 21291);
        assert_eq!(
            open_with(&item, &tables, false, 7),
            Open::Opened(Opened::Item(21291))
        );
        let gold = container(Container::RedChest, Contents::Gold, 9, 210);
        assert_eq!(
            open_with(&gold, &tables, false, 7),
            Open::Opened(Opened::Gold(210))
        );
        let monster = container(Container::RedChest, Contents::Monster, 4, 38);
        assert_eq!(
            open_with(&monster, &tables, false, 7),
            Open::Opened(Opened::Monster(38))
        );
        let empty = container(Container::RedChest, Contents::Nothing, 4, 0);
        assert_eq!(
            open_with(&empty, &tables, false, 7),
            Open::Opened(Opened::Nothing)
        );
    }

    #[test]
    fn an_open_container_stays_open_and_a_locked_one_does_not() {
        let tables = tables();
        let chest = container(Container::RedChest, Contents::Item, 3, 21291);
        assert_eq!(open_with(&chest, &tables, true, 7), Open::AlreadyOpen);
        let mut locked = chest;
        locked.lock = Lock::Thief;
        assert_eq!(
            open_with(&locked, &tables, false, 7),
            Open::Locked(Lock::Thief)
        );
        locked.lock = Lock::Magic;
        assert_eq!(
            open_with(&locked, &tables, false, 7),
            Open::Locked(Lock::Magic)
        );
    }

    /// A blue chest draws from `randTBox`: the item at 60 of a hundred, the gold at 40, and the
    /// same seed gives the same draw.
    #[test]
    fn a_blue_chest_draws_from_the_chest_table() {
        let tables = tables();
        let blue = container(Container::BlueChest, Contents::Nothing, 566, 1);
        let first = open_with(&blue, &tables, false, 12345);
        assert_eq!(first, open_with(&blue, &tables, false, 12345));
        assert!(matches!(
            first,
            Open::Opened(Opened::Item(22000)) | Open::Opened(Opened::Gold(210))
        ));
        // A rank the table does not hold draws nothing.
        let mut other = blue;
        other.value = 4;
        assert_eq!(
            open_with(&other, &tables, false, 12345),
            Open::Opened(Opened::Nothing)
        );
    }

    /// A pot's rank sums to 40 of a hundred: past the rows, nothing, and the roll is spent.
    #[test]
    fn a_pot_reads_the_pot_table_and_nothing_past_its_weights() {
        let tables = tables();
        let pot = container(Container::Pot, Contents::Nothing, 288, 2);
        let mut items = 0;
        let mut empties = 0;
        for seed in 0..64 {
            match open_with(&pot, &tables, false, seed) {
                Open::Opened(Opened::Item(22004)) => items += 1,
                Open::Opened(Opened::Nothing) => empties += 1,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(items > 0 && empties > 0, "items {items}, empties {empties}");
    }

    #[test]
    fn the_three_mimics_are_monsters_38_to_40() {
        assert!(is_mimic(38));
        assert!(is_mimic(39));
        assert!(is_mimic(40));
        assert!(!is_mimic(37));
        assert!(!is_mimic(41));
        assert_eq!(MIMICS, 38..41);
    }

    #[test]
    fn the_field_start_clears_the_700_non_red_flags() {
        let reset = field_reset();
        assert_eq!(reset.start, 0x79e);
        assert_eq!(reset.end, 0x79e + 700);
        assert_eq!(reset.count(), 700);
    }
}
