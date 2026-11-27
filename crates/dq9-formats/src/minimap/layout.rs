//! `.bmmp` layouts: which `.obg` picture an area draws, at what scale and corner, and the
//! marks placed on it. Tags measured on the cartridge in `docs/re/minimap.md` (source: the
//! DQIX formats wiki, `Mini-Map`); the records come from the tagged table reader
//! ([`Table`]), so string and float values keep their types.
//!
//! The layout is a plain record table, not a script run by the game.

use crate::table::{Record, Table, Value};

use crate::Result;

/// Tag of the picture: an unestablished integer, then its `.obg` name.
pub const TAG_PICTURE: u16 = 0x66;
/// Tag of the backdrop's picture name (`minimapbg2` to `4`).
pub const TAG_BACKDROP: u16 = 0x6a;
/// Tag of the scale: pixels per map unit (float on 267 files, an integer on 16).
pub const TAG_SCALE: u16 = 0x69;
/// Tag of the corner: the picture's origin in tiles, two values.
pub const TAG_CORNER: u16 = 0x64;
/// Tag of the maps the picture is drawn for: map ids, one or more records.
pub const TAG_MAPS: u16 = 0x6b;
/// Tag of a mark: a position, then the map ids it stands for.
pub const TAG_MARK: u16 = 0x6c;
/// Tag of the map codes by id: `(id, code)` pairs.
pub const TAG_CODES: u16 = 0x70;

/// A mark on the picture: a position in map units, and the maps it stands for.
///
/// The record holds no marker index: every integer is a map id (`data/map/maplist9.bin`), and
/// the field's map screen draws no mark — it uses the list to attach the party's objects, the
/// `shipMPos.bin` ship/harbour entry and the room's containers to the picture
/// (`docs/re/minimap.md` § 3.1). The archive's `marker0`–`marker4` pictures are named by no
/// layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// Across.
    pub x: f32,
    /// Down (the maps' z).
    pub z: f32,
    pub maps: Vec<u32>,
}

/// One `.bmmp` layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout<'a> {
    pub table: Table<'a>,
}

fn text<'a>(value: &Value<'a>) -> Option<&'a str> {
    match value {
        Value::Str(Some(bytes)) => core::str::from_utf8(bytes).ok(),
        _ => None,
    }
}

impl<'a> Layout<'a> {
    /// Parses the table; four files (`F07`, `H07`, `M05`, `M12`) begin with the word 16,
    /// which is their record count, not an empty compressed stream.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        Ok(Self {
            table: Table::parse(data)?,
        })
    }

    /// Records carrying `tag`, in file order.
    pub fn records(&self, tag: u16) -> impl Iterator<Item = &Record<'a>> + '_ {
        self.table.records.iter().filter(move |r| r.kind == tag)
    }

    fn first(&self, tag: u16) -> Option<&Record<'a>> {
        self.records(tag).next()
    }

    /// The `.obg` name of the picture.
    pub fn picture(&self) -> Option<&'a str> {
        self.first(TAG_PICTURE)?.values.get(1).and_then(text)
    }

    /// The backdrop's picture name, if the layout names one.
    pub fn backdrop(&self) -> Option<&'a str> {
        self.first(TAG_BACKDROP)?.values.first().and_then(text)
    }

    /// Pixels per map unit.
    pub fn scale(&self) -> Option<f32> {
        Some(self.first(TAG_SCALE)?.values.first()?.float())
    }

    /// The picture's corner, in tiles.
    pub fn corner(&self) -> Option<(f32, f32)> {
        let values = &self.first(TAG_CORNER)?.values;
        Some((values.first()?.float(), values.get(1)?.float()))
    }

    /// Every map id the picture is drawn for.
    pub fn maps(&self) -> Vec<u32> {
        self.records(TAG_MAPS)
            .flat_map(|r| r.values.iter().map(Value::int))
            .collect()
    }

    /// The marks on the picture, in file order.
    pub fn marks(&self) -> Vec<Mark> {
        self.records(TAG_MARK)
            .filter_map(|r| {
                let (x, z) = (r.values.first()?, r.values.get(1)?);
                Some(Mark {
                    x: x.float(),
                    z: z.float(),
                    maps: r.values[2..].iter().map(Value::int).collect(),
                })
            })
            .collect()
    }

    /// The map code of each id the layout lists.
    pub fn codes(&self) -> Vec<(u32, &'a str)> {
        self.records(TAG_CODES)
            .flat_map(|r| r.values.as_chunks::<2>().0.iter())
            .filter_map(|pair| Some((pair[0].int(), text(&pair[1])?)))
            .collect()
    }

    /// Where a map position falls on the picture:
    /// `pixel = position × scale − corner × 8`, x across and z down.
    pub fn pixel(&self, x: f32, z: f32) -> Option<(f32, f32)> {
        let scale = self.scale()?;
        let (corner_x, corner_z) = self.corner()?;
        Some((
            super::axis(x, scale, corner_x),
            super::axis(z, scale, corner_z),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One record: tag, 2-bit types, values.
    fn record(tag: u16, types: &[u8], values: &[u32]) -> Vec<u8> {
        let mut out = tag.to_le_bytes().to_vec();
        out.push(types.len() as u8);
        for chunk in types.chunks(4) {
            out.push(
                chunk
                    .iter()
                    .enumerate()
                    .fold(0, |acc, (i, t)| acc | t << (2 * i)),
            );
        }
        out.resize(out.len().next_multiple_of(4), 0xFF);
        values.iter().for_each(|v| out.extend(v.to_le_bytes()));
        out
    }

    fn table(records: &[Vec<u8>], pool: &[u8]) -> Vec<u8> {
        let body: Vec<u8> = records.concat();
        let pool_offset = (0x10 + body.len()).next_multiple_of(16);
        let mut out = Vec::new();
        for word in [
            records.len() as u32,
            pool_offset as u32,
            pool.len() as u32,
            2,
        ] {
            out.extend(word.to_le_bytes());
        }
        out.extend(body);
        out.resize(pool_offset, 0xFF);
        out.extend(pool);
        out.resize(out.len().next_multiple_of(16), 0);
        out
    }

    #[test]
    fn reads_the_village_layout() {
        let pool = b"M01M0001\0minimapbg2\0M01\0M01M01\0";
        let data = table(
            &[
                record(TAG_SCALE, &[2], &[3.2f32.to_bits()]),
                record(TAG_CORNER, &[1, 1], &[(-18i32) as u32, (-12i32) as u32]),
                record(TAG_BACKDROP, &[0], &[9]),
                record(TAG_PICTURE, &[1, 0], &[0, 0]),
                record(TAG_MAPS, &[1], &[1100]),
                record(
                    TAG_MARK,
                    &[2, 2, 1, 1],
                    &[(-3.29f32).to_bits(), 3.60f32.to_bits(), 1101, 1109],
                ),
                record(TAG_CODES, &[1, 0, 1, 0], &[1101, 24, 1100, 20]),
            ],
            pool,
        );
        let layout = Layout::parse(&data).unwrap();
        assert_eq!(layout.picture(), Some("M01M0001"));
        assert_eq!(layout.backdrop(), Some("minimapbg2"));
        assert_eq!(layout.scale(), Some(3.2));
        assert_eq!(layout.corner(), Some((-18.0, -12.0)));
        assert_eq!(layout.maps(), [1100]);
        let mark = layout.marks().pop().unwrap();
        assert_eq!(mark.maps, [1101, 1109]);
        assert!((mark.x - -3.29).abs() < 1e-6);
        assert_eq!(layout.codes(), [(1101, "M01M01"), (1100, "M01")]);
        // pixel = position × scale − corner × 8, x across and z down.
        let (px, pz) = layout.pixel(0.0, 16.31).unwrap();
        assert!((px - 144.0).abs() < 1e-3);
        assert!((pz - 148.192).abs() < 1e-2);
    }

    #[test]
    fn integer_scale_and_corner_read_as_floats() {
        let data = table(
            &[
                record(TAG_SCALE, &[1], &[4]),
                record(TAG_CORNER, &[1, 1], &[0, 0]),
            ],
            b"",
        );
        let layout = Layout::parse(&data).unwrap();
        assert_eq!(layout.scale(), Some(4.0));
        assert_eq!(layout.pixel(8.0, 4.0), Some((32.0, 16.0)));
    }

    #[test]
    fn rejects_a_broken_table() {
        assert!(Layout::parse(b"not a table").is_err());
    }

    /// The backdrop names and the marks of the ROM's 283 layouts (skipped without the ROM).
    #[test]
    fn the_rom_names_the_backdrop_and_the_marks() {
        let Some(bytes) = testrom::load() else { return };
        let rom = nds_rom::Rom::parse(bytes).unwrap();
        let archive =
            crate::Gpc2::parse(rom.file_by_path("data/pack_lv5/minimap.gp2").unwrap()).unwrap();
        let mut layouts = 0;
        let mut backdrops = std::collections::BTreeMap::new();
        let mut marks = 0;
        for entry in archive.entries.iter().filter(|e| e.name.ends_with(".bmmp")) {
            let data = archive.member(entry).unwrap();
            let layout = Layout::parse(&data).unwrap();
            layouts += 1;
            if let Some(name) = layout.backdrop() {
                *backdrops.entry(name.to_owned()).or_insert(0) += 1;
            }
            marks += layout.marks().len();
        }
        assert_eq!(layouts, 283);
        assert_eq!(marks, 273);
        assert_eq!(
            backdrops,
            [
                ("minimapbg2".to_owned(), 207),
                ("minimapbg3".to_owned(), 13),
                ("minimapbg4".to_owned(), 1)
            ]
            .into_iter()
            .collect()
        );
        // The pass's first mark stands for map 20001, its second for 20002 (§ 4).
        let data = archive
            .member(archive.find("S01M01.bmmp").unwrap())
            .unwrap();
        let pass = Layout::parse(&data).unwrap();
        let marks = pass.marks();
        assert_eq!((marks[0].x, marks[0].z), (-18.52, 41.69));
        assert_eq!(marks[0].maps, [20001]);
        assert_eq!(marks[1].maps, [20002]);
        // The fields have no backdrop.
        let data = archive.member(archive.find("F01.bmmp").unwrap()).unwrap();
        let field = Layout::parse(&data).unwrap();
        assert_eq!(field.backdrop(), None);
    }
}
