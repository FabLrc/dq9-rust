//! Actions: `actdt_a_<lang>.nat` (63 actions: the healing items and the spells that do not
//! strike), `actdt_b_<lang>.nat` (618 actions: the attack, defending, fleeing, the monsters'
//! moves), their range tables `actdamage_a.nat`/`actdamage_b.nat`, and the names of
//! `actname.nat`. Layout and evidence in `docs/re/battle-data.md`; wiki *Actions*.
//!
//! The range index a record carries is shared by both halves: `actdamage_a` indexes 16, 17,
//! 18 … and `actdamage_b` indexes 1, 2, 3 …, so a lookup must try the half's own table.

use nds_rom::bytes::{slice, u16_at, u32_at};
use nds_rom::{Error, Result};

use crate::battle::FixedRecords;

/// 60-byte action records in both halves.
const RECORD_SIZE: usize = 60;
/// 8-byte range records; the range table opens with a `u32` count.
const RANGE_SIZE: usize = 8;
/// 8-byte `actname.nat` records: a number and a string offset.
const NAME_RECORD_SIZE: usize = 8;

/// One action record. Fields are the ones the wiki names; every other byte is in [`raw`].
///
/// [`raw`]: Action::raw
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action<'a> {
    /// `+0x00`: the name's offset.
    pub name: &'a [u8],
    /// `+0x34`: the plural's offset.
    pub plural: &'a [u8],
    /// `+0x04` bits 0–11: the action's number (the code masks twelve bits).
    pub number: u16,
    /// `+0x04` bits 12–21: the user's number at which a scaling amount leaves its least.
    pub scaled_low: u16,
    /// `+0x04` bits 22–31: the user's number at which it reaches its most.
    pub scaled_high: u16,
    /// `+0x08` low byte: its cost in MP, INFERRED (0 on the 488 actions that are no spell,
    /// 255 on four spells that spend all a caster has).
    pub mp_cost: u8,
    /// `+0x08` bits 8–9 at 1, with [`works_on_metal`](Action::works_on_metal) carried: a blow
    /// that is not a critical deals exactly 0 to a metal body (USA reading).
    pub metal: bool,
    /// `+0x08` bits 14–21: its range, an index into the half's range table (0 for none).
    pub range: u8,
    /// `+0x08` bits 22–26: the element of what it deals, which the target's resistance
    /// multiplies.
    pub element: u8,
    /// `+0x08` bit 29: always a critical (the roll returns 1 without a draw).
    pub always_critical: bool,
    /// `+0x08`: bits 10–13 and 27–28 are not established.
    pub word_08: u32,
    /// `+0x10`: the flags read by the battle code (bits 3 to 24, see the fields below).
    pub word_10: u32,
    /// `+0x10` bit 3: spoilt by a status on the attacker, INFERRED dazzle.
    pub spoilt: bool,
    /// `+0x10` bit 4: defending halves it (EU reading).
    pub halved_by_defending: bool,
    /// `+0x10` bit 5: can be dodged.
    pub dodgeable: bool,
    /// `+0x10` bit 6: can be blocked.
    pub blockable: bool,
    /// `+0x10` bit 11: a blow that lands draws `NextRandomMax(100)`, INFERRED the chance of
    /// waking its target.
    pub wakes: bool,
    /// `+0x10` bit 13: tension works on it and is spent by it.
    pub tension: bool,
    /// `+0x10` bits 14 and 15: the number it scales by, magical might and magical mending.
    pub scales_magical_might: bool,
    pub scales_magical_mending: bool,
    /// `+0x10` bit 16: a party member's trait throws a die of four on the accuracy roll;
    /// which trait is not established (USA reading).
    pub party_trait: bool,
    /// `+0x10` bit 17: strikes several, weakening as it goes (1.0, 0.8, 0.6, 0.4, 0.2).
    pub weakens: bool,
    /// `+0x10` bit 18: an elemental weapon's second product (USA reading).
    pub weapon_element: bool,
    /// `+0x10` bit 24: works on a metal body.
    pub works_on_metal: bool,
    /// `+0x14`: a monster's chance, the party's accuracy and the critical multiplier; the
    /// fields below are slices of it.
    pub word_14: u32,
    /// `+0x14` bits 0–6: a monster's chance with it, in a hundred.
    pub monster_chance: u8,
    /// `+0x14` bits 7–13: a party member's least accuracy with it.
    pub party_accuracy_low: u8,
    /// `+0x14` bits 14–20: a party member's most accuracy with it.
    pub party_accuracy_high: u8,
    /// `+0x14` bits 21–27: the critical chance's multiplier, in hundredths.
    pub critical_multiplier: u8,
    /// `+0x14` bits 28–31 (the high nibble of `+0x17`): whom it reaches. 1 the actor, 2 one,
    /// 3 everyone, 4 a group (INFERRED); 5 the attack's alone; 6 and 8 not established;
    /// 7 used outside battle.
    pub reach: u8,
    /// `+0x18`: the rider, kind, scaling and damage handler; the fields below are slices.
    pub word_18: u32,
    /// `+0x18` bits 0–4: what rides on its blow, a slot of 22 (0 for none).
    pub rider: u8,
    /// `+0x18` bits 5–11: its kind, which picks the handler that applies it (1 deals damage;
    /// 2 heals; 3 to 5 a stat; 6 poison; 8 sleep; 93 numbers in use, most not established).
    pub kind: u8,
    /// `+0x18` bits 16–17: what scales, 1 its accuracy, 2 its amount.
    pub scales: u8,
    /// `+0x18` bits 18–26: which of 67 damage handlers its damage goes through (0 none).
    pub damage_handler: u8,
    /// `+0x18` bits 27–31: the element its landing is resisted by.
    pub resist_element: u8,
    /// `+0x1C`: bits 0–13 the most it can deal, 0 no limit; bits 14–18 not established.
    pub word_1c: u32,
    /// `+0x1C` bits 0–13: the most it can deal; 0 is no limit.
    pub max_amount: u16,
    /// `+0x20`: bits 10–19 the opening message, 20–31 the result message (`actmsg` indices,
    /// INFERRED); bits 0–9 are not established.
    pub word_20: u32,
    /// `+0x20` bits 10–19: how it opens, a message said first (0 for none).
    pub opening_message: u16,
    /// `+0x20` bits 20–31: what it says (0 for none).
    pub result_message: u16,
    /// `+0x24`: the low byte is [`effect`](Action::effect); the word holds three ten-bit
    /// `actmsg` lines — hit at a monster (bits 0–9), failed at a party member (10–19) and at
    /// a monster (20–29).
    pub word_24: u32,
    /// `+0x24` low byte: `0x05` deals damage, INFERRED.
    pub effect: u8,
    /// `+0x28`: the killing lines and the critical's, `actmsg` indices (wiki
    /// *Ability Handlers*).
    pub word_28: u32,
    /// `+0x2C`: bits 0–26 not established; bit 27 is [`combo`](Action::combo).
    pub word_2c: u32,
    /// `+0x2C` bit 27: its blows chain into a combo (damage of at least 1 multiplied by 1.0,
    /// 1.2, 1.5 or 2.0 by the chain counter, `func_ov024_021ea584`).
    pub combo: bool,
    /// `+0x30`: the levels it moves a stat (held to two either way); on a metamorphosis the
    /// monster it becomes; on a kind-15 action the steps of tension it gives.
    pub stat_change: i16,
    /// `+0x32`: the same for its rider.
    pub stat_change_rider: i16,
    /// The whole 60-byte record, for fields not named here.
    pub raw: &'a [u8],
}

impl Action<'_> {
    /// `+0x24` bits 0–9: the `actmsg` line said when it hits a monster.
    pub fn hit_message(&self) -> u16 {
        (self.word_24 & 0x3ff) as u16
    }

    /// `+0x24` bits 10–19: the line said when it fails on a party member.
    pub fn failed_party_message(&self) -> u16 {
        ((self.word_24 >> 10) & 0x3ff) as u16
    }

    /// `+0x24` bits 20–29: the line said when it fails on a monster.
    pub fn failed_monster_message(&self) -> u16 {
        ((self.word_24 >> 20) & 0x3ff) as u16
    }
}

/// One half of the action tables (`actdt_a` or `actdt_b`), with its range table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actions<'a> {
    pub actions: Vec<Action<'a>>,
}

impl<'a> Actions<'a> {
    /// Parses one `<lang>` member of `actdt_a.gp2` or `actdt_b.gp2`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed head word, record or string offset.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = FixedRecords::parse(data, RECORD_SIZE)?;
        let mut actions = Vec::with_capacity(table.count);
        for i in 0..table.count {
            let r = table.record(i)?;
            let word_04 = u32_at(r, 4)?;
            let word_08 = u32_at(r, 8)?;
            let word_10 = u32_at(r, 0x10)?;
            let word_14 = u32_at(r, 0x14)?;
            let word_18 = u32_at(r, 0x18)?;
            let word_1c = u32_at(r, 0x1c)?;
            let word_20 = u32_at(r, 0x20)?;
            let word_24 = u32_at(r, 0x24)?;
            let word_2c = u32_at(r, 0x2c)?;
            let sign = |offset: usize| -> Result<i16> { Ok(u16_at(r, offset)? as i16) };
            actions.push(Action {
                name: table.string(u32_at(r, 0)?)?,
                plural: table.string(u32_at(r, 0x34)?)?,
                number: (word_04 & 0xfff) as u16,
                scaled_low: ((word_04 >> 12) & 0x3ff) as u16,
                scaled_high: ((word_04 >> 22) & 0x3ff) as u16,
                mp_cost: word_08 as u8,
                metal: (word_08 >> 8) & 3 == 1,
                range: ((word_08 >> 14) & 0xff) as u8,
                element: ((word_08 >> 22) & 0x1f) as u8,
                always_critical: word_08 >> 29 & 1 != 0,
                word_08,
                word_10,
                spoilt: word_10 >> 3 & 1 != 0,
                halved_by_defending: word_10 >> 4 & 1 != 0,
                dodgeable: word_10 >> 5 & 1 != 0,
                blockable: word_10 >> 6 & 1 != 0,
                wakes: word_10 >> 11 & 1 != 0,
                tension: word_10 >> 13 & 1 != 0,
                scales_magical_might: word_10 >> 14 & 1 != 0,
                scales_magical_mending: word_10 >> 15 & 1 != 0,
                party_trait: word_10 >> 16 & 1 != 0,
                weakens: word_10 >> 17 & 1 != 0,
                weapon_element: word_10 >> 18 & 1 != 0,
                works_on_metal: word_10 >> 24 & 1 != 0,
                word_14,
                monster_chance: (word_14 & 0x7f) as u8,
                party_accuracy_low: ((word_14 >> 7) & 0x7f) as u8,
                party_accuracy_high: ((word_14 >> 14) & 0x7f) as u8,
                critical_multiplier: ((word_14 >> 21) & 0x7f) as u8,
                reach: (word_14 >> 28) as u8,
                word_18,
                rider: (word_18 & 0x1f) as u8,
                kind: ((word_18 >> 5) & 0x7f) as u8,
                scales: ((word_18 >> 16) & 3) as u8,
                damage_handler: ((word_18 >> 18) & 0x1ff) as u8,
                resist_element: (word_18 >> 27) as u8,
                word_1c,
                max_amount: (word_1c & 0x3fff) as u16,
                word_20,
                opening_message: ((word_20 >> 10) & 0x3ff) as u16,
                result_message: ((word_20 >> 20) & 0xfff) as u16,
                word_24,
                effect: word_24 as u8,
                word_28: u32_at(r, 0x28)?,
                word_2c,
                combo: word_2c >> 27 & 1 != 0,
                stat_change: sign(0x30)?,
                stat_change_rider: sign(0x32)?,
                raw: r,
            });
        }
        Ok(Self { actions })
    }

    /// The action numbered `number`.
    pub fn find(&self, number: u16) -> Option<&Action<'a>> {
        self.actions.iter().find(|a| a.number == number)
    }
}

/// One range table record, `actdamage_a.nat` or `actdamage_b.nat`: the amounts an action's
/// damage or healing is drawn between. All four readings were INFERRED from the reference and
/// then read from `GetAttackBaseDamage` (wiki *Battle resolution*, "The amount").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    /// `+0x00` bits 0–7: the index the action's `+0x08` names.
    pub index: u8,
    /// `+0x00` bits 8–17: a draw between ∓ this is added last, whoever uses the action.
    pub spread: u16,
    /// `+0x04` bits 0–9: a monster's amount.
    pub monster: u16,
    /// `+0x04` bits 10–19: a party member's least, what it is until their number passes the
    /// action's `scaled_low`.
    pub party_least: u16,
    /// `+0x04` bits 20–29: a party member's most, what it is from the action's `scaled_high`.
    pub party_most: u16,
}

/// The range table of one half of the actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranges {
    pub ranges: Vec<Range>,
}

impl Ranges {
    /// Parses `actdamage_a.nat` or `actdamage_b.nat`: a `u32` count, then 8-byte records.
    ///
    /// # Errors
    /// [`nds_rom::Error`] when the records do not fit the file exactly.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let count = u32_at(data, 0)? as usize;
        let len = count
            .checked_mul(RANGE_SIZE)
            .ok_or(Error::Malformed("range table too large"))?;
        if 4 + len != data.len() {
            return Err(Error::Malformed("range table does not end the file"));
        }
        let mut ranges = Vec::with_capacity(count);
        for i in 0..count {
            let r = slice(data, 4 + i * RANGE_SIZE, RANGE_SIZE)?;
            let word_0 = u32_at(r, 0)?;
            let word_4 = u32_at(r, 4)?;
            ranges.push(Range {
                index: word_0 as u8,
                spread: ((word_0 >> 8) & 0x3ff) as u16,
                monster: (word_4 & 0x3ff) as u16,
                party_least: ((word_4 >> 10) & 0x3ff) as u16,
                party_most: ((word_4 >> 20) & 0x3ff) as u16,
            });
        }
        Ok(Self { ranges })
    }

    /// The range record with `index`.
    pub fn find(&self, index: u8) -> Option<&Range> {
        self.ranges.iter().find(|r| r.index == index)
    }
}

/// One `actname.nat` record: an action's number and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionName<'a> {
    /// The record's first word, `number` in its low 12 bits; bits 12 and up are not
    /// established (the first record, number 0 with the empty name, carries 57 there).
    pub word: u32,
    /// Its low 12 bits: the action's number.
    pub number: u16,
    /// `+0x04`: the name's offset.
    pub name: &'a [u8],
}

/// `actname.nat`: action names by number, in the layout of the system strings (a head word,
/// then number and offset records, then the strings). 682 records: the 681 actions and an
/// entry numbered 0 with the empty name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionNames<'a> {
    pub names: Vec<ActionName<'a>>,
}

impl<'a> ActionNames<'a> {
    /// # Errors
    /// [`nds_rom::Error`] on a malformed head word, record or string offset.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = FixedRecords::parse(data, NAME_RECORD_SIZE)?;
        let mut names = Vec::with_capacity(table.count);
        for i in 0..table.count {
            let r = table.record(i)?;
            let word = u32_at(r, 0)?;
            names.push(ActionName {
                word,
                number: (word & 0xfff) as u16,
                name: table.string(u32_at(r, 4)?)?,
            });
        }
        Ok(Self { names })
    }

    /// The name of action `number`.
    pub fn name(&self, number: u16) -> Option<&'a [u8]> {
        self.names
            .iter()
            .find(|n| n.number == number)
            .map(|n| n.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nat(count: u32, records: &[u8], strings: &[u8]) -> Vec<u8> {
        let mut data = ((strings.len() as u32) << 12 | count)
            .to_le_bytes()
            .to_vec();
        data.extend_from_slice(records);
        data.extend_from_slice(strings);
        data
    }

    fn action_record(number: u16) -> [u8; 60] {
        let mut r = [0u8; 60];
        r[0..4].copy_from_slice(&1u32.to_le_bytes());
        let word_04 = u32::from(number) | 50 << 12 | 999 << 22;
        r[4..8].copy_from_slice(&word_04.to_le_bytes());
        // MP 2, range 16, element 1, always a critical.
        let word_08: u32 = 2 | 16 << 14 | 1 << 22 | 1 << 29;
        r[8..12].copy_from_slice(&word_08.to_le_bytes());
        // Dodgeable, tension, works on metal.
        let word_10: u32 = 1 << 5 | 1 << 13 | 1 << 24;
        r[0x10..0x14].copy_from_slice(&word_10.to_le_bytes());
        // Monster chance 12, least 75, most 100, critical 100, reach 2.
        let word_14: u32 = 12 | 75 << 7 | 100 << 14 | 100 << 21 | 2 << 28;
        r[0x14..0x18].copy_from_slice(&word_14.to_le_bytes());
        // Kind 1, scales by amount, handler 45, resisted by fire.
        let word_18 = 1u32 << 5 | 2 << 16 | 45 << 18 | 1 << 27;
        r[0x18..0x1c].copy_from_slice(&word_18.to_le_bytes());
        r[0x1c..0x20].copy_from_slice(&999u32.to_le_bytes());
        // Opening 46, result 22.
        let word_20 = 46u32 << 10 | 22 << 20;
        r[0x20..0x24].copy_from_slice(&word_20.to_le_bytes());
        r[0x24] = 5;
        r[0x2c..0x30].copy_from_slice(&(1u32 << 27).to_le_bytes());
        r[0x30..0x32].copy_from_slice(&1i16.to_le_bytes());
        r[0x32..0x34].copy_from_slice(&(-2i16).to_le_bytes());
        r[0x34..0x38].copy_from_slice(&6u32.to_le_bytes());
        r
    }

    #[test]
    fn action_record_decodes() {
        // Strings: "", "Heal", "heals".
        let data = nat(1, &action_record(30), b"\0Heal\0heals\0");
        let table = Actions::parse(&data).unwrap();
        let a = table.find(30).unwrap();
        assert_eq!(a.name, b"Heal");
        assert_eq!(a.plural, b"heals");
        assert_eq!((a.scaled_low, a.scaled_high), (50, 999));
        assert_eq!(a.mp_cost, 2);
        assert!(!a.metal);
        assert_eq!(a.range, 16);
        assert_eq!(a.element, 1);
        assert!(a.always_critical);
        assert!(a.dodgeable && a.tension && a.works_on_metal);
        assert!(!a.spoilt && !a.blockable);
        assert_eq!(
            (
                a.monster_chance,
                a.party_accuracy_low,
                a.party_accuracy_high,
                a.critical_multiplier,
                a.reach
            ),
            (12, 75, 100, 100, 2)
        );
        assert_eq!((a.kind, a.scales, a.damage_handler), (1, 2, 45));
        assert_eq!(a.resist_element, 1);
        assert_eq!(a.max_amount, 999);
        assert_eq!((a.opening_message, a.result_message), (46, 22));
        assert_eq!(a.hit_message(), 5);
        assert!(a.combo);
        assert_eq!((a.stat_change, a.stat_change_rider), (1, -2));
        assert!(table.find(31).is_none());
    }

    #[test]
    fn combo_bit_and_messages() {
        let record = action_record(1);
        // bit 27 is set in `action_record`'s word_2c.
        let data = nat(
            1,
            &{
                let mut r = record;
                r[0x34..0x38].copy_from_slice(&8u32.to_le_bytes());
                r
            },
            b"\0Attack\0attacks\0",
        );
        let table = Actions::parse(&data).unwrap();
        let a = table.find(1).unwrap();
        assert!(a.combo);
        assert_eq!(a.effect, 5);
    }

    #[test]
    fn range_table_decodes_and_rejects_bad_size() {
        let mut data = 2u32.to_le_bytes().to_vec();
        data.extend_from_slice(&(16u32 | 5 << 8).to_le_bytes());
        data.extend_from_slice(&(35u32 | 35 << 10 | 160 << 20).to_le_bytes());
        data.extend_from_slice(&(49u32 | 5 << 8).to_le_bytes());
        data.extend_from_slice(&(35u32 | 35 << 10 | 35 << 20).to_le_bytes());
        let ranges = Ranges::parse(&data).unwrap();
        let r = ranges.find(16).unwrap();
        assert_eq!(
            (r.spread, r.monster, r.party_least, r.party_most),
            (5, 35, 35, 160)
        );
        assert_eq!(ranges.find(49).unwrap().party_most, 35);
        data.push(0);
        assert!(Ranges::parse(&data).is_err());
    }

    #[test]
    fn action_names_decode_by_number() {
        let mut records = Vec::new();
        records.extend_from_slice(&0u32.to_le_bytes());
        records.extend_from_slice(&0u32.to_le_bytes());
        records.extend_from_slice(&1u32.to_le_bytes());
        records.extend_from_slice(&1u32.to_le_bytes());
        let data = nat(2, &records, b"\0Attack\0");
        let names = ActionNames::parse(&data).unwrap();
        assert_eq!(names.name(0), Some(&b""[..]));
        assert_eq!(names.name(1), Some(&b"Attack"[..]));
        assert_eq!(names.name(2), None);
    }
}
