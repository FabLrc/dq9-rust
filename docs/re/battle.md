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
*Battle-Resolution*, *Actions*, *Ability-Handlers*, *Battle-AI*, *Battle-Weight-Tables*,
*Monsters* (facts only, not copied); Ghidra on the European ROM
(`scripts/ghidra-query.ps1`; the batch's excerpts are in
`build/evidence/j4-battle-actions/ghidra-{handlers,helpers,kinds,status,status3}.txt`); DQIX/BattleEmulator (MIT, commit
`6c4ad874d1c84579690c22c16efc58c945a6b218`) as the differential oracle.

**Addresses.** EU first, USA in brackets where the ARM9 module shifts them (`AGENTS.md`: USA
`0x0200F3B8..0x0200FB20` is +`0x8`, `0x0200FB20..0x020EEBD0` is +`0x10`); overlays are identical.

## Scope of the kernel

One action, of a party member or a monster, on any number of targets: the plain Attack, the
damage and healing spells of chapter 2 (kind 1 and kind 2), the stat changes of Buff and
Accelerate (kinds 4 and 5), Dazzle (kind 19), Weird Dance's MP damage (kind 57), a call for
backup (kind 12), a kind-0 action and the monsters' Flee `0xE1`. The accuracy roll models an
action whose accuracy stands at 100 and a monster's scaling accuracy with the dazzle die of
eight, the guard level and the dazzle flag. Everything else is **refused before any draw** with
`Unsupported`: damage handlers 1..66, riders, a party member's scaling accuracy, tension, a guard
level above 3, metal bodies, a trait `0x11d` critical, an elemental weapon, a weapon that strikes
several, the multi-target falloff against several targets, the accuracy die of four, and the
actions the code singles out by number (`0x1B`, `0x40`, `0x48`, `0x70`, `0x7E`, `0xAF`, `0x79`,
`0x395`, `0x1FF`, `0x200`, `0x20B`, `0x20C`, `0x82`, `0x205`). The chapter 2 actions and handlers
are below.

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
- **Fullheal is a kind-handler override, not another range.** The European action 33 has
  range 0, kind 2, cost 24 and no damage handler. `ov_024::021db3a4` (the heal handler,
  also identified by the wiki's [Ability-Handlers](https://github.com/DQIX/wiki/blob/main/Ability-Handlers.md))
  replaces its incoming amount with the target's maximum HP for action `0x21` (33) or
  `0x310` (784), then passes it to `ov_000::0215a16c`, which holds HP under that maximum.
  The normal base/final amount calculations and their draws have already happened.
  `resolve_action` applies this override at the kind-2 application boundary; changing every
  range-0 action to a new "full heal" amount would incorrectly skip those draws.
  Action 784 is Omniheal with MP cost 128: that cost remains explicitly unsupported, so
  this correction does not claim Omniheal support or model its MP policy.

  Checked on the EU ROM with the existing `dq9re action` command and Ghidra; local evidence
  `build/evidence/j4-full-heal/ghidra-heal-kind.txt` (`69b5515a8fed`),
  `ghidra-apply-hp.txt` (`5ab254622c18`), `action-records.txt` (`69fee1fd0d7e`).
  The permanent ROM regression failed before (HP 37 → 41 rather than 321) and passes after,
  at defence 0 and 999 while retaining the respective five/four ordinary draws:
  `before.log` (`075bff88690d`), `after.log` (`cbbe8c54de72`).
  A throwaway executable exercised the real `Battle::round` consumer with a synthetic
  wounded party and ROM-backed skill/monster data: HP 37 → 321 and MP 40 → 16
  (`smoke.log`, `2d4fc32e2122`). It was removed after the run; this is not a continuous
  chapter-route or interactive UI claim. No new function is marked ported: the handler's
  other state/message work is still partial.

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

### The chapter 2 kinds and the oracle

BattleEmulator (commit `6c4ad874…`) does not model Defend, Buff, Accelerate, Dazzle, the
medicinal herb or a call for backup: `callAttackFun`'s switch has no `BUFF`, `MEDICINAL_HERBS`,
`ACCELERATE` or `DAZZLE` case (its `BUFF` and `MEDICINAL_HERBS` constants only name actions), its
`DEFENCE` is an enemy attack named so, and `KASAP`/`DECELERATLE` model the *down* kinds 4 and 5
with a seven-turn state the game writes as six. No new vector could therefore be produced from it
for this batch; the 306 existing vectors still replay unchanged, and the statuses and handlers are
pinned by the Ghidra reads (`build/evidence/j4-battle-actions/ghidra-{handlers,kinds,status}.txt`)
and the 20-seed whole-battle tests instead. The kernel batch's harness and its patch lived in
`build/` of that batch's worktree and were not kept: `docs/re/battle.md` records the method, not
the program.

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
| 225 Flee | 0 | 1, 4, 20, 23, 30, 32, 43, 56, 57, 63, 290, 292 | resolved (leaves, no rewards) | handler 112 |
| 236 medicinal herb | 2 | 23 | resolved | handler 66 |
| 41 Buff | 4 | 23 | resolved (defence +1) | handler 18 |
| 45 Accelerate | 5 | 20 | resolved (agility +1) | handler 26 |
| 59 Dazzle | 19 | 32 | resolved (dazzle, count 4) | handler 54 |
| 323 | 19 | 63 | resolved (dazzle, count 4) | handler 54 |
| 243 Weird Dance | 57 | 43 | resolved (MP damage) | handler 117 |
| 344 | 12 | 43 | resolved (calls for backup) | handler 116 |
| 364, 366 | 0 | 56, 43 | resolved (no effect) | default pick |

`tests/battle_rom.rs` plays each of these monsters to the end over 20 seeds
(`chapter_two_ways_play_to_the_end_over_twenty_seeds`), every chapter 2 monster that holds Flee
(`fleeing_monsters_leave_over_twenty_seeds`), and checks that Buff moved the defence level,
Accelerate the agility level and Dazzle the party's status. The party's Defend and an item are
checked by `defend_guards_for_the_round_and_an_item_is_reported`.

## The chapter 2 actions

Read on the European ROM in overlay 24 unless said. The kind dispatch is a table of
pointer-to-members at `data_ov024_021ff508` indexed by the action's kind (`+0x18` bits 5–11),
called once per target with the accuracy roll's answer (`param_7`) and the final damage; none of
these handlers draws.

| kind | handler (EU) | effect |
|---|---|---|
| 0 | `0x021da670` | nothing; for action `0xE1` (and `0x395`) it removes the combatant (below) |
| 1 | `0x021da9e0` | damage, riders (as before) |
| 2 | `0x021db3a4` | a heal (as before) |
| 4 | `0x021db7c0` | defence level `+0x30` held to ±2 (`func_020878b4`, count 6 at `+0x6f`) |
| 5 | `0x021db994` | agility level `+0x30` held to ±2 (`func_020879a8`, count 6 at `+0x70`) |
| 12 | `0x021dc2a0` | a call for backup: adds monsters (`func_0x02161380`), no HP change |
| 19 | `0x021dd534` | dazzle: `func_02088854` sets `+0x14` bit 6, count 4 at `+0x5f`, second count `+0x82` |
| 57 | `0x021e0bf4` | MP damage: the final amount, held under the target's MP, taken from it |

**Flee `0xE1`.** Action 225: kind 0, reach 1 (the actor), accuracy 100 unscaled, no range. The
kind-0 handler's `param_3 == param_2` arm, for action numbers `0xE1` and `0x395`, calls
`func_0x021554f4(battle, victim, victim, action, 0, 2)` — the defeat routine with reason 2 — sets
the status HP to 0 and `func_02088f68`. A fled monster is therefore gone for `CheckBattleEnd`
(`func_ov000_02157d3c`), worth no experience or drop (only `func_ov000_02155184` pays, and the
victory collects `kills`), and the battle is won when none is left. `Battle::has_fled` reports it.

**Buff and Accelerate.** `+0x30` is 1 on both (action records 41 and 45). `func_02087860` /
`func_02087954` refuse a level already at the action's bound, else `func_020878b4` /
`func_020879a8` store `clamp(level + step, −2, 2)`, a count of 6, and the flag `+0x14` `0x800` /
`0x1000`; a level of 0 clears it. The multipliers are
`CalculateDefenseBuffMultiplier`/`…Agility` (Unlicense decompilation): `1 + 0.5 × level` up,
`0.5` at −1 and `0.25` at −2; `RoundUp` then gives the number the resolver reads
(`UpdateCombatantDefence`, `…Agility`), and initiative scores the buffed agility. The counts run
down per action (below).

**Dazzle.** `func_02088854` sets `+0x14` bit 6, a count of 4 at `+0x5f` and clears the second
count `+0x82`; the handler stores `+0x30` at `+0x22` bits 6–8, a cosmetic sort. While the flag is
set, `RollAccuracy`'s last step throws `NextRandomMax(8)` for an action carrying `+0x10` bit 3
(the plain Attack, Dazzle's own record included) and misses under 5 — read at `0x02156a90` in the
EU decompilation, and the die is drawn **only** when the striker is dazzled.

**Weird Dance, kind 57.** `func_ov024_021e0bf4`: the amount the resolver computed (range 43, 2 ± 1,
capped at 5) is held under the target's MP and taken from it; no HP change, no draw. The record
carries element 12, its landing resisted by 12, 100 accuracy and the combo bit.

**A call for backup, kind 12.** `func_ov024_021dc2a0`, on a landed pass, asks
`func_0x0215fb54` for the monster numbers the action calls (its own kind by default), tests each
with `func_0x02162d88`, then adds each with `func_0x02161380` while the battle holds fewer than
five monsters — eight for the battle whose monster is 266 (`0x021f4764`). Handler 116 refuses the
way when the count is reached. The added monster's HP is drawn by `InitMonsterBattleStatus`
(`func_02089630`) from the world's generator: **[hypothesis]**, the call into it was not read; the
model draws from the world generator and clones the caller's record.

**Actions 364 and 366 (kind 0).** Nothing: opening lines 350 and 354 ("is just fluffing
around"), no range, reach the actor. The resolver still computes an amount for the landed pass
(base amount's draws and the coin), which the kind-0 handler ignores.

### Targeting handlers 18, 26, 54, 66, 116, 117

Read in the European decompilation; all are `func_ov024_…` in overlay 24.

- **18 Buff** (`0x021eee68`, waiting for `func_02087860`): the caller's own standing side whose
  defence is below `0xffff` and whose level is below 2; one by `NextRandomMax(count)`
  (`func_ov024_021ed890`).
- **26 Accelerate** (`0x021ef7b8`): the same for agility below 999 and level below 2.
- **54 Dazzle** (`0x021f1198`): every standing party member (`func_0x0215e9fc(…, 4, 1)`), usable
  while one of them lacks `+0x14` bit 6 (`func_ov024_021df6ec` tests it).
- **66 medicinal herb** (`0x021f1c64`): one of the caller's own side below half its HP
  (`func_ov024_021db358` returns HP / maximum), by `NextRandomMax`. **[hypothesis]** the
  comparison's direction: Ghidra loses it (`_fgeq`), the herb on a wounded ally is the only
  reading that makes sense.
- **116 a call for backup** (`0x021f45c8`): the caller itself, while the battle holds fewer than
  five monsters (eight for monster 266); the group-record tests run first.
- **117 Weird Dance** (`0x021f4768`): a party member with MP left, then `func_ov024_021ed8c0`'s
  weighted picks by the hit code (one pick for 0 and 10; several are refused). Action 243's hit
  code is 0 or 10 in practice (the whole-battle test picks it).

### Defend, guard level and items

`Command::Defend` sets the member's guard level to 1 at the command phase
(`func_ov024_021e57c0`, reached through the action's sub-effect table `0x021ff3f8`, entry 2;
action 3 "Defend", kind 0, reach 1); `func_ov000_0215e6e8` clears it for everyone at the round's
end. `CalculateFinalDamage` applies `[1.0, 0.5, 0.1, 0.0][level]` (table `0x020e88c0`) to an
action carrying `+0x10` bit 4 — the plain Attack, Frizz and Crack among 243 of the 681 — **before**
the 0-or-1 coin, whoever struck. No link between the guard level and the monsters' pick weights
was found: the weighted pick (`func_ov000_02154f30`) halves a member's weight under status
`0x8000000`, not `+0x21` (**[unknown]** what sets `0x8000000`). **[hypothesis]**: Defend costs no
turn record and no draw, since the stance is taken up as the round begins; the command code was
not read.

`Command::Item` resolves an item's action like any action and emits `Event::ItemUsed { who,
action }`. The item comes from the acting member's **held items**, not the party bag: the
battle's item command carries the held slot (`func_ov017_021d0164` in the item handler table
at `0x021d823c`, which clears that member's slot through `func_0208386c`, EU `0x0208387c`),
and the battle command UI builds its item list from the member's item entries
(`func_ov000_0217ae90` case 4 → `func_ov000_02171c04`), never from the bag. A public rule
states the same for the series: items in the bag cannot be used during battle (DQ Wiki,
*Bag*, <https://dragonquest-wiki.com/Bag>: "Items in the bag cannot be used during
battle"). `dq9-app` therefore lists `Character::held_items`, keeps the row's slot (duplicates
included), and spends exactly that slot when the engine reports `ItemUsed`.

**Which uses spend the item.** The item record's `+0x08` word, bit 19, is the "used up" flag:
the AI reads it to score an item that is consumed (`func_ov024_021f7478`, `(word & 0xfffff)
>> 0x13`), and it is set exactly on the consumable tools and on the prayer ring (18023, an
accessory that breaks when used). Measured on the EU tables over all 1,178 records
(`build/evidence/j4-battle-held-items/item-flags.txt`, `755490182234`): 46 of 234 tools and 1 of 52
accessories set it; every weapon, shield, helm, armour, glove, legwear and other accessory
leaves it clear, including the weapons and shields with a battle action (cautery sword 20010,
brain drainer 21010) and the reusable tools (sage's stone 22035, mimic stone 22125,
blarney stone 22126). Consumables such as the medicinal herb (22000) set it. The field item
use (`func_020ddb38`, category 8–10) clears the chosen held slot the same way; the battle rule
adds the flag test so a reusable item use is not spent. The app models only held items, not
the worn-gear half of the original's item list (equipment UI is out of scope).

A member who falls before its turn produces no `ItemUsed`: the reservation is dropped at the
next command phase and the held slot stays. Cancelling or reselecting a command releases the
reservation likewise. **[unknown]** the exact routine that fills the original's item menu
with the held tools (the list builder preserves tool entries but its writer was not found);
the consumer and the slot identity are read, the population path is not. The bag is never
read or changed by the app's battle item path.

### Accuracy with statuses

`RollAccuracy` (`func_ov000_02156648`) draws `NextRandomMax(100)` first whatever the accuracy;
then a party member's trait die of four (refused); then, for a scaling action, a monster's
`+0x14` bits 0–6 times the target's resistance to the landing element `+ 0.5f`, truncated; then
the dazzle die of eight (only while dazzled); it lands under the accuracy. A party member's
scaling accuracy (a stat-scaled or drawn value) stays `Unsupported::AccuracyScaling`.

### Statuses' run-down

`func_ov000_021599f4` takes one off every count held on each of its holder's passes; at 0 the
second count starts at 4 (`data_ov000_02182efc`, 4 for defence, agility and dazzle). The wear-off
`func_ov000_0215858c` runs just before it on the same pass: after the tick's mandatory
`NextRandomMax(100)`, each status whose second count is running takes one off it, draws
`NextRandomMax(100)` and is cleared where the chance by the count (`0x02182ad4`: 1.0, 0.875,
0.75, 0.625, by the remaining count from 1 to 4) is above the draw. A defence level therefore
holds six of its holder's passes; the model's counts and second counts are in `Status`.

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
- **[unknown]** the accuracy roll's remaining statuses and traits (the party trait's die of 4)
  and the metal-body early miss: refused rather than modelled. Dazzle's die of 8 and a monster's
  scaling accuracy are modelled.
- **[hypothesis]** Defend's absence of a turn record and of draws: the stance is taken up as the
  round begins (the wiki's reading of the command), but the command code was not read.
- **[hypothesis]** a call for backup's new HP is drawn from the world's generator by
  `func_02089630`; the call into it was not read, only the handler's `func_0x02161380`.
- **[hypothesis]** the wear-off second count starts at 4 for defence, agility and dazzle
  (`data_ov000_02182efc`'s 4), and the chance table `0x02182ad4` is indexed by the count that
  remains, from 1 (1.0) to 4 (0.625).
- **[hypothesis]** a scaling accuracy's critical does not land outright: `RollAccuracy` returns 1
  before the multiplier when the target's resistance is above 0 and the cast was a critical
  (`0x02156a90`'s neighbourhood), which the model does not carry (a Dazzle critical would land
  where the model still rolls).
- **[hypothesis]** the medicinal herb's handler uses "below half its HP"; Ghidra loses the
  comparison's direction (`_fgeq`), and the wounded-ally reading is the only sensible one.
- **[hypothesis]** the combo counter's stepping (`Battle::step_combo`) is the wiki's reading of
  `func_ov024_021ea584` and `func_ov000_0215cd80`: not checked against the code here.

## Symbols

The names this note establishes (decompilation names or the wiki's proposals, EU addresses) are
in `re/symbols.txt`, section "Battle kernel and flow". `0x02032380` is `RandBelow` there
(`(int)(n × (rand() − 1) / RAND_MAX)`, decompiled), the same function `docs/re/encounter.md`
uses. `ov_024::021e7bc0 GetAttackBaseDamage` and the `NextRandom*`/`CalculateCritRate`/
`CalculatePhysicalDamage`/`RoundUp` functions are already named by the decompilation.

### Symbols this batch proposes (for `re/symbols.txt`, which is not this batch's)

| address (EU) | name |
|---|---|
| `ov_024::021da670` | `KindNothing` (kind 0; removes a combatant for `0xE1`/`0x395`) |
| `ov_024::021db7c0` | `KindDefence` |
| `ov_024::021db994` | `KindAgility` |
| `ov_024::021dc2a0` | `KindCallForBackup` |
| `ov_024::021dd534` | `KindDazzle` |
| `ov_024::021e0bf4` | `KindMpDamage` (kind 57) |
| `ov_024::021eee68` | `TargetHandlerBuff` (18) |
| `ov_024::021ef7b8` | `TargetHandlerAccelerate` (26) |
| `ov_024::021f1198` | `TargetHandlerDazzle` (54) |
| `ov_024::021f1c64` | `TargetHandlerHerb` (66) |
| `ov_024::021f45c8` | `TargetHandlerCallForBackup` (116) |
| `ov_024::021f4768` | `TargetHandlerWeirdDance` (117) |
| `ov_024::021ed890` | `PickTargetEven` |
| `ov_024::021ed8c0` | `PickTargetsByHitCode` |
| `ov_024::021db358` | `HpShare` |
| `ov_024::021df6ec` | `IsDazzled` |
| `ov_024::021e57c0` | `SetGuardLevel` |
| `0x020878b4` | `SetDefenceLevel` |
| `0x020879a8` | `SetAgilityLevel` |
| `0x02088854` | `SetDazzle` |
| `0x02087860` / `0x02087954` | `CanMoveDefenceLevel` / `CanMoveAgilityLevel` |
| `0x02088840` | `IsStanding` |

## `port:` markers

Fully translated and cited: `NextRandom`, `NextRandomMax`, `NextRandomFloat01`,
`NextRandomFloatBetween` (`battle/random.rs`), `RoundUp`, `CalculatePhysicalDamage`,
`CalculateCritRate` (`battle/damage.rs`). Not marked: `ResolveAction`, `CalculateFinalDamage`,
`GetAttackBaseDamage`, `InitMonsterBattleStatus` and the rolls (still partial: no tension, no
riders, no damage handler).

The battle's flow (`setup`, `round`, `ai`, `end`) marks nothing either: `ProcessCombatTurn`,
the way rules, the targeting handlers, `TryPartyFlee` and the victory routine are translated for a
battle without tension, co-op action or grotto, and with the chapter 2 statuses only, so none is
fully reimplemented.
