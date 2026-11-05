//! Message preparation: the six text passes run by `Msg_Prepare` (`0x02067ea8`) before a
//! message is drawn. Each pass is a pure function here; game state (hero name, gender…) is
//! supplied by the caller through closures. Evidence in `docs/re/text.md`.
//!
//! Order: [`expand_values`] → [`normalize`] → [`eval_conditionals`] → [`expand_names`] →
//! [`apply_cap`] → [`compile`].
//!
//! port: 0x020424bc, 0x0206832c, 0x02068428, 0x02068f50, 0x02068e2c, 0x02069244
//! port: 0x02069604, 0x02067ffc, 0x02069ffc
//! port: 0x0206974c..=0x02069fd4
//!
//! The range is the control-tag handlers of `Msg_Compile`, reduced here to their codes.

use crate::font::Font;

/// End of the tag starting at `text[0] == b'<'`: index of the first `>` not followed by
/// another `>` (`Msg_FindTagEnd`, `0x020424bc`), so the glyph `<>>>` stays one tag.
pub fn find_tag_end(text: &[u8]) -> Option<usize> {
    (0..text.len()).find(|&i| text[i] == b'>' && text.get(i + 1) != Some(&b'>'))
}

/// `Some((tag, end))` when `text` starts with a complete tag: `tag` excludes the brackets,
/// `end` is the index of the closing `>`.
fn tag_at(text: &[u8]) -> Option<(&[u8], usize)> {
    if text.first() != Some(&b'<') {
        return None;
    }
    find_tag_end(text).map(|end| (&text[1..end], end))
}

/// Optional 1-based index written right after a tag name (`<STR_2>`): returns `n - 1`, or 0.
fn digit_index(rest: &[u8]) -> u32 {
    match rest.first() {
        Some(d @ b'1'..=b'9') => {
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            let n: u32 = std::str::from_utf8(&rest[..digits])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(u32::from(d - b'0'));
            n - 1
        }
        _ => 0,
    }
}

/// Pass 0, `Msg_ExpandValues` (`0x0206832c`): replaces `<val_n>` / `<VAL_n>` by
/// `value(n - 1)` (0 when no digit follows).
pub fn expand_values(text: &[u8], mut value: impl FnMut(u32) -> Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some((tag, end)) = tag_at(&text[i..])
            && let Some(rest) = tag
                .strip_prefix(b"val_")
                .or_else(|| tag.strip_prefix(b"VAL_"))
        {
            out.extend(value(digit_index(rest)));
            i += end + 1;
            continue;
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

/// Pass 1, `Msg_Normalize` (`0x02068428`): glyph strings are kept, `\n` (two characters)
/// kept, other complete tags are kept in ASCII upper case, `' '`, `'/'`, `'\n'` kept, and every
/// other byte becomes the space glyph `< >`.
pub fn normalize(font: &Font, text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with(b"\\n") {
            out.extend_from_slice(b"\\n");
            i += 2;
        } else if let Some(index) = font.find(rest) {
            let glyph = font.glyphs[usize::from(index)].text;
            out.extend_from_slice(glyph);
            i += glyph.len();
        } else if let Some((_, end)) = tag_at(rest) {
            out.extend(rest[..=end].iter().map(u8::to_ascii_uppercase));
            i += end + 1;
        } else {
            match rest[0] {
                b @ (b' ' | b'/' | b'\n') => out.push(b),
                _ => out.extend_from_slice(b"< >"),
            }
            i += 1;
        }
    }
    out
}

/// One conditional block of `g_MsgCondRules` (`0x020e834c`). Tag names without brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Condition {
    pub start: &'static str,
    /// Separator before branch 1 (`ELSE_…`, or the second `IF_…` of gender rules).
    pub middle: &'static str,
    /// Separator before branch 2 (gender rules only).
    pub middle2: Option<&'static str>,
    pub end: &'static str,
    /// ARM9 address of the `MsgCond_*` function that picks the branch.
    pub handler: u32,
}

/// The 56 rules in table order. A rule matches when the text starts with `<start>`
/// (case-insensitive), closing bracket included.
#[rustfmt::skip]
pub const CONDITIONS: [Condition; 56] = [
    Condition { start: "IF_SP", middle: "ELSE_NOT_SP", middle2: None, end: "ENDIF_SP", handler: 0x0206853c },
    Condition { start: "IF_HOST_MALE", middle: "ELSE_HOST_NOT_MALE", middle2: None, end: "ENDIF_HOST_MALE", handler: 0x02068568 },
    Condition { start: "IF_ADDRESSEE_MALE", middle: "ELSE_ADDRESSEE_NOT_MALE", middle2: None, end: "ENDIF_ADDRESSEE_MALE", handler: 0x02068590 },
    Condition { start: "IF_HERO_MALE", middle: "ELSE_HERO_NOT_MALE", middle2: None, end: "ENDIF_HERO_MALE", handler: 0x020685b4 },
    Condition { start: "IF_MALE", middle: "ELSE_NOT_MALE", middle2: None, end: "ENDIF_MALE", handler: 0x020685dc },
    Condition { start: "IF_FEMALE_PARTY", middle: "ELSE_NOT_FEMALE_PARTY", middle2: None, end: "ENDIF_FEMALE_PARTY", handler: 0x02068600 },
    Condition { start: "IF_SOLO", middle: "ELSE_NOT_SOLO", middle2: None, end: "ENDIF_SOLO", handler: 0x02068660 },
    Condition { start: "IF_SOLOHOST", middle: "ELSE_NOT_SOLOHOST", middle2: None, end: "ENDIF_SOLOHOST", handler: 0x0206868c },
    Condition { start: "IF_ALONE", middle: "ELSE_NOT_ALONE", middle2: None, end: "ENDIF_ALONE", handler: 0x020686f0 },
    Condition { start: "IF_SING VAL_1", middle: "ELSE_NOT_SING", middle2: None, end: "ENDIF_SING", handler: 0x02068778 },
    Condition { start: "IF_SING VAL_2", middle: "ELSE_NOT_SING", middle2: None, end: "ENDIF_SING", handler: 0x0206878c },
    Condition { start: "IF_SING VAL_3", middle: "ELSE_NOT_SING", middle2: None, end: "ENDIF_SING", handler: 0x020687a0 },
    Condition { start: "IF_SING VAL_4", middle: "ELSE_NOT_SING", middle2: None, end: "ENDIF_SING", handler: 0x020687b4 },
    Condition { start: "IF_SING VAL_5", middle: "ELSE_NOT_SING", middle2: None, end: "ENDIF_SING", handler: 0x020687c8 },
    Condition { start: "IF_I_NAME_PLRNOUN", middle: "ELSE_NOT_PLRNOUN", middle2: None, end: "ENDIF_PLRNOUN", handler: 0x020687dc },
    Condition { start: "IF_I_NAME1_PLRNOUN", middle: "ELSE_NOT_PLRNOUN", middle2: None, end: "ENDIF_PLRNOUN", handler: 0x020687dc },
    Condition { start: "IF_M_NAME_PLRNOUN", middle: "ELSE_NOT_PLRNOUN", middle2: None, end: "ENDIF_PLRNOUN", handler: 0x02068808 },
    Condition { start: "IF_I_NAME_M", middle: "IF_I_NAME_F", middle2: Some("IF_I_NAME_N"), end: "ENDIF_I_NAME_MFN", handler: 0x02068834 },
    Condition { start: "IF_M_NAME_M", middle: "IF_M_NAME_F", middle2: Some("IF_M_NAME_N"), end: "ENDIF_M_NAME_MFN", handler: 0x02068850 },
    Condition { start: "IF_ACTION_M", middle: "IF_ACTION_F", middle2: Some("IF_ACTION_N"), end: "ENDIF_ACTION", handler: 0x0206886c },
    Condition { start: "IF_ACTOR_MALE", middle: "ELSE_ACTOR_NOT_MALE", middle2: None, end: "ENDIF_ACTOR_MALE", handler: 0x02068888 },
    Condition { start: "IF_ACTOR_M", middle: "IF_ACTOR_F", middle2: Some("IF_ACTOR_N"), end: "ENDIF_ACTOR_MFN", handler: 0x020688a4 },
    Condition { start: "IF_TARGET_M", middle: "IF_TARGET_F", middle2: Some("IF_TARGET_N"), end: "ENDIF_TARGET_MFN", handler: 0x020688c0 },
    Condition { start: "IF_TARGET_PARTY", middle: "ELSE_TARGET_NOT_PARTY", middle2: None, end: "ENDIF_TARGET_PARTY", handler: 0x020688dc },
    Condition { start: "IF_ACTOR_PARTY", middle: "ELSE_ACTOR_NOT_PARTY", middle2: None, end: "ENDIF_ACTOR_PARTY", handler: 0x020688f4 },
    Condition { start: "IF_ACTOR_TARGET", middle: "ELSE_ACTOR_NOT_TARGET", middle2: None, end: "ENDIF_ACTOR_TARGET", handler: 0x0206890c },
    Condition { start: "IF_ACTOR_PR", middle: "ELSE_ACTOR_NOT_PR", middle2: None, end: "ENDIF_ACTOR_PR", handler: 0x02068938 },
    Condition { start: "IF_TARGET_PR", middle: "ELSE_TARGET_NOT_PR", middle2: None, end: "ENDIF_TARGET_PR", handler: 0x02068964 },
    Condition { start: "IF_TARGET_SING", middle: "ELSE_TARGET_PLR", middle2: None, end: "ENDIF_TARGET", handler: 0x02068990 },
    Condition { start: "IF_TARGET_MIXED", middle: "ELSE_TARGET_NOT_MIXED", middle2: None, end: "ENDIF_TARGET_MIXED", handler: 0x0206899c },
    Condition { start: "IF_VOWEL_FR_HERO", middle: "ELSE_CONSONANT_FR_HERO", middle2: None, end: "ENDIF_VOWEL_FR_HERO", handler: 0x020689b4 },
    Condition { start: "IF_VOWEL_FR_ADDRESSEE", middle: "ELSE_CONSONANT_FR_ADDRESSEE", middle2: None, end: "ENDIF_VOWEL_FR_ADDRESSEE", handler: 0x020689f4 },
    Condition { start: "IF_VOWEL_FR_LEADER", middle: "ELSE_CONSONANT_FR_LEADER", middle2: None, end: "ENDIF_VOWEL_FR_LEADER", handler: 0x02068a30 },
    Condition { start: "IF_VOWEL_FR_I", middle: "ELSE_CONSONANT_FR_I", middle2: None, end: "ENDIF_VOWEL_FR_I", handler: 0x02068a6c },
    Condition { start: "IF_VOWEL_FR_I1", middle: "ELSE_CONSONANT_FR_I", middle2: None, end: "ENDIF_VOWEL_FR_I", handler: 0x02068a6c },
    Condition { start: "IF_VOWEL_FR_I2", middle: "ELSE_CONSONANT_FR_I", middle2: None, end: "ENDIF_VOWEL_FR_I", handler: 0x02068a9c },
    Condition { start: "IF_VOWEL_FR_M", middle: "ELSE_CONSONANT_FR_M", middle2: None, end: "ENDIF_VOWEL_FR_M", handler: 0x02068acc },
    Condition { start: "IF_VOWEL_FR_VOCATION", middle: "ELSE_CONSONANT_FR_VOCATION", middle2: None, end: "ENDIF_VOWEL_FR_VOCATION", handler: 0x02068afc },
    Condition { start: "IF_VOWEL_FR_ACTOR", middle: "ELSE_CONSONANT_FR_ACTOR", middle2: None, end: "ENDIF_VOWEL_FR_ACTOR", handler: 0x02068b2c },
    Condition { start: "IF_VOWEL_FR_TARGET", middle: "ELSE_CONSONANT_FR_TARGET", middle2: None, end: "ENDIF_VOWEL_FR_TARGET", handler: 0x02068b5c },
    Condition { start: "IF_SING_FR VAL_1", middle: "ELSE_NOT_SING_FR", middle2: None, end: "ENDIF_SING_FR", handler: 0x02068b8c },
    Condition { start: "IF_SING_FR VAL_2", middle: "ELSE_NOT_SING_FR", middle2: None, end: "ENDIF_SING_FR", handler: 0x02068ba4 },
    Condition { start: "IF_SING_FR VAL_3", middle: "ELSE_NOT_SING_FR", middle2: None, end: "ENDIF_SING_FR", handler: 0x02068bbc },
    Condition { start: "IF_SING_FR VAL_4", middle: "ELSE_NOT_SING_FR", middle2: None, end: "ENDIF_SING_FR", handler: 0x02068bd4 },
    Condition { start: "IF_SING_FR VAL_5", middle: "ELSE_NOT_SING_FR", middle2: None, end: "ENDIF_SING_FR", handler: 0x02068bec },
    Condition { start: "IF_VOWEL_HERO", middle: "ELSE_CONSONANT_HERO", middle2: None, end: "ENDIF_VOWEL_HERO", handler: 0x02068c04 },
    Condition { start: "IF_VOWEL_LEADER", middle: "ELSE_CONSONANT_LEADER", middle2: None, end: "ENDIF_VOWEL_LEADER", handler: 0x02068c40 },
    Condition { start: "IF_VOWEL_I", middle: "ELSE_CONSONANT_I", middle2: None, end: "ENDIF_VOWEL_I", handler: 0x02068c7c },
    Condition { start: "IF_VOWEL_M", middle: "ELSE_CONSONANT_M", middle2: None, end: "ENDIF_VOWEL_M", handler: 0x02068cac },
    Condition { start: "IF_VOWEL_VOCATION", middle: "ELSE_CONSONANT_VOCATION", middle2: None, end: "ENDIF_VOWEL_VOCATION", handler: 0x02068cdc },
    Condition { start: "IF_VOWEL_ACTOR", middle: "ELSE_CONSONANT_ACTOR", middle2: None, end: "ENDIF_VOWEL_ACTOR", handler: 0x02068d0c },
    Condition { start: "IF_VOWEL_TARGET", middle: "ELSE_CONSONANT_TARGET", middle2: None, end: "ENDIF_VOWEL_TARGET", handler: 0x02068d3c },
    Condition { start: "IF_LAST_LETTER_S_I", middle: "ELSE_NOT_LAST_LETTER_S_I", middle2: None, end: "ENDIF_LAST_LETTER_S_I", handler: 0x02068d6c },
    Condition { start: "IF_LAST_LETTER_S_M", middle: "ELSE_NOT_LAST_LETTER_S_M", middle2: None, end: "ENDIF_LAST_LETTER_S_M", handler: 0x02068d9c },
    Condition { start: "IF_LAST_LETTER_S_DE_ACTOR", middle: "ELSE_NOT_LAST_LETTER_S_DE_ACTOR", middle2: None, end: "ENDIF_LAST_LETTER_S_DE_ACTOR", handler: 0x02068dcc },
    Condition { start: "IF_LAST_LETTER_S_DE_TARGET", middle: "ELSE_NOT_LAST_LETTER_S_DE_TARGET", middle2: None, end: "ENDIF_LAST_LETTER_S_DE_TARGET", handler: 0x02068dfc },
];

fn bracketed(name: &str) -> Vec<u8> {
    [b"<", name.as_bytes(), b">"].concat()
}

fn search(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Case-insensitive prefix test (`FUN_020d85ec`).
fn starts_with_ci(text: &[u8], prefix: &[u8]) -> bool {
    text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Pass 2, `Msg_EvalConditionals` (`0x02068f50`). For each `<IF_…>` tag of a rule in
/// [`CONDITIONS`], `branch(rule)` picks the branch to keep (0, 1 or 2; other values act
/// as 0). The kept branch is scanned again, so nested blocks of other rules work.
///
/// Selection (`Msg_SelectBranch`, `0x02068e2c`) searches the delimiters with plain substring
/// search from the current position: the first occurrence of the closing tag wins. When a
/// delimiter is missing, the `<` is copied and scanning continues.
pub fn eval_conditionals(text: &[u8], mut branch: impl FnMut(&Condition) -> u8) -> Vec<u8> {
    let mut work = text.to_vec();
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < work.len() {
        if starts_with_ci(&work[i..], b"<IF_")
            && find_tag_end(&work[i..]).is_some()
            && let Some(rule) = CONDITIONS
                .iter()
                .find(|r| starts_with_ci(&work[i..], &bracketed(r.start)))
        {
            let [start, middle, end] = match (branch(rule), rule.middle2) {
                (1, None) => [rule.middle, rule.end, rule.end],
                (1, Some(m2)) => [rule.middle, m2, rule.end],
                (2, _) => [rule.middle2.unwrap_or(rule.end), rule.end, rule.end],
                _ => [rule.start, rule.middle, rule.end],
            };
            if let Some(next) = select_branch(
                &mut work,
                i,
                &bracketed(start),
                &bracketed(middle),
                &bracketed(end),
            ) {
                i = next;
                continue;
            }
        }
        out.push(work[i]);
        i += 1;
    }
    out
}

/// Keeps `[start tag end, middle)` and drops `[middle, end tag end)`, in place. Returns the
/// position to resume scanning (just after the start tag).
fn select_branch(
    work: &mut Vec<u8>,
    from: usize,
    start: &[u8],
    middle: &[u8],
    end: &[u8],
) -> Option<usize> {
    let a = from + search(&work[from..], start)?;
    let b = a + search(&work[a..], middle)?;
    let c = b + search(&work[b..], end)?;
    work.drain(b..c + end.len());
    Some(a + start.len())
}

/// Grammatical parameters of a name tag, from the `g_MsgGrammarTags` prefixes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Grammar {
    /// `DEF_` = 0, `INDEF_` = 1.
    pub article: Option<u8>,
    /// `ART_` = 1.
    pub with_article: Option<u8>,
    /// `SGL_` = 0, `PLR_` = 1.
    pub number: Option<u8>,
    /// `NOM_` = 0, `GEN_` = 1, `DAT_` = 2, `ACC_` = 3.
    pub case: Option<u8>,
}

/// Named entity of a grammar tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entity {
    Item,
    Monster,
    Actor,
    Target,
    Action,
    Reflex,
}

/// A substitution requested by [`expand_names`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Name<'a> {
    /// `<HERO>`, `<LEADER>`, `<ADDRESSEE>`, `<TMAP_SEC…>`, `<STR_n>`: the table name and the
    /// index from a trailing digit (`n - 1`, else 0).
    Substitution { tag: &'a str, index: u32 },
    /// `<INDEF_ART_SGL_I_NAME>`-style tag, resolved by the game's inflection routine
    /// (`FUN_020e4968`).
    Inflected {
        grammar: Grammar,
        entity: Entity,
        index: u32,
    },
    /// `[WPN…]`: current weapon name.
    Weapon,
    /// `[ACTER…]`: current actor name.
    Actor,
}

/// `g_MsgSubstTags` (`0x020e7e9c`); `SIZE=` has no handler: the tag is just removed.
const SUBSTITUTIONS: [&str; 6] = ["HERO", "LEADER", "ADDRESSEE", "TMAP_SEC", "STR_", "SIZE="];

/// `g_MsgGrammarTags` (`0x020e7f14`), consumed in table order.
const GRAMMAR: [(&str, u8, i8); 15] = [
    ("DEF_", 0, 0),
    ("INDEF_", 0, 1),
    ("ART_", 1, 1),
    ("SGL_", 2, 0),
    ("PLR_", 2, 1),
    ("NOM_", 3, 0),
    ("GEN_", 3, 1),
    ("DAT_", 3, 2),
    ("ACC_", 3, 3),
    ("I_NAME", 4, -1),
    ("M_NAME", 5, -1),
    ("ACTOR", 6, -1),
    ("TARGET", 7, -1),
    ("ACTION", 8, -1),
    ("REFLEX", 9, -1),
];

/// Pass 3, `Msg_ExpandNames` (`0x02069244`): replaces name tags with `resolve(name)`.
pub fn expand_names(text: &[u8], mut resolve: impl FnMut(Name) -> Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some((tag, end)) = tag_at(rest) {
            if let Some(name) = SUBSTITUTIONS.iter().find(|n| tag.starts_with(n.as_bytes())) {
                if *name != "SIZE=" {
                    let index = digit_index(&tag[name.len()..]);
                    out.extend(resolve(Name::Substitution { tag: name, index }));
                }
                i += end + 1;
                continue;
            }
            if let Some((grammar, entity, index)) = parse_grammar(tag) {
                if let Some(entity) = entity {
                    out.extend(resolve(Name::Inflected {
                        grammar,
                        entity,
                        index,
                    }));
                }
                i += end + 1;
                continue;
            }
        } else if rest.first() == Some(&b'[') {
            let name = if rest.starts_with(b"[WPN") {
                Some((Name::Weapon, 4))
            } else if rest.starts_with(b"[ACTER") {
                Some((Name::Actor, 6))
            } else {
                None
            };
            if let Some((name, skip)) = name {
                out.extend(resolve(name));
                match rest[skip..].iter().position(|&b| b == b']') {
                    Some(close) => i += skip + close + 1,
                    None => {
                        // Unterminated: the game appends "error" and stops.
                        out.extend_from_slice(b"error");
                        return out;
                    }
                }
                continue;
            }
        }
        out.push(rest[0]);
        i += 1;
    }
    out
}

fn parse_grammar(mut tag: &[u8]) -> Option<(Grammar, Option<Entity>, u32)> {
    let mut grammar = Grammar::default();
    let (mut entity, mut index, mut matched) = (None, 0, false);
    for (prefix, kind, value) in GRAMMAR {
        let Some(rest) = tag.strip_prefix(prefix.as_bytes()) else {
            continue;
        };
        tag = rest;
        matched = true;
        let value = value as u8;
        match kind {
            0 => grammar.article = Some(value),
            1 => grammar.with_article = Some(value),
            2 => grammar.number = Some(value),
            3 => grammar.case = Some(value),
            _ => {
                index = digit_index(tag);
                entity = Some(match kind {
                    4 => Entity::Item,
                    5 => Entity::Monster,
                    6 => Entity::Actor,
                    7 => Entity::Target,
                    8 => Entity::Action,
                    _ => Entity::Reflex,
                });
            }
        }
    }
    matched.then_some((grammar, entity, index))
}

/// Pass 4, `Msg_ApplyCap` (`0x02069604`): removes `<CAP>` and upper-cases the following glyph
/// string when the font marks it lowercase.
pub fn apply_cap(font: &Font, text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some((tag, end)) = tag_at(&text[i..])
            && tag.starts_with(b"CAP")
            && text.get(i + 4) == Some(&b'>')
        {
            i += end + 1;
            if let Some(index) = font.find(&text[i..]) {
                let glyph = &font.glyphs[usize::from(index)];
                if glyph.lowercase {
                    out.extend(glyph.text.iter().map(u8::to_ascii_uppercase));
                    i += glyph.text.len();
                }
            }
            continue;
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

/// Whether a name starts with a vowel sound (`FUN_02067ffc`): first glyph marked vowel; or
/// `h` + vowel + non-vowel (French mute h); or `y` + non-vowel. The game first honours a
/// per-entity override flag, which callers apply themselves.
pub fn starts_with_vowel(font: &Font, name: &[u8]) -> bool {
    let glyph = |text: &[u8]| font.find(text).map(|i| &font.glyphs[usize::from(i)]);
    let Some(first) = glyph(name) else {
        return false;
    };
    if first.vowel {
        return true;
    }
    let rest = &name[first.text.len()..];
    let vowel_at = |text: &[u8]| glyph(text).is_some_and(|g| g.vowel);
    match name[0] {
        b'h' => match glyph(rest) {
            Some(second) if second.vowel => !vowel_at(&rest[second.text.len()..]),
            _ => false,
        },
        b'y' => !vowel_at(rest),
        _ => false,
    }
}

/// Element of a compiled message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Bytes drawn with the font (see [`Font::layout`]).
    Text(Vec<u8>),
    /// Line break, code `0xFF18`.
    NewLine,
    /// Control code (`0xFF01` end, `0xFF0D` page…) with the raw text of its parameter
    /// (after `=` or the tag name), when the tag has one.
    Control { code: u16, arg: Option<Vec<u8>> },
}

/// `g_MsgControlTags` (`0x020e7f94`): tag, code; `LB_`/`JP_` add `letter - 0x40`.
const CONTROLS: [(&str, u16); 40] = [
    ("PAGE_T=", 0xFF0C),
    ("PAGE>", 0xFF0D),
    ("AUTO=", 0xFF0B),
    ("ADD>", 0xFF0A),
    ("TIME=", 0xFF1A),
    ("PAD_WAIT>", 0xFF1D),
    ("PAD_T=", 0xFF1E),
    ("PAD_WAIT_NOCUR>", 0xFF1F),
    ("WIN_ON>", 0xFF26),
    ("WIN_OFF>", 0xFF27),
    ("CEN_ON>", 0xFF28),
    ("CEN_OFF>", 0xFF29),
    ("ALL_RECOVER=", 0xFF1B),
    ("ST=", 0xFF1C),
    ("YESNO>", 0xFF04),
    ("NOYES>", 0xFF05),
    ("YESNO_NOTSE>", 0xFF06),
    ("YESNO_NOTSE_IIE>", 0xFF07),
    ("UKEYAME>", 0xFF08),
    ("LB_", 0xFFD0),
    ("JP_", 0xFFE0),
    ("TURN=", 0xFF2A),
    ("QUEST_SE>", 0xFF0F),
    ("QUEST=", 0xFF10),
    ("QUEST_HAN>", 0xFF11),
    ("QUEST_FAILED>", 0xFF13),
    ("/QUEST>", 0xFF12),
    ("N_TURN>", 0xFF2B),
    ("END_R_TURN>", 0xFF02),
    ("R_TURN>", 0xFF2C),
    ("TURN_P>", 0xFF2D),
    ("EXC>", 0xFF2E),
    ("QES>", 0xFF2F),
    ("YES>", 0xFF14),
    ("NO>", 0xFF15),
    ("UKE>", 0xFF16),
    ("YAME>", 0xFF17),
    ("END>", 0xFF01),
    ("CLOSE>", 0xFF03),
    ("SHAKE>", 0xFF0E),
];

/// Tags removed by `Msg_Compile` (`g_MsgSkippedTagPrefixes`, `0x020e7e84`). The game's list
/// also has `RENKIN` without `<`, which can never match at a `<`.
const SKIPPED: [&str; 4] = ["<INN=", "<CHURCH=", "<BANK>", "<SHOP"];

const END: u16 = 0xFF01;
const VOICE_VOLUME_BASE: u16 = 0xFF4D;
const MUSIC_EFFECT: u16 = 0xFF4B;

/// Pass 5, `Msg_Compile` (`0x02069ffc`): line breaks and control tags become tokens; other
/// bytes stay text. An `0xFF01` end token is appended, except when a sound tag is left
/// unterminated (the game aborts there).
pub fn compile(text: &[u8]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut pending = Vec::new();
    let emit = |tokens: &mut Vec<Token>, pending: &mut Vec<u8>, token: Token| {
        if !pending.is_empty() {
            tokens.push(Token::Text(std::mem::take(pending)));
        }
        tokens.push(token);
    };
    let mut voice_slot = 0u16;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with(b"\\n") || rest.starts_with(b"\r\n") {
            emit(&mut tokens, &mut pending, Token::NewLine);
            i += 2;
            continue;
        }
        if rest[0] == b'\n' {
            emit(&mut tokens, &mut pending, Token::NewLine);
            i += 1;
            continue;
        }
        let Some((_, end)) = tag_at(rest) else {
            pending.push(rest[0]);
            i += 1;
            continue;
        };
        // Matching is a case-insensitive prefix test on the text after '<' (`FUN_020d85ec`);
        // names include their terminating '>' or '='.
        if let Some(&(name, code)) = CONTROLS
            .iter()
            .find(|(n, _)| starts_with_ci(&rest[1..], n.as_bytes()))
        {
            let after = &rest[1 + name.len()..end.max(1 + name.len())];
            let token = match name {
                "LB_" | "JP_" => {
                    let letter = rest.get(1 + name.len()).copied().unwrap_or(0x40);
                    Token::Control {
                        code: code.wrapping_add(u16::from(letter)).wrapping_sub(0x40),
                        arg: None,
                    }
                }
                _ if name.ends_with('>') => Token::Control { code, arg: None },
                _ => Token::Control {
                    code,
                    arg: Some(after.to_vec()),
                },
            };
            emit(&mut tokens, &mut pending, token);
            i += end + 1;
            continue;
        }
        // Tags the compiler drops; scanning resumes right after them.
        let mut j = i;
        if SKIPPED.iter().any(|p| rest.starts_with(p.as_bytes())) {
            j += end + 1;
        }
        let rest = &text[j..];
        let sound = if let Some(arg) = rest.strip_prefix(b"<VOICE_VOLUME=") {
            let code = VOICE_VOLUME_BASE + voice_slot;
            voice_slot = (voice_slot + 1) & 3;
            Some((code, arg))
        } else if let Some(arg) = rest.strip_prefix(b"<ME_") {
            Some((MUSIC_EFFECT, arg))
        } else if let Some(arg) = rest.strip_prefix(b"<SE_") {
            let n = arg
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .fold(0u16, |n, d| {
                    n.wrapping_mul(10).wrapping_add(u16::from(d - b'0'))
                });
            Some((n.wrapping_sub(0xCC), arg))
        } else {
            None
        };
        match sound {
            Some((code, arg)) => {
                let Some(close) = arg.iter().position(|&b| b == b'>') else {
                    // The game stops compiling here, without an end code.
                    if !pending.is_empty() {
                        tokens.push(Token::Text(pending));
                    }
                    return tokens;
                };
                emit(
                    &mut tokens,
                    &mut pending,
                    Token::Control {
                        code,
                        arg: Some(arg[..close].to_vec()),
                    },
                );
                i = text.len() - arg.len() + close + 1;
            }
            None => {
                // The game copies one byte at the resume position, even a '<'.
                if j < text.len() {
                    pending.push(text[j]);
                }
                i = j + 1;
            }
        }
    }
    emit(
        &mut tokens,
        &mut pending,
        Token::Control {
            code: END,
            arg: None,
        },
    );
    tokens
}

/// What the tags at the very start of a message ask of the message window: the ones the game
/// consumes before compiling (`0x0206a3d0`, called first by `Msg_Compile`), instead of
/// turning them into codes. Meaning of the window fields: `docs/re/dialogue.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeadingTags {
    /// `<N_TURN>`: the speaker keeps its facing instead of turning to the hero.
    pub no_turn: bool,
    /// `<EXC>` / `<QES>`: a "!" / "?" balloon over the speaker.
    pub exclamation: bool,
    pub question: bool,
    /// `<WIN>`: window byte `+0x19b1` cleared, i.e. the frame is not drawn.
    pub frameless: bool,
    /// `<CEN>`: window byte `+0x19b8` set, the window of narration cards **[hypothèse :
    /// boîte et lignes centrées]**.
    pub centered: bool,
    /// `<SKIP>`: window byte `+0x19bb` cleared.
    pub no_skip: bool,
    /// `<GYOU=n>`: number of lines of the box, at least 1 (default 3).
    pub lines: Option<i32>,
    /// `<MOJI=w,h>`: font cell, 12 and 16 when a value is not positive.
    pub cell: Option<(i32, i32)>,
    /// `<COLOR=r,g,b>`: BGR555 text colour (components clamped to 0-255, negative → 255).
    pub color: Option<u16>,
    /// `<RECT=x,y,w,h>`: window rectangle on the 256×192 screen.
    pub rect: Option<[i32; 4]>,
}

/// Parses a signed decimal integer at the start of `text`, as `atoi` (0 without digits).
fn atoi(text: &[u8]) -> i32 {
    let (negative, digits) = match text.first() {
        Some(b'-') => (true, &text[1..]),
        _ => (false, text),
    };
    let n = digits
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0i32, |n, d| {
            n.wrapping_mul(10).wrapping_add(i32::from(d - b'0'))
        });
    if negative { n.wrapping_neg() } else { n }
}

/// Comma-separated integers of a tag argument (`TextParseTagArgs`-style), up to the `>`.
fn tag_args(args: &[u8]) -> Vec<i32> {
    let end = args.iter().position(|&b| b == b'>').unwrap_or(args.len());
    args[..end].split(|&b| b == b',').map(atoi).collect()
}

/// Consumes the leading layout tags of `text` (the loop of `0x0206a3d0`): returns what they
/// ask for and the rest of the text. A tag without its `>` ends the loop with the text left
/// at that tag. Matching ignores case (the normal pass upper-cases tags).
pub fn leading_tags(text: &[u8]) -> (LeadingTags, &[u8]) {
    let mut tags = LeadingTags::default();
    let mut rest = text;
    while let Some(end) = rest.iter().position(|&b| b == b'>') {
        let tag = &rest[..=end];
        let has = |name: &str| starts_with_ci(tag, name.as_bytes());
        if has("<N_TURN>") {
            tags.no_turn = true;
        } else if has("<EXC>") {
            tags.exclamation = true;
        } else if has("<QES>") {
            tags.question = true;
        } else if has("<WIN>") {
            tags.frameless = true;
        } else if has("<CEN>") {
            tags.centered = true;
        } else if has("<SKIP>") {
            tags.no_skip = true;
        } else if has("<GYOU=") {
            tags.lines = Some(atoi(&tag[6..]).max(1));
        } else if has("<MOJI=") {
            let a = tag_args(&tag[6..]);
            let value =
                |i: usize, default: i32| a.get(i).copied().filter(|&v| v > 0).unwrap_or(default);
            tags.cell = Some((value(0, 12), value(1, 16)));
        } else if has("<COLOR=") {
            let a = tag_args(&tag[7..]);
            if a.len() >= 3 {
                let c = |v: i32| if v < 0 { 255 } else { v.min(255) } as u16;
                tags.color = Some((c(a[0]) >> 3) | ((c(a[1]) >> 3) << 5) | ((c(a[2]) >> 3) << 10));
            }
        } else if has("<RECT=") {
            let a = tag_args(&tag[6..]);
            if a.len() >= 4 {
                let x = a[0].clamp(0, 0x100);
                let y = a[1].clamp(0, 0xc0);
                let w = if (0..=0x100).contains(&a[2]) {
                    a[2]
                } else {
                    0x100 - x
                };
                let h = if (0..=0xc0).contains(&a[3]) {
                    a[3]
                } else {
                    0xc0 - y
                };
                tags.rect = Some([x, y, w, h]);
            }
        } else {
            break;
        }
        rest = &rest[end + 1..];
    }
    (tags, rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontId;

    /// Font with glyphs "a" (vowel, lowercase), "b" (lowercase), "A" (vowel), "B", "h"
    /// (lowercase), "<'e>" (vowel, lowercase), "<'E>" (vowel), "<>>>".
    fn font_files() -> (Vec<u8>, Vec<u8>) {
        let glyphs: [(&[u8], u8); 8] = [
            (b"a", 0xC0),
            (b"b", 0x80),
            (b"A", 0x40),
            (b"B", 0),
            (b"h", 0x80),
            (b"<'e>", 0xC0),
            (b"<'E>", 0x40),
            (b"<>>>", 0),
        ];
        let table = 0x18;
        let strings = table + glyphs.len() * 8;
        let mut info = b"1.1\0".to_vec();
        for v in [glyphs.len(), 0, table, table, strings] {
            info.extend((v as u32).to_le_bytes());
        }
        let mut pool: Vec<u8> = Vec::new();
        for (text, flags) in glyphs {
            info.extend(((strings + pool.len()) as u32).to_le_bytes());
            info.extend([1, flags | text.len() as u8, 0, 0]);
            pool.extend(text);
        }
        info.extend(pool);
        let mut data = b"1.0\0".to_vec();
        for v in [8u16, 1] {
            data.extend(v.to_le_bytes());
        }
        data.extend(1u32.to_le_bytes());
        data.extend(16u32.to_le_bytes());
        data.push(0);
        (info, data)
    }

    #[test]
    fn tag_end_skips_doubled_bracket() {
        assert_eq!(find_tag_end(b"<>>>x"), Some(3));
        assert_eq!(find_tag_end(b"<CAP>a"), Some(4));
        assert_eq!(find_tag_end(b"<open"), None);
    }

    #[test]
    fn values_are_substituted_with_their_index() {
        let out = expand_values(b"x<val_2>y<VAL_1>", |i| format!("[{i}]").into_bytes());
        assert_eq!(out, b"x[1]y[0]");
    }

    #[test]
    fn normalize_keeps_glyphs_uppercases_tags_and_blanks_unknown_bytes() {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        assert_eq!(normalize(&font, b"a<'e> <Cap>b\\n%"), b"a<'e> <CAP>b\\n< >");
    }

    #[test]
    fn conditionals_keep_the_selected_branch() {
        let text = b"x<IF_MALE>he<ELSE_NOT_MALE>she<ENDIF_MALE>y";
        assert_eq!(eval_conditionals(text, |_| 0), b"xhey");
        assert_eq!(eval_conditionals(text, |_| 1), b"xshey");
        let gender = b"<IF_I_NAME_M>m<IF_I_NAME_F>f<IF_I_NAME_N>n<ENDIF_I_NAME_MFN>!";
        assert_eq!(eval_conditionals(gender, |_| 1), b"f!");
        assert_eq!(eval_conditionals(gender, |_| 2), b"n!");
        // Unknown rule or missing delimiter: copied unchanged.
        assert_eq!(eval_conditionals(b"<IF_NOPE>x", |_| 0), b"<IF_NOPE>x");
        assert_eq!(eval_conditionals(b"<IF_MALE>x", |_| 0), b"<IF_MALE>x");
    }

    #[test]
    fn rule_tags_match_the_whole_tag() {
        let mut seen: Vec<&str> = Vec::new();
        let out = eval_conditionals(
            b"<IF_VOWEL_FR_I1>a<ELSE_CONSONANT_FR_I>b<ENDIF_VOWEL_FR_I>",
            |r| {
                seen.push(r.start);
                1
            },
        );
        // The rule tag is built with its closing '>', so "IF_VOWEL_FR_I" (earlier in the
        // table) does not catch "IF_VOWEL_FR_I1".
        assert_eq!(seen, ["IF_VOWEL_FR_I1"]);
        assert_eq!(out, b"b");
    }

    #[test]
    fn names_and_grammar_tags_are_resolved() {
        let out = expand_names(
            b"<HERO>, <STR_2> & <INDEF_ART_SGL_I_NAME2>[WPN x]<SIZE=3>.",
            |n| format!("{n:?}").into_bytes(),
        );
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with(
            "Substitution { tag: \"HERO\", index: 0 }, Substitution { tag: \"STR_\", index: 1 } & "
        ));
        assert!(out.contains("Inflected { grammar: Grammar { article: Some(1), with_article: Some(1), number: Some(0), case: None }, entity: Item, index: 1 }"));
        assert!(out.ends_with("Weapon."));
    }

    #[test]
    fn cap_uppercases_lowercase_glyphs_only() {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        assert_eq!(apply_cap(&font, b"<CAP><'e>t <CAP>B"), b"<'E>t B");
    }

    #[test]
    fn vowel_rules() {
        let (info, data) = font_files();
        let font = Font::parse(&info, &data, FontId::Message).unwrap();
        assert!(starts_with_vowel(&font, b"<'e>b"));
        assert!(starts_with_vowel(&font, b"hab"));
        assert!(!starts_with_vowel(&font, b"haa"));
        assert!(!starts_with_vowel(&font, b"bab"));
    }

    #[test]
    fn compile_tokens() {
        let tokens = compile(b"Hi\\nyou<PAGE>x<PAGE_T=30><LB_A><INN=2><SE_5>!");
        assert_eq!(
            tokens,
            [
                Token::Text(b"Hi".to_vec()),
                Token::NewLine,
                Token::Text(b"you".to_vec()),
                Token::Control {
                    code: 0xFF0D,
                    arg: None
                },
                Token::Text(b"x".to_vec()),
                Token::Control {
                    code: 0xFF0C,
                    arg: Some(b"30".to_vec())
                },
                Token::Control {
                    code: 0xFFD1,
                    arg: None
                },
                Token::Control {
                    code: 5u16.wrapping_sub(0xCC),
                    arg: Some(b"5".to_vec())
                },
                Token::Text(b"!".to_vec()),
                Token::Control {
                    code: 0xFF01,
                    arg: None
                },
            ]
        );
    }

    #[test]
    fn leading_tags_are_consumed_and_stop_at_text() {
        let (tags, rest) = leading_tags(b"<N_TURN><CEN><GYOU=0><MOJI=0,5>*: hi<CEN>");
        assert!(tags.no_turn && tags.centered && !tags.frameless);
        assert_eq!(tags.lines, Some(1));
        assert_eq!(tags.cell, Some((12, 5)));
        assert_eq!(rest, b"*: hi<CEN>");
        let (tags, rest) = leading_tags(b"<color=300,-1,8><RECT=10,20,-1,50>x");
        assert_eq!(tags.color, Some(31 | 31 << 5 | 1 << 10));
        assert_eq!(tags.rect, Some([10, 20, 246, 50]));
        assert_eq!(rest, b"x");
        assert_eq!(leading_tags(b"<PAGE>a").1, b"<PAGE>a");
    }
}
