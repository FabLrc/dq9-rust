//! The facility rules on the real game data; skipped when the ROM is absent.
//!
//! The shops, the inns' prices and the treasure of Angel Falls are the cartridge's; the
//! chapter 2 map ids are 1100 to 1112 (`M01M01` to `M01M12`), `S01M01` (5101) and
//! `D01M01` to `D01M05` (7101 to 7105).

use std::borrow::Cow;

use dq9_core::battle::Random;
use dq9_core::facility::church::{self, MemberStatus, Service};
use dq9_core::facility::inn::{self, Stay};
use dq9_core::facility::shop::{self, BuyError};
use dq9_core::facility::treasure::{self, Open, Opened, Tables};
use dq9_core::party::{BAG_CAPACITY, Character, Party};
use dq9_core::state::{DayClock, TimeOfDay};
use dq9_formats::Gpc2;
use dq9_formats::battle::text::{ItemNames, Language};
use dq9_formats::facility::inn_church::{Church, Inn};
use dq9_formats::facility::shop::{ItemPrices, ShopTable};
use dq9_formats::facility::treasure::{
    Container, Contents, RandomTable, RandomTreasure, TreasureMap,
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

fn item_prices(rom: &Rom<'_>) -> ItemPrices {
    let mut members = Vec::new();
    for category in dq9_formats::item::CATEGORIES {
        let path = format!("data/prm/itemdt_{category}.gp2");
        let name = format!("itemdt_{category}_en.nat");
        members.push(member(rom, &path, &name));
    }
    let borrowed: Vec<&[u8]> = members.iter().map(|m| m.as_ref()).collect();
    ItemPrices::from_members(&borrowed).expect("item prices")
}

fn treasures<'a>(rom: &Rom<'a>) -> Narc<'a> {
    Narc::parse(file(rom, "data/scenario/treasure.nsarc")).expect("treasure.nsarc")
}

fn treasure_of<'a>(narc: &Narc<'a>, map: &str) -> TreasureMap {
    let name = TreasureMap::member(map);
    let index = (0..narc.len() as u16)
        .find(|&i| narc.name(i) == Some(name.as_str()))
        .unwrap_or_else(|| panic!("{name} in treasure.nsarc"));
    TreasureMap::parse(narc.file(index).unwrap()).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn random_table<'a>(narc: &Narc<'a>, which: RandomTable) -> RandomTreasure {
    let index = (0..narc.len() as u16)
        .find(|&i| narc.name(i) == Some(which.member()))
        .unwrap();
    RandomTreasure::parse(narc.file(index).unwrap())
        .unwrap_or_else(|error| panic!("{}: {error}", which.member()))
}

/// Four members, one down, the hero alone at the inn: [`inn::price`] counts the living.
fn party() -> Party {
    let mut party = Party::new_game();
    party.members[0].levels[6] = 3;
    party.members[0].hp = 10;
    party.members[0].max_hp = 40;
    for (level, hp) in [(4, 0u16), (5, 20), (6, 30)] {
        let mut member = Character::hero();
        member.levels[6] = level;
        member.hp = hp;
        member.max_hp = 30;
        party.members.push(member);
    }
    party.gold = 100;
    party
}

/// The shops are the wiki's 37, and the village's 32 sells 18 things at the prices of the
/// let's play: the herb 8, the copper sword 150.
#[test]
fn the_shops_are_thirty_seven_and_the_village_sells_at_known_prices() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let shops = ShopTable::parse(file(&rom, "data/bin/menu/shopdata1.bin")).unwrap();
    assert_eq!(shops.len(), 37);
    let prices = item_prices(&rom);
    let village = shops.shop(32).expect("the village shop");
    assert_eq!(village.items().count(), 18);
    assert_eq!(shop::buy_price(village, &prices, 22000), Some(8));
    assert_eq!(shop::buy_price(village, &prices, 20004), Some(150));
    assert_eq!(shop::sell_price(&prices, 20004), Some(15));

    // A herb bought with 100 gold: 8 paid, the copy in the bag.
    let mut party = party();
    let paid = shop::buy(&mut party, village, &prices, 22000, 2).unwrap();
    assert_eq!(paid, 16);
    assert_eq!(party.gold, 84);
    assert_eq!(party.bag.count(22000), 2);
    // An item the village does not stock is refused, and nothing is paid.
    assert_eq!(
        shop::buy(&mut party, village, &prices, 25000, 1),
        Err(BuyError::NotSold)
    );
    // The leather whip is stocked but out of reach at 84 gold.
    assert_eq!(
        shop::buy(&mut party, village, &prices, 20300, 1),
        Err(BuyError::NotEnoughGold { price: 95 })
    );
    assert_eq!(party.gold, 84);

    // Selling the two herbs back gives 4 the copy, the word of their record.
    assert_eq!(shop::sell(&mut party, &prices, 22000, 2), Ok(8));
    assert_eq!(party.gold, 92);
    assert_eq!(party.bag.count(22000), 0);
    assert_eq!(party.bag.slots()[0].item, None, "the slot is freed");
}

/// Ivor's inn (`str_in0`, the file of `<INN=1>`) asks 5 a head; a stay on four members with
/// one down costs 15, heals the three living to full and puts the clock at the day's start.
#[test]
fn a_stay_at_ivors_costs_five_a_living_head() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = member(&rom, "data/scenario/str_in0.gp2", "str_in0_en.nat");
    let ivor = Inn::parse(&data).unwrap();
    assert_eq!(ivor.price, 5);

    let mut party = party();
    let mut clock = DayClock::new(400.0, true);
    assert_eq!(inn::price(ivor.price, &party), 15);
    assert_eq!(
        inn::stay(&mut party, &mut clock, ivor.price),
        Stay::Paid { price: 15 }
    );
    assert_eq!(party.gold, 85);
    assert_eq!(party.members[0].hp, 40);
    assert_eq!(party.members[1].hp, 0);
    assert_eq!(party.members[2].hp, 30);
    assert_eq!(party.members[3].hp, 30);
    assert_eq!(clock.phase(), TimeOfDay::Day);
    assert_eq!(clock.seconds(), 210.0);
}

/// The church of Angel Falls (`str_ch0`, the file of `<CHURCH=1>`) offers every service; the
/// resurrection asks ⌊(L² + 20) / 20⌋ × 10 — 460 at level 30, 4,910 at 99 — and brings a dead
/// member back at full HP with its poison gone and its MP untouched.
#[test]
fn the_church_services_and_the_resurrection_price_table() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = member(&rom, "data/scenario/str_ch0.gp2", "str_ch0_en.nat");
    let church = Church::parse(&data).unwrap();
    assert_eq!(church.services(), Service::ALL.to_vec());
    assert_eq!(
        Church::archive(1).as_deref(),
        Some("data/scenario/str_ch0.gp2")
    );

    let mut member = Character::hero();
    member.levels[6] = 30;
    assert_eq!(church::resurrection_price(member.level()), 460);
    assert_eq!(church::price(Service::Resurrection, &member), Some(460));
    member.levels[6] = 99;
    assert_eq!(church::resurrection_price(member.level()), 4_910);
    member.levels[6] = 1;
    assert_eq!(church::resurrection_price(member.level()), 10);

    let mut party = party();
    party.gold = 1_000;
    party.members[1].mp = 4;
    party.members[1].max_mp = 9;
    let mut statuses = [
        MemberStatus::default(),
        MemberStatus {
            poisoned: true,
            cursed: true,
        },
        MemberStatus::default(),
        MemberStatus::default(),
    ];
    party.members[1].levels[6] = 30;
    let cure = church::cure(Service::Resurrection, &mut party, &mut statuses, 1).unwrap();
    assert_eq!(cure, church::Cure::Done { price: 460 });
    assert_eq!(party.gold, 540);
    assert_eq!(party.members[1].hp, party.members[1].max_hp);
    assert_eq!(party.members[1].mp, 4);
    assert_eq!(
        statuses[1],
        MemberStatus {
            poisoned: false,
            cursed: true
        }
    );
    assert_eq!(
        church::offer(Service::Confession),
        church::Offer::Unavailable(church::NOT_AVAILABLE)
    );
}

/// Angel Falls (`M01M03` to `M01M10`) holds twelve containers, all drawing from the random
/// tables; the Mountain Pass and the Hexagon hold seven. A blue chest's seeded draw lands on
/// one of its rank's rows.
#[test]
fn the_chapter_two_containers_are_read_and_a_blue_chest_draws_from_its_rank() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let narc = treasures(&rom);

    let angelfalls = ["M01M03", "M01M04", "M01M07", "M01M08", "M01M09", "M01M10"];
    let mut count = 0;
    let mut blue = None;
    for map in angelfalls {
        let map = treasure_of(&narc, map);
        for treasure in map.treasures() {
            count += 1;
            assert!(treasure.container.draws(), "no red chest in the village");
            assert!(treasure.contents == Contents::Nothing);
            if treasure.container == Container::BlueChest {
                blue = Some(*treasure);
            }
        }
    }
    assert_eq!(count, 12, "the village's containers");
    let blue = blue.expect("M01M08's blue chest");
    assert_eq!(blue.value, 1, "its rank");
    assert_eq!(blue.id, 566);

    let rest = ["S01M01", "D01M01", "D01M02", "D01M03", "D01M04", "D01M05"];
    let mut rest_count = 0;
    for map in rest {
        rest_count += treasure_of(&narc, map).len();
    }
    assert_eq!(rest_count, 10);

    let tables = Tables::new(
        random_table(&narc, RandomTable::Chest),
        random_table(&narc, RandomTable::Grotto),
        random_table(&narc, RandomTable::Pot),
    );
    let chest = tables.table(RandomTable::Chest);
    let rank: Vec<_> = chest
        .rank(blue.value as u8)
        .map(|row| (row.contents, row.value))
        .collect();
    // `randTBox`'s rank 1 is ten rows summing to 100: a draw always lands on one.
    assert_eq!(rank.len(), 10);
    assert_eq!(
        chest
            .rank(blue.value as u8)
            .map(|row| u32::from(row.weight))
            .sum::<u32>(),
        100
    );
    for seed in 0..16 {
        let mut random = Random::new(seed);
        match treasure::open(&blue, &tables, treasure::OpenState::new(false, &mut random)) {
            Open::Opened(Opened::Item(id)) => {
                assert!(rank.contains(&(Contents::Item, id)), "item {id} of rank 1")
            }
            Open::Opened(Opened::Gold(gold)) => {
                assert!(
                    rank.contains(&(Contents::Gold, gold)),
                    "gold {gold} of rank 1"
                )
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    // The same seed gives the same draw.
    let draw = |seed: u64| {
        let mut random = Random::new(seed);
        treasure::open(&blue, &tables, treasure::OpenState::new(false, &mut random))
    };
    assert_eq!(draw(99), draw(99));
}

/// The contents of the chapter 2 chests name their items: the Mountain Pass's gold ring, the
/// Hexagon's leather shield, feather fan and seed of deftness.
#[test]
fn the_chapter_two_chest_contents_name_their_items() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let narc = treasures(&rom);
    let names_data = member(&rom, "data/prm/itemname.gp2", "itemname_en.nat");
    let names = ItemNames::parse(&names_data).unwrap();

    let mut items = Vec::new();
    let mut gold = Vec::new();
    for map in ["S01M01", "D01M01", "D01M03", "D01M05"] {
        for treasure in treasure_of(&narc, map).treasures() {
            match treasure.contents {
                Contents::Item => {
                    let record = names
                        .find(treasure.value)
                        .unwrap_or_else(|| panic!("item {}", treasure.value));
                    items.push(record.name.to_vec());
                }
                Contents::Gold => gold.push(treasure.value),
                _ => {}
            }
        }
    }
    let items: Vec<String> = items
        .iter()
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect();
    assert_eq!(
        items,
        vec![
            "gold ring",
            "leather shield",
            "feather fan",
            "seed of deftness",
            "scale armour"
        ]
    );
    assert_eq!(gold, vec![210]);
    assert_eq!(Language::English.code(), 1);
    assert_eq!(BAG_CAPACITY, 152);
}
