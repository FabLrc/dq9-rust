//! Cartridge header (first 0x200 bytes of the ROM).

use crate::Result;
use crate::bytes::{crc16, slice, u8_at, u16_at, u32_at};

pub const HEADER_SIZE: usize = 0x200;
/// The header CRC covers bytes `0..0x15E`.
const CRC_COVERED: usize = 0x15E;

/// Location and load parameters of one CPU's static binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryInfo {
    pub rom_offset: u32,
    pub entry: u32,
    pub load_addr: u32,
    pub size: u32,
}

/// A `(offset, size)` table location inside the ROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Table {
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub game_code: String,
    pub maker_code: String,
    pub unit_code: u8,
    pub version: u8,
    pub arm9: BinaryInfo,
    pub arm7: BinaryInfo,
    pub fnt: Table,
    pub fat: Table,
    pub arm9_overlays: Table,
    pub arm7_overlays: Table,
    pub banner_offset: u32,
    /// RAM address the ARM9 autoload "done" hook lives at.
    pub arm9_autoload_hook: u32,
    pub used_rom_size: u32,
    pub header_crc: u16,
    /// CRC recomputed from the header bytes.
    pub computed_crc: u16,
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect()
}

impl Header {
    pub fn parse(rom: &[u8]) -> Result<Self> {
        let h = slice(rom, 0, HEADER_SIZE)?;
        let binary = |base: usize| -> Result<BinaryInfo> {
            Ok(BinaryInfo {
                rom_offset: u32_at(h, base)?,
                entry: u32_at(h, base + 4)?,
                load_addr: u32_at(h, base + 8)?,
                size: u32_at(h, base + 12)?,
            })
        };
        let table = |base: usize| -> Result<Table> {
            Ok(Table {
                offset: u32_at(h, base)?,
                size: u32_at(h, base + 4)?,
            })
        };
        Ok(Self {
            title: ascii(&h[0..12]),
            game_code: ascii(&h[12..16]),
            maker_code: ascii(&h[16..18]),
            unit_code: u8_at(h, 0x12)?,
            version: u8_at(h, 0x1E)?,
            arm9: binary(0x20)?,
            arm7: binary(0x30)?,
            fnt: table(0x40)?,
            fat: table(0x48)?,
            arm9_overlays: table(0x50)?,
            arm7_overlays: table(0x58)?,
            banner_offset: u32_at(h, 0x68)?,
            arm9_autoload_hook: u32_at(h, 0x70)?,
            used_rom_size: u32_at(h, 0x80)?,
            header_crc: u16_at(h, 0x15E)?,
            computed_crc: crc16(&h[..CRC_COVERED]),
        })
    }

    pub fn crc_ok(&self) -> bool {
        self.header_crc == self.computed_crc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields_and_checks_crc() {
        let mut h = vec![0u8; HEADER_SIZE];
        h[..4].copy_from_slice(b"TEST");
        h[12..16].copy_from_slice(b"ABCE");
        h[0x20..0x24].copy_from_slice(&0x4000u32.to_le_bytes());
        h[0x28..0x2C].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        let crc = crc16(&h[..CRC_COVERED]);
        h[0x15E..0x160].copy_from_slice(&crc.to_le_bytes());

        let header = Header::parse(&h).unwrap();
        assert_eq!(header.title, "TEST");
        assert_eq!(header.game_code, "ABCE");
        assert_eq!(header.arm9.rom_offset, 0x4000);
        assert_eq!(header.arm9.load_addr, 0x0200_0000);
        assert!(header.crc_ok());

        h[0] = b'X';
        assert!(!Header::parse(&h).unwrap().crc_ok());
    }

    #[test]
    fn rejects_truncated_header() {
        assert!(Header::parse(&[0; 0x100]).is_err());
    }
}
