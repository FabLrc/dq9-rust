//! Equipment battle parameters: `data/prm/itembtlprm.nat`. A `u32` count in its low 20 bits,
//! then one 44-byte record per item, in order of the item's id. Layout and evidence in
//! `docs/re/battle-data.md`; wiki *Equipment Battle Parameters*.
//!
//! The records are sorted by their `s16` id and the game finds one by binary search
//! (`func_0209a004`); overlay 17 copies a worn item's record into the character
//! (`func_ov017_021b3780`) and `func_02083e28` sums the twenty resistance bytes onto a
//! baseline of 100.

use nds_rom::bytes::u32_at;
use nds_rom::{Error, Result};

/// 44-byte records.
const RECORD_SIZE: usize = 44;
/// The count is held in the head word's low 20 bits (`func_0209a088` masks it so).
const COUNT_MASK: u32 = 0x000f_ffff;

/// One equipment battle parameter record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleParam {
    /// `+0x28`: the item's id.
    pub id: i16,
    /// `+0x00`: flags, tested a bit at a time by `func_020852a4` … `func_020855d0`; bit 16
    /// is an experience bonus, bits 23–25 the weapon's element. The rest is not established.
    pub flags: u32,
    /// `+0x08` to `+0x10`: a weapon's killer bonus by monster family 1 to 12, in tenths,
    /// signed — 10 is none (`func_ov024_021fa7ec`).
    pub family_bonus: [i8; 12],
    /// `+0x14`: what it adds to a resistance, one `i8` an element, in the order
    /// `1 2 3 4 5 6 7 9 10 11 12 13 14 15 16 17 18 19 20 21` (8 and 22 absent).
    pub resistances: [i8; 20],
    /// `+0x2A`: 0 on every record.
    pub unknown_2a: u16,
}

impl BattleParam {
    /// The flags' bits 23–25, the weapon's element: 1 to 7 stand for themselves, 0 for 8, the
    /// plain Attack's (INFERRED from its one use in the battle AI).
    pub fn weapon_element(&self) -> u8 {
        let element = (self.flags >> 23 & 7) as u8;
        if element == 0 { 8 } else { element }
    }

    /// The flags' bit 16: an experience bonus.
    pub fn experience_bonus(&self) -> bool {
        self.flags >> 16 & 1 != 0
    }
}

/// `itembtlprm.nat`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleParams {
    pub items: Vec<BattleParam>,
}

impl BattleParams {
    /// # Errors
    /// [`nds_rom::Error`] when the records do not fill the file exactly.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let count = (u32_at(data, 0)? & COUNT_MASK) as usize;
        let len = count
            .checked_mul(RECORD_SIZE)
            .ok_or(Error::Malformed("equipment table too large"))?;
        if 4 + len != data.len() {
            return Err(Error::Malformed("equipment table does not end the file"));
        }
        let mut items = Vec::with_capacity(count);
        for i in 0..count {
            let start = 4 + i * RECORD_SIZE;
            let r = data
                .get(start..start + RECORD_SIZE)
                .ok_or(Error::Malformed("equipment record past end"))?;
            let word = |offset: usize| {
                u32::from_le_bytes([r[offset], r[offset + 1], r[offset + 2], r[offset + 3]])
            };
            let signed = |offset: usize| r[offset] as i8;
            let (w08, w0c, w10) = (word(0x08), word(0x0c), word(0x10));
            // The twelve family accessors' word and bit range (wiki, table of accessors).
            let field = |word: u32, shift: u32| -> i8 {
                // Six signed bits, sign-extended in 32 bits then narrowed.
                ((((word >> shift) & 0x3f) as i32) << 26 >> 26) as i8
            };
            let mut family_bonus = [0; 12];
            family_bonus[0] = field(w0c, 6);
            family_bonus[1] = field(w08, 0);
            family_bonus[2] = field(w0c, 24);
            family_bonus[3] = field(w10, 0);
            family_bonus[4] = field(w0c, 18);
            family_bonus[5] = field(w0c, 12);
            family_bonus[6] = field(w0c, 0);
            family_bonus[7] = field(w08, 6);
            family_bonus[8] = field(w08, 24);
            family_bonus[9] = field(w10, 6);
            family_bonus[10] = field(w08, 18);
            family_bonus[11] = field(w08, 12);
            let mut resistances = [0; 20];
            for (slot, value) in resistances.iter_mut().enumerate() {
                *value = signed(0x14 + slot);
            }
            items.push(BattleParam {
                id: i16::from_le_bytes([r[0x28], r[0x29]]),
                flags: word(0),
                family_bonus,
                resistances,
                unknown_2a: u16::from_le_bytes([r[0x2a], r[0x2b]]),
            });
        }
        Ok(Self { items })
    }

    /// The record of item `id`.
    pub fn find(&self, id: i16) -> Option<&BattleParam> {
        self.items.iter().find(|i| i.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: i16) -> [u8; 44] {
        let mut r = [0u8; 44];
        // Experience bonus and element 3 in the flags.
        r[0..4].copy_from_slice(&(1u32 << 16 | 3 << 23).to_le_bytes());
        // Family 1 at +0x0C bits 6–11: 12 (one and two tenths).
        r[0x0c..0x10].copy_from_slice(&(12u32 << 6).to_le_bytes());
        // Family 2 at +0x08 bits 0–5: −5 (sign-extended).
        r[0x08..0x0c].copy_from_slice(&((-5i32 as u32) & 0x3f).to_le_bytes());
        r[0x14] = 250; // −6 on element 1, one of the whole multiples of five.
        r[0x14 + 8] = 245; // −11 on element 10.
        r[0x28..0x2a].copy_from_slice(&id.to_le_bytes());
        r
    }

    #[test]
    fn record_decodes() {
        let mut data = 1u32.to_le_bytes().to_vec();
        data.extend_from_slice(&record(20918));
        let table = BattleParams::parse(&data).unwrap();
        let item = table.find(20918).unwrap();
        assert_eq!(item.id, 20918);
        assert!(item.experience_bonus());
        assert_eq!(item.weapon_element(), 3);
        assert_eq!(item.family_bonus[0], 12);
        assert_eq!(item.family_bonus[1], -5);
        assert_eq!(item.family_bonus[2], 0);
        assert_eq!(item.resistances[0], -6);
        assert_eq!(item.resistances[8], -11);
    }

    #[test]
    fn element_zero_is_the_plain_attack() {
        let mut data = 1u32.to_le_bytes().to_vec();
        data.extend_from_slice(&[0; RECORD_SIZE]);
        let table = BattleParams::parse(&data).unwrap();
        assert_eq!(table.items[0].weapon_element(), 8);
    }

    #[test]
    fn wrong_size_is_an_error() {
        let mut data = 1u32.to_le_bytes().to_vec();
        data.extend_from_slice(&[0; RECORD_SIZE - 1]);
        assert!(BattleParams::parse(&data).is_err());
        let data = 0u32.to_le_bytes().to_vec();
        assert!(BattleParams::parse(&data).is_ok(), "empty table");
    }
}
