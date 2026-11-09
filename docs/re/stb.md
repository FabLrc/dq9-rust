# Event scripts (`evNNNNN.stb`, `SB2`)

Implementation: `crates/dq9-formats/src/stb.rs` (`Script::parse`, `Instr`,
`Function::native_calls`). Tool: `cargo dq9re stb data/event/ev03030.gp2 [--lang fr]`
disassembles the script and prints the message text next to the native 400 calls.
The interpreter that runs these scripts is in `dq9-core` (§ [Interpreter](#interpreter-dq9-core-j3));
their effects are played on screen by `dq9-app` (§ [On-screen execution](#on-screen-execution-dq9-corecutscene-dq9-app)).

These are the scripted cutscenes started by trigger action 0x77 (`trigger.md`):
camera, character movement, dialogue, battles.

## Files

689 scripts, one per archive: 523 in `data/event/evNNNNN.gp2`, 165 in
`data/evspt_lv5/evNNNNN.gp2`, 1 in `data/event_lv5`. Each archive contains
`evNNNNN.stb` and a per-language text table `evNNNNN_<lg>.bin` (`table.md`,
records `[message number, text]`, numbers starting at 100).

The 524 event numbers passed to action 0x77 are archives of `data/event` or
`data/evspt_lv5` (502) or entries of `data/event/eventlist6.bin` and `evl_quest.bin`
(record tables of type 102). Observed fields of these entries:
`[chapter, ?, ?, ?, room, event, archive, Japanese name, script name, 100, 200,
300, x, y, z, angle, ?, -1, ?, ?, font "font12eventA…", ?, ?]`, the archive possibly
differing from the event (1520 → `ev01515`) **["?" fields unknown]**.

## Header

32-bit words, little-endian (`ov_017::021d4c04` checks the 3 bytes `SB2`):

| Word | Content |
|---|---|
| 0 | `SB2\0` |
| 1 | function record run without an entry number (`ov_017::021d4ce4`, number < 0) |
| 2 | code base: code, jump, call and string offsets are relative to it |
| 3, 4 | table of entries `(number, record)` × count (e.g. 100, 200, 300) |
| 6 | number of script global variables (0 to 5) |
| 5, 7 | always 0 |

## Functions

A `0x38`-byte record `[code, ?, frame slots, parameters, zeros…]` is
followed by its code: `record + 0x38 == base + code` for every reached
function (check `event_scripts_decode`). On call (`ov_017::021d4af8`), the last
`parameters` stack values become the first frame slots, the
others are zeroed. Constant strings (animation names `stand`, `walk`…,
Shift-JIS markers) are stored between functions: `Script::parse` only decodes
code reached from the entries (jumps and calls followed), like the interpreter.

## Machine (`ov_017::021d4e38`)

Three-word instructions `[op, a, b]`. Stack of 8-byte values `(type, value)`:
0 integer, 1 float, 2 string, 3 address. Arithmetic operations accept mixed integer and
float (the integer is converted), otherwise the interpreter stops with an error
(`FUN_0200159c(-1)`).

| Op | Effect |
|---|---|
| 0 | nothing |
| 1 / 2 | pushes the value / the address of variable `a` with scope `b` |
| 3 | pushes constant `b`: `a` = 1 integer, 2 float, 3 string (offset from the base) |
| 4 | pops |
| 5 | assignment: pops value and address, writes, pushes the value back |
| 6 / 7 / 8 / 9 / 0xa | `+` `−` `×` `/` `%` (0xa: integers) |
| 0xb / 0xc / 0xd | negation, integer → float, float → integer |
| 0xe | comparison `a`: 0x28 `==`, 0x29 `!=`, 0x2a `<`, 0x2b `<=`, 0x2c `>`, 0x2d `>=`; pushes 0 or 1 |
| 0xf | return with the top value; at the outermost frame, end of script (value kept at `+0x4c`) |
| 0x10 | jump to `base + a` |
| 0x11 / 0x12 | pops; jumps if false / true, pushing back 0 / 1 when `b` ≠ 0 (`&&`, `\|\|`) |
| 0x13 | call of the function whose record is at `base + b` |
| 0x14 | pops `a` values and passes them to a debug output |
| 0x15 | native call: pops `a` values, the first is the native number |
| 0x16 | nothing at run time; `a` = 0 at loop head, 1 at its exit in the observed functions **[exact meaning unknown]** |
| 0x17 | suspends the script until the next frame (resumes at the next instruction) |
| 0x18 / 0x19 / 0x1a | bitwise and / or, logical not (integers) |
| 0x1b | end of script |
| 0x1c | increments `+0x50` and resumes after a suspension **[meaning unknown]** |
| 0x1d / 0x1e | sine / cosine |

Variable scopes (`b` of ops 1 and 2): 1 frame slot, 2 slot `a + popped integer`,
4 element `popped integer` of the array whose address is in slot `a`, 0x40 global;
8, 0x10, 0x20 and 0x200 are the same as 1, 2, 4 and 0x40, retyping the slot as float.
The data only use 1, 8 and 0x40.

Statistics (reached code of the 689 scripts): 393,719 constants, 80,237 native calls,
20,685 function calls, no opcode beyond 0x1e.

## Natives

Table `{function, number}` × 304 at `ov_001::02164d6c` (terminated by `{0, -1}`), numbers
0–9, 100–122, 200–240, 300–328, 400–421, 500–603, 700–738, 800–845; the functions are in
overlay 1 (`ov_001::0215ae1c…`). `ov_017::021d4bc8` calls the native with a pointer to
its arguments (without the number) and their count. A native pushes nothing: it returns its
results through address arguments (op 2).

The handler's C return value is **ignored**: `Stb_CallNative` (`ov_017::021d4bc8`) calls
the pointer then returns; case 0x15 of `Stb_Run` continues at the next instruction.
A return of 0 is therefore neither a wait nor a result on the stack. Waits are
script loops that poll an address output then execute `yield`.

Argument access (8-byte values, indices below exclude the native number):

| Function (`ov_017`) | Effect |
|---|---|
| `021d60f4` (`StbArg_Int`) | reads the value word; converts a float to integer via `0200c5fc` if type 1 |
| `021d6110` (`StbArg_Float`) | reads the value word; converts an integer to float via `0200c670` if type 0 |
| `021d612c` (`StbArg_Str`) | reads the value word (string pointer) |
| `021d6134` / `021d6148` | write the value word of the pointed slot if the argument is of type 3 (address); do not change its type |

These facts come from the decompilation and the ARM9 software conversions;
they are not a run-time observation under an emulator.

The compiler writes the number as `base + offset` (`int 400; int 5; add`):
folded, it is constant for all 80,237 calls, which use 228 natives
(`Function::native_calls`, test `event_scripts_decode`).

The [inventory of the 228 natives used](stb-natives.md) details, by family,
arguments, established effects, handler/engine addresses and identification limits.
Entries still uncertain stay numbered.

206 queues an absolute position, 208 a rotation (radians), 210 a named
animation; they are not three movement variants. STB actors, their
bindings and the characters/models belong to three distinct spaces.
Commands are queued per channel; 224 waits for the current animation to end,
unlike 210. Duration units: `stb-natives.md`, § Time and units (fade in 60 Hz
ticks, movements in tick pairs, i.e. 30 Hz nominal).

Summary of the five most frequent natives (EU ROM, cloned USA source, wiki):

- **206, 208, 210**: EU handlers located (`ov_001`, `0215bfd4`, `0215c138`, `0215c298`,
  names in `re/symbols.txt`). Absent from the cloned USA decompilation (it contains neither
  the native table nor the SB2 VM); roles established from the EU ROM (queue commands, `stb-natives.md`).
  The wiki's readings (208 in radians, 210 "bit 1") are **[hypothesis] EU**. Open conflict
  on the bits of 210 (`0x01` versus "bit 1").
- **570**: established. `570(slot, b)` hides (`b=0`) or shows (`b≠0`) the object bound to the slot:
  the handler sets mask `1` of `Object3D::flags_`, i.e. `OBJECT3D_FLAG_HIDDEN`
  (USA source `Object3D.cpp`). It is not a "polled" predicate: it writes nothing through
  an address. 201 acts on the same bit by character identifier.
- **571**: same slots, mask `0x10` of `flags_` (bit 4, no reader in the USA source)
  and bit `0x40` of a placement. Always called with 570; meaning **[unknown]**.

None of these five natives is marked `port:`.

400 displays a message by identifier or string; 405 exposes the wait state
(`ev03030:fn@0xe3c` loops while it is 1), and 558 reads the answer separately.
The documented YESNO path uses 0=yes, 1=no, without generalising to the variants.
The orbital camera takes azimuth, relative height and total distance, not a pitch.
726–728 load/restore sound resources and play an SE index, not flags.

Utility functions are compiled into the file, not provided by the engine; in
`ev03030`: wait for `n` frames (`while (n >= 0) { n -= 1; yield; }`, `fn@0x58`) and
display a message, waiting for it to end (`fn@0xe3c`).

## Interpreter (`dq9-core`, J3)

Implementation: `crates/dq9-core/src/script.rs` and `script/`; headless trial:
`cargo dq9re script <archive|event|stage> [--quiet]` (`data/event/ev01130.gp2`, `1130`,
`1.1-3`), which plays the scripts with a dummy application that ends every wait
immediately.

| Module | Role |
|---|---|
| `script/vm.rs` | the machine (`Stb_Run` and its helpers), without natives; `port:` markers |
| `script/natives.rs` | native number → `Event`, and the state the scripts poll |
| `script/event.rs` | the `Event`s: fades, messages, actors, camera, sound, cast declarations |
| `script/load.rs` | event number → archive, entries and texts; scenario stages |
| `script.rs` | `Script` (entries played one by one, `step`, `signal`) and `Director` (event task) |

**Machine.** Function-by-function translation of `ov_017::021d4980…021d4e38` (addresses are
in the comments and in the `port:` of `vm.rs`): 8-byte `(type, word)` values, a
single area for globals, stack and frames, type 3 (address) values being indices into this
area here, so that a native writes its result like `StbArg_SetInt`: through the received address,
without touching the type. ROM limits: stack of 0x80 values, globals included, and 0x200
frames (`Event_InitStb`, `ov_001::021538d4`). The game does not check the former; here an
overflow, a wrong type (`FUN_0200159c(-1)`, the game's fatal error), an integer division
by zero, an out-of-area address or a loop of more than one million instructions without
`yield` stop the script with a `Fault`, never a panic. **[unknown]**: the effect
of a division by zero on the DS (the `_s32_div_f` routine is not analysed); **[hypothesis]**
`sin`/`cos`: the game converts to double and calls a software routine (`0200c578`,
`02009424`, `02008dcc`), reproduced by `f64::sin`, which may differ in the last bit.

The interpreter is a resumable machine: `Vm::run` stops at `yield` (0x17) and restarts at
the next instruction, like `Stb_Resume`. Operations 0x10 to 0x12, 0x15 and 0x17 have no
effect while word `+0x40` is set (`Vm::set_skip`); op 0x1c clears it and
suspends. No code sets `+0x40` outside the interpreter in what has been read:
**[hypothesis]** it is a cutscene skip (the skip test `ov_001::02156858` moves
`EventState_Run` to state 0xe and sets `+0x15c`, read by native 595). Not used by
the application.

**Event task.** The driver functions are in `ov_001` (names in
`re/symbols.txt`): `Event_InitStb` creates the machine, `Event_StartEntry` (`Stb_Start` then
`Event_UpdateActors`), `Event_Resume` (`Stb_Resume`, then the queues of the 24 actors and of the
camera; at the end `Event_End`). The order established by the data (the 30 scripts from 1.1 to 1.3):

1. entry **200** (`prepare`): the only one calling 566/567/568 (74 + 25 + 20 calls);
2. `Event_QueueLoads`, then `Event_BuildScene` build the declared cast;
3. entry **100** (`main`, field `+0x4a`, `EventState_Build`): all the staging;
4. entry **300** (`finish`, field `+0x4e`, `EventState_Finish`): state restoration.

The three numbers come from the event record (values 9 to 11 of the list, 100,
200, 300 everywhere). **[unknown]**: the call of entry 200 has not been found (`Stb_Start` has
only two callers in `ov_001`, for 100 and 300); its rank before the build is established
by the data only. An entry absent from the script (most often 300) does nothing
(`Stb_Start` returns -1). Native 538 chains the next event (`EventTask_Update`): the
`Director` replays it before the other pending events, and `Event::Ended` tells
the application to fire trigger 11, as the task does.

**Waits.** No native blocks: the script loops on an address output with `yield`.
The script therefore itself raises what becomes busy when it queues work (405 after 400, 204
after an actor command, 301 after a camera command, 507 after 506) and
the application lowers it through a `Signal` (`MessageClosed`, `ActorIdle`, `CameraIdle`,
`Preloaded`, `Answer`); the outputs only the application knows (actor pose 543/544,
current animation 234) are in `Runtime`. Fades have no end predicate: the
scripts count updates (840 returns `Frame::ticks`, 2 by default, see
[stb-natives.md](stb-natives.md#time-and-units-j0)). A message whose number is missing from the
script's table displays nothing and waits for nothing (`ov_001::0215e2a4`): real case in `ev21595`
(message 1).

**Natives.** 96 of the 120 prologue natives are handled: those that act yield a typed
`Event` (arguments decoded like the handlers, game units), those the scripts
poll answer from `Runtime`; 502, 503, 512, 587 and 724, which only
manage DS memory or hardware (like 706 to 709), are ignored without
report; the others are reported **once** (`Event::Unimplemented`) and counted, never
fatal. Identifiers outside the table ranges (`0–9`, `100–122`, `200–240`, `300–328`,
`400–421`, `500–603`, `700–738`, `800–845`) are ignored as in `Stb_CallNative`. 596
(`StbNat_GetActorStatus`, `ov_001::02161ab8`) looks up the 566 declaration whose second
byte equals the argument: category (`byte0 >> 4`, 1 local hero, 2 other character, 0 otherwise)
and state; absent: -1 and 2. The state of a declared character (0 here) comes from its animation
controller **[hypothesis]**, modes 9 and 10 of 566 are not modelled **[unknown]**.

**Result on the prologue** (`cargo dq9re script 1.1-3`, test `prologue_scripts_run_to_completion`):
the 30 scripts run to completion, with no machine error, from 56 updates (`ev01140`) to
3,579 (`ev21500`, 1,023 events, about two minutes at 30 Hz; it chains `ev21507` via
native 538); every `Text::Id` message
exists in the script's table; 7 natives remain without a modelled effect in `dq9-core`:
547, 560, 571, 581, 582, 809 and 815 (the same as "unidentified" in
[stb-natives.md](stb-natives.md)). Native 559 now writes the `+0x490` signal read by
trigger condition `0x1f`; this write must precede the end-of-cutscene trigger.
This is
not an emulator validation: order and durations are not compared against melonDS.

## On-screen execution (`dq9-core::cutscene`, `dq9-app`)

Implementation: `crates/dq9-core/src/cutscene.rs` (cutscene state, no I/O, tests),
`crates/dq9-app/src/event.rs` (`EventPlayer`: `Director` + rendering), actors and movements in
`dq9-render` (`AnimatedScene::add_actor`…); usage and per-cutscene result in
[`app.md`](../app.md), § "Events". No game function is ported in full
(no `port:` marker): these are behaviour models drawn from the natives
([`stb-natives.md`](stb-natives.md)) and the data. The points below are what the
30 prologue scripts require; this is not a comparison against melonDS.

**Established by the data** (checks on the 30 scripts, `cargo dq9re script`):

- Event list (`eventlist6.bin`, `eventlist_lv5.bin`, records 102): value 4
  is the cutscene's map (`ev21500` → 1112 `M01M12`, `ev21520` → 20101 `F01M01`, `ev01130` →
  4105 `X01M05`), values 12 to 15 the hero's starting position (x, y, z) and angle, all
  zero (or integers) when the script places the characters itself (`dq9_core::script::load::place`).
  The first record of stage 1.1 in `eventlist_lv5.bin` is `21500`
  (`stage_opening`).
- The 566 declarations number the cast members by their `attach` field: member
  `n` is that of the binding slots and actors numbered `n` (`ev01130`: actor 0
  = hero, `attach 0` mode 0; actor 1 = NPC 105, `attach 1` mode 5; actor 2 = model
  `effect/ev999990700.chr`, `attach 2` mode 2; `ev01330` declares 3, 4, 2, 1 in that order).
  Modes: 0 the hero, 2/3/6/7 a model (or sprite) with a given path, 4/5 a map NPC by its
  number (`reference`). Mode 4 too: `Event_BuildScene` looks it up in the NPC table
  (`NpcTable_FindById`) and reserves a `Chara_Get` slot for it ([stb-natives.md](stb-natives.md#effects-executed-on-screen-j3));
  its movements (`stand`, `walk`, `talk_loop`) come from the NPC's archive and the event's
  packages.
- An explicit binding (202) targets a character by its type: 4 with character 0 is the
  hero, 1 with a negative identifier a model loaded by 200/228/233, 2 a map object.
- A clone (228) sees the animation packages later added to its original model: in `ev21507`
  character −5 (clone of −4) plays movements only −4 receives (`LoadModelArchive`),
  likewise −12 in `ev21500`. `RemoveAnimationPackage` is called for each.
- The opening camera (572, `eye`/`lookat`) is carried by a polygon-less model
  (`ev01000cc000.chr`, `ev01000c000.chr`, movement `cut_00`/`cut_01` of a slice of the
  `nsbca` file of the type-0 package): the eye and the target are the bones of that name in the posed model, in the
  actor's frame. At the 0x10a/4096 scale every loaded model receives, the eye and target of
  `cut_01` land a few units from the characters the script places shortly before (eye
  (−11.75; 12.35; −14.58), target (−27.40; 0.65; −9.20), characters around (−27; −0.3; −9));
  at scale 1 they would be 400 units away.
- Every model loaded by 200 (`0215b72c`), cloned by 228 (`0215ba64`) or monster loaded by
  233 (`0215ca4c`) is a new `Object3D` with `SetScale(0x10a, 0x10a, 0x10a)`: constants
  `0x10a` read in overlay 1 at `0215b9c0`, `0215bb38`, `0215ccd8`. The third argument
  only selects the allocator (`021658b8[selector]`). Native 211 (`0215c330`)
  multiplies its three floats by 4096.0 (`0215c3e4`) and writes the absolute scale
  (`Object3D::SetScale`, `020374f0`, `+0x5c..+0x60`). Check: the old rule "selector 1
  → scale 1" rendered the villagers, horses, Aquila and monsters (`s058`, `s064`,
  `s066`, `s067`, `s070`, `z063a`…) in `ev21500` 15 times too large and pushed the camera away from its carrier
  `ev01000cc000.chr` (edges of the starry sky visible); with 0x10a, `--event 21500` and
  `--event 21520` are framed (`build/evidence/cutscene-load-scale/`, before `before_late.png`
  `ad50fc44cb16`, after `after_late.png` `4a1e9b1c9077`, sky `after.png` `7d25b277dc0d`).
- A cutscene's messages are closed sometimes by the player (the script loops on 405), sometimes by
  native 401 after a duration (`<CEN>` texts of the cinematics, frame removed by 410).

**Hypotheses** (marked in the code):

- An actor's commands are grouped into six channels (position, rotation, animation, scale,
  alpha, radius); the game's eight queues have not been mapped one to one. An instant command
  takes no tick, the actor is idle when all channels are empty; 210 does not block,
  224 and 213 wait for the current movement to end (end = the end of the slice reached
  once, including for a looping movement).
- Camera durations (304 to 322) and path durations (217) are counted like the actors'
  (2 ticks per unit); the half field-of-view angle of 532/580 is the game's (15° by default); the
  roll 327 rotates the up vector around the line of sight, positive towards the right of the image.
- `Event_BuildScene` does not itself bind the starting position of a map NPC: it keeps
  its place until the first command, and gets it back at the end of the cutscene.
- Fades: the "main" engine is the 3D screen; a locked engine (112, 114, 116) ignores
  simple fades (100, 101, 105, 106, 107) until a fade that unlocks it
  (117 to 122); the state returns to normal when the last cutscene ends.
- Lighting: the duration of 578 is counted like the actors' (2 ticks per unit, the game
  multiplies it by `s16(02010218(save))`, ≈ 33 ms); the phase forced by 588 is taken at the start of the
  phase, without fading to the next; 579 and 808 change the application's time of day
  (map reloaded afterwards). The phase of basic maps is set as on entering
  an area (`settle_basic_entry`) only when the script does not force it.
- A model attached to a bone (235) is drawn in the parent's frame × the bone matrix
  (`HeroRig::bone`, `actor_bone`), its own pose being relative to that bone; the parent's scale
  is inherited **[hypothesis]**, which the scripts reset to 1 for the attached model.
- The bubble (541) is a sprite placed on the ray through the DS screen pixel (actor
  projection offset by `(−6, −76)`), slightly in front of the actor, at a scale of one texel per DS
  pixel **[hypothesis]**: the game draws it in 2D over the image.
- Placements 574/575: `Scene::build` builds an instance for every model or
  object placement, including those hidden by the chosen time (`Instance::hidden`, bit `0x4` of `+2`).
  The hidden instance has a zero scale but keeps its original matrix (`SceneInstance::home`,
  `0x02013168`); 574 and 575 change the scale and translation from this state: a 575
  alone moves the placement without showing it, 574 shows it at its new position (in either
  order), until the map is reloaded, like the game which keeps the record's
  fields. Collision placements have no instance: their wall/floor change
  is not applied and the report counts it (`collision placement`) since the
  "remaining cutscenes" batch.
- Captions (409/411/413): `W+0x9a8` gives the hold; `020658b8` varies `W+0x9b0` by
  `0x8444` per frame up to `0x1f0000` (60 frames per fade), `02066d00` derives the text alpha
  from it. `0206b6c8` moves the window to `(192 − (lines − 1) × 20 − 8) / 2 − 16` (411) and
  `02045608` cuts the beep when `W+0x19b2 == 0` (413). Executed by `Talk`: quad alpha,
  centred text when 410 has removed the frame, blips filtered; the message counter is
  **frozen** during the caption (`020657c8`: the action does not close before 401, 405 stays 1).
- Three-bone camera (552): `0204a170` follows the source's `eye` and `lookat` and sets the target on
  `foot`; the latter is turned every three updates towards its displacement. Executed by
  `Camera::ThreeBoneCamera` and `EventPlayer::drag_three_bone` (`event/camera.rs`); its
  capture at frame 1820 of `21507` shows the change of viewpoint, without melonDS comparison.
- Door (584): `02018b44` finds the area's `(block entry, identifier)` placement;
  `02018310` sets the target angle to `+6434` (90°) and the speed to `409` per update. Executed by
  `Event::OpenDoor`: rotation of the placement visible at frames 2900→2960 of `21500`; the sound
  (index into `g_DoorSeTable` depending on the area's low nibble) is still reported.

**Executed since J3**: lighting and time (578, 579, 588, 589, 548/549, 808) and fog (804)
via `dq9_core::cutscene::SceneLight`; bone attachments (235, 236); named movements of map
NPCs; script 2D sprites (521/522); bubbles (541/542/561); placements (574/575);
movements 545/546; palette (238); camera return (328) — details and evidence in
[stb-natives.md](stb-natives.md#effects-executed-on-screen-j3).

- Screen swap (109/110): `Cutscene::screen_swapped` keeps `POWCNT1` bit 15 across
  events; `screens::field/overlay` and `app.rs` (window and capture) put the field and the
  window on the other screen.

**Not executed** (reported by `--events-report`): native 547 (battle transition, outside v0.1);
the door sound (584); 560, 571, 815, 581, 582 and 809 have no established visible effect or
no drawn object.

## Prologue chaining (`dq9-core::scenario`)

What chains the cutscenes is established (EU ROM, Ghidra, `cargo dq9re triggers`):

- `ev21500` → `ev21507`: the script's native 538 (`Director`).
- `ev21507` → `ev21520`: the end of `ev21507` fires trigger 11 of `M01` (`8 : 21507`), whose
  action `0x78(26)` is **a scripted battle**, not an unknown task (`trigger.md`, § Scripted
  battles: `Trig_RunDeferred` case `0x78` fills the battle task `+0x3718`, number 26 in
  `eventbattle.bin`). Victory fires trigger 15 (`12 : 26`), which starts `ev21520`
  (`triggerM01.bin` and `triggerF.bin`). There is no battle before J4: the application
  declares it won (`scenario::stand_in`).
- `ev21520 → 21521 → 21522 → 21596`: natives 538; map changes (`F01M01`, `X01`,
  `X01M01`, `X01M05`) requested by the scripts from the event list
  record. At the end of `21596`, trigger 11 of `X01M05` raises flag 337, gives item
  22159 and hands control back to the player.
- After that, everything comes from triggers: conversations (trigger 0 then 1 on close:
  `Field::talk_ended`), areas (2), doors and warps (17, 6), arrival (20, 3), and trigger 11 of
  cutscenes (follow-up, stage `0x84`, transport `0xe1`/`0x85`). Example: `21591` (going down to Angel
  Falls) starts from the conversation with NPC 101 (`6 : 101`, `11 : 193`, `16 : 0` → `0x77(21591)`),
  ends with `0xe1(1100)`; `21592` starts from the arrival trigger (3) of `M01` at stage 1.2;
  `21593` from a conversation (NPC 99) and ends with `0x84(1, 4, 1)` (stage 1.4) and `0x85(4105)`.
- Full run: `dq9-app --new-game --headless --auto [--pilot]` (`app.md`, § "New
  game").

## Open questions

- Natives still without a role (571, 560, 547, 815): "unidentified" table of
  [stb-natives.md](stb-natives.md#angel-falls-prologue-natives-used-j0).
- Palette of 238: modes 1 and 2 are not observed; objects of 809 and shadows of 581/582
  **[hypothesis]**.
- Nature (30 Hz or 60 Hz) of `currentNumTicks_`: [hypothesis] VBlank, not demonstrated.
- Meaning of op 0x16, of op 0x1c and of the second word of function records.
- Unknown fields of the event lists.
- Caller of entry 200 and origin of `+0x40` (skip) in the event task.
- Door 584: area low nibble (sound index 200–204) not read, sliding branch (bit
  `0x20`) and reverse direction not modelled. 571 remains unidentified (bit `0x10` without reader,
  no established visible effect); 839 (`+0x265`) does not appear in the prologue (only
  `ev28920`) and is therefore not driven in v0.1.
