# The field's map screen: `minimap.gp2`, `.obg` pictures, `.bmmp` layouts, `obj_minimap`

What the field's top LCD is drawn with (`docs/re/bottom-screen.md` § 2.1): an area plan from
`/data/pack_lv5/minimap.gp2`, the party's dots and name strips from
`/data/ani/obj_minimap.NCER/.NCGR/.NCLR` (and the same three files inside
`/data/ani/obj_mm.pac`). The readers live in `nds_rom::nitro2d` (NCLR/NCGR/NCER) and
`dq9_formats::minimap` (`.obg`, `.bmmp`, `.pac`); `dq9re minimap` prints them.

The formats below are **the DQIX formats wiki's** (`Mini-Map`, `2D-Graphics`, `Pac`,
`Tagged-Data-Table`, read on the European ROM): it is the source of the bytes' meaning, cited
per section. Every count on this page was then measured again on our ROM, by the tests of
`crates/dq9-formats/tests/minimap_rom.rs` and `crates/nds-rom/tests/nitro2d_rom.rs`; where a
measurement disagrees with the wiki it is said so.

No game function is reimplemented here (data readers only), so no `port:` marker: the code
addresses cited are the ones `docs/re/bottom-screen.md` already established — `0x02020b98`
opens the archive, `0x02021578` is the mode-0 loader (`minimap.gp2` + `obj_mm.pac`),
`0x02020aa0` maps the VRAM banks to the sub engine.

## 1. What the cartridge holds

`cargo dq9re -- minimap` (evidence `counts.txt`):

| file | contents on our ROM |
|---|---|
| `data/pack_lv5/minimap.gp2` | **568 members**: 283 `.bmmp`, 268 `.obg`, 17 others (below) |
| `data/ani/obj_minimap.NCER` | 44 cells, mapping 0, no boxes |
| `data/ani/obj_minimap.NCGR` | 209 four-bit 8×8 characters |
| `data/ani/obj_minimap.NCLR` | 8 stored palettes, 16 colours each, slots 0–7 |
| `data/ani/obj_mm.pac` | 3 members, the three files above (its `.NCER` has 28 cells) |

The archive's other members: `z01` to `z05` as `.bncg`/`.bncl`, `pd_ab_kari.bncg/.bncl/.bnsc`,
`C01M0000.MAP`, `C01M0000.MBK`, `C01M0001.MBK` and `shipMPos.bin`. Only the `.bmmp`/`.obg` are
read here; the `z` sets and `pd_ab_*` are [unknown].

Over the whole cartridge (**501 Nitro 2D files**, `cargo` test `every_nitro_2d_file_parses`):

| kind | count | reach |
|---|---|---|
| `NCLR` | 139 | 3 loose (`data/ani/obj_minimap`, `tmap_field_obj`, `tmap_ufix_obj`), 136 in packs and GPC2 members |
| `NCGR` | 224 | 3 loose, 221 in packs and GPC2 members |
| `NCER` | 138 | 3 loose, 135 in packs and GPC2 members |

All 501 have a 16-byte header, a file-size word equal to the file, and blocks that end exactly
at it; the version is `0x0100` for palettes and cells and `0x0101` for characters on every
one. That is the wiki's evidence, re-measured.

**`.pac` on our ROM: 501 files are named `.pac`, not 468.** 33 of them are NARCs (head
`NARC`; `data/ani/bgii20/*.pac` and `bglii*`, `data/event_lv5/inevent.pac`,
`data/menu/bg_lv5.pac`, `data/scenario/flditem.pac`, `data/tmap/param.pac`…), 5 are
`tdata_<lang>` (`data/tmap/tdata.gp2`) and **463 are packs** — the wiki's 468 is our 501 minus
the 33 NARC-named files, and its 463 packs match exactly. Pack reader test:
`every_pac_parses_except_the_tdata_files`.

Two packs carry **Shift-JIS names** (`data/ani/bg_qm.pac` starts `qt\x82\x8c_01.bncg`,
`data/menu/bg_lv5.pac` `\x82\x8c\x82\x965.bncg`): names are bytes up to the NUL, mapped one
character per byte when not UTF-8.

The seven effect packs' end marker is a head whose **data size** and **step** are
`0xFFFFFFFF` while the head-size word stays `0x50` (`data/effect/ev001_kenhikari.pac`, 14,080
bytes, five entries). The wiki says "size and step"; the head size is not `0xFFFFFFFF`.
Rejecting exactly that marker is what the reader does; a step that is not the head plus the
data rounded up to sixteen is an error.

## 2. `.obg` — a picture

Eight-byte head, sixteen BGR555 colours, `tile count` tiles of 32 bytes, then a `u16` per cell
(row by row). Measured on all 268:

- **`40 + 32 × tiles + 2 × width × height` bytes, 268 of 268** (enforced by `Picture::parse`;
  the village's `M01M0001` is 35×19 cells, 659 tiles, 22,458 bytes; the field's `F01M0001`
  32×28, 822 tiles, 28,136).
- 97,930 tiles and **135,168 cells** over the 268 pictures; no cell names a tile past the
  count and none sets a bit above bit 9, so no flip and no palette bit is in use (rejecting
  them is the parser's rule; this ROM never triggers it).
- `unknown_02` is 0 and `unknown_06` 0 on all 268; `unknown_03` is 137 on 117 files and 247 on
  132 (nine other values), still [unknown].
- A pixel is a nibble, the **low nibble on the left** (the DS's own order for sixteen-colour
  characters). Drawn that way the village's `INN` sign reads correctly — the wiki's check.
- **Colour 0 is clear — INFERRED.** It is the DS's rule for a sixteen-colour background; here
  it occurs only outside the village's torn paper edge: **5,735 transparent pixels of 280×152**,
  and none on the field's picture, whose paper fills its rectangle (re-measured by
  `a_picture_decodes_to_rgba`). Nothing in the data proves the engine draws index 0 as clear
  rather than as its stored colour; the count is what is established.

The decode (`Picture::image`) gives RGBA8 rows, transparent for index 0; `dq9re minimap M01
--out …` writes it as a PNG (`M01M0001.png`, 280×152, 36,593 bytes: Stornway's plan with the
road along the bottom edge and its torn paper margin).

## 3. `.bmmp` — which picture, at what scale, with what marks

A tag-record table (`docs/re/table.md`), read through `Table`. The tags we use, counted over
the 283 layouts (files / records):

| tag | on | values | read as |
|---|---|---|---|
| `0x64` | 283 / 283 | two integers on 282 (`-18, -12` in the village), two floats on `S07M01` (`-15, -12`) | the picture's corner, in tiles |
| `0x66` | 283 / 283 | an integer (0 on all) then a string | the `.obg` name: `M01M0001`, `F01M0001` |
| `0x69` | 283 / 283 | a float on 267, an integer on 16 | the scale, pixels per map unit |
| `0x6a` | 221 / 221 | a string: `minimapbg2` 207, `minimapbg3` 13, `minimapbg4` 1 | the backdrop |
| `0x6b` | 282 / 359 | integers | the map ids the picture is drawn for |
| `0x6c` | 91 / 273 | two floats then integers | a mark and the maps it stands for |
| `0x70` | 268 / 268 | `(integer, string)` pairs, **520 pairs** | map codes by id |
| `0x65`, `0x67`, `0x68` | 283 / 283 each | 1, 1, and four 0s | [unknown] |
| `0x6d` | 78 / 78 | one integer | [unknown] |
| `0x6e`, `0x6f` | 1 / 1 each (`D01M01`) | `(0, "D01M0101_MG", 1)` and `0, 0, 104, 53, 130, 62` | [unknown] |

- The scale's integers are 4 on thirteen `C02`/`C04` layouts, `D17M04`, `M05`, and 1 on `O00`;
  the scales seen are 3.2 (village), 1 (field), 2 (the pass), 4, 8, 2.67, 0.67.
- **244 of the 268 pictures are named by a layout**, 283 of 283 layouts name a picture that
  exists in the archive. Names are the base name plus `.obg` (`0x66` stores the base name).
- `0x66` also carries the picture's short name inside `M01M0001_MG`-style members in
  `D01M01`'s extra `0x6e` record; not reused.

`F07`, `H07`, `M05` and `M12` begin with the word **16**, which the wiki flags as looking
like an empty LZ77 stream and as "not empty". It is their **record count**: each has exactly
16 records, with 5 strings (`F07`, `H07`) or 9 (`M05`, `M12`), and the table reader reads them
like the other 279. The wiki's alternative ("the table header's own size") is falsified: the
stream ends at the declared string table and the count of records is 16.

## 4. Where a point falls on the picture

**`pixel = position × scale − corner × 8`**, x across and z down, corner in tiles
(`minimap::axis`, `Layout::pixel`). The wiki infers it from fits; the three examples it states
are re-measured (`the_position_formula_lands_where_the_wiki_says`):

| layout | mark (x, z) | scale | corner | row | picture height |
|---|---|---|---|---|---|
| `M01` (village), the road's mark | −7.16, 16.31 | 3.2 | −18, −12 | **148** (148.19) | 152 |
| `S01M01` (pass), first mark | −18.52, 41.69 | 2 | −10, −13 | **4** (3.62) | 200 |
| `S01M01` (pass), second mark | 14.33, −50.19 | 2 | −10, −13 | **187** (187.38) | 200 |

**The row is rounded, not truncated**: 3.62 must land on 4, which truncation would put on 3.
The wiki's own table (rows 4 and 187) therefore selects rounding; the other two examples do
not distinguish the two. The scale being pixels per unit and the corner tiles is still read
from these fits, not from code — the game-side computation was not located.

Marks are in the map's own units, the units of a doorway (`docs/re/map-archive.md`): the
village's nine marks stand within four units of their doorways, eight of nine within three
(wiki's measurement, plausible against the numbers above). The party's position uses the same
formula; that the engine uses this exact expression is [hypothesis].

## 5. `obj_minimap` — the party strip and the dots

`Ncer::parse` of `data/ani/obj_minimap.NCER` (`the_party_strip_and_the_dots`):

- **44 cells**, one part each, mapping 0 (one tile a number, 32 bytes a number).
- **Cell 2 is the party panel**: one 64×64 square part at (0, 0), tile 8. Its **rows 0 to 15**
  are the 64×16 name strip drawn under the map at y = 176 (`minimap::party_strip` returns
  `(0, 0, 64, 16)`; the panel's own box is 64×64).
- **Cells 3 to 7 are the five dots**: one 8×8 part each, palette slot 4, tiles `0x48` to
  `0x4c` (blue, green, pink, yellow, red in the wiki's order).
- Cells 0 and 1 are 16×8 parts at x = 496 — **−16 read as signed**, centred on the cell's
  origin; that is the case the wiki uses for its INFERRED sign, and reading them unsigned
  would place them 496 pixels to the right, outside every picture. Supporting, not proof.
- The `.NCGR` holds 209 tiles (the panel's 64 tiles start at 8 and the dots follow at `0x48`),
  the `.NCLR` eight 16-colour palettes in slots 0–7 (`PCMP` count 8, `0xBEEF`, list at 8).
  `obj_mm.pac` member `obj_mm.NCER` has 28 cells (the panel cut narrower, per the wiki).

What a cell's own `u16` attribute means stays [unknown] (5, 23, 3, 9 and 6 occur here); the
panel's attribute is 23.

## 6. INFERRED, confirmed, and open

- **Confirmed on our ROM** (previously measured on the wiki's copy): file framing and block
  extents of 501 Nitro 2D files; palette depth 3, `0xBEEF`, colours filling the `PCMP` count
  (139 files; 120 with a `PCMP`, 19 without); NCGR depth 3, no width/height, characters at
  `0x18`; NCER cells at `0x18`, no boxes, `CEBK+0x0c..0x14` zero; tile numbers at
  `32 << mapping` bytes (the panel at tile 8 with 64 tiles, the dots at `0x48`); `.obg` size
  formula on 268; no cell past the count, no bit above 9; nibble order; the three formula
  examples; the strip and the dots.
- **INFERRED, unchanged**: a part's position is signed (nine bits of x, eight of y) — our
  positive case justifies it, an independent proof is missing; colour 0 is clear (the count is
  established, the engine's treatment is the DS rule); scale in pixels per unit and corner in
  tiles.
- **Corrected here**: the pass's first mark lands on row 4 only if the pixel row is rounded;
  the `.pac` end marker of the effect packs carries `0xFFFFFFFF` in the data-size and step
  words (head size stays `0x50`); `.pac` names can be Shift-JIS; 33 `.pac` files are NARCs, so
  the wiki's 468 is 501 not 468; the "16" first word of four `.bmmp` files is their record
  count.
- **[unknown]**: `0x65`/`0x67`/`0x68`/`0x6d`, `0x6e`/`0x6f` on `D01M01`, `0x66`'s first
  value, `unknown_03` of `.obg`, `CEBK`'s `unknown_0x10` and a cell's attribute, the `z` sets
  and `pd_ab_*`, how the DS scrolls a picture larger than 256×192, how a character's dot colour
  is chosen (the wiki could not tie it to these palettes), and the tab at the picture's top
  right.

## 7. Tools

- `cargo dq9re -- minimap` prints the counts above.
- `cargo dq9re -- minimap M01` prints `M01.bmmp`: picture, scale, corner, backdrop, maps,
  the nine marks (position, pixel, map codes) and the party strip. `M01M01` resolves through
  the layout's `0x70` codes to the same layout.
- `cargo dq9re -- minimap M01 --out build/…/M01M0001.png` writes the decoded picture.

## Evidence

`build/evidence/v3-map-screen-data/` (manifest `manifest.json`):

- `counts.txt` (`372c0ffc21b5`): `cargo dq9re -- minimap`.
- `M01.txt` (`3048d9e7e1d9`): `cargo dq9re -- minimap M01`.
- `M01M0001.png` (`c75bfb0c1e99`): the village's picture, 280×152.
- `tests.txt` (`f59158135276`): the four test runs (nitro2d, nitro2d_rom, minimap, minimap_rom).
- `verify.log` (`5aa906f145b8`): `verify.ps1 -RequireRom`, all five steps ok.
