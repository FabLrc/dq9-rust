//! The protagonist's body: which parts of `chara_pc.gp2` a character is drawn with
//! (`docs/re/characters.md` § "The hero").
//!
//! A human character is a set of ten worn items (slots h0–h9 of `CharaParts_GetPartNumbers`,
//! USA `func_02072afc`): armour, legwear, face, hair, gloves, footwear, headgear, weapon, shield
//! and accessory. Each item names a model of `data/pack_lv5/chara_pc.gp2` by its id (wiki
//! Character-Parts); faces and hairs are items whose model number comes from the `itemdt_def`
//! record (`func_020de234`). This module resolves a [`HeroLook`] to part names; assembling and
//! animating the parts is `dq9-render`'s job. No I/O.

use crate::save::Character;
use crate::save::EquipSlot;
use nds_rom::{Error, Result};

/// Sex of a character (appearance record `+0x14` bit 0: 0 a man, 1 a woman).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Man,
    Woman,
}

/// What a character wears and looks like, as the save keeps it (`Character` bytes
/// `+352..+373`, `docs/re/save.md`). Items are item ids; `None` is an empty slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroLook {
    pub sex: Sex,
    /// Item 9020..=9033.
    pub face_item: u16,
    /// Item 9000..=9013.
    pub hair_item: u16,
    /// Hair colour 0..=9 (low nibble of `+373`): selects the hair texture file.
    pub hair_color: u8,
    /// Skin tone and eye colour (appearance bits; not applied to palettes yet).
    pub skin_tone: u8,
    pub eye_color: u8,
    /// Width and height factors of the body (`+376`, `+378`, 4096 = 1): the object's scale is
    /// `0x10a × factor / 4096` on x and z, and on y for the height (RAM of the hero, melonDS).
    pub build: (u16, u16),
    pub armour: Option<u16>,
    pub legwear: Option<u16>,
    pub gloves: Option<u16>,
    pub footwear: Option<u16>,
    pub headgear: Option<u16>,
}

fn worn(id: u16, first: u16) -> Option<u16> {
    (id / 1000 == first / 1000).then_some(id)
}

impl HeroLook {
    /// The hero a new game starts with: a man (face 9024, hair 9006: the male presets of
    /// `charapreset.bin`) in the Celestrian outfit of the opening, **[hypothesis]**: celestial
    /// suit, stockings and shoes, and the halo (items 13007, 16215, 17120, 12805).
    pub fn new_game() -> Self {
        Self {
            sex: Sex::Man,
            face_item: 9024,
            hair_item: 9006,
            hair_color: 0,
            skin_tone: 3,
            eye_color: 0,
            build: (0x1000, 0x1000),
            armour: Some(13007),
            legwear: Some(16215),
            gloves: None,
            footwear: Some(17120),
            headgear: Some(12805),
        }
    }

    /// The look of a saved character. Slots hold item ids; anything outside a slot's id band
    /// (`0xffff`, 0) is an empty slot.
    pub fn from_character(c: &Character<'_>) -> Self {
        let slot = |s: EquipSlot, first: u16| worn(c.equipment(s), first);
        let bits = c.bytes()[372];
        Self {
            sex: if bits & 1 == 0 { Sex::Man } else { Sex::Woman },
            face_item: c.face_item(),
            hair_item: c.hair_item(),
            hair_color: c.hair_color().min(9),
            skin_tone: c.skin_color(),
            eye_color: c.eye_color(),
            build: c.body(),
            armour: slot(EquipSlot::Torso, 13000),
            legwear: slot(EquipSlot::Legs, 16000),
            gloves: slot(EquipSlot::Arms, 15000),
            footwear: slot(EquipSlot::Feet, 17000),
            headgear: slot(EquipSlot::Head, 12000),
        }
    }
}

/// Model numbers of faces and hairs: the 32-byte records of `itemdt_def.nat` (member of
/// `data/prm/itemdt_def.gp2`), a 32-byte header then one record per item, the item id at
/// `+4` of the record.
#[derive(Debug, Clone, Copy)]
pub struct ItemModels<'a>(&'a [u8]);

const RECORD: usize = 32;
const HEADER: usize = 0x20;

impl<'a> ItemModels<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < HEADER + RECORD {
            return Err(Error::Malformed("itemdt_def too short"));
        }
        Ok(Self(data))
    }

    /// `(letter, number)` of the model of item `id` for `sex`: the word 8 bytes before the
    /// id, bits 0–9 the man's number, 10–19 the woman's, 999 meaning the other sex's, bits
    /// 20–27 the letter (`func_020de234`). `None` for an unknown item or one without a model.
    pub fn model(&self, id: u16, sex: Sex) -> Option<(char, u16)> {
        let d = self.0;
        let mut at = HEADER;
        while at + 6 <= d.len() {
            if u16::from_le_bytes([d[at + 4], d[at + 5]]) == id {
                let word = u32::from_le_bytes(d[at - 4..at].try_into().ok()?);
                let (man, woman) = (word & 0x3ff, word >> 10 & 0x3ff);
                let n = match sex {
                    Sex::Man if man != 999 => man,
                    Sex::Man => woman,
                    Sex::Woman if woman != 999 => woman,
                    Sex::Woman => man,
                } as u16;
                let letter = char::from(u8::try_from(word >> 20 & 0xff).ok()?);
                return (n != 999 && letter.is_ascii_lowercase()).then_some((letter, n));
            }
            at += RECORD;
        }
        None
    }
}

/// Part model of a worn item by the wiki's naming rule: the thousands choose the letter
/// (12 `m` headgear, 13 `b` armour, 14 `a` arms, 15 `g` gloves, 16 `p` legwear, 17 `r`
/// footwear, 20 `w` weapon, 21 `s` shield), the rest is a three-digit number: `13007` is
/// `p_b007`.
pub fn item_part(id: u16) -> Option<String> {
    let letter = match id / 1000 {
        12 => 'm',
        13 => 'b',
        14 => 'a',
        15 => 'g',
        16 => 'p',
        17 => 'r',
        20 => 'w',
        21 => 's',
        _ => return None,
    };
    Some(format!("p_{letter}{:03}", id % 1000))
}

/// The hair's shape letter, from the headgear (`func_02072e94`): `a` with none, else the
/// headgear's model number ÷ 100 indexes `"bbdcc\0cea\0"`; `None` draws no hair (hundreds 5
/// and 9, and from ten up).
fn hair_shape(headgear_number: Option<u16>) -> Option<char> {
    let Some(n) = headgear_number else {
        return Some('a');
    };
    match n / 100 {
        0 | 1 => Some('b'),
        2 => Some('d'),
        3 | 4 | 6 => Some('c'),
        7 => Some('e'),
        8 => Some('a'),
        _ => None,
    }
}

/// The hair part and its colour texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hair {
    /// Model, `p_h060a` (`.nsbmd` member).
    pub model: String,
    /// Colour texture file, `p_h060a` (`.nsbtx` member; its texture is bound by name).
    pub texture: String,
}

/// Part names of one character, resolved against the parts that exist. Member names carry no
/// extension: `.nsbmd` for models, `.nsbtx` for texture files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outfit {
    /// Body on the 14-bone rig (armour, or the underclothes `p_p090` without armour).
    pub body: String,
    /// Legs on the same rig; `None` when the body already is the underclothes.
    pub legs: Option<String>,
    /// Face, hung on the head bone.
    pub face: Option<String>,
    pub hair: Option<Hair>,
    /// Headgear, hung on the head bone.
    pub headgear: Option<String>,
    /// Arms texture file: the gloves, else the bare arms numbered as the armour.
    pub arms_texture: Option<String>,
    /// Footwear texture file.
    pub feet_texture: Option<String>,
}

/// Underclothes: a body, legs and feet made as a set for no item (wiki Character-Parts).
pub const UNDERCLOTHES: &str = "p_p090";

impl Outfit {
    /// Resolves `look`; `has(member)` tells whether a member (`p_b007.nsbmd`) is in
    /// `chara_pc.gp2`. A worn item without a part (the wiki lists such legwear ids) falls back
    /// to the underclothes or to nothing.
    pub fn resolve(look: &HeroLook, models: &ItemModels<'_>, has: impl Fn(&str) -> bool) -> Self {
        let model = |name: &str| has(&format!("{name}.nsbmd"));
        let texture = |name: &str| has(&format!("{name}.nsbtx"));
        let part = |id: Option<u16>| id.and_then(item_part).filter(|p| model(p));
        let number = |id: u16| id % 1000;
        let armour = part(look.armour);
        let (body, legs) = match armour {
            Some(body) => (
                body,
                Some(part(look.legwear).unwrap_or_else(|| UNDERCLOTHES.to_owned()))
                    .filter(|p| model(p)),
            ),
            None => (UNDERCLOTHES.to_owned(), None),
        };
        let headgear = part(look.headgear);
        let headgear_number = look.headgear.map(number);
        let hair = models
            .model(look.hair_item, look.sex)
            .filter(|&(letter, _)| letter == 'h')
            .and_then(|(_, n)| {
                let mut shape = hair_shape(headgear_number)?;
                if shape == 'c'
                    && look.hair_item == 9001
                    && headgear_number.map(|h| h / 100) == Some(3)
                {
                    shape = 'f';
                }
                let find = |shape: char| {
                    let name = format!("p_h{n:03}{shape}");
                    model(&name).then_some(name)
                };
                let model_name = find(shape).or_else(|| find('a'))?;
                // `p_h<ss><c>a.nsbtx`: style number ÷ 10, colour, `a`.
                let tex = |c: u8| format!("p_h{:02}{c}a", n / 10);
                let texture_name = [look.hair_color, 0]
                    .into_iter()
                    .map(tex)
                    .find(|t| texture(t))?;
                Some(Hair {
                    model: model_name,
                    texture: texture_name,
                })
            });
        let face = models
            .model(look.face_item, look.sex)
            .filter(|&(letter, _)| letter == 'f')
            .map(|(_, n)| format!("p_f{n:03}"))
            .filter(|p| model(p));
        let arms_texture = look
            .gloves
            .and_then(item_part)
            .filter(|p| texture(p))
            .or_else(|| {
                look.armour
                    .map(|a| format!("p_a{:03}", number(a)))
                    .filter(|p| texture(p))
            });
        let feet_texture = look.footwear.and_then(item_part).filter(|p| texture(p));
        Self {
            body,
            legs,
            face,
            hair,
            headgear,
            arms_texture,
            feet_texture,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `itemdt_def` layout: the model word sits 8 bytes before the id, i.e. in the previous
    /// 32-byte slot's last four bytes.
    fn table(entries: &[(u16, u32)]) -> Vec<u8> {
        let mut d = vec![0u8; HEADER];
        for &(id, word) in entries {
            let at = d.len();
            d.extend([0u8; RECORD]);
            d[at + 4..at + 6].copy_from_slice(&id.to_le_bytes());
            d[at - 4..at].copy_from_slice(&word.to_le_bytes());
        }
        d
    }

    fn letter(c: char, man: u32, woman: u32) -> u32 {
        (u32::from(c as u8) << 20) | (woman << 10) | man
    }

    #[test]
    fn model_numbers_follow_sex_and_the_999_marker() {
        let d = table(&[
            (9006, letter('h', 60, 180)),
            (9010, letter('h', 200, 999)),
            (9011, letter('h', 999, 210)),
            (9024, letter('f', 4, 14)),
        ]);
        let m = ItemModels::parse(&d).unwrap();
        assert_eq!(m.model(9006, Sex::Man), Some(('h', 60)));
        assert_eq!(m.model(9006, Sex::Woman), Some(('h', 180)));
        assert_eq!(m.model(9010, Sex::Woman), Some(('h', 200)));
        assert_eq!(m.model(9011, Sex::Man), Some(('h', 210)));
        assert_eq!(m.model(9024, Sex::Woman), Some(('f', 14)));
        assert_eq!(m.model(1, Sex::Man), None);
    }

    #[test]
    fn items_name_parts_by_the_thousands() {
        assert_eq!(item_part(13007).as_deref(), Some("p_b007"));
        assert_eq!(item_part(12805).as_deref(), Some("p_m805"));
        assert_eq!(item_part(17120).as_deref(), Some("p_r120"));
        assert_eq!(item_part(9000), None);
    }

    #[test]
    fn hair_shape_follows_the_headgear() {
        assert_eq!(hair_shape(None), Some('a'));
        assert_eq!(hair_shape(Some(30)), Some('b'));
        assert_eq!(hair_shape(Some(250)), Some('d'));
        assert_eq!(hair_shape(Some(340)), Some('c'));
        assert_eq!(hair_shape(Some(805)), Some('a'));
        assert_eq!(hair_shape(Some(500)), None);
        assert_eq!(hair_shape(Some(900)), None);
        assert_eq!(hair_shape(Some(1200)), None);
    }

    #[test]
    fn missing_parts_fall_back() {
        let d = table(&[(9006, letter('h', 60, 180)), (9024, letter('f', 4, 14))]);
        let models = ItemModels::parse(&d).unwrap();
        let have = [
            "p_b007.nsbmd",
            "p_p090.nsbmd",
            "p_f004.nsbmd",
            "p_h060a.nsbmd",
            "p_h060a.nsbtx",
        ];
        let has = |n: &str| have.contains(&n);
        let mut look = HeroLook::new_game();
        let o = Outfit::resolve(&look, &models, has);
        assert_eq!(o.body, "p_b007");
        // p_p215 does not exist: underclothes legs.
        assert_eq!(o.legs.as_deref(), Some("p_p090"));
        assert_eq!(o.face.as_deref(), Some("p_f004"));
        assert_eq!(o.headgear, None);
        // Hair 060 with the halo (805 → `a`).
        assert_eq!(o.hair.as_ref().map(|h| h.model.as_str()), Some("p_h060a"));
        // Hair colour 5 has no file: colour 0.
        look.hair_color = 5;
        assert_eq!(
            Outfit::resolve(&look, &models, has).hair.map(|h| h.texture),
            Some("p_h060a".to_owned())
        );
        // No armour: the underclothes alone.
        look.armour = None;
        let o = Outfit::resolve(&look, &models, has);
        assert_eq!((o.body.as_str(), o.legs), ("p_p090", None));
    }
}
