//! Party invariants on the real game data; skipped when the ROM is absent.
//!
//! The level tables are the cartridge's, and the new-game hero is the value set character
//! creation produces (`pro1.ml1`, `docs/re/party.md`). The save test needs `DQ9_SAVE` (a DS
//! save), like `real_rom.rs`'s position tests.

use dq9_core::party::{
    BAG_CAPACITY, GOLD_MAX, NEW_GAME_ARMOUR, NEW_GAME_FOOTWEAR, NEW_GAME_LEGWEAR,
};
use dq9_core::party::{NOVICE_VOCATION, Party};
use dq9_core::state::{GameState, NEW_GAME_WEAPON};
use dq9_formats::Gpc2;
use dq9_formats::battle::equipment::BattleParams;
use dq9_formats::hero::Sex;
use dq9_formats::item::ItemStats;
use dq9_formats::level::{LevelTables, VOCATION_COUNT};
use dq9_formats::save::{EquipSlot, NAME_LEN, NO_ITEM, Save};
use nds_rom::Rom;

fn level_tables(rom: &Rom) -> LevelTables {
    let files: [&[u8]; VOCATION_COUNT] =
        core::array::from_fn(|v| rom.file_by_path(&format!("data/prm/level{v}.bin")).unwrap());
    LevelTables::parse(files).unwrap()
}

/// One English member of an `itemdt_<category>.gp2` archive, decompressed.
fn item_member(rom: &Rom, category: &str) -> Vec<u8> {
    let path = format!("data/prm/itemdt_{category}.gp2");
    let archive = Gpc2::parse(rom.file_by_path(&path).unwrap()).unwrap();
    let name = format!("itemdt_{category}_en.nat");
    archive
        .member(archive.find(&name).unwrap())
        .unwrap()
        .into_owned()
}

/// The eight equipment categories' stats tables.
fn equipment_stats(rom: &Rom) -> ItemStats {
    let members: Vec<Vec<u8>> = ["a", "b", "d", "h", "l", "s", "u", "w"]
        .iter()
        .map(|&category| item_member(rom, category))
        .collect();
    let members: Vec<&[u8]> = members.iter().map(Vec::as_slice).collect();
    ItemStats::from_members(&members).unwrap()
}

fn battle_params(rom: &Rom) -> BattleParams {
    BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat").unwrap()).unwrap()
}

/// The Minstrel's table (`level6`) carries the values the published guide prints, as the wiki
/// records them: level 1's attributes, the skill-point column (12 at 10, 200 at 99) and a
/// rising experience column.
#[test]
fn minstrel_level_table_matches_the_published_guide() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = level_tables(&rom);
    let minstrel = tables.table(6).unwrap();
    let level1 = minstrel.level(1).unwrap();
    assert_eq!(
        (
            level1.experience,
            level1.strength,
            level1.resilience,
            level1.agility,
            level1.deftness,
            level1.charm,
            level1.magical_might,
            level1.magical_mending,
            level1.max_hp,
            level1.max_mp,
            level1.skill_points,
        ),
        (0, 9, 8, 8, 12, 9, 6, 7, 20, 6, 0)
    );
    assert_eq!(minstrel.level(10).unwrap().skill_points, 12);
    assert_eq!(minstrel.level(99).unwrap().skill_points, 200);
    assert!((390..=600).contains(&minstrel.level(99).unwrap().max_hp));
    assert!(
        minstrel
            .levels()
            .windows(2)
            .all(|w| w[0].experience < w[1].experience)
    );
    assert!(minstrel.level(0).is_none() && minstrel.level(100).is_none());
}

/// The hero a new game makes equals character creation's record: Minstrel, level 1, no
/// experience or gold, the copper sword and the opening clothes. The level table fills the
/// attributes, maximum HP/MP and the full current HP/MP.
#[test]
fn new_game_hero_is_the_original_at_the_first_control() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = level_tables(&rom);
    let mut state = GameState::new_game();
    state.party.apply_level_tables(&tables);
    let hero = state.party.leader().unwrap();
    let level = tables.stat(NOVICE_VOCATION, 1).unwrap();
    assert_eq!(hero.vocation, NOVICE_VOCATION);
    assert_eq!(hero.levels, [1; VOCATION_COUNT]);
    assert_eq!(hero.experience, [0; VOCATION_COUNT]);
    assert_eq!(hero.skill_points, 0);
    assert_eq!(hero.sex, Sex::Man);
    assert_eq!(
        (
            hero.stats.strength,
            hero.stats.resilience,
            hero.stats.agility,
            hero.stats.deftness,
            hero.stats.charm,
            hero.stats.magical_might,
            hero.stats.magical_mending,
        ),
        (
            level.strength,
            level.resilience,
            level.agility,
            level.deftness,
            level.charm,
            level.magical_might,
            level.magical_mending,
        )
    );
    assert_eq!((hero.max_hp, hero.max_mp), (level.max_hp, level.max_mp));
    assert_eq!((hero.hp, hero.mp), (level.max_hp, level.max_mp));
    assert_eq!(hero.equipment.torso, Some(NEW_GAME_ARMOUR));
    assert_eq!(hero.equipment.legs, Some(NEW_GAME_LEGWEAR));
    assert_eq!(hero.equipment.feet, Some(NEW_GAME_FOOTWEAR));
    assert_eq!(hero.equipment.weapon, Some(NEW_GAME_WEAPON));
    assert_eq!(hero.equipment.head, None);
    assert_eq!(hero.equipment.shield, None);
    assert_eq!(state.party.gold, 0);
    assert_eq!(state.party.bag.capacity(), BAG_CAPACITY);
    assert!(state.party.bag.is_empty());
}

/// The equipment categories carry the measured stats tables: 268 weapons, 183 armour and
/// 52 accessories, with the indexes dense and unique. The copper sword's attack is 7, the
/// celestial costume's defence 2 and the critical acclaim's critical field 40.
#[test]
fn equipment_stats_have_the_measured_shape() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    for (category, count) in [("w", 268), ("b", 183), ("d", 52)] {
        let member = item_member(&rom, category);
        let stats = ItemStats::from_members(&[&member]).unwrap();
        assert_eq!(stats.len(), count, "category {category}");
    }
    let stats = equipment_stats(&rom);
    assert_eq!(stats.get(20004).unwrap().attack, 7);
    assert_eq!(stats.get(13007).unwrap().defence, 2);
    assert_eq!(stats.get(18049).unwrap().critical, 40);
}

/// The new-game hero's fighter carries the level table's strength plus the copper sword's
/// attack (7) and the costume's defence (2); evasion starts at `GetEvasionRate`'s 2.0.
#[test]
fn the_new_game_hero_fighter_includes_the_copper_sword() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = level_tables(&rom);
    let mut state = GameState::new_game();
    state.party.apply_level_tables(&tables);
    let hero = state.party.leader().unwrap();
    let items = equipment_stats(&rom);
    let fighter = hero.fighter(&items, &battle_params(&rom));
    let worn = |field: fn(&dq9_formats::item::EquipStats) -> u16| -> i32 {
        EquipSlot::ALL
            .iter()
            .filter_map(|&slot| hero.equipment.get(slot))
            .filter_map(|id| items.get(id))
            .map(|stats| i32::from(field(stats)))
            .sum()
    };
    assert_eq!(hero.equipment.weapon, Some(20004));
    assert_eq!(items.get(20004).unwrap().attack, 7);
    assert_eq!(
        fighter.attack,
        i32::from(hero.stats.strength) + worn(|s| s.attack)
    );
    assert_eq!(
        fighter.defence,
        i32::from(hero.stats.resilience) + worn(|s| s.defence)
    );
    assert_eq!(
        (fighter.hp, fighter.max_hp, fighter.mp),
        (
            i32::from(hero.hp),
            i32::from(hero.max_hp),
            i32::from(hero.mp)
        )
    );
    assert_eq!(fighter.block_rate, 0.0);
    assert!(fighter.evasion_rate >= 2.0);
    assert_eq!(fighter.resistances[7], 100);
    assert_eq!(fighter.weapon_element, 0);
}

/// `bat6.ml1`'s record after battle 26: 7 experience in the Minstrel (still level 1), 13
/// gold and one copy of item 22087 in the bag. `pro1.ml1` differs from it only in those
/// three values (`docs/re/party.md`, oracle section).
#[test]
fn award_seven_experience_thirteen_gold_and_the_slimedrop_make_bat6() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = level_tables(&rom);
    let mut state = GameState::new_game();
    state.party.apply_level_tables(&tables);
    let level_ups = state.party.award(7, 13, &[22087], &tables);
    assert!(level_ups.is_empty());
    let hero = state.party.leader().unwrap();
    assert_eq!((hero.level(), hero.current_experience()), (1, 7));
    assert_eq!(state.party.gold, 13);
    assert_eq!(state.party.bag.count(22087), 1);
    // The gold cap is the game's 9,999,999, not a wrap.
    state.party.award(0, GOLD_MAX, &[], &tables);
    assert_eq!(state.party.gold, GOLD_MAX);
}

/// A synthetic award that crosses the Minstrel's levels 1 to 3 takes that table's row: the
/// attributes, the maxima and column 10's skill-point difference, while current HP and MP
/// are left alone (the game's level-up writes the maxima).
#[test]
fn awarding_experience_to_level_three_takes_the_minstrel_rows() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let tables = level_tables(&rom);
    let level1 = *tables.stat(NOVICE_VOCATION, 1).unwrap();
    let level3 = *tables.stat(NOVICE_VOCATION, 3).unwrap();
    let level4 = *tables.stat(NOVICE_VOCATION, 4).unwrap();
    assert!(level3.experience < level4.experience);
    let mut state = GameState::new_game();
    state.party.apply_level_tables(&tables);
    let level_ups = state.party.award(level3.experience, 0, &[], &tables);
    assert_eq!(level_ups.len(), 1);
    let up = level_ups[0];
    assert_eq!((up.member, up.level), (0, 3));
    assert_eq!(up.strength, level3.strength - level1.strength);
    assert_eq!(up.resilience, level3.resilience - level1.resilience);
    assert_eq!(up.agility, level3.agility - level1.agility);
    assert_eq!(up.deftness, level3.deftness - level1.deftness);
    assert_eq!(up.charm, level3.charm - level1.charm);
    assert_eq!(
        up.magical_might,
        level3.magical_might - level1.magical_might
    );
    assert_eq!(
        up.magical_mending,
        level3.magical_mending - level1.magical_mending
    );
    assert_eq!(up.max_hp, level3.max_hp - level1.max_hp);
    assert_eq!(up.max_mp, level3.max_mp - level1.max_mp);
    assert_eq!(up.skill_points, level3.skill_points - level1.skill_points);
    let hero = state.party.leader().unwrap();
    assert_eq!(hero.level(), 3);
    assert_eq!(hero.current_experience(), level3.experience);
    assert_eq!(hero.stats.strength, level3.strength);
    assert_eq!(hero.stats.magical_mending, level3.magical_mending);
    assert_eq!((hero.max_hp, hero.max_mp), (level3.max_hp, level3.max_mp));
    assert_eq!((hero.hp, hero.mp), (level1.max_hp, level1.max_mp));
    assert_eq!(hero.skill_points, level3.skill_points - level1.skill_points);
}

/// A state restored from the demo save lists the slot's four members with the values
/// `dq9re state --save` compares, plus the gold and the bag.
#[test]
fn a_save_restores_its_party_with_the_slot_values() {
    let Some(rom_bytes) = testrom::load() else {
        return;
    };
    let Some(save_bytes) = std::env::var_os("DQ9_SAVE").map(|p| std::fs::read(p).unwrap()) else {
        eprintln!("skipped: no DQ9_SAVE");
        return;
    };
    let _ = Rom::parse(rom_bytes).unwrap();
    let save = Save::parse(&save_bytes).unwrap();
    let slot = save
        .current_slot()
        .and_then(|i| save.slot(i))
        .expect("a valid slot");
    let state = GameState::from_save(&save).unwrap();
    let standby = usize::from(slot.standby_count());
    assert_eq!(state.party.members.len(), usize::from(slot.party_count()));
    for (i, member) in state.party.members.iter().enumerate() {
        let record = slot.character(standby + i).unwrap();
        assert_eq!(member.vocation, record.vocation());
        assert_eq!(
            member.name,
            core::array::from_fn::<_, NAME_LEN, _>(|i| record.name_raw()[i])
        );
        assert_eq!(
            member.sex,
            if record.gender() & 1 == 1 {
                Sex::Woman
            } else {
                Sex::Man
            }
        );
        for v in 0..VOCATION_COUNT {
            assert_eq!(member.levels[v], record.level(v).unwrap());
            assert_eq!(member.experience[v], record.experience(v).unwrap());
        }
        assert_eq!(member.skill_points, record.unallocated_skill_points());
        for equip in EquipSlot::ALL {
            let item = record.equipment(equip);
            assert_eq!(
                member.equipment.get(equip),
                (item != NO_ITEM && item != 0).then_some(item)
            );
        }
        let held = slot.held_items(i).unwrap();
        assert_eq!(
            member.held_items,
            held.map(|item| (item != NO_ITEM && item != 0).then_some(item))
        );
    }
    assert_eq!(state.party.gold, slot.gold_on_hand());
    assert_eq!(state.party.bag.capacity(), BAG_CAPACITY);
    assert_eq!(
        state.party.members.len(),
        Party::from_slot(&slot).unwrap().members.len()
    );
}
