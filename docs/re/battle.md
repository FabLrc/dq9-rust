# Battle kernel: generators, draw order, damage

Implementation: `crates/dq9-core/src/battle.rs` and `battle/` (`random`, `model`, `damage`,
`resolve`, `monster` for the kernel; `setup`, `round`, `ai`, `end` for the whole battle, see "The
battle's flow"). Tests: `battle/tests.rs` and `battle/flow_tests.rs` (unit), `tests/battle_oracle.rs`
(differential, fixtures `tests/data/battle_oracle_{primitives,actions}.txt`) and
`tests/battle_rom.rs` (whole battles on the ROM).

Sources, in the order of `AGENTS.md`: the USA decompilation (DQIX/dqix-decomp, commit
`b5c617aa3113a57baba07465ede3be7a0be5328f`, Unlicense: `src/Util/Random.cpp`,
`src/Combat/Main/{BasicAttackCalculation,CritRateCalculation,TensionBonusCalculation}.cpp`,
`src/Combat/Overlay_24/GetAttackBaseDamage.cpp`); the DQIX formats wiki, pages
*Battle-Resolution*, *Actions*, *Monsters* (facts only, not copied); Ghidra on the European ROM
(`scripts/ghidra-query.ps1`); DQIX/BattleEmulator (MIT, commit
`6c4ad874d1c84579690c22c16efc58c945a6b218`) as the differential oracle.

**Addresses.** EU first, USA in brackets where the ARM9 module shifts them (`AGENTS.md`: USA
`0x0200F3B8..0x0200FB20` is +`0x8`, `0x0200FB20..0x020EEBD0` is +`0x10`); overlays are identical.

## Scope of the kernel

One action, of a party member or a monster, on any number of targets: the plain Attack and the
damage and healing spells of chapter 2 (kind 1 and kind 2, no damage handler, no rider, accuracy
standing at 100). Everything else is **refused before any draw** with `Unsupported`: damage
handlers 1..66, riders, scaling accuracy, statuses, tension, guard level, metal bodies, a trait
`0x11d` critical, an elemental weapon, a weapon that strikes several, the multi-target falloff
against several targets, the accuracy die of four, and the actions the code singles out by number
(`0x1B`, `0x40`, `0x48`, `0x70`, `0x7E`, `0xAF`, `0x79`, `0xE1`, `0x395`, `0x1FF`, `0x200`,
`0x20B`, `0x20C`, `0x82`, `0x205`).

## Generators

Both are `struct Random` (`src/Util/Random.cpp`): `state = state × 0x5D588B656C078965 +
0x269EC3` (64-bit), the draw its top 32 bits **after** the step.

| function | EU (USA) | kernel |
|---|---|---|
| `NextRandom` | `0x020742d0` (`0x020742c0`) | `Random::next_u32` |
| `NextRandomMax` | `0x0207430c` (`0x020742fc`) | `Random::next_max`: `max ≤ 0` returns 0 **without drawing**; `(int)((float)max × float01)` held below `max` |
| `NextRandomFloat01` | `0x0207435c` (`0x0207434c`) | `Random::next_float01`: `(float)(draw / (double)0xFFFFFFFF)`, so the largest draw is exactly 1.0 |
| `NextRandomFloatBetween` | `0x02074398` (`0x02074388`) | `Random::next_float_between`: `(max − min) × float01 + min` in `f32` |
| `GetBTRandom` (world's) | `0x020741e0` (`0x020741d0`) | not a global: the caller owns a second `Random` |
| `rand` (C library) | `0x02003d14` | `CRand`: `seed × 0x41C64E6D + 0x3039`, draw bits 16–30. Drops and the surprise roll |

Checked on the **European** ROM (Ghidra decompilation of the four `NextRandom*` functions,
`build/evidence/j4-battle-kernel/ghidra-prims.txt`): the 64-bit multiply-add on the state words;
`NextRandomMax` does `_fflt(max)`, `_fmul`, `_ffix`; `NextRandomFloat01` converts the draw with
`_dfltu`, divides by the double `4294967295.0` (`0x41EFFFFFFFE00000`) and narrows with `_d2f`;
`FloatBetween` is `_fsub`, `_fmul`, `_fadd`. The USA decompilation and the European code agree
on every one: **fact**.

Which generator feeds what: the battle's is seeded when the battle is made and every roll of
`ResolveAction` takes it; the world's is `GetBTRandom()` and gives a monster's HP. That the world's
generator is the one at `0x02108ddc` seeded at `0x0200f540` is the wiki's USA reading, **not**
checked on EU (**[hypothesis]**); `func_02089630` on EU does call `GetBTRandom` (`0x020741e0`)
before the HP draw, which is what the kernel relies on.

### Float edges (unit tests)

- Critical threshold `(int)(100.0f × rate)` with `rate = CalculateCritRate`: deftness 159 gives
  **208**, not 209. Counting the deftness values 150..=999 where the threshold differs from
  `200 + (deftness − 150)`: **151** of 850, exactly the wiki's count.
- `deftness − 150` goes through a `short` (the decompilation notes the 16-bit shifts), held at 0.
- `NextRandomMax(n)` in whole numbers `(top × n) >> 32` differs from the float form about once
  in 4000 draws at `n = 10000` (wiki, USA); the kernel uses the float form, the oracle vectors
  below pin it (`next`/`max` rows, 50 seeds × 64 draws each).

## The monster's HP

`InitMonsterBattleStatus` (`func_02089630`, EU `0x02089640`, USA `0x02089630`), decompiled on EU:
when its third argument is 0, `HP = (uint)(0.5f + (float)table_HP × NextRandomFloatBetween(
GetBTRandom(), 0.8f, 1.0f))` (`_ffixu`), else the table's HP; the result is both current and
maximum HP. `init_monster_hp(world, table_hp, set_battle)`. **[hypothesis]**: the third argument
is "a scripted battle" (the wiki reads the setup's `+0x0C` ≠ −1); the kernel only takes a bool.

## Order of draws of one action — `ResolveAction`

`func_ov024_021eb5d0`, `ov_024::021eb5d0` (EU = USA; in `re/functions.txt`). The kernel's
`resolve_action` draws, for the modelled subset:

1. **Before any target**, once, if the reach is All (3) or Group (4) and `+0x1C` bits 14–18 are
   0 (`ov_024::021ea4d0`, `021ea500`): the critical roll.
2. **Per target**: `NextRandomMax(100)` (the die, kept at `[battle + 0x8e6e]`, never read here);
   the critical roll if it was not rolled once; the evasion roll if `+0x10` bit 5; the block roll
   if bit 6 **and the blow was not dodged**; the accuracy roll always; then, if it landed, the
   amount (`GetAttackBaseDamage`, also for a dodged or blocked blow), `CalculateFinalDamage`
   (critical's draw, coin), and for a kind-1 blow that dealt damage with `+0x10` bit 11 the wake
   roll `NextRandomMax(100)`.

| roll | function EU (USA) | kernel |
|---|---|---|
| critical | `ov_000::02156cc4`: always-critical (`+0x08` bit 29) returns 1 with no draw, else `NextRandomMax(10000) < (int)(100.0f × rate)` | `roll_critical` |
| party rate | `CalculateCritRate` `0x020747b4` (`0x020747a4`): `(1/hits) × skill × (2.0 + 0.01 × max(0, deft − 150) + accessory + book)`, skill = `+0x14` bits 21–27 / 100 | `calculate_crit_rate` |
| monster rate | `func_020748f8` `0x02074908`: literal zero; the draw is spent | `critical_rate` |
| evasion | `ov_000::02156f98`: `NextRandomMax(100) < (int)rate`, rate **truncated** | `roll_evasion` |
| block | `ov_000::02156e30`: `(float)NextRandomMax(100) < rate`, rate **not** truncated | `roll_block` |
| accuracy | `ov_000::02156648`: draws `NextRandomMax(100)` before anything is compared; for the actions modelled accuracy stands at 100 | `STANDING_ACCURACY` |
| amount | `GetAttackBaseDamage` `ov_024::021e7bc0` | `damage::base_amount` |
| final | `CalculateFinalDamage` `ov_024::021e6a90` | `resolve::final_amount` |

Evasion and block rates, resistances and the party's rates are **inputs** (`Fighter`): how a
party member's come from equipment, and a monster's from its record grade (`0, 2, 4, 8, 25`,
`grade_rate`), belongs to the readers of the next wave.

### Amount

- **Blow** (no range): `RoundUp(attack × multiplier)` → `CalculatePhysicalDamage` (`0x020744d0`
  EU, `0x020744c0` USA): `d = (attack − defence/2)/2`, nothing if not above 0; above
  `attack/16`: `d += between(−d/16, d/16)` then `d += between(−1, 1)`; else
  `d = between(0, attack/16)`. EU decompilation read: same operations in the same order
  (`ghidra-prims.txt`).
- **Range**: monster `mon ± spread` (1 draw); party with a scaling number
  `(int)((stat − lo) × ((max − min)/(hi − lo))) + min` between `lo` and `hi`, then the spread's
  draw (1 draw); party without a scaling number: one draw between the least and the most, then
  the spread's (2 draws). **[hypothesis]**: where the truncations fall for the 2-draw arm. The
  oracle does not exercise it (Heal's mending is below `lo`).

### Final damage (`ov_024::021e6a90`), EU decompilation read

Order read in `build/evidence/j4-battle-kernel/ghidra-final-damage.txt`: tension multiply at the
head (not modelled) → critical → `× GetResistance(element)` (`_fmul`, line 182, float) → block
and dodge zero it → **coin** `NextRandomMax(2)` (line 529) when the damage is not above 0, the
target was not blocked or dodged, the action is not `0x1B`/`0x48`/`0x70` and the target's
resistances to the action's both elements are above 0 → metal slash adder (not modelled) → guard
(not modelled) → combo table (`0x021fe778`, `1.0 1.2 1.5 2.0`, counter held to 3, line 574–587)
→ `_ffix` **once** (line 594) → cap `+0x1C` bits 0–13 (0 = none).

- **Critical**: the greatest of `1.2 × damage` and `CalculateCriticalDamage` (`func_02074838`, EU
  `0x02074848`); for action 1 (and `0xDB`, `0x1F9`) the latter is the attacker's attack power ×
  `between(0.95, 1.05)`, else `damage × between(1.5, 2.0)`. One draw either way.
- **A resistance stays a float.** A blow of 1 against a resistance of 50 is 0.5: no coin, `_ffix`
  gives 0 (unit test).
- **Heal** (kind 2) goes through the same function: a critical heal multiplies by
  `between(1.5, 2.0)`; the combo bit is not set on it.

### Action fields (EU action tables, read with `cargo dq9re gp2 data/prm/actdt_b.gp2`)

Decoded from the European `actdt_a_en.nat` / `actdt_b_en.nat` / `actdamage_*.nat` for actions
1, 2, 9, 12, 16, 18, 30, 31, 223, 224: the fields of `Action` in `battle/model.rs` carry the
wiki bit names, and these records bear them out (**fact**, EU):

| action | `+0x10` | reach | crit % | kind | scale | cost | range (spread / party min / monster) |
|---|---|---|---|---|---|---|---|
| 1 Attack (2 identical) | `0x01ff3bf8` | 5 | 100 | 1 | 2 | 0 | none |
| 223 / 224 (Attack variants) | `0x01ff3a78` | 4 / 3 | 100 | 1 | 2 | 0 | none |
| 12 Crack | `0x7451` | 2 | 50 | 1 | 2 | 3 | 5 / 30 / 17 |
| 16 Crackle | `0x7451` | 4 | 50 | 1 | 2 | 8 | 8 / 50 / 33 |
| 18 Woosh | `0x7411` | 4 | 50 | 1 | 2 | 3 | 8 / 16 / 14 |
| 30 Heal | `0x0100a401` | 2 | 50 | 2 | 2 | 2 | 5 / 35 / 35 |

So: Crack and Crackle are blockable (bit 6) and Woosh and Heal are not, the plain Attack is
dodgeable and blockable and carries bits 16, 17, 18 (the accuracy die, the falloff, the weapon
element), the combo bit (`+0x2C` bit 27) is on the Attack and Crack and **off on Crackle, Woosh
and Heal**, and every `+0x1C` bits 14–18 and every rider/handler of these is 0. These are the
flags BattleEmulator's spells draw with (a block draw for Crack and Crackle only).

## Differential oracle

Two oracles in one host program, `build/oracle-harness/harness.cpp` (not committed: `build/` is
local; its source, the oracle patch and the build command are in
`build/evidence/j4-battle-kernel/`, `oracle-build.txt`):

- **Primitives** — the decomp's `Random.cpp`, `CritRateCalculation.cpp`,
  `BasicAttackCalculation.cpp`, compiled **unchanged** on the host (`-ffp-contract=off`, SSE
  single operations = the ARM9's soft-float `_fadd`/`_fmul`), driven over: 24 seeds × 8 outputs,
  `NextRandomMax` at 2, 4, 8, 100, 10000 (64 draws × 10 seeds each), `NextRandomFloatBetween` on 8
  ranges (16 draws × 6 seeds, compared bit for bit), `CalculateCritRate` for **every deftness
  0..=999** × (skill 1.0, 0.5) plus accessory/book/hit combinations, `RoundUp`, and 160
  `CalculatePhysicalDamage` runs comparing the `f32` bits **and** the generator state (so the draw
  count). All match: **fact**.
- **Actions** — BattleEmulator's `callAttackFun` (MIT) with two edits (`oracle.patch`): the
  critical thresholds (compile-time constants) become runtime variables and a probe records the
  position after the action's own draws, crit, dodge, block and amount. 306 vectors: party plain
  Attack (40 normal, 40 critical, deftness spread across 0..999 with specials 149–160, 233, 999),
  monster plain Attack on the hero (evasion 2, block 0.5, combo counter 0..4; 40 normal, 18
  dodged, 10 blocked, 18 of 1 damage or less), Crack, Crackle, Woosh (wind resistance 75) and Heal
  (20 normal and 15 critical each). Compared: draws spent, critical, dodge, block, amount, MP.
  **All 306 agree**, with the one deviation below on 4 of the 15 Woosh criticals.

BattleEmulator calls the action with `(*position) += 2` before its own draws: two draws that the
kernel's contract starts **after** (the test advances the generator by two). What they are is not
established (**[unknown]**; the wiki lists four actions that draw before any target, which is
not this).

### Oracle deviation: Woosh critical, who is right

BattleEmulator truncates the critical's damage to an integer, then applies Woosh's `× 0.75`, then
truncates again. The game's `CalculateFinalDamage` keeps the float through `× GetResistance` and
has a single float-to-int conversion at its end (EU decompilation, `_fmul` at line 182,
`_ffix` at line 594). The kernel follows the game: on 4 of 15 Woosh criticals it returns the
oracle's amount plus 1 (never otherwise); the test accepts exactly "+1" on a Woosh critical and
asserts the count is at most the number of Woosh criticals. Which is right in play is
**[hypothesis]** (the oracle may be right if Woosh's 0.75 is not the resistance but a separate
integer step in a damage handler; Woosh's EU record has handler 0, so the resistance reading is
the one the code supports; a state of the original at a Woosh critical would settle it).

## The battle's flow (batch `j4-battle-flow`)

Implementation: `battle/{setup, round, ai, end}.rs`. Tests: `battle/flow_tests.rs` (synthetic) and
`tests/battle_rom.rs` (whole battles on the ROM, seeded). Ghidra evidence under
`build/evidence/j4-battle-flow/` (local): `ghidra-process-turn.txt`, `ghidra-turn-helpers.txt`,
`ghidra-exec-helpers.txt`, `ghidra-ai-arm9.txt`, `ghidra-ai-targets.txt`, `ghidra-retarget.txt`,
`ghidra-party-target.txt`, `ghidra-flee.txt`, `ghidra-victory.txt`, `ghidra-resolve-action.txt`,
`ghidra-reset-status.txt`, `ghidra-prio.txt`. EU addresses; overlay ones are the USA's. The wiki pages
*Battle-Resolution*, *Battle-AI*, *Battle-Weight-Tables*, *Event-Battles* gave the structure (USA
readings); everything below marked **read** was checked in the European decompilation.

### Setup (`setup.rs`)

- **Fighter of a monster** from `mon_btldata`: HP drawn by `init_monster_hp` on the **world's**
  generator, one draw per monster in the order they stand (group by group: **[hypothesis]** for the
  order), table HP for a set battle (no draw); MP, attack, defence, agility; evasion and block
  grades through `GRADE_RATES`; resistances; metal from `mon_data` bit 12.
- **Action of the kernel** from the action tables, the range looked up in the action's own half
  (`Skill`): amount a blow (range 0, multiplier 1.0) or a range record, scaling `Some` when
  `+0x18` bits 16–17 are 2 and bit 14 or 15 names the number, `scales_accuracy` when they are 1.
  `tests/battle_rom.rs` checks that the adapter gives the numbers the oracle's hand-decoded actions
  carry (Attack, Crack, Heal).
- **Read in code** (facts): `+0x08` bits 8–9 are the **side an action aims at** (1 the opponents,
  2 its own; `func_ov000_02154a04` branches on it; the "metal" reading of the formats crate is the
  same field); `+0x0C` / `+0x0E` (u16) are the targeting handler of an AI mode 1 / mode 2 monster
  (`func_ov024_021f66cc`); `+0x1C` bits 14–18 are the **hit code** (`func_ov000_0215fbe0`);
  `+0x2C` bits 14–19 the **turn class** (`func_ov000_0215f1ac`).
- **Monster record `+0x10`** (read): bits 3–4 the **AI mode** (`func_ov000_02159d94`), 5–7 the way
  rule, 8–10 the **extra-actions class** (`func_ov000_0215f57c`), 13–15 evasion grade, 16–18 block
  grade, 20–25 the once-a-battle mask, bit 26 "remembers its attackers" (`func_ov000_02154f30`).
- `ResetBattleStatus` (`0x020891dc`, read) zeroes the way counts (`+0x38`) and the extra-actions flag
  (`+0x3c` bit 0): they start at 0.
- A set battle (`eventbattle.bin` record, `scenario::battle`) is `BattleSetup::set_battle`; an
  encounter is `BattleSetup::encounter` from `(monster, count)` groups (at most 3 groups; the
  groups are built by the encounter batch). `fld_mondata`'s "run margin" is **not** read by the
  battle's flight (below): it belongs to the field (**[hypothesis]**).

### One round (`round.rs`; `ProcessCombatTurn`, `ov_000::0215d63c`, read)

Draws on the battle generator unless said, in this order:

1. **Flight** at the command phase (below), world generator.
2. **Initiative**: the standing party in slot order, then the monsters group by group
   (`func_ov000_02153e40`); each scored `(float)agility × NextRandomFloatBetween(0.51f, 1.0f)` on the
   buffed agility held to 999 (`0.51f` = `0x3f028f5c` at `0x0215e40c`). In the **first round**
   (`[battle + 0xe20] == 0`): surprise 2 (monsters) skips every party member and, for each monster
   after the first scored, draws `NextRandomMax(100)` first and skips it above `0x42`; surprise 1
   (party) skips every monster; skipped combatants are not scored and draw nothing. Sorted highest
   first by `func_020749ac` (a non-stable quicksort): **[hypothesis]** that ties break by order.
3. **Commands**, in that order, all before any action: a monster chooses its way and targets
   (below), then `func_ov000_0215f57c` draws `NextRandomMax(2)` **always** and returns
   `[0, coin, 1, 2, b, …][class]` extra actions (table bytes `0, 0, 1, 2` at `0x02182a6c`; `b` is the
   flag the function sets on its first call), each extra one a new way choice. A monster of AI mode 2
   chooses at its turn instead, and spends MP: refused (`Unsupported::AiMode`), as is mode 3.
   A party member's command is the player's: **[hypothesis]** the hero's tactic is Follow Orders, so
   `func_ov024_021f8f20` (the tactics AI, which draws) does nothing for them.
4. **Turn classes** (`func_ov000_0215f1ac`, read; classes from `0x02182aac` = 10, 5, 1 and `0x02182a5c`
   = 32): a party action with `+0x10` bit 19 and a weapon with bit 6 first; then classes 10, 5, 1
   each stably moved to the front (so 1 leads), then class 32 stably to the back. Plain Attack and
   the spells are class 0.
5. **Execution**, record by record; a combatant fallen before its turn does not act:
   `NextRandomMax(100)` (`func_ov000_0215833c`: the actor's status roll, drawn whatever its
   state); the targets are settled (`func_ov000_0215440c` for a monster: the chosen ones still
   standing, a group or everyone reach rebuilt from the whole side, none left → a fresh default
   pick; `func_ov000_021540fc` for a party member: the chosen target, re-chosen by
   `func_ov000_02153aa4` with `NextRandomMax` when it has fallen); `func_ov000_0215fbe0`'s **two
   draws** and its hit table (`0x02182a85`: `0 0 0 [3..4] 2 4 0 7 3 0 1 [6..8]` by hit code;
   anything above 0 strikes several times at random targets: refused); then `resolve_action`;
   then `func_ov000_02157d3c`: the party down is a defeat (`[battle + 0x8e14] = 2`), the monsters
   down a victory (1), the defeat first; while the battle goes on, `func_ov000_0215858c` draws
   `NextRandomMax(100)` for the actor (its status tick; every other draw in it is gated by a status).
6. The round counter goes up. **[unknown]** whether the round's end draws (status ticks at the end
   of the round, `func_ov000_0215e6e8`): none is modelled.

Not followed, **[unknown]**: what `func_ov000_0215b278`, `0215ae80`, `0215b5a0`, `0215ab88`,
`0215b198`, `0215bbbc`, `0215bf5c`, `0215c2d0`, `0215c4cc` (called after each action) draw through
nested calls: the direct calls were searched and none draws; `func_ov000_0215704c` (the monster's
"charmed by its target" roll, status byte `+0x53`) draws only when that byte is not 0, assumed 0;
`func_ov000_0215fc60` (a battle kind, `[battle + 0x81fe]` = 800 or 801) is assumed 0.

### Monster AI (`ai.rs`)

- **Way rule** by `+0x10` bits 5–7 (`func_0208a91c`, table at `0x020f10b0`): 0, 1, 2, 4 the weight tables
  (`0x020e8cba`, `NextRandomMax(256) + 1` walked down six weights, the way tried, then down to the
  first, then up: `func_0208a370`); 3 in turn (count, `(count + 1) mod 6`, six tries); 5 a pair
  with a draw's low bit (`NextRandom`, three tries); 6 the first way then a draw among the rest
  (`NextRandomMax(5)`, even `NextRandomMax(1)` draws); 7 (group count) refused. None usable →
  action 2 (the second plain Attack) through the default pick. All read.
- **Usable** (`func_0208a03c`, read, AI modes 0 and 1): action `0xE1` is not usable in the
  monsters' surprise round; a way in the once-a-battle mask is not usable when its **group** has used
  it; otherwise the way's targeting handler decides, and a usable way marks itself used for the
  group (any way, kind `0x22` excepted). The MP test applies in mode 2 only (`+0x3c` bit 3 is
  cleared by the reset).
- **Targeting handler** (`func_ov024_021f66cc`): the table is indexed by `+0x0C` in mode 1 and
  `+0x0E` in mode 2, and by 0 in mode 0 (and above `0xa0`). Modelled: **0**, the default pick
  (`func_ov000_02154a04`: reach 1 the actor; aimed at the opponents reach 2 and 5 one of the standing
  party by the weighted pick, 3 and 4 all of them; aimed at its own side reach 2 a standing monster
  by `NextRandomMax`, 4 its group, 3 all; then `func_ov000_0215fbe0`), and **112**, Flee (usable only
  when the mean attack + defence of the whole party, the fallen included, summed in a `short`, is at
  least three times the monster's own: `func_ov024_021f418c`). Any other handler is
  `Unsupported::TargetHandler`.
- **Weighted pick** (`func_ov000_02154f30`, read): weight 2 per front-line standing member, 1 per
  back-line one; `NextRandomMax(total) + 1` walked down; a second draw `NextRandomMax(count)` only
  if the walk ends unspent. A monster with the "remembers" bit adds 2 and 1 for the last two who
  struck it: what writes those is not read, so it is refused against several candidates.

### Flight (`Battle::try_flee`; `func_ov000_0215f7a8`, read; the command overlay is
`ov_026::021dd3dc`)

In order: the setup is a set battle → refused for good (`BattleError::FleeForbidden`, before any draw);
the first round with surprise 1 → away, no draw; no monster standing → away; the party's mean
attack + defence at least three times the monsters' mean (integer means over the standing) → away,
no draw; otherwise `max over the fleeing members of (int)(deftness × 0.05f) + 10`, held up to
`[25, 50, 75, 100, 65535][attempt]` (`0x02182c04`; the attempt counter `[battle + 0x8e1c]` goes up
on every such call), and `NextRandomMax(100) < percentage` on the **world's** generator.
Success ends the battle (`Fled`, no round). **[hypothesis]**: a failed flight costs the fleeing
members their turn and the round goes on (the flight state `ov_026::021d8ba0` state 0xb shows the
failure; what follows was not read). **[unknown]** the list `func_ov000_0215ed34` checks against
`0x26..0x28` (members that forbid flight).

### End (`end.rs`)

Experience and gold accrue per monster beaten (`func_ov000_02155184`), once each; the victory
routine turns the sums into the totals `(uint)((float)sum × [battle + 0x8e3c])` (gold `0x8e40`):
both multipliers taken as 1.0 (**[hypothesis]**). Drops (`func_ov023_021f454c`): once per **kind**
(monster number in a group) in the order first beaten, rare then ordinary only if the rare did not
land, `func_02032380(oneIn) == 0` = `(int)(oneIn × ((rand() − 1) / 32767.0)) == 0` in `double`
(`0x020323d0` holds the divisor's high word), `oneIn` = 1, 8, 16, 32, 64, 128, 256, 0 (`0x021fd888`; 0
draws nothing), at most eight kept; the C library's `rand` is the caller's `CRand` (nothing in the
battle seeds it). Not modelled: the four extra passes of the item-finding trait (`0xa4`), the
grotto and treasure-map branch, the group flags that force a drop. Sharing the experience is
`func_ov023_021f4098` (`Party::award`'s).

### Chapter 2 coverage (`tests/battle_rom.rs`, `chapter_two_coverage_…`)

Monsters of `encfld`/`encbtl` for `F01` (20001) and the Hexagon floors (7102–7104), set battle 2
(Hexagoon, monster 300) and 26 (prologue: monsters 290 slime, 292 cruelcumber). All are AI mode 0
or 1 and a way rule the model has. Action by action, the kernel's verdict and the AI's:

| action | kind | monsters | kernel | targeting (mode 1 handler) |
|---|---|---|---|---|
| 1, 2 Attack | 1 | all | resolved | default pick |
| 9 Frizz | 1 | 7 firespirit | resolved | default pick (mode 0) |
| 546 | 1 | 300 Hexagoon | resolved | default pick (mode 0) |
| 225 Flee | 0 | 1, 4, 20, 23, 30, 32, 43, 56, 57, 63, 290, 292 | `ActionNumber(0xE1)` | handler 112 modelled |
| 236 medicinal herb | 2 | 23 | resolved | handler 66: `TargetHandler(66)` |
| 41 Buff | 4 | 23 | `Kind(4)` | handler 18 |
| 45 Accelerate | 5 | 20 | `Kind(5)` | handler 26 |
| 59 Dazzle | 19 | 32 | `Kind(19)` | handler 54 |
| 323 | 19 | 63 | `Kind(19)` | handler 54 |
| 243 Weird Dance | 57 | 43 | `Kind(57)` | handler 117 |
| 344 | 12 | 43 | `Kind(12)` | handler 116 |
| 364, 366 | 0 | 56, 43 | `Kind(0)` | default pick |

So a battle against slimes, cruelcumbers, sacksquatches, teeny sanguini, dracky, spirits, the
firespirit, the Hexagoon and the prologue's two is played whole as long as no monster
**chooses** a way in the lower half of the table; the Flee way is refused by the kernel only when
its handler lets it through (the party at least 3× as strong): a hero that strong beats them
anyway. Inputs of `j4-battle-actions`: Flee `0xE1` (a monster leaving the battle, with the `fled`
flag), kinds 0 (364, 366: what they do), 4, 5, 19 (statuses and stat changes), 12, 57 and their
handlers 18, 26, 54, 66, 116, 117; the party's Defend (guard level); items.

## Open points

- **[hypothesis]** the two draws BattleEmulator spends before every action are `func_ov000_0215fbe0`
  (`NextRandomBetween(3, 4)` then `NextRandomBetween(6, 8)`, drawn on **every** call whatever the hit
  code), which `ResolveAction`'s head calls once target settling is done (`func_ov000_0215440c` for a
  monster, `func_ov000_021540fc` for a party member). The call is read in the European decompilation;
  that these are the oracle's two is not checked (the oracle models no targeting). The kernel still
  starts after them; `Battle` draws them.
- **[unknown]** what follows the wake roll in the original: the monster's provocation draws
  (`func_ov024_021eb08c`), `func_ov000_0215b5a0`'s hit rise, the tail of `ResolveAction`
  (`0x021ed48c`–`0x021ed564` tension spend). `ActionOutcome::generator_state` is exact up to the
  wake roll of the last target, not beyond; the oracle's `special charge` and `rage` draws are the
  oracle's modelling of those and are not compared.
- **[hypothesis]** the multi-target falloff of the plain Attack against **one** target is 1.0
  (its table starts at 1.0); not exercised by a multi-target oracle.
- **[hypothesis]** the group spells' per-target draws (die, accuracy, amount per target, one
  critical for the action): from the wiki's reading and unit tests; BattleEmulator is one-on-one
  and cannot check it.
- **[unknown]** `GetAttackBaseDamage`'s blow multiplier (`Amount::Blow::multiplier`, 1.0 for the
  plain Attack): which field of the action holds it.
- **[unknown]** the accuracy roll's statuses and traits (dazzle's die of 8, the trait's die of 4)
  and the metal-body early miss: refused rather than modelled.
- **[hypothesis]** the combo counter's stepping (`Battle::step_combo`) is the wiki's reading of
  `func_ov024_021ea584` and `func_ov000_0215cd80`: not checked against the code here.

## Symbols to add

For `re/symbols.txt` (owner: another batch). All named by the DQIX decompilation or by the wiki's
proposals; evidence above. EU addresses:

```
0x02074748 GetTensionMultiplier        # func_02074738: 1.0 1.5 2.5 4.0 6.0 party / 1.0 1.3 2.0 3.0 4.5 monster
0x02074848 CalculateCriticalDamage     # func_02074838: attack power x between(0.95,1.05) (action 1) or damage x between(1.5,2.0)
0x02074908 CalculateMonsterCritRate    # func_020748f8: literal zero
0x02074958 GetMultiTargetFalloff       # func_02074948: 1.0 0.8 0.6 0.4 0.2
0x02089640 InitMonsterBattleStatus     # func_02089630: HP draw from GetBTRandom
ov_000::02156118 GetBlockRate
ov_000::02156270 GetEvasionRate
ov_000::02156648 RollAccuracy
ov_000::02156b38 GetResistance
ov_000::02156cc4 RollCritical
ov_000::02156e30 RollBlock
ov_000::02156f98 RollEvasion
ov_024::021da55c DispatchDamageHandler
ov_024::021e6a90 CalculateFinalDamage
ov_024::021eb5d0 ResolveAction
ov_000::02153aa4 RechooseMonsterInGroup   # picks another standing monster of the group (NextRandomMax)
ov_000::02153c24 RechooseGroup            # a group with none standing -> a drawn one that has
ov_000::0215440c SettleMonsterTargets      # alive stored targets, else DefaultPick; then RollHitCount
ov_000::021540fc SettlePartyTargets        # by side and reach; then RollHitCount
ov_000::02154a04 PickDefaultTargets        # by side and reach; weighted pick for reach 2/5; then RollHitCount
ov_000::02154f30 WeightedPartyPick         # front line 2, back line 1; NextRandomMax(total)+1
ov_000::02155184 RegisterDefeatedMonster   # exp/gold sums, kind list
ov_000::0215833c RollActorStatusWake       # NextRandomMax(100) per action before ResolveAction
ov_000::0215858c TickActorStatus           # NextRandomMax(100) per action after it, battle going on
ov_000::02157d3c CheckBattleEnd            # [battle+0x8e14]: 2 party down, 1 monsters down
ov_000::0215f1ac ReorderTurnRecords        # turn classes 10, 5, 1 to the front, 32 to the back
ov_000::0215f57c GetExtraActionCount       # NextRandomMax(2) always; table [0,coin,1,2,...] by record bits 8-10
ov_000::0215f7a8 TryPartyFlee              # world generator
ov_000::0215fbe0 RollHitCount              # two draws every call: Between(3,4), Between(6,8)
ov_024::021f66cc DispatchTargetHandler     # action +0x0c (AI mode 1) / +0x0e (mode 2)
ov_024::021f418c TargetHandlerFlee         # handler 112
ov_023::021edf54 BattleVictory
ov_023::021f454c RollDrops
0x0208a04c IsWayUsable                     # func_0208a03c
0x0208a380 ChooseWayByWeights              # func_0208a370
0x0208a92c ChooseMonsterWay                # func_0208a91c: rule by record +0x10 bits 5-7
0x02032380 RollOneIn                       # func_02032370: rand() based
```

(`ov_024::021e7bc0 GetAttackBaseDamage` and the `NextRandom*`/`CalculateCritRate`/
`CalculatePhysicalDamage`/`RoundUp` functions are already named by the decompilation.)

## `port:` markers

Fully translated and cited: `NextRandom`, `NextRandomMax`, `NextRandomFloat01`,
`NextRandomFloatBetween` (`battle/random.rs`), `RoundUp`, `CalculatePhysicalDamage`,
`CalculateCritRate` (`battle/damage.rs`). Not marked: `ResolveAction`, `CalculateFinalDamage`,
`GetAttackBaseDamage`, `InitMonsterBattleStatus` and the rolls (partial, status-free subsets).

The battle's flow (`setup`, `round`, `ai`, `end`) marks nothing either: `ProcessCombatTurn`,
the way rules, the targeting handlers, `TryPartyFlee` and the victory routine are translated for a
battle without status, tension, guard, co-op action or grotto, so none is fully reimplemented.
