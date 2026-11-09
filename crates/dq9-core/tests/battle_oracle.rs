//! Differential test of the battle kernel against its two oracles (`docs/re/battle.md`).
//!
//! Both fixtures are outputs of programs, not ROM data (see their headers):
//!
//! - `battle_oracle_primitives.txt`: the decomp's `Random.cpp`, `CritRateCalculation.cpp` and
//!   `BasicAttackCalculation.cpp` compiled unchanged on the host (Unlicense);
//! - `battle_oracle_actions.txt`: DQIX/BattleEmulator's `callAttackFun` (MIT) driven with
//!   arbitrary numbers.
//!
//! The numbers of the action vectors are synthetic; they stand for the fields of an action and a
//! fighter the kernel reads.

use std::collections::HashMap;

use dq9_core::battle::damage::{
    calculate_crit_rate, calculate_physical_damage, critical_threshold, round_up,
};
use dq9_core::battle::{
    Action, ActionRange, Amount, Fighter, Random, Reach, ScaleStat, Side, resolve_action,
};

const PRIMITIVES: &str = include_str!("data/battle_oracle_primitives.txt");
const ACTIONS: &str = include_str!("data/battle_oracle_actions.txt");

/// `key=value` tokens of a fixture line.
fn fields<'a>(tokens: impl Iterator<Item = &'a str>) -> HashMap<&'a str, &'a str> {
    tokens
        .filter(|t| *t != "|")
        .map(|t| {
            t.split_once('=')
                .unwrap_or_else(|| panic!("token {t:?} is not key=value"))
        })
        .collect()
}

fn num<T: std::str::FromStr>(map: &HashMap<&str, &str>, key: &str) -> T {
    map[key]
        .parse()
        .unwrap_or_else(|_| panic!("field {key}={:?}", map[key]))
}

fn hex64(map: &HashMap<&str, &str>, key: &str) -> u64 {
    u64::from_str_radix(map[key], 16).unwrap()
}

fn hex32(map: &HashMap<&str, &str>, key: &str) -> u32 {
    u32::from_str_radix(map[key], 16).unwrap()
}

fn f32_bits(map: &HashMap<&str, &str>, key: &str) -> f32 {
    f32::from_bits(hex32(map, key))
}

#[test]
fn primitives_match_the_decompilation_compiled_on_the_host() {
    let mut seen = HashMap::<&str, usize>::new();
    for line in PRIMITIVES
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let mut tokens = line.split_whitespace();
        let kind = tokens.next().unwrap();
        let map = fields(tokens);
        *seen.entry(kind).or_default() += 1;
        match kind {
            "next" => {
                let mut rng = Random::new(hex64(&map, "seed"));
                for (k, expected) in map["out"].split(',').enumerate() {
                    assert_eq!(
                        rng.next_u32(),
                        u32::from_str_radix(expected, 16).unwrap(),
                        "{line} #{k}"
                    );
                }
            }
            "max" => {
                let n: i32 = num(&map, "n");
                let mut rng = Random::new(hex64(&map, "seed"));
                for (k, expected) in map["out"].split(',').enumerate() {
                    assert_eq!(
                        rng.next_max(n),
                        expected.parse::<i32>().unwrap(),
                        "{line} #{k}"
                    );
                }
            }
            "between" => {
                let (min, max) = (f32_bits(&map, "min"), f32_bits(&map, "max"));
                let mut rng = Random::new(hex64(&map, "seed"));
                for (k, expected) in map["out"].split(',').enumerate() {
                    let got = rng.next_float_between(min, max).to_bits();
                    assert_eq!(
                        got,
                        u32::from_str_radix(expected, 16).unwrap(),
                        "{line} #{k}"
                    );
                }
            }
            "crit" => {
                let rate = calculate_crit_rate(
                    num(&map, "deftness"),
                    f32_bits(&map, "accessory"),
                    f32_bits(&map, "book"),
                    f32_bits(&map, "skill"),
                    num(&map, "hits"),
                );
                assert_eq!(rate.to_bits(), hex32(&map, "rate"), "{line}");
                assert_eq!(
                    critical_threshold(rate),
                    num::<i32>(&map, "threshold"),
                    "{line}"
                );
            }
            "roundup" => {
                assert_eq!(
                    round_up(f32_bits(&map, "in")),
                    num::<i32>(&map, "out"),
                    "{line}"
                );
            }
            "physical" => {
                let mut rng = Random::new(hex64(&map, "seed"));
                let got =
                    calculate_physical_damage(num(&map, "attack"), num(&map, "defence"), &mut rng);
                assert_eq!(got.to_bits(), hex32(&map, "out"), "{line}");
                assert_eq!(rng.state(), hex64(&map, "state"), "draws spent: {line}");
            }
            other => panic!("unknown primitive vector {other:?}"),
        }
    }
    for kind in ["next", "max", "between", "crit", "roundup", "physical"] {
        assert!(seen.get(kind).copied().unwrap_or(0) > 0, "no {kind} vector");
    }
}

/// The two draws BattleEmulator spends before every action's own draws (`(*position) += 2`).
/// What they are is not established (`docs/re/battle.md`, "Open points"); the kernel's contract
/// starts after them.
const ORACLE_PRELUDE_DRAWS: u32 = 2;

/// The plain Attack (action 1) with the flags the EU action table sets on it (`docs/re/battle.md`,
/// "Action fields"): reach 5, elements 8 and 8, dodgeable, blockable, the wake bit, the combo
/// bit, and `+0x10` bits 16, 17 and 18, which only matter for a multi-hit weapon, several
/// targets and an elemental weapon.
fn plain_attack() -> Action {
    let mut action = Action::new(1, 1, Amount::Blow { multiplier: 1.0 });
    action.reach = Reach::Attack;
    action.element = 8;
    action.landing_element = 8;
    action.dodgeable = true;
    action.blockable = true;
    action.wakes_target = true;
    action.critical_percent = 100;
    action.chains_combo = true;
    action.multi_target_falloff = true;
    action.weapon_element = true;
    action.trait_accuracy_die = true;
    action
}

/// A damaging spell of the EU table: kind 1, critical multiplier 50, tension-able. The range
/// numbers are synthetic except the spread and the party's least amount, which are the ones
/// BattleEmulator draws with (Crack 5 and 30, Crackle 8 and 50, Woosh 8 and 16, Heal 5 and 35).
fn spell(number: u16, reach: Reach, element: u8, cost: u8, spread: u16, min: u16) -> Action {
    let range = ActionRange {
        spread,
        monster_amount: min / 2,
        party_min: min,
        party_max: min * 4,
        lo: 50,
        hi: 999,
        scaling: Some(ScaleStat::Might),
    };
    let mut action = Action::new(number, 1, Amount::Range(range));
    action.reach = reach;
    action.element = element;
    action.mp_cost = cost;
    action.critical_percent = 50;
    action
}

/// The actor, the action and the target a scenario stands for, with the numbers of one vector.
fn scenario(name: &str, map: &HashMap<&str, &str>) -> (Fighter, Action, Fighter) {
    let atk: i32 = num(map, "atk");
    let def: i32 = num(map, "def");
    let hp: i32 = num(map, "hp");
    let max_hp: i32 = num(map, "max_hp");
    let mut party = Fighter::new(Side::Party);
    party.deftness = num(map, "deftness");
    party.mp = num(map, "mp");
    party.hp = hp;
    party.max_hp = max_hp;
    let mut monster = Fighter::new(Side::Monster);
    monster.hp = 10_000;
    monster.max_hp = 10_000;
    match name {
        "attack_party" => {
            party.attack = atk;
            monster.defence = def;
            (party, plain_attack(), monster)
        }
        "attack_monster" => {
            monster.attack = atk;
            party.defence = def;
            party.evasion_rate = 2.0;
            party.block_rate = 0.5;
            (monster, plain_attack(), party)
        }
        "crack" => {
            // Blockable, chains into a combo (`+0x10` bit 6, `+0x2C` bit 27).
            let mut action = spell(0x100, Reach::One, 2, 3, 5, 30);
            action.blockable = true;
            action.chains_combo = true;
            (party, action, monster)
        }
        "crackle" => {
            let mut action = spell(0x101, Reach::Group, 2, 8, 8, 50);
            action.blockable = true;
            (party, action, monster)
        }
        "woosh" => {
            // Not blockable; the wind resistance stands for the oracle's `× 0.75`.
            let action = spell(0x102, Reach::Group, 3, 3, 8, 16);
            monster.resistances[2] = 75;
            (party, action, monster)
        }
        "heal" => {
            let mut action = spell(0x103, Reach::One, 0, 2, 5, 35);
            action.kind = 2;
            if let Amount::Range(range) = &mut action.amount {
                range.scaling = Some(ScaleStat::Mending);
            }
            let mut target = Fighter::new(Side::Party);
            target.hp = 1;
            target.max_hp = 10_000;
            (party, action, target)
        }
        other => panic!("unknown scenario {other:?}"),
    }
}

/// Oracle deviation, settled by the ROM: BattleEmulator's Woosh truncates the critical's damage
/// to an integer **before** the wind resistance (`(int)(damage × 0.75)` on an integer), while the
/// game's `CalculateFinalDamage` (`ov_024::021e6a90`, EU) keeps the float through the resistance
/// and has a single float-to-int conversion at its end (`docs/re/battle.md`, "Oracle deviations").
/// The kernel follows the game: on a Woosh critical it is the oracle's amount or one more.
fn is_woosh_critical(name: &str, map: &HashMap<&str, &str>) -> bool {
    name == "woosh" && num::<i32>(map, "crit") != 0
}

#[test]
fn actions_match_battle_emulator() {
    let mut cases = 0;
    let mut per_class = HashMap::<String, usize>::new();
    let mut mismatches = Vec::new();
    let mut deviations = 0;
    for line in ACTIONS.lines().filter(|l| l.starts_with("case ")) {
        let mut tokens = line.split_whitespace().skip(1);
        let name = tokens.next().unwrap();
        let map = fields(tokens);
        let (actor, action, target) = scenario(name, &map);
        // The critical threshold of the vector is the decomp's, on the host; the kernel must
        // reach the same one from the deftness.
        if actor.side == Side::Party {
            let rate = dq9_core::battle::damage::critical_rate(&actor, &action);
            assert_eq!(
                critical_threshold(rate),
                num::<i32>(&map, "crit_p"),
                "{line}"
            );
        }
        let mut rng = Random::new(hex64(&map, "seed"));
        for _ in 0..ORACLE_PRELUDE_DRAWS {
            rng.next_u32();
        }
        let combo: u8 = num(&map, "combo");
        let outcome = resolve_action(&mut rng, &actor, &action, &[target], combo)
            .unwrap_or_else(|e| panic!("{line}: {e}"));
        let got = &outcome.targets[0];
        let mut expected = (
            num::<u32>(&map, "draws"),
            num::<i32>(&map, "crit") != 0,
            num::<i32>(&map, "dodged") != 0,
            num::<i32>(&map, "blocked") != 0,
            num::<i32>(&map, "amount"),
            num::<i32>(&map, "mp_after"),
        );
        let actual = (
            outcome.draws,
            got.critical,
            got.dodged,
            got.blocked,
            got.amount,
            actor.mp - outcome.mp_spent,
        );
        if is_woosh_critical(name, &map) && actual.4 - expected.4 == 1 {
            expected.4 += 1;
            deviations += 1;
        }
        cases += 1;
        *per_class
            .entry(format!("{name}/{}", map["class"]))
            .or_default() += 1;
        if expected != actual {
            mismatches.push(format!("{line}\n   kernel: {actual:?}"));
        }
    }
    println!("{cases} action vectors replayed, {deviations} Woosh critical oracle deviations");
    assert!(cases >= 300, "only {cases} vectors");
    assert!(
        mismatches.is_empty(),
        "{} of {cases} differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    // The classes the vectors exist for: dodges, blocks, criticals, coins, every scenario.
    for class in [
        "attack_party/crit",
        "attack_monster/dodged",
        "attack_monster/blocked",
        "attack_monster/small",
        "crack/crit",
        "crackle/crit",
        "woosh/crit",
        "heal/crit",
    ] {
        assert!(
            per_class.get(class).copied().unwrap_or(0) >= 10,
            "class {class}: {per_class:?}"
        );
    }
    assert!(
        deviations <= per_class["woosh/crit"],
        "{deviations} deviations"
    );
}
