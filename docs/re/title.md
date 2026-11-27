# Title screen (`ov_004`)

What the game's own title screen does, from the decompilation, the melonDS notes and the save
layout: the overlay, its boot state, the slot check that ranks the entries, and what each entry
loads. The port's side is [`app/title.md`](../app/title.md); the save mechanics are
[`save.md`](save.md), § "Which slot a start-up takes" and § "The suspend slot".

## The overlay and the boot state

`ov_004` is the title: a melonDS state in game holds `ov_003`, a state on the title holds
`ov_004` (`melonds-state.md`, § "Useful addresses"). The DS boots into it (direct boot, free
BIOS): the state at the title has the static ARM9 module and ITCM identical to the ROM, and no
member object matches a save, so the title has not loaded one. Reading the adventure log for
the Continue row's display already sets scenario 19/2/1 on the title
(`melonds-state.md`, § "Correction") — the rows' data is read before a slot is loaded.

## The slot check: `ov_004::0216dad0`

The function calls `Save_SlotStatus` (`0x020ab9d0`) on slot 1 and slot 0 and builds an entry
code. The European decompilation reads (quoted from the note in `save.md`, § "Which slot a
start-up takes"):

- **status 4 (intermission) on slot 1 first** → entry code **0**, the quick-save entry; it also
  records whether slot 0 is **empty** (`*puVar1 = 1` when slot 0 is status 3);
- status 1 (unreadable) → **6**;
- status 2 (bad header checksum) on slot 1 → **7**, on slot 0 → **4**;
- status 0 (valid) on slot 0 → **1**, Continue, the row whose adventure log the title reads;
- status 3 (empty) → **2**, the new-game entry.

A present suspend save therefore ranks **above** a normal one in this check, and slot 1's
mirror copy of a normal log is not a title entry of its own (only slot 0's valid status gives
Continue). The port's `Save::title_slot` (`dq9-formats/src/save.rs`) reproduces exactly that
ranking, with one addition: a slot must also pass the three checksums, because
`Save_LoadSlot` (`0x020aaf94`) rejects it otherwise.

**The menu consumer is [unknown].** The function that reads the entry code and builds the
title menu is not in the decompilation corpus; nothing says which rows the menu shows when the
code is 0 *and* slot 0 holds a log (the two possible entries), nor their order. The port has
one Continue row and ranks the suspend save above the normal log, as the check does, and does
not model the quick save's separate entry or the corrupted-log refusals (codes 4, 6, 7).

## What each entry loads

Both entries end in `Save_LoadSlot` (`0x020aaf94`), whose **only selector is its intermission
argument** (the second argument is 0 in every caller and unused in the body):

| Entry | Handler | Call | Boot |
|---|---|---|---|
| Continue | `ov_004::0216d504` | `Save_LoadSlot(buf, 0, 0, 0)` | `GameState+4` stays 0 |
| Load quick save data | `ov_004::0216e490` | `Save_LoadSlot(buf, 0, 1, 0)` | `GameState+4` becomes 5 |

`GameState+4` is what the field's entry (`ov_017::0218b688`) switches on: 5 loads slot 1, 0
loads slot 0. The two loads therefore differ only by the slot argument. The suspend load unpacks
the `Warp` and `Suspend` blocks, so the party stands at `Save_Warp+0x10` with the angle `+0x1c`
(`Party_Place`, `ov_017::0219c598`); a normal save has no warp block and starts at the map's
`start` point (`save.md`, § "Position on resume").

## Deleting the quick save log

`ov_004::0216e490` writes header A of slot 1 after a good load (`Save_WriteHeaderA`,
`0x020aba6c`, gated on the menu's confirmation byte `*pcRam0216e5e0 == 1`), and a separate title
step does the same unconditionally (`ov_004::0216e414`, message `0xd4` on failure). The log is
deleted **at the resume**, not by the next normal save; `Save_WriteHeaderA` empties the 0x14
header A bytes with the version string kept, and `Save_SlotStatus` then reads the slot as
empty (`SlotMut::clear_header`). A new game writes an empty header A into **both** slots and
clears their header C (`func_020abb74`).

## Evidence

- The ranking and the entry codes: `save.md`, § "Which slot a start-up takes" (read from
  `ov_004::0216dad0`, `ov_004::0216d504`, `ov_004::0216e490`, `0x020aaf94`); the quotation is
  kept there because the function was read in the decompilation corpus, not re-decompiled for
  this note.
- The boot state: `melonds-state.md`, § "Useful addresses" (`ov_004` = title screen, `ov_003` =
  in game) and § "Correction" (the title reads the log, scenario 19/2/1).
- The resolved UI facts stay **[hypothesis]**: the menu's own rows and their order are
  **[unknown]** (see above), and the title's graphics are not decoded.
