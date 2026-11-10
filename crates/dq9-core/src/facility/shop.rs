//! Shops: what an item costs, what a shop gives for it, and buying one.
//!
//! Source: the wiki's *Items* — § Shops for `shopdata1.bin`, § The price for the two words of
//! an item record — checked on the European cartridge (`docs/re/facility.md`). The prices are
//! the readers' ([`dq9_formats::facility::shop`]); the rules here are the party's side.
//!
//! **What a shop asks** is the item's ask word, scaled by the shop's rate in percent: `0xFFFF`
//! twice what a shop gives, `0xFFFE` twice and one, `0xFFFD` twice less one, `0xFFFC` ten
//! times, anything else the number itself (**[hypothesis]** the rate multiplies the asking
//! price and not the given one — wiki *Items*, § The price). **What a shop gives** for an item
//! is the given word as it stands.
//!
//! A stack holds 99 copies (`func_0207c378`) and the bag 152 slots
//! ([`crate::party::BAG_CAPACITY`]); gold is held to 9,999,999 ([`crate::party::GOLD_MAX`]).
//! **[Unknown]** whether the shop refuses a sale at those limits: this module does, rather
//! than lose gold or copies.

use dq9_formats::facility::shop::{ItemPrices, Shop};

use crate::party::{BAG_STACK, GOLD_MAX, Party};

/// Why a purchase did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyError {
    /// The shop's stock does not hold the item.
    NotSold,
    /// The item tables hold no price for it.
    NoPrice,
    /// The quantity is 0.
    Quantity,
    /// The party would hold more than 99 copies: a stack holds 99 (`func_0207c378`).
    TooMany { held: u32 },
    /// The gold does not reach the price.
    NotEnoughGold { price: u32 },
    /// The bag has no room for a new stack.
    BagFull,
}

/// What a shop asks for `item`, its rate applied. `None` when no table holds its price.
pub fn buy_price(shop: &Shop, prices: &ItemPrices, item: u16) -> Option<u32> {
    prices.get(item)?.at_rate(shop.rate)
}

/// What a shop gives for `item`: the given word of its record, which no rate scales
/// (**[unknown]** whether a rate touches it).
pub fn sell_price(prices: &ItemPrices, item: u16) -> Option<u32> {
    prices.get(item).map(|price| u32::from(price.given))
}

/// The copies of `item` the party may still buy before a stack is full.
pub fn max_quantity(party: &Party, item: u16) -> u8 {
    let held = party.bag.count(item);
    u32::from(BAG_STACK).saturating_sub(held) as u8
}

/// Buys `quantity` copies of `item` from `shop`: the price paid, or the reason nothing was.
///
/// The gold is taken and the copies added only when both fit; a partial purchase is not made
/// (**[unknown]** whether the game offers one).
pub fn buy(
    party: &mut Party,
    shop: &Shop,
    prices: &ItemPrices,
    item: u16,
    quantity: u8,
) -> Result<u32, BuyError> {
    if quantity == 0 {
        return Err(BuyError::Quantity);
    }
    if !shop.sells(item) {
        return Err(BuyError::NotSold);
    }
    let unit = buy_price(shop, prices, item).ok_or(BuyError::NoPrice)?;
    let held = party.bag.count(item);
    if held + u32::from(quantity) > u32::from(BAG_STACK) {
        return Err(BuyError::TooMany { held });
    }
    let total = unit * u32::from(quantity);
    if total > party.gold {
        return Err(BuyError::NotEnoughGold { price: total });
    }
    let room = held > 0 || party.bag.slots().iter().any(|slot| slot.item.is_none());
    if !room || !party.bag.add_copies(item, quantity) {
        return Err(BuyError::BagFull);
    }
    party.gold -= total;
    Ok(total)
}

/// Why a sale did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SellError {
    /// The bag holds no copy of the item.
    NotHeld,
    /// The item tables hold no price for it.
    NoPrice,
}

/// Sells up to `quantity` copies of `item` from the bag: the gold added, or the reason nothing
/// was.
///
/// [`Bag::remove`] takes the copies, the first stack first, and frees the slots it empties.
/// Fewer copies than asked go when the bag holds fewer. The gold is held to [`GOLD_MAX`], the
/// game's cap when a battle's gold is added (`func_ov023_021f5150`); **[hypothesis]** a sale
/// is capped the same way, and the gold a full purse cannot take is not paid.
pub fn sell(
    party: &mut Party,
    prices: &ItemPrices,
    item: u16,
    quantity: u8,
) -> Result<u32, SellError> {
    let unit = sell_price(prices, item).ok_or(SellError::NoPrice)?;
    let gone = party.bag.remove(item, quantity);
    if gone == 0 {
        return Err(SellError::NotHeld);
    }
    let before = party.gold;
    party.gold = before.saturating_add(unit * gone).min(GOLD_MAX);
    Ok(party.gold - before)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::party::BAG_CAPACITY;

    /// An `itemdt_<category>` member: a 12-byte head then 32-byte records (id at `+0x18`, the
    /// given price at `+0x1a`, the ask word at `+0x1c`).
    fn item_member(items: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut out = (items.len() as u16).to_le_bytes().to_vec();
        out.extend(0x8000u16.to_le_bytes());
        out.extend([0u8; 8]);
        for &(id, given, asked) in items {
            let mut record = [0u8; 32];
            record[0x18..0x1a].copy_from_slice(&(id as i16).to_le_bytes());
            record[0x1a..0x1c].copy_from_slice(&given.to_le_bytes());
            record[0x1c..0x1e].copy_from_slice(&asked.to_le_bytes());
            out.extend(record);
        }
        out
    }

    const HERB: u16 = 22000;
    const SOLDIERS_SWORD: u16 = 20005;

    fn prices() -> ItemPrices {
        // The herb gives 4 and asks twice (8); the soldier's sword gives 120 and asks twice.
        let member = item_member(&[(HERB, 4, 0xffff), (SOLDIERS_SWORD, 120, 0xffff)]);
        ItemPrices::from_members(&[&member]).unwrap()
    }

    fn shop() -> Shop {
        let mut stock = [None; 18];
        stock[0] = Some(HERB);
        stock[1] = Some(SOLDIERS_SWORD);
        Shop {
            number: 32,
            class: 3,
            stock,
            rate: 100,
            kind: 3,
        }
    }

    fn party(gold: u32) -> Party {
        let mut party = Party::new_game();
        party.gold = gold;
        party
    }

    #[test]
    fn the_price_is_the_items_ask_word_at_the_shops_rate() {
        let prices = prices();
        let shop = shop();
        assert_eq!(buy_price(&shop, &prices, HERB), Some(8));
        assert_eq!(buy_price(&shop, &prices, SOLDIERS_SWORD), Some(240));
        assert_eq!(buy_price(&shop, &prices, 12345), None);
        let mut dearer = shop;
        dearer.rate = 500;
        assert_eq!(buy_price(&dearer, &prices, HERB), Some(40));
        assert_eq!(sell_price(&prices, HERB), Some(4));
        assert_eq!(sell_price(&prices, SOLDIERS_SWORD), Some(120));
        assert_eq!(sell_price(&prices, 12345), None);
    }

    #[test]
    fn a_sale_takes_the_copies_and_adds_the_gold() {
        let prices = prices();
        let mut state = party(10);
        state.bag.add_copies(HERB, 5);
        state.bag.add_copies(SOLDIERS_SWORD, 1);
        assert_eq!(sell(&mut state, &prices, HERB, 3), Ok(12));
        assert_eq!(state.gold, 22);
        assert_eq!(state.bag.count(HERB), 2);
        // Fewer copies than asked: what is there goes.
        assert_eq!(sell(&mut state, &prices, HERB, 9), Ok(8));
        assert_eq!(state.bag.count(HERB), 0);
        assert_eq!(state.gold, 30);
        assert_eq!(sell(&mut state, &prices, HERB, 1), Err(SellError::NotHeld));
        assert_eq!(sell(&mut state, &prices, 12345, 1), Err(SellError::NoPrice));
        assert_eq!(state.bag.count(SOLDIERS_SWORD), 1, "other stacks stay");
        assert_eq!(
            sell(&mut state, &prices, SOLDIERS_SWORD, 0),
            Err(SellError::NotHeld)
        );
    }

    #[test]
    fn a_sale_does_not_pass_the_gold_cap() {
        let prices = prices();
        let mut state = party(GOLD_MAX - 10);
        state.bag.add_copies(SOLDIERS_SWORD, 1);
        assert_eq!(sell(&mut state, &prices, SOLDIERS_SWORD, 1), Ok(10));
        assert_eq!(state.gold, GOLD_MAX);
        assert_eq!(state.bag.count(SOLDIERS_SWORD), 0);
    }

    #[test]
    fn a_purchase_takes_the_gold_and_fills_one_stack() {
        let mut state = party(100);
        let paid = buy(&mut state, &shop(), &prices(), HERB, 3).unwrap();
        assert_eq!(paid, 24);
        assert_eq!(state.gold, 76);
        assert_eq!(state.bag.count(HERB), 3);
        assert_eq!(max_quantity(&state, HERB), 96);
    }

    #[test]
    fn a_stack_stops_at_ninety_nine() {
        let mut state = party(1_000);
        state.bag.add_copies(HERB, 95);
        assert_eq!(max_quantity(&state, HERB), 4);
        assert_eq!(
            buy(&mut state, &shop(), &prices(), HERB, 10),
            Err(BuyError::TooMany { held: 95 })
        );
        assert_eq!(state.gold, 1_000);
        assert_eq!(state.bag.count(HERB), 95);
        assert!(buy(&mut state, &shop(), &prices(), HERB, 4).is_ok());
        assert_eq!(state.bag.count(HERB), 99);
    }

    #[test]
    fn a_shop_sells_only_its_stock_at_a_price_it_has() {
        let mut state = party(1_000);
        assert_eq!(
            buy(&mut state, &shop(), &prices(), 12345, 1),
            Err(BuyError::NotSold)
        );
        let empty = ItemPrices::default();
        assert_eq!(
            buy(&mut state, &shop(), &empty, HERB, 1),
            Err(BuyError::NoPrice)
        );
        assert_eq!(
            buy(&mut state, &shop(), &prices(), HERB, 0),
            Err(BuyError::Quantity)
        );
        assert_eq!(state.gold, 1_000);
    }

    #[test]
    fn gold_short_changes_nothing() {
        let mut state = party(239);
        assert_eq!(
            buy(&mut state, &shop(), &prices(), SOLDIERS_SWORD, 1),
            Err(BuyError::NotEnoughGold { price: 240 })
        );
        assert_eq!(state.gold, 239);
        assert_eq!(state.bag.count(SOLDIERS_SWORD), 0);
        assert!(buy(&mut state, &shop(), &prices(), SOLDIERS_SWORD, 0).is_err());
    }

    #[test]
    fn a_full_bag_refuses_a_new_stack_but_not_a_stack_it_has() {
        let mut state = party(1_000);
        let mut bag = crate::party::Bag::empty(BAG_CAPACITY);
        for slot in 0..BAG_CAPACITY - 1 {
            bag.add(23000 + slot as u16);
        }
        // The last slot holds the herb, so it may still grow.
        bag.add_copies(HERB, 1);
        state.bag = bag;
        assert!(buy(&mut state, &shop(), &prices(), HERB, 5).is_ok());
        let mut full = state.clone();
        full.bag.add(23000 + BAG_CAPACITY as u16);
        assert_eq!(
            buy(&mut full, &shop(), &prices(), SOLDIERS_SWORD, 1),
            Err(BuyError::BagFull)
        );
        assert_eq!(full.gold, state.gold);
    }
}
