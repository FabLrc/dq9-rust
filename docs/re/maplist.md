# Map list (`data/map/maplist9.bin`)

Index of all maps: a map's id (the one used by triggers, placements,
doors and `g_Zone3D+0`) leads to its code, which names its archives (`<code>.ambl`,
`<code>.amdj`). Implementation: `dq9_formats::maplist`; `cargo dq9re maps` lists the
maps, `cargo dq9re maps --state <state.ml1>` compares them with RAM.

## How the game reads it

`MapList_Load` (`0x020998d4`) passes the file, a record table (`table.md`),
to the handlers `g_MapListRecordHandlers` (`0x020f1524`):

| type | handler | effect |
|---|---|---|
| `0x66` | `MapList_RecCount` (`0x020995d0`) | allocates `n × 16` bytes, capacity = `n` (1,010) |
| `0x67` | `MapList_RecMap` (`0x02099608`) | 22 values → one 16-byte record, appended (`MapList_Append`, `0x02099a00`) if it has a code and space remains |
| `0x64`, `0x65`, `0x68`, `0x69` | nothing | |

The list lives at `GameState+0x468` (`GameState_MapList`, `0x02011594`): pointer,
count, capacity. `MapList_Find` (`0x02099960`) returns the **first** record with that
id; the 139 unshipped maps all have id 0.

## Record (16 bytes)

"Value n" = n-th value of the type `0x67` record (read with `Tbl_FieldInt`,
`Tbl_FieldStr`).

| bytes | value | meaning |
|---|---|---|
| `+0` u16 | 0 | map id |
| `+2` bits 0-14 | 1 | zone shared by the maps of a location (`Zone3D+4`; Stornway = 198) |
| `+4` bit 0 | 21 | [unknown] (4 maps); bits 1-7 uninitialised |
| `+5..+0xb` | 4 (string) | map code, 7 bytes copied, `+0xb` = 0: 6 characters at most |
| `+0xc` bits 0-3 | 3 | map kind, below |
| `+0xc` bit 4 | 9 | copied to `func_0208a9b4()+0x9c` by `Zone3D::SwitchZone` [unknown] |
| `+0xc` bits 5-6, bit 7 | 10, 12 | [unknown] |
| `+0xd` bits 0-6 | 6 | passed by `0x0209cb00` into a 47-byte table (with cases depending on `LightingManager+0x98`); [hypothesis, wiki Map-List] the music |
| `+0xd` bit 7 | 16 | [unknown] |
| `+0xe` bits 0-1 | 17 | space: 1 indoor, 2 outdoor, 0 none (wiki) |
| `+0xe` bits 2-5 | 18 | 8 out of 930 maps; [hypothesis, wiki] battle backdrop floor |
| `+0xe` bits 6, 7 | 19, 20 | [unknown] |
| `+0xf` | — | never written |

Values 2 (region) and 5 (developer label) are read then discarded; 7, 8, 11, 13,
14 and 15 are not read here (the wiki ties 14 and 15 to the map's position in the
world, read elsewhere).

**Kind (`+0xc` bits 0-3)**, distributed by the first letter of the code: 0 = 64 terrain
regions `F` (+ 9 others), 1 = dungeons `D`, castles, towers (157), 2 = towns, houses,
story locations `M`/`C`/`H`/`S`/`X` (383), 4 = battle backdrops `B` (182), 6 = caves
`Z` (165), 5 and 8 = `R` codes, 7 = `O00`/`O01`, 3 = one tower. When it is 0,
`Zone3D::SwitchZone` (`0x0201384c`) copies the map id into the protagonist object
(`+0x566`): [hypothesis] the last terrain region visited.

## Verification

- `Zone3D::SwitchZone`: `g_Zone3D+0` = current id, `+2` = previous id, `+4` = zone,
  `+8` = pointer to the record.
- On the demo melonDS states (`melonds-state.md`), the 1,010 computed records
  are identical to those in RAM, except undefined bytes (`MapRecord::to_bytes`); the
  current map is 109, `C01M09`, "Stornway / Church", zone 198.
- The DQIX wiki ([Map-List](https://github.com/DQIX/wiki/blob/main/Map-List.md)) numbers
  the values the same way; its readings of music, space and floor are
  kept as hypotheses, the rest is read from the code above.
