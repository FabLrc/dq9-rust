//! The protagonist's body: which parts of `chara_pc.gp2` a character is drawn with
//! (`docs/re/characters.md` § "The hero").
//!
//! A human character is a set of ten worn items (slots h0–h9 of `CharaParts_GetPartNumbers`,
//! USA `func_02072afc`): armour, legwear, face, hair, gloves, footwear, headgear, weapon, shield
//! and accessory. Each item names a model of `data/pack_lv5/chara_pc.gp2` by its id (wiki
//! Character-Parts); faces and hairs are items whose model number comes from the `itemdt_def`
//! record (`func_020de234`). This module resolves a [`HeroLook`] to part names; assembling and
//! animating the parts is `dq9-render`'s job. No I/O.

use crate::Table;
use crate::save::Character;
use crate::save::EquipSlot;
use nds_rom::g3d::colour_rgba;
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
    /// suit, stockings and shoes, and the halo (items 13007, 16215, 17120, 12805). The
    /// creation screen's own male default is hair 9000 (`HeroLook::for_sex`): the preset file's
    /// first male entry and overlay 9's default are not the same value.
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

    /// The appearance the creation screen starts from for `sex`: the per-sex defaults of
    /// `ov_009::02185634` (`+0xda4`–`+0xdaf`, one value per sex at `+0xda3`) resolved through
    /// `ov_009::02188b14`'s preset tables (`docs/re/characters.md` § "Character creation").
    /// Both sexes share skin tone 3 and the opening outfit. A man takes face 9024, hair 9000
    /// (the shared table `0…9` at `0x0218a9bc`, preset 0), eye colour 0 and hair colour 0; a
    /// woman takes face 9021, hair 9006 (her override `[6, 0, 1, 2, 3, 4, 5]`), eye colour 5
    /// and hair colour 3.
    pub fn for_sex(sex: Sex) -> Self {
        let (face_item, hair_item, eye_color, hair_color) = match sex {
            Sex::Man => (9024, 9000, 0, 0),
            Sex::Woman => (9021, 9006, 5, 3),
        };
        Self {
            sex,
            face_item,
            hair_item,
            hair_color,
            skin_tone: 3,
            eye_color,
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

/// The colour rows of `data/chara/palette.bin`: the tagged table `func_02099cb8` loads once
/// at startup (wiki Character-Colours; EU addresses, the file is byte-identical on both
/// releases). Its six handlers each read their record's values and store them as halfwords,
/// row after row, into a table of their own (`0x02109928`..`0x02109a54`). Rows are BGR555
/// colours, one ramp per skin tone (pale to dark), one pair per hair or eye colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterColours {
    /// Tag `0x64` (`func_02099ac4`): the brow colour pair of each hair colour, 10 rows.
    pub brows: Vec<[u16; 2]>,
    /// Tag `0x65` (`func_02099b20`): the two-shade skin ramp of each tone, 8 rows.
    pub skin_two: Vec<[u16; 2]>,
    /// Tag `0x66` (`func_02099b7c`): the four-shade ramp of each tone, 8 rows.
    pub skin_four: Vec<[u16; 4]>,
    /// Tag `0x67` (`func_02099bd8`): the eight-shade ramp of each tone, 8 rows. The face
    /// takes this one, the body parts one of the three by its record's `kind`.
    pub skin_eight: Vec<[u16; 8]>,
    /// Tag `0x68` (`func_02099c34`): the eye colour pair of each eye colour, 8 rows.
    pub eyes: Vec<[u16; 2]>,
    /// Tag `0x69` (`func_02099c90`): one value; what reads it is **[unknown]**.
    pub single: u16,
}

/// Rows per table (`func_02099cb8` clears `0x12c` bytes, which the six tables fill exactly).
const BROWS: usize = 10;
const TONES: usize = 8;
const EYES: usize = 8;

/// Tag of each record and the table its handler fills.
const TAG_BROWS: u16 = 0x64;
const TAG_SKIN_TWO: u16 = 0x65;
const TAG_SKIN_FOUR: u16 = 0x66;
const TAG_SKIN_EIGHT: u16 = 0x67;
const TAG_EYES: u16 = 0x68;
const TAG_SINGLE: u16 = 0x69;

/// The shade count a body part's record selects (`func_020de2a4` returns 1, 2 or 4; 0 is
/// "no ramp"), which picks [`CharacterColours`]' two-, four- or eight-shade ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skin {
    Two,
    Four,
    Eight,
}

/// The rule `func_020730e0` applies to a part index (`CharaParts_GetPartNumbers`, USA
/// `func_02072afc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourRule {
    /// Part 2: `func_02099e18` writes the brow pair at slots 2–3, the eye pair at 4–5 and the
    /// tone's eight shades at 8–15 of the face palette.
    Face,
    /// Parts 0, 1, 5 and 6: `func_02099d34` writes the tone's ramp at slot 2 of every
    /// 16-colour palette. Which shade count a worn item gives is **[unknown]** (the eleven
    /// part records `func_020de2a4` reads are built at runtime), hence [`Skin`].
    Skin(Skin),
}

impl Skin {
    /// The number of halfwords the ramp writes.
    pub fn shades(self) -> usize {
        match self {
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    fn row(self, colours: &CharacterColours, tone: u8) -> Option<&[u16]> {
        let i = usize::from(tone);
        match self {
            Self::Two => colours.skin_two.get(i).map(|r| &r[..]),
            Self::Four => colours.skin_four.get(i).map(|r| &r[..]),
            Self::Eight => colours.skin_eight.get(i).map(|r| &r[..]),
        }
    }
}

/// `values` as `count` rows of `N` halfwords (`Script::Parameter::ToInt` stored as a
/// halfword per value).
fn rows<const N: usize>(values: &[u16], count: usize) -> Result<Vec<[u16; N]>> {
    if values.len() != count * N {
        return Err(Error::Malformed("palette.bin row count"));
    }
    Ok(values
        .as_chunks::<N>()
        .0
        .iter()
        .map(|row| core::array::from_fn(|i| row[i]))
        .collect())
}

/// Writes `row`'s colours at `slot` of `entries`; `false` when it does not fit.
fn write_entries(entries: &mut [u16], slot: usize, row: &[u16]) -> bool {
    let Some(target) = entries.get_mut(slot..slot + row.len()) else {
        return false;
    };
    target.copy_from_slice(row);
    true
}

/// [`write_entries`] on the RGBA entries the pipeline's textures are decoded from.
fn write_rgba(entries: &mut [[u8; 4]], slot: usize, row: &[u16]) -> bool {
    let Some(target) = entries.get_mut(slot..slot + row.len()) else {
        return false;
    };
    for (entry, &c) in target.iter_mut().zip(row) {
        *entry = colour_rgba(c);
    }
    true
}

/// [`write_entries`] on the cartridge's palette bytes, starting at a palette's first entry.
fn write_bytes(data: &mut [u8], slot: usize, row: &[u16]) -> bool {
    let Some(target) = data.get_mut(slot * 2..(slot + row.len()) * 2) else {
        return false;
    };
    for (out, &c) in target.as_chunks_mut::<2>().0.iter_mut().zip(row) {
        out.copy_from_slice(&c.to_le_bytes());
    }
    true
}

/// A recolour row over RGBA palette entries: the pure write of `func_020730e0`/`func_02099e18`
/// — the texels stay, the palette entries at `slot` change. Returns whether the row fit.
pub fn recolour_rgba(entries: &mut [[u8; 4]], slot: usize, row: &[u16]) -> bool {
    write_rgba(entries, slot, row)
}

/// [`recolour_rgba`] on BGR555 palette entries (the form the game's handlers write).
pub fn recolour_entries(entries: &mut [u16], slot: usize, row: &[u16]) -> bool {
    write_entries(entries, slot, row)
}

impl CharacterColours {
    /// Parses `data/chara/palette.bin` (`Table`: a count, a string pool at the end, one
    /// record per tag). Every table must be present with its exact row count; a malformed row
    /// is an error, never a panic.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let table = Table::parse(data)?;
        let mut colours = Self {
            brows: Vec::new(),
            skin_two: Vec::new(),
            skin_four: Vec::new(),
            skin_eight: Vec::new(),
            eyes: Vec::new(),
            single: 0,
        };
        let mut seen = [false; 6];
        for record in &table.records {
            let values: Vec<u16> = record.values.iter().map(|v| v.int() as u16).collect();
            let slot = match record.kind {
                TAG_BROWS => {
                    colours.brows = rows(&values, BROWS)?;
                    0
                }
                TAG_SKIN_TWO => {
                    colours.skin_two = rows(&values, TONES)?;
                    1
                }
                TAG_SKIN_FOUR => {
                    colours.skin_four = rows(&values, TONES)?;
                    2
                }
                TAG_SKIN_EIGHT => {
                    colours.skin_eight = rows(&values, TONES)?;
                    3
                }
                TAG_EYES => {
                    colours.eyes = rows(&values, EYES)?;
                    4
                }
                TAG_SINGLE if values.len() == 1 => {
                    colours.single = values[0];
                    5
                }
                _ => return Err(Error::Malformed("palette.bin record")),
            };
            if core::mem::replace(&mut seen[slot], true) {
                return Err(Error::Malformed("palette.bin table twice"));
            }
        }
        if seen.iter().any(|seen| !seen) {
            return Err(Error::Malformed("palette.bin table missing"));
        }
        Ok(colours)
    }

    /// The face's three writes as `(slot, row)`: brow pair (hair colour), eye pair (eye
    /// colour) and the tone's eight shades. A colour past its table is left out (the game
    /// would read the next table or stale memory; presets stay in range).
    fn face_rows(&self, look: &HeroLook) -> [Option<(usize, &[u16])>; 3] {
        [
            self.brows
                .get(usize::from(look.hair_color))
                .map(|row| (2, &row[..])),
            self.eyes
                .get(usize::from(look.eye_color))
                .map(|row| (4, &row[..])),
            self.skin_eight
                .get(usize::from(look.skin_tone))
                .map(|row| (8, &row[..])),
        ]
    }

    /// Applies `rule` to one 16-colour palette of RGBA entries, as `func_020730e0` does to a
    /// part. Returns whether any row was written.
    pub fn recolour(&self, rule: ColourRule, look: &HeroLook, entries: &mut [[u8; 4]]) -> bool {
        match rule {
            ColourRule::Face => {
                let mut written = false;
                for (slot, row) in self.face_rows(look).into_iter().flatten() {
                    written |= write_rgba(entries, slot, row);
                }
                written
            }
            ColourRule::Skin(skin) => skin
                .row(self, look.skin_tone)
                .is_some_and(|row| write_rgba(entries, 2, row)),
        }
    }

    /// [`recolour`](Self::recolour) on the cartridge's palette bytes, starting at a palette's
    /// first entry. The face palette alone is written for a face; for a body part every
    /// 32-byte (16-colour) palette of the block takes the tone's ramp at slot 2, as
    /// `func_02099d34` does over the model's TEX0. Returns whether any row was written.
    pub fn recolour_block(&self, rule: ColourRule, look: &HeroLook, data: &mut [u8]) -> bool {
        match rule {
            ColourRule::Face => {
                let Some(face) = data.get_mut(..32) else {
                    return false;
                };
                let mut written = false;
                for (slot, row) in self.face_rows(look).into_iter().flatten() {
                    written |= write_bytes(face, slot, row);
                }
                written
            }
            ColourRule::Skin(skin) => {
                let Some(row) = skin.row(self, look.skin_tone) else {
                    return false;
                };
                let mut written = false;
                for palette in data.as_chunks_mut::<32>().0 {
                    written |= write_bytes(palette, 2, row);
                }
                written
            }
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

    /// The six tables with one distinguishable value each.
    fn colours() -> CharacterColours {
        CharacterColours {
            brows: (0..10).map(|i| [0x100 + i, 0x200 + i]).collect(),
            skin_two: (0..8).map(|i| [0x300 + i, 0x400 + i]).collect(),
            skin_four: (0..8)
                .map(|i| [0x500 + i, 0x600 + i, 0x700 + i, 0x800 + i])
                .collect(),
            skin_eight: (0..8)
                .map(|i| core::array::from_fn(|k| 0x900 + i as u16 * 8 + k as u16))
                .collect(),
            eyes: (0..8).map(|i| [0xa00 + i, 0xb00 + i]).collect(),
            single: 0x1234,
        }
    }

    fn look(hair_color: u8, eye_color: u8, skin_tone: u8) -> HeroLook {
        HeroLook {
            hair_color,
            eye_color,
            skin_tone,
            ..HeroLook::new_game()
        }
    }

    #[test]
    fn the_face_rule_writes_brows_eyes_and_eight_skin_shades() {
        let c = colours();
        let look = look(2, 3, 5);
        let mut entries = [[0u8; 4]; 16];
        assert!(c.recolour(ColourRule::Face, &look, &mut entries));
        // Brow pair at 2–3, eye pair at 4–5, the tone's eight shades at 8–15.
        assert_eq!(entries[0], [0; 4]);
        assert_eq!(entries[2], colour_rgba(0x102));
        assert_eq!(entries[3], colour_rgba(0x202));
        assert_eq!(entries[4], colour_rgba(0xa03));
        assert_eq!(entries[5], colour_rgba(0xb03));
        for (k, &word) in c.skin_eight[5].iter().enumerate() {
            assert_eq!(entries[8 + k], colour_rgba(word), "shade {k}");
        }
    }

    #[test]
    fn a_body_part_takes_the_tone_ramp_at_slot_two_of_every_palette() {
        let c = colours();
        let look = look(0, 0, 2);
        let mut block = vec![0u8; 64];
        assert!(c.recolour_block(ColourRule::Skin(Skin::Four), &look, &mut block));
        for palette in block.as_chunks::<32>().0 {
            for (k, &word) in c.skin_four[2].iter().enumerate() {
                let at = (2 + k) * 2;
                assert_eq!(u16::from_le_bytes([palette[at], palette[at + 1]]), word);
            }
            // Slots 0–1 and 6–15 stay as they were.
            assert_eq!(&palette[..4], &[0, 0, 0, 0]);
            assert_eq!(&palette[12..], &[0; 20]);
        }
    }

    #[test]
    fn a_colour_past_its_table_leaves_the_palette_alone() {
        let c = colours();
        let mut entries = [[0u8; 4]; 16];
        // Eye colour 15 (the save keeps four bits) has no row: only the brow and tone are
        // written.
        assert!(c.recolour(ColourRule::Face, &look(0, 15, 0), &mut entries));
        assert_eq!(entries[4], [0; 4]);
        assert_eq!(entries[2], colour_rgba(0x100));
    }

    /// `data/chara/palette.bin` on the cartridge: the six tables, their row counts, and the
    /// values the wiki quotes (brows 0, eyes 0, tone 3's lightest shade, the single word).
    #[test]
    fn palette_bin_holds_the_six_colour_tables() {
        let Some(bytes) = testrom::load() else { return };
        let rom = nds_rom::Rom::parse(bytes).unwrap();
        let data = crate::map::rom_file(&rom, "data/chara/palette.bin").expect("palette.bin");
        let c = CharacterColours::parse(data).unwrap();
        assert_eq!(c.brows.len(), BROWS);
        assert_eq!(c.skin_two.len(), TONES);
        assert_eq!(c.skin_four.len(), TONES);
        assert_eq!(c.skin_eight.len(), TONES);
        assert_eq!(c.eyes.len(), EYES);
        assert_eq!(c.brows[0], [0x14e9, 0x359d]);
        assert_eq!(c.eyes[0], [0x2108, 0x1084]);
        assert_eq!(c.skin_eight[3][0], 0x46fe);
        assert_eq!(c.single, 0x1086);
    }

    /// The face palette of the new game's face (9024 is `p_f004`) is painted with brow pair
    /// 0, eye pair 0 and tone 3 — 7 of the 8 shades to the bit, the wiki's claim.
    #[test]
    fn the_cartridge_face_palette_matches_tone_three() {
        let Some(bytes) = testrom::load() else { return };
        let rom = nds_rom::Rom::parse(bytes).unwrap();
        let data = crate::map::rom_file(&rom, "data/chara/palette.bin").expect("palette.bin");
        let colours = CharacterColours::parse(data).unwrap();
        let pc = crate::map::rom_file(&rom, "data/pack_lv5/chara_pc.gp2").expect("chara_pc");
        let pc = crate::Gpc2::parse(pc).unwrap();
        let member = pc.find("p_f004.nsbmd").expect("p_f004");
        let part = pc.member(member).unwrap();
        let file = nds_rom::g3d::G3dFile::parse(&part).unwrap();
        let palette = &file.tex.as_ref().unwrap().palettes[0];
        assert_eq!(palette.color(2), colours.brows[0][0], "brow pair 0");
        assert_eq!(palette.color(4), colours.eyes[0][0], "eye pair 0");
        let same = (0..8)
            .filter(|&k| palette.color(8 + k) == colours.skin_eight[3][k])
            .count();
        assert_eq!(same, 7, "tone 3's eight shades on the cartridge");
    }
}
