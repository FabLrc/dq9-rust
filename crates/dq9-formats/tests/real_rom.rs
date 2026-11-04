//! Format invariants on the real game data; skipped when the ROM is absent.

use std::collections::HashSet;

use dq9_formats::dialogue::{self, Kind, Session, TalkState};
use dq9_formats::trigger::{self, World};
use dq9_formats::{Font, FontId, Gpc2, Table, Value, level5};
use nds_rom::Rom;

/// Calls `f(archive path, member name, decompressed bytes)` for every GPC2 member.
fn for_each_member(rom: &Rom, mut f: impl FnMut(&str, &str, &[u8])) {
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let archive = Gpc2::parse(rom.file(file.id).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", file.path));
        for entry in &archive.entries {
            let content = archive
                .member(entry)
                .unwrap_or_else(|e| panic!("{}/{}: {e}", file.path, entry.name));
            f(&file.path, &entry.name, &content);
        }
    }
}

/// Every `.gp2` is a GPC2 archive whose index hashes match its names (checked by `parse`) and
/// whose members all decompress to their declared size.
#[test]
fn all_gp2_archives_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut archives = 0;
    let mut members = 0;
    for file in rom.files.files.iter().filter(|f| f.path.ends_with(".gp2")) {
        let archive = Gpc2::parse(rom.file(file.id).unwrap()).unwrap();
        for entry in &archive.entries {
            let content = archive.member(entry).unwrap();
            if let (Ok((_, size)), std::borrow::Cow::Owned(_)) =
                (level5::header(archive.stored(entry)), &content)
            {
                assert_eq!(content.len(), size, "{}/{}", file.path, entry.name);
            }
            members += 1;
        }
        archives += 1;
    }
    assert_eq!(archives, 1671);
    assert_eq!(members, 53_639);
}

/// Record tables: every string offset starts a string (checked by `parse`), the header's
/// string count equals the number of distinct strings referenced, and the known population
/// parses.
#[test]
fn record_tables_parse() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let mut tables = 0;
    for_each_member(&rom, |archive, name, data| {
        if !(name.ends_with(".bin") || name.ends_with(".bmmp") || name.ends_with(".bcfg")) {
            return;
        }
        let Ok(table) = Table::parse(data) else {
            return;
        };
        let strings: HashSet<*const u8> = table
            .records
            .iter()
            .flat_map(|r| &r.values)
            .filter_map(|v| match v {
                Value::Str(Some(s)) => Some(s.as_ptr()),
                _ => None,
            })
            .collect();
        assert_eq!(
            strings.len(),
            table.string_count as usize,
            "{archive}/{name}"
        );
        tables += 1;
    });
    // 47 075 `.bin` + 283 `.bmmp` + 28 `.bcfg`; the other 152 `.bin` use other formats.
    assert_eq!(tables, 47_386);
}

/// NPC dialogue files: every record has a handled kind, a text, a quest id and condition in
/// the ranges the game tests, and a slot in groups 0-2.
#[test]
fn dialogue_files_decode() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut files, mut lines) = (0, 0);
    let mut kinds = HashSet::new();
    for_each_member(&rom, |archive, name, data| {
        let is_dialogue = archive.starts_with("data/scenario/")
            && name.len() == 10
            && name[..3].bytes().all(|b| b.is_ascii_digit())
            && name.ends_with(".bin");
        if !is_dialogue {
            return;
        }
        files += 1;
        let table = Table::parse(data).unwrap();
        for line in dialogue::parse(&table).unwrap_or_else(|e| panic!("{archive}/{name}: {e}")) {
            lines += 1;
            let kind = line
                .kind
                .unwrap_or_else(|| panic!("{archive}/{name}: unknown kind"));
            kinds.insert(format!("{kind:?}"));
            assert!(line.text.is_some(), "{archive}/{name}");
            assert!(
                (0..240).contains(&line.slot),
                "{archive}/{name}: slot {}",
                line.slot
            );
            if matches!(kind, Kind::Quest | Kind::HostQuest) {
                assert!(
                    (0..0xcc).contains(&line.a),
                    "{archive}/{name}: quest {}",
                    line.a
                );
                assert!(
                    (-1..=4).contains(&line.b),
                    "{archive}/{name}: state {}",
                    line.b
                );
            }
        }
    });
    assert_eq!(files, 42_305);
    assert_eq!(lines, 189_940);
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort();
    assert_eq!(kinds, ["HostStep", "Quest", "SessionStepB", "Step"]);
}

/// George (Stornway, chapter M, NPC 18) once quest 11 is accepted: the in-progress line.
#[test]
fn dialogue_selection_follows_quest_state() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let archive = Gpc2::parse(rom.file_by_path("data/scenario/M02M0.gp2").unwrap()).unwrap();
    let data = archive.member(archive.find("018_en.bin").unwrap()).unwrap();
    let table = Table::parse(&data).unwrap();
    let lines = dialogue::parse(&table).unwrap();
    let quest = |state: u8| move |q: i32| if q == 11 { state } else { 0 };
    let accepted = quest(2);
    let mut talk = TalkState {
        step: 1,
        substep: 0,
        context: 0,
        talk_count: [0, 0],
        daytime: true,
        session: Session::default(),
        quest: &accepted,
    };
    let chosen = dialogue::select(&lines, &talk).unwrap();
    let text = lines[chosen].text.unwrap();
    assert!(text.starts_with(b"<QUEST=11>//George// Those betterflies"));
    // Quest done (state 3): the closing line.
    let done = quest(3);
    talk.quest = &done;
    let chosen = dialogue::select(&lines, &talk).unwrap();
    assert!(
        lines[chosen]
            .text
            .unwrap()
            .starts_with(b"//George// I suppose those monsters")
    );
}

/// Every scenario trigger file compiles: each word's operands fit in its record.
#[test]
fn trigger_files_compile() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let (mut files, mut triggers) = (0, 0);
    for file in rom
        .files
        .files
        .iter()
        .filter(|f| f.path.starts_with("data/scenario/trigger") && f.path.ends_with(".bin"))
    {
        let table = Table::parse(rom.file(file.id).unwrap()).unwrap();
        let parsed = trigger::parse(&table).unwrap_or_else(|e| panic!("{}: {e}", file.path));
        for t in &parsed {
            if let Some((_, context)) = t.talk() {
                assert!(
                    (0..240).contains(&context),
                    "{}: context {context}",
                    file.path
                );
            }
        }
        files += 1;
        triggers += parsed.len();
    }
    assert_eq!(files, 76);
    assert_eq!(triggers, 5_895);
}

/// George's quest offer goes through a talk trigger: with quest 11 available (state 1), it
/// forces context 202 (offer) from level 25, 203 ("if only someone strong enough…") below.
#[test]
fn talk_triggers_force_the_dialogue_context() {
    struct George {
        level: i32,
    }
    impl World for George {
        fn flag(&self, _: i32) -> bool {
            false
        }
        fn player_flag(&self, _: u8, _: i32) -> bool {
            false
        }
        fn quest(&self, q: i32) -> u8 {
            u8::from(q == 11)
        }
        fn daytime(&self) -> bool {
            true
        }
        fn session(&self) -> Session {
            Session {
                hero_flag: true,
                ..Session::default()
            }
        }
        fn substep(&self) -> i32 {
            0
        }
        fn event(&self, i: usize) -> u32 {
            if i == 0 { 18 } else { 0 }
        }
        fn item_count(&self, _: i32) -> i32 {
            0
        }
        fn max_level(&self) -> i32 {
            self.level
        }
        fn quest_log_len(&self) -> u8 {
            0
        }
        fn quest_log_flag(&self, _: i32) -> bool {
            false
        }
        fn quest_known(&self, _: i32) -> bool {
            false
        }
    }
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let table = Table::parse(rom.file_by_path("data/scenario/triggerM02.bin").unwrap()).unwrap();
    // Chapter 15 = letter M, room 1200 (George's room in M02place.bin).
    let triggers: Vec<_> = trigger::parse(&table)
        .unwrap()
        .into_iter()
        .filter(|t| t.loaded(1200, 15, 1))
        .collect();
    let archive = Gpc2::parse(rom.file_by_path("data/scenario/M02M0.gp2").unwrap()).unwrap();
    let data = archive.member(archive.find("018_en.bin").unwrap()).unwrap();
    let table = Table::parse(&data).unwrap();
    let lines = dialogue::parse(&table).unwrap();
    let available = |q: i32| u8::from(q == 11);
    for (level, context, start) in [
        (25, 202, &b"*: Just lately I<1>ve had real trouble"[..]),
        (5, 203, &b"*: There<1>ve been so many monsters"[..]),
    ] {
        let world = George { level };
        let t = trigger::first_match(&triggers, 0, &world).unwrap().unwrap();
        assert_eq!(t.talk(), Some((18, context)));
        let talk = TalkState {
            step: 1,
            substep: 0,
            context,
            talk_count: [0, 0],
            daytime: true,
            session: world.session(),
            quest: &available,
        };
        let chosen = dialogue::select(&lines, &talk).unwrap();
        assert!(
            lines[chosen].text.unwrap().starts_with(start),
            "level {level}"
        );
    }
}

/// Both fonts parse, and every accented-letter tag used in the European texts (`<'e>`,
/// `<^a>`, `<:O>`, `<,c>`, `<~n>`…) is a glyph of the message font.
#[test]
fn fonts_cover_accent_tags() {
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let font = |id: FontId| {
        let info = rom
            .file_by_path(&format!("data/pack_lv5/fi_{}.bin", id.name()))
            .unwrap();
        let data = rom
            .file_by_path(&format!("data/pack_lv5/fd_{}.bin", id.name()))
            .unwrap();
        Font::parse(info, data, id).unwrap()
    };
    assert_eq!(font(FontId::Small).glyphs.len(), 245);
    let message = font(FontId::Message);
    assert_eq!(message.glyphs.len(), 242);

    let mut tags = HashSet::new();
    for_each_member(&rom, |_, name, data| {
        let Ok(table) = Table::parse(data) else {
            return;
        };
        if !name.ends_with(".bin") {
            return;
        }
        for value in table.records.iter().flat_map(|r| &r.values) {
            let Value::Str(Some(s)) = value else { continue };
            for w in s.windows(4) {
                if w[0] == b'<'
                    && b"`'^:~,".contains(&w[1])
                    && w[2].is_ascii_alphabetic()
                    && w[3] == b'>'
                {
                    tags.insert(w.to_vec());
                }
            }
        }
    });
    assert!(tags.len() > 40, "only {} accent tags found", tags.len());
    for tag in &tags {
        let index = message
            .find(tag)
            .unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(tag)));
        assert_eq!(message.glyphs[usize::from(index)].text, tag.as_slice());
    }
}

/// Every dialogue/menu string of the five languages goes through the whole message pipeline;
/// afterwards, text bytes are glyphs, spaces, or tags the pipeline leaves to other systems.
#[test]
fn message_pipeline_leaves_only_glyphs() {
    use dq9_formats::message::{self, Name, Token};
    let Some(bytes) = testrom::load() else { return };
    let rom = Rom::parse(bytes).unwrap();
    let info = rom.file_by_path("data/pack_lv5/fi_me.bin").unwrap();
    let data = rom.file_by_path("data/pack_lv5/fd_me.bin").unwrap();
    let font = Font::parse(info, data, FontId::Message).unwrap();
    let mut leftovers = std::collections::BTreeMap::<String, usize>::new();
    let mut messages = 0;
    for_each_member(&rom, |_, name, data| {
        if !name.ends_with(".bin") {
            return;
        }
        let Ok(table) = Table::parse(data) else {
            return;
        };
        for value in table.records.iter().flat_map(|r| &r.values) {
            let Value::Str(Some(s)) = value else { continue };
            let text = message::expand_values(s, |_| b"1".to_vec());
            let text = message::normalize(&font, &text);
            let text = message::eval_conditionals(&text, |_| 0);
            let text = message::expand_names(&text, |n| match n {
                Name::Substitution { .. } | Name::Inflected { .. } => b"Name".to_vec(),
                Name::Weapon | Name::Actor => b"Name".to_vec(),
            });
            let text = message::apply_cap(&font, &text);
            for token in message::compile(&text) {
                let Token::Text(bytes) = token else { continue };
                let mut rest = bytes.as_slice();
                while !rest.is_empty() {
                    if let Some(i) = font.find(rest) {
                        rest = &rest[font.glyphs[usize::from(i)].text.len()..];
                    } else if rest[0] == b' ' {
                        rest = &rest[1..];
                    } else {
                        let end = message::find_tag_end(rest).filter(|_| rest[0] == b'<');
                        let len = end.map_or(1, |e| e + 1);
                        let key: String = String::from_utf8_lossy(&rest[..len])
                            .chars()
                            .map(|c| if c.is_ascii_digit() { '#' } else { c })
                            .collect();
                        *leftovers.entry(key).or_default() += 1;
                        rest = &rest[len..];
                    }
                }
            }
            messages += 1;
        }
    });
    assert_eq!(messages, 272_484);
    // Tags left for other systems (see docs/re/text.md), plus two typos in the game data
    // (`<ADDRESSE>`, `<END_IF_MALE>` with its block) and `<END>`/`<END_R_TURN>` right after a
    // tag `Msg_Compile` drops (it then copies the next '<' as text, like the game).
    let expected = [
        "<ADDRESSE>",
        "<CEN>",
        "<CURSOR=#>",
        "<DAMA>",
        "<ELSE_NOT_MALE>",
        "<END>",
        "<END_IF_MALE>",
        "<END_R_TURN>",
        "<IF_MALE>",
        "<LG>",
        "<LUIDA>",
        "<PLTT=##>",
        "<PLTT=#>",
        "<RENKIN>",
        "<RIKKA>",
        "<TC##>",
        "<TC#>",
        "<TRI-L>",
        "<TRI-R>",
        "<W=###>",
        "<W=##>",
    ];
    assert_eq!(
        leftovers.keys().map(String::as_str).collect::<Vec<_>>(),
        expected
    );
}
