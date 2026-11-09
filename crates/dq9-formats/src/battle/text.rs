//! Battle text: the five message files a battle says from and the builder that turns one of
//! their messages into the text the window draws. Layouts, counts and evidence in
//! `docs/re/battle-text.md`; wiki *Battle Text*, *System Strings*, *Articles*.
//!
//! The message files are in the system-strings layout (a head word, then a number and an
//! offset per message, then the strings) except `str_bres`, which is a Level-5 record table
//! (a number and the message as its first two fields). Names, articles and the German case
//! templates are folded in by the message pipeline ([`crate::message::inflect`],
//! `FUN_020e4968`), so a message comes out with its articles, plurals, genders and numbers
//! resolved.

use crate::Font;
use crate::battle::{FixedRecords, monster};
use crate::message::{self, Condition, Declension, Entity, Name, NameRecord, Token};
use crate::table::{Table, Value};
use nds_rom::bytes::{u16_at, u32_at};
use nds_rom::{Error, Result};

/// The languages of the European release, with the game's language code: English 1, French 2,
/// German 3, Italian 4, Spanish 5 (the setting read by `func_0200fb08`, `0x0200fb18` EU,
/// which falls back to 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    French,
    German,
    Italian,
    Spanish,
}

impl Language {
    /// Every language of the European ROM, in game-code order.
    pub const ALL: [Self; 5] = [
        Self::English,
        Self::French,
        Self::German,
        Self::Italian,
        Self::Spanish,
    ];

    /// The member suffix: `en`, `fr`, `de`, `it`, `es`.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::French => "fr",
            Self::German => "de",
            Self::Italian => "it",
            Self::Spanish => "es",
        }
    }

    /// The game's own language code (`func_0200fb08`): 1 to 5.
    pub fn code(self) -> u8 {
        match self {
            Self::English => 1,
            Self::French => 2,
            Self::German => 3,
            Self::Italian => 4,
            Self::Spanish => 5,
        }
    }

    /// The language of a member suffix.
    pub fn from_suffix(suffix: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.suffix() == suffix)
    }
}

/// A message file a battle says from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum File {
    /// Monsters drawing near and fleeing (17 messages).
    Strbtl,
    /// What an action says (591 messages).
    Actmsg,
    /// What is said as a battle ends; a Level-5 table, not the system-strings layout.
    StrBres,
    /// The battle menu's commands (106 messages).
    StrBtl,
    /// The field menu's strings, said outside and inside a battle (329 messages).
    StrTm,
}

impl File {
    /// Every battle text file.
    pub const ALL: [Self; 5] = [
        Self::Strbtl,
        Self::Actmsg,
        Self::StrBres,
        Self::StrBtl,
        Self::StrTm,
    ];

    /// The name the `dq9re battle-text` command takes.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strbtl => "strbtl",
            Self::Actmsg => "actmsg",
            Self::StrBres => "str_bres",
            Self::StrBtl => "str_btl",
            Self::StrTm => "str_tm",
        }
    }

    /// The archive inside the ROM that holds it.
    pub fn archive(self) -> &'static str {
        match self {
            Self::Strbtl => "data/bin/strbtl.gp2",
            Self::Actmsg => "data/prm/actmsg.gp2",
            Self::StrBres => "data/bin/str_bres.gp2",
            Self::StrBtl => "data/bin/menu/str_btl.gp2",
            Self::StrTm => "data/bin/menu/str_tm.gp2",
        }
    }

    /// The member of `lang` inside the archive.
    pub fn member(self, lang: Language) -> String {
        let extension = if self == Self::StrBres { "bin" } else { "nat" };
        format!("{}_{}.{extension}", self.name(), lang.suffix())
    }

    /// The file named `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|file| file.name() == name)
    }
}

/// A `.nat` message file in the system-strings layout: a head word, a number and an offset
/// per message, then the strings (wiki *System Strings*).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageTable<'a> {
    messages: Vec<(u16, &'a [u8])>,
}

impl<'a> MessageTable<'a> {
    /// Parses `data`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed head word, record or string offset; a number above
    /// `0xffff` is a malformed record.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = FixedRecords::parse(data, 8)?;
        let mut messages = Vec::with_capacity(table.count);
        for i in 0..table.count {
            let record = table.record(i)?;
            // The message number is the low halfword; the high bits are flags (actmsg's
            // record 0, numbered 0, carries 0x39 there).
            let number = (u32_at(record, 0)? & 0xffff) as u16;
            messages.push((number, table.string(u32_at(record, 4)?)?));
        }
        Ok(Self { messages })
    }

    /// The text of message `number`, markup included.
    pub fn message(&self, number: u16) -> Option<&'a [u8]> {
        self.messages
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, text)| *text)
    }

    /// Number of messages in the file.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// The numbers of the messages, in file order.
    pub fn numbers(&self) -> impl Iterator<Item = u16> + '_ {
        self.messages.iter().map(|(number, _)| *number)
    }
}

/// `str_bres`: what is said as a battle ends, in a Level-5 record table. The records of kind
/// `0x67` carry the message number and the text as their first two fields (`dq9re table`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleResult<'a> {
    messages: Vec<(u16, &'a [u8])>,
}

impl<'a> BattleResult<'a> {
    /// Parses `str_bres_<lang>.bin`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] when the table is malformed or a `0x67` record lacks its number or
    /// a string.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut messages = Vec::new();
        for record in &table.records {
            if record.kind != 0x67 || record.values.len() < 2 {
                continue;
            }
            let Value::Int(number) = record.values[0] else {
                return Err(Error::Malformed("str_bres record without a number"));
            };
            let Value::Str(Some(text)) = record.values[1] else {
                return Err(Error::Malformed("str_bres record without a string"));
            };
            let number =
                u16::try_from(number).map_err(|_| Error::Malformed("str_bres message number"))?;
            messages.push((number, text));
        }
        Ok(Self { messages })
    }

    /// The text of message `number`.
    pub fn message(&self, number: u16) -> Option<&'a [u8]> {
        self.messages
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, text)| *text)
    }

    /// Number of messages in the file.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// The numbers of the messages, in file order.
    pub fn numbers(&self) -> impl Iterator<Item = u16> + '_ {
        self.messages.iter().map(|(number, _)| *number)
    }
}

/// One item name record, `itemname_<lang>.nat`: the singular at `+0x00`, the plural at
/// `+0x04`, the grammar word at `+0x08` and the item id at `+0x0C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRecord<'a> {
    pub id: u16,
    pub name: &'a [u8],
    pub plural: &'a [u8],
    /// Bits 0–5 indefinite singular, 6–11 and 18–23 the plurals, 12–17 definite singular,
    /// 24–25 the gender (wiki *Articles*).
    pub grammar: u32,
}

impl<'a> ItemRecord<'a> {
    /// The record as the inflection routine reads it.
    pub fn name_record(&self) -> NameRecord<'a> {
        NameRecord {
            singular: self.name,
            plural: self.plural,
            grammar: self.grammar,
        }
    }
}

/// `itemname.gp2/itemname_<lang>.nat`: 1,178 records of 16 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemNames<'a> {
    items: Vec<ItemRecord<'a>>,
}

impl<'a> ItemNames<'a> {
    /// Parses `itemname_<lang>.nat`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed head word, record or string offset.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = FixedRecords::parse(data, 16)?;
        let mut items = Vec::with_capacity(table.count);
        for i in 0..table.count {
            let record = table.record(i)?;
            items.push(ItemRecord {
                id: u16_at(record, 0x0c)?,
                name: table.string(u32_at(record, 0)?)?,
                plural: table.string(u32_at(record, 4)?)?,
                grammar: u32_at(record, 8)?,
            });
        }
        Ok(Self { items })
    }

    /// The item numbered `id`.
    pub fn find(&self, id: u16) -> Option<&ItemRecord<'a>> {
        self.items.iter().find(|item| item.id == id)
    }

    /// The first item whose singular or plural is `name`, ASCII case-insensitively.
    pub fn find_name(&self, name: &[u8]) -> Option<&ItemRecord<'a>> {
        self.items.iter().find(|item| {
            item.name.eq_ignore_ascii_case(name) || item.plural.eq_ignore_ascii_case(name)
        })
    }

    /// Number of items named.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// The six tables of one language, borrowed from their decompressed members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleFiles<'a> {
    pub strbtl: MessageTable<'a>,
    pub actmsg: MessageTable<'a>,
    pub str_bres: BattleResult<'a>,
    pub str_btl: MessageTable<'a>,
    pub str_tm: MessageTable<'a>,
    /// `article.gp2/article_<lang>.nat`, the articles by number.
    pub articles: MessageTable<'a>,
}

/// One named entity a grammar tag can inflect.
type Named<'a> = Option<NameRecord<'a>>;

/// What one battle message says about: the actor and the target (a party member's name or a
/// monster's name record), the monster and the item it names, the action it names, the values
/// `<val_1>` to `<val_n>`, the strings `<str_1>` to `<str_n>`, and the state its conditions
/// test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context<'a> {
    pub actor: Named<'a>,
    pub target: Named<'a>,
    /// What `<…_M_NAME>` inflects: the target when it is a monster.
    pub monster: Named<'a>,
    pub item: Named<'a>,
    /// What `<ACTION>` inflects.
    pub action: Named<'a>,
    /// `<HERO>`, `<LEADER>`, `<ADDRESSEE>`.
    pub hero: Vec<u8>,
    /// `<str_1>` to `<str_n>`.
    pub strings: Vec<Vec<u8>>,
    /// `<val_1>` to `<val_n>`.
    pub values: Vec<u32>,
    /// `IF_SOLO`: the party is the leader alone.
    pub solo: bool,
    /// `IF_ACTOR_PARTY`, `IF_TARGET_PARTY`.
    pub actor_party: bool,
    pub target_party: bool,
    /// `IF_TARGET_SING`: 1 for one target, more for a group.
    pub target_count: u16,
}

impl Default for Context<'_> {
    fn default() -> Self {
        Self {
            actor: None,
            target: None,
            monster: None,
            item: None,
            action: None,
            hero: b"Hero".to_vec(),
            strings: Vec::new(),
            values: Vec::new(),
            solo: true,
            actor_party: false,
            target_party: false,
            target_count: 1,
        }
    }
}

/// Builds one battle message: the six passes of `Msg_Prepare` (`0x02067ea8`) with the battle's
/// names and state, then the layout and compilation passes, reduced to the bytes the window
/// draws (control codes say when to wait or close; `<ADD>` appends and draws nothing).
#[derive(Debug, Clone)]
pub struct BattleText<'a> {
    pub lang: Language,
    pub font: Font<'a>,
    pub files: BattleFiles<'a>,
    /// German case templates; `None` for the other languages.
    pub declension: Option<Declension>,
}

impl<'a> BattleText<'a> {
    /// A builder for `lang` and its files. `declension` is German's, from
    /// [`Declension::from_arm9`].
    pub fn new(
        lang: Language,
        font: Font<'a>,
        files: BattleFiles<'a>,
        declension: Option<Declension>,
    ) -> Self {
        Self {
            lang,
            font,
            files,
            declension,
        }
    }

    /// The raw markup of `file`'s message `number`.
    ///
    /// # Errors
    /// [`Error::Malformed`] when the file has no such message.
    pub fn raw(&self, file: File, number: u16) -> Result<&'a [u8]> {
        let text = match file {
            File::Strbtl => self.files.strbtl.message(number),
            File::Actmsg => self.files.actmsg.message(number),
            File::StrBres => self.files.str_bres.message(number),
            File::StrBtl => self.files.str_btl.message(number),
            File::StrTm => self.files.str_tm.message(number),
        };
        text.ok_or(Error::Malformed("no such battle message"))
    }

    /// The text the window draws for `file`'s message `number`: every pass run and the text
    /// tokens joined (a line break is `\n`; control codes draw nothing).
    ///
    /// # Errors
    /// [`Error::Malformed`] when the file has no such message.
    pub fn message(&self, file: File, number: u16, context: &Context<'a>) -> Result<Vec<u8>> {
        let text = self.raw(file, number)?;
        let text = message::expand_values(text, |i| {
            context
                .values
                .get(i as usize)
                .copied()
                .unwrap_or(0)
                .to_string()
                .into_bytes()
        });
        let text = message::normalize(&self.font, &text);
        let text = message::eval_conditionals(&text, |rule| self.branch(context, rule));
        let text = message::expand_names(&text, |name| self.resolve(context, name));
        let text = message::apply_cap(&self.font, &text);
        let (_, text) = message::leading_tags(&text);
        Ok(drawn(&message::compile(text)))
    }

    /// The name record a grammar tag resolves to.
    fn record(&self, context: &Context<'a>, entity: Entity) -> Option<NameRecord<'a>> {
        let fallback = || context.monster.or(context.target);
        match entity {
            Entity::Item => context.item,
            Entity::Monster => fallback(),
            Entity::Actor => context.actor,
            Entity::Target => context.target,
            Entity::Action => context.action,
            Entity::Reflex => context.actor,
        }
    }

    /// The name a vowel or last-letter condition tests.
    fn test_name<'c>(&self, context: &'c Context<'a>, suffix: &str) -> Option<&'c [u8]> {
        let record = match suffix {
            "HERO" | "LEADER" => return Some(&context.hero),
            "I" | "I1" | "I2" => context.item,
            "M" => context.monster.or(context.target),
            "ACTOR" | "DE_ACTOR" => context.actor,
            "TARGET" | "DE_TARGET" => context.target,
            _ => None,
        };
        record.map(|record| record.singular)
    }

    /// One branch of a conditional block. The message's values and state decide it; a rule
    /// the context says nothing about keeps its first branch.
    fn branch(&self, context: &Context<'a>, rule: &Condition) -> u8 {
        let start = rule.start;
        // `IF_SING val_n` / `IF_SING_FR val_n`: the value is one or not.
        if let Some(value) = start
            .strip_prefix("IF_SING VAL_")
            .or_else(|| start.strip_prefix("IF_SING_FR VAL_"))
            && let Ok(index) = value.parse::<usize>()
        {
            return u8::from(context.values.get(index - 1) != Some(&1));
        }
        if start.starts_with("IF_VOWEL") {
            let entity = start
                .trim_start_matches("IF_VOWEL_FR_")
                .trim_start_matches("IF_VOWEL_");
            return match self.test_name(context, entity) {
                Some(name) => u8::from(!message::starts_with_vowel(&self.font, name)),
                None => 0,
            };
        }
        if start.starts_with("IF_LAST_LETTER_S") {
            let entity = start.trim_start_matches("IF_LAST_LETTER_S_");
            return match self.test_name(context, entity) {
                Some(name) => u8::from(name.last() != Some(&b's')),
                None => 0,
            };
        }
        let gender = |record: Option<NameRecord<'a>>| record.map_or(0, |r| r.gender());
        match start {
            "IF_TARGET_SING" => u8::from(context.target_count != 1),
            "IF_TARGET_PARTY" => u8::from(!context.target_party),
            "IF_ACTOR_PARTY" => u8::from(!context.actor_party),
            "IF_SOLO" | "IF_SOLOHOST" | "IF_ALONE" => u8::from(!context.solo),
            "IF_ACTOR_M" => gender(context.actor),
            "IF_TARGET_M" => gender(context.target),
            "IF_M_NAME_M" => gender(context.monster.or(context.target)),
            "IF_I_NAME_M" => gender(context.item),
            "IF_ACTION_M" => gender(context.action),
            "IF_ACTOR_MALE" => u8::from(gender(context.actor) != 0),
            _ => 0,
        }
    }

    /// One name tag of the message.
    fn resolve(&self, context: &Context<'a>, name: Name<'_>) -> Vec<u8> {
        match name {
            Name::Substitution { tag, index } => match tag {
                "HERO" | "LEADER" | "ADDRESSEE" => context.hero.clone(),
                "STR_" => context
                    .strings
                    .get(index as usize)
                    .cloned()
                    .unwrap_or_default(),
                _ => Vec::new(),
            },
            Name::Inflected {
                grammar, entity, ..
            } => match self.record(context, entity) {
                Some(record) => message::inflect(
                    &record,
                    grammar,
                    |number| self.files.articles.message(number),
                    self.declension.as_ref(),
                ),
                None => Vec::new(),
            },
            Name::Weapon | Name::Actor => context
                .actor
                .map(|record| record.singular.to_vec())
                .unwrap_or_default(),
        }
    }
}

/// The visible bytes of a compiled message: text as drawn, line breaks as `\n`, control codes
/// as nothing.
pub fn drawn(tokens: &[Token]) -> Vec<u8> {
    let mut out = Vec::new();
    for token in tokens {
        match token {
            Token::Text(bytes) => out.extend_from_slice(bytes),
            Token::NewLine => out.push(b'\n'),
            Token::Control { .. } => {}
        }
    }
    out
}

/// A monster's name record as the inflection routine reads it.
pub fn monster_name<'a>(record: &monster::NameRecord<'a>) -> NameRecord<'a> {
    NameRecord {
        singular: record.name,
        plural: record.plural,
        grammar: record.grammar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nat(count: u32, record_size: usize, records: &[u8], strings: &[u8]) -> Vec<u8> {
        let mut data = ((strings.len() as u32) << 12 | count)
            .to_le_bytes()
            .to_vec();
        data.extend_from_slice(records);
        data.extend_from_slice(strings);
        assert_eq!(data.len(), 4 + count as usize * record_size + strings.len());
        data
    }

    #[test]
    fn message_tables_read_messages_by_number() {
        let mut records = Vec::new();
        for (number, offset) in [(1u32, 1u32), (2, 10)] {
            records.extend(number.to_le_bytes());
            records.extend(offset.to_le_bytes());
        }
        let data = nat(2, 8, &records, b"\0Attacks.\0Takes damage.\0");
        let table = MessageTable::parse(&data).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(table.message(1), Some(&b"Attacks."[..]));
        assert_eq!(table.message(2), Some(&b"Takes damage."[..]));
        assert_eq!(table.message(3), None);
        assert_eq!(table.numbers().collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn a_bad_message_offset_is_an_error() {
        let mut records = Vec::new();
        records.extend(1u32.to_le_bytes());
        records.extend(2u32.to_le_bytes()); // inside "Attacks."
        let data = nat(1, 8, &records, b"\0Attacks.\0");
        assert!(MessageTable::parse(&data).is_err());
    }

    #[test]
    fn item_names_read_the_grammar_triple_and_the_id() {
        // 16-byte records: name, plural, grammar, id.
        let strings = b"\0slime\0slimes\0";
        let mut record = [0u8; 16];
        record[0..4].copy_from_slice(&1u32.to_le_bytes());
        record[4..8].copy_from_slice(&7u32.to_le_bytes());
        record[8..12].copy_from_slice(&0x0204_1041u32.to_le_bytes());
        record[12..14].copy_from_slice(&22000u16.to_le_bytes());
        let data = nat(1, 16, &record, strings);
        let items = ItemNames::parse(&data).unwrap();
        assert_eq!(items.len(), 1);
        let item = items.find(22000).unwrap();
        assert_eq!((item.name, item.plural), (&b"slime"[..], &b"slimes"[..]));
        assert_eq!(item.name_record().gender(), 2);
        assert_eq!(items.find_name(b"SLIMES").unwrap().id, 22000);
        assert!(items.find(1).is_none());
    }

    #[test]
    fn the_file_names_and_languages_agree() {
        for lang in Language::ALL {
            assert_eq!(Language::from_suffix(lang.suffix()), Some(lang));
            assert!((1..=5).contains(&lang.code()));
        }
        for file in File::ALL {
            assert_eq!(File::from_name(file.name()), Some(file));
            let member = file.member(Language::French);
            assert!(member.ends_with("_fr.nat") || member.ends_with("_fr.bin"));
        }
        assert_eq!(File::StrBres.member(Language::English), "str_bres_en.bin");
    }
}
