# Project status

Rewritten by the coordinator at each integration; do not add a log here.
Plan and milestones: [`roadmap.md`](roadmap.md); working rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026 (`main` after wave v2-3 and `j4-roaming-fix`; orchestration paused, no active worker)

- **v0.1 functional natively**: `--new-game` plays the opening, the Observatory prologue, the
  physical return to Angel Falls and the village tasks 1.2 to the return 1.4.1, covered by
  real tests. The driver replay reproduces its 615 checkpoints; parity 2/2 (first control and
  1.1.2 equal to the original except `4135`, battle 26 loot).
- **Cadence**: fixed 60 Hz loop; hero, `Field::step` and model pose at 30 Hz.
- **Browser** (Pages, build `21e1e42`): opening to the raised hall; no continuous session
  further (`app.md`, § Verification).
- **CI**: green on `main` since the fix of the Linux-only dead-code error (tests of `dq9-app` are
  Windows-only, `app.rs`: `cfg(all(test, windows))`; rule in `orchestration/worker-rules.md`).

## v0.2 (J4): new game → end of event 2750 (landslide cleared), every battle fought

Integrated, each with `verify.ps1 -RequireRom` green (details in the `docs/re` notes):

| Batch | Model | Result |
|---|---|---|
| `j4-party-state` | Flash | level tables, `party::Party` in `GameState`; new game equal to `pro1.ml1` — `re/party.md` |
| `j4-battle-data` | Flash | `dq9_formats::battle`, `dq9re monster/action/encounters`; no roamers on `S01M01`, `D01M01`, `D01M05` — `re/battle-data.md` |
| `j4-battle-kernel` | Sonnet | one action resolved like the game; 306 vectors equal to BattleEmulator (Woosh critical differs) — `re/battle.md` |
| `v2-ch2-cutscenes` | Sonnet | `--events-report 2.1-1` 47/50 complete (was 8); party guest Ivor, screen cover — `re/stb-natives.md` |
| `j4-battle-text` | Flash | battle messages and name inflection in five languages — `re/battle-text.md` |
| `j4-battle-flow` | Sonnet | whole battles headless from ROM records — `re/battle.md` |
| `j4-party-progress` | Flash | party fighters from equipment, `Party::award`; reproduces `bat6.ml1` — `re/party.md` |
| `j4-roaming` | Sonnet | `dq9_core::encounter`: spawn, zone, roamer states, contact, company, stage — `re/encounter.md` |
| `j4-battle-scene` | Flash | battle scene (stage, grid, camera, 547 transition) — `app.md` § "Battle scene" |
| `j4-battle-actions` | Flash | chapter 2 monster actions (Flee, buffs, Dazzle, Weird Dance, Defend, items) resolved — `re/battle.md`. **No oracle** for them: BattleEmulator has no such cases; code-read rules and 20-seed battle tests only |
| `j4-facilities-core` | Flash | data and rules (no UI) of the inn, church, shop and treasure containers, `Bag::remove` — `re/facility.md` |
| `j4-roaming-oracle` | Sonnet | F02 field states from the demo save by real key presses (`scripts/melonds-field.ps1`), 20 states, claims (a)–(g) — `re/encounter.md` § "Claims checked" |
| `j4-battle-play` | Sonnet | battles **played** in the window and on the Web: command menus (Attack, Defend, Items, spells, Flee), game messages, award and level-up lines, wipe-out, `--fight --seed N`; headless keeps the stand-in for the parity traces — `app.md` § "Battle" |
| `j4-billboards` | Flash | trees and flat scenery of the maps turn toward the camera (G3D commands `0x07`/`0x08`; 342 of 740 map archives) — `re/g3d.md` § Billboards |

Challenger on the roaming rules (Sonnet, `build/evidence/j4-roaming-challenge/report.md`, local):
(a) zone, (b) node and point, (d) contact **confirmed** on new seeds and on F38; "a standing party
spawns nothing" **refuted** (272 spawns without input; only a fresh arrival waits for the first
step); the spawn tick **weaker** than claimed (update step 33 or 50 ms; conditions necessary, not
sufficient); "6→1 after 1386–1452 ms" **false** in general; two repo tests were brittle
(`plan_spawn` draw 0, a 34 ms step, a box contact). Corrected by `j4-roaming-fix` (`05fc30f`):
tests and `re/encounter.md` now replay the game's node, use each update's `dt` and a radial reach,
the model waits for the first step only, and the suite passes (11/11) on the author's run and the
challenger's three. Not tested by anyone: F44, states 4/5/11/12, the 126° threshold, the surprise
roll, the company, the stage.

What a player sees today: the chapter 2 scenes, battles fought for real in the window (scripted
ones and, through `start_encounter`, the entry the roaming monsters will call), trees that face
the camera. No roaming monster is drawn, no field menu, inn, shop or church screen.

## Active batch

None. No worker is running, no worktree or `FabLrc/*` branch is left.

## Blockers and next steps (the plan, `roadmap.md` § J4)

1. **Roaming monsters on screen**: draw them with their field models, call `start_encounter` on
   contact, the stage from the ground. The surprise roll, company, stage and
   "others in the fight" stay **[hypothesis]/[unknown]** (a level-91 party never meets a fight to
   measure them: a field run with a low-level party is needed).
2. **Facilities and field menu**: inn, church, shop screens on top of `dq9_core::facility`;
   chests, pots and barrels on the field; field menu (Items, Equipment, Attributes); chapter 2
   route test 1.4.1 → 2750 with the Hexagoon won and lost; wipe-out details (priest line, revival
   map by save, trigger 180).
3. **Battle finishing**: attack and damage motions (monsters have only `stand/run/walk/appear`),
   `Esc` exits the app instead of cancelling, held items and abilities not offered, window not driven
   by hand (tests and captures only); re-record the parity references with real battles (the
   headless stand-in is kept for the traces).
4. **Chapter 2 cutscenes**: native 417 (2700+22593), placements (0, 28)/(0, 30) of 2535+22510.
5. **Browser**: continuous session from a new game to 2750; the interactive window of the
   billboards and of battles checked by hand.
6. **Parity beyond 1.1.2** and the original reference beyond `X01M09` (`re/melonds-state.md`,
   § Return doors); door cadence 4 vs 8 VBlanks (`re/movement.md`). `4135` (battle 26 loot) clears
   once battles are wired with the original's generator state.
7. **Cast, not modelled**: task 0x30 fade, cast teardown during a cutscene, `questorder3.bin`,
   what `021a2fa0` rebuilds besides NPCs (**[unknown]**).
8. **Orchestration**: `integrate.ps1 -Cleanup` fails at `git worktree remove` on Orca worktrees
   (cause **[unknown]**), the empty worktree directories stay locked; `build/scratch/cleanup.ps1`
   (local) chains the manual steps. A worker's committed README block conflicts on rebase: the
   coordinator regenerates it (`orchestration/README.md`, § CI).

## Evidence and comparison

Local evidence: `build/evidence/<batch>/manifest.json` (`scripts/evidence.ps1`). Parity:
`scripts/parity.ps1`; diagnosis: `scripts/compare-runs.ps1`, `dq9re state diff/watch`. Original
field states of the roaming work: `build/oracle/field-F02`, runs of the challenger under
`build/evidence/j4-roaming-challenge/`.
