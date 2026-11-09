# Scenario triggers (`data/scenario/trigger<map>.bin`)

Implementation: `crates/dq9-formats/src/trigger.rs` (format: `parse`, `Trigger::loaded`,
`Trigger::talk`, `action_name`) and `crates/dq9-core/src/trigger.rs` (state and execution:
`first_match`, `fire`, `run_action`, `run_deferred`, `State`); the map region events
(2, 3, 5, 6, 17, 20) are fired by `crates/dq9-core/src/field.rs` (see "Zones"
below). Tool: `cargo dq9re talk`
fires the map's conversation triggers, runs their actions, then picks the
line (`dialogue.md`).

"If conditions then actions" rules fired by game events. They are what
set the **conversation context** 192–223 that half of the NPC lines expect
(quest offers and rewards), launch scripted events (`stb.md`), advance
the scenario, give items and gold.

## Loading (`Trig_LoadMap`, `0x0206462c`)

When a room loads: file `data/scenario/trigger%s.bin`, `%s` = first three
characters of the map (`M02`), or `F` alone for field maps `F*`. It is a table
of records (`table.md`) read by `Tbl_*` with `g_TrigRecordHandlers` (`0x020f05bc`):
types 100 and 101 have no effect, type 1 = a trigger (`Trig_LoadRecord`, `0x0205f9dc`):

```
room, chapter_min, step_min, chapter_max, step_max, event, words…
```

It is kept only if `room` is the current room and
`chapter_min*1000 + step_min <= chapter*1000 + step <= chapter_max*1000 + step_max`
(`Save_Chapter`, `Save_ScenarioStep`, i.e. the progress of the current track,
see below). `Trig_Register` files it in the chain of its event type, in
file order.

76 files, 5,895 triggers, all type 1 (test `trigger_files_compile`).

## Scenario tracks

The quest manager starts with five blocks of `0x1c` bytes:
`[chapter, step, sub-step, flags 0 (13 bytes, +3), flags 1 (12 bytes, +0x10)]`.
Byte `+0x332` selects the current block; `FUN_02064ba8` sets it from the room on
entering a map then copies its progress into the save (`+0x5cb0..+0x5cb8`,
`Save_Chapter`…):

| Track | Rooms (maps) |
|---|---|
| 1 | 4200–4202 (X02), 9000–9008 (T01) |
| 2 | 1700–1706 (M07), 1800–1808 (M08), 6000–6001 (S10), 7700–7709 (D07) |
| 3 | 200–219 (C02), 7802–7809 (D08) |
| 4 | 2100–2109 (M11), 8301–8303 (D13) |
| 0 | all others |

**[hypothesis]** These are the scenario arcs the player can follow in any
order: each keeps its own progress.

A track's progress never goes backwards (`FUN_020703d8` compares
`chapter*10000 + step*100 + sub-step`). A chapter change clears both flag
sets of the track (`FUN_0206e090`), a step change clears set 1 (`FUN_0206e0e0`).
The "per-player flags" of earlier notes are therefore **track-local
flags**: set 0 holds for the current chapter, set 1 for the current step
(conditions 2–5, actions 0x66–0x69, 0x86/0x87).

## Events

`Trig_Fire(manager, event, parameters)` (`0x020649c0`): first valid trigger of
the chain, then its actions; deferred actions are only run
(`Trig_RunDeferred`, `0x0206f82c`) if the list ran to its end. The parameters are
a block of words compared by conditions 6 to 0xC (words 0 to 6), 0x1E (word 8), 0x4B/0x4C
(word 9), 0x1D (words 7 and 10) and 0x20/0x21 (bytes `+0x2a..+0x2d`).

Each caller passes a constant (read from `r1` before each call):

| Event | Triggers | Fired by | Parameters |
|---|---|---|---|
| 0 | 2,831 | conversation start (`Talk_Start`) | 0 = NPC, 5 = conversation zone |
| 1 | 1,143 | conversation end (`TalkTask_Update`, closing states); wired: `Field::talk_ended` | 0 = NPC, 5 = conversation context (the one that picked the line), `+0x954` = answer to the last `<YESNO>` (0 without question) |
| 2 / 5 | 110 / 0 | the leader enters / leaves a zone (`ov_017::0219814c`, `02198e30`) | 1 = zone: map object of type 3 or zone added by action 0x8f |
| 3 | 249 | arrival in a room after a transition (`ov_017::0219e384`, `0219fc88`, via `FUN_02064a04`) | 3 = room |
| 6 | 35 | the leader enters the box of a `0x72` warp (`Field_CheckWarps`, `ov_017::0219ca88`): the transport is ready (`Save_Warp`), the trigger runs before the map change | 3 = room left |
| 9 / 19 | 0 / 11 | `FUN_02064a50` (local character) / `FUN_02064aac` (leader), called by `FUN_02053644` when a character's counter reaches its target | 1 = current zone; bytes `+0x2a..+0x2d` = four bytes of the character (`+0x178`) **[meaning unknown]** |
| 11 | 669 | end of a scripted event (`ov_017::021bc77c`) | 2 = event number |
| 15 / 16 | 47 / 33 | end of a scripted battle (`BattleTask_Update`, `ov_017::021b790c` → `Trig_FireBattleEnd` `0x02064a18` / `Trig_FireBattleEndAlt` `0x02064a34`): 15 when won, 16 when byte `+0x8e14` of the battle structure is 2 (defeat); wired: `Field::battle_ended` (§ "Scripted battles") | 6 = battle number (field 0 of `data/event/eventbattle.bin`: all 40 values appear there) |
| 17 | 504 | type-2 doorway touched: the leader walked 3 frames against its orientation, a fourth takes it (`Field_TouchDoorways`, `ov_017::02198f84`, list `+0x36bc`) | 7 = (class `+0x2c`, number `+0x2d`), 10 = orientation `+0x20` |
| 20 | 195 | end of the room's trigger loading (`Trig_LoadTable`) | 3 = room |
| 22 | 41 | overlay 2 (`ov_002::02155960`), maps H18 and M08 **[meaning unknown]** | — |
| 23 | 13 | `ov_017::02199138`, map S08 **[meaning unknown]** | 3 = global value, 9 = first object of `+0x36bc` |
| 27 | 12 | `ov_017::021a9bf8`, after the quest log **[meaning unknown]** | — |
| 30 | 2 | same task as event 3, M11 | 3 = room |
| 10, 12, 18, 24, 25, 26, 29 | 0 | overlays 1, 3, `ov_017` (wireless sessions for 26/29) | — |

Findings on the data: event 1 carries the follow-ups of lines (actions 0x72/0x73
items, 0x77 event, 0x7d/0x7f quests, conditions 0xB on the context and 0x10 on the
yes/no answer); event 20 prepares the room (0x95/0x96 objects, 0x8f zones);
event 17 marks the object (0x6c) then makes someone talk.

## Zones: areas, doorways and warps (`dq9_core::field`)

Events 2, 5, 6, 17 and 20 do not come from conversations but from the map's
regions (`.bmbl`, `map-archive.md`, "Doors, zones and map changes") and from arrival
in a room. `Field::step` evaluates them at each fixed step with the leader's position;
`Field::enter` plays 20 then, after a transition, 3. Triggers are reread from the file at
each event, filtered by `Trigger::loaded` against the progress of the room's track
(`FUN_02064ba8` picks it on arrival, `track_of_room`). A trigger that yields a transport
(actions `0x79`/`0x7a`/`0x7b`…) becomes a map change; the other requests
(`Request::Talk`, `Event`…) are handed to the application, which logs them up to J3.

**Map areas** (type-3 zones, `Area_CheckLeader`, `ov_017::0219814c`): look for
the first area in the list (most recently defined first) that holds the leader
(`Zone_Holds`). If the area changes, event 5 with word 1 = number of the area left, then 2
with that of the area reached. The game stores the area (pointer) on the leader's object; two
areas with the same number are therefore distinct. 22 in the ROM, 15 maps.

**Areas added by action `0x8f`** (`Trig_RunAction`, case `0x2b`, `Trig_AddArea`
`0x02064b08`): `0x8f(n)` then top corner (x y z), bottom corner (x y z), angle in degrees. 0x30-byte
structure added **at the end of the list**: `+0` n, `+2` angle `degrees × π / 180 × 4096`
truncated to 16 bits then brought into `[0, 2π]` (`0x6488`), `+4` centre = (top + bottom) >> 1,
`+0x10` radius² = (width/2)² + (**height**/2)² (not the depth: a tall, thin
rectangle rejects its ends when rotated), `+0x14` top corner, `+0x20` bottom corner. The
floats are rounded (half unit away from zero). `ov_017::02198e30` tests the leader in
each, the first that fits wins; the current area is `+0x491` of the triggers
object (−1 = none), event 5 carries the area left in word 1, 2 the area reached, and
`+0x4446` of the field manager stores the last one reached (read by actions `0xd1` and `0xd2`). The 116 occurrences
are mostly in event-20 records (80 according to the wiki). On the Angel Falls
prologue (1.1), event 20 of `M01` adds area 10 and no zone trigger
responds to it.

**Doorway touched** (event 17, `Field_TouchDoorways`, `ov_017::02198f84`): word 7 = class
`+0x2c` (low half) and number `+0x2d` (high half), word 10 = orientation `+0x20`
(condition `0x1d` compares them: arg = class, operand 0 = number, operand 1 =
orientation in degrees). If the trigger set a transport, it replaces the destination,
arrival position and orientation of the doorway; otherwise the doorway goes to its own. A
trigger that stops (action with no case) abandons its deferred actions: the doorway keeps its
destination.

**Arrival**: the loaded room plays event 20 (end of trigger loading: word 3 =
room, this is where triggers add their areas and mark their doorways), then, after
a doorway or a transport, event 3 (word 3 = room).

**Check against a save**: in Stornway (`C01`, 19.2, night), event 20
(condition `0x11(1)`: night) plays `0x6c` for numbers 24 and 22; the RAM of a melonDS state
shows exactly these two type-2 doorways (zones 12 and 13, the two entrances of `C01M14`) with
flags 12 and 14 instead of 8 and 10, i.e. bit `0x4` (`dq9re doors --state`, test
`stornway_at_night_marks_its_two_locked_doorways`). The church doorways (numbers 18 and 20)
stay free, and the hero takes them.

**Not modelled**: the doorway animation and automatic walking, the guards of
`ov_017::0219755c`, zones of other types, and the application by the window of map
objects `0x6d`, `0x99`… For `0x95`/`0x96`, the emission is done (`Request::SetPlacementHidden`,
see the actions table); applying it to rendering and collisions belongs to
`dq9-app`.

## Words (`Trig_CompileWords`, `0x0205ec80`)

Each word is `code << 16 | arg` and is followed by a fixed number of operand fields, specific
to the code (table in `dq9_formats::trigger::operand_count`). Codes 100 to 499 are **actions**, the
others **conditions**; the compiler splits them into two lists. Check: the words of the
5,895 triggers end exactly at the end of their record.

Compiled nodes store the argument and the operand halves in half-words; the
layouts, read from the two jump tables (`0x0205ed1c` conditions,
`0x0205f1f4` actions), are reproduced by `dq9-core/src/trigger.rs`. Notable cases:

- 0x1D and actions 0x6c/0x6d/0x88/0x98–0x9a: the second operand is an angle in degrees,
  converted to `(π·x/180 brought once into [0, 2π]) × 4096` truncated;
- 0x32/0x52: one word yields two nodes (see conditions);
- 0x4A: the argument is lost, only the low half of the operand is kept;
- 0x7a/0x7b: floats × 4096 rounded (positions, direction); 0x8b/0x8c: 32-bit integer
  operand;
- 0x8f: seven floats (zone).

## Selection (`Trig_FindMatch`, `0x020644a0`)

For an event, the triggers of its chain are tried in order; the first whose
conditions are **all** true (evaluated in order, stopping at the first false) is
kept. Two quirks preserved: the equipment mask of 0x32/0x52
(`+0x492`) is only reset once per event, and the "all true" boolean
is not reinitialised between two triggers (no effect on the data: no trigger has
zero conditions). If no trigger fits, the game follows its default behaviour
(conversation: context = zone).

## Conditions (`Trig_EvalCondition`, `0x0205fb04`)

All conditions present in the data are reimplemented (test
`trigger_conditions_are_modelled`, `dq9-core/tests/real_rom.rs`); only 0x2C, 0x45, 0x4F, 0x53 and 0x5B, absent from the data,
read an unmodelled state (`dq9_core::trigger::UNMODELLED`). "Character `n`" = entry `n` of the
character table (`FUN_0200fd80`, 0 = the hero), "party" = list at `+0x397c` of the
save (the first is the leader).

| Code | True if |
|---|---|
| 0 / 1 | scenario flag `arg` set / cleared (bit array `+0x8c` of the quest manager) |
| 2 / 3, 4 / 5 | flag `arg` of set 0 / 1 of the current track set / cleared |
| 6…0xC | word `code-6` of the parameters == `arg` |
| 0xD / 0xE / 0xF | number of characters 0 to 3 present ≥ / ≤ / == `arg` |
| 0x10 | `+0x954` of the message window == `arg`: **answer to the last `<YESNO>`**, 0 = yes, 1 = no (M05, NPC 7: the "no" branch gives the hint and trigger 0x10(1) registers quest 201) |
| 0x11 | `arg` = 1: night, otherwise day (`+0x3dc` zero at night) |
| 0x12 / 0x13 | the party has / does not have item `arg` |
| 0x14, 0x16, 0x19, 0x1C | quest `arg` in state 2, 3, 1, 0 (and allowed to a guest) |
| 0x15 | bit 2 of quest `arg` |
| 0x17 | session mode `arg`: 0 solo, 1 session, 2 solo or peer flag zero, 3 session and peer flag |
| 0x1A / 0x1B | extended flag (`arg >= 0x400` shifted by `0x6fa`) set / cleared |
| 0x1D | word 7 == (`arg`, operand 0) and word 10 == angle of operand 1 (object of event 17) |
| 0x1E | word 8 == `arg` |
| 0x1F | signed byte `+0x490` of the manager == `arg` **[meaning unknown]** |
| 0x20 / 0x21 | operand bytes (high first, 0 = don't care) == bytes `+0x2a..+0x2d` of the parameters, in order / each in a distinct place |
| 0x22 | `arg` ≠ 0: at least two members in the party; `arg` = 0: fewer than two |
| 0x23 | track sub-step == `arg` |
| 0x24 | hero's HP (`+0x130 → +4`) zero if `arg` ≠ 0, non-zero if `arg` = 0 (see below) |
| 0x25 / 0x26 | quest bit in the log's `+0x84` array set / cleared **[meaning unknown]** |
| 0x27 / 0x28, 0x43 / 0x44 | flag `arg + 0x776`, `arg + 0x1136` |
| 0x29 | the leader is (`arg` ≠ 0) / is not (`arg` = 0) in a conversation zone of the NPC of word 0 (NPC rectangles, strict bounds, like `Talk_NearestNpc`); false if the NPC is not on the map |
| 0x2A | hero's vocation (`+0x150 → +0x950`) == `arg` (0 Minstrel, 1 Warrior… 12 Ranger, `str_sta` menu order) |
| 0x2B | bits 19–29 of save `+0x56a0` == `arg` **[meaning unknown]** |
| 0x2D / 0x2E | six 10-bit fields of the hero (`+0xc`, `+0x10`, `+0x14`): field `arg` is the largest / exceeds (or not) by a limit the field of `+4` **[meaning unknown]** |
| 0x2F | number of the leader's slots (11 × 32 bytes at `+0x194`) whose bit 18 of word `+8` is set, compared to the operand |
| 0x30 | at least `operand` copies of item `arg` |
| 0x31 | a party member (status bit 0 clear, HP non-zero) has vocation `arg` and at least level `operand` in that vocation |
| 0x32 / 0x52 | equipment: node 1 (arg, operand 0, high half of operand 1; < 1 = don't care) compared to slots 0, 1, 5, 6 of each member (0x32) or of the local character (0x52), accepted members go into the mask; node 2 (low half of operand 1, operand 2, low half of operand 3; 0 = don't care) compared to slots 7 to 10 of the masked members |
| 0x33 | the log entry of quest `arg` has its bit 15 **[meaning unknown]** |
| 0x34 | NPC == `arg`, flag `high` of the track's set 1 cleared, session mode `low` |
| 0x35 | NPC == `arg`, quest condition, zone == `high` of operand 1, answer (0x10) == `low` |
| 0x36 | NPC == `arg`, quest condition, the party has item `high` of operand 1 |
| 0x37 | NPC == `arg` and quest condition (`high` = quest, `low` = condition encoded like dialogue `b`, `Trig_QuestCondition`) |
| 0x38–0x3D | 0x37 then: 0x24 (0x38), extended flag and/or session mode (0x39–0x3D) |
| 0x3E / 0x3F | NPC == `arg`, flag `high` set / cleared, session mode `low` |
| 0x40 / 0x41 | up to four flags (`arg` and three operand halves, negative = absent) set / cleared |
| 0x42 | more than `low` copies of item `high` if `arg` ≠ 0, at most `low` otherwise |
| 0x46 | bit 18 of the entry of quest `arg` == (operand ≠ 0) |
| 0x49 | signed half-word `+0x2ca` of the manager ≥ operand if `arg` ≠ 0, < otherwise (reset to 0 by action 0xbb) |
| 0x4A | save half-word `+0x71dc` == operand |
| 0x4B / 0x4C | word 9 == operand |
| 0x4D | flags 0x364 and 0x365 set |
| 0x4E | value of the log entry of quest `arg` (field `high`, half-words `+4..+0xe`) compared to `low`: 0 ≥, 1 <, 2 == (mode = operand 1) |
| 0x50 | a member (status bit 0 clear, HP non-zero) has at least level `arg` in their current vocation |
| 0x51 | session active if `arg` ≠ 0, otherwise inactive |
| 0x54 | save byte `+0x64de` == `arg` |
| 0x55 | gold (`+0x3970`, capped at 9,999,999 by action 0x8b) < operand if `arg` = 0, ≥ otherwise |
| 0x56 | the first of characters 0 to 3 whose byte `+0x56b` has a non-zero low nibble has non-zero HP (with no such character: character 0xce exists): `arg` = 1 if yes, 0 if no |
| 0x57 | quest log: fewer than 8 quests if `arg` = 0, full (8) otherwise |
| 0x58 / 0x59 | flag `arg + 0x33e` set / cleared (`arg` < 0x49) |
| 0x5A | console clock date == (month `arg`, day operand): 24 and 25 December in the data |
| 0x5C | bit 9 of the entry of quest `arg` clear |
| 0x5D / 0x5E / 0x5F | bit `arg` of `+0x238c` / word `+0x237c` / half-word `+0x27b4` of a global structure (`FUN_02012ff4`) == `arg` **[meaning unknown]** |
| 0x60 | at least `arg` vocations at level 99 for the local character |
| 0x62 / 0x63 | global byte `+0x27d0` ≤ / ≥ `arg` |
| 0x1F4 | the first member whose high nibble of `+0x56b` equals `arg` has status bit 0 clear if operand = 1, set if 0 |
| 0x1F5 | `+0x3dc` (time of day) == `arg` (0, 2, 3 in the data) |
| 0x1F6 | global byte `+0x23a1`, reset to zero by the read **[meaning unknown]** |
| 0x1F7 | flag 0x798 set if `arg` ≠ 0, cleared otherwise (written by action 0xe0) |
| 0x1F8 | clock date == date stored at `+0x569c` (month bits 12–15, day 16–20): "already done today" **[hypothesis]** |
| 0x1F9 | bit 0 of save `+0x63dc` |
| 0x1FA | bit 3 of quest `arg` |
| 0x1FB | destination room (`+0x3f8`, written by transport actions) == `arg` |
| 0x18, 0x47, 0x48, 0x61, others | always false |

### The "hero word": hero's HP

0x24, 0x38 and lines of types 5/6 (`dialogue.md`) read the half-word
`character 0 → +0x130 → +4`. It is the hero's **HP** **[supported hypothesis]**:

- action 0xcb only touches members whose half-word is 0 and writes it with
  `+0x134 → +0x30` (max HP), half of it or 1 depending on `arg`: a resurrection;
- 0x31 and 0x50 only count members where it is non-zero;
- lines that require it to be zero address another member: the bath keeper in
  Gleeba (C02, NPC 103) says "you who saved our monarch" when it is non-zero, "you,
  friend of `<HERO>`… Be thankful that you know such a great one as `<HERO>`" when it is
  zero; 12% of type-5 lines are silences `*: ......` (1% for type 1),
  which suits spirits that only the hero hears.

## Actions (`Trig_RunAction`, `0x02061c14`)

`Trig_RunActions` (`0x02064540`) runs the list in order and stops at the first
action that returns 0; **deferred** actions are appended to list `+0x30` of the
parameter block (`Trig_Defer`, `0x0206446c`) and are only run by `Trig_RunDeferred` if
the list ran to the end. `dq9_core::trigger::run_action` reproduces the flow and the effects
on `State` (flags, tracks, quests, log, gold, items, time of day, conversation
counters); effects on what `State` does not hold (map objects, NPCs, menus, sound,
wireless sync, HP) are ignored.

`high` / `low` = halves of operand 0 unless stated.

| Code | Effect | Occurrences |
|---|---|---|
| 0x64 / 0x65 | sets / clears flag `arg` | 429 / 330 |
| 0x66 / 0x67, 0x68 / 0x69 | sets / clears flag `arg` of set 0, 1 of the track | 57 / 8, 251 / 21 |
| 0x6a | resets both conversation counters of NPC `arg` | 55 |
| 0x6b | session host: flag 0x113a | 53 |
| 0x6c / 0x88 | sets / clears the "used" bit (`0x4` of the flags) of type-2 doorways of class `arg`, number operand 0 and orientation operand 1 (degrees); a used doorway is no longer touched; `0x6c` does not act on a doorway with bit `0x1` (`Trig_RunAction`, cases 8 and `0x24`). Observed in RAM ("Zones") | 502 / 28 |
| 0x6d | removes the map object | 179 |
| 0x6e | time of day (`+0x3dc`) = `arg` (< 4) | 47 |
| 0x6f–0x71, 0x98, 0x9a, 0xa3, ≥ 0xea | no case: **stops** the list | — |
| 0x72 / 0x74 † | gives item `arg` (1 / `high` copies, with message) and sets its flag `0xc76 + compendium` on the first copy (`func_0207cc18`, [`item.md`](item.md)) | 378 / 4 |
| 0x73 / 0x75 † | removes item `arg` (1 / `high`), bag first then equipment | 163 / 26 |
| 0x76 † | conversation with NPC `arg`, context `high` | 3,378 |
| 0x77, 0x80 † | launches event `arg`: the 524 values of 0x77 are `data/event/evNNNNN.gp2`, `data/evspt_lv5/evNNNNN.gp2` or entries of `eventlist6.bin` / `evl_quest.bin` (`stb.md`) | 622 / 0 |
| 0x78 † | launches scripted battle `arg` (record at index `arg` of `data/event/eventbattle.bin`), § "Scripted battles" | 65 |
| 0x79, 0x89, 0xe1 † | transport to room `arg` (structure `+0x3f8` of the save) | 163, 0, 22 |
| 0x85, 0x8a, 0xe2 † | same with an **arrival event** `high` (`Save_Warp+0x20`, `Trig_RunDeferred` case 0x85: `*(u32*)(warp+0x20) = operand`) | 30, 1, 5 |
| 0x7a / 0x7b † | position (3 floats × 4096) / direction of the transport | 179 / 33 |
| 0x7c † | removes NPC `arg` (flag 0x8000 of its object) | 39 |
| 0x7d | registers quest `arg` in the log; **stops** the list if the log is full or the quest is not offered; flag 0x119d | 258 |
| 0x7e | if quest `arg` is in state 2: quest window `+0x3bbc` **[meaning unknown]** | 1 |
| 0x7f | quest `arg` completed: date stored (log `+0x178`), state 3, removed from the log **[hypothesis for the removal]** | 209 |
| 0x81 | quest `arg` in state 1 (offerable), bits 2–3 kept | 2 |
| 0x82 / 0x83 | sets / clears extended flag `high` (`arg` = quest, for sync) | 294 / 148 |
| 0x84 † | advances the current track to (chapter, step, sub-step) = low halves of the operands; before deferring, clears its flags if chapter or step differ | 217 |
| 0x94 † / 0xd6 † | same for the five tracks / for track `arg` | 2 / 12 |
| 0x86 / 0x87 | sets / clears flag `arg` of set `high` (3 scenario, 4 set 0, 5 set 1 of the track) | 59 / 0 |
| 0x8b / 0x8c | adds / subtracts the operand (32 bits) to gold, cap 9,999,999, floor 0 | 13 / 0 |
| 0x8d † | if `arg` ≠ 0: queues task 0x30 (`+0x3b58`), which **rebuilds the NPC cast** of the current map, after the trigger reload and event 20 (§ "Reloads after deferred actions") | 236 |
| 0x8e | flag `0xbf1 + rank` of `arg` in a table of 32 values (`0x020e87d0`), ranks 17 to 31 only | 14 |
| 0x8f | adds an area (events 2/5), see "Zones"; runs as the list goes, not deferred | 116 |
| 0x90 | bits 11–13 of the entry of quest `arg` = `high` (quest in state 2, not a guest) | 155 |
| 0x91 † | window chosen by `arg` **[meaning unknown]** | 49 |
| 0x92 / 0x93 | flag `arg + 0x776` | 0 |
| 0x95 / 0x96 | hides / shows the map object **immediately**: block = `arg & 0xff`, placement = high half of operand 0. `Trig_RunAction` case 0x31/0x32 (`0x02061c14`): `func_02019508(manager, arg, operand)` then `func_02013380(object, arg, 0/1)`; if the object is not loaded, `func_0206ea8c(manager, arg & 0xff, (operand + 0x80) >> 16, …)` records the state in `+0x41b`. Emitted as `Request::SetPlacementHidden { block, placement, hidden }`, in list order and before deferred actions. `0x95(0, 0x100000)` = block 0, placement 16, hidden (tower `X01M01`) | 342 / 97 |
| 0x97 | window `+0x3b60` with `arg` **[meaning unknown]** | 23 |
| 0x9b † | flag `high` of set 1 then launches event `arg` | 14 |
| 0x9c † | completes quest `arg`, takes back item `high`, gives item `low` | 15 |
| 0x9d † | quest `arg`: registration (`high` = 0), completion (2) or offer (3), then gives item `low` | 28 |
| 0x9e / 0x9f † | defers 0x75 / 0x74 with the same values | 0 |
| 0xa0, 0xca / 0xaf | sets `arg + 0x113f`, `arg + 0x1198` / clears `arg + 0x1136` | 6, 6 / 7 |
| 0xa2 | flag 0x113b = (`arg` = 0) | 6 |
| 0xb0 | bit 17 of the entry of quest `arg` = `high` | 27 |
| 0xb1 | time of day = `high` | 16 |
| 0xba | flag 0x1138 = (`arg` ≠ 0) | 4 |
| 0xbb | manager `+0x2ca` = 0 (condition 0x49) | 7 |
| 0xbc | value `high` of the entry of quest `arg`: = / + (cap 0xffff) / − (floor 0) `low` depending on operand 1 | 29 |
| 0xbe / 0xbf | random log values; **stops** the list if the quest is not there | 0 |
| 0xc5 | save `+0x5cbc` = `arg` **[meaning unknown]** | 178 |
| 0xcb | revives members at 0 HP: max HP (`arg` = 0), half (1) or 1 | 2 |
| 0xce | flag 0x79a | 1 |
| 0xd3 | sets bit `arg` of `+0x238c` (condition 0x5d) | 0 |
| 0xd8 | host: `+0x27b4` = `arg` (condition 0x5f) | 13 |
| 0xde | `+0x27d0` = 0 (conditions 0x62/0x63) | 3 |
| 0xdf / 0xe0 | flag 0x799 / 0x798 = (`arg` ≠ 0) | 2 / 4 |
| 0xe6 | flag `arg + 0xbea` = (`high` ≠ 0) | 1 |
| 0xe8 | conversation type of NPC `arg` (bits 3–5 of byte 10 of its definition) = `high` | 1 |
| 0x99 | sets bit 0x100 of the map object (type, number, angle) then removes it | 13 |

† deferred. The other codes present in the data (0xa1, 0xa4, 0xa6, 0xa7, 0xa9,
0xb2, 0xb4, 0xb9, 0xbd, 0xc7, 0xcc, 0xcd, 0xcf, 0xd0, 0xd4, 0xd5, 0xd7, 0xda–0xdd, 0xe3,
0xe5, 0xe7, 0xe9) touch windows, sound, wireless sessions, equipment or
unidentified structures; they do not change the flow of the list, except 0xdb (returns
the result of an item operation, not modelled). Absent from the data, 0xc6 and
0xd1 stop it depending on the number of players in a session.

Deferred, in list order (`dq9_core::trigger::run_deferred`): items and progress are
applied to the state, the rest is returned as requests (`Request::Talk`, `Event`,
`Warp`, `RemoveNpc`), then `ReloadTriggers` and `ReloadCast` (next paragraph).

### Reloads after deferred actions

**Found** by reading `Trig_RunDeferred` (`0x0206f82c`) then the functions it calls
(Ghidra, EU addresses). After the loop over the list, in this order:

1. **Triggers.** Cases 0x84, 0x94 and 0xd6 call `Track_Advance` (`0x020703d8`) then
   set a boolean **unconditionally**, whether the track advanced or not. 0x84 ignores its
   first argument and acts on the current track (`+0x332`); 0x94 advances tracks 0 to 4;
   0xd6 uses `a[0]`. When chapter or step change, `Track_Advance` resets all
   the conversation counters (`0x0206ec04`, `gameres+0x44c4`); see `dialogue.md`.
   After the loop, if `BackgroundLoader::GetNumQueuedTasks() < 1`, the function calls
   `Trig_LoadMap` (`0x0206462c`) on the current map; **otherwise no reload happens
   and nothing relaunches it later**. `Trig_LoadMap` zeroes `+0x47c` (head of the
   trigger table, `Trig_Register`, `0x0206440c`) and `+0x494` (the only list of areas, those
   added by 0x8f, `Trig_AddArea`, `0x02064b08`), rereads `trigger<map>.bin` filtered by
   the save's chapter and step (`Save_Chapter`, `Save_ScenarioStep`), then
   `Trig_LoadTable` (`0x02064584`) fires **event 20** (word 3 = room) and runs its
   deferred actions, still inside `Trig_RunDeferred`, hence before the
   0x8d task is queued. The area where the leader stands (`+0x491`) is not touched. `Trig_LoadMap`
   then calls `QuestLog_Get` (`0x02094d7c`) then `0x02095588`, which resets
   allocator `+0x164` and reloads `data/scenario/questorder3.bin` into it (string at
   `0x020f1474`, parse `0x0209563c` into `0x02109404`), filtered by map, chapter, step
   and completed quests: the map's quest list, **[not modelled]** in the runtime.
   No NPCs, placements, doorways or hero are rebuilt here.
2. **Cast.** Case 0x8d keeps its argument (the last one in the list wins); if it
   is non-zero and the game-resources task `+0x3b58` is not already queued (byte
   `+2`), `ov_017::021c16a8` initialises it (type `0x30`) and `func_020469b4` puts it in the
   task queue (`+0x36fc`), **behind** an event or conversation queued
   earlier in the same list. Its update `ov_017::021c16cc` (called by
   `ov_017::021a3544`): `SetBrightness(-16, 15)`, waits for the end of the fade and of
   loads, then `ov_017::021a2fa0`, then back to brightness 0 over 15 frames if no
   event task is queued. `ov_017::021a2fa0` is the function that map loading
   also calls (`ov_017::0219e384`), in the same order: `Trig_LoadMap` then
   cast rebuild. `ov_017::021a316c` destroys the cast allocators,
   the field monsters `0x70..0x9f` and empties the 32 entries of the NPC
   table (`0x0203cfc4`: cast list, placement chains, entries, count; the list
   of kept positions `+0x9e` remains), then `Cast_LoadMap` (`ov_017::021a2c14`) reruns
   `place.bin` and `npc.bin` for the current map and the current story point. The map, its
   collision, its placements (hidden or shown), its doorways and the hero stay; the
   `0x8000` flag set by 0x7c (`NpcTable_FindById`, `0x0203df88`) lived in the old table:
   a removed NPC that the placement script still places comes back. What the NPC snapshot
   (`0x0203e2d4` / `0x0203e0b0`) keeps is described in `characters.md`.

`EventTask_Update` (`ov_017::021bc77c`) also calls `ov_017::021a2fa0` at the end of an
event that took the cast memory (`+0x90`) or carries bit `+0x50 & 4`,
**before** event 11: the cast is rebuilt there at the old story point,
then the `0x8d` of event 11 rebuilds it at the new one. In that case the screen of task
0x30 differs: if `+0x3b58` is queued, `EventTask_Update` blacks out the screen
instantly (no 15-frame fade); light only returns if no
event task (`+0x3734`) is queued, and event `[9]` (time change) keeps
the screen black. **[not modelled]** in the runtime.

Example: `room 4104 1.1..1.1 event 11: if 0x8(21510) then 0x73(22159) 0x84(0, 1, 2, 1)
0xc5(2) 0x8d(1) 0x6a(139)`. At 1.1.2, `X01M04` only has the thing to examine 199; at 1.2.1,
`place.bin` also places Aquila (NPC 99, `s019`, `cargo dq9re cast X01M04 --step 2`), whom
`room 4104 1.2..1.3 event 0: if 0x6(99) 0x3(4) then 0x76(99, …) 0x66(4)` makes talk and
sets flag 4 of set 0. Another example: the end of 1340 in the stable (`0x8d(1)` after the
five piles are collected) no longer places piles 211–215 and puts stable boy 15 back on his feet.

Model (`dq9_core::field::Field`, `dq9-app`):

- `run_deferred` returns `Request::ReloadTriggers` for any 0x84/0x94/0xd6 and
  `Request::ReloadCast` for a non-zero 0x8d, in that order, after the transport.
  `Field::collect` performs the trigger reload itself: it empties the added
  areas, fires event 20 and processes its requests before the rest of the list. Since
  triggers are filtered at each event by the current track, rereading the file
  has no other effect here. The loading queue is assumed empty **[hypothesis]** (in the
  game, a non-empty queue suppresses the reload). `questorder3.bin` is not reread
  **[not modelled]**.
- The application queues the cast rebuild and runs it on the first
  step where no event or conversation is in progress (`App::reload_cast`):
  `AnimatedScene::rebuild_cast` replaces the cast instances without touching the
  map, the conversation list is replaced and 0x7c removals are forgotten. The fade
  and its duration (about 30 frames during which the field waits) are **not
  modelled**: the cast changes in one step. What `021a2fa0` additionally rebuilds
  (field monsters, characters `0xc9`–`0xce`, objects loaded by `ov_017::021a28a8`)
  remains **[unknown]**; the conversation counter reset comes from
  `Track_Advance` (point 1), not from the rebuild.
- Test: `the_end_of_21510_rebuilds_the_cast_and_npc_99_raises_game_0_flag_4` (fails without
  the rebuild: Aquila is not drawn) and
  `a_progress_change_reruns_event_20_and_drops_the_added_areas`. No original state at
  `X01M04` in 1.2 was catalogued (`dq9re states` on `build/v01-20261107/oracle`,
  `build/orca/route`, `build/melonds*`): Aquila's presence without a map change is
  not compared against melonDS.

## Global flags set at the opening

Several distinct mechanisms write global flags (`g_QuestManager+0x8c`) during
the prologue: the cutscene, the visited map, acquiring an item and the held key, as well
as two trigger actions (item given and session host).

### Cutscene flag (`EventTask_Update`, `ov_017::021bc77c`)

**Found** by decompiling `EventTask_Update` (Ghidra, EU addresses), then checked against
the melonDS states of first player control (`build/orca/oracle/`, `build/orca/oracle-session-1711efc/`):

- On the first pass not blocked by loading the cutscene's files (immediate
  return while `BackgroundLoader::GetNumQueuedTasks() > 0`, `0x020303a0`), the task sets
  two optional flags on its context `ctx` (pointer `GameResources+0x3734`,
  `ov_017::0218b5b0`):
  - `Flags_Set(manager, manager+0x8c, s16(ctx+0x52) + 0x38e, 1)` if `s16(ctx+0x52) >= 0`;
  - `Flags_Set(manager, manager+0x8c, s8(ctx+0xa0) + 0xbea, 1)` if `s8(ctx+0xa0) >= 0` and
    `ctx+0x9c != 0`.
  Both fields are `-1` at initialisation (`func_ov017_021bba08`, `ov_017::021bbae4`).
- The cutscene index comes from the **rank of the record in `eventlist_lv5.bin`**: the table
  reader (`EventRecord_Parse`, `0x02071498`) adds to `ctx+0x52` the base 0x21c (540) for
  events 21000..39999. A second base 0x302 (770) exists in the code of
  `Flags_SetStoryBases` (`0x02071698`, called by `main`) for the other records,
  but it is **not observed** on the prologue: the original states after 1130, 1150,
  1160 and 1600 carry neither 1680 nor 1681. The rank (0, 1, 2…) comes from the
  `Tbl_Run` counter incremented by the previous record (`EventRecord_Build`, `0x02071218`,
  `+4`), which `EventRecord_Parse` copies to `ctx+0x46` when the record asks for it,
  then adds the base to.
- Checked in RAM on the real states: `ctx+0x52` is 540, 541, 542, 543, 544 and 551 for
  `ev21500`, `ev21507`, `ev21520`, `ev21521`, `ev21522` and `ev21596`, i.e. flags
  **1450 to 1454 and 1461**; these are the six cutscene flags of first player control
  (`bat6.ml1`). State `pro1.ml1` already shows 540 in `ctx+0x52` but **not** 1450: the task
  was still blocked on its loads, which confirms the flag is only set on the
  first useful pass and not when the context is created.
- **Reimplementation** (`dq9-core`): `script::load::scene_flag` reads the rank in
  `eventlist_lv5.bin` and adds the base then `0x38e`, only for the observed opening
  range — test `scene_flags_match_the_oracle_checkpoints` on the ROM
  (`21500`→1450 … `21596`→1461) and a synthetic test that rejects 1130/1160/1600, with no list
  of event constants. The flag
  travels on `EventScript` (same list read as the script, no separate parse); the
  wiring is done in `dq9-app::begin_event`, after `EventPlayer::prepare` and before the
  first script update (never in `GameState::new_game`): it is an effect of
  running cutscenes.

### Visited-map flag (`Flags_SetMapVisited`, `0x0201ba78`)

**Found** by decompiling the code called when a map loads (`0x02015448+200`):
`Flags_SetMapVisited(Zone3D*)` reads the map id at `*Zone3D` and sets, in the
quest manager:

- `99 < id <= 9999`: flag `0xa96 + id / 100` (base 2710). Checked on the states: map
  1112 (`M01M12`) → 11 → **2721**, present from `pro1`; map 4100 (`X01`) → 41 → **2751**,
  absent before battle 26 and present after `X01` loads.
- `20000 <= id <= 29999`: flag `0xa5a + f(id - 20000)` (`0x02012f78`, remapping 0x3c..0x3f,
  `-1` beyond 0x3b → no flag). The exact value of `f` for the prologue's field maps
  is not checked.
- `func_0201b588()` (condition **[unknown]**) sets flag **1949** (`0x0201bb84`): it
  is not a map id, so `map_visit_flag(1949)` follows the general branch
  (`2729`). Branch `0x02012f24` for maps outside `99..=9999` and `20000..=29999` remains
  **[unknown]**.
- Checked: **2774** = 2710 + 64 is already set in `pro1`, which matches map group
  6400 (`S14`, Starflight Express). The libretro harness gives the missing link: after
  character creation, frame 3762 has map **6405** (`S14M05`, "For Events") and
  area 6401, frame 3764 carries 2774, and frame 3864 is on `M01M12` (1112). The train map
  is therefore indeed loaded before the prologue; the cutscene played there is not modelled.
- **Reimplementation** (`dq9-core`): `state::map_visit_flag` reproduces the branch
  `99 < id <= 9999` and returns `None` for the others (not established). Wiring: on map
  entry, before triggers (`dq9-app::change_map`), not in `GameState::new_game`;
  `dq9-app::start_new_game` loads `S14M05` (6405) before launching the first cutscene, like
  the original run.

### Item-obtained flag (`func_0207cc18`, `0x0207cc28`)

**Found** by decompiling `func_0207cc18`, called by `Items_Give`, adding to the bag
(`func_0207cf30`) and equipping (`func_0207ccf0`): on the **first** copy of an item,
the game reads the **compendium index** from its record
(`(record[0xc] & 0x7fffff) >> 0xc`, table `data/prm/itemdt_<category>.gp2`) and sets
`Flags_Set(g_QuestManager, +0x8c, 0xc76 + index, 1)`. Index 0: no flag.
Format details and item table: [`item.md`](item.md).

- **Starting equipment**: the four flags set together at frame 3740 of the original
  run are the copper sword (20004 → index 1 → **3191**), the celestial costume (13007 →
  585 → **3775**), the celestial tights (16215 → 765 → **3955**) and the celestial shoes (17120 →
  856 → **4046**). The sword comes from the `ov_009` creation table `0x0218636c`; the three
  outfit pieces from `HeroLook::new_game`.
  The real writer is `Party_FlagEquippedItems` (`func_0206e424`, EU `0x0206e434`,
  trace `lr = 0x0206e524`): it walks the equipment of a member 0..3 (slots
  `0x0206e564`) and sets `0xc76 + compendium` for each item of class < 8 with a non-zero
  compendium, not `Item_FlagObtained`; callers `ov_017::0218b688` and `ov_017::021b688c`.
  `State::obtain` does not cover this path (equipment of a recruited companion); details in
  `item.md`, § "First acquisition at the opening".
- **Event loot**: trigger `triggerX01.bin`, room 4105, event 11 of `21596`
  (`0x68(set_track_flag) 0x64(set_flag 337) 0x72(give_item 22159) 0x6b(0)`) gives
  the benevolessence (22159 → 1093 → **4283**), observed at frame 21142; the same trigger sets
  337 and, through `0x6b`, **4410** (below).
- **Reimplementation**: `dq9-formats::item::ItemTable` reads the nine tables; `dq9-app`
  fills it from the ROM; `dq9-core::trigger::State::obtain` applies `0xc76 + compendium` on the
  first copy (actions `0x72`/`0x74`, `0x9c`/`0x9d`), and `start_new_game` gives
  the starting equipment by the same rule. No flag constant is copied: the
  item table provides the index.

### Held-key flag (`KeyManager::RefreshKeys`, `0x020122ac`)

**Found** in `KeyManager::RefreshKeys` (USA decompilation `src/Input/Input.cpp`, EU address
`0x020122ac`): each frame, if input is active, the screen open and brightness
sufficient, it reads the twelve buttons; if `currentKeys != 0`, it sets
`Flags_Set(g_QuestManager, +0x8c, 0x113e, 1)` — flag **4414**. The original run
shows it at frame 1501, one frame after the first press.

**Reimplementation**: `dq9-app::run_steps` sets `state::KEY_REFRESH_FLAG` (0x113e) as soon as a
key is held (d-pad, action key, `--auto` driver or replay). The
screen/brightness conditions have no equivalent and always pass.

### Action `0x6b`: session host (`Trig_RunAction`, case 7)

**Found** in `Trig_RunAction` (`0x02061c14`, case 7 = code `0x6b`): if the session
predicate (`func_0202c508`, `0x0202c518`) is true, `Flags_Set(g_QuestManager, +0x8c,
*(u32*)0x02063008)`; the word read in RAM is **0x113a**. In solo the runtime is host: the
predicate is considered true. The end trigger of `21596` runs it with argument 0, which
gives **4410** at frame 21137, together with 337 and 1461.

### Remaining unknowns

**Measurement after wiring** (`--new-game --headless --auto`, strict comparator on
`bat6.ml1` and on the RAM image `f000090.bin` of first player control): the runtime sets the
**17** flags `337, 1450–1454, 1461, 2721, 2751, 2774, 3191, 3775, 3955, 4046, 4283, 4410,
4414`; map, track, progress and `step_flags {1}` identical. Gap: **4135** alone against
`f000090.bin` (female run), **4212** alone against `bat6` — never both.

- **4135** (item 22000, medicinal herb) and **4212** (item 22087, slimedrop) are
  **random battle loot**: three real runs with identical inputs except the sex
  give the herb, or neither; `bat6` gives the slimedrop. The runtime substitutes
  battle 26 and draws no loot (`melonds-state.md`, § "Differential oracle"): no
  flag is forced to make it match.
- The origin of the other flags is established above (equipment, map, key,
  trigger); there is no longer any constant copied from a checkpoint.

## Scripted battles (action `0x78`)

**Found** by reading `Trig_RunDeferred` (`0x0206f82c`), case `0x78` (Ghidra, EU addresses). The
DQIX wiki (`Triggers.md`, operation 120) says so too ("starts set battle *n*"): we
verified it against the code and the data before adopting it.

- The case takes the field manager's task `+0x3718`, initialises it (`BattleTask_Init`,
  `ov_017::021b6f18`), fills a `0x18`-byte request (byte 0 = 1, byte 1 = local character,
  half-word `+4` = `arg + 0x8005`, word `+8` = `arg`, `+0xc`/`+0xd` = states read from the field
  monster and the treasure map type by `ov_017::021b6e70`, byte `+0x12` = time of day)
  then copies it into the task (`BattleTask_Setup`, `021b7104`: battle number at `+0x28`,
  `arg + 0x8005` at `+0x24`) and queues it (`0x020469b4`).
- The task (`BattleTask_Update`, `ov_017::021b790c`) runs the battle. At its end, if `+0x28 ≥ 0`,
  it builds the `Trig_Fire` parameter block (`0x34` bytes, **word 6 = `+0x28`**) and calls
  `Trig_FireBattleEnd` (`0x02064a18`, event 15) or, when byte `+0x8e14` of the battle
  structure is 2, `Trig_FireBattleEndAlt` (`0x02064a34`, event 16), then `Trig_RunDeferred`. It is
  word 6 that condition `0xc` reads (`12 : n` in the wiki). What `arg + 0x8005` means (task
  id, compared to `+0x44ae` of the game resources at the end) remains **[unknown]**.
- Check on the data (`tests/scenario_rom.rs`): every `0x78` action in the trigger
  files (65) names a record of `eventbattle.bin`, and each has, in the same file
  and for the same room, an event-15 trigger on that number, without exception (the wiki
  counted 65 of 66 per file).

In `dq9-core`: `Request::Battle(n)` (`run_deferred`), `scenario::battle(files, n)` reads
the record (monsters, music, arena), `Field::battle_ended(n, won, state)` fires 15 or 16.
**There is no battle system before J4**: the application does not play the battle, it
declares it won at once (`scenario::stand_in`, visible in the log and on stderr) then
fires event 15, as the task would. This is not a battle model: it serves to reach
the cutscenes that follow it. Prologue example: end of `ev21507` (`M01`, room 1112, trigger 11
`8 : 21507` → `0x78(26)`), battle 26 (monsters 290, 292 and 290, music 23, arena 30116), victory
(`triggerM01.bin` and `triggerF.bin`, event 15, `12 : 26` → `0x77(21520)`).

## Doorways without destination and inner doors (native 584)

A type-2 doorway whose `0x74` action lacks flag `0x8` does not change map:
`Field_TouchDoorways` (`ov_017::02198f84`) still calls the door routine
(`0x02018b44`, the same as native 584). It finds the placement whose block entry equals
the zone's **class** and whose id equals its **number**, then rotates that placement and its whole
subtree by `0x1922` (+90°) around its origin: 16 updates of 2 frames, `409` per
update (`Door::SPEED`, `Door::QUARTER_TURN` of `dq9_core::cutscene`). The child collision
follows the rotation (`CollisionWorld::set_door_angle`, `fx32` delta), otherwise the passage stays closed.
Event 17 fires before the rotation; event 3 (arrival) fires when it is done. The
sound is `g_DoorSeTable[low nibble of value 3 of 0x74]`, base effects **200 to 204**.
Direct evidence for the table: `dq9re state build/orca/oracle/bat6.ml1 --peek 0x020e6e84:14`
gives `c8000000 c9000000 ca000000 cb000000 cc000000` (200, 201, 202, 203, 204); the pointer
`0x020e6e84` is `0x02018754`, `02018310` copies exactly five entries and indexes
`flags & 0xf`: `200 + low nibble` is correct for indices 0 to 4.

Evidence on the ROM:

- X01M07 (library) has **two** zones at the same volume `(-8.6, 0.5, -7.2)`, `1.5 × 2 × 2`:
  class 0 number 2 (flags `0x10`, sound 2) and class 0 number 3 (flags `0x12`, sound 2).
  Their placements 2 and 3 (block entry 0) each carry a collision child (4 and 5).
  Test `x01m07_inner_door_opens_the_way`: closed, the hero stops at `z = -6.45`; after
  `set_door_angle(0, 2, +0x1922)` **and** `set_door_angle(0, 3, +0x1922)`, he walks through
  (`z < -8`). A single leaf is not enough: both zones are touched together.
- X01M01, X01M07, X01M05: the type-1 `0x73` zones (43 in the ROM) are **inert** at
  runtime — no routine of `Field_CheckRegions` reads their list —; they are neither a
  lift nor a task 8. See the route limit in `app.md`.

## End of conversation (event 1)

`Field::talk_ended(npc, context, answer, state)` fires event 1 when the window of a
conversation closes: word 0 = NPC, word 5 = context that picked the line (the one from the
event-0 trigger, `0x76(npc, context)`, otherwise the zone), `+0x954` = answer to the last `<YESNO>`
(0 = yes; 0 without question, since the window sets it to 0 on opening). It is the follow-up of lines:
`0x6(101) 0xb(193) 0x10(0)` → `0x77(21591)` (descend to Angel Falls after answering yes to the
Observatory guardian). Wired in `dq9-app` (`Talk::take_ended`). This trigger is filed
under **1.2..1.2** (`dq9re triggers X01 --with 21591`): it is not loaded at sub-step
1.1.2. The original sequence goes through `4101 → 4103 → 4109 → 4103 → 4104`, event `21510`
(1.2.1) then `21591`; a direct route `21591` after 1.1.2 is excluded by the original
conditions.

## Angel Falls prologue chain (1.1.2 → 1.4.1)

Established on the European ROM with `dq9re stage-events 1.1-3`, `dq9re triggers`, `dq9re cast`,
`dq9re talk`, `dq9re stb` and `dq9re script 1.2-3`; the dumps and traces are under
`build/v01-20261107/village/` (ignored). Each transition below is the **only** `0x84`
action that reaches that story point: a story point only advances through a trigger
action, never through a cutscene. Cutscenes are only `0x77`s; they only change
progress through the event-11 trigger.

### 1.1.2 → 1.2: Yggdrasil and the Observatory

| Step | Evidence (`dq9re triggers`) | Interaction |
|---|---|---|
| 1.1.2, `X01M04` (4104), Yggdrasil | `room 4104 1.1..1.1 event 1: if 0x6(199) 0xb(80) 0x10(0) then 0x77(21510)` | type-1 NPC 199 (examine point, invisible) at `(0, 9.1, -3.04)`, zone 80; answer **yes** to `Offer the benevolessence up to Yggdrasil?` |
| end of `21510` | `room 4104 1.1..1.1 event 11: if 0x8(21510) then 0x73(22159) 0x84(0, 1, 2, 1) …` | removes the benevolessence, moves to **1.2.1** |
| 1.2.1, `X01M07` (4107), Apus Major | `room 4107 1.2..1.3 event 0: if 0x6(139) 0x5(0) then 0x76(139, 192) 0x68(0)` | dialogue 192 ("Well done…"), then context 193 ("Make ready, and then speak with the female Celestrian"); action `0x68(0)` sets flag **set 1 no. 0** |
| `X01M05` (4105), guardian 101 | `room 4105 1.2..1.2 event 0: if 0x6(101) 0x4(0) 0x52(13007, …) then 0x76(101, 193)`; `event 1: if 0x6(101) 0xb(193) 0x10(0) then 0x77(21591)` | condition `0x52(13007)` requires the celestial outfit, worn since creation; line 193 is the question "descend to the Protectorate?"; answer **yes** |
| end of `21591` | `room 4105 1.2..1.2 event 11: if 0x8(21591) then 0xe1(1100)` | transport to map **1100** (`M01`) |

### 1.2: arrival and village tasks

On arrival with a transition, `Field::enter` plays event 20 then event 3:

- event 20 (`room 1100 1.1..1.2`): `0x95(0, 0x1a0000)` (hides placement 26),
  `0x96(0, 0x1b0000)` (shows 27), `0x95(0, 0x1c0000)` (hides 28), then
  `0x8f(10, -2, -1, 21, -10, -2, 15, 0)`: area **10**, around the village entrance;
- event 3 (`1.2..1.2`, set 1 flag no. 1 cleared): `0x97(1)`, `0x77(21592)`,
  `0x68(1)`, `0xc5(3)`. `21592` is Ivor's cutscene (three `//Ivor//` messages); it
  ends without any other trigger.

The `M01` starting point is `(-6.45, -1.95, 13.55)`, outside area 10; walking
north (the entrance at `z ≈ 18`) enters the area:

| Step | Evidence | Interaction |
|---|---|---|
| area 10 reached | `room 1100 1.2..1.2 event 2: if 0x7(10) then 0x76(230, 192)` | automatic conversation with invisible NPC 230 (`-6.51, -1.85, 17.91`) |
| end of that conversation | `event 1: if 0x6(230) 0xb(192) then 0x77(1310)` | cutscene 1310 (fade, camera, hero brought back to `(-6.45, -1.85, 13.55)`) |

The three tasks that advance 1.2 (**set 1** flags 3, 4 and 5):

| Task | Evidence | Effect |
|---|---|---|
| **Spirit ring**: talk to NPC 210 (`s097`, `26.91, -1.05, 2.16`) | `room 1100 1.2..1.2 event 0: if 0x6(210) 0x5(3) then 0x77(1390)`; `event 11: if 0x8(1390) then 0x72(22158) 0x68(3) 0x7c(210)` | cutscene 1390 (`<HERO> acquires a keepsake ring`), item **22158**, set 1 flag no. 3, NPC 210 removed |
| **The widow**: talk to NPC 12 of `M01M06` (1106), `1.50, 0.15, 0` | with flag 3 set: `event 0: if 0x6(12) 0x4(3) 0x5(4) then 0x76(12, 193)`; `event 1: if 0x6(12) 0xb(193) 0x10(0) then 0x77(1330) 0x73(22158) 0x72(22159) 0x68(4)` | line 193 ("Please, Guardian… help to reunite my husband's ring") with a choice; answer **yes**; cutscene 1330 (the ring slipped into the coat), item 22158 taken, benevolessence **22159** given, set 1 flag no. 4 |
| **The stable**: talk to NPCs 211 to 215 of `M01M04` (1104) | `event 0: if 0x6(211) 0x1(316) then 0x76(211, 192)`; `event 1: if 0x6(211) 0x1(316) 0x0(317) 0x0(318) 0x0(319) 0x0(320) 0x5(5) then 0x72(22079) 0x64(316) 0x7c(211) 0x77(1340)`; variant without the four other flags for the first four | each of the five gives **22079**, sets its flag (316…320) and is removed; the **fifth** (the four other flags set, set 1 flag no. 5 cleared) plays 1340: the stable boy thanks, benevolessence 22159 is given (`event 11: if 0x8(1340) then 0x68(5) 0x64(315) 0x72(22159)`) |

The **house exit** is the lock of 1.2 → 1.3: the `0x72` zone of 1104 or 1106
fires event 6 (`room 1104 1.2..1.2 event 6: if 0x9(1104) 0x4(4) 0x4(5) then
0x66(1) 0xb1(1, 0) 0x84(0, 1, 3, 1)`, variant 1106 with `0x66(0)`). So the
set 1 flags no. 4 (1330) **and** no. 5 (1340) are required; the set 0 flag (0 or 1 depending on the house)
picks the arrival cutscene 1350 or 1360. `0xb1(1, 0)` sets the time of day to **0
(night)**.

The `0x72` zone remains an **ordinary map exit**: without flags 4 and 5, the warp
still returns to 1100. `Field::step_warps` prepares the warp's own destination and
`Field::collect` uses it as a fallback when event 6 returns no `0x79`/`0x85` action;
event 6 only replaces it. The 1.2 → 1.3 lock is therefore on track **progress**
(`0x84`) and on **night** (`0xb1`), not on the map change nor on the arrival
cutscene: without both flags, the hero returns to the village in 1.2.1, by day, and no
cutscene 1350/1360 is queued. Tests:
`house_exit_requires_both_task_flags_and_advances_to_1_3` (1104 and 1106) and
`house_exit_without_a_task_flag_returns_to_1100_without_advancing`.

The two remaining tasks are covered end to end by application tests on the
real ROM (`App::new_headless`, no EventLoop) that go through talk and playback of
real events: `keepsake_ring_task_gives_22158_raises_flag_3_and_removes_npc_210`
(first plays the arrival cutscene 21592 queued by event 3, then talks to 210:
1390 gives 22158, sets set 1 flag no. 3, removes 210 and sets no other flag);
`widow_task_takes_22158_gives_22159_and_raises_flag_4` (1106, flag 3 and ring 22158
as preconditions: context 193, answer yes, 1330, 22158 taken, 22159 given, flag 4,
widow still drawn); `widow_without_flag_3_starts_no_scene` (context 192, no
event queued, items and flags unchanged).

**Optional but story** cutscene: the dog 95 (`n097a`, `20.29, -1.05, -0.67`) →
`if 0x6(95) 0x5(2) 0x5(3) then 0x77(1320)`; the end of 1320 does `0x68(2)` and advances to
**1.2.2** (`0x84(0, 1, 2, 2)`). No 1.2 → 1.3 lock tests the sub-step: the cutscene
can be seen at any time before leaving, and missing it does not block the transition.

### 1.3: night, wandering soul and return

- event 20 (`room 1100 1.3..1.4`): `0x96(0, 0x1a0000)`, `0x96(0, 0x1b0000)`,
  `0x95(0, 0x1c0000)`, then `0x8f(12, 3, 0, 0, 0, -1, -4, 17)`: area **12**;
- event 3: `0x2(0)` (set 0 flag no. 0 set) → `0x77(1350)`; `0x2(1)` → `0x77(1360)`;
  the variant played sets set 0 flag no. 2 and clears global flag 315;
- area 12 (x `0..3`, z `-4..0`) → `room 1100 1.3..1.3 event 2: if 0x7(12) 0x5(0) then
  0x77(1410)`: Aquila's lesson on wandering souls; its end sets set 1 flag no. 0;
- NPC 94 (`n007g`, `15.76, -1.05, -6.78`) →
  `room 1100 1.3..1.3 event 0: if 0x6(94) 0x5(1) then 0x77(21595) 0x72(22159)`: the
  wandering soul cutscene. It opens no conversation (no `0x76`), so no
  `094_en.bin` line file is needed.

End of `21595`, normal case in the original: the script calls native **559**
(`ov_001::021609e4`) with 1 on the normal path (`559(1)` just before the final fade of
`mark(13)`; `559(0)` only on the `mark(12)` branch), which writes `+0x490` of the
quest manager. The end trigger therefore takes the branch
`event 11: if 0x8(21595) 0x1f(1) then 0x84(0, 1, 4, 1) 0x97(2) 0x85(4105, …)`: direct
return to the Observatory (`4105`) at **1.4.1**. The runtime applies 559 since `8d41723`:
`dq9-core` emits `Event::SetQuestSignal(u8)` and `dq9-app` writes `Raw::Quest490` while handling
`Outcome::SetQuestSignal`, in result order and therefore **before** the `Ended` that runs
event 11. The fallback branch `0x1f(0)` (`0x68(1) 0x84(0, 1, 3, 2) 0x8d(1)`), then
the conversation with Aquila 99 (`s019n`, `4.46, -1.05, -2.06`) and cutscene 21593 remain in
the data but are no longer played: the original does not play them either.

The `0x85` operand is not a map entry point: it is **the event to chain
on the arrival map**. `0x85(4105, 1415380992)` is `0x85(4105, 21597)` and 21597 is
exactly the opening cutscene of 1.4 (`evspt_lv5/ev21597`, "Aquila : I have important
matters to discuss with Apus Major"). Same pattern in `M01`: 2200 → 2210, 2400 → 2410,
2750 → 2760, 2760 → 2800. Evidence in the code: `Trig_RunDeferred` (case 0x85) writes this
operand at `Save_Warp+0x20` (`(uint *)(puVar15 + 0x10)` on a `ushort *`), and the arrival
`ov_017::0219e384` calls `ov_017::0219fc88(param_1, *(u32 *)(Save_Warp + 0x20))`; that
function puts the number at `ctx+8` and queues the event task (`func_020469b4`).
`MapChange.entry` carries this value and `dq9-app::change_map` now sends it to the
event queue **after** loading the destination map and its own
entry triggers, only when `cause` is `Cause::Trigger`: a doorway (`Doorway`) or
the spawn entry of a `0x72` (`Warp`) is not an event. A zero `entry` launches nothing
(the ROM has no `ev00000`), and the event is only queued once per map
change. The event plays on the already loaded map, without reload.

### Story tasks and numbered quests

- Tasks 1330 and 1340 are **story tasks**, not numbered quests: no
  log action (`0x7d`, `0x7f`, `0x90`, `0x9c`/`0x9d`); their state lives in the
  set 1 flags no. 3–5 and the global flags 315 and 316–320.
- The numbered quests of the same maps (quest 7 at NPC 1, quest 103 at NPC 1,
  quest 193 at Ivor's inn) are conditions `0x14`/`0x19`/`0x1a` of triggers
  `1.1..19.99`: they stay available without ever taking part in the
  1.2 → 1.3 lock. `dq9re triggers M01 --with 7` shows them next to the prologue lines.
- Cutscene 1610 (`X01M07`, NPC 130, answer **no**) is lore (the disappearance of a
  Guardian); it touches neither progress nor quest.
- `1370` (Ivor, choice) and `1380` (NPC 2) are local-colour cutscenes: `0x68(13)` and
  `0x66(7)` respectively, no `0x84` action.

### Wired behaviours and remaining gaps

1. **Native 559** (`ov_001::021609e4`) — **wired** in `8d41723`. `dq9-core` emits
   `Event::SetQuestSignal(u8)` (argument read by `StbArg_Int`, `u8` truncation); `dq9-app`
   writes `Raw::Quest490` in `step_event`, in result order (`Outcome`), hence
   before the `Ended` that runs event 11. Files: `script/natives.rs`,
   `script/event.rs`, `cutscene.rs` (exhaustive match arm), `dq9-app/src/event.rs` and
   `app.rs`. End-to-end test:
   `event_21595_sets_quest490_and_takes_the_1_4_1_warp` (`Raw::Quest490 == 1`, track
   (1, 4, 1), warp `4105`, neither 21593 nor its start).
2. **Arrival event of `0x85`/`0x8a`** — **wired** (this batch). `MapChange.entry` is
   queued by `App::change_map` after `load_map` (hence after the destination map and
   its entry triggers), only for `Cause::Trigger`, once per change, and not
   for a doorway (`Doorway`) nor the spawn entry of a `0x72` (`Warp`). A zero `entry` is
   ignored (no `ev00000` in the ROM). Tests:
   `event_21595_sets_quest490_and_takes_the_1_4_1_warp` (4105 loaded once, 21597 starts
   once and its event 11 sets set 1 flag no. 0) and
   `only_a_trigger_warp_starts_its_entry_event` (same causes `Warp`/`Doorway`: no
   event; `Trigger`: 21597).
3. **Rendering time of day** — **wired** (this batch). At the start of
   `App::apply_events`, `App::sync_day_phase`: if `options.time` and `options.clock` are
   absent and `state.time_of_day != self.day.phase().index()`, it does
   `self.day.set_phase(lighting::phase(state.time_of_day))` and
   `self.time = self.day.phase().index()`, then rebuilds `Environment` (and
   `Renderer::environment` when it exists) — **without** `set_time`, which reloads the map and
   would replay its entry triggers. The `MapChange` handled next in the same
   `apply_events` loads the destination at the new time (night placements and
   lighting; cast via `story_from_state` for a new game). `--time`/`--clock` keep
   priority for oracle captures. Tests:
   `house_exit_time_of_day_reaches_the_application_before_the_map_change` (1104 not
   reloaded, 1100 loaded at night) and `a_pinned_time_of_day_wins_over_the_trigger`.
4. **NPC removal (0x7c)** — **wired** (this batch). `App::remove_npc` always removes the
   NPC from the conversation list (`Talk::remove_npc`) and calls
   `EventPlayer::remove_npc`, which hides the cast instance (zero scale, the object carries
   flag `0x8000`, `02013380`). The removed id is kept: `seed`, `present` and
   `restore_npcs` no longer put it back during following cutscenes; a map change
   clears the list with the map, like the cast rebuild by `0x8d`.
   Tests: `removing_npc_210_hides_its_cast_instance` and
   `stable_pickups_hide_each_pile_and_the_scene_keeps_them_hidden` (the five piles, 1340 whose
   `0x8d(1)` rebuilds the cast without them, and no return after the cutscene; the
   stable boy 15 stays drawn).
5. `560` and `571` do not change the observed flow. `0x8d` rebuilds the cast
   (§ "Reloads after deferred actions"). The
   11 `data/event` cutscenes of steps 1.2–1.3 run to their end (`dq9re script 1.2-3`); the
   `evspt_lv5` cutscenes 21592, 21593 and 21595 also play to the end, but in isolation
   (`--event`), since `stage_events`/`script` do not include them in that list. No
   fix is established for these two natives.

### Reproduction

```powershell
$ROM = "rom/Dragon Quest IX - Sentinels of the Starry Skies (Europe) (En,Fr,De,Es,It).nds"
dq9re --rom $ROM stage-events 1.1-3
dq9re --rom $ROM triggers X01 --story 1.2
dq9re --rom $ROM triggers M01 --story 1.2
dq9re --rom $ROM triggers M01 --story 1.3
dq9re --rom $ROM triggers M01 --with 21510
dq9re --rom $ROM triggers M01 --with 1330
dq9re --rom $ROM triggers M01 --with 1390
dq9re --rom $ROM triggers M01 --with 1340
dq9re --rom $ROM script 1.2-3 --quiet
# isolated diagnostic (does not prove the continuous run):
dq9-app --rom $ROM --events-report 1.2-3 --mute
dq9-app --rom $ROM --event 21591 --chapter 1 --step 2 --substep 1 --headless --headless-steps 4000 --press-every 30 --mute
dq9-app --rom $ROM --event 21595 --chapter 1 --step 3 --substep 1 --night --headless --headless-steps 12000 --press-every 20 --mute
# expected: map change 1100 -> 4105 (Trigger), then "event 21597 queued", "starts",
# "ended" only once; no "event 21593" line.
dq9-app --rom $ROM --event 21597 --chapter 1 --step 4 --substep 1 --headless --headless-steps 4000 --press-every 20 --mute
```

Native evidence (Windows, ROM found via `DQ9_ROM` or `rom/`):

```powershell
cargo test -p dq9-app event_21595_sets_quest490_and_takes_the_1_4_1_warp -- --nocapture
cargo test -p dq9-app only_a_trigger_warp_starts_its_entry_event -- --nocapture
cargo test -p dq9-app house_exit_time_of_day_reaches_the_application_before_the_map_change -- --nocapture
cargo test -p dq9-app a_pinned_time_of_day_wins_over_the_trigger -- --nocapture
cargo test -p dq9-app removing_npc_210_hides_its_cast_instance -- --nocapture
cargo test -p dq9-app stable_pickups_hide_each_pile_and_the_scene_keeps_them_hidden -- --nocapture
cargo test -p dq9-app house_exit_requires_both_task_flags_and_advances_to_1_3 -- --nocapture
cargo test -p dq9-app house_exit_without_a_task_flag_returns_to_1100_without_advancing -- --nocapture
cargo test -p dq9-app village_area_12_plays_1410_then_npc_94_takes_the_1_4_1_return -- --nocapture
cargo test -p dq9-app the_1_3_arrival_scene_does_not_revive_removed_npcs -- --nocapture
cargo test -p dq9-app the_end_of_21510_rebuilds_the_cast_and_npc_99_raises_game_0_flag_4 -- --nocapture
```

### Proposed regression assertions

- **0x95/0x96 emission** (done, `field_triggers_emit_placement_hide_and_show`): a
  synthetic trigger carries `0x95(0x1234, 0x00100000)` then `0x96(0x01ff, 0x00070000)` and a
  deferred `0x77(5000)`; `Field::event_ended` returns
  `[SetPlacementHidden{block:0x34, placement:16, hidden:true},
  SetPlacementHidden{block:0xff, placement:7, hidden:false}, Event(5000)]`.
- **Exit lock** (done, `house_exit_requires_both_task_flags_and_advances_to_1_3`
  for 1104 and 1106, and `house_exit_without_a_task_flag_returns_to_1100_without_advancing`
  for one of the flags missing): state 1.2.1, set 1 flags 3–5 set, leader in the
  `0x72` zone of 1104/1106 → `Field::step` returns `MapChange{destination:1100, cause:Warp}`,
  `state.time_of_day == 0`, track (1, 3, 1) and the house's set 0 flag (1 for 1104,
  0 for 1106); `App::sync_day_phase` sets `app.day`/`app.time` to night **before**
  loading 1100 and the arrival cutscene (1360 or 1350) is queued. With flag
  4 or 5 missing, no `0xb1`/`0x84`: the track stays (1, 2, 1), the time of day
  stays at 2 (day) and no cutscene 1350/1360 is queued; the `0x72` warp still
  returns to 1100 (warp fallback, see "1.2: arrival and village tasks").
- **1330**: state 1.2.1 with set 1 flag no. 3, event 1 of NPC 12, context 193,
  answer 0 → `Event(1330)`, 22158 removed, 22159 given, flag 4 set (and
  obtained flag 4283).
- **1340**: event 1 of NPC 211 with flags 317–320 set and 316 cleared →
  `Event(1340)` and `RemoveNpc(211)`; with 317 only, no event but 22079 given,
  316 set and `RemoveNpc(211)`.
- **559** (done, `event_21595_sets_quest490_and_takes_the_1_4_1_warp`): after running
  21595, `Raw::Quest490 == 1` and trigger 11 returns warp 4105 with track (1, 4, 1),
  without 21593.
- **`0x85` entry** (done, `event_21595_sets_quest490_and_takes_the_1_4_1_warp` and
  `only_a_trigger_warp_starts_its_entry_event`): map 4105 loads **once**, then
  `Event(21597)` is queued once and plays; its event 11 sets set 1 flag
  no. 0. Causes `Warp` and `Doorway` with the same `entry` launch nothing, nor does a zero
  `entry`.
- **Time of day** (done,
  `house_exit_time_of_day_reaches_the_application_before_the_map_change` and
  `a_pinned_time_of_day_wins_over_the_trigger`): the house exit (0xb1 night) sets
  `app.time`/`app.day` to night before the `MapChange`, loads 1100 at night without reloading
  1104, and `--time` keeps priority.
- **NPC removal** (done, `removing_npc_210_hides_its_cast_instance` and
  `stable_pickups_hide_each_pile_and_the_scene_keeps_them_hidden`): after `RemoveNpc(210)`
  from 1390 instance 210 is no longer drawn, the other NPCs still are; piles 211–215
  each disappear when picked up, 1340 does not bring them back (its `0x8d(1)`
  rebuilds a cast that no longer places them), and stable boy 15 stays drawn.
- **Cast rebuilt by `0x8d`** (done,
  `the_end_of_21510_rebuilds_the_cast_and_npc_99_raises_game_0_flag_4`): the end of 21510
  on `X01M04` moves to 1.2.1 and returns `ReloadCast` without a map change; on the next step
  Aquila (NPC 99) is drawn, talking to him sets flag 4 of set 0.
- **1.3: area 12, 1410 and 1.4.1 return** (done,
  `village_area_12_plays_1410_then_npc_94_takes_the_1_4_1_return`): at 1.3.1 at night,
  area 12 added by event 20 is found by the trigger it fires on a
  copy of the field, then the leader is placed on collision ground inside it; one `App`
  step queues 1410, its end sets set 1 flag no. 0. NPC 94, through its real
  partner search (`Talk::partners`/`Talk::begin`, not a direct call), queues 21595.
  The normal end of 21595 writes `Raw::Quest490 == 1`, moves to 1.4.1, loads 4105 once and
  queues 21597; `play_pending_event` plays it, `started` counts it **once**, does not
  contain 21593, and `app.time`/`state.time_of_day` stay at 0 (night).
- **1.3 arrival cutscene and removed NPCs** (done,
  `the_1_3_arrival_scene_does_not_revive_removed_npcs`): after the `RemoveNpc(210)` from 1390,
  event 3 of 1100 (1.3) is played on the already loaded map — this is what `load_map`
  runs after a transition, without the rebuild that empties the removal list. It
  queues 1350 (set 0 flag no. 0) or 1360 (no. 1); after the cutscene, 210 stays hidden and
  211–215 are not drawn on 1100. A map change (`load_map` 1100) rebuilds
  the cast and forgets the removals: 210 is drawn again, the documented exception of
  `EventPlayer::map_loaded`.

## Examples

George in Stornway (`triggerM02`, room 1200, chapter 15 = letter M) when quest 11 is
in state 1 (test `talk_triggers_force_the_dialogue_context`):

```
if 0x37(18) [quest 11, cond. 3]  0x57(0)  0x50(25)  then 0x76(18) context 202  → quest offer
if 0x37(18) [quest 11, cond. 3]                     then 0x76(18) context 203  → "if only there was someone… high enough level"
```

Brunhild in Gleeba (`triggerC02`, room 201, track 3):
`if 0x6(210) 0x19(30) 0x31(9)[15] then 0x76(210) context 214` → "So, you have reached
the fifteenth level as a paladin" (vocation 9 = Paladin, level 15).

Sign 137 of Stornway castle (`triggerC01`, room 100): `if 0x29(1) 0x6(137) then
0x76(137) context 202` → "`<LEADER>` reads the sign" only from the sign's
zone.
