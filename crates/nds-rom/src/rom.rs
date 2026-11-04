//! A whole ROM image: header, filesystem, ARM9 binary and overlays.

use crate::bytes::{slice, u32_at};
use crate::compress::blz;
use crate::fnt::{Fat, FileTable};
use crate::{Error, Header, Result};

/// Magic closing the ARM9 module parameters: `0xDEC00621` then `0x2106C0DE`, little-endian.
const NITROCODE: [u8; 8] = [0x21, 0x06, 0xC0, 0xDE, 0xDE, 0xC0, 0x06, 0x21];
const OVERLAY_ENTRY_SIZE: usize = 32;
pub const BANNER_LANGUAGES: [&str; 6] = [
    "Japanese", "English", "French", "German", "Italian", "Spanish",
];

#[derive(Debug, Clone)]
pub struct Rom<'a> {
    data: &'a [u8],
    pub header: Header,
    fat: Fat,
    pub files: FileTable,
}

impl<'a> Rom<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let header = Header::parse(data)?;
        let fnt = slice(data, header.fnt.offset as usize, header.fnt.size as usize)?;
        let fat = slice(data, header.fat.offset as usize, header.fat.size as usize)?;
        Ok(Self {
            data,
            fat: Fat::parse(fat)?,
            files: FileTable::parse(fnt)?,
            header,
        })
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Number of FAT entries (named files + overlay files).
    pub fn file_count(&self) -> usize {
        self.fat.entries.len()
    }

    pub fn file(&self, id: u16) -> Result<&'a [u8]> {
        self.fat.file(self.data, id)
    }

    pub fn file_by_path(&self, path: &str) -> Result<&'a [u8]> {
        let entry = self
            .files
            .find(path)
            .ok_or_else(|| Error::NotFound(path.to_owned()))?;
        self.file(entry.id)
    }

    /// Raw ARM7 binary (never compressed).
    pub fn arm7(&self) -> Result<&'a [u8]> {
        let a = &self.header.arm7;
        slice(self.data, a.rom_offset as usize, a.size as usize)
    }

    /// ARM9 binary, BLZ-decompressed when the module parameters say it is compressed.
    pub fn arm9(&self) -> Result<Arm9> {
        let info = &self.header.arm9;
        let raw = slice(self.data, info.rom_offset as usize, info.size as usize)?;
        let params_offset = raw
            .windows(NITROCODE.len())
            .position(|w| w == NITROCODE)
            .and_then(|p| p.checked_sub(0x1C))
            .ok_or(Error::Malformed("ARM9 module parameters not found"))?;
        let params = ModuleParams::parse(raw, params_offset)?;
        let (data, compressed) = if params.compressed_static_end == 0 {
            (raw.to_vec(), false)
        } else {
            let end = params
                .compressed_static_end
                .checked_sub(info.load_addr)
                .map(|e| e as usize)
                .filter(|&e| e <= raw.len())
                .ok_or(Error::Malformed("ARM9 compressed end outside binary"))?;
            let mut data = blz(&raw[..end])?;
            data.extend_from_slice(&raw[end..]);
            (data, true)
        };
        Ok(Arm9 {
            load_addr: info.load_addr,
            entry: info.entry,
            params_offset,
            params,
            compressed,
            data,
        })
    }

    pub fn overlays(&self) -> Result<Vec<Overlay>> {
        let t = &self.header.arm9_overlays;
        let table = slice(self.data, t.offset as usize, t.size as usize)?;
        table
            .as_chunks::<OVERLAY_ENTRY_SIZE>()
            .0
            .iter()
            .map(|e| Overlay::parse(e))
            .collect()
    }

    /// Overlay contents, BLZ-decompressed when flagged.
    pub fn overlay_data(&self, overlay: &Overlay) -> Result<Vec<u8>> {
        let file = self.file(overlay.file_id as u16)?;
        if overlay.compressed {
            blz(file)
        } else {
            Ok(file.to_vec())
        }
    }

    /// Game titles from the banner, one per [`BANNER_LANGUAGES`] entry.
    pub fn banner_titles(&self) -> Result<Vec<String>> {
        let base = self.header.banner_offset as usize;
        (0..BANNER_LANGUAGES.len())
            .map(|lang| {
                let raw = slice(self.data, base + 0x240 + lang * 0x100, 0x100)?;
                let units: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&c| u16::from_le_bytes(c))
                    .take_while(|&u| u != 0)
                    .collect();
                Ok(String::from_utf16_lossy(&units))
            })
            .collect()
    }
}

/// SDK module parameters embedded in the ARM9 binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleParams {
    pub autoload_list_start: u32,
    pub autoload_list_end: u32,
    pub autoload_start: u32,
    pub static_bss_start: u32,
    pub static_bss_end: u32,
    /// RAM address where BLZ-compressed data ends; 0 when uncompressed.
    pub compressed_static_end: u32,
    pub sdk_version: u32,
}

impl ModuleParams {
    fn parse(data: &[u8], at: usize) -> Result<Self> {
        Ok(Self {
            autoload_list_start: u32_at(data, at)?,
            autoload_list_end: u32_at(data, at + 4)?,
            autoload_start: u32_at(data, at + 8)?,
            static_bss_start: u32_at(data, at + 12)?,
            static_bss_end: u32_at(data, at + 16)?,
            compressed_static_end: u32_at(data, at + 20)?,
            sdk_version: u32_at(data, at + 24)?,
        })
    }
}

/// Decompressed ARM9 binary as loaded at `load_addr` before autoload copying.
#[derive(Debug, Clone)]
pub struct Arm9 {
    pub load_addr: u32,
    pub entry: u32,
    /// Offset of [`ModuleParams`] inside `data`.
    pub params_offset: usize,
    pub params: ModuleParams,
    /// Whether the ROM copy was BLZ-compressed.
    pub compressed: bool,
    pub data: Vec<u8>,
}

/// One block copied by the ARM9 boot code from the end of the static binary to its final
/// address (ITCM, DTCM or main RAM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Autoload {
    pub ram_addr: u32,
    pub size: u32,
    pub bss_size: u32,
    /// Offset of the block's bytes inside [`Arm9::data`].
    pub data_offset: usize,
}

impl Arm9 {
    fn offset_of(&self, addr: u32) -> Result<usize> {
        addr.checked_sub(self.load_addr)
            .map(|o| o as usize)
            .filter(|&o| o <= self.data.len())
            .ok_or(Error::Malformed("ARM9 address outside binary"))
    }

    /// Bytes that stay at `load_addr` (everything before the autoload blocks).
    pub fn static_module(&self) -> Result<&[u8]> {
        Ok(&self.data[..self.offset_of(self.params.autoload_start)?])
    }

    pub fn autoloads(&self) -> Result<Vec<Autoload>> {
        let list_start = self.offset_of(self.params.autoload_list_start)?;
        let list_end = self.offset_of(self.params.autoload_list_end)?;
        if list_end < list_start {
            return Err(Error::Malformed("ARM9 autoload list"));
        }
        let mut data_offset = self.offset_of(self.params.autoload_start)?;
        let list = &self.data[list_start..list_end];
        list.as_chunks::<12>()
            .0
            .iter()
            .map(|e| {
                let block = Autoload {
                    ram_addr: u32_at(e, 0)?,
                    size: u32_at(e, 4)?,
                    bss_size: u32_at(e, 8)?,
                    data_offset,
                };
                slice(&self.data, data_offset, block.size as usize)?;
                data_offset += block.size as usize;
                Ok(block)
            })
            .collect()
    }
}

/// Entry of the ARM9 overlay table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlay {
    pub id: u32,
    pub ram_addr: u32,
    pub ram_size: u32,
    pub bss_size: u32,
    pub sinit_start: u32,
    pub sinit_end: u32,
    pub file_id: u32,
    pub compressed_size: u32,
    pub compressed: bool,
}

impl Overlay {
    fn parse(e: &[u8]) -> Result<Self> {
        let packed = u32_at(e, 28)?;
        let file_id = u32_at(e, 24)?;
        if file_id > u32::from(u16::MAX) {
            return Err(Error::Malformed("overlay file id"));
        }
        Ok(Self {
            id: u32_at(e, 0)?,
            ram_addr: u32_at(e, 4)?,
            ram_size: u32_at(e, 8)?,
            bss_size: u32_at(e, 12)?,
            sinit_start: u32_at(e, 16)?,
            sinit_end: u32_at(e, 20)?,
            file_id,
            compressed_size: packed & 0x00FF_FFFF,
            compressed: packed >> 24 & 1 != 0,
        })
    }
}
