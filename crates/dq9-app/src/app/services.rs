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
//! the held slot) only when the action executed **and changed something**
//! (`battle::item::effect_applied`): the battle's `ResolveAction` gates its held-item helper on
//! its applied counter, and the field menu does the same — its action applier
//! `func_ov002_021536ec` returns whether a handler reported a change, and only that result lets
//! `func_ov002_0215a7b4` spend the copy (bag or held slot; the held slot also needs the
//! record's `+0x08` bit 19, `func_020ddb20`). A herb on a full-HP member is therefore kept.

mod actions;
pub(crate) mod containers;
mod draw;
mod open;
mod tables;
mod transfer;
use self::actions::{heal_targets, step_living};
use self::open::inn_labels;
use self::tables::{equipment_messages, item_rules};

use std::collections::BTreeMap;

use anyhow::Result;
use dq9_core::battle::{Random, Reach};
use dq9_core::facility::church::{self, ChurchError, MemberStatus, Service};
use dq9_core::facility::inn::{self, Choice as InnChoice};
use dq9_core::party::{HELD_ITEMS, Party};
use dq9_core::state::DayClock;
use dq9_formats::facility::inn_church::{church_line, inn_line};
use dq9_formats::facility::shop::{ItemPrices, ShopTable};
use dq9_formats::save::EquipSlot;

use crate::app::status;
use crate::battle::data::Data;
use crate::talk::Button;

pub use transfer::{bag_to_held, equip, held_to_bag};

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
    /// The church in mode 2 after a wipe-out: the priest's line 1082, no menu.
    Priest,
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

/// The church's mode 2 (`func_ov003_02158e94`, state 10): one line, then the visit ends.
#[derive(Debug, Clone)]
struct PriestState {
    /// The line of `str_ch<voice - 1>` (message 1082).
    text: Vec<u8>,
}

/// What the equipment screen and the church read from an item's record (`itemdt_*`).
///
/// The record's `+0x08` word holds the category in its low nibble and the cursed bit at 18
/// (read: Benediction's loop tests `(word & 0x7ffff) >> 18`, `func_ov003_0215a740`; ten
/// European records carry it — 12184, 12794, 13307, 18031, 18032, 18038, 20311, 20312, 20913,
/// 21393). The vocation mask is **word 1 of the record's stats entry** (`+0x04` of the 32-byte
/// entry the record's `+0x00` indexes), the word `func_020dd4c4` (EU `0x020dd4d4`) reads at
/// `record + 4`: bits 0–11, bit `v − 1` for vocation `v`, tested for the worn armour kinds
/// (categories 2 to 7) only. Measured on the European tables: all 313 weapons and shields have
/// bits 0–11 clear, all 52 accessories `0xfff`, holy mail bit 27 (male) with 0x100 (Paladin)
/// and holy femail bit 28 (female) with the same bit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ItemRule {
    /// `+0x08` bits 0–3: 0 weapon, 1 shield, 2 armour, 3 legwear, 4 head, 5 arms, 6 feet,
    /// 7 accessory, 8 tools.
    category: u8,
    /// `+0x08` bit 18: the cursed bit Benediction takes off.
    cursed: bool,
    /// The stats entry's `+0x04` bits 0–11: who may wear it.
    vocations: u16,
}

/// `str_eq` 10004: "Members of this character's current vocation can't be equipped with this
/// item." — the refusal line of `func_020dd4c4`'s `0x002` bit.
const EQUIP_VOCATION_REFUSED: u16 = 10004;

/// The field menu, the facility screen and their state. One instance lives in
/// [`App`](crate::app::App); a map change or a replaced session closes it.
#[derive(Debug, Default)]
pub struct Services {
    data: Option<Box<Data>>,
    prices: Option<ItemPrices>,
    shops: Option<ShopTable>,
    /// The item records' category, cursed bit and vocation mask, by item id.
    rules: Option<BTreeMap<u16, ItemRule>>,
    /// The equipment screen's words (`data/bin/menu/str_eq.gp2`), by message number.
    equipment_messages: Option<BTreeMap<u16, Vec<u8>>>,
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
    priest: Option<PriestState>,
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
        self.priest = None;
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

    /// Loads the item records' category, cursed bit and vocation mask, and the equipment
    /// screen's words, once. The field menu needs both; the church needs the cursed bit.
    fn ensure_rules(&mut self, rom: &[u8]) -> Result<()> {
        if self.rules.is_none() {
            self.rules = Some(item_rules(rom)?);
        }
        if self.equipment_messages.is_none() {
            self.equipment_messages = Some(equipment_messages(rom)?);
        }
        Ok(())
    }

    /// Whether the leader may wear `item`, from the item record's vocation mask
    /// (`func_020dd4c4`, EU `0x020dd4d4`): the worn armour categories 2 to 7 refuse when bit
    /// `vocation − 1` is clear, and the Guardian (0) wears anything. Weapons and shields
    /// (categories 0 and 1) go by the vocation skill trees, which this build does not hold;
    /// the level, sex and hero-only bits are not modelled either.
    fn may_wear(&self, item: u16, party: &Party) -> bool {
        let Some(rule) = self.rules.as_ref().and_then(|rules| rules.get(&item)) else {
            return true;
        };
        let Some(member) = party.members.first() else {
            return false;
        };
        if !(2..=7).contains(&rule.category) || member.vocation == 0 {
            return true;
        }
        let Some(bit) = 1u16.checked_shl(u32::from(member.vocation) - 1) else {
            return true;
        };
        rule.vocations & bit != 0
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
            Screen::Priest => {
                // State 10 waits for a key, then state 11 ends the visit (`0215af9c`).
                if matches!(button, Button::Confirm | Button::Cancel) {
                    self.close();
                }
                Effect::None
            }
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
                        if let Some(item) = item
                            && !self.may_wear(item, party)
                        {
                            status(&format!(
                                "equip {slot:?}: item {item} refused for vocation {}",
                                party.members.first().map_or(0, |member| member.vocation)
                            ));
                            self.show_line(
                                EQUIP_VOCATION_REFUSED,
                                Vec::new(),
                                NoticeNext::Return,
                                Screen::Equip,
                            );
                            self.cursor = SLOTS
                                .iter()
                                .position(|(candidate, _)| *candidate == slot)
                                .unwrap_or(0);
                            return Effect::None;
                        }
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
                            let rules = self.rules.as_ref();
                            let is_cursed = |item: u16| {
                                rules.is_some_and(|rules| {
                                    rules.get(&item).is_some_and(|rule| rule.cursed)
                                })
                            };
                            let applied = church::cure_with(
                                service,
                                party,
                                &mut self.statuses,
                                index,
                                is_cursed,
                            );
                            match applied {
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
            .or_else(|| {
                self.equipment_messages
                    .as_ref()
                    .and_then(|messages| messages.get(&line).cloned())
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

#[cfg(test)]
mod tests;
