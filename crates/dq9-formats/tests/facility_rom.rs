//! The facility readers on the real game data; skipped when the ROM is absent.
//!
//! The counts and prices are the wiki's own (*Items*, *Inns-and-Churches*, *Treasure*),
//! re-checked on the European cartridge.

use std::borrow::Cow;

use dq9_formats::Gpc2;
use dq9_formats::battle::text::Language;
use dq9_formats::facility::inn_church::{CHURCH_FILES, Church, INN_FILES, Inn, Service};
use dq9_formats::facility::shop::{ItemPrices, SHOP_COUNT, STOCK_SLOTS, ShopTable};
use dq9_formats::facility::treasure::{
    Container, Contents, Lock, RandomTable, RandomTreasure, TreasureMap,
};
use nds_rom::{Narc, Rom};

fn file<'a>(rom: &Rom<'a>, path: &str) -> &'a [u8] {
    rom.file_by_path(path)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn member<'a>(rom: &Rom<'a>, path: &str, name: &str) -> Cow<'a, [u8]> {
    let bytes = file(rom, path);
    let gpc2 = Gpc2::parse(bytes).unwrap_or_else(|error| panic!("{path}: {error}"));
    let entry = gpc2
        .find(name)
        .unwrap_or_else(|| panic!("{name} in {path}"));
    gpc2.member(entry)
        .unwrap_or_else(|error| panic!("{name} of {path}: {error}"))
}

/// The nine `itemdt_<category>` members of one language.
fn item_prices(rom: &Rom<'_>, language: Language) -> ItemPrices {
    let mut members = Vec::new();
    for category in dq9_formats::item::CATEGORIES {
        let path = format!("data/prm/itemdt_{category}.gp2");
        let name = format!("itemdt_{category}_{}.nat", language.suffix());
        members.push(member(rom, &path, &name));
    }
    let borrowed: Vec<&[u8]> = members.iter().map(|m| m.as_ref()).collect();
    ItemPrices::from_members(&borrowed).expect("item prices")
}

/// The shops are the wiki's: 37 records of 22 integers, numbers 0 to 37 but 31, a rate of 100
/// on 35 and 500 on 2, every stocked item priced above 0 in the item tables.
#[test]
fn shops_hold_the_wikis_37_records_and_their_stock_is_priced() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = ShopTable::parse(file(&rom, "data/bin/menu/shopdata1.bin")).expect("shopdata1");
    assert_eq!(table.len(), SHOP_COUNT);
    let numbers: Vec<u16> = table.shops().iter().map(|shop| shop.number).collect();
    assert_eq!(numbers.len(), 37);
    assert!((0..=37).filter(|n| !numbers.contains(n)).count() == 1);
    assert!(!numbers.contains(&31));
    assert!(
        table
            .shops()
            .iter()
            .all(|shop| (1..=5).contains(&shop.class))
    );
    assert!(table.shops().iter().all(|shop| shop.kind <= 5));
    let cheap = table.shops().iter().filter(|shop| shop.rate == 100).count();
    assert_eq!((cheap, table.len() - cheap), (35, 2));
    for shop in table.shops() {
        let filled = shop.stock.iter().flatten().count();
        assert!(matches!(filled, 1 | 6 | 12 | STOCK_SLOTS), "{shop:?}");
        assert_eq!(shop.items().count(), filled);
    }

    let prices = item_prices(&rom, Language::English);
    assert_eq!(prices.len(), 1178);
    for shop in table.shops() {
        for item in shop.items() {
            let price = prices.get(item).unwrap_or_else(|| panic!("{item} priced"));
            assert!(
                price.price().is_some_and(|price| price > 0),
                "shop {} sells {item} without a price",
                shop.number
            );
            assert!(price.at_rate(shop.rate).is_some());
        }
    }

    // The village shop, the one the shopkeeper's `<SHOP=32>` opens: the herb 8, the copper
    // sword 150 — the prices of the wiki's let's play.
    let village = table.shop(32).expect("shop 32");
    assert_eq!(village.rate, 100);
    assert_eq!(village.items().count(), STOCK_SLOTS);
    let herb = prices.get(22000).unwrap().at_rate(village.rate);
    assert_eq!(herb, Some(8));
}

/// Every `str_in*` and `str_ch*` member of the five languages parses, and the prices are the
/// wiki's: 5 at Ivor's files, 4 to 12 elsewhere, 0 for the bed. The services follow the
/// labels: `str_ch3` has no Resurrection and no Purification, `str_ch7` no Confession.
#[test]
fn every_inn_and_church_member_reads() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    for language in Language::ALL {
        for k in 0..INN_FILES {
            let path = format!("data/scenario/str_in{k}.gp2");
            let name = format!("str_in{k}_{}.nat", language.suffix());
            let data = member(&rom, &path, &name);
            let inn = Inn::parse(&data).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(matches!(inn.price, 0 | 4..=12), "{name} asks {}", inn.price);
            assert!(inn.style() <= 3);
            for line in [
                dq9_formats::facility::inn_church::inn_line::STAY,
                dq9_formats::facility::inn_church::inn_line::REST,
                dq9_formats::facility::inn_church::inn_line::CANCEL,
                dq9_formats::facility::inn_church::inn_line::GREETING_DAY,
                dq9_formats::facility::inn_church::inn_line::GREETING_NIGHT,
                dq9_formats::facility::inn_church::inn_line::TOO_POOR,
                dq9_formats::facility::inn_church::inn_line::LEAVE,
                dq9_formats::facility::inn_church::inn_line::PRICE,
            ] {
                assert!(inn.message(line).is_some(), "{name} has no line {line}");
            }
        }
        for k in 0..CHURCH_FILES {
            let path = format!("data/scenario/str_ch{k}.gp2");
            let name = format!("str_ch{k}_{}.nat", language.suffix());
            let data = member(&rom, &path, &name);
            Church::parse(&data).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    let ivor = member(&rom, "data/scenario/str_in0.gp2", "str_in0_en.nat");
    assert_eq!(Inn::parse(&ivor).unwrap().price, 5);
    let bed = member(&rom, "data/scenario/str_in14.gp2", "str_in14_en.nat");
    assert_eq!(Inn::parse(&bed).unwrap().price, 0);

    let observatory_data = member(&rom, "data/scenario/str_ch3.gp2", "str_ch3_en.nat");
    let observatory = Church::parse(&observatory_data).unwrap();
    assert!(!observatory.offers(Service::Resurrection));
    assert!(!observatory.offers(Service::Purification));
    assert!(observatory.offers(Service::Benediction));
    let log_data = member(&rom, "data/scenario/str_ch7.gp2", "str_ch7_en.nat");
    let log = Church::parse(&log_data).unwrap();
    assert!(!log.offers(Service::Confession));
    let village_data = member(&rom, "data/scenario/str_ch0.gp2", "str_ch0_en.nat");
    let village = Church::parse(&village_data).unwrap();
    assert_eq!(village.services(), Service::ALL.to_vec());
    assert_eq!(
        Church::archive(1).as_deref(),
        Some("data/scenario/str_ch0.gp2")
    );
}

/// Every member of `treasure.nsarc` reads: 265 treasure members whose `0x66` spans run from 0
/// to 847 without a gap, and the three random tables with 68, 162 and 98 rows.
#[test]
fn every_treasure_member_reads_and_the_numbering_has_no_gap() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let narc = Narc::parse(file(&rom, "data/scenario/treasure.nsarc")).expect("treasure.nsarc");
    assert_eq!(narc.len(), 268);

    let mut spans = Vec::new();
    let mut containers = [0usize; 5];
    let mut locks = 0usize;
    let mut monsters = 0usize;
    let mut random = Vec::new();
    for index in 0..narc.len() as u16 {
        let data = narc.file(index).expect("member");
        let name = narc.name(index).unwrap_or("?").to_owned();
        if name.starts_with("rand") {
            let table =
                RandomTreasure::parse(data).unwrap_or_else(|error| panic!("{name}: {error}"));
            random.push((name, table.len()));
            continue;
        }
        let map = TreasureMap::parse(data).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(!map.is_empty(), "{name} has no treasure");
        spans.push((map.first, map.len(), name));
        for treasure in map.treasures() {
            containers[treasure.container as usize] += 1;
            if treasure.lock != Lock::None {
                locks += 1;
            }
            if treasure.contents == Contents::Monster {
                monsters += 1;
            }
            if treasure.container == Container::RedChest {
                assert!(treasure.flag() >= 0x212 && treasure.flag() < 0x212 + 207);
            } else {
                assert!(treasure.flag() >= 0x79e && treasure.flag() < 0x79e + 700);
            }
        }
    }
    random.sort();
    assert_eq!(
        random,
        vec![
            ("randTBox.bin".to_owned(), 68),
            ("randTD.bin".to_owned(), 162),
            ("randTTT.bin".to_owned(), 98),
        ]
    );
    assert_eq!(RandomTable::ALL.len(), 3);

    spans.sort();
    assert_eq!(spans.len(), 265);
    let mut next = 0u16;
    for (first, count, name) in &spans {
        assert_eq!(*first, next, "{name} does not run on from the last span");
        next += *count as u16;
    }
    assert_eq!(next, 847);
    assert_eq!(containers.iter().sum::<usize>(), 847);
    assert_eq!(containers, [167, 279, 185, 151, 65]);
    assert_eq!(locks, 6);
    assert_eq!(monsters, 0, "no red chest of the cartridge is a monster");
}
