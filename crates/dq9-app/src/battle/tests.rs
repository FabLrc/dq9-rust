//! The played fight on the real tables (skipped without the ROM).

use dq9_core::battle::{BattleResult, Event, Surprise, Who};
use dq9_core::party::Party;

use super::data::Data;
use super::fight::{Fight, Outcome, Pad, Setup, World};
use super::{BattleScene, Fighter};

fn level_one_party(data: &Data) -> Party {
    let mut party = Party::new_game();
    party.apply_level_tables(&data.levels);
    party
}

fn setup(groups: Vec<(u16, u16)>, set_battle: bool) -> Setup {
    Setup {
        groups,
        set_battle,
        surprise: Surprise::None,
        names: vec![b"Hero".to_vec()],
    }
}

/// The fight of `groups` against `party`, on the world seeded with `seed`.
fn fight(
    rom: &[u8],
    party: &Party,
    groups: Vec<(u16, u16)>,
    set: bool,
    seed: u64,
) -> (Fight, World) {
    let data = Data::load(rom).unwrap();
    let mut world = World::seeded(seed);
    let fight = Fight::new(data, party, &setup(groups, set), &mut world).unwrap();
    (fight, world)
}

fn confirm() -> Pad {
    Pad {
        confirm: true,
        ..Pad::default()
    }
}

/// Presses confirm every step until the fight is over or `stop` says so.
fn press_until(fight: &mut Fight, world: &mut World, stop: impl Fn(&Fight) -> bool) {
    for _ in 0..30_000 {
        if fight.is_done() || stop(fight) {
            return;
        }
        fight.step(confirm(), world);
        fight.take_cues();
    }
    panic!("the fight did not settle: {:?}", fight.log());
}

/// What the menus do is what the automatic policy does: confirm held through Fight, Attack and
/// the first target plays the fight Attack-the-first-monster, window for window.
#[test]
fn the_menus_play_the_same_fight_as_the_automatic_policy() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let (mut auto, mut world) = fight(rom, &party, vec![(1, 3)], false, 11);
    auto.play_out(&mut world).unwrap();
    let (mut played, mut world) = fight(rom, &party, vec![(1, 3)], false, 11);
    press_until(&mut played, &mut world, |_| false);
    assert_eq!(played.log(), auto.log());
    assert_eq!(
        played.outcome().map(|o| (o.result, o.experience, o.gold)),
        auto.outcome().map(|o| (o.result, o.experience, o.gold))
    );
}

/// Flight (`Flee` in the first menu): a set battle greys it and refuses it; an encounter allows
/// it and the draw decides, by seed, between getting away (an immediate end) and "the enemy block
/// the way" (the fight goes on).
#[test]
fn flight_depends_on_the_seed_and_a_set_battle_refuses_it() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);

    // A set battle: the row is not selectable and a press on it does nothing.
    let (mut set, mut world) = fight(rom, &party, vec![(1, 1)], true, 1);
    press_until(&mut set, &mut world, |f| f.view().menu.is_some());
    let menu = set.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 4);
    assert!(!menu.rows[1].1, "Flee is greyed in a set battle");
    set.step(
        Pad {
            down: true,
            ..Pad::default()
        },
        &mut world,
    );
    set.step(confirm(), &mut world);
    let menu = set.view().menu.unwrap();
    assert_eq!(
        menu.cursor, 1,
        "the cursor stays on the refused row, the menu stays"
    );

    // An encounter: seeds that get away and seeds that do not, each repeatable.
    let (mut away, mut blocked) = (Vec::new(), Vec::new());
    for seed in 0..24u64 {
        let run = |seed: u64| -> (Option<BattleResult>, bool) {
            let (mut f, mut world) = fight(rom, &party, vec![(1, 3)], false, seed);
            press_until(&mut f, &mut world, |f| f.view().menu.is_some());
            let menu = f.view().menu.unwrap();
            assert!(menu.rows[1].1, "Flee is allowed in an encounter");
            f.step(
                Pad {
                    down: true,
                    ..Pad::default()
                },
                &mut world,
            );
            f.step(confirm(), &mut world);
            // The round plays; the first menu back (or the end) tells how it went.
            press_until(&mut f, &mut world, |f| f.view().menu.is_some());
            let blocked = f.log().iter().any(|l| l.contains("block the way"));
            (f.outcome().map(|o: &Outcome| o.result), blocked)
        };
        let (result, blocked_way) = run(seed);
        assert_eq!((result, blocked_way), run(seed), "seed {seed} must repeat");
        if result == Some(BattleResult::Fled) {
            away.push(seed);
        } else {
            assert!(
                blocked_way,
                "seed {seed}: the flight failed, the enemy block the way"
            );
            blocked.push(seed);
        }
    }
    assert!(
        !away.is_empty() && !blocked.is_empty(),
        "{away:?} / {blocked:?}"
    );
}

/// A level-1 hero against the Hexagoon (a set battle, monster 300) is wiped out: the closing
/// line is the wipe-out's (`str_bres` 20) and the outcome carries no experience.
#[test]
fn the_hexagoon_wipes_out_a_level_one_hero() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let (mut fight, mut world) = fight(rom, &party, vec![(300, 1)], true, 5);
    let outcome = fight.play_out(&mut world).unwrap().clone();
    assert_eq!(outcome.result, BattleResult::Lost);
    assert_eq!((outcome.experience, outcome.gold), (0, 0));
    assert_eq!(
        fight.log().last().map(String::as_str),
        Some("Hero is wiped out!")
    );
    assert_eq!(
        outcome.party.members[0].hp, 0,
        "the hero fell; the wipe-out revives them"
    );
}

/// A win that crosses the next level's experience: the party's experience is the award's, the
/// level line and the skill points line follow the experience line.
#[test]
fn a_win_that_reaches_the_next_level_says_so() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    let next = data
        .levels
        .stat(party.members[0].vocation, 2)
        .unwrap()
        .experience;
    let vocation = usize::from(party.members[0].vocation);
    party.members[0].experience[vocation] = next - 1;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 2);
    let outcome = fight.play_out(&mut world).unwrap().clone();
    assert_eq!(outcome.result, BattleResult::Won);
    assert_eq!(outcome.level_ups.len(), 1);
    assert_eq!(outcome.party.members[0].level(), 2);
    let log = fight.log();
    let at = log
        .iter()
        .position(|l| l.contains("receives some experience"))
        .unwrap();
    assert!(log[at + 1].contains("level increases to"), "{log:?}");
    assert!(log[at + 1].contains("Lv. 2"), "{log:?}");
    assert_eq!(outcome.party.gold, outcome.gold);
}

/// The cancel key backs out one menu at a time, never past the first: Fight → Attack → target
/// and back to the top, and a refused row (Defend, Items) keeps the cursor where it is.
#[test]
fn cancel_backs_out_of_the_menus_one_level_at_a_time() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 2)], false, 6);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    let press = |f: &mut Fight, world: &mut World, pad: Pad| {
        f.step(pad, world);
        f.view().menu.map(|m| (m.rows.len(), m.cursor, m.targets))
    };
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    let cancel = Pad {
        cancel: true,
        ..Pad::default()
    };
    // Top menu: Fight, Flee, Examine, Misc.
    assert_eq!(
        press(&mut fight, &mut world, Pad::default()),
        Some((4, 0, false))
    );
    assert_eq!(
        press(&mut fight, &mut world, cancel),
        Some((4, 0, false)),
        "nothing before"
    );
    // Fight: Attack, Spells, Defend, Abilities, Items; the hero knows no spell or ability at
    // level 1 and the bag is empty, Defend is allowed.
    assert_eq!(
        press(&mut fight, &mut world, confirm()),
        Some((5, 0, false))
    );
    assert_eq!(press(&mut fight, &mut world, down), Some((5, 1, false)));
    let menu = fight.view().menu.unwrap();
    assert!(
        !menu.rows[1].1 && menu.rows[2].1 && !menu.rows[3].1 && !menu.rows[4].1,
        "spells and Items grey, Defend allowed, no ability known"
    );
    assert_eq!(
        press(&mut fight, &mut world, confirm()),
        Some((5, 1, false)),
        "a grey row stays"
    );
    // Attack opens the target list: the two standing slimes, with the scene's marker.
    fight.step(
        Pad {
            up: true,
            ..Pad::default()
        },
        &mut world,
    );
    assert_eq!(press(&mut fight, &mut world, confirm()), Some((2, 0, true)));
    assert!(fight.view().marker.is_some());
    assert_eq!(press(&mut fight, &mut world, down), Some((2, 1, true)));
    assert_eq!(
        press(&mut fight, &mut world, cancel),
        Some((5, 0, false)),
        "back to Fight"
    );
    assert_eq!(
        press(&mut fight, &mut world, cancel),
        Some((4, 0, false)),
        "back to the top"
    );
    assert!(fight.view().marker.is_none());
}

/// Defend: the row is allowed and the member says so (action 3, `actmsg` 10); the round goes on
/// with the slimes' blows.
#[test]
fn defend_says_so_and_the_round_goes_on() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    // Confirm on Fight, two downs to Defend, confirm: then hold confirm through the windows.
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 3)], false, 21);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    fight.step(down, &mut world);
    fight.step(down, &mut world);
    assert_eq!(fight.view().menu.unwrap().cursor, 2);
    fight.step(confirm(), &mut world);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    let log = fight.log();
    assert!(log.iter().any(|l| l == "Hero defends."), "{log:?}");
    let blows: Vec<u32> = log
        .iter()
        .filter_map(|l| {
            l.strip_prefix("Hero takes ")?
                .split(' ')
                .next()?
                .parse()
                .ok()
        })
        .collect();
    assert!(!blows.is_empty());
}

/// A learned skill panel is offered in the Abilities menu with its action and MP cost; an
/// action the engine refuses stays greyed and is never sent, and the one it models resolves
/// like any action, spends its MP and deals its damage.
#[test]
fn a_learned_ability_is_cast_and_an_unmodelled_one_stays_greyed() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    // Panel 0 is Dragon Slash (action 63, damage handler 1: refused); panel 8 is Gigaslash
    // (action 67, 15 MP, damage handler 0, reach 4).
    party.members[0].skill_panels[0] = 1 << 0;
    party.members[0].skill_panels[1] = 1 << 0;
    party.members[0].mp = 15;
    party.members[0].max_mp = 15;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 31);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..3 {
        fight.step(down, &mut world);
    }
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.cursor, 3);
    assert!(menu.rows[3].1, "Abilities is allowed with a learned panel");
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(
        menu.rows.len(),
        2,
        "one row per learned panel that names an action"
    );
    assert_eq!(String::from_utf8_lossy(&menu.rows[0].0), "Dragon Slash  0");
    assert!(!menu.rows[0].1, "the kernel does not model action 63");
    assert_eq!(String::from_utf8_lossy(&menu.rows[1].0), "Gigaslash  15");
    assert!(menu.rows[1].1, "Gigaslash is modelled and paid for");
    // Confirming the refused row changes nothing.
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.cursor, 0, "the cursor stays on the refused row");
    assert!(!menu.targets);
    fight.step(down, &mut world);
    fight.step(confirm(), &mut world);
    // Reach 4 asks for a target, then the round resolves.
    assert!(fight.view().menu.unwrap().targets);
    fight.step(confirm(), &mut world);
    assert_eq!(fight.battle().party()[0].fighter.mp, 0, "15 MP spent");
    assert_eq!(fight.battle().monsters()[0].fighter.hp, 0, "the slime fell");
    press_until(&mut fight, &mut world, |_| false);
    assert!(
        fight.log().iter().any(|l| l == "Hero uses Gigaslash."),
        "{:?}",
        fight.log()
    );
}

/// An ability the member cannot pay for stays greyed: confirming it neither takes the command
/// nor spends anything.
#[test]
fn an_ability_the_member_cannot_pay_for_stays_greyed() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    // Gigaslash (panel 8) costs 15 MP; the level-1 Minstrel has 6.
    party.members[0].skill_panels[1] = 1 << 0;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 32);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..3 {
        fight.step(down, &mut world);
    }
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 1);
    assert!(!menu.rows[0].1, "15 MP against the member's 6");
    let mp = fight.battle().party()[0].fighter.mp;
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.cursor, 0, "the cursor stays on the refused row");
    assert!(!menu.targets);
    assert_eq!(fight.battle().party()[0].fighter.mp, mp);
    assert!(
        fight.log().iter().all(|l| !l.contains("Gigaslash")),
        "{:?}",
        fight.log()
    );
}

/// A held item of the acting member: the medicinal herb in slot 0 is offered, heals a wounded
/// member and leaves that held slot; the bag holds the same item and is neither read nor
/// changed.
#[test]
fn a_held_medicinal_herb_is_used_and_leaves_its_slot() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    party.members[0].held_items[0] = Some(22000);
    assert!(party.bag.add_copies(22000, 2));
    party.members[0].hp = 5;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 8);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    let menu = fight.view().menu.unwrap();
    assert!(menu.rows[4].1, "Items is allowed with a held herb");
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 1);
    assert!(menu.rows[0].1);
    fight.step(confirm(), &mut world);
    // The herb heals one of the party: the target list is the party's.
    let menu = fight.view().menu.unwrap();
    assert!(menu.targets && menu.rows.len() == 1);
    fight.step(confirm(), &mut world);
    // The round resolved in that step: the engine healed the fighter and the held slot is
    // spent, while the bag still holds its two copies.
    assert!(fight.battle().party()[0].fighter.hp > 5);
    assert_eq!(
        fight.party().members[0].held_items,
        [None, None, None, None, None, None, None, None]
    );
    assert_eq!(fight.party().bag.count(22000), 2);
    press_until(&mut fight, &mut world, |_| false);
    let outcome = fight.outcome().unwrap();
    assert_eq!(outcome.party.members[0].held_items, [None; 8]);
    assert_eq!(outcome.party.bag.count(22000), 2, "the bag is untouched");
}

/// The bag alone does not feed the battle's Items command: a herb only in the bag leaves the
/// row grey, is never offered, and the bag keeps every copy.
#[test]
fn a_bag_herb_is_not_offered_in_battle() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    assert!(party.bag.add_copies(22000, 2));
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 9);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert!(!menu.rows[3].1, "Items is grey without a held item");
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    assert_eq!(fight.view().menu.unwrap().cursor, 4);
    fight.step(confirm(), &mut world);
    assert!(
        fight.view().menu.unwrap().rows.len() == 5,
        "the Fight menu stays"
    );
    fight.play_out(&mut world).unwrap();
    let outcome = fight.outcome().unwrap();
    assert_eq!(outcome.party.bag.count(22000), 2);
}

/// Duplicate held slots of the same item, separated by another item: choosing the second herb
/// spends that slot only. The exact ordered slots catch a wrong same-id slot, and the bag's
/// own copy of the herb stays.
#[test]
fn duplicate_held_slots_spend_only_the_chosen_one() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    party.members[0].held_items[0] = Some(22001);
    party.members[0].held_items[1] = Some(22000);
    party.members[0].held_items[2] = Some(22001);
    party.members[0].held_items[3] = Some(22000);
    assert!(party.bag.add_copies(22000, 2));
    party.members[0].hp = 5;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 10);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 4, "one row per held slot");
    // The fourth row is the second herb (slot 3), after another item.
    for _ in 0..3 {
        fight.step(down, &mut world);
    }
    assert_eq!(fight.view().menu.unwrap().cursor, 3);
    fight.step(confirm(), &mut world);
    fight.step(confirm(), &mut world);
    assert_eq!(
        fight.party().members[0].held_items,
        [
            Some(22001),
            Some(22000),
            Some(22001),
            None,
            None,
            None,
            None,
            None
        ],
        "slot 3 must be the one spent"
    );
    assert_eq!(fight.party().bag.count(22000), 2);
    press_until(&mut fight, &mut world, |_| false);
    let outcome = fight.outcome().unwrap();
    assert_eq!(outcome.party.bag.count(22000), 2);
}

/// Cancelling an item command releases the reservation: backing out of the target list and
/// choosing Defend spends nothing, and the held slot and the bag are as they were.
#[test]
fn a_cancelled_item_command_spends_nothing() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    party.members[0].held_items[0] = Some(22000);
    assert!(party.bag.add_copies(22000, 2));
    party.members[0].hp = 5;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 12);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    fight.step(confirm(), &mut world);
    // The herb asks for a party target; cancel it, back to Fight (cursor on Items).
    fight.step(confirm(), &mut world);
    assert!(fight.view().menu.unwrap().targets);
    let cancel = Pad {
        cancel: true,
        ..Pad::default()
    };
    fight.step(cancel, &mut world);
    fight.step(cancel, &mut world);
    assert_eq!(fight.view().menu.unwrap().cursor, 4);
    // Defend is index 2: two ups from Items.
    for _ in 0..2 {
        fight.step(
            Pad {
                up: true,
                ..Pad::default()
            },
            &mut world,
        );
    }
    assert_eq!(fight.view().menu.unwrap().cursor, 2);
    fight.step(confirm(), &mut world);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    press_until(&mut fight, &mut world, |_| false);
    let outcome = fight.outcome().unwrap();
    assert_eq!(
        outcome.party.members[0].held_items,
        [Some(22000), None, None, None, None, None, None, None]
    );
    assert_eq!(outcome.party.bag.count(22000), 2);
}

/// A member who falls before its turn keeps its item: the engine reports `Skipped` for it and
/// no `ItemUsed`, so its held slot survives. The second member is slow and fragile; the seed
/// makes the monster act first and strike it down.
#[test]
fn an_actor_defeated_before_its_turn_keeps_its_held_item() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    // Seed 5: the Hexagoon acts before the slow second member and knocks it out.
    let seed = 5;
    let mut party = level_one_party(&data);
    let mut second = party.members[0].clone();
    second.stats.agility = 0;
    second.hp = 1;
    second.held_items[0] = Some(22000);
    party.members[0].held_items[0] = Some(22000);
    party.members.push(second);
    let (mut fight, mut world) = fight(rom, &party, vec![(300, 1)], true, seed);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    // Both members choose the herb in slot 0 and aim it at the first party member; the last
    // press settles the second command and resolves the round.
    for _ in 0..2 {
        fight.step(confirm(), &mut world);
        let down = Pad {
            down: true,
            ..Pad::default()
        };
        for _ in 0..4 {
            fight.step(down, &mut world);
        }
        fight.step(confirm(), &mut world);
        fight.step(confirm(), &mut world);
        assert!(fight.view().menu.unwrap().targets);
        fight.step(confirm(), &mut world);
    }
    let events = fight.last_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Skipped(Who::Party(1)))),
        "seed {seed}: the engine did not skip the second member"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::ItemUsed {
                who: Who::Party(1),
                ..
            }
        )),
        "seed {seed}: the skipped member reported an item use"
    );
    assert_eq!(
        fight.party().members[1].held_items[0],
        Some(22000),
        "seed {seed}: the skipped member's held slot was spent"
    );
    assert_eq!(fight.party().members[0].held_items[0], None);
    assert_eq!(fight.party().bag.count(22000), 0);
}

/// The sage's stone (22035, action 416) is reusable: its record leaves the used-up bit clear.
/// If the kernel models action 416, using it heals and keeps the held item; otherwise the row
/// is visibly disabled and the item is never spent.
#[test]
fn a_reusable_held_item_is_never_spent() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    party.members[0].held_items[0] = Some(22035);
    party.members[0].hp = 5;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 13);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 1);
    assert!(menu.rows[0].1, "action 416 is modelled by the kernel");
    fight.step(confirm(), &mut world);
    fight.step(confirm(), &mut world);
    assert!(fight.battle().party()[0].fighter.hp > 5);
    assert_eq!(fight.party().members[0].held_items[0], Some(22035));
    assert_eq!(fight.party().bag.count(22035), 0);
}

/// A worn item whose record names a battle action is offered in Items after the held tools,
/// resolves that action through `Command::Item` and is kept: the consumer clears a held slot
/// only for a tool's record (`func_020ddb38`), never the equipment. The cautery sword (20010)
/// carries action 369, a kind-1 blow; the celestial costume (13007) carries none (252) and has
/// no row.
#[test]
fn a_usable_worn_item_is_offered_used_and_kept() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    party.members[0].equipment.weapon = Some(20010);
    party.members[0].equipment.torso = Some(13007);
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 17);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 1, "only the sword names a battle action");
    assert_eq!(menu.rows[0].0.as_slice(), "cautery sword".as_bytes());
    assert!(menu.rows[0].1, "action 369 is modelled by the kernel");
    // Reach 3 is a single chosen target: the command settles and the round resolves.
    fight.step(confirm(), &mut world);
    assert!(
        fight.last_events().iter().any(|event| matches!(
            event,
            Event::ItemUsed {
                who: Who::Party(0),
                action: 369
            }
        )),
        "the worn sword did not report its use"
    );
    assert_eq!(
        fight.party().members[0].equipment.weapon,
        Some(20010),
        "a worn item is never spent"
    );
    assert_eq!(fight.party().members[0].equipment.torso, Some(13007));
    assert_eq!(fight.party().members[0].held_items, [None; 8]);
}

/// A worn record without a battle action (the record's `+0x16` is 252) has no row: the
/// original appends every worn entry, but confirming one whose action the battle's table lacks
/// refuses it (`func_ov000_02171ffc`), so the app lists only the rows it can use, as it already
/// does for held items. The new-game equipment (copper sword, celestial costume, stockings,
/// shoes) all carry 252.
#[test]
fn a_worn_item_without_a_battle_action_is_not_listed() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    party.members[0].held_items[0] = Some(22000);
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 18);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 1, "the worn records name no battle action");
    assert_eq!(menu.rows[0].0.as_slice(), "medicinal herb".as_bytes());
}

/// Battle 26's monsters hold the Flee way: a monster that flees says so and leaves with no
/// reward (`docs/re/battle.md`, "Flee `0xE1`"). Seed 7: the slime and the cruelcumber that
/// have not fallen leave, the battle is won and pays only for the slime that was beaten.
#[test]
fn monsters_that_flee_leave_and_pay_nothing() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let (mut fight, mut world) = fight(rom, &party, vec![(290, 1), (292, 1), (290, 1)], true, 7);
    let outcome = fight.play_out(&mut world).unwrap().clone();
    assert_eq!(outcome.result, BattleResult::Won);
    assert_eq!(
        (outcome.experience, outcome.gold),
        (2, 4),
        "one slime beaten"
    );
    let log = fight.log();
    let at = log.iter().position(|l| l == "The slime flees.").unwrap();
    assert_eq!(log[at + 1], "The cruelcumber flees.");
    assert_eq!(log[at + 2], "The enemy are defeated.");
}

/// A call for backup (monster 43, action 344): the caller says so and the record's line says
/// a monster shows up; the new monster is named, targeted and fought like the others, and
/// every seed still ends.
#[test]
fn a_call_for_backup_brings_a_monster_that_is_fought() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let mut called = 0;
    for seed in 0..12u64 {
        let (mut fight, mut world) = fight(rom, &party, vec![(43, 1)], false, seed);
        fight.play_out(&mut world).unwrap();
        let log = fight.log();
        if let Some(at) = log
            .iter()
            .position(|l| l == "The bag o' laughs calls for backup.")
        {
            called += 1;
            assert!(
                log[at + 1] == "A bag o' laughs shows up." || log[at + 1] == "But nobody shows up.",
                "{:?}",
                &log[at..=at + 1]
            );
        }
        assert!(fight.battle().monsters().len() <= 5, "seed {seed}");
    }
    assert!(called > 0, "some seed calls for backup");
}

/// The battle archives of a monster: its body, the rest of its motions (`.cmot`) and its
/// action script. The body alone has no `damage` or `death`; the `.cmot` beside it does.
#[test]
fn battle_bodies_carry_their_blows_in_the_cmot() {
    let Some(rom) = testrom::load() else { return };
    let archive = super::motion::monster_members(rom, "z000a").unwrap();
    let cmot = archive.motions.expect("the slime has a .cmot");
    assert!(archive.script.is_some(), "the slime has a .bact");
    let body_spans = super::motion::motion_spans(&[&archive.body]);
    let all = super::motion::motion_spans(&[&archive.body, &cmot]);
    for name in ["stand", "attack0a", "appear"] {
        assert!(
            super::motion::span_of(&body_spans, name).is_some(),
            "the body has {name}"
        );
    }
    for name in ["attack1a", "damage", "death", "escape"] {
        assert!(
            super::motion::span_of(&all, name).is_some(),
            "the .cmot adds {name}"
        );
        assert!(
            super::motion::span_of(&body_spans, name).is_none(),
            "the body alone has no {name}"
        );
    }
}

/// The action scripts of the fighters the chapter 2 companies use: the hero's, the slime's
/// and the Hexagoon's choose `attack1a` from apart, with their own lunge and landing point
/// (`docs/re/battle-data.md`, "Action scripts").
#[test]
fn the_action_scripts_pick_the_blow_the_lunge_and_the_hit() {
    let Some(rom) = testrom::load() else { return };
    let scripts = super::motion::Scripts::load(rom).unwrap();
    let hero = scripts.hero(1, 9.0).expect("the hero attacks");
    assert_eq!(hero.motion, "attack1a");
    let hero_lunge = hero.lunge.expect("the hero's blow lunges");
    assert!((hero_lunge.from - 0.06).abs() < 1e-3, "{hero_lunge:?}");
    assert!((hero_lunge.to - 0.33).abs() < 1e-3, "{hero_lunge:?}");
    assert!((hero_lunge.gap - 0.25).abs() < 1e-3, "{hero_lunge:?}");
    assert!((hero.hit - 0.61).abs() < 1e-3, "{}", hero.hit);

    let bact = super::motion::monster_members(rom, "z000a")
        .unwrap()
        .script
        .unwrap();
    let slime = super::motion::Script::parse(&bact).unwrap();
    let slime = super::motion::plan(slime.section(1).unwrap(), 9.0).unwrap();
    assert_eq!(slime.motion, "attack1a");
    let slime_lunge = slime.lunge.unwrap();
    assert!((slime_lunge.from - 0.120).abs() < 1e-3, "{slime_lunge:?}");
    assert!((slime_lunge.to - 0.407).abs() < 1e-3, "{slime_lunge:?}");
    assert!((slime.hit - 0.58).abs() < 1e-3, "{}", slime.hit);

    let bact = super::motion::monster_members(rom, "b003a")
        .unwrap()
        .script
        .unwrap();
    let hexagoon = super::motion::Script::parse(&bact).unwrap();
    let hexagoon = super::motion::plan(hexagoon.section(1).unwrap(), 9.0).unwrap();
    assert_eq!(hexagoon.motion, "attack1a");
    let hexagoon_lunge = hexagoon.lunge.unwrap();
    assert!((hexagoon_lunge.from - 0.110).abs() < 1e-3);
    assert!((hexagoon_lunge.to - 0.350).abs() < 1e-3);
    assert!((hexagoon_lunge.gap - 0.5).abs() < 1e-3);
    assert!((hexagoon.hit - 0.55).abs() < 1e-3);
}

/// The equipped weapon's set poses the blow; bare-handed is the body's own `mp0200be`. The
/// two `attack1a` clips last the same (frames 1..19 at 0.25) but are different animations, so
/// the posed vertices tell which set the scene actually loaded.
#[test]
fn the_equipped_weapon_poses_the_blow() {
    let Some(rom) = testrom::load() else { return };
    let hero = Fighter::party(0);
    let slime = Fighter::monster(0);
    let posed = |scene: &mut BattleScene| {
        scene.act(hero, 1, &[slime], &[slime]);
        for _ in 0..15 {
            scene.step().unwrap();
        }
        scene
            .runtime()
            .hero_vertices()
            .unwrap()
            .iter()
            .take(400)
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>()
    };
    // The scene starts on the new game's copper sword (set 1, `mp0201be`).
    let mut equipped = battle_scene(rom, vec![(1, 1)]);
    let sword = posed(&mut equipped);
    let mut bare = battle_scene(rom, vec![(1, 1)]);
    bare.set_hero_weapon(rom, None).unwrap();
    let hand = posed(&mut bare);
    assert_eq!(
        sword.len(),
        hand.len(),
        "the same rig, one animation set each"
    );
    assert_ne!(
        sword, hand,
        "the sword's attack1a is not the bare-handed one"
    );
}

/// A fighter's blow steps in on its script's lunge, the struck one plays `damage` when the
/// blow lands, and the blow returns the fighter to its place; a second command during the
/// lunge does not leave it out of place.
#[test]
fn a_blow_lunges_hits_and_comes_back() {
    let Some(rom) = testrom::load() else { return };
    let mut scene = battle_scene(rom, vec![(1, 1)]);
    let hero = Fighter::party(0);
    let slime = Fighter::monster(0);
    let base = scene.fighter(hero).unwrap().position;
    let target = scene.fighter(slime).unwrap().position;
    scene.act(hero, 1, &[slime], &[slime]);
    assert_eq!(
        scene.gesture(hero).map(|(motion, _)| motion),
        Some("attack1a")
    );
    let mut moved = false;
    let mut reacted = false;
    for _ in 0..120 {
        scene.step().unwrap();
        let at = scene.drawn_position(hero).unwrap();
        if (at.z - target.z).abs() < 2.0 {
            moved = true;
        }
        if scene
            .gesture(slime)
            .is_some_and(|(motion, _)| motion == "damage")
        {
            reacted = true;
        }
    }
    assert!(moved, "the actor stepped in");
    assert!(reacted, "the target reacted");
    assert_eq!(scene.gesture(hero), None, "the blow is over");
    assert_eq!(
        scene.drawn_position(hero).unwrap(),
        base,
        "back on its place"
    );

    // A command during a lunge replaces it and still comes back: a cancelled or rapidly
    // confirmed action leaves no fighter displaced.
    scene.act(hero, 1, &[slime], &[slime]);
    for _ in 0..12 {
        scene.step().unwrap();
    }
    assert_ne!(scene.drawn_position(hero).unwrap(), base);
    scene.act(hero, 1, &[slime], &[slime]);
    assert_eq!(scene.drawn_position(hero).unwrap(), base);
    for _ in 0..120 {
        scene.step().unwrap();
    }
    assert_eq!(scene.drawn_position(hero).unwrap(), base);
}

/// A defeated monster plays `death`, then disappears once; a knocked-out actor cannot play a
/// later action. A monster that flees plays `escape` and goes.
#[test]
fn death_and_flight_hide_once_and_a_fallen_actor_stays_still() {
    let Some(rom) = testrom::load() else { return };
    let mut scene = battle_scene(rom, vec![(1, 2)]);
    let slime = Fighter::monster(0);
    let other = Fighter::monster(1);
    scene.kill(slime);
    assert_eq!(
        scene.gesture(slime).map(|(motion, _)| motion),
        Some("death")
    );
    for _ in 0..200 {
        scene.step().unwrap();
    }
    assert!(!scene.fighter(slime).unwrap().alive);
    assert!(!scene.remove(slime), "the model is only hidden once");
    // Its later action (the engine skips a fallen actor) animates nothing.
    scene.act(slime, 1, &[other], &[other]);
    assert_eq!(scene.gesture(slime), None);
    assert_eq!(scene.drawn_position(slime), None);

    scene.flee(other);
    assert_eq!(
        scene.gesture(other).map(|(motion, _)| motion),
        Some("escape")
    );
    for _ in 0..200 {
        scene.step().unwrap();
    }
    assert!(!scene.fighter(other).unwrap().alive);
}

/// A call for backup adds the called monster, which plays `appear`, and its caller plays
/// `call` (`default.bact` 347/242). The joined monster is `Fighter::monster(1)` and reacts to
/// a blow with its own `damage` animation, so the scene's fighter list stays in step.
#[test]
fn a_called_monster_appears_and_reacts() {
    let Some(rom) = testrom::load() else { return };
    let mut scene = battle_scene(rom, vec![(1, 1)]);
    let caller = Fighter::monster(0);
    scene.summon(rom, caller, 1).unwrap();
    assert_eq!(scene.fighters().len(), 3);
    assert_eq!(
        scene.gesture(caller).map(|(motion, _)| motion),
        Some("call")
    );
    let joined = Fighter::monster(1);
    assert_eq!(scene.fighter(joined).unwrap().number, Some(1));
    assert_eq!(
        scene.animation(joined).map(|(motion, _)| motion),
        Some("appear"),
        "the called monster plays its own appear, not an idle"
    );
    scene.act(Fighter::party(0), 1, &[joined], &[joined]);
    let mut reacted = false;
    for _ in 0..150 {
        scene.step().unwrap();
        if scene
            .animation(joined)
            .is_some_and(|(motion, _)| motion == "damage")
        {
            reacted = true;
            assert!(
                scene
                    .gesture(joined)
                    .is_some_and(|(motion, _)| motion == "damage")
            );
            break;
        }
    }
    assert!(reacted, "the joined monster reacts to the blow it took");
    assert_eq!(scene.fighter(joined).unwrap().number, Some(1));
}

/// Weird Dance (243) is not one of the spell archives' files: the wiki's dispatch reads the
/// fighter's own script first, and `z017a.bact` serves it under key 243 with `skill1`. Defend
/// is `default.bact`'s `guard`.
#[test]
fn weird_dance_is_its_monsters_own_script_and_defend_is_the_fallback() {
    let Some(rom) = testrom::load() else { return };
    let script = super::motion::monster_members(rom, "z017a")
        .unwrap()
        .script
        .unwrap();
    let script = super::motion::Script::parse(&script).unwrap();
    let blow = super::motion::plan(script.section(243).unwrap(), 9.0).unwrap();
    // The ROM has no `sp243.bact` in any of the three spell/skill archives, so the monster's
    // own section is the only script: the wiki's dispatch reads it first.
    assert_eq!(blow.motion, "skill1");

    let mut scene = battle_scene(rom, vec![(43, 1)]);
    let monster = Fighter::monster(0);
    let hero = Fighter::party(0);
    scene.act(monster, 243, &[hero], &[]);
    assert_eq!(
        scene.gesture(monster).map(|(motion, _)| motion),
        Some("skill1")
    );
    assert_eq!(
        scene.animation(monster).map(|(motion, _)| motion),
        Some("skill1"),
        "the monster plays its own skill motion"
    );
    for _ in 0..200 {
        scene.step().unwrap();
    }
    scene.act(hero, 3, &[], &[]);
    assert_eq!(
        scene.animation(hero).map(|(motion, _)| motion),
        Some("guard")
    );
}

/// Repeated commands keep the newest motion: settling the previous one plays an idle, which
/// must come before the new motion, not after it. The renderer's own clip name and frame are
/// asserted, not the position or the timeline alone.
#[test]
fn a_repeated_command_keeps_the_new_motion_playing() {
    let Some(rom) = testrom::load() else { return };
    let mut scene = battle_scene(rom, vec![(1, 1)]);
    let hero = Fighter::party(0);
    let slime = Fighter::monster(0);
    scene.act(hero, 1, &[slime], &[slime]);
    for _ in 0..10 {
        scene.step().unwrap();
    }
    let (motion, frame) = scene.animation(hero).unwrap();
    assert_eq!(motion, "attack1a");
    // A second command while the first is still swinging: its settle plays `stand` first.
    scene.act(hero, 1, &[slime], &[slime]);
    let (motion, first) = scene.animation(hero).unwrap();
    assert_eq!(
        motion, "attack1a",
        "the settle's stand replaced the new blow"
    );
    assert!(scene.gesture(hero).is_some_and(|(_, age)| age == 0));
    scene.step().unwrap();
    let (motion, next) = scene.animation(hero).unwrap();
    assert_eq!(motion, "attack1a");
    assert_ne!(first, next, "the new clip advances");
    assert_ne!(frame, first);
}

/// The windows wait for the scene: a damage number, a death or a flight cannot start while a
/// blow is still on its way to its landing, and the end of the fight cannot cut a death or a
/// flight. The engine's outcome stays the one the headless policy gets from the same seed.
#[test]
fn the_windows_wait_for_hits_deaths_and_flights() {
    let Some(rom) = testrom::load() else { return };
    let groups = vec![(1, 3)];
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let (mut reference, mut reference_world) = fight(rom, &party, groups.clone(), false, 11);
    reference.play_out(&mut reference_world).unwrap();
    let expected = reference
        .outcome()
        .map(|outcome| (outcome.result, outcome.experience, outcome.gold));

    let (mut played, mut world) = fight(rom, &party, groups.clone(), false, 11);
    let mut scene = battle_scene(rom, groups);
    let (mut saw_damage, mut saw_death) = (false, false);
    for steps in 0..60_000 {
        if played.is_done() {
            break;
        }
        let presenting = scene.presenting();
        let (advanced, _) =
            super::advance_fight(&mut played, &mut scene, &mut world, confirm(), Some(rom));
        assert_eq!(
            advanced, !presenting,
            "a window advanced while the scene was still presenting (step {steps})"
        );
        scene.step().unwrap();
        // A number stays up while the next actor swings; only its first frame, when the
        // window that raised it opened, proves the blow had landed.
        if played
            .view()
            .popups
            .iter()
            .any(|popup| popup.kind == super::narrate::PopupKind::Damage && popup.age == 0)
        {
            saw_damage = true;
            assert!(
                !scene.pending_hit(),
                "a damage number was raised before the blow landed (step {steps})"
            );
        }
        for placed in scene.fighters() {
            if scene.falling(placed.fighter) {
                saw_death = true;
                assert!(
                    !scene.pending_hit(),
                    "a death or a flight began before the blow landed (step {steps})"
                );
            }
        }
    }
    assert!(
        played.is_done(),
        "the fight did not settle: {:?}",
        played.log()
    );
    assert!(saw_damage && saw_death);
    assert!(
        !scene.presenting(),
        "the fight closed while a death or a flight was playing"
    );
    assert_eq!(
        played
            .outcome()
            .map(|outcome| (outcome.result, outcome.experience, outcome.gold)),
        expected,
        "gating the windows changed the engine's outcome"
    );
}

/// `--battle-capture swing` stops on the frame a blow has just landed and is still shown: the
/// actor at its target, not back on the grid as the later `damage` phase is.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_capture_can_stop_at_the_swing() {
    let Some(rom) = testrom::load() else { return };
    let groups = vec![(1, 1)];
    let (mut played, mut world) = fight(
        rom,
        &level_one_party(&Data::load(rom).unwrap()),
        groups.clone(),
        false,
        7,
    );
    let mut scene = battle_scene(rom, groups);
    // The caller's party has no weapon here: the scene must take the bare-handed set, which
    // is what `--battle-capture` passes from `Party::new_game`.
    crate::app::run_to_phase(&mut played, &mut scene, &mut world, "swing", rom, None).unwrap();
    assert!(scene.presenting(), "the blow is still shown");
    assert!(!scene.pending_hit(), "its landing has happened");
}

/// A capture run has the ROM, so a call for backup adds its model to the scene instead of
/// being dropped (the cue's `Summon` reaches `BattleScene::summon`).
#[test]
fn a_capture_run_adds_a_called_monster() {
    let Some(rom) = testrom::load() else { return };
    let groups = vec![(43, 1)];
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let mut seed = None;
    for candidate in 0..12u64 {
        let (mut probe, mut probe_world) = fight(rom, &party, groups.clone(), false, candidate);
        probe.play_out(&mut probe_world).unwrap();
        if probe.battle().monsters().len() > 1 {
            seed = Some(candidate);
            break;
        }
    }
    let seed = seed.expect("a seed calls for backup that arrives");
    let (mut played, mut world) = fight(rom, &party, groups.clone(), false, seed);
    let mut scene = battle_scene(rom, groups);
    let before = scene.fighters().len();
    let mut joined = false;
    for _ in 0..60_000 {
        if played.is_done() || joined {
            break;
        }
        let (_, added) =
            super::advance_fight(&mut played, &mut scene, &mut world, confirm(), Some(rom));
        joined |= added;
        scene.step().unwrap();
    }
    assert!(
        joined,
        "the call for backup reached the scene (seed {seed})"
    );
    assert_eq!(scene.fighters().len(), before + 1);
    let joined = Fighter::monster(1);
    assert_eq!(scene.fighter(joined).unwrap().number, Some(43));
    assert_eq!(
        scene.animation(joined).map(|(motion, _)| motion),
        Some("appear"),
        "the called monster plays its own appear"
    );
}

/// A renderer that already owns the battle must show the same lunges and removals as a
/// newly built renderer. Rebuilding only for captures used to conceal stale GPU worlds.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_existing_gpu_battle_moves_and_hides_its_fighters() {
    use dq9_render::{Renderer, Viewport};

    let Some(rom) = testrom::load() else { return };
    let (device, queue) = pollster::block_on(async {
        let instance = crate::gpu::instance().await;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("a GPU adapter for the native battle regression");
        crate::gpu::device(&adapter).await.unwrap()
    });
    let mut scene = battle_scene(rom, vec![(1, 1)]);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&device, &queue, format, &scene.runtime().scene);
    renderer.environment = scene.environment.clone();
    let hero = Fighter::party(0);
    let slime = Fighter::monster(0);
    let home = scene.runtime().scene.instances[scene.instances[0]].world;
    scene.act(hero, 1, &[slime], &[slime]);
    for _ in 0..40 {
        scene.step().unwrap();
    }
    assert_ne!(
        scene.runtime().scene.instances[scene.instances[0]].world,
        home,
        "the hero is lunging, not on its grid place"
    );
    for hidden in [false, true] {
        if hidden {
            scene.clear_gestures();
            assert!(scene.remove(slime));
            assert!(!scene.fighter(slime).unwrap().alive);
        }
        scene.upload(&mut renderer, &device, &queue).unwrap();
        let mut rebuilt = Renderer::new(&device, &queue, format, &scene.runtime().scene);
        rebuilt.environment = scene.environment.clone();
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 256,
            height: 192,
        };
        let frame = |renderer: &mut Renderer| {
            renderer
                .render_to_rgba_with(
                    &device,
                    &queue,
                    (256, 192),
                    viewport,
                    scene.camera(),
                    |_, _| {},
                )
                .unwrap()
        };
        let actual = frame(&mut renderer);
        let expected = frame(&mut rebuilt);
        let different = actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .filter(|(actual, expected)| actual != expected)
            .count();
        assert_eq!(
            different, 0,
            "stale GPU fighter transforms (hidden: {hidden})"
        );
    }
}

/// A scene of `monsters` on the field stage, as `--battle-stage 30116 --monsters` builds it.
fn battle_scene(rom: &[u8], monsters: Vec<(u16, u16)>) -> BattleScene {
    let clock = dq9_core::state::DayClock::new(0.0, true);
    let request = super::Request {
        stage: super::DEFAULT_STAGE,
        monsters,
        ..Default::default()
    };
    let resolved = request.resolve(rom).unwrap();
    BattleScene::build(
        rom,
        &resolved,
        clock.phase().index(),
        clock,
        &dq9_formats::cast::Story::default(),
        &dq9_formats::hero::HeroLook::new_game(),
    )
    .unwrap()
}

/// A save whose current slot holds one character: `vocation` at `level`, `mp` maximum MP and
/// the skill panels of `panels` learned (the `+0x11A` bit array, `Party::from_slot`). A
/// synthetic save like `dq9_core::party`'s tests; the header says the slot exists and the
/// checksums are sealed, so that [`dq9_formats::save::Save::current_slot`] takes it.
fn save_with_abilities(vocation: u8, level: u8, mp: u16, panels: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0u8; dq9_formats::save::FILE_SIZE];
    bytes[21] = 1; // slot 0's header: the slot exists
    bytes[7577] = 1; // one party member, character record 0
    let at = 136;
    bytes[at + 2 + usize::from(vocation)] = level;
    bytes[at + 80] = vocation;
    // `+0x154`: word 2 fields 7 and 8 are the current HP and MP (full here); word 3
    // (`+0x158`) fields 9 and 10 are the maxima, which is what the load reads as such
    // (`func_020830cc` writes both pairs, `PartyMember_LoadRecord` reads 7 and 8).
    let word = (30u32 << 10) | (u32::from(mp) << 20);
    bytes[at + 0x154..at + 0x158].copy_from_slice(&word.to_le_bytes());
    let word = 30u32 | (u32::from(mp) << 10);
    bytes[at + 0x158..at + 0x15c].copy_from_slice(&word.to_le_bytes());
    for &panel in panels {
        let panel = usize::from(panel);
        bytes[at + 0x11A + panel / 8] |= 1 << (panel % 8);
    }
    let mut save = dq9_formats::save::Save::parse(&bytes).unwrap();
    save.slot_mut(0).unwrap().seal();
    save.as_bytes().to_vec()
}

/// A scripted battle with `--save` fights with the save's current slot: its learned panels
/// reach the Abilities menu, greyed rows included, and the member's MP pays for the row.
/// Without a save the same battle offers nothing — the new game's hero has no panel.
#[test]
fn a_scripted_battle_with_a_save_offers_its_learned_abilities() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    // Panel 0 is Dragon Slash (action 63, damage handler 1: refused by the kernel); panel 8 is
    // Gigaslash (action 67, 15 MP, modelled).
    let save = save_with_abilities(6, 3, 20, &[0, 8]);
    let start = crate::native::battle_start(super::DEFAULT_STAGE, Some(&save)).unwrap();
    let mut party = start.party;
    assert_eq!(party.members.len(), 1, "the save's one member");
    assert_eq!(party.members[0].level(), 3);
    assert_eq!(party.members[0].max_mp, 20);
    assert!(party.members[0].has_skill_panel(8));
    party.apply_level_tables(&data.levels);

    let down = Pad {
        down: true,
        ..Pad::default()
    };
    let abilities_menu = |party: &Party, seed: u64| {
        let (mut fight, mut world) = fight(rom, party, vec![(1, 1)], false, seed);
        press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
        fight.step(confirm(), &mut world);
        for _ in 0..3 {
            fight.step(down, &mut world);
        }
        fight.step(confirm(), &mut world);
        (fight, world)
    };
    let (saved, _world) = abilities_menu(&party, 41);
    assert!(
        saved.showing_abilities(),
        "Abilities opened on the save's panels"
    );
    let menu = saved.view().menu.unwrap();
    assert_eq!(
        menu.rows.len(),
        2,
        "one row per learned panel that names an action"
    );
    assert_eq!(String::from_utf8_lossy(&menu.rows[0].0), "Dragon Slash  0");
    assert!(!menu.rows[0].1, "the kernel does not model action 63");
    assert_eq!(String::from_utf8_lossy(&menu.rows[1].0), "Gigaslash  15");
    assert!(menu.rows[1].1, "the save's 20 MP pay for it");

    // Without a save the same scripted battle has a new game's party: no learned panel, so
    // Abilities is greyed and its list stays empty.
    let start = crate::native::battle_start(super::DEFAULT_STAGE, None).unwrap();
    let mut party = start.party;
    assert_eq!(party.members.len(), 1);
    assert!(party.members[0].skill_panels.iter().all(|byte| *byte == 0));
    party.apply_level_tables(&data.levels);
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 42);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    for _ in 0..3 {
        fight.step(down, &mut world);
    }
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 5, "the Fight menu");
    assert!(!menu.rows[3].1, "Abilities greyed without a learned panel");
}

/// The victory awards through the battle's own inputs (`Party::award_with`): the counter the
/// engine kept (`ov_026::021dacb0`) and the worn items' `itembtlprm.nat` (`func_0208539c`).
/// Two Minstrels, levels 1 and 5, take a slime trio's 6 experience as 3 and 4 — the weights
/// `k + level` (5 and 9 of 14) rounded up, the counter equal for both and so cancelled — and the
/// wearer of the item whose battle-parameter flags carry bit 16 takes 5, the ×1.05 landing before
/// the rounding. One member still takes the whole total, as the neutral award did.
#[test]
fn the_victory_awards_by_the_battles_counter_and_the_item_bonus() {
    use dq9_core::party::{Character, NOVICE_VOCATION};

    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();

    let mut party = level_one_party(&data);
    let mut second = Character::hero();
    second.levels[usize::from(NOVICE_VOCATION)] = 5;
    party.members.push(second);
    party.apply_level_tables(&data.levels);
    assert_eq!(party.members.len(), 2);
    let (mut played, mut world) = fight(rom, &party, vec![(1, 3)], false, 3);
    let outcome = played.play_out(&mut world).unwrap().clone();
    assert_eq!(outcome.result, BattleResult::Won);
    assert_eq!(outcome.experience, 6);
    assert_eq!(outcome.party.members[0].current_experience(), 3);
    assert_eq!(outcome.party.members[1].current_experience(), 4);

    let bonus = data
        .params
        .items
        .iter()
        .find(|param| param.experience_bonus())
        .expect("an item whose battle parameters carry the experience bonus");
    let mut wearing = level_one_party(&data);
    let mut second = Character::hero();
    second.levels[usize::from(NOVICE_VOCATION)] = 5;
    second.equipment.accessory = Some(bonus.id as u16);
    wearing.members.push(second);
    wearing.apply_level_tables(&data.levels);
    let (mut played, mut world) = fight(rom, &wearing, vec![(1, 3)], false, 3);
    let outcome = played.play_out(&mut world).unwrap().clone();
    assert_eq!(outcome.result, BattleResult::Won);
    assert_eq!(
        outcome.experience, 6,
        "the bonus moves the share, not the total"
    );
    assert_eq!(outcome.party.members[0].current_experience(), 3);
    assert_eq!(
        outcome.party.members[1].current_experience(),
        5,
        "9/14 of 6 is 3.86, ×1.05 is 4.05, rounded up"
    );

    // Single member: the counter cancels against itself and the share is the whole total.
    let alone = level_one_party(&data);
    let (mut played, mut world) = fight(rom, &alone, vec![(1, 1)], false, 2);
    let outcome = played.play_out(&mut world).unwrap().clone();
    assert_eq!(outcome.result, BattleResult::Won);
    assert_eq!(outcome.experience, 2);
    assert_eq!(outcome.party.members[0].current_experience(), 2);
}

/// A herb resolved on an ally at full HP changes nothing, so the item is kept: the original's
/// `ResolveAction` removes the held slot only when a kind handler raised its applied counter
/// (EU `0x021ed724..0x021ed7e4`). Seed 8 leaves the hero untouched in that round (HP 20 of
/// 20), so the herb heals nothing.
#[test]
fn a_herb_at_full_hp_is_kept() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
    party.members[0].held_items[0] = Some(22000);
    assert!(party.bag.add_copies(22000, 2));
    let full = party.members[0].hp;
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 1)], false, 8);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    fight.step(confirm(), &mut world);
    let down = Pad {
        down: true,
        ..Pad::default()
    };
    for _ in 0..4 {
        fight.step(down, &mut world);
    }
    fight.step(confirm(), &mut world);
    fight.step(confirm(), &mut world);
    fight.step(confirm(), &mut world);
    assert_eq!(
        fight.battle().party()[0].fighter.hp,
        i32::from(full),
        "healed nothing"
    );
    assert_eq!(
        fight.party().members[0].held_items[0],
        Some(22000),
        "the herb is kept when its heal did nothing"
    );
    assert_eq!(fight.party().bag.count(22000), 2, "the bag is untouched");
}

/// The swirl over a field camera, tick by tick, as the transition draws it: the camera rolls
/// and zooms, the disc keeps the screen's height, both screens go black at the end. With
/// `DQ9_EVIDENCE_DIR` set the frames at ticks 0, 9, 19, 29 and 37 are written there.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_swirl_rolls_zooms_and_blacks_out_the_field() {
    use dq9_render::swirl::{CHR_PATH, Swirl as Disc};
    use dq9_render::{Camera, Renderer, Viewport};

    use super::transition::Swirl;

    let Some(rom) = testrom::load() else { return };
    let files = nds_rom::Rom::parse(rom).unwrap();
    let chr = files.file_by_path(CHR_PATH).unwrap();
    let (device, queue) = pollster::block_on(async {
        let instance = crate::gpu::instance().await;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("a GPU adapter for the swirl capture");
        crate::gpu::device(&adapter).await.unwrap()
    });
    let mut scene = battle_scene(rom, vec![(1, 1)]);
    let disc = Disc::add(scene.runtime_mut(), chr)
        .unwrap()
        .expect("the swirl model draws");
    let mut renderer = Renderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &scene.runtime().scene,
    );
    renderer.environment = scene.environment.clone();
    let mut camera = Camera::field(glam::Vec3::new(0.0, 0.0, 0.0), 0.0);
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 256,
        height: 192,
    };
    let mut frame = |scene: &mut BattleScene, camera: &Camera, clock: &Swirl, shown: bool| {
        let world = disc.place(scene.runtime_mut(), shown.then_some(camera));
        disc.pose(scene.runtime_mut(), clock.model_frame()).unwrap();
        scene.upload(&mut renderer, &device, &queue).unwrap();
        renderer.update_instance_world(&queue, disc.instance(), world);
        let cover = clock.cover();
        renderer.environment.cover = (cover > 0.0).then_some([0.0, 0.0, 0.0, cover]);
        renderer
            .render_to_rgba_with(&device, &queue, (256, 192), viewport, camera, |_, _| {})
            .unwrap()
    };
    let evidence = std::env::var_os("DQ9_EVIDENCE_DIR").map(std::path::PathBuf::from);
    let mut clock = Swirl::new();
    let mut visible = 0usize;
    for tick in 0..38u32 {
        clock.step();
        camera.roll = clock.roll_degrees().to_radians();
        camera.fovy = 2.0 * clock.half_fov_degrees().to_radians();
        let with = frame(&mut scene, &camera, &clock, true);
        if [0, 9, 19, 29, 37].contains(&tick) {
            if tick == 19 {
                let without = frame(&mut scene, &camera, &clock, false);
                visible = with
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(without.as_chunks::<4>().0)
                    .filter(|(a, b)| a != b)
                    .count();
            }
            if let Some(dir) = &evidence {
                std::fs::create_dir_all(dir).unwrap();
                let file =
                    std::fs::File::create(dir.join(format!("swirl-tick{tick:02}.png"))).unwrap();
                let mut encoder = png::Encoder::new(file, 256, 192);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&with)
                    .unwrap();
            }
        }
        if tick == 37 {
            assert!(
                with.as_chunks::<4>().0.iter().all(|p| p[..3] == [0, 0, 0]),
                "both screens are black when the swirl ends"
            );
        }
    }
    assert!(
        visible > 5_000,
        "the rings cover a good part of the picture at tick 19 ({visible} pixels differ)"
    );
}

/// The party's row: `z` +2.5, 1.5 apart and centred, turned by the table `func_ov000_021675a0`
/// reads at `0x02183158` — a one-hero party to the ROM's own π (`0x323d`), a four-hero one from
/// `0x3570` to `0x2f0a` (π ± 0x333/4096 = ±0.2).
#[test]
fn the_row_party_turns_by_the_roms_table() {
    use super::row_party;

    let (position, heading) = row_party(0, 1);
    assert_eq!(position, glam::Vec3::new(0.0, 0.0, 2.5));
    assert!(
        (heading - std::f32::consts::PI).abs() < 2e-3,
        "0.0017 below π"
    );
    assert!((heading - 0x323d as f32 / 4096.0).abs() < 1e-6);

    let places: Vec<_> = (0..4).map(|index| row_party(index, 4)).collect();
    let xs: Vec<f32> = places.iter().map(|(placed, _)| placed.x).collect();
    // `func_ov000_021675a0` starts at +(count−1)·0x1800/2 and walks down: member 0 is the +x
    // end and takes the row's first turn.
    assert!(
        (xs[0] - 2.25).abs() < 1e-6 && (xs[1] - 0.75).abs() < 1e-6,
        "1.5 apart and centred, member 0 at +x: {xs:?}"
    );
    assert!((xs[3] + 2.25).abs() < 1e-6, "the last member at −x: {xs:?}");
    let pi = 0x323d as f32 / 4096.0;
    for (index, (_, heading)) in places.iter().enumerate() {
        let fx = [0x3570, 0x335c, 0x311e, 0x2f0a][index];
        assert!(
            (heading - fx as f32 / 4096.0).abs() < 1e-6,
            "member {index}: {heading}"
        );
    }
    // Heading 0 faces +z and (sin, cos) is the direction: the +x end turns toward −x and the
    // −x end toward +x, so both ends turn inward.
    assert!(
        ((places[0].1 - pi) - 0.2).abs() < 1e-3 && places[0].1.sin() < 0.0,
        "the +x end turns +0.2, inward: {}",
        places[0].1
    );
    assert!(
        ((places[3].1 - pi) + 0.2).abs() < 1e-3 && places[3].1.sin() > 0.0,
        "the −x end turns −0.2, inward: {}",
        places[3].1
    );
}

/// The monsters' row: side by side at `z` −2.5 on the gap of `func_ov000_02167b5c`, with the
/// five wide monster numbers of `func_ov000_021681f8` (`0xbd`, `0xbe`, `0xbf`, `0x110`,
/// `0x155`) narrowed to `0xb33`/4096 = 0.7 once more than one fights.
#[test]
fn the_row_monsters_narrow_the_five_numbers() {
    use super::{NARROWED_NUMBERS, row_monster};

    let plain = [Some(1), Some(1), Some(1)];
    let places: Vec<_> = (0..3)
        .map(|index| row_monster(&[1.0; 3], &plain, index).unwrap())
        .collect();
    let xs: Vec<f32> = places.iter().map(|(placed, _)| placed.x).collect();
    // The gap fills the target 4 + 2 × 0x199/4096 = 4.1997 with three widths of 1: 0.5999.
    assert!(
        (xs[0] + 1.59985).abs() < 1e-3 && xs[1].abs() < 1e-4 && (xs[2] - 1.59985).abs() < 1e-3,
        "{xs:?}"
    );
    for (index, (placed, heading)) in places.iter().enumerate() {
        assert!((placed.z + 2.5).abs() < 1e-6, "monster {index} on −z");
        assert_eq!(*heading, 0.0);
    }

    for number in NARROWED_NUMBERS {
        let numbers = [Some(number), Some(number), Some(number)];
        let narrowed: Vec<f32> = (0..3)
            .map(|index| row_monster(&[1.0; 3], &numbers, index).unwrap().0.x)
            .collect();
        assert!(
            narrowed[0] > xs[0] + 0.19 && narrowed[2] < xs[2] - 0.19,
            "number {number:#x} narrowed to 0.7: {narrowed:?} against {xs:?}"
        );
        assert!(
            (narrowed[2] - narrowed[0] - 2.7993).abs() < 1e-3,
            "number {number:#x}: {narrowed:?}"
        );
    }
    // A number outside the five keeps its width, and one monster is never narrowed.
    for number in [1u16, 2, 0xbc, 0x111, 0x156] {
        let numbers = [Some(number), Some(number), Some(number)];
        let kept: Vec<f32> = (0..3)
            .map(|index| row_monster(&[1.0; 3], &numbers, index).unwrap().0.x)
            .collect();
        assert_eq!(kept, xs, "number {number:#x} is not narrowed");
    }
    for number in NARROWED_NUMBERS {
        let Some((placed, _)) = row_monster(&[1.0], &[Some(number)], 0) else {
            panic!("one monster stands in the row");
        };
        assert_eq!(placed.x, 0.0, "one monster stands on the middle");
        assert_eq!(placed.z, -2.5);
    }
}

/// The opening's approach, update by update (`func_ov000_0216f0d0` read on the EU ROM): each
/// difference moves by `(difference × 0xcd + 0x800) >> 12` — the framing keeps `0xf33`/4096 of
/// it, `0.95` a 30 Hz update — and the step snaps once every difference is under `0x28`.
#[test]
fn the_opening_eases_into_the_side_shot_by_five_percent_a_step() {
    use super::{Approach, Framing, INTRO_GAIN, approach_step, fix_mul};

    let target = glam::Vec3::new(0.0, 0.5, 0.0);
    let settle = Framing {
        yaw: 0x800,
        pitch: 0x800,
        distance: 0x4000,
    };
    let mut approach = Approach {
        current: Framing {
            yaw: 0x1000,
            pitch: 0x1000,
            distance: 0x7000,
        },
        target: settle,
    };
    // One step: yaw −(0x800 × 0xcd)>>12 = −102, pitch the same, distance −615.
    let (eye, settled) = approach_step(&mut approach, target);
    assert!(!settled, "one step does not reach the side shot");
    assert_eq!(INTRO_GAIN, 0xcd);
    assert_eq!(fix_mul(-0x800, INTRO_GAIN), -0x66);
    assert_eq!(approach.current.yaw, 0x1000 - 0x66);
    assert_eq!(approach.current.pitch, 0x1000 - 0x66);
    assert_eq!(approach.current.distance, 0x7000 - 0x267);
    // The eye is the framing's: the horizontal reach turned by the yaw, `y` the height above
    // the target.
    let pitch = 0x1000 - 0x66;
    let distance = 0x7000 - 0x267;
    let flat = f64::from(distance * distance - pitch * pitch).sqrt() / 4096.0;
    let reach = f64::from(eye.x).hypot(f64::from(eye.z));
    assert!((reach - flat).abs() < 1e-3, "{eye:?}");
    assert!(
        (f64::from(eye.y) - (0.5 + f64::from(pitch) / 4096.0)).abs() < 1e-3,
        "{eye:?}"
    );
    let yaw = f64::from(approach.current.yaw) / 4096.0;
    assert!(
        (f64::from(eye.x) - yaw.sin() * flat).abs() < 1e-3,
        "{eye:?}"
    );
    assert!(
        (f64::from(eye.z) - yaw.cos() * flat).abs() < 1e-3,
        "{eye:?}"
    );

    // Every step keeps about 95% of each difference: the easing is geometric, not linear.
    let mut previous = distance - settle.distance;
    for _ in 0..10 {
        let (_, settled) = approach_step(&mut approach, target);
        assert!(!settled);
        let remaining = approach.current.distance - settle.distance;
        assert!(remaining > 0 && remaining < previous);
        let ratio = f64::from(remaining) / f64::from(previous);
        assert!(
            (0.9499..0.9502).contains(&ratio),
            "kept {ratio} of the distance"
        );
        previous = remaining;
    }

    // The snap: 0x28 on every parameter, taken at the 114th step of this opening.
    let mut approach = Approach {
        current: Framing {
            yaw: 0x1000,
            pitch: 0x1000,
            distance: 0x7000,
        },
        target: settle,
    };
    let mut steps = 0;
    loop {
        steps += 1;
        let (_, settled) = approach_step(&mut approach, target);
        if settled {
            break;
        }
        assert!(steps < 1000, "the easing never settles");
    }
    assert_eq!(steps, 114, "the 0x95 retention settles at the 114th step");
    assert_eq!(approach.current, settle);
}

/// The fixed shots of the big monster kinds (`data_ov026_021de87c`, `ov_026::021d8aac`): the
/// Hexagoon (300 = `0x12c`) has its own eye and target, a named kind earlier in the table wins
/// when several stand, and an ordinary slime has none.
#[test]
fn the_shot_table_holds_the_big_monster_kinds() {
    use super::shots;

    assert_eq!(
        shots::kind_shot(&[300]),
        Some(([0, 5816, 16588], [0, 7495, 0])),
        "the Hexagoon's record"
    );
    assert_eq!(
        shots::kind_shot(&[300, 258]),
        Some(([-1105, 5570, 9666], [-1105, 5611, 0])),
        "0x102 stands before 0x12c in the table"
    );
    assert_eq!(shots::kind_shot(&[1]), None, "a slime has no record");
    assert_eq!(
        shots::kind_shot(&[0]),
        None,
        "the 0 record needs the battle type"
    );
    // The five narrow row kinds are not all shot kinds, and a variant record still answers.
    assert_eq!(
        shots::kind_shot(&[0x115]),
        Some(([0, 3604, 11059], [0, 6062, 0])),
        "0x115's first variant"
    );
    assert!(
        shots::kind_shot(&[0xbd]).is_none(),
        "0xbd is a row number only"
    );
    // The table is keyed by kind, not number: 300 is the Hexagoon's number, 0x101 its kind.
    assert_eq!(
        shots::kind_shot(&[0x101]),
        Some(([0, 4341, 9297], [0, 4423, 0])),
        "kind 0x101"
    );
}

/// The opening on the real Hexagoon: the table is keyed by the monster's **kind** (`+0x10`,
/// `func_ov000_0215fc8c`), and the Hexagoon is kind `0x101` — the table's first record — not
/// number 300 (`0x12c` is Estark's). The eye starts pulled back by 0.5 and 3.0 from that shot
/// (`func_ov000_0216118c`, `func_0202e5d8`) and eases back into it at one step per 30 Hz
/// update (`func_ov000_0216f0d0`), at the wide 22°.
#[test]
fn the_opening_approaches_the_hexagoon_shot_and_settles() {
    use super::Shot;

    let Some(rom) = testrom::load() else { return };
    let mut scene = battle_scene(rom, vec![(300, 1)]);
    assert_eq!(scene.shot(), Shot::Opening);
    assert_eq!(
        scene.fighter(super::Fighter::monster(0)).unwrap().kind,
        Some(0x101),
        "the Hexagoon's kind"
    );
    let target = scene.camera().target;
    // Kind 0x101's shot: eye (0, 4341, 9297), target (0, 4423, 0), fx12.
    assert!((target.y - 4423.0 / 4096.0).abs() < 1e-3, "{target:?}");
    let eye = scene.camera().eye();
    // Pulled back by 3.0 along the eye-target distance and up by 0.5 from the record's own eye
    // (`func_ov000_0216118c`: y + 0x800 on the record's 4423 − 4341 = 82, distance + 0x3000).
    assert!(eye.z > 5.2 && eye.z < 5.4, "pulled back 3.0: {eye:?}");
    let settle_y = 4341.0 / 4096.0;
    assert!(
        (eye.y - (settle_y + 0.5)).abs() < 0.01,
        "pulled up 0.5 from {settle_y}: {eye:?}"
    );
    assert!((scene.camera().fovy.to_degrees() - 44.0).abs() < 1e-3);

    // The game's update is 30 Hz: the first eased frame is the second fixed step, and the
    // settle is the record's own eye.
    scene.step().unwrap();
    assert_eq!(scene.camera().eye(), eye, "the first tick spends no update");
    scene.step().unwrap();
    let one_step = scene.camera().eye();
    assert!(
        one_step.z < eye.z && one_step.z > 9297.0 / 4096.0,
        "{one_step:?}"
    );
    for _ in 0..400 {
        scene.step().unwrap();
    }
    let settled = scene.camera().eye();
    assert!((settled.z - 9297.0 / 4096.0).abs() < 1e-3, "{settled:?}");
    assert!((settled.y - 4341.0 / 4096.0).abs() < 1e-3, "{settled:?}");
    assert!(settled.x.abs() < 1e-3, "{settled:?}");
    // Once settled, the camera stays put however many steps pass.
    scene.step().unwrap();
    assert_eq!(scene.camera().eye(), settled);
}

/// The battle's places stop at its tables: the party's four rows (`0x02183108`) and the
/// monsters' eight (`0x02183118`, `func_ov000_0215eb1c`'s cap). A ninth monster has no grid
/// slot and no row place — the original has none for it — and the row it does not join is the
/// eight-monster row.
#[test]
fn a_set_past_eight_has_no_place() {
    use super::{MONSTER_SLOTS, monster_slots, party_slots, row_monster};

    assert_eq!(MONSTER_SLOTS, 8);
    assert_eq!(monster_slots(8).len(), 8);
    assert_eq!(monster_slots(9), monster_slots(8), "no ninth row");
    assert_eq!(monster_slots(9), vec![19, 10, 11, 12, 13, 14, 15, 25]);
    assert_eq!(party_slots(4).len(), 4);
    assert_eq!(party_slots(5), party_slots(4), "no fifth party slot");

    let widths = [3.2; 9];
    let kinds = [Some(1u16); 9];
    assert_eq!(row_monster(&widths, &kinds, 8), None, "no ninth place");
    assert_eq!(
        row_monster(&widths, &kinds, 7),
        row_monster(&widths[..8], &kinds[..8], 7),
        "the eighth of nine stands where the eighth of eight does"
    );

    // A request for nine builds the eight the game's slots hold, and nothing at a ninth place.
    let Some(rom) = testrom::load() else { return };
    let scene = battle_scene(rom, vec![(1, 9)]);
    assert_eq!(scene.fighters().len(), 1 + MONSTER_SLOTS);
}

/// The opening at three ticks and the row of a nine-monster set, rendered offscreen on the real
/// stage. With `DQ9_EVIDENCE_DIR` set, `opening-tick00/60/240-hexagoon.png` (the Hexagoon's
/// kind 0x101 shot of `data_ov026_021de87c`, pulled back at tick 0 and settled by 240 — 114
/// 30 Hz updates) and `row-nine-monsters.png` (the eight places `0x02183118` has; the ninth has
/// none) are written there.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_opening_camera_and_the_nine_monster_row_are_captured() {
    use dq9_render::{Renderer, Viewport};

    use super::{Formation, Shot, Side};

    let Some(rom) = testrom::load() else { return };
    let (device, queue) = pollster::block_on(async {
        let instance = crate::gpu::instance().await;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("a GPU adapter for the battle camera capture");
        crate::gpu::device(&adapter).await.unwrap()
    });
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 256,
        height: 192,
    };
    let evidence = std::env::var_os("DQ9_EVIDENCE_DIR").map(std::path::PathBuf::from);
    let mut scene = battle_scene(rom, vec![(300, 1)]);
    let mut renderer = Renderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &scene.runtime().scene,
    );
    renderer.environment = scene.environment.clone();
    let capture = |scene: &mut BattleScene, renderer: &mut Renderer, name: &str| {
        scene.upload(renderer, &device, &queue).unwrap();
        let camera = *scene.camera();
        let frame = renderer
            .render_to_rgba_with(&device, &queue, (256, 192), viewport, &camera, |_, _| {})
            .unwrap();
        if let Some(dir) = &evidence {
            std::fs::create_dir_all(dir).unwrap();
            let file = std::fs::File::create(dir.join(name)).unwrap();
            let mut encoder = png::Encoder::new(file, 256, 192);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&frame)
                .unwrap();
        }
        frame
    };
    let start = capture(&mut scene, &mut renderer, "opening-tick00-hexagoon.png");
    for _ in 0..60 {
        scene.step().unwrap();
    }
    let tick60 = capture(&mut scene, &mut renderer, "opening-tick60-hexagoon.png");
    for _ in 0..180 {
        scene.step().unwrap();
    }
    let tick240 = capture(&mut scene, &mut renderer, "opening-tick240-hexagoon.png");
    let differing = |a: &[u8], b: &[u8]| {
        a.as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .filter(|(a, b)| a != b)
            .count()
    };
    assert!(
        differing(&start, &tick60) > 1_000 && differing(&tick60, &tick240) > 200,
        "the eye moves between the three ticks"
    );
    assert!(
        differing(
            &tick240,
            &capture(&mut scene, &mut renderer, "opening-tick241-hexagoon.png")
        ) < 500,
        "the opening has settled by tick 240"
    );

    // The row of nine: eight monsters side by side (`0x02183118`), the ninth without a place.
    let mut row = battle_scene(rom, vec![(1, 9)]);
    row.set_formation(Formation::Row);
    row.set_shot(Shot::Side(Side::Monsters));
    assert_eq!(row.fighters().len(), 1 + super::MONSTER_SLOTS);
    let placed: Vec<f32> = row
        .fighters()
        .iter()
        .filter(|f| f.fighter.side == Side::Monsters)
        .map(|f| f.position.x)
        .collect();
    assert_eq!(placed.len(), 8, "the ninth monster has no model");
    let mut row_renderer = Renderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &row.runtime().scene,
    );
    row_renderer.environment = row.environment.clone();
    let frame = capture(&mut row, &mut row_renderer, "row-nine-monsters.png");
    assert!(frame.as_chunks::<4>().0.iter().any(|p| p[..3] != [0, 0, 0]));
}
