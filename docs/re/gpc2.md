# GPC2 archives (`*.gp2`) and Level-5 compression

Implementation: `crates/dq9-formats/src/{gpc2,level5}.rs`. Tool:
`cargo dq9re gp2 <path>` (lists, `--out` extracts) and `cargo dq9re extract --unpack`
(all contents into `build/rom/gp2/`).

## How it was found

1. The string `GPC2` appears only once in the ARM9 (`0x020f27f8`, tag table
   `GPC0`/`GPC1`/`GPC2` at `0x020f27e8`).
2. `FileTags_StaticInit` (`0x020e6720`) copies these tags into the globals
   `g_TagGpc0..2` (`0x020f27c8..d0`).
3. `g_TagGpc2` is only read by `Gpc2_LoadIndex` (`0x020d91ec`), which reads a
   0x14-byte header then decompresses the index; `Gpc_FindEntryByHash` (`0x020d902c`) and
   `Gpc_EntryLocation` (`0x020d8fdc`) give the entry format.
4. Hypotheses checked on the 1,671 `.gp2` files of the ROM: sizes, name hashes
   and decompression of every member (test `crates/dq9-formats/tests/real_rom.rs`).

## Level-5 compressed block

```
u32  method | (decompressed_size << 3)
...  body
```

| method | Body encoding (Nintendo BIOS codecs, without their header) |
|---|---|
| 0 | stored as is |
| 1 | LZ10 |
| 2 | Huffman, 4-bit symbols (low nibble emitted first) |
| 3 | Huffman, 8-bit symbols |
| 4 | RLE |

Observed methods (archives without flag) — index: 0 ×1,257, 2 ×403, 1 ×9, 3 ×2;
names: 1 ×1,657, 0 ×14; members: 1 (LZ10) ×42,402, 4 (RLE) ×7,763, 3 ×2,437, 2 ×392,
0 ×32. Sanity checks: the 758 decompressed `.nsbtx` start with `BTX0`, the
RLE texts are readable (`bin/bstr.gp2`).

## GPC2 archive

All positions are in **32-bit words**, relative to the start of the file.

| Offset | Type | Field |
|---|---|---|
| 0x00 | `[u8;4]` | `GPC2` |
| 0x04 | u16 | bits 0-11: entry count; bits 12-15: binary search depth (`ceil(log2(n))`) |
| 0x06 | u16 | index position (always 5 = right after the header) |
| 0x08 | u16 | name table position |
| 0x0A | u16 | data area position |
| 0x0C | u16 | decompressed index size |
| 0x0E | u16 | decompressed name table size |
| 0x10 | u32 | bits 0-27: data area size; bits 28-31: flags |

- Index and name table are each a Level-5 block.
- The data area runs to the end of the file (`(0x0A + size) * 4 == file size`).
- Flag `0x1` (bit 28): members stored **without** a Level-5 header. Seen only in
  `pack_lv5/enemy.gp2` and `prm/actdt_a.gp2`/`actdt_b.gp2`.
- On load, the game replaces the tag with `GPC1` for the decompressed in-memory version
  (`Gpc1_FindMember`, `0x020d9110`) **[hypothesis: `GPC0` = uncompressed variant]**.

### Index entry (12 bytes, sorted by ascending hash)

| Offset | Field |
|---|---|
| 0x0 | CRC-32 (zlib) of the member name |
| 0x4 | bits 0-23: member position (words) relative to the data area; bits 24-31: low byte of the name position |
| 0x8 | bits 0-23: stored size in bytes; bits 24-31: high byte of the name position |

Name position (bytes in the name table) = `(entry[0x8] >> 24) << 8 | entry[0x4] >> 24`.
Names are zero-terminated.

## Contents

53,639 members in 1,671 archives. Extensions: `.bin` (47,227), `.nsbmd` (1,913),
`.nsbtx` (758), `.stb` (689), `.nat` (571), `.mes` (529), `.pac` (375), `.bmmp` (283),
`.obg` (268), `.chr` (169), …

Example: `data/scenario/M02M0.gp2` contains `NNN_{de,en,es,fr,it}.bin`, the dialogue texts
per language (see `docs/re/text.md`).
