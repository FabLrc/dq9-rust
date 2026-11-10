//! The played fight on the real tables (skipped without the ROM).

use dq9_core::battle::{BattleResult, Surprise};
use dq9_core::party::Party;

use super::data::Data;
use super::fight::{Fight, Outcome, Pad, Setup, World};

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

/// The message sequence of a seeded battle against two slimes, line by line: the opening, each
/// action's opening and result (an attack at a monster says `actmsg` 5, at the hero 2), the
/// kill line, then the victory lines (`str_bres` 50, 25, 16: a party of one reads its own name).
#[test]
fn a_seeded_slime_battle_says_the_games_lines() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let party = level_one_party(&data);
    let (mut fight, mut world) = fight(rom, &party, vec![(1, 2)], false, 4);
    let outcome = fight.play_out(&mut world).unwrap().clone();
    assert_eq!(outcome.result, BattleResult::Won);
    assert_eq!(
        (outcome.experience, outcome.gold),
        (4, 8),
        "two slimes: 2 and 4 each"
    );
    assert_eq!(
        fight.log(),
        [
            "Some slimes draw near!",
            "Hero attacks.",
            "Does 6 points of damage to the slime.",
            "The slime attacks.",
            "Hero takes 1 point of damage.",
            "The slime attacks.",
            "Hero takes 1 point of damage.",
            "Hero attacks.",
            "Does 5 points of damage to the slime.",
            "The slime is defeated.",
            "The slime attacks.",
            "Hero takes 1 point of damage.",
            "The slime attacks.",
            "Hero takes 1 point of damage.",
            "Hero attacks.",
            "Does 7 points of damage to the slime.",
            "The slime is defeated.",
            "The enemy are defeated.",
            "Hero receives some experience!",
            "Hero obtains 8 gold coins!",
        ]
    );
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

/// An item of the bag: the medicinal herb is listed with its count and is allowed; using it
/// says the game's lines, heals the hero (HP shown follows) and takes one from the bag.
#[test]
fn a_medicinal_herb_is_used_from_the_bag_and_leaves_it() {
    let Some(rom) = testrom::load() else { return };
    let data = Data::load(rom).unwrap();
    let mut party = level_one_party(&data);
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
    assert!(menu.rows[3].1, "Items is allowed with a herb in the bag");
    fight.step(confirm(), &mut world);
    let menu = fight.view().menu.unwrap();
    assert_eq!(menu.rows.len(), 1);
    assert_eq!(
        String::from_utf8_lossy(&menu.rows[0].0),
        "medicinal herb x2"
    );
    assert!(menu.rows[0].1);
    fight.step(confirm(), &mut world);
    // The herb heals one of the party: the target list is the party's.
    let menu = fight.view().menu.unwrap();
    assert!(menu.targets && menu.rows.len() == 1);
    fight.step(confirm(), &mut world);
    press_until(&mut fight, &mut world, |f| f.view().menu.is_some());
    let log = fight.log();
    let at = log
        .iter()
        .position(|l| l == "Hero uses a medicinal herb.")
        .unwrap_or_else(|| panic!("{log:?}"));
    assert_eq!(log[at + 1], "Hero's wounds are healed.");
    // The bag lost one; the status shows the healing.
    let hp = fight.view().members[0].hp;
    assert!(hp > 5 || fight.is_done(), "the herb healed 30 to 40 HP");
    press_until(&mut fight, &mut world, |_| false);
    let outcome = fight.outcome().unwrap();
    assert_eq!(outcome.party.bag.count(22000), 1);
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
