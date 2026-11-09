# Application `dq9-app` and renderer `dq9-render`

The shared native/Web application displays a map, its models, textures, animations, NPCs and the
animated hero (pieces of `chara_pc.gp2` assembled, idle/walk/run depending on speed;
[`re/characters.md`](re/characters.md) § "The hero"). `dq9-core` consumes the `.col2` collisions
([`re/collision.md`](re/collision.md)) for experimental terrain movement;
`dq9_core::field` tests the doors, areas and warps of the `.bmbl` at each step
([`re/map-archive.md`](re/map-archive.md)) and `dq9-app` loads the destination map it
returns; the map's lighting, fog and time of day are applied
([`re/lighting.md`](re/lighting.md)).

- `crates/dq9-core` (no I/O, compiles to wasm): `lighting` (day clock `DayClock`,
  phase `TimeOfDay`, selection and blending of the `.bats` entry, fog registers, light
  vectors); `CollisionWorld` assembles the `.col2` of the
  visible placements; `Hero` advances at the fixed step, snaps to the ground and is blocked by vertical faces.
  Speed and start are provisional ([`re/movement.md`](re/movement.md)).
- `crates/dq9-render` (no I/O, compiles to wasm): `Scene::build(map, file)` remains
  available for a static scene; `AnimatedScene::build(map, id, code, story, file)`
  adds animations and NPCs. `step(camera)` advances by one 60 Hz step;
  `face_camera` and `upload` prepare rendering. `Renderer` holds the GPU resources;
  `Camera` is the free camera.
- `crates/dq9-app` (winit + wgpu): the only crate that touches the system. Reads the ROM (file in
  native, `<input type=file>` on the Web), opens the window, runs the loop: fixed-step 60 Hz
  updates (accumulator), one render per displayed frame.

## Running natively

```powershell
cargo run -p dq9-app --release -- --map M01            # ROM: --rom, DQ9_ROM, or the single *.nds in rom/
cargo run -p dq9-app --release -- --map C01M09 --time morning --layout side
cargo run -p dq9-app --release -- --map F01 --clock 389 --screenshot build/scratch/app/f01-dusk.png
cargo run -p dq9-app --release -- --save rom/dq9.sav   # map, position, heading and time from the save
# One offscreen PNG frame, no window, then exit:
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/m01.png
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/village.png `
    --size 1024x768 --eye=-5,20,40 --target=-20,0,5
cargo run -p dq9-app --release -- --map M01 --frame 60 --screenshot build/scratch/app/m01-f60.png
cargo run -p dq9-app --release -- --map C01M09 --chapter 19 --step 2 --substep 1 --night
# No window: walk following a script (world axes, seconds) and log maps
# and positions, doors included (enter a house in Angel Falls and leave it):
cargo run -p dq9-app --release -- --map M01 --start=-19.5,-9 --walk "-z:3,+z:3,idle:0.5"
# A screenshot after inputs: walk then talk to the NPC in front ("Conversations"):
cargo run -p dq9-app --release -- --map M01 --step 2 --start=5.4,2.5 `
    --input "+x:0.05,a,idle:1.2" --eye=4,3,14 --target=6,-2.5,2.5 --layout top `
    --screenshot build/scratch/app/hugo.png
```

With `--screenshot`, `--walk` and `--start` drive the hero during the `--frame N` fixed steps
(the script runs from step 0, then the hero stays still): the screenshot shows the gait
chosen from its speed, for example walking west from Angel Falls:

```powershell
cargo run -p dq9-app --release -- --map M01 --walk "-x:5" --start=-1.74,1.22 --frame 20 `
    --eye=-1.5,0,-3 --target=-3,-0.6,1.2 --screenshot build/scratch/app/hero-walk.png
cargo run -p dq9-app --release -- --map M01 --frame 10 `
    --eye=-1.74,0.3,-3.5 --target=-1.74,-0.5,1.22 --screenshot build/scratch/app/hero.png
```

`--walk` plays the **same fixed step as the window** (`App::run_steps`: `Hero::step` with the
D-pad keys the direction asks for, camera at yaw 0, then `Field::step`, doors and
field requests): `+x`, `-x`, `+z`, `-z` (two axes make a diagonal: `+x-z`) or
`idle`, a colon and a duration; `--start x,z` puts the hero on the ground at that point (also with
`--screenshot`, facing +z). Positions and segments are written to standard output;
map loads and door openings to standard error. If the map does not
load, the command fails instead of succeeding with "no map". An internal door
opens as in game, for example the double door of the library in `X01M07`:

```powershell
cargo run -p dq9-app --release -- --map X01M07 --start -8.6,-6 --walk=-z:2 --mute
# door 0/3 opens
# door 0/2 opens
# after step 0: map 4107 X01M07 hero (-9.307, 0.168, -9.381) yaw -0.000
```

### Conversations

In the window: `Enter`, `Space` or `Z` (A button) talks to the NPC in front of the hero (the most
"looked at" within 1.5 units, or inside its conversation zone: `dq9_core::talk::nearest`),
advances the text (while typing, shows up to the next wait) and closes at the
end; `Backspace` or `X` (B button) answers No; `↑`/`↓` or `W`/`S` move the cursor of
questions. The hero does not move during a conversation. The line is the one the game
chooses (`dialogue.md`) for the map, chapter, step, quests and time of day of the
application state, in English. On the Web the loaded ROM is enough: same keys.

`--input "SCRIPT"` (with `--screenshot`) plays inputs before the screenshot, with the same fixed
steps: `--walk` segments (`+x:0.5`, `idle:2`) and the buttons `a` (action: talk,
advance), `b` (cancel), `up`, `down`, one step each. The hero stays at its last position
(the orange marker in the screenshot); a script that changes map is an error. Example: the
Hugo screenshot above (step 2 of Angel Falls, `<N_TURN>//Hugo// …`).

The drawing of the frame, name, text and arrow comes from `dq9_render::ui`; the
layout constants are those of `MsgWin_Show` where they have been read, otherwise
hypotheses (`dialogue.md`, § "Message window"). The window is drawn on the top screen,
within the area it occupies (`screens.rs`).

`--map`: code (`M01`, `C01M09`) or id (`1100`) from the map list; it takes precedence over the
map read from `--save`. Without either option, the map stays `M01`. `--save` reads a
local `.sav` and selects the map of the first valid normal slot; no data is sent
over the network. The hero starts at the position and heading the game gives after "Continue" (the
map's start point, `re/save.md` § "Position on resume"), placed on the ground; the
`--screenshot` output prints this pose (`hero: …`) and centers the camera on it. `--time`: time of
day `night`, `morning`, `day`, `evening` or 0 to 3 (`LightingManager+0x98`: selection of
variants, of visible instances and of the lighting entry), by default the phase of the save's
clock, otherwise night; `--clock S`: second of the 420 s day (night 0, morning 180, day 210,
evening 390); its phase replaces `--time`, and the last two seconds of a phase blend
into the next one in an advanced zone (field); `--fog OFFSET,SHIFT,COLOR,D0..D31` replaces the
fog (comparisons with melonDS);
`--layout stacked|side|top`: DS screen layout (section "DS screens").

The cast follows the save's current scenario track when one is loaded:
progress, flags and quests are restored. Otherwise, it is that of the prologue `1.1.1` by day.
`--chapter`, `--step`, `--substep` explicitly replace the corresponding fields;
`--night` forces night placements and conversations, independently of `--time`.
With `--map`, the save provides neither the position nor the progress. The rest of the party and
the bag are not loaded. `--frame N` evaluates exactly N fixed steps from the start of
motions before the screenshot (0 by default), without depending on GPU load time.
Sprites are oriented toward the `--eye`/`--target` camera before this evaluation.

Controls: drag with the left button to rotate, wheel to zoom. In orbit mode,
`ZQSD`/`WASD` (physical keys) or arrows are the game's D-pad: the hero turns, accelerates and
walks at 5.76 units/s like `Character_Update`, blocked by walls and placed on the ground of the
map ([`re/movement.md`](re/movement.md)); `Q`/`E` (physical `A`/`E` on AZERTY) are the
game's `L`/`R` buttons: they rotate the camera by at most ± 45° around its yaw on entering
the map, together they reset it. `Tab` switches to free flight, where the movement keys
move the camera and `Q`/`E` move the target down/up; `Shift` speeds up the camera. `F` restores
the terrain orbit around the hero (distance 11, relative height 6, target at mid-height), or
frames the whole map in free flight. `R` changes the screen layout (`stacked`, `side`, `top`),
`T` next time of day ("Time" list on the Web), `M` mutes/unmutes the music, `PageUp`/`PageDown` previous/next map, `Enter`/`Space` talk, `Esc` quit.

**Sound**: on each map load, `dq9-app` plays the `bgm.sdat` sequence the game
chooses for it (`re/sdat.md`: music slot of `maplist9.bin`, night variant, overrides from
`mapbgm.bin`), through `cpal` natively and through WebAudio on the Web (the browser only allows sound
after a gesture: the music starts on the first click or first key press). Event
scripts play their effects (natives 712 to 738), jingles (`ME_`, the music ducks then
resumes) and music changes; the message window plays the typing beep, `<EXC>`,
`<QES>` and the opening of choices. `--mute` (`?mute=1` on the Web) opens no audio
device; `dq9re sseq <n> --wav f.wav` renders a sequence, `dq9re se <n> --wav f.wav` an effect
without a window; `--event N --headless-steps S --sound-wav f.wav` writes the mixed sound of an event
(the log lists the effects requested by the scripts).

## Events (`.stb` cutscenes)

The game's cutscenes (prologue cinematics, scripted dialogues) are `.stb` scripts
([`re/stb.md`](re/stb.md)) that `dq9-app` plays on screen:

```powershell
# The prologue: first cutscene of stage 1.1 (`21500`, Angel Falls, "Opening (Background)"),
# then, via native 538, `21507`. In the window; Enter/Space reads the messages.
cargo run -p dq9-app --release -- --stage 1.1
cargo run -p dq9-app --release -- --event 1130          # a single cutscene, on its map
# A screenshot after N 60 Hz steps (script, fades, messages and camera replayed
# from the start), with action pressed every 100 steps to read the messages:
cargo run -p dq9-app --release -- --stage 1.1 --frame 420 --press-every 100 `
    --size 512x384 --screenshot build/scratch/ev/hello.png
# Game logic without a window (triggers, map changes, cutscenes), logged:
cargo run -p dq9-app --release -- --stage 1.1 --headless-steps 60000 --press-every 100
# What each cutscene of a range of stages plays (table below):
cargo run -p dq9-app --release -- --events-report 1.1-3
```

`--stage MAJOR.MINOR` chooses the story point and the first cutscene of the stage in
`eventlist_lv5.bin` (`dq9_core::script::load::stage_opening`); `--event N` a cutscene by
number from `data/event` or `data/evspt_lv5`. A cutscene plays on the map its
event-list record designates (value 4), the hero at the record's position
when it has one: the application loads that map if it is elsewhere.
On the Web, `?event=N`. In the window, a trigger that requests `Request::Event` (actions
`0x77`, `0x80`, `0x9b`: entering a zone, talking) queues the event; at the end of an
event the application fires trigger event 11 (`Field::event_ended`), like
`EventTask_Update`.

**Architecture.** `dq9_core::cutscene` holds the cutscene state the script produced (actors,
camera, fades) without I/O; `dq9-app/src/event.rs` (`EventPlayer`) runs the `Director`,
applies this state to rendering and returns the `Signal`s to the script; `dq9_render::AnimatedScene`
provides the actors (`add_actor`, `play_actor`, `actor_bone`, `play_hero`, `set_npc_angle`), the
NPC motions (`play_npc`), the script `.spr` sheets (`add_sprite`) and the palette
(`tint_hero`, `tint_actor`, `tint_npc`).

- **Cadence.** A 60 Hz fixed step is a tick; the script advances every two ticks (one
  game update, [`re/stb-natives.md`](re/stb-natives.md) § "Time and units"), the scene on
  every tick. Timed moves, rotations, scale and alpha last `2 × duration` ticks;
  fades count ticks.
- **Models.** Before playing a cutscene, the application dry-runs it (every wait
  finishes immediately) to learn which `.chr` each character loads (natives 200, 228, 229,
  233, 566, 567), adds them to the scene and uploads the scene to the GPU once. A model without
  polygons (the prologue cameras, `ev01000cc000.chr`) has no instance: it keeps its
  skeleton and motions. An actor's motions: `BCFG` table of its archive and of the added
  packages (`LoadModelArchive`), a motion without its own file playing a slice of the
  file of the type-0 package. The hero plays the motions of the `evNNNNNp000.chr` packages.
- **Camera.** Eye, target, orbit, field of view (half-angle, 15° by default), roll, shakes,
  following an actor, camera carried by two bones of a model (native 572) and three-bone camera
  (native 552: `eye`/`lookat` of the source, `foot` carrying the target) are those of the script;
  the terrain camera resumes at the end, recentered on the
  hero's final pose without keeping the cutscene's close-up, roll or field of view. If the chain
  changed map, it resumes the destination's orbit; free flight gets back its independent
  view. A cutscene is framed in 4:3 on the top screen (`screens.rs`).
- **Messages.** The conversation window (`talk.rs`) shows text from the cutscene's table;
  without an end tag it waits for action (`Dialogue::for_event`); native 401 closes it,
  native 410 removes its frame. Natives 409, 411 and 413 make it a **caption**: timed
  alpha (`W+0x9a8`, one-second fade, hold, fade), text vertically centered under
  `(192 − (lines−1)×20 − 8)/2 − 16` when the frame is removed, and typing beep muted. During
  the caption, the message counter is frozen (`020657c8`): action can neither advance nor
  close the window before 401, so 405 does not fire before the end of the fade. The player is
  locked (no walking, no conversation).
- **Fades.** A black or white rectangle over the whole 3D screen, level −16 to +16; a
  locked engine (`112`, `114`, `116`) ignores simple fades. The "main" screen is taken to be
  the 3D screen **[hypothesis]**; the other engine's fade is ignored. The screen swap
  (109/110) keeps `POWCNT1` bit 15 in `Cutscene`; `app.rs` and the screenshot path use
  `screens::field/overlay` to place the field and the window on the other screen, the panel
  filling the freed screen (Top: no panel).

**Result on the prologue** (`--events-report 1.1-3`, EU ROM, English text; all 30 scripts
run to completion, no "not played" cutscene; sound is played by `audio.rs` and reported
nowhere). "Complete": everything the script asks for is executed, except for natives with no rendered
effect (559, 560, 571, 815; 581, 582, 809 whose objects are not drawn; motions whose
model lacks the name:
[`re/stb-natives.md`](re/stb-natives.md#effects-executed-on-screen-j3)). **Collision
placements** (574/575) are no longer excluded: the report counts them since the "remaining
cutscenes" batch. Before the J3-effects work: **22 partial cutscenes out of 30** (18 report
rows); after: **2 partial cutscenes** (1 row).

| Cutscene(s) | Maps | Result | What is missing |
|---|---|---|---|
| 1130, 1140, 1150, 1160, 1170, 1310, 1320, 1340, 1350, 1360, 1370, 1380, 1390, 1410, 1600, 1610 | `X01M05`, `X01M07`, `X01M01`, `M01`, `M01M04` | complete | — |
| 21520 → 21521 → 21522 → 21596 | `F01M01`, `X01`, `X01M01`, `X01M05` | complete | — |
| 21591, 21592, 21593, 21594, 21595, 21597 | `X01M05`, `M01`, `X01M04` | complete | — |
| 1330 | `M01M06` | complete | — |
| 21510 | `X01M04` | complete | — |
| 21500 + 21507 | `M01M12` | partial | collision placement ×7 (walls/floor not modified), door sound ×1 (index not read), motion absent ×1, 547 (battle transition, J4), 571 (no reader) |

The prologue chain plays end to end without intervention: `21500 → 21507` (native 538),
then trigger 11 of `M01` launches scripted battle 26 (action `0x78`), whose victory (trigger
15) launches `21520 → 21521 → 21522 → 21596` (natives 538), see "New game" below.

**Cutscene effects** (J3, `dq9_core::cutscene`, `dq9-app/src/event.rs`, details and evidence in
[`re/stb-natives.md`](re/stb-natives.md#effects-executed-on-screen-j3)):

- **Light and time.** 578 (light scale with fade), 588/589 (cutscene phase), 548/549,
  804 (fog) build the rendering `Environment` via `dq9_app::lighting::scene_environment`
  (`Lighting::select`: the scale weights the lights, background and fog, like
  `lightRGBScale_`); 579 and 808 stop or set the day clock. `--screenshot`
  captures apply the same states; in the window the environment is replaced when the
  script changes it and given back to the map at the end.
- **Bone attachments (235/236).** A model attached to the `head` or `chest` bone of the hero (halo,
  wings) or of an actor follows that bone's pose (`AnimatedScene::hero_bone`, `actor_bone`).
- **Map NPCs.** Named motions of model NPCs (`play_npc`, NPC archive, extra
  package and event packages); `walk`/`stand` of sprites; "!" bubbles (541/561/542);
  `.spr` sheets created by script (521/522); placements hidden, shown or moved (574/575);
  character palette (238); camera return to the hero (328).
- **Captions (409/411/413).** The message window becomes a caption: timed alpha (fade
  of 60 ticks, hold `W+0x9a8`, fade of 60), text vertically centered when 410 has removed
  the frame, typing beep filtered. Screenshots: `21500` frame 420 (partial alpha, pale text) and 600
  (full, no frame, centered).
- **Three-bone camera (552).** `eye`/`lookat` of a camera model give the eye and the aim, its
  `foot` bone carries the target character; screenshot `21507` frame 1820 (viewpoint high in the sky),
  versus 1780 before the setup.
- **Door (584).** The placement `(block entry, identifier)` rotates by 90° in 16 updates;
  screenshots `21500` frames 2900/2960, where the panel changes orientation. The sound stays reported
  as long as the index (`g_DoorSeTable[low nibble]`, 200–204) is not wired.
- **Screen swap (109/110).** The `POWCNT1` bit 15 state is shared with the layout; six
  swaps in the opening move the field and the window from one screen to the other. Screenshots
  `stacked`/`side`/`top` before/after the first 109 (frames 100 and 5000 of 21500).

`--eye`/`--target` with `--event` and `--screenshot` look at the scene the script left
(they replace the script's camera): useful to inspect an actor.

**Limits and hypotheses.** See also [`re/stb.md`](re/stb.md), § "On-screen execution": default binding
of declared member `n` to slot and actor `n`, actor channels, roll direction, unit of
camera and light durations, terrain camera pose for 328, bubble placed in the
world. The game's hero has wings and a halo (winged form) that the renderer does not draw; outside
events, NPCs keep their initial motion; NPC shadows are not drawn. The
models are not yet compared with melonDS. For 584, the sound and the sliding/reverse
branches are not read (the zone's low nibble is the only source of the index);
for 552, byte `+0x265` of 839 is not driven, 839 not appearing in the
prologue cutscenes (only `ev28920`). 571 remains unidentified: bit `0x10` with no reader, no established
visible effect.

## New game (`--new-game`) and automatic run

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
replayed gives **the same 615 checkpoints, byte for byte** (sha256 `c2b10c767640`,
`build/cast-reload/`), which is the evidence of reproducible inputs: the driver no longer reloads
the map when the story advances, it sees the cast that the game's action `0x8d`
rebuilds, like the replay and a player. The driver gives world directions with the camera at
angle 0 and `--replay` forces the same heading: the trace proves the headless driver's evidence in
our engine, not a replay with an arbitrary manual camera nor a melonDS gamepad.

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

**Scripted battles.** There is no battle before J4: `scenario::stand_in` declares them won
immediately, which the log states explicitly ("stand-in, no battle system yet"). The
victory fires trigger 15, as `BattleTask_Update` would (`re/trigger.md`, § Scripted
battles).

**Result** (`--new-game --headless --auto`, EU ROM, English text; game time at 60 Hz):

| Time | Map | Cutscene / fact |
|---:|---|---|
| 0 s | `M01M12` | `21500` "stage_opening" (80 s), then `21507` (43 s) via native 538 |
| 123 s | — | trigger 11 of `21507` → **scripted battle 26** (monsters 290, 292, 290; arena 30116), won by stand-in → trigger 15 → `21520` |
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

Precise limit: on `X01M01` (Observatory tower), the upper terrace is not connected to
the story pose by the walking collision. A **3D** exploration (edges validated by
`Hero::step` with the height reached, climb ≤ 1.0, drop ≤ 10) from the story pose
as from the map's start plateaus at **y 11.25**; the `X01M03` warp is at y 15.5. The
thin wall `X01A01S1` (block 0, placement 12, normal `+x`, `x 0.018..0.105`, `z 15.42..18.99`,
`y 9..14`) separates the terrace from the foot of the ramp. The `0x73` zones of **type 1** (43 in the
ROM, three on `X01M01`) are **inert**: no routine of `Field_CheckRegions` reads their
list, it is neither a lift nor a task 8. The missing connection (moved collision
placement, animated platform or scripted map change) is **[unknown]**, to be
established on melonDS. The driver therefore stops at 1.1.2; chapter 2 is not reached and
no teleport enters the evidence.

**Stops where and why** (end log of `--pilot`): player control with nothing that
advances the story according to the simulation (conversations that no longer change progress or
flags, areas already taken), or step cap reached. Known limits:

- Battles are not played (stand-in); experience, party and items are not
  modeled, so triggers that depend on them (conditions `0x24`, `0x31`, `0x32`…) can give
  other branches than the real game: only the hero's opening outfit is set in the state.
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

## Running in a browser

### Hosted demo (GitHub Pages)

Open **https://fablrc.github.io/dq9-rust/?newgame=1**, choose your European ROM `YDQP01`:
the "New game" button becomes active as soon as the ROM is read, and the keyboard is given back to the game
without an extra click. Arrows: walk; Enter/Space: talk and advance messages;
Q/E: camera; M: sound. A DS save is optional.
Files are read locally: no ROM or save is sent to GitHub.
A computer with a keyboard and a recent browser with WebGPU or WebGL 2 are recommended.
The limits of the technical proof still apply: battles skipped, incomplete effects
and provisional bottom screen.

The ROM/save fields and the selectors stay disabled during the download and
initialization of the engine. "New game" stays disabled until the bytes of
the ROM are queued for the game loop: the order ROM then new game is therefore
guaranteed, and the ROM field is disabled while it is being read (no concurrent reads). If
loading fails, the page shows an error instead of accepting a file it could
not read.

The keyboard returns to the canvas after choosing a file, a click on "New game",
a change of time of day or layout; clicking in the image also takes it back. Game
keys do not leave the canvas: outside the page's fields, selectors and buttons,
arrows, Space, Page Up/Down and Home/End do not scroll the page. A loss of
window focus (tab change, file dialog) releases held
keys: `web.rs` removes focus from the canvas, winit emits `Focused(false)` and `App` clears its
key set, so the hero does not keep walking. Errors (unreadable file,
ROM that is not `YDQP01`, missing graphics engine) are shown in red in `#status`;
a rejected ROM disables "New game" again.

The ROM and save fields are cleared as soon as a file is read: choosing the same
path again (same ROM after an error, same save after "New game") therefore triggers
a new `change` and replaces the current game. Without this the browser sends no
event and the file stays the one from the previous session.

The `.github/workflows/pages.yml` workflow builds the optimized wasm version on each push
to `main` that touches the web binary (`crates/**`, `Cargo.toml`, `Cargo.lock`,
`rust-toolchain*`) or the workflow itself, as well as on manual dispatch: a
documentation commit redeploys nothing. It installs the `wasm-bindgen-cli` version from `Cargo.lock`
and deploys only `index.html` and the JavaScript/WebAssembly bindings to GitHub Pages.
An allowlist checks the artifact before upload (only `index.html` and
`pkg/*.js`/`pkg/*.wasm`): any ROM, save, screenshot or other file fails the
job. Deployment is conditioned on `refs/heads/main`, so a manual dispatch on a
branch publishes nothing. The repository remains private; the site and the compiled program are public.
The relative path `./pkg/dq9-app.js` lets the page be served under `/dq9-rust/` without a custom
domain name. No game data enters the Pages artifact.

### Build and local server

```powershell
cargo build -p dq9-app --release --target wasm32-unknown-unknown
# Once: the wasm-bindgen-cli version must match Cargo.lock.
cargo install wasm-bindgen-cli --version 0.2.130 --locked --root tools/wasm-bindgen
tools/wasm-bindgen/bin/wasm-bindgen --target web --no-typescript `
    --out-dir crates/dq9-app/web/pkg target/wasm32-unknown-unknown/release/dq9-app.wasm
python -m http.server 8000 -d crates/dq9-app/web     # or any static file server
```

Open `http://localhost:8000/` (parameters `map`, `time` (name or 0 to 3), `clock`, `layout` (`stacked`, `side` or `top`), `chapter`, `step`,
`substep`, `night=1`, `mute=1`, `newgame=1`), then choose the ROM and, optionally, a `.sav` save in the
page. Both files are read locally in the browser, never sent or hosted;
an explicit `map` takes precedence over the save's map. The "New game" button (or `?newgame=1`)
launches the prologue (§ "New game"); it stays disabled until the ROM is read and
the keyboard returns to the canvas afterwards. The page reports errors in red and names the
rejected file.
WebGPU when the browser offers it, otherwise WebGL 2. `crates/dq9-app/web/pkg/` is generated
(ignored by git).

## Development loop

`scripts/verify.ps1` is the single source of verification commands: AGENTS.md, the docs
and CI call it. It chains five stages in this order and stops at the first failure:
`fmt` (`cargo fmt --all --check`), `clippy` (`cargo clippy --workspace --all-targets --
-D warnings`), `test` (`cargo test --workspace`), `wasm` (build of `nds-rom`,
`dq9-formats`, `dq9-core`, `dq9-render`, `dq9-audio` and `dq9-app` for
`wasm32-unknown-unknown`) and `coverage` (`cargo dq9re coverage --readme README.md`).

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1                    # all five stages
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Fast              # fmt + clippy
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Only test,wasm    # subset
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -RequireRom        # integration gate
```

`-Fast` is a short pass while editing. `-Only` takes stage names (array or
comma-separated string) and runs them in canonical order; an unknown name is an
error, and `-Only` excludes `-Fast`. The final summary gives each stage's duration.

Locally, a ROM also runs the `tests/real_rom.rs` tests, which CI skips. The
lookup is that of `crates/testrom`: `DQ9_ROM`, otherwise the `*.nds` in `rom/`, otherwise the one
at the root. `-RequireRom` fails before any stage if this lookup finds nothing
(or a nonexistent `DQ9_ROM`, or several `*.nds` in the same folder) and prints the selected
path: it is a batch's integration gate, which guarantees the ROM tests have
run.

The `coverage` stage rewrites the README's progress block locally from the `port:`
markers; the updated `README.md` is committed with the batch. CI recomputes the block, shows it
in the job summary and fails if it differs from the committed README (stale block); it never
commits.

Branch integration (`scripts/integrate.ps1`), the evidence manifest
(`scripts/evidence.ps1`, protected by `scripts/prune-build.ps1`) and branch cleanup
(`scripts/prune-branches.ps1`) are described in `docs/orchestration/README.md`.

Orca worktrees need not recompile the same crates: sharing the Cargo target
is enough.

```powershell
$env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\dq9-target"   # cmd: %LOCALAPPDATA%\dq9-target
# For all sessions: setx CARGO_TARGET_DIR "%LOCALAPPDATA%\dq9-target"
```

Cargo locks a target: two worktrees building at the same time wait for each other instead of
stepping on each other, and the first build fills the cache for the following ones. The shared target
stays outside the repository (`build/` is only for project outputs: screenshots, extracted ROM).
A shared target can reuse a test binary compiled in another worktree, even a
deleted one: a path frozen by `env!("CARGO_MANIFEST_DIR")` then designates that worktree. The
tests therefore read `CARGO_MANIFEST_DIR` at runtime (`crates/testrom`), and
`verify.ps1 -RequireRom` passes the found ROM to the tests through `DQ9_ROM`.

`sccache` is optional: it caches `rustc` invocations, useful when the shared
target is absent or discarded (branch change, cleanup), useless when it is
warm.

```powershell
cargo install sccache --locked
$env:RUSTC_WRAPPER = 'sccache'
```

`cargo nextest` is optional too: it runs integration and unit tests
faster and names the slowest ones. CI keeps `cargo test`; workspace doctests are not
part of a nextest run and remain covered by `cargo test --doc`.

```powershell
cargo install cargo-nextest --locked
cargo nextest run --workspace
```

## Rendering

What `dq9-render` does, and what deviates from the DS 3D engine.

Faithful to what the game reads:

- **Instances**: one per model and object placement (`.nsbmd`), including those the chosen
  time hides (`Instance::hidden`): the hidden instance has a zero scale and keeps its
  original matrix (`SceneInstance::home`), so that natives 574 and 575 can show or
  move it; world matrix = translation, `y` rotation, scale from `Instance::world`
  (`0x02013168`). `dq9-core` decompresses the `.col2`, transforms the visible
  faces and uses them for floor and walls (see `re/movement.md`).
- **Textures**: all the `.nsbtx` of the `.ambl` (`Zone3D+0x418`), bound to materials by
  texture and palette name. When several files have the same name, the first in the
  list (the most recent) wins **[hypothesis]**. A texture that no file provides
  (30 in the ROM, `Scene::unbound_textures`) leaves the material untextured **[hypothesis]**.
- **Materials**: vertex colors, polygon alpha (`POLYGON_ATTR` bits 16-20), culled
  faces (bits 6-7, front faces counterclockwise), modulation and decal modes
  (bits 4-5; all map materials are modulation), repeat and mirror
  (`TEXIMAGE_PARAM` bits 16-19), material texture matrix (`Model::tex_matrices`),
  nearest filtering, no alpha-0 pixel.
- **Passes**: opaque polygons then translucent ones (alpha < 31 or texture with partial alpha,
  A3I5/A5I3); alpha-31 texels of an alpha-31 polygon are drawn in the opaque
  pass, like the DS's opaque pixels; translucent pixels only write
  depth with bit 11; "equal" depth test (bit 14) after the other
  opaques.
- **Camera**: the game's by default (`0x0202df78`): vertical half-angle of 15°
  (`0x0202e9b4`, sine and cosine for `NNS_G3dGlbPerspective`), planes at 1 and 400; free in
  orbit or flight; 256×192 option.
- **Hero** (`dq9_render::hero`, `AnimatedScene::add_hero`): body and legs on the
  14-bone template, animated by `mp0200n`/`mp0200ne` (`stand`, `walk`, `run`), face, hair and headgear
  placed on the `head` bone, arms and shoes by texture files, merged into one model;
  oriented by heading (`angle_of_yaw`), scale `0x10a × build`; the gait comes from the
  distance covered per step (`Gait::from_speed`). Appearance: the first character of the
  save (`--save`), otherwise a default hero **[hypothesis]**; no weapons, no shields, no
  wings, no tints from `palette.bin` ([`re/characters.md`](re/characters.md)).
- **NPCs**: cast and placements from `cast::load`, `.spr` sprites decoded once
  (cut-out images), `stand_*` names chosen by `cast::sprite_stand`, billboards facing the
  camera; seated `sleep_*` variants with mirroring. Game scales: sprites `0x8f/4096`
  per pixel, models `0x10a/4096`; positions `fx32/4096`, rotation in radians.
  `.chr` archives, motion from `cast::initial_motion` (including the extra package of
  type 19), monsters from the `.mon` member of `enemy.gp2` then first `.cchr`.
- **Animations**: files explicitly selected by the flags of the map
  parts, G3D pose (`JNT0`), texture matrices (`SRT0`), colors/alpha (`MAT0`) and
  texture/palette changes (`PAT0`). Simple models loop over the full
  frame count (`Animation3D::AdvanceTimer`), objects with a BCFG package use the
  bounds and speed of the first motion; per-motion `.chr` use its speed
  and the file's range `[0, frames−1[`, not the old BCFG slices
  (see `characters.md`).
  Display lists are decoded once; poses only rewrite the
  vertices, without rebuilding indices or reallocating GPU resources. SRT/PAT alone do not
  redo the geometry. The animated CPU state is separate from the static scene API.

Approximated or absent:

- **Translucent sorting**: back to front by the depth of the center of
  each mesh × instance; the DS sorts by on-screen `Y` (or keeps submission order).
- **Lighting**: map models are lit per vertex by the two DS lights of the
  `.bats` (GBATEK equation: emission + per light `color × (diffuse·max(0, −L·N) + ambient
  + specular·shininess)`, on 5 bits; bits 0-3 of `POLYGON_ATTR`), model normals, zone ambient
  for the terrain in advanced mode. Base zones have no light: their
  vertex colors are transformed (hue, brightness, contrast); sprites and characters
  are multiplied by the zone's diffuse; characters keep an extra fixed light (the
  real DS lights go through their own matrix, not ported). Background: vertical gradient
  centered on the horizon; DS fog (offset, shift, table of 32 densities, color) on
  materials with bit 15 of `POLYGON_ATTR` and sprites; edge marking. The shininess
  table (`SetShininessTable`, bit 15 of `SPE_EMI`) is not ported.
- **Not rendered**: antialiasing, shadow polygons (mode 3) and wireframe (alpha 0), G3D
  billboards of models, fog alpha-only mode and `FOG_COLOR` alpha, DS precision
  (6-bit colors, W depth; fog depth is recomputed on 24 bits).
- **NPCs, limits**: NPCs neither stop nor turn toward the hero while he talks
  to them (sprites follow the camera; model orientation has no API in
  `AnimatedScene`); NPC heights are those of the placements; ground snapping and
  hero movement are approximate (no acceleration or radius).
  Doors, areas and warps follow `dq9_core::field` (compared with
  melonDS on the Stornway church) without the door animation; zone triggers are
  evaluated; their event requests are executed (§ Events), the other requests
  only logged, and stairs, falls and NPC paths are not executed. Network
  visitors, the `s069` accessory and additive palette tint (type 22) remain
  absent. Blending of several poses, `.beff`/`.bact` effects/actions and sprite
  modes other than standing/seated are not executed. MAT colors are evaluated;
  vertices with normals remain subject to the fixed light described above, not to the DS lights.

## DS screens

The window shows both DS screens (`crates/dq9-app/src/screens.rs`):

- **Top**: the 3D field, with the game's camera and projection (`0x0202df78`, vertical
  half-angle of 15°). The screen is 256×192; only the rectangle size varies with the window.
- **Bottom**: plain background (`dq9_render::Panel`), no content. The touch screen content is
  not identified **[unknown]**; no ROM data is displayed there.
- **Message window**: drawn on the top screen (`talk.rs`, `dialogue.md`).

Layouts: `stacked` (default, top screen above the bottom one), `side` (side by side),
`top` (top screen alone). The screens are centered and scaled together to the maximum of the
available space, without distorting their 4:3 ratio; margins stay black, independently
of sky, fog and fades. On the Web, the canvas follows the available size below the
controls and the screen's pixel density. Native: `--layout`, and `R` switches to the next
layout. Web: the "Screens" select or `?layout=`. The old `--ds-aspect` and `ds=` are removed.

## Verification

- Browser run on Pages (November 7, 2026, partial evidence, final build): successful
  deployment `42384404646`, commit `21e1e42`, URL
  `https://fablrc.github.io/dq9-rust/?newgame=1&release=21e1e42`, local European ROM
  of 256 MiB, driven Chromium, window 1280 × 900 (DPR 1.25). After reading the ROM, the
  keyboard is active on `dq9`; the visual opening reaches events `21500`, `21507`,
  `21520`, `21521`, `21522`, then `21596` and first control on `X01M05` (4105).
  Arrows and Space open on foot a conversation with NPC 108
  (`X01A0.gp2/108_en.bin`, record 0), which is advanced and closed by keyboard. A
  second new game replays the driver's directional keys from control
  4105: conversation with NPC 111, event `1140`, NPC 105 (context 80) and event `1130`
  reached visually and in the console. After cutscene 1130, short key presses
  and screenshots confirmed climbing the stairs up to the raised stone path
  and exploring the hall; the hero however stays on 4105, with no event or 4107 transition,
  even at the end of the path in front of the banner. Final screenshots `50` to `61` are in
  `build/v01-20261107/browser/`, with the screenshots of the raised path and the east wing in
  `browser-navigation-addendum.SHA256SUMS`. No save, teleport or injection of internal
  state. An accelerated replay of the native inputs in the final page reached 4105 and the dialogue
  of NPC 101, but its virtual clock desynchronizes inputs from fixed steps: it proves
  neither 4107, nor 1170, nor the tower, nor 1.2.1, and is not used as certification.
  **Final limit**: the full browser run up to Angel Falls and the village
  tasks is not certified; 4107, 1170, the tower, 21510/1.2.1 and 1100 remain verified by
  the native run and the ROM tests, not by a continuous browser session.
- Loading a save after "New game": native Windows scenario exercised
  with the EU ROM and a local save (`a_save_selected_after_a_new_game_loads_that_save`).
  `new_game` becomes false and `C01M09` (map 109) replaces the prologue. The test covers an
  event still pending, then an active prologue with an open caption: task and
  dialogue are cancelled and do not resume during the next 120 fixed steps. The test and
  its imports are compiled only on Windows; the browser path remains in wasm.
- Session reset on the published site (November 7, 2026): page
  `https://fablrc.github.io/dq9-rust/` (successful Pages deployment `42284105022`, head
  `9a25199`), local European ROM of 256 MiB
  (SHA-256 `f305494b…a63b1f`) and `build/sav/demo.sav` (C01M09, SHA-256 `cf0f43b4…1c24a8`),
  headless driven browser, 1280 × 900, DPR 1.25. A file that is not a ROM
  shows the error and disables "New game"; a valid ROM gives focus back
  to `dq9` and enables the button. "New game" then the same save immediately (before
  any caption): the console shows `event 21500 queued` then `event 21500 starts`, followed by
  a "save read locally" line and `109 C01M09`; the church is displayed with no caption or prologue
  cutscene, and about 3 s of up arrow go through the door (`map change 109 -> 100`)
  without any `event 21500` reappearing. Active-caption case from the coordinator: caption
  "Hello? Is there anybody there?" open, save loaded, church with no caption, then
  120 frames of up arrow with hero movement. Entering the church opens the
  conversation with NPC 25 (`C01Q0/025_en.bin`, "Hi! Welcome"). Screenshots and SHA-256
  manifest: `build/v01-20261107/web/` (`web-40` to `web-51`, `coordinator-*.png`,
  `SHA256SUMS`).
- Same file chosen twice (defect observed on the published page): after "New
  game", re-choosing the path already present in the save field triggers no
  `change`, so the save is not re-read and the prologue stays displayed; another path
  works. Fix in `web.rs`: clear the ROM and save fields as soon as the
  file is read. Evidence before the fix: `web-48-same-save-ignored.png` (prologue sky, field
  `demo.sav`, no "save read" line) versus
  `web-49-other-save-loads-c01m09.png`. No native test can exercise the `change`
  semantics of an `<input type=file>`; the native test
  `a_save_selected_after_a_new_game_loads_that_save` covers the back-end state (cancellation of
  the event and the dialogue), and the browser fix remains checked by the Chromium
  smoke. Local candidate (wasm release + bindgen, `scripts/verify.ps1` green in 45.6 s:
  fmt, clippy, workspace tests with real ROM, wasm build) served on
  `http://127.0.0.1:8007`: save loaded first, then "New game" up to the
  opening sky, then the same `demo.sav` re-chosen in the save field; the
  C01M09 church comes back, both file fields are empty, focus is on `dq9` and no
  page error appears. Screenshots `candidate-save-first.png` (SHA-256
  `949db6e7…f029d`), `candidate-newgame-between.png` (`23dc84fd…b0ab1`) and
  `candidate-same-save-reselected.png` (`ef296340…7943`).
- Internal doors of published version `17bef8c`: on `X01M07`, ROM chosen locally,
  keyboard walk around the columns then opening of both leaves (`door 0/3 opens`,
  `door 0/2 opens`) and visible crossing into the library. Screenshot kept
  outside the repository: `build/orca/pages-door-crossing-17bef8c.png`. Linux CI `42229577289` and
  Pages deployment `42229577139` successful; this smoke does not validate the whole prologue.
- Browser batch and delivery chain (November 6, 2026): local wasm page served by
  `python -m http.server` (`?newgame=1&mute=1`, Chromium 150, 1929 × 921, DPR 1, WebGPU and
  WebGL 2 available), local European ROM of 256 MiB read through `<input type=file>`. The
  server log only shows `/`, `pkg/dq9-app.js` and `pkg/dq9-app_bg.wasm`: no
  POST request, so no ROM or save sent. After reading,
  `document.activeElement` is the canvas, "New game" is active and `?newgame=1`
  launches `21500`; Enter advances the cutscene. A held arrow moves the hero
  (screenshots); a simulated focus loss (`window.blur`) gives focus back to the document and stops
  movement (two screenshots 1.5 s apart, same position), because `Focused(false)` clears
  held keys. Changing the layout gives focus back to the canvas; an arrow pressed
  outside the canvas leaves `scrollY` at 0. A non-ROM file shows the error "This file is not a
  European Dragon Quest IX ROM (YDQP01)." in red and disables the button again;
  the error stays displayed despite the engine's progress lines (sound effects), then
  a valid ROM re-enables the game. The `pages.yml` allowlist accepts `index.html` and
  `pkg/*.js|*.wasm` and rejects a `leak.nds` added to `_site` (local trial of the commands).
  Linux CI failed on `alsa-sys` for lack of `alsa.pc` (run 42212225607); the `lint`
  and `test` jobs now install `libasound2-dev` before building.
- Integration published on GitHub Pages (November 6, 2026): Web batch `aab6daa` passed
  the Linux jobs `lint`, `test`, `wasm` and `coverage` (run 42216397722), then Pages
  deployment 42216397713. On the public site, the local ROM immediately gives focus to
  `dq9`, enables "New game" and launches the opening; Enter advances the message.
  The keyboard run of the previous version reaches `X01M05`, allows talking to NPCs
  108 and 101, playing 1130 with NPC 105, then climbing the side stairs to the south
  and the central bridge up to the warp `X01M05 → X01M07`. No direct placement of the hero
  is used in this browser run.
- Integrated cutscenes batch `e6b885b`: CI 42219786799 and Pages deployment 42219786761 successful.
  The native screenshot of `21500` at frame 420 shows the caption fading in.
  On the public page `?event=21507&time=2&mute=1`, the local ROM launches the cutscene,
  keyboard inputs advance the messages and `21507` ends after 42.5 s observed;
  `21520` then starts. The report no longer lists 552 among absent effects;
  the cutscene view is captured, without comparison with melonDS.
  This cutscene smoke validates neither the full manual run nor the prologue flags.
- J3 batch (November 6, 2026): native screenshot of Ivor with the female branch of the demo
  save; cutscene 1330 rendered and initially hidden placements checked by a
  real test; `M01` screenshot identical pixel for pixel before/after this change. Event
  1160 executed to completion and mix written to WAV (6.28 s, 32,728 Hz), without listening or
  comparison with melonDS. Web bundle rebuilt: loading and replacing a save
  on `C01M09`, conversation with NPC 25 and start of a new game checked in
  Chromium (1024 × 820, DPR 1.25, sound muted), with no page error observed.
- Browser verification of the integrated version (Chromium, November 6, 2026, window
  1000 × 808, DPR 1.25, sound muted): ROM and save loaded locally, conversation
  with NPC 25 on `C01M09` with `C01Q0/025_en.bin`, line advanced then closed; stacked,
  side-by-side and top-only layouts with black margins. New game played up to
  the end of `21596` after `21500`, `21507`, `21520`, `21521` and `21522`: return to the hero
  on `X01M05`, walking and camera following checked on screen. Direct load of `M01`:
  terrain framing and walking checked as well. No page error observed.
  Limits kept: non-ported effects reported in the log, plain bottom screen,
  roofs/beams that can hide the hero; this run verifies neither sound nor frame-by-frame
  fidelity to melonDS.
- `crates/dq9-render/tests/real_rom.rs`: the 738 distinct archive sets (time 0,
  cave environment 1) build without error, hidden placements included:
  4,204 models, 4,349 instances, 19,557 decoded textures, 697,035 visible triangles;
  30,447 materials, of which 48 untextured and 236 with a texture matrix.
- `--screenshot` captures of `M01` (Angel Falls: village, waterfall, river), `C01M09`
  (Stornway church) and `F01` (Angel Falls plain): textures in place, foliage and
  flowers cut out, waterfall and mist translucent without holes.
- CPU integration: synthetic clocks (BCFG range, fractional speed, loop and
  single-frame file); cached poses compared with `Geometry::build_posed`
  at frames 0, 4 and 8 of `s000/walk`. Real test
  `runtime_cast_and_animation_are_deterministic`: same vertices and materials after
  60 steps on `M01`, `C01M09`, `F01` and `M01M08` (the latter with the monster of quest 152),
  same topology, body count consistent with the cast.
- Hero: `M01` screenshots of the hero idle from the front and walking in profile (legs, arms and
  hair animated, heading followed); real test `hero_model_follows_its_gait` (deterministic model, motion
  different per gait).
- Native integration screenshots: `M01 --frame 0` and `--frame 60` give different
  PNGs with an identical camera; `C01M09 --chapter 19 --step 2 --substep 1 --night`
  shows the sprites of the nun and the priest facing a `--eye/--target` camera.
  These are functional checks, not a pixel-by-pixel comparison with melonDS.
- Lighting: `M01` screenshots (day, night, morning, evening: fog and sky visible, gradient
  different per time); comparison with melonDS on `C01M09` at night with forced fog:
  mean difference 12.8/255 per channel without fog; fog density within 0.01 on
  five tables (details and limits: [`re/lighting.md`](re/lighting.md) § "Comparison with melonDS",
  `scripts/melonds-capture.ps1`). DS lights, sky and advanced-zone fade not compared.
- Browser: wasm bundle rebuilt with `wasm-bindgen`, ROM chosen locally in
  the page; `M01` and `C01M09` display the map and NPCs, with no JavaScript error observed.
  Native screenshots of frames 0 and 60 of `M01` differ on 41,143 pixels with an identical
  camera. Rendering has not yet been compared frame by frame with the DS's.
