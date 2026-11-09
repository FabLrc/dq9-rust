//! Party invariants on the real game data; skipped when the ROM is absent.
//!
//! The level tables are the cartridge's, and the new-game hero is the value set character
//! creation produces (`pro1.ml1`, `docs/re/party.md`). The save test needs `DQ9_SAVE` (a DS
//! save), like `real_rom.rs`'s position tests.

use dq9_core::party::{BAG_CAPACITY, NEW_GAME_ARMOUR, NEW_GAME_FOOTWEAR, NEW_GAME_LEGWEAR};
use dq9_core::party::{NOVICE_VOCATION, Party};
use dq9_core::state::{GameState, NEW_GAME_WEAPON};
use dq9_formats::hero::Sex;
use dq9_formats::level::{LevelTables, VOCATION_COUNT};
use dq9_formats::save::{EquipSlot, NAME_LEN, NO_ITEM, Save};
use nds_rom::Rom;

fn level_tables(rom: &Rom) -> LevelTables {
    let files: [&[u8]; VOCATION_COUNT] =
        core::array::from_fn(|v| rom.file_by_path(&format!("data/prm/level{v}.bin")).unwrap());
    LevelTables::parse(files).unwrap()
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
