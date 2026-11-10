//! An inn's and a church's words: the system-string files `str_in<k>` and `str_ch<k>`.
//!
//! A talk line ending in `<INN=n>` opens the inn of file `n − 1`, `<CHURCH=n>` the church of
//! file `n − 1` (wiki DQIX *Inns-and-Churches*; `docs/re/facility.md`). Both are `.nat` files
//! in the system-strings layout, which [`MessageTable`] reads (wiki *System Strings*; the same
//! reader as `strbtl`/`actmsg`, [`crate::battle::text`]).
//!
//! An inn's file holds its keeper's lines, the menu labels in messages 0 to 2 and **the price
//! a head in message 10000** (5 at Ivor's, 4 to 12 elsewhere, 0 for the bed of `<INN=15>`). A
//! church's file holds its menu labels in 0 to 5: **a label left empty leaves the service out
//! of the menu** (`str_ch3`, the Observatory, has no Resurrection and no Purification;
//! `str_ch7` has no Confession label).

use crate::battle::text::{Language, MessageTable};
use nds_rom::{Error, Result};

/// Inn files on the cartridge: `str_in0` to `str_in14`, so `<INN=1>` to `<INN=15>`.
pub const INN_FILES: u8 = 15;
/// Church files on the cartridge: `str_ch0` to `str_ch7`, so `<CHURCH=1>` to `<CHURCH=8>`.
pub const CHURCH_FILES: u8 = 8;

/// Message numbers of an inn's file, as the service reads them (wiki *Inns-and-Churches*).
pub mod inn_line {
    /// Menu label: Stay Overnight.
    pub const STAY: u16 = 0;
    /// Menu label: Rest.
    pub const REST: u16 = 1;
    /// Menu label: Cancel.
    pub const CANCEL: u16 = 2;
    /// The keeper's greeting by day; `<val_1>` the beds — the living — `<val_2>` the price.
    pub const GREETING_DAY: u16 = 1000;
    /// The keeper's greeting at night.
    pub const GREETING_NIGHT: u16 = 1001;
    /// The greeting of a guest in another world's inn (no session is modelled here).
    pub const GREETING_GUEST: u16 = 1002;
    /// Said when the party cannot pay.
    pub const TOO_POOR: u16 = 1020;
    /// Said as the screen goes black (the price has been paid).
    pub const RESTING: u16 = 1010;
    /// Said after the rest.
    pub const RESTED: u16 = 1011;
    /// Said when the party leaves.
    pub const LEAVE: u16 = 1090;
    /// The price a head, as decimal digits.
    pub const PRICE: u16 = 10000;
}

/// Message numbers of a church's file, as the service reads them (wiki *Inns-and-Churches*).
pub mod church_line {
    /// The menu's "On whom?" question of a cure.
    pub const ON_WHOM: u16 = 11;
    /// The line each member stands by in the status window (the member's name is `<val_1>`).
    pub const STATUS: u16 = 10;
    /// Dead, on a member's status line.
    pub const DEAD: u16 = 13;
    /// Poisoned.
    pub const POISONED: u16 = 14;
    /// Cursed.
    pub const CURSED: u16 = 15;
    /// Yes.
    pub const YES: u16 = 20;
    /// No.
    pub const NO: u16 = 21;
    /// First vocation name of the divination's line: `30 + the vocation's number`.
    pub const VOCATION: u16 = 30;
    /// The priest's greeting by day.
    pub const GREETING_DAY: u16 = 1000;
    /// The greeting at night.
    pub const GREETING_NIGHT: u16 = 1001;
    /// Divination: the prayer, then one line a member.
    pub const DIVINATION: u16 = 1020;
    /// Divination: the experience the next level wants.
    pub const DIVINATION_EXPERIENCE: u16 = 1021;
    /// Divination: the vocation's path is complete (level 99).
    pub const DIVINATION_COMPLETE: u16 = 1022;
    /// Divination: the next battle brings a level.
    pub const DIVINATION_BATTLE: u16 = 1023;
    /// Resurrection: "whose soul…" (the cure's own question).
    pub const RESURRECTION: u16 = 1030;
    /// Purification.
    pub const PURIFICATION: u16 = 1040;
    /// Benediction.
    pub const BENEDICTION: u16 = 1050;
    /// Refused after the price was shown.
    pub const REFUSED: u16 = 1062;
    /// Too poor.
    pub const TOO_POOR: u16 = 1063;
    /// The price, `<val_1>` gold coins.
    pub const PRICE: u16 = 1061;
    /// Back to the menu after a cure.
    pub const DONE: u16 = 1070;
    /// A member brought in a coffin is raised for nothing (no writer of the flag found).
    pub const COFFIN: u16 = 1100;
}

/// The services of a church's menu, in the order of its labels 0 to 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    /// Confession: saves the adventure log. Not in this build (v0.3).
    Confession,
    /// Divination: free, what the next level wants.
    Divination,
    /// Resurrection: a dead member comes back at full HP.
    Resurrection,
    /// Purification: poison.
    Purification,
    /// Benediction: a curse.
    Benediction,
    /// Nothing: closes the menu.
    Nothing,
}

impl Service {
    /// Every service, in menu order.
    pub const ALL: [Self; 6] = [
        Self::Confession,
        Self::Divination,
        Self::Resurrection,
        Self::Purification,
        Self::Benediction,
        Self::Nothing,
    ];

    /// The label's number in the church's file and the menu's order.
    pub fn label(self) -> u16 {
        self as u16
    }
}

/// A `.nat` file of system strings, with the two numbers both facilities carry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Strings<'a> {
    messages: MessageTable<'a>,
    /// Message 50: the keeper window's style (0 to 3; 3 **inferred** narration).
    style: u8,
}

impl<'a> Strings<'a> {
    fn parse(data: &'a [u8]) -> Result<Self> {
        let messages = MessageTable::parse(data)?;
        let style = match messages.message(50) {
            None => 0,
            Some(bytes) => u8::try_from(number(bytes, "keeper window style")?)
                .map_err(|_| Error::Malformed("keeper window style above 0xff"))?,
        };
        Ok(Self { messages, style })
    }

    fn message(&self, number: u16) -> Option<&'a [u8]> {
        self.messages.message(number)
    }

    fn numbers(&self) -> impl Iterator<Item = u16> + '_ {
        self.messages.numbers()
    }
}

/// An inn's file (`str_in<k>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inn<'a> {
    strings: Strings<'a>,
    /// The price a head, message 10000. 0 for the bed of `<INN=15>`.
    pub price: u32,
}

impl<'a> Inn<'a> {
    /// Parses one `str_in<k>_<language>.nat`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed `.nat` head, record or string, or on a message 50 or
    /// 10000 that is not a decimal number.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let strings = Strings::parse(data)?;
        let price = match strings.message(inn_line::PRICE) {
            None => 0,
            Some(bytes) => number(bytes, "the inn's price")?,
        };
        Ok(Self { strings, price })
    }

    /// The archive a `<INN=n>` tag opens: `data/scenario/str_in<k>.gp2` for `k = n − 1`.
    pub fn archive(tag: u16) -> Option<String> {
        Self::index(tag).map(|k| format!("data/scenario/str_in{k}.gp2"))
    }

    /// The member of that archive, `str_in<k>_<language>.nat`.
    pub fn member(tag: u16, language: Language) -> Option<String> {
        Self::index(tag).map(|k| format!("str_in{k}_{}.nat", language.suffix()))
    }

    fn index(tag: u16) -> Option<u16> {
        tag.checked_sub(1).filter(|&k| k < u16::from(INN_FILES))
    }

    /// The text of message `number`, markup included.
    pub fn message(&self, number: u16) -> Option<&'a [u8]> {
        self.strings.message(number)
    }

    /// The style of the keeper window (message 50).
    pub fn style(&self) -> u8 {
        self.strings.style
    }

    /// The message numbers of the file, in file order.
    pub fn numbers(&self) -> impl Iterator<Item = u16> + '_ {
        self.strings.numbers()
    }
}

/// A church's file (`str_ch<k>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Church<'a> {
    strings: Strings<'a>,
}

impl<'a> Church<'a> {
    /// Parses one `str_ch<k>_<language>.nat`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed `.nat` head, record or string, or on a message 50 that
    /// is not a decimal number.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        Ok(Self {
            strings: Strings::parse(data)?,
        })
    }

    /// The archive a `<CHURCH=n>` tag opens: `data/scenario/str_ch<k>.gp2` for `k = n − 1`.
    pub fn archive(tag: u16) -> Option<String> {
        Self::index(tag).map(|k| format!("data/scenario/str_ch{k}.gp2"))
    }

    /// The member of that archive, `str_ch<k>_<language>.nat`.
    pub fn member(tag: u16, language: Language) -> Option<String> {
        Self::index(tag).map(|k| format!("str_ch{k}_{}.nat", language.suffix()))
    }

    fn index(tag: u16) -> Option<u16> {
        tag.checked_sub(1).filter(|&k| k < u16::from(CHURCH_FILES))
    }

    /// Whether the church offers `service`: its label is not empty.
    pub fn offers(&self, service: Service) -> bool {
        self.strings
            .message(service.label())
            .is_some_and(|text| !text.is_empty())
    }

    /// The services the menu lists, in menu order.
    pub fn services(&self) -> Vec<Service> {
        Service::ALL
            .into_iter()
            .filter(|&service| self.offers(service))
            .collect()
    }

    /// The text of message `number`, markup included.
    pub fn message(&self, number: u16) -> Option<&'a [u8]> {
        self.strings.message(number)
    }

    /// The style of the priest window (message 50).
    pub fn style(&self) -> u8 {
        self.strings.style
    }

    /// The message numbers of the file, in file order.
    pub fn numbers(&self) -> impl Iterator<Item = u16> + '_ {
        self.strings.numbers()
    }
}

/// A message that holds a decimal number, as lines 50 and 10000 do.
fn number(bytes: &[u8], what: &'static str) -> Result<u32> {
    let text = core::str::from_utf8(bytes).map_err(|_| Error::Malformed(what))?;
    text.trim()
        .parse::<u32>()
        .map_err(|_| Error::Malformed(what))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.nat` member in the system-strings layout: a head word, 8-byte records, the strings.
    fn nat(messages: &[(u16, &str)]) -> Vec<u8> {
        let (mut strings, mut records) = (Vec::new(), Vec::new());
        for (number, text) in messages {
            records.extend(u32::from(*number).to_le_bytes());
            records.extend((strings.len() as u32).to_le_bytes());
            strings.extend(text.as_bytes());
            strings.push(0);
        }
        let head = messages.len() as u32 | ((strings.len() as u32) << 12);
        let mut out = head.to_le_bytes().to_vec();
        out.extend(records);
        out.extend(strings);
        out
    }

    fn inn() -> Inn<'static> {
        let data = Box::leak(
            nat(&[
                (0, "Stay Overnight"),
                (1, "Rest"),
                (2, "Cancel"),
                (50, "0"),
                (1000, "by day <val_2>"),
                (1001, "at night <val_2>"),
                (10000, "5"),
            ])
            .into_boxed_slice(),
        );
        Inn::parse(data).unwrap()
    }

    #[test]
    fn an_inn_reads_its_price_style_and_lines() {
        let inn = inn();
        assert_eq!(inn.price, 5);
        assert_eq!(inn.style(), 0);
        assert_eq!(
            inn.message(inn_line::GREETING_NIGHT),
            Some(&b"at night <val_2>"[..])
        );
        assert_eq!(inn.message(3), None);
        assert_eq!(inn.numbers().count(), 7);
    }

    #[test]
    fn a_bed_without_a_price_reads_as_nought() {
        let data = Box::leak(nat(&[(1000, "The bed is lovely"), (10000, "0")]).into_boxed_slice());
        assert_eq!(Inn::parse(data).unwrap().price, 0);
        let data = Box::leak(nat(&[(1000, "no price line")]).into_boxed_slice());
        assert_eq!(Inn::parse(data).unwrap().price, 0);
    }

    #[test]
    fn a_price_that_is_not_a_number_is_an_error() {
        let data = Box::leak(nat(&[(10000, "five")]).into_boxed_slice());
        assert!(Inn::parse(data).is_err());
        let data = Box::leak(nat(&[(50, "<val_1>")]).into_boxed_slice());
        assert!(Inn::parse(data).is_err());
    }

    /// `str_in<k>` is the file of `<INN=k + 1>`: files 0 to 14, tags 1 to 15.
    #[test]
    fn a_tag_names_the_file_before_it() {
        assert_eq!(
            Inn::archive(1).as_deref(),
            Some("data/scenario/str_in0.gp2")
        );
        assert_eq!(
            Inn::member(15, Language::French).as_deref(),
            Some("str_in14_fr.nat")
        );
        assert_eq!(Inn::archive(0), None);
        assert_eq!(Inn::archive(16), None);
        assert_eq!(
            Church::archive(8).as_deref(),
            Some("data/scenario/str_ch7.gp2")
        );
        assert_eq!(Church::member(0, Language::German), None);
        assert_eq!(Church::member(9, Language::German), None);
    }

    fn church(labels: [&str; 6]) -> Church<'static> {
        let mut messages: Vec<(u16, &str)> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| (i as u16, *label))
            .collect();
        messages.push((50, "2"));
        messages.push((1000, "welcome"));
        let data = Box::leak(nat(&messages).into_boxed_slice());
        Church::parse(data).unwrap()
    }

    #[test]
    fn an_empty_label_leaves_the_service_out() {
        let all = church([
            "Confession (Save)",
            "Divination",
            "Resurrection",
            "Purification",
            "Benediction",
            "Nothing",
        ]);
        assert_eq!(all.services(), Service::ALL.to_vec());
        assert!(all.offers(Service::Resurrection));
        assert_eq!(all.style(), 2);

        let observatory = church([
            "Confession (Save)",
            "Divination",
            "",
            "",
            "Benediction",
            "Nothing",
        ]);
        assert!(!observatory.offers(Service::Resurrection));
        assert!(!observatory.offers(Service::Purification));
        assert_eq!(
            observatory.services(),
            vec![
                Service::Confession,
                Service::Divination,
                Service::Benediction,
                Service::Nothing
            ]
        );

        let no_confession = church(["", "Divination", "Resurrection", "Purification", "", ""]);
        assert!(!no_confession.offers(Service::Confession));
        assert!(!no_confession.offers(Service::Nothing));
        assert_eq!(Service::Benediction.label(), 4);
    }
}
