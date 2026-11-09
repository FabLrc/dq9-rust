# DS save (`.sav`)

Module `dq9_formats::save`, command `dq9re save <file> [-v]` (no ROM).

## Sources and verification

1. Field layout: save editor [DQIX/editor](https://github.com/DQIX/editor)
   (`src/game/layout.js`, `src/saveManager.js`, `pattern.hexpat`, **no license**: read,
   reimplemented, nothing copied; its demo save is not versioned).
2. **Game code** (European ARM9, read in Ghidra): the save subsystem occupies
   `0x020a94f8..0x020ac9bc`. It confirms the structure below and gives the complete table
   of blocks (§ "Serialization"). Names in `re/symbols.txt`, section *Save file*.
3. Check on the demo save (64 KiB): the 6 checksums recompute;
   rewriting (checksums recomputed) gives back the file byte for byte; version `save013`,
   chapter 19, step 2, substep 1, hero Sage level 91, 98 h 32 min 44 s, 38,754 gold.

The `save +0x…` offsets in `re/symbols.txt` (`Save_Chapter`…) are those of `GameState` in
RAM, **not** those of the file: `Save_Serialize` copies blocks of `GameState` to fixed
file offsets. Example: scenario step at `GameState+0x5cb4`, at `slot+11572`
in the file.

## Container

EEPROM image of **0x10000 bytes** = 2 slots of **0x8000** (0x6fe4 useful bytes each).

| Offset | Content | Evidence |
|---:|---|---|
| file 0 | `DRAGON QUEST IX` + byte 0, **only once for the whole file** | `Save_CheckMagic` (`0x020ac920`) reads 16 bytes at 0, compares, otherwise erases and rewrites |
| slot+16 | CRC A, covers `+20..+36` | `Save_ReadHeaderA` (`0x020a9508`): reads 0x14 bytes at `+0x10`, CRC-32 of the following 0x10 |
| slot+20 | "intermission" byte (1 = suspend save) | `Save_SlotStatus` (`0x020ab9d0`): status 4 |
| slot+21 | "exists" byte (0 = empty slot) | same function: status 3 |
| slot+24 | version `save013\0` (`save011` is rejected) | `Save_ReadHeaderA`, `Save_WriteHeaderA` (`0x020aba6c`) |
| slot+36 | CRC C, covers `+40..+132` (0x5c bytes) | `Save_ReadHeaderC` (`0x020ac874`); the 0x54 bytes `+36..+120` (**CRC included**) are copied to `GameState+0x6380` (seen in RAM) |
| slot+132 | CRC B, covers `+136..+28644` (0x6f5c bytes) | `Save_LoadSlot` (`0x020aaf94`): reads 0x6f60 bytes at `0x84` / `0x8084` |
| slot+136 | characters (13 × 572) then the rest (next §) | `Save_Serialize` (`0x020a9ec8`) |

Standard CRC-32 (IEEE, initial value 0). `Save_SlotStatus` statuses: 0 valid, 1 unreadable,
2 CRC A wrong, 3 empty, 4 intermission; in normal mode, the button tested at `0x2000` (held)
forces 2 [unknown: probably a deliberate reset].
A load error sets `GameState+0x5cc8` to 2 (slot 0) or 3 (slot 1).

**The magic is not per slot.** Slot 1 of the demo is the copy of
slot 0 except for the first 16 bytes (`0xff`), which makes sense: the magic only exists
at offset 0 of the file.

## Role of the two slots and write protocol

`Save_LoadSlot` (`0x020aaf94`) receives `intermission` (0 or 1): **slot 0 = normal
save, slot 1 = suspend save** ("intermission"). It reads the header at
`0x10` or `0x8010`, the blocks at `0x84` or `0x8084`. A checksum B failure sets
`GameState+0x5cc8` to 2 (slot 0) or 3 (slot 1); there is **no automatic fallback**
to the other slot in this function.

`Save_WriteStep` (`0x020aad2c`, one state per call, `*param_1` = state):

| State | Action |
|---:|---|
| 0 | tests the device (reads 1 byte at 0); **reads the whole slot 1** (`0x8000`, `0x6fe4` bytes) into the buffer (to keep what serialization does not rewrite) → 1 |
| 1 | waits for completion, `Save_Serialize`, then writes `0x6fe4` bytes at `0x10` (slot 0, normal save) or at `0x8010` (suspend) → 2 |
| 2 | suspend save: done. Normal save: rewrites **the same image at `0x8010`** → 3 |
| 3 | waits for completion, releases the locks → 4 (done) |

A normal save therefore writes **two identical copies** (slot 0 then 1), which
explains the demo (slot 1 = copy of 0). The header's intermission byte is only 1
in the copy written by a suspend save; only `Save_SlotStatus` reads it.
`Save::current_slot` takes the first slot with a valid status (0 before 1).

## Serialization (`Save_Serialize`, `0x020a9ec8`)

The function's offsets are relative to a buffer shifted by `-0x84`: add 0x84 to get the
slot offset. The 16 blocks below **exactly tile** the checksum B area
(`136..28644`, test `blocks_tile_checksum_b_exactly`): size = distance to the next block,
cross-checked with the code's copy lengths. Raw access: `Slot::block(Block)`.

| `Block` | Offset | Size | Content | Read in the game |
|---|---:|---:|---|---|
| `Main` | 136 | 11432 | characters (13 × 0x23c), counts, party order, held items, gold, bank, medals, zone, settings | `Save_UnpackMain` (`0x020a95b4`) |
| `Progress` | 11568 | 836 | 4 words (chapter, step, substep, 1 word) + event block (0x334) | `Save_UnpackProgress` (`0x020a9a60`) |
| `QuestLog` | 12404 | 160 | `QuestLog_Get()` | direct `memcpy` |
| `QuestTimers` | 12564 | 816 | `QuestLog+0x178` | direct `memcpy` |
| `Hints` | 13380 | 404 | 1 byte (`func_020120d4`) + 400 bytes → `GameState+0x5cdc` | `Save_UnpackHints` (`0x020a9b08`) |
| `Monsters` | 13784 | 1232 | `GameState+0x75f0` | direct `memcpy` |
| `Alchemy` | 15016 | 948 | `GameState+0x7ac0` | direct `memcpy` |
| `Accolades` | 15964 | 236 | `GameState+0x7504` | direct `memcpy` |
| `Zone` | 16200 | 6976 | 0x1b30 bytes of the zone object (`func_02012fe4()+0x840`), then 5 fields at `+0x1b30..+0x1b3f` (→ `+0x2374..0x23a1`) | `Save_UnpackZone` (`0x020a9b3c`) |
| `DqvcMessages` | 23176 | 1028 | 2 × 512 bytes → `GameState+0x5f80`, `+0x6180`; 1 word → `+0x5f7c` | `Save_UnpackDqvcMessages` (`0x020a9bb4`) |
| `TreasureMaps` | 24204 | 2776 | 0xad6 bytes → `GameState+0x64f4`: 99 maps of 0x1c bytes (`TreasureMapMetadata`), after a 2-byte header; then rebuilds the current grotto | `Save_UnpackTreasureMaps` (`0x020a9c24`) |
| `Profile` | 26980 | 124 | `GameState+0x569c` | direct `memcpy` |
| `Warp` | 27104 | 28 | resume destination: map u16, angle s16, position (3 × fx32), § "Position on resume"; **suspend save only** | `Save_UnpackWarp` (`0x020a9d88`) |
| `Dqvc` | 27132 | 708 | `GameState+0x71fc` | direct `memcpy` |
| `Misc` | 27840 | 272 | `GameState+0x5e6c` | direct `memcpy` |
| `Suspend` | 28112 | 532 | suspend: 0x200 bytes → `GameState+0x6fcc`, 10 bytes (`+0x204`) → `+0x5cd0`, one word (`+0x200`); normal save: the same block reads as `u16` presence, `u16` n, then n × 0x14 bytes → `func_02010828()+0xf2c` (3 at most) | `Save_UnpackSuspend` (`0x020a9e40`) |

`Main` in detail (slot offsets): characters `136 + 572 n`; `7572` characters in
reserve; `7573` party order (4); `7577` party size; `7578 + 18 n` held items
(8 × u16 + 2 bytes); `11448` gold on hand; `11452` gold in bank; `11460` mini medals;
`11476` current map id (109 in the demo; `Save_LoadSlot` copies it to
`GameState+0x7e74`, and the observed map `C01M09` has id 109); `7652` bag (0x1d4 bytes,
see "Unknown"); settings and starting data `11488..11519`; intermission flag
`11530` (buffer `+0x2ca2`).

Checksum C (`40..132`) does not cover these blocks: it protects the small block copied to
`GameState+0x6380` (with the CRC word itself).

## Position on resume

`Save_LoadSlot` places no one: it fills `GameState`. The party is placed later by
`Party_Place` (`ov_017::0219c598`), called in state 3 of the map entry state machine
(`Field_EnterMap`, `ov_017::0219e384`), after the map is loaded. Without an explicit
position, it chooses:

- **byte `Save_Warp+7` zero** (normal save, new game): the **map's start
  point**, record `0x6e` of its `.bmbl` (`BlockList_RecStart`, `0x0201d4a4`):
  three floats × 4096 → `BlockList+0x70` (= `g_Zone3D+0xdc`), a fourth × 4096, wrapped
  to `0..2π` (`0x02030f40`), → `+0x7c` (= `g_Zone3D+0xe8`); read by `BlockList_GetStart`
  (`0x0201e81c`) and `BlockList_GetStartAngle` (`0x0201e830`);
- **byte `+7` non-zero** (suspend save, set by `Save_UnpackWarp`): the vector
  `Save_Warp+0x10` and the angle (`s16`) `+0x1c`, i.e. the fields of the `Warp` block below.

Each party member (order `GameState_Party+0xf78`, size `+0xf7c`) is placed by
`Object_Place` (`0x020399c0`): map (`Object3D` field 6), position `+0x44`, angle `+0x54`,
without ground snapping. State 9 of `Field_EnterMap` then copies the hero's position to
`GameState+0x7f60` (seen in RAM).

`Warp` block (`slot+27104`, 28 bytes, read by `Save_UnpackWarp` `0x020a9d88`):

| Offset | Type | Destination | Meaning |
|---:|---|---|---|
| +0 | u16 | `Save_Warp+0` | map id |
| +2 | s16 | `Save_Warp+0x1c` | angle, fx32 radians |
| +4 | 3 × fx32 | `Save_Warp+0x10` | world position |
| +0x10, +0x12 | u16 | two fields of `g_Zone3D` (`0x0201bc88`, `0x0201bc7c`) | [unknown] |
| +0x14 | u8 | bit 3 of a zone flag | [unknown] |
| +0x15 | u8 | `GameState+0x7e7e` | [unknown] |
| +0x16, +0x18, +0x1a | u16 | `GameState+0x7e78..0x7e7c` | [unknown] |

On a **normal** save this block contains stale bytes that nobody reads
(demo: `21 4e f7 57…`); `Slot::warp` only decodes it for a suspend save.

**Verified in RAM** (`Party_Place` without position, byte `+7` zero): on the demo, the state
`before.ml1` (map 109 `C01M09`) has the four members at (860; 491; −5,652) in fx32 with
angle `0x323d` at `+0x54`, exactly record `0x6e` of `C01M09.bmbl`
(0.21; 0.12; −1.38; 3.14) as read by our code, and that same triplet is at `g_Zone3D+0xdc`
(`0x020fb4cc`). Same save, map id patched to 100 (`C01`, checksums recomputed) then
loaded in melonDS: the hero is at x = −819, z = 113,664, angle `0x3245`, identical to
the record of `C01.bmbl` (−0.2; 0.1; 27.75); the height is 821 instead of 409:
the game places the hero `0x199` above the ground it finds (`Zone3D_GroundHeight`) after
the fact; on the church floor the written height is already that one (0.12 = 0.02 + 0.1).
Test: `dq9-core/tests/real_rom.rs`, `save_start_matches_melonds_ram` and
`patched_save_start_matches_melonds_ram` (`DQ9_SAVE`, `DQ9_STATE`, `DQ9_STATE_MAP100`).

**Not verified in RAM**: the suspend save (no save of that kind, and the
`melonds-state.ps1` script only drives "Continue"); state 8 of `Field_EnterMap`
can still replace the hero's position (`ov_017::021a01f4`, table indexed by map and
time of day) **[unknown: which maps]**; a map without a `0x6e` record
places the party at (0, 0, 0).

## Clock and time of day

Header A carries the time: an `f32` at `slot+32` (seconds, 420 s day;
`Save_Serialize` writes `GameState::GetDayTimer` there). `Save_LoadSlot` passes it to
`GameState::SetDayTimer` (`0x02010298`, `GameState+0x3cc`), which derives the time of day (`+0x3dc`):
night 0 (< 180 s), morning 1 (≥ 180), day 2 (≥ 210), evening 3 (≥ 390); thresholds read in RAM
(`0x020f33b8..0x020f33c4`: 420, 390, 210, 180). The demo: 42.649 s, night, identical in RAM;
the "clock running" flag (`+0x3d8`, 1 in RAM) is not saved. `dq9_core::state`
carries `SetDayTimer`, `AdvanceDayTimer` (`+0x3c4` ticks × `+0x3d4` = 1/60 s), `SetTimeOfDay`.

## In RAM after a load

Checked on a melonDS state taken in game after "Continue" on the demo
(`dq9re state … --save`, [`melonds-state.md`](melonds-state.md)): every plain copy of
`Save_LoadSlot` (`0x020aaf94`) and its functions is found **byte for byte** at
the address read in the code. Complete table: `melonds::SAVE_COPIES`.

| Data | Destination | Seen |
|---|---|---|
| header C (`slot+36`, 0x54) | `GameState+0x6380` | identical |
| bag (`7652`, 0x1d4), then `+0x2b08` (0x128) | `GameState_Party` (`0x02010838` → `GameState+0x2a04`) `+0`, `+0xe04` | identical |
| bank, gold, word `11456`, mini medals | `GameState+0x396c`, `+0x3970`, `+0x3974`, `+0x3978` (party `+0xf68..+0xf74`) | identical |
| party order (4) and size | `GameState+0x397c`, `+0x3980` | identical |
| map id (`11476`, u16); `11490`, `11491` | `GameState+0x7e74` (also party `+0x2c94`); party `+0xf7d`, `GameState+0x63dc` | identical; id 109 matches map `C01M09` in the list |
| `11492`, `11494` (u16, reversed order), vector `11496` (3 × fx32) + word `11508`, `11512`, `11516`, `11519` | `g_Zone3D+0x2786`, `+0x2784`, `+0x2774..+0x2784`, `+0x2794`, `+0x2788`, `+0x27d0` | identical; the vector is not the hero's position on resume (§ "Position on resume") |
| party members (572 bytes each) | converted by `PartyMember_LoadRecord` (`0x02082d7c`) to `GameState_PartyMember` (`GameState+0x474 + i×0x964`), next § | all compared fields identical |
| characters in reserve | `GameState+0x3984` (n × 572, direct copy) | demo: 0 in reserve |
| progress words (16) | `GameState+0x5cb0` | identical |
| event block | `g_QuestManager` (`0x02108844`, returned by `0x0205ec44`) | identical except `+0x180..+0x1cf` reset to zero |
| `QuestLog`, `QuestTimers` | `g_QuestLog` (`0x02109418`), `+0x178` | identical |
| `Hints` (`+4`, 400) | `GameState+0x5cdc` | 180 bytes changed by the game |
| `Monsters`, `Alchemy`, `Accolades`, `TreasureMaps`, `Profile`, `Dqvc`, `Misc`, DQVC messages | `GameState+` offsets from the table above | identical |
| `Zone` (0x1b30) | `g_Zone3D+0x840` (`0x020fb3f0`, returned by `0x02012ff4`) | identical |

The whole loaded save is in the ARM9 `.bss` (`0x020f2e60..0x021536e0`): fixed addresses.
The slot read buffer is also in RAM (`slot+0x84` at `0x0214633c`): a
search for save bytes hits it first.

### Party members

`Save_UnpackMain` handles the i-th party character (record `standby + i`): it searches among
the 4 member objects for the one whose word `+0x568` equals `order[i]` (otherwise a free object, negative
word), loads the record into it, writes `order[i]` to `+0x568`, copies the 8 held items
(`7578 + 18 i`, 16 bytes) to `+0x454`, then removes item `0x5612` from the held items
(`PartyMember_DropHeldItem`, `0x020838d4`: the first copy becomes `0xffff`, then **a
single pass** shifts each hole by one rank) and from the bag (`Bag_Remove(…, 0x5612, 99)`). The member
with id 0 (the hero in the demo) receives 5 at `+0x94c`. The party order is therefore **not** a
list of record indices: it is the member object id for each position.

`PartyMember_LoadRecord` (record → member object):

| Record | Member object | Known meaning |
|---|---|---|
| `+1` bits 0-3 | `+0x56a` | [unknown] |
| `+1` bit 4 | `+0x78` bit 30 | [unknown] |
| `+1` bits 5-7, `+0x13f` bits 0-2 | `+0x56b` bits 0-2, 4-6 | [unknown] |
| `+2` (13 bytes) | `+0x16c` (13 × u16) | levels per vocation |
| `+0xf` (13) | `+0x186` | revocations |
| `+0x1c` (13 × u32) | `+0x138` | experience |
| `+0x50` (u32), `+0x54` (u16) | `+0x950`, `+0x954` | current vocation (used to load the vocation file, `0x02082fd4`); [unknown] |
| `+0x58` (13 × 12) | `+0x850` | seeds |
| `+0xf4` (u16) | `+0x564` | free points |
| `+0xf6` (27), `+0x111` (9), `+0x11a` (0x24) | `+0x464`, `+0x910`, `+0x8ec` | skills / masteries [hypothesis] |
| `+0x13e` bits 0-2 | `+0x18` bits 0-2 (set, never cleared) | [unknown] |
| `+0x13f` bits 3-7 | `+0x94c` | [unknown] (5 in the demo) |
| `+0x140` (name) | `+0x3c`, text (glyph indices converted by `func_02042774`) | name |
| `+0x154` (u32) | `+0x1c` = bits 10..19, `+0x1e` = bits 20..29 | [unknown] |
| `+0x160` (0x1c) | `+0x488` | equipment and appearance |
| `+0x17c` (0xc0) | `+0x4a4` | [unknown] |

Checked on the demo: the 4 members, all these fields except the name and the bits of `+0x18`
(`melonds::State::compare_party`, `dq9re state --save`). The names are indeed ASCII at
`+0x3c` ("Adenine"…).

### Deviations caused by the game

[unknown: which function]

- Global flags `+0x180..+0x1cf` of the event block (indices 1,952 to 2,591, 80 bytes)
  reset to zero in the game (the demo had 211 set). [hypothesis] temporary zone
  flags, cleared on map load.
- `Hints`: words `1e281ea2` become `012800a2` (bytes 0 and 2 rewritten).

The title screen already reads the slot (adventure log) and sets chapter/step/substep
in `GameState`, without the rest: a state taken on the title screen has the right scenario but neither
the gold nor the bestiaries (`ov_004` loaded instead of `ov_003`).

## Event block (`slot+11584`, 0x334 bytes)

`func_020a99d0` copies it in five pieces (`0x8c`, `0x234`, `0xc`, `0x66` bytes, then 1);
`func_020a9a50` does the reverse and first sets the current chapter/step/substep
(`0x02010774`, `0x020107a8`, `0x020107dc`, `0x02010810`). The block is the start of the quest
manager of `trigger.md`: its offsets apply as is.

| Offset | Size | Content |
|---:|---:|---|
| `0x000` | 5 × 0x1c | tracks: chapter, step, substep bytes, 13 bytes of chapter flags, 12 bytes of step flags |
| `0x08c` | 0x234 | global flags (4,512 bits, least significant bit first: `Flags_Test`); script index `>= 0x400` is shifted by `0x6fa` (`Flags_TestExt`) |
| `0x2c0` | 0xc | [unknown] |
| `0x2cc` | 0x66 | 204 quests, two quartets per byte (state and flags, see `dialogue.md`) |
| `0x332` | 1 | current track (0..4) |

Verified on the demo: current track 0, with 19 / 2 / 1 = chapter/step/substep of the
header words; 1,618 global flags set. Accessors: `Slot::event_flags`, `EventFlags`,
`Track`, `test_bit`.

A **new game** does not start from an empty block: the cutscene flags
(`EventTask_Update`), visited-map flags (`Flags_SetMapVisited`), obtained-item flags
(`func_0207cc18`) and held-button flags (`KeyManager::RefreshKeys`) are set by the engine
before and during the prologue, without going through a save load. The starting
equipment (copper sword, costume, celestial bottoms and shoes) comes from the creation table
`ov_009` `0x0218636c` and sets 3191, 3775, 3955 and 4046 through the item rule
([`item.md`](item.md)). Mechanisms, RAM evidence and remaining gap: [`trigger.md`](trigger.md),
§ "Global flags set at the opening".

## Fields typed by `dq9_formats::save`

See the accessors of `Slot` and `Character`; all are verified on the demo. Character
(572 bytes): levels `+2` (13 bytes, vocation 0 = Guardian), revocations `+15`, experience
`+28` (13 × u32), vocation `+80`, seeds `+88` (12 bytes per vocation: 3 words of 3 × 10
bits: strength, agility, resilience / deftness, charm, magical mending / magical might,
max HP, max MP), free points `+244`, masteries `+282`, name `+320` (10 bytes, **one byte per
character = index in the glyph table of the `me` font** (`font.md`), terminated by 0:
`12 2f 30 39 34 39 30` = A d e n i n e; the demo gives four readable names), equipment
(u16) torso `+352`, legs `+354`, arms `+360`, feet `+362`, head `+364`, weapon `+366`, shield
`+368`, accessory `+370`, face `+356`, hairstyle `+358`, skin/eyes/gender `+372`, hair
`+373`, body shape `+376`/`+378`.

## Unknown

- Names: see above (glyph indices); characters outside the `me` font are not
  checked.
- Exact meaning of vector `11496`: copied to `g_Zone3D+0x2774`, it is **not** the
  hero's position on resume (§ "Position on resume"). **[hypothesis]** return point
  on the overworld map (id `11492` is 20002 = `F02` in the demo, the vector
  (−57; 0.1; −31.5) is a position on that map).
- Bits of `visited places` (`0xfe77ffff` in the demo, the editor only names bits 1..=18).
- Meaning of the flags themselves (indices): the block is typed, not the bits. The `+0x2c0` block
  (12 bytes) has no established role.
- Internal content of the blocks `QuestLog`, `QuestTimers`, `Hints`, `Monsters`, `Alchemy`,
  `Accolades`, `TreasureMaps`, `Profile`, `Dqvc`, `Misc`: sizes and RAM destinations
  established, **fields not decoded**. The bag (`Main`+7652) is decoded in
  [party.md](party.md) (`dq9_core::party::Bag`).
- The write protocol is understood (§ "Role of the two slots") but **not ported**:
  it drives the hardware (`Save_BackupWrite`) and a load state machine, hence no
  `port:` marker; `dq9_formats::save` only writes the image (`Save::seal`).
- Number and order of skill allocations (`+247`).
