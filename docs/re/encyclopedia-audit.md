# DQ9 Encyclopedia audit: battle and roaming

Systematic comparison of the site's "Under the hood" blocks with what `dq9-core` already does,
for the v0.2 scope (chapter 2: Angel Falls region, Mountain Pass, the Hexagon, the Hexagoon).
The four pages read on 2026-11-17 are [battle formulas](https://dq9encyclopedia.com/en/guide/battle-the-games-formulas),
[randomness](https://dq9encyclopedia.com/en/guide/the-games-randomness-for-speedrunners),
[stats and effects](https://dq9encyclopedia.com/en/guide/stats-and-effects) and
[spells and abilities](https://dq9encyclopedia.com/en/guide/spells-and-abilities)
(`external-resources.md`). The site has no license: every claim below is restated in our words,
no prose or table is copied, and a claim is **[hypothesis]** until we re-check it on the ROM or in
its function — "agrees" below means *our* code or *our* decompilation confirms it, never that the
site is a source of truth.

## Method

- **Addresses.** The pages cite USA ARM9 addresses and overlay addresses. Converted to the
  European release with `AGENTS.md`'s rule (`0x0200F3B8..0x0200FB20` → +`0x8`,
  `0x0200FB20..0x020EEBD0` → +`0x10`; overlays unchanged) and each checked as a function start
  in `re/functions.txt`. The tables below give the **EU** address.
- **Disagreements.** Where the claim and `dq9-core` disagreed, the cited EU function was
  decompiled (`scripts/ghidra-query.ps1`). Evidence, local, under
  `build/evidence/j4-encyclopedia-audit/` (sha256, first 12):

| file | sha256 | what it settles |
|---|---|---|
| `ghidra-flight.txt` | `7db44aa86f32` | `ov_000::0215f7a8`: flight's tests, and species 38–40 refusing it |
| `ghidra-company.txt` | `2e7ef7fd4150` | `0x0209afe4`: the company draw shuffles the order on land |
| `ghidra-spawn-point-test.txt` | `d95f141a2a59` | `0x02073e0c` and `0x02073d68`: the 3.5-unit spawn block |
| `ghidra-status-tick.txt` | `8078738699e2` | `ov_000::0215858c`: two separate wear-off chance arrays |
| `ghidra-final-damage.txt` | `1aa95ba7247a` | `ov_024::021e6a90`: the cap is applied last; Defend's gate |
| `ghidra-xp-share.txt` | `0e989bc184b5` | `ov_023::021f4098`: two per-member factors in the experience share |
| `ghidra-xp-helpers.txt` | `a11fb45e8245` | the helpers of the share (`0x0202053c` neighbourhood, `ov_023::021f5578`) |

- **Verdicts.** *agrees*: our code does what the claim says. *differs*: both do it, differently —
  the exact difference is stated. *not implemented*: the engine refuses the case before any draw,
  or has no representation for it. *n/a chapter 2*: unreachable in the v0.2 route.
- **Tests.** The column names the check that would catch a regression: a `dq9-core` unit test, a
  BattleEmulator oracle vector (`docs/re/battle.md`, "Differential oracle"), a real-ROM test, or
  "none".

## Battle formulas

Page: [battle-the-games-formulas]. `Ours` gives `file:line` in `crates/dq9-core/src`.

| # | Claim (our words) | Original (EU) | Ours | Verdict | Tests |
|---|---|---|---|---|---|
| B1 | A blow is `(attack − defence/2)/2`; nothing when that is not above 0; at or under `attack/16` a draw from 0 to `attack/16`; above it two draws `±that/16` then `±1`. | `0x020744d0` `CalculatePhysicalDamage` | `battle/damage.rs:22` | **agrees** (same operations, same order, `f32`; oracle-compared bit for bit) | `CalculatePhysicalDamage` oracle vectors (160) |
| B2 | Monster HP is 80–100 % of the record in a random encounter, exact in a scripted one (except reinforcements). | `0x02089640` `InitMonsterBattleStatus` | `battle/monster.rs:15`, `battle/setup.rs:355` (a set battle passes no world generator) | **agrees**; a reinforcement draws like an encounter (`battle/round.rs:1062`) | `monster_hp_comes_from_the_worlds_generator_below_the_table` |
| B3 | One action draws in a fixed order: per target a die of 100, the critical roll (once for the whole action when it reaches all or a group), evasion, block if not dodged, accuracy, the amount, the final damage; a landed damaging blow then draws the wake roll. | `ov_024::021eb5d0` `ResolveAction` | `battle/resolve.rs:211` | **agrees** (plus the two `func_ov000_0215fbe0` draws before it, drawn by `Battle` — `battle/ai.rs:405`) | 306 BattleEmulator action vectors; `a_blow_draws_die_critical_dodge_block_accuracy_then_the_amount` |
| B4 | A party critical's rate is `(1/hits) × action chance × (2 + (deftness − 150)/100 above 150 + gear + book) %`; spells use the same roll with an action chance of 50. | `0x020747b4` `CalculateCritRate` | `battle/damage.rs:45`, `:75`; the action chance is `+0x14` bits 21–27 (`battle/setup.rs:193`) | **agrees** (deftness held at 0 below 150, the `short` conversion kept) | `CalculateCritRate` for every deftness 0..=999, two skill values; `critical_threshold_follows_the_float_rounding_of_the_game` |
| B5 | A monster never rolls a critical: its "critical" is a separate action. | `0x02074908` (literal zero rate) | `battle/damage.rs:75` | **agrees** (the draw is still spent) | `an_always_critical_action_draws_no_critical_roll_and_a_monster_never_criticals` |
| B6 | A critical takes the greater of 1.2 × damage and — for the plain Attack — attack × `random(0.95;1.05)`; a critical spell is damage × `random(1.5;2)`. | `ov_024::021e6a90` | `battle/resolve.rs:148` (action 1, `0xDB`, `0x1F9` take the attack power) | **agrees** | `a_plain_attack_critical_is_the_attack_power_not_the_damage_times_up_to_two`; critical vectors |
| B7 | Every action carries a value cap in its record (999, 1999, 2999, 4017, 9999…), and the page lists it as the second step of the multiplication order. | `ov_024::021e6a90`, `+0x1C` bits 0–13 | `battle/model.rs:255` (`max_damage`), applied at `battle/resolve.rs:195` **after** critical, resistance, coin, guard and combo | **agrees** on the cap; the page's order is misleading — the function truncates once and *then* clamps (`ghidra-final-damage.txt`, the `& 0x3fff` clamp after `_ffix`), so the cap is last | `the_cap_holds_damage_and_a_heal_is_held_to_the_maximum` |
| B8 | The target's resistance multiplies the damage (`× GetResistance(element)`). | `ov_024::021e6a90` | `battle/resolve.rs:176` | **agrees**; a resistance stays a float to the single truncation (site implies rounding down only) | `a_resistance_stays_a_float_until_the_end_and_the_coin_needs_a_resistance`; Woosh vectors with resistance 75 |
| B9 | A weapon's family bonus multiplies the damage (×1.1, ×1.2 for the dedicated weapons), on the Attack and on weapon techniques. | none cited | absent | **not implemented** (no family field anywhere) | none |
| B10 | Defending halves the damage of blows; Blockenspiel ×0.5, Defending Champion ×0.1. | `ov_024::021e6a90`, guard table | `battle/resolve.rs:182` with `GUARD_MULTIPLIERS` `:33`; the level is set by Defend (`battle/round.rs:585`) and cleared at the round's end (`:673`) | **agrees**, with the page's wording imprecise: the halving gates on the action's `+0x10` bit 4 (`ghidra-final-damage.txt`), not on every blow | `defend_guards_for_the_round_and_an_item_is_reported`; `flight_follows_the_original_order_of_tests` |
| B11 | Several-target actions lose damage by rank: ×1, ×0.8, ×0.6, ×0.4, ×0.2, for twelve enumerated actions. | none cited | `battle/model.rs:281` reads the flag; `battle/model.rs:470` refuses several targets; one target is 1.0 (**[hypothesis]**) | **not implemented** (refused by `Unsupported::MultiTargetFalloff`) | none |
| B12 | Tension multiplies by level (party 1.5/2.5/4/6, monsters 1.3/2/3/4.5) and adds a flat bonus by level; it is spent by the next action that uses it. | `TensionBonusCalculation` (decompilation) | `battle/model.rs:94` refuses a non-zero level (`Unsupported::Tension`) | **not implemented** | none |
| B13 | At tension 100 the fighter is immune to sleep, paralysis, confusion, poison and lost turns (except two abilities). | none cited | absent (no status at all) | **not implemented** | none |
| B14 | An action without the metal flag always misses a metal monster; the others deal 0 or 1 except a critical, Quadraslash and Needle Shot, Metal Slash and Metalicker (1 + `random(2)`), a "+1 against metal" weapon, the poison needle. Damaging metal is set to 0 except on a critical and actions 517/130 (`0x205`/`0x82`). | `ov_024::021e6a90` for the damage side; `ov_000::02156648` for the miss | `battle/resolve.rs` (`roll_accuracy`'s early miss, `final_amount`'s zeroing, coin, `1 + random(2)`, `+1`, needle); `battle/model.rs` (`Action::works_on_metal` = `+0x10` bit 24, `Action::aims_at_opponents` = `+0x08` bits 8–9, `Fighter::metal_bonus`, `Fighter::poison_needle`) | **agrees**, read on the EU code (`build/evidence/v4-metal-rules/accuracy.c`, `final-damage.c`, hashes in `manifest.json`). Both lists are the code's: the metal flag is `+0x10` bit 24 (the formats crate's `metal` is the aim, `+0x08` bits 8–9), the zeroing spares a critical and `0x205`/`0x82`, and Slash/Metalicker add `1 + random(2)`. A plain Attack on a metal body is 0 or 1 (the coin after the zeroing), which is what the page says and the decompiler's `0x47 < number` artefact contradicted (assembly: `final-damage.asm`) | `a_blow_that_does_not_work_on_metal_misses_the_metal_body_before_the_accuracy_die`, `a_blow_that_works_on_metal_is_zeroed_and_the_coin_leaves_zero_or_one`, `a_critical_is_not_zeroed_against_a_metal_body`, `quadraslash_and_needle_shot_are_spared_and_metal_slash_deals_one_or_two`, `the_metal_zeroing_needs_an_action_that_aims_at_the_opponents`, `tests/battle_rom.rs::the_metal_body_rules_read_the_rom_records` |
| B15 | A buff or debuff holds two levels; defence and agility are ×1.5 then ×2 up, ×0.5 then ×0.25 down; attack is ×1.25 a level. | none cited | `battle/round.rs:267` (`level_multiplier`), applied by `:989` (`apply_kind`, kinds 4 and 5) | **agrees** for defence and agility; attack (Oomph/Blunt, kind 3) is **not implemented** | `Buff moved the defence level` / `Accelerate the agility level` (real-ROM chapter-2 test) |
| B16 | A defence or agility buff lasts 6 turns, then wears off with 63 %, 75 %, 88 %, 100 % on each following turn. | `ov_000::0215858c` | `battle/round.rs` (`WEAR_OFF_CHANCE`, `wear_off_chance`), called after the count reaches 0 (a second count of 4 is a literal of `func_ov000_021599f4`) | **agrees** since `j4-audit-fixes` (`ed0974b`): the table is indexed by the decremented second count, so the four rolls are 62.5/75/87.5/100 % — the page's 63/75/88/100 rounded | `wear_off_rolls_by_the_decremented_count_and_dazzle_has_its_own_table` |
| B17 | A dazzle lasts 4 turns, then wears off with 38 %, 63 %, 88 %, 100 %. | `ov_000::0215858c` | `battle/round.rs` (`DAZZLE_WEAR_OFF_CHANCE`, `wear_off_chance`), count 4 | **agrees** since `j4-audit-fixes` (`ed0974b`): the ROM's dazzle array (`0x02158d6c` → `0x02182bd4`) holds 37.5/62.5/87.5/100 %, the page's 38/63/88/100 rounded (`build/evidence/j4-audit-fixes/rom-wear-off-tables.txt`, `cdc2df1cee2d`) | `wear_off_rolls_by_the_decremented_count_and_dazzle_has_its_own_table` |
| B18 | Recursing a buff already at two levels does nothing and does not restart its duration; below two it gains a level and restarts the duration. | `0x0208777c`, `0x020877d0` (the page's ARM9 addresses) | `battle/round.rs:1002`–`:1055`: the level check gates the whole write, and every move sets the count back to 6 | **agrees** | none (real-ROM: the chapter-2 Buff casts) |
| B19 | A party member evades 2 % plus gear and skills; a monster's evasion is 0, 2, 4, 8 or 25 % from its record. | `RollEvasion` `ov_000::02156f98`; grade table | `party.rs:504` (`2.0` + item `+0x0c` bits 10–19 / 10), `battle/model.rs:30` (`GRADE_RATES`) | **agrees** (the roll truncates the rate) | `a_dodged_blow_skips_the_block_roll_but_still_draws_its_amount`; 18 dodged vectors |
| B20 | Block comes from the shield; a monster's block is 0 to 25 % from its record. | `RollBlock` `ov_000::02156e30` | `party.rs:506` (zero with no shield, then the item sum), `battle/model.rs:30` | **agrees** (this roll does *not* truncate the rate) | `a_blocked_blow_deals_nothing_and_a_block_rate_is_not_truncated` |
| B21 | Flight is impossible in a scripted battle; it always works on a surprise first round, when no monster is left, or when the party's mean (attack + defence) is at least three times the monsters'; otherwise the chance is the greater of 25/50/75/100 % by attempt and `10 + best deftness/20`, drawn on the world's generator. | `ov_000::0215f7a8` | `battle/round.rs:682` (`try_flee`), refused up front for a set battle at `:544` | **agrees** (order, integer means, floor table `[25,50,75,100,65535]`, `NextRandomMax(100)` on the world generator) | `flight_follows_the_original_order_of_tests`; `fleeing_monsters_leave_over_twenty_seeds` |
| B22 | Flight is also impossible against cannibox, mimic and Pandora's box. | `ov_000::0215f7a8` | absent | **differs**: the routine returns "no" when any monster of the battle is species 38, 39 or 40 (`ghidra-flight.txt`: `*psVar11 - 0x26U < 3`); our engine only refuses a set battle. The species-to-name mapping is the site's **[hypothesis]**; species 38–40 are the same three our encounter code never lets surprise the party (`encounter.rs:33`) | none |
| B23 | A single-target pick weights a front-line member 2 and a back-line member 1, by `NextRandomMax(total) + 1` walked down, with a second even draw if the walk ends unspent. | `ov_000::02154f30` | `battle/ai.rs:376` (`weighted_pick`) | **agrees** | `chapter_two_ways_play_to_the_end_over_twenty_seeds`; `a_surprise_round_leaves_the_surprised_side_out_with_the_original_draws` |
| B24 | A grudge monster adds +2 for the last member who struck it and +1 for the one before; 263 of 438 records carry the bit. | `ov_000::02154f30`; the writer is `0x0208979c` (EU; USA `0x0208978C`), called from `ov_024::021eb5d0`'s target pass (EU `0x021ed0d4`) | `battle/ai.rs` (`target_weights`, `weighted_pick`), `battle/round.rs` (`last_attackers`, written in `resolve`) | **agrees / implemented**; the ROM counts **263 of 438** records with `+0x10` bit 26, so the page's number is a fact (`battle_rom.rs`, `the_grudge_bit_and_the_enrage_triggers_count_on_the_rom`) | `a_remembering_monster_weighs_its_last_two_attackers`; `a_party_of_two_plays_against_a_remembering_monster`; none on the chapter-2 pilot (one hero) |
| B25 | Invisibility halves a member's weight, the draw still uses the pre-cut total, and whatever falls past it goes to an even draw including that member. | none cited | `battle/ai.rs:398` has the remainder draw; no invisibility | **not implemented** (no status; the remainder draw is already there) | none |
| B26 | A dazzled striker throws a die of eight on an action spoilt by a status and misses under 5; the die is drawn only while dazzled. | `ov_000::02156648` `RollAccuracy` | `battle/resolve.rs:127` | **agrees** | `a_dazzled_striker_throws_the_die_of_eight_and_misses_under_five` |
| B27 | Status durations are counted on each **action** of the holder, so a monster acting twice uses two turns of an effect. | `ov_000::0215858c` | `battle/round.rs:905` (`resolve` ticks the actor after every action, extra actions included) | **agrees** | `a_once_a_battle_way_is_spent_for_the_whole_group` (two actions per round) |
| B28 | A blow wakes a sleeping target (a monster only half the time). | `ov_000::02157288` (the wake roll at `0x02157340`) | `battle/resolve.rs:246` draws the wake roll on a landed damaging blow (`wakes_target`) | **agrees** on the draw; no sleep state exists to be cleared | vectors of the Attack, whose record carries the bit |
| B29 | 287 monsters enrage on a trigger (HP below half or a quarter, Whistle, Eyes on Me, a healing spell), for 1–9 actions, and then only target one member; critical hits and resurrections anger nobody. | `ov_024::021eb08c` (and five callers); the fields are `+0x24` (two kind/chance pairs) and `+0x28`/`+0x29` (the length range) | absent; `docs/re/battle.md`'s "Enrage" names the fields and the codes read at the call sites | **not implemented, no chapter-2 divergence**: the trigger word and codes are read, and the ROM counts **341 of 438** records with an armed pair (not the page's 287); **the Hexagoon (300) has kinds 0 and 0** (`0x1000_0000` is only the boss bit), so no chapter-2 fight enrages and the target lock `+0x2e` is not needed | `battle_rom.rs` counts both; a heal in a fight with an armed monster would draw in the original where the kernel does not (**[unknown]** for our actions' `+0x1C` bits 19–31) |
| B30 | Charm enthralment: each living member may enthral an acting monster, `(charm − 100)/50 % × sensitivity`, and a monster at 0 charm is never enthralled. | none cited | absent; `Battle::round`'s comment assumes the charm byte `+0x53` is 0 (`docs/re/battle.md`) | **not implemented**, and consistent with the existing assumption only while no member's charm exceeds 100 | none |
| B31 | Experience is shared as `XP × (K + level) × turns spent alive ÷ Σ`, rounded up, with K = 4 up to 10 000, 3 up to 20 000, 2 above; a partner in multiplayer adds 10 % a player. | `ov_023::021f4098` | `party.rs:686` (`award`), `party.rs:57` (`EXP_BANDS`) | **differs**: our share is `XP × (K + level) / Σ(K + level)` over the members left standing — no per-member "turns alive" factor, and no multiplayer bonus. The decompiled function multiplies a **second** per-member float into each term and divides by the weighted sum (`ghidra-xp-share.txt`); that float's source (`0x02053dfc`, a counter bumped when a status bit is clear — `ghidra-xp-helpers.txt`) is **[hypothesis]** "turns alive" | none | **Update 2026-11-17 (`j4-xp-wire`): refuted** — the counter counts command passes of every present slot, not turns alive; it is uniform for a party present from the start and cancels in the share.
| B32 | Monster letters: several monsters of a kind get A…Q, a newcomer takes the next letter, a lone one has none. | the letter table `0x020F015C` (data, identical in both regions), `ov_000::0215ed8c` (the opening), `ov_000::02161380` (a reinforcement) | absent (battle UI does not name monsters) | **not implemented** (presentation only) | none |
| B33 | Coups de grâce and co-op moves exist (vocation level 10, a glow, a damage-share table). | several | absent | **n/a chapter 2**: the route's party is level 4–8 at the Hexagoon (`docs/status.md`), so no vocation reaches 10 on the v0.2 path | none |
| B34 | Monster attacks that depend only on a hidden level exist (2.5 × level, 3 × level, 30 + 2 × level), each ±10 %. | none cited | absent (those actions are range records or refused) | **n/a chapter 2** (no chapter-2 way uses them — `docs/re/battle.md`, "Chapter 2 coverage") | none |

## Randomness

Page: [the-games-randomness-for-speedrunners].

| # | Claim (our words) | Original (EU) | Ours | Verdict | Tests |
|---|---|---|---|---|---|
| R1 | Three streams: A = 32-bit LCG `r × 0x41C64E6D + 0x3039`, roll `(r >> 16) & 0x7fff`; B = 64-bit LCG `s × 0x5D588B656C078965 + 0x269EC3`, the top 32 bits; C = the same generator, a separate instance reseeded at every battle. | `0x02032380` `RandBelow`; `NextRandom*` | `battle/random.rs:89` (`CRand`), `:21` (`Random`); `Battle::new` seeds the battle's instance (`battle/round.rs:344`) | **agrees**; the app seeds both from `--seed`/the world deliberately (a test driver, `docs/re/encounter.md` "Application wiring") | `c_library_rand_matches_the_published_sequence`; `generator_steps_then_draws_the_top_word` |
| R2 | An integer from 0 to n−1 is `min(n − 1, ⌊n × roll ÷ 4 294 967 295⌋)`, one roll. | `0x0207430c` `NextRandomMax` | `battle/random.rs:55`: `(int)((float)n × float01)`, held below n | **differs from the page, ours follows the ROM**: the ROM widens to `f32`, multiplies and truncates (`_fflt`, `_fmul`, `_ffix`; `docs/re/battle.md`, "Generators", checked on the EU ROM). The two forms disagree about once in 4 000 draws at n = 10 000; the 306 oracle vectors pin ours | `largest_draw_is_exactly_one_and_next_max_holds_below_the_maximum`; `next_max_of_nothing_draws_nothing`; oracle `max` rows |
| R3 | Spawn: after a 1 s timer, the free nodes linked to the party's node (plus that node itself when it is more than 5 units away); the node most in front of the gaze is taken with no roll; when it is more than 10 units away the point is brought onto the path 10 units from the hero; facing a node whose path stays 10 units away freezes the table (a "stop spot") — no roll at all. | `0x020733e8` (the tick), `0x02073dfc` (the point test) | `encounter.rs:281` (`spawn`), `:920` (`spawn_candidates`, the 5.0 test in 3-D at `:935`), `:948` (`plan_spawn`, the 10.0 path and the `0x9fff` give-up), `:963` (dot product) | **agrees**; the plotted evidence is the roamer oracle (≈1 250 spawns, 0 violations, `docs/re/encounter.md` "Claims checked" (b)) | `encounter_rom` (four runs, 11 tests) |
| R4 | A party member within 3.5 units of the chosen node blocks the spawn, and that frame rolls nothing. | `0x02073e0c` | `encounter.rs:327` tests the **leader alone** against the spawn point (`< 0x3800`), before the zone and species draws | **differs**: the function loops over all four members (`GetPartyMemberByIndex`), each compared with the spawn **point** and only when the point's field is the member's (`ghidra-spawn-point-test.txt`). Our model is exact for a one-member party, wrong for any other. The page's wording ("of the chosen node") is also imprecise — it is the point, node or moved point | none (the oracle party is the hero at level 91, so the second member never ran) |
| R5 | Nothing spawns while 12 monsters are on the map; a species that cannot spawn still burns its roll each frame, and the timer only resets on an actual spawn. | `0x020733e8` | `encounter.rs:28` (`SLOTS` 12), `:291` (timer), `:355` (zone wait, the timer kept on failure), `:384` (reset only on a spawn) | **agrees**; our model also keeps the visible-roamer box test (three or more near the party → no spawn, `:299`), which the page does not mention and the ROM has (`docs/re/encounter.md` (b) step 2) | `encounter_rom` |
| R6 | The species is rolled from the zone by weight, on table A. | `0x02073fec` | `encounter/pick.rs:71` | **agrees** (one `rand_below` draw over bits 12–14) | (a) in "Claims checked"; the monster always belongs to the drawn zone |
| R7 | The pre-emptive strike is table A: a roll of 100 under `2 + deftness/20` (the party's highest; +10 when the monster's back is turned), otherwise the monsters surprise when their roll is under 2 (+10 when they catch the party from behind); monsters 38–40 never surprise. | `ov_017::021970a0` | `encounter/pick.rs:87`; the species test at `encounter.rs:33`, used at `:819` | **agrees** (draw order, the two arms, the exception); not observed in a run — every oracle contact was with a weak monster that fled | none (the oracle never entered a fight with a surprise) |
| R8 | On land the group of the monster touched stays first, left to right; only at sea is the order shuffled. | `0x0209afe4` | `encounter/pick.rs:171` (`draw_company`): a coin for two kinds and one of six permutations for three (the table `0x020e8e3c`, read as `0,1,2 / 0,2,1 / …`) | **differs, the page is wrong**: the land routine itself draws the coin and the permutation (`ghidra-company.txt`, the `NextRandomMax(2)` swap and the six-row table copy), so the touched group is shuffled on land too. Our port is faithful to the address | `company_stays_within_its_bounds`; the company is **[hypothesis]** against the oracle (all observed fights were one monster) |
| R9 | A company's make-up is table B: the touched monster's count, the number of kinds (weights 1/2/3 by the roamer word), each other kind by weight with its count, then a shrink to the zone's party limit. | `0x0209afe4` | `encounter/pick.rs:171`; the limit from zone bits 10–12 (`:838`–`:842`) | **agrees** (the shrink subtracts `max(excess/3, 1)` a slot at a time, first slot held at 1) | `company_stays_within_its_bounds`; `encounter_rom` |
| R10 | A monster's starting HP is `(int)(HP × random(0.8;1) + 0.5)` on table B — the metal slime at 4 HP shows 3 HP about 37.5 % of the time. | `0x02089640` | `battle/monster.rs:15` (world generator, `NextRandomFloatBetween(0.8, 1.0)`, round half up) | **agrees** | `monster_hp_comes_from_the_worlds_generator_below_the_table` |
| R11 | Ordinary loot: one roll per group, `⌊N × (roll − 1) ÷ 32 767⌋ = 0`; the rare item first and the common one only if the rare did not drop; at most 8 items a battle; the Thief's Theory adds a pass per holder. | `0x02032380`, `ov_023::021f454c` | `battle/end.rs:64` (`drop_roll`), `:95` (`finish`: rare then ordinary, `MAX_DROPS` 8), `:69` (one chance per kind) | **agrees** on the formula, the order and the cap; "per kind (number in a group)" vs the page's "per group" is unobservable in chapter 2 (a group holds one number). The Thief's Theory passes are **not implemented** | `the_drop_roll_matches_the_formula_and_the_cap_is_eight`; `a_won_battle_pays_the_sums_and_rolls_one_drop_per_kind` |
| R12 | A battle has ~153 KB of monster memory; the first species that no longer fits is dropped with every species after it, and on land the touched species is placed first so it is never dropped. | none cited | absent | **not implemented**; for chapter 2 the drawn companies are all **[hypothesis]** small enough to fit (the page's largest share is 96 KB, and no chapter-2 species is near it) | none |
| R13 | A visible monster vanishes when no party member is within 25 units of it on each of the three axes. | `ov_017::021a1ad0` | `encounter.rs:400` (`release_far`, `±0x19000`) | **agrees** (the roamers in a fight stay) | `encounter_rom` (a) — a slot's next spawn follows the release |
| R14 | Only a monster walking or waiting notices the party, and not in the first 2 s of its life. | `0x020790bc` | `encounter.rs:514` (notice in states 1 and 2, awake ≥ 2 000 ms — `encounter/roamer.rs`) | **agrees** | `encounter_rom` |
| R15 | The Hoimi accounting: Heal +1, herb +2, Inspect +1, battle start +2 for one group, +5 for two, +7 for three, then +1 per monster. | none cited | the company is drawn at the contact (`encounter.rs:843`), the HP per monster at the battle's build (`battle/setup.rs:355`) | **[unknown]**: our draw counts along table B are not measured against the original, so the page's per-group numbers can neither confirm nor refute them | none |
| R16 | Grotto floors reseed table A on entry, one spawn node per walkable square, the upward stairs never hold a monster; at sea the group order is table B's. | several | absent (no grotto, no sea in v0.2) | **n/a chapter 2** | none |

## Stats and effects

Page: [stats-and-effects]. These are equipment rules; chapter 2's reachable gear is the
copper sword and the opening clothes (`party.rs`, the new-game template), so most of the page is
outside the route.

| # | Claim (our words) | Original (EU) | Ours | Verdict | Tests |
|---|---|---|---|---|---|
| S1 | A weapon's added effect is one roll of 100 per target hit, shared by all of the weapon's effects; the monster's resistance only matters for immunity; a skill can force it. | none cited | `battle/model.rs:261` (`rider`) is read and refused above 0 (`battle/model.rs:451`) | **not implemented** (refused). Whether a chapter-2 reachable weapon carries one is **[unknown]** | none |
| S2 | Whips and boomerangs hit a group or everyone with ×1, ×0.8, ×0.6, ×0.4, ×0.2 by rank. | none cited | `battle/model.rs:281` (the flag) and `:470` (refused) | **not implemented** | none |
| S3 | The falcon blade (and two other items) strike the same target twice at full power. | none cited | `battle/model.rs:107` (`weapon_strikes_several`), refused by `Unsupported::MultiHitWeapon` | **not implemented** (refused) | none |
| S4 | The miracle sword and hero spear give back a quarter of the damage as HP. | none cited | absent | **not implemented** | none |
| S5 | The metal slime swords do +1 against metal; the poison needle always deals 1 and cannot be dodged or blocked. | `ov_024::021e6a90` (weapon `+0x10` bit 18 and the `+1`, `func_02085128`; the needle's `1.0` and cleared flags, `func_020851d8`) | `battle/resolve.rs::final_amount` (`metal_bonus` adds 1.0 after the coin; `poison_needle` forces 1.0 and clears the dodge and the block in `resolve_action`) | **agrees** on the code; the item record behind each flag is **[hypothesis]**: `itembtlprm` `+0x00` bits 4 and 10, seven metal-family weapon ids and the needle alone (19057) | `the_plus_one_weapon_makes_one_and_the_poison_needle_deals_exactly_one`; `the_metal_body_rules_read_the_rom_records` (the two flag sets on the ROM) |
| S6 | The rusty sword, Hela's hammer and the oh-no bow miss one time in four. | none cited | absent | **not implemented** | none |
| S7 | Some armour restores a flat 25 HP at the end of every turn, and does not stack. | none cited | absent (no end-of-turn effect) | **not implemented** | none |
| S8 | Some armour returns a fifth of a blow half the time, reflects spells one time in four, or is cursed (a lost turn one time in four). | none cited | absent | **not implemented** | none |
| S9 | A healing item is spent only if it did something (HP or MP restored, an ailment cured, an ally revived); attacking items are always spent; weapons used as items are never spent. | `ResolveAction` (EU `0x021ed724`..`0x021ed7e4`) gating `func_020ddb38` on the action's applied counter; the kinds' handlers raise it | `battle/item.rs` (`effect_applied`, `clears_held_slot`), fed by `battle/resolve.rs`'s `TargetOutcome` | **agrees** for the reachable items since `j4-item-spend` (`2da4e51`): a herb on an ally at full HP, a revive on a living ally and an antidote with nothing to cure all keep the item; the record's `+0x08` bit 19 and the category gate the held slot. The field menu's own no-effect rule is unread (`facility.md`) | `a_herb_at_full_hp_is_kept`, `a_herb_at_full_hp_is_kept_and_a_heal_spends_one` |
| S10 | The prayer ring breaks one time in four each use. | none cited | absent | **n/a chapter 2** (the ring is not reachable) | none |
| S11 | A book in a pocket grants an aptitude (an ability, a critical bonus, a tension retention, the Thief's Theory loot pass…). | none cited | absent; `party.rs:515` leaves `crit_book_bonus` at 0 | **not implemented** (v0.2 has no book) | none |
| S12 | Items that inflict a status roll `0–99 < 90 × multiplier` (25 % for the sandstorm spear), never against metal. | none cited | absent (no item status path) | **not implemented** | none |

## Spells and abilities

Page: [spells-and-abilities]. The value column is data; the chapter-2 actions are the ones
`docs/re/battle.md` decoded from the EU records.

| # | Claim (our words) | Original (EU) | Ours | Verdict | Tests |
|---|---|---|---|---|---|
| A1 | Action value ranges: Frizz 14→99 ±2, Crack 30→119 ±5, Heal 35→160 ±5, Woosh 16→69 ±8, Crackle 50→150 ±8, each with its cap. | `actdamage_*.nat` records | the ranges are read by `battle/setup.rs:223`; `docs/re/battle.md`'s decoded table gives Crack's spread 5 / least 30 / monster 17 and Heal's 5 / 35 / 35 | **agrees** for the actions we decoded (the site's "119" is the scaled maximum, not the record's least) | `tests/battle_rom.rs` checks the adapter against the oracle's hand-decoded actions |
| A2 | Fullheal and Omniheal restore the maximum HP; their normal amount draws and coin still happen. | `ov_024::021db3a4` | `battle/resolve.rs:258` (kind 2 replaces the amount for `0x21` and `0x310` after the draws) | **agrees** (the batch `j4-full-heal`'s correction) | the real-ROM full-heal regression (HP 37 → 321, five draws) |
| A3 | MP costs: Crack 3, Crackle 8, Woosh 3, Heal 2, Buff 3, Accelerate 2, Dazzle 5, Fullheal 24; Omniheal 128. | `actdt_*.nat` `+0x08` low byte | `battle/setup.rs:185`; a cost of 128 is refused (`Unsupported::MpCost`) and the app checks the cost before the round (`battle/round.rs:510`) | **agrees** on the costs; Omniheal stays **not implemented** by design | the Omniheal refusal test (batch `j4-full-heal`) |
| A4 | Buff, Accelerate and Dazzle land with `75 % → 100 %` by magical might, then by the target's resistance. | none cited | a monster's Dazzle lands on its own scaling accuracy (`battle/resolve.rs:127`); a party member's scaling accuracy is refused (`Unsupported::AccuracyScaling`) | **not implemented** for a party cast; **agrees** for the monsters' Dazzle record we play | `a_scaling_accuracy_is_the_chance_times_the_resistance` |
| A5 | Ability multipliers (Falcon Slash ×0.75 a hit, Multifists ×0.5, Dragon Slash ×1.5…) and their hit counts/caps. | none cited | absent (those actions are not in the chapter-2 ways; a hit code above 0 is refused, `battle/ai.rs:405`) | **n/a chapter 2** | none |

## Where the address decides

Four claims disagreed with our code. Each was resolved by the European function, not by the
site:

1. **The damage cap is last** (B7). The page lists the cap as the second multiplier.
   `ov_024::021e6a90` truncates once with `_ffix` and clamps `& 0x3fff` afterwards
   (`ghidra-final-damage.txt`); our order stands.
2. **The land company is shuffled** (R8). The page keeps the touched group first on land.
   `0x0209afe4` draws the same coin and six-row permutation the sea uses
   (`ghidra-company.txt`); our port stands.
3. **Flight refuses species 38–40** (B22). The page names three monsters; the routine tests
   `0x26..0x28` before anything else. The range is the one our encounter code already reserves
   for "never surprises", so it is the species, not a member index. The engine refuses it since
   `ed0974b` (`flight_is_refused_against_species_38_to_40`).
4. **Defend halves a flagged blow, not every blow** (B10). The page words it as a rule for all
   blows; the function gates it on the action's `+0x10` bit 4. Our gate stands.

Two more claims were ours to fix, not the site's; both are done:

5. **The wear-off chances were one index late** (B16), and **dazzle has its own array** (B17).
   `ov_000::0215858c` decrements the second count and *then* indexes; it also reads two different
   arrays for dazzle and for defence/agility. `ed0974b` keys the table by the decremented count and
   wires dazzle's array (`battle/round.rs`, `WEAR_OFF_CHANCE`/`DAZZLE_WEAR_OFF_CHANCE`).
6. **The spawn block looks at every member** (R4). `0x02073e0c` loops over four members; `e775c65`
   gives `StepInput::party` the three other slots. `dq9-app` passes the leader only: it has no
   follower field characters, so that call is the party of one the block sees on the field.

## Work items from this audit, by value for v0.2

Each item names the work, the evidence and its status: done, decided or still open.

1. **Wear-off chances — done (`j4-audit-fixes`/`ed0974b`)** (`battle/round.rs`). The table is
   indexed by the decremented second count (rolls 62.5/75/87.5/100, the page's 63/75/88/100) and
   dazzle has its own array, read on the ROM: `DAZZLE_WEAR_OFF_CHANCE` holds 37.5/62.5/87.5/100
   (`build/evidence/j4-audit-fixes/rom-wear-off-tables.txt`, `cdc2df1cee2d`), so the page's
   38/63/88/100 is that table rounded. Regression test:
   `wear_off_rolls_by_the_decremented_count_and_dazzle_has_its_own_table`.
2. **Flight against species 38–40 — done (`ed0974b`)** (`battle/round.rs`, first test, before the
   surprise round). Evidence: `ghidra-flight.txt`; test `flight_is_refused_against_species_38_to_40`.
   No chapter-2 monster is concerned, so this is correctness, not a route fix.
3. **Spawn block for every member — done (`e775c65`)** (`encounter.rs`). `StepInput::party` carries
   the three other slots' positions and `spawn` compares each with the spawn point at `< 0x3800`;
   evidence `ghidra-spawn-point-test.txt`. `dq9-app` passes the leader only (it has no follower
   field characters), so a party of one is unchanged.
4. **Experience sharing — done (`ca074aa`, wired by `3f8422e`)** (`Party::award_with`): the
   decompiled `ov_023::021f4098`'s two per-member factors, the band lookup and the ×1.05 of the
   elevating shoes are applied. Evidence: `ghidra-xp-share.txt`, `ghidra-xp-helpers.txt`; the
   counter's writer is **refuted** as "turns alive" (below). Still unmodelled: the hold bit of
   `live+0xA8` and mid-battle joins.
5. **Item spend rule — done (`2da4e51`)** (`battle/item.rs`, `battle/fight.rs`). A healing item
   that healed nothing is kept: `ResolveAction`'s applied counter (`0x021ed724`..`0x021ed7e4`)
   gates the held-item helper, and each kind handler raises it only on a real change. The field
   menu's own no-effect rule stays **[unknown]** (`facility.md`).
6. **Grudge targeting — done (`j4-grudge-enrage`/`4e0a0ed`)** (`battle/ai.rs`). The writer is EU
   `0x0208979c` (USA `0x0208978C`), called from `ov_024::021eb5d0` at `0x021ed0d4`; the memory
   (`+0x32`/`+0x34`) and the weights are modelled (`Battle::last_attackers`, `target_weights`) and
   the multi-candidate refusal is gone. The page's 263/438 is a fact: the ROM counts **263 of 438**
   records with `mon_btldata` bit 26. Unblocks any multi-member party against those monsters — the
   v0.2 pilot fights with one hero.
7. **Enrage — read, not modelled, and not needed for v0.2** (`ov_024::021eb08c`). The trigger
   fields the page does not name are `+0x24` (two kind/chance pairs) and `+0x28`/`+0x29` (the
   length range), read by `j4-grudge-enrage`; the Hexagoon's word is `0x1000_0000` with both kind
   fields zero, so **no chapter-2 fight enrages** and the target override is not modelled. The
   ROM counts 341 records with an armed pair, not the page's 287.
8. **Metal rules** (B14, S5) — **done** (`v4-metal-rules`): the miss, the zeroing and its two
   exemptions, the critical, `1 + random(2)`, the "+1 against metal" weapon and the poison needle
   are modelled (`docs/re/battle.md`, "The metal body"). Metal Slash, Metalicker, Needle Shot and
   Quadraslash are now refused by the damage handler or the rider they carry, not by number. No
   chapter-2 monster is metal, so the route cannot exercise them: the unit and real-ROM tests are
   the only proof.
9. **Not for v0.2**: tension (B12–B13), the multi-target falloff (B11), family bonuses (B9),
   multi-hit weapons (S1–S3), the battle memory (R12), monster letters (B32), coups and co-op
   (B33). They are refused, not approximated, so no silent divergence exists today.

## What the page does not settle

- The wear-off arrays are read (item 1); **not** settled: where the two pointers (`0x02158d68`,
  `0x02158d6c`) are written, and whether a fourth status ever indexes them.
- The exact spawn block's field test (`*param_1 == member's field`) and whether a guest counts as
  a member (**[unknown]**; `0x02073e0c` iterates the four party slots of `GetPartyMemberByIndex`).
- The "turns alive" factor's writer (**[hypothesis]**: the counter `0x02053dd0` bumps). **Update 2026-11-17 (`j4-xp-wire`): refuted** — the counter counts command passes of every present slot, not turns alive; it is uniform for a party present from the start and cancels in the share.
- The company's draw accounting against the Hoimi table (R15, **[unknown]**).
- The battle memory's size for chapter-2 species (R12, **[hypothesis]** no effect).

## Symbols

No new name is claimed here: every function this note cites is already in `re/functions.txt`
under its decompilation name, and `docs/re/battle.md` already lists the names this project
proposes for the battle kernel. The two functions whose behaviour changed our reading keep their
`func_` names in `re/symbols.txt`: `0x0209afe4` (the company draw) and `0x02073e0c` (the spawn
point's party test).
