//! Monsters: the list (`mon_list_<lang>.nat`), the battle data (`mon_btldata.nat`) and the
//! name records (`mon_data_<lang>.nat`). Layout and evidence in `docs/re/battle-data.md`;
//! wiki *Monsters*.
//!
//! The list is read for a monster's code and name per language; the battle data and the name
//! records are read side by side, their monster numbers agreeing on all 438 records.

use nds_rom::bytes::{slice, u16_at, u32_at};
use nds_rom::{Error, Result};

use crate::battle::FixedRecords;

/// `.nat` member of the name records, by language: `mon_data.gp2/mon_data_<lang>.nat`.
pub const NAME_MEMBER_PREFIX: &str = "mon_data_";
/// `.nat` member of the list, by language: `mon_list.gp2/mon_list_<lang>.nat`.
pub const LIST_MEMBER_PREFIX: &str = "mon_list_";
/// 32-byte monster list records.
const LIST_RECORD_SIZE: usize = 0x20;
/// 132-byte battle data records.
const BATTLE_RECORD_SIZE: usize = 0x84;
/// 28-byte name records.
const NAME_RECORD_SIZE: usize = 0x1c;

/// One monster list record: a code and a name, in one language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListRecord<'a> {
    /// `+0x0C`: the monster's number (the key of the other two tables).
    pub number: u16,
    /// `+0x04`: the code, e.g. `z000a` (a letter, three digits, a letter).
    pub code: &'a [u8],
    /// `+0x08`: the name, e.g. `slime`.
    pub name: &'a [u8],
    /// `+0x00`: 0 on every record.
    pub zero: u32,
    /// `+0x0E`: not established (364, 356, 246 … on the first three).
    pub unknown: u16,
    /// `+0x10`: 16 bytes, not established.
    pub unknown_10: [u8; 0x10],
}

/// The monster list of one language, `mon_list_<lang>.nat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List<'a> {
    pub monsters: Vec<ListRecord<'a>>,
}

impl<'a> List<'a> {
    /// Parses `mon_list_<lang>.nat`.
    ///
    /// # Errors
    /// [`nds_rom::Error`] on a malformed head word, record or string offset.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = FixedRecords::parse(data, LIST_RECORD_SIZE)?;
        let mut monsters = Vec::with_capacity(table.count);
        for i in 0..table.count {
            let r = table.record(i)?;
            monsters.push(ListRecord {
                zero: u32_at(r, 0)?,
                number: u16_at(r, 0x0c)?,
                code: table.string(u32_at(r, 4)?)?,
                name: table.string(u32_at(r, 8)?)?,
                unknown: u16_at(r, 0x0e)?,
                unknown_10: slice(r, 0x10, 0x10)?
                    .try_into()
                    .map_err(|_| Error::Malformed("list record bytes"))?,
            });
        }
        Ok(Self { monsters })
    }

    /// The record naming `number`.
    pub fn find_number(&self, number: u16) -> Option<&ListRecord<'a>> {
        self.monsters.iter().find(|m| m.number == number)
    }

    /// The record whose code is `code`, ASCII case-insensitively.
    pub fn find_code(&self, code: &[u8]) -> Option<&ListRecord<'a>> {
        self.monsters
            .iter()
            .find(|m| m.code.eq_ignore_ascii_case(code))
    }

    /// The record whose name is `name`, ASCII case-insensitively (several records share a
    /// name, so the first listed wins).
    pub fn find_name(&self, name: &[u8]) -> Option<&ListRecord<'a>> {
        self.monsters
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

/// One battle data record, `mon_btldata.nat`: what a monster is in a battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleMonster<'a> {
    /// `+0x00` bits 0–14: the monster's number.
    pub number: u16,
    /// `+0x00` bit 15: set on 388 of the 438 records of this ROM (the wiki gives all 438;
    /// what it marks is **[unknown]**).
    pub number_flag: bool,
    /// `+0x02`: each drop's chance step, the ordinary drop's then the rare's. 1 to 6 are one
    /// in `2^(step+2)`; 7 never drops; 0 is 100% (the word at `0x021fd888`).
    pub drop_steps: [u8; 2],
    /// `+0x04`: the two drops, ordinary then rare (item ids).
    pub drops: [u16; 2],
    /// `+0x08`: experience.
    pub experience: u32,
    /// `+0x0C`: gold.
    pub gold: u16,
    /// `+0x10`: bits 5–7 the selector, 13–15 the evasion grade, 16–18 the block grade, 20–25
    /// the once-a-battle mask; the rest not established.
    pub word_10: u32,
    /// `+0x14`: not established (500 to 605 on ordinary monsters, 0 on most bosses).
    pub unknown_14: u32,
    /// `+0x18`: its six ways of acting (action numbers).
    pub ways: [u16; 6],
    /// `+0x24`: what enrages it: bits 0–6 a provocation, 7–13 its chance in 100, 14–20 a
    /// second, 21–27 its chance. Read by `func_ov024_021eb08c`; the kinds are listed in the
    /// wiki, *Ability Handlers*.
    pub enrage: u32,
    /// `+0x28`: the least and most passes Knight Watch holds it (`func_ov024_021e1de8`).
    pub knight_watch: [u8; 2],
    /// `+0x5C`: maximum HP (a battle's HP is drawn from 0.8 to 1.0 of it).
    pub hp: u16,
    /// `+0x5E`: maximum MP.
    pub mp: u16,
    /// `+0x60`: attack.
    pub attack: u16,
    /// `+0x62`: defence.
    pub defence: u16,
    /// `+0x64`: agility.
    pub agility: u16,
    /// `+0x68`: three 10-bit numbers copied into the battle status; not established.
    pub status_bits: u32,
    /// `+0x6C`: what it takes of each element, in hundredths, by `element − 1`; 0 immune,
    /// 100 whole, 125 a quarter more. Byte 7 is element 8, the plain Attack's, 100 on all
    /// 438; byte 21 (element 22) is unused.
    pub resistances: [u8; 22],
    /// `+0x82`: copied beside the resistances; not established (0 on every monster read).
    pub unknown_82: [u8; 2],
    /// The whole 132-byte record, for fields not named here (the rest is not established).
    pub raw: &'a [u8],
}

impl BattleMonster<'_> {
    /// `+0x10` bits 5–7: which of eight handlers chooses among the six ways
    /// (`func_0208a91c`; see the wiki, *Battle AI*).
    pub fn selector(&self) -> u8 {
        ((self.word_10 >> 5) & 7) as u8
    }

    /// `+0x10` bits 13–15: its evasion grade, an index into the table `0, 2, 4, 8, 25` (the
    /// float rate in a hundred) by which `func_ov000_02156270` gets its evasion
    /// (`func_02074988` indexes the table).
    pub fn evasion_grade(&self) -> u8 {
        ((self.word_10 & 0xffff) >> 13) as u8
    }

    /// `+0x10` bits 16–18: its block grade, through the same table as its evasion
    /// (`func_ov000_02156118`).
    pub fn block_grade(&self) -> u8 {
        ((self.word_10 >> 16) & 7) as u8
    }

    /// `+0x10` bits 20–25: which of the six ways may be used once a battle (`0x0208a0a0`).
    pub fn once_mask(&self) -> u8 {
        ((self.word_10 >> 20) & 0x3f) as u8
    }

    /// `+0x27` bit 4 (bit 28 of the word at `+0x24`): set on the bosses, read by no
    /// instruction in this ROM.
    pub fn boss(&self) -> bool {
        self.enrage >> 28 & 1 != 0
    }

    /// Its resistance to `element` (1 to 22) in hundredths; `None` outside that range.
    pub fn resistance(&self, element: u8) -> Option<u8> {
        self.resistances
            .get(usize::from(element).checked_sub(1)?)
            .copied()
    }
}

/// `mon_btldata.nat`: 438 battle data records, no strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleData<'a> {
    pub monsters: Vec<BattleMonster<'a>>,
}

impl<'a> BattleData<'a> {
    /// # Errors
    /// [`nds_rom::Error`] on a malformed head word or record.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = FixedRecords::parse(data, BATTLE_RECORD_SIZE)?;
        let mut monsters = Vec::with_capacity(table.count);
        for i in 0..table.count {
            let r = table.record(i)?;
            let number = u16_at(r, 0)?;
            let mut resistances = [0; 22];
            resistances.copy_from_slice(slice(r, 0x6c, 22)?);
            monsters.push(BattleMonster {
                number: number & 0x7fff,
                number_flag: number & 0x8000 != 0,
                drop_steps: [r[0x02], r[0x03]],
                drops: [u16_at(r, 0x04)?, u16_at(r, 0x06)?],
                experience: u32_at(r, 0x08)?,
                gold: u16_at(r, 0x0c)?,
                word_10: u32_at(r, 0x10)?,
                unknown_14: u32_at(r, 0x14)?,
                ways: [
                    u16_at(r, 0x18)?,
                    u16_at(r, 0x1a)?,
                    u16_at(r, 0x1c)?,
                    u16_at(r, 0x1e)?,
                    u16_at(r, 0x20)?,
                    u16_at(r, 0x22)?,
                ],
                enrage: u32_at(r, 0x24)?,
                knight_watch: [r[0x28], r[0x29]],
                hp: u16_at(r, 0x5c)?,
                mp: u16_at(r, 0x5e)?,
                attack: u16_at(r, 0x60)?,
                defence: u16_at(r, 0x62)?,
                agility: u16_at(r, 0x64)?,
                status_bits: u32_at(r, 0x68)?,
                resistances,
                unknown_82: [r[0x82], r[0x83]],
                raw: r,
            });
        }
        Ok(Self { monsters })
    }

    /// The battle data of `number`.
    pub fn find(&self, number: u16) -> Option<&BattleMonster<'a>> {
        self.monsters.iter().find(|m| m.number == number)
    }
}

/// One name and grammar record, `mon_data_<lang>.nat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameRecord<'a> {
    /// `+0x08`: the monster's number.
    pub number: u16,
    /// `+0x00`: the name (`slime`).
    pub name: &'a [u8],
    /// `+0x14`: the plural (`slimes`).
    pub plural: &'a [u8],
    /// `+0x04`: the code (`z000a`).
    pub code: &'a [u8],
    /// `+0x0A` bits 0–6: the level.
    pub level: u8,
    /// `+0x0A` bits 7–10: the family (the killer-bonus families of `itembtlprm.nat`).
    pub family: u8,
    /// `+0x0A` bit 11: a fall off its feet is refused (`func_ov000_02156068` with 2).
    pub refuses_fall: bool,
    /// `+0x0A` bit 12: metal.
    pub metal: bool,
    /// `+0x0C`: its body's collision radius, in 1024ths (EU reading, `func_ov017_021a2128`).
    pub radius: i16,
    /// `+0x0E`: its body's collision height, `fx32` (EU reading).
    pub height: i16,
    /// `+0x10`: its kind, one value to a monster across its story versions, 1 to `0x133`,
    /// the bosses from `0x101` (the command phase's fixed shots).
    pub kind: u16,
    /// `+0x12`: its size in battle, in 4096ths (EU reading, `func_02048588`).
    pub battle_size: i16,
    /// `+0x18`: the name's grammar, its articles and gender (wiki *Articles*); not read.
    pub grammar: u32,
}

/// `mon_data_<lang>.nat`: 438 name records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names<'a> {
    pub monsters: Vec<NameRecord<'a>>,
}

impl<'a> Names<'a> {
    /// # Errors
    /// [`nds_rom::Error`] on a malformed head word, record or string offset.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let table = FixedRecords::parse(data, NAME_RECORD_SIZE)?;
        let mut monsters = Vec::with_capacity(table.count);
        for i in 0..table.count {
            let r = table.record(i)?;
            let level = u16_at(r, 0x0a)?;
            // `s16` fields: the two's-complement bits, read back as a signed value.
            let sign = |offset: usize| -> Result<i16> { Ok(u16_at(r, offset)? as i16) };
            monsters.push(NameRecord {
                number: u16_at(r, 0x08)?,
                name: table.string(u32_at(r, 0x00)?)?,
                plural: table.string(u32_at(r, 0x14)?)?,
                code: table.string(u32_at(r, 0x04)?)?,
                level: (level & 0x7f) as u8,
                family: ((level >> 7) & 0xf) as u8,
                refuses_fall: level >> 11 & 1 != 0,
                metal: level >> 12 & 1 != 0,
                radius: sign(0x0c)?,
                height: sign(0x0e)?,
                kind: u16_at(r, 0x10)?,
                battle_size: sign(0x12)?,
                grammar: u32_at(r, 0x18)?,
            });
        }
        Ok(Self { monsters })
    }

    /// The record of `number`.
    pub fn find(&self, number: u16) -> Option<&NameRecord<'a>> {
        self.monsters.iter().find(|m| m.number == number)
    }

    /// The first record whose code is `code`, ASCII case-insensitively.
    pub fn find_code(&self, code: &[u8]) -> Option<&NameRecord<'a>> {
        self.monsters
            .iter()
            .find(|m| m.code.eq_ignore_ascii_case(code))
    }

    /// The first record whose name is `name`, ASCII case-insensitively.
    pub fn find_name(&self, name: &[u8]) -> Option<&NameRecord<'a>> {
        self.monsters
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

/// Builds a battle data record from its fields, for tests: every byte but the named fields is
/// zero.
#[cfg(test)]
pub(crate) fn battle_record(number: u16, hp: u16, attack: u16, defence: u16) -> [u8; 132] {
    let mut r = [0u8; 132];
    r[0..2].copy_from_slice(&(number | 0x8000).to_le_bytes());
    r[0x02] = 1;
    r[0x03] = 7;
    r[0x04..0x06].copy_from_slice(&22000u16.to_le_bytes());
    r[0x08..0x0c].copy_from_slice(&7u32.to_le_bytes());
    r[0x0c..0x0e].copy_from_slice(&3u16.to_le_bytes());
    r[0x10..0x14].copy_from_slice(&(2u32 << 13 | 1 << 16 | 5 << 20).to_le_bytes());
    r[0x18..0x1a].copy_from_slice(&1u16.to_le_bytes());
    r[0x1a..0x1c].copy_from_slice(&225u16.to_le_bytes());
    r[0x27] = 0x10;
    r[0x28] = 2;
    r[0x29] = 4;
    r[0x5c..0x5e].copy_from_slice(&hp.to_le_bytes());
    r[0x60..0x62].copy_from_slice(&attack.to_le_bytes());
    r[0x62..0x64].copy_from_slice(&defence.to_le_bytes());
    r[0x6c] = 50;
    r[0x6c + 7] = 100;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nat(count: u32, record_size: usize, records: &[u8], strings: &[u8]) -> Vec<u8> {
        let mut data = ((strings.len() as u32) << 12 | count)
            .to_le_bytes()
            .to_vec();
        data.extend_from_slice(records);
        data.extend_from_slice(strings);
        assert_eq!(data.len(), 4 + count as usize * record_size + strings.len());
        data
    }

    #[test]
    fn battle_record_decodes() {
        let mut records = Vec::new();
        records.extend_from_slice(&battle_record(300, 93, 30, 22));
        let data = nat(1, BATTLE_RECORD_SIZE, &records, &[]);
        let table = BattleData::parse(&data).unwrap();
        let m = table.find(300).unwrap();
        assert_eq!(m.number, 300);
        assert!(m.number_flag);
        assert_eq!(m.drop_steps, [1, 7]);
        assert_eq!(m.drops, [22000, 0]);
        assert_eq!(m.experience, 7);
        assert_eq!(m.gold, 3);
        assert_eq!(m.ways[0], 1);
        assert_eq!(m.ways[1], 225);
        assert_eq!((m.hp, m.attack, m.defence), (93, 30, 22));
        assert_eq!(m.resistance(1), Some(50));
        assert_eq!(m.resistance(8), Some(100));
        assert_eq!(m.resistance(23), None);
        assert_eq!((m.evasion_grade(), m.block_grade()), (2, 1));
        assert_eq!(m.once_mask(), 5);
        assert!(m.boss());
        assert_eq!(table.monsters.len(), 1);
        assert!(table.find(301).is_none());
    }

    #[test]
    fn truncated_battle_data_is_an_error() {
        let mut data = nat(1, BATTLE_RECORD_SIZE, &battle_record(1, 8, 10, 7), &[]);
        assert!(BattleData::parse(&data).is_ok());
        data.truncate(data.len() - 1);
        assert!(BattleData::parse(&data).is_err());
    }

    #[test]
    fn list_and_names_decode_their_strings() {
        // One list record and one name record, each pointing at the strings "z000a",
        // "slime" and "slimes".
        let strings = b"\0z000a\0slime\0slimes\0";
        let mut list = [0u8; 32];
        list[4..8].copy_from_slice(&1u32.to_le_bytes());
        list[8..12].copy_from_slice(&7u32.to_le_bytes());
        list[0x0c..0x0e].copy_from_slice(&1u16.to_le_bytes());
        list[0x0e..0x10].copy_from_slice(&364u16.to_le_bytes());
        let data = nat(1, LIST_RECORD_SIZE, &list, strings);
        let monsters = List::parse(&data).unwrap();
        let m = &monsters.monsters[0];
        assert_eq!(
            (m.number, m.code, m.name),
            (1, &b"z000a"[..], &b"slime"[..])
        );
        assert_eq!(m.unknown, 364);
        assert_eq!(monsters.find_name(b"SLIME").unwrap().number, 1);
        assert_eq!(monsters.find_code(b"Z000A").unwrap().number, 1);

        let mut names = [0u8; 28];
        names[0..4].copy_from_slice(&7u32.to_le_bytes());
        names[4..8].copy_from_slice(&1u32.to_le_bytes());
        names[8..10].copy_from_slice(&1u16.to_le_bytes());
        // Level 12, family 3, refuses a fall, metal.
        names[0x0a..0x0c].copy_from_slice(&(12u16 | 3 << 7 | 1 << 11 | 1 << 12).to_le_bytes());
        names[0x0c..0x0e].copy_from_slice(&(-3i16).to_le_bytes());
        names[0x0e..0x10].copy_from_slice(&4096i16.to_le_bytes());
        names[0x10..0x12].copy_from_slice(&0x101u16.to_le_bytes());
        names[0x12..0x14].copy_from_slice(&(12779i16).to_le_bytes());
        names[0x14..0x18].copy_from_slice(&13u32.to_le_bytes());
        let data = nat(1, NAME_RECORD_SIZE, &names, strings);
        let names = Names::parse(&data).unwrap();
        let n = names.find(1).unwrap();
        assert_eq!(n.name, b"slime");
        assert_eq!(n.plural, b"slimes");
        assert_eq!(n.code, b"z000a");
        assert_eq!((n.level, n.family), (12, 3));
        assert!(n.refuses_fall);
        assert!(n.metal);
        assert_eq!((n.radius, n.height, n.battle_size), (-3, 4096, 12779));
        assert_eq!(n.kind, 0x101);
        assert_eq!(names.find_name(b"SLIMES"), None);
        assert_eq!(names.find_code(b"z000a").unwrap().number, 1);
    }

    #[test]
    fn bad_string_offset_is_an_error() {
        let mut list = [0u8; 32];
        list[4..8].copy_from_slice(&2u32.to_le_bytes()); // inside "z000a"
        list[8..12].copy_from_slice(&7u32.to_le_bytes());
        let data = nat(1, LIST_RECORD_SIZE, &list, b"\0z000a\0slime\0slimes\0");
        assert!(List::parse(&data).is_err());
    }
}
