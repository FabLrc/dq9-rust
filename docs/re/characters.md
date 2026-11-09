# Characters: a map's cast, `.chr` archives and `.bcfg` tables

Implementation: `crates/dq9-formats/src/cast.rs` (`load`, `run_placements`, `Story`,
`Placement`, `Npc::appearance`, `initial_motion`, `sprite_stand`) and
`crates/dq9-formats/src/chr.rs` (`Chr`, `MotionTable`, `role`, `Motion::advance`); the hero
(parts, assembly, gaits): § "The hero". Reading from
RAM: `melonds::State::npcs` and `State::story`. Tools:

```powershell
cargo dq9re cast M01                                   # prologue (1.1.1, day)
cargo dq9re cast M01 --chapter 2 --night --quest 4=3   # another point in the story
cargo dq9re cast C01M09 --state build/melonds/demo.ml1 # compared with the NPC table in RAM
cargo dq9re chr data/chara_sub/s017.chr                # member roles, motions
```

Sources: ARM9 code and `ov_017` (European addresses below), USA decompilation
(`src/Resource/BCFG.cpp`, `src/World/Object3D.cpp`, Unlicense), DQIX wiki (pages Area-Cast,
Motion-Tables, Sprites, Character-Parts: hypotheses rechecked in the code). The
`place.bin` records are called "tags" there; here "type n records"
(`table.md`).

## Loading chain

After the map is assembled (`map-archive.md`), `ov_017::021a2c14` (`Cast_LoadMap`):

1. opens `data/scenario/%s.npc` (NARC), `%s` = first three characters of the map code,
   or `F` alone for `F*` maps (`cast::archive_path`);
2. runs each member whose name contains `place.bin` with `Place_Run` (`0x0206da90`):
   (chapter, stage, sub-stage) of the current track (`Save_Chapter`…), map
   id; each run **starts from an empty list** (only the last one counts; there is only
   one per archive);
3. runs each `npc.bin` member with `Cast_RunNpcBin` (`0x02064e3c`); on maps whose
   name contains `R0` except `R05`, `0x02064e98` adds visitors drawn from the save's zone
   block (**not modelled**);
4. builds the NPC table (`NpcTable_Build`, `0x0203d038`), then applies the snapshot
   kept for this map (`0x0203e2d4`, § "The NPC snapshot");
5. puts on the ground each NPC with neither flag `0x800` nor `0x1000`: `Zone3D_GroundHeight`
   (`0x02018fcc`) casts a ray from `y + 1` to `y − 10` in a ±0.5 square and returns the height
   hit + 0.1 (`0x199`). With no map collision here, `cast` returns the file height.

The NPC table (`g_NpcTable`, `0x021075f4`, returned by `0x0203cf5c`): `+0xc` 32 entry
pointers, `+0x8c` count, `+0x90` cast list, `+0x94` placement list.

## The NPC snapshot (`0x0203e0b0`, `0x0203e2d4`)

**Found** by decompiling (Ghidra, EU addresses) the `Cast_LoadMap` call at `0x0203e2d4`
(`ov_017::021a2c14+672`, argument: the current map number) and its writer
`0x0203e0b0`, called by `ov_001::02154f24` (`+184`, `+276`), `ov_017::021a1284`,
`ov_017::021c0850`, `ov_017::021b1564` and `ov_011::02184cc0` (`xrefs`).

NPC table fields: `+0x1de` snapshot map (0: none), `+0x1e0` entry
count, `+0x1e4` mask by entry rank, `+0x9e` 10-byte entries (5 `s16`):

| Word | Written by `0x0203e0b0` | Read back by `0x0203e2d4` |
|---|---|---|
| 0–2 | position `fx32 >> 4` (sprite `+4`, else object `+0x44`) | `<< 4`, copied back to the same place |
| 3 | angle (`0x0203cdd0` of the sprite, else object `+0x54`) | `0x02040b2c`, `0x02040af0`, and `0x02033874` for an object at `+0x1c` |
| 4, bits 0–8 | member number (`0x02040528`) | key: the entry applies to the NPC of the new cast that has this number |
| 4, bit 9 | object flag `0x8000` (cleared by action 0x7c) | set: restores `0x8000` |
| 4, bits 10–14 | count `n` of unfinished nodes (`+0xc == 0`) at the head of the movement queue (`0x02040538`, `+0x34`) | the first `n` nodes become unfinished again, the following ones finished |
| 4, bit 15 | movement state `(+10 & 0x3f) >> 3 == 3` | restores this state |

Bit `n` of `+0x1e4` copies flag `0x40000` of the entry of rank `n`; the read-back
sets it on the entry of the **same rank** in the new table. When `+0x1de` differs from the map,
nothing is restored. In both cases `0x0203e2d4` ends with `+0x1de = 0`, `+0x1e4 = 0`:
**every cast load consumes the snapshot**, then, if `GameState +0x7e7e` ≠ −1,
it moves the object of `0x0203df78` to `+0x7e78..+0x7e7c` and resets `+0x7e7e` to −1. The
restored positions then go through ground placement again (step 5).

**Event start** (`ov_001::02154f24`). Field `+0x50` of the event task: value
21 of the type 102 record of the event list (`EventRecord_Build`,
`0x02071218`, 16 bits; bit 3 cleared when `0x0202c540` returns non-zero). If `+0x50 & 1` and
`TestFlags00(0x1000)` is zero: snapshot (`0x0203e0b0`), cast teardown
(`ov_017::021a316c`) and `+0x90` = size of its memory, handed to the event. Otherwise, if
`+0x50 & 4`: snapshot and teardown only. **End** (`EventTask_Update`,
`ov_017::021bc77c`): if `TestFlags00(0x1000)` is zero or no event is chained
(`+0x11a`), `+0x90 ≠ 0` (memory recreated, then `+0x90 = 0`) or `+0x50 & 4` rebuilds the
cast (`ov_017::021a2fa0`) **before** `Trig_Fire(0xb)`. Consequence: the cast
comes back as the event found it (positions, angles, 0x7c removals kept), at the
story point of the event's end; a later rebuild (action 0x8d, task
0x30) finds the snapshot consumed and restarts from `place.bin`.

ROM check (`event_records_say_which_scenes_keep_the_cast_aside`): value 21
is 0x4 for 21500, 21507, 21520–21522, 0x1 for 21510, 21593, 21595, 0x40 or 0x140 for the
prologue cutscenes 1130–1620; 29 records of `eventlist6.bin`, 1 of `evl_quest.bin`
and 118 of `eventlist_lv5.bin` have bit 0 or 2.

**Modelled** (`EventPlayer::keep_cast`, `cast_rebuilt`, `cast_loaded`, `App::reload_cast`):
the snapshot taken at the start of an event with bit 0 or 2 set (after loading
its map), the rebuild at event end before event 11, the restoration of
positions (to 1/256), angles (to 1/4096) and removals on the same map, its
consumption by every cast load (event end, 0x8d, map
load). Test: `the_end_of_21595_rebuilds_the_cast_with_the_poses_and_removals_kept_at_its_start`.

**Not modelled**: the teardown at the start (the map's NPCs remain drawn during the
cutscene); bits 10–15 of word 4 and the `0x40000` mask; the `+0x7e7e` jump; the other
writers (`ov_017::021a1284`, `ov_017::021c0850`, `ov_017::021b1564`,
`ov_011::02184cc0`, role **[unknown]**). **[hypothesis]** `TestFlags00(0x1000)` is zero: this
flag is not modelled, so bit 0 always suffices and the rebuild happens
even before a chained event (native 538). **[hypothesis]** a chained event
reads back its own record (`+0x50`): the application decides on the rebuild based on
the event that ends.

## `<zone>npc.bin`: the cast

Record table; handlers `{1, 2: nothing; 3: Cast_RecMember}`
(`g_CastHandlers`, `0x020f05f8`). Type 3 (`0x02064d58`), `0x14`-byte node:

| Field | Node | Meaning |
|---|---|---|
| 0 | `+0` u16 | NPC id (dialogue files `NNN_<lg>.bin`, placements) |
| 1 | `+2` | kind: 0 sprite, 1 nothing (thing to examine, `dialogue.md`), 2 model, 5 monster; 4 = visitor (set by `0x02064e98`, absent from the data) |
| 2 | — | Japanese name, read then discarded |
| 3 | `+4` (12 bytes, `strcpy`) | resource name (`n003a`, `s017`, `z015d`) |
| 4 | `+3` | **[unknown]** (0, 1 or 2) |

A record is kept only if the NPC has a placement chain (see below), and not
for NPCs 1 to 63 of maps 50101 to 50523 (`0x020981f4`, `0x02098278`: visitors). The
nodes are **prepended**: the NPC table is in reverse file order.

## `<zone>place.bin`: the placement script

Record table where each type is an opcode (`g_PlaceHandlers`, `0x020f0994`,
types 3 to 22). Context `g_PlaceContext` (`0x02108cec`): `+0` sub-stage, `+1` stage,
`+2` chapter, `+4`/`+8` last quests of types 21/14, `+6` last NPC of types 14/21,
`+0xa` **counter** (incremented by types 3, 4, 5, 14, 21, and 17 if it has at least 9
fields), `+0xc` map, `+0x10` list. `Place_Run` resets `+4`, `+6`, `+8` to −1 and the counter to
0 before each run except the very first of the game (zero BSS values: identical
effects, checked case by case).

**Persistent fields.** The table reader (`Tbl_Next`, `0x020307e0`) decodes a
record's fields into an array it never clears: a short record that reads past
its fields reads **those of previous records**. Examples: a five-field conversation
zone takes the label from the previous field 5; a two-field type 8 segment, the
previous length. Reproduced by `table::Fields`; no record of the 508 maps reads
a never-written field (test `casts_resolve_to_existing_archives`).

### Records that place

Conversions: float × 4096 truncated (`fx32`, `Script::Parameter::ToVec3fix`); the angle in
radians × 4096 is kept on 16 bits (`+0x1c`). "Time": 0 day only, 1 night
only, any other value always (`Save_IsDaytime`, `0x020103c4`: save `+0x3dc` ≠ 0).
A progress value is `chapter × 1000 + stage × 10 + sub-stage`.

| Type | Function | Fields | Effect |
|---|---|---|---|
| 3 block | `0x0206c020` | map, NPC, x, y, z, angle, (motion) | if the map is this one: fewer than 3 fields → removes the NPC, else places; another map does **nothing** |
| 4 point | `0x0206c108` | chapter, stage, sub-stage, time, map, NPC, x… | counts if all three equal the progress and the time fits; other map → **removes** the NPC; fewer than 7 fields → removes; else places |
| 5 interval | `0x0206c2d0` | from (3), to (3), time, map, NPC, x… | counts if `from ≤ now ≤ to` and the time fits; other map → removes; fewer than 10 fields → removes; else places |
| 14 quest | `0x0206cbdc` | quest, condition, time, map, NPC, x… | see "Quests"; fewer than 6 fields → removes |
| 17 flags | `0x0206d4f0` | (flag, set)…, time, map, NPC, x…, (motion if even count) | fewer than 9 fields: ignored; each pair: high part 1 → `Flags_Test(n)`, 2 → `Flags_TestExt(n)` (n ≥ `0x400` offset by `0x6fa`), other → ignored; the time is not kept |
| 21 quest and interval | `0x0206cf68` | quest, condition, from (3), to (3), time, map, NPC, x… | quest as 14, then interval and time; fewer than 12 fields → removes |

The optional field after the angle (`+0x1e`) is the model's **initial motion index**
(see below). Removing (`Place_Remove`, `0x0206dd78`) detaches the NPC's whole chain: a
following record can place it again.

### Chains and priorities (`Place_Insert`, `0x0206db58`)

Each NPC has a chain of `0x78`-byte nodes (`Place_InitNode`, `0x0206bf3c`); its head
is where it stands. Node class, lowest at the head:

| Class | Node |
|---|---|
| 0 | type 17 (`+0xa` bit 2) |
| 1 | type 21 (`+0xa` bit 6) |
| 2 | type 14 (`+0x46` ≥ 0) |
| 3 | non-zero start stage and zero end stage: type 4, or "open" interval |
| 4 | other interval |
| 5 | block |

At equal class, classes 1 and 4 put the **earliest start** first (bytes `+4..+6`),
then the oldest record; the others, the oldest record. A node that
gets ahead of no other is **dropped** (it would never be at the head). The node keeps the
counter value (`+0x44`): this is how the following records find it.

### Quests (types 14 and 21)

Condition (field 1) on the quest's nibble (quest manager `+0x2cc`, `0xcc`
quests, `Quest_State` = 2 low bits): 0 in progress (state 2), 1 bit 2, 2 completed (3), 3 offered
(1), 4 bit 3 and state 0, −1 state 0; any other value: always true.

For the same NPC (counter `+6` changes with the NPC and resets `+4`/`+8` to −1), a completed
quest is **ignored** if a completed quest with a higher number has already been handled by a
record of the same type; and any type 14 is ignored after a successful type 21 with quest > 0.
Example: C01M09, NPC 10, quest 4 completed → the type 14 node gets ahead of the block.

### Records that complete the last placement

They look, in the chain of the NPC of field 0, for the node made at the current counter; without it,
nothing.

| Type | Function | Fields | Node |
|---|---|---|---|
| 6 | `0x0206c4f8` | NPC, f1–f4, label | conversation zone added at `+0x40`: max x and z, min x and z (× 4096); `Talk_NearestNpc` (`dialogue.md`) |
| 7 | `0x0206c624` | NPC, max x, max z, min x, min z | zone `+0x24..+0x30`, mode `+0x1f` = 7 **[hypothesis: wander zone]** |
| 8 | `0x0206c704` | NPC, direction, distance | mode 8, `+0x22` = direction, zone = position ± distance·(sin, cos) reordered max/min **[hypothesis: back and forth]** |
| 9, 10 | `0x0206c83c`, `0x0206c948` | NPC, (x, y, z)… | looped path `+0x34`, mode 9 / 10; 9 also writes the point count − 1 in the low byte of `+0x22` |
| 11 | `0x0206ca4c` | NPC, values… | OR into flags `+0xc` (64 and 4096 in the data) |
| 12 | `0x0206cae8` | NPC, NPC | `+0xb` **[unknown: linked NPC]** |
| 13 | `0x0206cb58` | NPC, f | `+0x38` = f × 4096 (`0x7a` by default), given to the object by `0x02040c0c` **[hypothesis: speed]** |
| 15 | `0x0206d3e4` | NPC, angle | `+0x3c`, flag `0x800`: **seated** NPC (see sprites) |
| 16 | `0x0206d474` | NPC, x, y, z | `+0x48`, flag `0x2000`: shared `data/chara_sub/s069.chr` object, at NPC scale **[unknown]** |
| 18 | `0x0206d744` | NPC, n, (f) | `+0xa` bits 3–5 = n, `+0x60` = f × 4096 **[unknown]** |
| 19 | `0x0206d7f4` | NPC, archive, motion | `+0x54` path under `data/`, `+0x58` motion name (see "Initial motion"); a null archive makes the game copy from address 0, a load that then fails (equivalent to "none") |
| 20 | `0x0206d8f8` | NPC, f1, f2, f3 | `+0x5c` = (f2, 0, f3, f1) × 4096 **[unknown]** |
| 22 | `0x0206d9e8` | NPC, r, g, b | `+0x20` = r \| g≪5 \| b≪10, `+0xa` bit 7: colour **added** (saturated) to the model palette (`0x0203db54`) |

## What gets drawn (`NpcTable_Build`, `0x0203d038`)

One `0x20`-byte entry per cast NPC, in list order: `+0` flags
(the placement's are added, `0x02040550`), `+8` cast node, `+0xc` placement
node, `+0x10` `s069` object, `+0x14` sprite, `+0x18` model, `+0x1c` game object.

| Kind | Body |
|---|---|
| 2 | model `data/chara_sub/<name>.chr`, `Object3D::LoadFromCCHROrCMOTArchive` (`0x02036814`) |
| 5 | monster: member `<name>.mon` of `data/pack_lv5/enemy.gp2` (NARC), its first `.cchr` member (LZ10) loaded as a model |
| 4 | visitor (character from another player's party) |
| other | sprite object; sheet loaded only if the kind is 0 and the name non-empty: `<name>_s.spr` if the placement has flag `0x800` and `data/ani/<name>_s.spr` exists, else `<name>.spr`; the loader prepends `data/ani/` (`0x0203fcf0`) |

Then, with the placement: position (`0x02040784`: sprite `+4`, `Object3D +0x44`), rotation
`(0, angle, 0)` (`0x0204086c`: sprite `+0x14`, `Object3D +0x50..+0x58`), model scale
`0x10a` (≈ 0.065), initial motion, tint (type 22), seated sprite (`+0x90` = type 15
angle).

**Units.** Positions and angles are in world-object units (`fx32`, radians ×
4096), the same as the hero's position (`Object3D +0x44`): in `demo.ml1`, the hero and the church NPCs
are in the same frame. Each interior is a separate map, with its own
origin.

### Initial motion of models

1. `NpcModel_SetMotionIndex` (`0x020415a8`) → `Object3D::MaybeSetBCFGAnimation`
   (`0x020370b0`) with placement byte `+0x1e` (0 by default) on the table of the NPC's
   archive (first package); time starts at the start of the entry.
2. If type 19 names a motion: `Object3D::MaybeSetRegularAnimation` (`0x02036e44`)
   looks for this name in the packages in order (its own, then the type 19 one, appended at
   the end) and only keeps a motion that has animation files; time starts at 0.

`cast::initial_motion` returns `(package, index)`. Example: `s017` in M01 at the prologue:
`stand`.

### Sprites: displayed direction

`NpcSprite_FaceCamera` (`0x0203cbe8`) picks the animation each frame from the NPC's angle
minus the camera's (`0x0202e800`), brought into [0, 2π[: eight sectors centred on
multiples of π/4 (`FX_Div((a + 2π)·8 + π, 2π) >> 12 & 7`), table `0x020e7948` to the names at
`0x020efd00`: `stand_down`, `stand_r_down`, `stand_right`, `stand_r_up`, `stand_up`,
`stand_l_up`, `stand_left`, `stand_l_down` (`cast::sprite_stand`). Seated NPCs
(sprite flag 8) add the type 15 angle and read another table (indices 16–20:
`sleep_*`); other modes use four sectors (`0x020e7928`, `0x020e7930`)
**[not modelled]**. The `.spr` format is described by the wiki (Sprites) **[not implemented here]**.

## `.chr` archives

Uncompressed NARCs (2,683 in the ROM). Role of a member from its name (`strstr`,
`chr::role`):

| Member | Role |
|---|---|
| contains `.nsbmd` | model (only the first counts) |
| contains `.bcfg` | motion table (only the first counts) |
| `<motion>.nsbca/.nsbma/.nsbtp/.nsbta` | the motion's animations |
| `.beff`, `.bact` | effect and action scripts (read elsewhere) |

Two loaders:

- **per motion** (`LoadFromCCHROrCMOTArchive`, `0x02036814`, NPCs and monsters): for each
  motion in the table, members named exactly `<name>.<ext>` in the order nsbca, nsbma,
  nsbtp, nsbta (`object3DsData.extensionLookup`); a motion without files plays nothing
  (`Chr::motions`);
- **four-slot** (`InternalLoadFromCHRArchive`, `0x0203644c`, effects and objects):
  every member whose name contains an animation extension fills the slot of its
  kind, the last one wins (`Chr::slots`).

### Playing a motion (per-motion package)

`Object3D::AdvanceAnimations_v1` (`0x02035188`): time (`fx32`, in file frames)
advances by `speed × delta × object speed` (rounded products) and loops over
`[0, frame count − 1[`; `delta` = 4096 per 17 ms of game time
(`GameState::CalculateDeltaTime`, `0x02010160`). The table's `start`/`end` bounds
only serve four-slot packages (`AdvanceAnimations_v0`): in NPC `.chr` files
they designate slices of an old single animation (`walk` 10–18, `step_left` 10–14…)
**[hypothesis]**. `Motion::advance`.

## `.bcfg` tables (`BCFG::LoadFromScript`, `0x0203462c`)

Record table run with `bcfgScriptOpcodes` (decompilation, `BCFG.cpp`):

| Type | Fields | Effect |
|---|---|---|
| `0x64` | n | reserves n motions (the table restarts from zero) |
| `0x66` | name, start, end, speed | adds a motion if there is room left (values × 4096; name copied on 16 bytes) |
| `0x6a` / `0x6c` | n / motion, time, value | reserves n marks; adds a mark **at the head** of the motion's list **[unknown]** |
| `0x6d` / `0x6f` | n / motion, time, sound | same for cues played when the normalised time crosses the value (0: at start and on each loop, 1: at the end) |
| `0x71` | bone | root bone (`waist` if absent) |
| `0x65`, `0x6b`, `0x6e` | — | nothing |
| others (`0x67`–`0x69`, `0x70`) | — | no handler: ignored |

Capacity overflows and unknown motions are ignored as in the game.

## The hero (`dq9_formats::hero`, `dq9_render::hero`)

Implementation: `crates/dq9-formats/src/hero.rs` (`HeroLook`, `ItemModels`, `Outfit`),
`crates/dq9-render/src/hero.rs` (assembly, `Gait`), `AnimatedScene::add_hero`. Tool:

```powershell
cargo dq9re hero                                   # new-game hero: resolved parts
cargo dq9re hero --save rom/dq9.sav                # hero from a save (--character N for another one)
cargo run -p dq9-app --release -- --map M01 --frame 30 --walk "-x:5" --start=-1.74,1.22 `
    --eye=-1.5,0,-3 --target=-3,-0.6,1.2 --screenshot build/scratch/app/hero.png
```

Sources: DQIX wiki (Character-Parts, Character-Presets, Single-Questions, Motion-Tables,
Weapon-Positions: everything there is read from the European ROM, rechecked below), ARM9 code for
the animation tables, melonDS RAM for the scale.

### A human character is a set of parts

A character's ten equipment slots h0–h9 (`CharaParts_GetPartNumbers`, USA
`func_02072afc`; h2 the face, h3 the hair) are **items**. Each item names a
model of `data/pack_lv5/chara_pc.gp2` by its id (`item_part`): the thousands
give the letter (12 `m` headgear, 13 `b` armour, 14 `a` arms, 15 `g` gloves, 16 `p` legs,
17 `r` shoes, 20 `w` weapon, 21 `s` shield), the rest the three-digit number: 13007 →
`p_b007`. The 14 preset ids without a part (wiki) also exist in
saves (13309 → `p_b309`, absent): the game ties them to another item's part
**[unknown]**; we fall back to the `p_p090` underwear.

- **Save** (`save.md`, `Character::equipment`, `face_item`, `hair_item`): face `+356` and
  hair `+358` are item ids 9020–9033 and 9000–9013 (real save:
  9027 and 9008); equipment `+352..+370`; byte `+372` bit 0 the gender (0 male, 1 female,
  from gender-restricted items, wiki), bits 1–3 skin tone, 4–7 eyes; `+373`
  hair colour; `+376`/`+378` body build.
- **Face and hair**: `itemdt_def.nat` (member of `data/prm/itemdt_def.gp2`, 32 bytes per
  record, id at `+4`); the word **8 bytes before the id** gives the
  model (`func_020de234`, wiki): bits 0–9 male number, 10–19 female number (999: the one of
  the other gender), bits 20–27 the letter (`h`, `f`). Checked on the 28 items for both genders:
  every part exists (`hero_parts_resolve`). Face 9024 = `p_f004` (male), `p_f014`
  (female); hair 9006 = `p_h060` (male), `p_h180` (female).
- **Hair shape** (`func_02072e94`, wiki): `a` without headgear, else the headgear
  number ÷ 100 indexes `"bbdcc\0cea\0"`; hundreds 5, 9 and ten or more **draw
  no hair**; a male with hair 9001 under a 3xx takes `f`. Model `p_h<nnn><shape>`,
  colour texture `p_h<nn><colour>a.nsbtx` (texture `p_h<nn>0a_00` bound by name; the
  colour is the save's hair colour **[hypothesis]**, wiki: "INFERRED"),
  colour 0 by default.
- **Arms, gloves, shoes** are **textures**: the `p_b…` body binds a material to
  `p_a000_00`, the `p_p…` legs another one to `p_r000_00`; the loaded file decides
  (`p_g<nnn>.nsbtx` if gloves are worn, else `p_a<armour number>.nsbtx`;
  shoes `p_r<nnn>.nsbtx`). Assembly places these files **before** the model's
  textures in the by-name lookup (`TextureSources`).

### The rig and assembly

Body and legs carry the same 14-bone skeleton (`root`, part, `waist`, `chest`,
`arm0L/1L/0R/1R`, `head`, `usiro`, `leg0L/1L/0R/1R`); animation tracks are read by
**object index** (`JointAnimation::transforms`), so the same animation poses both. Face,
hair and headgear have only one bone of their own: they are posed at rest then moved by the
body's `head` bone matrix, the one its render commands hold after
`MulObject(head)` (`object_worlds` replays the matrix stack). Checked: face and
hair land on the neck (total height ≈ 21 model units, test
`hero_model_follows_its_gait`). The parts are merged into a single scene model.

**Scale.** Hero object in RAM (`demo.ml1`, `Object3D+0x5c`, fix16 ×3): `(0xec, 0x109,
0xec)` for body-build words `0xe39` and `0xffb` in the save: `0x10a × word / 4096`
truncated, on x and z for width, y for height (same `0x10a` as NPCs). Object
rotation: game angle (0 faces +z), `dq9_core::field::angle_of_yaw`.

### Motions

The game picks a character's animation by state (`Character_UpdateAnimation`,
`0x02033de4`): state `+0xbe`, kind `+0xc0`; the next kind is `g_CharaStateKinds[state × 13 +
kind]` (`0x020ef9c4`, read in RAM) and `Character_PlayKindAnimation` (`0x02033bb0`) plays the name
`g_CharaKindNames[kind]` (`0x020ef994`) or, for kind 0, `g_CharaIdleNames` (`0x020ef978`) depending on
the expression (`+0xc1 >> 4`). Result: **idle (state 0) → `stand`, keypad walking
(state 1) → `run`**; the game plays `walk` only from scripts (natives 210…). The names come from the
`mp%02d%02d` packages (body `02`, weapon motion set `00` with no weapon, wiki):

| Gait | Package (`chara_pc.gp2`) | Motion | Speed (`.bcfg`) |
|---|---|---|---|
| idle | `mp0200n` | `stand` | 0.1 |
| walk | `mp0200ne` | `walk` | 0.1 |
| run | `mp0200n` | `run` | 0.4 |

`stand` exists in three packages (`n`, `f`, `n2`): we take `n`, the one that moves the
character the least (wiki) **[hypothesis]**; "per motion" playback (`AdvanceAnimations_v1`,
loop over `[0, frames − 1[`). **Gait choice in the application** (`Gait::from_speed`):
horizontal speed measured over the step (units/s): idle under 0.05, walk up to
`RUN_SPEED` = 5, run above **[hypothesis]**: the game plays `run` at any speed, `walk`
is used here for speeds under the established maximum speed (5.76 units/s, `movement.md`), hence for
the provisional 4 units/s speed of `dq9-core` and for the acceleration of faithful
movement. To revisit once faithful movement is in place.

### Default appearance (new game)

`HeroLook::new_game`: male, face 9024, hair 9006 (male presets from
`charapreset.bin`), skin tone 3, Celestrian outfit from the start of Angel Falls **[hypothesis]**:
celestial costume 13007, celestial stockings 16215, celestial shoes 17120 and halo 12805 (character
creation is in overlay 9: the real hero is not modelled). With `--save`:
the hero of the current slot (first character of the party).

### Not done

- **Weapons and shields** are not drawn (`wpnpos.bin`: placements partly
  inferred, shield absent from the file); nor are the wings (`d_hane`, 21-bone rig from
  `chara_pd.gp2`).
- **Skin tones, eyes, eyebrows**: `data/chara/palette.bin` recolours the face and
  arm palettes (Character-Colours); only the original palettes are used **[unknown]**.
- Hair colours: the skin ramp of `GetItemSkinShades` (`0x020731f0`, wiki) is not applied.
- Lighting: fixed character light (`app.md`), not the DS lights.
- Item parts without a model (see above); party companions outside cutscenes (the guest of
  the scenes is drawn: § The guest); the hero during
  cutscenes (script `walk`, event scale and camera).

## The guest (cast mode 10 of native 566)

The chapter 2 scenes carry a companion that is **not in the party list**: Ivor, then (chapter 4)
others. Evidence (Ghidra EU, `ov_001`/`ov_017`):

- Trigger action `0xcd` (`Trig_RunAction` case `0x69` of the switch on `code − 100`, `ov_017::021913d0` = `Guest_Start`) takes
  record `arg` of **`data/bin/attnpc.gp2`** (`attnpc_<lang>.bin`: 5 records, `0x24` bytes each in
  memory: `[id, character number, name, …]`, read by `020d3d88`) and starts the guest task
  `ov_017::021c1e6c`, which loads `data/chara_sub/s%03dn.chr` (`s017n.chr` for Ivor: model
  `s017`, motions `stand`, `run`, `step_*`) into **character `0xce`** (`Chara_Get(0xce)`),
  scale `0x10a`. Action `0xcc` (case `0x68`, `ov_017::02191484` = `Guest_Dismiss`) destroys it.
  `ov_017::021a316c` (the cast teardown of an event) also frees `0xce`.
- Mode 10 of native 566 (`ov_001::02160da8`) binds that character to the cast member: with no
  party member accepted by `02061bd8` it uses `0xce`. The member then has the motions of
  `s017n.chr` plus the package the script declares with 567 (`event_lv5/ev01000s017.chr`:
  `talk`).
- Which trigger rules add or dismiss it (`dq9re triggers M01 --with 2210`): the end of `2210`
  and of `2350` add record 1; the end of `2400` and of `22591` dismiss it
  (`0xcd(1)` / `0xcc(1)`; chapter 4: `4080` adds record 2, `24170` and `24596` dismiss
  record 1).
- **Application**: the trigger state does not hold the guest, so `EventPlayer` keeps it
  (`guest`, `event/guest.rs`) and replays the rules of `Event::Ended` (`guest::rules`).
  The report plays each scene at its own story point and gives it the guest the latest rule
  introduced at or before its step (`guest::introduced_by`; **[hypothesis]**: the order of
  the events of one step is not in the data; 2430 to 2450 come before 2400, whose end
  dismisses Ivor). Test
  `the_guest_of_the_party_stands_in_cast_mode_10_and_plays_his_motions`.
- **[unknown]**: the guest's other places (field walking, battles) and what `Guest_LoadTask`
  does with record fields 3 onwards (`+0x10` bits 0–2 go into byte `+0x56b` of the character).

## Checks

- **RAM (melonDS)**: `cargo dq9re cast --state build/melonds/demo.ml1` (map 109 `C01M09`,
  story 19.2.1, night, quest 4 completed): 2 NPCs, `n016a` (25, block) then `n002a` (10, type
  14), identical to the table in RAM: order, id, kind, byte 3, name, map, position,
  angle, motion byte, counter (`0x3d`, `0x21`), quest, flags; sprite position and angle
  too, except the height put on the ground (0.350 → 0.349, 0.100 → 0.099). Same result
  on `before.ml1`, `field.ml1`, `moved.ml1`.
- **ROM** (`crates/dq9-formats/tests/real_rom.rs`):
  - `character_archives_read_by_role`: 2,683 `.chr` read, 1,910 models, 2,511 tables,
    6,466 motions of which 3,782 with files; in `chara_sub`, 1,055 motions of which 626
    without files (`step_*`) and 7 animations that no motion names; no name longer than
    15 characters; 601 monsters (`.cchr` of `enemy.gp2`) with model and table;
  - `casts_resolve_to_existing_archives`: 508 maps with a `.npc` archive, at the start of
    each of the 19 chapters, by day and by night (19,304 casts, 36,170 NPCs: 21,167
    sprites, 2,511 models, 370 monsters, 12,122 without a body): every sheet, archive or
    member exists and reads (238 distinct); no read of a never-written field; C01M09 in
    the `demo.ml1` state gives the nodes seen in RAM.
- **Hero**: `hero_parts_resolve` (`dq9-formats`, 28 face and hair items × 2 genders
  resolved to existing parts, complete starting outfit) and `hero_model_follows_its_gait`
  (`dq9-render`, assembled model, deterministic, motion according to gait); scale compared with
  the `demo.ml1` RAM (see "The hero").

## Unknown

- Field 4 of `npc.bin` (`+3`); types 12, 16, 18, 20; exact meaning of flags `+0xc` (64,
  4096) and of modes 7–10 (movement: `ov_017`, not read).
- NPC behaviour after loading (wandering, paths, looking at the hero) and what
  changes their motion afterwards (`stand`/`walk`).
- Visitors of `R0*` maps (`0x02064e98`); in the NPC snapshot, the movement
  bits, the `0x40000` mask, the `+0x7e7e` jump and the non-event writers.
- The `.spr` format (wiki Sprites) and the camera angle subtracted by `0x0203cbe8`.
