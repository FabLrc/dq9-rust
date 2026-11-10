//! The inn: what a stay or a rest costs and what it does.
//!
//! Source: `func_ov003_0215c924` (the inn's flow, overlay 3 of the USA release, read through
//! the dqix-decomp) and the wiki's *Inns-and-Churches*; the price arithmetic is at
//! `0x0215d0d4`–`0x0215d108` and the clock's writer is `GameState::SetTimeOfDay`
//! (`0x02010374`). EU-only prices and lines are in the `str_in<k>` files
//! ([`dq9_formats::facility::inn_church`]).
//!
//! **The price is a head's price times the living** — a member with no HP needs no bed
//! (`func_02061bd8`'s test is HP 0; `docs/re/party.md`). The file's line 10000 holds the
//! head's price (5 at Ivor's, 4 to 12 elsewhere, 0 for the bed of `<INN=15>`).
//!
//! **Stay** puts the clock at the day's start (210 s), **Rest** at the night's start (0 s);
//! the menu offers Rest by day alone (wiki *Time-Of-Day*, § What sets it). After either, the
//! living's HP and MP are full: the dead are not raised, and poison or a curse is not touched.
//!
//! **[Hypothesis]** the keeper's greeting and the menu labels are chosen by the time alone;
//! the guest's greeting (line 1002) is a second console's session, which this build has not.

use dq9_formats::facility::inn_church::inn_line;

use crate::party::Party;
use crate::state::{DayClock, TimeOfDay};

/// The Quester's Rest counter's price a head, a constant in code (`0x0215d0c4`) whatever the
/// inn's own file says (wiki *The Quester's Rest*, § The counter). Its flow and its word file
/// (`str_rki`, not a `str_in`) are v0.3's.
pub const QUESTERS_REST_PRICE: u32 = 3;

/// What the keeper asks after the greeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Stay Overnight: the clock to the day's start.
    Stay,
    /// Rest: the clock to the night's start. Offered by day alone.
    Rest,
    /// Cancel.
    Cancel,
}

/// The menu's choices in order: Stay, Rest, Cancel by day; Stay and Cancel at night
/// (`func_ov003_0215c924`; labels 0, 1 and 2 of the file).
pub fn menu(time: TimeOfDay) -> &'static [Choice] {
    if time.is_daytime() {
        &[Choice::Stay, Choice::Rest, Choice::Cancel]
    } else {
        &[Choice::Stay, Choice::Cancel]
    }
}

/// The living: the members a bed is charged for (`func_02061bd8`: HP 0 is down).
pub fn living(party: &Party) -> u32 {
    party.members.iter().filter(|m| m.hp > 0).count() as u32
}

/// The price of a night or a rest: the head's price times the living (`0x0215d0d4`).
pub fn price(per_head: u32, party: &Party) -> u32 {
    per_head.saturating_mul(living(party))
}

/// The keeper's greeting: line 1000 by day, 1001 at night, 1002 for a guest (not modelled:
/// no session).
pub fn greeting(time: TimeOfDay) -> u16 {
    if time.is_daytime() {
        inn_line::GREETING_DAY
    } else {
        inn_line::GREETING_NIGHT
    }
}

/// What a confirmed Stay or Rest did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stay {
    /// The price was paid, the living healed and the clock set: the keeper says
    /// [`Self::line`], the screen goes black with jingle 55, a hold of 180 ticks, then
    /// [`Self::closing_line`].
    Paid { price: u32 },
    /// The party cannot pay: [`Self::line`], and nothing else happens.
    TooPoor { price: u32 },
}

impl Stay {
    /// The line the keeper says as the choice is confirmed: 1010 paid, 1020 too poor.
    pub fn line(&self) -> u16 {
        match self {
            Self::Paid { .. } => inn_line::RESTING,
            Self::TooPoor { .. } => inn_line::TOO_POOR,
        }
    }

    /// The line said after the rest (1011); `None` when nothing was paid.
    pub fn closing_line(&self) -> Option<u16> {
        match self {
            Self::Paid { .. } => Some(inn_line::RESTED),
            Self::TooPoor { .. } => None,
        }
    }

    /// The price asked, paid or not.
    pub fn price(&self) -> u32 {
        match *self {
            Self::Paid { price } | Self::TooPoor { price } => price,
        }
    }
}

/// The line of a cancelled visit (1090).
pub fn cancel() -> u16 {
    inn_line::LEAVE
}

/// Stay Overnight: the price paid, **the living's HP and MP full**, the clock at the day's
/// start (`GameState::SetTimeOfDay`, `0x02010374`, with phase [`TimeOfDay::Day`]).
///
/// The dead keep their 0 HP, and nothing reads or writes poison or a curse here.
pub fn stay(party: &mut Party, clock: &mut DayClock, per_head: u32) -> Stay {
    pay(party, clock, per_head, TimeOfDay::Day)
}

/// Rest: as [`stay`], the clock at the night's start ([`TimeOfDay::Night`]). The menu offers
/// it by day alone; the rule itself does not test the hour.
pub fn rest(party: &mut Party, clock: &mut DayClock, per_head: u32) -> Stay {
    pay(party, clock, per_head, TimeOfDay::Night)
}

/// The flow both choices share: pay, heal the living, set the clock, then 1011.
fn pay(party: &mut Party, clock: &mut DayClock, per_head: u32, phase: TimeOfDay) -> Stay {
    let price = price(per_head, party);
    if price > party.gold {
        return Stay::TooPoor { price };
    }
    party.gold -= price;
    for member in &mut party.members {
        if member.hp > 0 {
            member.hp = member.max_hp;
            member.mp = member.max_mp;
        }
    }
    clock.set_phase(phase);
    Stay::Paid { price }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::party::Character;

    fn member(hp: u16, max_hp: u16) -> Character {
        let mut member = Character::hero();
        member.levels[usize::from(member.vocation)] = 7;
        member.hp = hp;
        member.max_hp = max_hp;
        member.mp = 0;
        member.max_mp = 12;
        member
    }

    /// Four beds: the hero and two companions, and one member down.
    fn party() -> Party {
        let mut party = Party::new_game();
        party.members[0] = member(10, 40);
        party.members.push(member(0, 30));
        party.members.push(member(30, 30));
        party.members.push(member(1, 50));
        party.gold = 100;
        party
    }

    #[test]
    fn the_price_is_a_heads_price_times_the_living() {
        let party = party();
        assert_eq!(living(&party), 3);
        assert_eq!(price(5, &party), 15);
        let mut all_down = party.clone();
        for member in &mut all_down.members {
            member.hp = 0;
        }
        assert_eq!(price(5, &all_down), 0);
    }

    #[test]
    fn the_menu_offers_rest_by_day_alone() {
        assert_eq!(
            menu(TimeOfDay::Day),
            [Choice::Stay, Choice::Rest, Choice::Cancel]
        );
        assert_eq!(
            menu(TimeOfDay::Morning),
            [Choice::Stay, Choice::Rest, Choice::Cancel]
        );
        assert_eq!(
            menu(TimeOfDay::Evening),
            [Choice::Stay, Choice::Rest, Choice::Cancel]
        );
        assert_eq!(menu(TimeOfDay::Night), [Choice::Stay, Choice::Cancel]);
        assert_eq!(greeting(TimeOfDay::Day), inn_line::GREETING_DAY);
        assert_eq!(greeting(TimeOfDay::Evening), inn_line::GREETING_DAY);
        assert_eq!(greeting(TimeOfDay::Night), inn_line::GREETING_NIGHT);
    }

    #[test]
    fn a_stay_pays_heals_the_living_and_sets_the_days_start() {
        let mut state = party();
        let mut clock = DayClock::new(400.0, true);
        let stay = stay(&mut state, &mut clock, 5);
        assert_eq!(stay, Stay::Paid { price: 15 });
        assert_eq!(state.gold, 85);
        assert_eq!(state.members[0].hp, 40);
        assert_eq!(state.members[0].mp, 12);
        assert_eq!(state.members[1].hp, 0, "the dead are not raised");
        assert_eq!(state.members[2].hp, 30);
        assert_eq!(state.members[3].hp, 50);
        assert_eq!(clock.seconds(), 210.0);
        assert_eq!(clock.phase(), TimeOfDay::Day);
        assert_eq!(stay.line(), inn_line::RESTING);
        assert_eq!(stay.closing_line(), Some(inn_line::RESTED));
    }

    #[test]
    fn a_rest_sets_the_nights_start() {
        let mut state = party();
        let mut clock = DayClock::new(250.0, true);
        assert_eq!(rest(&mut state, &mut clock, 4), Stay::Paid { price: 12 });
        assert_eq!(clock.seconds(), 0.0);
        assert_eq!(clock.phase(), TimeOfDay::Night);
    }

    #[test]
    fn too_poor_pays_nothing_and_heals_nothing() {
        let mut state = party();
        state.gold = 14;
        let mut clock = DayClock::new(400.0, true);
        let stay = stay(&mut state, &mut clock, 5);
        assert_eq!(stay, Stay::TooPoor { price: 15 });
        assert_eq!(state.gold, 14);
        assert_eq!(state.members[0].hp, 10);
        assert_eq!(clock.seconds(), 400.0);
        assert_eq!(stay.line(), inn_line::TOO_POOR);
        assert_eq!(stay.closing_line(), None);
        assert_eq!(stay.price(), 15);
        assert_eq!(cancel(), inn_line::LEAVE);
    }

    #[test]
    fn the_questers_rest_price_is_three_a_head() {
        assert_eq!(QUESTERS_REST_PRICE, 3);
        assert_eq!(price(QUESTERS_REST_PRICE, &party()), 9);
    }
}
