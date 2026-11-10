# Project status

Rewritten by the coordinator at each integration; do not add a log here.
Plan and milestones: [`roadmap.md`](roadmap.md); working rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 10, 2026 (`main` at `81dd3cd`; wave v2-3 launched, CI green again)

- **v0.1 functional natively**: `--new-game` plays the opening, the Observatory prologue, the
  physical return to Angel Falls and the village tasks 1.2 to the return 1.4.1, covered by
  real tests. The driver replay reproduces its 615 checkpoints; parity 2/2 (first control and
  1.1.2 equal to the original except `4135`, battle 26 loot).
- **Cadence**: fixed 60 Hz loop; hero, `Field::step` and model pose at 30 Hz.
- **Browser** (Pages, build `21e1e42`): opening to the raised hall; no continuous session
  further (`app.md`, § Verification).

## v0.2 (J4): new game → end of event 2750 (landslide cleared), every battle fought

Integrated, each with `verify.ps1 -RequireRom` green (details in the `docs/re` notes):

| Batch | Model | Result |
|---|---|---|
| `j4-party-state` | Flash | level tables, `party::Party` in `GameState`; new game equal to `pro1.ml1` (`pro11.ml1` already holds battle 26's rewards) — `re/party.md` |
| `j4-battle-data` | Flash | `dq9_formats::battle`, `dq9re monster/action/encounters`; no roamers on `S01M01`, `D01M01`, `D01M05` — `re/battle-data.md` |
| `j4-battle-kernel` | Sonnet | one action resolved like the game; 306 action vectors equal to BattleEmulator (Woosh critical: BattleEmulator truncates early) — `re/battle.md` |
| `v2-ch2-cutscenes` | Sonnet | `--events-report 2.1-1` 47/50 complete (was 8); party guest Ivor (cast mode 10), screen cover 102/103 — `re/stb-natives.md` |
| `j4-battle-text` | Flash | battle messages and name inflection in five languages, `dq9re battle-text`; French `str_bres` 6 embeds English lines in the ROM — `re/battle-text.md` |
| `j4-battle-flow` | Sonnet | whole battles headless from ROM records (rounds, monster ways, flight on the world generator, drops on C `rand`); slimes won, Hexagoon lost at level 1 — `re/battle.md` |
| `j4-party-progress` | Flash | party fighters from equipment (`itemdt` stats reader), `Party::award`; reproduces `bat6.ml1` — `re/party.md`, `re/item.md` |
| `j4-roaming` | Sonnet | `dq9_core::encounter`: spawn, zone by the hour, roamer states, contact (126°), surprise, company, stage from the ground — `re/encounter.md` |
| `j4-battle-scene` | Flash | battle scene (stage, grid, side camera, 547 transition), `--battle N`, `--battle-stage ID --monsters n[xc]`; prologue report 26/26 — `app.md` § "Battle scene" |

Names of the battle and roaming functions are in `re/symbols.txt`.

What a player sees today: the chapter 2 scenes and the battle scene of scripted battles; the
fight itself is still the stand-in ("won"), no roaming monster is drawn, no menu or facility.

## Active wave v2-3 (run `run_5fc3a0162f54`, one Cargo target each)

| Batch | Model | Goal | Owns (shared files) |
|---|---|---|---|
| `j4-battle-actions` | Flash | chapter 2 monster actions (Flee, buffs, Dazzle, Weird Dance, Defend, items) resolved | `dq9-core/src/battle/**` |
| `j4-battle-play` | Sonnet | battle played in the app: commands, messages, award, wipe-out; `--fight` | `dq9-app` (`app.rs`, `battle*`), `dq9-render/**` |
| `j4-roaming-oracle` | Sonnet | melonDS field states check the roaming rules, then a challenger | `dq9-core/src/encounter*`, `scripts/melonds-field.ps1` |
| `j4-facilities-core` | Flash | inn, church, shop, treasure data and rules (no UI) | `dq9-formats`/`dq9-core` `facility*` |

CI failure of `2bec2e2` (wasm step, `-D warnings`, a method used only natively) fixed in
`81dd3cd`; `verify.ps1` now denies warnings in its wasm step like CI. Workers must not commit the
README progress block (the coordinator regenerates it at integration).

## Blockers and next steps

1. **Battle in play**: wire `dq9_core::battle` and `Party::award` to the scene with command and
   message windows, remove `scenario::stand_in`; `j4-battle-actions` for the statuses, damage
   handlers and special actions the chapter 2 monsters use (list in `re/battle.md`).
2. **Roaming monsters**: draw them, start encounters; zone choice and spawn rule stay
   **[hypothesis]** until a melonDS field state (`demo.sav`, `C01` → `F02`/`F38`) checks them,
   then a challenger; the list of other roamers pulled into a fight is **[unknown]**.
3. **Chapter 2 cutscenes**: native 417 (2700+22593) and placements (0, 28)/(0, 30) of
   2535+22510.
4. **Field menu and Angel Falls facilities**, chests and examine, wipe-out; then the chapter 2
   route test 1.4.1 → 2750 with the Hexagoon won and lost, and the browser session.
5. **Parity beyond 1.1.2** and the original reference beyond `X01M09` (`re/melonds-state.md`,
   § Return doors); door cadence 4 vs 8 VBlanks (`re/movement.md`).
6. **`4135`**: battle 26 loot now has a rule (`RollDrops`, C `rand`); it clears once the battle
   is wired with the original's generator state.
7. **Cast, not modelled**: task 0x30 fade, cast teardown during a cutscene, `questorder3.bin`,
   what `021a2fa0` rebuilds besides NPCs (**[unknown]**).
8. **Orchestration**: `integrate.ps1 -Cleanup` fails at `git worktree remove` on Orca
   worktrees (cause **[unknown]**); procedure in `orchestration/README.md`. A worker that
   commits the README progress block conflicts on rebase: drop or regenerate it
   (`verify.ps1 -Only coverage`).

## Evidence and comparison

Local evidence: `build/evidence/<batch>/manifest.json` (`scripts/evidence.ps1`). Parity:
`scripts/parity.ps1`; diagnosis: `scripts/compare-runs.ps1`, `dq9re state diff/watch`.
