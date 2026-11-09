# Battle kernel: generators, draw order, damage

Implementation: `crates/dq9-core/src/battle.rs` and `battle/` (`random`, `model`, `damage`,
`resolve`, `monster`). Tests: `battle/tests.rs` (unit) and `tests/battle_oracle.rs` (differential,
fixtures `tests/data/battle_oracle_{primitives,actions}.txt`).

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

## Open points

- **[unknown]** what the two draws before every action in the oracle are; the kernel starts after
  them. A real action of the game spends the die, critical, … as above; the two are the oracle's
  own bookkeeping or an unexplained pair.
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
- The combo counter is an input (`combo_count`); stepping and resetting it
  (`func_ov024_021ea584`, `func_ov000_0215cd80`) is the caller's.

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
```

(`ov_024::021e7bc0 GetAttackBaseDamage` and the `NextRandom*`/`CalculateCritRate`/
`CalculatePhysicalDamage`/`RoundUp` functions are already named by the decompilation.)

## `port:` markers

Fully translated and cited: `NextRandom`, `NextRandomMax`, `NextRandomFloat01`,
`NextRandomFloatBetween` (`battle/random.rs`), `RoundUp`, `CalculatePhysicalDamage`,
`CalculateCritRate` (`battle/damage.rs`). Not marked: `ResolveAction`, `CalculateFinalDamage`,
`GetAttackBaseDamage`, `InitMonsterBattleStatus` and the rolls (partial, status-free subsets).
