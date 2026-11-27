# DS save (`.sav`)

Module `dq9_formats::save`, command `dq9re save <file> [-v] [--set <name>=<value>…] [--out <file>]`
(no ROM). § "Writing" covers the write path.

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
`Save::current_slot` takes the first slot with a valid status (0 before 1); the slot a
start-up resumes is [`Save::resume_slot`] — the same, else the suspend save (§ "The suspend
slot"). The only caller that passes `intermission` 1 — `Save_WriteStep`'s fourth argument — is
the field menu's Quick Save (`ov_002::02169efc`, § "The suspend slot"); the church's
Confession (`ov_003::02159d0c`) passes 0.

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
see "Unknown"); settings and starting data `11488..11519`; the byte `11562` (buffer
`+0x2ca2`) `Save_UnpackMain` (`0x020a95b4`) hands back to its caller, which
`Save_Serialize` sets to 1 only for a normal save whose flags carry `0x10` (the church's
Confession passes 1, or `0x15` in another's world) — never for a suspend save (§ "The
suspend slot").

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

## The suspend slot

Slot 1 is the **suspend** ("intermission") save. Its writer is the field menu's
**Misc. → Quick Save**, and its reader is a normal load with `intermission` 1.

**The trigger.** `I` on the field opens the menu; its Misc. list is built and run by
`ov_002::02166168`, which switches on the chosen entry code: 0 Heal All, 1 Tactics,
2 Allocate Skill Points, 3 Assign Party Tricks, 4 Profile Settings, 5 Volume Settings,
6 Quest List, **7 Quick Save**, 8 Call to Arms. The labels and descriptions are the ROM's
own, `data/bin/menu/str_tm.gp2` (`str_tm_en.nat`): 4000 "Misc.", 4001..4009 the labels,
4020..4028 the descriptions — entry 7 is 4008 "Quick Save", described by 4027 "Temporarily
suspends your adventure in its current state." Choosing it sets the menu's step to `0x24`,
whose handler in the overlay's step table (`0x0216cd74`, entry `0x24`, filled through the
lazy-init path of `func_ov002_02155960`) is `ov_002::02169efc`.

**The task.** `ov_002::02169efc` runs on its own sub-state (`+0x1bc0`):

| Sub-state | Action |
|---:|---|
| 0 | message **4700** "Do you want to make a quick save log and suspend your game for the moment?" with its answer window; in multiplayer (`func_0202b7d8`) message **4703** instead, no question |
| 1 | message **4701** "Recording quick save log. Do not turn the power OFF…" and starts loading a file |
| 2 | waits for that load |
| 99 | `func_020a9ea4` (the save buffer's own init, the same call the Confession makes before its write) |
| 100 | `Save_WriteStep(buf, …, 0, 1)` — **intermission 1** (`0x020aad2c`) |
| 0x65 | message **4702** "Quick save log successfully created. Returning to title screen." |
| 3, 4 | the fade and the return to the title screen |

**What the write does.** `Save_WriteStep` state 0 reads the **whole** slot 1 (`0x8000` bytes
at `0x8000`) into its buffer, so what serialization does not rewrite is kept from the
previous image — a mirror of slot 0 on a cartridge that has only ever seen normal saves.
State 1 calls `Save_Serialize` (`0x020a9ec8`) with its fourth argument (intermission) set,
then writes `0x6fe4` bytes at `0x8010`. With that argument:

- the header A gets the **intermission byte 1** (slot+20) and the **exists byte 1**
  (slot+21) — `local_1464`/`local_1463` in the decompilation, the two bytes
  `Save_SlotStatus` (`0x020ab9d0`) reads;
- the `Warp` block (`slot+27104`) is filled from the **live leader object**
  (`GameState::GetUnknownGameObject`): `+0` = the object's map id (its field 6), `+2` = the
  low halfword of its angle `+0x54`, `+4`/`+8`/`+0xc` = its position `+0x44` (3 × fx32);
  `+0x10`/`+0x12` come from the zone/return record (`func_02027ca4`, `func_02028bd0`),
  `+0x14` from a zone byte's bit 2 and `+0x15` plus the three words `+0x16`, `+0x18`, `+0x1a`
  from `func_0203e524` (their meanings stay **[unknown]**, as on the read side);
- the `Suspend` block (`slot+28112`) carries suspend data instead of a normal save's short
  list: `0x200` bytes from `GameState+0x6fcc`, 10 from `+0x5cd0`, and the word `+0x71cc`
  (`| 1` when the grotto struct's byte 3 is set);
- the `Main` block's byte at `11562` stays **0** (`local_1508` is only set for
  a *normal* save whose flags carry `0x10`, see § "Serialization").

State 2 only completes: `Save_WriteStep`'s mirror copy (`0x8010`) is the **normal** save's
second step, so a suspend write leaves slot 0 untouched. States 3/4 release the lock.

**The resume.** `Save_LoadSlot` (`0x020aaf94`) with `intermission` 1 reads the header at
`0x8010` and the blocks at `0x8084`; `Save_UnpackWarp` copies the block to `Save_Warp` and
sets its byte `+7`, the "use this position" flag. `Party_Place` (`ov_017::0219c598`) then
stands the party at `Save_Warp+0x10` with the angle `+0x1c` instead of the map's start point
(§ "Position on resume"), and `Slot::area` reads the warp block's map for such a slot. On the
title screen the quick save is its own entry: `str_op` 38 "Load quick save data.", 57/58
"Your adventure will begin from the quick save location" / "When you restart your adventure,
the quick save log will be deleted.", 60/61 the confirmation and 82/92 the corrupted-log
refusal.

**Which slot a start-up takes.** `Save_LoadSlot` (`0x020aaf94`) decides nothing by itself:
its **only selector is the intermission argument** — its second argument is 0 in every caller
and unused in the body — which picks slot 1 (header `0x8010`, blocks `0x8084`, the `Warp` and
`Suspend` blocks unpacked) or slot 0 (`0x84`). The title screen chooses the value:

| Entry | Function | Call | Boot |
|---|---|---|---|
| Continue | `ov_004::0216d504` | `Save_LoadSlot(buf, 0, 0, 0)` | `GameState+4` stays 0 |
| Load quick save data | `ov_004::0216e490` | `Save_LoadSlot(buf, 0, 1, 0)` | sets `GameState+4` to 5 |

`+4` (`func_0200fb9c` reader, `func_0200fb94` writer) is what the field's entry
(`ov_017::0218b688`) switches on: 5 → `Save_LoadSlot(buf, 0, 1, 0)`, 0 →
`Save_LoadSlot(buf, 0, 0, 0)`. The Continue handler's setter call lost its argument in the
decompilation; it is 0, the field branch that loads slot 0. The adventure log the title
screen displays for Continue is read by `func_020ab5d4` (`0x020ab5e4`), which loads slot 0
with intermission 0 and then its header C (`Save_ReadHeaderC`, `0x020ac874`): Continue needs
all three checksums, the quick save checks the same three.

The title screen's slot check (`ov_004::0216dad0`) reads `Save_SlotStatus` of both slots and
sets its entry code: **4** (intermission) on slot 1 first — entry code 0, the quick save, and
it only records whether slot 0 is empty (`*puVar1 = 1` when slot 0 is status 3) —, then 1
(unreadable) → 6, 2 on slot 1 → 7, 2 on slot 0 → 4, 0 (valid) on slot 0 → 1 (Continue, the
adventure log it reads for the display), 3 (empty) → 2 (new game). A present suspend save
therefore ranks **above** a normal one in this check; the function that consumes the code and
builds the menu is not in the decompilation corpus, so what the menu does with both flags is
**[unknown]**.

Erasing is the other half: a new game writes an empty header A into **both** slots and clears
their header C (`func_020abb74`).

**The deletion.** `Save_WriteHeaderA` (`0x020aba6c`) writes an empty header A: the 0x14 bytes
at `slot+0x10` zeroed (CRC A, intermission, exists, day timer) and the version string kept —
`Save_SlotStatus` then reads the slot as 3 (empty), and the blocks stay. The quick-save
handler calls it on slot 1 **after** a good `Save_LoadSlot` (intermission 1) and the header C
write-back, gated on the menu's confirmation byte (`*pcRam0216e5e0 == 1`, whose writer is not
in the corpus; `ov_004::0216e490`, message 58 above), and a separate title step does the same
unconditionally (`ov_004::0216e414`, message `0xd4` when the write fails). So the log is
deleted **at the resume**, not by the next normal save (which does overwrite slot 1 as the
mirror copy). `SlotMut::clear_header` is that write; `Slot::status` then answers `Empty`.

Careful with slot 1: on a cartridge that has only seen normal saves it holds the **mirror
copy** of slot 0 (`Save_WriteStep` state 2), a `Valid` adventure log — `Save::current_slot`
takes it when slot 0 is empty or unreadable, and it is not a quick save to delete
(`resume_slot`'s suspend branch requires the intermission header).

**A normal save from a suspend-only container.** A container whose only save is the quick
save log — slot 0 empty, as a new game leaves both header A blocks (`func_020abb74`) when only
a quick save has been taken since — has no adventure log for the CLI's `--save-out` or the
Confession to update. The original has no such problem: `Save_WriteStep` (`0x020aad2c`) state 0
reads **slot 1's** image (`0x8000`, `0x6fe4` bytes) into its buffer whatever the slots hold, and
state 1 writes that buffer from `+0x10` at slot 0's `0x10` for a normal save (the literal
`0x10` of the disassembly; state 2 then writes the same image at `0x8010`). `Save_Serialize`
(`0x020a9ec8`) rewrites header A — CRC over the 0x10 bytes at `+0x14`, the intermission byte
(0 for a normal save), the exists byte 1, the `save013` constant and the day timer — and the
modelled blocks from `GameState`; the `Main` block's own map word (`11476`) is always
written, from the live map. Everything serialization does not rewrite keeps the bytes the
buffer took from **slot 1**: `Main`'s remainder (bank, medals, visited places…), `QuestLog`,
`QuestTimers`, `Hints`, `Monsters`, `Alchemy`, `Accolades`, `Zone`, `DqvcMessages`,
`TreasureMaps`, `Profile`, `Dqvc`, `Misc` and the `Warp` block (a normal save writes no pose
there; a normal reader never reads it). Those are the quick save log's blocks, i.e. the live
state at the time of the quick save. State 2 mirrors the new log onto slot 1, so the suspend
log is gone afterwards, as after any normal save.

In the port, `Save::prepare_normal_write` is that choice: the container's adventure log
(`Save::current_slot`) when it has one — today's behaviour, nothing is seeded — else slot 0
seeded with slot 1's image when the latter is a sealed `Intermission` slot, else `None` (a
fresh container's opaque blocks are not modelled, so the write refuses rather than zeroing
them). `App::write_save_out` then writes the normal header (`set_intermission(false)`,
`set_exists(true)`), the modelled state, seals and mirrors. A normal save carries no pose, so
the state written this way has `Placement::MapStart` and the party resumes at the map's start
record (§ "Position on resume") — the live position of the resumed quick save is not carried
over. **[limit]** the `Suspend` block: the original serializes a normal save's short list
there, the port keeps the bytes of the image (slot 1's suspend payload for a fresh slot 0);
its reader in the port does not read that block, but the DS `Save_UnpackSuspend` would.
[hypothesis] the DS accepts that slot 0 — header, three checksums and modelled blocks match
what the original writes, melonDS was not driven for this batch.

**In the port.** `dq9_core::state::GameState::write_suspend(&mut SlotMut, HeroPose)` is the
writer: the modelled state at the offsets `write_into` uses (`Main`'s own map word included),
the header's intermission and exists bytes, and the `Warp` block of the **live** pose
(`App::write_suspend_out` takes it from the hero: `dq9_core::field::to_fx` of the position and
`angle_of_yaw` as an `i16`, the low halfword the original stores). The whole container goes
through a temporary file and a rename; slot 0 is never written; the input is untouched when
the destination is another file. The field menu's Misc. row lists the ROM's nine entries with
Quick Save as the only one with a handler (the others draw greyed, their description in the
footer), asks 4700, and writes into `--suspend-out`, or into the `--save` container itself
when that option is absent — the way the DS writes its own cartridge.

The reader is `Save::resume_slot` (`dq9-formats`): the normal log ([`Save::current_slot`])
when the container has one, else the slot whose header says intermission with its three
checksums (slot 1 in a save the game wrote); `GameState::from_save` builds the state from that
slot, so a suspend save gives its warp map, its `Warp` pose ([`Placement::At`]), story, party,
gold and clock. Two readers exist: **`Save::title_slot`** is the title screen's ranking (a present
quick save above the normal log, `ov_004::0216dad0`), used when the player chooses Continue
(`app/title.rs`); **`Save::resume_slot`** is the start-up used by `--save` and the headless paths,
which takes the normal log first — so a normal+suspend container started with `--save` resumes the
older normal log, while Continue on the title resumes the quick save. `--save` is the cartridge,
so a resume from an intermission slot deletes that log in the file (`SlotMut::clear_header` + `seal`,
temporary file and rename, `native::main`'s `consume_suspend_log`), but only once
`GameState::from_save` has read the slot, as the original writes the header after
`Save_LoadSlot` unpacked it; the Web page keeps its uploaded bytes, having no file. A recorded
run (`--play-record`) does not delete it: it checks the container's hash before replaying.

Not modelled: the `Suspend` block's payload (the port keeps the input slot 1's bytes, which
on a mirrored cartridge are a normal save's list — the original's is the field's own state),
the `Warp` block's extra fields `+0x10..+0x1b` (they keep the input's bytes) and the jingle and
the fade; 4702 returns to the title screen (`app/title.rs`), as the original's entry does.
**DS acceptance of the suspend slot is not attempted**: the written slot 1 has
the right header, checksums, state and `Warp` block (`build/evidence/v3-suspend-save/reload.txt`,
its `Suspend` block kept from the input), but whether the cartridge's title screen offers the
quick save and resumes at that position is **[hypothesis]** — melonDS was not driven through
its title menu for this batch. The port's own start-up is checked instead: a suspend-only
container resumes at the written pose with slot 1's gold and story, a normal+suspend container
at the normal slot's map start with slot 0's gold, and the resumed log is deleted
(`build/evidence/v3-suspend-resume/`).

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

## Writing (`SlotMut`, `dq9re save --set`)

Every field the readers above expose has a setter that writes through the **same constant**
the reader uses (`crates/dq9-formats/src/save.rs`: one `const` per field, read and written
through it), and returns the slot-relative byte range it wrote. `SlotMut::seal` stays the only
CRC writer: a setter leaves the three checksums stale, like a write in progress, so a caller
that skips it produces an image the game rejects (`Save_ReadHeaderA` / `Save_LoadSlot`) — the
contract is stated on `seal` and checked by `a_write_without_a_seal_reports_a_bad_checksum`.

| Family | Setters |
|---|---|
| header | `set_exists`, `set_intermission`, `set_day_timer` (the `f32` at `slot+32`) |
| summary | `set_play_time`, `set_area` |
| resume | `set_warp` (`map`, `angle`, 3 × `fx32`; intermission save only) |
| scenario | `set_chapter`, `set_scenario_step`, `set_scenario_substep` |
| event block | `event_flags_mut` → `set_current_track`; `track_mut` → chapter / step / substep / chapter flags / step flags; `set_global_flag` (`story` applies the `0x6fa` shift of `Flags_TestExt`) |
| party | `set_standby_count`, `set_party_count`, `set_party_order`, `set_held_item`, `set_held_items` |
| money and counters | `set_gold_on_hand`, `set_gold_in_bank`, `set_mini_medals`, `set_visited_locations`, `set_special_guests`, `set_party_tricks`, `set_unlocked_vocations` |
| character | `character_mut(i)` → `set_name_raw`, `set_vocation`, `set_level`, `set_revocations`, `set_experience`, `set_seed`, `set_unallocated_skill_points`, `set_proficiency_flags`, `set_equipment`, `set_face` / `set_face_item`, `set_hair_style` / `set_hair_item`, `set_gender`, `set_skin_color`, `set_eye_color`, `set_hair_color`, `set_body` |

Not writable: the version string at `slot+24` (a format marker — the game rejects `save011`
in `Save_ReadHeaderA`, so a wrong value makes the slot unreadable) and HP/MP, which the
readers above do not expose: the record holds the per-vocation **seeds** (max HP/MP among
them, `set_seed`), the current values live in `GameState`.

**The mirror copy.** `Save_WriteStep` state 2 rewrites the **same image at `0x8010`** after a
normal save (§ "Role of the two slots and write protocol"), so the game leaves two identical
copies; the demo proves it (`build/sav/demo.sav`: slot 1 = slot 0 except its first 16 bytes,
`ff`, where no header lives). `Save::mirror_normal_slot` performs that copy: `0x6fe4` bytes
from `slot+16` to slot 1 `+0x10`, leaving the first 16 bytes of slot 1 alone; it returns
`false` for an empty or suspend slot, which the game never copies. The test
`a_real_saves_no_op_edit_is_byte_identical` checks that mirroring the demo changes no byte,
and `mirror_skips_a_suspend_save` that a suspend image is left alone.

**CLI.** `dq9re save <file> --set <name>=<value>… --out <file>` applies the sets to the
current slot (`Save::current_slot`), seals, mirrors when slot 0 was the target, prints every
changed field with its byte range and the number of differing bytes (data and checksums
apart), then writes `--out`; the input is never modified. With `--out` and no `--set` the copy
is byte-identical (nothing is sealed); `--set seal` seals without changing a field. Names
mirror the accessors, values are decimal or `0x` hex:

| Example | Meaning |
|---|---|
| `gold=1234` (alias of `gold_on_hand`), `gold_in_bank=0`, `mini_medals=12` | the words at `11448`, `11452`, `11460` |
| `play_time=98:32:44`, `day_timer=42.5`, `area=109` | header and summary fields |
| `chapter=19`, `scenario_step=2`, `scenario_substep=1` | the progress words |
| `global_flag.9=1`, `story_flag.0x400=1` | one bit of the event block |
| `track.2.chapter=19`, `track.2.step_flags=1,0` | one track |
| `current_track=4` | the track selector at `+0x332` |
| `exists=1`, `intermission=0` | the two header flags `Save_SlotStatus` tests |
| `party_count=4`, `party_order=3,2,1,0`, `held_items.0=1,2,3,4,5,6,7,8` | party list and held items |
| `character.0.name=18,47,48`, `character.0.level.5=42`, `character.0.equipment.head=9001` | one character record (glyph indices, vocation index) |
| `warp=109:12866:0:0:0` | `map:angle:x:y:z`, intermission saves |

An unknown name or an out-of-range index is refused before anything is written.

**Checked by tests** (`crates/dq9-formats/tests/save_write.rs`; synthetic image always, the
demo when `DQ9_SAVE` names it): each setter's diff is confined to its own field range plus
the three checksum words and never touches slot 1, each round-trips through `Save::parse`,
and a dump of every reader accessor shows that no other field moved. `visited_locations`
(`11788`), `party_tricks` (`12108`) and `unlocked_vocations` (`12276`) are **views into the
event block's global flag array** (`11724..12288`), so writing them changes two readers at
once: that is the format, not a bug.

Not done yet: the hardware protocol (lock, EEPROM write, wait states) stays unported
(§ "Unknown").

## Writing a session

`dq9_core::state::GameState::write_into(&self, slot: &mut SlotMut)` binds the modelled state to
a slot: the **inverse of `from_slot`**, every field the reader fills going back at the offset
the reader takes it from (`crates/dq9-core/src/state.rs`, tests in the same module; the real
save test runs when `DQ9_SAVE` names one).

| Written | Offset | From the state |
|---|---|---|
| map (`set_area`) | 11476, or the warp map of an intermission save | `map` |
| resume block (`set_warp`) | 27104 | `placement` when it carries a pose (`Placement::At`) |
| day timer (`set_day_timer`) | header A `+32` | `clock.seconds()` |
| chapter / step / substep (`set_chapter`…) | 11568, 11572, 11576 | `scenario` (the active track's progress) |
| five tracks (`track_mut`: chapter/step/substep, 13 chapter flags, 12 step flags) | event block `+0`, 5 × `0x1c` | the event block's track bytes |
| global flags (`set_global_flag`, all 4,512 bits) | event block `+0x8c`, `0x234` | the event block's flag array |
| quest nibbles (`set_quest`, 204) | event block `+0x2cc`, `0x66` | the event block's quest quartets |
| current track (`set_current_track`) | event block `+0x332` | the event block's selector |
| party count (`set_party_count`) | 7577 | `party.members.len()` (at most 4) |
| members (`character_mut`): name glyphs, sex, vocation, 13 levels, 13 experiences, equipment, skill points, skill panel bytes, maximum HP and MP (`set_max_hp`/`set_max_mp`, 10-bit fields 7 and 8 of `+0x14c`) | records `136 + 572 n` | `party.members`, records `standby_count + i` |
| held items (`set_held_items`) | `7578 + 18 i` | `member.held_items` |
| bag (`set_bag`: capacity, ids, quantities) | `7652`, `0x1d4` bytes | `party.bag` |
| gold (`set_gold_on_hand`) | 11448 | `party.gold` |

The maxima are stored by the format: `Save_UnpackMain` copies `+0x14c` fields 7 and 8 into
the member's HP and MP, so a load restores the member **at full** HP and MP. The record's
**current** HP/MP are not saved. The base attributes (fields 0 to 6) are not written either:
the record holds the values **with the worn equipment's modifiers** (`func_02083e28`, e.g. the
demo's Sage 91 has 246 strength for a table value of 186), while the state's copy
(`apply_level_tables`) is the level table's base only, so writing it would corrupt a valid
record — the file keeps what it has, and a level gained in the port leaves those fields one
level behind **[limit]**. A maximum of 0 in the state means "not established yet" (a new
game's hero before the tables) and keeps the record's own value instead of zeroing it.

Kept **byte for byte**, because the state does not carry them: the standby count and the
party order (member object ids, not record indices), the per-vocation **seeds** (`+0x58`),
the record's `+0x14c` base attributes (fields 0 to 6), the bank, medals, visited places,
special guests, party tricks, unlocked vocations, the version string, and every block the
reader leaves opaque (`QuestLog`…`Suspend`, the 12 bytes at `+0x2c0` of the event block). An
empty equipment, held-item or bag place keeps the empty word it already had (`0` or
`0xffff`), so a write of unchanged state changes no byte.

A normal save carries **no hero position** (§ "Position on resume"): `from_slot` gives
`Placement::MapStart`, so the reload stands the party at the map's start record, as the
original does; only a suspend (intermission) save resumes at `Save_Warp`, and `write_into`
writes that block only for a state that holds one.

**The live state.** `GameState::from_play(&trigger::State, base: &GameState, map, clock, party)`
builds the state of a played session: the application's live trigger state supplies the event
block's tracks, global flags (indices outside the saved array are dropped), quest nibbles and
selector — the inverse of `dq9_core::field::saved_state` — and the live map, day clock and
party the rest; `base` is the state loaded from the input slot, which lends its placement and
the opaque bytes above.

**The CLI.** `dq9-app --save <in.sav> --save-out <out.sav>` (with `--headless-steps`,
`--auto`, `--replay`, `--walk`, `--play-record` or the window) writes at the end of the run:
the input's normal-write target is updated (`Save::prepare_normal_write`: the adventure log,
else slot 0 seeded from the suspend save, § "The suspend slot"), sealed (`SlotMut::seal`) and
mirrored when it was slot 0 (`Save::mirror_normal_slot`, § "Role of the two slots"), then the
whole 64 KiB container is written through a sibling `.tmp` file and a rename — a truncated
image can never appear under the requested name, and the input file is never modified (an
`--save-out` that names the input, through any spelling, is refused). Without `--save` the
command refuses: a fresh container's opaque blocks are not modelled, so they would be zeroed.
The run logs one line with the map, story, party and gold written and how many bytes differ
from the input.

**In game** the church's Confession writes the same container, mid-session, when the greeting's
1010 is answered "Yes" (the write is state 1's Yes; 1013 only asks whether to go on): the live
state through `write_into`, the slot `seal`ed, slot 0 mirrored, and the whole image through the
same temporary file and rename. The destination is `--save-out` when it is given and the
`--save` container **itself** otherwise, as the DS writes its own cartridge's slot; the input is
only replaced when no output was named. The slot updated is `Save::prepare_normal_write`'s (the
first valid normal slot, else slot 0 seeded from the suspend save): a Confession after a quick
save resume therefore writes a normal adventure log, as the original's does whatever slot was
loaded. Without `--save` the port has no container, so the entry shows its own reason
(a port line, not a ROM one) and writes nothing (`docs/re/facility.md`, § "The Confession";
`crates/dq9-app/src/app/facility.rs`, `App::confess`). A refused confession writes nothing, but a
window run with `--save-out` still writes the session's state when it exits, as it always did.

**The Quick Save** writes the suspend slot mid-session through the same temporary file and
rename: `GameState::write_suspend(&mut SlotMut, HeroPose)` (the state, the intermission header
bytes and the live pose's `Warp` block), `slot.seal()`, and the whole image out — slot 0 is
never written, so no mirror step. The destination is `--suspend-out` when it is given and the
`--save` container itself otherwise; without `--save` the entry says so and writes nothing
(`App::write_suspend_out`, `App::quick_save`, § "The suspend slot"). The command line needs no
new run mode: the option only names the destination, and the write happens when the field
menu's Misc. → Quick Save is answered "Yes".

**Tests** (`dq9-core` state module; `dq9-app` `app/headless.rs`): a state read from a save and
written back is byte-identical, including the demo (`DQ9_SAVE`); a mutated state — map, story,
clock, gold, bag, a member's name, vocation, level, experience, weapon and maxima — changes
only bytes inside the ranges `write_into` reports (plus the three checksum words); the app's
write, checked against the same ranges on a synthetic mirrored save, stays inside them and
its mirror copy; `from_play` round-trips through `field::saved_state`; a party that would fall
outside the 13 records is refused; a real ROM + `DQ9_SAVE` headless run (bag and a maximum
mutated) reloads to the state the run held. Evidence: `build/evidence/v3-save-bind/`. The
suspend writer's own tests: a normal save's slot 0 stays byte-identical while slot 1 becomes
an `Intermission` slot whose `Warp` block holds the given pose and whose reload
(`GameState::from_slot`) gives back the same map, story, party, gold, clock, flags and
`Placement::At`; a second write over an existing intermission slot still writes the `Main`
block's map word (`dq9-core` state module); the app's write on a synthetic mirrored container
keeps slot 0 and the input file byte-identical and its reload lands at the live hero's
position (`app/headless.rs`); the Misc. list is the ROM's and only Quick Save answers
(`app/services/tests.rs`). Evidence: `build/evidence/v3-suspend-save/`.

The suspend-only write's tests (`dq9-formats` `save` module; `dq9-app` `app/headless.rs`): a
container with only a suspend save is seeded from slot 1 (`prepare_normal_write` returns slot
0, copies header A on, leaves the first 16 bytes, and is idempotent), a container with an
adventure log keeps its own bytes instead, and a fresh container has no target; the app's
`--save-out` on a suspend-only container writes a `Valid` slot 0 with its checksums, a `None`
`Warp`, slot 1 as its mirror, the same bytes on a second write, and a reload
(`GameState::from_save`) equal to the state written — map, story, party, gold, clock, flags and
`Placement::MapStart`, with the opaque bank and medal words taken from slot 1; a
normal+suspend container still updates the normal log without mixing the suspend image in.
Evidence: `build/evidence/v3-suspend-saveout/`.

**DS acceptance: the original loads the written container.** melonDS with the written file as
`dq9.sav` shows its adventure log on the title screen (the four members with their vocations
and levels, play time, "Stornway") and continues into Stornway's church with that party on the
map (`build/evidence/v3-save-bind/melonds-title.png`, `melonds-continue.png`): the header, the
three checksums and the blocks are accepted by the game's own loader. It does not prove the
meaning of every written field, nor a mutation made in game and saved from the DS menu.

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
- The write protocol is understood (§ "Role of the two slots") and its **file** effect is
  ported (`Save::mirror_normal_slot`); the hardware side (`Save_BackupWrite`, the lock and
  wait states) is not, hence no `port:` marker.
- Number and order of skill allocations (`+247`).
