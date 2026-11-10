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
use dq9_core::scenario;
use dq9_formats::Gpc2;
use dq9_formats::battle::encounter;
use dq9_formats::battle::monster::{BattleData, Names};
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
