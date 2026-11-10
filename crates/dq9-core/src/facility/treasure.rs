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

use core::ops::Range;

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

/// Opens `container` and returns what it gives. The caller sets
/// [`Treasure::flag`] on [`Open::Opened`], and clears the 700 from [`field_reset`] when the
/// field starts.
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
