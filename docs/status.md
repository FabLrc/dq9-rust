# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026

**v0.1 functional natively; v0.2's native criterion is met, the browser one is not.** A new game
played by the pilot with the real battle engine reaches **event 2750** on main: Hexagoon lost at
Lv 4–7, won at Lv 8, story **2.6.5**, `stopped: event 2750 ended` after 3057 s of game time
(`accept-2750`, pid 11008, exit 0). Opening, Observatory, Angel Falls, the village tasks, the
Mountain Pass, the Hexagon and chapter 2's scenes play in order; fields and chapter 2 events are
47/50 in the report of the earlier batch.
Driver: 972 checkpoints on that run; parity 2/2 (references now play battle 26 with
`--fight --seed 7`, but their checkpoint fields do not observe the party's HP/XP/gold).
Fixed 60 Hz loop; hero, field and model pose at 30 Hz.

## Integrated on main (all passed `verify.ps1 -RequireRom`)

| Batch / commit | Result | Gate |
|---|---|---|
| `j4-battle-motions` / `de583e0` | ROM motions/scripts, weapon sets, hit reactions, death/flight, presentation barriers | 5/5, app 102/102 |
| `j4-battle-gpu-sync` / `4327fd8` | Live lunge/hide matrices reach the GPU; browser victory without ghost models | 5/5, 109.4 s, app 103/103 |
| `j4-full-heal` / `5723a5d` | Original max-HP override after the ordinary draws; Omniheal MP128 still refused | 5/5, core battle-ROM 10/10 |
| `j4-field-services` / `660d682` | Field menu, inn/shop/church screens, bounded hero rebuild, copy-loss and group-heal fixes | 5/5, 95.6 s, app 131/131 |
| `j4-guest-slot` / `d079b97` | Condition 0x56's `0xce` slot is the field guest; M01 2.2 starts event 2230 | 5/5, 122.0 s |
| `j4-gate-pair` / `19906dc` → `j4-gate-polarity` / `cd399c5` | Action `0xa9` opens D01M01's gate pair on its value and flag `0x347`; the first polarity was wrong and corrected | 5/5, 112.1 s |
| `j4-chapter2-route` / `a647098` | Route mode (`--route`), story-aware exits, walkable grid; **opening → 2750 by continuous play** | 5/5 |
| `j4-field-containers` / `b454ad4` | Field containers from `treasure.nsarc`: one-shot red chests, refills, locks, tables; positions are world units (residuals ≤ 0.005) | 5/5 |
| `j4-gate-load-guard` / `ef56959` | The gate initialiser runs on room 7101 only (constant `0xbd1b`), so M01/F01 keep their placements | 5/5 |
| `j4-battle-skills` / `0207b7c` | `skilltable.bin` reader, learned panels (`+0x11A`), Abilities row through `Command::Act` | 5/5 |

The route's acceptance run on `main` (`ef56959`): `build/evidence/j4-route-accept/accept-2750.out`
(`9f979a299fc4`), 972 checkpoints; trajectory and the Hexagoon's five attempts in
`build/evidence/j4-chapter2-route/report-route.md`. Chapter-2 battle 26, the Hexagoon both ways
and the win branch (event 15 → 2550 → 2555 → 2.5.1 → … → 2750) are played, not substituted.

## Limits recorded, not hidden

- **Parity and battle effects**: the references replay the engine's battle 26, but the compared
  fields are the story, flags, quests and hero; the fought and stand-in runs give byte-identical
  checkpoints (`re/tools.md`). Widening the checkpoint is the open step.
- **Abilities**: the menu, MP cost and effects are proven by a ROM-backed menu test; one native
  window capture of a cast is missing because `--battle` refuses `--save`, so a skilled party
  cannot be combined with a scripted battle, and no roamer contact was reachable
  (`build/evidence/j4-battle-skills/`).
- **Containers**: no `.bmdj` placement sits at a treasure position — the containers are runtime
  `LootableContainer` objects created by the loader (`0x0207baa0`), so nothing is drawn or hidden;
  the flag alone refuses a second open. Cupboards have no position; a mimic opens nothing and its
  battle request is unread; locked chests refuse with no flag.
- **Route mode** is a test driver, not a game feature; `D01M02`'s west stair being a dead end is
  **[hypothesis]**. `re/stb.md` § "Chapter 2 route" records the pass itself.
- Patch persistence: pending placement states (`+0x41b`) are still not modelled; this gate pair is
  restored from flag `0x347` on load.

## Browser and publication

Pages `42812911849` (`6ecf718`): Hexagon contact → Esc cancel → flight → loss → church 1106.
Graphics `1e0326d`: CI `42831807723`, Pages `42831807515`. Field services published as `deb317f`
(CI `42854310912`, Pages `42854311711`), with the deployed bundle checked (field menu, battle 26
won, empty stage). Fullheal `1934b83` (CI `42847350996`), guest slot `5a9d78b` (CI `42861073884`),
gate polarity `4e54496` → docs `5d06467`.
**No continuous browser session to 2750 exists**: the standalone fights and contacts above do not
certify the route.

## Active work and next steps

Nothing is unmerged. Next: (1) the continuous browser session to 2750 with the same evidential bar
as the native run; (2) widen the parity checkpoint to the party's HP/XP/gold so a played battle
differs from the stand-in; (3) the abilities window capture above; (4) the remaining chapter 2
scene gaps and the wipe-out details (priest line, revival by save, trigger 180).
Evidence stays local under `build/evidence/<batch>/`, never committed.
