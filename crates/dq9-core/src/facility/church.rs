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

pub use dq9_formats::facility::inn_church::Service;
use dq9_formats::facility::inn_church::church_line;
use dq9_formats::level::LevelTables;

use crate::party::{Character, Party};

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
/// poisoned bit goes. Purification clears the poisoned bit. **Benediction clears the cursed
/// bit only**: the game also takes every worn item whose slot word has bit 18 set into the
/// bag, and no reader here holds that bit, so the unequipping is **not modelled**
/// (**[unknown]** which items).
pub fn cure(
    service: Service,
    party: &mut Party,
    statuses: &mut [MemberStatus],
    index: usize,
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
    Ok(Cure::Done { price })
}

/// What this build does with a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// The service runs here.
    Runs,
    /// The service does not: the text to show.
    Unavailable(&'static str),
}

/// The text [`Offer::Unavailable`] gives for the Confession, which needs saving (`docs/status.md`,
/// v0.3).
pub const NOT_AVAILABLE: &str = "not available in this build";

/// Whether a service runs in this build: everything but the Confession, which saves the
/// adventure log and ends the game on "No" (wiki *Inns-and-Churches*, § The church).
pub fn offer(service: Service) -> Offer {
    match service {
        Service::Confession => Offer::Unavailable(NOT_AVAILABLE),
        Service::Divination
        | Service::Resurrection
        | Service::Purification
        | Service::Benediction
        | Service::Nothing => Offer::Runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_confession_is_not_in_this_build() {
        assert_eq!(
            offer(Service::Confession),
            Offer::Unavailable("not available in this build")
        );
        assert_eq!(NOT_AVAILABLE, "not available in this build");
        for service in [
            Service::Divination,
            Service::Resurrection,
            Service::Purification,
            Service::Benediction,
            Service::Nothing,
        ] {
            assert_eq!(offer(service), Offer::Runs);
        }
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
}
