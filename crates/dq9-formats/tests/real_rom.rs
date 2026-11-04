//! Format invariants on the real game data; skipped when the ROM is absent.

use std::collections::HashSet;

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
