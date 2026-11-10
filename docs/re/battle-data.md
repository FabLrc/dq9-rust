# Battle data: monsters, actions, spells, equipment, encounters

Implementation: `crates/dq9-formats/src/battle.rs` and `battle/{monster, action, spell,
equipment, encounter}.rs`; inspection with `cargo dq9re monster|action|encounters`.
Real-ROM checks: `crates/dq9-formats/tests/battle_rom.rs` (skipped without a ROM).

Sources: the DQIX wiki pages *Monsters*, *Actions*, *Spell Table*, *Equipment Battle
Parameters*, *Encounters*, *Event Battles*, *Tagged Data Table*, *System Strings*,
*Battle Resolution*, *Battle AI*, *Ability Handlers* (local checkout, unlicensed: facts reused,
no text copied), and the USA decompilation for the code addresses quoted. Every count below
was re-measured on the European ROM (`YDQP`); where it differs from the wiki the measured
value is given.

## The shared `.nat` layout

The monster list, the battle data, the name records, `actname.nat` and both action tables open
with the same head word: **bits 0–11 the record count**, **bits 12–27 the string section's
size**, the top nibble 0. Unique strings follow the fixed-size records and end the file
exactly — checked on every one of these files (e.g. `mon_list_en.nat` opens `0x01545159`:
345 records, 5,445 bytes, `4 + 345 × 32 + 5,445 = 16,489`, the file's size).
`battle::FixedRecords` splits a file this way and refuses a string section that does not end
it, a string offset that points inside a string, and truncated records.

## Monsters — `mon_list`, `mon_btldata.nat`, `mon_data`

### The list — `mon_list.gp2/mon_list_<lang>.nat`

345 records of 32 bytes, then the strings (5,445 bytes in English, 5,785 German, 5,788
French, 5,560 Italian, 6,223 Spanish). Per record: `+0x00` a word that is 0 on every record,
`+0x04` the code's offset, `+0x08` the name's offset, `+0x0C` the monster's number, `+0x0E`
not established, `+0x10` 16 bytes not established. Every offset lands at a string start.

Numbers run 1 to 514 with gaps (345 distinct values). Codes: 278 open `z`, 67 open `b`; 306
distinct names for 345 records (records share a name). Number 300 is `b003a`, the Hexagoon.

### Battle data — `mon_btldata.nat`

438 records of 132 bytes, no strings (`4 + 438 × 132 = 57,820`, the file's size).

| offset | type | reading |
|---:|---|---|
| `+0x00` | `u16` | the number in bits 0–14; **bit 15 set on 388 of 438 on this ROM** (the wiki gives all 438 — what it marks is **[unknown]**) |
| `+0x02` | `u8×2` | each drop's chance step, ordinary then rare: 0 is 100%, 1–6 one in `2^(step+2)`, 7 never (the word at `0x021fd888`) |
| `+0x04` | `u16×2` | the two drops, item ids |
| `+0x08` | `u32` | experience |
| `+0x0C` | `u16` | gold |
| `+0x10` | `u32` | bits 5–7 the ways selector (`func_0208a91c`), bits 13–15 the evasion grade (`func_ov000_02156270`), bits 16–18 the block grade (`func_ov000_02156118`), bits 20–25 the once-a-battle mask (`0x0208a0a0`); bits 8–12, 19 and 26–31 not established |
| `+0x14` | `u32` | not established |
| `+0x18` | `u16×6` | the six ways: action numbers |
| `+0x24` | `u32` | what enrages it (`func_ov024_021eb08c`); bit 28 (byte `+0x27` bit 4) the boss bit, read by no instruction |
| `+0x28` | `u8×2` | the least and most passes Knight Watch holds it (`func_ov024_021e1de8`) |
| `+0x5C` | `u16×5` | maximum HP, MP, attack, defence, agility — the block `func_02089630` copies from `+0x2C` |
| `+0x68` | `u32` | the wiki's three 10-bit numbers; **0 on all 438 on this ROM**, so nothing is read from it |
| `+0x6C` | `u8×22` | resistances in hundredths by `element − 1` (0 immune, 100 whole); byte 7 is element 8, the plain Attack's, **100 on all 438**; byte 21 (element 22) unused |
| `+0x82` | `u8×2` | copied beside the resistances; not established |

The selector's distribution over the 438: 0 on 96, 1 on 281, 2 on 2, 3 on 9, 4 on 25, 5 on
22, 6 on 3.

**The evasion and block grades were read from the code.** `func_ov000_02156270` (evasion,
`0x021563ac`–`0x021563c8` in the wiki) takes a monster's record, masks `+0x10` with `0xffff`
and shifts by 13; `func_ov000_02156118` (block) takes `+0x10 >> 16` and masks 7. Both pass the
3-bit result to `func_02074988` (the symbol carries its USA address; EU `0x02074998`), which
indexes a five-entry float table — **0, 2, 4, 8, 25**, the rate in a hundred (the wiki's
table, USA `0x020e88e4`). Measured over the 438: evasion
grade 0 on 295, 1 on 92, 2 on 39, 3 on 10, 4 on 2; block grade 0 on 399, 1 on 6, 2 on 22, 3 on
7, 4 on 4. The metal slime's evasion grade is 3. The boss bit (byte `+0x27` bit 4) is set on 149 records — byte values `0x10` (114),
`0x19` (32) and `0x1C` (3); the other bytes are `0x00` (87), `0x0C` (111), `0x09` (84),
`0x03` (5) and `0x06` (2), and their other bits are not read.

**Checks over all 438**: numbers unique and sorted; each agrees with the name record's number
in the same order; every monster list number has a battle record and agrees on its code.

### Name records — `mon_data.gp2/mon_data_<lang>.nat`

438 records of 28 bytes, then the strings (8,784 bytes in English).

| offset | type | reading |
|---:|---|---|
| `+0x00` | `u32` | the name's offset |
| `+0x04` | `u32` | the code's offset |
| `+0x08` | `u16` | the number |
| `+0x0A` | `u16` | bits 0–6 level, 7–10 family, bit 11 a fall refused (`func_ov000_02156068` with 2), bit 12 metal |
| `+0x0C` | `s16` | **EU only** collision radius in 1024ths; 737 to 7,987 over the 438 — the slime 819 (0.800), the Hexagoon 3,194 (3.119) |
| `+0x0E` | `s16` | **EU only** collision height, `fx32`; 2,457 to 21,504 |
| `+0x10` | `u16` | the kind, 1 to `0x133`, bosses from `0x101` |
| `+0x12` | `s16` | **EU only** size in battle, in 4096ths; 4,096 to 27,852 (1.0 to 6.8) |
| `+0x14` | `u32` | the plural's offset |
| `+0x18` | `u32` | grammar (articles and gender); not read |

438 records carry 312 distinct codes: a code names the story's versions of one monster.
Elements of the numbers' block (`+0x0A`) measured: levels 0 to 99, 11 metal monsters, 115
refusing a fall, 14 family values (0 to 13).

**[unknown]**: the list's `+0x0E` and `+0x10`–`+0x1F`; battle data `+0x14`, `+0x2A`–`+0x5B`,
`+0x66`–`+0x67`, `+0x82`; the name record's `+0x18`; bit 15 of the battle data's number.

## Actions — `actdt_a`, `actdt_b`, `actdamage_*`, `actname.nat`

`actdt_a.gp2/actdt_a_<lang>.nat` holds **63** actions and `actdt_b.gp2/actdt_b_<lang>.nat`
**618**; both have 60-byte records and a string section (1,313 and 3,969 bytes in English;
`4 + 618 × 60 + 3,969 = 41,053`, the file's size). `_a` holds the healing items and the spells
that do not strike, `_b` the attack, defending, fleeing and the monsters' moves.

| offset | reading |
|---:|---|
| `+0x00` | the name's offset |
| `+0x04` bits 0–11 | the action's number (the code masks twelve bits) |
| `+0x04` bits 12–21, 22–31 | the user's number at which a scaling amount leaves its least and reaches its most |
| `+0x08` low byte | the MP cost, **INFERRED** (0 on the 488 non-spells, 255 on four spend-all spells, 128 on two not established) |
| `+0x08` bits 8–9 | at 1, a blow that is not a critical deals 0 to a metal body |
| `+0x08` bits 14–21 | the range index |
| `+0x08` bits 22–26 | the element of what it deals |
| `+0x08` bit 29 | always a critical |
| `+0x10` bits 3–6, 11, 13–18, 24 | the flags read by the battle code: spoilt by a status, halved by defending, dodgeable, blockable, wakes, tension, magical might, magical mending, party trait, weakens, weapon element, works on metal |
| `+0x14` bits 0–6 | a monster's chance, in a hundred |
| `+0x14` bits 7–13, 14–20 | a party member's least and most accuracy |
| `+0x14` bits 21–27 | the critical chance's multiplier in hundredths |
| `+0x14` bits 28–31 (`+0x17` high nibble) | whom it reaches, **INFERRED** (1 the actor, 2 one, 3 everyone, 4 a group, 5 the attack's, 7 outside battle) |
| `+0x18` bits 0–4 | the rider, a slot of 22 |
| `+0x18` bits 5–11 | the kind (1 damage, 2 heal, 3–5 stats, 6 poison, 8 sleep, 93 values in use) |
| `+0x18` bits 16–17 | what scales: 1 accuracy, 2 amount |
| `+0x18` bits 18–26 | the damage handler index (0 to 66) |
| `+0x18` bits 27–31 | the element its landing is resisted by |
| `+0x1C` bits 0–13 | the most it can deal, 0 no limit |
| `+0x20` bits 10–19, 20–31 | the opening and result messages in `actmsg`, **INFERRED** |
| `+0x24` | low byte: `0x05` deals damage, INFERRED; the word holds three ten-bit `actmsg` lines (hit, failed at a party member, failed at a monster) |
| `+0x28` | the killing lines and the critical's |
| `+0x2C` bit 27 | its blows chain into a combo (`func_ov024_021ea584`) |
| `+0x30`, `+0x32` | `s16` levels a stat and its rider move |
| `+0x34` | the plural's offset |

The range tables sit beside each half: `actdamage_a.nat` holds **17** records and
`actdamage_b.nat` **107**, after a `u32` count (`4 + 17 × 8 = 140`). A record: `+0x00` bits
0–7 the index (**read here as 8 bits**: the wiki's `+0x00` bits 8–17 spread implies the index
below them), bits 8–17 the spread; `+0x04` bits 0–9 a monster's amount, 10–19 a party
member's least, 20–29 their most. The index space is shared by both halves and need not be
contiguous: `_a` indexes 16, 17, 18, 19, 33, 49, 50, 80, 81, 84–89, 103, 133 and `_b` 1 to 106
less those. **Index 103 is present in both tables, with the same record.**

`actname.nat` names the actions by number in the system-strings layout: 682 records of eight
bytes, the number in the first word's low 12 bits, the name's offset at `+0x04`.
681 numbers are the actions' (1 to 945); the first record is numbered 0 with the empty name and
carries 57 above bit 12 (not established).

**Checks over all 681**: numbers unique across both halves; every number named in
`actname.nat`; 37 `_a` actions and 117 `_b` actions carry a range, every index present in its
half's table, and every range record is some action's (17 of 17, 107 of 107). The plain attack is 1: 0 MP, kind 1,
element 8, dodgeable, blockable, tension, combo. Heal is 30: 2 MP, range 16 (35 ± 5, party
least 35, most 160), scaling by magical mending. The medicinal herb is 255: range 49 (35 ± 5,
no scaling).

**[unknown]**: `+0x08` bits 10–13 and 27–28; `+0x1C` bits 14–18; the rest of each record;
what the 66 damage handlers past slot 0 do; the trait behind `+0x10` bit 16; reach 6 and 8.

## Spells — `spelltable.bin`

A tagged data table, 2,576 bytes, 175 records: a `0x65` record (0), a `0x64` record (20) as
the level tables open, **65 `0x66`** records (place, action) and **108 `0x67`** records
(vocation, place, level). Places run 0 to 65 with 25 missing; three entries name action 0
(places 61, 62 and 65). The listed actions are in the action tables (62 of 65; the three 0
are none). Learned vocations: 2, 3, 5, 6 and 8 to 12, levels 1 to 78; every learned place is
listed. Confirmed on the Minstrel (6): Heal (place 26) at level 3.

## Equipment battle parameters — `itembtlprm.nat`

A `u32` count in its **low 20 bits** — 1,423 (`func_0209a088` masks it so) — then 44-byte
records; `4 + 1,423 × 44 = 62,616`, the file's size. Records are sorted by the `s16` item id
at `+0x28`, strictly ascending from 994 to 22,290; `+0x2A` is 0.

- `+0x00`: flags. Bit 16 an experience bonus; bits 23–25 the weapon's element (1–7 stand for
  themselves, 0 for 8, the plain Attack's); the rest not established.
- `+0x08`–`+0x10`: twelve 6-bit signed fields, a weapon's killer bonus by monster family 1 to
  12, in tenths (10 none), through the accessors of the wiki's table.
- `+0x14`: twenty `i8` resistance additions, one an element, in the order
  `1 2 3 4 5 6 7 9 10 11 12 13 14 15 16 17 18 19 20 21` (8 and 22 absent).

**Measured over the 1,423**: 928 records hold 10 in all twelve family fields, 245 hold 0 in
all, 250 hold 11 or 12 for one or two families, 183 carry a resistance. 23 records carry an
element of 1 to 5. The resistance values are whole multiples of five, mostly negative.

## Field encounters — `encfld.bin`, `encbtl.bin`, `fld_mondata.bin`

All three are tagged data tables. Each opens with two records whose values are 0 and 20 (the
tags differ from file to file: 0x65/0x64 in `encfld` and `encbtl`, 0x66/0x67 in
`fld_mondata`); not established and ignored.

### `encfld.bin` — by map

1,830 records: **210 `0x69`** map groups (five values; the first the map's own id of
`maplist9.bin`, the other four 0 on all 210), **287 `0x68`** zones, **287 `0x66`** zone words,
**1,044 `0x67`** roamers. All 210 map ids are in `maplist9.bin`; the zone numbers are unique
across the file.

A roamer's word: bits 0–11 the monster's number, bits 12–14 its weight among the zone's
(INFERRED); bits 15 and up are 0 on all 1,044. Its second value is an integer on 606 records
and a **float** on 438; not established (the wiki notes 0.85 and 0.9 in `F06`).

A zone word: low three bits a kind (INFERRED) — 0 on 38 zones, 1 on 38, 2 on 211; bits 3–7
and 21–23 take many values and are not established; bits 24–31 are 0 on all 287.

### `encbtl.bin` — by zone

2,877 records: **290 `0x68`** zones, **1,058 `0x66`** roamer words, **1,527 `0x67`**
company words. The company records carry **one value** here (the wiki's "second value" is not
in the European file); the roamer records carry two, the second 0 on 1,045 of 1,058 and
something else on 13 — not established.

Both words' low 21 bits are the same three fields: bits 0–11 the monster's number, 12–14 a
weight, 15–17 the least that join, 18–20 the most. The company's upper bits are 0 on all
1,527; a roamer's continue (25 distinct values above bit 20), not established.

**Checks**: on all 287 zones `encfld` gives roamers for, `encbtl`'s roamer list is the same
monsters in the same order (the three remaining zones — 194, 305, 306 — have no `encfld`
roamers but do list some in `encbtl`). Least ≤ most on all 1,527 company words.

### `fld_mondata.bin` — per monster

441 records: the opening pair, a `0x64` record holding **438**, then 438 `0x65` records of
seven values: number, level (INFERRED), run-away margin (INFERRED), a packed word (not
established), a float speed (INFERRED), attack, defence. Level and margin are negative on the
metal family (−99). The numbers are the battle data's in the same order and attack/defence
equal it on all 438.

**[unknown]**: how a map chooses among its zones; `encfld`'s roamer second value; the
`encbtl` roamer's upper bits; zone word bits 3–7 and 21–23; `fld_mondata`'s packed word.

## The chapter 2 slice (v0.2)

Measured on this ROM, from `dq9re encounters`:

| map | id | groups |
|---|---|---|
| `F01` Angel Falls Region | 20001 | zones 12 (kind 0: 1 slime 7, 56 teeny sanguini 6, 57 cruelcumber 5, 63 sacksquatch 3), 14 (kind 1: same monsters at 4-4-4-5), 15 (kind 2: 23 bodkin archer 5, 32 batterfly 5, 57 cruelcumber 2) |
| `S01M01` Mountain Pass | 5101 | **none** |
| `D01M01` Hexagon Lv 1 | 7101 | **none** |
| `D01M02` Hexagon B1 | 7102 | zone 32 (kind 2: 43, 20, 4) |
| `D01M03` Hexagon B2 | 7103 | zone 33 (kind 2: 7, 43, 20) |
| `D01M04` Hexagon B1-A | 7104 | zone 34 (kind 2: 7, 20, 30) |
| `D01M05` Hexagon B1-B | 7105 | **none** |

The Hexagon's first and last floors carry no roaming monsters in `encfld`, whatever their
collision records say; the roaming-monster batch must not expect a group there. The company
of `F01`'s zones (slime, cruelcumber, teeny sanguini, sacksquatch, batterfly, dracky) is in
`encbtl`, printed by `dq9re encounters F01`.

## Evidence

`crates/dq9-formats/tests/battle_rom.rs` asserts the counts and cross-checks above: 438
battle/name records with the same numbers and the name record's code equal to the list's;
345 list records; 388 number flags; code `b003a` at 300; element 8 at 100; 63 + 618 actions,
all named, every range present and used; 210 encounter maps in `maplist9.bin`, 287 zones,
1,044 roamers, `F01`'s zones and weights; 290 battle zones, roamers equal on 287, 1,527
company words with least ≤ most; 438 field monsters equal to the battle data; 1,423 equipment
records with the bonus and resistance counts; 65 + 108 spell records. The test prints the
chapter 2 table; its output is in `build/evidence/j4-battle-data/`.

## Action scripts and battle motions

Implementation: `crates/dq9-app/src/battle/motion.rs`; the scene plays the plans in
`BattleScene::act`/`flee`/`kill` (`crates/dq9-app/src/battle.rs`). Sources: the wiki
*Battle-Action-Scripts*, *Motion-Tables*, *Monsters* and *Weapon-Positions*; re-read on the
European ROM.

### Where the files are

| file | what |
|---|---|
| `data/pack_lv5/enemy.gp2/<code>.mon` | a NARC of three members: `<code>.cchr` (LZ11), `<code>.cmot` (LZ11), `<code>.bact` (plain) |
| `<code>.cchr` | the model and its first motions: `appear`, `attack0a`, `run`, `stand` |
| `<code>.cmot` | **the rest**: `attack1a`, `call`, `damage`, `death`, `escape`, `sake` |
| `<code>.bact` | the monster's action script |
| `data/pack_lv5/chara_mp.gp2/mp0200b.chr/mp0200.bact` | the hero's, whatever weapon it carries |
| `data/bin/actdef.nsarc/default.bact` | Defend (3), Flee (225/917), a call for help (347/242), a spell's fallback (504/914) |
| `data/skill/act{spl,skl,etc}.nsarc/sp%03d.bact` | spells, skills and the rest by action number |
| `data/prm/itemdt_w.gp2/itemdt_w_en.nat` | a weapon's motion set: record `+0x10` bits 12–19 (1 swords, 3 hammers, 4 knives, 5 wands, 6 spears, 7 axes, 8 boomerangs, 9 bows, 10 whips, 11 staves, 12 claws, 13 fans) |
| `data/pack_lv5/chara_mp.gp2/mp02%02d%02dbe.chr` | the hero's blow pack: `attack1a` 1..19 and `attack1b` 20..36 at speed 0.25 (`mp0200` bare-handed, `mp0201` with the copper sword) |

The **field** bodies `<code>_f.mon` have no `.cmot`: a field model carries only `appear`,
`attack0a`, `run` and `stand` (**the roaming batch must not reuse them in battle**). The
battle body alone has no `attack1a`, `damage` or `death` either; those come from the `.cmot`.

### A `.bact`

A [record table](table.md) the game runs once (`func_ov000_0216d1c4`); each record's tag is
an opcode. `1 k…` opens a section keyed by every value it carries (a key of 1 brings 2 and
219), `16` opens one keyed {1, 2, 219}, and `2`/`17` build nothing. Read on the slime's,
the Hexagoon's, the hero's and `default.bact`:

| tag | what the presentation reads |
|---|---|
| 3 | `who "name"`: a motion on the one acting (`7`); `25` is the camera's |
| 5 | `from to gap`: the lunge, fractions of the motion in thousandths, a gap edge to edge |
| 8 | `ms`: a wait (not played) |
| 9/10 | a skip: `9 id type v` jumps to `10 id` when type 0 (always) or 1–4 (gap `< > ≤ ≥`) holds |
| 11 | the end |
| 26 | `who point`: a point of a motion (thousandths, or a float); to the end when absent |
| 47 | `who alpha ms`: a fade; the flight's |
| 61 | the reaction block (the blow lands before it) |

Measured plans (the values the implementation uses; `26` after the lunge is the landing):

| script | motion | lunge | lands |
|---|---|---|---|
| hero `mp0200.bact` §1 | `attack1a` | 6% → 33%, gap 0.25 | 61% |
| slime `z000a.bact` §1 | `attack1a` | 12% → 40.7%, gap 0.3 | 58% |
| Hexagoon `b003a.bact` §1 | `attack1a` | 11% → 35%, gap 0.5 | 55% |
| slime/Hexagoon near branch | `attack1b`/`attack0a` | none | 50% |
| spells (e.g. `sp030`, `sp041`, `sp045`) | `magic` | none | the motion's end (`26 7`) |
| `default.bact` | `guard` (3), `escape` (225), `call` + `appear` (347/242) | none | — |

The branch is chosen from the gap: 9.0 for the grid's middle rows, so all three attacks above
take the far blow. The game's `79` (formation), its camera commands and its effects are not
modelled; the scene keeps the opening shot.

**Dispatch, checked on the actions used by the chapter 2 companies.** A monster's own file is
read first (the wiki's step 1), then `sp%03d.bact`, then `default.bact`. Weird Dance is action
243, kind 57, selector `+0x18` bits 12–15 = 1 (a party actor would use `actskl`; a monster
actor uses `actetc`), and **no `sp243.bact` exists** in `actspl` (84 files), `actskl` (160) or
`actetc` (67), nor a 243 section in `default.bact`; the monster's own archive does —
`z017a.bact` (the monster's code at number 43) carries a section keyed 243 playing `skill1`.
The scene therefore plays `skill1` for Weird Dance, not a log-skip.

### What the scene shows

- A blow: the motion from its first frame, once, at speed 1; the lunge is linear over the
  motion's `from`–`to`; the struck fighters play `damage` at the landing point. A fighter
  whose script has no motion, or whose archive lacks the named motion, is skipped and logged
  (`battle motion: …`), never posed by a stand-in.
- Death: `death`, then hidden once (`alive` false; a second `remove` does nothing). A fighter
  that is falling cannot start another action.
- Flight: `escape`, then hidden; a model without it goes at once.
- A call for backup: the caller plays `call`, the joined monster is added playing `appear`,
  and the joined fighter is the next monster slot, so later cues still name the right model.
- The next action of the same fighter ends its previous gesture first (back to its grid place,
  idle) and then starts the new motion: a settle's `stand` can never replace the new blow
  (`battle/tests.rs`, "a repeated command keeps the new motion playing", which asserts the
  renderer's own clip name and frame, not the timeline). A cancelled or rapidly confirmed
  command leaves nobody displaced. The original puts everyone back at the next action's start;
  restoring at the end of each motion is the same place (**[hypothesis]** for the moment).
- **The windows wait for the scene.** A window's step does not advance while the scene is
  still presenting the previous command — a blow before its landing, a reaction, a death or a
  flight (`BattleScene::presenting`, `crates/dq9-app/src/battle.rs::advance_fight`). The pad is
  dropped during that time, as the game ignores input during an action, and the generator and
  the round are untouched: only the narrative clock waits. This replaces the earlier window
  cadence that could show a damage number, a death or the next actor before the blow landed
  (the game's reaction queue, `67`, waits for the same). `HOLD` (40 ticks) is still the
  **[hypothesis]** grouping of lines into windows, not a bound on a motion.
- The hit is applied at the script's percentage of the motion (hero 61%, slime 58%, Hexagoon
  55%), with the motion's own length. `--battle-capture swing` stops on the frame the blow has
  landed and is still being shown; `damage` stops later, at the number's window. The capture
  caller (`native.rs`) passes its ROM and party weapon into `run_to_phase`, so the captured
  scene takes the hero's real motion set and a call for backup adds its model there too
  (tested: "a capture run adds a called monster").
- **Live GPU transforms.** `BattleScene::upload` transfers each fighter's world matrix as
  well as the animated geometry/materials; `App::draw_battle` uses that boundary on every
  draw. Hidden fighters are included: their zero scale must replace the existing GPU
  instance, not merely mark the CPU scene dead. A capture builds its renderer after the
  motion, so captures alone could not expose the missing live transfer. The native GPU
  regression compares a renderer created before the action with a fresh renderer at the
  same lunge and hidden-monster states; it failed with 300 differing pixels before the fix
  and passes both states afterwards. Local evidence:
  `build/evidence/j4-battle-gpu-sync/native-before.log` (`68eda57021be`),
  `native-after.log` (`b79d9b6cf677`); the original browser victory still showing the
  defeated/fled models is `web-before-ghosts.png` (`a412c6933bff`).

Motion lengths come from the `.bcfg` tables (`0x66`: frames and speed, ×4096): a motion's
ticks are `(end − start) / speed`. The ROM's `attack1a` is therefore 72 ticks (1.2 s at
60 Hz), matching the wiki's measurement.

### Evidence

`build/evidence/j4-battle-motions/` (local, sha256 prefixes):

- **During a blow** — `battle26-swing.png` (`780cc8be2a0a`: the hero at the left slime
  with "Hero attacks.", the frame the blow has landed and the `damage` reaction starts) and
  `hexagoon-swing.png` (`1d5f145219cd`: the Hexagoon on the hero, "The Hexagoon attacks.").
- **Before and after** — `battle26-before.png` (`c7ed26e23bcb`, byte-identical to
  `j4-battle-scene/battle26.png`) and `battle26-after.png` (`aeba2ddf7956`: the hero back at
  (0, 4.5), the three monsters gone, "Hero receives some experience!"); `hexagoon-before.png`
  (`dd025965b166`, likewise the older capture) and `hexagoon-after.png` (`52fa24d4e7db`:
  "Hero is wiped out!", HP 0/20).
- **After the landing** — `battle26-damage.png` (`85b1eef75e92`) and `hexagoon-damage.png`
  (`4ea1f63d4392`): the damage number's window, which the gated windows show only once the
  motion is over.
- `scripts.log` (`a53fe431cbc0`): every `battle motion:` line of those runs — the motion
  actually playing (the renderer's clip, e.g. `playing Some("attack1a")`), its ticks, hit
  percentage, lunge and gap; the hero's set; the deaths and flights "ended, hidden"; the blows
  "back at" their grid places.
- `smoke.log` (`51d8c0845cd6`): `--new-game --fight --seed 7 --headless --auto --pilot
  --headless-steps 12000` (bounded so the pilot stops cleanly), battle 26 won in 4 rounds,
  2 experience, 4 gold, no drop — the same outcome as before this batch. The unbounded pilot
  wanders after the prologue and dies at its step cap in this environment; the bounded run
  makes the battle the last thing logged.
- `parity.log` (`d8c1aa92f901`): `scripts/parity.ps1`, 2 of 2 references.

Tests: `crates/dq9-app/src/battle/tests.rs` (the body/`.cmot` split, the scripts' plans,
Weird Dance 243 as `z017a`'s `skill1`, the equipped-versus-bare-handed posed vertices, the
lunge/hit/return, a repeated command keeping the new clip and its frame, the windows waiting
for hits/deaths/flights with the engine's outcome unchanged, the `swing` capture frame and
its weapon argument, a capture run adding a called monster, death and flight once, a called
monster that appears and reacts) and `battle/motion.rs` (the skip interpreter and the
timeline).
