//! `dq9re monster`, `dq9re action` and `dq9re encounters`: the battle tables read by
//! `dq9_formats::battle` (`docs/re/battle-data.md`).

use std::borrow::Cow;

use anyhow::{Context as _, Result, bail};
use dq9_formats::battle::{action, encounter, monster};
use dq9_formats::maplist::{self, MapList};
use dq9_formats::{Gpc2, Table, map};
use nds_rom::Rom;

/// Language suffix of the name and list members read.
const LANG: &str = "en";

#[derive(clap::Args)]
pub struct MonsterArgs {
    /// The monster's number (`300`), code (`b003a`) or name (`slime`).
    pub target: String,
}

fn file<'a>(rom: &'a Rom, path: &str) -> Result<&'a [u8]> {
    map::rom_file(rom, path).with_context(|| format!("no {path}"))
}

fn member<'a>(rom: &'a Rom, path: &str, name: &str) -> Result<Cow<'a, [u8]>> {
    let bytes = file(rom, path)?;
    let gpc2 = Gpc2::parse(bytes).with_context(|| format!("parsing {path}"))?;
    let entry = gpc2
        .find(name)
        .with_context(|| format!("no {name} in {path}"))?;
    gpc2.member(entry)
        .with_context(|| format!("reading {name} of {path}"))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn yes(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
}

/// The drop chance a `+0x02` step stands for (the table at `0x021fd888`).
fn drop_chance(step: u8) -> String {
    match step {
        0 => "100%".to_owned(),
        1..=6 => format!("1 in {}", 1u32 << (step + 2)),
        7 => "never".to_owned(),
        other => format!("step {other} (not in the table)"),
    }
}

fn monster_name(names: &monster::Names<'_>, number: u16) -> String {
    names
        .find(number)
        .map_or_else(|| format!("monster {number}"), |n| text(n.name))
}

pub fn run_monster(rom: &Rom, args: &MonsterArgs) -> Result<()> {
    let list_data = member(
        rom,
        "data/prm/mon_list.gp2",
        &format!("mon_list_{LANG}.nat"),
    )?;
    let names_data = member(
        rom,
        "data/prm/mon_data.gp2",
        &format!("mon_data_{LANG}.nat"),
    )?;
    let battles = monster::BattleData::parse(file(rom, "data/prm/mon_btldata.nat")?)?;
    let actions = action::ActionNames::parse(file(rom, "data/prm/actname.nat")?)?;
    let list = monster::List::parse(&list_data)?;
    let names = monster::Names::parse(&names_data)?;

    let number = match args.target.parse::<u16>() {
        Ok(number) => number,
        Err(_) => {
            let sought = args.target.as_bytes();
            let found = list
                .find_code(sought)
                .or_else(|| list.find_name(sought))
                .map(|m| m.number)
                .or_else(|| names.find_code(sought).map(|m| m.number))
                .or_else(|| names.find_name(sought).map(|m| m.number));
            found.with_context(|| {
                format!(
                    "no monster {:?} by number, code or name in mon_list_{LANG} or mon_data_{LANG}",
                    args.target
                )
            })?
        }
    };
    let battle = battles
        .find(number)
        .with_context(|| format!("no battle data for monster {number}"))?;
    let record = names
        .find(number)
        .with_context(|| format!("no name record for monster {number}"))?;
    let listed = list.find_number(number);

    println!(
        "monster {number}: {} ({})",
        text(record.name),
        text(record.code)
    );
    if let Some(listed) = listed {
        println!(
            "  list: {} / {}; +0x0E = {} (not established)",
            text(listed.name),
            text(listed.code),
            listed.unknown
        );
    } else {
        println!("  list: not listed in mon_list_{LANG}");
    }
    println!(
        "  names: plural {}, level {}, family {}, metal {}, refuses a fall {}",
        text(record.plural),
        record.level,
        record.family,
        yes(record.metal),
        yes(record.refuses_fall)
    );
    println!(
        "  body: radius {}/1024 = {:.3}, height {}/4096 = {:.3}, battle size {}/4096 = {:.3}, kind {:#x}",
        record.radius,
        f64::from(record.radius) / 1024.0,
        record.height,
        f64::from(record.height) / 4096.0,
        record.battle_size,
        f64::from(record.battle_size) / 4096.0,
        record.kind
    );
    println!(
        "  battle: HP {} MP {} attack {} defence {} agility {}",
        battle.hp, battle.mp, battle.attack, battle.defence, battle.agility
    );
    println!(
        "  rewards: {} experience, {} gold",
        battle.experience, battle.gold
    );
    println!(
        "  drops: {} ({}), {} ({})",
        battle.drops[0],
        drop_chance(battle.drop_steps[0]),
        battle.drops[1],
        drop_chance(battle.drop_steps[1])
    );
    let ways: Vec<String> = battle
        .ways
        .iter()
        .map(|&way| match actions.name(way) {
            Some(name) if !name.is_empty() => format!("{way} {}", text(name)),
            _ => way.to_string(),
        })
        .collect();
    println!(
        "  ways (selector {}): {}",
        battle.selector(),
        ways.join(", ")
    );
    println!(
        "  evasion grade {} and block grade {} (table 0, 2, 4, 8, 25); once-a-battle mask {:#04x}; enrage word {:#010x}; Knight Watch {} to {}; boss {} (the bit is read by nothing)",
        battle.evasion_grade(),
        battle.block_grade(),
        battle.once_mask(),
        battle.enrage,
        battle.knight_watch[0],
        battle.knight_watch[1],
        yes(battle.boss())
    );
    let resistances: Vec<String> = (1..=22)
        .map(|element| format!("{element}:{}", battle.resistance(element).unwrap_or(0)))
        .collect();
    println!(
        "  resistance by element (hundredths): {}",
        resistances.join(" ")
    );
    println!(
        "  number flag {} (bit 15 of +0x00)",
        yes(battle.number_flag)
    );
    Ok(())
}

pub fn run_action(rom: &Rom, number: u16) -> Result<()> {
    let a_data = member(rom, "data/prm/actdt_a.gp2", &format!("actdt_a_{LANG}.nat"))?;
    let b_data = member(rom, "data/prm/actdt_b.gp2", &format!("actdt_b_{LANG}.nat"))?;
    let a_ranges = action::Ranges::parse(&member(rom, "data/prm/actdt_a.gp2", "actdamage_a.nat")?)?;
    let b_ranges = action::Ranges::parse(&member(rom, "data/prm/actdt_b.gp2", "actdamage_b.nat")?)?;
    let names = action::ActionNames::parse(file(rom, "data/prm/actname.nat")?)?;
    let a = action::Actions::parse(&a_data)?;
    let b = action::Actions::parse(&b_data)?;
    let (half, found, ranges) = if let Some(found) = a.find(number) {
        ("a", found, &a_ranges)
    } else if let Some(found) = b.find(number) {
        ("b", found, &b_ranges)
    } else {
        bail!("no action {number} in actdt_a or actdt_b");
    };

    let plural = if found.plural.is_empty() {
        String::new()
    } else {
        format!(" ({})", text(found.plural))
    };
    println!(
        "action {number}: {}{plural} — actdt_{half}",
        text(found.name)
    );
    if let Some(name) = names.name(number)
        && name != found.name
    {
        println!("  actname: {} (differs from the record)", text(name));
    }
    println!(
        "  MP {}; element {}; reach {}; kind {}; rider {}; damage handler {}; resisted by element {}",
        found.mp_cost,
        found.element,
        found.reach,
        found.kind,
        found.rider,
        found.damage_handler,
        found.resist_element
    );
    match ranges.find(found.range) {
        Some(range) => println!(
            "  range {}: ±{}; monster {}; party {} to {}",
            range.index, range.spread, range.monster, range.party_least, range.party_most
        ),
        None => println!("  range {}: none", found.range),
    }
    println!(
        "  accuracy: monster {}%, party {}% to {}%; critical multiplier {}%; metal {}; always critical {}",
        found.monster_chance,
        found.party_accuracy_low,
        found.party_accuracy_high,
        found.critical_multiplier,
        yes(found.metal),
        yes(found.always_critical)
    );
    let mut flags = Vec::new();
    for (set, name) in [
        (found.spoilt, "spoilt by a status"),
        (found.halved_by_defending, "halved by defending"),
        (found.dodgeable, "dodgeable"),
        (found.blockable, "blockable"),
        (found.wakes, "wakes its target"),
        (found.tension, "tension works and is spent"),
        (found.scales_magical_might, "scales by magical might"),
        (found.scales_magical_mending, "scales by magical mending"),
        (found.party_trait, "party trait die of four"),
        (found.weakens, "weakens as it strikes several"),
        (found.weapon_element, "elemental weapon product"),
        (found.works_on_metal, "works on a metal body"),
    ] {
        if set {
            flags.push(name);
        }
    }
    println!("  flags: {}", flags.join(", "));
    println!(
        "  scaling {} to {}; amount at most {}; combo {}; stat {} / rider {}",
        found.scaled_low,
        found.scaled_high,
        found.max_amount,
        yes(found.combo),
        found.stat_change,
        found.stat_change_rider
    );
    println!(
        "  messages: opening {}, result {}; hit {}, failed at party {}, failed at monster {}; effect {:#04x}",
        found.opening_message,
        found.result_message,
        found.hit_message(),
        found.failed_party_message(),
        found.failed_monster_message(),
        found.effect
    );
    Ok(())
}

pub fn run_encounters(rom: &Rom, target: &str) -> Result<()> {
    let field = encounter::Field::parse(file(rom, "data/prm/encfld.bin")?)?;
    let battles = encounter::Battle::parse(file(rom, "data/prm/encbtl.bin")?)?;
    let field_monsters = encounter::FieldMonsters::parse(file(rom, "data/prm/fld_mondata.bin")?)?;
    let names_data = member(
        rom,
        "data/prm/mon_data.gp2",
        &format!("mon_data_{LANG}.nat"),
    )?;
    let names = monster::Names::parse(&names_data)?;
    let map_table = Table::parse(file(rom, maplist::PATH)?)?;
    let maps = MapList::parse(&map_table)?;

    let record = match target.parse::<u16>() {
        Ok(id) => maps.find(id),
        Err(_) => maps
            .records
            .iter()
            .find(|r| r.code.eq_ignore_ascii_case(target.as_bytes())),
    }
    .with_context(|| format!("no map {target:?} in {}", maplist::PATH))?;
    let code = text(record.code);
    println!("encounters of {} ({}):", code, record.id);

    let Some(map) = field.find(record.id) else {
        println!("  no encounter group: this map has no roaming monsters");
        return Ok(());
    };
    for zone in &map.zones {
        println!(
            "  zone {} (kind {}, word {:#010x})",
            zone.id,
            zone.kind(),
            zone.word
        );
        for roamer in &zone.roamers {
            let second = match roamer.second {
                dq9_formats::Value::Int(v) => v.to_string(),
                dq9_formats::Value::Float(v) => format!("{v}"),
                dq9_formats::Value::Str(_) => "string".to_owned(),
            };
            let field_data = field_monsters
                .find(roamer.monster)
                .map_or_else(String::new, |m| {
                    format!(
                        ", level {}, run margin {}, speed {:.2}, attack {}, defence {}",
                        m.level, m.run_margin, m.speed, m.attack, m.defence
                    )
                });
            println!(
                "    roams: {} {} weight {} (second {}{})",
                roamer.monster,
                monster_name(&names, roamer.monster),
                roamer.weight,
                second,
                field_data
            );
        }
        if let Some(zone) = battles.find(zone.id) {
            for companion in &zone.company {
                println!(
                    "    company: {} {} weight {} joins {} to {}",
                    companion.monster,
                    monster_name(&names, companion.monster),
                    companion.weight,
                    companion.least,
                    companion.most
                );
            }
        }
    }
    Ok(())
}
