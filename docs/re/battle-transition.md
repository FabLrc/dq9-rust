# Battle transition: contact to the first command menu, and back to the field

Batch `j4-battle-transition`. What the original shows when a fight starts and ends, read from the
European ROM (code, Ghidra, EU addresses) and measured in the melonDS DS core (the libretro host
`scripts/melondsds_host.py`, frame by frame, RAM read every frame). Everything below is **observed
on the original** unless marked **[hypothesis]** / **[unknown]**. Frame numbers are emulator
frames (60 Hz) from a core state saved on the frame a roamer touched the party (state
`w-contact1`, `build/oracle/field-F02/run/`, map `F02`, level-70s party, one slime). Ticks are the
reimplementation's 60 Hz fixed steps.

## Where the transition lives

| Address (EU) | Name | What |
|---|---|---|
| `ov_017::021b790c` | `BattleTask_Update` | the battle task (kind 10, `GameResources+0x3718`). Its state 6 starts the swirl: `021b621c` on the task at `GameResources+0x3b5c`, `+0xfe` = battle number (`-1` for a roamer), then state 0 (the fight runs in `ov_000`); state 5 fades back and ends |
| `ov_017::021b621c` | `SwirlTask_Init` | task kind `0x16`; `+0xfd` = 15 (delay) when no object is given, 0 when native 547 gives one; `+0xfc` state |
| `ov_017::021b6290` | `SwirlTask_Update` | the swirl task's state machine (below) |
| `0x02046e80` | `SwirlEffect_Init` | the effect record at task `+0xc`: tick counter `[0]`, roll `[2]`, fov `[3]` (15.0), fade-pending byte `+0x1c` |
| `0x02046ca8` / `0x02046cd8` | `SwirlEffect_QueueLoad` / `_LoadStep` | queue `data/effect/ev999991500.chr` and, once loaded, build its `Object3D` (polygon id `0x3d`, position `(0, -10, 1)`, motion 0) |
| `0x0204701c` | `SwirlEffect_Update` | one update of the swirl (roll, field of view, effects, fade request) |
| `0x02046f94` | `SwirlEffect_Release` | camera roll 0, half field of view 15°, frees the effect |
| `ov_017::02195ecc` | `FieldDraw_OrthoPass` | the frame's 2D pass; with the front task of kind `0x16` it draws the task queue (`021a3af0`) in an orthographic view |
| `ov_001::0215ce4c` | `StbNat_CreateBattleSwirl` | **native 237** (table entry `0xed`): loads the same chr as an object of an event slot; native 547 (`02163ccc`) then shows it and hands it to a `0x16` task |

Two entries reach the same task: a roamer's contact (the contact code queues the battle task, whose
state 6 creates the swirl task with no object: the task loads the chr itself) and a script
(native 237 creates the object, 547 starts the task with it).

## The swirl task (kind `0x16`), by state (`SwirlTask_Update`)

- **0** (first update): camera object locked (`020397cc`), `+0xb2` cleared. If `+0xfe` (battle
  number) is below 1, `+0xfd` (delay) is zeroed; so a **roaming encounter and native 547 have no
  delay, a set battle (`Request::Battle`, number ≥ 1) waits 15 updates** [the delay counts task
  updates, which run every `numTicks` frames: not measured on a set battle]. `+0xfe ≥ 0` queues
  the `eventbattle` file for the battle's own track. → state 1.
- **1**: waits for the chr (`0x02046cd8`). → state 2.
- **2**: when the delay is 0: starts the battle sequence (`0x17`, or the record's track),
  `SetFlags04(0x40)`, then, if the screen is not already fully black, → **3** (swirl); else
  `SetMainBrightness(-16, 0)` → 4.
- **3**: `SwirlEffect_Update` each task update until it returns true. → 4.
- **4**: waits for the brightness transition to end, then restores the field camera
  (`SwirlEffect_Release`), sets the task's done byte (`+1`); the next task (kind 3, the stage's
  map load) takes over.

### `SwirlEffect_Update` (`0x0204701c`), per task update

Constants read from the literal pool (`0x02047158..0x02047168`): −180.0, 180.0, 3.14, 0.4333, 4096.0;
`0x02046f88` = 15.0.

1. `[2] -= 8.0`; if `[2] <= -180.0` then `[2] = 180.0` (the roll accumulator, degrees).
2. roll (fx32 radians) = `reduce_0_2pi(4096 · 3.14 · [2] / 180)` → `Camera+0x7c`
   (`0x0202e884`). Net: **the camera rolls −8° per update** (352°, 344°, … measured).
3. `[3] -= 0.4333`; **half field of view** = `[3]` degrees (`Camera_SetHalfFovDegrees`
   `0x0202e9b4`): starts 15.0, **−0.4333° per update** (7.2° at the last update: a 2× zoom).
4. `Object3D::AdvanceEffects` on the swirl model (joint + material animation, motion 0,
   speed 0.5: frames 1 → 40 at half speed; the swirl reaches frame ≈ 19.5).
5. `[0] += GetTickCount()` (ticks since the last update, 3 on the first, 2 after: **the game
   updates at 30 Hz**). When `[0] > 15` the first time: `SetMainBrightness(-16, 20)` and
   `SetSubBrightness(-16, 20)` (both screens to black over 20 ticks).
6. Returns true when `[0] > 35`.

Measured (core state `w-contact1`, `RAM` read every frame; log `build/evidence/j4-battle-transition/swirl-camera.log`):

| Update n | frame | `[0]` | roll | half FoV |
|---:|---:|---:|---:|---:|
| 1 | 18 | 3 | 352° | 14.567° |
| 2 | 20 | 5 | 344° | 14.133° |
| 8 | 32 | 17 | 296° | 11.533° |
| 17 | 50 | 35 | 224° | 7.634° |
| 18 | 52 | 37 (done) | 216° | 7.200° |
| release | 54-56 | | 0° | 15° |

The screens are untouched until frame 34, darker from 35 and black at 55: the fade starts one
update after `[0]` passes 15 and lasts 20 ticks.

## The swirl's drawing

`FieldDraw_OrthoPass` (`ov_017::02195ecc`, parameter 2): saves the field's view, then
`MTX_OrthoW(top 14.0, bottom −34.0, left −32.0, right 32.0, near 1.0, far 400.0)` (a 64 × 48
box: the screen's 4:3, centred on y = −10) and a look-at **eye (0, 0, 10), up (0, 1, 0), target
(0, 0, 0)** (vectors at `ov_017::021d6420` / `021d642c`), draws the queue (the swirl's `Object3D`
at world `(0, −10, 1)`, scale 1, no rotation, so the disc lies centred on the screen at 9 units
from the eye) and puts the field's projection back. The roll and the field of view therefore act
on the field alone; the swirl is a screen-sized orthographic overlay drawn on top, then the master
brightness darkens both.

The model (`data/effect/ev999991500.chr`: one flat 135-vertex spiral disc in the xy plane,
radius 20.5-33.1 at rest, one 64 × 128 `A5I3` texture `encounter03`, two materials): its joint
animation (41 frames) **scales (×1 → ×2.2) and rotates** the disc; its material animation
raises the ring material's alpha from 0 (frame 1) to 0.90 (frame 15, held to 25), the other
material stays at 0. At frame ≈ 19.5 the radius is ≈ 54 against a half-diagonal of 40 in the
ortho box: the rings fill the screen at the end.

## Timeline of a roaming encounter (frames from the contact frame 0)

| Frames | What | Source |
|---|---|---|
| 0 | the roamer enters state 8; the battle task is queued | `w-contact1` queue `[10]` |
| 1-3 | `BattleTask_Update` runs its set-up and state 6: swirl task created (frame 2), state 1 → 2 (frame 4) | trace |
| 5-17 | **the picture is frozen**: no field step runs (screen mean unchanged for 12 frames) while the game loads `ov_000` and the battle's data: **[hypothesis]** a blocking load, not a designed delay | `build/evidence/j4-battle-transition/c1-brightness.txt` |
| 17 | swirl state 3 | trace |
| 18-52 | swirl updates (18 of them, every 2nd frame): roll, zoom, rings | table above |
| 35-55 | both screens fade to black (20 ticks) | brightness trace |
| 54-56 | swirl task done, camera reset, the stage's map-load task (kind 3) begins | trace |
| 55-148 | **black, 94 frames**: the battle stage and the monsters load | frames |
| 149-161 | the top screen (the 3D scene) fades in over ~15 ticks | frames, `0x021d9d18` |
| 149-282 | **intro camera**: an arc around the party, eye from (−4.7, 1.2, 5.8) through (0.5, 0.8, 9.2) to (4.6, 0.5, 5.0), target moving from (0, 0.81, −1.30) to (0, 0.93, −0.32), half-FoV 15°, 133 frames, eased | RAM, camera object `0x023387c4` |
| 283-304 | the battle camera (eye (0, 1.63, 5.56), target (0, 1.2, 0), half-FoV 22°) holds, then dollies in (eye z 5.56 → 4.7 by 317) | RAM, camera `0x02392668` |
| 307-320 | the bottom screen's command window fades in (first "draws near" line at 313) | frames |

**Back to the field** (victory, from the frame the last window closes): the screens fade to
black over ~15 ticks (frames 1043-1057), stay black **95 frames** (field reload, **[hypothesis]**
load time), the bottom (3D) screen comes up over ~30 ticks (`BattleTask` state 5,
`SetMainBrightness(0, 30)`, frames 1153-1183) and the top screen (the map) fades in over 20 ticks
about 20 frames after it (1173-1193). The battle task is removed 55 frames after the fight
ends (frame 1098, queue `[3]`, then empty at 1153).

## What the application does with it

**Wired** (batch `j4-swirl-wire`), in `battle/transition.rs` (the clock), `dq9_render::swirl`
(the disc) and the application:

- `App::load_map` adds the disc to the field scene (`data/effect/ev999991500.chr`, hidden)
  after the map is built and before its first upload, and `App` keeps it across the battle: the
  renderer is built from that scene, so the disc is an ordinary instance of the field's. A
  battle stage or a preview is loaded without one (`load::load` knows nothing of the
  transition).
- `App::step_battle` carries the clock instead of a tick counter for both entries: the
  `Entering` phase of a roaming contact or a scripted battle, and native 547's `Transition`. Each
  fixed step it reads `roll_degrees`/`half_fov_degrees` into `camera.roll`/`camera.fovy`, the
  `cover` into the black overlay of both screens, and builds the battle scene when `done()` and
  the cover is full — 38 steps (the port's old constants said 35, two early, and had no roll).
- On that step `SwirlEffect_Release` runs: roll 0, half field of view 15°, the disc hidden in
  the field scene *before* `save_field`, so native 844 gives the field its own camera back.
  `App::redraw` poses the disc at `model_frame()` and pins it in front of the camera while the
  clock runs (`dq9-render::swirl::world`), and hides it once placed.
- The way back to the field fades over its 15 steps again: `fade_in` was reused there and
  returned 0 (an instant black).
- `BATTLE_ENTER_AT`/`BATTLE_ENTER_DONE`/`fade_in` are deleted; the transition module is no
  longer `allow(dead_code)`.
- Headless runs (`battles_instant()`) still return before any step is spent: the traces, the
  route digest and `parity.ps1` do not move. A shown run spends 3 more steps per battle
  (38 instead of 35) and nothing else.

Checked on the pilot's route (`--new-game --pilot --fight --route 2750 --seed 7`; the shown
battles of the Web session's `battles=show` are the same `battles_instant()` false path, run
here through `App::new_replay`; 60,000 fixed steps from a new game):
the instant run's 173 checkpoints are **byte-identical** before and after
(`build/evidence/j4-swirl-wire/{before,after}/route-instant-checkpoints.jsonl`,
`191aeec870d3`), and the shown run's 167 checkpoints are identical apart from
`tick`/`vblank`/`seconds`: the one battle checkpoint moved from tick 9387 to 9390 (**+3**, the
entry's 35 → 38) and the last state (map 4105, chapter 1 step 4, the same flags, party 13/20 HP,
2 xp, 4 gold) is the same — `route-shown-checkpoints.jsonl`, before `d46fb483c2a3`, after
`ea2345449f71`. At 60,000 steps the pilot is still at the start of chapter 1 (its own stuck
point on `4101`), so this is a window of one shown battle, not the whole route; shown and
instant runs diverge past it on the roamers' draws, a **[hypothesis]** recorded before this
batch.

**Not modelled / [unknown]**: the load-bound black periods (94 and 95 frames; headless runs never
spend them and the window would add 3 s per fight for nothing), the 12-frame freeze after contact,
the intro camera's script (the arc above is measured, its source in `ov_000` not read), the set
battle's 15-update delay (derived from the code; no set battle was run in the DS core), the map
screen's own transition on the top screen.

Ours, `cargo test -p dq9-app -- the_swirl_is_drawn_over_the_field` with `DQ9_EVIDENCE_DIR`
(local, `build/evidence/j4-swirl-wire/`; map 1100, the window path's own field scene, clock and
camera, 256 × 192):
`swirl-field-step01.png` `8b0272a81aca`, `-10` `00792e17c4d8`, `-20` `9447da14bd7f` (the rings
cover the field), `-30` `4fb0d094967a`, `-38` `82b2f963a8c2` (black, the same bytes as the
earlier swirl capture's last frame). `cargo test -p dq9-app -- swirl` also runs
`battle::tests::the_swirl_rolls_zooms_and_blacks_out_the_field` (the disc over a battle stage).

## Evidence (local, `build/evidence/j4-battle-transition/`, sha256 first 12)

Original, melonDS DS core 1.4.0 through `scripts/melondsds_host.py --state-in` on the saved
`w-contact1` state (frame 0 = the contact):

- `swirl-camera.log` `0516d8759fc4`: `Camera` fov and roll every second frame (the table above).
- `swirl-record.log` `712f65146015`: the effect record (`[0]` counter, `[2]` roll, `[3]` fov) and
  the task state every frame.
- `task-queue.log` `9a22eab5fad5`: the task queue and the battle task's state over 1,200 frames
  (swirl task created at frame 2, map-load task 56 → 1098, battle ended 1082-1098).
- `c1-brightness.txt` `511ddc83e17b`: mean brightness of both screens per frame, 1 to 399 (the
  12-frame freeze, the fade 35-55, the black 55-148).
- `intro-camera.log` `2de51169c44e`: the intro camera (eye, target, fov) every 6 frames.
- `contact-swirl-bottom.png` `d49758a89b27`, `contact-to-menu.png` `9349b219d1bb`,
  `back-to-field.png` `20f402b827d9`: contact sheets.

Ours, `cargo test -p dq9-app battle::tests::the_swirl_rolls_zooms_and_blacks_out_the_field` with
`DQ9_EVIDENCE_DIR` (F01 stage, field camera, swirl clock tick by tick): `swirl-tick00.png`
`a59b04ca1573`, `-09` `13729bc2ec7c`, `-19` `c323ec4b5224`, `-29` `040e93a28c39`, `-37`
`82b2f963a8c2` (black). Side by side with the original's frames 18/27/37/47/55 the roll direction,
the zoom, the growth of the rings and the fade agree; the hero sprites differ (another map).
