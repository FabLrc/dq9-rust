# Party, character records and the level tables

Implementation: `crates/dq9-formats/src/level.rs` (`LevelTable`, `LevelTables`) and
`crates/dq9-core/src/party.rs` (`Party`, `Character`, `Bag`), wired into
`dq9_core::state::GameState` (`party`). Data: `data/prm/level0.bin` … `level12.bin` (ROM) and
the save's `Main` block. The field origins below were checked on the demo save
(`build/sav/demo.sav`, main checkout) and on the oracle states `pro1.ml1`, `pro11.ml1` and
`bat6.ml1` (`build/orca/oracle/`, main checkout) through the party member object at
`GameState+0x474` (`GameState_PartyMember`, `0x020108e8`), plus the wiki *Party* and
*Level-Tables* pages (facts only).

## Level tables (`data/prm/level<N>.bin`)

Thirteen files, one per vocation in the status screen's order: `level0` Guardian, `level1`
Warrior … `level6` Minstrel … `level12` Ranger. Each is a **tagged record table** (`table.rs`,
the game's `Script` container), 5,312 bytes, 102 records: kind `0x64` (1 value), kind `0x65`
(1), the **99 level records** (kind `0x66`, 11 integer values) and kind `0x67` (22 values).
`table.rs` already checks the framing (record headers, `0xFF` padding, string pool) and fails
on malformed input.

A level record, fields 0 to 10 (wiki *Level-Tables*; the file's own order was checked on all
thirteen files):

| field | meaning | evidence |
|---:|---|---|
| 0 | experience the level is reached at | **INFERRED** by the wiki (guide prints no experience); 0 at level 1, rising on every record |
| 1 | strength | guide's Minstrel table, all 72 quoted values |
| 2 | resilience | same |
| 3 | agility | same |
| 4 | deftness | same |
| 5 | charm | same |
| 6 | magical might | same; 0 on Warrior, Martial Artist, Gladiator |
| 7 | magical mending | same; 0 on Warrior, Martial Artist, Gladiator |
| 8 | maximum HP | the largest column at level 1 on every file, 390–600 at 99 |
| 9 | maximum MP | highest on Mage and Sage |
| 10 | skill points gained, all told | the guide's table at every level; 0 at 1, 12 at 10, 200 at 99 (Minstrel) |

The record's order of fields 6 and 7 is **might then mending**, while the live member
object's stat words carry **mending then might** (see below): the status screen's string order
is `… Charm · Magical Mending · Magical Might · Max. HP · Max. MP`.

`level6` is the Minstrel's: the published guide's attribute table agrees with it at all 72
values it prints (levels 1, 5, 15, 25, 40, 60, 80 and 99), and no other file agrees at more
than two. The other files' vocations are INFERRED from the status screen's order; the Abbey's
ungated list (Warrior…Minstrel) agrees with that order. The `0x64`, `0x65` and `0x67` records
are kept raw by `level.rs` (`RawRecord`): their meaning is not established.

Test: `dq9-core/tests/party_rom.rs`, `minstrel_level_table_matches_the_published_guide` (level
1's row, 12 and 200 skill points, the rising experience column).

## The character record (`save::Character`, 572 bytes)

Offsets are those of the record, whether in the save (`Main`, record `n` at `136 + 572 n`) or
in RAM (`GameState+0x3984`, `func_0208660c`). `save.rs` types most of them; the ones it does
not are decoded by `Party`/`Character`.

| field | record | save | RAM member object | source |
|---|---:|---|---|---|
| name | `+0x140` | 10 bytes, one glyph index per character, zero-terminated | `+0x3c`, converted to text (`func_02042774`) | `Character::name_raw`, `melonds::copies::MEMBER_COPIES` |
| sex | `+0x174` bit 0 (0 man, 1 woman) | copied with the `+0x160` look block | `+0x49c` bit 0 | wiki *Party* § "The appearance is `+0x160`" |
| current vocation | `+0x50` (word) | yes | `+0x950` | wiki *Party*; `GetExperience`, `ov_023::021eea98` |
| level per vocation | `+0x02`, 13 bytes | yes | `+0x16c`, 13 × u16 | `PartyMember_LoadRecord`, `0x02082d7c` |
| experience per vocation | `+0x1C`, 13 × u32 | yes | `+0x138`, 13 × u32 | same |
| skill-point pool | `+0xF4` (u16) | yes | `+0x564` | same |
| equipment and look | `+0x160`, 0x1C bytes | yes | `+0x488` | same; ten `s16` in the order torso, legwear, **face, hair**, arms, footwear, headgear, weapon, shield, accessory |
| base stats/maxima | `+0x14C`, 5 words | yes | `+0x00…+0x14`, `+0x1c`, `+0x1e`, `+0x6c`, `+0x6e` | `func_020830cc` packs them; see below |
| held items (per party slot) | — | `7578 + 18 i`, 8 × u16 | `+0x454` | `Save_UnpackMain`, `0x020a95b4` |
| bag | — | `Main+0x1d5c`, 0x1D4 bytes | `GameState+0x2a04` | `Save_UnpackMain`; layout below |
| gold on hand | — | `11448` | `GameState+0x3970` | `Save_UnpackMain` |

The hero is the first party member: party slot `i` is record `standby_count + i` (the count is
`7572`, the party size `7577`). `Slot::party_order` (`7573`) holds the member-object ids the
game assigns, not record indices, and the model does not need them.

**The stat block (`+0x14C`)** is five words, fourteen 10-bit fields, three to a word
(`(bits 0-9, 10-19, 20-29)`). Fields 0 to 6 are the base attributes in the live object's order
(strength, resilience, agility, deftness, charm, magical mending, magical might), fields 7 and
8 are the maximum HP and MP, and fields 9 to 13 are **unknown**. The record keeps the values
after equipment modifiers have been added (`func_02083e28` sums each worn piece and clamps to
999); whether seeds also enter this block is **[hypothesis]** (the demo's Sage has none). On
the demo's Sage 91, strength is 246 against the level table's 186 and magical mending 556
against 192: the model's `Character::stats` is the level table's **base** value (no equipment,
no seeds), and the equipment bonuses wait for `itembtlprm` (next wave).

**HP and MP on load.** `Save_UnpackMain`/`func_02082d7c` write the record's fields 7 and 8
(maximum HP and MP) into the member object's `+0x1c` and `+0x1e`
(`melonds::copies::member_fields`, checked on the demo). Those are the fields the game reads
as the character's current HP and MP: the status block pointed to by `GameObject+0x130` has
current HP at `+4` (= member `+0x1c`) and current MP at `+6` (= `+0x1e`); the maxima sit at
member `+0x6c`/`+0x6e` (block `+0x30` of `GameObject+0x134`). So a save restores every
character **at full HP and MP**, and `Character::from_record` does the same. [hypothesis: the
load does not keep wounded values, which the file format cannot carry.]

**Computed values.** `Party::apply_level_tables` fills `Character::stats` from the level table
at the current vocation and level. It also fills a new-game hero's maxima and current HP/MP
(the character has none yet); a restored character keeps the record's maxima, which include
seeds and equipment exactly as the load writes them.

## The party in a checkpoint (`dq9re state --json`)

Batch `j4-parity-party` widened the parity checkpoint to the party and the gold
([`melonds-state.md`](melonds-state.md), § "Checkpoint"): the reader
(`dq9_formats::melonds::Memory::party`, `PartyRam`, `PartyMemberRam`) uses the offsets below,
no new guess. The count and the order list are the save's copies (`Save_UnpackMain`,
`0x020a95b4`); the per-member fields are those `PartyMember_LoadRecord` (`0x02082d7c`) writes
and the game updates.

| Field | Address | Source |
|---|---|---|
| party count | `GameState+0x3980` (u8, ≤ 4) | save `7577`, copied by `Save_UnpackMain` |
| party order | `GameState+0x397c`, `count` bytes | save `7573`, same copy |
| slot → member object | `+0x568` = order byte (u16) | `Save_UnpackMain`'s search of the four objects |
| current vocation | `+0x950` (u32) | `PartyMember_LoadRecord` |
| level | `+0x16c + 2 × vocation` (u16) | same |
| experience | `+0x138 + 4 × vocation` (u32) | same |
| current HP / MP | `+0x1c`, `+0x1e` (u16) | `func_020830cc`'s stat block (§ "HP and MP on load") |
| maximum HP / MP | `+0x6c`, `+0x6e` (u16) | same |
| gold on hand | `GameState+0x3970` (u32) | save `11448` |

A count above four, an order entry with no member object and a vocation outside the thirteen
are errors (`Memory::party`), never zero-filled fields; the tests build a synthetic state with
two members in reversed slot order and check the refusals
(`melonds::tests::party_reads_slot_order_levels_and_gold`,
`party_refuses_what_the_game_cannot_hold`).

Measured on the original references (the same image each checkpoint was validated against):

| Checkpoint | Party | Gold |
|---|---|---|
| `build/oracle-host/ram_male.bin`, 1.1.1, vblank 23033 | Lv 1 Minstrel, HP 16/20, MP 6/6, experience 7 | 13 |
| `build/v01-20261107/oracle/control112-ram.bin`, 1.1.2, vblank 58393 | Lv 1, HP 20/20, MP 6/6, experience 7 | 13 |

Both agree with `bat6.ml1`'s documented 7 experience and 13 gold after battle 26; the male run
kept the HP its fight left, the female 1.1.2 image is at full HP. The party is unchanged at
vblanks 58303–61077 in the 1.1.2 captures, so the one-frame gap between the archived
checkpoint (58392) and its RAM image (58393) does not touch these fields. Empty member objects
(id `0xffff`, HP 1/1) are never read: the count and the order list name the slots.

**What it changes in parity.** `scripts/compare-runs.ps1` compares the six fields and the gold
exactly. With `--fight --seed 7` battle 26's monsters are `[(290, 1), (292, 1), (290, 1)]` and
the engine pays 2 experience and 4 gold, leaving the hero at 13/20 HP (`Lv 1`, MP 6/6); the
stand-in writes `0/0` HP, `0/0` MP, 0 experience and 0 gold (it never applies the level
tables), and the originals' own RNG drew another monster set (7 experience, 13 gold; 16/20 or
20/20 HP). So the fought and stand-in runs now differ on 611 of their 615 checkpoint lines,
and the fields the original's draw explains are accepted **per field** in the reference:
`scripts/parity.ps1` sets the original copy's value to the exact value the seed-7 run must
produce (`{ "member": 0, "field": "experience", "value": 2, "reason": … }`, or
`{ "field": "gold", "value": 4, … }`), with the reason in the reference. The comparison stays
exact: a regression to 0 fails (measured, `build/evidence/j4-parity-party/`). Level, the
maxima and the MP match the originals without acceptance.

## Held items

A party slot carries **eight** held item ids (`Save` `7578 + 18 i`, 8 × u16; RAM member
`+0x454`; `Save_UnpackMain`, `func_02082d7c`). `0` and `0xffff` are empty
(`Character::held_items`, `docs/re/party.md`'s `from_record`). They are the items the member
carries into battle: the battle item command operates a held slot, not the bag (see
[`battle.md`](battle.md) § "Defend, guard level and items"; the consumer is
`func_ov017_021d0164`, which clears the slot behind a command descriptor with
`func_0208386c`, EU `0x0208387c`). Consumption is the original's single compaction pass: the
chosen slot becomes `0xffff`, then each hole shifts one rank down, so duplicates of the same
id keep their order and only the chosen copy goes. The field item use (`func_020ddb38`,
EU `0x020ddb34`) reaches the same slots by index for categories 8–10.

Whether an item is spent is the item record's `+0x08` bit 19 ("used up", the bit the battle
AI reads in `func_ov024_021f7478`); measured over the 1,178 EU records it is set on 46 tools
and the prayer ring only, never on a weapon, shield or armour with a battle action
(`build/evidence/j4-battle-held-items/item-flags.txt`, `755490182234`). `dq9-app` keeps that rule.

## Battle numbers of a member

`Character::fighter` (`crates/dq9-core/src/party.rs`) turns a member into the battle
kernel's `Fighter`. The level table's attributes are the base; the worn equipment adds
the item stats table's fields (`item.md`, § "The stats table"), summed over the eight
places by the same loop `func_02083e28` makes (EU `0x02083e38`, USA `0x02083e28`):

- attack = strength + item attack, defence = resilience + item defence;
- deftness, agility and magical might + their item fields; magical mending has no
  established item field and stays the level table's value;
- evasion = `GetEvasionRate`'s `2.0` (`ov_000::02156270`) + the item evasion fields over
  `10.0` (`func_02084f68`, EU `0x02084f68`, USA `0x02084f58`); block = the block fields
  over `10.0` (`func_02084ef8`, EU `0x02084ef8`, USA `0x02084ee8`), zero with no shield
  (`ov_000::02156118`); the critical bonus = the critical fields over `10.0`
  (`func_02084e78`, EU `0x02084e88`, USA `0x02084e78`);
- resistances start at 100 and add, for each worn item, the twenty signed bytes of its
  `itembtlprm.nat` record on elements 1–7 and 9–21 (`func_02083e28`); element 8 is the
  plain Attack's and stays 100; a negative sum is held at 0;
- the weapon's element is the `itembtlprm` flags' bits 23–25 (0 = none);
- current and maximum HP/MP are the member's (`Save_UnpackMain`/`func_02082d7c`).

The base `stats` must have been filled by `apply_level_tables` first: a restored save's
record only carries the stat block, which the model does not fold into `stats`. A field
the game takes from a skill panel or a trait (the book's critical bonus, trait `0x11d`,
the accuracy die, a multi-strike weapon) stays at the kernel's neutral value.

Evidence: `pro1.ml1`'s hero is level 1 Minstrel, strength 9, with the copper sword
(20004, attack 7 in stats entry 0) — the fighter's attack is 16; the armour 13007's
defence is 2. **[hypothesis]**: the attack and defence fields as named come from the
wiki and the item descriptions, not from a battle state; the rate fields are read in
`func_02084ef8`/`f68`/`e78`.

## A won battle's award

`Party::award` follows the victory routine (`func_ov023_021edf54`) and its helpers:

1. **Experience is shared** by `func_ov023_021f4098`, per member, in 32-bit floats:
   `ceil(total × (k + level) × counter ÷ Σ (k + level) × counter)`. `level` is the current
   vocation's (`func_0202053c`), the counter the battle's (`func_02053dfc`), and the share is
   multiplied by 1.05 before the rounding for a member whose equipment carries the
   experience bonus (`func_0208538c`). A member who is down (`func_02061bd8`) receives and
   weighs nothing. `k` is the band of `expadj.nat` (`func_ov023_021f5534`): 4 up to 10,000
   experience, 3 up to 20,000, 2 beyond; the European file has no second block. The total is
   first multiplied by `1 + (n − 1) ÷ 10` for `n` connected players (1 alone, so no change
   here). The counter, the bonus and the float rounding are in
   [§ Sharing the experience: the second factor](#sharing-the-experience-the-second-factor);
   the multiplayer path is **not modelled**.
2. The share is added to the **current vocation's** experience (`func_ov023_021f5150`),
   never to the other twelve (`GetExperience`, `ov_023::021eea98`).
3. **Level-up** is the field's result window (`func_ov023_021f0a5c`): it loads
   `level<vocation>.bin`, parses rows with `func_020824a0` (`func_02082064` per record),
   and takes a level when the next row's column 0 is not above the total
   (`func_020824a0` returns the new level; `func_ov023_021f0a5c` compares the row's
   experience). Every level crossed at once is taken.
   `func_02083ccc` writes the new row's attributes and maxima into the member, then
   `func_02083e28` adds seeds and equipment again; HP and MP are **not** healed. Skill
   points receive column 10's difference, the pool held at **2,600** (`iRam021f1224`,
   `0xA28`). The row's experience column 0 is the wiki's **[INFERRED]** experience
   threshold.
4. **Gold is added whole** and held at **9,999,999** (`func_ov023_021f5150`'s literal;
   `func_0207d2c4`, EU `0x0207d2d4`, has the same cap).
5. **Drops** take `func_0207c378`'s rule (EU `0x0207c388`): add to the first stack of the
   item, clamped to **99**, else to the first empty slot; a full bag loses the copy. The
   drop rolls themselves are the C library `rand` (`re/battle-data.md`, § "A monster's
   drops") and are not this batch's: they are passed in already rolled.

**Check:** `bat6.ml1` after battle 26 is experience 7 in the Minstrel (still level 1,
level 2 needs more), gold 13 and one copy of item 22087 in the bag. A new-game hero
awarded 7/13/`[22087]` reproduces exactly that record; `pro1.ml1` + those three values
is `bat6.ml1` (`docs/re/party.md` oracle section). The synthetic 1→3 test pins the row
changes, the skill-point difference, the maxima and the unhealed HP/MP.

## The bag (`Main+0x1d5c`, 0x1D4 bytes)

Not typed by `save.rs`; decoded by `Party`/`Bag`:

| offset | type | meaning |
|---:|---|---|
| `+0x00` | u32 | runtime pointer to `bag+0x0c` (`0x020f5de8` in the oracle states); not data |
| `+0x04` | u32 | runtime pointer to `bag+0x13c` (`0x020f5f18`); not data |
| `+0x08` | u16 | **capacity** (152 on the demo and in the oracle states) |
| `+0x0c` | `capacity` × u16 | item id; `0xffff` marks an empty slot |
| `+0x0c + 2·capacity` | `capacity` × u8 | quantity of the slot at the same index |

The block is exactly `12 + 3 × 152 = 0x1d4` bytes, and a capacity that would not fit is
rejected (`PartyError::BadBag`). Evidence: the demo save (152 slots, 101 non-empty, quantities
1–74; e.g. item 22004 × 44) and `pro1.ml1` (152 empty slots). `pro11.ml1`/`bat6.ml1` hold one
copy of item 22087 — battle 26's loot (see the oracle below). Whether the game enforces 152 is
**[unknown]**; the file only states it.

## The new-game hero

Character creation (`ov_009`; the table at `0x0218636c`, the `u16` list at `0x0218ab78`, and
the creation flow wiki *Party* § "Recruiting asks the vocation first") produces:

| field | value | evidence |
|---|---|---|
| vocation | 6, Minstrel | `pro1.ml1` member object `+0x950`; `level6`'s level-1 row matches the member's stats |
| levels | 1 in all thirteen vocations | `+0x16c`; `0x02086450`'s initialiser loop |
| experience | 0 in all thirteen | `+0x138`; same initialiser |
| HP / MP | 20 / 6, current = maximum | level 6 row 1; `+0x1c`/`+0x1e` in `pro1.ml1` |
| base stats | 9, 8, 8, 12, 9, 6, 7 (strength, resilience, agility, deftness, charm, might, mending) | level 6 row 1; the member object's first seven fields |
| equipment | torso 13007, legwear 16215, footwear 17120, weapon 20004 (copper sword) | `0x0218636c`/`0x0218ab78`; `pro1.ml1` `+0x488` block |
| skill points | 0 | `+0x564` |
| gold | 0 | `GameState+0x3970` in `pro1.ml1` |
| bag | 152 empty slots | `GameState+0x2a04` in `pro1.ml1` |
| name | **empty** [hypothesis] | the name is typed on the creation screen (`ov_009` step 8); the oracle run typed `1` |
| sex | **man** [hypothesis: the default the oracle run kept] | `+0x49c` bit 0 = 0 in `pro1.ml1`; the creation screen offers both |

No headgear is worn: the member object's head slot is `0xffff` in `pro1.ml1` and `bat6.ml1`.
`dq9_formats::hero::HeroLook::new_game` still says item 12805 (the halo) there; that is a
**[hypothesis]** of `hero.rs`, not of the record — the record is the evidence used here.

## Oracle (evidence)

Program: a throwaway `build/scratch/oracle` (not committed) builds `GameState::new_game()`,
applies the ROM's level tables and compares its leader with `pro1.ml1`, `pro11.ml1` and
`bat6.ml1` field by field; it also lists the party restored from the demo save.

Files: `build/evidence/j4-party-state/` — `newgame-vs-pro11.txt`, `bat6-diff.txt`,
`demo-save-party.txt` (hashes in `manifest.json` by `scripts/evidence.ps1`).

**New game vs `pro1.ml1` (the opening):** every compared field matches — vocation, the
thirteen levels and experiences, current/maximum HP and MP, the seven base attributes, the
eight equipment slots, the skill points, the held items, the sex, gold 0 and the empty 152-slot
bag. The only difference is the name: the oracle run typed `1`, the ROM has no default name.

**`pro11.ml1` is not before battle 26.** It is labelled "entrée du combat 26" in the oracle
README, but its hero already carries the battle's rewards: experience 7 in the Minstrel (still
level 1), 13 gold and one copy of item 22087 in the bag — the batch's expected differences. It
is byte-for-byte equal to `bat6.ml1` on every compared field (`bat6-diff.txt`), and `pro1.ml1`
vs `pro11.ml1` differs only in those three values. So neither state is a *pre*-battle
reference; `pro1.ml1` is, and `bat6.ml1` adds no difference after the battle beyond what
`pro11.ml1` already held.

**Demo save:** `GameState::from_save` lists the slot's four members in party order with the
values `dq9re state --save` compares — vocation, level and experience in the current vocation,
equipment, skill points, name glyphs, sex, held items — plus the maximum HP/MP from the
record's stat block (Sage 91: 481/481 and 552/552), gold 38,754 and the 101 occupied bag slots
(`demo-save-party.txt`).

**Party in the widened checkpoint (`build/evidence/j4-parity-party/`):** the two regenerated
original references carry `party`/`gold` read from `ram_male.bin` and `control112-ram.bin`;
`regen-originals.txt` (`df726deb7ad1`) shows every previously compared field identical to the
archived checkpoint before the fields were appended (`references.json`, `980250de6f8e`, cites
the provenance). `party-controls.txt` (`5bbe15431f79`) gives the three sides at 1.1.1 and
1.1.2: original 7 experience / 13 gold (HP 16/20 and 20/20), fought 2 / 4 (HP 13/20), stand-in
all zero. `parity-fought.log` (`ccebea99e966`) is 2/2 ok with the three party accepted fields
per reference; `parity-falsification.log` (`d67e0ecb00c5`, same references without them) fails
on `party member 0 hp`, `party member 0 experience` and `gold` only;
`compare-fought-vs-standin.txt` (`7093e57aa149`) and `compare-standin-vs-original.txt`
(`3a32a9d81a5d`) are the `compare-runs.ps1` runs behind 611/615 differing lines. The stand-in's
maxima are 0 because a headless stand-in run never applies the level tables
(`Party::apply_level_tables` happens at the first fight): party maxima cannot be compared from
a stand-in reference.

## Not established

- The record's stat fields 9 to 13.
- Level column 0 as experience (the guide prints none; the wiki INFERS it).
- The `0x64`, `0x65` and `0x67` records of a level file.
- Seeds and equipment do not enter `Character::stats` yet; only the level table does. The
  record's computed maxima do enter `Character::max_hp`/`max_mp` on a restored save.
- What drives the bag's two pointer words; they are written at runtime and serialized as they
  were.
- Whether the game enforces the bag capacity of 152.
- The exact default name and the creation screen's default sex: the oracle run typed `1` and
  kept the man's look; the model starts empty/man.
- A new-game event block (`GameState::new_game` still leaves it empty; `state.rs`).

## Sharing the experience: the second factor

Read 17 November 2026 on the European ROM from `func_ov023_021f4098` (overlay 23,
`0x021f4098`) and its helpers. The audit note quoted the USA addresses (`0x0202053c`,
`0x02053dfc`, `0x0208538c`); on the European ARM9 they are `0x0202054c`, `0x02053e0c` and
`0x0208539c` (`+0x10` in `0x0200FB20..0x020EEBD0`). Decompilation and disassembly:
`build/evidence/j4-xp-share/xp-share-decomp.txt` (`0143d3a2ffa9`).

**The share has three factors, in 32-bit floats.** The loop at `0x021f4134` fills two arrays
for each of the four party slots that the battle's mask (`[battle + 0x2a0]`,
`func_020a35f0`) says are filled and whose member is not down (`func_02061bd8`): the counter
`0x02053e0c` returns (`0x021f416c`) and the level `0x0202054c` returns (`0x021f4180`). The
share of member `i` is then

```text
share_i = total × ((k + level_i) × counter_i) ÷ Σ_j ((k + level_j) × counter_j)
```

(`0x021f4350`–`0x021f4384`: `i2f(k)`, add the level, multiply the counter, divide by the
sum, multiply by the total). The sum is `p3 + (p2 + (p0 + p1))` (`0x021f42e8` on); a sum of
0 is replaced by 1.0 (`0x021f4318`: `moveq r11,#0x3f800000`); the total is truncated once
for the `k` band lookup (`0x021f4240`, `_ffix`) and kept whole for the share (`0x021f4348`).
The rounding is a ceiling in floats: truncate, add 1.0 when the fraction is above 0,
truncate again (`0x021f43a8`). A share whose exact value is not representable therefore
rounds up where integer arithmetic would not — 13 experience at weights 7 and 6 gives 8
and 6, not 7 and 6.

**`0x0202054c`, the level** (`PartyMember_GetLevel`): the halfword at
`+0x16c + 2 × vocation` of the live record (`[member + 0x150]`, its vocation at `+0x950`),
the value `GetExperience` reads as well (`0x021eea98`).

**`0x02053e0c`, the counter** (`PartyMember_GetBattleCounter`): the word at
`[[member + 0x19c] + 8]`, 0 when the member has no block. Its writers:

| address | what it does | callers |
|---|---|---|
| `0x02053db0` `PartyMember_SetBattleCounter` | stores the caller's block at `member + 0x19c`, then clears `+0x0` (u16), `+0x2`, `+0x3`, `+0x4` (bytes) and `+0x8` (word) | `func_ov000_021643d4` +1812, `func_ov000_02166e5c` +240, `func_ov026_021dce34` +416 |
| `0x02053dd0` `PartyMember_BattleCounter` | returns `[member + 0x19c]` (the block, 0 without one) | `func_ov000_021540fc`, `func_ov000_02169850`, `func_ov024_021f8628`, `func_ov026_021d8ba0` |
| `0x02053de0` `PartyMember_AddBattleCounter` | `++[[member + 0x19c] + 8]`, unless bit 0 of `[[member + 0x138] + 0x14]` is set (then nothing) | `ov_026::021dacb0` only |

`member + 0x138` points at the live record `+ 0x94` (`func_02053c4c` sets it), so the gate
word is `live + 0xA8` bit 0. The increment's single call site is `ov_026::021dacb0`, inside
`func_ov026_021d8ba0`: after the command entry of one round completes (`[+0x54e0]`, the overlay's
step 4 shows every present member's command is in; the battle RNG state is stored at
`+0x6e3c`/`+0x6e40`) step 5 destroys the round's arena (`+0x774c`, `0x16000` bytes allocated at
step 3), clears `+0x7749`, clears the flags `0x4000000`, `0x8000000`, `0x10000000` and
`0x20000000` (`0x02160fbc`), and then walks the four slots (`0x021dac88`–`0x021dacbc`):
`0x020a35f0([[battle + 0x2a0] + 0x34] mask, slot)` (the slot is in the battle) and `0x0200ff2c(game,
slot)` (the actor record exists: its flag word has bit `0x100`) both non-zero →
`0x02053de0(member)`. The round is then handed to phase `0xb` (`0x02160d80`); the battle's
input step is where the flow resumes (`[+0xeac] = 4`). Disassembly and the decompiled bracketing
code:
`build/evidence/j4-xp-wire/counter-call-site.txt` (`6b0f44f66fc4`); the pilot's before/after route
(`route-before.*`, `route-after.*`: journal and checkpoints byte-identical, `9767eea3a0fb` and
`135ddaa29dd9`) is in the same directory.

**The counter is one increment per command pass, for every member the battle holds.** The loop
tests the slot's presence in the battle's mask and the actor record, never its HP, so a member
who fell at the start of the round still takes the increment; every member present from the
start therefore carries the **same** count, and the counter cancels exactly in the division of
`0x021f4098`. It separates members in two cases only: a member that joins the battle late
(fewer passes) and one whose `live + 0xA8` bit 0 holds it (0 increments; its weight is 0 and it
takes 0 experience). The wiki's reading of the call site as "turns spent alive" is refuted by
the missing HP test. **[unknown]**: what bit 0 of `live + 0xA8` is.

`Party::award` therefore takes the neutral `Share` (one for every slot), which is identical to
the battle's own count for a party that was present from the start: the single-member pilot's 7
experience stays 7, at any count above 0. `Party::award_with` takes the battle's own counts.
`dq9-core`'s battle engine tracks them (`Battle::passes`, one increment per round for every slot
the battle holds, `0x02053de0`) and `dq9-app` awards through `award_with` with them and with
`itembtlprm.nat`, so the shoes bonus below reaches the game. Counters all 0 are **not** neutral
— the sum becomes 1 and every share is 0 × the level weight, i.e. 0.

**The × 1.05 experience bonus** (`0x0208539c`, `HasExperienceBonus`): it walks the eight
equipment slots of the live member and answers 1 when any worn item's `itembtlprm.nat`
record has flag bit 16 (`(flags & 0x1ffff) >> 16 != 0`, the table offset `i * 0x2c`). The
share is then multiplied by `[0x021f4434]` — 1.05 (`0x3f866666`, wiki *Battle resolution*) —
**before** the rounding (`0x021f4398`–`0x021f43a4`), so the shares can come to more than the
total: 14 experience split 5/9 becomes 6/9 for the wearer. `Party::award_with` applies it
from `BattleParams` (`experience_bonus`), `None` applying none; `Party::award` passes none.

Tests: `crates/dq9-core/src/party.rs` (`one_member_takes_the_whole_total_whatever_its_counter`,
`the_counters_weight_each_share`, `the_share_follows_the_originals_float_rounding`,
`an_experience_bonus_item_multiplies_the_share_before_the_rounding`) and
`crates/dq9-core/tests/party_rom.rs`
(`two_member_share_on_the_real_level_tables_takes_the_bonus_flag`, a synthetic two-Minstrel
party on the cartridge's level tables and `itembtlprm.nat`).
The engine's counter and the application's award:
`crates/dq9-core/src/battle/flow_tests.rs`
(`the_counter_adds_one_per_round_for_every_member_the_battle_holds`: one increment per round for
every slot the battle holds, a fallen member included, and a refused round changes nothing) and
`crates/dq9-app/src/battle/tests.rs`
(`the_victory_awards_by_the_battles_counter_and_the_item_bonus`: two Minstrels at levels 1 and 5
through `Fight` on the cartridge's tables take a slime trio's 6 experience as 3 and 4, the
shoes-wearer takes 5, and a single member takes the whole total).

Parity: the seed-7 reference replayed on this branch matches `first-control-1.1.1-male`'s
aligned checkpoint exactly, and `control-1.1.2` differs only on flag 4135,
the battle 26 drop the reference already accepts (`parity-first-control.txt`
`cbde6fdc34f8`, `parity-control-112.txt` `3a2d05063e7c`, runtime
`runtime-checkpoints.jsonl` `c2b10c767640`). The checkpoint carries no party or gold on this
branch, so it observes no share either way; the award of the pilot's single member is the
whole total in both the old and the new arithmetic.
