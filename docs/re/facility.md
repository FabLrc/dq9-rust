# Facilities: inn, church, shop and treasure

Implementation: `crates/dq9-formats/src/facility/` (`shop.rs`, `inn_church.rs`, `treasure.rs`)
and `crates/dq9-core/src/facility/` (`inn.rs`, `church.rs`, `shop.rs`, `treasure.rs`); real-ROM
tests `crates/dq9-formats/tests/facility_rom.rs` and `crates/dq9-core/tests/facility_rom.rs`.
The data are read from `data/bin/menu/shopdata1.bin`, `data/prm/itemdt_*.gp2`,
`data/scenario/str_in<k>.gp2`, `data/scenario/str_ch<k>.gp2` and
`data/scenario/treasure.nsarc`; the rules cite the game's code.

Everything below was checked on the European release (`YDQP`). The wiki pages read are
*Items* (shops, the two prices), *Inns-and-Churches*, *Time-Of-Day*, *The Quester's Rest*,
*Treasure*, *Getting-Around* (§ locked chests) and *Text events*. Wiki addresses are the USA
release's unless the page marks them **EU only**, as this one does; the code was re-read
through the [dqix-decomp](https://github.com/DQIX/dqix-decomp).

## How a talk line opens a facility

`func_0206f550` strips a service tag off a talk line and `func_0206f6cc` switches on the code
it leaves (wiki *Text events*): `<INN=n>` and `<CHURCH=n>` open the files `str_in<n − 1>` and
`str_ch<n − 1>`, `<SHOP=n>` the shop numbered *n*. The tag number is the shop's own number
(value 0 of its record), with no indirection.

Chapter 2's talk files are `M01B0`, `S01B0` and `D01B0` (letter B is chapter 2: `TalkTask_Update`,
`ov_017::021b8e8c`, applies `"ABCDEFGHIJSTKLMNOPQ"[chapter − 1]`). Counted over the English
files (`cargo dq9re text`, then a search for the tags):

| file | `<SHOP=32>` | `<INN=1>` | `<INN=2>` | `<CHURCH=1>` |
|---|---:|---:|---:|---:|
| `M01B0` (Angel Falls) | 3 | 8 | 6 | 6 |
| `S01B0` (Mountain Pass) | 0 | 0 | 0 | 0 |
| `D01B0` (Hexagon) | 0 | 0 | 0 | 0 |

The Mountain Pass and the Hexagon have no shop, no inn and no church. `M01A0` (chapter 1) also
carries `<INN=1>`, `<INN=2>`, `<CHURCH=1>`, `<SHOP=32>` and one `<CHURCH=3>` (the
adventure log on the village table, `str_ch2`, per the wiki); the facility a map opens
therefore depends on the chapter, not on the map alone.

`<INN=1>` and `<INN=2>` are both Ivor's (NPC 7 of the village): the first opens `str_in0`, the
second `str_in1`, and both files ask **5 a head**. `<CHURCH=1>` opens `str_ch0`; `<SHOP=32>` is
shop 32.

## Chapter 2: what the application needs

Map ids: Angel Falls `M01M01` to `M01M12` are 1100 to 1112, the Mountain Pass `S01M01` is
5101, the Hexagon `D01M01` to `D01M05` are 7101 to 7105 (`docs/re/maplist.md`).

| | Angel Falls (1100–1112) | Mountain Pass (5101) | Hexagon (7101–7105) |
|---|---|---|---|
| shop | **32**, 18 items, rate 100, kind 3 | — | — |
| inn | `<INN=1>` → `str_in0`, `<INN=2>` → `str_in1`, 5 a living head; greeting 1000/1001, menu 0/1/2, too poor 1020, resting 1010, rested 1011, leave 1090; style 0 | — | — |
| church | `<CHURCH=1>` → `str_ch0`: Confession, Divination, Resurrection, Purification, Benediction, Nothing; prices 1061, too poor 1063, done 1070 | — | — |
| containers | 12, all drawing: 4 cupboards, 4 barrels, 3 pots, 1 blue chest | 1 red chest (gold ring) | 9: 5 red chests, 2 blue chests, 2 pots |

The village shop's 18 items and what it asks (rate 100, so the ask word as it stands), in stock
order — the numbers the shop window prints:

| item | asks | item | asks |
|---|---:|---|---:|
| medicinal herb 22000 | 8 | leather shield 21291 | 90 |
| antidotal herb 22004 | 10 | bandana 12410 | 45 |
| chimaera wing 22019 | 25 | plain clothes 13005 | 30 |
| copper sword 20004 | 150 | wayfarer's clothes 13095 | 70 |
| soldier's sword 20005 | 240 | leather armour 13101 | 180 |
| leather whip 20300 | 95 | cotton gloves 15003 | 50 |
| feather fan 20100 | 110 | boxer shorts 16094 | 30 |
| pot lid 21296 | 40 | cotton trousers 16213 | 80 |
| | | sandals 17406 | 18 |
| | | leather shoes 17404 | 40 |

### The containers

The chapter 2 maps' records, from `treasure.nsarc` (number = the game-wide numbering, id = the
container's own). Positions are in the maps' units and omitted here; `facility_rom` checks a
few. A cupboard's record has three values and so no place of its own (its cabinet pairs it).

| map | number | id | container | contents |
|---|---:|---:|---|---|
| `M01M03` | 16, 17 | 189, 190 | cupboard, cupboard | rank 16, rank 14 (`randTTT`) |
| `M01M04` | 18, 19 | 288, 289 | pot, pot | rank 6, rank 6 |
| `M01M09` | 20 | 191 | cupboard | rank 16 |
| `M01M07` | 21 | 290 | pot | rank 2 |
| `M01M07` | 22, 23 | 3, 4 | barrel, barrel | rank 2, rank 1 |
| `M01M10` | 24 | 192 | cupboard | rank 14 |
| `M01M08` | 25 | 566 | **blue chest** | rank 1 (`randTBox`) |
| `M01M08` | 26, 27 | 5, 6 | barrel, barrel | rank 1, rank 3 |
| `S01M01` | 28 | 150 | red chest | item 18007, a gold ring |
| `D01M01` | 29 | 7 | red chest | item 21291, a leather shield |
| `D01M02` | 30 | 568 | blue chest | rank 1 |
| `D01M03` | 31 | 8 | red chest | item 20100, a feather fan |
| `D01M03` | 32 | 9 | red chest | 210 gold |
| `D01M04` | 33 | 569 | blue chest | rank 2 |
| `D01M05` | 34, 35 | 10, 161 | red chest, red chest | item 22048 (a seed of deftness), item 13102 (a scale armour) |
| `D01M05` | 36, 37 | 564, 565 | pot, pot | rank 7, rank 5 |

No chapter 2 chest is locked. The chapter 2 red chests' flags are therefore
`0x212 + id`: 0x2a8 (150), 0x219 (7), 0x21a (8), 0x21b (9), 0x21c (10), 0x2b3 (161).

## Shops — `data/bin/menu/shopdata1.bin`

A tagged data table: the date strings, a `0x66` record holding 37, and 37 `0x67` records of
22 integers (wiki *Items*, § Shops; the reader `facility/shop.rs`). Measured over the 37:

| value | meaning | measured |
|---|---|---|
| 0 | the shop's number, what `<SHOP=n>` names | 0 to 37, **31 absent** |
| 1 | not established | 1 to 5; 3 on 11 shops, 4 on 8, 1 on 11, 2 on 6, 5 on 1 |
| 2–19 | eighteen item ids, 0 empty | 1, 6, 12 or 18 filled (18 on most) |
| 20 | a price rate in percent, **INFERRED** | 100 on 35 shops, 500 on 2 (12 and 34) |
| 21 | the kind of shop, **INFERRED** | 0 on 8, 1 on 10, 2 on 10, 3 on 6, 4 on 2, 5 on 1 |

Shop 12 (12 items) and shop 34 (6 items: the herbs, the wings, the seeds and the rings) are the
rate-500 ones. Every item any shop sells has a price above 0 in the item tables — checked over
all 330 sold ids.

### The two prices

An item record (`itemdt_<category>_<language>.nat`: a 12-byte head, then 32-byte records,
`docs/re/item.md`) carries the id as `s16` at `+0x18`, **what a shop gives** as `u16` at
`+0x1a` and **what it asks** at `+0x1c`, a price or a code on the given price (the wiki
numbers these `+0x06` and `+0x08` from a record starting four bytes earlier):

| ask word | what a shop asks | the village's |
|---|---|---|
| `0xFFFF` | twice the given price | 11 of its 18 |
| `0xFFFE` | twice, and one | 2 |
| `0xFFFD` | twice, less one | 1 |
| `0xFFFC` | ten times | 4 |
| any other | the number itself | — |
| 0 | no shop sells the item | — |

Checked: the medicinal herb gives 4 and asks 8; the chimaera wing gives 12 and asks 25; the
leather whip gives 48 and asks 95; the copper sword gives 15 and asks 150 — the four codes,
with the prices of the wiki's let's play and the Signature Series guide. A shop's rate
multiplies what it asks (**[hypothesis]**: the wiki leaves open whether it also touches what
a shop gives, which this model does not).

**Rules** (`dq9_core::facility::shop`): a stack holds 99 copies (`func_0207c378`) and the bag
152 slots; buying takes the gold and fills one stack, and refuses a quantity past 99, a price
past the gold, or a stack that will not fit the bag — **[unknown]** whether the game offers a
partial purchase instead. A sale gives what a shop gives for each copy (the given word) and
takes the copies from the bag (`Bag_Remove`, `0x020a0a18`, behind `Bag::remove`), the gold held
to 9,999,999.

## Inns — `str_in<k>.gp2`

A `.nat` file in the system-strings layout (wiki *System Strings*; the reader is
`battle::text::MessageTable`, reused by `facility/inn_church.rs`). Currently 15 files,
`str_in0` to `str_in14`, so `<INN=1>` to `<INN=15>`.

| message | |
|---|---|
| 0, 1, 2 | the menu labels: Stay Overnight, Rest, Cancel |
| 50 | the keeper window's style — 0 on 13 files, 2 on 2, 3 on the bed of `str_in14` (the wiki's INFERRED narration) |
| 1000 | the greeting by day: `<val_1>` the beds, `<val_2>` the price |
| 1001 | the greeting at night |
| 1002 | the greeting of a guest in another world's inn |
| 1010, 1011 | as the screen goes black, and after the rest |
| 1020 | too poor |
| 1090 | leaving |
| 10000 | **the price a head**, as decimal digits |

**Measured:** 5 in `str_in0` and `str_in1` (Ivor's), 4 in `str_in2` and `str_in3`, 5 to 12
elsewhere, 0 in `str_in14` (the soft bed, `<INN=15>`). The price is a head's price times the
**living** (`0x0215d0d4`–`0x0215d108`; a member with HP 0 needs no bed, `func_02061bd8`). The
Quester's Rest counter charges a flat 3 a head, a constant in code (`0x0215d0c4`), and its
words are `str_rki`, not a `str_in` (v0.3).

**EU only:** in `str_in3` to `str_in13`, line 1000 prints the price as `<val_1>`, the bed count,
where its talk lines' copies say `<val_2>` — checked in all 11 files; the wiki reads it as a
slip of the service's text (**INFERRED**).

The flow (`func_ov003_0215c924`, overlay 3): greeting; the menu — **Stay, Rest, Cancel by
day; at night Stay and Cancel** (night is the night phase alone, morning and evening counting
as day); Stay or Rest pays, black, jingle 55, a hold of 180 ticks, **the living's HP and MP
full** (never the dead raised, nor poison or a curse touched), the clock to the **day's start
(210 s) for Stay** and to the **night's start (0 s) for Rest** (`GameState::SetTimeOfDay`,
`0x02010374`; wiki *Time-Of-Day*, § What sets it), then 1011; Cancel says 1090. The talk line
says Rest is "until evening", which is the text's word for it, not the clock's value.

## Churches — `str_ch<k>.gp2`

Eight files, `str_ch0` to `str_ch7`, so `<CHURCH=1>` to `<CHURCH=8>`. Labels 0 to 5 are the
menu — Confession, Divination, Resurrection, Purification, Benediction, Nothing — and **an
empty label leaves the service out**. Measured over the English files:

| file | services missing |
|---|---|
| `str_ch3` (the Observatory) | Resurrection, Purification |
| `str_ch7` | Confession (a table with no priest, the adventure log) |
| the other six | none |

The words: 10 the status window, 11 "On whom?", 12 the member's name, 13 Dead, 14 Poisoned,
15 Cursed, 20/21 Yes/No, 31 to 42 the vocation names (`30 + the vocation's number`; message 30
itself is in no European file, so the Guardian has none — **not established**), 1020 to 1023
the divination, 1030 to 1032 Resurrection, 1040 to 1042 Purification, 1050 to 1052
Benediction, 1061 the price, 1062 refused, 1063 too poor, 1070 back to the menu, 1100 a
coffin brought in, 1081/1082 waking after a stay.

The prices, by the member's level in the vocation they stand in L
(`0x0215a8e0`–`0x0215a99c`):

| service | price | check |
|---|---|---|
| Resurrection | ⌊(L² + 20) / 20⌋ × 10 | 10 at L 1, 60 at 10, **460 at 30**, **4,910 at 99** |
| Purification | 5 | — |
| Benediction | 30 × L | 360 at 12 |
| Divination | free | — |

**Effects.** Resurrection fills HP and clears every status bit but the curse
(`func_02048150`), MP untouched. Purification clears the poisoned bit (its line 1041 says
poison; **[hypothesis]** that this is its whole effect). Benediction clears the cursed bit;
the game also takes every worn item whose slot word has bit 18 into the bag (INFERRED cursed),
which **no reader here holds**: the unequipping is **not modelled** and which items carry the
bit is **[unknown]**. The status word is `[member+0x130]+0`: bit 0 dead, 1 poisoned, 2 cursed;
death is the member's HP being 0, and what sets the other two is **not established**. The
caller keeps the two bits beside the party (`facility::church::MemberStatus`).

**Confession is v0.3** (it saves the adventure log, and "No" ends the game): `church::offer`
returns `Offer::Unavailable("not available in this build")`.

**Divination**: free; line 1021 with the experience wanted when the next level's row is above
the member's experience, 1022 at level 99, **[hypothesis]** 1023 when the experience for the
next level is already held (the level is taken after the battle, `func_ov023_021f0a5c`).

## Treasure — `data/scenario/treasure.nsarc`

268 members: one `<map>.bin` per map with treasure, plus `randTBox`, `randTD` and `randTTT`.
Each map member is a tagged data table: the date strings, a `0x66` record holding **the
game-wide number of the file's first treasure**, then `0x67` records of 3, 5 or 6 values
(wiki *Treasure*; the reader `facility/treasure.rs`).

**Measured over the 268:** 265 map members (the three random tables have no `0x66`) and 847
treasure records, whose spans run **0 to 846 without a gap or an overlap**; 151 records have 3
values, 464 have 5 and 232 have 6. The wiki's counts (145/448/228) were taken while `M09M05`
and `D13M02` were misread as empty: each holds 16 records, its first word being the record
count 16, which a decompressor takes for an LZ10 header.

Value 0 packs **the container's id** in its high half and, in its low half, the item, the gold
or the rank. Value 1 is the kind word, read by `LootManager_CreateContainer` (USA
`0x0207ba90`, `src/World/LootableContainer.cpp` of the decompilation):

| bits | meaning | measured over 847 |
|---|---|---|
| 4–6 | 0 red chest, 1 pot, 2 barrel, 3 cupboard, 4 blue chest | 167, 279, 185, 151, 65 |
| 2–3 | 0 nothing, 1 gold, 2 item, 3 monster | 686, 16, 145, 0 |
| 0–1 | 0 none, 1 a thief's lock, 2 a magic lock | 841, 6 (all thief), 0 |

No record holds a monster: the three mimics come from the random tables alone. The six locked
chests are all a thief's lock (`C01M16`, `M05M05`, `M08M07`, `M09M05`, `M09M14`, `S08`; wiki
*Getting-around*). A locked chest returns `Locked(kind)`; the unlocking (the ultimate, the
thief's and the magic keys, lines 41 and 43) is not modelled.

Values 2 to 4 are the position, in the maps' units, and value 5 a facing (INFERRED radians);
one position or facing may be written as a whole number and another as a float — `M01M08`'s
blue chest has `0` for its facing. The three-value records have no position: value 2 is the
room's cabinet less one in the village (**[hypothesis]**, wiki *Treasure*, § Layout).

### What remembers an open

`func_ov017_021adcb0` (flags at `0x021ae528`–`0x021ae58c`) sets **`0x212 + id` for a red
chest**, for ever, and **`0x79e + id` for anything else**; the field's start clears the 700
flags from `0x79e` (`func_ov017_0218b688`, `0x0218c180`: `func_0206dfe8(bank, 0x79e,
0x2bc)`), exactly the ids 0 to 699 of the non-red containers. So a red chest never refills and
a pot, barrel, cupboard or blue chest does, at every game start. `Treasure::flag` returns the
index; `facility::treasure::field_reset` the range to clear.

### The random tables

`randTBox`, `randTD`, `randTTT`: a `0x6A` record with the row count and one `0x69` row per
line, each row one word — bits 26–31 the rank, 23–25 what it gives (1 gold, 2 item, 3
monster), 7–22 the gold, the item's id or the monster's number, 0–6 its weight (wiki
*Treasure*, § Random treasure).

| table | rows | ranks | weights a rank |
|---|---:|---|---|
| `randTBox` (a blue chest) | 68 | 1–5 | 100 |
| `randTD` (no village treasure's) | 162 | 1–10 | 100 |
| `randTTT` (a pot, barrel, cupboard) | 98 | 1–20 | 20 to 50, the shortfall the chance of nothing |

A draw is `func_02032370(100)` — the game's `rand_below(100)` — taken against the rank's rows
in file order (`LootDistribution::Sample`), the weight summed until the draw falls under it;
past the rank's total, nothing. Rows 38 to 40 (a cannibox, a mimic, a Pandora's box) are a
battle, not a gift: `Opened::Monster`. Measured: all ten monster rows are in `randTBox` ranks 4
and 5 and `randTD` ranks 3 to 10, none in `randTTT`; `randTBox` rank 1 is ten rows summing to
100, so a blue chest always gives something.

## Open questions

- **Which generator draws**: this model draws from the **world** generator
  (`GetBTRandom`, `0x020741e0`; a `dq9_core::battle::Random` and `next_max(100)`), where the
  wiki names `func_02032370(100)` and `docs/re/encounter.md` reads that function as the C
  library's `rand`. **[hypothesis]**: which of the two the cartridge drains is open.
- **When it draws**: the cartridge draws a container's contents when the map loads (one draw a
  container, `LoadZoneContainers`), so the draw sits in the stream before the open; here the
  draw is made at the open. Same distribution, different generator position.
- The kind-0 `0x0` records (six of them, value 0 all 0) and what the high half of value 0 is.
- Which kind is a chest, a pot or a barrel: only `0x30` = cupboard is read, `0x10` pot and
  `0x20` barrel are INFERRED.
- What `unknown_2` counts outside the village; whether a rate touches what a shop gives;
  whether the shop offers a partial purchase; which items carry the cursed slot bit.
- The Quester's Rest's own flow (v0.3), and the inn's guest lines (no session here).

## Symbols

Names to add to `re/symbols.txt` (not applied by this batch; an address must be a function
start in `re/functions.txt`):

| address | name | source |
|---|---|---|
| `0x0207ba90` (USA) | `LootManager_CreateContainer` | wiki *Treasure*, read in `src/World/LootableContainer.cpp` |
| `0x021adcb0` (overlay 17) | `Loot_OpenContainer` **[proposed]** | wiki *Treasure*, flags at `0x021ae528`–`0x021ae58c`, lock at `0x021ade84` |
| `0x0218b688` (overlay 17) | `Field_ClearContainerFlags` **[proposed]** | wiki *Treasure*, `0x0218c180` clears 700 from `0x79e` |
| `0x02032370` | already `RandBelow` (`0x02032380`) | `re/symbols.txt`; the draw the tables use |
| `0x0215c924` (overlay 3) | `Inn_Function` **[proposed]** | wiki *Inns-and-Churches*, `func_ov003_0215c924` |
| `0x02158e94` (overlay 3) | `Church_Function` **[proposed]** | wiki *Inns-and-Churches*, `func_ov003_02158e94` |
| `0x0215d0c4` | `QUESTERS_REST_PRICE` **[proposed, data]** | wiki *The Quester's Rest*: the flat 3 |
| `0x020a0a18` | already `Bag_Remove` | `re/symbols.txt`; `Bag::remove` |

The overlay-3 and overlay-17 addresses are the wiki's, and overlays are identical in both
releases (`AGENTS.md`); they are still unconfirmed against `re/functions.txt`.

## Evidence

- Counts over all records: 37 shops, 22 integers each, numbers 0 to 37 but 31; 330 sold item
  ids, all priced; 1,178 item name and price records; 15 inn files × 5 languages; 8 church
  files × 5 languages; 268 treasure members, 847 records, spans 0 to 846.
- Prices: the four ask codes checked on the village shop's 18 items (the let's play's 8 for the
  herb and 150 for the copper sword); the resurrection formula against the wiki's 10, 60, 460
  and 4,910.
- Tests: `facility_rom` (both crates) read every member of the five files above; the chapter 2
  containers' count, kinds and item names; a seeded blue-chest draw inside `randTBox` rank 1;
  an inn stay on a four-member synthetic party at Ivor's; a sale and a purchase at the village
  shop.
- Local evidence (`build/evidence/j4-facilities-core/`, `manifest.json`): `verify.log`
  (`5ee30c6b5177`), the whole `verify.ps1 -RequireRom`; `facility-rom.log` (`e27b6012c817`),
  both `facility_rom` tests; `chapter2-facilities.txt` (`8d7c1878650b`), the tags, the village
  shop's prices, the inns', the churches' and the chapter 2 containers, printed from the ROM by
  `build/facility/chapter2.py`.
