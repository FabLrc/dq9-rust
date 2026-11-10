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
| `j4-field-services` / `660d682` | Field menu (items/equipment/attributes/spells), inn/shop/church screens, bounded hero rebuild, copied-item and group-heal fixes; one copy per trade, no copy lost to a 99 stack | 5/5, 95.6 s, app 131/131 |
| `j4-guest-slot` / `d079b97` | Condition 0x56's `0xce` slot is the field guest (actions `0xcd`/`0xcc`); the M01 2.2 scene now starts event 2230, without a hero tag | 5/5, 122.0 s, core 264 unit + field-ROM 7/7 |

Native F01 contact with monster 57 → victory in 3 rounds (3 XP, 5 gold) → same field, no ghost;
D01M02 models visible. Evidence: `j4-roaming-play/field-restored-after-win.png` (`61a585913e52`),
`hexagon-field.png` (`fceee50be300`), `smoke-win.log` (`0ec1f1917e68`), under `build/evidence/`.
Held-item UI was rasterized offscreen, not driven in a native window or browser:
`j4-battle-held-items/item-menu.png` (`9e2260ec767e`), `item-used.png` (`f4baed36c8c8`).
Kernel: 306 BattleEmulator vectors (Woosh critical differs); chapter 2 special actions have
code-read rules and 20-seed tests, not an independent battle oracle (`re/battle.md`).
Roaming F02/F38 oracle/challenge covers zone, node/point and contact. First-step waiting,
original surprise/company/stage, F44, states 4/5/11/12 and exact post-flight state remain open.

Field services (`j4-field-services`, native windows on the European ROM and the Web canvas):
paid shop (gold 100 → 92, one herb), inn (the living healed to 40/40 and 9/9, the dead left at
0/30, night clock 0 → 210), church resurrection (0 → 30 HP for 60 gold), Divination with its
line, Multiheal over `[0, 1]` with MP charged once, Fullheal 20 → 80 for 24 MP and a dead
actor's visible refusal; the bounded hero rebuild is asserted over 20 alternating outfits with
stable model/instance/texture counts. Two independent read-only reviews cleared the corrected
state (their reports are local). Evidence: `build/evidence/j4-field-services/` (manifest at
`0ef7a82`) and the review manifests. Direct-map and single-service checks; continuous play is
still not certified.

## Browser and publication

Pages `42812911849` (`6ecf718`): Hexagon contact → Esc cancel → flight, another contact → loss
→ church 1106; `j4-combat-web-final/console.json` (`2798e51a2364`), church PNG (`430671952e62`).
Graphics `1e0326d`: CI `42831807723` and Pages `42831807515` green; wide remains optional.
Latest local browser build `4327fd8`: battle 26, seed 7 → one victory, 4 rounds, 2 XP, 4 gold;
hero lunges/returns, all defeated/fled models disappear, extra input/layout switches do not restart.
Evidence: `j4-battle-gpu-sync/web-after-lunge.png` (`c461e1d32686`),
`web-after-victory.png` (`c39b70b7ba1a`), `web-after-console.json` (`e616bcfb24af`).
Latest publication `deb317f` (services `660d682`): CI `42854310912` and Pages `42854311711` green.
The deployed bundle at `https://fablrc.github.io/dq9-rust/?map=M01` loaded map 1100 and opened
the field menu on the bottom screen (`j4-field-services/web-published-field-menu.png`,
`ecb4ee34992f`); the ROM is read locally and never sent. The same bundle played battle 26
(`?battle=26&seed=7&layout=wide`) to **Won after 4 rounds, 2 XP, 4 gold** with the hero's
lunges and an empty stage afterwards (`web-published-battle-won.png`, `a3c501e11d18`), so the
new menu input path did not disturb battle input. Earlier `1934b83` published Fullheal
(CI `42847350996`, Pages `42847350994`); no interactive Fullheal-menu claim there.
Direct-map checks do not certify continuous new-game play to 2750.

## Active work and next steps

One implementation batch retains its own ownership; nothing else is unmerged.
`j4-chapter2-route`: two Flash failures; the Sonnet retry `ctx_273f59bcad5d` now crosses
M01M10 by a generic warp-volume approach and reaches **2.2.3 in D01M01** with real battles
(commits `c9227e6`, `42b0174`). The stall it hit was **trigger 0x56** at M01M05: the party tag
`+0x56b` is **0** in `pro1.ml1`/`pro11.ml1` and in the demo save, so the condition resolves
through "character `0xce` exists" — the field guest (`Guest_LoadTask`, actions `0xcd`/`0xcc`),
which the port did not hold. The proposed hero `tag: 1` was refused; the slot is now modelled
(`j4-guest-slot` / `d079b97`, `re/trigger.md`) and a ROM regression pins that the end of 2210
attaches the guest, the mayor's line carries `0xC0` and its end starts 2230. Hexagon interior
routing, S01M01 events 2300–2350, Hexagoon won/lost and 2750 remain unreached, so v0.2 is not
certified.
`j4-field-services` progress is now integrated (table above); its field limits are recorded in
`docs/app.md` § "Field menu and facility screens" and `re/facility.md`.
Headless parity keeps stand-ins unless `--fight` is given and disables roaming.
Remaining presentation: full camera/effects, original alpha fades, companions; timing limits
are labelled [hypothesis] in `re/battle-data.md`. Abilities/worn-item commands remain open.
Completed graphics/motion/field-services and coordinator-fix worktrees/local branches removed; the graphics empty root stays Windows-locked. Evidence stays local, never committed.
