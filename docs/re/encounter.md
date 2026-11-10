# Roaming monsters and the encounter they start

Implementation: `crates/dq9-core/src/encounter.rs` and `encounter/{tables, graph, pick, roamer}.rs`;
real-ROM checks `crates/dq9-core/tests/encounter_rom.rs`. Data: `encfld.bin`, `encbtl.bin`,
`fld_mondata.bin` (`battle-data.md`), `encmons.bin`, and `data/pack_lv5/path.gp2` (new).

**Status of the evidence.** The rules below are a reading of the code (Ghidra, EU addresses;
overlay addresses are the USA's, `external-resources.md`), checked against the ROM tables where a
count is given, **and since 2026-11-10 against the roamer objects of the original in RAM**: states
of `F02` (Western Stornway Region) taken from `demo.sav` by real key presses in melonDS
(`scripts/melonds-field.ps1`, `melonds-state.md` § "Field states"), with every spawn, state change
and contact logged frame by frame. Two independent sets of runs are read: the author's (`F02`, 257
spawns, four phases) and a challenger's with other seeds and the second map (`F02`: 202, 222 and
100 spawns; `F38`: 388, 56 and 22; report in `build/evidence/j4-roaming-challenge/report.md`,
local). Each claim's verdict, its evidence and the command that reproduces it are in the last
section, **"Claims checked"**; a statement in the body that is not repeated there is still a
reading of the code.


Names are the USA decompilation's `func_xxxxxxxx` (USA address; EU is +`0x10` in the ARM9
module). Object fields are offsets of the 0x198-byte monster object.

## Where the monsters live

`func_02027ca4` returns the **map slot array**: four slots of `0x318` bytes (`+0` map id,
`+2` flags: low two bits the slot's group, bit 2 in use, bit 3 loaded; `+6` spawned count, `+8` the
spawn timer in ms, `+0xc` "has a graph", `+0x18` the spawn graph, `+0x44` the model list,
`+0x60` the `encfld` zones, `+0x124` the `encbtl` zones, `+0x2f8`/`+0x304` the name records
and `fld_mondata` records of the map's monsters). Each slot owns twelve monster objects,
`GameState` objects `0x70 + 12 × group + i` (`func_02028a54`). A free object has `+2 < 0`.

`ov_017::021b478c` → `021b498c` → `021b4a88` → `021b4dc8` → `021b4f48` → `021b5070` → `021b5250` →
`021b5348` → `021b5458` → `021b5648` is the loader chain (one background load per state):

| state | file | read by |
|---:|---|---|
| 0, 1 | `data/pack_lv5/path.gp2` member `<code>P00.bin` (code of 5 characters or less) or `<code>.bin` with the 4th character replaced by `P` (`D01M02` → `D01P02.bin`) | `021b498c`: the **spawn graph** |
| 3 | `data/prm/encmons.bin` | `021b4dc8`, handler `0x0209bee8`: per map, up to 12 monster numbers (two per value) = the **model list** to load |
| 4 | `data/prm/encfld.bin` | `021b4f48` → `0x0209bca8`, handlers `0x0209ba2c`…`0x0209bb7c` |
| 5 | `data/prm/encbtl.bin` | `021b5070` |
| 6 | `data/prm/mons_dmy.bin` | `021b5250` |
| 7 | `data/prm/fld_mondata.bin` | `021b5348` → `0x0206ee90` |
| 8 | `data/prm/mon_moddata.nat` | `021b5458` |

`encmons.bin` (209 records of tag `0x66`: map id, a story range, then monster numbers packed two to a
value: `F01` = 1, 57, 56, 32, 23, 63) is **not a zone table**: it is the list of models to load. `021b5648` is
reached for **grottos only** (it spawns one monster at a fixed point); `021d46a4` and
`021a23e4`'s callers are the multiplayer sync and the monster-lure item. The roamers of a field
are spawned by one function: `0x020733e8` (the `Zone3D`-slot method, called each update).

## (a) Which zone applies where — the answer

The `encfld` zone is **not chosen by position**. A zone is picked **at each spawn**, from two
things of the spawn point and one of the clock (`func_02073ec4`, EU `0x02073ed4`):

1. **the hour** (`GameState::GetTimeOfDay`, night 0, morning 1, day 2, evening 3, `lighting.md`): a zone
   of kind 0 (zone word bits 0–2) is eligible when it is **not night**, kind 1 **only at night**, kind 2 always;
2. **the spawn node's mask** (byte `+1` of the graph node): among the eligible zones, those whose
   **region mask** (word bits 13–20, `(word & 0x1fffff) >> 13`) is 0 or shares a bit with the node's mask
   are preferred; with none, any eligible zone. One is drawn with `func_02032370` (`rand`).

(The wiki's "not established" and its ruled-out candidates — collision attribute, night pieces, the
`0x69` record's other values — are consistent: none of them is read. `Time-Of-Day.md` already
gave the hour part.) The monster is drawn in the zone by **weight** (`func_02073fdc`: bits 12–14,
cumulative, one `rand` draw). Zone word bits 21–24 are a **wait**: the zone spawns only when the
map's timer holds at least `(7 − n) × 1000` ms (`n` = 7: every second). Bits 10–12 are the
**party limit** of a battle fought there (5 when the zone is missing; `0x0209afe4`).

At battle time the zone is chosen again (`func_0209bdc8`, EU `0x0209bdd8`) with the hour and the
**ground's region mask** (`func_02099a68`: the stage code under the party looked up in the map
list; bits 2–5 of byte `+0xe` give `v`, mask `1 << v`, 0 for `v = 8`; an unlisted stage gives 0
and no zone): the first zone of the right kind **sharing a bit** with it; none → the roamer's own zone.

`F01`'s three zones: 12 (kind 0, by day), 14 (kind 1, by night), 15 (kind 2, always) — the old
"pair" is day/night. **Confirmed on `F02` and `F38`** (≈1,250 spawns over seven runs, 0 violations):
see "Claims checked", (a).

## (b) Spawn (`0x020733e8`, called for the current map each update)

Single player; time in ms (`GameState::GetTrueDeltaTime`: 33 ms at 30 Hz, **50 ms when an update
runs long**, both measured in the field runs):

0. **Nothing spawns until the party has stepped once since the map was entered**
   (**[hypothesis]**: on a fresh `F02` arrival nothing spawned for 40 s of standing, and the first
   spawn came 2 frames after the first position change, although the slot's timer was already past
   40 s; the cause is **[unknown]**, `+0x160` below is compatible. The model keeps it as
   `Roamers::stepped`). No other rule depends on the party standing.
1. `timer += dt`; stop unless `timer ≥ 1000` (the timer is *kept* when a later check fails; reset to 0
   on a spawn). The map needs a graph, a free object, and not the story flag range of
   `0x02073d54` (not modelled).
2. Count the map's visible roamers in the box `±0xf000` (x, z), `−0x800…+0x1800` (y) around the
   leader: **three or more → no spawn**.
3. The leader's node (`Object+0xb8`, an index) must exist and have links.
4. **Candidates**: the free nodes linked to it (a node is taken when a visible roamer's node has its id,
   `0x02073d68`), plus the leader's own node when it is more than `0x5000` (5.0) away and free.
5. **Pick**: the candidate with the greatest dot product (> 0) between the leader's facing
   `(sin θ, 0, cos θ)` and the unit vector leader → node (y included): **ahead of the party**. None ahead →
   `rand() % count` (the C `rand`); no candidate → no spawn.
6. If the node is farther than `0x5000`… precisely when it is not the leader's node and more than `0xa000`
   (10.0) away, the position is brought **onto the segment node–leader's node, inside the circle of radius
   10 around the leader**: `d` = distance of the leader to the segment (`func_02031468`), fail if `d > 0x9fff`;
   half-chord `√(1 − (d/10)²) × 10`; if above 5.0 the point is `closest + dir × min(|node − closest|, chord)`.
7. Reject when the point is within `0x3800` (3.5) of the leader, or a visible roamer lies in
   `±(0x3000, 0x5000, 0x3000)` of it, or the node's flag byte has bit 1.
8. **Zone** by (a) with the node's mask, **wait check** `(7 − n) × 1000 ≤ timer`, then the **monster**
   by weight. At most one attempt per update for a party of one (a second pass excludes the leader and fails).
9. Spawn (`ov_017::021a2128`): the object at the point (height from `Zone3D_GroundHeight`), facing
   the leader, scale `0x10a × size` (size = the `encfld` second value: float × 4096 or integer << 12, 0 = 1.0),
   collision radius `name.radius << 2`, height `name.height`, node `+0xb8` = the node's index, `+0x164` its
   id, **state 0**.
10. **Despawn** (`ov_017::021a1ad0`): a roamer more than `0x19000` away on any axis from every party
    member is hidden, except those in a fight (states 8, 9). **No respawn timer other than the 1 s tick**; at most
    twelve per map.

The graph: tagged table, `0x68` the node capacity, `0x64` a node (id, mask, x, y, z as floats, kept as
integer world units), `0x65` an edge, `0x66` a chain of ids, `0x67` a run `a..b`; links are built in edge
order (`0x02028214`). `F01P00.bin`: 79 nodes (masks 1 and 2), 139 edges; the Hexagon floors
`D01P02`–`D01P04` 14–23 nodes. `S01M01`, `D01M01`, `D01M05` have no `encfld` group: nothing spawns.
**Night/day**: only through the zone kind. No other time test.

## (c) Roamer movement (EU `0x02076b68`, USA `func_02076b58`; update tables `0x020e893c`/`0x020e89a4`)

Each update: `state_ms += dt` (`+0x14c`), `awake_ms += dt` unless state 0 (`+0x154`), notice test, then the
state's update. `fld_mondata` value 3 is **five 3-bit fields** (`0x020795f8`): wander `+0x138`
(bits 0–2, forced 0 when the walk class is 0), walk class `+0x139` (3–5: speed 0, `0x9a`, `0xe6`, `0x1c2`),
vision `+0x13a` (6–8), flee type `+0x13b` (9–11), attack type `+0x13c` (12–14). **Value 4 is not a speed**:
`func_02077a20` returns it ×4096 as the monster's *size* in the touch radius (below). Values 1 and 2
are stored as a **byte** (`+0x140`) and a signed byte (`+0x144`): the run-away test compares `+0x140` with 99
and −99, and −99 never matches a byte, so metal monsters have no special flee test **[hypothesis]**.

| state | meaning | rule |
|---:|---|---|
| 0 | appearing | waits 1001 ms, then 1 |
| 1 | standing | after 4001 ms in the state (and not waiting for a standing leader): wander type 0 stays; 2 picks a node and **blinks** (3); 1, 3, 4, 5 pick a node and walk (2) |
| 2 | walking to node `+0x164` | heading to the node; arrives within 1.0 (xz) → ground, state 1; a wall → state 1 |
| 3 | blink | fade out 250 ms, move to the node, fade in, state 1 |
| 4 | chase | faces the leader at speed `0x1c2`, turn `0x97`; gives up at 30.0 (`0x7800`), or touches |
| 5 | dash | runs at the point 10.0 ahead of the leader (taken on entering); ends within 1.5 |
| 6 | flee | runs 10.0 away, state 1 |
| 7 | vanish | walks 5.0 away, fades out in 125 ms, gone |
| 8 / 9 | touched / the one fought | stands, faces the party |
| 10 | noticed | after 226 ms decides |
| 11 / 12 | defeated / startled | fade out / stands 3 s then resumes wandering |

**Notice** (`0x020790bc`, states 1 and 2, awake ≥ 2000 ms): vision 1 / 2 = within 3.5 / 7.5 units and within
`cos > 0xb50/4096` (45°) of its heading; 3 / 4 = all round within 3.5 / 7.5. **Decision** (state 10):
`3 × (attack + defence)` of the monster less than the leader's (`+0x34 + +0x36` of its battle data, an input
here) and level byte ≠ 99 → **flee** by type (0 startle, 1 flee, 2 vanish, 3 either by the parity of `rand`);
otherwise **attack** by type (0 dash, 1 chase, 2 flee/vanish by parity). Wander node choice
(`0x02078e70`): the linked nodes whose mask is 0 or shares a bit with the zone's, not taken; one by
`rand() % n`; first stand: its own node if free.

## (d) Contact (`ov_017::02196430`, per frame, `02196fc8` the distance test)

For the twelve objects in slot order, a visible, fully opaque (alpha 31) roamer not in the four-id
"recent" list (`+0x44b8`) and past state 0 is **touched** when the leader's centre is within
`(size/2) × (scale × 15) >> 12` (rounded; `scale` = the object's scale, `size` = value 4 × 4096 as above) plus **half the leader's
radius** (`0x1000 >> 1`) on x, on z and in distance, and not on it. The first touched stops the loop; the roamer goes
to state 8 facing the leader. The facing test measures `|signed(facing, atan2(bearing))|` against the float
**`0x460cbe66` = 9007.6**: the engine's angles are `fx32` radians (π = `0x3244`), so this is **0.7 π
(126°)**, not the wiki's 49.48° (a reading in 65536ths). A monster whose bearing to the leader is
more than that off its heading has its **back turned** (case 1); else a leader whose own heading is more than that
off the bearing to the monster is **caught from behind** (case 2); else face to face (0).

**Surprise roll** (`ov_017::021970a0`, draws: the C `rand` through `func_02032370(100)`): with `D` = the highest deftness
(`+4 & 0x3ff` of the party member record) the party wins when the draw is below `int(2.0 + D × 0.05 (+10 if case 1))`
(not asked in case 2), then the monsters when below `int(2.0 (+10 if case 2))` (not asked in case 1): two draws face to face
when the first misses. Monsters 38–40 never surprise (`0x021d6438`: bytes 38, 39, 40). The result becomes the opening
(`[battle+0x8e49]`, `ov_017::021b848c`): 1 party first, 2 monsters first.

## (e) The company (`ov_017::021b7284`, `0x0209afe4`)

1. The zone: (a)'s battle pick; its `Surroundings` mask from the ground stage.
2. A **scripted company**: the roamer's record in its own zone's `encbtl` list has a second value
   (13 records: `0x198b`… = chance 25 / 50 / 75 %, `eventbattle.bin` number 139–143); the draw `rand_below(100)`
   is taken whenever the monster is found (`func_ov017_021b8a34`); a hit uses that preset (not read here: `Surroundings::event_company`).
3. Otherwise the draw on the **world generator** (`NextRandomMax`): the touched monster's count `least + next(most − least + 1)`
   with the roamer word's bits 12–14 / 15–17 (**not** a weight and least as the wiki had it), then the number
   of kinds from bits 18–20 / 21–23 / 24–26 (weights for one, two, three kinds: `r = next(w1+w2+w3)+1`),
   then for each other kind a company word of the battle zone by weight (bits 12–14) and a count
   `least + next(most − least + 1)` (bits 15–17 / 18–20), then the shrink: while the total exceeds the party limit
   (zone bits 10–12) subtract `max(excess / 3, 1)` from each count in turn (the first stays ≥ 1), then the order
   (two kinds: a coin; three kinds: one of the six permutations of `0x020e8e3c`). Monsters 38–40, or a roamer with no record, come alone (count 1).

## (f) The stage (`func_0204bd7c`, `func_02033fa0`)

The record under the leader (`Character+0x114`, the ground ray's triangle: high seven bits of its attribute word) names the stage
`30000 + 100a + 10b + c` from its first halfword's three five-bit digits. `ov_000::02166880` puts **30116** in its place when
the map list has no such map or the code is 30000. **Checked on the ROM** (`encounter_rom.rs`): `F01`'s floor records read 30116
(714 triangles) and 30117 (112). Which of several stacked floors the game keeps is **[hypothesis]** (the highest).

## (g) The others in the fight

The geometry is known (`ov_000::021643d4`, `0x02164600`–`0x02164710`): each other monster of the battle is moved along the line to the
touched one until it is `(r₁ + r₂)/2 + 0x1000` from it, ground-clamped, and turned to face it; the party's field places are
kept and restored. **[unknown]** how the list of "others" is built: the encounter check stops at the first roamer
touched, and the list read at `+0xf78` of the battle context is not filled by any code reached; `Contact::others` is empty.

## Corrections to the wiki's `Encounters.md`

- a roamer's `encbtl` word: bits 12–14 / 15–17 are the **least and most** of the touched monster, 18–26 the kinds' weights;
  the "second value" holds a scripted-company chance and number;
- value 4 of `fld_mondata` is a size, not a speed; value 1/2 are bytes;
- the 49.48° is 126° (`0.7 π`);
- zone word bits 10–12 (party limit), 13–20 (region mask), 21–24 (wait) are **read**; bits 3–7 and 8–9 are not read by any code found.

## Not modelled / open

- **Oracle**: done on `F02` (night, morning, day, evening; "Claims checked") and, by the
  challenger, on `F38` (map 20038) over four phases. Not done: `F44` (its zone 282 holds monsters
  of level 29–66) and every map met by a party other than level 91, so states 4, 5, 11, 12, the
  chase and dash speeds and the losing side of the notice decision never occurred; the surprise
  roll and the facing threshold; the company; the others in the fight.
- The leader's node (`+0xb8`): the model recomputes the nearest node each update; the game's own
  value equals it on 89–91 % of the spawns (228/257 author's; 179/202, 208/222, 353/388, 91/100,
  53/56 challenger's), so **the game keeps it differently** (**[unknown]** how: it changes while
  walking, not at each update). The spawn is reproduced with **the game's node**: on every spawn of
  both runs (see "Claims checked", (b)).
- **A standing party spawns** (272 spawns with no input at all over one challenger run, the longest
  after 67 s), so there is no rule against standing still. What the data shows is **the first step
  after a map is entered**: on a fresh `F02` arrival nothing spawned for 40 s of standing, and the
  first spawn came 2 frames after the first position change, although the slot's timer was already
  past 40 s. Cause **[unknown]** (`func_02073dfc` tests the spawn point against each party member's
  position or its `+0x160` vector; the `(byte)param_1[0xd] > 4` clause is unmodelled). The model
  waits for the party's first step (`Roamers::stepped`) and has no other standing rule.
- Roamer movement after the state is entered is `Character_Update` with the roamer's constants; whether a roamer slows when turning like
  the hero, the wall test of `0x020322c0`, and the alpha steps of a fade are **[hypothesis]**. The `(byte)param_1[0xd] > 4` clause of the spawn,
  the story-flag range of `0x02073d54`, A-button contacts (`+0xbe == 2` objects), multiplayer, solid monsters (`+0x17d & 6`) are not modelled.
- `func_02028638` (the nearest node used when a roamer stops running) is approximated by the nearest node.

## Symbols

The names this note establishes are in `re/symbols.txt`, section "Roaming monsters and
encounters" (`0x02032380 RandBelow` in the battle section).

## Claims checked

For a **third** reviewer who has not read the reasoning. **Subject**: the roaming-monster rules
above, on `F02` (map 20002, Western Stornway Region) and `F38` (map 20038), from `demo.sav` (party
level 91, so the monsters met are weak: they flee, vanish or are touched; none chased). **Method**:
the original runs in the melonDS DS libretro core, keys are pressed frame by frame (no write to its
memory); the twelve roamer objects of the map slot are read every frame (`melonds-state.md` §
"Field states"; reader `crates/dq9re/src/roamers.rs`). The logs have one numeric line per event: `S`
spawn, `T` state change, `C` contact (columns in the comments of `tests/encounter_rom.rs`), and —
in the challenger's runs — `frames.tsv` (the slot's timer each frame, hence the update's `dt`) and
`near.tsv` (every roamer within 3.0 on x and z, with its state). The model is the code in
`crates/dq9-core/src/encounter*` (`spawn_candidates` and `plan_spawn` are the spawn's node list and
point, `touches` the contact test — the functions the game's tick and the tests both read).

**Two independent runs.** The author's: `F02`, `-Seed 4`, random walk (idle 0.15), 60 000 frames
from clock 45.9 s, four phases: 257 spawns, 1197 state changes, 19 contacts. The challenger's:
`F02` seeds 17, 5 and 11 (202, 222 and 100 spawns), `F38` seeds 77 and 31 (388 and 52, the second
abandoned at a game freeze) and two patrols (56 and 22), with its own watcher (same keys, superset
of the logs). Its report, scripts and logs are in `build/evidence/j4-roaming-challenge/`
(git-ignored, local) — every verdict below was re-read on those logs while writing this section.

**Reproduce everything** (needs the ROM, `tools/build/melondsds_libretro.dll`, `build/sav/demo.sav`
and a release `dq9re`; ~3 minutes; the run is deterministic: two runs gave byte-identical logs, and
main RAM images that differ in two bytes at `0x023ffded`):

```powershell
cargo build --release -p dq9re
powershell -ExecutionPolicy Bypass -File scripts/melonds-field.ps1 -Out build/field
$env:DQ9_FIELD_DIR = (Resolve-Path build/field/run).Path; $env:DQ9_FIELD_REPORT = 1
cargo test -p dq9-core --test encounter_rom -- --nocapture
```

Evidence kept under `build/evidence/j4-roaming-oracle/` (sha256, first 12): `w.tsv` `ff9c9ceb1c75`
(the log), `recipe.txt` `543bc7cc8491` and `recipe.log` `3c97bbdb1aa3` (the route to `F02`),
`states-index.json` `1e525e98c84a` (20 states: ten RAM images and their core states, all map 20002),
`roamers-<state>.txt` (e.g. `roamers-c250.txt` `712a169c220a`), `battles.png` `13c77bfd3a23` (the first
screen of each of the 19 fights), `field-test.log` `020ca8789633` (the tests' output). Run length: 60 000
frames from the day clock 45.9 s (night), through 180 s (morning), 210 s (day), 390 s (evening) and
420 s (night again). The challenger's, same base (sha256, first 12): `report.md` (the verdicts),
`runA-F02-seed17/run/w.tsv` `b77d83ff033b`, `runC-F02-seed5-wait/w.tsv` `d6c52ae20f11`,
`runF-F02-seed11-ticks/w.tsv` `201f6dbd0084` (and its `near.tsv`/`frames.tsv`), `runB2-F38-seed77/w.tsv`
`3be6a3f35b06`, `runD-F38-patrol-east/w.tsv` `b0d920ca0bd8`, `runE-F38-patrol-ticks/ticks.tsv`
`1ed7b0919192`, `runG-F02-stand-push/frames.tsv` `c080b29a277b`, and its analyses `analysis-all.log`
`0b8d59bebce8`, `analysis-box-metric.log` `0f89b71312ed`.

The tests read `F02`'s map and graph, so the challenger's `F38` runs were read with a copy of the
test pointed at `F38P00.bin` and map 20038 (not in the repository); only its numbers are cited
here. The four runs the repository's test reads are the author's and the challenger's A, C and F:

```powershell
$env:DQ9_FIELD_DIR = '<build/oracle/field-F02/run | …/runA-F02-seed17/run | …/runC-F02-seed5-wait | …/runF-F02-seed11-ticks>'
cargo test -p dq9-core --test encounter_rom
```
All four give 11 passed; this batch's logs are `build/evidence/j4-roaming-fix/test-<run>.log`.

| Claim | Verdict | Evidence |
|---|---|---|
| (a) the zone is drawn per spawn from the hour and the node mask | **confirmed**, two maps, two branches untested | 0 violations over ≈1,250 spawns: author 257 (night → zones 2 and 3, kind 1 and 2; phases 1–3 → zones 0 and 3, kind 0 and 2), A 202, C 222, F 100 (`F02`: night on 81–89 mask-1 nodes all to zone 2, never a kind-0 zone at night nor a kind-1 one by day), B2 388, D 56, E 22 (`F38`: zones 36 kind 2 mask 3 and 37 kind 2 mask 4 — zone 37 only on mask-4 nodes and never zone 36 on one; the zone-37 `(7−4)×1000` wait respected). The monster is always in the drawn zone's list and the zone id is the object's `+0x168`. Not exercised: every spawn had a zone meeting the node's mask, so the "none → any eligible zone" fallback never ran; `F38`'s zones are all kind 2, so the hour rule was only discriminating on `F02`; the zone's monster weights and the draws' distribution were not tested (membership only) |
| (b) spawn tick, node, point, count limit | node and point **confirmed given the game's node `+0xb8`**; tick **weaker** | The node is among `spawn_candidates` and the point equals the model's (the node, or the point brought onto the path) to 2 `fx32` on **every** spawn when the game's own node is replayed as the draw: 257/257, 202/202, 222/222, 100/100, 388/388, 56/56, 22/22; the most-ahead candidate is taken whenever one is ahead (201/201, 143/143, 143/143, 253/253, 31/31) and no spawn went behind the party while one was ahead; the leader's own node is taken only above 5.0 in **3-D** (11 spawns over the runs have x-z ≤ 5.0 and 3-D > 5.0, none the reverse); 0 spawns within 3.5 of the leader, 0 in another roamer's box, 0 with three roamers in the leader's box. Tick: spawns only at slot-timer updates whose timer reached a second (min reading 990 with `dt` 33 = 1023, none below), but the step is 33 **or 50 ms** and the threshold is only bounded to (990, 1023]; and the model's conditions are **necessary, not sufficient**: on `F38` run E 39 updates allowed a spawn and 17 did not — one episode of 17 ticks (leader at y −0.89 on node 11, its own node the only candidate, two roamers near) where the game did not spawn; the unmodelled clauses are the probable cause. The earlier "2 of 257 [unknown]" and its 2 % tolerance were a flaw of the test's draw 0, not a game behaviour: the game's node reproduces all of them |
| (c) roamer states | **confirmed** for 0, 1, 2, 3, 6, 7, 10 with each update's own `dt`; **[hypothesis]** for 4, 5, 11, 12 (never seen) | 0→1 only at an update with reading + `dt` ≥ 1001 (author 252 changes, readings 974–990); 1→2/3 only with reading + `dt` ≥ 4001 (author, A, B2, C, D: 0 violations — the literal 3960 ms readings of B2 and C were 50 ms updates, 3960 + 50 = 4010); 3→1 after 462 ms (every one of 56); 10→6/7 at reading 198 (every one); the post-update values are 1007–1023 and 4010–4059, so the thresholds are only bounded to **(990, 1007]** and **(3993, 4010]**: consistent with 1001/4001, **not pinned** to them. "6→1 after 1386–1452 ms" holds **only for the author's run** (17/17): the challenger measured A 18/20, C 29/32, B2 90/113, D 12/17, the others shorter (792, 957, 990 and 0 ms): a flee ends when its 10.0 run is done **or on a wall**, not at a fixed time (the model ends it on the distance; test `a_flee_ends_on_its_run_not_on_a_fixed_time`) |
| (d) contact | **confirmed**, radial, necessary and sufficient at one update | ≈100 contacts (A 23, C 15, F 12, B2 30, D 40 of 13 distinct after the replays, E a subset): the leader is inside the model's `touches` (the roamer's `touch_reach` plus half the leader's, on x, on z and in distance) at the frame the roamer entered state 8 — 0 outside, with no slack (`logged_contacts_are_within_the_models_reach`). The converse, from `near.tsv`: no roamer of state other than 0, 8 or 9 stays in that reach more than one update (2 frames) without a touch — 0 episodes in D, B2, F; 1 in C, a roamer already in state 9 (the one fought, outside the claim); the touch follows the first in-reach row by exactly 2 frames. The box test (|dx|, |dz| ≤ reach) is **not** sufficient: 7 in-reach untouched episodes of 65 in D, 12/236 in B2, 3/94 in C, 1/41 in F. The author's log has no `near.tsv`, so only the first half runs on it. **[hypothesis]** for the 126° facing threshold and the surprise roll: of the author's 19 first battle screens, 6 show a party-first message ("doesn't notice the party's presence", "is too stunned to move") and none a monsters-first one; the other screens had not shown theirs |
| (e) company | **[hypothesis]** | all 19 fights of the author's run show one monster (`battles.png`); the challenger's fights likewise; the model's draw for these roamers was not compared |
| (f) stage | **[hypothesis]** | not checked on `F02` or `F38` (the screens show the field backdrop) |
| (g) others in the fight | **[unknown]** | no fight had a second monster, so there was nothing to pull in |

**What was not tested.** `F44` (monsters of level 29–66 in its zone 282) and any party weaker than
level 91: every contact was with a level 6–12 monster, so states 4, 5, 11 and 12, the chase and
dash speeds and the losing side of the notice decision were never observed. Not tested either: a
phase change exactly on a spawn tick; the fallback zone choice when no zone meets the node's mask;
the zone and monster weights and the `rand()` distribution; a party of more than one member and
multiplayer; the 126° facing threshold; the surprise roll; the company, the stage and the others
pulled into a fight; the cause of the 17-tick `F38` no-spawn episode and of the first-step effect
after an arrival (only bracketed: it ends with the first step); the exact threshold constants
(1001/4001 against anything in (990, 1007] and (3993, 4010]).

What would refute them: (a) a spawn at night from a kind-0 zone (or by day from kind 1) in a run
of `scripts/melonds-field.ps1`; (b) a spawn on a node that is neither linked to the leader's node
nor the leader's own, or that is behind the party while a node is ahead, or a point different from
the model's when the game's node is replayed as the draw; (c) a change out of state 0 at an update
with `state_ms + dt < 1001`, or out of state 1 with `state_ms + dt < 4001`, `dt` being the frame's
own (`frames.tsv`); (d) a contact with the leader outside `touches`, or a roamer of state other
than 0, 8 or 9 inside that reach and not touched after two frames. Try different seeds (`-Seed`),
and start `F38`/`F44` the same way (the recipe only needs a different door).

## Application wiring (j4-roaming-play)

`crates/dq9-app/src/app/roaming.rs` draws the roamers of the loaded map and hands their contact to
the existing battle entry (`App::start_encounter`). It is wiring, not a second model: the spawn tick,
movement, notice decision, contact, surprise roll, company and stage are `dq9_core::encounter`'s.

- **Lifecycle.** `enter_roaming` runs after every map load. A different map starts a fresh
  `Roamers` (timer, spawn count, graph and generators); a reload of the same map (time of day, a
  scene rebuild) keeps the `Roamers` and drops only the scene indices of the field models, because
  the new scene's instances are different objects. A new ROM, save or new game clears the cached
  ROM tables. The step runs only on the field's 30 Hz update, after the field task and before the
  animation; the field loop already skips it while a conversation, an event, a cast reload, a map
  change or a battle holds the field.
- **Models.** One field model is added per monster number of the map's `encfld` zones at load
  (`enemy/<code>_f.mon`, first `.cchr`), from that number's `mon_data` code, never another
  monster's archive. A spawn takes a free instance from that number's pool; the model is placed at
  the roamer's `position` and `angle` with scale `scale / 4096`, its motion follows the state
  (`stand`, `walk`, `run`, `appear`; a missing name falls back within the same archive), and its
  alpha is `GetInheritedAlpha`. A model added after the scene was uploaded forces one renderer
  rebuild; nothing else does.
- **Generators.** One live stream is shared by the field and every battle. A map's `Roamers` are
  seeded from the application's `World` (so `--seed` reaches the spawns and companies); every field
  step copies the roamers' `CRand` and world generator into `World`, a battle starts from that state
  (contact or scripted), and `App::settle_roaming` copies the advanced state back through
  `Roamers::set_generators`. No pre-battle draw is replayed after a map change or a scripted battle.
- **Settlement.** A contact stores its slot. A won monster enters state 11 and fades out; the
  engine's own update drops the slot at alpha 0 and the application releases its field model. A
  fled or lost monster enters state 12 (STARTLED): it stands three seconds and resumes wandering.
  The touch that starts the battle also puts the roamer's id at the head of the four-entry recent
  list (`+0x44b8`), so the same roamer is not touched again while it is recent. The party, drops
  and story triggers are settled only by `settle_battle`, so a contact pays once. **[hypothesis]**:
  the field log only has won contacts (each followed by the slot's next spawn), so the fled/lost
  resumption is the engine's STARTLED rule, not a measured post-battle state; filling `+0x44b8` at
  the contact is the only place its four ids can come from without inventing a second writer.
- **Party numbers.** The notice decision and the surprise roll read the first living member's
  attack plus defence and the highest deftness among living members from
  `Character::fighter` with the cached `ItemStats` and `BattleParams`: level table and worn
  equipment both count. The level tables are applied at the first contact (or fight), so a new
  game's empty attributes are filled first.
- **Scripted companies.** `Surroundings::event_company` is the `eventbattle.bin` record the
  `encbtl` roamer word names (its preset bits); the tables are read once with the map tables, so a
  scripted company is no longer silently replaced by the drawn one.
- **Field models.** Every pooled field model's instance matrix is uploaded each frame, including a
  released model's zero matrix: a defeated, despawned or respawned roamer leaves no ghost on the
  renderer. The per-frame path allocates nothing for the twelve slots.
- **Headless.** A run without a window does not roam (its recorded traces keep replaying); the
  window and the Web always do. The native behaviour tests switch it on explicitly.
- **Not modelled** (unchanged from the core): the story-flag range of `0x02073d54`, the
  `(byte)param_1[0xd] > 4` clause, A-button contacts, multiplayer and solid monsters.
