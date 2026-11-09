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

1. **Experience is shared** by `func_ov023_021f4098`, per member:
   `ceil(total × (k + level) × rounds / Σ (k + level) × rounds)`. A member who is down
   (`func_02061bd8`) receives and weighs nothing. The `rounds` factor is the same for
   every member and cancels in the share. `k` is the band of `expadj.nat`
   (`func_ov023_021f5534`): 4 up to 10,000 experience, 3 up to 20,000, 2 beyond; the
   European file has no second block. The total is first multiplied by
   `1 + (n − 1) ÷ 10` for `n` connected players (1 alone, so no change here).
   **Not modelled:** the `× 1.05` for a member wearing an item with `itembtlprm` flag bit
   16 (`func_0208538c`) — the award carries no equipment table; and the multiplayer path.
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
