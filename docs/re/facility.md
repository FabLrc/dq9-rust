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

### After a wipe-out — `data/scenario/chur_messet.bin`

`func_ov017_0219bfb4` (mode 2) is the map request the wipe-out makes; once the revival map is
loaded it picks **which priest speaks**. The file has 21 tagged records: three headers
(`0x65` date, `0x64` time, `0x66` count 18) and **18 `0x67` map records**, each a map then
**three spans of four strings**: the lowest and highest story major the span covers, and the
voice by day and by night. The first span that holds wins; a span whose two ends are `0` (or
empty) holds for every major, which is how the last span works as a catch-all. An empty voice
string is 1 (`0x020d3018` tests the string, `0x02005a94` is `atoi`; `Save_IsDaytime` chooses
the column). **Voice 3 says nothing**; any other voice runs overlay 3's church in mode 2
(`ov_017::021ba998`: `ov_003::02158bb8` builds it, `02159174` sets the mode, `+0x5a0` holds
the voice, `02158e94` steps it), whose state 10 (`ov_003::0215af9c`) shows
`str_ch<voice - 1>` message **1080 + mode = 1082**, and whose state 11 ends the visit. Voice 0
reaches `str_ch0` too (the task clamps `voice - 1` to 0). A map with no record uses voice 1
(the wiki's "the rest").

The European voices, read from the file:

| map | story majors | day / night voice |
|---|---|---|
| 1106, Angel Falls | 1 only | **3 (silent)** |
| 1106 | 2 to 17 | 1 |
| 216, Gleeba | 1 to 16 | 1 / 2 |
| 216 | 17 | 1 |
| 1800, Dourbridge | any | 5 |
| 2005, Batsureg | any | 6 |
| 406, Wormwood | any | 7 |
| 5700, Gortress | any | 7 |
| 4506, the Observatory | any | 4 |
| 4106 | any | 4 |
| 2205, 5801 | any | 2 |
| 109, 1202, 1307, 1506, 1903, 2104, 2309, 4201 | any | 1 |

A record's third span is the catch-all `0 0 0 0`, so major 18 and above at 1106 give voice 0,
which the task turns into `str_ch0` as well.

**No line announces the halved purse.** `func_02010604` halves it (`lsr #1`) and says nothing;
the church files' messages 1080 to 1083 are only 1081 and 1082, both prayers, so a "gold
line" does not exist on the cartridge. The application's `status` log prints the amount; the
game does not.

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
poison; **[hypothesis]** that this is its whole effect). Benediction clears the cursed bit and
takes every worn item whose record's `+0x08` word has bit 18 into the bag — the exact rule,
the ten European items and the absence of a line are in "Benediction's unequipping" below.
The status word is `[member+0x130]+0`: bit 0 dead, 1 poisoned, 2 cursed;
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

Values 2 to 4 are the position, **in world units** (the map's placements and collision), and
value 5 a facing (INFERRED radians); one position or facing may be written as a whole number
and another as a float — `M01M08`'s blue chest has `0` for its facing. The loader scales them
itself: `LootManager_CreateContainer` (EU `0x0207baa0`) copies value 0 and the kind, reads
values 2–5 as `Script::Parameter`s (three for a pot or a barrel, four for the others) and
converts each to `fx32` — `Int << 12`, or `Float × [0x0207bcd0]` = `4096.0` — before it
registers the object. The reader `coordinate()` therefore keeps the values as they are; a
division there would be wrong.

Checked against the maps' own collision (`CollisionWorld`, built from the models, independent
of the treasure file), floor at `(x, z)` versus the record's `y`: `M01M08` #25
`(4.32, 1.17, -3.02)` → 1.170; `M01M07` #21 `(-4.05, 0.16, 2.65)` → 0.160; `D01M01` #29
`(22.99, 1.26, 0.08)` → 1.260; `M01M04` #18 `(-2.93, 0.15, 4.86)` → 0.155; `D01M03` #32
`(0.01, 0.95, 22.16)` → 0.947; `C01M16` #72 `(13.75, 0.1, -37.03)` → 0.100. Residuals of the
four candidate scales, `|floor(x/S, z/S) - y/S|`: raw ≤ 0.005 on all six; `/4096` 0.099 to
1.259 (no floor at all for `C01M16`); `/256` 0.097 to 1.254; `/64` 0.080 to 1.241. The nearest
`.bmdj` placement is always the block's base model at the origin, which is why a
nearest-placement metric would favour `/4096`: the containers are not map placements but
`LootableContainer` objects made by that loader, so the floor test is the independent join.
Evidence: `build/evidence/j4-field-containers/scale-residuals.log` (`c135a8c6a220`, the six
records and four scales) and `scale-loader.log` (`43b61c1d3064`, the loader's `<< 0xc`, `_fmul`
and `ToFloat` arms).
The three-value records have no position: value 2 is the room's cabinet less one in the
village (**[hypothesis]**, wiki *Treasure*, § Layout).

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

## The field menu and the facility screens (application)

Implementation: `crates/dq9-app/src/app/services.rs` (the screens and their state),
`app/services/transfer.rs` (bag ↔ held slots, equipment) and `app/services/tests.rs`; the
service tag is caught in `crates/dq9-app/src/talk.rs`. The rules stay in `dq9_core::facility`;
this layer is the ROM tables, the cursors and the commands' effect on `Party`.

**Opening a facility.** `Talk_SelectLine`'s chosen text is scanned for `<INN=n>`,
`<CHURCH=n>` or `<SHOP=n>` before `Msg_Compile` drops the tag
(`dq9_formats::message::SKIPPED`). When the line ends (`<END>`, `<CLOSE>` or `<ADD>`), the
conversation's `Ended` is held back and the screen of that number opens; trigger event 1 and
the talk counters fire only after the screen closes, which is the order of the game's talk
task. The map's talk triggers, conditions and counters are unchanged.

**Field menu.** `I` on the field (not while a conversation, event or battle runs) opens:
Items, Equipment, Attributes, Spells, Close. `Enter`/`Space`/`Z` confirm, `Backspace`/`X`
cancel, the arrows or `WASD` move. Movement and roaming are frozen while any menu or service
screen is open, and a map change or a replaced session closes the menu and clears its input.

**Containers.** Implementation: `crates/dq9-app/src/app/services/containers.rs`; the rules
are `dq9_core::facility::treasure`. A map load reads `<map>.bin` of `treasure.nsarc` and the
three tables. The action key with no conversation, event or menu tries a container **before**
a conversation; the target is the nearest record with a position inside the talk partner's
1.5-unit square and within a right angle of the hero's facing (`treasure::nearest`), earlier
in file order on a tie. **[hypothesis]**: the cartridge reaches a container as an examine
object; the treasure record carries no talk zone, so the port uses the talk partner's range
and facing on the ground plane.

The join is the record's **world** position (above): the map's collision floor at its `(x, z)`
is at its `y` (residual ≤ 0.005 on `M01M08`, `M01M07`, `D01M01`, `M01M04` and `D01M03`), and
the ROM test `a_record_position_is_the_world_position_of_its_floor` pins it. The `.bmdj` has
**no placement at a treasure's position** (checked on `M01M07`, its pot and two barrels,
`M01M08` and `D01M01`): the cartridge's `LootableContainerManager` creates the container at
run time from the record (`LootManager_CreateContainer`, which converts the record's world
values to `fx32` itself). The port draws no container model, so there is no placement to hide;
the flag alone stops a second open in the same session. A cupboard's record has three values
and no position; the cabinet join is **[hypothesis]**, so those records are counted in the
load line and never reachable.

The open draws from the **world** generator (`App::world.random`, `GetBTRandom`,
`0x020741e0`) at the open, the same choice as the field's item and spell actions; the
generator is **[hypothesis]** (the core rule, § "The random tables" and its two open
questions). The `--screenshot --input` preview has no field world and uses `Random::new(0)`,
which its line prints. A red chest sets `0x212 + id` for ever; a pot, barrel, cupboard or blue
chest sets `0x79e + id`, and `treasure::reset_field` clears the 700 at the field's start (a
session start restores a save's flags, then clears them). The gift is applied to `Party`:
gold to the purse, an item to the bag with its obtained flag (`State::obtain`); a full bag is
reported and the flag stays set. A **locked** chest shows its lock and sets no flag (the keys
are not modelled). A **monster** row (the mimics, 38 to 40) opens nothing and gives nothing:
it is shown and logged, no battle starts, and its flag is cleared so the container stays and
the unmodelled battle is not swallowed; the container battle's stage and event request are not
read. The window's text is a placeholder line (`Taken::text`), not the event script's words.

Tests: the core unit tests cover the selection, one-shot vs refill, locked refusal and an
empty rank; the ROM-backed tests in `app/services/containers.rs` read the village pots
(`M01M04`), the leather-shield chest (`D01M01`), the 210-gold chest (`D01M03`), a locked chest
(`C01M16`), a short `randTTT` rank and the cupboards of `M01M03`, and assert the flags, bag
and purse rather than any wording.

Evidence (`build/evidence/j4-field-containers/`, manifest `manifest.json`): gold chest
`chest-gold.png` (`622c34bfdc02`), leather-shield chest `chest-item.png` (`dbf34a24402b`),
pot `pot.png` (`74c2c054cd03`), the same red chest and pot refused again
(`chest-already-open.png`, `be691a6f1acc`; `pot-already-open.png`, `ed84f0b2e951`), the
thief-locked chest (`chest-locked.png`, `eaf8d945a066`), and the test logs (`tests-app.log`,
`75ceea88d7fb`; `tests-core.log`, `632cb77fc4ad`). The captures come from `--screenshot
--input`, the preview path: its log says the party is not updated, and the purse/bag changes
are asserted by `the_app_takes_the_gold_and_refuses_the_red_chest_again` and the item test.

**Items.** The list is the bag's non-empty slots in bag order, then the leader's eight held
slots (each one item, `7578 + 18 i`). `Enter` uses the item on a chosen party member,
`H` puts the bag item under the cursor into a chosen held slot, `T` takes a held item back.
A transfer is exactly one copy: `Bag::remove` (first stack, order kept) into the chosen slot;
a held slot that was occupied hands its item back to the bag in the same command, so a full
bag refuses the swap rather than losing a copy. `Bag::add` returns true when it clamps a 99
stack, so the transfers preflight the real destination (`bag_can_store`: an existing stack
below 99, else an empty slot) before mutating; a failed swap restores the chosen copy, and a
99 stack plus the held copy keeps all 100 copies. Removing the only copy of the chosen item
frees its slot and lets the old piece in. Duplicates and the other slots keep their order. A field use runs the item's action record through
`dq9_core::battle::resolve_action` on the target (the range table, the healing cap, no MP
cost for a herb), and the source is spent only when that action **changed something**: one
copy from the bag stack, or the chosen held slot when the item record's `+0x08` bit 19
("used up", `Item_IsConsumed` = `func_020ddb20`, EU `0x020ddb30`) is set and the field menu's
consumer runs, followed by the hole compaction (`func_0208386c`, EU `0x0208387c`). The rule is
the same as the battle's applied counter, read on the field path: `func_ov002_021536ec` runs
the action's handlers and returns whether one changed something, and only a non-zero result
reaches `func_ov002_0215a7b4`, which spends the copy (bag or held slot); the item menu itself
is `func_ov002_02157634` (overlay 2). A herb on a member at full HP is therefore **kept**, and
a revive on a living member or an antidote with nothing to cure likewise. Only field actions of
kind 2 (heal) aimed at the party are offered; other tools are refused with a message. The
field menu's own "nothing happened" line was not read; the copy is kept silently.

**Generator of a field action.** `App::service_button` passes `&mut self.world.random` into
`Services::press`, and `Services::apply` hands that same `Random` to
`dq9_core::battle::resolve_action` for both the item and spell arms. A field item or healing
spell therefore draws from the **shared world generator** — the one encounters and roaming
also use — not from a battle's separate generator (`re/battle.md`, § Generators). The
original field call site is **not established**: the item helper `func_020ddb38` (EU
`0x020ddb48`) accepts a `Random*` and uses that pointer for its 25 % draw, falling back to
`GetBTRandom()` only when it is null, but Ghidra finds its only xref from `ResolveAction`
(`ov_024::021eb5d0+0x2214`), not from the field menu; the held-slot consumption helper
`func_020ddcbc` (EU `0x020ddccc`) takes no `Random*`. Whether the cartridge's field menu passes
a world or battle generator is **[hypothesis]**; the app's shared-generator wiring is an
implementation choice. Consequence: a field heal or item advances the encounter/roaming draw
stream in continuous play.

**Equipment.** The eight slots list what the leader wears; `Enter` offers the bag's items of
that id band (helmets 12, armour 13, gloves 15, leggings 16, shoes 17, accessories 18,
weapons 19–20, shields 21) plus "Take it off". The chosen item leaves the bag and the worn one
comes back; when the bag cannot take the old piece nothing changes. The `Character` equipment
is the source of truth for battle numbers; after the menu closes `AnimatedScene::replace_hero`
rebuilds the hero in the same scene model and instance slots at its live pose and gait. The
new rig is built in a detached scene first, so a failed resource leaves the old rig and
storage untouched. Matching textures are reused, unreferenced hero texture slots are released
to 1×1 placeholders (other models' indices never move), and only textures beyond those slots
are appended. The field's cast, talk counters, doors, camera, events and generators stay as
they were. The regression `app/services/tests.rs`
(`changing_gear_rebuilds_the_hero_without_reloading_the_field`) changes torso gear through the
menu, observes the drawn vertices change, alternates two outfits 20 times with stable hero
instance, model/instance/texture counts and bounded texel bytes, checks other actors' poses
and texture references, and verifies a failed replacement is transactional; the map-load
journal, talk counter, a field flag, the world generator, the event list and the hero pose do
not change. It fails on the old `load_map` refresh (`setgear-before.log`/`setgear-after.log`,
`b043a8eb6bf4`, `5f51d1db3cb5`). The weapon and shield are not parts of `HeroLook` and do not
change the model.

**Who may wear it** — `Equip_RefusalMask` = `func_020dd4c4` (EU `0x020dd4d4`), read on the
European ROM. It returns a bitmask of reasons, zero meaning yes: `0x001` a weapon or shield
whose skill tree the character has not got, `0x002` an armour **whose vocation bit is clear**,
`0x008` a category above 7, `0x010` the record's word 0 bits 30–31 above `live[0x186 +
vocation]`, `0x020` no such character, `0x080` sex, `0x100` word 0 bits 29–30 nonzero and the
member not the protagonist, `0x200` no entry. The vocation test is `(1 << (vocation − 1)) & mask & 0xfff`,
applied only to categories 2 to 7, and a vocation of 0 (the Guardian) wears anything. The mask
is **word 1 of the item's stats entry** (the 32-byte entry the record's `+0x00` indexes, at
`+0x04`); measured over the 1,178 European records: all 313 weapons and shields have bits 0–11
clear, all 52 accessories `0xfff`, holy mail (13200) 0x100 with bit 27 (male) and holy femail
(13603) the same bit with bit 28 (female). The app reads it with the item's record (category
`+0x08` low nibble, mask from the stats entry) and refuses a confirmed unwearable piece with
`str_eq` line **10004**, "Members of this character's current vocation can't be equipped with
this item."; nothing leaves the bag. Not modelled: the skill-tree rule for weapons and shields
(the Omnivocational panels and the vocation rows at `0x020ee748`, which no save here holds),
the `0x010` rule (word 0 bits 30–31 against `live[0x186 + vocation]`; what it compares is
**not established**), sex, and the hero-only bit — the app equips the leader only, who is
the hero. The cursed bits and their lines (11004, 11005) belong to the equipment screen and
are not shown here; the worn-cursed pieces are what Benediction takes off (below).

**Attributes.** The rows show each member's level, HP/MP and the numbers
`Character::fighter` computes from the level table and worn items (attack, defence, agility,
deftness, magical might), so the page follows an equipment change immediately.

**Spells.** The leader's healing spells are the actions `spelltable.bin` learns by the current
vocation and level, filtered to kind 2 and aimed at its own side and validated against a
party fighter. `Reach::All`/`Group` (Multiheal) resolves once against every living member in
party order and skips the single-target screen; `Reach::Actor` is the leader; `One`/`Attack`
asks for one member. Dead members are never targets (the cursor skips them and a forced
Confirm shows a refusal), and a dead leader has no enabled spell or item command. A cast
spends the action's real MP cost once and applies every resolver outcome; MP is held below
what the member has. The same reach mapping drives a kind-2 field item, not an action-number
special case.

**Shop.** `<SHOP=n>` reads `shopdata1.bin` and the two price words of `itemdt_*`; 32 is the
village shop. Buy mode lists the 18 stock slots with the ask price (`rate` 100), sell mode the
bag's items with what the shop gives. `Enter` buys or sells **one** copy through
`dq9_core::facility::shop` (99 a stack, 152 bag slots, 9,999,999 gold); `Left`/`Right` switch
mode, `X` leaves. A partial purchase is still **[unknown]** and is not offered.

**Inn.** `<INN=n>` reads `str_in<n − 1>`; the menu is Stay/Rest/Cancel by day and Stay/Cancel
at night (`dq9_core::facility::inn::menu`), priced the head's price (message 10000) times the
living. The choices and their labels are frozen together and refreshed after a Stay/Rest, so
a displayed Cancel cannot become Rest. Stay sets the clock to the day's start and Rest to the
night's, heals the living's HP and MP, never raises the dead; the application's time, lighting
and trigger `time_of_day` follow. The keeper's lines 1010 and 1011 are shown in turn; on the
paid/closed notices X advances or closes exactly as Confirm does, so no stale menu is
reachable. 1020 too poor mutates nothing and 1090 leaves. The retail labels 0/1/2 and price
line 10000 are required at open (a missing or empty one is an error); an explicit price 0
(inn 15) is a valid free stay.

**Church.** `<CHURCH=n>` reads `str_ch<n − 1>`; the menu is its non-empty labels (the
Observatory's missing Resurrection and Purification, the adventure log's Confession). Divination
is free, shows its 1021/1022/1023 notice (1023 **[hypothesis]**) and returns to the service
row; `<TARGET>` is the member's name, and `<str_2>` takes the church file's vocation label
(message 30 + vocation) when it exists — the Guardian's message 30 is in no European file,
so its tag stays visibly unknown. Resurrection, Purification and Benediction ask the member
and its price (1061), then execute `church::cure` with the caller's `MemberStatus` bits
(dead = HP 0) and charge the party; a NotNeeded refusal also names the member. Confession
shows the core's "not available in this build" and nothing is saved (v0.3).

**Benediction's unequipping** — `Church_CureFlow` = `func_ov003_0215a740` (overlay 3), state 5,
read on the ROM. After `func_02048418(member, 4)` clears the cursed bit it walks the eleven
in-RAM worn entries (`member+0x150+0x194+0x20 i`). Places 2, 3 and 4 are skipped: the slot
table at `0x0217f39c` is `00 01 ff ff ff 04 05 06 07 08 09`, so they index no save slot. Every
remaining entry whose word at `+0x08` has **bit 18** set — tested as `(word & 0x7ffff) >> 18`
— goes into the bag with one copy (`Bag_Add` = `func_0207c378`, EU `0x0207c388`, count 1 and
its category) and its place is cleared (`func_02052d7c`, EU `0x02052d8c`, item id and `-1`).
The eight kept places are exactly the app's eight equipment slots. **Measured over the 1,178
European records: ten pieces carry the bit** — 12184 skull helm, 12794 Hades' helm, 13307
nightmare gown, 18031 terrible tattoo, 18032 skull ring, 18038 reckless necklace, 20311 demon
whip, 20312 archdemon whip, 20913 oh-no bow, 21393 ruinous shield; all armour, accessories, a
whip and a bow, no tool. **No line announces the removal**: the branch's own words are the
prayer 1051 before it and 1070 after; the cursed-item lines of `str_eq` (11004 "…is cursed, and
cannot be unequipped", 11005 "Oh no! … is cursed!") belong to the equipment screen. The app
passes the record's bit to `church::cure_with`; a bag with no room leaves the piece worn (the
original does not test the add, and losing it would not be observable).

**The coffin line** — `str_ch` message **1100**, "Praise be! To see that you would trouble to
bring a fallen companion all the way to our church! … we will have your companion back to
life…". All eight church files carry it; `str_ch3`, the Observatory, is the developers'
placeholder `THIS IS A BUG / THIS LINE HAS BEEN CUT`, and `str_ch4` (Dourbridge)/`str_ch5`
(Batsureg) say it in their voices — `str_ch4` names the price: "this resurrection's on the
house". The quick line is `1080 + mode` (`Church_QuickLine` =
`func_ov003_0215af9c`, state 10), so the coffin greeting is **mode 20**; the greeting reaches
state 10 only for mode 1 or 2, the builder (`func_ov003_02158bb8`) resets the mode to 0, and the
only setter found (`Church_SetMode` = `func_ov003_02159174`) is called by the wipe-out task
`ov_017::021ba998` with its own mode (2 at Angel Falls → 1082). **No code sets 20**: the coffin
greeting is unreachable in the retail flow. State 10 only shows the line and then ends the
visit (state 11); it performs **no resurrection and charges no price**, so the text's promise
is a design stub. The church's paid Resurrection (1030/1031, ⌊(L² + 20) / 20⌋ × 10) remains
the only way the modelled facilities raise the dead; the coffin line is a constant
(`facility::church::COFFIN_LINE`) with no screen of its own.

**Wipe-out.** `App::wipe_out` (battle end, `app/fight.rs`) heals, halves the purse and loads
the revival map; `App::wipe_out_arrival` (`app/flow.rs`) then reads `chur_messet.bin` through
`facility::church::VoiceTable`, and a voice other than 3 opens `Services::open_priest`: the
church's mode-2 screen with line 1082 of `str_ch<voice - 1>`, which a key closes before the
party gets control. Voice 3 opens nothing. The line keeps its bytes (`*:` is drawn, as the
game's `MsgWin_ParseSpeakerName` only sets an arrow byte for it); the window is the notice's
geometry **[hypothesis]**. The gold halving has no line (above).

**Open.** The window geometry, colours and title/footer are **[hypotheses]**: the menu uses
the field message window's rectangle grown to the DS screen, drawn on the screen the field
does not use. The field action of an item is the record's first action number, read here
because the battle's second one is the one `battle_action` uses; the field and battle records
of the medicinal herb both name action 255. Which `<Target0>`/`<val_n>` a line wants is
substituted for the service lines (member name, living, price); other name tags stay
`[tagN]`. The inn's guest lines are not modelled. The native
smoke drives real windows; the browser capture is the coordinator's.

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
  whether the shop offers a partial purchase.
- The field menu's own no-effect line (the copy is kept silently) and whether the church
  would show anything for a coffin if mode 20 were ever set.
- The Quester's Rest's own flow (v0.3), and the inn's guest lines (no session here).

- Field-menu and facility screens (`build/evidence/j4-field-services/`, native windows on
  the European ROM, `manifest.json`): `field-menu.png` (`6669e1ad98e4`), `items.png`
  (`e96b6fecfcdb`), `held-swap.png` (`86384dc0755b`) and `held-use.png`/`held-use.log`
  (`298cdf4a7336`, `e6f548be5b7b`: "field held item 22000 … slot 0 spent"), `equipment.png`
  (`f0d25fe2a8aa`), `attributes.png` (`0379109e4882`), `spells.png` (`70a5ed97296d`),
  `shop-menu.png`/`shop.log` (`9492670c4a11`, `bbc4016b7d65`: herb asks 8, refused with no
  gold), `inn-menu.png`/`inn.log` (`014f49f282d9`, `d3cabdd4452d`: "Stay price 5 for 1 bed"),
  `church-resurrection.png` and `church-result.png`/`church.log` (`92efb6ac5ddc`,
  `c9b254acc879`, `8b00f88b9f0b`: Resurrection on a living member returns the line).
- Paid and refused services, driven in the window with an explicit **throwaway synthetic
  party** (gold 100 or 0; HP `10/40`, `0/30`, `20/30`; MP `1/9`, `0/9`, `7/9`), then removed
  with its native flag: no save was modified and no debug/give API remains. `shop-paid.log`/
  `shop-paid.png` (`c47eaba03983`, `cecbfc5c446c`: gold 100 → 92, herb in the bag, 1);
  `shop-poor.log`/`shop-poor.png` (`c47e89dad887`, `ca111329841f`: refused, gold 0, bag 0);
  `inn-paid.log`/`inn-rested.png` (`57324a991daf`, `94a10396507b`: night clock 0 → 210, gold
  100 → 90, living `40/40` and `9/9`, the dead stay `0/30`); `inn-poor.log`/`inn-poor.png`
  (`f428c3fd3a33`, `385fafc4f8d1`: price 10 for two beds, gold, clock and the three members
  unchanged, the ROM's too-poor line); `revival.log`/`revival-price.png`/`revival-done.png`
  (`34ba1a43b1ac`, `7823943a2133`, `39dc17c14e4d`: member 2 at 0 HP and level 10 comes back
  at `30/30`, 60 gold paid, gold 100 → 40, `<TARGET>` replaced by its name);
  `revival-poor.log`/`revival-poor.png` (`db767cbd7c97`, `07a886c61d3a`: refused, HP 0, gold
  0). The same transitions remain covered without a window in `app/services/tests.rs`.
- Review-driven surface evidence: `divination-result.png`/`divination-return.png`
  (`044932510558`, `f3e473277da6`: the 1022 line names Hero and "minstrel", then returns to the
  Divination row), `inn-night-x.log`/`inn-night-x.png` (`31507e7fc45b`, `3c72616e0872`: one
  Stay charge, X shows 1011 and no Rest), `multiheal.log`/`multiheal-after.png`
  (`a691caa6479f`, `6592d06768ea`: action 34 heals members `[0,1]`, dead skipped, MP once),
  `fullheal.log`/`fullheal-list.png`/`fullheal-after.png` (`8a7e036ac72c`, `7fb70e831bf0`,
  `c24ecb26688e`: action 33, HP 20 → 80, MP 100 → 76), `dead-actor.log`/`dead-actor-refusal.png`
  (`1db5b20d3292`, `f9f2c74e00c2`: visible refusal, nothing spent). Web canvas smoke on the same
  wasm build: `web-field-menu-canvas.png` (`5baa467cd031`) and `web-spells-canvas.png`
  (`c7a3950647cd`), with `web-smoke.log` (`d35afe4cfb87`). After
  the rebase onto the GPU-fixed main, the field menu was captured again
  (`field-menu-rebased.png`, `76f62ffeec95`); the paid and revival runs above are from the
  services commit they prove, before that rebase.
- The field menu rules (`build/evidence/j4-field-menu-rules/`, `manifest.json`):
  `ghidra-field-item-use.txt` (`511193376ff9`) — the item menu `func_ov002_02157634`, the
  action applier `func_ov002_021536ec` and the consumer `func_ov002_0215a7b4`;
  `ghidra-item-rules.txt` (`c07e96712a9d`) — `Item_IsConsumed`, `Item_ConsumeHeld` and
  `Equip_RefusalMask`; `ghidra-benediction.txt` (`b4b35b097d41`) — `Church_CureFlow`;
  `ghidra-coffin.txt` (`183bdad7ffbc`) — the quick line, the mode setter, the builder and the
  wipe-out caller; `slot-table.txt` (`0eda93d59e6a`) — the worn-place table at `0x0217f39c`
  and the church's line words; `item-rules.txt` (`469352ec8756`) — the ten cursed records with
  their names, the mask checks over the 1,178 records and line 1100 of the eight church files,
  printed by `item-rules-probe.py` (`16494e492f46`); and the route pair
  `route-before-checkpoints.jsonl`/`route-after-checkpoints.jsonl` (`8ce4af30e497`, 972
  checkpoints, byte-identical; `route-before.out` and `route-after.out` are byte-identical
  too, `b97be937ab35`).

## Trigger action 180 — a set battle's own revival map

`Trig_RunAction` case `0x50` (EU `0x020633ac`; the wiki's USA `0x0206339c`, inside
`func_02061c04`), read on the European ROM:

- `func_02046b60(gameres+0x36fc, 10)` must find the **battle task** (kind 10);
- `func_0x021b8478(gameres+0x3718)` gives the **battle request**;
- `func_020a3694()` must be non-zero; the wiki reads this test as **a set battle** (the
  request's `+0x0c` then holds a battle number);
- if the request's `+0x0c` equals the action's argument (`180 : b, m` stores `b` as the
  argument), its **`+0x22`** (the battle task's `+0x3e`) receives **`m`**, the parameter's
  high half (`func_0205ec70` case 72; the record's operand is `m << 16`); otherwise 0.

The record runs with the battle's **lost** record (event 16), which the game fires **before**
the wipe-out (`func_0206f81c` then `func_ov017_021b790c`), so the write decides where the
party wakes. **Two European records carry it**, re-read on the ROM (`dq9re triggers`):

- `triggerD16.bin`, room 8612 at `14.4`, event 16: `if 0xc(14) then 0x68(1) 0xb4(14,
  151322624)` — the action's first operand is the battle **14** and the second's high half is
  **2309** (`151322624 = 2309 << 16`), Upover's church;
- `triggerS07.bin`, room 5704 at `14.6`, event 16: `if 0xc(16) then 0x68(3) 0xc5(119)
  0xb4(16, 373555200)` — battle **16**, map **5700** (`373555200 = 5700 << 16`), the Gortress
  exterior.

The wiki *Travel* gives the same two (its "record 14/16" is the battle number here, not an
index).

**Not modelled.** `crates/dq9-core/src/trigger/action.rs` has **no `0xb4` arm**: the action
falls into the `_ => {}` of the immediate match, so the trigger neither writes anything nor
stops — unlike `Request::Other`, it is not even reported. The application's
`battle::Request` has no wake-map field, and `App::wipe_out` always uses `App::revival_map`.
The exact change needed, when the trigger crate is open:

1. `trigger/action.rs`: an arm for `0xb4` pushing
   `Request::SetBattleRevivalMap { battle: op.arg, map: high(op, 0) }` (`op.arg` is the
   record's first operand, `m` the parameter's high half, as action `0xe6` reads its own);
2. the application applies that request to the running battle's resolved request (a
   `wake_map: Option<u16>` beside its number) as `field.battle_ended` returns the lost
   record's events, **before** `wipe_out`;
3. `wipe_out` prefers `wake_map` to `revival_map`.

## The revival map a save carries (v0.3)

`GameState+0x5698` is the map the wipe-out returns to. A new game sets **1106** (Angel
Falls' church); a save made in a church (save bit 0) carries the map it was saved on unless
flag `0x113c` is set, a save with bit 3 carries **109** (Stornway's church); trigger action
208 sets it to its argument, action 179 to its second value with `0x113c` (**EU only:** on no
record), and the battle that opens the postgame sets 109 (wiki *Travel*). The application
holds it in `App::revival_map` and `start_new_game` sets 1106; there is no save writer yet
(v0.3), so the field is documented and nothing more is done here.

## Symbols

Names added to `re/symbols.txt` by this batch (each is a function start in
`re/functions.txt`): `Equip_RefusalMask` `0x020dd4d4`, `Item_IsConsumed` `0x020ddb30`,
`Item_ConsumeHeldBattle` `0x020ddb48`, `Item_ConsumeHeld` `0x020ddccc`, `Bag_Add`
`0x0207c388`, `HeldItem_ClearSlot` `0x0208387c`, `Equip_ClearWorn` `0x02052d8c`,
`Field_ApplyAction` `ov_002::021536ec`, `FieldMenu_UseItem` `ov_002::02157634`,
`FieldMenu_ConsumeItem` `ov_002::0215a7b4`, `Church_SetMode` `ov_003::02159174`,
`Church_CureFlow` `ov_003::0215a740`, `Church_QuickLine` `ov_003::0215af9c`.

Still proposed:

| address | name | source |
|---|---|---|
| `0x0207ba90` (USA) | `LootManager_CreateContainer` | wiki *Treasure*, read in `src/World/LootableContainer.cpp` |
| `0x021adcb0` (overlay 17) | `Loot_OpenContainer` **[proposed]** | wiki *Treasure*, flags at `0x021ae528`–`0x021ae58c`, lock at `0x021ade84` |
| `0x0218b688` (overlay 17) | `Field_ClearContainerFlags` **[proposed]** | wiki *Treasure*, `0x0218c180` clears 700 from `0x79e` |
| `0x02032370` | already `RandBelow` (`0x02032380`) | `re/symbols.txt`; the draw the tables use |
| `0x0215c924` (overlay 3) | `Inn_Function` **[proposed]** | wiki *Inns-and-Churches*, `func_ov003_0215c924` |
| `0x02158e94` (overlay 3) | `Church_Function` **[proposed]** | wiki *Inns-and-Churches*, `func_ov003_02158e94` |
| `0x021ba998` (overlay 17) | `WipeOut_PriestTask` **[proposed]** | read: builds overlay 3's church, sets mode and voice, steps it |
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
- Wipe-out (`build/evidence/j4-wipeout-priest/`, `manifest.json`): `chur-messet.txt`
  (`88aa38556fd9`), the 18 map records and the voice each gives; `voice-table-tests.log`
  (`56014e52bcd0`), the synthetic and real-ROM `VoiceTable` tests; `wipe-out-priest-church-2.log`
  (`6aaf3df7b85e`) and `wipe-out-silent-major-1.log` (`6be13bb4d73a`), the native headless runs
  (map 1106, voice 1 at story major 2 / voice 3 silent at major 1);
  `priest-line-1082-services.log` (`d6d91ed974bc`), `str_ch0` and `str_ch4` line 1082;
  `trigger-180.log` (`ed6830dd9dd4`), the two room records. **No window capture**: `--battle`
  skips the field on a loss and `--new-game` conflicts with `--screenshot`, so the scene cannot
  be reached by the screenshot driver; `priest-line-font.png` (`d5f8626c732d`) is line 1082
  drawn with the game font, not the window.
