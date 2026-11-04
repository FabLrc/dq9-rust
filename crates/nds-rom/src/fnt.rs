//! File Name Table (FNT) and File Allocation Table (FAT), shared by the ROM filesystem and NARC.

use crate::bytes::{range, slice, u8_at, u16_at, u32_at};
use crate::{Error, Result};

const ROOT_DIR: u16 = 0xF000;

/// One named file of a filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// FAT index.
    pub id: u16,
    /// Full path with `/` separators and no leading slash, e.g. `data/map/F01M01.ambl`.
    pub path: String,
}

/// Named files from an FNT, sorted by id.
#[derive(Debug, Clone, Default)]
pub struct FileTable {
    pub files: Vec<FileEntry>,
}

impl FileTable {
    /// Walks the directory tree of an FNT blob.
    pub fn parse(fnt: &[u8]) -> Result<Self> {
        let dir_count = usize::from(u16_at(fnt, 6)?);
        if dir_count == 0 || dir_count > 0x1000 {
            return Err(Error::Malformed("FNT directory count"));
        }
        let mut files = Vec::new();
        let mut visited = vec![false; dir_count];
        // Explicit stack instead of recursion: (directory id, path prefix).
        let mut stack = vec![(ROOT_DIR, String::new())];
        while let Some((dir_id, prefix)) = stack.pop() {
            let index = usize::from(dir_id.wrapping_sub(ROOT_DIR));
            if index >= dir_count || std::mem::replace(&mut visited[index], true) {
                return Err(Error::Malformed("FNT directory id out of range or cyclic"));
            }
            let mut cursor = u32_at(fnt, index * 8)? as usize;
            let mut file_id = u16_at(fnt, index * 8 + 4)?;
            loop {
                let kind = u8_at(fnt, cursor)?;
                cursor += 1;
                if kind == 0 {
                    break;
                }
                let len = usize::from(kind & 0x7F);
                let name = slice(fnt, cursor, len)?;
                cursor += len;
                let name: String = name.iter().map(|&b| char::from(b)).collect();
                let path = format!("{prefix}{name}");
                if kind & 0x80 != 0 {
                    let sub = u16_at(fnt, cursor)?;
                    cursor += 2;
                    stack.push((sub, format!("{path}/")));
                } else {
                    files.push(FileEntry { id: file_id, path });
                    file_id = file_id
                        .checked_add(1)
                        .ok_or(Error::Malformed("FNT file id"))?;
                }
            }
        }
        files.sort_by_key(|f| f.id);
        Ok(Self { files })
    }

    pub fn find(&self, path: &str) -> Option<&FileEntry> {
        let path = path.trim_start_matches('/');
        self.files.iter().find(|f| f.path == path)
    }
}

/// Parsed FAT: `(start, end)` byte offsets per file id, relative to the owning container.
#[derive(Debug, Clone, Default)]
pub struct Fat {
    pub entries: Vec<(u32, u32)>,
}

impl Fat {
    pub fn parse(fat: &[u8]) -> Result<Self> {
        let entries = (0..fat.len() / 8)
            .map(|i| Ok((u32_at(fat, i * 8)?, u32_at(fat, i * 8 + 4)?)))
            .collect::<Result<_>>()?;
        Ok(Self { entries })
    }

    /// Bytes of file `id` inside `container` (offsets are relative to it).
    pub fn file<'a>(&self, container: &'a [u8], id: u16) -> Result<&'a [u8]> {
        let &(start, end) = self
            .entries
            .get(usize::from(id))
            .ok_or_else(|| Error::NotFound(format!("file id {id}")))?;
        range(container, start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// root: "a.bin", dir "sub" -> "b.bin", "c.bin"; first file id 5.
    fn sample_fnt() -> Vec<u8> {
        let mut root = vec![5, b'a', b'.', b'b', b'i', b'n'];
        root.extend([0x83, b's', b'u', b'b', 0x01, 0xF0, 0]);
        let sub = [
            5, b'b', b'.', b'b', b'i', b'n', 5, b'c', b'.', b'b', b'i', b'n', 0,
        ];
        let table_len = 16u32;
        let mut fnt = Vec::new();
        fnt.extend(table_len.to_le_bytes());
        fnt.extend(5u16.to_le_bytes());
        fnt.extend(2u16.to_le_bytes());
        fnt.extend((table_len + root.len() as u32).to_le_bytes());
        fnt.extend(6u16.to_le_bytes());
        fnt.extend(ROOT_DIR.to_le_bytes());
        fnt.extend(root);
        fnt.extend(sub);
        fnt
    }

    #[test]
    fn walks_nested_directories() {
        let table = FileTable::parse(&sample_fnt()).unwrap();
        let got: Vec<_> = table
            .files
            .iter()
            .map(|f| (f.id, f.path.as_str()))
            .collect();
        assert_eq!(got, [(5, "a.bin"), (6, "sub/b.bin"), (7, "sub/c.bin")]);
        assert_eq!(table.find("/sub/c.bin").unwrap().id, 7);
    }

    #[test]
    fn rejects_directory_cycles() {
        let mut fnt = sample_fnt();
        // Make "sub" (dir id 0xF001) point back at the root directory 0xF000.
        let pos = fnt.windows(2).position(|w| w == [0x01, 0xF0]).unwrap();
        fnt[pos] = 0x00;
        assert!(FileTable::parse(&fnt).is_err());
    }

    #[test]
    fn fat_rejects_inverted_range() {
        let mut fat = Vec::new();
        fat.extend(8u32.to_le_bytes());
        fat.extend(4u32.to_le_bytes());
        let fat = Fat::parse(&fat).unwrap();
        assert!(fat.file(&[0; 16], 0).is_err());
        assert!(fat.file(&[0; 16], 1).is_err());
    }
}
