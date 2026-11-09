# Project status

Rewritten by the coordinator at each integration; do not add a log here.
Plan and milestones: [`roadmap.md`](roadmap.md); working rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 9, 2026 (`main` at `c28d318`, v0.2 waves v2-1 integrated, v2-2 active)

- **v0.1 functional natively**: `cargo run -p dq9-app --release -- --new-game` plays
  the opening, the Observatory prologue, the physical return to Angel Falls (1100) and the
  village cutscenes; tasks 1.2 (ring, widow, stable), exit lock, area 12 and return
  1.4.1 covered by real tests (`keepsake_ring_task_…`, `widow_task_…`,
  `stable_pickups_…`, `house_exit_requires_both_task_flags_…`,
  `village_area_12_plays_1410_…`). Battles substituted ("won").
- **In-place progression**: deferred actions reload triggers (event 20) and
  0x8d rebuilds the map cast without reloading it (`re/trigger.md`, § Reloads);
  a cutscene marked in its event list renders at its end the poses and removals from before it
  (NPC snapshot, `re/characters.md`); a stage change resets conversation counters
  to zero (`re/dialogue.md`). The driver replay reproduces its 615 checkpoints
  byte for byte up to `M01M08`.
- **Cadence**: fixed 60 Hz loop; hero, `Field::step` and model pose at 30 Hz
  (odd frames, `Hero::step(ticks = 2)`, `7db7f41`, test
  `field_cadence_holds_the_pose_and_matches_the_original_every_vblank` in `app/tests.rs`).
- **Measured parity**: first player control and free checkpoint 1.1.2 identical to
  the original except `4135`, random loot from battle 26 (accepted deviation, `re/item.md`).
- **Browser** (Pages, build `21e1e42`): opening, first player control, 1140, 1130 and raised
  hall; no continuous session up to 4107, the tower or Angel Falls (`app.md`,
  § Verification).

## v0.2 (J4): new game → end of event 2750 (landslide cleared), every battle fought

Integrated (wave v2-1, all `verify.ps1 -RequireRom` green):

- `j4-party-state` (Flash, `613f29c`): level tables (`dq9_formats::level`), `party::Party`
  in `GameState`; new game equal to `pro1.ml1` on every compared field but the typed name.
  `pro11.ml1` already holds battle 26's rewards (experience 7, gold 13, item 22087): it is not
  "before battle 26" (`re/party.md`).
- `j4-battle-data` (Flash, `68df94f`): `dq9_formats::battle` (monsters, actions, spells,
  `itembtlprm`, encounters), `dq9re monster|action|encounters`; `S01M01`, `D01M01`, `D01M05`
  have no roamers (`re/battle-data.md`).
- `j4-battle-kernel` (Sonnet, `6b5223f`): `dq9_core::battle` resolves one action; 306 action
  and 2400 primitive vectors equal to BattleEmulator / the decompilation; Woosh critical
  deviation where BattleEmulator truncates early (`re/battle.md`).
- `v2-ch2-cutscenes` (Sonnet, `a48ff86`): `--events-report 2.1-1` 47 complete, 3 partial
  (was 8 complete). The report now plays each scene at its own story point; cast mode 10 is
  the party guest (Ivor, `attnpc.gp2`, actions `0xcd`/`0xcc`); natives 102/103 (screen
  cover), 540, 223, 577 drawn; 509, 550, 556, 571 documented without rendered effect.
  Open in range: 417 (2700+22593), placements (0, 28)/(0, 30) of 2535+22510; prologue report
  and `M01` capture unchanged.

Active (one Cargo target each, run `run_5fc3a0162f54`):

| Batch | Model | Owns (shared files) |
|---|---|---|
| `j4-battle-flow` | Sonnet (state machine design) | `dq9-core/src/battle/**` |
| `j4-roaming` | Sonnet (open RE, challenger on zone/spawn) | `dq9-core/src/encounter*`, `lib.rs` |
| `j4-party-progress` | Flash | `dq9-core/src/party*` |
| `j4-battle-text` | Flash | `dq9-formats` `battle/text*`, `message.rs`, `dq9re/src/main.rs` |
| `j4-battle-scene` | Flash | `dq9-app` (`app.rs`, `event*`, `battle*`), `dq9-render/**` |

Next: `j4-battle-actions` (statuses and damage handlers of the chapter 2 monsters, after the
flow), battle windows and messages, roaming monsters on screen, stand-in removal, field menu
and facilities.

## Blockers and next steps

1. **Original reference beyond `X01M09`**: the low return `X01M01` → `X01M05` is
   not obtained in the original (**[unknown]**) and direct replay of native pads is
   falsified (`re/melonds-state.md`, § Return doors). World positions or waypoints
   are needed, not pads.
2. **Parity beyond 1.1.2**: produce with the harness the references for Angel Falls,
   the village tasks and the end of the prologue, then replay them through
   `scripts/parity.ps1`.
3. **Door cadence**: `Field::step` at 30 Hz (event 17 in 8 VBlanks) remains
   **[hypothesis]**; melonDS measurement of 4 vs 8 VBlanks still to do (`re/movement.md`).
4. **End-to-end browser certification**: continuous session on Pages up to
   Angel Falls and the village tasks, by hand (the driver is not a product criterion).
5. **J4, battle**: battle 26 loot must come from its own rule to clear `4135`.
6. **Cast, not modeled**: the fade and black screen of task 0x30, the teardown of the
   cast during the cutscene, the `questorder3.bin` list, what `021a2fa0` rebuilds in addition
   (monsters, objects: **[unknown]**); `TestFlags00(0x1000)` assumed zero. No original state
   at 4104 in 1.2 yet confirms NPC 99 without a map change.

## Evidence and comparison

Local evidence: `build/evidence/<batch>/manifest.json` (`scripts/evidence.ps1`). Parity:
`scripts/parity.ps1`; diagnosis: `scripts/compare-runs.ps1`, `dq9re state diff/watch`.
