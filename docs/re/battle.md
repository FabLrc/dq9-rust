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

Cross-check, added 2026-11-17: the [DQ9 Encyclopedia](https://dq9encyclopedia.com/en/guide/battle-the-games-formulas)
states the multiplication order (tension, caps, critical, element, weapon family, guard,
multi-target falloff), the metal rules, the coup-de-grâce thresholds, the enrage triggers, the
front/back-line and grudge target weights and the 21 "wants" of the companion tactics, each
block citing the function that implements it (overlay addresses as they are, ARM9 ones in USA).
No license: facts only, never copied, and **[hypothesis]** here until re-checked on our ROM or
in the cited function (`external-resources.md`).

Audited 2026-11-17 in [`encyclopedia-audit.md`](encyclopedia-audit.md), function by function. The
verdicts that touch this note: the multiplication order **agrees** except the cap's place in the
page's list — `ov_024::021e6a90` truncates once and clamps `+0x1C` bits 0–13 last, as this note
already read. The coup-de-grâce thresholds, the enrage triggers (`ov_024::021eb08c`) and the
companion tactics are **not implemented** (refused or absent), so nothing diverges silently; the
metal rules **agree** and are modelled (`v4-metal-rules`, below, "The metal body"), and so does
the family bonus, modelled by `v4-family-bonus` (below, "The weapon's killer bonus"). The rules
the audit found **differing** are now modelled: the two buff wear-off
chance arrays — `ov_000::0215858c` reads one for dazzle (`0x02158d6c`) and one for defence and
agility (`0x02158d68`, four bytes earlier), indexed by the decremented count (`j4-audit-fixes`,
`ed0974b`; below, "Statuses' run-down") — and the grudge weights (`j4-grudge-enrage`, `4e0a0ed`;
below, "Monster AI"). Flight's species 38–40 refusal is modelled too (`ed0974b`; below, "Flight").
Enrage itself is read but not modelled, and no chapter-2 fight carries a trigger (below, "Enrage").

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
level above 3, a trait `0x11d` critical, an elemental weapon, a weapon that strikes several, the
multi-target falloff against several targets, the accuracy die of four, and the actions the code
singles out by number (`0x1B`, `0x48`, `0x70`, `0xAF`, `0x79`, `0x395`, `0x1FF`, `0x200`, `0x20B`,
`0x20C`). A metal body is not refused any more: its rules are below, and the four actions they
single out (`0x40`, `0x7E`, `0x82`, `0x205`) are now refused by the handler or the rider they
carry. The chapter 2 actions and handlers are below.

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

Order read in `build/evidence/j4-battle-kernel/ghidra-final-damage.txt` and re-read on the EU
decompilation for this batch (`build/evidence/v4-metal-rules/final-damage.c`): tension multiply
at the head (not modelled) → critical → `× GetResistance(element)` (`_fmul`, line 182, float) →
**the weapon's killer bonus by the target's family** (party attacker, `+0x10` bit 18; the twelve
`_fmul` at lines 300–365, below) → block and dodge zero it → the poison needle's `1.0` →
**the metal body's zeroing** → **coin**
`NextRandomMax(2)` (line 529) when the damage is not above 0, the target was not blocked or
dodged, the action is not `0x1B`/`0x48`/`0x70` and the target's resistances to the action's both
elements are above 0 → **Metal Slash's `1 + random(2)`** → **the `+1` weapon** → guard (not
modelled) → combo table (`0x021fe778`, `1.0 1.2 1.5 2.0`, counter held to 3, line 574–587) →
`_ffix` **once** (line 594) → cap `+0x1C` bits 0–13 (0 = none).

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

### The weapon's killer bonus (`ov_024::021e6a90`, `0x021e7200`–`0x021e7494`)

`CalculateFinalDamage` multiplies a blow by the worn weapon's killer bonus for the **target's
monster family** (`docs/re/battle-data.md`, `mon_data` `+0x0A` bits 7–10). Read on the EU
decompilation (`build/evidence/v4-family-bonus/final-damage-family.txt`; decompilation lines
300–365 of `v4-metal-rules/final-damage.c`) and on the accessors' own code and literal pools
(`accessors.txt`):

1. **The gate.** `0 <= attacker < 4 && func_0200ff1c(gs, attacker) != 0 &&
   (action + 0x10 & 0x40000) != 0` — a **party** attacker and an action carrying `+0x10` bit 18
   (`Action::weapon_element`; the plain Attack carries it). A monster attacker never enters the
   branch.
2. **The twelve queries.** For `n = 1` to `12`: `func_02156068(battle, target, n, 0) != 0`
   (the target's record bits 7–10 equal `n`) → `damage = _fmul(damage, accessor(character))`.
   The accessors `func_02085968` … `func_02085888` read a **signed six-bit** field of the
   weapon's copied `itembtlprm` record (`char + 0x2F4`, its `+0x08`/`+0x0C`/`+0x10` — the twelve
   fields `docs/re/battle-data.md` names), convert it with `_fflt`, then `_fdiv` by the literal
   `0x41200000` (**10.0f**) at each accessor's pool in `arm9.bin`. The multiplier is therefore
   `tenths / 10.0` in single precision, and 10 is none.
3. **One multiply in practice.** The family field holds one value: bits 7–10 answer one `n`, or
   none — 0, 13, 14 and 15 answer no query at all (31 of the 438 name records hold 0, 111 hold
   13). Every step is `f32`; the single `_ffix` (line 594) truncates once, at the end, so a blow
   of 10 against eleven tenths deals 11.
4. **A party target cannot be boosted**: `func_02156068` answers 0 for a party index, so the loop
   body never runs. `Fighter::family` is 0 on a party fighter.
5. **The empty weapon slot** (`char + 0x2AC < 1`): all twelve accessors answer
   `movle r0, #0x3f800000` — **1.0f** — before reading their field (`0x02085984`–`0x0208598C`;
   the decompiled bodies render it as a bare `return`, `accessors.txt`). An unarmed party blow is
   therefore multiplied by exactly 1.0, which the port's default ten reproduces bit for bit
   (`Fighter::family_bonus = [10; 12]`).

Where the step sits: after the resistance multiplication, **before** the target's status
multipliers (`0x021e7494`), the needle and the metal zeroing. The port applies it at the same
place (`resolve::final_amount`), and it draws nothing, so no vector or route changes from its
call count.

**The pilot's route is unaffected, but not for the reason first assumed.** The new game's copper
sword 20004 is **not** all ten: its family-2 row is eleven (`item-rows.txt`). What keeps the
route's checkpoints in place is that no monster of the chapter-2 set answers family 2: the set
(F01 map 20001 and the Hexagon floors 7102–7104 through `encfld`/`encbtl`, plus set battles 2 and
26) answers families 0, 1, 3, 4, 5, 6, 7, 8, 9, 10 and 11, and the Hexagoon (set battle 2) is
family 1, whose row is ten. The hero is the only party member on the route. Both facts are pinned
by `tests/battle_rom.rs::chapter_two_coverage_lists_what_the_kernel_and_the_ai_resolve`.

Tests: `battle/tests.rs` — `the_weapon_family_bonus_multiplies_a_party_blow_by_its_tenths`
(ten/eleven/twelve/thirteen tenths and one tenth on a flat 10, the bonus drawing nothing) and
`the_weapon_family_bonus_needs_the_party_the_bit_and_the_target_family` (the bit-18 gate, a party
target, a monster attacker); `tests/battle_rom.rs::the_weapon_family_bonus_reads_the_rom_rows_and_the_monster_family`
(the copper sword's rows, the metal slime spear's eleven tenths against a family-1 monster of the
battle data — `boosted == (plain × 1.1) truncated` on every seed that is not a critical — and a
party target left alone). The 306 oracle vectors replay unchanged (no fighter there sets a family
or a weapon row; the default ten is an exact `× 1.0`).

Evidence (`build/evidence/v4-family-bonus/`): `final-damage-family.txt` (`6a4154f63ae5`),
`accessors.txt` (`558086c0be9a`), `item-rows.txt` (`d53a2aee9d3a`), `families.txt`
(`1edd40de85c9`), `challenger.md` (`7ef09a895177`), `verify-full.log` (`8e0b1cdef429`),
`route-long.log` (`8569516d0380`), `parity.log` (`b0ff7fac51e4`), `notes.md`
(`1d5f470d9904`).

### The metal body (`ov_000::02156648`, `ov_024::021e6a90`)

The monster's metal flag is `mon_data_` record `+0x0A` bit 12, read by
`func_ov000_02156068(battle, index, 0, 1)` — the same accessor that answers bit 11 (a refused
fall) and bits 7–10 (the family); a party index never answers it. Facts, all read on the EU
decompilation and its disassembly (evidence `build/evidence/v4-metal-rules/accuracy.c`,
`final-damage.c`, `resolve-action.c`, `accuracy.asm`, `final-damage.asm`, `notes.md`):

1. **The miss, before any draw.** `RollAccuracy` (`ov_000::02156648`) leaves with 0 when the
   target is metal and the action does **not** carry `+0x10` bit 24 (`0x100_0000`). The miss
   comes before the accuracy's own `NextRandomMax(100)`, so such a blow costs its die, critical,
   evasion and block draws and then nothing: no accuracy die, no amount, no coin. The `!= 0x150`
   guard at the head of the function covers only its two **status** early misses (target `+0x18`
   bits `0x800`/`0x40`), not the metal branch — read in the assembly
   (`build/evidence/v4-metal-rules/accuracy.asm`: `cmp r1,#0x150; beq` jumps over the status
   blocks to `0x02156714`, and the metal test is the later `bl 0x02156068` at `0x021567ac`).
2. **The zeroing.** With `+0x10` bit 24 set and an action aimed at the opponents (`+0x08` bits
   8–9 at 1, else `0xDB`), a blow that is **not** a critical has its damage set to exactly 0
   unless the action is Quadraslash `0x205` or Needle Shot `0x82`. The coin that follows then
   makes it **0 or 1** — the encyclopedia's "the others deal 0 or 1" — and it is the only draw a
   metal body adds to an ordinary blow. The gate at `0x021e77a4`–`0x021e7820` in
   `final-damage.asm` is `IsMetalBody` → bit 24 → aim 1 or `0xDB` → not critical → number not
   `0x205`/`0x82` → not the needle → **damage = 0**; the coin is
   `damage <= 0 && !dodged && !blocked && number != 0x70/0x48/0x1B && both resistances > 0 && !(metal
   with bit 24 clear)`. Ghidra's decompilation of the same coin renders a `0x47 < number` bound
   that the assembly does not contain (there is only `cmp r0,#0x48` for the equality): the bound
   is a decompiler artefact, and the pinned action-1 data would read a plain Attack as 0 if it
   were real. `uRam021e7ba0` is read from the overlay at `0x021e7ba0` = **`0x205`**.
3. **A critical is not zeroed.** The staging byte's bit 7 (`[battle + 0xC] + 0x1C`, set by
   `ResolveAction` on a successful `RollCritical`) is the gate; the blow keeps
   `CalculateCriticalDamage`'s amount.
4. **Metal Slash `0x40` and Metalicker `0x7E`**, not critical, on a metal body: `1.0f +
   NextRandomMax(2)`, **after** the coin, so they spend the coin's draw and then their own — 1 or
   2.
5. **The `+1 against metal` weapon**: `func_02085128` tests **bit 4** of the `itembtlprm`
   `+0x00` word of the worn weapon — the record `func_ov017_021b3780` copies to the character's
   `+0x2F4` (`docs/re/item.md`, "The weapon words") — and adds `1.0` to a metal body's damage on
   a party blow carrying `+0x10` bit 18: with the coin it is 1 or 2. Seven of the table's 1,423
   records carry the bit: ids 20021–20023, 20119 and 20517–20519, all weapons, all element 8
   (`build/evidence/v4-metal-equipment/item-flags.txt`, `6f15e467da33`). The other
   accessors of the same record read bits 5 to 7; their meaning is not established here.
6. **The poison needle**: `func_020851d8` tests **bit 10** of the same copied record (19057 is
   the one record of 1,423 that carries it, and it also holds bits 5 and 24; the accessor of bit
   5, `func_02085154`, clears bit 2 of the staging byte on an action carrying `+0x10` bit 19).
   On a party blow carrying `+0x10` bit 18 the damage is exactly `1.0`, and the dodge and block
   flags it caused are cleared — S5's "always deals 1 and cannot be dodged or blocked". Needle
   Shot `0x82` sets `1.0` the same way, before the metal rules, and is exempt from the zeroing.
   The needle's `1.0` and the zeroing's `0` are both overwritten by nothing that follows, except
   the combo table.
   The same bit 10 also **auto-hits** (`RollAccuracy`, `ov_000::02156648`, `0x02156814`–`0x02156830`):
   after its die of 100 (`NextRandomMax(100)` at `0x021567d4`) and before the scaling accuracy
   (`0x02156868`) and the spoilt action's die of eight (`0x02156a90`), a party attacker whose
   weapon has bit 10 and whose action carries `+0x10` bit 18 returns 1 outright — the die of 100
   is spent, the die of eight is not, and no accuracy comparison happens. The port reads the two
   existing inputs, `Action::weapon_element` (bit 18) and `Fighter::poison_needle` (the weapon's
   bit 10), not a new duplication of the same bit. The instruction order in
   `accuracy.asm` is `ldr r0,[r7,#0x10]` / `tst r0,#0x40000` → `func_020851d8` → `movne r0,#1` →
   `bne 0x02156ae4` (the function's return), with the `0x10000` trait gate and the accuracy block
   after it. A dazzled needle user's spoilt (`+0x10` bit 3) action therefore never misses
   (`build/evidence/v4-needle-accuracy/`).
7. **What stays refused or absent.** The records of Metal Slash (64), Needle Shot (130),
   Metalicker (126) and Quadraslash (517) carry damage handler 2 / rider 20 / handler 66: they are
   refused by **those** rules now (`Unsupported::DamageHandler`, `Unsupported::Rider`), not by
   number. The metal rules themselves are modelled, and the "+x against metal" and needle flags
   are the `Fighter` inputs `metal_bonus` and `poison_needle`, filled from the equipped weapon's
   record by `Character::fighter` (`party.rs`, `func_ov017_021b3780` + `func_02085128` /
   `func_020851d8`). Still absent, named: action `0xDB`'s zeroing
   without the aim gate (its reach 8 is refused anyway); action `0x61`'s second blow, which
   bypasses the whole accuracy roll (`ResolveAction`'s `param_7`, so it would hit a metal body at
   full damage); Needle Shot's `1.0` when the target's guard level is not 3 (its rider is refused
   first); the combo gate's second bit (`+0x2C` bit 26 as well as 27); and the metal-only
   re-multiply by a weapon's element on a metal body, which tension alone reaches (refused).

Tests: `battle/tests.rs` (the miss, the 0/1 the coin leaves, the critical, the two exemptions,
`1 + random(2)`, the aim gate, the two weapon flags, and
`the_poison_needle_auto_hits_after_the_die_of_one_hundred_before_the_die_of_eight` — a dazzled
needle bearer lands on every seed where a dazzled copper-sword hero misses on the die of eight,
with the generator walked by hand: five draws, the die of 100 present, the die of eight absent),
`tests/battle_rom.rs::the_metal_body_rules_read_the_rom_records` (monster 3 the metal slime and
monster 1 the slime, Frizz's miss, the four refusals by name, action `0x150`'s miss) and
`tests/battle_rom.rs::the_equipped_weapon_sets_the_metal_and_needle_flags` (the seven bit-4 ids
and 19057 over the 1,423 records, the copper sword's two false flags, the metal weapon's and the
needle's, and the needle's exactly-1 blow through the party's adapter), and
`tests/battle_rom.rs::a_dazzled_needle_bearer_auto_hits_before_the_die_of_eight` (an action record
of the ROM bearing bits 3 and 18, item 19057 through the adapter: it lands on all 64 seeds while
the copper sword keeps the die of eight). The assembly of the two
functions and the constants they compare against are kept with the dumps
(`accuracy.asm`, `final-damage.asm`); the decompiler's `0x47 < number` reading of the
coin was refuted there by a separate challenger pass and by the disassembly. The item record
behind the two flags was attacked by a separate challenger pass too (verdict confirmed; the ROM
relocation of the slot table, the two category switches and the table's raw bytes,
`build/evidence/v4-metal-equipment/challenger.md`). The chapter 2 route
is unchanged: `verify.ps1 -Only long` re-ran the pilot to event 2750 with the pinned digest
`d721c11fd0d2…`, 964 checkpoints (`build/evidence/v4-metal-rules/route-2750.jsonl`), and
`chapter_two_coverage_lists_what_the_kernel_and_the_ai_resolve` now asserts that no chapter-2
monster is metal and that action `0x150` is not one of its ways.

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
- **Weighted pick** (`func_ov000_02154f30`, read; `build/evidence/j4-grudge-enrage/ghidra-weighted-pick.txt`,
  `398f84e9a78c`): weight 2 per front-line standing member, 1 per
  back-line one; `NextRandomMax(total) + 1` walked down; a second draw `NextRandomMax(count)` only
  if the walk ends unspent. A monster whose record `+0x10` bit 26 is set adds **+2** for the party
  slot held at the record's `+0x32` and **+1** for the one at `+0x34`; a member held in both slots
  takes both. The memory is written by `func_0208978c` (EU `0x0208979c`, read: `+0x32` into
  `+0x34`, the new slot into `+0x32`; `ghidra-remember-attacker.txt`, `9bbfed1ba6e5`), called once
  per target from the resolution pass of
  `func_ov024_021eb5d0` (call at EU `0x021ed0d4`; `ghidra-remember-xrefs.txt`, `496bb2a7aefb`) for
  every party member's action that
  resolves on a monster — a miss included, the write sitting outside the handler's success test.
  `Battle::last_attackers` and `Battle::target_weights` model it; the memory starts at
  **`0xFFFF`** — no attacker — because `ResetBattleStatus` sets the four halfwords at `+0x28`,
  `+0x2A`, `+0x2C` and `+0x2E` and the two at `+0x32`/`+0x34` to `0xFFFF`
  (`build/evidence/j4-grudge-enrage/ghidra-reset-status.txt`, `43fc396acbc6`), which no party slot
  can equal. Two further rules
  of the same reader are read but **not modelled**: a candidate whose record `+0x14` bit 27 is set
  halves its weight (`func_ov000_0215516c`; the page reads that bit as invisibility, B25, and the
  model has no invisibility status, so the bit is never set), and a monster
  whose record `+0x18` bit 12 is set returns the slot at `+0x2e` outright (`func_ov000_02155154`:
  the enrage/Knight Watch lock, below). Neither bit is set anywhere in this model, so no divergence
  is hidden — a small party already picks by the weights above. The pilot's route to event 2750
  (seed 7, a solo hero) is **byte-identical** before and after the model: 972 checkpoints and the
  whole journal (`build/evidence/j4-grudge-enrage/before-2750-checkpoints.jsonl` and
  `after-2750-checkpoints.jsonl`, `8ce4af30e497`; `before-2750.out` and `after-2750.out`,
  `b97be937ab35`; the baseline binary was built from `b11a38f` in a throwaway checkout).

### Enrage (`ov_024::021eb08c`, read; not modelled)

**The record's fields.** `mon_btldata` `+0x24` is a word of **two trigger pairs**: kind A in bits
0–6 with its chance in 100 in bits 7–13, kind B in bits 14–20 with its chance in bits 21–27; bit
28 (byte `+0x27` bit 4) is a boss flag no instruction reads. `+0x28` and `+0x29` are the least and
most passes the state holds, drawn by `NextRandomBetween`. Both facts are read in
`ov_024::021eb08c` (`build/evidence/j4-grudge-enrage/ghidra-enrage.txt`, `f0b3cf311ca8`) and its
callees (`0x02088de8`, `0x02088e14`, `0x02088e58`): the function gets the target combatant, requires a
party slot as the provoking member (0–3), draws `NextRandomMax(100)` **before** any test, then
takes the pair whose kind equals the code asked; a draw below the chance fires. Firing draws the
length, stores it at the record's `+0x7e`, sets the record's `+0x18` bit 12, marks the combatant
(`+0x17f`), and writes the provoking slot at the record's **`+0x2e`** — the lock
`func_ov000_02155154` reads, which makes the weighted pick return that slot outright.

**The codes asked** (read at the call sites): `3` a party blow that took the monster's HP from at
least a half to below it, `4` the same across a quarter (`func_ov024_021da9e0`;
`ghidra-damage-ov24.txt`, `f0e8eb53e2e1`); `0x11` Whistle
(`func_ov024_021e0b48`), `0x12` Eyes on Me (`func_ov024_021e04e0`); `0x13` a party heal and `0x14`
a Zing (`func_ov024_021eb5d0`, the action's `+0x1C` bits 19–31 at 5 and `0xc`; which is which is
the wiki's reading, **[hypothesis]**); `0x18` a flag of the action record (`+0x0A` bit 0,
**[unknown]** what sets it). Every call iterates the **opposing side** (`func_0x0215eb1c`), so a
heal draws once per standing monster before each one's trigger is tested.

**Scope on the ROM** (`tests/battle_rom.rs`,
`the_grudge_bit_and_the_enrage_triggers_count_on_the_rom`; run
`build/evidence/j4-grudge-enrage/rom-counts.txt`, `4d05d28eb3e4`): 341 of the 438 records carry at
least one pair with a kind
and a non-zero chance (the page's 287 is not reproduced by the record's own word); the kinds
present are 3, 4, 0x11, 0x12, 0x13, 0x14 and 0x18. **The Hexagoon (set battle 2, monster 300) is
not one of them**: its word is `0x1000_0000`, both kinds and both chances zero, so no chapter-2
fight enrages and the target override is not modelled (the lock and `+0x18` bit 12 stay unread in
the engine). A heal or a Zing in a fight against an armed monster would make the original draw
`NextRandomMax(100)` per monster where the kernel draws nothing; none is modelled — the note's
open point on the draws after the wake roll stands.

### Flight (`Battle::try_flee`; `func_ov000_0215f7a8`, read; the command overlay is
`ov_026::021dd3dc`)

In order: the setup is a set battle → refused for good (`BattleError::FleeForbidden`, before any draw);
a monster of species 38, 39 or 40 in the battle → refused with no draw (`func_ov000_0215ed34`
lists the battle's groups, up to three, reading the species word at `+0x81b4` stride `0x18` from
the group count in bits 6–7 of `[battle + 0x81b1]`; the routine returns "no" on `species − 0x26 < 3`
before the surprise round is even considered — the page names them cannibox, mimic and Pandora's
box (**[hypothesis]** for the mapping), and the same range never surprises in `encounter.rs`;
disassembly in `build/evidence/j4-audit-fixes/rom-flight-species.txt`, `0f7ad1d25b5b`); the first
round with surprise 1 → away, no draw; no monster standing → away; the party's mean attack +
defence at least three times the monsters' mean (integer means over the standing) → away, no draw;
otherwise `max over the fleeing members of (int)(deftness × 0.05f) + 10`, held up to
`[25, 50, 75, 100, 65535][attempt]` (`0x02182c04`; the attempt counter `[battle + 0x8e1c]` goes up
on every such call), and `NextRandomMax(100) < percentage` on the **world's** generator.
Success ends the battle (`Fled`, no round). **[hypothesis]**: a failed flight costs the fleeing
members their turn and the round goes on (the flight state `ov_026::021d8ba0` state 0xb shows the
failure; what follows was not read). Test: `flight_is_refused_against_species_38_to_40`.

### End (`end.rs`)

Experience and gold accrue per monster beaten (`func_ov000_02155184`), once each; the victory
routine turns the sums into the totals `(uint)((float)sum × [battle + 0x8e3c])` (gold `0x8e40`):
both multipliers taken as 1.0 (**[hypothesis]**). Drops (`func_ov023_021f454c`): once per **kind**
(monster number in a group) in the order first beaten, rare then ordinary only if the rare did not
land, `func_02032380(oneIn) == 0` = `(int)(oneIn × ((rand() − 1) / 32767.0)) == 0` in `double`
(`0x020323d0` holds the divisor's high word), `oneIn` = 1, 8, 16, 32, 64, 128, 256, 0 (`0x021fd888`; 0
draws nothing), at most eight kept; the C library's `rand` is the caller's `CRand` (nothing in the
battle seeds it). Not modelled: the grotto and treasure-map branch, the group flags that force a
drop. Sharing the experience is `func_ov023_021f4098` (`Party::award_with`'s), weighed by the
member's level and by the battle's counter.

**The order the reward anchors impose** (batch `v3-reward-hooks`, EU code and the DQ9Recompiled
manifests' anchors, `docs/re/dq9recompiled.md` § 11):

| step | original (EU) | ours |
|---|---|---|
| totals | `ov_023::021edf54`: `(u32)(float([+0x8e28]) × [+0x8e3c])` → `ctrl+0x58c0`, gold `[+0x8e2c] × [+0x8e40]` → `ctrl+0x58c4` | `Battle::finish`: `(sum as f32) as u32`, both multipliers 1.0 |
| experience per slot | the same routine, per slot of the battle's mask: `func_ov023_021f4098(ctrl, slot, 0)` → `ctrl+0x5758 + 4·slot` (22360), **before** the results snapshot; every machine reaches `0x021ee4f4` once the array holds the leader's figures (guests receive them with message 106) | `Party::award_with` splits the total with `func_ov023_021f4098`'s formula and the battle's counter when the fight closes, before the level-up rows are read — the same numbers in the same order relative to the snapshot, for one player (`func_ov023_021f4098` scales the total by `1 + (connected players − 1) ÷ uRam021f442c` in a session; `award_with` has no such factor, the multiplayer path being unmodelled) |
| experience applied | `ov_023::021f5150`: each slot's `ctrl+0x5758 + 4·slot` added to `member + 0x138 + 4·vocation` | `Party::award_with`: the current vocation's experience |
| gold share | `ov_023::021f44d0`: `x = (float)ctrl+0x58c4 × ((float)u16 ctrl+0x58ce ÷ (float)u16 ctrl+0x58cc)`, raised to `1.0` when `0 < x < 1` (`0x021f4520`–`0x021f453c`, `0x200c088` = `fcmplt`; 0 stays 0), then truncated | the whole total: the ratio is 1 whenever the player stood through the battle (below), and the total is far above 1 |
| gold applied | `ov_023::021f5150`: the share is stored at `0x021f51ec` (`str r0,[r7]`, `r7` = the caller's `ctrl+0x5768`, the results amount) and added to the party's gold at `0x021f51f0`–`0x021f5208`, held to the literal `0x0098967F` | `Party::award_with`: `saturating_add`, held to `GOLD_MAX` |

`ctrl+0x58cc` is written from `ctrl+0x8e20` (the turn count, incremented by `func_ov000_0215e6e8`
at the end of every turn, `ov_000::02169770`'s call), and `ctrl+0x58ce` is counted once per turn in
`ov_026::021d8ba0` (the command-entry block that also adds the battle counter) for a player with at
least one member `func_02010088` answers 0 for. The ratio `0x58ce ÷ 0x58cc` is therefore 1 for a
single player whose party stood, and the whole total is stored and added — the `bat6.ml1` oracle
(13 gold from battle 26's 13) agrees with ours. A positive share below 1.0 is raised to 1.0 before
the truncation, so a player who was wiped for part of the battle still receives at least 1 gold; a
zero share stays 0. The two fields' multiplayer meaning remains **[hypothesis]** (which member
answers `func_02010088` and when); no reference observes it.

**The item-finding passes** (`func_ov023_021f454c`, read in the disassembly at `0x021f4618`–
`0x021f4b64`): the leader's pass (`psVar17[5] = 0xFF`) is followed by one extra pass per **party
slot** (`0x021f4628`: `slot = pass − 1`) whose member

- exists and is in the battle's mask (`0x0200ff2c`, `0x020a35f0`),
- is not held by `func_02010088` (`0x021f4664`): bit 0 of the live record's `+0xA8`, the same
  bit that freezes the battle counter (`PartyMember_AddBattleCounter`, `0x02053de0`, increments
  only when it is clear). Its meaning is **[unknown]** (`docs/re/party.md`). This port evaluates
  neither that bit nor an HP test: a member that took the bit mid-battle but kept a half counter
  would take an extra pass here where the original skips it — the one known divergence, dormant
  while nothing sets `PartyMember::item_finder`,
- holds at least **half** the battle's counter (`func_02053e0c ÷ ctrl+0x58cc ≥ 0.5`,
  `0x021f4674`–`0x021f4690`, a float compare),
- and carries bit `0xA4` of the live record's flag block (`func_02083b00(live, 0xA4)`, EU
  `0x02083b10`: byte `live+0x900` bit 4, `0x021f4694`).

That member's level (`func_0202053c`, EU `0x0202054c`, `0x021f46ac`) scales the pass: the rare
and ordinary `1/N` becomes `1/(N × 100 ÷ level)` (`0x021f49c4`–`0x021f49d0`,
`0x021f4a8c`–`0x021f4a98`), so its odds are `level ÷ 100` of the leader's. A step byte of 0
("one in one") is forced to 0 on an extra pass (`moveq` after the division, `0x021f49e0` and
`0x021f4aa8`), so a guaranteed drop is **never granted twice**; a "never" step stays 0 too.
**A second member's flag does add a pass**: the batch's premise that only the leader's matters is
refuted by this code. The pass order is the leader over every kind first, then each eligible slot
over every kind again, rare before ordinary inside each, so a kind can drop twice when its step
is not a guaranteed one. Our `Battle::finish` walks `0..=PARTY_MAX` that way and takes the slot's
level from `PartyMember::item_finder`; nothing in `dq9-app` sets that field yet (what grants the
flag is **[unknown]**). Tests: `an_extra_pass_never_replays_a_certain_drop`,
`an_extra_pass_needs_half_the_counter_and_rolls_n_times_one_hundred_over_the_level`.

**The battle's counter** (`PartyMember_AddBattleCounter`, `0x02053de0`; its only call site is
`ov_026::021dacb0`): one increment per command pass, for every slot of the battle's mask whose
actor exists — dead or standing, the call site tests no HP — unless bit 0 of the member's
`live + 0xA8` holds it. [`Battle::passes`] keeps it the same way (one increment per round for
every slot the battle holds), so a party present from the start has equal counts and the counter
cancels in the share; it separates only a late joiner and a held member (0 experience). The
facts, the disassembly and the open **[unknown]** (what bit 0 is) are in `docs/re/party.md`
§ "Sharing the experience". The same counter gates the item-finding pass above.

**The per-hit damage handoff** (`ov_024::021ec7ac`): the action executor stores the value
`ov_024::021e6a90` returned at `sp+0x54`, the amount the action-type handler applies and shows,
with `sp+0xD2`/`sp+0xD0` the attacker's and target's actor ids. Ours is the same quantity: after
tension, resistance, block/dodge, the coin, the combo table and the cap, `resolve_action` truncates
once into `TargetOutcome::amount` (`0x021e6a90`'s `_ffix`), subtracts exactly it from the target's
HP (`hp_after`), and the round reports that `TargetOutcome` to the caller (`Event::Acted`), so the
reward and the UI read one number. Test:
`the_final_damage_of_one_hit_is_the_hp_the_target_loses_and_the_report_carries`.

**The application's award** (`crates/dq9-app/src/battle/fight.rs`, `closing`): `Fight` passes
`Party::award_with` the battle's counter and `itembtlprm.nat`, so the ×1.05 shoes bonus
(`func_0208539c`) reaches the game; a single member still takes the whole total.

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
action }`. The item comes from the list the Items menu built — the acting member's **held
items**, then their worn equipment (below) — never the party bag: the battle's item command
carries the list index, and `ResolveAction` (`ov_024::021eb5d0`, EU
`0x021ed724..0x021ed7e4`) maps the actor's command entry (`battle + 0x3760`, through
`ov_000::02160130`/`ov_000::02160f08`) to that index and calls the "use an item" helper
`func_020ddb38` (EU `0x020ddb48`), which clears a held slot for a tool through `func_0208386c`
(EU `0x0208387c`). The battle command UI builds its item list from the member's item entries
(`func_ov000_0217ae90` case 4 → `func_ov000_02171c04`), never from the bag. A public rule
states the same for the series: items in the bag cannot be used during battle (DQ Wiki,
*Bag*, <https://dragonquest-wiki.com/Bag>: "Items in the bag cannot be used during
battle"). `dq9-app` therefore lists `Character::held_items` then the worn records, keeps the
held row's slot (duplicates included), and spends exactly that slot when the engine reports
`ItemUsed` **and** the use changed something (below); a worn row spends nothing.

**Which uses spend the item.** Two tests, in this order.

1. The item record's `+0x08` word, bit 19, is the "used up" flag: the AI reads it to score an
   item that is consumed (`func_ov024_021f7478`, `(word & 0xfffff) >> 0x13`), and the helper
   clears the held slot only when it is set. It is set exactly on the consumable tools and on
   the prayer ring (18023, an accessory that breaks when used). Measured on the EU tables over
   all 1,178 records (`build/evidence/j4-battle-held-items/item-flags.txt`, `755490182234`):
   46 of 234 tools and 1 of 52 accessories set it; every weapon, shield, helm, armour, glove,
   legwear and other accessory leaves it clear, including the weapons and shields with a
   battle action (cautery sword 20010, brain drainer 21010) and the reusable tools (sage's
   stone 22035, mimic stone 22125, blarney stone 22126). Consumables such as the medicinal
   herb (22000) set it. The field item use (`func_020ddb38`, category 8–10) clears the chosen
   held slot the same way.
2. The use changed something. `ResolveAction` gates the helper on its applied counter
   (`context + 0x14`) being at least 1, and each kind handler raises that counter only on a
   real change: kind 1 damage (`ov_024::021da9e0`) on a blow of at least 1; kind 2 heal
   (`ov_024::021db3a4`, EU `0x021db4e4..0x021db514`) on HP actually restored, or on a rider
   result when the restored amount is 0 (riders are not modelled); kind 7 cure
   (`ov_024::021dbc64`) only when a status was actually cured; kind 18 revive
   (`ov_024::021dd278`) only on a fallen target. So a herb on an ally at full HP, a revive on
   a living ally and an antidote with nothing to cure all **keep** the item; the same herb on
   a wounded ally spends it; a multi-target use spends it when at least one target changed.
   `dq9_core::battle::item::effect_applied` reads the same condition from the outcome's HP and
   MP totals. Kinds that move only a status or restore MP (4, 5, 6, 19, 26) are not visible
   there, but the resolver refuses them; among the 47 used-up EU records, 16 carry kind 2
   (healing) and none carries kind 1 or 57, the only other kinds the resolver applies, so the
   predicate covers every reachable item.

Evidence: `build/evidence/j4-item-spend/ghidra-consumer.txt` (`d9721f00c2de`),
`ghidra-kind-handlers.txt` (`1733a78059a5`) and `ghidra-resolve-tail.txt` (`bc31e380cfec`);
`route-before-*`/`route-after-*` checkpoints are byte-identical (`8ce4af30e497`); permanent
tests `crates/dq9-core/tests/battle_item.rs` and the app's held-herb tests. The field notes
that used to call `func_ov017_021d0164` the battle consumer were wrong: that is an event-script
native (ov_017 native table entry at `0x021d823c`) that clears a held item from a script, not
the battle path.

#### Using a worn item

The Items list is not held tools only. `func_ov000_0217ae90`'s Items row (case 4) calls
`func_ov000_02171c04`: it counts the list's leading tool entries (`menu+0x3f0`, records whose
category nibble is above 7 up to the first that is not), clears the rest, and appends every
worn equipment record of the acting member. The eight live equipment slots are read in the order
of the 8-byte table the two list helpers point at (`ov_000::02183380`, `08 09 07 00 05 01 06
0a`); for each, a non-null record with a positive id and a category below 8 is appended through
`func_ov000_02171b68`. So the Items menu is **the held tools first, then the worn entries**, and
`func_ov000_02171d90` (confirm, by item id) searches the worn slots first and returns that record;
only when the id is not worn does it fall through to the held/bag lookup. The action is then the
record's `+0x16` battle action, exactly as for a held tool (`func_ov000_02171ffc` after the
`+0x1c == 4` test). The Items cursor's confirm (`func_ov000_02171e60`) looks that number up in the
battle's action table (`func_020797dc` then `func_02079e2c`) and refuses the row when the lookup
returns 0 (it moves to state `0xe` with message 5). 1,103 of the 1,178 records carry **252** in
`+0x16`, the app's `NO_ACTION` (every worn piece of the new-game set included): a use that does
nothing. Whether the original's table (the object at `0x02108e10`, constructed by
`__sinit_020e60f8`) registers the empty action 252 is **[unknown]**; if it does, the target-mode
helper (`func_ov000_02171210`: its `+0x08` bits 8–9 are 2 and `+0x14`'s high nibble 1) sends the
row to the monster target list and resolves a kind-0 action, i.e. a turn spent for nothing
(**[hypothesis]**). The app lists only a worn row whose action the kernel models, as it already
lists only a held item that names one; the no-op rows are omitted. The drawing distinguishes the
two halves only by a layout tag, `<W=3>` for an equipment row and `<W=8>` for a tool
(`func_ov000_0217979c`, `func_02041acc`) and an empty prefix (`str_btl` 19, `<E>`); the names are
not decorated.

The consumer never takes a worn entry. `func_020ddb38` (EU `0x020ddb48`) first tests the
record's used-up bit (`func_020ddb20`, EU `0x020ddb30`, `+0x08` bit 19) and then clears a held
slot only for a category 8–10 record (`func_0208386c`, EU `0x0208387c`, slots 0–7). Equipment
(0–7) falls through with no removal, so a worn item used from Items stays equipped. Its
`param_2` is the **list index** (`ResolveAction` reads it at `ov_024::021ed7d8`), which is the
held slot for a tool only because the tools lead the list. The one exception is the prayer ring
(18023, EU constant at `0x020ddcc8`), the only equipment record with the used-up bit: the
equipment branch draws `NextRandomMax(100)` and, below 25, clears live equipment slot 10 (the
accessory, `func_02083738` case 7) — a break with a 25 % chance, not a held-slot removal. Its
action 397 is kind 26, which the resolver refuses, so the app never reaches that draw.
`dq9_core::battle::item::clears_held_slot` is the category test; the app lists the held rows
first, then a worn record whose action the kernel models, and spends a held slot only.

Measured on the EU tables over all 1,178 records: 75 carry a battle action — 39 equipment
(27 weapons, 11 shields and the prayer ring) and 36 tools; the other 1,103 carry 252. The
cautery sword (20010) is action 369, a kind-1 blow; the celestial costume (13007) carries 252.
Evidence: `build/evidence/j4-battle-worn-items/item-actions.txt` (`3162cbffaa58`); the
decompilations `menu-builder.txt` (`a7b2629fa4df`), `menu-append.txt` (`cf67d88ffdb0`),
`menu-helpers.txt` (`09522571937d`), `item-select.txt` (`0fee1cd4ce5b`),
`item-list-view.txt` (`9f4166bc5450`), `consumer-held.txt` (`7ea2a57f3355`) and
`consumer-helpers2.txt` (`eaf1a2f59d9b`); `route-before-*`/`route-after-*` checkpoints are
byte-identical (`8ce4af30e497`); permanent tests
`crates/dq9-core/tests/battle_item_rom.rs` and the app's two worn-item tests.

A member who falls before its turn produces no `ItemUsed`: the reservation is dropped at the
next command phase and the held slot stays. Cancelling or reselecting a command releases the
reservation likewise. The population path is now read (`func_ov000_02171c04`, above); the bag is
never read or changed by the app's battle item path. **[unknown]** why the confirmation singles
out action 522 (Electro Light), which no item record carries.

### Accuracy with statuses

`RollAccuracy` (`func_ov000_02156648`) draws `NextRandomMax(100)` first whatever the accuracy;
then a party member's trait die of four (refused); then, for a scaling action, a monster's
`+0x14` bits 0–6 times the target's resistance to the landing element `+ 0.5f`, truncated; then
the dazzle die of eight (only while dazzled); it lands under the accuracy. A party member's
scaling accuracy (a stat-scaled or drawn value) stays `Unsupported::AccuracyScaling`.

### Statuses' run-down

`func_ov000_021599f4` takes one off every count held on each of its holder's passes; at 0 the
second count starts at four, a literal of that function (`mov r1, #4`) for defence, agility and
dazzle. The wear-off `func_ov000_0215858c` runs just before it on the same pass: after the tick's
mandatory `NextRandomMax(100)`, each status whose second count is running takes one off it, draws
`NextRandomMax(100)` and is cleared where the chance by the **decremented** count is above the
draw. Two tables, each four floats, reached through a pointer and indexed 0 to 3 (the first of
the four rolls reads index 3, the last index 0):

| statuses | pointer | table | rolls |
|---|---|---|---|
| defence, agility | `0x02158d68` | `0x02182ad4` | 62.5 %, 75 %, 87.5 %, 100 % |
| dazzle | `0x02158d6c` | `0x02182bd4` | 37.5 %, 62.5 %, 87.5 %, 100 % |

Both tables read on the European ROM (`build/evidence/j4-audit-fixes/rom-wear-off-tables.txt`,
`cdc2df1cee2d`): the pointer array sits in overlay 000's image, the floats in its data, and the
disassembly's `ldr r2, [pc, #…]` operands name the two pointers. Dazzle's values are the page's
38/63/88/100 once rounded, so the two sources agree. A defence level therefore holds six of its
holder's passes; the model's counts and second counts are in `Status` (`WEAR_OFF_CHANCE` and
`DAZZLE_WEAR_OFF_CHANCE`); the regression test is
`wear_off_rolls_by_the_decremented_count_and_dazzle_has_its_own_table`.

**Correction, 2026-11-17** (EU decompilation of `0x0215858c`,
`build/evidence/j4-encyclopedia-audit/ghidra-status-tick.txt`, `8078738699e2`): the routine reads
**two** chance arrays, `0x02158d68` for defence and agility and `0x02158d6c` (four bytes later) for
dazzle, and it decrements the second count *before* indexing it. With a second count of 4 the four
rolls are therefore `T[3]`, `T[2]`, `T[1]`, `T[0]` — `0.625, 0.75, 0.875, 1.0` if `T` is read
ascending by remaining count (the page's 63/75/88/100), and dazzle's own array gives
37.5/62.5/87.5/100 (the page's 38/63/88/100). Until `j4-audit-fixes` (`ed0974b`) the model had
one array keyed one index late (rolls 75/88/100/100, dazzle reusing the defence table); it now
holds `WEAR_OFF_CHANCE` and `DAZZLE_WEAR_OFF_CHANCE`, both read on the ROM (above), indexed by the
decremented count.

## The transition's phases (batch `v4-battle-presentation`, read)

The swirl task's states and the delay, re-read on the European ROM to turn the measured frames
of [`battle-transition.md`](battle-transition.md) into explicit ticks (`battle/entry.rs`):

- `SwirlTask_Init` (`ov_017::021b621c`) writes `+0xfd` = 15 and `+0xfd` stays 0 when native 547
  creates the task with an object; `SwirlTask_Update` (`ov_017::021b6290`) **state 0** zeroes it
  when the battle number `+0xfe` is below 1 (a roamer's contact: `+0xfe` = `-1`), **state 2**
  starts the battle track and the swirl only once the delay is spent. The task is updated once
  per two frames (`GetTickCount` returns 2: the game's 30 Hz loop), so 15 updates = **30 fixed
  steps**; the delay therefore belongs to a set battle (`Request::Battle`, number ≥ 1) and no
  roamer, and it runs while the chr loads (state 1), so it is a wait, not a black.
- **state 2** starts the swirl when the main brightness is not already full black
  (`GetBrightness` 0 is `-0x10`), else it calls `SetMainBrightness(-16, 0)` and
  `SetSubBrightness(-16, 15)` and goes to **state 4** (the camera restore); **state 3** runs
  `SwirlEffect_Update` (`0x0204701c`) each update; **state 4** waits
  `IsBrightnessTransitionActive` and then restores the camera and ends the task.
- The **12-frame freeze** and the two load blacks are measured, not read: the freeze is the
  battle task's blocking zone load (`ov_017::021b790c` state 0 runs `func_02015438` in a loop
  while the zone's lock is held) **[hypothesis]**; the blacks are the stage's and the field's
  load times. The port spends them as ticks (`battle/entry.rs`: 12, 94, 95) because on the
  original the picture does not move during them. The scene's fade-in is the one count that is
  not exact: the brightness log changes over frames 149-161 (~13 frames, 7 levels at the 30 Hz
  pairing) while the port spends `SCENE_FADE_STEPS` = 15 fixed steps; the brightness call's
  count (`0x021d9d18`, not a function in the corpus) was not read.

## The row formation (`ov_000::02167dd8`/`02167e6c`, read)

Batch `v4-battle-presentation`. The battle's set-up (`ov_000::02164d74`) computes both sets of
places and then applies the grid: `ov_000::021675a0` (party row), `ov_000::021677fc` (monster
row), `ov_000::02167cd4` (grid places from the slot table at `iRam02167dd0`), and
`ov_000::02167dd8` writes the low two bits of `+0x5951` to 2. `ov_000::02167e6c` writes them to
1 (the row) and re-applies each combatant's row place. The port's `Formation::Row` is that
layout; the tables, read on the EU ROM (`build/evidence/v4-battle-presentation/row-tables.txt`):

| data | address (EU) | fact |
|---|---|---|
| party row turns | `0x02183158` (reached through `iRam0216769c` = `0x02183158`), 16 bytes per party size | one hero `0x323d`; two `0x335c`, `0x311e`; three `0x3570`, `0x323d`, `0x2f0a`; four `0x3570`, `0x335c`, `0x311e`, `0x2f0a` (fx12 radians) |
| party row place | `0x021830e4` (y `0xcc`, z `0x2800`), `0x021830b4` (rotation, y `0x3244`) | y 0.05, z +2.5; the heading is the table's, not a π plus a turn |
| row gap | `ov_000::02167b5c`, literals `0x02167bac` = `0x199`, `0x02167bb0` = `0xb33` | gap = clamp((0x4000 + (n−1)·0x199 − total)/(n−1), 0x199, 0xb33) |
| narrowing | `ov_000::021681f8`, literals `0x02168228` = `0x155`, `0x0216822c` = `0xb33` | kinds `0xbd`, `0xbe`, `0xbf`, `0x110` and `0x155` get `0xb33`/0x1000 = 0.7 of their width, and only when more than one monster fights; every other kind keeps `0x1000` |
| widths | `ov_000::021677fc` | each width is `(radius · factor + 0x800) >> 12` (rounded fx12); the x walk adds half a width, places, then adds the other half plus the gap; the row is centred on `-(total + gap·(n−1))/2` |

Facts: `func_ov000_021675a0` walks the party **from +x down** (`iVar3` starts at
`(count−1)·0x1800/2` and loses `0x1800` per member) while it reads the turn table forward, so
member 0 (the hero) is the **+x** end and takes the row's first turn; with heading 0 facing +z,
`0x3570` (π + 0x333/4096) at the +x end faces back toward the middle and `0x2f0a` (π − 0x333) at
the −x end likewise: **both ends turn inward** (pinned by
`battle::tests::the_row_party_turns_by_the_roms_table`, which reads the headings' sine against
the pairing). The one-hero turn is `0x323d`/4096 = 3.13989 rad,
0.0017 rad below a float π (the ROM's own approximation: `0x021830b4`'s rotation y is `0x3244` =
12868, so the table's π is its own).
The ends of a four-hero row are `0x333`/4096 = 0.19995 rad from its middle, hence the "±0.2".
The gap's target is 4.0 + one minimum gap per interval, which the old **[hypothesis]** had right;
what it lacked was the five-kind narrowing and the integer fx12 rounding (a width of 1.0 becomes
2867/4096 with the narrow factor, half of it 1433/4096: the row is asymmetric by 1/4096 per
member, as the original's).

**[unknown]**: which scripts call `ov_000::02167e6c` (none in the corpus's `calls.txt`); whether a
call for backup switches the flag to the row (`ov_025::021d8c30` recomputes the row's places
through `0x021677fc` when a monster joins, but the flag write was not found); the reinforcement
table at `+0x81b1` that `0x021677fc` reads before the placement (it builds the monster list from
the `.bact` groups); and `func_ov000_0215fc60`, which forces a one-member party row. The port's
monster row has no eight-monster cap (`0x021677fc` walks eight slots): the ROM's records ask at
most eight, so a ninth is unmodelled.

## Open points

- **[hypothesis]** the two draws BattleEmulator spends before every action are `func_ov000_0215fbe0`
  (`NextRandomBetween(3, 4)` then `NextRandomBetween(6, 8)`, drawn on **every** call whatever the hit
  code), which `ResolveAction`'s head calls once target settling is done (`func_ov000_0215440c` for a
  monster, `func_ov000_021540fc` for a party member). The call is read in the European decompilation;
  that these are the oracle's two is not checked (the oracle models no targeting). The kernel still
  starts after them; `Battle` draws them.
- **[unknown]** what follows the wake roll in the original: the monster's provocation draws
  (`func_ov024_021eb08c`: `NextRandomMax(100)` per opposing combatant when the action's `+0x1C`
  bits 19–31 is 5 or `0xc`, or its `+0x0A` bit 0 is set — the kinds our actions carry there are
  unread), `func_ov000_0215b5a0`'s hit rise, the tail of `ResolveAction`
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
- **[unknown]** the accuracy roll's remaining statuses and traits (the party trait's die of 4).
  The metal-body early miss is modelled now ("The metal body"); what `+0x18` bits `0x800` and
  `0x40` mean — the two other early misses of `RollAccuracy`, which no chapter-2 fight sets — is
  not: `func_02089174` sets `0x800` with a counter at `+0x7c`, and who calls it is not read.
  Dazzle's die of 8 and a monster's scaling accuracy are modelled.
- **[hypothesis]** Defend's absence of a turn record and of draws: the stance is taken up as the
  round begins (the wiki's reading of the command), but the command code was not read.
- **[hypothesis]** a call for backup's new HP is drawn from the world's generator by
  `func_02089630`; the call into it was not read, only the handler's `func_0x02161380`.
- **[hypothesis]** the wear-off second count starts at 4 for defence, agility and dazzle; the
  value is a literal of `func_ov000_021599f4` (`mov r1, #4`) and the fields it writes were not
  traced further.
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
