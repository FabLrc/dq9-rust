//! Battle tables on the real game data; skipped when the ROM is absent.
//!
//! The counts and cross-checks are the wiki's own (*Monsters*, *Actions*, *Spell Table*,
//! *Equipment Battle Parameters*, *Encounters*), re-checked on the European ROM.

use std::borrow::Cow;
use std::collections::HashSet;

use dq9_formats::Gpc2;
use dq9_formats::battle::{action, encounter, equipment, monster, spell};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::table::Table;
use nds_rom::Rom;

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

fn battle_data<'a>(rom: &Rom<'a>) -> monster::BattleData<'a> {
    monster::BattleData::parse(file(rom, "data/prm/mon_btldata.nat")).unwrap()
}

/// 438 records each; the same numbers in the battle data and the name records; every monster
/// list entry a battle record; the Hexagoon's code; element 8 whole on all.
#[test]
fn monsters_agree_between_the_battle_and_name_records() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let battles = battle_data(&rom);
    let names_data = member(&rom, "data/prm/mon_data.gp2", "mon_data_en.nat");
    let names = monster::Names::parse(&names_data).unwrap();
    let list_data = member(&rom, "data/prm/mon_list.gp2", "mon_list_en.nat");
    let list = monster::List::parse(&list_data).unwrap();

    assert_eq!(battles.monsters.len(), 438);
    assert_eq!(names.monsters.len(), 438);
    assert_eq!(list.monsters.len(), 345);
    let battle_numbers: Vec<u16> = battles.monsters.iter().map(|m| m.number).collect();
    let name_numbers: Vec<u16> = names.monsters.iter().map(|m| m.number).collect();
    assert_eq!(battle_numbers, name_numbers, "same order, same numbers");
    assert_eq!(
        battle_numbers.iter().copied().collect::<HashSet<_>>().len(),
        438,
        "numbers are unique"
    );
    assert!(
        battle_numbers.iter().all(|&n| n != 0),
        "no record is numbered 0"
    );
    let flagged = battles.monsters.iter().filter(|m| m.number_flag).count();
    assert_eq!(flagged, 388, "bit 15 of +0x00 set on 388 of 438");

    for entry in &list.monsters {
        assert!(
            battles.find(entry.number).is_some(),
            "list number {} has no battle record",
            entry.number
        );
        assert_eq!(
            names.find(entry.number).unwrap().code,
            entry.code,
            "code agrees for {}",
            entry.number
        );
    }

    let hexagoon = names.find(300).expect("monster 300");
    assert_eq!(hexagoon.code, b"b003a");
    assert_eq!(hexagoon.name, b"hexagoon");
    assert_eq!(battles.find(300).unwrap().hp, 93);

    assert!(
        battles
            .monsters
            .iter()
            .all(|m| m.resistance(8) == Some(100)),
        "element 8, the plain Attack's, is 100 on all 438"
    );
    // Codes repeat: a code names the story's versions of one monster.
    let codes: HashSet<&[u8]> = names.monsters.iter().map(|m| m.code).collect();
    assert_eq!(codes.len(), 312);

    // Evasion and block grades (bits 13–15 and 16–18 of +0x10) index a five-entry table.
    for battle in &battles.monsters {
        assert!(battle.evasion_grade() <= 4);
        assert!(battle.block_grade() <= 4);
    }
    assert_eq!(
        battles
            .monsters
            .iter()
            .filter(|m| m.evasion_grade() > 0)
            .count(),
        143
    );
    assert_eq!(
        battles
            .monsters
            .iter()
            .filter(|m| m.block_grade() > 0)
            .count(),
        39
    );
    assert_eq!(battles.find(3).unwrap().evasion_grade(), 3, "metal slime");
}

/// 63 + 618 actions, every number named in `actname.nat`, every range index present in the
/// half's range table and every range record used.
#[test]
fn actions_and_their_ranges_agree() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let a_data = member(&rom, "data/prm/actdt_a.gp2", "actdt_a_en.nat");
    let b_data = member(&rom, "data/prm/actdt_b.gp2", "actdt_b_en.nat");
    let a = action::Actions::parse(&a_data).unwrap();
    let b = action::Actions::parse(&b_data).unwrap();
    let r_a =
        action::Ranges::parse(&member(&rom, "data/prm/actdt_a.gp2", "actdamage_a.nat")).unwrap();
    let r_b =
        action::Ranges::parse(&member(&rom, "data/prm/actdt_b.gp2", "actdamage_b.nat")).unwrap();
    let names = action::ActionNames::parse(file(&rom, "data/prm/actname.nat")).unwrap();

    assert_eq!(a.actions.len(), 63);
    assert_eq!(b.actions.len(), 618);
    assert_eq!(r_a.ranges.len(), 17);
    assert_eq!(r_b.ranges.len(), 107);
    assert_eq!(
        names.names.len(),
        682,
        "681 actions and an entry numbered 0"
    );

    let mut numbers = HashSet::new();
    for (table, ranges) in [(&a, &r_a), (&b, &r_b)] {
        for found in &table.actions {
            assert!(numbers.insert(found.number), "duplicate number");
            assert!(
                names.name(found.number).is_some(),
                "action {} has no name in actname",
                found.number
            );
            if found.range != 0 {
                assert!(
                    ranges.find(found.range).is_some(),
                    "action {} names range {} absent from the table",
                    found.number,
                    found.range
                );
            }
        }
        for range in &ranges.ranges {
            assert!(
                table.actions.iter().any(|a| a.range == range.index),
                "range {} unused",
                range.index
            );
        }
    }
    assert_eq!(numbers.len(), 681);

    // The plain attack: 1, damage, dodgeable and blockable, tension, combo, 0 MP.
    let attack = b.find(1).unwrap();
    assert_eq!(attack.name, b"Attack");
    assert_eq!((attack.mp_cost, attack.kind, attack.element), (0, 1, 8));
    assert!(attack.dodgeable && attack.blockable && attack.tension && attack.combo);
    assert_eq!(b.find(2).unwrap().name, b"Attack");
    // Heal: 30, in the healing half, 2 MP, range 16 (35 ± 5), mending.
    let heal = a.find(30).unwrap();
    assert_eq!(heal.name, b"Heal");
    assert_eq!(heal.mp_cost, 2);
    assert_eq!(heal.range, 16);
    assert!(heal.scales_magical_mending);
    let range = r_a.find(16).unwrap();
    assert_eq!(
        (range.spread, range.monster, range.party_least),
        (5, 35, 35)
    );
    // The medicinal herb: 255, range 49, 35 ± 5 with no scaling.
    let herb = a.find(255).unwrap();
    let herb_range = r_a.find(herb.range).unwrap();
    assert_eq!(
        (herb.mp_cost, herb_range.spread, herb_range.party_most),
        (0, 5, 35)
    );
}

/// 210 `encfld` groups, each naming a map of `maplist9.bin`; 287 zones and 1,044 roamer
/// records; `F01`'s three zones, their roamers and weights; the chapter 2 slice printed.
#[test]
fn field_encounters_name_maps_and_f01_zones() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let field = encounter::Field::parse(file(&rom, "data/prm/encfld.bin")).unwrap();
    let maps = MapList::parse(&Table::parse(file(&rom, maplist::PATH)).unwrap()).unwrap();

    assert_eq!(field.maps.len(), 210);
    for map in &field.maps {
        assert!(
            maps.find(map.map).is_some(),
            "encounter group for map {} is not in maplist9.bin",
            map.map
        );
    }
    assert_eq!(field.maps.iter().map(|m| m.zones.len()).sum::<usize>(), 287);
    assert_eq!(
        field
            .maps
            .iter()
            .flat_map(|m| &m.zones)
            .flat_map(|z| &z.roamers)
            .count(),
        1044
    );
    let zone_ids: HashSet<u16> = field
        .maps
        .iter()
        .flat_map(|m| &m.zones)
        .map(|z| z.id)
        .collect();
    assert_eq!(zone_ids.len(), 287, "zone numbers are unique");

    let f01 = field.find(20001).expect("F01, Angel Falls Region");
    let zones: Vec<u16> = f01.zones.iter().map(|z| z.id).collect();
    assert_eq!(zones, [12, 14, 15]);
    assert_eq!(f01.zones[0].kind(), 0);
    assert_eq!(f01.zones[1].kind(), 1);
    assert_eq!(f01.zones[2].kind(), 2);
    let roamers = |zone: usize| -> Vec<(u16, u8)> {
        f01.zones[zone]
            .roamers
            .iter()
            .map(|r| (r.monster, r.weight))
            .collect()
    };
    assert_eq!(roamers(0), [(1, 7), (56, 6), (57, 5), (63, 3)]);
    assert_eq!(roamers(1), [(1, 4), (56, 4), (57, 4), (63, 5)]);
    assert_eq!(roamers(2), [(23, 5), (32, 5), (57, 2)]);

    // The v0.2 slice: every map of the wave, with the zones and roamers the groups list.
    for (id, code) in [
        (20001, "F01"),
        (5101, "S01M01"),
        (7101, "D01M01"),
        (7102, "D01M02"),
        (7103, "D01M03"),
        (7104, "D01M04"),
        (7105, "D01M05"),
    ] {
        match field.find(id) {
            Some(map) => {
                let zones: Vec<String> = map
                    .zones
                    .iter()
                    .map(|zone| {
                        let roamers: Vec<String> = zone
                            .roamers
                            .iter()
                            .map(|r| format!("{}({})", r.monster, r.weight))
                            .collect();
                        format!(
                            "zone {} kind {}: {}",
                            zone.id,
                            zone.kind(),
                            roamers.join(" ")
                        )
                    })
                    .collect();
                println!("{code} ({id}): {}", zones.join("; "));
            }
            None => println!("{code} ({id}): no encounter group"),
        }
    }
    // The Hexagon's middle floors have groups; the first and last do not (printed above).
    for id in [7102, 7103, 7104] {
        assert!(field.find(id).is_some(), "Hexagon floor {id}");
    }
}

/// 290 `encbtl` zones; its roamers equal `encfld`'s on every zone that has them; least ≤ most
/// on every company word.
#[test]
fn battle_encounters_repeat_the_field_roamers() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let field = encounter::Field::parse(file(&rom, "data/prm/encfld.bin")).unwrap();
    let battle = encounter::Battle::parse(file(&rom, "data/prm/encbtl.bin")).unwrap();

    assert_eq!(battle.zones.len(), 290);
    let mut with_roamers = 0;
    for map in &field.maps {
        for zone in &map.zones {
            let listed = battle
                .find(zone.id)
                .unwrap_or_else(|| panic!("encbtl has no zone {}", zone.id));
            if !zone.roamers.is_empty() {
                let field_monsters: Vec<u16> = zone.roamers.iter().map(|r| r.monster).collect();
                let battle_monsters: Vec<u16> = listed.roamers.iter().map(|r| r.monster).collect();
                assert_eq!(
                    field_monsters, battle_monsters,
                    "zone {}: encbtl's roamers differ from encfld's",
                    zone.id
                );
                with_roamers += 1;
            }
        }
    }
    assert_eq!(with_roamers, 287);

    let company = battle
        .zones
        .iter()
        .flat_map(|z| &z.company)
        .collect::<Vec<_>>();
    assert_eq!(company.len(), 1527);
    for companion in company {
        assert!(companion.least <= companion.most);
        assert!(companion.weight <= 7);
        assert!((1..=5).contains(&companion.most));
    }
}

/// 438 field records; attack and defence equal the battle data's on all; the same numbers.
#[test]
fn field_monsters_match_the_battle_data() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let battles = battle_data(&rom);
    let field = encounter::FieldMonsters::parse(file(&rom, "data/prm/fld_mondata.bin")).unwrap();

    assert_eq!(field.monsters.len(), 438);
    let numbers: Vec<u16> = field.monsters.iter().map(|m| m.number).collect();
    let expected: Vec<u16> = battles.monsters.iter().map(|m| m.number).collect();
    assert_eq!(numbers, expected);
    for field_monster in &field.monsters {
        let battle = battles.find(field_monster.number).unwrap();
        assert_eq!(
            (field_monster.attack, field_monster.defence),
            (battle.attack, battle.defence),
            "monster {}",
            field_monster.number
        );
    }
}

/// `itembtlprm.nat`: 1,423 records filling the file, ids strictly ascending, and the family
/// bonus and resistance counts the wiki gives.
#[test]
fn equipment_battle_parameters_cover_every_item() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let params = equipment::BattleParams::parse(file(&rom, "data/prm/itembtlprm.nat")).unwrap();

    assert_eq!(params.items.len(), 1423);
    assert!(
        params.items.windows(2).all(|w| w[0].id < w[1].id),
        "ids strictly ascending"
    );
    assert_eq!(params.items[0].id, 994);
    assert_eq!(params.items.last().unwrap().id, 22290);
    let all = |bonus: i8| {
        params
            .items
            .iter()
            .filter(|i| i.family_bonus.iter().all(|&b| b == bonus))
            .count()
    };
    assert_eq!(all(10), 928, "no bonus, in tenths");
    assert_eq!(all(0), 245, "empty");
    let some = params
        .items
        .iter()
        .filter(|i| i.family_bonus.iter().any(|&b| b == 11 || b == 12))
        .count();
    assert_eq!(some, 250, "11 or 12 for one or two families");
    let resisted = params
        .items
        .iter()
        .filter(|i| i.resistances.iter().any(|&r| r != 0))
        .count();
    assert_eq!(resisted, 183);
    let elements = params
        .items
        .iter()
        .filter(|i| (1..=5).contains(&i.weapon_element()))
        .count();
    assert_eq!(elements, 23, "records carrying an element");
}

/// `spelltable.bin`: 65 spell entries, 108 learning entries, every learned place listed, the
/// Minstrel's Heal at 3.
#[test]
fn spell_table_lists_the_spells_and_who_learns_them() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let spells = spell::SpellTable::parse(file(&rom, "data/prm/spelltable.bin")).unwrap();
    let a_data = member(&rom, "data/prm/actdt_a.gp2", "actdt_a_en.nat");
    let b_data = member(&rom, "data/prm/actdt_b.gp2", "actdt_b_en.nat");
    let a = action::Actions::parse(&a_data).unwrap();
    let b = action::Actions::parse(&b_data).unwrap();

    assert_eq!(spells.spells.len(), 65);
    assert_eq!(spells.learned.len(), 108);
    assert_eq!(spells.at(25), None, "place 25 is missing");
    assert_eq!(spells.at(0).unwrap().action, 9, "Frizz");
    let mut unnamed = 0;
    for entry in &spells.spells {
        if entry.action == 0 {
            unnamed += 1;
            continue;
        }
        assert!(
            a.find(entry.action).is_some() || b.find(entry.action).is_some(),
            "spell place {} names action {}, absent from both halves",
            entry.place,
            entry.action
        );
    }
    assert_eq!(unnamed, 3, "places 61, 62 and 65 name action 0");
    for learned in &spells.learned {
        assert!(
            spells.at(learned.place).is_some(),
            "place {} is learned but not listed",
            learned.place
        );
        assert!(learned.level <= 99);
    }
    assert!(
        spells
            .learned
            .iter()
            .any(|l| l.vocation == 6 && l.place == 26 && l.level == 3),
        "the Minstrel learns Heal at 3"
    );
}
