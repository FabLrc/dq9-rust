# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026

**v0.1 functional natively; v0.2 still in progress** (new game → event 2750, every battle fought).
Opening, Observatory, physical Angel Falls return and village tasks through 1.4.1 work on main.
Driver: 615 checkpoints; parity 2/2 at first control and 1.1.2 except flag `4135` (battle loot).
Fixed 60 Hz loop; hero, field and model pose at 30 Hz. Chapter 2 events: 47/50 complete.

## Combat and monsters — integrated on main

Workers: Orca/OMP `opencode-go/deepseek-v4.1-flash`; route → `anthropic/claude-sonnet-5-5` after two Flash failures (AGENTS policy).

| Batch / commit | Result | Full ROM gate |
|---|---|---|
| `j4-battle-held-items` / `c064473` | Held slots, duplicates, execution-only consumption, cancellation, skipped actor, reusable stone; bag unchanged | 5/5, 104.7 s, app 71/71 |
| `j4-roaming-play` / `2801473` | Visible models, contact → battle → field, shared RNG, equipment/preset companies, released models hidden; Esc/one-shot preview | 5/5, 90.4 s, app 82/82; parity 2/2 |
| `j4-battle-lifecycle` / `749ae40` | Finished preview retains stage/camera; dialogue pauses roamers | 5/5, 103.5 s, app 83/83 |
| `add-graphics-tech` / `49843de` | Optional wide/post/HUD; native and local browser wide → stacked → wide checked | 5/5, 150.3 s, app 86/86 |
| `j4-battle-motions` / `de583e0` | ROM motions/scripts, weapon set, hit reactions, death/flight, summon capture, presentation barriers | 5/5, 80.4 s, app 102/102; parity 2/2 |
| `j4-battle-gpu-sync` / `4327fd8` | Live lunge/hide matrices reach GPU; native image regression and real Web victory without ghost models | 5/5, 109.4 s, app 103/103 |
| `j4-full-heal` / `5723a5d` | Original max-HP override after ordinary draws; real `Battle::round` HP 37 → 321, MP 40 → 16; Omniheal MP128 still refused (`re/battle.md`) | 5/5, 110.2 s, core battle-ROM 10/10 |

Native F01 contact with monster 57 → victory in 3 rounds (3 XP, 5 gold) → same field, no ghost;
D01M02 models visible. Evidence: `j4-roaming-play/field-restored-after-win.png` (`61a585913e52`),
`hexagon-field.png` (`fceee50be300`), `smoke-win.log` (`0ec1f1917e68`), under `build/evidence/`.
Held-item UI was rasterized offscreen, not driven in a native window or browser:
`j4-battle-held-items/item-menu.png` (`9e2260ec767e`), `item-used.png` (`f4baed36c8c8`).
Kernel: 306 BattleEmulator vectors (Woosh critical differs); chapter 2 special actions have
code-read rules and 20-seed tests, not an independent battle oracle (`re/battle.md`).
Roaming F02/F38 oracle/challenge covers zone, node/point and contact. First-step waiting,
original surprise/company/stage, F44, states 4/5/11/12 and exact post-flight state remain open.

## Browser and publication

Pages `42812911849` (`6ecf718`): Hexagon contact → Esc cancel → flight, another contact → loss
→ church 1106; `j4-combat-web-final/console.json` (`2798e51a2364`), church PNG (`430671952e62`).
Graphics `1e0326d`: CI `42831807723` and Pages `42831807515` green; wide remains optional.
Latest local browser build `4327fd8`: battle 26, seed 7 → one victory, 4 rounds, 2 XP, 4 gold;
hero lunges/returns, all defeated/fled models disappear, extra input/layout switches do not restart.
Evidence: `j4-battle-gpu-sync/web-after-lunge.png` (`c461e1d32686`),
`web-after-victory.png` (`c39b70b7ba1a`), `web-after-console.json` (`e616bcfb24af`).
Motion/GPU `af76e8d`: CI `42838706753` and Pages `42838706825` green; published help text checked.
Direct-map checks do not certify continuous new-game play to 2750.

## Active work and next steps

Two implementation batches retain separate ownership; neither is integrated or certified yet.
`j4-field-services`: `93957c6` avoids map reload; reviewed full-stack item loss, group-heal and
inn/church state bugs plus unbounded hero resources are being corrected. Paid native proof exists.
`j4-chapter2-route`: two Flash failures; Sonnet retry `ctx_273f59bcad5d` owns the same task.
Latest run stops at 1.2.1 on 1107; M01M10 approach also remains open. Hexagoon/2750 not reached.
Headless parity keeps stand-ins unless `--fight` is given and disables roaming.
Remaining presentation: full camera/effects, original alpha fades, companions; timing limits
are labelled [hypothesis] in `re/battle-data.md`. Abilities/worn-item commands remain open.
Completed graphics/motion and coordinator-fix worktrees/local branches removed; graphics empty root Windows-locked. Evidence stays local, never committed.
