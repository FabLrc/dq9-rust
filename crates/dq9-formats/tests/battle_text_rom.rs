//! Battle text on the real game data; skipped when the ROM is absent.
//!
//! The messages, their counts and the article numbers are the ROM's own; the expected lines
//! are what the message window draws once the grammar tags are resolved. Layouts and the
//! action-message mapping: `docs/re/battle-text.md`.

use dq9_formats::battle::{action, monster, text};
use dq9_formats::message::NameRecord;
use dq9_formats::{Font, FontId, Gpc2};
use nds_rom::Rom;
use std::borrow::Cow;

fn file<'a>(rom: &Rom<'a>, path: &str) -> &'a [u8] {
    rom.file_by_path(path)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn member<'a>(rom: &Rom<'a>, path: &str, name: &str) -> Cow<'a, [u8]> {
    let bytes = file(rom, path);
    let gpc2 = Gpc2::parse(bytes).unwrap_or_else(|error| panic!("{path}: {error}"));
    let entry = gpc2
        .find(name)
        .unwrap_or_else(|| panic!("{name} in {path}"));
    gpc2.member(entry)
        .unwrap_or_else(|error| panic!("{name} of {path}: {error}"))
}

/// The tables every test needs, borrowing the decompressed members of `with_battle`.
struct BattleRefs<'a> {
    builder: text::BattleText<'a>,
    monsters: monster::Names<'a>,
    items: text::ItemNames<'a>,
    actions_a: action::Actions<'a>,
    actions_b: action::Actions<'a>,
}

/// Builds one language's battle text for `f`; the decompressed members live for the call.
fn with_battle<R>(rom: &Rom, lang: text::Language, f: impl FnOnce(&BattleRefs<'_>) -> R) -> R {
    let suffix = lang.suffix();
    let font_name = FontId::Message.name();
    let font_info = file(rom, &format!("data/pack_lv5/fi_{font_name}.bin"));
    let font_data = file(rom, &format!("data/pack_lv5/fd_{font_name}.bin"));
    let font = Font::parse(font_info, font_data, FontId::Message).expect("message font");

    let strbtl = member(rom, "data/bin/strbtl.gp2", &format!("strbtl_{suffix}.nat"));
    let actmsg = member(rom, "data/prm/actmsg.gp2", &format!("actmsg_{suffix}.nat"));
    let str_bres = member(
        rom,
        "data/bin/str_bres.gp2",
        &format!("str_bres_{suffix}.bin"),
    );
    let str_btl = member(
        rom,
        "data/bin/menu/str_btl.gp2",
        &format!("str_btl_{suffix}.nat"),
    );
    let str_tm = member(
        rom,
        "data/bin/menu/str_tm.gp2",
        &format!("str_tm_{suffix}.nat"),
    );
    let article = member(
        rom,
        "data/prm/article.gp2",
        &format!("article_{suffix}.nat"),
    );
    let mon_list = member(
        rom,
        "data/prm/mon_list.gp2",
        &format!("mon_list_{suffix}.nat"),
    );
    let mon_data = member(
        rom,
        "data/prm/mon_data.gp2",
        &format!("mon_data_{suffix}.nat"),
    );
    let itemname = member(
        rom,
        "data/prm/itemname.gp2",
        &format!("itemname_{suffix}.nat"),
    );
    let a_data = member(
        rom,
        "data/prm/actdt_a.gp2",
        &format!("actdt_a_{suffix}.nat"),
    );
    let b_data = member(
        rom,
        "data/prm/actdt_b.gp2",
        &format!("actdt_b_{suffix}.nat"),
    );

    let files = text::BattleFiles {
        strbtl: text::MessageTable::parse(&strbtl).expect("strbtl"),
        actmsg: text::MessageTable::parse(&actmsg).expect("actmsg"),
        str_bres: text::BattleResult::parse(&str_bres).expect("str_bres"),
        str_btl: text::MessageTable::parse(&str_btl).expect("str_btl"),
        str_tm: text::MessageTable::parse(&str_tm).expect("str_tm"),
        articles: text::MessageTable::parse(&article).expect("article"),
    };
    let declension = (lang == text::Language::German).then(|| {
        let arm9 = rom.arm9().expect("ARM9");
        dq9_formats::message::Declension::from_arm9(&arm9.data, arm9.load_addr).expect("declension")
    });
    let refs = BattleRefs {
        builder: text::BattleText::new(lang, font, files, declension),
        monsters: monster::Names::parse(&mon_data).expect("mon_data"),
        items: text::ItemNames::parse(&itemname).expect("itemname"),
        actions_a: action::Actions::parse(&a_data).expect("actdt_a"),
        actions_b: action::Actions::parse(&b_data).expect("actdt_b"),
    };
    let _ = mon_list;
    f(&refs)
}

/// The party leader, as the CLI names it.
const HERO: NameRecord<'static> = NameRecord {
    singular: b"Hero",
    plural: b"Hero",
    grammar: 1 << 12,
};

fn monster_record<'a>(refs: &BattleRefs<'a>, number: u16) -> NameRecord<'a> {
    text::monster_name(refs.monsters.find(number).expect("monster name record"))
}

fn context_with<'a>(
    actor: Option<NameRecord<'a>>,
    target: Option<NameRecord<'a>>,
) -> text::Context<'a> {
    text::Context {
        actor,
        target,
        monster: target,
        ..text::Context::default()
    }
}

#[test]
fn the_message_files_carry_the_expected_messages() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::English, |refs| {
        let builder = &refs.builder;
        assert_eq!(builder.files.strbtl.len(), 17);
        assert_eq!(builder.files.actmsg.len(), 591);
        assert_eq!(builder.files.str_bres.len(), 44);
        assert_eq!(builder.files.str_btl.len(), 106);
        assert_eq!(builder.files.str_tm.len(), 329);
        assert_eq!(builder.files.articles.len(), 38);
        assert_eq!(refs.items.len(), 1178);
        for numbers in [
            builder.files.strbtl.numbers().collect::<Vec<_>>(),
            builder.files.actmsg.numbers().collect::<Vec<_>>(),
            builder.files.str_bres.numbers().collect::<Vec<_>>(),
            builder.files.str_btl.numbers().collect::<Vec<_>>(),
            builder.files.str_tm.numbers().collect::<Vec<_>>(),
        ] {
            let unique: std::collections::HashSet<_> = numbers.iter().copied().collect();
            assert_eq!(unique.len(), numbers.len(), "message numbers are unique");
        }
        // The names the chapter 2 messages use are where the inflection expects them.
        assert_eq!(monster_record(refs, 1).singular, b"slime");
        assert_eq!(monster_record(refs, 1).plural, b"slimes");
    });
}

#[test]
fn a_slime_draws_near_alone_and_in_a_group() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::English, |refs| {
        let slime = monster_record(refs, 1);
        let mut context = context_with(Some(slime), Some(slime));
        context.values = vec![1];
        assert_eq!(
            refs.builder
                .message(text::File::Strbtl, 6, &context)
                .unwrap(),
            b"A slime draws near!"
        );
        context.values = vec![2];
        assert_eq!(
            refs.builder
                .message(text::File::Strbtl, 6, &context)
                .unwrap(),
            b"Some slimes draw near!"
        );
    });
}

#[test]
fn attack_and_damage_with_the_hero_and_a_slime() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::English, |refs| {
        let slime = monster_record(refs, 1);
        let slime_context = context_with(Some(slime), Some(slime));
        let hero_context = context_with(Some(HERO), Some(HERO));
        let mut damage = context_with(Some(slime), Some(slime));
        damage.values = vec![5];
        let draw = |file, number, context: &text::Context<'_>| {
            String::from_utf8_lossy(&refs.builder.message(file, number, context).unwrap())
                .into_owned()
        };
        assert_eq!(
            draw(text::File::Actmsg, 1, &slime_context),
            "The slime attacks."
        );
        assert_eq!(
            draw(text::File::Actmsg, 2, &damage),
            "The slime takes 5 points of damage."
        );
        damage.values = vec![1];
        assert_eq!(
            draw(text::File::Actmsg, 2, &damage),
            "The slime takes 1 point of damage."
        );
        assert_eq!(
            draw(text::File::Actmsg, 1, &hero_context),
            "The Hero attacks."
        );
        damage.target = Some(HERO);
        damage.monster = Some(HERO);
        damage.values = vec![5];
        assert_eq!(
            draw(text::File::Actmsg, 2, &damage),
            "The Hero takes 5 points of damage."
        );
    });
}

#[test]
fn a_critical_has_its_own_line() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::English, |refs| {
        let slime = monster_record(refs, 1);
        let context = context_with(Some(slime), Some(slime));
        assert_eq!(
            refs.builder
                .message(text::File::Actmsg, 140, &context)
                .unwrap(),
            b"Critical hit!"
        );
    });
}

#[test]
fn a_level_up_and_the_experience_lines_are_drawn() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::English, |refs| {
        let slime = monster_record(refs, 1);
        let mut context = context_with(Some(slime), Some(slime));
        context.values = vec![2];
        assert_eq!(
            refs.builder
                .message(text::File::StrBres, 10, &context)
                .unwrap(),
            b"The slime<1>s level increases to\nLv. 2!"
        );
        context.strings = vec![b"Hero".to_vec()];
        context.values = vec![3];
        assert_eq!(
            refs.builder
                .message(text::File::StrBres, 6, &context)
                .unwrap(),
            b"Hero earns 3 experience!"
        );
        context.values = vec![1];
        assert_eq!(
            refs.builder
                .message(text::File::StrBres, 13, &context)
                .unwrap(),
            b"1 skill point earned."
        );
        context.values = vec![2];
        assert_eq!(
            refs.builder
                .message(text::File::StrBres, 13, &context)
                .unwrap(),
            b"2 skill points earned."
        );
    });
}

#[test]
fn an_item_message_names_its_item() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::English, |refs| {
        let slime = monster_record(refs, 1);
        let herb = refs.items.find(22000).expect("medicinal herb");
        let mut context = context_with(Some(slime), Some(slime));
        context.item = Some(herb.name_record());
        assert_eq!(
            refs.builder
                .message(text::File::Actmsg, 12, &context)
                .unwrap(),
            b"The slime uses a medicinal herb."
        );
    });
}

#[test]
fn french_articles_and_gender_agree_with_the_rom() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::French, |refs| {
        let slime = monster_record(refs, 1);
        assert_eq!(slime.singular, b"gluant");
        let mut context = context_with(Some(slime), Some(slime));
        context.values = vec![5];
        assert_eq!(
            refs.builder
                .message(text::File::Actmsg, 2, &context)
                .unwrap(),
            b"Le gluant subit 5 points de d<'e>g<^a>ts."
        );
    });
}

#[test]
fn german_case_templates_expand() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::German, |refs| {
        let slime = monster_record(refs, 1);
        assert_eq!(slime.singular, b"Schleim[gs]");
        let mut context = context_with(Some(slime), Some(slime));
        context.values = vec![5];
        assert_eq!(
            refs.builder
                .message(text::File::Actmsg, 2, &context)
                .unwrap(),
            b"Der Schleim nimmt 5 Punkte Schaden."
        );
    });
}

/// The chapter 2 actions name their messages in `+0x20`: bits 10–19 the opening, 20–31 the
/// result. Checked on the plain attack, a spell, Heal and the medicinal herb: the lines are
/// the action's own on all four.
#[test]
fn actions_point_at_the_messages_they_say() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    with_battle(&rom, text::Language::English, |refs| {
        let action = |number: u16| {
            refs.actions_a
                .find(number)
                .or_else(|| refs.actions_b.find(number))
                .unwrap_or_else(|| panic!("action {number}"))
        };
        for (number, opening, result) in [
            (1, 1, 2),
            (10, 46, 2),
            (30, 46, 22),
            (74, 179, 2),
            (255, 70, 22),
        ] {
            let found = action(number);
            assert_eq!(
                (found.opening_message, found.result_message),
                (opening, result),
                "action {number}"
            );
        }
        let line = |number: u16| {
            String::from_utf8_lossy(refs.builder.files.actmsg.message(number).unwrap()).into_owned()
        };
        assert!(line(1).contains("attacks."));
        assert!(line(2).contains("takes <val_1>"));
        assert!(line(46).contains("casts <ACTION>."));
        assert!(line(22).contains("wounds are healed."));
        assert!(line(70).contains("uses <INDEF_ART_SGL_I_NAME>."));
        assert!(line(179).contains("calls forth bolts from the blue!"));
    });
}
