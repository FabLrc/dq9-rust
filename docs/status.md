# Project status

Rewritten by the coordinator at each integration; no history log here.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026

- **v0.1 functional natively**: opening, Observatory, physical return to Angel Falls,
  village tasks 1.2 through return 1.4.1. Driver: 615 checkpoints; parity 2/2 at first
  control and 1.1.2, except flag `4135` (battle 26 loot).
- **Cadence**: fixed 60 Hz; hero, field and model pose at 30 Hz.
- **Browser**: Pages deployment `42800444183`, build `b91f43b`; standalone battle 26
  played to victory (4 rounds, 5 XP, 9 gold). Reward closure restarts that standalone fight;
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

Roaming rules (`re/encounter.md`) have original F02/F38 evidence and an independent
challenge. Zone/node/point/contact checked; standing still does not prevent spawning.
The model waits only for the first step after arrival; that cause remains [hypothesis].
`j4-roaming-fix` (`05fc30f`) corrected variable update delta and radial contact checks;
11/11 oracle tests passed in the recorded author/challenger runs. Surprise, company,
stage selection, F44 and states 4/5/11/12 lack corresponding original-runtime evidence.

## Active batches — Orca / OMP

Both workers use **`opencode-go/deepseek-v4.1-flash`**, base `b91f43b`;
no model escalation. Local specs: `build/batches/<batch>.md`.

| Batch / branch | Dispatch | Contract |
|---|---|---|
| `j4-roaming-play` / `FabLrc/j4-roaming-play` | `ctx_24fdc9c80929` | field models, existing roaming rules, natural contact → real battle → field; battle Esc cancels |
| `j4-battle-held-items` / `FabLrc/j4-battle-held-items` | `ctx_fe142fe11c08` | establish original item-source rules, held-slot identity, execution-only consumption, cancellation |

Workers own disjoint files; coordinator owns status, roadmap, README, app documentation
and integration. Runtime smoke evidence is required; the single full ROM verification
runs at each integration. No implementation from these batches is integrated yet.

## Remaining after the active contracts

Battle action/damage motions, abilities, wipe-out details, real-battle parity references;
facilities and field-menu UI; physical chapter 2 route through both Hexagoon outcomes;
native 417 and missing placements; continuous browser play to 2750 and manual visual
checks. Until integration, roaming monsters remain absent from the displayed field.

Evidence: `build/evidence/<batch>/manifest.json`; original field runs:
`build/oracle/field-F02`, `build/evidence/j4-roaming-challenge/`. Cleanup risk: Orca
worktrees can remain locked during `integrate.ps1 -Cleanup`; remove junctions safely
before Orca release (`orchestration/README.md`).
