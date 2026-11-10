//! A shop's stock (`data/bin/menu/shopdata1.bin`) and an item's two prices
//! (`data/prm/itemdt_<category>.gp2`, member `itemdt_<category>_<language>.nat`).
//!
//! `shopdata1.bin` is a tagged data table (wiki DQIX *Items*, § Shops; `docs/re/facility.md`):
//! a `0x66` record holding 37 and 37 `0x67` records of 22 integers, one per shop.
//!
//! | value | meaning |
//! |---|---|
//! | 0 | the shop's number — what a talk line's `<SHOP=n>` names |
//! | 1 | 1 to 5; **not established** |
//! | 2–19 | eighteen item ids, 0 for an empty slot |
//! | 20 | a price rate in percent (100 on 36 shops, 500 on one) — **INFERRED** |
//! | 21 | the kind of shop, 0 to 5 — **INFERRED** |
//!
//! An item record carries its id as `s16` at `+0x18` ([`crate::item`]), and the two prices as
//! the `u16` at `+0x1a` (what a shop gives for it) and `+0x1c` (what it asks, or a code on what
//! it gives). The wiki numbers these from a record that begins four bytes before the id, hence
//! its own `+0x06` and `+0x08` (wiki *Items*, § The price). What a shop asks is
//! `0xFFFC`/`0xFFFD`/`0xFFFE`/`0xFFFF` scaled from the given price by ten or two, and anything
//! else as the number itself; a 0 means no shop sells the item.

use std::collections::BTreeMap;

use nds_rom::bytes::u16_at;
use nds_rom::{Error, Result};

use crate::table::{Table, Value};

/// Shops the table holds (its `0x66` record says so; checked on all 37 records).
pub const SHOP_COUNT: usize = 37;
/// Item slots of a shop (values 2 to 19).
pub const STOCK_SLOTS: usize = 18;
/// Integers of a `0x67` record.
const SHOP_FIELDS: usize = 22;
/// Ask-price codes, from `0xFFFC` to `0xFFFF`; no item costs that much.
const ASK_TEN_TIMES: u16 = 0xFFFC;
const ASK_TWICE_LESS_ONE: u16 = 0xFFFD;
const ASK_TWICE_AND_ONE: u16 = 0xFFFE;
const ASK_TWICE: u16 = 0xFFFF;

/// Offset of the first item record and its stride ([`crate::item`]).
const ITEM_RECORDS_START: usize = 12;
const ITEM_RECORD_SIZE: usize = 0x20;
/// The id (`s16`) and the two prices (`u16`) inside an item record.
const ITEM_ID_OFFSET: usize = 0x18;
const ITEM_GIVEN_OFFSET: usize = 0x1a;
const ITEM_ASKED_OFFSET: usize = 0x1c;

/// One shop of `shopdata1.bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shop {
    /// Value 0: the number a `<SHOP=n>` tag names.
    pub number: u16,
    /// Value 1, 1 to 5. **Not established.**
    pub class: u8,
    /// Values 2–19 by item id; `None` for an empty slot.
    pub stock: [Option<u16>; STOCK_SLOTS],
    /// Value 20: a price rate in percent, **INFERRED** (`items.md`, § Shops).
    pub rate: u16,
    /// Value 21: the kind of shop, **INFERRED**.
    pub kind: u8,
}

impl Shop {
    /// Whether the shop's stock holds `item`.
    pub fn sells(&self, item: u16) -> bool {
        self.stock.contains(&Some(item))
    }

    /// The items of the stock, in slot order.
    pub fn items(&self) -> impl Iterator<Item = u16> + '_ {
        self.stock.iter().flatten().copied()
    }
}

/// The shops of `shopdata1.bin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopTable {
    shops: Vec<Shop>,
}

impl ShopTable {
    /// Parses the file.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed table, a record that is not a `0x67` of 22 integers,
    /// a value out of a `u16`, a shop number or class out of its range, a `0x66` record that is
    /// not the shop count, or no `0x66` record at all.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut declared = None;
        let mut shops = Vec::new();
        for record in &table.records {
            match record.kind {
                // The file's date and date-short strings, and the 0x66 record holding 37.
                0x64 | 0x65 => continue,
                0x66 => {
                    let value = record
                        .values
                        .first()
                        .copied()
                        .ok_or(Error::Malformed("shopdata1 count without a value"))?;
                    declared = Some(match value {
                        Value::Int(v) => v,
                        _ => return Err(Error::Malformed("shopdata1 count is not an integer")),
                    });
                    continue;
                }
                0x67 => {}
                _ => return Err(Error::Malformed("shopdata1 record is not a shop")),
            }
            if record.values.len() != SHOP_FIELDS {
                return Err(Error::Malformed("shopdata1 record is not 22 integers"));
            }
            let int = |i: usize| -> Result<u16> {
                match record.values[i] {
                    Value::Int(v) => u16::try_from(v)
                        .map_err(|_| Error::Malformed("shopdata1 value above 0xffff")),
                    _ => Err(Error::Malformed("shopdata1 value is not an integer")),
                }
            };
            let mut stock = [None; STOCK_SLOTS];
            for (slot, value) in stock.iter_mut().zip(2..20) {
                let id = int(value)?;
                *slot = (id != 0).then_some(id);
            }
            let class = u8::try_from(int(1)?)
                .map_err(|_| Error::Malformed("shopdata1 class above 0xff"))?;
            shops.push(Shop {
                number: int(0)?,
                class,
                stock,
                rate: int(20)?,
                kind: u8::try_from(int(21)?)
                    .map_err(|_| Error::Malformed("shopdata1 kind above 0xff"))?,
            });
        }
        if declared != Some(shops.len() as u32) {
            return Err(Error::Malformed("shopdata1 count is not its shop count"));
        }
        Ok(Self { shops })
    }

    /// The shops, in file order.
    pub fn shops(&self) -> &[Shop] {
        &self.shops
    }

    /// The shop numbered `number` (what a `<SHOP=n>` tag names).
    pub fn shop(&self, number: u16) -> Option<&Shop> {
        self.shops.iter().find(|shop| shop.number == number)
    }

    pub fn len(&self) -> usize {
        self.shops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shops.is_empty()
    }
}

/// The two prices of an item record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemPrice {
    /// What a shop gives for the item (`+0x1a`; the wiki's `+0x06`).
    pub given: u16,
    /// What it asks (`+0x1c`; the wiki's `+0x08`), a price or one of the four codes.
    pub asked: u16,
}

impl ItemPrice {
    /// What a shop asks for one, scaled by the shop's rate in percent (`rate` 100 asks the
    /// price as it stands). `None` when no shop sells the item.
    ///
    /// **[Hypothesis]** the rate multiplies what is asked and not what is given (wiki *Items*,
    /// § The price: "Whether it touches what a shop gives is not established").
    pub fn at_rate(self, rate: u16) -> Option<u32> {
        self.price().map(|price| price * u32::from(rate) / 100)
    }

    /// What a shop asks for one, before the shop's rate. `None` when the ask word is 0, which
    /// the wiki reads as "no shop sells it".
    pub fn price(self) -> Option<u32> {
        let given = u32::from(self.given);
        match self.asked {
            0 => None,
            ASK_TWICE => Some(2 * given),
            ASK_TWICE_AND_ONE => Some(2 * given + 1),
            ASK_TWICE_LESS_ONE => Some(2 * given - 1),
            ASK_TEN_TIMES => Some(10 * given),
            price => Some(u32::from(price)),
        }
    }
}

/// Item id → the two prices of its record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemPrices {
    entries: BTreeMap<u16, ItemPrice>,
}

impl ItemPrices {
    /// Parses one `itemdt_<category>_<language>.nat` member and merges its records.
    ///
    /// # Errors
    /// [`nds_rom::Error`] when the member is shorter than its records or an id is not
    /// positive (the rule of [`crate::item::ItemTable::add_member`]).
    pub fn add_member(&mut self, data: &[u8]) -> Result<()> {
        let count = usize::from(u16_at(data, 0)?);
        let end = ITEM_RECORDS_START
            .checked_add(count * ITEM_RECORD_SIZE)
            .ok_or(Error::Malformed("item price record count"))?;
        if end > data.len() {
            return Err(Error::Malformed(
                "item price table shorter than its records",
            ));
        }
        for index in 0..count {
            let at = ITEM_RECORDS_START + index * ITEM_RECORD_SIZE;
            let id = i16::from_le_bytes([data[at + ITEM_ID_OFFSET], data[at + ITEM_ID_OFFSET + 1]]);
            if id <= 0 {
                return Err(Error::Malformed("item price id not positive"));
            }
            self.entries.entry(id as u16).or_insert(ItemPrice {
                given: u16_at(data, at + ITEM_GIVEN_OFFSET)?,
                asked: u16_at(data, at + ITEM_ASKED_OFFSET)?,
            });
        }
        Ok(())
    }

    /// Parses the members of every category, in any order.
    ///
    /// # Errors
    /// As [`ItemPrices::add_member`].
    pub fn from_members(members: &[&[u8]]) -> Result<Self> {
        let mut prices = Self::default();
        for data in members {
            prices.add_member(data)?;
        }
        Ok(prices)
    }

    /// The prices of `item`; `None` when no table holds it.
    pub fn get(&self, item: u16) -> Option<ItemPrice> {
        self.entries.get(&item).copied()
    }

    /// Number of items known.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Record;

    /// An `itemdt_<category>` member: a 12-byte head then 32-byte records.
    fn item_member(items: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend((items.len() as u16).to_le_bytes());
        out.extend(0x8000u16.to_le_bytes());
        out.extend([0u8; 8]);
        for &(id, given, asked) in items {
            let mut record = [0u8; ITEM_RECORD_SIZE];
            record[ITEM_ID_OFFSET..ITEM_ID_OFFSET + 2].copy_from_slice(&(id as i16).to_le_bytes());
            record[ITEM_GIVEN_OFFSET..ITEM_GIVEN_OFFSET + 2].copy_from_slice(&given.to_le_bytes());
            record[ITEM_ASKED_OFFSET..ITEM_ASKED_OFFSET + 2].copy_from_slice(&asked.to_le_bytes());
            out.extend(record);
        }
        out
    }

    fn shop_record(number: u32, stock: &[u32], rate: u32, kind: u32) -> Record<'static> {
        let mut values = vec![Value::Int(number), Value::Int(3)];
        for slot in 0..STOCK_SLOTS {
            values.push(Value::Int(stock.get(slot).copied().unwrap_or(0)));
        }
        values.push(Value::Int(rate));
        values.push(Value::Int(kind));
        Record { kind: 0x67, values }
    }

    fn table(records: Vec<Record<'static>>) -> Vec<u8> {
        crate::test_util::encode_table(&records)
    }

    fn shops() -> ShopTable {
        let records = vec![
            Record {
                kind: 0x66,
                values: vec![Value::Int(2)],
            },
            shop_record(32, &[22000, 22004], 100, 3),
            shop_record(7, &[20004], 500, 0),
        ];
        ShopTable::parse(&table(records)).unwrap()
    }

    #[test]
    fn shops_read_their_slots_rate_and_kind() {
        let table = shops();
        assert_eq!(table.len(), 2);
        let shop = table.shop(32).unwrap();
        assert_eq!(shop.class, 3);
        assert_eq!(shop.items().collect::<Vec<_>>(), vec![22000, 22004]);
        assert!(shop.sells(22004));
        assert!(!shop.sells(20004));
        assert_eq!(shop.stock[2], None);
        assert_eq!(shop.rate, 100);
        assert_eq!(shop.kind, 3);
        assert_eq!(table.shop(99), None);
    }

    #[test]
    fn a_shop_record_must_hold_22_integers() {
        let records = vec![Record {
            kind: 0x67,
            values: vec![Value::Int(1); 21],
        }];
        assert!(ShopTable::parse(&table(records)).is_err());
        let records = vec![Record {
            kind: 0x67,
            values: vec![Value::Float(1.0); 22],
        }];
        assert!(ShopTable::parse(&table(records)).is_err());
    }

    /// The four codes and the plain number, checked against the guide's values: the herb costs
    /// 8 and gives 4, the chimaera wing 25 and 12, the leather whip 95 and 48, the copper
    /// sword 150 and 15, the bamboo lance 85 and 8.
    #[test]
    fn the_ask_word_scales_the_given_price() {
        let price = ItemPrice {
            given: 4,
            asked: ASK_TWICE,
        };
        assert_eq!(price.price(), Some(8));
        let price = ItemPrice {
            given: 12,
            asked: ASK_TWICE_AND_ONE,
        };
        assert_eq!(price.price(), Some(25));
        let price = ItemPrice {
            given: 48,
            asked: ASK_TWICE_LESS_ONE,
        };
        assert_eq!(price.price(), Some(95));
        let price = ItemPrice {
            given: 15,
            asked: ASK_TEN_TIMES,
        };
        assert_eq!(price.price(), Some(150));
        let price = ItemPrice {
            given: 8,
            asked: 85,
        };
        assert_eq!(price.price(), Some(85));
        let price = ItemPrice {
            given: 6_600,
            asked: 11_200,
        };
        assert_eq!(price.price(), Some(11_200));
        assert_eq!(ItemPrice::default().price(), None);
    }

    #[test]
    fn a_shop_rate_scales_the_price_asked() {
        let price = ItemPrice {
            given: 4,
            asked: ASK_TWICE,
        };
        assert_eq!(price.at_rate(100), Some(8));
        assert_eq!(price.at_rate(500), Some(40));
        assert_eq!(ItemPrice { given: 4, asked: 0 }.at_rate(100), None);
    }

    #[test]
    fn item_prices_read_the_two_words_of_a_record() {
        let member = item_member(&[(22000, 4, ASK_TWICE), (20004, 15, ASK_TEN_TIMES)]);
        let prices = ItemPrices::from_members(&[&member]).unwrap();
        assert_eq!(prices.len(), 2);
        let herb = prices.get(22000).unwrap();
        assert_eq!((herb.given, herb.asked), (4, ASK_TWICE));
        assert_eq!(herb.price(), Some(8));
        assert_eq!(prices.get(20004).unwrap().price(), Some(150));
        assert_eq!(prices.get(12345), None);
    }

    #[test]
    fn a_truncated_item_table_is_an_error() {
        let member = item_member(&[(22000, 4, ASK_TWICE)]);
        assert!(ItemPrices::from_members(&[&member[..40]]).is_err());
        let mut bad = member;
        bad[ITEM_RECORDS_START + ITEM_ID_OFFSET] = 0;
        bad[ITEM_RECORDS_START + ITEM_ID_OFFSET + 1] = 0;
        assert!(ItemPrices::from_members(&[&bad]).is_err());
    }
}
