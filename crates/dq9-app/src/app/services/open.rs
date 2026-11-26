//! Opening a screen: the field menu, a shop, an inn, a church and the priest's line.

use super::ChurchState;
use super::InnState;
use super::PriestState;
use super::Screen;
use super::Services;
use super::ShopMode;
use super::tables::archive_member;
use super::tables::copy_messages;
use super::tables::item_prices;
use crate::app::status;
use crate::talk::ServiceTag;
use anyhow::Context as _;
use anyhow::Result;
use dq9_core::facility::church::MemberStatus;
use dq9_core::facility::inn;
use dq9_core::facility::inn::Choice as InnChoice;
use dq9_core::party::Party;
use dq9_core::state::DayClock;
use dq9_formats::battle::text::Language;
use dq9_formats::facility::inn_church::Church;
use dq9_formats::facility::inn_church::Inn;
use dq9_formats::facility::inn_church::inn_line;
use dq9_formats::facility::shop::ShopTable;
use nds_rom::Rom;
use std::collections::BTreeMap;

/// The retail `str_in` files carry the three menu labels and the price line. A file without
/// them would otherwise become a free inn with debug labels; the explicit free inn `<INN=15>`
/// is accepted because its price line exists and reads 0.
pub(super) fn validate_inn_messages(messages: &BTreeMap<u16, Vec<u8>>) -> Result<()> {
    for line in [
        inn_line::STAY,
        inn_line::REST,
        inn_line::CANCEL,
        inn_line::PRICE,
    ] {
        match messages.get(&line) {
            Some(text) if !text.is_empty() => {}
            Some(_) => anyhow::bail!("empty inn message {line}"),
            None => anyhow::bail!("missing inn message {line}"),
        }
    }
    Ok(())
}

/// The inn menu's labels for `choices`, from the file's messages 0, 1 and 2.
pub(super) fn inn_labels(messages: &BTreeMap<u16, Vec<u8>>, choices: &[InnChoice]) -> Vec<Vec<u8>> {
    choices
        .iter()
        .map(|choice| {
            let line = match choice {
                InnChoice::Stay => inn_line::STAY,
                InnChoice::Rest => inn_line::REST,
                InnChoice::Cancel => inn_line::CANCEL,
            };
            messages.get(&line).cloned().unwrap_or_default()
        })
        .collect()
}

impl Services {
    /// Opens the field menu (`I`). The ROM tables load once and are kept for later screens.
    pub fn open_menu(&mut self, rom: &[u8], party: &mut Party) -> Result<()> {
        self.ensure_data(rom)?;
        self.ensure_rules(rom)?;
        self.ensure_misc(rom)?;
        self.apply_levels(party);
        self.screen = Screen::Menu;
        self.cursor = 0;
        self.notice = None;
        self.pending = None;
        self.held_item = None;
        Ok(())
    }

    /// Opens the facility a talk line names.
    ///
    /// # Errors
    /// The ROM tables, the shop table, the item prices or the facility's own `str_in`/`str_ch`
    /// file is missing or malformed; nothing is shown then.
    pub fn open(
        &mut self,
        tag: ServiceTag,
        rom: &[u8],
        party: &mut Party,
        clock: &DayClock,
    ) -> Result<()> {
        self.ensure_data(rom)?;
        self.apply_levels(party);
        self.notice = None;
        self.pending = None;
        self.held_item = None;
        self.cursor = 0;
        self.cursor_reset();
        match tag {
            ServiceTag::Shop(number) => self.open_shop(number, rom)?,
            ServiceTag::Inn(number) => self.open_inn(number, rom, clock)?,
            ServiceTag::Church(number) => self.open_church(number, rom, party)?,
        }
        Ok(())
    }

    fn open_shop(&mut self, number: u16, rom: &[u8]) -> Result<()> {
        if self.shops.is_none() {
            let image = Rom::parse(rom).context("parsing the ROM")?;
            let file = image
                .file_by_path("data/bin/menu/shopdata1.bin")
                .context("the shop table")?;
            self.shops = Some(ShopTable::parse(file).context("parsing shopdata1.bin")?);
        }
        if self.prices.is_none() {
            self.prices = Some(item_prices(rom)?);
        }
        let shop = self
            .shops
            .as_ref()
            .and_then(|table| table.shop(number))
            .with_context(|| format!("no shop {number} in shopdata1.bin"))?;
        self.shop_number = shop.number;
        self.shop_mode = ShopMode::Buy;
        self.screen = Screen::Shop;
        Ok(())
    }

    fn open_inn(&mut self, number: u16, rom: &[u8], clock: &DayClock) -> Result<()> {
        let member = Inn::member(number, Language::English)
            .with_context(|| format!("<INN={number}> names no str_in file"))?;
        let archive = Inn::archive(number).context("inn archive")?;
        let bytes = archive_member(rom, &archive, &member)?;
        let inn = Inn::parse(&bytes).with_context(|| format!("parsing {archive}/{member}"))?;
        let messages = copy_messages(inn.numbers(), |number| inn.message(number));
        validate_inn_messages(&messages).with_context(|| format!("{archive}/{member}"))?;
        let choices = inn::menu(clock.phase()).to_vec();
        let labels = inn_labels(&messages, &choices);
        self.inn = Some(InnState {
            messages,
            per_head: inn.price,
            choices,
            labels,
        });
        self.screen = Screen::Inn;
        Ok(())
    }

    fn open_church(&mut self, number: u16, rom: &[u8], party: &mut Party) -> Result<()> {
        self.ensure_rules(rom)?;
        let member = Church::member(number, Language::English)
            .with_context(|| format!("<CHURCH={number}> names no str_ch file"))?;
        let archive = Church::archive(number).context("church archive")?;
        let bytes = archive_member(rom, &archive, &member)?;
        let church =
            Church::parse(&bytes).with_context(|| format!("parsing {archive}/{member}"))?;
        let services = church.services();
        let messages = copy_messages(church.numbers(), |number| church.message(number));
        self.church = Some(ChurchState {
            messages,
            services,
            cursor: 0,
            selected: None,
            cure: None,
            member: 0,
            confirm: 0,
        });
        if self.statuses.len() != party.members.len() {
            self.statuses
                .resize(party.members.len(), MemberStatus::default());
        }
        self.screen = Screen::Church;
        Ok(())
    }

    /// The church in mode 2, after a wipe-out: `func_ov017_0219bfb4` hands overlay 3 the
    /// revival map and the voice `chur_messet.bin` gave; state 10 says line 1082 of
    /// `str_ch<voice - 1>` and state 11 ends the visit. Voice 0 and 1 both use `str_ch0`.
    ///
    /// # Errors
    /// The voice's archive or member is missing, or the file has no line 1082.
    pub fn open_priest(&mut self, rom: &[u8], voice: u8) -> Result<()> {
        let file = dq9_core::facility::church::wipe_out_voice_file(voice)
            .context("voice 3 says nothing")?;
        let tag = u16::from(file) + 1;
        let member = Church::member(tag, Language::English)
            .with_context(|| format!("voice {voice} names no str_ch file"))?;
        let archive = Church::archive(tag).context("church archive")?;
        let bytes = archive_member(rom, &archive, &member)?;
        let church =
            Church::parse(&bytes).with_context(|| format!("parsing {archive}/{member}"))?;
        let text = church
            .message(dq9_core::facility::church::WIPE_OUT_LINE)
            .with_context(|| format!("{archive}/{member} has no line 1082"))?
            .to_vec();
        self.close();
        status(&format!(
            "wipe-out: voice {voice} of {archive}/{member}, line {}",
            dq9_core::facility::church::WIPE_OUT_LINE
        ));
        self.priest = Some(PriestState { text });
        self.screen = Screen::Priest;
        Ok(())
    }

    /// The priest's line while [`Screen::Priest`] is open (tests).
    #[cfg(test)]
    pub fn priest_line(&self) -> Option<&[u8]> {
        self.priest.as_ref().map(|priest| priest.text.as_slice())
    }
}
