//! The church: Resurrection, Purification, Benediction, Divination, and the Confession this
//! build does not hold.
//!
//! Source: `func_ov003_02158e94` (the church's flow, overlay 3 of the USA release, read
//! through the dqix-decomp) and the wiki's *Inns-and-Churches*; the prices are the code at
//! `0x0215a8e0`–`0x0215a99c`, the resurrection itself `func_02048150`. The menu, the words and
//! the services a church offers are in its `str_ch<k>` file
//! ([`dq9_formats::facility::inn_church`]): a label left empty leaves the service out.
//!
//! Prices, by the member's level in the vocation they stand in (L):
//!
//! | service | price |
//! |---|---|
//! | Resurrection | ⌊(L² + 20) / 20⌋ × 10 — 10 at L 1, 60 at 10, 460 at 30, 4,910 at 99 |
//! | Purification | 5 |
//! | Benediction | 30 × L |
//!
//! A member who needs nothing hears the service's own line plus two and pays nothing. The
//! rest of the flow is the caller's: the price (1061), a Yes or No, refused 1062, too poor
//! 1063, the prayer (the cure's line plus one) and 1070 back to the menu.
//!
//! **Benediction's unequipping** (`func_ov003_0215a740`, EU overlay 3 state 5): after
//! clearing the cursed bit it walks the eleven in-RAM worn entries (`member+0x150+0x194+0x20
//! i`), skips places 2, 3 and 4 (the slot table at `0x0217f39c` maps them to `0xff`), and for
//! every remaining entry whose word at `+0x08` has **bit 18** set — `(word & 0x7ffff) >> 18`,
//! read on the European tables as 10 items: 12184, 12794, 13307, 18031, 18032, 18038, 20311,
//! 20312, 20913, 21393, all armour or accessories, none a tool — it adds one copy to the bag
//! (`func_0207c378`, EU `0x0207c388`) and clears the place (`func_02052d7c`,
//! EU `0x02052d8c`) with its item id and `-1`. The eight remaining places are exactly the
//! eight slots [`Character::equipment`] holds. **No line announces it**: the branch shows
//! nothing of its own, only the prayer 1051 before and 1070 after; the cursed-item lines of
//! `str_eq` (11004 "…is cursed, and cannot be unequipped", 11005 "Oh no! … is cursed!") belong
//! to the equipment screen, not to the church. The original does not test the bag add; here a
//! bag with no room leaves the piece worn rather than losing it.
//!
//! **The coffin greeting** is `str_ch` line 1100, "…bring a fallen companion all the way to our
//! church…". The church's quick line is `1080 + mode` (`func_ov003_0215af9c`, state 10), so
//! the coffin is mode **20**; the flow only enters state 10 from the greeting when the mode is
//! 1 or 2, the builder resets the mode to 0 (`func_ov003_02158bb8`) and the only setter
//! (`func_ov003_02159174`) is called by the wipe-out task `ov_017::021ba998` with its own mode.
//! No code sets 20, so the line cannot be reached in the retail flow; state 10 ends the visit
//! and performs no resurrection, and the text names no price. The church's paid Resurrection
//! stays the only way the facilities raise the dead.

pub use dq9_formats::facility::inn_church::Service;
use dq9_formats::facility::inn_church::church_line;
use dq9_formats::level::LevelTables;
use dq9_formats::save::EquipSlot;

use crate::party::{BAG_STACK, Character, Party};

/// What the priest asks for a Purification (`0x0215a8e0`–`0x0215a99c`; the same for every
/// member).
pub const PURIFICATION_PRICE: u32 = 5;

/// The bits of a member's status word that [`Party`] does not hold (`[member+0x130]+0`: bit 0
/// dead, 1 poisoned, 2 cursed; wiki *Inns-and-Churches*, § A character's status).
///
/// Dead is not stored: it is the member's HP being 0 (`func_02061bd8`), which [`Party`] holds.
/// What sets the poisoned and the cursed bit is **not established**; the caller hands the
/// flags over the way its own model keeps them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemberStatus {
    pub poisoned: bool,
    pub cursed: bool,
}

impl MemberStatus {
    /// The two bits of a status word.
    pub fn from_word(word: u8) -> Self {
        Self {
            poisoned: word & 0b010 != 0,
            cursed: word & 0b100 != 0,
        }
    }

    /// The status word, the dead bit left clear (HP says it).
    pub fn word(self) -> u8 {
        u8::from(self.poisoned) << 1 | u8::from(self.cursed) << 2
    }

    /// Whether `member` needs `service`: a dead member for Resurrection, a poisoned one for
    /// Purification, a cursed one for Benediction.
    pub fn needs(self, service: Service, member: &Character) -> bool {
        match service {
            Service::Resurrection => member.hp == 0,
            Service::Purification => self.poisoned,
            Service::Benediction => self.cursed,
            Service::Confession | Service::Divination | Service::Nothing => false,
        }
    }
}

/// Resurrection's price: ⌊(L² + 20) / 20⌋ × 10 (`0x0215a8e0`).
pub fn resurrection_price(level: u8) -> u32 {
    let level = u32::from(level);
    (level * level + 20) / 20 * 10
}

/// Benediction's price: 30 × L (`0x0215a8e0`–`0x0215a99c`).
pub fn benediction_price(level: u8) -> u32 {
    30 * u32::from(level)
}

/// What the priest asks for `service` from `member`: Resurrection and Benediction by the
/// member's level in its current vocation, Purification flat. `None` for a service that costs
/// nothing or is not a cure.
pub fn price(service: Service, member: &Character) -> Option<u32> {
    match service {
        Service::Resurrection => Some(resurrection_price(member.level())),
        Service::Purification => Some(PURIFICATION_PRICE),
        Service::Benediction => Some(benediction_price(member.level())),
        Service::Confession | Service::Divination | Service::Nothing => None,
    }
}

/// The line a cure is introduced by: 1030 Resurrection, 1040 Purification, 1050 Benediction.
pub fn cure_line(service: Service) -> Option<u16> {
    match service {
        Service::Resurrection => Some(church_line::RESURRECTION),
        Service::Purification => Some(church_line::PURIFICATION),
        Service::Benediction => Some(church_line::BENEDICTION),
        Service::Confession | Service::Divination | Service::Nothing => None,
    }
}

/// The base of the church's quick line: state 10 shows `1080 + mode`
/// (`func_ov003_0215af9c`).
pub const MODE_LINE: u16 = 1080;

/// The mode whose quick line is the coffin greeting: 1080 + 20 = 1100.
pub const COFFIN_MODE: u8 = 20;

/// `str_ch` line 1100, the coffin greeting. **Unreachable in the retail flow**: nothing sets
/// [`COFFIN_MODE`], and the greeting only enters state 10 for mode 1 or 2.
pub const COFFIN_LINE: u16 = MODE_LINE + COFFIN_MODE as u16;

/// The line state 10 says for `mode`: `1080 + mode`, the wipe-out's 1081/1082 included.
pub fn mode_line(mode: u8) -> u16 {
    MODE_LINE + u16::from(mode)
}

/// The divination's vocation name: `str_ch` message 30 + the vocation's number (1 Warrior …
/// 12 Ranger). Message 30 itself is in no European file, so the Guardian has no name here —
/// **not established** where the game takes it.
pub fn vocation_line(vocation: u8) -> u16 {
    church_line::VOCATION + u16::from(vocation)
}

/// What a divination tells a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Divination {
    /// Line 1021 and the experience still wanted for the next level.
    Experience(u32),
    /// Line 1022: the path of the vocation is complete (level 99).
    Complete,
    /// Line 1023: the next battle brings a level. **[Hypothesis]** — the experience for the
    /// next level is already held (`func_ov023_021f0a5c`'s test), the level being taken after
    /// the battle.
    Battle,
}

impl Divination {
    /// The line the priest says: 1021, 1022 or 1023.
    pub fn line(self) -> u16 {
        match self {
            Self::Experience(_) => church_line::DIVINATION_EXPERIENCE,
            Self::Complete => church_line::DIVINATION_COMPLETE,
            Self::Battle => church_line::DIVINATION_BATTLE,
        }
    }
}

/// The divination of `member` under `tables`: free, and it changes nothing.
pub fn divine(member: &Character, tables: &LevelTables) -> Divination {
    let level = member.level();
    if level >= dq9_formats::level::LEVEL_COUNT as u8 {
        return Divination::Complete;
    }
    let Some(next) = tables.stat(member.vocation, level + 1) else {
        return Divination::Complete;
    };
    let wanted = next.experience.saturating_sub(member.current_experience());
    if wanted == 0 {
        Divination::Battle
    } else {
        Divination::Experience(wanted)
    }
}

/// What a cure did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cure {
    /// Paid and applied.
    Done { price: u32 },
    /// The member does not need it: the cure's line plus two (1032, 1042, 1052).
    NotNeeded,
    /// The party cannot pay: 1063.
    TooPoor { price: u32 },
}

impl Cure {
    /// The line that tells the outcome: the member's state when nothing was needed, the
    /// too-poor line when the gold was short; `None` when the cure was done (the prayer
    /// follows).
    pub fn notice(self, service: Service) -> Option<u16> {
        match self {
            Self::NotNeeded => cure_line(service).map(|line| line + 2),
            Self::TooPoor { .. } => Some(church_line::TOO_POOR),
            Self::Done { .. } => None,
        }
    }

    /// The prayer said as the cure is applied: the cure's line plus one, paid only.
    pub fn prayer(self, service: Service) -> Option<u16> {
        match self {
            Self::Done { .. } => cure_line(service).map(|line| line + 1),
            Self::NotNeeded | Self::TooPoor { .. } => None,
        }
    }
}

/// Why a cure did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChurchError {
    /// The service is not a cure (Confession, Divination, Nothing).
    NotACure(Service),
    /// No member at that place in the party.
    NoMember,
    /// The status list does not run along the party.
    NoStatus,
}

impl core::fmt::Display for ChurchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotACure(service) => write!(f, "{service:?} is not a cure"),
            Self::NoMember => f.write_str("no such party member"),
            Self::NoStatus => f.write_str("no status for that party member"),
        }
    }
}

impl std::error::Error for ChurchError {}

/// Applies `service` to member `index`, **charging the party** [`price`].
///
/// The effects are the game's (`func_02048150` for the resurrection, `func_ov003_02158e94`):
/// Resurrection fills HP and clears every status bit but the curse — MP is as it was; the
/// poisoned bit goes. Purification clears the poisoned bit. Benediction clears the cursed bit
/// and, through [`cure_with`], takes the cursed worn items off.
pub fn cure(
    service: Service,
    party: &mut Party,
    statuses: &mut [MemberStatus],
    index: usize,
) -> Result<Cure, ChurchError> {
    cure_with(service, party, statuses, index, |_| false)
}

/// [`cure`] with the item table's cursed bit, which the caller reads from the ROM.
///
/// Benediction's unequipping is the game's (`func_ov003_0215a740`, state 5): every worn item
/// whose record has bit 18 of its `+0x08` word set goes into the bag, one copy each, and its
/// slot is cleared. The eight slots [`Character::equipment`] holds are exactly the eight worn
/// places the original's slot table keeps (`0x0217f39c` clears places 2, 3 and 4). A bag with
/// no room for a piece leaves it worn — the original does not test the add, and losing the
/// item would not be observable here.
pub fn cure_with(
    service: Service,
    party: &mut Party,
    statuses: &mut [MemberStatus],
    index: usize,
    is_cursed: impl Fn(u16) -> bool,
) -> Result<Cure, ChurchError> {
    let member = party.members.get(index).ok_or(ChurchError::NoMember)?;
    let status = *statuses.get(index).ok_or(ChurchError::NoStatus)?;
    let price = price(service, member).ok_or(ChurchError::NotACure(service))?;
    if !status.needs(service, member) {
        return Ok(Cure::NotNeeded);
    }
    if price > party.gold {
        return Ok(Cure::TooPoor { price });
    }
    party.gold -= price;
    let member = &mut party.members[index];
    let status = &mut statuses[index];
    match service {
        Service::Resurrection => {
            member.hp = member.max_hp;
            status.poisoned = false;
        }
        Service::Purification => status.poisoned = false,
        Service::Benediction => status.cursed = false,
        Service::Confession | Service::Divination | Service::Nothing => {
            return Err(ChurchError::NotACure(service));
        }
    }
    if matches!(service, Service::Benediction) {
        unequip_cursed(party, index, &is_cursed);
    }
    Ok(Cure::Done { price })
}

/// Benediction's unequipping: the cursed worn pieces go into the bag, in slot order.
fn unequip_cursed(party: &mut Party, index: usize, is_cursed: &impl Fn(u16) -> bool) {
    for slot in EquipSlot::ALL {
        let Some(item) = party.members[index].equipment.get(slot) else {
            continue;
        };
        if !is_cursed(item) || !bag_has_room(party, item) {
            continue;
        }
        let _ = party.bag.add(item);
        party.members[index].equipment.set(slot, None);
    }
}

/// Whether the bag can take one more copy of `item` without losing it (`func_0207c378`'s
/// first stack else the first empty slot; a stack holds 99).
fn bag_has_room(party: &Party, item: u16) -> bool {
    match party
        .bag
        .slots()
        .iter()
        .find(|stack| stack.item == Some(item))
    {
        Some(stack) => stack.quantity < BAG_STACK,
        None => party.bag.slots().iter().any(|stack| stack.item.is_none()),
    }
}

/// The line the priest says when the party wakes after a wipe-out: overlay 3's church in
/// mode 2 reads `str_ch<voice - 1>` message `1080 + 2` (`func_ov003_0215af9c`, state 10).
pub const WIPE_OUT_LINE: u16 = 1082;

/// The voice that says nothing: `chur_messet.bin`'s `3` (`func_ov017_0219bfb4`).
pub const WIPE_OUT_SILENT: u8 = 3;

/// `data/scenario/chur_messet.bin`: which priest speaks after a wipe-out.
///
/// `func_ov017_0219bfb4` (mode 2) reads the file once the revival map has loaded: each
/// `0x67` record holds the map, then **three spans of four strings** — the lowest and highest
/// story major it covers (both `0`, or both empty, hold for every major), the voice by day and
/// the voice by night. The first span that holds gives the voice (`atoi`; an empty string is
/// 1). Voice 3 says nothing; any other runs the church in mode 2 with `str_ch<voice - 1>`.
/// A map with no record, or no span that holds, uses voice 1 (the wiki's "the rest").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceTable<'a> {
    records: Vec<(u16, [VoiceSpan<'a>; WIPE_OUT_SPANS])>,
}

/// One `chur_messet.bin` span: the story majors it covers and its two voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VoiceSpan<'a> {
    low: &'a [u8],
    high: &'a [u8],
    day: &'a [u8],
    night: &'a [u8],
}

/// The record kind of a `chur_messet.bin` map entry.
const WIPE_OUT_RECORD: u16 = 0x67;
/// Spans a record holds.
const WIPE_OUT_SPANS: usize = 3;

impl<'a> VoiceTable<'a> {
    /// Parses `data/scenario/chur_messet.bin`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] when the table is malformed or a map does not fit a halfword.
    pub fn parse(data: &'a [u8]) -> nds_rom::Result<Self> {
        let table = dq9_formats::table::Table::parse(data)?;
        let mut records = Vec::new();
        for record in table.records {
            if record.kind != WIPE_OUT_RECORD {
                continue;
            }
            let Some(dq9_formats::table::Value::Int(map)) = record.values.first() else {
                continue;
            };
            let map = u16::try_from(*map)
                .map_err(|_| nds_rom::Error::Malformed("chur_messet map above 0xffff"))?;
            let string = |i: usize| match record.values.get(i) {
                Some(dq9_formats::table::Value::Str(Some(text))) => *text,
                _ => &[][..],
            };
            let mut spans = [VoiceSpan {
                low: &[],
                high: &[],
                day: &[],
                night: &[],
            }; WIPE_OUT_SPANS];
            for (index, span) in spans.iter_mut().enumerate() {
                let at = 1 + 4 * index;
                *span = VoiceSpan {
                    low: string(at),
                    high: string(at + 1),
                    day: string(at + 2),
                    night: string(at + 3),
                };
            }
            records.push((map, spans));
        }
        Ok(Self { records })
    }

    /// The voice of `map` at story major `chapter` and `daytime`; 1 when the file has no
    /// record or no span that holds.
    pub fn voice(&self, map: u16, chapter: u32, daytime: bool) -> u8 {
        let Some((_, spans)) = self.records.iter().find(|(entry, _)| *entry == map) else {
            return 1;
        };
        let Some(span) = spans.iter().find(|span| span.holds(chapter)) else {
            return 1;
        };
        let text = if daytime { span.day } else { span.night };
        if text.is_empty() {
            1
        } else {
            u8::try_from(atoi(text)).unwrap_or(1)
        }
    }
}

impl VoiceSpan<'_> {
    /// `func_ov017_0219bfb4`'s test: both ends 0 hold for every major, otherwise
    /// `low <= chapter <= high`.
    fn holds(&self, chapter: u32) -> bool {
        let (low, high) = (atoi(self.low), atoi(self.high));
        (low == 0 && high == 0) || (low <= chapter && chapter <= high)
    }
}

/// The decimal number a `chur_messet.bin` voice or span string holds; 0 for none.
fn atoi(bytes: &[u8]) -> u32 {
    core::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// The `str_ch` file a voice speaks through: voice 1 and 0 use `str_ch0`, voice 5 `str_ch4`;
/// the silent voice 3 has none.
pub fn wipe_out_voice_file(voice: u8) -> Option<u8> {
    (voice != WIPE_OUT_SILENT).then(|| voice.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::party::{BAG_CAPACITY, Bag};

    fn member(level: u8, hp: u16, max_hp: u16) -> Character {
        let mut member = Character::hero();
        member.vocation = 6;
        member.levels[6] = level;
        member.hp = hp;
        member.max_hp = max_hp;
        member.mp = 3;
        member.max_mp = 9;
        member
    }

    fn party(members: Vec<Character>) -> Party {
        let mut party = Party::new_game();
        party.members = members;
        party.gold = 1000;
        party
    }

    /// The wiki's own table: 10 at level 1, 60 at 10, 460 at 30, 4,910 at 99.
    #[test]
    fn the_prices_follow_the_level_in_the_vocation() {
        assert_eq!(resurrection_price(1), 10);
        assert_eq!(resurrection_price(10), 60);
        assert_eq!(resurrection_price(30), 460);
        assert_eq!(resurrection_price(99), 4_910);
        assert_eq!(benediction_price(1), 30);
        assert_eq!(benediction_price(30), 900);
        assert_eq!(PURIFICATION_PRICE, 5);

        let hero = member(30, 12, 40);
        assert_eq!(price(Service::Resurrection, &hero), Some(460));
        assert_eq!(price(Service::Purification, &hero), Some(5));
        assert_eq!(price(Service::Benediction, &hero), Some(900));
        assert_eq!(price(Service::Divination, &hero), None);
        assert_eq!(price(Service::Nothing, &hero), None);
    }

    #[test]
    fn a_resurrection_fills_hp_clears_poison_and_keeps_the_curse() {
        let mut state = party(vec![member(30, 0, 40)]);
        let mut statuses = [MemberStatus {
            poisoned: true,
            cursed: true,
        }];
        let cure = cure(Service::Resurrection, &mut state, &mut statuses, 0).unwrap();
        assert_eq!(cure, Cure::Done { price: 460 });
        assert_eq!(state.gold, 540);
        assert_eq!(state.members[0].hp, 40, "HP is filled");
        assert_eq!(state.members[0].mp, 3, "MP is as it was");
        assert_eq!(
            statuses[0],
            MemberStatus {
                poisoned: false,
                cursed: true
            },
            "every status bit but the curse"
        );
        assert_eq!(cure.prayer(Service::Resurrection), Some(1031));
        assert_eq!(cure.notice(Service::Resurrection), None);
        assert_eq!(cure_line(Service::Resurrection), Some(1030));
        assert_eq!(cure_line(Service::Benediction), Some(1050));
        assert_eq!(cure_line(Service::Divination), None);
    }

    #[test]
    fn a_member_who_needs_nothing_hears_the_line_two_after() {
        let mut state = party(vec![member(12, 20, 40)]);
        let mut statuses = [MemberStatus::default()];
        let cure = cure(Service::Resurrection, &mut state, &mut statuses, 0).unwrap();
        assert_eq!(cure, Cure::NotNeeded);
        assert_eq!(cure.notice(Service::Resurrection), Some(1032));
        assert_eq!(cure.prayer(Service::Resurrection), None);
        assert_eq!(state.gold, 1000);
        assert_eq!(state.members[0].hp, 20);
    }

    #[test]
    fn purification_costs_five_and_clears_the_poison_alone() {
        let mut state = party(vec![member(12, 20, 40)]);
        let mut statuses = [MemberStatus {
            poisoned: true,
            cursed: true,
        }];
        assert_eq!(
            cure(Service::Purification, &mut state, &mut statuses, 0),
            Ok(Cure::Done { price: 5 })
        );
        assert_eq!(state.gold, 995);
        assert_eq!(
            statuses[0],
            MemberStatus {
                poisoned: false,
                cursed: true
            }
        );
        assert_eq!(state.members[0].hp, 20);
        assert_eq!(Cure::NotNeeded.notice(Service::Purification), Some(1042));
    }

    #[test]
    fn benediction_clears_the_curse_bit_only() {
        let mut state = party(vec![member(12, 20, 40)]);
        let mut statuses = [MemberStatus {
            poisoned: false,
            cursed: true,
        }];
        assert_eq!(
            cure(Service::Benediction, &mut state, &mut statuses, 0),
            Ok(Cure::Done { price: 360 })
        );
        assert_eq!(statuses[0], MemberStatus::default());
        assert_eq!(state.gold, 640);
        assert_eq!(
            Cure::TooPoor { price: 360 }.notice(Service::Benediction),
            Some(1063)
        );
    }

    #[test]
    fn a_cure_without_gold_changes_nothing() {
        let mut state = party(vec![member(30, 0, 40)]);
        state.gold = 459;
        let mut statuses = [MemberStatus::default()];
        assert_eq!(
            cure(Service::Resurrection, &mut state, &mut statuses, 0),
            Ok(Cure::TooPoor { price: 460 })
        );
        assert_eq!(state.gold, 459);
        assert_eq!(state.members[0].hp, 0);
        assert_eq!(
            Cure::TooPoor { price: 460 }.prayer(Service::Resurrection),
            None
        );
        assert_eq!(
            Cure::Done { price: 460 }.notice(Service::Resurrection),
            None
        );
    }

    #[test]
    fn benediction_takes_the_cursed_worn_pieces_into_the_bag() {
        const CURSED: [u16; 3] = [13307, 20913, 12184];
        let mut state = party(vec![member(12, 20, 40)]);
        state.members[0].equipment.torso = Some(CURSED[0]);
        state.members[0].equipment.weapon = Some(CURSED[1]);
        state.members[0].equipment.head = Some(CURSED[2]);
        state.members[0].equipment.shield = Some(21291);
        state.bag = Bag::empty(BAG_CAPACITY);
        let mut statuses = [MemberStatus {
            poisoned: false,
            cursed: true,
        }];
        let cure = cure_with(Service::Benediction, &mut state, &mut statuses, 0, |item| {
            CURSED.contains(&item)
        });
        assert_eq!(cure, Ok(Cure::Done { price: 360 }));
        assert_eq!(state.gold, 640);
        assert_eq!(statuses[0], MemberStatus::default(), "the curse is gone");
        for item in CURSED {
            assert_eq!(state.bag.count(item), 1, "item {item} is in the bag");
        }
        let worn = &state.members[0].equipment;
        assert_eq!(
            (worn.torso, worn.weapon, worn.head),
            (None, None, None),
            "the cursed pieces left their slots"
        );
        assert_eq!(worn.shield, Some(21291), "the shield is not cursed");
    }

    #[test]
    fn a_full_bag_leaves_a_cursed_piece_worn() {
        let mut state = party(vec![member(12, 20, 40)]);
        state.members[0].equipment.torso = Some(13307);
        state.bag = Bag::empty(1);
        assert!(state.bag.add(22000));
        let mut statuses = [MemberStatus {
            poisoned: false,
            cursed: true,
        }];
        assert_eq!(
            cure_with(
                Service::Benediction,
                &mut state,
                &mut statuses,
                0,
                |item| item == 13307,
            ),
            Ok(Cure::Done { price: 360 })
        );
        assert_eq!(
            state.members[0].equipment.torso,
            Some(13307),
            "the piece stays worn rather than being lost"
        );
        assert_eq!(state.bag.count(22000), 1);
        assert!(!statuses[0].cursed, "the cure itself is done");
    }

    #[test]
    fn the_quick_line_is_1080_plus_the_mode_and_the_coffin_is_1100() {
        assert_eq!(mode_line(0), 1080);
        assert_eq!(mode_line(1), 1081);
        assert_eq!(mode_line(2), WIPE_OUT_LINE);
        assert_eq!(COFFIN_MODE, 20);
        assert_eq!(COFFIN_LINE, 1100);
        assert_eq!(mode_line(COFFIN_MODE), COFFIN_LINE);
        // The coffin line is no service's line and no service's refused/notice line.
        for service in [
            Service::Confession,
            Service::Divination,
            Service::Resurrection,
            Service::Purification,
            Service::Benediction,
            Service::Nothing,
        ] {
            for line in [
                cure_line(service),
                cure_line(service).map(|line| line + 1),
                cure_line(service).map(|line| line + 2),
            ] {
                assert_ne!(line, Some(COFFIN_LINE));
            }
        }
    }

    #[test]
    fn the_wrong_service_or_a_missing_member_is_an_error() {
        let mut state = party(vec![member(5, 0, 40)]);
        let mut statuses = [MemberStatus::default()];
        assert_eq!(
            cure(Service::Divination, &mut state, &mut statuses, 0),
            Err(ChurchError::NotACure(Service::Divination))
        );
        assert_eq!(
            cure(Service::Resurrection, &mut state, &mut statuses, 1),
            Err(ChurchError::NoMember)
        );
        let mut none = [];
        assert_eq!(
            cure(Service::Resurrection, &mut state, &mut none, 0),
            Err(ChurchError::NoStatus)
        );
        assert_eq!(state.gold, 1000);
    }

    #[test]
    fn the_status_word_carries_the_two_bits_but_not_death() {
        assert_eq!(
            MemberStatus::from_word(0b111),
            MemberStatus {
                poisoned: true,
                cursed: true
            }
        );
        assert_eq!(MemberStatus::from_word(0b001).word(), 0);
        assert_eq!(MemberStatus::default().word(), 0);
        assert_eq!(vocation_line(1), church_line::VOCATION + 1);
    }

    #[test]
    fn the_divination_lines_are_1021_to_1023() {
        assert_eq!(Divination::Experience(120).line(), 1021);
        assert_eq!(Divination::Complete.line(), 1022);
        assert_eq!(Divination::Battle.line(), 1023);
    }

    /// A tagged table (`data/scenario/chur_messet.bin`'s layout): each record a `0x67` with
    /// its map and `spans` of `(low, high, day, night)`.
    fn chur_messet(records: &[(u16, [[&str; 4]; 3])]) -> Vec<u8> {
        fn intern(pool: &mut Vec<u8>, text: &str) -> u32 {
            let at = pool.len() as u32;
            pool.extend_from_slice(text.as_bytes());
            pool.push(0);
            at
        }
        let (mut body, mut pool) = (Vec::new(), Vec::new());
        for (map, spans) in records {
            body.extend_from_slice(&0x67u16.to_le_bytes());
            body.push(13);
            // Field 0 is the map, the twelve others are strings.
            body.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
            body.push(0xff);
            body.extend_from_slice(&u32::from(*map).to_le_bytes());
            for span in spans {
                for text in span {
                    body.extend_from_slice(&intern(&mut pool, text).to_le_bytes());
                }
            }
        }
        let pool_at = (0x10 + body.len()).next_multiple_of(16);
        let mut out = Vec::new();
        for word in [records.len(), pool_at, pool.len(), 0] {
            out.extend_from_slice(&(word as u32).to_le_bytes());
        }
        out.extend_from_slice(&body);
        out.resize(pool_at, 0xff);
        out.extend_from_slice(&pool);
        out.resize(out.len().next_multiple_of(16), 0);
        out
    }

    #[test]
    fn the_wipe_out_voice_follows_the_story_major_and_the_hour() {
        let data = chur_messet(&[
            // Angel Falls: silent at major 1, voice 1 from 2 to 17, then the catch-all.
            (
                1106,
                [
                    ["1", "1", "3", "3"],
                    ["2", "17", "1", "1"],
                    ["0", "0", "0", "0"],
                ],
            ),
            // Gleeba: voice 1 by day, 2 by night, up to 16.
            (
                216,
                [
                    ["1", "16", "1", "2"],
                    ["17", "17", "1", "1"],
                    ["0", "0", "0", "0"],
                ],
            ),
            // The Observatory: always voice 4 (both ends 0 hold for every major).
            (
                4506,
                [
                    ["0", "0", "4", "4"],
                    ["0", "0", "0", "0"],
                    ["0", "0", "0", "0"],
                ],
            ),
        ]);
        let table = VoiceTable::parse(&data).expect("chur_messet");
        assert_eq!(table.voice(1106, 1, true), WIPE_OUT_SILENT);
        assert_eq!(table.voice(1106, 1, false), WIPE_OUT_SILENT);
        assert_eq!(table.voice(1106, 2, true), 1);
        assert_eq!(table.voice(1106, 17, false), 1);
        assert_eq!(table.voice(1106, 18, true), 0, "the catch-all span");
        assert_eq!(table.voice(216, 1, true), 1);
        assert_eq!(table.voice(216, 1, false), 2);
        assert_eq!(table.voice(216, 17, true), 1);
        assert_eq!(table.voice(4506, 9, true), 4);
        assert_eq!(table.voice(999, 1, true), 1, "a map with no record");
        assert_eq!(wipe_out_voice_file(0), Some(0));
        assert_eq!(wipe_out_voice_file(1), Some(0));
        assert_eq!(wipe_out_voice_file(5), Some(4));
        assert_eq!(wipe_out_voice_file(WIPE_OUT_SILENT), None);
    }

    #[test]
    fn the_cartridges_wipe_out_voices() {
        let Some(rom) = testrom::load() else {
            return;
        };
        let image = nds_rom::Rom::parse(rom).expect("the ROM");
        let data = image
            .file_by_path("data/scenario/chur_messet.bin")
            .expect("chur_messet.bin");
        let table = VoiceTable::parse(data).expect("chur_messet.bin");
        assert_eq!(table.voice(1106, 1, true), WIPE_OUT_SILENT);
        assert_eq!(table.voice(1106, 2, true), 1);
        assert_eq!(table.voice(1106, 17, false), 1);
        assert_eq!(table.voice(216, 2, true), 1);
        assert_eq!(table.voice(216, 2, false), 2);
        assert_eq!(table.voice(4506, 2, true), 4);
        assert_eq!(table.voice(5700, 2, true), 7);
        assert_eq!(table.voice(2309, 2, true), 1);
        assert_eq!(table.voice(999, 2, true), 1, "the rest");
    }
}
