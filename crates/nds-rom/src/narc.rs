//! NARC ("Nitro ARChive") containers.

use crate::bytes::{magic_at, slice, u16_at, u32_at};
use crate::fnt::{Fat, FileTable};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Narc<'a> {
    fat: Fat,
    /// Member names, when the archive has a populated name table.
    pub names: FileTable,
    /// GMIF payload; FAT offsets are relative to it.
    image: &'a [u8],
}

fn section<'a>(data: &'a [u8], offset: usize, magic: &[u8; 4]) -> Result<(&'a [u8], usize)> {
    let found = magic_at(data, offset)?;
    if &found != magic {
        return Err(Error::BadMagic {
            what: "NARC section",
            found,
        });
    }
    let size = u32_at(data, offset + 4)? as usize;
    if size < 8 {
        return Err(Error::Malformed("NARC section size"));
    }
    Ok((slice(data, offset + 8, size - 8)?, offset + size))
}

impl<'a> Narc<'a> {
    pub fn is_narc(data: &[u8]) -> bool {
        data.starts_with(b"NARC")
    }

    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let found = magic_at(data, 0)?;
        if &found != b"NARC" {
            return Err(Error::BadMagic {
                what: "NARC",
                found,
            });
        }
        if u16_at(data, 4)? != 0xFFFE {
            return Err(Error::Malformed("NARC byte order mark"));
        }
        let header_size = usize::from(u16_at(data, 12)?);
        let (btaf, next) = section(data, header_size, b"BTAF")?;
        let (btnf, next) = section(data, next, b"BTNF")?;
        let (image, _) = section(data, next, b"GMIF")?;
        let count = usize::from(u16_at(btaf, 0)?);
        let fat = Fat::parse(slice(btaf, 4, count * 8)?)?;
        Ok(Self {
            fat,
            names: FileTable::parse(btnf)?,
            image,
        })
    }

    pub fn len(&self) -> usize {
        self.fat.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fat.entries.is_empty()
    }

    pub fn file(&self, index: u16) -> Result<&'a [u8]> {
        self.fat.file(self.image, index)
    }

    /// Name of member `index`, if the archive stores names.
    pub fn name(&self, index: u16) -> Option<&str> {
        self.names
            .files
            .iter()
            .find(|f| f.id == index)
            .map(|f| f.path.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(members: &[&[u8]]) -> Vec<u8> {
        let mut btaf = Vec::new();
        btaf.extend((members.len() as u16).to_le_bytes());
        btaf.extend([0, 0]);
        let mut image = Vec::new();
        for m in members {
            btaf.extend((image.len() as u32).to_le_bytes());
            image.extend(*m);
            btaf.extend((image.len() as u32).to_le_bytes());
        }
        // Unnamed name table: root directory only.
        let btnf = [4u32.to_le_bytes(), [0, 0, 1, 0]].concat();
        let mut out = b"NARC".to_vec();
        out.extend(0xFFFEu16.to_le_bytes());
        out.extend(0x0100u16.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(3u16.to_le_bytes());
        for (magic, body) in [(b"BTAF", &btaf), (b"BTNF", &btnf), (b"GMIF", &image)] {
            out.extend(magic);
            out.extend(((body.len() + 8) as u32).to_le_bytes());
            out.extend(body);
        }
        out
    }

    #[test]
    fn reads_unnamed_members() {
        let data = build(&[b"one", b"", b"three"]);
        let narc = Narc::parse(&data).unwrap();
        assert_eq!(narc.len(), 3);
        assert_eq!(narc.file(0).unwrap(), b"one");
        assert_eq!(narc.file(1).unwrap(), b"");
        assert_eq!(narc.file(2).unwrap(), b"three");
        assert!(narc.file(3).is_err());
        assert_eq!(narc.name(0), None);
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut data = build(&[b"x"]);
        data[0] = b'X';
        assert!(matches!(Narc::parse(&data), Err(Error::BadMagic { .. })));
    }
}
