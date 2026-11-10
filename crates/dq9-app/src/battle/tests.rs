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
    // Fight: Attack, Spells, Defend, Items; the hero knows no spell at level 1 and the bag is
    // empty, Defend is allowed.
    assert_eq!(
        press(&mut fight, &mut world, confirm()),
        Some((4, 0, false))
    );
    assert_eq!(press(&mut fight, &mut world, down), Some((4, 1, false)));
    let menu = fight.view().menu.unwrap();
    assert!(
        !menu.rows[1].1 && menu.rows[2].1 && !menu.rows[3].1,
        "spells and Items grey, Defend allowed"
    );
    assert_eq!(
        press(&mut fight, &mut world, confirm()),
        Some((4, 1, false)),
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
        Some((4, 0, false)),
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
    for _ in 0..3 {
        fight.step(down, &mut world);
    }
    let menu = fight.view().menu.unwrap();
    assert!(menu.rows[3].1, "Items is allowed with a held herb");
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
    for _ in 0..3 {
        fight.step(down, &mut world);
    }
    assert_eq!(fight.view().menu.unwrap().cursor, 3);
    fight.step(confirm(), &mut world);
    assert!(
        fight.view().menu.unwrap().rows.len() == 4,
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
    for _ in 0..3 {
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
    for _ in 0..3 {
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
    assert_eq!(fight.view().menu.unwrap().cursor, 3);
    // Defend is index 2.
    fight.step(
        Pad {
            up: true,
            ..Pad::default()
        },
        &mut world,
    );
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
        for _ in 0..3 {
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
    for _ in 0..3 {
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
