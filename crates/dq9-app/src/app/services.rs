//! The field menu and the facilities a talk line opens: items, equipment, attributes, the
//! learned healing spells, the inn, the shop and the church.
//!
//! The rules are the ones already written in `dq9_core::facility` (prices and effects) and
//! `dq9_core::party` (the bag, held slots, equipment); this module is the application side:
//! the ROM tables the windows read, the cursors and the transfer of what a command does into
//! the party. The windows are laid out in the field message window's geometry
//! (`crate::talk::WINDOW`) on the screen the field does not use; the real overlay's layout is
//! not read, so every position here is a **[hypothesis]** (`docs/re/facility.md`).
//!
//! A talk line's `<INN=n>`, `<CHURCH=n>` or `<SHOP=n>` tag ([`crate::talk::ServiceTag`]) opens
//! the screen of that number once the message window closes. The conversation's end is held
//! back until the screen is over ([`crate::talk::Talk::finish_service`]), so trigger event 1
//! and the talk counters keep the order the game's talk task gives them.
//!
//! Using an item or a spell runs the game's action on a party member with
//! `dq9_core::battle::resolve_action`: the same range table, the same MP cost and the same
//! healing cap as a battle. An item is taken from the source the player chose (a bag stack or
//! the held slot) only when the action executed, as the battle's `ItemUsed` rule does.

mod transfer;

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use dq9_core::battle::{Random, Reach, resolve_action};
use dq9_core::facility::church::{self, ChurchError, MemberStatus, Service};
use dq9_core::facility::inn::{self, Choice as InnChoice};
use dq9_core::facility::shop;
use dq9_core::party::{HELD_ITEMS, Party};
use dq9_core::state::DayClock;
use dq9_core::talk::Names;
use dq9_formats::battle::text::Language;
use dq9_formats::facility::inn_church::{Church, Inn, church_line, inn_line};
use dq9_formats::facility::shop::{ItemPrices, ShopTable};
use dq9_formats::save::EquipSlot;
use dq9_formats::{Font, Gpc2};
use dq9_render::ui::{Atlas, Quad};
use nds_rom::Rom;

use crate::app::status;
use crate::battle::data::{Data, NO_ACTION};
use crate::talk::{Button, ServiceTag};

pub use transfer::{bag_to_held, equip, equip_slot_of, held_to_bag};

/// The screen of the 256×192 DS display the menu uses **[hypothesis]**: the field's message
/// window rectangle, grown to the screen.
const WINDOW: [i32; 4] = [4, 4, 248, 184];
const MARGIN: [i32; 2] = [12, 10];
const ROW: i32 = 18;
const VISIBLE_ROWS: usize = 7;
const WHITE: [f32; 4] = [1.0; 4];
const GREY: [f32; 4] = [0.55, 0.55, 0.6, 1.0];

/// What a pressed command changed outside the menu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Effect {
    #[default]
    None,
    /// The inn moved the day clock: the application follows with its lighting.
    Clock,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Screen {
    #[default]
    Closed,
    Menu,
    Items,
    HeldSlots,
    Equip,
    EquipChoice(EquipSlot),
    Stats,
    Spells,
    Targets,
    Shop,
    Inn,
    Church,
    ChurchMembers,
    ChurchConfirm,
    Notice,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ShopMode {
    #[default]
    Buy,
    Sell,
}

/// Where an item a command uses comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Bag(u16),
    Held(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Item(Source),
    Spell { action: u16, cost: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeNext {
    /// Back to what was on screen.
    Return,
    /// After an inn's paid stay: line 1011, then the screen closes.
    Rested,
    /// Nothing more: the facility closes.
    Close,
}

#[derive(Debug, Clone)]
struct Notice {
    /// The line of the facility's file, and the `<val_n>` values it wants.
    text: Vec<u8>,
    values: Vec<(u8, u32)>,
    next: NoticeNext,
    return_screen: Screen,
    /// The member a church line's `<Target0>` names, when it has one.
    target: Option<Vec<u8>>,
    /// The vocation label a divination line's `<str_2>` names, when the church file has it.
    vocation: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct InnState {
    /// `<INN=n>`'s file, by message number.
    messages: BTreeMap<u16, Vec<u8>>,
    /// The price a head (message 10000).
    per_head: u32,
    /// The choices the labels name, frozen together with them so a row cannot mean another.
    choices: Vec<InnChoice>,
    /// The labels the menu offers at the current time of day (Stay, Rest, Cancel by day;
    /// Stay and Cancel at night).
    labels: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct ChurchState {
    /// `<CHURCH=n>`'s file, by message number.
    messages: BTreeMap<u16, Vec<u8>>,
    /// The services whose label is not empty, in menu order.
    services: Vec<Service>,
    /// The service row the church menu was on, restored when a notice returns.
    cursor: usize,
    /// The service the member list is asking about.
    selected: Option<Service>,
    /// The cure being paid for, with the member it is asked of.
    cure: Option<(Service, usize, u32)>,
    member: usize,
    confirm: usize,
}

/// The field menu, the facility screen and their state. One instance lives in
/// [`App`](crate::app::App); a map change or a replaced session closes it.
#[derive(Debug, Default)]
pub struct Services {
    data: Option<Box<Data>>,
    prices: Option<ItemPrices>,
    shops: Option<ShopTable>,
    screen: Screen,
    cursor: usize,
    /// The bag item waiting for its held slot.
    held_item: Option<u16>,
    pending: Option<Pending>,
    /// The screen a target list returns to.
    target_return: Screen,
    shop_mode: ShopMode,
    shop_number: u16,
    inn: Option<InnState>,
    church: Option<ChurchState>,
    notice: Option<Notice>,
    statuses: Vec<MemberStatus>,
    look_dirty: bool,
    /// The party members' names as the font draws them, set by the application.
    names: Vec<Vec<u8>>,
}

impl Services {
    /// The menu is showing (the field must not move).
    pub fn is_open(&self) -> bool {
        self.screen != Screen::Closed
    }

    /// Closes whatever is open; the application then ends the facility conversation.
    pub fn close(&mut self) {
        self.screen = Screen::Closed;
        self.notice = None;
        self.pending = None;
        self.held_item = None;
        self.cursor = 0;
        self.cursor_reset();
    }

    fn cursor_reset(&mut self) {
        self.shop_mode = ShopMode::Buy;
    }

    /// Whether equipment changed while the menu was open.
    pub fn look_dirty(&self) -> bool {
        self.look_dirty
    }

    pub fn clear_look_dirty(&mut self) {
        self.look_dirty = false;
    }

    /// The names the rows print, from the application (`Hero`, then the saved names).
    pub fn set_names(&mut self, names: Vec<Vec<u8>>) {
        self.names = names;
    }

    fn name(&self, index: usize) -> Vec<u8> {
        self.names
            .get(index)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Member {}", index + 1).into_bytes())
    }

    fn ensure_data(&mut self, rom: &[u8]) -> Result<()> {
        if self.data.is_none() {
            self.data = Some(Box::new(Data::load(rom)?));
        }
        Ok(())
    }

    fn item_name(&self, item: u16) -> Vec<u8> {
        let Some(data) = &self.data else {
            return format!("#{item}").into_bytes();
        };
        data.item_names()
            .ok()
            .and_then(|names| names.find(item).map(|record| record.name.to_vec()))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("#{item}").into_bytes())
    }

    fn apply_levels(&self, party: &mut Party) {
        let Some(data) = &self.data else {
            return;
        };
        for member in &mut party.members {
            // `Character::apply_level_tables` fills a new game's empty maxima and calls a
            // member with 0 HP fresh; a party member a battle left at 0 HP keeps it, and the
            // wounds a menu opened on stay as they are.
            let fresh = member.max_hp == 0 && member.hp == 0;
            let (hp, mp) = (member.hp, member.mp);
            member.apply_level_tables(&data.levels);
            if !fresh {
                member.hp = hp;
                member.mp = mp;
            }
        }
    }

    /// Opens the field menu (`I`). The ROM tables load once and are kept for later screens.
    pub fn open_menu(&mut self, rom: &[u8], party: &mut Party) -> Result<()> {
        self.ensure_data(rom)?;
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

    /// The item at the cursor when the screen is a list of the bag and the held slots.
    fn item_source(&self, party: &Party) -> Option<Source> {
        self.item_sources(party).get(self.cursor).copied()
    }

    /// The rows of the item screen in order: the bag's non-empty slots, then the leader's
    /// eight held slots.
    fn item_sources(&self, party: &Party) -> Vec<Source> {
        let mut out = Vec::new();
        for stack in party.bag.slots() {
            if let Some(item) = stack.item {
                out.push(Source::Bag(item));
            }
        }
        if let Some(member) = party.members.first() {
            for (slot, item) in member.held_items.iter().enumerate() {
                if item.is_some() {
                    out.push(Source::Held(slot));
                }
            }
        }
        out
    }

    fn bag_rows(&self, party: &Party) -> usize {
        party
            .bag
            .slots()
            .iter()
            .filter(|s| s.item.is_some())
            .count()
    }

    /// Advances a notice without input: back to its screen, the inn's 1011 line, or the
    /// facility's close.
    fn finish_notice(&mut self, notice: Notice) {
        self.notice = None;
        match notice.next {
            NoticeNext::Return => {
                self.screen = notice.return_screen;
                if self.screen == Screen::Church {
                    self.cursor = self.church.as_ref().map_or(0, |church| church.cursor);
                }
            }
            NoticeNext::Rested => {
                if let Some(inn) = &self.inn {
                    let text = inn
                        .messages
                        .get(&inn_line::RESTED)
                        .cloned()
                        .unwrap_or_default();
                    self.notice = Some(Notice {
                        text,
                        values: Vec::new(),
                        target: None,
                        vocation: None,
                        next: NoticeNext::Close,
                        return_screen: Screen::Inn,
                    });
                } else {
                    self.close();
                }
            }
            NoticeNext::Close => self.close(),
        }
    }

    /// A command key (confirm, cancel, up, down). Left/right reach [`Services::side`].
    pub fn press(
        &mut self,
        button: Button,
        party: &mut Party,
        rng: &mut Random,
        clock: &mut DayClock,
    ) -> Effect {
        if let Some(notice) = self.notice.clone() {
            if matches!(button, Button::Confirm) {
                self.finish_notice(notice);
                return Effect::None;
            }
            if matches!(button, Button::Cancel) {
                self.notice = None;
                match notice.next {
                    // Back to the screen the notice belongs to.
                    NoticeNext::Return => {
                        self.screen = notice.return_screen;
                        if self.screen == Screen::Church {
                            self.cursor = self.church.as_ref().map_or(0, |church| church.cursor);
                        }
                    }
                    // The paid/closed service has no menu to return to: X advances or closes
                    // exactly as Confirm does, so a cached menu cannot be reached.
                    NoticeNext::Rested | NoticeNext::Close => self.finish_notice(notice),
                }
                return Effect::None;
            }
            return Effect::None;
        }
        match self.screen {
            Screen::Closed | Screen::Notice => Effect::None,
            Screen::Menu => {
                let rows = 5;
                match button {
                    Button::Up => self.cursor = (self.cursor + rows - 1) % rows,
                    Button::Down => self.cursor = (self.cursor + 1) % rows,
                    Button::Confirm => match self.cursor {
                        0 => {
                            self.cursor = 0;
                            self.screen = Screen::Items;
                        }
                        1 => {
                            self.cursor = 0;
                            self.screen = Screen::Equip;
                        }
                        2 => {
                            self.cursor = 0;
                            self.screen = Screen::Stats;
                        }
                        3 => {
                            self.cursor = 0;
                            self.screen = Screen::Spells;
                        }
                        _ => self.close(),
                    },
                    Button::Cancel => self.screen = Screen::Menu,
                }
                Effect::None
            }
            Screen::Items => {
                let rows = self.item_sources(party).len().max(1);
                match button {
                    Button::Up => self.cursor = (self.cursor + rows - 1) % rows,
                    Button::Down => self.cursor = (self.cursor + 1) % rows,
                    Button::Cancel => self.screen = Screen::Menu,
                    Button::Confirm => {
                        if party.members.first().is_none_or(|member| member.hp == 0) {
                            self.show_text("You cannot act.");
                        } else if let Some(source) = self.item_source(party) {
                            if let Some(action) = self.item_action(party, source) {
                                let reach = self.action_reach(action).unwrap_or(Reach::One);
                                if matches!(reach, Reach::All | Reach::Group | Reach::Actor)
                                    && heal_targets(reach, 0, party).is_empty()
                                {
                                    self.show_text("No one can be healed.");
                                } else {
                                    self.pending = Some(Pending::Item(source));
                                    self.target_return = Screen::Items;
                                    self.cursor = 0;
                                    if matches!(reach, Reach::All | Reach::Group | Reach::Actor) {
                                        if let Some(pending) = self.pending.take() {
                                            self.apply(pending, 0, party, rng);
                                        }
                                        self.screen = Screen::Items;
                                    } else {
                                        self.screen = Screen::Targets;
                                    }
                                }
                            } else {
                                self.show_text("This item is not used from the field.");
                            }
                        }
                    }
                }
                Effect::None
            }
            Screen::HeldSlots => {
                match button {
                    Button::Up => self.cursor = (self.cursor + HELD_ITEMS - 1) % HELD_ITEMS,
                    Button::Down => self.cursor = (self.cursor + 1) % HELD_ITEMS,
                    Button::Cancel => self.screen = Screen::Items,
                    Button::Confirm => {
                        if let Some(item) = self.held_item
                            && bag_to_held(party, item, self.cursor)
                        {
                            status(&format!(
                                "item {item}: bag -> held slot {}, bag {} left",
                                self.cursor,
                                party.bag.count(item)
                            ));
                            self.held_item = None;
                            self.screen = Screen::Items;
                        }
                    }
                }
                Effect::None
            }
            Screen::Equip => {
                match button {
                    Button::Up => self.cursor = (self.cursor + 7) % 8,
                    Button::Down => self.cursor = (self.cursor + 1) % 8,
                    Button::Cancel => self.screen = Screen::Menu,
                    Button::Confirm => {
                        self.screen = Screen::EquipChoice(SLOTS[self.cursor].0);
                        self.cursor = 0;
                    }
                }
                Effect::None
            }
            Screen::EquipChoice(slot) => {
                let choices = self.equip_choices(party, slot);
                let rows = choices.len().max(1);
                match button {
                    Button::Up => self.cursor = (self.cursor + rows - 1) % rows,
                    Button::Down => self.cursor = (self.cursor + 1) % rows,
                    Button::Cancel => {
                        self.cursor = SLOTS
                            .iter()
                            .position(|(candidate, _)| *candidate == slot)
                            .unwrap_or(0);
                        self.screen = Screen::Equip;
                    }
                    Button::Confirm => {
                        let item = choices.get(self.cursor).and_then(|choice| *choice);
                        if equip(party, slot, item) {
                            status(&format!(
                                "equip {slot:?}: {:?} (bag {} copies)",
                                item,
                                item.map_or(0, |item| party.bag.count(item))
                            ));
                            self.look_dirty = true;
                        }
                        self.screen = Screen::Equip;
                        self.cursor = SLOTS
                            .iter()
                            .position(|(candidate, _)| *candidate == slot)
                            .unwrap_or(0);
                    }
                }
                Effect::None
            }
            Screen::Stats => {
                let rows = party.members.len().max(1);
                match button {
                    Button::Up => self.cursor = (self.cursor + rows - 1) % rows,
                    Button::Down => self.cursor = (self.cursor + 1) % rows,
                    Button::Cancel | Button::Confirm => self.screen = Screen::Menu,
                }
                Effect::None
            }
            Screen::Spells => {
                let rows = self.spells(party).len().max(1);
                match button {
                    Button::Up => self.cursor = (self.cursor + rows - 1) % rows,
                    Button::Down => self.cursor = (self.cursor + 1) % rows,
                    Button::Cancel => self.screen = Screen::Menu,
                    Button::Confirm => {
                        if party.members.first().is_none_or(|member| member.hp == 0) {
                            self.show_text("You cannot act.");
                            return Effect::None;
                        }
                        let spells = self.spells(party);
                        if let Some((action, cost, enabled)) = spells.get(self.cursor)
                            && *enabled
                        {
                            let reach = self.action_reach(*action).unwrap_or(Reach::One);
                            if heal_targets(reach, 0, party).is_empty() {
                                self.show_text("No one can be healed.");
                            } else {
                                self.pending = Some(Pending::Spell {
                                    action: *action,
                                    cost: *cost,
                                });
                                self.target_return = Screen::Spells;
                                self.cursor = 0;
                                if matches!(reach, Reach::All | Reach::Group | Reach::Actor) {
                                    if let Some(pending) = self.pending.take() {
                                        self.apply(pending, 0, party, rng);
                                    }
                                    self.screen = Screen::Spells;
                                } else {
                                    self.screen = Screen::Targets;
                                }
                            }
                        }
                    }
                }
                Effect::None
            }
            Screen::Targets => {
                match button {
                    Button::Up => self.cursor = step_living(party, self.cursor, -1),
                    Button::Down => self.cursor = step_living(party, self.cursor, 1),
                    Button::Cancel => {
                        self.pending = None;
                        self.cursor = 0;
                        self.screen = self.target_return;
                    }
                    Button::Confirm => {
                        let Some(pending) = self.pending else {
                            self.screen = self.target_return;
                            return Effect::None;
                        };
                        let reach = match pending {
                            Pending::Spell { action, .. } => {
                                self.action_reach(action).unwrap_or(Reach::One)
                            }
                            Pending::Item(source) => {
                                let action = self.item_action(party, source);
                                action
                                    .and_then(|action| self.action_reach(action))
                                    .unwrap_or(Reach::One)
                            }
                        };
                        if heal_targets(reach, self.cursor, party).is_empty() {
                            self.show_text("They cannot be healed.");
                            return Effect::None;
                        }
                        self.pending = None;
                        self.apply(pending, self.cursor, party, rng);
                        self.cursor = 0;
                        self.screen = self.target_return;
                    }
                }
                Effect::None
            }
            Screen::Shop => {
                let mode = self.shop_mode;
                let rows = self.shop_rows(party).len().max(1);
                match button {
                    Button::Up => self.cursor = (self.cursor + rows - 1) % rows,
                    Button::Down => self.cursor = (self.cursor + 1) % rows,
                    Button::Cancel => self.close(),
                    Button::Confirm => {
                        let rows = self.shop_rows(party);
                        if let Some(item) = rows.get(self.cursor).and_then(|row| row.1) {
                            self.trade(mode, item, party);
                        }
                        let count = self.shop_rows(party).len();
                        self.cursor = self.cursor.min(count.saturating_sub(1));
                    }
                }
                Effect::None
            }
            Screen::Inn => {
                let choices = self
                    .inn
                    .as_ref()
                    .map_or_else(Vec::new, |inn| inn.choices.clone());
                let rows = choices.len().max(1);
                match button {
                    Button::Up => {
                        self.cursor = (self.cursor + rows - 1) % rows;
                        Effect::None
                    }
                    Button::Down => {
                        self.cursor = (self.cursor + 1) % rows;
                        Effect::None
                    }
                    Button::Cancel => {
                        self.show_line(inn_line::LEAVE, Vec::new(), NoticeNext::Close, Screen::Inn);
                        Effect::None
                    }
                    Button::Confirm => {
                        let Some(choice) = choices.get(self.cursor).copied() else {
                            return Effect::None;
                        };
                        let per_head = self.inn.as_ref().map_or(0, |inn| inn.per_head);
                        match choice {
                            InnChoice::Cancel => {
                                self.show_line(
                                    inn_line::LEAVE,
                                    Vec::new(),
                                    NoticeNext::Close,
                                    Screen::Inn,
                                );
                                Effect::None
                            }
                            InnChoice::Stay | InnChoice::Rest => {
                                let stay = match choice {
                                    InnChoice::Stay => inn::stay(party, clock, per_head),
                                    _ => inn::rest(party, clock, per_head),
                                };
                                let price = stay.price();
                                let living = inn::living(party);
                                if let Some(inn) = &self.inn {
                                    let choices = inn::menu(clock.phase()).to_vec();
                                    let labels = inn_labels(&inn.messages, &choices);
                                    if let Some(inn) = &mut self.inn {
                                        inn.choices = choices;
                                        inn.labels = labels;
                                    }
                                }
                                status(&format!(
                                    "inn: {:?} price {price} for {living} beds, gold {}, clock {}, members {:?}",
                                    choice,
                                    party.gold,
                                    clock.seconds(),
                                    party
                                        .members
                                        .iter()
                                        .map(|member| (
                                            member.hp,
                                            member.max_hp,
                                            member.mp,
                                            member.max_mp
                                        ))
                                        .collect::<Vec<_>>()
                                ));
                                if stay.closing_line().is_some() {
                                    // The line said as the screen goes black, then 1011.
                                    self.show_line(
                                        stay.line(),
                                        vec![(1, living), (2, price)],
                                        NoticeNext::Rested,
                                        Screen::Inn,
                                    );
                                } else {
                                    self.show_line(
                                        stay.line(),
                                        vec![(1, living), (2, price)],
                                        NoticeNext::Return,
                                        Screen::Inn,
                                    );
                                }
                                Effect::Clock
                            }
                        }
                    }
                }
            }
            Screen::Church => {
                let rows = self.church.as_ref().map_or(1, |c| c.services.len().max(1));
                match button {
                    Button::Up => {
                        self.cursor = (self.cursor + rows - 1) % rows;
                        Effect::None
                    }
                    Button::Down => {
                        self.cursor = (self.cursor + 1) % rows;
                        Effect::None
                    }
                    Button::Cancel => {
                        self.close();
                        Effect::None
                    }
                    Button::Confirm => {
                        let service_cursor = self.cursor;
                        if let Some(church) = &mut self.church {
                            church.cursor = service_cursor;
                        }
                        let Some(service) = self
                            .church
                            .as_ref()
                            .and_then(|church| church.services.get(service_cursor))
                            .copied()
                        else {
                            return Effect::None;
                        };
                        match service {
                            Service::Nothing => self.close(),
                            Service::Confession => {
                                let text = church::NOT_AVAILABLE.as_bytes().to_vec();
                                self.notice = Some(Notice {
                                    text,
                                    values: Vec::new(),
                                    target: None,
                                    vocation: None,
                                    next: NoticeNext::Return,
                                    return_screen: Screen::Church,
                                });
                                self.screen = Screen::Notice;
                            }
                            Service::Divination
                            | Service::Resurrection
                            | Service::Purification
                            | Service::Benediction => {
                                if let Some(church) = &mut self.church {
                                    church.selected = Some(service);
                                    church.cure = None;
                                    church.member = 0;
                                    church.confirm = 0;
                                }
                                self.cursor = 0;
                                self.screen = Screen::ChurchMembers;
                            }
                        }
                        Effect::None
                    }
                }
            }
            Screen::ChurchMembers => {
                let service = self.church_service();
                let rows = party.members.len().max(1);
                match button {
                    Button::Up => {
                        self.cursor = (self.cursor + rows - 1) % rows;
                        Effect::None
                    }
                    Button::Down => {
                        self.cursor = (self.cursor + 1) % rows;
                        Effect::None
                    }
                    Button::Cancel => {
                        self.cursor = self.church.as_ref().map_or(0, |church| church.cursor);
                        self.screen = Screen::Church;
                        Effect::None
                    }
                    Button::Confirm => {
                        let Some(service) = service else {
                            return Effect::None;
                        };
                        let index = self.cursor;
                        match service {
                            Service::Divination => {
                                self.show_divination(index, party);
                            }
                            Service::Resurrection
                            | Service::Purification
                            | Service::Benediction => {
                                let member = &party.members[index];
                                let bits = self.statuses.get(index).copied().unwrap_or_default();
                                let needs = match service {
                                    Service::Resurrection => member.hp == 0,
                                    Service::Purification => bits.poisoned,
                                    Service::Benediction => bits.cursed,
                                    _ => false,
                                };
                                let line = if needs {
                                    church_line::TOO_POOR
                                } else {
                                    church::cure_line(service).map(|line| line + 2).unwrap_or(0)
                                };
                                let price = church::price(service, member).unwrap_or(0);
                                if !needs || price > party.gold {
                                    status(&format!(
                                        "church {service:?} on member {index}: refused (needs {needs}, price {price}, gold {}), HP {}",
                                        party.gold, member.hp
                                    ));
                                    let text = self
                                        .church
                                        .as_ref()
                                        .and_then(|church| church.messages.get(&line).cloned())
                                        .unwrap_or_default();
                                    let name = self.name(index);
                                    self.notice = Some(Notice {
                                        text,
                                        values: Vec::new(),
                                        target: Some(name),
                                        vocation: None,
                                        next: NoticeNext::Return,
                                        return_screen: Screen::Church,
                                    });
                                    self.screen = Screen::Notice;
                                    return Effect::None;
                                }
                                if let Some(church) = &mut self.church {
                                    church.member = index;
                                    church.cure = Some((service, index, price));
                                    church.confirm = 0;
                                }
                                self.cursor = 0;
                                self.screen = Screen::ChurchConfirm;
                            }
                            _ => {}
                        }
                        Effect::None
                    }
                }
            }
            Screen::ChurchConfirm => {
                let mut effect = Effect::None;
                match button {
                    Button::Up | Button::Down => {
                        if let Some(church) = &mut self.church {
                            church.confirm ^= 1;
                        }
                    }
                    Button::Cancel => {
                        self.cursor = 0;
                        self.screen = Screen::ChurchMembers;
                    }
                    Button::Confirm => {
                        let yes = self
                            .church
                            .as_ref()
                            .is_some_and(|church| church.confirm == 0);
                        let cure = self.church.as_ref().and_then(|church| church.cure);
                        self.screen = Screen::Church;
                        self.cursor = 0;
                        if yes && let Some((service, index, _)) = cure {
                            match church::cure(service, party, &mut self.statuses, index) {
                                Ok(cure) => {
                                    status(&format!(
                                        "church {service:?} on member {index}: {cure:?}, gold {}, HP {}",
                                        party.gold, party.members[index].hp
                                    ));
                                    let line =
                                        cure.prayer(service).or_else(|| cure.notice(service));
                                    if let Some(line) = line {
                                        self.show_line(
                                            line,
                                            Vec::new(),
                                            NoticeNext::Return,
                                            Screen::Church,
                                        );
                                        let name = self.name(index);
                                        if let Some(notice) = &mut self.notice {
                                            notice.target = Some(name);
                                        }
                                    }
                                    effect = Effect::None;
                                }
                                Err(ChurchError::NoStatus | ChurchError::NoMember) => {
                                    self.show_text("No such party member.");
                                }
                                Err(error) => {
                                    self.show_text(&format!("{error}."));
                                }
                            }
                        }
                    }
                }
                effect
            }
        }
    }

    /// Left/right on a screen: switches the shop between buying and selling.
    pub fn side(&mut self, right: bool) {
        if self.screen == Screen::Shop {
            self.shop_mode = if right { ShopMode::Sell } else { ShopMode::Buy };
            self.cursor = 0;
        }
    }

    /// `H` on the item screen: pick a held slot for the bag item under the cursor.
    pub fn hold(&mut self, party: &Party) {
        if self.screen != Screen::Items {
            return;
        }
        let bag_rows = self.bag_rows(party);
        if self.cursor >= bag_rows {
            return;
        }
        if let Some(Source::Bag(item)) = self.item_source(party) {
            self.held_item = Some(item);
            self.cursor = 0;
            self.screen = Screen::HeldSlots;
        }
    }

    /// `T` on the item screen: take the held item under the cursor back to the bag.
    pub fn take(&mut self, party: &mut Party) {
        if self.screen != Screen::Items {
            return;
        }
        if let Some(Source::Held(slot)) = self.item_source(party)
            && held_to_bag(party, slot)
        {
            status(&format!("item: held slot {slot} -> bag"));
            let count = self.item_sources(party).len();
            self.cursor = self.cursor.min(count.saturating_sub(1));
        }
    }

    fn church_service(&self) -> Option<Service> {
        self.church
            .as_ref()
            .and_then(|church| church.selected)
            .filter(|service| {
                matches!(
                    service,
                    Service::Divination
                        | Service::Resurrection
                        | Service::Purification
                        | Service::Benediction
                )
            })
    }

    fn show_text(&mut self, text: &str) {
        self.notice = Some(Notice {
            text: text.as_bytes().to_vec(),
            values: Vec::new(),
            target: None,
            vocation: None,
            next: NoticeNext::Return,
            return_screen: self.screen,
        });
        self.screen = Screen::Notice;
    }

    fn show_line(
        &mut self,
        line: u16,
        values: Vec<(u8, u32)>,
        next: NoticeNext,
        return_screen: Screen,
    ) {
        let text = self
            .inn
            .as_ref()
            .and_then(|inn| inn.messages.get(&line).cloned())
            .or_else(|| {
                self.church
                    .as_ref()
                    .and_then(|church| church.messages.get(&line).cloned())
            })
            .unwrap_or_default();
        self.notice = Some(Notice {
            text,
            values,
            target: None,
            vocation: None,
            next,
            return_screen,
        });
        self.screen = Screen::Notice;
    }

    fn show_divination(&mut self, index: usize, party: &Party) {
        let Some(data) = &self.data else {
            return;
        };
        let Some(member) = party.members.get(index) else {
            return;
        };
        let divination = church::divine(member, &data.levels);
        let values = match divination {
            church::Divination::Experience(wanted) => vec![(1, wanted)],
            _ => Vec::new(),
        };
        let Some(church) = &self.church else {
            return;
        };
        let text = church
            .messages
            .get(&divination.line())
            .cloned()
            .unwrap_or_default();
        let vocation = church
            .messages
            .get(&church::vocation_line(member.vocation))
            .cloned()
            .filter(|label| !label.is_empty());
        let name = self.name(index);
        self.notice = Some(Notice {
            text,
            values,
            target: Some(name),
            vocation,
            next: NoticeNext::Return,
            return_screen: Screen::Church,
        });
        self.screen = Screen::Notice;
    }

    /// The action of the item's field record (`+0x14`), when the action tables know it.
    fn item_action(&self, party: &Party, source: Source) -> Option<u16> {
        let data = self.data.as_ref()?;
        let item = match source {
            Source::Bag(item) => item,
            Source::Held(slot) => party
                .members
                .first()
                .and_then(|member| member.held_items.get(slot).copied().flatten())?,
        };
        let [field, _] = *data.item_actions.get(&item)?;
        if field == NO_ACTION {
            return None;
        }
        let tables = data.tables().ok()?;
        let skill = tables.actions.skill(field)?;
        (skill.kind == 2).then_some(field)
    }

    /// The reach of an action the field can resolve, or `None` when its tables cannot name it.
    fn action_reach(&self, action: u16) -> Option<Reach> {
        let tables = self.data.as_ref()?.tables().ok()?;
        let skill = tables.actions.skill(action)?;
        skill.action.ok().map(|action| action.reach)
    }

    fn apply(&mut self, pending: Pending, target: usize, party: &mut Party, rng: &mut Random) {
        let Some(data) = &self.data else {
            return;
        };
        let actor = party
            .members
            .first()
            .map(|member| member.fighter(&data.items, &data.params));
        let Some(actor) = actor else {
            return;
        };
        if actor.hp == 0 {
            return;
        }
        match pending {
            Pending::Spell { action, cost } => {
                let Ok(tables) = data.tables() else {
                    return;
                };
                let Some(skill) = tables.actions.skill(action) else {
                    return;
                };
                let Ok(action_data) = skill.action else {
                    return;
                };
                if actor.mp < i32::from(cost) {
                    return;
                }
                let indices = heal_targets(action_data.reach, target, party);
                if indices.is_empty() {
                    return;
                }
                let fighters: Vec<_> = indices
                    .iter()
                    .map(|&index| party.members[index].fighter(&data.items, &data.params))
                    .collect();
                let Ok(outcome) = resolve_action(rng, &actor, &action_data, &fighters, 0) else {
                    return;
                };
                let before: Vec<u16> = indices
                    .iter()
                    .map(|&index| party.members[index].hp)
                    .collect();
                for (&index, hit) in indices.iter().zip(&outcome.targets) {
                    party.members[index].set_hp(hit.hp_after.max(0) as u16);
                }
                if let Some(caster) = party.members.first_mut() {
                    caster.set_mp(caster.mp.saturating_sub(u16::from(cost)));
                }
                status(&format!(
                    "field spell {action} on members {:?}: HP {:?} -> {:?}, MP {}",
                    indices,
                    before,
                    indices
                        .iter()
                        .map(|&index| party.members[index].hp)
                        .collect::<Vec<_>>(),
                    party.members.first().map_or(0, |member| member.mp)
                ));
            }
            Pending::Item(source) => {
                let Some(action) = self.item_action(party, source) else {
                    return;
                };
                let item = match source {
                    Source::Bag(item) => item,
                    Source::Held(slot) => match party
                        .members
                        .first()
                        .and_then(|member| member.held_items[slot])
                    {
                        Some(item) => item,
                        None => return,
                    },
                };
                let Ok(tables) = data.tables() else {
                    return;
                };
                let Some(skill) = tables.actions.skill(action) else {
                    return;
                };
                let Ok(action_data) = skill.action else {
                    return;
                };
                let indices = heal_targets(action_data.reach, target, party);
                if indices.is_empty() {
                    return;
                }
                let fighters: Vec<_> = indices
                    .iter()
                    .map(|&index| party.members[index].fighter(&data.items, &data.params))
                    .collect();
                let Ok(outcome) = resolve_action(rng, &actor, &action_data, &fighters, 0) else {
                    return;
                };
                let before: Vec<u16> = indices
                    .iter()
                    .map(|&index| party.members[index].hp)
                    .collect();
                for (&index, hit) in indices.iter().zip(&outcome.targets) {
                    party.members[index].set_hp(hit.hp_after.max(0) as u16);
                }
                // The item is spent once, after the action executed, from the chosen source.
                match source {
                    Source::Bag(item) => {
                        party.bag.remove(item, 1);
                        status(&format!(
                            "field item {item} on members {:?}: HP {:?} -> {:?}, bag {} left",
                            indices,
                            before,
                            indices
                                .iter()
                                .map(|&index| party.members[index].hp)
                                .collect::<Vec<_>>(),
                            party.bag.count(item)
                        ));
                    }
                    Source::Held(slot) => {
                        let used_up = data.item_meta.get(&item).is_some_and(|meta| meta.used_up);
                        if used_up {
                            consume_held(party, slot);
                        }
                        status(&format!(
                            "field held item {item} on members {:?}: HP {:?} -> {:?}, slot {slot} {}",
                            indices,
                            before,
                            indices
                                .iter()
                                .map(|&index| party.members[index].hp)
                                .collect::<Vec<_>>(),
                            if used_up { "spent" } else { "kept" }
                        ));
                    }
                }
            }
        }
    }

    fn trade(&mut self, mode: ShopMode, item: u16, party: &mut Party) {
        let (Some(shops), Some(prices)) = (&self.shops, &self.prices) else {
            return;
        };
        let Some(shop) = shops.shop(self.shop_number) else {
            return;
        };
        match mode {
            ShopMode::Buy => match shop::buy(party, shop, prices, item, 1) {
                Ok(total) => {
                    status(&format!(
                        "shop {item}: bought for {total}, gold {}, bag {}",
                        party.gold,
                        party.bag.count(item)
                    ));
                    self.show_text(&format!("Bought for {total} gold."));
                }
                Err(error) => {
                    status(&format!(
                        "shop {item}: buy refused ({error:?}), gold {}, bag {}",
                        party.gold,
                        party.bag.count(item)
                    ));
                    self.show_text(&format!("{error:?}"));
                }
            },
            ShopMode::Sell => match shop::sell(party, prices, item, 1) {
                Ok(total) => {
                    status(&format!(
                        "shop {item}: sold for {total}, gold {}",
                        party.gold
                    ));
                    self.show_text(&format!("Sold for {total} gold."));
                }
                Err(error) => {
                    status(&format!(
                        "shop {item}: sell refused ({error:?}), gold {}, bag {}",
                        party.gold,
                        party.bag.count(item)
                    ));
                    self.show_text(&format!("{error:?}"));
                }
            },
        }
    }

    /// The shop's rows for the mode in use: `(label, item)`. A buy row is one stock item, a
    /// sell row one item the bag holds (its first slot, in bag order).
    fn shop_rows(&self, party: &Party) -> Vec<(Vec<u8>, Option<u16>)> {
        let Some(shops) = &self.shops else {
            return Vec::new();
        };
        let Some(shop) = shops.shop(self.shop_number) else {
            return Vec::new();
        };
        let prices = self.prices.as_ref();
        let mut out = Vec::new();
        match self.shop_mode {
            ShopMode::Buy => {
                for item in shop.items() {
                    let price = prices
                        .and_then(|prices| shop::buy_price(shop, prices, item))
                        .unwrap_or(0);
                    let mut label = self.item_name(item);
                    label.extend_from_slice(format!("  {price}").as_bytes());
                    out.push((label, Some(item)));
                }
            }
            ShopMode::Sell => {
                let mut seen = Vec::new();
                for stack in party.bag.slots() {
                    let Some(item) = stack.item else { continue };
                    if seen.contains(&item) {
                        continue;
                    }
                    seen.push(item);
                    let price = prices
                        .and_then(|prices| shop::sell_price(prices, item))
                        .unwrap_or(0);
                    let mut label = self.item_name(item);
                    label.extend_from_slice(format!("  x{}  {price}", stack.quantity).as_bytes());
                    out.push((label, Some(item)));
                }
            }
        }
        out
    }

    /// The equipment choices for `slot`: the bag's items of that band, then `None` (take the
    /// piece off).
    fn equip_choices(&self, party: &Party, slot: EquipSlot) -> Vec<Option<u16>> {
        let mut out = Vec::new();
        for stack in party.bag.slots() {
            let Some(item) = stack.item else { continue };
            if equip_slot_of(item) == Some(slot) && !out.contains(&Some(item)) {
                out.push(Some(item));
            }
        }
        out.push(None);
        out
    }

    /// The leader's learned healing spells: actions up to the member's level in its vocation
    /// whose kind is 2 (heal) and whose targets are its own side. `(action, MP cost, usable)`.
    fn spells(&self, party: &Party) -> Vec<(u16, u8, bool)> {
        let (Some(data), Some(member)) = (&self.data, party.members.first()) else {
            return Vec::new();
        };
        let tables = match data.tables() {
            Ok(tables) => tables,
            Err(_) => return Vec::new(),
        };
        let actor = member.fighter(&data.items, &data.params);
        let target = actor.clone();
        data.spells_known(member.vocation, member.level())
            .into_iter()
            .filter_map(|action| {
                let skill = tables.actions.skill(action)?;
                if skill.kind != 2 || skill.aims != 2 {
                    return None;
                }
                let usable = member.hp > 0
                    && skill.action.as_ref().is_ok_and(|action| {
                        i32::from(action.mp_cost) <= actor.mp
                            && dq9_core::battle::validate(
                                &actor,
                                action,
                                std::slice::from_ref(&target),
                            )
                            .is_ok()
                    });
                Some((
                    action,
                    data.lines.get(&action).map_or(0, |lines| lines.mp_cost),
                    usable,
                ))
            })
            .collect()
    }

    /// The quads of the menu on the 256×192 screen; `None` when nothing is open.
    pub fn quads(&self, atlas: &Atlas, font: &Font<'_>, party: &Party) -> Option<Vec<Quad>> {
        if !self.is_open() {
            return None;
        }
        if self.screen == Screen::Notice {
            return Some(self.notice_quads(atlas, font));
        }
        let mut title = Vec::new();
        let mut rows: Vec<(Vec<u8>, bool)> = Vec::new();
        let mut footer = Vec::new();
        let mut cursor = None;
        match self.screen {
            Screen::Closed => return None,
            Screen::Menu => {
                title.extend_from_slice(b"Field");
                for (label, _) in MENU {
                    rows.push((label.to_vec(), true));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: close");
            }
            Screen::Items => {
                title.extend_from_slice(
                    format!("Items  bag {}  gold {}", self.bag_rows(party), party.gold).as_bytes(),
                );
                let sources = self.item_sources(party);
                let bag_rows = self.bag_rows(party);
                for (index, source) in sources.iter().enumerate() {
                    let label = match *source {
                        Source::Bag(item) => {
                            let stack = party
                                .bag
                                .slots()
                                .iter()
                                .find(|stack| stack.item == Some(item));
                            let quantity = stack.map_or(0, |stack| stack.quantity);
                            let mut label = self.item_name(item);
                            label.extend_from_slice(format!("  x{quantity}").as_bytes());
                            label
                        }
                        Source::Held(slot) => {
                            let item = party
                                .members
                                .first()
                                .and_then(|member| member.held_items[slot])
                                .unwrap_or(0);
                            let mut label = self.item_name(item);
                            label.extend_from_slice(format!("  (held {})", slot + 1).as_bytes());
                            label
                        }
                    };
                    let _ = index;
                    let _ = bag_rows;
                    rows.push((label, true));
                }
                if rows.is_empty() {
                    rows.push((b"(empty)".to_vec(), false));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: use  H: hold  T: take  X: back");
            }
            Screen::HeldSlots => {
                let item = self.held_item.unwrap_or(0);
                title.extend_from_slice(b"Hold ");
                title.extend_from_slice(&self.item_name(item));
                title.extend_from_slice(b" in...");
                for slot in 0..HELD_ITEMS {
                    let held = party
                        .members
                        .first()
                        .and_then(|member| member.held_items[slot]);
                    let mut label = format!("Slot {}  ", slot + 1).into_bytes();
                    match held {
                        Some(item) => {
                            label.extend_from_slice(&self.item_name(item));
                            rows.push((label, false));
                        }
                        None => {
                            label.extend_from_slice(b"empty");
                            rows.push((label, true));
                        }
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: hold here   X: back");
            }
            Screen::Equip => {
                title.extend_from_slice(b"Equipment");
                let equipment = party.members.first().map(|member| member.equipment);
                for (slot, label) in SLOTS {
                    let item = equipment.and_then(|equipment| equipment.get(slot));
                    let mut row = label.to_vec();
                    row.extend_from_slice(b": ");
                    match item {
                        Some(item) => row.extend_from_slice(&self.item_name(item)),
                        None => row.extend_from_slice(b"(none)"),
                    }
                    rows.push((row, true));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: back");
            }
            Screen::EquipChoice(slot) => {
                title.extend_from_slice(b"Choose ");
                title.extend_from_slice(
                    SLOTS
                        .iter()
                        .find(|(candidate, _)| *candidate == slot)
                        .map_or(b"?", |(_, label)| *label),
                );
                for choice in self.equip_choices(party, slot) {
                    match choice {
                        Some(item) => {
                            let name = self.item_name(item);
                            let stack = party
                                .bag
                                .slots()
                                .iter()
                                .find(|stack| stack.item == Some(item));
                            let mut label = name;
                            if let Some(stack) = stack {
                                label
                                    .extend_from_slice(format!("  x{}", stack.quantity).as_bytes());
                            }
                            rows.push((label, true));
                        }
                        None => rows.push((b"Take it off".to_vec(), true)),
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: equip   X: back");
            }
            Screen::Stats => {
                let index = self.cursor.min(party.members.len().saturating_sub(1));
                title.extend_from_slice(b"Attributes");
                if let Some(member) = party.members.get(index) {
                    let mut line = self.name(index);
                    line.extend_from_slice(
                        format!(
                            "  Lv{}  HP {}/{}  MP {}/{}",
                            member.level(),
                            member.hp,
                            member.max_hp,
                            member.mp,
                            member.max_mp
                        )
                        .as_bytes(),
                    );
                    rows.push((line, true));
                    if let Some(data) = &self.data {
                        let fighter = member.fighter(&data.items, &data.params);
                        rows.push((
                            format!("ATK {}  DEF {}", fighter.attack, fighter.defence).into_bytes(),
                            false,
                        ));
                        rows.push((
                            format!("AGI {}  DFT {}", fighter.agility, fighter.deftness)
                                .into_bytes(),
                            false,
                        ));
                        rows.push((
                            format!(
                                "MGT {}  MDG {}",
                                fighter.magical_might, member.stats.magical_mending
                            )
                            .into_bytes(),
                            false,
                        ));
                    }
                }
                footer.extend_from_slice(b"Up/Down: member   X: back");
            }
            Screen::Spells => {
                title.extend_from_slice(b"Healing spells");
                let spells = self.spells(party);
                if spells.is_empty() {
                    rows.push((b"(none learned)".to_vec(), false));
                }
                for (action, cost, usable) in spells {
                    let mut label = self
                        .data
                        .as_ref()
                        .and_then(|data| data.lines.get(&action))
                        .map(|lines| lines.name.clone())
                        .unwrap_or_else(|| format!("#{action}").into_bytes());
                    label.extend_from_slice(format!("  MP {cost}").as_bytes());
                    rows.push((label, usable));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: cast   X: back");
            }
            Screen::Targets => {
                title.extend_from_slice(b"On whom?");
                for (index, member) in party.members.iter().enumerate() {
                    let mut label = self.name(index);
                    label.extend_from_slice(
                        format!("  HP {}/{}", member.hp, member.max_hp).as_bytes(),
                    );
                    rows.push((label, member.hp > 0));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: back");
            }
            Screen::Shop => {
                title.extend_from_slice(
                    format!(
                        "Shop {}  {}  gold {}",
                        self.shop_number,
                        match self.shop_mode {
                            ShopMode::Buy => "Buy",
                            ShopMode::Sell => "Sell",
                        },
                        party.gold
                    )
                    .as_bytes(),
                );
                for (label, _) in self.shop_rows(party) {
                    rows.push((label, true));
                }
                if rows.is_empty() {
                    rows.push((b"(empty)".to_vec(), false));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: trade   Left/Right: buy/sell   X: leave");
            }
            Screen::Inn => {
                let price = self
                    .inn
                    .as_ref()
                    .map_or(0, |inn| inn::price(inn.per_head, party));
                title.extend_from_slice(
                    format!("Inn  {} gold  {} beds", price, inn::living(party)).as_bytes(),
                );
                if let Some(inn) = &self.inn {
                    for choice in &inn.labels {
                        rows.push((choice.clone(), true));
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: leave");
            }
            Screen::Church => {
                title.extend_from_slice(b"Church");
                if let Some(church) = &self.church {
                    for service in &church.services {
                        let label = church
                            .messages
                            .get(&service.label())
                            .cloned()
                            .unwrap_or_else(|| format!("{service:?}").into_bytes());
                        rows.push((label, true));
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: leave");
            }
            Screen::ChurchMembers => {
                let service = self.church_service();
                title.extend_from_slice(
                    format!("{}: on whom?", service.map_or("?", |s| service_name(s))).as_bytes(),
                );
                for (index, member) in party.members.iter().enumerate() {
                    let mut label = self.name(index);
                    if member.hp == 0 {
                        label.extend_from_slice(b"  (dead)");
                    }
                    let status = self.statuses.get(index).copied().unwrap_or_default();
                    if status.poisoned {
                        label.extend_from_slice(b"  (poisoned)");
                    }
                    if status.cursed {
                        label.extend_from_slice(b"  (cursed)");
                    }
                    rows.push((label, true));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: back");
            }
            Screen::ChurchConfirm => {
                let cure = self.church.as_ref().and_then(|church| church.cure);
                if let Some((service, _, price)) = cure {
                    title.extend_from_slice(
                        format!("{}: {price} gold", service_name(service)).as_bytes(),
                    );
                }
                rows.push((b"Yes".to_vec(), true));
                rows.push((b"No".to_vec(), true));
                cursor = Some(self.church.as_ref().map_or(0, |church| church.confirm));
                footer.extend_from_slice(b"Enter: answer   X: back");
            }
            Screen::Notice => {}
        }
        Some(draw_rows(atlas, font, &title, &rows, cursor, &footer))
    }

    /// The bytes a notice draws: `<val_n>`, the target and the vocation label replaced; an
    /// unresolved tag remains visible rather than pretending a name is known.
    fn resolved_notice_text(&self, notice: &Notice) -> Vec<u8> {
        let mut text = substitute_values(&notice.text, &notice.values);
        if let Some(target) = &notice.target {
            replace_tag(&mut text, b"<TARGET>", target);
            replace_tag(&mut text, b"<TARGET0>", target);
            replace_tag(&mut text, b"<Target0>", target);
        }
        if let Some(vocation) = &notice.vocation {
            replace_tag(&mut text, b"<STR_2>", vocation);
            replace_tag(&mut text, b"<str_2>", vocation);
        }
        text
    }

    /// A notice: the facility's own line laid out with the font, with the `<val_n>` values
    /// substituted; an application message is drawn the same way.
    fn notice_quads(&self, atlas: &Atlas, font: &Font<'_>) -> Vec<Quad> {
        let mut out = Vec::new();
        atlas.window(WINDOW, crate::talk::FILL, &mut out);
        let Some(notice) = &self.notice else {
            return out;
        };
        let text = self.resolved_notice_text(notice);
        let lines = glyph_lines(font, &text, WINDOW[2] - 2 * MARGIN[0]);
        for (index, line) in lines.iter().take(VISIBLE_ROWS).enumerate() {
            let y = WINDOW[1] + MARGIN[1] + 20 + index as i32 * ROW;
            atlas.text(line, WINDOW[0] + MARGIN[0], y, WHITE, &mut out);
        }
        let footer = b"Enter: continue";
        atlas_text(
            atlas,
            font,
            footer,
            WINDOW[0] + MARGIN[0],
            WINDOW[1] + WINDOW[3] - MARGIN[1] - 8,
            GREY,
            &mut out,
        );
        out
    }
}

/// The item's slot names, in the menu's order.
const SLOTS: [(EquipSlot, &[u8]); 8] = [
    (EquipSlot::Weapon, b"Weapon"),
    (EquipSlot::Shield, b"Shield"),
    (EquipSlot::Head, b"Head"),
    (EquipSlot::Torso, b"Torso"),
    (EquipSlot::Arms, b"Arms"),
    (EquipSlot::Legs, b"Legs"),
    (EquipSlot::Feet, b"Feet"),
    (EquipSlot::Accessory, b"Accessory"),
];

const MENU: [(&[u8], Screen); 5] = [
    (b"Items", Screen::Items),
    (b"Equipment", Screen::Equip),
    (b"Attributes", Screen::Stats),
    (b"Spells", Screen::Spells),
    (b"Close", Screen::Closed),
];

/// The party members an action of `reach` heals from the field, in party order. A heal never
/// raises the dead; `Actor` is the leader and `One`/`Attack` the chosen row.
fn heal_targets(reach: Reach, chosen: usize, party: &Party) -> Vec<usize> {
    let living = |index: usize| party.members.get(index).is_some_and(|member| member.hp > 0);
    match reach {
        Reach::All | Reach::Group => (0..party.members.len())
            .filter(|&index| living(index))
            .collect(),
        Reach::Actor => (0..party.members.len())
            .find(|&index| living(index))
            .into_iter()
            .collect(),
        Reach::One | Reach::Attack => (living(chosen)).then_some(chosen).into_iter().collect(),
    }
}

/// Moves the target cursor to the next living member, leaving it in place when none is.
fn step_living(party: &Party, cursor: usize, delta: isize) -> usize {
    let len = party.members.len();
    if len == 0 {
        return 0;
    }
    let mut at = cursor.min(len - 1);
    for _ in 0..len {
        at = (at as isize + delta).rem_euclid(len as isize) as usize;
        if party.members[at].hp > 0 {
            return at;
        }
    }
    cursor.min(len - 1)
}

fn service_name(service: Service) -> &'static str {
    match service {
        Service::Confession => "Confession",
        Service::Divination => "Divination",
        Service::Resurrection => "Resurrection",
        Service::Purification => "Purification",
        Service::Benediction => "Benediction",
        Service::Nothing => "Nothing",
    }
}

/// The retail `str_in` files carry the three menu labels and the price line. A file without
/// them would otherwise become a free inn with debug labels; the explicit free inn `<INN=15>`
/// is accepted because its price line exists and reads 0.
fn validate_inn_messages(messages: &BTreeMap<u16, Vec<u8>>) -> Result<()> {
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
fn inn_labels(messages: &BTreeMap<u16, Vec<u8>>, choices: &[InnChoice]) -> Vec<Vec<u8>> {
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

/// Takes the used held item out of its slot, shifting the holes down as `func_0208386c` does.
fn consume_held(party: &mut Party, slot: usize) {
    let Some(character) = party.members.first_mut() else {
        return;
    };
    if slot >= HELD_ITEMS {
        return;
    }
    let items = &mut character.held_items;
    items[slot] = None;
    for index in slot..HELD_ITEMS - 1 {
        if items[index].is_none() {
            items[index] = items[index + 1];
            items[index + 1] = None;
        }
    }
}

/// Fills `<val_n>` with the numbers a facility line wants (`expand_values` would otherwise
/// use `n` itself).
fn substitute_values(text: &[u8], values: &[(u8, u32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index..].starts_with(b"<val_") {
            let rest = &text[index + 5..];
            if let Some(&digit) = rest.first()
                && digit.is_ascii_digit()
                && rest.get(1) == Some(&b'>')
                && let Some((_, value)) = values
                    .iter()
                    .find(|(slot, _)| *slot == digit.saturating_sub(b'0'))
            {
                out.extend_from_slice(value.to_string().as_bytes());
                index += 7;
                continue;
            }
        }
        out.push(text[index]);
        index += 1;
    }
    out
}

/// Replaces every `tag` byte sequence with `value`.
fn replace_tag(text: &mut Vec<u8>, tag: &[u8], value: &[u8]) {
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index..].starts_with(tag) {
            out.extend_from_slice(value);
            index += tag.len();
        } else {
            out.push(text[index]);
            index += 1;
        }
    }
    *text = out;
}

/// The lines of a message as glyphs, through the game's own layout (`talk::prepare`).
fn glyph_lines(font: &Font<'_>, text: &[u8], width: i32) -> Vec<Vec<(u8, i32)>> {
    let script = dq9_core::talk::prepare(font, text, &Names::default(), width);
    let mut lines = vec![Vec::new()];
    for op in script.ops {
        match op {
            dq9_core::talk::Op::Glyph { glyph, x } => {
                lines.last_mut().unwrap().push((glyph, x));
            }
            dq9_core::talk::Op::Line | dq9_core::talk::Op::Page => lines.push(Vec::new()),
            dq9_core::talk::Op::Name(plate) => {
                lines.last_mut().unwrap().extend(plate.glyphs);
            }
            _ => {}
        }
    }
    lines.retain(|line| !line.is_empty());
    lines
}

/// Draws a titled window with its rows, the cursor's row highlighted, and a footer.
fn draw_rows(
    atlas: &Atlas,
    font: &Font<'_>,
    title: &[u8],
    rows: &[(Vec<u8>, bool)],
    cursor: Option<usize>,
    footer: &[u8],
) -> Vec<Quad> {
    let mut out = Vec::new();
    atlas.window(WINDOW, crate::talk::FILL, &mut out);
    atlas_text(
        atlas,
        font,
        title,
        WINDOW[0] + MARGIN[0],
        WINDOW[1] + MARGIN[1],
        WHITE,
        &mut out,
    );
    let start = cursor.map_or(0, |cursor| cursor.saturating_sub(3));
    for (visible, (label, enabled)) in rows.iter().skip(start).take(VISIBLE_ROWS).enumerate() {
        let index = start + visible;
        let y = WINDOW[1] + MARGIN[1] + 20 + visible as i32 * ROW;
        if cursor == Some(index) {
            out.push(atlas.solid(
                [WINDOW[0] + 6, y - 2, WINDOW[2] - 12, ROW - 2],
                crate::talk::SELECTED,
            ));
        }
        let color = if *enabled { WHITE } else { GREY };
        let fitted = fit(font, label, WINDOW[2] - 2 * MARGIN[0]);
        atlas_text(
            atlas,
            font,
            &fitted,
            WINDOW[0] + MARGIN[0],
            y,
            color,
            &mut out,
        );
    }
    let fitted = fit(font, footer, WINDOW[2] - 2 * MARGIN[0]);
    atlas_text(
        atlas,
        font,
        &fitted,
        WINDOW[0] + MARGIN[0],
        WINDOW[1] + WINDOW[3] - MARGIN[1] - 8,
        GREY,
        &mut out,
    );
    out
}

fn atlas_text(
    atlas: &Atlas,
    font: &Font<'_>,
    bytes: &[u8],
    x: i32,
    y: i32,
    color: [f32; 4],
    out: &mut Vec<Quad>,
) {
    let (glyphs, _) = font.layout(bytes);
    atlas.text(&glyphs, x, y, color, out);
}

/// Cuts `bytes` to what `width` pixels hold (a whole glyph at a time).
fn fit(font: &Font<'_>, bytes: &[u8], width: i32) -> Vec<u8> {
    if font.measure(bytes) <= width {
        return bytes.to_vec();
    }
    let mut out = Vec::new();
    for &byte in bytes {
        out.push(byte);
        if font.measure(&out) > width {
            out.pop();
            break;
        }
    }
    out
}

/// The item price tables of every category (`itemdt_<category>_<language>.nat`).
fn item_prices(rom: &[u8]) -> Result<ItemPrices> {
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

/// One member of a `.gp2` archive, decompressed and owned.
fn archive_member(rom: &[u8], archive: &str, member: &str) -> Result<Vec<u8>> {
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

fn copy_messages<'a>(
    numbers: impl Iterator<Item = u16>,
    message: impl Fn(u16) -> Option<&'a [u8]>,
) -> BTreeMap<u16, Vec<u8>> {
    numbers
        .filter_map(|number| message(number).map(|bytes| (number, bytes.to_vec())))
        .collect()
}

#[cfg(test)]
mod tests;
