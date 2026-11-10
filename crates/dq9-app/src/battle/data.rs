//! The ROM tables a played battle reads, decompressed once and owned: the monsters' battle
//! records and names, the action tables, the message files, the font, the equipment stats and
//! the level tables. `dq9_core::battle` and `dq9_formats::battle::text` borrow from members,
//! so [`Data`] hands out borrowing views ([`Data::tables`], [`Data::text`]) per use.
//!
//! Everything is the English edition (the application's language, as `talk.rs`).

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use dq9_core::battle::{ActionTables, Tables};
use dq9_formats::battle::equipment::BattleParams;
use dq9_formats::battle::monster::{BattleData, Names};
use dq9_formats::battle::skill::SkillTable;
use dq9_formats::battle::spell::SpellTable;
use dq9_formats::battle::text::{
    BattleFiles, BattleResult, BattleText, ItemNames, Language, MessageTable,
};
use dq9_formats::battle::{action, spell};
use dq9_formats::item::ItemStats;
use dq9_formats::level::{LevelTables, VOCATION_COUNT};
use dq9_formats::{Font, FontId, Gpc2};
use nds_rom::Rom;

/// The language of the battle text.
pub const LANGUAGE: Language = Language::English;

/// The `actmsg` lines of one action and what the menus show of it (`docs/re/battle-text.md`,
/// "Which message an action says"; wiki *Ability-Handlers*, "The record's lines").
///
/// `+0x20` bits 0–9 open the action at a party target and 10–19 at a monster, bits 20–29 say
/// what it did at a party target; `+0x24` bits 0–9 what it did at a monster, 10–19 and 20–29
/// its failure at a party member and at a monster; `+0x28` bits 0–9 and 10–19 the kill at a
/// party member and at a monster, 20–29 the critical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionLines {
    pub name: Vec<u8>,
    pub mp_cost: u8,
    /// The action's kind (12: a call for backup).
    pub kind: u8,
    pub opening: [u16; 2],
    pub done: [u16; 2],
    pub failed: [u16; 2],
    pub killed: [u16; 2],
    pub critical: u16,
}

impl ActionLines {
    fn new(record: &action::Action<'_>) -> Self {
        let bits = |word: u32, shift: u32| ((word >> shift) & 0x3ff) as u16;
        // Index 0 is a party target, 1 a monster.
        Self {
            name: record.name.to_vec(),
            mp_cost: record.mp_cost,
            kind: record.kind,
            opening: [bits(record.word_20, 0), bits(record.word_20, 10)],
            done: [bits(record.word_20, 20), bits(record.word_24, 0)],
            failed: [bits(record.word_24, 10), bits(record.word_24, 20)],
            killed: [bits(record.word_28, 0), bits(record.word_28, 10)],
            critical: bits(record.word_28, 20),
        }
    }
}

/// The item record field the battle consumes by: the "used up" bit (`+0x08` bit 19), the bit
/// the AI scores an item with (`func_ov024_021f7478`) and the battle's item handler removes the
/// held slot for (`ov_017::021d0164`). The same record word's low nibble is the category
/// measured in `docs/re/item.md` (0–7 equipment, 8–9 tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemMeta {
    pub used_up: bool,
}

/// The owned ROM members.
#[derive(Debug)]
pub struct Data {
    monsters: Vec<u8>,
    names: Vec<u8>,
    actions_a: Vec<u8>,
    actions_b: Vec<u8>,
    ranges_a: Vec<u8>,
    ranges_b: Vec<u8>,
    font_info: Vec<u8>,
    font_data: Vec<u8>,
    strbtl: Vec<u8>,
    actmsg: Vec<u8>,
    str_bres: Vec<u8>,
    str_btl: Vec<u8>,
    str_tm: Vec<u8>,
    articles: Vec<u8>,
    item_names: Vec<u8>,
    /// What the battle says of every action, by number.
    pub lines: BTreeMap<u16, ActionLines>,
    /// An item's field and battle actions (252 for none), by item id.
    pub item_actions: BTreeMap<u16, [u16; 2]>,
    /// The item record's "used up" flag, by item id (the word's low nibble is its category,
    /// read by the equipment rules, not stored here).
    pub item_meta: BTreeMap<u16, ItemMeta>,
    /// Which vocation learns which spell at which level.
    pub spells: SpellTable,
    /// Which skill panel teaches which action, and at what cost.
    pub skills: SkillTable,
    /// The worn items' stats (attack, defence, …).
    pub items: ItemStats,
    /// `itembtlprm.nat`: resistances and weapon elements.
    pub params: BattleParams,
    /// The thirteen vocations' level tables.
    pub levels: LevelTables,
}

fn member(rom: &Rom<'_>, path: &str, name: &str) -> Result<Vec<u8>> {
    let archive = Gpc2::parse(rom.file_by_path(path).with_context(|| path.to_owned())?)
        .with_context(|| format!("parsing {path}"))?;
    let entry = archive
        .find(name)
        .with_context(|| format!("{name} is not in {path}"))?;
    Ok(archive
        .member(entry)
        .with_context(|| format!("{name} of {path}"))?
        .into_owned())
}

impl Data {
    /// Reads and decompresses everything a battle needs.
    ///
    /// # Errors
    /// A missing or malformed member, named.
    pub fn load(rom: &[u8]) -> Result<Self> {
        let rom = Rom::parse(rom).context("parsing the ROM")?;
        let suffix = LANGUAGE.suffix();
        let font_name = FontId::Message.name();
        let font_info = rom
            .file_by_path(&format!("data/pack_lv5/fi_{font_name}.bin"))?
            .to_vec();
        let font_data = rom
            .file_by_path(&format!("data/pack_lv5/fd_{font_name}.bin"))?
            .to_vec();
        let actions_a = member(
            &rom,
            "data/prm/actdt_a.gp2",
            &format!("actdt_a_{suffix}.nat"),
        )?;
        let actions_b = member(
            &rom,
            "data/prm/actdt_b.gp2",
            &format!("actdt_b_{suffix}.nat"),
        )?;
        let mut lines = BTreeMap::new();
        for half in [&actions_a, &actions_b] {
            for record in &action::Actions::parse(half)
                .context("parsing an action table")?
                .actions
            {
                lines
                    .entry(record.number)
                    .or_insert_with(|| ActionLines::new(record));
            }
        }
        let level_files: Vec<&[u8]> = (0..VOCATION_COUNT)
            .map(|v| rom.file_by_path(&format!("data/prm/level{v}.bin")))
            .collect::<Result<_, _>>()
            .context("reading the level tables")?;
        let level_files: [&[u8]; VOCATION_COUNT] = level_files
            .try_into()
            .map_err(|_| anyhow::anyhow!("thirteen level tables"))?;
        let item_members = ["a", "b", "d", "h", "l", "s", "u", "w"]
            .iter()
            .map(|c| {
                member(
                    &rom,
                    &format!("data/prm/itemdt_{c}.gp2"),
                    &format!("itemdt_{c}_en.nat"),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let item_members: Vec<&[u8]> = item_members.iter().map(Vec::as_slice).collect();
        // What using an item does: two action numbers a record (wiki *Items*, "The record's
        // layout" and "The two actions"; the field's first, the battle's second INFERRED), read
        // from every category, the tools included.
        let mut item_actions = BTreeMap::new();
        let mut item_meta = BTreeMap::new();
        let tools = member(
            &rom,
            "data/prm/itemdt_t.gp2",
            &format!("itemdt_t_{ITEM_SUFFIX}.nat"),
        )?;
        for table in item_members
            .iter()
            .copied()
            .chain(std::iter::once(tools.as_slice()))
        {
            read_item_records(table, &mut item_actions, &mut item_meta);
        }
        Ok(Self {
            monsters: rom.file_by_path("data/prm/mon_btldata.nat")?.to_vec(),
            names: member(
                &rom,
                "data/prm/mon_data.gp2",
                &format!("mon_data_{suffix}.nat"),
            )?,
            ranges_a: member(&rom, "data/prm/actdt_a.gp2", "actdamage_a.nat")?,
            ranges_b: member(&rom, "data/prm/actdt_b.gp2", "actdamage_b.nat")?,
            actions_a,
            actions_b,
            font_info,
            font_data,
            strbtl: member(&rom, "data/bin/strbtl.gp2", &format!("strbtl_{suffix}.nat"))?,
            actmsg: member(&rom, "data/prm/actmsg.gp2", &format!("actmsg_{suffix}.nat"))?,
            str_bres: member(
                &rom,
                "data/bin/str_bres.gp2",
                &format!("str_bres_{suffix}.bin"),
            )?,
            str_btl: member(
                &rom,
                "data/bin/menu/str_btl.gp2",
                &format!("str_btl_{suffix}.nat"),
            )?,
            str_tm: member(
                &rom,
                "data/bin/menu/str_tm.gp2",
                &format!("str_tm_{suffix}.nat"),
            )?,
            articles: member(
                &rom,
                "data/prm/article.gp2",
                &format!("article_{suffix}.nat"),
            )?,
            item_names: member(
                &rom,
                "data/prm/itemname.gp2",
                &format!("itemname_{suffix}.nat"),
            )?,
            lines,
            item_actions,
            item_meta,
            spells: SpellTable::parse(rom.file_by_path("data/prm/spelltable.bin")?)
                .context("parsing spelltable.bin")?,
            skills: SkillTable::parse(rom.file_by_path("data/prm/skilltable.bin")?)
                .context("parsing skilltable.bin")?,
            items: ItemStats::from_members(&item_members).context("parsing the item stats")?,
            params: BattleParams::parse(rom.file_by_path("data/prm/itembtlprm.nat")?)
                .context("parsing itembtlprm.nat")?,
            levels: LevelTables::parse(level_files).context("parsing the level tables")?,
        })
    }

    /// The tables `BattleSetup` is built from.
    pub fn tables(&self) -> Result<Tables<'_>> {
        Ok(Tables {
            monsters: BattleData::parse(&self.monsters).context("mon_btldata")?,
            names: self.monster_names()?,
            actions: ActionTables::parse(
                &self.actions_a,
                &self.actions_b,
                &self.ranges_a,
                &self.ranges_b,
            )
            .context("the action tables")?,
        })
    }

    /// The monsters' name records.
    pub fn monster_names(&self) -> Result<Names<'_>> {
        Names::parse(&self.names).context("mon_data names")
    }

    /// The items' name records.
    pub fn item_names(&self) -> Result<ItemNames<'_>> {
        ItemNames::parse(&self.item_names).context("itemname")
    }

    /// The message builder: the six passes of `Msg_Prepare` with the battle's names.
    pub fn text(&self) -> Result<BattleText<'_>> {
        let font = Font::parse(&self.font_info, &self.font_data, FontId::Message)
            .context("parsing the message font")?;
        let files = BattleFiles {
            strbtl: MessageTable::parse(&self.strbtl).context("strbtl")?,
            actmsg: MessageTable::parse(&self.actmsg).context("actmsg")?,
            str_bres: BattleResult::parse(&self.str_bres).context("str_bres")?,
            str_btl: MessageTable::parse(&self.str_btl).context("str_btl")?,
            str_tm: MessageTable::parse(&self.str_tm).context("str_tm")?,
            articles: MessageTable::parse(&self.articles).context("articles")?,
        };
        Ok(BattleText::new(LANGUAGE, font, files, None))
    }

    /// The spells `vocation` knows at `level`: the learned entries of `spelltable.bin` up to
    /// it, as action numbers. Spells come from the vocation and level; the skill panels'
    /// abilities are [`Data::skills_known`].
    pub fn spells_known(&self, vocation: u8, level: u8) -> Vec<u16> {
        let mut known: Vec<(u8, u16)> = self
            .spells
            .learned
            .iter()
            .filter(|l: &&spell::Learned| l.vocation == vocation && l.level <= level)
            .filter_map(|l| {
                let action = self
                    .spells
                    .spells
                    .iter()
                    .find(|s| s.place == l.place)?
                    .action;
                (action != 0).then_some((l.level, action))
            })
            .collect();
        known.sort_unstable();
        known.into_iter().map(|(_, action)| action).collect()
    }

    /// The abilities the panel bits in `panels` teach, as action numbers, in panel order.
    ///
    /// The bits are the character record's `+0x11A` array
    /// ([`Character::skill_panels`](dq9_core::party::Character::skill_panels)), one per panel;
    /// a panel with no action (a stat or a message alone) offers nothing
    /// (`docs/re/battle-data.md`, "Skill panels").
    pub fn skills_known(&self, panels: &[u8]) -> Vec<u16> {
        self.skills
            .panels
            .iter()
            .filter(|panel| panel.action != 0)
            .filter(|panel| {
                let place = usize::from(panel.place);
                panels
                    .get(place / 8)
                    .is_some_and(|byte| byte >> (place % 8) & 1 != 0)
            })
            .map(|panel| panel.action)
            .collect()
    }
}

/// The suffix of the item tables' members (`itemdt_<category>_en.nat`).
const ITEM_SUFFIX: &str = "en";
/// The action number that stands for "nothing" in an item's record.
pub const NO_ACTION: u16 = 252;

/// Reads the two actions and the category/used-up word of every record of one category
/// table: `u16` ×2 at `+0x14` and the id at `+0x18` of the reader's 32-byte record, the word
/// at `+0x08` (low nibble category, bit 19 used up), as `dq9_formats::item` lays the records
/// out.
fn read_item_records(
    table: &[u8],
    out: &mut BTreeMap<u16, [u16; 2]>,
    meta: &mut BTreeMap<u16, ItemMeta>,
) {
    let u16_at = |at: usize| {
        table
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let Some(count) = u16_at(0) else { return };
    for index in 0..usize::from(count) {
        let at = 12 + index * 32;
        let (Some(id), Some(field), Some(battle), Some(word)) = (
            u16_at(at + 0x18),
            u16_at(at + 0x14),
            u16_at(at + 0x16),
            table
                .get(at + 8..at + 12)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        ) else {
            return;
        };
        out.entry(id).or_insert([field, battle]);
        meta.entry(id).or_insert(ItemMeta {
            used_up: (word >> 19) & 1 == 1,
        });
    }
}

impl Data {
    /// The action using `item` does in battle: the record's second number, when it names one.
    pub fn battle_action(&self, item: u16) -> Option<u16> {
        let [_, battle] = *self.item_actions.get(&item)?;
        (battle != NO_ACTION && self.lines.contains_key(&battle)).then_some(battle)
    }
}
