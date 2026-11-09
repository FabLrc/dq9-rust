//! `dq9re battle-text`: one battle message through the message pipeline, with the names,
//! articles, plurals and numbers of the battle resolved (`docs/re/battle-text.md`).

use std::borrow::Cow;

use anyhow::{Context as _, Result};
use dq9_formats::battle::{action, monster, text};
use dq9_formats::message::{Declension, NameRecord};
use dq9_formats::{FontId, Gpc2};
use nds_rom::Rom;

#[derive(clap::Args)]
pub struct BattleTextArgs {
    /// Message file: `strbtl`, `actmsg`, `str_bres`, `str_btl` or `str_tm`.
    pub file: String,
    /// The message's number.
    pub number: u16,
    /// Language: `en`, `fr`, `de`, `it` or `es`.
    #[arg(long, default_value = "en")]
    pub lang: String,
    /// The actor: a monster's number, code or name, or `hero`.
    #[arg(long)]
    pub actor: Option<String>,
    /// The target: a monster's number, code or name, or `hero`.
    #[arg(long)]
    pub target: Option<String>,
    /// The item named by `<I_NAME>` tags: an item id or its name.
    #[arg(long)]
    pub item: Option<String>,
    /// The action named by `<ACTION>` tags.
    #[arg(long)]
    pub action: Option<u16>,
    /// `<val_n>` values, in order.
    #[arg(long = "val")]
    pub values: Vec<u32>,
    /// `<str_n>` strings, in order.
    #[arg(long = "str")]
    pub strings: Vec<String>,
    /// How many targets the message addresses (`IF_TARGET_SING`).
    #[arg(long, default_value_t = 1)]
    pub count: u16,
    /// The party is the leader alone (`IF_SOLO`).
    #[arg(long)]
    pub solo: bool,
}

/// The party leader, as the battle context names it: no article but the definite singular one,
/// male.
const HERO: NameRecord<'static> = NameRecord {
    singular: b"Hero",
    plural: b"Hero",
    grammar: 1 << 12,
};

fn member<'a>(rom: &'a Rom, path: &str, name: &str) -> Result<Cow<'a, [u8]>> {
    let bytes = rom
        .file_by_path(path)
        .with_context(|| format!("no {path}"))?;
    let gpc2 = Gpc2::parse(bytes).with_context(|| format!("parsing {path}"))?;
    let entry = gpc2
        .find(name)
        .with_context(|| format!("no {name} in {path}"))?;
    gpc2.member(entry)
        .with_context(|| format!("reading {name} of {path}"))
}

/// A monster or the hero, by number, code or name.
fn named<'a>(
    spec: &str,
    list: &monster::List<'a>,
    names: &monster::Names<'a>,
) -> Result<(Option<NameRecord<'a>>, bool)> {
    if spec.eq_ignore_ascii_case("hero") {
        return Ok((Some(HERO), true));
    }
    let number = match spec.parse::<u16>() {
        Ok(number) => number,
        Err(_) => {
            let sought = spec.as_bytes();
            list.find_code(sought)
                .or_else(|| list.find_name(sought))
                .map(|m| m.number)
                .or_else(|| names.find_code(sought).map(|m| m.number))
                .or_else(|| names.find_name(sought).map(|m| m.number))
                .with_context(|| format!("no monster {spec:?} by number, code or name"))?
        }
    };
    let record = names
        .find(number)
        .with_context(|| format!("no name record for monster {number}"))?;
    Ok((Some(text::monster_name(record)), false))
}

pub fn run(rom: &Rom, args: &BattleTextArgs) -> Result<()> {
    let lang = text::Language::from_suffix(&args.lang)
        .with_context(|| format!("unknown language {:?} (en, fr, de, it, es)", args.lang))?;
    let file = text::File::from_name(&args.file).with_context(|| {
        format!(
            "unknown message file {:?} ({})",
            args.file,
            text::File::ALL.map(text::File::name).join(", ")
        )
    })?;

    let font = crate::font::load(rom, FontId::Message)?;
    let suffix = lang.suffix();
    let data = |file: text::File| -> Result<Cow<'_, [u8]>> {
        member(rom, file.archive(), &file.member(lang))
    };
    let strbtl_data = data(text::File::Strbtl)?;
    let actmsg_data = data(text::File::Actmsg)?;
    let bres_data = data(text::File::StrBres)?;
    let str_btl_data = data(text::File::StrBtl)?;
    let str_tm_data = data(text::File::StrTm)?;
    let article_data = member(
        rom,
        "data/prm/article.gp2",
        &format!("article_{suffix}.nat"),
    )?;
    let files = text::BattleFiles {
        strbtl: text::MessageTable::parse(&strbtl_data)?,
        actmsg: text::MessageTable::parse(&actmsg_data)?,
        str_bres: text::BattleResult::parse(&bres_data)?,
        str_btl: text::MessageTable::parse(&str_btl_data)?,
        str_tm: text::MessageTable::parse(&str_tm_data)?,
        articles: text::MessageTable::parse(&article_data)?,
    };
    let declension = if lang == text::Language::German {
        let arm9 = rom.arm9()?;
        Some(Declension::from_arm9(&arm9.data, arm9.load_addr)?)
    } else {
        None
    };
    let builder = text::BattleText::new(lang, font, files, declension);

    // Names the message's grammar tags inflect.
    let mon_lists = member(
        rom,
        "data/prm/mon_list.gp2",
        &format!("mon_list_{suffix}.nat"),
    )?;
    let mon_names = member(
        rom,
        "data/prm/mon_data.gp2",
        &format!("mon_data_{suffix}.nat"),
    )?;
    let list = monster::List::parse(&mon_lists)?;
    let names = monster::Names::parse(&mon_names)?;
    let (actor, actor_party) = match &args.actor {
        Some(spec) => named(spec, &list, &names)?,
        None => (None, false),
    };
    let (target, target_party) = match &args.target {
        Some(spec) => named(spec, &list, &names)?,
        None => (None, false),
    };

    let item_data = match args.item {
        Some(_) => Some(member(
            rom,
            "data/prm/itemname.gp2",
            &format!("itemname_{suffix}.nat"),
        )?),
        None => None,
    };
    let item = match (&args.item, &item_data) {
        (Some(spec), Some(item_data)) => {
            let items = text::ItemNames::parse(item_data)?;
            let record = match spec.parse::<u16>() {
                Ok(id) => items.find(id),
                Err(_) => items.find_name(spec.as_bytes()),
            }
            .copied()
            .with_context(|| format!("no item {spec:?} by id or name"))?;
            Some(record.name_record())
        }
        _ => None,
    };

    let a_data = match args.action {
        Some(_) => Some(member(
            rom,
            "data/prm/actdt_a.gp2",
            &format!("actdt_a_{suffix}.nat"),
        )?),
        None => None,
    };
    let b_data = match args.action {
        Some(_) => Some(member(
            rom,
            "data/prm/actdt_b.gp2",
            &format!("actdt_b_{suffix}.nat"),
        )?),
        None => None,
    };
    let action_record = match (args.action, &a_data, &b_data) {
        (Some(number), Some(a_data), Some(b_data)) => {
            let a = action::Actions::parse(a_data)?;
            let b = action::Actions::parse(b_data)?;
            let found = a
                .find(number)
                .or_else(|| b.find(number))
                .with_context(|| format!("no action {number} in actdt_a or actdt_b"))?;
            Some(NameRecord {
                singular: found.name,
                plural: found.plural,
                grammar: 0,
            })
        }
        _ => None,
    };

    let monster = if !target_party {
        target.or(actor.filter(|_| !actor_party))
    } else {
        actor.filter(|_| !actor_party)
    };
    let context = text::Context {
        actor,
        target,
        monster,
        item,
        action: action_record,
        hero: HERO.singular.to_vec(),
        strings: args
            .strings
            .iter()
            .map(|s| s.clone().into_bytes())
            .collect(),
        values: args.values.clone(),
        solo: args.solo,
        actor_party,
        target_party,
        target_count: args.count,
    };
    let drawn = builder.message(file, args.number, &context)?;
    let text = String::from_utf8_lossy(&drawn);
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
    Ok(())
}
