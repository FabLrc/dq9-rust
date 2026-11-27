//! Whole battles on the real game data, played to the end headless; skipped when the ROM is
//! absent. The hero is a kernel [`Fighter`] with the new-game numbers of `docs/re/party.md`
//! (HP 20, MP 6, strength 9, resilience 8, agility 8, deftness 12): the game's own attack and
//! defence formulas (equipment included) are the party batch's, so these two are the base
//! attributes, not the original's derived values. Run with `--nocapture` to read every round.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use dq9_core::battle::{
    ActionTables, Battle, BattleError, BattleResult, BattleSetup, CRand, Command, Event, Fighter,
    PartyMember, Random, RoundReport, Side, Skill, Surprise, Tables, Target, Unsupported, Who,
    resolve_action, validate,
};
use dq9_core::party::Character;
use dq9_core::scenario;
use dq9_formats::Gpc2;
use dq9_formats::battle::encounter;
use dq9_formats::battle::equipment::BattleParams;
use dq9_formats::battle::monster::{BattleData, Names};
use dq9_formats::item::ItemStats;
use nds_rom::Rom;

fn member<'a>(rom: &Rom<'a>, path: &str, name: &str) -> Cow<'a, [u8]> {
    let bytes = rom
        .file_by_path(path)
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    let gpc2 = Gpc2::parse(bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
    let entry = gpc2
        .find(name)
        .unwrap_or_else(|| panic!("{name} in {path}"));
    gpc2.member(entry)
        .unwrap_or_else(|e| panic!("{name} of {path}: {e}"))
}

/// The ROM files the tables borrow from.
struct Data<'a> {
    monsters: &'a [u8],
    names: Cow<'a, [u8]>,
    actions_a: Cow<'a, [u8]>,
    actions_b: Cow<'a, [u8]>,
    ranges_a: Cow<'a, [u8]>,
    ranges_b: Cow<'a, [u8]>,
}

impl<'a> Data<'a> {
    fn load(rom: &Rom<'a>) -> Self {
        Self {
            monsters: rom.file_by_path("data/prm/mon_btldata.nat").unwrap(),
            names: member(rom, "data/prm/mon_data.gp2", "mon_data_en.nat"),
            actions_a: member(rom, "data/prm/actdt_a.gp2", "actdt_a_en.nat"),
            actions_b: member(rom, "data/prm/actdt_b.gp2", "actdt_b_en.nat"),
            ranges_a: member(rom, "data/prm/actdt_a.gp2", "actdamage_a.nat"),
            ranges_b: member(rom, "data/prm/actdt_b.gp2", "actdamage_b.nat"),
        }
    }

    fn tables(&self) -> Tables<'_> {
        Tables {
            monsters: BattleData::parse(self.monsters).unwrap(),
            names: Names::parse(&self.names).unwrap(),
            actions: ActionTables::parse(
                &self.actions_a,
                &self.actions_b,
                &self.ranges_a,
                &self.ranges_b,
            )
            .unwrap(),
        }
    }
}

/// The new-game hero as the kernel's numbers.
fn hero() -> PartyMember {
    let mut f = Fighter::new(Side::Party);
    f.hp = 20;
    f.max_hp = 20;
    f.mp = 6;
    f.attack = 9;
    f.defence = 8;
    f.agility = 8;
    f.deftness = 12;
    f.evasion_rate = 2.0;
    PartyMember::new(f)
}

/// Fullheal's kind handler replaces the resolved amount with the target's maximum HP
/// (`ov_024::021db3a4`), after the ordinary resolver's draws, even at overwhelming defence.
#[test]
fn fullheal_restores_hp_after_spending_the_ordinary_draws() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let action = tables.actions.skill(33).unwrap().action.unwrap();
    let mut actor = Fighter::new(Side::Party);
    actor.mp = 40;
    actor.attack = 9;
    for (defence, draws) in [(0, 5), (999, 4)] {
        let mut target = Fighter::new(Side::Party);
        target.hp = 37;
        target.max_hp = 321;
        target.defence = defence;
        let mut rng = Random::new(7);
        let outcome =
            resolve_action(&mut rng, &actor, &action, std::slice::from_ref(&target), 0).unwrap();
        let restored = &outcome.targets[0];
        assert_eq!((restored.hp_before, restored.hp_after), (37, 321));
        assert_eq!(
            restored.amount, 321,
            "the handler replaces the capped amount"
        );
        assert_eq!(outcome.mp_spent, 24);
        assert_eq!(outcome.draws, draws);
        let mut expected = Random::new(7);
        for _ in 0..draws {
            expected.next_u32();
        }
        assert_eq!(rng, expected, "Fullheal must not bypass the ordinary draws");
    }
}

fn attack_command(tables: &Tables<'_>, target: usize) -> Command {
    Command::Act {
        skill: tables.actions.skill(1).unwrap(),
        target: Target::Monster(target),
    }
}

fn name(who: Who) -> String {
    match who {
        Who::Party(i) => format!("party{i}"),
        Who::Monster(i) => format!("monster{i}"),
    }
}

/// Prints a round the way the log reads.
fn print_round(label: &str, report: &RoundReport) {
    for event in &report.events {
        match event {
            Event::Flee { success, percent } => {
                println!(
                    "{label} r{} flee success={success} percent={percent:?}",
                    report.round
                );
            }
            Event::Initiative(order) => {
                let order: Vec<String> = order
                    .iter()
                    .map(|(w, s)| format!("{}={s:.2}", name(*w)))
                    .collect();
                println!("{label} r{} order {}", report.round, order.join(" "));
            }
            Event::Chose {
                who,
                action,
                targets,
            } => {
                let t: Vec<String> = targets.iter().map(|w| name(*w)).collect();
                println!(
                    "{label} r{} {} chooses {action} -> {}",
                    report.round,
                    name(*who),
                    t.join(",")
                );
            }
            Event::Acted {
                who,
                action,
                outcomes,
            } => {
                for (target, o) in outcomes {
                    println!(
                        "{label} r{} {} does {action} to {}: amount {} hp {} -> {}{}{}{}",
                        report.round,
                        name(*who),
                        name(*target),
                        o.amount,
                        o.hp_before,
                        o.hp_after,
                        if o.critical { " critical" } else { "" },
                        if o.dodged { " dodged" } else { "" },
                        if o.blocked { " blocked" } else { "" },
                    );
                }
            }
            Event::Skipped(who) => {
                println!("{label} r{} {} does not act", report.round, name(*who))
            }
            Event::Defended(who) => {
                println!("{label} r{} {} defends", report.round, name(*who))
            }
            Event::ItemUsed { who, action } => {
                println!(
                    "{label} r{} {} uses item action {action}",
                    report.round,
                    name(*who)
                )
            }
            Event::Summoned { who, number } => {
                println!(
                    "{label} r{} {} calls monster {number} for backup",
                    report.round,
                    name(*who)
                )
            }
            Event::End(result) => println!("{label} r{} ends: {result:?}", report.round),
        }
    }
}

/// Plays `battle` with Attack on the first standing monster every round.
fn play_attacking(
    label: &str,
    tables: &Tables<'_>,
    battle: &mut Battle,
    world: &mut Random,
) -> Result<(), BattleError> {
    for _ in 0..100 {
        if battle.result().is_some() {
            return Ok(());
        }
        let target = battle
            .monsters()
            .iter()
            .position(|m| m.fighter.hp > 0)
            .unwrap();
        let report = battle.round(&[attack_command(tables, target)], world)?;
        print_round(label, &report);
    }
    panic!("{label}: the battle never ended");
}

/// The hero against an F01 slime (experience 2, gold 4, drops 22000 one in 8 and 22087 one in 16),
/// Attack every round, many seeds: always won, with the table's experience and gold, and the
/// drops are the slime's.
#[test]
fn the_hero_beats_an_f01_slime_with_the_tables_experience_and_gold() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let mut dropped = BTreeSet::new();
    for seed in 0..40u64 {
        let mut world = Random::new(0x1000 + seed);
        let setup = BattleSetup::encounter(&tables, &[(1, 1)], &mut world).unwrap();
        let hp = setup.monsters[0].fighter.hp;
        assert!(
            (6..=8).contains(&hp),
            "a slime's 8 comes to 6, 7 or 8, not {hp}"
        );
        let mut battle = Battle::new(setup, vec![hero()], Surprise::None, 0x2000 + seed).unwrap();
        play_attacking(
            &format!("slime seed {seed}"),
            &tables,
            &mut battle,
            &mut world,
        )
        .unwrap();
        assert_eq!(battle.result(), Some(BattleResult::Won), "seed {seed}");
        let end = battle.finish(&mut CRand::new(1 + seed as u32)).unwrap();
        assert_eq!((end.experience, end.gold), (2, 4), "seed {seed}");
        assert!(end.drops.len() <= 1 && end.drops.iter().all(|d| [22000, 22087].contains(d)));
        dropped.extend(end.drops);
    }
    assert!(
        dropped.contains(&22000),
        "the ordinary drop (1 in 8) fell in 40 battles"
    );
}

/// Two slimes and a cruelcumber from one group each: every beaten monster pays, the drops are
/// rolled once per kind.
#[test]
fn several_groups_pay_by_monster_and_drop_by_kind() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let mut strong = hero();
    strong.fighter.hp = 400;
    strong.fighter.max_hp = 400;
    strong.fighter.attack = 40;
    let mut world = Random::new(5);
    let setup = BattleSetup::encounter(&tables, &[(1, 2), (57, 1)], &mut world).unwrap();
    let mut battle = Battle::new(setup, vec![strong], Surprise::None, 77).unwrap();
    play_attacking("group", &tables, &mut battle, &mut world).unwrap();
    assert_eq!(battle.result(), Some(BattleResult::Won));
    let end = battle.finish(&mut CRand::new(3)).unwrap();
    // Two slimes (2 and 4 each) and a cruelcumber (3 and 5).
    assert_eq!((end.experience, end.gold), (2 * 2 + 3, 2 * 4 + 5));
    assert!(
        end.drops.len() <= 4,
        "two kinds, rare then ordinary: at most one each"
    );
}

/// The Hexagoon (set battle 2, HP 93 whole, attack 30) against a level 1 hero: lost.
#[test]
fn the_hexagoon_beats_a_level_one_hero() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let record = scenario::battle(&rom, 2).unwrap();
    assert_eq!(record.monsters, vec![(300, 1)]);
    let groups: Vec<(u16, u8)> = record
        .monsters
        .iter()
        .map(|&(m, n)| (m as u16, n as u8))
        .collect();
    let mut lost = 0;
    for seed in 0..10u64 {
        let setup = BattleSetup::set_battle(&tables, &groups).unwrap();
        assert!(setup.set_battle);
        assert_eq!(
            setup.monsters[0].fighter.hp, 93,
            "a set battle keeps the table's HP"
        );
        let mut battle = Battle::new(setup, vec![hero()], Surprise::None, seed).unwrap();
        let mut world = Random::new(seed);
        play_attacking(
            &format!("hexagoon seed {seed}"),
            &tables,
            &mut battle,
            &mut world,
        )
        .unwrap();
        assert_eq!(battle.result(), Some(BattleResult::Lost), "seed {seed}");
        let end = battle.finish(&mut CRand::new(1)).unwrap();
        assert_eq!((end.experience, end.gold, end.drops.len()), (0, 0, 0));
        lost += 1;
    }
    assert_eq!(lost, 10);
}

/// Flight from an encounter is a percentage on the world's generator: the hero (deftness 12,
/// 10 + 0 = 10, held to the first floor, 25) gets away on some seeds and not on others, and
/// the outcome is `NextRandomMax(100) < 25` of the world's generator. A set battle refuses it.
#[test]
fn flight_succeeds_or_fails_by_seed_and_a_set_battle_refuses_it() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let (mut got_away, mut caught) = (0, 0);
    for seed in 0..60u64 {
        let mut build = Random::new(0x500 + seed);
        let setup = BattleSetup::encounter(&tables, &[(1, 1)], &mut build).unwrap();
        let mut battle = Battle::new(setup, vec![hero()], Surprise::None, seed).unwrap();
        let mut world = Random::new(seed);
        let expected = Random::new(seed).next_max(100) < 25;
        let report = battle.round(&[Command::Flee], &mut world).unwrap();
        print_round(&format!("flee seed {seed}"), &report);
        assert_eq!(
            report.events[0],
            Event::Flee {
                success: expected,
                percent: Some(25)
            },
            "seed {seed}"
        );
        if expected {
            got_away += 1;
            assert_eq!(battle.result(), Some(BattleResult::Fled));
            let end = battle.finish(&mut CRand::new(1)).unwrap();
            assert_eq!(
                (end.result, end.experience, end.gold),
                (BattleResult::Fled, 0, 0)
            );
            assert_eq!(
                battle.round(&[Command::Flee], &mut world),
                Err(BattleError::Over)
            );
        } else {
            caught += 1;
            assert_eq!(battle.result(), None);
            assert_eq!(battle.round_number(), 1, "the round went on");
        }
    }
    assert!(
        got_away > 0 && caught > 0,
        "{got_away} away, {caught} caught"
    );

    let record = scenario::battle(&rom, 2).unwrap();
    let groups: Vec<(u16, u8)> = record
        .monsters
        .iter()
        .map(|&(m, n)| (m as u16, n as u8))
        .collect();
    let setup = BattleSetup::set_battle(&tables, &groups).unwrap();
    let mut battle = Battle::new(setup, vec![hero()], Surprise::None, 1).unwrap();
    let state = battle.generator_state();
    assert_eq!(
        battle.round(&[Command::Flee], &mut Random::new(1)),
        Err(BattleError::FleeForbidden)
    );
    assert_eq!(battle.generator_state(), state);
}

/// The adapter reads the records into the very numbers the kernel's differential test was
/// built from by hand (`docs/re/battle.md`, "Action fields").
#[test]
fn the_adapter_reads_the_actions_the_oracle_was_decoded_for() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let attack = tables.actions.skill(1).unwrap();
    let a = attack.action.clone().unwrap();
    assert_eq!(
        (a.number, a.mp_cost, a.kind, a.element, a.landing_element),
        (1, 0, 1, 8, 8)
    );
    assert_eq!(a.reach, dq9_core::battle::Reach::Attack);
    assert!(a.dodgeable && a.blockable && a.wakes_target && a.chains_combo);
    assert!(a.multi_target_falloff && a.weapon_element && a.trait_accuracy_die);
    assert_eq!(a.critical_percent, 100);
    assert_eq!(
        (
            a.rider,
            a.damage_handler,
            a.max_damage,
            a.shared_critical_bits
        ),
        (0, 0, 0, 0)
    );
    assert!(!a.scales_accuracy && !a.always_critical);
    assert_eq!(a.amount, dq9_core::battle::Amount::Blow { multiplier: 1.0 });
    assert_eq!((attack.aims, attack.reach, attack.handlers), (1, 5, [0, 1]));

    let crack = tables.actions.skill(12).unwrap().action.unwrap();
    assert_eq!(
        (
            crack.mp_cost,
            crack.kind,
            crack.element,
            crack.critical_percent
        ),
        (3, 1, 2, 50)
    );
    assert_eq!(crack.reach, dq9_core::battle::Reach::One);
    assert!(crack.blockable && !crack.dodgeable && crack.chains_combo);
    let dq9_core::battle::Amount::Range(range) = crack.amount else {
        panic!("{crack:?}")
    };
    assert_eq!(
        (range.spread, range.party_min, range.monster_amount),
        (5, 30, 17)
    );
    assert_eq!(range.scaling, Some(dq9_core::battle::ScaleStat::Might));

    let heal = tables.actions.skill(30).unwrap().action.unwrap();
    assert_eq!((heal.mp_cost, heal.kind, heal.critical_percent), (2, 2, 50));
    assert!(!heal.chains_combo && !heal.dodgeable && !heal.blockable);
    let dq9_core::battle::Amount::Range(range) = heal.amount else {
        panic!("{heal:?}")
    };
    assert_eq!(
        (range.spread, range.party_min, range.party_max),
        (5, 35, 160)
    );
    assert_eq!(range.scaling, Some(dq9_core::battle::ScaleStat::Mending));
}

/// A hero made to last: enough HP and defence for the monster to choose several ways before the
/// fight ends, weak enough not to one-shot it.
fn enduring_hero() -> PartyMember {
    let mut f = Fighter::new(Side::Party);
    f.hp = 200;
    f.max_hp = 200;
    f.mp = 6;
    f.attack = 9;
    f.defence = 8;
    f.agility = 8;
    f.deftness = 12;
    f.evasion_rate = 2.0;
    PartyMember::new(f)
}

/// Plays a battle to its end, Attack on the first standing monster, and returns the actions the
/// monsters acted and the rounds played.
fn play_to_the_end(
    label: &str,
    tables: &Tables<'_>,
    setup: BattleSetup,
    hero: PartyMember,
    seed: u64,
) -> (Battle, Vec<u16>, Vec<(i8, i8, u8)>) {
    let mut world = Random::new(seed ^ 0x5a5a_5a5a);
    let mut battle = Battle::new(setup, vec![hero], Surprise::None, seed).unwrap();
    let mut acted = Vec::new();
    let mut statuses = Vec::new();
    for _ in 0..200 {
        if battle.result().is_some() {
            break;
        }
        let Some(target) = battle.monsters().iter().position(|m| m.fighter.hp > 0) else {
            break;
        };
        let report = battle
            .round(&[attack_command(tables, target)], &mut world)
            .unwrap_or_else(|e| panic!("{label} seed {seed}: {e}"));
        for event in &report.events {
            match event {
                Event::Acted { who, action, .. } => {
                    if matches!(who, Who::Monster(_)) {
                        acted.push(*action);
                    }
                }
                Event::Summoned { number, .. } => acted.push(*number),
                _ => {}
            }
        }
        let monster = battle.status(Who::Monster(0));
        let party = battle.status(Who::Party(0));
        statuses.push((
            monster.defence_level,
            monster.agility_level,
            party.dazzle_count,
        ));
    }
    assert!(
        battle.result().is_some(),
        "{label} seed {seed}: the battle never ended"
    );
    (battle, acted, statuses)
}

/// The chapter 2 ways are played whole: the herb (monster 23), Accelerate (20), Dazzle (32 and
/// 63), Weird Dance and its neighbours (43) and a no-op action (56), over 20 seeds each, with no
/// rule refused.
#[test]
fn chapter_two_ways_play_to_the_end_over_twenty_seeds() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    for (number, wanted) in [
        (23u16, 236u16),
        (20, 45),
        (32, 59),
        (63, 323),
        (43, 243),
        (56, 364),
    ] {
        let mut seen = BTreeSet::new();
        let (mut buffed, mut accelerated, mut dazzled) = (false, false, false);
        for seed in 0..20u64 {
            let setup = BattleSetup::set_battle(&tables, &[(number, 1)]).unwrap();
            let (battle, acted, statuses) = play_to_the_end(
                &format!("monster {number}"),
                &tables,
                setup,
                enduring_hero(),
                seed,
            );
            // A buff, a status and a no-op all leave the monster standing and its numbers whole.
            assert!(
                battle.monsters().iter().all(|m| m.fighter.hp >= 0),
                "monster {number} seed {seed}"
            );
            buffed |= statuses.iter().any(|s| s.0 != 0);
            accelerated |= statuses.iter().any(|s| s.1 != 0);
            dazzled |= statuses.iter().any(|s| s.2 > 0);
            seen.extend(acted);
        }
        assert!(
            seen.contains(&wanted),
            "monster {number} never chose {wanted}: {seen:?}"
        );
        match number {
            23 => assert!(buffed, "Buff moved the defence level"),
            20 => assert!(accelerated, "Accelerate moved the agility level"),
            32 | 63 => assert!(dazzled, "Dazzle set the party's status"),
            _ => {}
        }
    }
}

/// A monster that flees leaves the battle with no draw of its own: the way is chosen, the
/// action resolves and the monster is gone, over 20 seeds and every chapter 2 monster that holds
/// the way.
#[test]
fn fleeing_monsters_leave_over_twenty_seeds() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let mut fled_somewhere = false;
    for number in [1u16, 4, 20, 23, 30, 32, 43, 56, 57, 63, 290, 292] {
        let mut acted = BTreeSet::new();
        for seed in 0..20u64 {
            let mut build = Random::new(0x900 + seed);
            let setup = BattleSetup::encounter(&tables, &[(number, 1)], &mut build).unwrap();
            let (battle, actions, _) = play_to_the_end(
                &format!("flee {number}"),
                &tables,
                setup,
                enduring_hero(),
                seed,
            );
            acted.extend(actions);
            for j in 0..battle.monsters().len() {
                if battle.has_fled(Who::Monster(j)) {
                    fled_somewhere = true;
                    assert_eq!(battle.monsters()[j].fighter.hp, 0, "a fled monster is gone");
                    assert!(!battle.result().is_some_and(|r| r == BattleResult::Lost));
                }
            }
        }
        println!("flee monster {number}: {acted:?}");
    }
    assert!(
        fled_somewhere,
        "no monster ever left, though every chapter 2 monster holds the way"
    );
}

/// What a party member's Defend, an item and a monster's own flight do in the round state.
#[test]
fn defend_guards_for_the_round_and_an_item_is_reported() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let mut world = Random::new(3);
    let setup = BattleSetup::encounter(&tables, &[(1, 1)], &mut world).unwrap();
    let mut battle = Battle::new(setup, vec![enduring_hero()], Surprise::None, 3).unwrap();
    let report = battle.round(&[Command::Defend], &mut world).unwrap();
    assert!(report.events.contains(&Event::Defended(Who::Party(0))));
    // The guard level is a round's state: `func_ov000_0215e6e8` clears it before the round
    // returns. The halving itself is the kernel's (`battle::tests`).
    assert_eq!(battle.status(Who::Party(0)).guard, 0);

    // The medicinal herb's action through the item command, reported for the caller's bag.
    let herb = tables.actions.skill(236).unwrap();
    let mut hero = enduring_hero();
    hero.fighter.hp = 10;
    let mut world = Random::new(4);
    let setup = BattleSetup::set_battle(&tables, &[(23, 1)]).unwrap();
    let mut battle = Battle::new(setup, vec![hero], Surprise::None, 4).unwrap();
    let hp = battle.party()[0].fighter.hp;
    let report = battle
        .round(
            &[Command::Item {
                skill: herb,
                target: Target::Party(0),
            }],
            &mut world,
        )
        .unwrap();
    assert!(
        report.events.iter().any(|e| matches!(
            e,
            Event::ItemUsed {
                who: Who::Party(0),
                action: 236
            }
        )),
        "{:?}",
        report.events
    );
    assert!(
        battle.party()[0].fighter.hp > hp,
        "the herb healed its user"
    );
}

/// What a way's action comes to against the hero: the kernel's verdict, and the AI's (the
/// way rule, the AI mode and the targeting handler). Printed as the chapter 2 coverage table of
/// `docs/re/battle.md`.
#[test]
fn chapter_two_coverage_lists_what_the_kernel_and_the_ai_resolve() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();

    // Every monster of F01 (20001) and the Hexagon floors 7102 to 7104, in `encfld` (roamers)
    // and `encbtl` (roamers and company), plus set battles 2 (Hexagoon) and 26 (prologue).
    let field = encounter::Field::parse(rom.file_by_path("data/prm/encfld.bin").unwrap()).unwrap();
    let battle =
        encounter::Battle::parse(rom.file_by_path("data/prm/encbtl.bin").unwrap()).unwrap();
    let mut monsters: BTreeMap<u16, BTreeSet<String>> = BTreeMap::new();
    for map in [20001u16, 7102, 7103, 7104] {
        let m = field.find(map).unwrap_or_else(|| panic!("map {map}"));
        for zone in &m.zones {
            for r in &zone.roamers {
                monsters
                    .entry(r.monster)
                    .or_default()
                    .insert(format!("{map} zone {}", zone.id));
            }
            let z = battle.find(zone.id).unwrap();
            for r in &z.roamers {
                monsters
                    .entry(r.monster)
                    .or_default()
                    .insert(format!("{map} zone {}", zone.id));
            }
            for c in &z.company {
                monsters
                    .entry(c.monster)
                    .or_default()
                    .insert(format!("{map} zone {}", zone.id));
            }
        }
    }
    for number in [2u16, 26] {
        let record = scenario::battle(&rom, number).unwrap();
        for (monster, _) in record.monsters {
            monsters
                .entry(monster as u16)
                .or_default()
                .insert(format!("set battle {number}"));
        }
    }

    // No chapter-2 monster is metal: the route's checkpoints cannot move with the metal rules
    // (`docs/re/battle.md`, "The metal body").
    for (&number, places) in &monsters {
        let setup = BattleSetup::set_battle(&tables, &[(number, 1)]).unwrap();
        assert!(
            !setup.monsters[0].fighter.metal_body,
            "monster {number} is metal ({places:?})"
        );
    }

    // Nor can the weapon's killer bonus move one: the new game hands the hero the copper sword
    // 20004 and no companion joins on the route, and that weapon's only row that is not ten is
    // the eleventh tenth against family 2 — a family no monster of this set answers (the
    // Hexagoon, set battle 2, is family 1, whose row is ten).
    let params = BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat").unwrap()).unwrap();
    let copper = params
        .find(dq9_core::state::NEW_GAME_WEAPON as i16)
        .unwrap();
    assert_eq!(
        copper.family_bonus,
        [10, 11, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10]
    );
    for (&number, places) in &monsters {
        let setup = BattleSetup::set_battle(&tables, &[(number, 1)]).unwrap();
        let family = setup.monsters[0].fighter.family;
        let row = match family {
            1..=12 => copper.family_bonus[usize::from(family) - 1],
            _ => 10,
        };
        assert_eq!(
            row, 10,
            "monster {number} (family {family}, {places:?}) would take the copper sword's bonus"
        );
    }

    let hero = hero();
    let mut by_action: BTreeMap<u16, (Skill, BTreeSet<u16>, BTreeSet<String>)> = BTreeMap::new();
    for (&number, places) in &monsters {
        let setup = BattleSetup::set_battle(&tables, &[(number, 1)]).unwrap();
        let monster = &setup.monsters[0];
        let ai = match (monster.ai_mode, monster.selector) {
            (mode, _) if mode > 1 => format!("AI mode {mode}"),
            (_, 7) => "way rule 7".to_string(),
            _ => "ok".to_string(),
        };
        println!(
            "monster {number}: selector {} mode {} ways {:?} once {:#04x} extras class {} ({ai}) in {}",
            monster.selector,
            monster.ai_mode,
            monster.ways,
            monster.once_mask,
            monster.extra_class,
            places.iter().cloned().collect::<Vec<_>>().join(", ")
        );
        assert_eq!(ai, "ok", "monster {number}");
        for &way in &monster.ways {
            let entry = by_action
                .entry(way)
                .or_insert_with(|| (setup.skills[&way].clone(), BTreeSet::new(), BTreeSet::new()));
            entry.1.insert(number);
            let handler = match monster.ai_mode {
                1 => entry.0.handlers[0],
                _ => 0,
            };
            let ai = match handler {
                0 | 18 | 26 | 54 | 66 | 112 | 116 | 117 => "ok".to_string(),
                h => format!("handler {h}"),
            };
            let kernel = match &entry.0.action {
                Err(u) => format!("{u:?}"),
                Ok(action) => match validate(
                    &monster.fighter,
                    action,
                    std::slice::from_ref(&hero.fighter),
                ) {
                    Ok(()) => "ok".to_string(),
                    Err(u) => format!("{u:?}"),
                },
            };
            entry.2.insert(format!("kernel {kernel}; targeting {ai}"));
        }
    }
    println!("coverage: action | monsters | verdicts");
    for (action, (skill, who, verdicts)) in &by_action {
        println!(
            "coverage: {action} kind {} reach {} aims {} handlers {:?} | {who:?} | {}",
            skill.kind,
            skill.reach,
            skill.aims,
            skill.handlers,
            verdicts.iter().cloned().collect::<Vec<_>>().join(" / ")
        );
    }

    // No chapter 2 way is kind 3: neither Oomph nor Blunt reaches the route, so the attack
    // buff's multiplier cannot move a checkpoint.
    for (action, (skill, who, _)) in &by_action {
        assert_ne!(
            skill.kind, 3,
            "action {action} of {who:?} is kind 3 (the attack buff)"
        );
        // Nor is any kind 15 (Psyche Up, Egg On, 330, 567, 337, 338): no chapter 2 action
        // raises tension, so the route's tension level stays 0 wherever it is read.
        assert_ne!(
            skill.kind, 15,
            "action {action} of {who:?} is kind 15 (a tension rise)"
        );
    }

    // Every way of the chapter 2 list is resolved by the kernel and its handler is modelled.
    for plain in [1u16, 2] {
        if let Some((_, _, verdicts)) = by_action.get(&plain) {
            assert!(
                verdicts.iter().all(|v| v == "kernel ok; targeting ok"),
                "{verdicts:?}"
            );
        }
    }
    for action in [225u16, 41, 45, 59, 323, 243, 344, 364, 366] {
        let (_, _, verdicts) = &by_action[&action];
        assert!(
            verdicts.iter().all(|v| v == "kernel ok; targeting ok"),
            "{action}: {verdicts:?}"
        );
    }
    let _ = Unsupported::Defend;
}

/// The ROM's attack-buff records and the gate on the blow: kind 3's sign is the `+0x30` short
/// (Oomph +2, Blunt −2, one kind for both), and the plain Attack carries `+0x10` bit 9, which is
/// what makes `GetAttackBaseDamage` (`ov_024::021e7bc0`) read the buffed `+0x8`.
#[test]
fn the_attack_buff_records_carry_the_sign_and_the_blow_its_bit() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    for (number, step) in [(49u16, 2i16), (50, -2), (159, -1), (196, 1)] {
        let skill = tables
            .actions
            .skill(number)
            .unwrap_or_else(|| panic!("action {number}"));
        let action = skill.action.as_ref().unwrap();
        assert_eq!(skill.kind, 3, "action {number}");
        assert_eq!(action.stat_step, step, "action {number}'s +0x30");
        assert!(!action.attack_buff_applies, "action {number} is no blow");
    }
    let attack = tables.actions.skill(1).unwrap();
    let action = attack.action.as_ref().unwrap();
    assert_eq!(attack.kind, 1);
    assert!(
        action.attack_buff_applies,
        "the plain Attack reads the buffed attack"
    );
    assert!(
        action.tension_applies,
        "the plain Attack carries +0x10 bit 13 (tension works on it and is spent by it)"
    );
}

/// The tension table on the European ARM9: `0x020e8908` (the USA one is `0x020e88f8`, ten bytes
/// lower with the shift), five levels per side, the party's then the monster's. `func_02074738`
/// (`0x02074748`) indexes it `level × 8 + side × 4`; a level past four falls back to 1.0.
#[test]
fn the_tension_table_is_the_roms() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let arm9 = rom.arm9().unwrap();
    let at = (0x020e_8908u32 - arm9.load_addr) as usize;
    let word = |i: usize| {
        let bytes = &arm9.data[at + i * 4..at + i * 4 + 4];
        f32::from_bits(u32::from_le_bytes(bytes.try_into().unwrap()))
    };
    let party = [word(0), word(2), word(4), word(6), word(8)];
    let monster = [word(1), word(3), word(5), word(7), word(9)];
    assert_eq!(party, [1.0, 1.5, 2.5, 4.0, 6.0]);
    assert_eq!(monster, [1.0, 1.3, 2.0, 3.0, 4.5]);
}

/// The six kind-15 records of the EU action tables and the rise their `+0x30` carries
/// (`ov_024::021dc93c`: Psyche Up 161, Egg On 168, 330, 337, 338 and 567; `docs/re/battle.md`,
/// "Tension"). The two actions the handler singles out by number (`0x021dcec8`, 0x151 and
/// 0x152) name the level to reach (3 and 4); the other four add their short, 1 or 2. All six
/// cost no MP. The records the 306 oracle vectors replay (the plain Attack, Crack, Crackle,
/// Woosh, Heal) are not among them, so the rise's new branch cannot move a vector.
#[test]
fn the_six_kind_fifteen_records_carry_the_rise_and_its_step() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let mut rises: Vec<(u16, u8)> = (0..0x600u16)
        .filter_map(|number| tables.actions.skill(number))
        .filter(|skill| skill.kind == 15)
        .map(|skill| (skill.number, skill.reach))
        .collect();
    rises.sort_unstable();
    // Six records carry kind 15 (the parsed table also gives their `+0x30`: 1 on 161, 168 and
    // 567, 2 on 330, 3 on 337, 4 on 338 — `build/evidence/v4-psyche-up/kind15-records.txt`).
    assert_eq!(
        rises,
        [(161, 1), (168, 8), (330, 8), (337, 1), (338, 1), (567, 3)]
    );
    // The kind is modelled now: the four records whose reach is a battle reach (1 the actor, 3
    // everyone) resolve, and their `+0x30` is the rise. Egg On 168 and 330 still stop at their
    // reach 8, which the model does not establish.
    for (number, step) in [(161u16, 1i16), (337, 3), (338, 4), (567, 1)] {
        let action = tables
            .actions
            .skill(number)
            .unwrap()
            .action
            .unwrap_or_else(|error| panic!("action {number}: {error:?}"));
        assert_eq!(action.kind, 15, "action {number}");
        assert_eq!(action.stat_step, step, "action {number}'s +0x30");
        assert_eq!(action.mp_cost, 0, "action {number} costs no MP");
    }
    for number in [168u16, 330] {
        assert_eq!(
            tables.actions.skill(number).unwrap().action,
            Err(Unsupported::Reach(8)),
            "action {number} is refused by its reach, not its kind"
        );
    }
    let absolute: Vec<u16> = [161u16, 337, 338, 567]
        .into_iter()
        .filter(|&number| {
            tables
                .actions
                .skill(number)
                .unwrap()
                .action
                .unwrap()
                .tension_is_absolute()
        })
        .collect();
    assert_eq!(absolute, [0x151, 0x152]);

    // A landed Psyche Up on the hero raises the level and changes no HP or MP.
    let action = tables.actions.skill(161).unwrap().action.unwrap();
    let mut hero = Fighter::new(Side::Party);
    hero.hp = 100;
    hero.max_hp = 100;
    hero.attack = 100;
    hero.defence = 50;
    let mut risen = hero.clone();
    risen.tension_level = 2;
    let outcome = resolve_action(&mut Random::new(3), &hero, &action, &[risen.clone()], 0).unwrap();
    assert_eq!(outcome.tension_rises, [Some(3)]);
    assert_eq!(outcome.targets[0].hp_after, risen.hp);
    assert_eq!(outcome.targets[0].mp_damage, 0);

    for number in [1u16, 2, 12, 16, 18, 30] {
        let action = tables.actions.skill(number).unwrap().action.unwrap();
        assert_ne!(action.kind, 15, "oracle action {number}");
    }
}

/// The scope of the grudge rule and of enrage, counted on the ROM's 438 records: `+0x10` bit 26
/// (the grudge memory, `func_ov000_02154f30`) and the two trigger pairs of `+0x24`
/// (`func_ov024_021eb08c`: kind bits 0–6 with chance 7–13, kind 14–20 with chance 21–27).
/// The page's "263 of 438 carry the bit" is thereby a fact; its enrage kinds 3/4 (a party blow
/// below a half or a quarter) appear on no record, only the provocation kinds 0x11 and 0x12.
#[test]
fn the_grudge_bit_and_the_enrage_triggers_count_on_the_rom() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let table = BattleData::parse(data.monsters).unwrap();
    let total = table.monsters.len();
    let grudge = table
        .monsters
        .iter()
        .filter(|m| m.word_10 >> 26 & 1 != 0)
        .count();
    let kinds = |m: &dq9_formats::battle::monster::BattleMonster<'_>| {
        (
            (m.enrage & 0x7f) as u8,
            ((m.enrage & 0x1f_ffff) >> 14) as u8,
        )
    };
    let mut histogram: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    for m in &table.monsters {
        let (k1, k2) = kinds(m);
        if k1 != 0 || k2 != 0 {
            *histogram.entry((k1, k2)).or_default() += 1;
        }
    }
    println!("total {total}, grudge {grudge}, kinds {histogram:?}");
    let armed = table
        .monsters
        .iter()
        .filter(|m| {
            let k1 = m.enrage & 0x7f;
            let c1 = (m.enrage & 0x3fff) >> 7;
            let k2 = (m.enrage & 0x1f_ffff) >> 14;
            let c2 = (m.enrage & 0xfff_ffff) >> 21;
            (k1 != 0 && c1 != 0) || (k2 != 0 && c2 != 0)
        })
        .count();
    let present: BTreeSet<u8> = table
        .monsters
        .iter()
        .flat_map(|m| [kinds(m).0, kinds(m).1])
        .filter(|&k| k != 0)
        .collect();
    println!("armed {armed}, kinds {present:?}");
    assert_eq!(total, 438);
    assert_eq!(grudge, 263, "the page's count holds on the European ROM");
    assert_eq!(
        armed, 341,
        "the page's 287 is not reproduced by the record's own trigger pairs"
    );
    assert_eq!(
        present,
        BTreeSet::from([3, 4, 0x11, 0x12, 19, 20, 0x18]),
        "a blow below a half/a quarter, Whistle, Eyes on Me, a heal, a Zing, and 0x18"
    );
    let hexagoon = table.find(300).unwrap();
    assert_eq!(hexagoon.enrage, 0x1000_0000);
    assert_eq!(
        kinds(hexagoon),
        (0, 0),
        "the Hexagoon carries no trigger kind: a chapter-2 fight never enrages"
    );
}

/// The metal body on the ROM's own records (`docs/re/battle.md`, "The metal body"): monster 3
/// (metal slime, `mon_data` `+0x0A` bit 12) takes 0 or 1 from the plain Attack and its 8 from a
/// critical, while slime 1 takes the blow; Frizz, which carries no `+0x10` bit 24, misses it;
/// the exempt actions keep their damage; and the records of Metal Slash, Needle Shot and
/// Quadraslash are still refused, by the handler or the rider that is not modelled.
#[test]
fn the_metal_body_rules_read_the_rom_records() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();

    // The two records, by the name table's own metal flag.
    let metal = tables.names.find_name(b"metal slime").unwrap();
    assert_eq!(metal.number, 3);
    assert!(metal.metal, "mon_data +0x0A bit 12");
    let plain = tables.names.find_name(b"slime").unwrap();
    assert_eq!(plain.number, 1);
    assert!(!plain.metal);

    let hero = hero();
    let attack = tables.actions.skill(1).unwrap().action.unwrap();
    assert!(
        attack.works_on_metal && attack.aims_at_opponents,
        "the plain Attack carries +0x10 bit 24 and aims at the opponents"
    );

    let metal_target = BattleSetup::set_battle(&tables, &[(3, 1)])
        .unwrap()
        .monsters[0]
        .fighter
        .clone();
    assert!(metal_target.metal_body);
    let slime_target = BattleSetup::set_battle(&tables, &[(1, 1)])
        .unwrap()
        .monsters[0]
        .fighter
        .clone();
    assert!(!slime_target.metal_body);

    let mut seen = [false; 2];
    let mut criticals = 0;
    for seed in 0..40u64 {
        let outcome = resolve_action(
            &mut Random::new(seed),
            &hero.fighter,
            &attack,
            std::slice::from_ref(&metal_target),
            0,
        )
        .unwrap();
        let got = &outcome.targets[0];
        if got.critical {
            criticals += 1;
            assert!(got.amount > 1, "seed {seed}: a critical is not zeroed");
        } else {
            assert!((0..=1).contains(&got.amount), "seed {seed}: {}", got.amount);
            seen[got.amount as usize] = true;
        }
    }
    assert_eq!(seen, [true; 2], "the coin leaves 0 and 1");
    assert!(criticals > 0, "the hero's critical rate shows at some seed");

    let mut over_one = false;
    for seed in 0..40u64 {
        let outcome = resolve_action(
            &mut Random::new(seed),
            &hero.fighter,
            &attack,
            std::slice::from_ref(&slime_target),
            0,
        )
        .unwrap();
        over_one |= outcome.targets[0].amount > 1;
    }
    assert!(over_one, "a body that is not metal is not zeroed");

    // Frizz (9) aims at the opponents and carries no `+0x10` bit 24: it misses the metal body.
    let frizz = tables.actions.skill(9).unwrap().action.unwrap();
    assert!(frizz.aims_at_opponents && !frizz.works_on_metal);
    assert!(
        validate(&hero.fighter, &frizz, std::slice::from_ref(&metal_target)).is_ok(),
        "Frizz is a modelled action"
    );
    let missed = resolve_action(
        &mut Random::new(2),
        &hero.fighter,
        &frizz,
        std::slice::from_ref(&metal_target),
        0,
    )
    .unwrap();
    assert!(!missed.targets[0].landed, "the metal body's early miss");
    assert_eq!(missed.targets[0].amount, 0);
    let landed = resolve_action(
        &mut Random::new(2),
        &hero.fighter,
        &frizz,
        std::slice::from_ref(&slime_target),
        0,
    )
    .unwrap();
    assert!(
        landed.targets[0].landed,
        "the same cast on a body that is not metal"
    );

    // The exemptions: Quadraslash keeps its damage and is refused on its damage handler alone;
    // Needle Shot on its rider; Metal Slash and Metalicker on theirs.
    let quadraslash = tables.actions.skill(0x205).unwrap().action.unwrap();
    assert!(quadraslash.works_on_metal && quadraslash.aims_at_opponents);
    assert_eq!(
        validate(
            &hero.fighter,
            &quadraslash,
            std::slice::from_ref(&metal_target)
        ),
        Err(Unsupported::DamageHandler(66))
    );
    let needle_shot = tables.actions.skill(0x82).unwrap().action.unwrap();
    assert_eq!(
        validate(
            &hero.fighter,
            &needle_shot,
            std::slice::from_ref(&metal_target)
        ),
        Err(Unsupported::Rider(20))
    );
    for number in [0x40u16, 0x7E] {
        let slasher = tables.actions.skill(number).unwrap().action.unwrap();
        assert_eq!(
            validate(&hero.fighter, &slasher, std::slice::from_ref(&metal_target)),
            Err(Unsupported::DamageHandler(2)),
            "{number:#x}"
        );
    }
    // Action 0x150 is not refused by number, but it carries no bit 24: the metal body misses it
    // too (`RollAccuracy`'s `!= 0x150` guard covers only the two status early misses).
    let other = tables.actions.skill(0x150).unwrap().action.unwrap();
    assert!(!other.works_on_metal);
    assert!(validate(&hero.fighter, &other, std::slice::from_ref(&metal_target)).is_ok());
    let missed = resolve_action(
        &mut Random::new(2),
        &hero.fighter,
        &other,
        std::slice::from_ref(&metal_target),
        0,
    )
    .unwrap();
    assert!(!missed.targets[0].landed);
}

/// The weapon behind the two metal flags. `func_02085128` (the "+1 against metal") tests bit 4 of
/// the `itembtlprm` `+0x00` word `func_ov017_021b3780` copies to the character's `+0x2F4`, and
/// `func_020851d8` (the poison needle) bit 10; the copy puts the weapon first (live slot 8, the
/// table `08 00 09 07 05 01 06 0A`), which is the record both accessors read. Counted over the
/// ROM's 268 weapons: seven carry bit 4 and one bit 10, and the party's adapter reads them from
/// the equipped weapon alone.
#[test]
fn the_equipped_weapon_sets_the_metal_and_needle_flags() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let params = BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat").unwrap()).unwrap();
    let ids = |bit: u32| -> Vec<i16> {
        params
            .items
            .iter()
            .filter(|p| p.flags >> bit & 1 != 0)
            .map(|p| p.id)
            .collect()
    };
    assert_eq!(ids(4), [20021, 20022, 20023, 20119, 20517, 20518, 20519]);
    assert_eq!(ids(10), [19057]);

    let weapons = member(&rom, "data/prm/itemdt_w.gp2", "itemdt_w_en.nat");
    let items = ItemStats::from_members(&[&weapons]).unwrap();
    let fighter = |weapon: u16| {
        let mut character = Character::hero();
        character.equipment.weapon = Some(weapon);
        character.fighter(&items, &params)
    };

    // The creation copper sword 20004 carries neither flag.
    let plain = fighter(20004);
    assert!(
        !plain.metal_bonus && !plain.poison_needle,
        "the copper sword"
    );
    let metal = fighter(20021);
    assert!(metal.metal_bonus && !metal.poison_needle);
    let needle = fighter(19057);
    assert!(!needle.metal_bonus && needle.poison_needle);

    // The needle's blow through the adapter: exactly 1 and neither dodged nor blocked, on a body
    // that is not metal and at rates no roll clears.
    let data = Data::load(&rom);
    let tables = data.tables();
    let attack = tables.actions.skill(1).unwrap().action.unwrap();
    assert!(
        attack.weapon_element,
        "the plain Attack carries +0x10 bit 18"
    );
    let mut slime = BattleSetup::set_battle(&tables, &[(1, 1)])
        .unwrap()
        .monsters[0]
        .fighter
        .clone();
    slime.evasion_rate = 100.0;
    slime.block_rate = 100.0;
    let outcome = resolve_action(&mut Random::new(5), &needle, &attack, &[slime], 0).unwrap();
    assert!(!outcome.targets[0].dodged, "the needle is not dodged");
    assert!(!outcome.targets[0].blocked, "the needle is not blocked");
    assert_eq!(outcome.targets[0].amount, 1);
}

/// The weapon's killer bonus on the ROM's own rows: `itembtlprm` `+0x08`–`+0x10` (twelve signed
/// six-bit tenths, 10 none) against the target's family (`mon_data` `+0x0A` bits 7–10). The
/// copper sword — the pilot's only weapon — is ten everywhere but **eleven against family 2**; a
/// metal slime spear's first row is eleven and multiplies a blow on a family-1 monster by 1.1;
/// and a party target answers no family at all (`func_ov000_02156068` with a party index).
#[test]
fn the_weapon_family_bonus_reads_the_rom_rows_and_the_monster_family() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let params = BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat").unwrap()).unwrap();

    let copper = params.find(20004).expect("the copper sword");
    assert_eq!(
        copper.family_bonus,
        [10, 11, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10],
        "the new game's weapon: eleven tenths against family 2 alone"
    );
    let spear = params.find(20517).expect("the metal slime spear");
    assert_eq!(spear.family_bonus[0], 11, "+0x0C bits 6-11, family 1");
    assert_eq!(spear.family_bonus[1..], [10; 11]);

    // The adapter: the row comes from the equipped weapon alone, and a member answers no family.
    let weapons = member(&rom, "data/prm/itemdt_w.gp2", "itemdt_w_en.nat");
    let items = ItemStats::from_members(&[&weapons]).unwrap();
    let fighter = |weapon: u16| {
        let mut character = Character::hero();
        character.equipment.weapon = Some(weapon);
        character.fighter(&items, &params)
    };
    let armed = fighter(20517);
    assert_eq!(armed.family_bonus[0], 11);
    assert_eq!(armed.family, 0, "a party member answers no family");
    assert_eq!(
        fighter(20004).family_bonus,
        [10, 11, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10]
    );

    // A real monster of family 1, built by the setup.
    let family_one = tables
        .names
        .monsters
        .iter()
        .find(|n| n.family == 1 && !n.metal && tables.monsters.find(n.number).is_some())
        .expect("a family-1 monster in the battle data");
    let target = BattleSetup::set_battle(&tables, &[(family_one.number, 1)])
        .expect("a set battle of one")
        .monsters[0]
        .fighter
        .clone();
    assert_eq!(
        target.family, 1,
        "mon_data +0x0A bits 7-10 through the setup"
    );

    let attack = tables.actions.skill(1).unwrap().action.unwrap();
    assert!(
        attack.weapon_element,
        "the plain Attack carries +0x10 bit 18"
    );
    let mut unscaled = armed.clone();
    unscaled.family_bonus = [10; 12];
    let mut differed = false;
    for seed in 0..64u64 {
        let boosted = resolve_action(
            &mut Random::new(seed),
            &armed,
            &attack,
            std::slice::from_ref(&target),
            0,
        )
        .unwrap()
        .targets[0]
            .clone();
        let plain = resolve_action(
            &mut Random::new(seed),
            &unscaled,
            &attack,
            std::slice::from_ref(&target),
            0,
        )
        .unwrap()
        .targets[0]
            .clone();
        // A critical's own damage is already truncated before the bonus, and a blow that deals
        // nothing takes the coin: those seeds do not pin the ratio.
        if plain.critical || plain.amount == 0 {
            continue;
        }
        assert_eq!(
            boosted.amount,
            (plain.amount as f32 * 1.1) as i32,
            "seed {seed}: eleven tenths of {}",
            plain.amount
        );
        differed |= boosted.amount != plain.amount;
    }
    assert!(differed, "the eleven-tenths row changed a blow");

    // A party target answers no family: the same blow, unchanged.
    let member = Fighter::new(Side::Party);
    let boosted = resolve_action(
        &mut Random::new(1),
        &armed,
        &attack,
        std::slice::from_ref(&member),
        0,
    )
    .unwrap()
    .targets[0]
        .amount;
    let plain = resolve_action(
        &mut Random::new(1),
        &unscaled,
        &attack,
        std::slice::from_ref(&member),
        0,
    )
    .unwrap()
    .targets[0]
        .amount;
    assert_eq!(boosted, plain);
    assert_eq!(member.family, 0);
}

/// The needle's accuracy auto-hit (`RollAccuracy`, `ov_000::02156648`, `0x02156814`–`0x02156830`):
/// on an action carrying `+0x10` bit 18, a party attacker whose equipped weapon carries bit 10 of
/// the `itembtlprm` record hits right after the die of 100 — before the dazzle's die of eight and
/// before the accuracy comparison. The poison needle (19057) comes from the item table, the spoilt
/// bit from an action record of the ROM.
#[test]
fn a_dazzled_needle_bearer_auto_hits_before_the_die_of_eight() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let spoiled = (0..0x600u16)
        .filter_map(|number| tables.actions.skill(number))
        .find(|s| {
            s.kind == 1
                && s.action
                    .as_ref()
                    .is_ok_and(|a| a.spoilt && a.weapon_element)
        })
        .expect("an action whose +0x10 carries bit 3 (spoilt) and bit 18");
    let action = spoiled.action.unwrap();

    let params = BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat").unwrap()).unwrap();
    let weapons = member(&rom, "data/prm/itemdt_w.gp2", "itemdt_w_en.nat");
    let items = ItemStats::from_members(&[&weapons]).unwrap();
    let fighter = |weapon: u16| {
        let mut character = Character::hero();
        character.equipment.weapon = Some(weapon);
        character.fighter(&items, &params)
    };
    let mut needle = fighter(19057);
    needle.dazzled = true;
    let mut plain = fighter(20004);
    plain.dazzled = true;
    assert!(needle.poison_needle && !plain.poison_needle);

    let mut target = Fighter::new(Side::Monster);
    target.hp = 100;
    target.max_hp = 100;
    let mut misses = 0;
    for seed in 0..64u64 {
        let auto = resolve_action(
            &mut Random::new(seed),
            &needle,
            &action,
            &[target.clone()],
            0,
        )
        .unwrap();
        assert!(
            auto.targets[0].landed,
            "seed {seed}: the needle auto-hits through the dazzle"
        );
        let spoilt = resolve_action(
            &mut Random::new(seed),
            &plain,
            &action,
            &[target.clone()],
            0,
        )
        .unwrap();
        if spoilt.targets[0].landed {
            // The same die, critical, dodge, block, accuracy and amount draws, plus the die of
            // eight the auto-hit skipped.
            assert_eq!(
                spoilt.draws,
                auto.draws + 1,
                "seed {seed}: the die of eight"
            );
        } else {
            misses += 1;
        }
    }
    assert!(misses > 0, "five faces of eight miss: {misses} of 64");
}

/// The twelve records of the EU action tables that carry `+0x10` bit 17 — and no other record of
/// the 681 — and the rank factor they get (`docs/re/battle.md`, "The several-target falloff";
/// the encyclopedia's B11). The page's "12 actions only" is thereby a fact: the four Attack
/// records (1, 2, 223, 224) plus Lashings of Love, Hit the Hay, Serpent's Bite, Party Pooper,
/// Axes of Evil, Crosscutter Throw, Ooze Bruiser and Metalicker. The four spells of the oracle's
/// 306 vectors (Crack, Crackle, Woosh, Heal) carry neither the bit, and the Attack vectors pass
/// one target, so the vectors cannot move.
#[test]
fn the_twelve_records_carry_the_falloff_and_no_others_do() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let mut weakening: Vec<u16> = (0..0x600u16)
        .filter_map(|number| tables.actions.skill(number))
        .filter(|skill| skill.action.as_ref().is_ok_and(|a| a.multi_target_falloff))
        .map(|skill| skill.number)
        .collect();
    weakening.sort_unstable();
    assert_eq!(
        weakening,
        [1, 2, 86, 87, 91, 94, 113, 121, 123, 126, 223, 224]
    );
    for number in [12u16, 16, 18, 30] {
        let action = tables.actions.skill(number).unwrap().action.unwrap();
        assert!(!action.multi_target_falloff, "action {number}");
    }

    // Party Pooper (94, one of the twelve; handler 0, rider 0) against three monsters: the first
    // target takes exactly the single-target amount of the same seed (rank 0 keeps the table's
    // 1.0), and the next two are four fifths and three fifths of their own bases.
    let action = tables.actions.skill(94).unwrap().action.unwrap();
    let mut actor = Fighter::new(Side::Party);
    actor.attack = 400;
    actor.deftness = 0;
    let mut target = Fighter::new(Side::Monster);
    target.hp = 1000;
    target.max_hp = 1000;
    let one = resolve_action(
        &mut Random::new(5),
        &actor,
        &action,
        std::slice::from_ref(&target),
        0,
    )
    .unwrap();
    let three = resolve_action(
        &mut Random::new(5),
        &actor,
        &action,
        &[target.clone(), target.clone(), target],
        0,
    )
    .unwrap();
    assert_eq!(one.targets[0].landed, three.targets[0].landed);
    assert_eq!(three.targets[0].amount, one.targets[0].amount);
    // The physical base is 200 ± its sixteenth ±1, read as 186..=213; the rank's factor
    // truncates it to 148..=170 and 111..=127 — three disjoint bands, so the list order is
    // pinned despite the amount's own draws.
    let amounts: Vec<i32> = three.targets.iter().map(|t| t.amount).collect();
    assert!((186..=213).contains(&amounts[0]), "{amounts:?}");
    assert!((148..=170).contains(&amounts[1]), "{amounts:?}");
    assert!((111..=127).contains(&amounts[2]), "{amounts:?}");
}

/// The battle's flow resolves a group action that carries `+0x10` bit 17: Party Pooper (94) on a
/// group of three slimes goes through the round's targeting (`Target::Monster(0)` widened by the
/// action's Group reach) and `Battle::step_combo` (the record has no combo bit), and the three
/// amounts fall in the rank bands around 1.0, 0.8 and 0.6 of a 100 base.
#[test]
fn the_flow_resolves_a_weakening_group_action_on_three_monsters() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let mut strong = hero();
    strong.fighter.attack = 200;
    let mut world = Random::new(11);
    let setup = BattleSetup::encounter(&tables, &[(1, 3)], &mut world).unwrap();
    let mut battle = Battle::new(setup, vec![strong], Surprise::None, 0x3000).unwrap();
    let command = Command::Act {
        skill: tables.actions.skill(94).unwrap(),
        target: Target::Monster(0),
    };
    let report = battle.round(&[command], &mut world).unwrap();
    let amounts: Vec<i32> = report
        .events
        .iter()
        .find_map(|event| match event {
            Event::Acted {
                who: Who::Party(0),
                action: 94,
                outcomes,
            } => Some(outcomes.iter().map(|(_, o)| o.amount).collect()),
            _ => None,
        })
        .expect("the hero's Party Pooper resolves in the first round");
    assert_eq!(amounts.len(), 3, "{amounts:?}");
    assert!((94..=106).contains(&amounts[0]), "{amounts:?}");
    assert!((75..=84).contains(&amounts[1]), "{amounts:?}");
    assert!((56..=63).contains(&amounts[2]), "{amounts:?}");
}

/// The EU weapons whose `itembtlprm` `+0x00` bits 0–1 name a reach: `func_ov000_021540fc`
/// (`0x021540fc`) turns a reach-5 party Attack into bit 0 → a group (4), bit 1 → everyone (3),
/// neither → one target (2), and `ResolveAction`'s several-target gate accepts the same two bits
/// (`0x021ec6b0`–`0x021ec6c8`). The table holds **42**: the 18 whips 20300–20317 (bit 0), the
/// four Gringham whips 20318–20321 and the 20 boomerangs 20700–20719 (bit 1). The pilot's copper
/// sword 20004 carries neither, so the chapter-2 route's Attack stays on one target.
#[test]
fn the_several_target_weapons_set_the_reach_from_the_rom() {
    use dq9_core::battle::model::WeaponReach;

    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let params = BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat").unwrap()).unwrap();
    let ids = |reach: u8| -> Vec<i16> {
        params
            .items
            .iter()
            .filter(|p| p.weapon_reach() == reach)
            .map(|p| p.id)
            .collect()
    };
    assert_eq!(
        ids(4),
        (20300..=20317).collect::<Vec<i16>>(),
        "the group whips"
    );
    assert_eq!(
        ids(3),
        (20318..=20321).chain(20700..=20719).collect::<Vec<i16>>(),
        "the Gringham whips and the boomerangs"
    );
    assert_eq!(
        ids(2).len(),
        1423 - 42,
        "every other record names one target"
    );

    let weapons = member(&rom, "data/prm/itemdt_w.gp2", "itemdt_w_en.nat");
    let items = ItemStats::from_members(&[&weapons]).unwrap();
    let fighter = |weapon: u16| {
        let mut character = Character::hero();
        character.equipment.weapon = Some(weapon);
        character.fighter(&items, &params)
    };
    assert_eq!(
        fighter(20004).weapon_reach,
        WeaponReach::One,
        "the copper sword"
    );
    assert_eq!(
        fighter(20300).weapon_reach,
        WeaponReach::Group,
        "the leather whip"
    );
    assert_eq!(
        fighter(20318).weapon_reach,
        WeaponReach::All,
        "the Gringham whip"
    );
    assert_eq!(
        fighter(20700).weapon_reach,
        WeaponReach::All,
        "the boomerang"
    );
    // An empty weapon slot keeps one target (`func_ov017_021b3780`'s zeroed record).
    let mut bare = Character::hero();
    bare.equipment.weapon = None;
    assert_eq!(bare.fighter(&items, &params).weapon_reach, WeaponReach::One);
}

/// A party plain Attack (record 1, `+0x10` bit 17) on three monsters: with a group whip every
/// target's base is truncated by the rank table (1.0, 0.8, 0.6) — the bands are the base's
/// 186..=213 around a 400 attack, scaled — while a sword's Attack keeps all three at full.
#[test]
fn a_whip_weakens_across_a_group_and_a_sword_does_not() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let data = Data::load(&rom);
    let tables = data.tables();
    let params = BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat").unwrap()).unwrap();
    let weapons = member(&rom, "data/prm/itemdt_w.gp2", "itemdt_w_en.nat");
    let items = ItemStats::from_members(&[&weapons]).unwrap();
    let attack = tables.actions.skill(1).unwrap().action.unwrap();
    assert!(
        attack.multi_target_falloff,
        "the plain Attack carries +0x10 bit 17"
    );
    let actor = |weapon: u16| {
        let mut character = Character::hero();
        character.equipment.weapon = Some(weapon);
        let mut fighter = character.fighter(&items, &params);
        // A clean base: 400 attack, no buff, no element, no family row for the bare target.
        fighter.attack = 400;
        fighter.deftness = 0;
        fighter
    };
    let mut target = Fighter::new(Side::Monster);
    target.hp = 1000;
    target.max_hp = 1000;
    let three = |weapon: u16| {
        resolve_action(
            &mut Random::new(5),
            &actor(weapon),
            &attack,
            &[target.clone(), target.clone(), target.clone()],
            0,
        )
        .unwrap()
        .targets
        .iter()
        .map(|t| t.amount)
        .collect::<Vec<i32>>()
    };

    let whip = three(20300);
    assert_eq!(whip.len(), 3);
    assert!((186..=213).contains(&whip[0]), "{whip:?}");
    assert!((148..=170).contains(&whip[1]), "{whip:?}");
    assert!((111..=127).contains(&whip[2]), "{whip:?}");

    let sword = three(20004);
    for amount in &sword {
        assert!((186..=213).contains(amount), "{sword:?}");
    }
}
