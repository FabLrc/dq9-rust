# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026

**v0.1 functional natively; v0.2 still in progress** (new game → event 2750, every battle fought).
Opening, Observatory, physical Angel Falls return and village tasks through 1.4.1 work.
Driver: 615 checkpoints; parity 2/2 at first control and 1.1.2 except flag `4135` (battle loot).
Fixed 60 Hz loop; hero, field and model pose at 30 Hz. Chapter 2 events: 47/50 complete.

## Combat and monsters — integrated on main

Both batches used Orca / OMP / **`opencode-go/deepseek-v4.1-flash`**; no model escalation.
Each passed its single full `verify.ps1 -RequireRom` gate (fmt, clippy, tests, wasm, coverage).

| Batch / commit | Result | Gate |
|---|---|---|
| `j4-battle-held-items` / `c064473` | Acting member's held slots, duplicates, execution-only consumption, cancellation, skipped actor, reusable stone; bag unchanged | 5/5, 104.7 s, app 71/71 |
| `j4-roaming-play` / `3711b50` | Animated field models, natural contact → battle → field, RNG continuity, equipment-aware inputs, preset companies, released models hidden; Esc cancels, previews run once | 5/5, 90.4 s, app 82/82; parity 2/2 |

Native roaming smoke: F01 contact with monster 57 → victory in 3 rounds (3 XP, 5 gold)
→ same field without a ghost model; D01M02 models visible. Tests cover Hexagon floors
D01M02–04, encounter-free S01M01/D01M01/D01M05, flight retention and one-time rewards.
Evidence: `build/evidence/j4-roaming-play/field-restored-after-win.png` (`61a585913e52`),
`hexagon-field.png` (`fceee50be300`), `smoke-win.log` (`0ec1f1917e68`).
Held-item UI was rasterized offscreen, not driven in a native window or browser:
`build/evidence/j4-battle-held-items/item-menu.png` (`9e2260ec767e`),
`item-used.png` (`f4baed36c8c8`). RE and limits: `re/encounter.md`, `re/battle.md`, `re/party.md`.

Earlier integrated work: party/levels, battle data/kernel/flow and chapter 2 monster actions,
five-language messages, battle scene/rewards/wipe-out, facility rules without UI, billboards.
Kernel: 306 BattleEmulator vectors (Woosh critical differs); chapter 2 special actions have
code-read rules and 20-seed tests, not an independent battle oracle (`re/battle.md`).
Roaming F02/F38 oracle/challenge covers zone, node/point and contact. Standing still can spawn;
only first-step waiting remains [hypothesis]. Original surprise/company/stage, F44, states
4/5/11/12, exact post-flight state and recent-list writer remain unmeasured or [hypothesis].

## Browser and publication

Last checked Pages deployment: `42808567716` (`f77b722`, held items included).
Standalone battle 26 was played to victory on `b91f43b` (4 rounds, 5 XP, 9 gold).
Its observed restart is fixed in `3711b50`; final roaming Web smoke/deployment is pending.
Continuous browser certification still stops at the raised hall, not at chapter 2.

## Active work and next steps

Implementation workers finished; coordinator is publishing the roaming batch and checking
its browser surface, followed by the docs-consistency review. No new implementation batch.
Next: battle motions/abilities/worn-equipment use and wipe-out details; field menu/facility
screens; physical chapter 2 route through both Hexagoon outcomes; native 417 and missing
placements; continuous browser play to 2750; real-battle parity references.
Headless parity keeps stand-ins unless `--fight` is given and disables roaming.
Evidence stays local under `build/evidence/<batch>/`; never committed. Orca worktree cleanup
can hit a directory lock: remove junctions before release (`orchestration/README.md`).
