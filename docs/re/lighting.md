# `.bats` lighting scripts

Reference source: `crates/dq9-formats/src/lighting.rs`. USA decompilation sources
(`tools/dqix-decomp`, commit b5c617a): `src/Graphics/LightingInfo.cpp`
(`LightingInfo::LoadFromScript` and the `s_lightingOpcodes` handlers),
`src/Resource/Script.cpp` (record table), `include/Graphics/LightingInfo.h`.

EU addresses: `ExecuteLightingScript` `0x0207b938` (size `0x64`), `LightingInfo::Initialize`
`0x0207b99c`, `Reset` `0x0207b9dc`, `LoadFromScript` `0x0207ba1c`. Caller: `Zone3D_LoadBats`
`0x02014424`, which stores the result in `Zone3D+0x10c` (see `map-archive.md`).

USA → EU addresses: ARM9 USA `0x0200F3B8..0x0200FB20` → +0x8, `0x0200FB20..0x020EEBD0` → +0x10.

## Container

- Archive `data/map/ats_<c>.ambl` (NARC), member `<name>.bats`, LZ10-compressed (BIOS header
  `0x10`, 24-bit size). Decompressed: 560 to 1072 bytes.
- The body is a record table in the sense of `table.rs` (`Table::parse`): 16-byte header
  (record count, pool offset, pool size, string count), then the
  records, `0xFF` padding, string pool.
- ROM measurement (EU): 504 `.bats` members in the 14 archives `ats_B`..`ats_Z`, all decodable;
  6400 records in total, 9 to 16 per script. Pool empty in all 504 (no strings).

## Records

Each record has an opcode (`kind`) and `INT` (type 1) or `FLOAT` (type 2) fields. Type 0
(string) and type 3 fields are invalid here: the scripts read contain none.

| Opcode | Handler | USA function (`s_lightingOpcodes`, `LightingInfo.cpp`) | EU address | ROM count |
|---|---|---|---|---|
| `0x64` | declares advanced lighting | `LightingScript_DeclareAdvancedLighting` | `0x0207b2dc` | 236 |
| `0x65` | nop | `LightingScript_Nop` | `0x0207b314` | 0 |
| `0x66` | alternate advanced entry | `LightingScript_CreateAdvancedLightingEntry_Alternate` | `0x0207b31c` | 0 |
| `0x67` | declares basic lighting | `LightingScript_DeclareBasicLighting` | `0x0207b4d0` | 268 |
| `0x68` | basic lighting entry | `LightingScript_CreateBasicLightingEntry` | `0x0207b504` | 715 |
| `0x69` | fog entry | `LightingScript_CreateFogEntry` | `0x0207b648` | 3529 |
| `0x6a` | advanced entry | `LightingScript_CreateAdvancedLightingEntry` | `0x0207b78c` | 1652 |

An unknown opcode is ignored.

### Slot index

Field 0 of each entry is the slot index, read in `0..7` (`SLOTS = 7`). A value
outside this range, negatives included, is ignored. On the ROM, no index is out of
bounds. The USA code indexes without a check for `0x68` and `0x6a`: undefined behavior, not
reproduced.

### Gradient

`0x64` and `0x67` read a gradient center offset (float, field 0) if the field count
is non-zero; otherwise it is 0.

### Opcode `0x66`

Present in the USA handler table, absent from the EU ROM (0 occurrences in 6400 records).
Its layout is reproduced as described by the USA code, without verification on the ROM.
`[hypothesis]`: field 6 is read then discarded, as in the USA code.

### Basic lighting (`0x67`, `0x68`)

`0x68` reads the position (fields 1-3), the background and secondary background colors (4, 5), the
pot color (6), `unk_70` (7), `unk_54` (8), the sprite and model diffuses (9, 10) and the
edge color (11); the edge color marks the entry as filled.
`[hypothesis]` (reported by peer `LightingFogRender`, not verified in the sources read): `unk_70` = brightness, `unk_54` = contrast. Sources: `LightingInfo.cpp` (`0x68`, lines 101-102), `LightingManager.cpp` (lerp of the two arrays, l. 216-233). `[unknown]`: role of `positions`.

### Fog (`0x69`)

Fields: index (0), enable (1), color (2), type (3), depth (4), offset (5), packed
density table (6-13, little-endian, 0x20 bytes), alpha (14). Application to rendering: § "Rendering and validation against melonDS" below.

#### Fog registers (ARM9 EU, disassembly)

Read in `build/rom/arm9.bin` (extracted from the player's ROM, not committed) and in Ghidra:

- `func_020c54a4` (`0x020c54b4`) `(enable, type, depthShift, offset)`: if `enable == 0`,
  `DISP3DCNT &= 0xCF7F` (clears enable, bit 7). Otherwise `FOG_OFFSET = offset` then
  `DISP3DCNT = DISP3DCNT & 0xC0BF | type << 6 | depthShift << 8 | 0x80`. Pool: `0x0400035C`
  (FOG_OFFSET), `0x04000060` (DISP3DCNT), `0x0000CF7F`.
- `func_020c5574` (`0x020c5584`) `(table)`: calls `func_020ca50c(table, 0x04000360)`, which copies
  32 bytes (8 words): write of the FOG_TABLE density table (32 entries).
- DISP3DCNT bits (GBATEK, confirmed by the melonDS source, `GPU3D_Soft.cpp`
  `ScanlineFinalPass`): bit 6 = mode (0 color + alpha, **1 alpha only**), bit 7 = enable,
  bits 8–11 = depth shift. The game writes `type << 6`: `type` 0 = color and alpha, 1 =
  alpha only, as in USA `FogInfo`.
- `FOG_COLOR` (`0x04000358`): written `color | alpha << 16` by `ProcessZoneChange`
  (`0x020519f4`, +408) and `RecomputeAdvancedLighting` (`0x02051d38`, +1272); verified by
  Ghidra EU xrefs (writes to `0x04000358`: these two functions and `func_020c51dc`).
- The `0xCF7F` mask (disable) also clears bits 12–13 ([unknown], no known effect).
- `DISP3DCNT` in the melonDS states `field.ml1`, `demo.ml1`, `before.ml1`, `moved.ml1`
  (`GP3D` section, see next §): `0x1b9` = textures (bit 0), alpha blending (3),
  anti-aliasing (4), **edge marking (5)**, fog enabled (7), shift 1; neither
  toon nor highlight (bit 1 at 0): the data does not use toon.

RAM validation (melonDS savestate `field.ml1`, `LightingManager` at `0x02107930`, 0x9c bytes):
the `Initialize` (USA) default values are present identically in EU: offset `0x5800`
at `+0x58`, color `0x7FFF` at `+0x5C`, alpha `15` at `+0x60`, `densityTable[i] = 2i` (32 bytes) at
`+0x64`. These offsets are [hypothesis]: they come from the value, not from a structure type.
The savestate `GP3D` field is read following melonDS `GPU3D::DoSavestate` (GPL code read to
understand the format, nothing copied): `DISP3DCNT` (u32), toon (64 bytes) and edge
(16 bytes) tables, `FOG_COLOR`, `FOG_OFFSET`, `FOG_TABLE` (32 bytes). In `field.ml1` (`C01M09`):
`EDGE_COLOR` = `0x0466` ×8, `FOG_COLOR` = `0x001f7fff`, `FOG_OFFSET` = `0x5800`, null table,
shift 1: exactly entry 0 of `C01M0300.bats` (edge `0x466`, fog `offset 22528`,
color `0x7fff`, alpha 31, null table, shift 1), fog enabled even though the script's
"enabled" field is false (`ProcessZoneChange` passes `true`).

### Advanced lighting (`0x6a`)

Light 1 (fields 1-5), light 0 (6-10), colors (11-17).

The basic and advanced blocks are kept separate. The game overlays them in a union
`[unknown]`: the parser does not reproduce the aliasing.

## Verification

- Test `lighting_scripts_parse` (`crates/dq9-formats/tests/real_rom.rs`): 504 members,
  all decoded by `LightingInfo::parse`.
- Synthetic tests in `lighting.rs`: each handler, ignored index, missing parameter,
  invalid type, truncation.

## Time of day and entry selection

Module `dq9_core::lighting`. Sources: `GameTime.cpp`, `LightingManager.cpp`, `AtmosphericEffect.cpp`
(USA decompilation), checked against the EU code (Ghidra) and the RAM of a melonDS state.

### The clock

- `GameState+0x3cc` (EU `0x020f37a4`): seconds, float; `+0x3d0`: day length,
  `420.0`; `+0x3d4`: `1/60` per tick (RAM: `0x3c888889`); `+0x3d8`: running; `+0x3dc`:
  phase. `AdvanceDayTimer` (`0x02010250`) adds `ticks * 1/60` if the clock runs; `SetDayTimer`
  (`0x02010298`) wraps the time into `0..420` then picks the phase; `SetTimeOfDay`
  (`0x02010374`) sets the time to the start of the phase.
- Thresholds read in EU RAM (`0x020f33bc`): `420`, `390`, `210`, `180`: night `0..180`, morning
  `180..210`, day `210..390`, evening `390..420`. The USA table is indexed `[2]`, `[3]`, `[4]` for
  the same values; the EU indexing was not re-read instruction by instruction **[hypothesis]**,
  but the values are those of the Time-Of-Day wiki page.
- `ov_017::0218d0ec` (once per game frame): calls `AdvanceDayTimer` only on a
  map of type `0` or `7` (field, ocean, sky), then copies `GetDayTimer` and `GetTimeOfDay` into
  `LightingManager+0x94` and `+0x98` (except during certain modes: `func_02046b24` values 3,
  4, 10 and 0x16 **[unknown]**: the code is not named). The same code raises story flag
  `0x1140` when the phase switches to morning or night.
- Loading a save (`func_020aaf84`, `0x020aaf94`+780): `SetDayTimer(read time)`, then
  copy into `+0x94` / `+0x98`, `SetZone`, `RecomputeAdvancedLighting`.
- In our port the phase is an **explicit input** (`--time night|morning|day|evening`,
  `--clock <second>`, Web drop-down list, `T` key): `DayClock` models the clock, the
  DS RTC plays no part. The game does not read the RTC for the phase: only the
  `GameState` clock counts (the Time-Of-Day wiki confirms it: 420 s per day, new game at 210 s).

### Entry selection for a map

- The `.bats` script is chosen by `Zone3D_QueueLighting` / `Zone3D_StepLighting` (map
  name, `map-archive.md`) and stored in `Zone3D+0x10c` (`LightingInfo`). `maybeMode_`: `1` basic,
  `2` advanced, `0` none.
- Slot index (`LightingManager+0x90`, override; `0` = unused): if
  non-zero it indexes all tables instead of the phase. **[unknown]**: never observed non-zero
  (USA `LightingManager.h`); reproduced but not used by the application.
- **Advanced mode** (fields, battlefields; 236 scripts): `ProcessZoneChange` fills in
  the missing entries (`FillMissingEntries` of the advanced and fog blocks, `0x020504b4`,
  `0x02050840`), then `RecomputeAdvancedLighting` (`0x02051d38`, every frame) calls
  `GetCurrentAdvancedLightingValues` (`0x02050c30`): current phase `i`, next `(i+1)%4`,
  `t = (time − (end of phase i − 2 s)) / 2 s` clamped to `[0, 1]` (end of phase: 180, 210, 390, 420).
  Colors blended per channel as truncated `char` (`InterpolateColors`, `0x02051160`), directions
  blended linearly. Fog follows `ComputeFogInfo` (`0x02051254`, same `t`, multiplied by
  the light scale): color, alpha, offset and table blended, type and depth shift
  of the current phase. Light directions are rounded to 1.19.12 (half away from zero),
  normalized by `VEC_Normalize` (`0x020c2f28`), capped at `0xff7`, then `SetLightVector`
  keeps bits 3..12: signed 1.0.9 vector (`dq9_core::lighting::light_vector`). Lights 2 and
  3 are black; lights 0 and 1 have the interpolated color.
- **Basic mode** (villages, buildings, grottoes; 268 scripts): `ProcessZoneChange` (`0x020519f4`) reads
  the **phase** entry without blending. If the time exceeds `end − 4 s`, the manager switches to the
  next phase and **sets the `GameState` clock to its start** (`SetDayTimer`):
  `settle_basic_entry`. It writes the entry's fog (`func_020c54a4(true, type, shift, offset)`,
  `FOG_COLOR`, `func_020c5574(table)`), the edge color, `SetDiffuseAmbientColors(model, 0)`;
  lights stay black. Models whose meshes send colors
  (`mesh+4 & 2`) are retouched once at load by
  `ModelTransformTintBrightnessContrast` (`0x020516a0`) unless the resource carries flag 0x20:
  `ColorTransformTintBrightnessContrast` (`0x02050980`) multiplies each 5-bit channel by
  `positions[i]` (`func_0207ba28`, EU `0x0207ba38`), adds the "brightness" `unk_70`, then the
  "contrast" `unk_54³` (added above 0.5, subtracted below, without crossing 0.5).
  **Verified in RAM** (`field.ml1`, `LightingManager+0x2c..0x3c`): `0.81`, `1.16`, `2.0`,
  brightness `0.03`, contrast `0.017576` = `0.26³`; these are the fields `unk_0`, `unk_70` and `unk_54`
  of entry 0 of `C01M0300.bats` (the script of `C01M09`), read by our parser.
- The background: `DrawBackgroundGradient` (`0x02052420`): basic mode, `background` /
  `background_second` colors of the phase; advanced mode, interpolated colors. Vertical gradient whose
  horizon color sits on the line where a point 2048 units away in the camera's horizontal
  direction falls (`MaybeComputeHorizonPosition`, `0x02052360`), plus `gradientCenterOffset_`.
- The game overlays `Fog::OFF` when `maybeMode_` is neither 1 nor 2 (map without `.bats`).

### Driven by event scripts (J3)

`BeginFade` (`lightRGBScale_`, native 578), the non-zero override index (548/549), the cutscene
phase (588/589) and the fog (804) are set by events
(`dq9_core::cutscene::SceneLight`, `dq9_app::lighting::scene_environment`, [`stb-natives.md`](stb-natives.md#effects-executed-on-screen-j3)):
the scale weights the colors of the lights, of the background **and of the gradient background** (`DrawBackgroundGradient`,
USA `LightingManager.cpp` l. 1087; `Lighting::select` applies `scale_channels` to the background and the
horizon), the duration is counted as `2 × value` ticks **[hypothesis]**. Check: on `21520`,
`SceneLight::scale` goes from 1.0 to 0.6 over 80 ticks then back (test
`fade_is_linear_and_ends_on_target`, darker capture during the fade).

### Not modeled

Forced mode of `ov_017` (`1 << 9`: battle phase read at `+5`), entry 4 of the
Starflight Express wagon (`zone 6401`), NPC `ambient` colors (`ApplyAmbientColorToModel` is only
applied to terrain models of an advanced world, see `func_02016874`), shininess effects (table
`SetShininessTable`). On basic maps, the light scale does not change the vertex
tint (`ColorTransformTintBrightnessContrast` does not depend on it in our model): only the background
follows it **[hypothesis]**.

## Rendering and validation against melonDS

Implementation: `dq9_render::lighting` (`Environment`), `shader.wgsl`, `background.wgsl`,
`edge.wgsl`. `dq9-app::lighting` connects `dq9_core::lighting::Lighting` to the renderer.

| Element | Source | State |
|---|---|---|
| DS lights (GBATEK / melonDS `CalculateLighting` equation), map model normals, `DIF_AMB`, `SPE_EMI`, bits 0-3 of `POLYGON_ATTR` | `LightingManager::SubmitToRenderConfig` `0x02052254` | applied per vertex, **not compared to melonDS** (no melonDS state in an advanced zone available; in a basic zone lights are black) |
| basic tint (vertex colors of `mesh+4 & 2` meshes) | `0x02050980`, `0x020516a0` | applied, input values verified in RAM; image **compared to melonDS** on `C01M09` (below) |
| sprite and character diffuse | `spriteDiffuseColor_`, `modelDiffuseColor_` | multiplies their color; basic map color read from `.bats` |
| background | `DrawBackgroundGradient` | gradient applied; **not compared** (the `C01M09` camera does not see the background) |
| fog | `func_020c54a4`, `FOG_COLOR`, `FOG_TABLE`, melonDS equation | **validated against melonDS** on density (below), alpha-only mode not validated |
| edge marking | `DISP3DCNT` bit 5, `EDGE_COLOR` | applied (`edge.wgsl`, melonDS rule: neighbor with a different and farther polygon ID), validated by eye only on `C01M09`; on `M01` it outlines ground patches with IDs 1 to 6, unstable outline because of coplanar decals (DS behavior not verified). Width: one DS pixel (3D screen height / 192) |
| toon, highlight, anti-aliasing | bits 1 and 4 of `DISP3DCNT` | toon/highlight: bit 1 at 0, so unneeded; anti-aliasing not done |

Fog equation (melonDS `CalculateFogDensity`, verified on hardware by its authors):
`z` = 24-bit depth of the Z buffer (`((z_clip * 0x4000) / w + 0x3fff) * 0x200`); if
`z < FOG_OFFSET * 0x200` density = `table[0]`, otherwise `zz = ((z − offset) >> 2) << DISP3DCNT[8..11]`, index
`zz >> 17` (32 at most), fraction `zz & 0x1ffff`, interpolation between entries `i−1` and `i` of the
table where entry 0 is doubled and the last one repeated (internal copy of 34 values), density capped at
128 (≥ 127 → 128); `color = (fog * d + pixel * (128 − d)) >> 7` with the 5-bit fog widened
to 6 bits (`2c + 1`, `0` stays `0`). Only polygons whose `POLYGON_ATTR` bit 15 is set are
fogged; the comparison target (C01M09) has them only for sprites.

### Comparison with melonDS (state `field.ml1`, `C01M09` at night)

`scripts/melonds-capture.ps1` loads a melonDS state in the portable window and saves the screen
(`build/scratch/ref-*.png`, 256×192). Our image: `--map C01M09 --time night --chapter 19
--step 2 --substep 1 --night --eye=0.21,6.87,7.84 --target=0.21,0.87,-1.38 --size 256x192`
(camera read from the state's RAM, `0x027e3950`, the game's projection).

- **Image without forced fog** (tint equation and edges): mean absolute difference
  (excluding the hero and the dialogue window, 256×105 pixels) **12.8 / 255** per channel (R 12.8, G 13.8, B 11.7);
  median 7; 43% of pixels below 8. Our render is on average 3 to 4 levels darker.
  Known gaps: no texture filtering or 6-bit rounding like the DS, hero replaced
  by a marker, edges and shadows in supersampled 256×192 pixels.
- **Fog**: the state is modified (`FOG_COLOR`, `FOG_OFFSET`, `FOG_TABLE`, `DISP3DCNT` shift
  written into the `GP3D` section before loading) and the melonDS image compared to ours with
  `--fog OFFSET,SHIFT,COLOR,D0..D31` (same values). The effective density (share of the fog
  color on the NPC pixels, estimated by least squares with the fog-free color):

| table | offset / shift / color | melonDS | us |
|---|---|---|---|
| constant 64 | `0x5800` / 1 / blue | 0.506 | 0.498 |
| `3i` | `0x5a00` / 2 / green | 0.632 | 0.631 |
| `127 − 4i` | `0x5400` / 0 / yellow | 0.750 | 0.745 |
| `5i` | `0x6000` / 2 / cyan | 0.822 | 0.828 |
| `4i` | `0` / 0 / magenta | 0.878 | 0.891 |

  The formula (depth, offset, shift, interpolation, blend at 128) matches within 0.01 on
  one depth point (the NPC): **no validation on terrain** (no `C01M09` material
  is fogged), **nor of the alpha-only mode** nor of the `FOG_COLOR` alpha.
- Not done: advanced zone (lights, sky gradient, 2 s fade) compared to melonDS: it would need
  a melonDS state on a field; the existing states are all at `C01M09`.
