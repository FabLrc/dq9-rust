# Field camera on a scene transition

Batch `j4-scene-camera`. User report (hand-played session): the camera did not change angle at a
scene transition (door, staircase, warp). This note records what the original does, where it
reads it, and what the port does now. The shoulder-button turn itself is in
[`movement.md`](movement.md) § "Field camera".

## The rule (read from the code, checked on 121 melonDS states)

On **every map load** the map-entry state machine `ov_017::0219e384` (`Field_EnterMap`), once the
cast, doors and triggers are in place and **unless `Save_Warp+0xa` is non-zero** (below), does:

1. `FieldCamera_SetHomeYaw` (`0x020a2798`): home yaw `FieldCamera+0x246` = the signed halfword
   `Zone3D+0x40`, which is **`maplist9.bin` value 13 × 4096 reduced to `[0, 2π]`**
   (`ZoneInfo::world_angle`, handler `0x0201c60c`; the field the earlier note called
   "orientation on the world map" **[hypothesis]** is read here as the camera's home yaw);
2. a framing switch on the record: `(record+0xc & 0x7f) >> 5` (bits 5-6) picks
   `FieldCamera_SetFraming0..3` (`0x020a27dc`, `0x020a2810`, `0x020a2840`, `0x020a2878`), which
   store the orbit elevation `+0x224`, the distance `+0x228` and the half field of view
   (`Camera_SetHalfFovDegrees`, `0x0202e9b4`);
3. `FieldCamera_ResetYaw` (`0x020a2728`): yaw `+0x220` := home and `Camera_SetOrbit`
   (`0x0202e5e8`) at that yaw, **at once**.

| bits 5-6 | `dy` | distance | half fov | maps |
|---|---|---|---|---|
| 0 | `0x7999` (7.60) | `0xe000` (14) | 15° | 438, among them `C01`, `M01`, `F02`, `X01M01`…`X01M09`, `D01M01`…`D01M03` |
| 1 | `0x6000` (6) | `0xb000` (11) | 15° | 431, among them the `C01M03`…`C01M14` interiors, `M01M01` and `R01M01` |
| 2 | `0x8b33` | `0xe666` | 25° | `O00` (ocean) |
| 3 | `0xc666` | `dy + 0x2000` | 25° | `O01` (sky) |

The constants are the literals and immediates of the four setters, read by
`crates/dq9-core/tests/field_camera_rom.rs`. The home yaw is **0 on 870 of the 871 maps**; the
exception is `M01` (the Angel Falls exterior, id 1100): 1228 (0.30 rad).

So the original **ignores the hero's heading and the previous map's camera**: every door, stair
map, warp or event map change puts the camera back at its destination's home yaw and framing.
The port used to start the new orbit at the hero's own facing (`Camera::field(.., hero.yaw())`),
a different yaw after every door, and to keep one distance (11) and elevation (6) on every map
(the framing of entry 1 only), so outdoor maps looked closer and lower than the original's.

**Not the cause** (each refuted): the hero's heading (110 states, hero yaw 0 to 5.7 rad on
maps 100–4109/20002, camera yaw 0 in all: `build/evidence/j4-scene-camera/hero_vs_camera.txt`),
the arrival angle of the warp (`Warp::arrival_angle` goes to the hero, not the camera), a
`.bmbl` camera zone (nothing in `ov_017::0219e384` reads one; **[unknown]** what sets
`+0x70/+0x74/+0x78` to `0x8000/0x17000` in 24 `X01M03` and 12 `X01M09` states while
`+0x224/+0x228` stay at `0x7999/0xe000`, an event or scripted camera **[hypothesis]**), the
map record's lock bit (`FieldCamera_ApplyMapLock` only decides whether `L`/`R` act).

### When the camera is **not** reset

- `Save_Warp+0xa` (byte) set: `ov_017::0219bcac` (`Warp_ReloadHere`) builds a warp to the current
  map at the protagonist's current pose with that byte = 1, and the map load then skips steps
  1-3 (`if ((char)puVar14[5] == 0)`, `ov_017::0219e384`). The other warp builders
  (`Field_WarpFromRegion` `ov_017::0219c6f4`, native 807 `ov_001::02161f80`,
  `Save_UnpackWarp` `0x020a9d88`) clear the whole `Save_Warp` through `0x02070388` first, so
  their byte is 0. `ov_017::0219e384` also clears the byte when `GameResources+0x42e9` is set.
  [hypothesis] the caller is a same-place reload such as the battle return; the port's
  equivalents (battle return, time-of-day reload, cast reload) are not map changes and keep the
  camera.
- `Save_Warp+0x6e` bit 6 set (`0x40`) resets the yaw even then (the second `FieldCamera_ResetYaw`
  call): a store of `0x40` to a warp-like record exists in `ov_003` (not traced), **[unknown]**
  which path sets it; the port has no equivalent.

### Other writers of the yaw (not changed here)

`FieldCamera_ResetYaw` is also called by the event start (`ov_001::0215657c`, `ov_001::021567c4`)
and by the wipe-out revival task `ov_017::021c37a4`; `FieldCamera_RestoreYaw` (`0x020a2754`,
yaw := `+0x21e`) at the end of an event (`ov_017::021bc77c`, `ov_003::0217c7a0`). The event
camera is another worker's file (`event/camera.rs`); whether it re-homes the yaw at an event
start is **[unknown]** for the port, so cutscenes are unchanged.

## Timing

Instant: step 3 is one call in the map-entry state, followed by the fade-in states (9/10). `FieldCamera_Update` (`0x020a213c`) then only moves the yaw while a shoulder button is
held or the return-to-home flag (`+0x245` bit 1) is set; with both clear (the state after a
load) it just repositions the target on the hero. There is no ease and no tick count to model.

## Which transitions

All of them take the same path (**[hypothesis]**: the warp builders above all fill `Save_Warp`,
whose pending byte `+2` starts `Field_EnterMap`; not every caller of those builders was traced),
so doors (`0x73`/`0x74` doorway zones with their own warp), `0x72` warps (kind 0
and kind 1 doors), trigger-driven warps (actions `0x85`/`0x8a`, native 807/842) and the wipe-out
return all reset the camera; a staircase that is a separate map (`X01M05` → `X01M06`, `D01M02` →
`D01M01`/`D01M03`) is a warp. A staircase inside one map (no map change) does nothing.
The port's `App::load_map` serves them all (`change_map`, `script.rs`, `fight.rs` wipe-out,
`flow.rs`, `app.rs` start): `frame = true` marks a map change and now builds the arrival camera;
the time-of-day reload (`frame = false`) and `reload_cast` keep the camera.

## Port

- `dq9_core::camera::{FRAMINGS, framing(byte_c), home_yaw(world_angle)}`: the pure rule, with
  the existing `rotation_allowed`.
- `app/camera.rs::arrival_camera`: pitch `asin(dy / distance)`, distance, `fovy = 2 × half fov`,
  yaw `-home / 4096` (the game turns the other way round from the application), and the
  application's `camera_home`; `app/map.rs::load_map` applies it in orbit mode.
- `turn_camera` now measures the buttons' yaw against the absolute game yaw and home (the band
  `(π/4, 7π/4)` of `FieldCamera_Update` is absolute; it only mattered for `M01`'s 1228), and a
  same-map reload no longer overwrites `camera_home` with the player's current yaw.
- Open: `input.rs` (`F` back to orbit: `Camera::field`, entry-1 framing) and `encounter.rs`
  (`FIELD_HALF_FOV` 15° after the swirl; the saved camera is restored afterwards, so only the
  swirl's own frames use 15° on `O00`/`O01`) belong to other files.

## Checked maps and transitions

Real crossings through the port (`crates/dq9-app/src/app/camera/tests.rs`, real-ROM; every
warp and doorway of the four start maps, the hero stepped into the volume, the camera turned to
yaw 2.0/pitch 0.1/distance 30 first), expected yaw/pitch/distance/fov taken from the ROM's map
list without the code under test:

| from | to (id, entry) |
|---|---|
| `C01` Stornway (100) | `C01M10` 110, `C01M12` 112, `C01M14` 114, `C01M09` 109, `C01M08` 108, `C01M07` 107, `C01M04` 104, `C01M06` 106, `C01M05` 105, `C01M03` 103, `R01M01` 50101 (all entry 1); `F02` 20002 (entry 0) |
| `X01M05` (4105) | `X01M06` 4106, `X01M07` 4107, `X01M01` 4101 (entry 0) |
| `D01M02` (7102) | `D01M01` 7101, `D01M03` 7103 (entry 0) |
| `M01M01` (1101) | `M01` 1100 (entry 0, **home yaw 1228**) |

`M01`'s own `L`/`R` return is tested (`the_exterior_camera_returns_to_its_own_home_yaw`).
Before the change the same tests fail at the first crossing (`yaw: C01 -> 110 …: -0.8379`, the
hero's heading, against 0) and the reload test shows `camera_home` overwritten.

## Evidence and falsification

`build/evidence/j4-scene-camera/` (local, `manifest.json`):

- `camera_state.py` / `camera_state.txt`: the field camera object (`GameState+0x3b0`, DTCM) and
  `Zone3D+0x40` of every melonDS state under the main `build/` (121 with a field camera, 9
  maps).
- `check_rule.py` / `check_rule.txt`: the rule's prediction (`+0x224/+0x228` = framing entry of
  the record, `+0x246` = `+0x40` = world angle) against those states: **121 states, 0
  mismatches**. The rule was derived from the code before any state was read, so none of the
  states was used to find it. They separate entry 0 (`C01`, `X01*`, `F02`, `F01M01`: `0x7999`)
  from entry 1 (`C01M09`: `0x6000`, which is also the constructor default) and show the home yaw
  is 0 everywhere they reach. The yaw of `M01` is **[hypothesis]** (code plus the parsed map
  list; no state on `M01`). A state of the camera turned by hand, then a door, would test the
  reset itself; none exists, and the 110 states show only that the camera never takes the hero's
  heading.
- `hero_vs_camera.txt`: hero heading and camera yaw of each state.
- `maplist-camera.txt`: the 871 maps with record byte, entry and world angle.
- `before-change-tests.log`: the new tests against the old `map.rs` (they fail: yaw -0.838 on the
  first crossing, `camera_home` overwritten by a reload).
- No before/after PNG: `--screenshot` (`native.rs::screenshot`) builds its own
  `Camera::field(hero, hero.yaw())` outside `App::load_map` and refuses a script that leaves the
  map, so it cannot show a transition and was not changed (not in this batch's files). A capture
  of a crossing needs the window path or a test that draws `App`.

## Replays

The pilot and the replays walk with the camera at yaw 0 in the pad (`app.rs`: `camera_yaw = 0`
when piloted or replayed), so `Hero::step` does not read the camera. The camera still feeds the
scene's NPC facing and the event camera's restore point, which is why the route digest and
`parity.ps1` were rerun (see the batch report). A `dq9rec` record made before this change
replays the same pad but its camera state differs; none exists yet.
