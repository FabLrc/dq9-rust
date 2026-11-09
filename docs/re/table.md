# Level-5 record tables (`.bin`, `.bmmp`, `.bcfg`)

Implementation: `crates/dq9-formats/src/table.rs`. Tools: `cargo dq9re table <archive.gp2>
<member>` (prints the records), `cargo dq9re text --lang en` (all texts).

Generic format of most game data: dialogue (`scenario/*/NNN_xx.bin`),
menus (`bin/menu`), parameters, maps (`map/*.bin`, position floats), etc.
The format name is unknown **[hypothesis: ancestor of the "cfg.bin" of Level-5 3DS games]**.
Game reader: `Tbl_*` (`0x02030644`–`0x02030b8c`), described in `dialogue.md`.

## Evidence

Hypothesis built on `scenario/M02M0/018_en.bin`, then checked on all data
(test `record_tables_parse` in `crates/dq9-formats/tests/real_rom.rs`):

- 47,386 GPC2 members parse without error: 47,075 `.bin` (of 47,227), all 283 `.bmmp`
  and the 28 `.bcfg`. The remaining 152 `.bin` have other formats (e.g.
  `pack_lv5/minimap/shipMPos.bin`).
- Every string offset points at a string start, and header word 3 is
  exactly the number of distinct referenced strings.
- Type 2 values are plausible floats (`map/loola/*.bin`: `-67.27`, `0.17`…).

## Structure

```
0x00 u32 record count
0x04 u32 string block position
0x08 u32 string block size
0x0C u32 distinct string count
0x10 records…
     0xFF bytes up to the string block
     string block (0-terminated strings), then padding to 16 bytes
```

Record:

```
u16  type (file-specific meaning)
u8   n = field count
u8[ceil(n/4)] field types, 2 bits per field, least significant bit first
     0xFF padding up to a multiple of 4 bytes
u32[n] values
```

| Type | Value |
|---|---|
| 0 | string: offset in the string block; `0xFFFFFFFF` = absent |
| 1 | 32-bit integer |
| 2 | 32-bit IEEE float |
| 3 | never encountered (rejected) |

Example (`018_en.bin`, record header `01 00 04 15`): type 1, 4 fields of types
`1,1,1,0` → three integers then a string. With 5 fields (`02 00 05 55 00 FF FF FF`) the types
take two bytes.

Strings are ASCII with `<…>` tags (see `text.md`); a few debug strings
are Shift-JIS (`bin/presetdt`).

## Dialogue (`scenario/<place>/NNN_<lang>.bin`)

Types 1, 2, 4, 5; 4 fields `[a, b, slot, text]` or 5 fields
`[a, b, night, slot, text]`. Full meaning and selection algorithm: `dialogue.md`.
