//! The ROM tables the facility screens read: prices, item rules and the message tables.

use super::ItemRule;
use anyhow::Context as _;
use anyhow::Result;
use dq9_formats::Gpc2;
use dq9_formats::battle::text::Language;
use dq9_formats::battle::text::MessageTable;
use dq9_formats::facility::shop::ItemPrices;
use nds_rom::Rom;
use std::collections::BTreeMap;

/// The item price tables of every category (`itemdt_<category>_<language>.nat`).
pub(super) fn item_prices(rom: &[u8]) -> Result<ItemPrices> {
    let mut prices = ItemPrices::default();
    for category in ["a", "b", "d", "h", "l", "s", "u", "w", "t"] {
        let path = format!("data/prm/itemdt_{category}.gp2");
        let member = format!("itemdt_{category}_{}.nat", Language::English.suffix());
        let bytes = archive_member(rom, &path, &member)?;
        prices
            .add_member(&bytes)
            .with_context(|| format!("parsing {path}/{member}"))?;
    }
    Ok(prices)
}

/// The item records' category, cursed bit and vocation mask, over every category table
/// (`itemdt_<category>_<language>.nat`).
pub(super) fn item_rules(rom: &[u8]) -> Result<BTreeMap<u16, ItemRule>> {
    let mut rules = BTreeMap::new();
    for category in ["a", "b", "d", "h", "l", "s", "u", "w", "t"] {
        let path = format!("data/prm/itemdt_{category}.gp2");
        let member = format!("itemdt_{category}_{}.nat", Language::English.suffix());
        let bytes = archive_member(rom, &path, &member)?;
        add_item_rules(&bytes, &mut rules).with_context(|| format!("parsing {path}/{member}"))?;
    }
    Ok(rules)
}

/// One category member: a `u16` count, then 32-byte records (the stats index at `+0x00`, the
/// category/cursed word at `+0x08`, the id as `s16` at `+0x18`), then one 32-byte stats entry
/// per record (`0xffffffff` on the tools, which have none).
fn add_item_rules(data: &[u8], rules: &mut BTreeMap<u16, ItemRule>) -> Result<()> {
    let head = data
        .get(0..2)
        .context("a category table shorter than its count")?;
    let count = usize::from(u16::from_le_bytes([head[0], head[1]]));
    let records_end = 12 + count * 32;
    anyhow::ensure!(
        data.len() >= records_end,
        "a category table shorter than its {count} records"
    );
    for index in 0..count {
        let at = 12 + index * 32;
        let record = &data[at..at + 32];
        let id = i16::from_le_bytes([record[0x18], record[0x19]]);
        if id <= 0 {
            continue;
        }
        let word = |bytes: &[u8], offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        let (category, stats) = (word(record, 0x08), word(record, 0x00) as usize);
        let vocations = stats
            .checked_mul(32)
            .and_then(|offset| records_end.checked_add(offset))
            .and_then(|at| at.checked_add(8).and_then(|end| data.get(at..end)))
            .map_or(0, |entry| (word(entry, 4) & 0xfff) as u16);
        rules.entry(id as u16).or_insert(ItemRule {
            category: (category & 0xf) as u8,
            cursed: (category >> 18) & 1 == 1,
            vocations,
        });
    }
    Ok(())
}

/// The equipment screen's words: `data/bin/menu/str_eq.gp2`, member `str_eq_<language>.nat`.
pub(super) fn equipment_messages(rom: &[u8]) -> Result<BTreeMap<u16, Vec<u8>>> {
    const ARCHIVE: &str = "data/bin/menu/str_eq.gp2";
    let member = format!("str_eq_{}.nat", Language::English.suffix());
    let bytes = archive_member(rom, ARCHIVE, &member)?;
    let table =
        MessageTable::parse(&bytes).with_context(|| format!("parsing {ARCHIVE}/{member}"))?;
    Ok(copy_messages(table.numbers(), |number| {
        table.message(number)
    }))
}

/// One member of a `.gp2` archive, decompressed and owned.
pub(super) fn archive_member(rom: &[u8], archive: &str, member: &str) -> Result<Vec<u8>> {
    let image = Rom::parse(rom).context("parsing the ROM")?;
    let data = image
        .file_by_path(archive)
        .with_context(|| archive.to_owned())?;
    let archive = Gpc2::parse(data).with_context(|| format!("parsing {archive}"))?;
    let entry = archive
        .find(member)
        .with_context(|| format!("{member} is not in {archive:?}"))?;
    Ok(archive
        .member(entry)
        .with_context(|| member.to_owned())?
        .into_owned())
}

pub(super) fn copy_messages<'a>(
    numbers: impl Iterator<Item = u16>,
    message: impl Fn(u16) -> Option<&'a [u8]>,
) -> BTreeMap<u16, Vec<u8>> {
    numbers
        .filter_map(|number| message(number).map(|bytes| (number, bytes.to_vec())))
        .collect()
}
