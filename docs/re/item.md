# Items: `itemdt_<category>.gp2` tables and "item obtained" flags

Implementation: `crates/dq9-formats/src/item.rs` (`ItemTable::add_member`,
`ItemTable::compendium`) and `crates/dq9-core/src/trigger/action.rs` (`State::obtain`);
`dq9-core::state::item_obtained_flag`. Data: `data/prm/itemdt_<category>.gp2`, member
`itemdt_<category>_<language>.nat` (decompressed). The DQIX wiki, *Items* page, documents the
same tables; the numbers below were checked on the European ROM and against the
first player control states.

## Table

The member starts with a **12-byte** header — `u16` record count, `u16`
flags (bit 15 set: an equipment member with its stats table, the low 15 bits count the
stats entries; the tools carry `0x8000`), `u32` `+0x04` unused, `u32` `+0x08` the name
pool's byte length — followed by `count` **32-byte** records. Tables of an equipment
category continue after the records with a stats table and a names block (below).
The nine categories, by file letter:

| letter | category | records (EN) | ids | compendium |
|---|---|---:|---|---|
| `w` | weapons | 268 | 19050…20918 | 1–268 |
| `s` | shields | 45 | 21000…21397 | 269–313 |
| `h` | helmets | 132 | 12170…12917 | 314–445 |
| `b` | armour | 183 | 13000…13794 | 446–628 |
| `a` | gloves | 78 | 15000…15299 | 629–706 |
| `u` | leggings | 85 | 16093…16390 | 707–791 |
| `l` | shoes | 101 | 17091…17421 | 792–892 |
| `d` | accessories | 52 | 18000…18055 | 893–944 |
| `t` | tools | 234 | 22000…22290 | 0 (2 items), 945–1176 |

A record:

| offset | type | meaning |
|---:|---|---|
| `+0x00` | `u32` | **stats-table index** (`0..count−1`, each used once; `0xffffffff` on the tools, which have no stats table) |
| `+0x04` | `u32` | **name offset**, from the start of the name pool; `0xffffffff` = no name (`func_020de574`) |
| `+0x08` | `u32` | category (low nibble = equipment class) |
| `+0x0c` | `u32` | bits 12–22: **compendium index**; low nibble: sort position |
| `+0x14` | `u16` | **field action** number (`252` = none; INFERRED, the wiki's *Items*, "The two actions") |
| `+0x16` | `u16` | **battle action** number (`252` = none); what the battle's Items menu resolves, for a held tool and for a worn item |
| `+0x18` | `s16` | **item id** (`func_020de56c`, `0x020de57c`) |

The compendium index is the one read by `func_0207cc18` (`0x0207cc28`):
`(record[0xc] & 0x7fffff) >> 0xc`. It is 0 for items with no compendium entry
(some of the tools): the game then sets no flag.

### The two action numbers

The battle's item confirmation reads `+0x16` (`func_ov000_02171ffc`; the same number for a held
tool and for a worn record), and the cursor's confirm (`func_ov000_02171e60`) looks it up in the
battle's action table (`func_020797dc` / `func_02079e2c`): a number the table lacks refuses the
row. 252 is the "no action" value (`NO_ACTION` in the application). Measured over all 1,178
records of the nine English members, 1,103 carry 252 in `+0x16`; the 75 others name one — 39
equipment (27 weapons, 11 shields and the prayer ring 18023) and 36 tools. Examples: medicinal
herb 22000 → 255 (kind 2 heal), cautery sword 20010 → 369 (kind 1 blow), sage's stone 22035 →
416. Whether the original registers the empty action 252 in that table is **[unknown]**, so
using a no-action worn record may still reach the target list and spend the turn for nothing
(**[hypothesis]**, `battle.md`). The 75 are listed in
`build/evidence/j4-battle-worn-items/item-actions.txt` (`3162cbffaa58`). The worn half of the
battle list, the consumer that never removes it and the prayer ring's 25 % break are in
[`battle.md`](battle.md), "Using a worn item".

### The stats table

An equipment member continues after its records with **one 32-byte stats entry per
record**, indexed by the record's `+0x00` word. Measured on the European tables: 268
weapons, 183 armour and 52 accessories, the indexes dense and unique; the stats table
ends where the 0x58-byte name-block header starts (weapons `+0x430c`, armour `+0x2dcc`,
accessories `+0xd0c`), and the name pool follows it. The tools' records hold
`0xffffffff` and the member has no stats table.
`dq9_formats::item::ItemStats` reads it (`add_member`, `get`).

Three words hold the battle numbers `func_02083e28` adds to the level table's
attributes, three signed 10-bit fields each:

| word | bits 0–9 | bits 10–19 | bits 20–29 |
|---|---|---|---|
| `+0x08` | **attack** | **defence** | not established |
| `+0x0c` | **block** | **evasion** | **critical** |
| `+0x10` | **deftness** | **agility** | **magical might** |

The rates are **in tenths of a hundredth**: `func_02084ef8` (block), `func_02084f68`
(evasion) and `func_02084e78` (critical) sum the three fields over the worn places and
divide by the `10.0` at `0x02084f64`/`0x02084ef4`. `GetEvasionRate`
(`ov_000::02156270`) adds `2.0`; `GetBlockRate` (`ov_000::02156118`) is zero without a
shield. Evidence beyond the code: the copper sword (20004) entry 0 is attack 7; the
strength ring 4, raging ruby 9 and soldier's medal 20 are their attack; the gold ring 2
and dragon scale 5 their defence; the critical acclaim's third rate field is 40; the
agility ring 20, meteorite bracer 100, utility belt 25 deftness, brainy bracer 8 and
sorcerer's ring 15 magical might. Attack, defence, deftness, agility and might are
INFERRED from those descriptions and the guide; the three rate fields are read in the
code. No established field carries magical mending.

### The names block

Implementation: `crates/dq9-formats/src/item.rs` (`ItemTable::add_names_member`,
`ItemTable::name`). Encoding: the game's **markup text**, one NUL-terminated ASCII string per
record — the same bytes `itemname_<language>.nat` holds (`<1>` is the possessive `'s`,
`<'e>`/`` <`e> `` the accented letters of the font's glyph table, [`font.md`](font.md)); not
Shift-JIS, not a fixed-stride array of glyph codes. No second decoder is needed: a string is
returned as-is and the text pipeline maps it to glyphs.

**How it was found.** The item menu loads the weapons member (`data/prm/itemdt_w.gp2`,
member `itemdt_w_<LG>.nat`; `func_ov000_0217d87c`, xref of both path strings) and parses it
with `func_020de6c8` (`0x020de6d8`) → `func_020de714` (`0x020de724`), which reads the 12-byte
head and computes two bases:

```c
memcpy(head, member, 0xc);
*(head + 0xc) = member + 0xc;                 // records
iVar1 = func_020de7a4(head);
*(head + 0x10) = member + iVar1 + 0xc;        // name pool
```

`func_020de7a4` (`0x020de7b4`) is the layout:

```c
uVar1 = head[1];                              // +0x02 flags
if ((short)uVar1 < 0) iVar2 = 0x58;           // bit 15: equipment
return iVar2 + (uVar1 & 0x7fff) * 0x20 + *head * 0x20;
```

so the pool starts at `12 + count×0x20 + stats entries×0x20 + 0x58` (0x58 only when bit
15 is set). The head's `+0x08` word (low 31 bits) is the pool's byte length:
`func_020de888` (`0x020de898`) allocates and copies exactly that many bytes from the pool
base into a separate buffer. The per-record name offset is then resolved by the callback
`func_020de574` (`0x020de584`) that `func_020de6c8` runs over every record (`func_020de7d0`):

```c
iVar1 = *(int *)(record + 4);
if (iVar1 != -1 && *(int *)(head + 0x10) != 0)
    iVar1 = *(int *)(head + 0x10) + iVar1;    // pool base + offset
else
    iVar1 = 0;
*(int *)(record + 4) = iVar1;                 // now a char *
```

**The 0x58-byte block** is skipped by both readers; its content is observed but its purpose is
**[unknown]**. It holds the record count (`268` at `+0x00` of the weapons' block) and one or
two **partitions of the name indices into group sizes** that sum to the count — for the
weapons, 12 groups of 30, 24, 21, 24, 22, 22, 23, 25, 19, 19, 20, 19 names, which are the
weapon families in compendium order (sword, lance, knife, staff, whip, club, claws, fan, axe,
hammer, boomerang, bow); for the tools, two groups of 146 and 88 and seven of 46, 9, 3, 65,
85, 0, 26. The pool itself is in **compendium order** (the weapons open with the copper
sword, compendium 1), while the records are in id order; the record's `+0x04` offset, not the
position, selects the string.

**Checks on the European ROM** (`build/evidence/v4-item-names/names-check.txt`,
`190849f11f35`; block dump `weapons-names-block.txt`, `33e7c5c51c9b`; reader
`reader.txt`, `25c8bb1a891b`): all 45 members (nine categories × five languages) declare a
pool that ends their file exactly, and all 5,890 records carry an offset inside it to a
printable NUL-terminated string (only three Spanish armour pairs share one string:
13018/13627, 13201/13303, 13502/13605); `itemname_<language>.nat` names the same ids
identically. The eight weapons that `v4-metal-equipment` opened:

| id | name (EN) | compendium | itembtlprm `+0x00` bit |
|---:|---|---:|---:|
| 20021 | metal slime sword | 22 | 4 |
| 20022 | liquid metal sword | 23 | 4 |
| 20023 | metal king sword | 24 | 4 |
| 20119 | attribeauty | 187 | 4 |
| 20517 | metal slime spear | 48 | 4 |
| 20518 | liquid metal spear | 49 | 4 |
| 20519 | metal king spear | 50 | 4 |
| 19057 | poison needle | 61 | 10 |

`crates/dq9-formats/tests/real_rom.rs::item_names_resolve_from_the_name_pool` pins the eight
against the item table and against `ItemNames` (`itemname_en.nat`).

## The weapon words (`itembtlprm` `+0x00`)

`func_ov017_021b3780` (`0x021b3780`) copies each worn item's `itembtlprm` record — 44 bytes,
[`battle-data.md`](battle-data.md) — into the character at `+0x2F4` of its `+0x150` word, one
record every `0x2C` bytes, in the order of the 8-byte live slot list the literal at `0x021b38C4`
points at (`0x021d6B20`: `08 00 09 07 05 01 06 0A`). Record 0 is therefore live slot 8, the
**weapon**: `func_02083738` maps the item record's category (low nibble of `+0x08`) 0 to live
slot 8, 1 to 9, 2 to 0, 3 to 1, 4 to 7, 5 to 5, 6 to 6 and 7 to 10, and the accessors behind
the copied records gate on `+0x2AC`, slot 8's id at `0x194 + 8 × 0x20 + 0x18`. Every reader of
the copies thus tests the weapon's `itembtlprm` `+0x00` word bit for bit.

Two of its bits are read in the final damage (`ov_024::021e6a90`):

| bit | reader | meaning | records (EU, of 1,423) |
|---:|---|---|---|
| 4 | `func_02085128` (`(word & 0x1F) >> 4`) | "+1 against metal": the damage gains `1.0` on a metal body | 20021, 20022, 20023, 20119, 20517, 20518, 20519 |
| 10 | `func_020851d8` (`(word & 0x7FF) >> 10`) | the poison needle: the damage becomes exactly `1.0` and the dodge and the block are cleared | 19057 |

Both accessors first require the weapon slot's id to be positive (`*(s16 *)(character + 0x2AC)
< 1` returns false), which the copy of an empty slot — a zeroed record with `0xFFFF` at
`+0x2A` — never satisfies; `func_ov017_021b3780` writes exactly that for a worn slot whose id is
not positive. The seven bit-4 records are all weapons, all with element 8 (none); 20119 carries
bits 4 to 7 and 19057 bits 5, 10 and 24 — the other bits' meanings are not established here. The
counts are over the whole table (`build/evidence/v4-metal-equipment/item-flags.txt`,
`6f15e467da33`), and
`crates/dq9-core/tests/battle_rom.rs::the_equipped_weapon_sets_the_metal_and_needle_flags` pins
them: the party's adapter (`Character::fighter`) fills `Fighter::metal_bonus` and
`Fighter::poison_needle` from the equipped weapon alone.

The same copied record's `+0x08`–`+0x10` hold the twelve **killer bonuses by monster family**
(signed six-bit tenths, 10 none). `CalculateFinalDamage` reads them for a party blow carrying
`+0x10` bit 18 against the target's `mon_data` family 1 to 12, dividing by `10.0f`; the copper
sword 20004 — the new game's weapon — is ten everywhere but **family 2, eleven**.
`Character::fighter` copies the twelve onto `Fighter::family_bonus`
(`docs/re/battle.md`, "The weapon's killer bonus"; `docs/re/battle-data.md`,
`build/evidence/v4-family-bonus/item-rows.txt`).

## "Item obtained" flag

`func_0207cc18` (`0x0207cc28`, called by `Items_Give`, bag additions and
equipping):

```c
compendium = (record[0xc] & 0x7fffff) >> 0xc;
if (compendium != 0 && !Flags_Test(manager, manager + 0x8c, compendium + 0xc76))
    Flags_Set(manager, manager + 0x8c, compendium + 0xc76, 1);
```

The flag is therefore **global** (`g_QuestManager+0x8c`), set on the **first** acquisition
(or first equipping) of the item, not on each copy. `dq9-core` reproduces the rule in
`State::obtain`: the first copy gives `0xc76 + compendium`; the index comes from
`ItemTable`, filled by the application from the ROM.

## First acquisition at the opening

The four flags set together at frame **3740** of the original run are those of the
starting equipment, confirmed at character creation:

| id | item | compendium | flag |
|---:|---|---:|---:|
| 20004 | copper sword | 1 | 3191 |
| 13007 | celestial costume | 585 | 3775 |
| 16215 | celestial stockings | 765 | 3955 |
| 17120 | celestial shoes | 856 | 4046 |

The sword comes from the creation table `ov_009` `0x0218636c` (file `0x20cc`: 20004, 20500,
20403, 20600, 20100; the `u16` list at `0x0218ab78` repeats 20004 with 13007, 16215,
17120). The three outfit pieces are already in `HeroLook::new_game`. The halo (12805,
compendium 398 → 3588) is not among the `pro1` flags: it is not an obtained item.

The actual writer of these four flags is not `Item_FlagObtained` but
`Party_FlagEquippedItems` (`func_0206e424`, EU `0x0206e434`): a RAM trace (hook on
`Flags_Set` writing `{lr, index, value}` at `0x020e7300`, read back by `Save_BackupRead`)
attributes them to `lr = 0x0206e524`. Decompilation (Ghidra): for a member 0..3
(`GetPartyMemberByIndex`), the function copies the equipment slot list from
`0x0206e564` (9 bytes, terminated by a negative value); for each slot
(`member + 0x194 + slot * 0x20`) whose class (`+8 & 0xf`) is < 8 and compendium
(`(+0xc & 0x7fffff) >> 12`) non-zero, if `Flags_Test(+0x8c, 0xc76 + compendium)` is false, it
does `Flags_Set(+0x8c, 0xc76 + compendium, 1)` and counts; the count then goes to
`func_020a04c8` [hypothesis: statistic/achievement]. Same formula, separate path, without
`Items_Give`. Callers (xrefs): `ov_017::0218b688` (call `(x, 0)` depending on the analysis
branch) and `ov_017::021b688c` [hypothesis: loop over the party list
`+0xf78`/`+0xf7c`]. `dq9-core` does not model this path: `State::obtain`
(`crates/dq9-core/src/trigger/action.rs`) only covers given items; the start is rendered by
`start_new_game` via the same rule, but a recruited companion's equipment will not set
its flags [hypothesis on the exact moment the original calls the function on
recruitment].

The end event of `21596` (`triggerX01.bin`, room 4105, event 11) gives item
**22159** (benevolessence) via `0x72`: its compendium is 1093, hence flag **4283**, set
at frame 21142 of the original run.

## Items in the control states

| flag | id | item | source |
|---:|---:|---|---|
| 4135 | 22000 | medicinal herb | battle 26 loot, random (female run) |
| 4212 | 22087 | slimedrop | battle 26 loot, other run (`bat6`) |
| 4283 | 22159 | benevolessence | end trigger of `21596` (`0x72`) |

`4135` and `4212` are not opening constants: three real runs with the same
inputs except gender give the herb, or neither, and `bat6` gives the slimedrop. This is
**random battle loot**; the runtime substitutes battle 26 and draws no loot
(`melonds-state.md`, § "Differential oracle"), hence the remaining, accepted gap.
