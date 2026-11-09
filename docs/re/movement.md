# Hero movement and ground

`dq9-core` reproduces the on-foot movement of the party leader as the `ov_017` code computes
it: bit-exact `fx32` arithmetic (4096 = 1.0), one step per game update, no
I/O. Four modules: [`fx`](../../crates/dq9-core/src/fx.rs) (hardware divider and square root,
vectors,
angles), [`movement`](../../crates/dq9-core/src/movement.rs) (`Hero`, d-pad, speed, sliding),
[`collision`](../../crates/dq9-core/src/collision.rs) (`.col2` meshes, grid, floor ray,
sphere against walls) and [`camera`](../../crates/dq9-core/src/camera.rs) (`L`/`R` rotation).
ROM and NARC resolution is supplied by the caller. Map resources come from
[`collision.md`](collision.md) (format); doors, regions and warps are tested by
`dq9_core::field` ([`map-archive.md`](map-archive.md), "Doors, zones and map
changes"), which receives the hero's pose after `Hero::step`.

## What is ported

A `Hero::step(world, keys, camera_yaw, ticks)` follows `ov_017::0218e210` for the party
leader, in game order:

1. **Generic update** (`PartyMember_Update` `0x02037d98` → `Character_Update`
   `0x020332bc`): the model turns toward the wanted heading by `0x324` per tick (exact stop on
   the target, `0x02033720`); speed rises by `0x28` per tick up to `0x189` (or falls back
   toward 0); **the tick's displacement uses the previous tick's speed**, limited to
   `0x189 × (1 − |model/heading gap| / π)`, along the wanted heading (not the model angle:
   the hero's `0x100` flag); finally gravity subtracts `+0xac` from `y`
   (`0x0203349c`).
2. **D-pad read** (`Hero_ReadDirectionInput` `0x02038148`), *after* the movement: a pressed
   key therefore only moves the hero on the next tick. Heading = `table[direction] + camera
   yaw`, table `0x020e90b8`. Diagonals have priority, then up, down, left,
   right (up + down = up). No key: idle state, speed decreases.
3. **Map collision** (`ov_017::0219344c`, see below), in two half-passes
   when the hero has covered more than `0x333` (0.2 unit) during the tick;
   the game gets there at 3 ticks: `0x189 × 3 = 0x497`, while 2 ticks (`0x312`) stay
   below the threshold.
4. **Sliding along the hit wall**: if the hit wall (triangle stored at `+0xe4`) and
   the displacement remain above `0x27`, the wall heading (`FX_Atan2` of its normal) is
   compared to the wanted heading: below ≈ 145° (`10294.4`) that heading is kept; beyond 90°
   (`0x1922`) the heading becomes the wall's ± 6772.6 (≈ 95°); if the actual displacement is
   within 643.4 (≈ 9°) of that heading, the wanted heading becomes it (`0x0218e4a0`–`0x0218e5a4`,
   floats read from the ROM).
5. **Displacement bounds**: a movement below 5 on each axis is cancelled; the
   distance covered after collision does not exceed the one before (`0x0218e5a8`–`0x0218e658`).

`ticks` is the number of frames elapsed since the last game update
(`GameState+0x3c4`, capped at 3, 2 at start): field logic runs at 30 Hz, 2 ticks
per update. The outer loop of `dq9-app` stays at 60 Hz (VBlanks); on each VBlank
`App::run_steps` increments `tick` then runs `Hero::step(..., 2)`, `Field::step`,
the gait and the render instance only on odd `tick`s, the pose being held on the
intermediate frame. Audio, doors, messages, event VM, camera and `runtime.step` stay at
60 Hz. `tick` is the game's free-running VBlank phase: it is reset to 0 only by
`start_new_game`, never by a map load, a save or a return of
control. Steady-state speed is the same (5.76 units/s), and so is the speed ramp-up of
`0x28` per tick.

This cadence is checked on the real ROM by
`field_cadence_holds_the_pose_and_matches_the_original_every_vblank` (`app.rs`): from
the pose `(1.756; −3.041)`, raw angle 0, speed 0 in `X01M05`, odd steps give the
exact raw angles 0, 1608, 3216, …, 12868 and the fx `z` of the original capture, even
steps holding the pose byte for byte (camera included). The first VBlank only reads the
d-pad, the first displacement is on the third field step (VBlank 5) and the U-turn settles
on the target at VBlank 19 (review
`build/v01-20261107/review/cadence-30hz-field-review.md` §7).

**[hypothesis]** `Field::step` (hence `step_doorways`) also runs once per
update, like `Field_CheckRegions`: the door approach counter, at
`APPROACH_FRAMES = 3`, then fires event 17 on the 4th call, i.e. 8 VBlanks. The static call
chain (`0x020d6cb4` → `ov_017::0218b688` → `0218cbd4` → `Field_CheckRegions`) does not
give the main loop cadence; the melonDS measurement (4 VBlanks ⇒ field at 60 Hz,
8 ⇒ 30 Hz) is still to be done. Do not re-gate `field.step` alone without that measurement.

**`--screenshot`/`--frame`** ([`tools.md`](tools.md)) is a preview path outside
`App::run_steps`: it keeps `Hero::step(..., 1)` per requested frame and does not claim to
reproduce the cadence. Game paths (window, Web, driver, replay) all go through
`run_steps`.

**[hypothesis]** scaling discrepancies (`0x28` ground hysteresis, half-pass rounding)
are negligible at this cadence.

### Collision (`CollisionWorld::resolve`, `ov_017::0219344c` / `02192b48`)

- A *piece* is a placed `.col2` (position and angle of the placement). The game works in the
  piece frame: position − piece origin, rotated by −angle (`MTX_RotY33_`,
  `MTX_MultVec33`), vertices = file integers `<< shift`.
- **Triangle selection** (`0x0204c040`, `Collision::query`): query box
  `[x ± radius/2, y − 10 … y + height, z ± radius/2]` (hero radius `0x1000`, height `0x1800`,
  `ov_017::0218f064`); test against the mesh bounds, then grid cells (see
  [`collision.md`](collision.md)) and the box of each triangle (six nibbles of its attribute
  word); at most 0x60 triangles. The box is not rotated with the piece (as in
  the game): **[hypothesis]** floor pieces have no angle.
- **Ground**: vertical ray from `y + 0x1800` to `y − 0xa000` against faces with normal `y >
  0x800`; the highest one (interpolated height), then the exact point on its plane (`0x02032090`,
  12-bit t: a few units of noise on `y`, like the game) + `0x199`. The hero takes the
  highest ground of the pieces if it is less than `0x800` (0.5) below him, or less than `0x1000`
  above; `y` only changes if the gap exceeds `0x28`. A ground more than 0.5 below him does not
  carry him: eight rays at 0.25 unit around confirm there is no nearby ground, the
  hero falls (`+0xac` grows by `0x51` per frame up to `0x1000`; reset to 0 on the ground).
  No ground at all: the hero stays in place (no falling into the void).
- **Walls** (`0x020315c8`): sphere of radius `0x800` centered at `y + 0x800`, against faces with
  normal `y ≤ 0x800`; first pass: segment from the center along −normal (sphere
  radius); if it crosses the triangle, the center is pushed to `radius` from the plane; second pass:
  closest point on the triangle (`0x02031d30`) for edges and vertices. A corner trap
  (nearly equal normals, `0x800 < n·n' < 0xfd7`) sends the sphere back to its previous position.
- The values at `0x021d62d8`, `0x021d62f8` (eight edge rays), `0x020319e0`/`e4`, the
  sliding floats and the heading table are read by `tests/movement_rom.rs`.

### Field camera (`dq9_core::camera::FieldCamera`, `0x020a213c`)

`L` (`0x200`) adds to the yaw, `R` (`0x100`) subtracts, by `trunc(0.0015 × Δms × 4096)` (202 at 33 ms);
the yaw cannot enter the band `(π/4, 7π/4)`: it stops at its edge. `L` and `R`
together bring the yaw back to the base yaw at `0.00225 × Δms × 4096` per update (stop at
`0x28`). The application (`Q`/`E` in orbit) measures the yaw from the one on map entry.
The application initializes the orbit with the constructor values (`0x0202df78`): distance 11,
eye 6 units above the target, vertical field of view 30°. The target follows the hero's position
+ half his height (0.75), like `FieldCamera_Update` (`0x020a213c`). Framing the
bounds of the whole map remains reserved for free flight (`F`); `F` in orbit restores following.

A cutscene keeps the field camera parameters; a map change in the
chain replaces that return point with the destination's. The last cutscene state is
applied **before** restoration, then the target joins the hero at his final pose: no
inactive presentation can reinstall the last close-up. Free flight, for its part, recovers
its viewpoint without following the hero. The projection of cinematic cameras stays separate.

### Out of scope (not ported)

Characters pushing the party leader (`ov_017::0218df50`, `0218dfd8`), 8-triangle colliders
of zone objects (`ov_017::02193290`), maps `0x1131` and `10000`, terrain record copied
onto the character (`02033f6c`), cave mode (`+0x23b8`), vehicle branch of `0x02017da0`,
snapping on oriented zones (`0x0201e90c`), stylus, ladders (`0x020385a8`), events
that take control, animations (states other than idle and walk), widened wall radius of flag `0x20`.

## Starting position

A normal save does not store the hero's position: it gives the map (id `11476`,
e.g. 109 = `C01M09`) and the game places the party at the map's starting point,
record `0x6e` of its `.bmbl` (`Party_Place`, `ov_017::0219c598`;
[`save.md`](save.md) § "Position on resume"). `dq9_core::state::GameState` derives the
pose from it (`hero_pose`) and `dq9-app --save` places the hero there (`Hero::at`), snapped to the ground found
by `CollisionWorld::ground_height` (`0x02018fcc`, `0x199` above, like the game). The game
sets the wanted heading and the model angle to the starting angle (`0x020399c0` → `0x02033874`) and
sets speed to 0: `Hero::place` does the same. A map without a record, a pose from another
map or no save keep the provisional point computed from the collisions
(`CollisionWorld::spawn`). Position and heading equal melonDS's on the demo (109) and on
the same save repointed to map 100.

## Code analysis

Obtained with `scripts/ghidra-dump.ps1` (decompilation of `ov_017` and the ARM9 into
`build/decomp/`). European addresses; names in `re/symbols.txt`.

### Who moves the hero

- `ov_017::0218d8cc` does not move the hero: it builds `field+0x4438` (d-pad direction
  in world space), read by ladders and dialogue.
- The party leader's movement goes through `ov_017::0218e674` (loop over members) →
  `ov_017::0218e210`.
- `0x020a654c` → `0x020a75fc` (keys + `Character_Update`) acts on the object `GameState+0x32c`,
  **null** in the `field.ml1` state: it is not the hero on foot (**[unknown]**: vehicle?).
- Order of `PartyMember_Update` (`0x02037d98`): `0x02052af8` (generic update,
  `Character_Update` via a jump table at `0x02048144`) then `Hero_ReadDirectionInput`
  then `0x020385a8` (ladder).

### Input → heading (`0x02038148`)

- Direction code `+0x250`: Up+Left `0xc`, Up+Right `0xd`, Down+Left `0xe`,
  Down+Right `0xf`, Up 8, Down 9, Left 10, Right 11 (keys `0x40`, `0x80`, `0x20`,
  `0x10`).
- Heading = `table[code-8] + camera yaw`, reduced to [0, 2π]; table `0x020e90b8` in fx32 radians
  (4096 = 1 rad, π = `0x3244`, 2π = `0x6488`): Up π, Down 0, Left `0x4b66` (3π/2), Right
  `0x1922` (π/2), Up+Left `0x3ed5`, Up+Right `0x25b3`, Down+Left `0x57f7`, Down+Right
  `0x0c91`. Convention: direction = (sin θ, 0, cos θ); the camera at yaw 0 looks toward −z.
- With a key: state 1 (walk), `Character_SetFacing` (`+0xae` = heading). Without: state 0.

### `Character_Update` (`0x020332bc`)

Fields: `+0x44` position fx32 ×3, `+0x54` model angle, `+0xac` fall speed,
`+0xae` wanted heading, `+0xb0` rotation speed, `+0xb2` current speed, `+0xb4` max
speed, `+0xb6` acceleration, `+0xbe` state. Initialization values (`0x02032e68`, read from the
ROM; `ov_017::0218f064` writes `0x324` and `0x189` for party members): rotation
**`0x324`** (≈ 0.196 rad / tick), max speed **`0x189`** (393/4096 = 0.096 unit/tick =
**5.76 units/s** at 60 ticks/s), acceleration **`0x28`**.

Speed is read **before** its update (`ldrsh r4, [r8, #0xb2]` at `0x0203355c`, then
written without re-reading); it rises by the acceleration without capping (`0x190` after ten
ticks), then drops back to `0x189` when the excess is less than the acceleration.
Deceleration can go below 0 for one tick (zero movement: displacement requires a speed
> 0). Gravity: `y −= +0xac`; below `−0x32000`, `y` is reset to `+0x32000`.

### Field camera

Camera object: `+4` eye, `+0x10` target, `+0x70` yaw, `+0x74` dy, `+0x78` distance. Default
(`0x0202df78`): yaw 0, dy `0x6000`, distance `0xb000`. `Camera_SetOrbit` (`0x0202e5e8`):
eye = target + RotY(yaw) · (0, dy, √(dist² − dy²)). `FieldCamera_Update` (`0x020a213c`): the
target is the character's position `+0x21c` + height/2 + offset `+0x22c`. Constants
(`0x020a2708`…`0x020a2724`): `0x020a270c` = 0.00225, `0x020a2714` = 0.0015, `0x020a2710` = 4096,
band `0xc91`/`0x57f7`, `0x020a2720` = `0xccc`, `0x020a2724` = `−13516`.

### Arithmetic

`FX_Div` = `((a << 32) / b + 0x80000) >> 20` (hardware divider, truncation; zero divisor: 0),
`VEC_Mag`/`VEC_Distance` = `(⌊√(4 Σ v²)⌋ + 1) >> 1`, `VEC_Normalize` = `⌊√(4 Σ v²)⌋ ×
⌊2⁵⁶ / Σ v²⌋` then `(· · v >> 32 + 0x1000) >> 13`, product and scalar `(a·b + 0x800) >> 12`,
`VEC_DotProduct` and cross product rounded once on the sum, `FX_Atan2` via the
129-entry table `0x020ed460` (`round(4096 · atan(i/128))`, verified), sin/cos via
`0x020e9460`. The piece rotation (`0x020c3260`/`32f8`) is a 64-bit sine/cosine truncated
to `fx32` after float conversion; the port evaluates `sin`/`cos` in `f64` on the argument
`0x6487 − angle` (one less than a full turn, like the game): **[hypothesis]** bit-identical,
possible one-unit discrepancy on rotated pieces.

## melonDS comparison

The existing states `build/melonds/before.ml1` and `moved.ml1`, produced in `C01M09`, expose
the VBlank counter and the hero's position: (0.210; 0.120; −1.380), then (−2.853; 0.106;
3.854), 1969 VBlanks apart; they are not aligned and do not validate speed.

**Speed measurement** (`scripts/melonds-state.ps1 -Save build/sav/demo.sav -Settle 8 -Keys
"A:0.2,idle:1.5 ×4, B:0.2,idle:1.5 ×3, Down:0.4"`): the four `A` scroll the Stornway church
dialogue up to the menu, the three `B` close it; the hero, at (0.210; −1.380)
facing north, receives `Down` for 0.4 s then 0.5 s of idle before the state. Position read from RAM
(`GameState+0x3ac` → object, `+0x44`): **Δz = 2.361**, final heading 0 (facing south). The port, same
starting position and heading, same inputs: Δz = 2.095 for 24 key frames at 60 Hz
(2.287 for 26) and 2.168 to 2.360 for 11 to 13 updates at 30 Hz (`ticks = 2`): the gap to the
measurement is on the order of the jitter frame of posted messages (0.096 unit per frame); the
provisional model (4 units/s, no acceleration or U-turn) gave 1.6. A second measurement
(`Down` 0.9 s) did not move the hero (the dialogue was not closed in time): **a single
measurement, ± 1 frame**; frame-by-frame validation is still to be done.

## Remaining work

All non-ported entries of "Out of scope"; smoothed following and camera zones
(the default orbit and direct hero following are applied); a position-by-position
comparison at equal frame count under melonDS.

The driver now judges its waypoints and goal in **3D** (§ "Return from 1.2.1");
the grid links two neighboring cells only if the floor height gap stays below `STEP_HEIGHT`
(0.75), so a continuous ramp or staircase, never two stacked walkways; the edge is
additionally validated by `Hero::step`. A cell can carry several floors. Remaining: the
melonDS comparison of the return route and the measurement of the `Field::step` cadence for
doors (§ "What is ported"). The driver's connectivity probe (`edge_pass`) keeps
`ticks = 1`: it is a traversal test, not the field cadence.

`cargo dq9re map --reach` ([`map-archive.md`](map-archive.md#viewing-the-collision-dq9re-map---image---reach))
replays `Hero::step` on a 0.5-unit grid and renders the components by visible height:
it is the tool to use before announcing a route, rather than a BFS that only compares
`x/z`. Two caveats: the fill grid is anchored on its starting point, so a very precise
`--to` can miss an already-reached cell (compare round points or
rerun from the other end); and the command loads the **static** map, without the
`0x95`/`0x96` requests of a trigger.

## Real player run of the prologue (walking evidence)

The headless driver walks (`Hero::step`, collisions, doors, partner search) and the
trace of its inputs can be replayed without it:

```powershell
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot `
  --checkpoints build/checkpoints.jsonl --trace-inputs build/inputs.jsonl
cargo run -p dq9-app --release -- --new-game --headless `
  --replay build/inputs.jsonl --checkpoints build/replay.jsonl
```

- **22,670 fixed steps** in `inputs.jsonl` (d-pad and action key per frame); the replay without
  driver gives **the same checkpoints byte for byte** (sha256
  `3da80263943fe45ab8bde29c4b23657bc2b82102fea2de09d2fb4db59f57e2a8`) up to story 1.1.2.
- The tower batch (main `8d41723`) extends the trace to **26,681 fixed steps** and to story
  **1.2.1** (end of 21510 at 391.12 s), recorded in
  `build/v01-20261107/tower/coordinator-fixed-pilot-inputs.jsonl` and its points
  `…-checkpoints.jsonl` (log `…stdout.txt`, errors `…stderr.txt`). The driver arrives there
  on `4101` at 407.67 s via warp 0 of `4103`, then stops at 444.7 s: that is the return
  described below.
- **First player control** at 210.42 s: `X01M05`, area 4100, 1.1.1, global flags
  `{337, 1450, 1451, 1452, 1453, 1454, 1461, 2721, 2751}`, step flags `{1}`, hero
  `(0.660, -0.250, -3.610)` — the six cutscene flags and the two map flags match the
  melonDS state `build/orca/oracle/bat6.ml1`; nine global flags remain unknown
  (`trigger.md`, § "Remaining unknowns").
- Walked steps: conversations 1140 and 1130 in `X01M05` (210–295 s), warp 9 to `X01M07`
  (312 s), library double door pushed then opened (two leaves, 331–334 s), area 2
  added by event 20 (event 1150), NPC 130 (1600), return through the door to
  `X01M01` (356 s), NPC 127 (1170) at 369.8 s → **story 1.1.2**.
- The driver stopped at 377.8 s on `X01M01`: its **probes in the eight directions** from
  y 8.25 and y 11.25 cap at y 11.25. It is not only the probe: an exhaustive walk
  from the story pose (`(2.77; 11.25; 18.19)`) **and** from the map's starting
  point (`(0; 8.25; 30)`), with edges validated in 3D (`Hero::step`, climb
  ≤ 1.0, drop ≤ 10), also caps at y 11.25. The thin wall `X01A01S1` (block 0, instance 16,
  normal `+x`, `x 0.018..0.105`, `z 15.42..18.99`, `y 9..14`) separates the terrace from the foot of
  the ramp leading to the `0x72` warp **type 0** (no. 9) to `X01M03`. The `0x73` zones of
  **type 1** are **inert** (no reader in `Field_CheckRegions`, see
  `map-archive.md`): it is neither a lift nor a task 8. The cause is not a
  missing connection but a **scripted barrier** the port did not apply
  (next §). `NavGrid` collects all floors of a column
  (`CollisionWorld::floors`) and validates each edge in 3D (`Hero::step`, 3D distance to the
  neighbor): an edge that only slides under a terrace is rejected.
- Limit of the trace: the driver gives directions with the game camera at angle 0 and the
  replay forces that same heading; it proves the headless route in our engine, not a
  melonDS controller replay with manual camera.

### Ramp barrier (actions 0x95/0x96, placement 16)

The barrier `X01A01S1` is **placement 16** of block 0 (instance id, not resource
number 12). The trigger that lifts it is in `data/scenario/triggerX01.bin`:

```
$ cargo dq9re triggers X01 --story 1.1
room 4101 1.1..1.1 event 11: if 0x8(1170) then 0x68(set_track_flag,4) \
    0x95(hide_object,0,1048576) 0x84(advance_track,0,1,1,2)
room 4101 1.1..1.1 event 20: if 0x9(4101) 0x23(1) then 0x96(show_object,0,1048576)
room 4101 1.1..1.1 event 20: if 0x9(4101) 0x23(2) then 0x95(hide_object,0,1048576)
```

The operand `1048576 = 0x00100000` carries the placement id in its high half: `16`. The
European decompilation (`Trig_RunAction` `0x02061c14`, cases `0x31`/`0x32` = actions
`0x95`/`0x96`) calls `Zone3D_FindPlacement` (`0x02019518`, block entry = `arg`, id =
high half) then `func_02013390(placement, arg, 0|1)`, which sets or clears bit `0x4` of
`placement+2` and sets bit `0x40`: exactly what native 574
(`StbNat_SetPlacementHidden`) does. The collision query reads that bit: `Zone3D_PieceTriangles`
(`0x02017da0`, `iVar3+0x28` = parent) returns 0 triangles if the piece's node, or one of its
parents, has bit `0x4` of `+2` (`(*(ushort *)(iVar3 + 2) & 4) == 0` then the loop over the
parents), exactly the state our `piece_hidden` walks. Event 11 at the end of 1170
therefore hides the barrier **just before** advancing the track to 1.1.2; load event 20
shows it at substep 1 and hides it at substep 2. Map loading
is faithful: on the melonDS state `build/melonds-height/probe.ml1`,
`cargo dq9re map X01M01 --state …` compares the 17 instances (placement 16 included, at
the origin) and concludes "the loaded blocks match the assembly"; only the trigger's effect
was missing.

Fix: `CollisionWorld::set_placement_hidden(block, id, hidden)` makes the placement and its
subtree invisible to queries (ground, walls, `floors`, `resolve`), with a
`revision()` counter; `set_placement_position` moves the subtree for native 575. Action
`0x95`/`0x96` produces a `Request::SetPlacementHidden` request (immediate, before deferred
actions) that the application applies to the collision and to the drawn instances; natives
574/575 follow the same path for collision placements, including from the
`--events-report` report, and the driver drops its ground grid when `revision()` changes or
when it enters a map (its fresh collision restarts at revision 0). The collision
tests cover hide/show, the non-matching block, the parent hiding its
descendant, subtree movement and rotation of a moved parent; the ROM tests
play the real request of event 11 then the continued walk to `X01M03`.

Verification: `fmt`, `clippy --all-targets`, the workspace tests on the real ROM
and the wasm build are green (including the application's 28 tests, run with the ROM
and an explicit save). Collision evidence, no teleport and no lowered wall:

```powershell
# NEGATIVE control: the static map keeps S1 (placement 16 is only hidden by
# event 11 at the end of 1170, which this command does not play); "not reached" is
# therefore expected and does not prove a port defect.
cargo dq9re map X01M01 --reach=2.77,11.25,18.19 --to=-8.226,13.97,16.379
# 9320 nodes, y 4.249..11.251, "not reached" — negative control on the static map

# From the west of S1, on the ramp, the 3D fill reaches the warp and climbs to y 23.118
# (still the static map: S1 is still there, the ramp is on the right side).
cargo dq9re map X01M01 --reach=-1.25,12.42,17.5 --to=-8.226,13.97,16.379
# 9542 nodes, y 4.249..23.118, "reached"

# POSITIVE control of hiding: the application test plays the real request of event 11
# (placement 16) then the hero's real walk to X01M03:
cargo test -p dq9-app trigger_1170_hides_the_tower_gate_and_the_hero_walks_to_x01m03
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot
# observed: 1170 ends at 334.45 s (story 1.1.2); map 4103 is reached at
# 336.40 s via warp no. 9, then 4109 -> 4103 -> 4104; 21510 (NPC 199) ends at
# 391.12 s (story 1.2.1). The return from 1.2.1 is described below.
```

The negative control and the application test do not measure the same thing: the first
loads the **static** map (S1 visible, no `0x95` request), the second plays the real
request of event 11 and the hero's walk. No `--reach` CLI option hides a
placement: do not present the static map as the game state after 1170.

## Return from 1.2.1 to the bottom of the Observatory

The driver reaches story **1.2.1** at 391.12 s (end of 21510 on `4104`), comes back through
`4103` then arrives on `4101` at 407.67 s at `(-3.9; 12.47; 16.75)`, on the upper ramp.
It used to stop there at 444.7 s. The cause is **in the driver**, not in the collision:

- the plan leads to `0x72` warp no. 8 of `4101` (box `(0; 5; 29)`, 4×4×2 → `y 3..7`) whose
  standing ground is at `y 4.25`. The follower tested its waypoints and goal in
  **`x/z` only**: arriving at `(-0.16; 8.25; 28.97)`, 0.26 from the goal in `x/z` but **4
  units above**, it declared the goal reached, pushed the warp's zero heading for 20
  frames then gave up ("no way out at Vec3(0.01, 4.25, 29.17)", stderr of
  `build/v01-20261107/tower/coordinator-fixed-pilot.stderr.txt`);
- the connection exists: the `4.25` landing connects with the terrace (`--reach` from
  `0,4.25,29` covers `y 4.249..11.251`), and warp no. 8 is reachable from the landing by
  going through the double door `class 0, number 3` (`4101`, record `0x74` #14, zone
  `0x73` #13 centered `(0; 5; 30.5)`, flag `8` = `FLAG_WARP`);
- the exits to `4105` from `4101` (`0x72` no. 7 at `(0; 1; 34)`, doors 0/6 and 0/7 at
  `(0; 1; 35)`) are **below** the landing (boxes `y -1..3` and `-0.5..2.5`): no plan
  reaches their cells from `4.25`, and it is not a defect to fix by widening
  the height tolerance. The return goes through `4107`;
- the exit log was keyed by `(map, destination, story)`: **one** failed exit
  to `4107` also forbade the doors to `4107`, hence the oscillation
  `4101 ↔ 4103 ↔ 4109 ↔ 4104` then "no walkable way back through map 4101 to 4105".

Fixes (`crates/dq9-app/src/app/auto.rs`, code held for central checks):

- `LEVEL_REACH = 0.8`: a waypoint or goal is only reached if `|Δy|` is
  below it, in addition to the `x/z` distance. A wider tolerance (1.5 was tried)
  let the hero "reach" the first step of a staircase from the ramp above
  it, then push east without ever descending; 0.8 covers the half-climb of an
  edge (≤ 0.625) and the standing height (0.0999);
- cells blocked after getting stuck are remembered per **node** `(cell, floor)`
  and not per cell: a walkway above a passage is no longer forbidden along with it;
- the exit log distinguishes each exit (`class`/`number`, position, destination):
  a failed exit no longer forbids the others to the same map;
- a "in front of the door" support point must be on the door's floor (`|Δy| < 1.5`
  relative to the volume center): the landing above a door below is no longer
  proposed;
- a route exhausted without reaching the goal is **no longer** taken as an arrival: the
  driver replans from its position; if the grid refuses the plan, the goal is abandoned.
  Before, an empty route made it count the 20 exit frames and conclude "no way out"
  while the hero was still four units above the goal;
- getting stuck no longer stops the run: the goal is abandoned and noted as tried (same
  log as exits), the driver picks another one, and `Stop::Stuck` is only returned
  when nothing is left to try. A single replan per 45-frame window
  (`stall` reset to zero), so that learned cells differ instead of exhausting the eight
  replans in eight frames; the replan blocks the **node where the hero is stuck**, not the
  waypoint (already validated by the probe), so as not to forbid the goal itself;
- an exit whose walk failed is remembered **independently of the story**
  (`failed_exits`: map, destination, exit key): a failing objective is not
  replayed indefinitely after each flag change; the other exits to the same
  map remain attemptable.

These rules are generic: no exception per map, per step or per door.

Regression fixed: the test
`trigger_1170_hides_the_tower_gate_and_the_hero_walks_to_x01m03` starts from a fresh driver on
`4101` at 1.1.2; the first exit tried is warp no. 8 to `4107`, whose room is
**behind** the double door (the warp zone only contains `z 28..30`) and whose grid plan
turns out to be impassable. With the 3D arrival test alone, the follower followed that plan and
got stuck at `(17.0; 6.24; 28.73)`; the old follower stopped earlier (arrival in `x/z`)
and moved on to warp no. 9 to `4103`. Now the goal is abandoned and the next exit
(`4103`) is taken, which the test requires.

Physical descent evidence, without `NavGrid`, without teleport, without forced flag: the prefix
of the driver's run up to frame 24,460 (which puts the hero on `4101` at
`(-3.9; 12.47; 16.75)`) followed by 450 frames of d-pad only (right 90, down+right 60,
left 150, up 150) crosses the map through the bottom east, the landing and the door:

```powershell
# inputs: build/v01-20261107/return/replay-descent-4107.jsonl (25,442 frames, no A key)
cargo run -p dq9-app --release -- --new-game --headless `
  --replay build/v01-20261107/return/replay-descent-4107.jsonl `
  --checkpoints build/v01-20261107/return/replay-descent-4107-cp.jsonl
# observed: frame 24,460, map 4101, hero (-3.9; 12.47; 16.75), story 1.2.1;
# frame 25,299, map 4107, hero (0; 0.17; 16.9), story 1.2.1, cause
# "Doorway { class: 0, number: 3 }". sha256 c15c6b54… (inputs) and 192881e1… (points).
```

Result of the fixed driver on the 1.2.1 state, with the requested step cap:

```powershell
cargo run -p dq9-app --release -- --map X01M01 --chapter 1 --step 2 --substep 1 `
  --headless --auto --pilot --headless-steps 30000 --mute
# observed: 4101 -> 4107 via "Doorway { class: 0, number: 3 }" at 44.6 s; NPC 139 (Apus);
# 4107 -> 4105 via warp 1 at 59.9 s; then 4105/4106 and 4105/4107/4101; stop at 207.8 s
# on 4101. No teleport.

# Full driver from a new game, same rules, two minutes of play:
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot `
  --checkpoints build/v01-20261107/return/full-return-cp.jsonl `
  --trace-inputs build/v01-20261107/return/full-return-inputs.jsonl
# observed: 407.7 s return to 4101 (1.2.1); 475.7 s exit to 4107, arrival at 485.8 s
# via door 0/3; Apus (NPC 139); 4105 at 500.9 s via warp 1; guardian 101 at 514.1 s;
# 21591 from 519.7 to 526.3 s then map 1100 (Angel Falls) via the warp of trigger 11;
# 21592, 1380, 1390, 1370 (village cutscenes); M01M08 at 582.4 s; stop at 665.2 s in
# free control on 1108, hero (-2.20; 0.10; 0.78). Story 1.2.1, track flags
# 0 {4, 7} and {0, 1, 3, 12, 13}. No teleport, no forced flag.
```

The corresponding physical path: go down the east flank (`8.25 → 6.14 → 4.25`), follow
`4.25` west to the landing `(0; 4.25; 31.1)`, enter south into the double
door → `4107`. In `4107`, Apus (NPC 139, event 0: `0x68(set_track_flag,0)`), then
`0x72` warp no. 8 at `(0; 0; 9.5)` to `4105`, the guardian (NPC 101, event 1:
`0xb(193) 0x10(0)` → `0x77` 21591), and 21591 → `0xe1` warp to `1100`.

Limits: the code is still to be committed after the central checks; the descent evidence is
a replay of our inputs in **our** engine, not a melonDS replay; order and durations are
not compared; what follows at `1100` (village quests) belongs to the village and
cutscene batches.

The step counts, times and positions above were measured before the 30 Hz field
cadence of `run_steps`. Since then, the driver still advances by a fixed 60 Hz step but field
logic only consumes the d-pad every other VBlank: trajectories stay in the engine,
but frame numbers and checkpoints may shift and must be
re-measured by the verification batch, without changing the driver's inputs.
