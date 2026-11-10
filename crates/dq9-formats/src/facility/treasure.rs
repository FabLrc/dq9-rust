//! Treasure containers: the members of `data/scenario/treasure.nsarc` and the three random
//! tables beside them.
//!
//! Every map with treasure has one member, `<map>.bin`, a tagged data table (wiki DQIX
//! *Treasure*; `docs/re/facility.md`). A `0x66` record holds **the game-wide number of the
//! file's first treasure**; each `0x67` record is one treasure and its **value 0** packs the
//! container's id in its high half and, in its low half, the item, the gold or the rank to draw
//! at. Value 1 is the **kind word**, read by `LootManager_CreateContainer`:
//!
//! | bits | meaning |
//! |---|---|
//! | 4–6 | the container: 0 a red chest, 1 a pot, 2 a barrel, 3 a cupboard, 4 a blue chest |
//! | 2–3 | what it holds: 0 nothing, 1 gold, 2 an item, 3 a monster |
//! | 0–1 | the lock: 1 a thief's lock, 2 a magic lock |
//!
//! A record has 3 values (position-less: a cabinet's treasure, value 2 its room's cabinet less
//! one — **[hypothesis]**), 5 (x, y, z) or 6 (and a facing, INFERRED radians). A position and a
//! facing are whole numbers or floats, one record may mix the two.
//!
//! `randTBox`, `randTD` and `randTTT` are the random tables: a `0x6A` record holds the row
//! count and each `0x69` row is one word (bits 26–31 rank, 23–25 what it gives, 7–22 the gold,
//! the item's id or the monster's number, 0–6 a weight). A blue chest draws from `randTBox` at
//! its rank, a pot, a barrel or a cupboard from `randTTT`; red chests do not draw.

use nds_rom::{Error, Result};

use crate::table::{Record, Table, Value};

/// A red chest remembers its opening for ever: flag `0x212 + id` of the game-wide bank
/// (`func_ov017_021adcb0`, `0x021ae528`–`0x021ae58c`; wiki *Treasure*, § What is remembered).
pub const RED_CHEST_FLAG_BASE: u16 = 0x212;
/// Everything else remembers it in `0x79e + id`, **cleared at the field's start**
/// (`func_ov017_0218b688`, `0x0218c180`: `func_0206dfe8(bank, 0x79e, 0x2bc)`).
pub const CONTAINER_FLAG_BASE: u16 = 0x79e;
/// The flags the field's start clears, exactly the ids 0 to 699 of the non-red containers.
pub const CONTAINER_FLAG_COUNT: u16 = 0x2bc;

/// Tagged record kinds of a treasure member and of the random tables.
const KIND_DATE: u16 = 0x65;
const KIND_DATE_SHORT: u16 = 0x64;
const KIND_FIRST_NUMBER: u16 = 0x66;
const KIND_TREASURE: u16 = 0x67;
const KIND_ROW_COUNT: u16 = 0x6a;
const KIND_ROW: u16 = 0x69;

/// The container, bits 4 to 6 of the kind word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Container {
    /// A red chest: its contents are in the record, and it never refills.
    RedChest,
    /// A pot; it draws from `randTTT` and is full again at every game start.
    Pot,
    /// A barrel, likewise.
    Barrel,
    /// A cupboard, likewise; its treasure has no position.
    Cupboard,
    /// A blue chest; it draws from `randTBox`.
    BlueChest,
}

impl Container {
    /// Every container, by its bits.
    pub const ALL: [Self; 5] = [
        Self::RedChest,
        Self::Pot,
        Self::Barrel,
        Self::Cupboard,
        Self::BlueChest,
    ];

    /// The container of bits 4 to 6 of a kind word; `None` above 4 (**not established**: no
    /// record of the cartridge carries one).
    pub fn from_bits(bits: u16) -> Option<Self> {
        match bits {
            0 => Some(Self::RedChest),
            1 => Some(Self::Pot),
            2 => Some(Self::Barrel),
            3 => Some(Self::Cupboard),
            4 => Some(Self::BlueChest),
            _ => None,
        }
    }

    /// Bits 4 to 6 of the kind word.
    pub fn bits(self) -> u16 {
        self as u16
    }

    /// Whether the container draws its contents from a random table at every load of the map
    /// (`LoadZoneContainers`): everything but a red chest does.
    pub fn draws(self) -> bool {
        self != Self::RedChest
    }

    /// The random table a container of this kind draws from, if any.
    pub fn table(self) -> Option<RandomTable> {
        match self {
            Self::RedChest => None,
            Self::BlueChest => Some(RandomTable::Chest),
            Self::Pot | Self::Barrel | Self::Cupboard => Some(RandomTable::Pot),
        }
    }

    /// The base of the flag that remembers the container opened: `0x212` for a red chest,
    /// `0x79e` for the rest.
    pub fn flag_base(self) -> u16 {
        if self == Self::RedChest {
            RED_CHEST_FLAG_BASE
        } else {
            CONTAINER_FLAG_BASE
        }
    }
}

/// What a container holds, bits 2 to 3 of the kind word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Contents {
    Nothing,
    Gold,
    Item,
    Monster,
}

impl Contents {
    /// The contents of bits 2 to 3; `None` is not a value the field takes.
    pub fn from_bits(bits: u16) -> Option<Self> {
        match bits {
            0 => Some(Self::Nothing),
            1 => Some(Self::Gold),
            2 => Some(Self::Item),
            3 => Some(Self::Monster),
            _ => None,
        }
    }
}

/// The lock, bits 0 and 1 of the kind word (wiki *Treasure*; *Getting around*, § Locked chests).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lock {
    /// None; the six locked chests of the cartridge are all a thief's lock.
    None,
    /// A thief's lock: the thief's key or the magic key opens it.
    Thief,
    /// A magic lock: only the magic key.
    Magic,
}

impl Lock {
    /// The lock of bits 0 and 1; `None` on the value 3, which no record carries.
    pub fn from_bits(bits: u16) -> Option<Self> {
        match bits {
            0 => Some(Self::None),
            1 => Some(Self::Thief),
            2 => Some(Self::Magic),
            _ => None,
        }
    }
}

/// One treasure, a `0x67` record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Treasure {
    /// The game-wide number: the file's first plus the record's place (0 to 847, no gap).
    pub number: u16,
    /// The container's id, the high half of value 0 (0 to 206 on red chests, 0 to 699 else).
    pub id: u16,
    pub container: Container,
    pub contents: Contents,
    pub lock: Lock,
    /// The low half of value 0: the item's id, the gold, or the random table's rank.
    pub value: u16,
    /// Values 2 to 4, the **world** position (the map's units, those of its placements and
    /// collision); `LootManager_CreateContainer` converts each to fx32 itself, `<< 12` for an
    /// integer and ×4096.0 for a float, so this reader keeps them as they are. `None` for a
    /// cupboard's treasure, which has no record of its own place.
    pub position: Option<[f32; 3]>,
    /// Value 5, INFERRED radians; only the six-value records have one.
    pub facing: Option<f32>,
    /// Value 2 of a three-value record: the room's cabinet, less one. **[Hypothesis]** — it
    /// matches on 45 of the 89 maps that have such records (wiki *Treasure*, § Layout).
    pub cabinet: Option<u16>,
}

impl Treasure {
    /// The flag that remembers this container opened (`func_ov017_021adcb0`).
    ///
    /// A red chest's flag is never cleared; another container's is, with all 700 others, at
    /// the field's start ([`CONTAINER_FLAG_BASE`], [`CONTAINER_FLAG_COUNT`]).
    pub fn flag(&self) -> u16 {
        self.container.flag_base() + self.id
    }

    /// The kind word, recomposed (for a caller that logs what it read).
    pub fn kind(&self) -> u16 {
        (self.container.bits() << 4) | ((self.contents as u16) << 2) | (self.lock as u16)
    }

    fn parse(record: &Record<'_>, number: u16) -> Result<Self> {
        let field = |i: usize| -> Result<Value<'_>> {
            record
                .values
                .get(i)
                .copied()
                .ok_or(Error::Malformed("treasure record field"))
        };
        let number_word = integer(field(0)?, "treasure id and value")?;
        let kind = u16::try_from(integer(field(1)?, "treasure kind")?)
            .map_err(|_| Error::Malformed("treasure kind above 0xffff"))?;
        let bits = |shift: u16| (kind >> shift) & 3;
        let container_bits = (kind >> 4) & 7;
        let container = Container::from_bits(container_bits)
            .ok_or(Error::Malformed("treasure container above 4"))?;
        let contents =
            Contents::from_bits(bits(2)).ok_or(Error::Malformed("treasure contents above 3"))?;
        let lock = Lock::from_bits(kind & 3).ok_or(Error::Malformed("treasure lock above 2"))?;
        let mut treasure = Self {
            number,
            id: (number_word >> 16) as u16,
            container,
            contents,
            lock,
            value: number_word as u16,
            position: None,
            facing: None,
            cabinet: None,
        };
        match record.values.len() {
            3 => treasure.cabinet = Some(integer(field(2)?, "treasure cabinet")? as u16),
            5 | 6 => {
                treasure.position = Some([
                    coordinate(field(2)?)?,
                    coordinate(field(3)?)?,
                    coordinate(field(4)?)?,
                ]);
                if record.values.len() == 6 {
                    treasure.facing = Some(coordinate(field(5)?)?);
                }
            }
            _ => return Err(Error::Malformed("treasure record is not 3, 5 or 6 values")),
        }
        Ok(treasure)
    }
}

/// One member of `treasure.nsarc`: `<map>.bin`.
#[derive(Debug, Clone, PartialEq)]
pub struct TreasureMap {
    /// The game-wide number of the first record (`0x66`). Every span runs on without a gap.
    pub first: u16,
    treasures: Vec<Treasure>,
}

impl TreasureMap {
    /// Parses one member.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table, a record kind a treasure file does not have, a
    /// treasure record that is not 3, 5 or 6 values, a field of the wrong type, or no `0x66`
    /// record (every one of the 268 members has one).
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut first = None;
        let mut treasures = Vec::new();
        for record in &table.records {
            match record.kind {
                KIND_FIRST_NUMBER => {
                    let value = record
                        .values
                        .first()
                        .copied()
                        .ok_or(Error::Malformed("treasure 0x66 record without a value"))?;
                    first = Some(
                        u16::try_from(integer(value, "treasure first number")?)
                            .map_err(|_| Error::Malformed("treasure first number above 0xffff"))?,
                    );
                }
                KIND_TREASURE => {
                    let number = first.ok_or(Error::Malformed(
                        "treasure record before the file's first number",
                    ))?;
                    let number = number
                        .checked_add(treasures.len() as u16)
                        .ok_or(Error::Malformed("treasure number overflow"))?;
                    treasures.push(Treasure::parse(record, number)?);
                }
                KIND_DATE | KIND_DATE_SHORT => {}
                _ => {
                    return Err(Error::Malformed(
                        "treasure member of an unknown record kind",
                    ));
                }
            }
        }
        let first = first.ok_or(Error::Malformed("treasure member without its first number"))?;
        Ok(Self { first, treasures })
    }

    /// The treasures, in file order.
    pub fn treasures(&self) -> &[Treasure] {
        &self.treasures
    }

    /// The treasure numbered `number` (the game-wide numbering).
    pub fn treasure(&self, number: u16) -> Option<&Treasure> {
        self.treasures.iter().find(|t| t.number == number)
    }

    pub fn len(&self) -> usize {
        self.treasures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.treasures.is_empty()
    }

    /// The member name of the map whose treasures these are, `<map>.bin`'s stem.
    pub fn member(map: &str) -> String {
        format!("{map}.bin")
    }
}

/// A random treasure table, beside the maps in `treasure.nsarc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RandomTable {
    /// `randTBox`: a blue chest's, ranks 1 to 5.
    Chest,
    /// `randTD`: no village treasure's; its name and ten ranks suggest the grottoes.
    Grotto,
    /// `randTTT`: a pot's, a barrel's or a cupboard's, ranks 1 to 20.
    Pot,
}

impl RandomTable {
    /// Every random table.
    pub const ALL: [Self; 3] = [Self::Chest, Self::Grotto, Self::Pot];

    /// The member's name in `treasure.nsarc`.
    pub fn member(self) -> &'static str {
        match self {
            Self::Chest => "randTBox.bin",
            Self::Grotto => "randTD.bin",
            Self::Pot => "randTTT.bin",
        }
    }
}

/// One row of a random table, a `0x69` word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomRow {
    /// Bits 26 to 31: the container's rank.
    pub rank: u8,
    /// Bits 23 to 25: what the row gives.
    pub contents: Contents,
    /// Bits 7 to 22: the gold, the item's id or the monster's number.
    pub value: u16,
    /// Bits 0 to 6: the row's weight among its rank's rows (they come to 100 on `randTBox`
    /// and `randTD`, 20 to 50 on `randTTT`, the shortfall being the chance of nothing).
    pub weight: u8,
}

impl RandomRow {
    /// Reads a row word.
    pub fn from_word(word: u32) -> Self {
        Self {
            rank: ((word >> 26) & 0x3f) as u8,
            contents: match (word >> 23) & 7 {
                1 => Contents::Gold,
                2 => Contents::Item,
                3 => Contents::Monster,
                _ => Contents::Nothing,
            },
            value: ((word >> 7) & 0xffff) as u16,
            weight: (word & 0x7f) as u8,
        }
    }
}

/// A random table, read from its member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RandomTreasure {
    rows: Vec<RandomRow>,
}

impl RandomTreasure {
    /// Parses one member.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table, a record kind the table does not have, a row
    /// that is not one integer, or a `0x6A` count that is not the number of rows.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut declared = None;
        let mut rows = Vec::new();
        for record in &table.records {
            match record.kind {
                KIND_ROW_COUNT => {
                    let value = record
                        .values
                        .first()
                        .copied()
                        .ok_or(Error::Malformed("random table count without a value"))?;
                    declared = Some(integer(value, "random table row count")?);
                }
                KIND_ROW => {
                    let value = record
                        .values
                        .first()
                        .copied()
                        .ok_or(Error::Malformed("random table row without a value"))?;
                    rows.push(RandomRow::from_word(integer(value, "random table row")?));
                }
                KIND_DATE | KIND_DATE_SHORT | KIND_FIRST_NUMBER => {}
                _ => return Err(Error::Malformed("random table of an unknown record kind")),
            }
        }
        if declared != Some(rows.len() as u32) {
            return Err(Error::Malformed("random table count is not its row count"));
        }
        Ok(Self { rows })
    }

    /// The rows, in file order.
    pub fn rows(&self) -> &[RandomRow] {
        &self.rows
    }

    /// The rows of `rank`, in file order (the draw walks them in this order).
    pub fn rank(&self, rank: u8) -> impl Iterator<Item = &RandomRow> + '_ {
        self.rows.iter().filter(move |row| row.rank == rank)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// An integer field; a float or a string does not read as a number here.
fn integer<'a>(value: Value<'a>, what: &'static str) -> Result<u32> {
    match value {
        Value::Int(v) => Ok(v),
        _ => Err(Error::Malformed(what)),
    }
}

/// A position or a facing, in **world units**: a whole number is world units and a float is
/// world units too (the wiki's "Integer and float values"), not fixed point, so neither is
/// scaled here. `LootManager_CreateContainer` (EU `0x0207baa0`) converts a script parameter to
/// fx32 itself: `Int << 12`, or `Float × [0x0207bcd0]` = 4096.0. Measured on the European ROM:
/// `M01M08`'s blue chest is `(4.32, 1.17, -3.02)` and the map's floor at `(4.32, -3.02)` is
/// 1.17; dividing by 4096 would put it at `(0.001, -0.001)`, where the floor is 0.10. A string
/// does not appear.
fn coordinate(value: Value<'_>) -> Result<f32> {
    match value {
        Value::Int(v) => Ok(v as i32 as f32),
        Value::Float(f) => Ok(f),
        Value::Str(_) => Err(Error::Malformed("treasure coordinate is a string")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Record;

    /// A two-value record: the id and the content word, then the kind; positions follow.
    fn record(values: Vec<Value<'static>>) -> Record<'static> {
        Record {
            kind: KIND_TREASURE,
            values,
        }
    }

    fn member(records: Vec<Record<'static>>) -> Vec<u8> {
        crate::test_util::encode_table(&records)
    }

    fn chapter() -> TreasureMap {
        let mut records = vec![
            Record {
                kind: KIND_DATE,
                values: vec![Value::Str(Some(b"2009/04/13 13:31:16"))],
            },
            Record {
                kind: KIND_FIRST_NUMBER,
                values: vec![Value::Int(16)],
            },
        ];
        // A cupboard holding a rank: no position, the room's cabinet less one.
        records.push(record(vec![
            Value::Int(189 << 16 | 16),
            Value::Int(0x30),
            Value::Int(0),
        ]));
        // A pot at a place.
        records.push(record(vec![
            Value::Int(288 << 16 | 6),
            Value::Int(0x10),
            Value::Float(-2.93),
            Value::Float(0.15),
            Value::Float(4.86),
        ]));
        // The village's blue chest, facing along +x.
        records.push(record(vec![
            Value::Int(566 << 16 | 1),
            Value::Int(0x40),
            Value::Float(4.32),
            Value::Float(1.17),
            Value::Float(-3.02),
            Value::Int(0),
        ]));
        // A red chest holding an item, its position written as whole numbers.
        records.push(record(vec![
            Value::Int(7 << 16 | 21291),
            Value::Int(0x08),
            Value::Int(22),
            Value::Int(1),
            Value::Int(0),
            Value::Float(core::f32::consts::PI),
        ]));
        TreasureMap::parse(&member(records)).unwrap()
    }

    #[test]
    fn treasures_number_on_from_the_files_first() {
        let map = chapter();
        assert_eq!(map.first, 16);
        assert_eq!(map.len(), 4);
        let numbers: Vec<u16> = map.treasures().iter().map(|t| t.number).collect();
        assert_eq!(numbers, vec![16, 17, 18, 19]);
        assert_eq!(map.treasure(18).unwrap().id, 566);
        assert_eq!(map.treasure(16), map.treasures().first());
        assert_eq!(map.treasure(99), None);
        assert_eq!(TreasureMap::member("M01M07"), "M01M07.bin");
    }

    #[test]
    fn the_kind_word_gives_the_container_its_contents_and_its_lock() {
        let map = chapter();
        let cupboard = map.treasure(16).unwrap();
        assert_eq!(cupboard.container, Container::Cupboard);
        assert_eq!(cupboard.contents, Contents::Nothing);
        assert_eq!(cupboard.lock, Lock::None);
        assert_eq!(cupboard.kind(), 0x30);
        assert_eq!(cupboard.cabinet, Some(0));
        assert_eq!(cupboard.position, None);

        let pot = map.treasure(17).unwrap();
        assert_eq!(pot.container, Container::Pot);
        assert_eq!(pot.value, 6);
        assert_eq!(pot.position, Some([-2.93, 0.15, 4.86]));
        assert_eq!(pot.facing, None);

        let blue = map.treasure(18).unwrap();
        assert_eq!(blue.container, Container::BlueChest);
        assert_eq!(blue.value, 1);
        assert_eq!(blue.facing, Some(0.0));

        let red = map.treasure(19).unwrap();
        assert_eq!(red.container, Container::RedChest);
        assert_eq!(red.contents, Contents::Item);
        assert_eq!(red.value, 21291);
        assert_eq!(red.position, Some([22.0, 1.0, 0.0]));
        assert_eq!(red.facing, Some(core::f32::consts::PI));
    }

    /// `0x212 + id` for a red chest, `0x79e + id` for the rest, and the field's start clears
    /// exactly 700 of those.
    #[test]
    fn the_flag_follows_the_container() {
        let map = chapter();
        assert_eq!(map.treasure(19).unwrap().flag(), RED_CHEST_FLAG_BASE + 7);
        assert_eq!(map.treasure(18).unwrap().flag(), CONTAINER_FLAG_BASE + 566);
        assert_eq!(map.treasure(16).unwrap().flag(), CONTAINER_FLAG_BASE + 189);
        assert_eq!(RED_CHEST_FLAG_BASE, 0x212);
        assert_eq!(CONTAINER_FLAG_BASE, 0x79e);
        assert_eq!(CONTAINER_FLAG_COUNT, 0x2bc);
        assert_eq!(map.treasure(16).unwrap().number, 16);
    }

    #[test]
    fn what_draws_from_which_table() {
        assert_eq!(Container::RedChest.table(), None);
        assert_eq!(Container::BlueChest.table(), Some(RandomTable::Chest));
        assert_eq!(Container::Pot.table(), Some(RandomTable::Pot));
        assert_eq!(Container::Barrel.table(), Some(RandomTable::Pot));
        assert_eq!(Container::Cupboard.table(), Some(RandomTable::Pot));
        assert!(!Container::RedChest.draws());
        assert!(Container::BlueChest.draws());
        assert_eq!(Container::ALL.map(Container::bits), [0, 1, 2, 3, 4]);
        assert_eq!(Container::from_bits(5), None);
        assert_eq!(Contents::from_bits(4), None);
        assert_eq!(Lock::from_bits(3), None);
        assert_eq!(Lock::from_bits(1), Some(Lock::Thief));
    }

    #[test]
    fn a_malformed_member_is_an_error() {
        let no_first = member(vec![record(vec![
            Value::Int(1),
            Value::Int(0x08),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
        ])]);
        assert!(TreasureMap::parse(&no_first).is_err());

        let bad_container = member(vec![
            Record {
                kind: KIND_FIRST_NUMBER,
                values: vec![Value::Int(0)],
            },
            record(vec![
                Value::Int(1),
                Value::Int(0x50),
                Value::Int(0),
                Value::Int(0),
                Value::Int(0),
                Value::Int(0),
            ]),
        ]);
        assert!(TreasureMap::parse(&bad_container).is_err());

        let bad_length = member(vec![
            Record {
                kind: KIND_FIRST_NUMBER,
                values: vec![Value::Int(0)],
            },
            record(vec![
                Value::Int(1),
                Value::Int(0x08),
                Value::Int(0),
                Value::Int(0),
            ]),
        ]);
        assert!(TreasureMap::parse(&bad_length).is_err());

        let string_coordinate = member(vec![
            Record {
                kind: KIND_FIRST_NUMBER,
                values: vec![Value::Int(0)],
            },
            record(vec![
                Value::Int(1),
                Value::Int(0x08),
                Value::Str(Some(b"x")),
                Value::Int(0),
                Value::Int(0),
                Value::Int(0),
            ]),
        ]);
        assert!(TreasureMap::parse(&string_coordinate).is_err());

        let unknown_kind = member(vec![Record {
            kind: 0x99,
            values: vec![Value::Int(0)],
        }]);
        assert!(TreasureMap::parse(&unknown_kind).is_err());
    }

    fn row(rank: u32, contents: u32, value: u32, weight: u32) -> Record<'static> {
        Record {
            kind: KIND_ROW,
            values: vec![Value::Int(
                rank << 26 | contents << 23 | value << 7 | weight,
            )],
        }
    }

    #[test]
    fn random_rows_read_rank_gift_value_and_weight() {
        let records = vec![
            Record {
                kind: KIND_DATE_SHORT,
                values: vec![Value::Str(Some(b"090413"))],
            },
            Record {
                kind: KIND_ROW_COUNT,
                values: vec![Value::Int(2)],
            },
            row(1, 2, 22000, 60),
            row(1, 1, 210, 40),
        ];
        let table = RandomTreasure::parse(&member(records)).unwrap();
        assert_eq!(table.len(), 2);
        let first = table.rows()[0];
        assert_eq!(first.rank, 1);
        assert_eq!(first.contents, Contents::Item);
        assert_eq!(first.value, 22000);
        assert_eq!(first.weight, 60);
        assert_eq!(table.rows()[1].contents, Contents::Gold);
        assert_eq!(table.rank(1).count(), 2);
        assert_eq!(table.rank(2).count(), 0);
        assert_eq!(RandomRow::from_word(3 << 23).contents, Contents::Monster);
        assert_eq!(RandomRow::from_word(4 << 23).contents, Contents::Nothing);
        assert_eq!(RandomTable::Chest.member(), "randTBox.bin");
        assert_eq!(RandomTable::Grotto.member(), "randTD.bin");
        assert_eq!(RandomTable::Pot.member(), "randTTT.bin");
    }

    #[test]
    fn a_random_table_count_must_match_its_rows() {
        let records = vec![
            Record {
                kind: KIND_ROW_COUNT,
                values: vec![Value::Int(3)],
            },
            row(1, 2, 22000, 60),
        ];
        assert!(RandomTreasure::parse(&member(records)).is_err());
    }
}
