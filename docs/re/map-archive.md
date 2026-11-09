# Map archives (`.ambl`, `.amdj`) and map assembly

What the game reads to display a map and how it assembles the pieces, from
`Zone3D::SwitchZone` to the placed instances. Implementation: `dq9_formats::map`
(`MapFiles`, `ZoneInfo`, `BlockList`, `Manifest`, `Map::assemble`); `cargo dq9re map M01`
prints the result, `cargo dq9re map --state <state.ml1>` compares it to RAM. The content of
`.nsbmd`/`.nsbtx`/`.col2` is out of scope here (`g3d.md`). Function names:
`re/symbols.txt`, section "Map archives".

Sources: reading the European ARM9 code in Ghidra (EU addresses below), USA decompilation
for a few library functions (`FileIO.cpp`, `ExtendedVectorMath.cpp`,
`AtmosphericEffect.cpp`, header `Zone3D.h`), checks over the whole ROM and on a melonDS
state. The DQIX wiki (Map-Archive, Map-Objects, Map-Textures) served as starting
hypotheses; discrepancies are listed at the end.

## Loading chain

`Zone3D::SwitchZone` (`0x0201384c`) resets `Zone3D`, records the map id (`+0`), the map list
record (`+8`, `maplist.md`) and queues `data/map/maplist9.bin`. Each frame,
`Zone3D_StepLoad` (`0x02013e28`) advances the steps, each one waiting for its file
(`BackgroundLoader`):

| step | function | file | effect |
|---|---|---|---|
| 1 | `Zone3D_StepMapList` (`0x02013fc4`) | `maplist9.bin` | zone settings (below), then queues the `.ambl` |
| 2 | `Zone3D_StepAmbl` (`0x02014118`) | `<code>.ambl` | textures, block list, `.dat`; then lighting |
| 3 | `Zone3D_StepLighting` (`0x02014c14`) | `ats_<c>.ambl` | `<name>.bats` → `LightingInfo` (`Zone3D+0x10c`); then `.amdj` |
| 4 | `Zone3D_StepAmdj` (`0x0201470c`) | `<code>.amdj` | `.bmdj` blocks, parts, instances; then `.mse` |
| 5 | `0x0201499c` | `<effects>.mse` | `AtmosphericEffectSet::ProcessArchive` |

If the `.ambl` or an `.amdj` fails to load (detailed status ≠ 2), the step ends **without
queuing the next one**: without `.ambl`, nothing else is loaded; without `.amdj`, no
`.mse`. Missing lighting stops nothing (step 3 moves on to the `.amdj` in every case).

Compressed members are read by `DecompressLZ77FileIntoScratchSpace` /
`IntoAllocatedSpace` (`0x02075618`, `0x02075664`): size = high 24 bits of the first word,
body always decoded as LZ10 regardless of the type byte. All 755 `.bmdj` and 667 `.bmbl`
start with `0x10` (LZ10) with a non-zero size.

## Archive names

`Zone3D_QueueAmbl` (`0x0201404c`) and `Zone3D_QueueAmdj` (`0x020145b8`), folder `data/map`:

| map | `.ambl` | `.amdj` |
|---|---|---|
| grotto floor, ids 40001–40096 (`Zone3D_IsGrottoFloor`, `0x0201b5c0`) | `Z0<e>M01.ambl` | `Z0<e>M01.amdj` |
| other grotto map, ids 41101–41505 (`0x0201b5e8`) | `Z0<e>M99.ambl` | `Z0<e>M99.amdj` |
| ocean (10000, `O00`), sky (10100, `O01`) | `<code>.ambl` | `<code>b.amdj` then `<code>a.amdj` |
| others | `<code>.ambl` | `<code>.amdj` |

`<e>` = `ActiveGrottoClass::GetActiveGrottoEnviron`; for floors, 0 counts as 1 and values
above 5 as 5, not for the other grotto maps. For ocean and sky, `Zone3D+0x42c` counts the
queued halves: after `b` (counter 1) step 4 queues `a` instead of finishing.

The SDK file system compares paths **case-insensitively** (`FSi_StrNICmp`): codes `S09M03`
and `M03M05` only have `S09m03.ambl`/`m03m05.ambl` (`map::rom_file`).

## Zone settings (`Zone3D+0xc`, `ZoneInfo`)

0x40-byte structure (`substruct_c_` of `Zone3D.h`), defaults set by `SwitchZone`.

| field | source | meaning |
|---|---|---|
| `+0xc` (10 B) | map list, value 11 | lighting name (`C01M0300` for the Stornway church) |
| `+0x16` (16 B) | `.dat`, type `0x6d` | atmospheric effects: `data/map/<name>.mse` (`AtmosphericEffectSet::LoadArchive`) |
| `+0x26` (16 B) | `.dat`, type `0x68` | a code (`C01`, `T00` on 150 battle backdrops) [unknown] |
| `+0x36` | `.dat`, type `0x65`; default 0x7fff | [unknown] |
| `+0x3c` | `.dat`, type `0x66`; default 10 | 21 on almost every map [unknown] |
| `+0x40` | list, value 13 × 4096 reduced to `[0, 2π]` | **[hypothesis]** orientation on the world map |
| `+0x44`, `+0x48` | list, values 14 and 15 (`fx32`) | **[hypothesis]** position on the world map |

- Map list: `Zone3D_LoadMapList` (`0x0201c734`) rereads `maplist9.bin` with its own
  handlers (`g_ZoneMapListHandlers`, `0x020ef2c0`); only type `0x67` (`0x0201c60c`) acts,
  on records whose value 0 is the current id. A missing value 11 leaves the name empty.
- `.dat`: `Zone3D_LoadDat` (`0x0205e114`, `g_DatHandlers` `0x020f051c`, types `0x64`–`0x6e`,
  the others have no effect). 657 files; 15 name a `.mse` (the 12 `.mse` of `data/map`).

## `.ambl`: textures, block list, `.dat`

`Zone3D_StepAmbl` mounts the archive and walks it twice in member order; only what follows
the last `.` of the name counts (`strrchr` then `strcmp`):

1. each `.nsbtx` becomes a named `Model3D` (`0x0201446c`), **prepended** to
   `Zone3D+0x418`: the list is in reverse archive order;
2. `.bmbl`: clears `Zone3D+0x6c` (`0x0201e258`, 0x88 bytes) then fills it (`0x020143a0`);
   `.dat`: settings above; `.bpos`: extends `Zone3D+0x6c` without clearing it
   (`0x020143e8`).

All textures of the archive are loaded; **nothing selects among them**: type `0x6c` of the
`.bmbl` (texture name, e.g. `C01M09T1`) has no handler.

### Block list (`.bmbl`/`.bpos`, `BlockList`)

Handlers `g_BlockListHandlers` (`0x020ef388`). Two types build the block list:

| type | handler | values |
|---|---|---|
| `0x64` | `0x0201c7ac` | n: new empty list of n entries of 0x58 bytes (`0x0201e26c`) |
| `0x65` | `0x0201c7d4` | id; offset x, y, z (`fx32`, `ToVec3fix`); name; [name 2]; [name 3] |

An entry without a name is ignored; the others are added while there is room
(`0x0201e314`). Names are copied into 16-byte fields (`+0x10`, `+0x20`, `+0x30`). On the
ROM: 754 entries, all with zero offset except `F07` (0.018, −0.037, 0.02).

The other types of the table are not part of `BlockList`: `0x66`/`0x67` boxes,
`0x68`/`0x69` and `0x72` regions, `0x6a`/`0x6b` typed elements, `0x6e` **party start
point** (`fx32` position at `BlockList+0x70`, angle at `+0x7c`; `BlockList::start`, read by
`Party_Place`, `save.md` § "Position on resume"), `0x70` BGR555 color (`+0x7e`),
`0x73`/`0x74` typed regions and their
action (doors, see below), `0x7b`/`0x7c` rotated rectangles, `0x7d`/`0x7e` grid of the
grotto `.bpos`. Remaining shapes [unknown].

### Doors, zones and map changes

Records `0x72`, `0x73` and `0x74` do not serve `BlockList` but the same structure
(`Zone3D+0x6c`, 0x88 bytes, cleared by `BlockList_Clear`): the `.bmbl` also stores there the
regions the field tests every frame. Implementation: `dq9_formats::map::Regions`
(reading), `dq9_core::field` (tests and effects); `cargo dq9re doors <map>` prints the
records, `--state` compares them to RAM. Names: `re/symbols.txt`, section "Map
regions".

| offset | content | writer |
|---|---|---|
| `+0x18` (count `+0x1c`, capacity `+0x20`) | **warps** `0x72`, 0x70 bytes, in file order | `MapRegion_RecWarp` (`0x0201ce90`), `MapRegion_AddWarp` (`0x0201e584`) |
| `+0x24` (count `+0x28`, capacity `+0x2c`) | **zones** `0x73`, 0x74 bytes, in file order | `MapRegion_RecZone` (`0x0201d540`), `MapRegion_AddZone` (`0x0201e720`) |
| `+0x3c + 4 × type` | head of the zone list of a type, **most recent first** (next at `+0x70`) | `MapRegion_AddZone` |

A zone is prepended to its list: the game therefore walks the zones of a type from the last
defined to the first, and the first that matches wins (`Regions::zones_of`).

**Warp `0x72`** (25 values, 978 on the ROM of which 976 name a map and 975 a map in the list). Floats are
converted to `fx32` by `ToVec3fix` (× 4096, truncated):

| values | meaning | RAM |
|---|---|---|
| 0–2 | center | `+0x58` |
| 3–5 | **full size** (width, height, depth) | box `+4` (top corner) and `+0x10` (bottom corner) = center ± size / 2; `+0x54` = (width/2)² + (depth/2)² |
| 6 | box angle, radians, **not reduced** | `+0x52` (16 bits) |
| 7 | type: 0 (935); 1 (43) queues task 8 of the field queue if `0x02039820` allows it **[hypothesis: animated door]** | `+0` |
| 8 | destination: map list code (or integer = id) | `+0x1c` |
| 9, 10 | −1, 0 or 1 **[unknown]** | `+0x64`, `+0x68` |
| 11–13 | arrival position in the destination map | `+0x20` |
| 14 | arrival orientation, radians | `+0x50` (16 bits) |
| 15 | entrance (−1 for 786 of 978, 0 to 4) **[unknown]** | `+0x6c` |
| 16–24 | three vectors, copies of the arrival on almost every door **[unknown]** | `+0x2c`, `+0x38`, `+0x44` |

**Zone `0x73`** (9 values): type, center, full size, box angle (radians,
**reduced** to `[0, 2π]`), orientation (radians, not reduced). ROM types: 2 (670, "doorway"
with or without destination), 3 (22, map area, 15 maps), 0, 1, 4, 5, 6, 8, 9, 10, 11, 12
(not decoded here).

| offset | value |
|---|---|
| `+4` | type |
| `+8` | center |
| `+0x14` | full size; `+0x24` = (width/2)² + (depth/2)² |
| `+0x22` | box angle (reduced) |
| `+0x20` | orientation: the direction the doorway **faces**, outward |
| `+0x28` | runtime bits (see `Zone_Holds`), zero on load |

**Action `0x74`** (`MapRegion_RecAction`, `0x0201d648`): completes **the last zone read**
(global pointer, even after other records) according to its type:

- type 3: value 0 is the area number (`+0x2c`);
- type 2: value 0 class (`+0x2c`), 1 number (`+0x2d`), 2 flags (12 bits, `+0x2e >> 4`),
  3 low nibble of `+0x2e` **[unknown]**; with flag `0x8`, 4 destination (`+0x30`),
  5 and 6 (−1) **[unknown]**, 7–9 arrival position (`+0x38`), 10 orientation (`+0x68`), 11
  (`+0x6a`), 12–20 three vectors **[unknown]**, and with `0x20` a float (`+0x6c`). On the ROM
  429 type-2 doorways have a destination.
- Doorway flags present in the data: `0x2`, `0x8` (destination), `0x10` (side test, see
  below), `0x20`, `0x40` and their sums (0, 2, 8, 10, 16, 18, 24, 26, 32, 40, 42,
  64, 66, 72, 74, 80, 82, 96, 104; two short records carry an aberrant value and
  are read without destination). Bits `0x1` and `0x4` never appear in the data: they are
  runtime bits (`0x4` = "used", set by action `0x6c`, cleared by `0x88`,
  observed in RAM below); `0x100` is set by action `0x99`.

#### Presence tests

All positions are `fx32`; the leader's is `Object3D+0x44` of the object of
`GameState+0x397c` (`GetUnknownGameObject`).

| function | address | test |
|---|---|---|
| `Region_InBox` | `0x02031128` | box `[top x y z, bottom x y z]`, edges **inclusive**, **height tested** |
| `Region_InRotatedBox` | `0x020321f0` | without angle: `Region_InBox`. With a non-zero angle: rejects if the ground distance to the center exceeds `r²`; otherwise rotates the point around the center by `-angle` |
| `Region_InCylinder` | `0x02032158` | `y` in `[cy, cy + h]` and ground distance ≤ width |
| `Zone_Holds` | `0x02094bac` | bit 8 of `+0x28`: never; **depth < 1: cylinder** (radius = width, height = height, from the center's `y`); otherwise box rotated by angle `+0x22`; "once only" bits `1`/`2`/`4` |

The rotation is that of `RotationMatrixY(fix32ReduceAngle0To2Pi(-angle))` applied by
`MTX_MultVec43`, whose matrices are read from the ROM (`MTX_RotY43_`: `[c 0 −s; 0 1 0; s 0
c]`, row vector): **`x' = x cos a − z sin a`, `z' = x sin a + z cos a`**. This is the inverse of
the model in the first version of the port (`x cos + z sin`), which was a hypothesis; height
is tested (it used to be ignored) and the test is on **presence** every frame, not on an
outside → inside crossing.

#### What the field does with each region

`ov_017::0219755c` (`Field_CheckRegions`), every frame, in this order for the types read here:

1. **Type-2 doorways** (`Field_CheckDoorways`, `ov_017::02197a64`). A doorway counts if its
   bits `0x1` and `0x4` are clear, it holds the leader (`Zone_Holds`) and the leader walks or
   looks **against** its orientation: the dot product of `(sin f, cos f)` with the requested
   walk direction (or, when stopped, the facing direction `+0x54`) is ≤ 0; with flag `0x10`
   it is the side the leader stands on relative to the center that decides. At most two
   doorways. A counter (`+0x42ec`) goes up by one per frame where the walk is straight
   (absolute dot product above `0xccc − 0x170`, about 45°) and drops to 0 otherwise; **at 3,
   the next frame triggers** `Field_TouchDoorways` (`ov_017::02198f84`): event **17** (word
   7 = class | number << 16, word 10 = orientation), then, without bit `0x4`, the doorway
   leaves for its destination with its own arrival, unless a trigger has set a transport
   (`Save_Warp`), which replaces it.
2. **Map areas** (type 3, `Area_CheckLeader`, `ov_017::0219814c`): the first area in the
   list that holds the leader; changing area triggers event 5 (area left) then
   2 (area reached), word 1 = number (`trigger.md`, "Zones").
3. **Areas added by triggers** (action `0x8f`, `ov_017::02198e30`): same rule.
4. **Warps `0x72`** (`Field_CheckWarps`, `ov_017::0219ca88`): the leader inside the box
   (`Region_InRotatedBox`) copies the warp data into `Save_Warp` (`Field_WarpFromRegion`,
   `ov_017::0219c6f4`: map, position, orientation, entrance), queues the door task
   if the type is 1, applies the transport and triggers event **6** (word 3 = the map
   left). The predicate type 1 tests on the leader (`0x02039820`) and the door task
   (animation, automatic walk, fade) are not read.

#### Type-1 zones and the Observatory height passage

`0x73` zones of **type 1** (43 on the ROM, three on `X01M01`) are read by no routine of
`Field_CheckRegions`: `ov_017::0219755c` calls `0x02197e2c`, `021975e4` (type 9),
`02197a64` (type-2 doorways), `Area_CheckLeader` (type 3), then `021982ac` (type 4),
`021983e0` (type 5), `021984f4` (type 8), `02198618` (type 10), `021986fc` and `02198aa0`
(grotto). None of these calls takes the type-1 list (`func_0201e838(zone3d+0x6c, 1)`).
`MapRegion_RecAction` (`0x0201d648`) does handle value 0 of a `0x74` action after a type-1
zone (`0x0201d6ec`): it writes class 1 into the low nibble of `+0x2c` and a halfword into
`+0x2d`, but nothing reads these fields back per frame. **Type-1 zones are therefore inert at
runtime**; they are neither a lift nor the assumed height transition (field task 8 comes
from the **warp type** `0x72`, value 7 = 1, not from a zone). `X01M01` contains no type-1
warp anyway.

The passage to `X01M03` is warp `0x72` **#9** of `X01M01`: 4 × 4 × 3 box centered at
`(-8.226; 15.5; 16.379)`, angle `5.759587`, **type 0**, destination `X01M03`, arrival
`(-9.75; 4.2; 17.62)` angle `-2.303835`. The arrival is the start point of `X01M03`
(`-10.810; 4.0; 16.910`, `X01M03` has its own y origin): it is a normal landing,
not an inconsistent destination.

The upper terrace is a **separate component** of the collision. From story pose 1.1.2
(`(2.77; 11.25; 18.19)`) as from the map start point (`(0; 8.25; 30)`), an exhaustive walk
with `Hero::step` (edges validated in 3D, climb ≤ 1.0, fall ≤ 10) tops out at **y 11.25**:
the thin wall `X01A01S1` (block 0, placement 12; two triangles at `x 0.018..0.105`,
`z 15.42..18.99`, `y 9..14`, normal `+x`) closes the passage to the ramp, and the ramp itself
forms a component whose foot (`(-1.25; 11.66; 17.5)`) is on the other side. A path found by
an edge that compares only `x`/`z` is **wrong** (edge `(-4.5; 11.25; 18) → (-4; 12.42; 18)`
passes by ignoring y): the `edge_pass` test of `dq9-app` compares
`flat(hero.position() - b)` and accepts these edges.

The warp is therefore **not reachable on foot** from the start point nor from 1.1.2 with the
collision as loaded: a connection (or a collision move) that the port does not model is
missing. Type-1 zones do not provide it (no reader, above) and the warp is type 0 (no task
8). **[unknown]**: the mechanism that brings the party onto the terrace (candidates:
movement of a collision placement by native `575`, animated platform, or scripted map
change). To be settled on the original game (melonDS), not by assumption.

#### Arrival

The destination map loads, the hero is placed at the arrival position and turned to the
arrival angle. **Angle convention**: an object's orientation is an `fx32` in radians and its
direction is `(sin a, 0, cos a)` (0 faces +z). RAM confirms it: the hero walking down (+z)
has angle 0, the one going left 19,544 = 4.77 rad (3π/2 ≈ 4.71), a negative arrival angle is
reduced to `[0, 2π]` (−2,645 becomes 23,091). The application uses
`yaw = π − angle` (`field::yaw_of_angle`); the arrival camera follows the hero, this is not
verified in the game.

`dq9-app` calls `Field::step` at every fixed step with the hero pose and does what the
events return: map change (loading, arrival pose, events 20 then 3 of the map), trigger
requests logged as unsupported until J3. The hero speed remains that of the movement port.

#### Verification

- **Data** (`maps_assemble`, `dq9-formats/tests/real_rom.rs`): 742 distinct archive sets
  expose 975 warps with a known destination, 429 type-2 doorways with a destination and 22
  type-3 areas (wiki figures).
- **RAM**: `cargo dq9re doors --state build/melonds/walk_1.5.ml1` (and `demo`, `before`,
  `moved`, all in `C01M09`): the warp read (type, top/bottom box, center, destination,
  arrival, angles, `r²`) is **identical** to the 0x70 bytes of `g_Zone3D+0x6c+0x18`. In
  `C01` (state `walk_1.95`) the 15 zones read are identical to the 0x74 bytes in RAM, except
  the flags of the two doorways of `C01M14` (numbers 22 and 24): RAM has them at 12 and 14,
  the reader at 8 and 10. That is the "used" bit set by event 20 of `triggerC01` (action
  `0x6c`, condition "night", the save is at night); `dq9_core::field` reproduces it
  (`stornway_at_night_marks_its_two_locked_doorways`).
- **Arrivals** (`dq9-core/tests/field_rom.rs`): of 1,426 arrivals (warps and doorways), 4
  land in a warp of the destination map with the test read from the code, 25 with the
  inverse rotation: the rotation direction is settled. Of 100 arrivals that land in a
  type-2 doorway of the destination, 96 face out of the doorway (dot product with its
  orientation > 0), as the rule "walk or look against the orientation" requires.
  The size is indeed the full size: with half-extents the Angel Falls arrivals
  would land in the return doorway.
- **melonDS**: see the next section.

#### Comparison with melonDS

Demo save (`build/melonds/dq9.sav`), scenario 19.2.1, night, Stornway church
(`C01M09`, 109). `scripts/melonds-state.ps1 -Keys` drives the game with held keys
(real-time durations, **± 2 frames**: these are not frame-aligned measurements,
`movement.md`); `.ml1` states reread by `cargo dq9re doors --state`. The priest's greeting
dialogue must be closed first (A, A, then B for the menu).

| state | keys | map | leader position | angle |
|---|---|---|---|---|
| start | — | 109 | (0.210; 0.120; −1.380) | 12,861 (π) |
| `walk_1.5` | down 1.5 s | 109 | (0.339; 0.106; 7.277) | 0 |
| `walk_1.65` | down 1.65 s | 109 | (0.339; 0.106; 7.853) | 0 |
| `walk_1.7` | down 1.7 s | 109 | (0.339; 0.106; 8.237) | 0 |
| `walk_1.75` | down 1.75 s | **100** (`C01`) | **(14.850; 2.458; −10.050)** | 23,091 (5.637 rad) |
| `walk_1.95` | down 1.95 s | 100 | identical | identical |
| `door_0.25` | then up + right 0.25 s | 100 | (15.321; 2.458; −10.561), inside zones 10 and 11 | 10,431 |
| `door_2.0` | then up + right 2 s | **109** | **(0.200; 0.100; 7.900)** | 12,869 (π) |

- **Leaving the church** (type-0 `0x72` warp, box centered at z 9.268 with depth 1.5,
  so from 8.518 to 10.018): at z = 8.237 the map has not changed, 0.05 s later it has.
  The predicted edge (8.518) is within the interval; the half-extent reading (7.77) is ruled out.
  The arrival is exactly the record's (14.85; −10.05), angle −2,645 reduced
  to 23,091, and `Save_Warp` (`GameState+0x3f8`) holds map 100, position (14.85; 2.22;
  −10.05), angle −2,645, bytes `+7` and `+0xc` at 1: exactly `Field_WarpFromRegion`.
  Height goes from 2.22 to 2.458: the game puts the hero on the ground (2.358 + 0.1); the
  port finds 2.459.
- **Return through the type-2 doorway** (volume centered at (15.475; 2.5; −11.104), 1.4 × 3 ×
  1.5, rotated by 5.72 rad): 0.25 s after arrival, the hero is at (15.321; −10.561), **inside
  both zones** according to `dq9_core::field` (the map has not changed yet: the door task
  plays its animation), and 2 s later the map is 109 at the doorway's exact arrival
  (0.2; 0.1; 7.9), orientation π. The same route simulated by `dq9-app --walk`:
  `cargo run -p dq9-app --release -- --map C01 --start=14.85,-10.05 --walk "+x-z:1.5"` changes
  map at (15.369; −10.569) to 109 at (0.200; 0.100; 7.900), `Doorway { number: 20 }`.
- Out of scope: the exact trigger instant (states are taken after the gesture), hero speed
  (3.8 to 5.8 units/s depending on the stretch, 4 in the port), door task duration,
  arrival camera. None of these states is a frame-by-frame comparison: the J2
  differential criterion is not validated.

Native Angel Falls smoke (`dq9-app`, windowless):

```text
> cargo run -p dq9-app --release -- --map M01 --start=-19.5,-9 --walk "-z:3,+z:3,idle:0.5"
start: map 1100 M01 hero (-19.500, -0.800, -9.000) yaw 3.142
map change 1100 -> 1102: Doorway { class: 0, number: 9 } with the hero at (-19.500, -0.150, -12.267), arrival Some(Vec3(0.39990234, 0.15991211, 6.0998535)) angle Some(-3.1418457)
arrived: map 1102 M01M02 hero (0.400, 0.160, 6.100) yaw 6.283
map change 1102 -> 1100: Warp { index: 0, kind: 0 } with the hero at (0.400, 0.158, 7.300), arrival Some(Vec3(-19.569824, -0.099853516, -11.799805)) angle Some(0.0)
arrived: map 1100 M01 hero (-19.570, -0.100, -11.800) yaw 3.142
```

The `M01M02` doorway (type-2 zone, z from −14 to −12) opens on the fourth frame spent
inside (z −12.27), the exit (type-0 warp, z from 7.25 to 8.75) when the hero enters it
(z 7.30).

(`angel_falls_houses_open_and_close` in `dq9-core/tests/field_rom.rs` replays this route.)

#### Limits and unknowns

- Door task (animation, walk and fade), type-1 predicate `0x02039820`, guards of
  `Field_CheckRegions` (`+0x424`, modal task), leader states `+0xbe` ("walking", here: a
  direction is requested) and held keys (here: the requested direction).
- Zone types 0, 1, 4 to 12 (`Field_CheckRegions` reads several: `021982ac`, `021983e0`,
  `021984f4`, `02198618`, `021986fc`, `02198aa0`), warp values 9, 10, 15 and 16–24,
  doorway values 5, 6, 11 and 12–20, low nibble of `+0x2e`, transport entrance.
- The "once only" bits of `+0x28` (what sets them is unknown: no record);
  actions `0x6d`, `0x95`, `0x96`… that touch door objects are not modeled.

## Lighting (`ats_<c>.ambl`)

`Zone3D_QueueLighting` (`0x02014b14`): on a grotto floor (`Zone3D+0x23b8`), the name
becomes `Z0<e>M0100`, `…01`, `…02`, `…03` depending on the floor (`id % 20`: < 5, < 9, < 13, < 17;
beyond, unchanged). Empty name: move on to the `.amdj`. Otherwise `data/map/ats_<1st character>.ambl`,
where `Zone3D_StepLighting` looks up `<name>.bats` (`GetFileInNarc`, `0x02075258`: `strcmp`, hence
case-sensitive). The `.bats` format (LZ10, record table, opcodes `0x64`..`0x6a`) is described in `lighting.md`.

On the ROM, 663 archive sets load lighting; two names are missing (`D07M0800.bats`,
`B10M0200.bats`): the map loads without it.

## `.amdj`: blocks

`Zone3D_StepAmdj` mounts the archive under the name `ARC`, then for each `.bmdj` member and
each block list entry **whose name is contained in the member's**
(`strstr`) loads a block (`Zone3D_LoadBlock`, `0x02014910`): 0x58-byte block (`+0` entry
id, `+4` manifest, `+0x48` entry offset, `+0x54` next), **prepended** to
`Zone3D+0x41c`. A `.bmdj` containing two names is loaded twice; a `.bmdj`
no name designates is ignored (only case: `M12M0001.bmdj`).

It then builds the blocks from the list head (`Zone3D_BuildBlock`, `0x02014a34`):
all of them, except for the second half of ocean/sky (counter 2) where only the first is.
Finally, if `Zone3D+0x16` is not empty, it queues the `.mse`.

### Manifest (`.bmdj`, `Manifest`)

Record table read by `Manifest_Parse` (`0x0201f050`) with `g_BmdjHandlers`
(`0x020ef418`) into a 0x3c-byte structure:

| type | handler | values | effect |
|---|---|---|---|
| `0x6a` | `0x0201ebd4` | n | array of n 12-byte resources (`+0`, capacity `+8`) |
| `0x6c` | `0x0201ec10` | id; name; [flags]; [textures] | adds a resource if room; becomes the current resource (first of this id) |
| `0x72` | `0x0201ece4` | time; name; [flags]; [textures] | variant chained to the current resource (none on the ROM) |
| `0x6d` | `0x0201edd0` | n | array of n 32-byte placements |
| `0x6f` | `0x0201ee00` | 7, 8–10, 11–13 or 14 values | adds a placement if room |
| `0x70` | `0x0201f000` | name | texture link, 8 slots (`+0x18`), no overflow check |
| `0x6b`, `0x6e` | nothing | | |
| `0x71` | no handler | | ignored (746 files, one float) |

**Resource** (12 bytes): `+0` id (i16), `+2` flags (value 2 & `0x3f`), `+3` texture links
(value 3: bit i = link i), `+4` source name (`C01M0900.imd`), `+8` variants.
Flags: `0x01` nsbca, `0x02` nsbma, `0x04` nsbta, `0x08` nsbtp, `0x10` animated object
(`Object3D`, `.bcfg`), `0x20` no tint (`LightingManager::ModelTransformTintBrightnessContrast`
is only applied to pieces without this bit, when `Save_Warp(...)+0x20 < 0`).

**Placement** (32 bytes):

| bytes | value | meaning |
|---|---|---|
| `+0` u16 | 0 | instance number |
| `+2` i16 | 1 | resource id; negative: the slot numbered by the instance |
| `+4` i16 | 5 | parent instance, negative: none |
| `+6` u16 | 13 (else `0xf`) | visibility, bits 4 and 5 inverted (below) |
| `+8` 3 × `fx32` | 2, 3, 4 | translation (× 4096, truncated) |
| `+0x14` 3 × 16 bits | 10–12 (7–9 if 8 to 10 values) | rotation, `fx32` radians kept on 16 bits |
| `+0x1a` 3 × 16 bits | 7–9 if more than 10 values, else 1.0 | `fx32` scale on 16 bits |
| — | 6 | string read then discarded (the parent's name in the data) |

Visibility (stored flags): bit 4 → always hidden; otherwise visible at time of
day `t` (`LightingManager+0x98`, 0 to 3) if bit `t` is set. File values therefore have
bit 4 set for "time-dependent": 63 → `0xf` everywhere, 49 → `0x1` only at time 0,
62 → `0xe` at times 1–3. **[hypothesis, wiki Map-Archive]** 0 = night: `…N1` pieces are
visible at time 0, `…L1` at the others. An overlay object
(`func_0x0218b5b0()+0x3718`) can replace the time used [unknown].

### Parts (`Part`, 0x18 bytes per resource)

`Zone3D_BuildBlock` builds one part per resource slot (`capacity` of type
`0x6a`), then the instances. A file is looked up in the mounted archive by
`GetFileFromNARCInMemory("ARC:/<name>")` (`0x020afd1c`), the name being the resource's
with the text after the last `.` replaced (`Zone3D_ArcPath`, `0x02014d28`); the
lookup goes through the SDK file system, hence case-insensitive (`m08M0600.imd` →
`M08M0600.nsbmd`, `D13M0203.imd` likewise).

| resource | kind (`+0`) | files |
|---|---|---|
| 4th character of the name `A` | 1 collision (`0x02015144`) | `.col2`; kind 1 even if missing |
| flag `0x10` | 2 object (`0x020151dc`) | `.nsbmd`, then nsbca/nsbma/nsbta/nsbtp/`.bcfg` per flags (`g_ObjectAnimFiles`, `0x020e6f58`) |
| other | 0 model (`0x02014d90`) | `.nsbmd`, then nsbca/nsbma/nsbta/nsbtp per flags (`g_ModelAnimFiles`, `0x020e6fb8`) |

- Model: first the variant whose time equals `LightingManager+0x98`, otherwise the
  resource. Without `.nsbmd`, kind `0xffff`. All textures of `Zone3D+0x418` are applied
  to it. Animation texture source: first link bit (`+3`) whose link name is contained in
  the name of a `.nsbtx` in the list. Without nsbca, the game creates the model's bone and
  material matrices.
- Animations are loaded **only if their bit is set and the file exists**:
  36 bits designate a missing file (ignored), and present files are not
  loaded for lack of a bit (`C01M0900.nsbtp`).
- Object: each instance showing it receives its own copy (`Object3D::ShallowCloneTo`).

### Instances (`Instance`, 0x70 bytes per placement)
1. instance i: number = placement.instance (`Zone3D_NodeInit`, `0x02013028`);
2. parent: negative placement.parent → sibling of instance 0 (except instance 0 itself,
   `0x020130f0`); otherwise first instance with that number (`0x020130c0`); a missing number
   leaves the instance outside the tree;
3. hidden according to the visibility above;
4. part: negative resource → slot `instance`; otherwise first resource with that id
   (`0x0201f1c0`); a kind 0/1/2 part without a file is not linked;
5. transforms (`Zone3D_PlaceNodes`, `0x0201311c`).
here [unknown: use elsewhere].

### Transforms

`Zone3D_PlaceNodeChain` (`0x02013168`) starts from instance 0 with zero position, angle 0
and scale 1, computes the instance, its siblings (chain `+0x30`) then descends into the
children (`+0x2c`) with the parent's transform:

- position = `MTX_MultVec33(translation, MTX_RotY33(sin, cos of the parent's angle))` +
  parent position (rotation around y only, translation not scaled);
- angle = `fix32ReduceAngle0To2Pi(parent angle + y rotation)` (`0x02030f40`);
- scale = scale × parent scale (`fx32`, rounded).

`fix32sin`/`fix32cos` (`0x02030c78`, `0x02030cac`) read the SDK table
(`g_fixSinCosTable`, `0x020e9460`) at index `((x << 16) / 2π) >> 4`; the table is
exactly `round(4096 · sin(2πi/4096))` (ROM test), so it can be computed.
`FX_Div`, used by angle reduction, is taken from the SDK (rounding `(q + 2¹⁹) >> 20`).
On the ROM, all rotations and scales are 0 and 1: only translations and
parents matter in practice. An unreached instance keeps a zero position; a tree
that loops would make the game loop (error here).

## Verification

- **ROM** (`maps_assemble`, `crates/dq9-formats/tests/real_rom.rs`): the 871 shipped maps
  (id ≠ 0) assemble; 796 have their `.ambl`. Per distinct archive set (742, including the 5
  grotto environments): 667 `.ambl` (all except the 14 `ats_*`) and 669 `.amdj` loaded,
  754 blocks, 5,341 resources, 5,651 placements, 529 texture links; 4,096 models,
  112 objects, 1,133 collisions, **all resolved within their archive**; 2,125 animations,
  845 texture sources; every placement leads to a part; 1,288 instances have a
  parent, none is outside the tree; 713 hidden at time 0; 15 `.mse`.
- **RAM**: on `build/melonds/demo.ml1` (map 109 `C01M09`, time 0 read at
  `0x02107930+0x98`), `cargo dq9re map --state` walks `g_Zone3D+0x41c` and compares each
  block: entry id, resources (fields and names), texture links, 32 bytes of each
  placement, kind and animation slots of each part, number, hidden, angle,
  position, scale, part and parent of each instance: **identical**. With `--time 1`
  the comparison reports the two instances whose visibility changes.
- The SDK sine table is compared entry by entry to the computed one (ROM test).

## Viewing the collision (`dq9re map --image`, `--reach`)

Two options of `cargo dq9re map` make the geometry visible, without models or textures:
`--image` writes a top-down orthographic view of the assembled `.col2` (same archives and
placements as `CollisionWorld::build`), `--reach` fills a walk grid and colors
the reached cells on `--reach-image`.

```powershell
cargo dq9re map X01M07 --image build/scratch/x01m07.png --scale 16 --at 0 6.8
cargo dq9re map X01M07 --reach 0 0.17 6.8 --to 0 0.17 17.5 --reach-image build/scratch/x01m07-reach.png
cargo dq9re map X01M01 --reach 2.77 11.25 18.19 --to 2.77 15.5 18.19 --reach-image build/scratch/x01m01-reach.png
```

`X,Y,Z` and `X,Z` are in world units (not `fx32`), space-separated; without `Y`,
the point is searched under the highest surface of the column. The start point of
`--at` and `--reach` is **placed on the ground** by `CollisionWorld::ground` (`0x02018fcc`); a
point with no ground within 1 unit above or 10 below is rejected with the error
`not on the floor`. A map without collision geometry fails the same way.

### Legend

| Color | Meaning |
|---|---|
| blue → green → yellow → red gradient | floor, from minimum to maximum height; the bar at top left gives both bounds |
| very dark gray | walls and ceilings (`normal y <= 0x800`); vertical faces are drawn as lines since their projection is a line |
| magenta | `0x72` warp volume, with `>CODE` when the map list resolves its destination |
| orange | type-2 `0x73`/`0x74` doorway zone, destination if flag `0x8` gives it |
| cyan | map area (type 3) |
| gray | other zone types |
| white square | map start point (`.bmbl` type `0x6e`), placed on the ground |
| red square | `--at` hero |
| red cross | `--to` target |
| cyan tint | reachable cells of `--reach-image` (masked under a higher floor) |

The view shows the **highest surface** of each pixel: it does not overlay floors,
it hides them. It is a development view: no textures, models, animation,
lighting or game camera; doors are drawn at their load position (closed).

### `--reach`: real reach, in 3D

The grid is **0.5 unit** and anchored at the start point. Each edge is walked by
a real `Hero::step` (fixed step, collision, sliding) and is accepted only if the hero
stops within **0.3** of the cell in 3D, with a climb of at most **1.0** and a
fall of at most **10.0**. Nodes are indexed by `x/z` cell **and by height** (0.5
bucket): two stacked floors remain two components, a plain `x/z` BFS would merge them.

The command prints the node count and the heights reached:

```text
reach from (2.770, 11.250, 18.190) stood on the floor: 9320 nodes, y 4.249..11.251 (step 0.50, 3D arrival < 0.30, climb <= 1.0, fall <= 10.0)
target (2.770, 15.500, 18.190): not reached
```

### Verified examples

- **`X01M07`**, Observatory library (`--scale 16`): 512×640 px, floor heights
  −0.93 to 2.27. The room, the orange double door at `z = 18.1` and the magenta warp
  extending it are visible. From the `X01M05` arrival at `(0; 0.17; 6.8)`, 2,224 nodes from
  −0.57 to 2.37; `--to 0 0.17 17.5` answers `reached` (in front of the two leaves), while
  `--to 0 0.17 18.1` (center of the closed leaves) answers `not reached`: the collision of the
  closed doors blocks; opening them is the job of the cutscene event (native 584).
- **`X01M01`**, Observatory tower: 736×736 px image (scale 8), heights 0.15 to
  23.15. From `(2.77; 11.25; 18.19)`, 9,320 nodes between 4.249 and 11.251; the high target
  `(2.77; 15.5; 18.19)` is **not** reached. These figures encode no route or
  conclusion: the tool recomputes them on every run.

### Limits

- No player camera: each edge walk starts with the camera at angle 0, heading steered toward
  the cell, and at speed 0 (the real hero would chain without stopping). The result is a
  conservative lower bound, not a frame-by-frame measurement.
- No events, animated doors, ladders, pushing characters, or magnet of
  oriented zones; these interactions can open or close a passage.
- Floors of the same pixel are merged if they fall into the same 0.5 bucket; descending
  more than 10 units or climbing more than 1 unit cuts the component.
- The view does not replace `melonDS`: it shows our collision, not the game's rendering.

## Discrepancies with the DQIX wiki

- Map-Textures: the `.bmbl` "names the map textures" — the game does not read its
  type `0x6c` and loads every `.nsbtx` of the `.ambl`.
- Map-Objects: "54 `.bmdj` start with word 16" — all members are LZ10 streams
  of non-zero size, which the game always decompresses; value 6 (counted from 1) does name
  an **instance**; values 11–13 are the rotation (read, only y is used), 14 the
  visibility per time of day; 7 is read then discarded.
- The resources' "unknown word" is the texture link mask (type `0x70`).

## Open questions

- Meaning of `Zone3D+0x26`, `+0x36`, `+0x3c` (`.dat`) and of names 2 and 3 of block entries.
- Use of the block offset (`Block+0x48`) elsewhere than in `0x020177e4`.
- The overlay object that can replace the time of day for visibility.
- Characters (`.chr`, NPCs) are not part of this chain: loaded by other
  steps (`0x02018f30`, `0x020181fc`, `0x02018ab8`, `0x02017a94` after loading) [not read].
