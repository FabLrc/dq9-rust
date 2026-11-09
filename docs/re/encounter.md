# Roaming monsters and the encounter they start

Implementation: `crates/dq9-core/src/encounter.rs` and `encounter/{tables, graph, pick, roamer}.rs`;
real-ROM checks `crates/dq9-core/tests/encounter_rom.rs`. Data: `encfld.bin`, `encbtl.bin`,
`fld_mondata.bin` (`battle-data.md`), `encmons.bin`, and `data/pack_lv5/path.gp2` (new).

**Status of the evidence.** Every statement below is a **reading of the code** (Ghidra, EU
addresses; overlay addresses are the USA's, `external-resources.md`), checked against the ROM
tables where a count is given. **No melonDS state was produced for this batch** (turn cap
reached before the harness was set up on a field), so nothing here is *fact with an original
state* in the sense of `AGENTS.md`: the rules (a) and (b) are **[hypothesis]** until a state of
`F02`/`F38` (`demo.sav`, `C01` exit) reads the roamer objects and compares spawn positions,
counts and zone. The code reading is complete for the spawn rule: the function is one piece of
code, not an inference from data.

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
"pair" is day/night. **[hypothesis]** for the whole rule until a state confirms the picks (below).

## (b) Spawn (`0x020733e8`, called for the current map each update)

Single player; time in ms (`GameState::GetTrueDeltaTime`, 33 at 30 Hz):

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

- **Oracle**: spawn counts, positions and zone on a melonDS field state (`C01` → `F02`/`F38` from `demo.sav`);
  the leader's node (`+0xb8`) is recomputed each update here (**[hypothesis]**; the game sets it at map entry, other writers not found).
- Roamer movement after the state is entered is `Character_Update` with the roamer's constants; whether a roamer slows when turning like
  the hero, the wall test of `0x020322c0`, and the alpha steps of a fade are **[hypothesis]**. The `(byte)param_1[0xd] > 4` clause of the spawn,
  the story-flag range of `0x02073d54`, A-button contacts (`+0xbe == 2` objects), multiplayer, solid monsters (`+0x17d & 6`) are not modelled.
- `func_02028638` (the nearest node used when a roamer stops running) is approximated by the nearest node.

## Symbols

The names this note establishes are in `re/symbols.txt`, section "Roaming monsters and
encounters" (`0x02032380 RandBelow` in the battle section).
