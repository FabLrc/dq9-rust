# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 17, 2026

**v0.1 functional natively; v0.2's native criterion is met and its browser criterion now has a
continuous session.** A new game played by the pilot with the real battle engine reaches **event
2750** (Hexagoon lost at Lv 4–7, won at Lv 8, story **2.6.5**, 3057 s of game time, 972 checkpoints).
The same run in the wasm build, in Edge, ends `stopped: event 2750 ended` after 3057.2 s of game
time with checkpoints **byte-identical** to the native ones (`j4-web-pilot`, instant battles).
Parity 2/2 with the party fields in the checkpoint. Fixed 60 Hz loop; hero, field and model pose at 30 Hz.

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
| `j4-abilities-capture` / `97fd1e2` | `--battle` takes `--save`; Abilities list and a cast captured (Gigaslash, MP 552 → 537) | 5/5 |
| `j4-encyclopedia-audit` / `236ea6a` | 67-row audit of the Encyclopedia against the engine, six divergences decompiled (`re/encyclopedia-audit.md`) | 5/5 |
| `j4-parity-party` / `daa9553` | Party HP/MP/XP/level and gold in the checkpoints, original side read from RAM; fought ≠ stand-in on 611/615 lines | 5/5, parity 2/2 |
| `j4-ch2-gaps` / `2930280` | Natives 560, 573, 417/421 (window shake), cast modes of absent members; chapter 2 report 47 → 49/50 | 5/5 |
| `j4-audit-fixes` / `ed0974b` | Wear-off chances by the decremented count (dazzle's own array), flight refused against species 38–40 | 5/5 |
| `j4-web-pilot` / `a861eaf` | Pilot and route mode in wasm, `speed=`/`render=`/`battles=`, Edge session to 2750 | 5/5 |
| `j4-xp-share` / `ca074aa` | `Party::award_with`: the original share (counter, second factor, ×1.05 shoes); **not yet called by the app** | 5/5 |

The route's acceptance run on `main` (`ef56959`): `build/evidence/j4-route-accept/accept-2750.out`
(`9f979a299fc4`), 972 checkpoints; trajectory and the Hexagoon's five attempts in
`build/evidence/j4-chapter2-route/report-route.md`. Chapter-2 battle 26, the Hexagoon both ways
and the win branch (event 15 → 2550 → 2555 → 2.5.1 → … → 2750) are played, not substituted.

## Limits recorded, not hidden

- **Parity and battle effects**: the party's HP/MP/XP/gold are compared, but the original's own RNG
  drew another battle-26 monster set (HP 16/20 vs 13, XP 7 vs 2, gold 13 vs 4); three `accepted`
  entries pin the runtime's exact value. A seed-7 fight stands in for that draw, it does not prove it.
- **Experience share**: the original formula is modelled in `dq9-core` but the application still
  awards through the neutral path (`j4-xp-wire`); the counter's round semantics are **[hypothesis]**.
- **Audit leftovers** (`re/encyclopedia-audit.md`): spawn block for every party member (solo in
  chapter 2), grudge targeting, enrage (whether the Hexagoon enrages is **[unknown]**), item spend rule.
- **Containers**: runtime `LootableContainer` objects, nothing drawn or hidden; cupboards have no
  position; a mimic opens nothing; locked chests refuse with no flag.
- **Route mode** is a test driver, not a game feature; `D01M02`'s west stair being a dead end is
  **[hypothesis]**. Patch persistence: pending placement states (`+0x41b`) are still not modelled.
- **Shown battles in the browser** (`battles=show`) stop at 2489 s: the pilot left `D01M05` without
  talking to NPC 203; cause **[unknown]** (`j4-show-battles`).
- **Wipe-out**: the priest's line, the save-carried revival map, trigger action 180 and `ME_009` are
  unmodelled (`j4-wipeout-priest`). 2535+22510 stays partial by construction.

## Browser and publication

Pages deploys on each push to `main` (`a861eaf`: Pages `44111729781` success). The Edge session above
ran on the local release bundle, not on the hosted page. Earlier hosted checks: field services
`deb317f`, graphics `1e0326d`, standalone battle 26 victory.
Evidence (local, `build/evidence/j4-web-pilot/`): console log `a566bf824b31`, checkpoints equal to
`j4-route-accept/accept-2750-checkpoints.jsonl` (`a0d6200bb7b8`).

## Active work and next steps

In flight (wave 3, base `ca074aa`): `j4-xp-wire` (award through the original share), `j4-wipeout-priest`,
`j4-spawn-block`, `j4-show-battles` (Sonnet). Then: grudge targeting and enrage, worn-equipment use,
remaining wipe-out details, a hosted-page run of the session, docs consistency.
Dev environment: `dev`/`test` profiles keep line tables only (the integrate target measures 3.2 GB against 4.7 GB before).
Evidence stays local under `build/evidence/<batch>/`, never committed.
