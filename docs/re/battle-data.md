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
