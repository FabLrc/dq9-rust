# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026

**v0.1 functional natively; v0.2 still in progress** (new game → event 2750, every battle fought).
Opening, Observatory, physical Angel Falls return and village tasks through 1.4.1 work.
Driver: 615 checkpoints; parity 2/2 at first control and 1.1.2 except flag `4135` (battle loot).
Fixed 60 Hz loop; hero, field and model pose at 30 Hz. Chapter 2 events: 47/50 complete.

## Combat and monsters — integrated on main

Combat batches used Orca / OMP / **`opencode-go/deepseek-v4.1-flash`**; each passed its full ROM gate.

| Batch / commit | Result | Gate |
|---|---|---|
| `j4-battle-held-items` / `c064473` | Acting member's held slots, duplicates, execution-only consumption, cancellation, skipped actor, reusable stone; bag unchanged | 5/5, 104.7 s, app 71/71 |
| `j4-roaming-play` / `2801473` | Animated field models, natural contact → battle → field, RNG continuity, equipment-aware inputs, preset companies, released models hidden; Esc cancels, previews run once | 5/5, 90.4 s, app 82/82; parity 2/2 |
| `j4-battle-lifecycle` / `749ae40` | Completed preview keeps stage/camera; dialogue pauses roamers and resumes them on close | 5/5, 103.5 s, app 83/83 |
| `add-graphics-tech` / `49843de` | Optional wide 16:9, cinematic post pass, HUD and shader validation; native captures and local browser wide → stacked → wide checked | 5/5, 150.3 s, app 86/86 |

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

Pages `42812911849` (`6ecf718`): F01 models visible; D01M02 natural contact → Esc
cancel → flight, then another monster → loss in four rounds → church 1106, all by keyboard.
Evidence: `build/evidence/j4-combat-web-final/console.json` (`2798e51a2364`),
`revival-church.png` (`430671952e62`). Standalone battle 26 won once (seed 7: 2 XP, 4 gold).
Combat build `ccb1811`: CI `42815645466` and Pages `42815645558` green; graphics `49843de` passed local native/Web smoke (`add-graphics-tech/web-wide-restored.png`, `5b598af7e34f`), publication pending.
These direct-map checks do not certify continuous new-game play to chapter 2.

## Active work and next steps

Active wave: three Orca / OMP / `opencode-go/deepseek-v4.1-flash` batches, separate ownership.
`j4-field-services`: field menu and inn/shop/church implementation in progress.
`j4-battle-motions`: fixes 1–4 committed as `05e67cd`; paired native capture API cutover
authorized after graphics integration. Not integrated or certified yet.
`j4-chapter2-route`: worker reports real-fight pilot reaches 1.4.1; Observatory route still blocked.
Headless parity keeps stand-ins unless `--fight` is given and disables roaming.
Graphics Git/Orca worktree and local branch removed; an empty Windows-locked directory remains.
Evidence stays local under `build/evidence/<batch>/`; never committed.
