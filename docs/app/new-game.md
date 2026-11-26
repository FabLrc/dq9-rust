# New game (`--new-game`) and automatic run

```powershell
# In the window: the prologue from the start (new game state, default hero).
cargo run -p dq9-app --release -- --new-game
# No window: presses action to read the messages, logs maps, cutscenes, scripted
# battles and story progress, and stops at the first moment the player has control:
cargo run -p dq9-app --release -- --new-game --headless --auto
# Same, with a driver that, at each player control, has the conversation or takes the exit
# that advances the story (cap `--headless-steps`, 200,000 steps by default):
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot
# JSONL checkpoints (same fields as `dq9re state --json`) and frame-by-frame inputs:
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot `
  --checkpoints build/checkpoints.jsonl --trace-inputs build/inputs.jsonl
# Replays the trace without a driver: same inputs, same checkpoints (byte for byte):
cargo run -p dq9-app --release -- --new-game --headless `
  --replay build/inputs.jsonl --checkpoints build/replay.jsonl
```

`--checkpoints` writes one JSON object per player control, map or story change,
cutscene start/end and battle: `tick`, `seconds`, `kind`, `vblank` (= `tick`), `map`, `area`
(numeric area code), `track`, `daytime`, `tracks` (5 × chapter/step/substep and the two
flag sets), global `flags`, `quests` (nibble per quest), `event` (identifier of the current cutscene, null in between) and
`hero` (position and heading). The first checkpoint of `--new-game --headless --auto` is
identical to the melonDS state at the same moment (`X01M05`, 210.42 s, 1.1.1, global flags
`{337, 1450, 1451, 1452, 1453, 1454, 1461, 2721, 2751}`, stage flags `{1}`, hero
`(0.660, -0.250, -3.610)`): the six cutscene flags and the two map flags match; nine
global flags remain unknown (see `docs/re/trigger.md`).

`--trace-inputs` writes the D-pad and action button of each fixed step (`frame`, `pad`
`up/down/left/right`, `a`). `--replay` reads this file back without a driver: each line feeds the
D-pad and action button of one fixed step. The trace of the driver's run (40,432 steps, from the
new game to `M01M08` in 1.2.1 via 1.1.2, `X01M04` and Angel Falls `M01`)
replayed gave **the same 615 checkpoints, byte for byte** (sha256 `c2b10c767640`,
`build/cast-reload/`), which is the evidence of reproducible inputs: the driver no longer reloads
the map when the story advances, it sees the cast that the game's action `0x8d`
rebuilds, like the replay and a player. The driver gives world directions with the camera at
angle 0 and `--replay` forces the same heading: the trace proves the headless driver's evidence in
our engine, not a replay with an arbitrary manual camera nor a melonDS gamepad.

Batch `j4-parity-regress` replaced that open-loop A-mashing trace with the pilot's own closed
loop (`inputs/pilot-inputs.jsonl`, 60,000 steps, recorded on `f516d79`): the pilot walks its
route and presses confirm where it must, so a timeline change no longer strands the replay at a
post — the mashing trace did exactly that after native 560 was implemented
(`build/evidence/j4-parity-regress/report.md`). The test
`a_pilot_trace_replays_to_the_first_control_point` pins that equality on a short prefix: the
pilot's trace replayed through `replay_step` must land on the pilot's own checkpoints, the first
control point included.

Batch `j4-message-flow` moved the route's timeline once more: `<ADD>` messages now wait for the
action key and draw the blinking wait arrow (state 3; `j4-message-arrow` corrected the
"no arrow" note), a Yes/No answer closes its window with the press, and an `<END>` closes 6 steps after its
key (`docs/re/dialogue.md` § "Message pacing"). At that batch the pilot's route kept its 972
checkpoints, story 2.6.5 and the five Hexagoon fights; 966 lines differ only in
`tick`/`seconds`/`vblank`, 6 in the hero's position, and the game time at event 2750 was
**3686.9 s** (was 3627.8 s), sha256 `f33a42d42a95`.

Re-pinned by `794829d` after the warp door task (`1d4e76b`) and the encounter settlement
(`94b95de`): **964 checkpoints**, story 2.6.5 and every battle fought, the Hexagoon lost at
Lv 5, 6, 7 and 8 and won at Lv 9 (175 experience, 100 gold), the game time at the last
checkpoint **3899.58 s**, sha256 `d721c11fd0d2` (`build/evidence/v3-route-after-door/`). Up to
checkpoint 701 only `tick`/`seconds`/`vblank` differ from the previous artifact; the first state
difference is at 702, an extra battle's 25 experience before the Hexagoon — the door task moves
the pilot's step timeline, so the draws on the way move with it.

On the Web: the page's "New game" button (or `?newgame=1`), after choosing the ROM.

**Start.** `GameState::new_game` gives progress 1.1.1, the start of the day and a running
clock; the hero is that of `HeroLook::new_game` (hypothesis, `re/characters.md`) and the first
cutscene is `stage_opening(1, 1)` (`ev21500`, map `M01M12`). Maps are built for the
current trigger progress (`current_story`: track, flags, quests), not for the
`--chapter/--step` options.

**Chaining.** Nothing is "hard-coded": these are the game's mechanisms.

| Link | Mechanism | Where |
|---|---|---|
| cutscene → neighbouring cutscene | native 538 | `Director` |
| end of a cutscene | trigger 11 (word 2 = event): follow-up, story progress (`0x84`), map change (`0x79`, `0xe1`, `0x85`), battle (`0x78`) | `Field::event_ended` |
| end of a scripted battle | trigger 15 / 16 (word 6 = number) | `Field::battle_ended` |
| end of a conversation | trigger 1 (NPC, context, yes/no answer) | `Field::talk_ended` |
| arrival on a map | triggers 20 then 3 | `Field::enter` |
| entering an area, door, warp | triggers 2, 17, 6 | `Field::step` |
| in-place story progress | `0x84`: triggers reloaded (0x8f areas forgotten, trigger 20 again); `0x8d`: cast rebuilt | `Field::collect`, `App::reload_cast` |
| end of a cutscene that sets the cast aside (event list, value 21, bit 0 or 2) | cast rebuilt before trigger 11, pre-cutscene poses and removals restored by the snapshot | `App::reload_cast`, `EventPlayer::cast_loaded` |
| 0x76 outside a conversation | dialogue window of the trigger's line | `App::talk_to` |

**Scripted battles.** A headless run does not play the battle unless `--fight` is given:
`scenario::stand_in` declares it won, which the log states explicitly ("stand-in, no battle
system yet"), so the recorded input traces and the 615 checkpoints of `scripts/parity.ps1`
keep their timeline. Action `0x78` (and native 547 in a cutscene) shows the **battle scene**
first — the stage, the monsters and the party on the grid, under the battle camera (§ [Battle scene](battle-scene.md)) — then the battle is settled and trigger 15 (or 16) starts what follows. In
a window, on the Web and with `--fight` the settling is the played fight of § [Battle](battle.md): the
party is paid (experience, gold, drops) and its HP persists.

**Result** (`--new-game --headless --auto`, EU ROM, English text; game time at 60 Hz):

| Time | Map | Cutscene / fact |
|---:|---|---|
| 0 s | `M01M12` | `21500` "stage_opening" (80 s), then `21507` (43 s) via native 538 |
| 123 s | — | trigger 11 of `21507` → **scripted battle 26** (monsters 290, 292, 290; arena 30116), won by stand-in (`--fight`: played by the engine, § [Battle](battle.md)) → trigger 15 → `21520` |
| 123 s | `F01M01`, `X01`, `X01M01`, `X01M05` | `21520 → 21521 → 21522 → 21596` (natives 538 and map changes requested by the scripts) |
| 210 s | `X01M05` | **first player control** (Observatory, story 1.1.1) |

Angel Falls itself comes after (`--pilot`): the guard's yes answers, 21591
(`X01M05` → `M01`), 21592, then stage 1.2 plays in the village (`M01`, 335 s, story
1.2.1).

**Driver** (`--pilot`, a stand-in for a player, not a game feature): at each player
control it simulates on copies (`Talk::simulate`, `Field::step`) each conversation the
map offers (anchor point and "first" answer chosen by the game's partner
search) and each area of the map; it plays the one that advances the story the most (progress,
event launched, battle, map change, flags) and, failing that, takes an exit of the
map family (`X01`, `M01`…) it has not taken, backtracking if needed.

It **really walks**: at each fixed step it gives the D-pad a world direction, `Hero::step`
turns and accelerates the hero, the map's collision pushes it, and only `Field::step` decides
that an area, a door or a conversation zone is reached. Paths come from a
ground grid built by probing the collision with the hero's ray and sphere
(`CollisionWorld`), multi-level (up to eight floors per column), with edges validated by
walking a hero from cell to cell; a move that gets stuck is learned and avoided.
The action button is pressed when `Talk_NearestNpc` sees the targeted NPC; an internal door without
destination is pushed, opened by the field (native 584, `trigger.md`) and the grid is
rebuilt. Door state is single (`EventPlayer::doors()`): the scene and the collision
read the same angle (`angle_fx`). No teleport enters the evidence.

**Route mode** (`--route <EVENT>`, with `--pilot --fight`; `app/auto/route.rs`): the continuous
route to an event's end. Roaming is on, so the field's monsters can be fought; after a lost
fight the pilot raises the strength it requires before trying again, hunting the field's
roamers and healing through the real systems (the inn when the story opens one, otherwise
medicinal herbs bought at a shop and used from the field menu); it then returns to the point
where the story stopped. Nothing is injected: no state, flag or statistic is forced, and
without the flag roaming stays off and the pilot behaves as before. The run stops when
`EVENT` has ended.

Run accepted on `main` (`ef56959`), European ROM, seed 7
(`scripts/chapter2-route.ps1 -Name accept-2750 -Route 2750 -Steps 3000000`, pid 11008, exit 0,
1900 s wall): battle 26 won in 4 rounds at 123 s; the Hexagoon (set battle 2, `D01` stage
30215) lost at Lv 4, 5, 6 and 7, then **won at Lv 8 in 7 rounds** (175 experience, 100 gold)
at 2637 s; event 15 → 2550 → 2555 → story 2.5.1, then 2620, 2600, 2610, 2700, 22593, 2720,
2730, 22594 and **2750**, `stopped: event 2750 ended (route reached) after 3057.2 s of game
time`, story 2.6.5. Evidence: `build/evidence/j4-route-accept/accept-2750.out` (`9f979a299fc4`),
972 checkpoints, and the trajectory in `build/evidence/j4-chapter2-route/report-route.md`.

Run verified on the European ROM, at 60 Hz (`--new-game --headless --auto --pilot`):

| Time | Map | Fact |
|---:|---|---|
| 210 s | `X01M05` | first control; conversations 1140 and 1130 while walking |
| 295–312 s | `X01M05`/`X01M06`/`X01M07` | real exits and warp 9 |
| 331–334 s | `X01M07` | library door (two leaves 2 and 3) pushed, opened, crossed |
| 334 s | `X01M07` | area 2 (added by event 20) reached on foot → `1150` |
| 356 s | `X01M01` | back through the library door |
| 370 s | `X01M01` | NPC 127, `1170` → story **1.1.2** |
| 378 s | `X01M01` | stop: the warp to `X01M03` (y 15.5) is not reachable on foot |

Precise limit of the J3 driver: on `X01M01` (Observatory tower), the upper terrace was not
connected to the story pose by the walking collision. A **3D** exploration (edges validated by
`Hero::step` with the height reached, climb ≤ 1.0, drop ≤ 10) from the story pose
as from the map's start plateaus at **y 11.25**; the `X01M03` warp is at y 15.5. The
thin wall `X01A01S1` (block 0, placement 12, normal `+x`, `x 0.018..0.105`, `z 15.42..18.99`,
`y 9..14`) separates the terrace from the foot of the ramp. The `0x73` zones of **type 1** (43 in the
ROM, three on `X01M01`) are **inert**: no routine of `Field_CheckRegions` reads their
list, it is neither a lift nor a task 8.

That J3 stop no longer bounds the driver: the walkable-grid and exit-order work of the route
batches crosses the tower, and the route mode above plays from the opening to event 2750 with
real battles. What remains **[hypothesis]**: the west stair of `D01M02` being a dead end, and
the driver itself, which is a stand-in for a player rather than a game feature.

**Stops where and why** (end log of `--pilot`): player control with nothing that
advances the story according to the simulation (conversations that no longer change progress or
flags, areas already taken), or step cap reached. Known limits:

- The driver keeps the battle stand-in by default (`--fight` plays them with the engine, § [Battle](battle.md));
  the party's items and the battles' loot are only modelled with `--fight`, so triggers that
  depend on them (conditions `0x24`, `0x31`, `0x32`…) can give other branches than the real game
  without it: only the hero's opening outfit is set in the state.
- `0x8d` (end of `21510`, `21595`, `21593`, `1340`…) rebuilds the cast of the current
  map without reloading it (`re/trigger.md`, § "Reloads after deferred
  actions"); its fade is not modeled. Natives without effect (`stb.md`) are
  logged but do nothing.
- The driver no longer reloads the map when the story advances: it sees what a player sees
  (triggers reloaded by `0x84`, cast by `0x8d`), and what the replay of its input
  trace redoes identically.
- The driver sometimes replays the same conversation (`2110`) as long as its trigger changes a flag.
- The Observatory's type-1 zones are inert; the missing height connection
  remains to be established (see the limit above).
- Order and durations are not compared with melonDS.
