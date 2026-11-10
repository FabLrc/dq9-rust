# Project status

Rewritten by the coordinator at each integration; no history log here.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026

- **v0.1 functional natively**: opening, Observatory, physical return to Angel Falls,
  village tasks 1.2 through return 1.4.1. Driver: 615 checkpoints; parity 2/2 at first
  control and 1.1.2, except flag `4135` (battle 26 loot).
- **Cadence**: fixed 60 Hz; hero, field and model pose at 30 Hz.
- **Browser**: Pages deployment `42803107861`, build `15d965a`; standalone battle 26
  played to victory on `b91f43b` (4 rounds, 5 XP, 9 gold). Reward closure restarts it;
  assigned to `j4-roaming-play`. Continuous certification still stops at the raised hall.
- **CI**: last recorded `main` green after Windows-only app-test dead-code fix;
  `cfg(all(test, windows))` must also govern test-only allowances on Linux.

## v0.2 — J4: new game → event 2750, every battle fought

Integrated with ROM verification: party state and levels (`re/party.md`); battle tables
(`re/battle-data.md`); battle kernel (306 BattleEmulator vectors, Woosh critical differs),
whole-battle engine, chapter 2 monster actions (`re/battle.md`); five-language battle text
(`re/battle-text.md`); scene, transition, interactive commands, rewards, level-ups and
wipe-out (`app.md`, Battle); facility rules without UI (`re/facility.md`); billboards
(`re/g3d.md`). Chapter 2 events: 47/50 complete (`re/stb-natives.md`).

Held-item commands integrated as `c064473` (`j4-battle-held-items`, Flash): member slots,
execution-only consumption, duplicates, cancellation, skipped actor and reusable stone;
bag unchanged. Full `verify.ps1 -RequireRom` passed (5/5, 104.7 s; app tests 71/71).
Offscreen UI evidence: `build/evidence/j4-battle-held-items/item-menu.png` (`9e2260ec767e`),
`item-used.png` (`f4baed36c8c8`); not an interactive native/Web item smoke.

Roaming rules (`re/encounter.md`) have original F02/F38 evidence and an independent
challenge. Zone/node/point/contact checked; standing still does not prevent spawning.
The model waits only for the first step after arrival; that cause remains [hypothesis].
`j4-roaming-fix` (`05fc30f`) corrected variable update delta and radial contact checks;
11/11 oracle tests passed in the recorded author/challenger runs. Surprise, company,
stage selection, F44 and states 4/5/11/12 lack corresponding original-runtime evidence.

## Active batches — Orca / OMP

Workers use **`opencode-go/deepseek-v4.1-flash`**; no model escalation.
Local specs: `build/batches/<batch>.md`.

| Batch / branch | Dispatch | Contract |
|---|---|---|
| `j4-roaming-play` / `FabLrc/j4-roaming-play` | `ctx_24fdc9c80929` | field models, existing roaming rules, natural contact → real battle → field; battle Esc cancels |

Held-items worker released after integration; its branch and private target removed.
Roaming delivery `f858c3f` is under review: generator return and equipment-aware party
inputs do not yet meet the contract. No roaming implementation is integrated yet.

## Remaining after the active contracts

Battle action/damage motions, abilities, wipe-out details, real-battle parity references;
facilities and field-menu UI; physical chapter 2 route through both Hexagoon outcomes;
native 417 and missing placements; continuous browser play to 2750 and manual visual
checks. Until integration, roaming monsters remain absent from the displayed field.

Evidence: `build/evidence/<batch>/manifest.json`; original field runs:
`build/oracle/field-F02`, `build/evidence/j4-roaming-challenge/`. Cleanup risk: Orca
worktrees can remain locked during `integrate.ps1 -Cleanup`; remove junctions safely
before Orca release (`orchestration/README.md`).
