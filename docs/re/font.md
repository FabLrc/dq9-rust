# Fonts

Implementation: `crates/dq9-formats/src/font.rs`. Tool: `cargo dq9re font me` (glyph
sheet) or `cargo dq9re font me --text "Caract<'e>ristiques"` (renders one line), PNG in
`build/`.

## How it was found

1. The message box engine (`MsgWin_DrawGlyphs`, `0x02066d00`) calls
   `Font_Kerning` (`0x020425f4`), which reads a global font array.
2. That array is filled by `Font_LoadAll` (`0x02042954`) → `Font_Load` (`0x02042814`), which
   loads `data/pack_lv5/fi_%s.bin` and `fd_%s.bin` for `s7` then `me`.
3. The decoded sheet (`fd_me.bin`) shows the expected alphabet; the strings of the glyph
   table (`fi_me.bin`) match exactly the accent tags of the texts (`<'e>`,
   `<^a>`…). Test `fonts_cover_accent_tags`: every accent tag used in the
   texts of the 5 languages is a glyph of the `me` font.

The files `pack/font/*.mes` (528) and `pack_lv5/font_lv5.gp2` (`f8.mes`) are partial
kanji fonts (Shift-JIS codes) inherited from the Japanese version; they are not used
for European text **[hypothesis: never loaded in the EU version]**.

| Font | Index | Files | Glyphs | Kerning pairs | Sheet | Space width |
|---|---|---|---|---|---|---|
| `s7` (small) | 0 | `fi_s7.bin`, `fd_s7.bin` | 245 | 22 | 1312 × 12 | 2 |
| `me` (messages) | 1 | `fi_me.bin`, `fd_me.bin` | 242 | 105 | 1600 × 12 | 3 |

The space width comes from `g_FontSpaceWidth` (`0x020e7aa4`) in the ARM9.

## `fi_*.bin`: glyph table

```
0x00 "1.1\0"
0x04 u32 glyph count
0x08 u32 kerning pair count
0x0C u32 kerning table position
0x10 u32 glyph table position
0x14 u32 string block position
```

Positions are relative to the start of the file (the game turns them into pointers on
load).

Glyph (8 bytes):

| Offset | Field |
|---|---|
| 0 | u32 position of the string replaced by the glyph (`A`, `<'e>`, `<1>`, `<heart>`…) |
| 4 | u8 width in pixels |
| 5 | bits 0-5: string length; bit 6: vowel; bit 7: lowercase |
| 6 | u16 start column in the sheet |

Bits 6 and 7 serve the language rules: `<CAP>` uppercases the next glyph
if it is lowercase; the `<IF_VOWEL_…>` conditions (French elision, "a"/"an") test the
vowel **[hypothesis for this last point: the test goes through `FUN_02067ffc`, not read yet]**.

Kerning (4 bytes): `u8 left glyph, u8 right glyph, s8 adjustment, u8 0`.

## `fd_*.bin`: glyph sheet

```
0x00 "1.0\0"
0x04 u16 width, u16 height (pixels)
0x08 u32 data size
0x0C u32 data position
```

Data: 1 bit per pixel, row by row, most significant bit first, 1 = ink.

## Layout (`Font_MeasureString`, `0x020420f8`)

For each text position:

1. Search for the **first** glyph, in table order, whose string is a prefix of the
   text (`Font_FindGlyph`, `0x020424f4`).
2. Glyph found: advance = kerning(previous, glyph) + width + 1, then skip the string
   length.
3. No glyph (e.g. space): advance = space width + 1, no kerning afterwards.

The final width is the sum of advances minus 1. `MsgWin_DrawGlyphs` draws each glyph at
`x + kerning`, then advances by `width + 1`; code `0xFF19` is a space and `0xFF18` moves to
the next line.
