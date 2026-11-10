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
`--layout stacked|side|top|wide`: DS screen layout (section "DS screens"); `wide` is the
single 16:9 cinematic presentation.

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
frames the whole map in free flight. `R` changes the screen layout (`stacked`, `side`, `top`,
`wide`); `T` next time of day ("Time" list on the Web), `M` mutes/unmutes the music,
`PageUp`/`PageDown` previous/next map, `Enter`/`Space` talk, `Esc` quit on the field
(cancel while a fight is in play).

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

## Field menu and facility screens

`I` on the field opens the game's menu — **Items**, **Equipment**, **Attributes**, **Spells**,
**Close** — unless a conversation, an event or a battle holds the player. A talk line carrying
`<INN=n>`, `<CHURCH=n>` or `<SHOP=n>` opens that facility when its message window closes; the
conversation ends (trigger event 1, talk counters) only after the screen closes, in the order
of the game's talk task. Movement and roaming freeze while any of these screens is open, `R`
still switches the layout, and a map change closes them. `Enter`/`Space`/`Z` confirm,
`Backspace`/`X` cancel, arrows or `WASD` move, `H` puts a bag item into a held slot, `T` takes
a held item back, `Left`/`Right` switch the shop between buying and selling.

The rules live in `dq9_core::facility` and `dq9_core::party`;
`crates/dq9-app/src/app/services.rs` adds the ROM tables, the cursors and the transfer into
`Party`, and [`re/facility.md`](re/facility.md) records what was read. A transfer moves **one**
copy and preflights the real destination, because `Bag::add` reports success while clamping a
full stack of 99: a swap that cannot store the old copy restores the chosen one, so no copy is
ever lost. Field item use and the leader's healing spells run through
`dq9_core::battle::resolve_action` with the battle's range table, healing cap and MP cost; a
group heal (Multiheal) resolves every living member in party order and spends MP once; a dead
member is never a target and a dead leader has no enabled command.

Equipment changes rebuild only the drawn hero (`AnimatedScene::replace_hero`), in the same
scene model and instance slots, at its live pose and gait; the field's cast, talk counters,
doors, events and generators are untouched and no map trigger replays. The inn moves the day
clock (time, lighting and the trigger's `time_of_day` follow) and heals the living only; the
church's Resurrection, Purification and Benediction charge the price the core computes and name
the member, Divination is free and returns to its row, and Confession is refused as unavailable
(v0.3).

Known limits: the window geometry, colours and title/footer are **[hypothesis]** (the field
message window's rectangle grown to the DS screen, on the screen the field does not use);
vocation restrictions on equipment are not modelled; a field item used at full HP still spends
the copy; the church's coffin line, Benediction's unequipping and the inn's guest lines are not
modelled; buying or selling is one copy per command and a partial purchase is not offered; a
sale at the gold cap is the documented **[hypothesis]**.

Evidence (`build/evidence/j4-field-services/`, native windows on the European ROM, sha256
prefixes): menu `field-menu.png` (`6669e1ad98e4`), held-item swap and use `held-swap.png`,
`held-use.log` (`298cdf4a7336`), equipment and attributes `equipment.png` (`f0d25fe2a8aa`),
`attributes.png` (`0379109e4882`), paid shop `shop-paid.png` (`cecbfc5c446c`, gold 100 → 92, one
herb), inn `inn-rested.png` (`94a10396507b`, the living healed, the dead untouched),
resurrection `revival-done.png` (`39dc17c14e4d`, 0 → 30 HP for 60 gold), Divination
`divination-result.png` (`044932510558`), Multiheal `multiheal.log` (`a691caa6479f`), Fullheal
`fullheal-after.png` (`c24ecb26688e`, HP 20 → 80 for 24 MP) and a dead actor's visible refusal
`dead-actor-refusal.png` (`f9f2c74e00c2`). The same transitions are covered without a window in
`app/services/tests.rs`, including the bounded hero replacement (alternating outfits keep the
model, instance and texture counts and the texel storage stable, and a failed resource leaves
the old rig drawn).

Controls here absorb the field's: `T` is "take a held item back" while a screen is open and the
next time of day otherwise.

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
rows); after: **2 partial cutscenes** (1 row). The battle scene batch executes native 547
(§ "Battle scene"): **no partial cutscene remains**, the 26 report rows are complete.

| Cutscene(s) | Maps | Result | What is missing |
|---|---|---|---|
| 1130, 1140, 1150, 1160, 1170, 1310, 1320, 1340, 1350, 1360, 1370, 1380, 1390, 1410, 1600, 1610 | `X01M05`, `X01M07`, `X01M01`, `M01`, `M01M04` | complete | — |
| 21520 → 21521 → 21522 → 21596 | `F01M01`, `X01`, `X01M01`, `X01M05` | complete | — |
| 21591, 21592, 21593, 21594, 21595, 21597 | `X01M05`, `M01`, `X01M04` | complete | — |
| 1330 | `M01M06` | complete | — |
| 21510 | `X01M04` | complete | — |
| 21500 + 21507 | `M01M12` | complete | — (571 ×2: no reader; one motion absent from its model) |

The prologue chain plays end to end without intervention: `21500 → 21507` (native 538),
then trigger 11 of `M01` launches scripted battle 26 (action `0x78`), whose victory (trigger
15) launches `21520 → 21521 → 21522 → 21596` (natives 538), see "New game" below.

**Result on chapter 2** (`--events-report 2.1-1`, 50 report rows: **47 complete, 3 partial**; the
prologue report above is unchanged by it and complete since 547 executes). The report plays
each scene at **its own story point**
(`report::story_of`): the chapter and step of the trigger rule that starts it, and the first
sub-step at which the NPC that rule talks to stands on the map; before this the 2.1.1 cast of
the base point made every NPC motion of 2.2 to 2.6 "absent" (the NPC was not in the cast). The
party's guest (Ivor) is carried from one scene to the next (`event/guest.rs`;
[`re/characters.md`](re/characters.md), § The guest). Natives counted "no rendered effect" now
also include 509 (flags `0x80`/`0x100` nothing reads), 550/556 (the hero's weapon and shield,
not drawn), 571 (bit `0x10` is the shadow flag, shadows not drawn) and a motion package the ROM
does not hold (the game's load fails too).

| Cutscene(s) | Maps | Result | What is missing |
|---|---|---|---|
| 2110, 2120, 2140, 2200, 2210, 2220, 2222, 2230, 2240, 2250, 2300, 2320, 2350, 2360, 2370, 2400, 2410, 2420, 2430, 2440, 2450 | `M01`, `M01M02`, `M01M05`, `M01M07`, `M01M09`, `M01M10`, `S01M01` | complete | — (NPC motions and the guest in cast mode 10 play; 2130, 2220 and 2600 use 509: no effect) |
| 2130, 2500, 2510, 2520, 2530, 2550, 2555, 2560, 2570, 2620, 2720, 2730 | `M01M10`, `D01M01`, `D01M05`, `D01`, `M01`, `M01M10` | complete | — (2130 opens doorway `(0, 2)` by 540; 2500/2520 hide and show a sprite by 223) |
| 2600 + 2610 | `M01M02` | complete | — |
| 2750, 2760, 2800 | `M01M10`, `M01` | complete | — (2750: native 103/102 fade the 3D screen to black under the captions; the candidate package `event/ev03060s011.chr` is not in the ROM) |
| 22590, 22591, 22592, 22594 | `M01`, `S01M01`, `M01M10` | complete | — |
| 2535 + 22510 | `D01M05` | partial | 574 on placements `(0, 28)` and `(0, 30)`, which **no map of area 7100 holds** (`dq9re map D01 … D01M05`: ids stop at 19), so the game's "record for a later load" has nothing to apply to; 571 (no reader) |
| 2700 + 22593 | `M01` | partial | 417 (`W+0x19c0`, `W+0x195d`: the message routine clears `W+0x40` while it is set; effect **[unknown]**) |

(Rows after 2750: 2810 to 2940, 22500 and 22595 are complete except 2900 + 2910, partial on 573,
outside the v0.2 target.)

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
- **Screen cover (102/103).** A translucent colour polygon over the 3D screen
  (`Environment::cover`, drawn after the edge marking by `Renderer::render`, `cover.wgsl`):
  103 fades it in to the 5-bit colour over `d` ticks (16.667 ms each, `ScreenCover_Update`),
  102 fades it out and switches it off; the message window and the brightness fades stay
  above it. Captures `build/evidence/v2-ch2-cutscenes/cap-2750-cover-1370.png` (half way) and
  `-1420.png` (black); test `native_103_covers_the_screen_and_102_takes_it_away`.
- **Party guest (566 mode 10).** The model of Ivor (`chara_sub/s017n.chr`, record 1 of
  `attnpc.gp2`) is a cast member of the scenes where he walks with the hero; his motions are
  `stand`, `talk`… of the script's package. `EventPlayer` keeps who the guest is and replays
  the trigger rules that add or dismiss him at the end of each event.
- **Doors by 540, show/hide of a sprite (223), placement scale (577).** 540 opens the doorway
  `(block entry, identifier)` like 584; 223 shows or hides a script sprite; 577 scales a map
  placement's drawn instance (not its collision).
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
prologue cutscenes (only `ev28920`). 571 sets the shadow flag (bit `0x10`, read by
`Character_DrawShadow`), which has no visible effect while NPC shadows are not drawn. **547**
(the scripted battle's transition) is executed since the battle scene
batch: the screens go to black and the battle track starts, and the battle request that follows
(`0x78`, or `Request::Battle` of a trigger) shows the scene (§ "Battle scene"); its arguments
are not carried by `Event::Unimplemented`, so the transition plays with the ordinary track
(`0x17`) and the record's track comes with the request.

## Battle scene

A battle is fought on a **stage**: an ordinary map of the map list (`B01M16`, "F01 - Field",
id 30116; `B02M15`, "D01 - Hexagoon", 30215…), assembled and lit like a field. The fight is
centred on the stage's origin, the party on `+z` facing π and the monsters on `−z` facing 0,
and starts everyone on the grid (`ov_000::02164d74`). `crates/dq9-app/src/battle.rs`
(`BattleScene`) builds it, `app.rs` shows it through the transition and gives the field back
afterwards. Facts from the wiki *Battle-Stages* and *Monsters*, rechecked on our ROM where the
tests could.

```powershell
# The Hexagoon's set battle (eventbattle.bin index 2: monster 300, stage 30215).
cargo run -p dq9-app --release -- --battle 2 --screenshot build/evidence/j4-battle-scene/hexagoon.png
# Three slimes on the field stage 30116.
cargo run -p dq9-app --release -- --battle-stage 30116 --monsters 1x3 `
    --screenshot build/evidence/j4-battle-scene/slimes-f01.png
# In the window: --battle 2, or ?battle=2 on the Web (`?battle-stage=30116&monsters=1x3`).
```

- **Stage.** `BattleScene::build` loads it with the field loader (map archives, textures, its
  own `.bats`), then keeps only the scene: the stage has no cast, no collision and no triggers
  worth stepping. A stage the record does not name, or id 0, is 30116, as
  `func_ov000_02166880` falls back. `--battle N` reads the record's stage, monsters and track
  (`dq9_core::scenario::battle`); `--battle-stage` takes an explicit stage and `--monsters`
  `number[xcount]` pairs.
- **Lighting.** The stage's `.bats` slot is taken **once**, at entry, from the day clock
  (`LightingManager+0x98` at the request, wiki *Battle-Stages*): the environment is computed
  once and never blended. The slot the record carries is not in `eventbattle.bin` (**[unknown]**).
- **Grid.** Slot *s* → column *s* mod 9, row *s* div 9; x = 2.598 × column − 10.392 (+1.299 on
  an odd row), z = 2.25 × row − 9 (`ov_000::0216f74c`). The slots by side size come from the
  tables at `0x02183108` (party) and `0x02183118` (monsters): one hero → 58 (0, 4.5), three
  monsters → 29, 22, 32 (−3.897, −2.25), (0, −4.5), (3.897, −2.25). Every fighter stands
  `0xcc` (0.05) above the stage. A set with more than eight monsters (the tables stop at
  eight; the ROM's records ask at most eight) gets free slots after them **[hypothesis]**.
- **Formation.** A battle starts on the grid; the row (`0x02167e6c`) is only used by a few
  action scripts, so `set_formation(Formation::Row)` asks for it: party at z 2.5, 1.5 apart and
  centred; monsters at z −2.5 by their width (radius ×4, gap 0.7, narrowed as `0x02167b5c`
  does). The turn of the party's ends (±0.2) and the five narrowed monster kinds are
  **[hypothesis]**: the angle table and the kind list were not read.
- **Models and motions.** The party is the hero model (`AnimatedScene::add_hero`); monsters
  load their `.cchr` bodies and separate `.cmot` motions from `<code>.mon` in
  `data/pack_lv5/enemy.gp2` (`LoadMonsterCharacter`, native 233). ROM `.bact` action sections
  select attack, magic, guard, call and escape motions; the hero takes its equipped weapon's
  `mp02NNbe` set. Damage reacts at the script's hit percentage; death/flight play once then
  hide the model. The original alpha fade and camera close-up size remain unmodelled.
  Timing, dispatch and limits: [`re/battle-data.md`](re/battle-data.md#action-scripts-and-battle-motions).
- **Camera.** `Shot::Opening` is the opening (`func_ov000_0216118c`): the wide side shot
  (22° half-angle), from the party's side; `Shot::Side(side)` is the 15° framing of
  `func_ov000_0216d600`. Distance is the larger of width × cos ÷ (sin × 2.2) and
  depth × cos ÷ (sin × 1.5), less 2.5 (at least 6.5, 3 wide), looking at (0, depth/2 clamped
  1.2–2, 0). `func_ov000_0216f2b8` then keeps the eye at most 17 from the target and at most 5
  high. Which sign "side 1" is, the opening's eased approach (0.95 a frame) and the fixed
  per-monster-kind shots of `data_ov026_021de87c` are **[unknown]/[hypothesis]**: the first
  is taken as the party's side.
- **API**: `act(actor, action, targets, damaged)` starts the ROM-backed gesture;
  `presenting()` gates windows/input until it finishes. `kill`/`flee` hide at motion end;
  `remove` hides immediately. `upload` transfers geometry and every fighter world matrix,
  including hidden entries, to the existing GPU renderer. `set_shot` and
  `fighter_screen_pos` provide framing and damage-number positions on the 256×192 top screen.
- **Transition** (native 547, `ov_001::02163ccc`). A scripted battle (`Request::Battle`, action
  `0x78`) or 547 starts it: the battle track (`eventbattle.bin` `+0x24`, else sequence `0x17`)
  plays at once, both screens go to black over 20 frames from the 15th, the scene is built
  while black and comes up over 15 (`0x021d9d18`); the fight is played on it (§ "Battle"),
  then the screens go to black again over 15, the field comes back and trigger event 15 or 16
  fires. When no fight can be built, the stand-in holds the scene 90 steps and settles it as
  won. 547 without a following battle gives the field back after 110 steps. The
  swirl's model (`ev999991500.chr`), its roll and field of view, and the encounter transition
  of a roaming monster are **[hypothesis]/not modelled** (J4 roaming batch).
- **Headless** (`--headless`, `--auto`, `--replay`, tests): the scene is built and checked,
  then the battle is settled in the **same fixed step** (`App::last_battle` records its stage
  and fighter count). No step is spent on a presentation: the headless timeline is the
  story's, so the recorded input traces replay identically (`scripts/parity.ps1`). What
  settles it is the J4 stand-in (won) unless `--fight` plays it with the engine (§ "Battle").
  The window and the Web show the transition and play the fight.
- **Back to the field.** Native 844's rule is modelled by keeping the field scene aside
  (`App::save_field`): the same map, cast, doors, collision, party pose and lighting come back
  unchanged, so the placements are as the scene left them. The map is not reloaded: no entry
  trigger fires.
- **Events.** 547 no longer counts as "not executed": `EventPlayer` turns it into
  `Outcome::BattleTransition`, which the application executes. The script keeps running during
  the transition in the report playback (no field to freeze there); in the window the field
  waits for it, like the game's task order.

`--screenshot` prints each fighter's placed position and heading, and the camera it used
(`camera: eye … target … half-angle …`). Captures (`build/evidence/j4-battle-scene/`):
`hexagoon.png` (`dd025965b166`: the Hexagoon on `B02M15`, the hero facing it),
`slimes-f01.png` (`3e1ba9308ed1`: three slimes on `B01M16`, the middle one hidden behind the
hero), `battle26.png` (`c7ed26e23bcb`: battle 26's record, monsters 290/292/290) and
`report-2.1.txt` (`fe41a9bcc437`, `--events-report 2.1-1`: no 547 left).
Real-ROM tests in `app/tests.rs` assert the grid of a
three-slime scene and of the Hexagoon's set battle, the eye clamp, `fighter_screen_pos`, and
that scripted battle 26 enters the scene from `--new-game` and gives the same map and pose
back.

## Battle

A battle is **played**: the player picks the commands, the engine (`dq9_core::battle`)
resolves every round, the scene shows its fighters, and the messages of the game
(`dq9_formats::battle::text`, `re/battle-text.md`) say what happens. A victory pays the party
(`Party::award`: experience, gold, drops, level-up lines), a defeat is a wipe-out (trigger 16,
the purse halved, the party wakes in a church), a flight returns to the field.

**Field encounters.** Native and Web display the map's roaming monsters with their own
`<code>_f.mon` models and stand/walk/run/appear motions. `app/roaming.rs` steps the existing
core rules at 30 Hz, on the actual collision world; equipment contributes to party strength
and deftness, preset companies come from `eventbattle.bin`, and the field and battles retain
one continuous generator stream. Natural contact calls `start_encounter`; victory fades and
releases the fought model, flight retains it and prevents immediate re-contact. The exact
original post-flight state/recent-list writer remains **[hypothesis]**
(`re/encounter.md`, Application wiring). Field-menu/facility screens are still absent.
Headless parity runs keep roaming disabled.

Native window evidence (`build/evidence/j4-roaming-play/`): natural F01 contact → victory
in 3 rounds (3 experience, 5 gold) → same field with no ghost model,
`field-restored-after-win.png` (`61a585913e52`), `smoke-win.log` (`0ec1f1917e68`);
`hexagon-field.png` (`fceee50be300`) shows field monsters on `D01M02`. ROM regressions cover
`D01M02`–`D01M04` and encounter-free `S01M01`/`D01M01`/`D01M05`.
Standalone `--battle`/`?battle=` previews now run once; `Esc` cancels battle commands.

```powershell
# The Hexagoon's set battle, played by hand (command menu, messages, defeat → trigger 16):
cargo run -p dq9-app --release -- --battle 2
# Three slimes on the field stage, to a victory with experience and gold messages:
cargo run -p dq9-app --release -- --battle-stage 30116 --monsters 1x3
# A new game whose battle 26 is played by the engine (an automatic policy attacks the first
# standing monster), the world's generators seeded with 7:
cargo run -p dq9-app --release -- --new-game --fight --seed 7 --headless --auto --pilot
# A capture of the windows: play with the confirm key held until the phase shows (`menu`,
# `damage`, `levelup`, `victory` or `lost`):
cargo run -p dq9-app --release -- --battle-stage 30116 --monsters 1x3 --fight --seed 7 `
    --battle-capture damage --screenshot build/evidence/j4-battle-play/damage.png
```

**Code.** `crates/dq9-app/src/battle/` (the scene is `battle.rs`): `data.rs` (the ROM tables
of a fight, decompressed once), `fight.rs` (`Fight`, the state machine: opening, command
entry, rounds, end; no scene, no GPU), `narrate.rs` (events → the game's messages and scene
cues), `ui.rs` (the windows as quads of the two DS screens), `tests.rs`. `app/fight.rs` is
the application's side: the party, the world's generators, the window's pad, the outcome's
effect on the story and the wipe-out.

**What is played.**

- *Entry.* `App::run_battle` (action `0x78`) and `App::start_encounter(groups, stage,
  opening)` (called by the field's roaming-monster contact) resolve the
  request, run the transition and build the scene (§ "Battle scene"), then the `Fight`:
  the party is `Party` (new game: the hero, level tables applied at the first fight; a save:
  its party), each member's `Character::fighter` from `ItemStats` and `itembtlprm.nat`; a
  scripted battle (`eventbattle.bin`) is a **set battle** (the table's HP, no flight), an
  explicit stage or an encounter draws the monsters' HP on the world's generator and allows
  flight. `Surprise` is the request's.
- *Generators.* The world's `Random` (monsters' HP, flight) and the C library's `CRand`
  (drops) are kept by the application across battles; the battle owns its own `Random`,
  seeded from the world's. `--seed N` (the Web's `?seed=N`) seeds them, else the game's clock
  and the fixed-step count at the first battle.
- *Opening.* `strbtl` 6 for each kind of monster ("Some slimes draw near!", "A cruelcumber
  draws near!"), 8 for a boss, then 14 (monsters strike first) or 11 (the party has the
  jump). Each window closes itself after 40 steps; a key shortens it.
- *Commands.* Per member, in the bottom screen: **Fight** (`str_btl` 30000) → Attack, Spells,
  Defend, Items (30004–30008); **Flee** (30001); Examine and Misc. are shown greyed. Attack,
  a single-target spell and an item open the target list (the monsters standing, or the
  party for an action aimed at its own side; the scene marks the monster under the cursor
  with the wait arrow). Spells are those the vocation has learned at the member's level in
  `spelltable.bin` (the Minstrel learns Heal at 3: a level 1 hero has none, so Spells is
  greyed). **Defend** is `Command::Defend` (no target); **Items** lists the acting member's
  held items with a supported battle action, one row per held slot (duplicates included).
  The party bag is unavailable in combat. Items is greyed when no held action is enabled;
  Flee is greyed in a set battle (`BattleError::FleeForbidden`).
- *Round.* `Battle::round`; its events become windows (`narrate.rs`): each action says its
  opening line (`actmsg`, by the record's `+0x20` bits 0–9 / 10–19 for a party / monster
  target), the critical line (`+0x28` bits 20–29), what it did (`+0x20` bits 20–29 at a
  party member, `+0x24` bits 0–9 at a monster, the failure lines `+0x24` bits 10–19 / 20–29
  on a dodge or a block) and the kill line (`+0x28` bits 0–9 / 10–19). The status window
  follows the HP as the windows come; damage and healing numbers float over the fighter at
  `fighter_screen_pos`. ROM action scripts select the attack and damage motions; death and
  flight hide the model after their one-shot motion. Windows wait for the loaded presentation.
- *End.* Victory: `str_bres` 1 (50 for several monsters), 25 (26 for several members), a
  level line (10) and the skill points (13) for each level gained, gold (16) and a treasure
  chest (17, 18) for each drop; every window waits for a key (wiki *Battle-Stages*). The
  drops roll on `CRand` (`Battle::finish`), the award goes to `Party::award`, each drop to
  the trigger state (`State::obtain`, which raises the item-obtained flag, so a drop of the
  medicinal herb raises flag 4135). Defeat: `str_bres` 20, then **the wipe-out**
  (below). Flight: `actmsg` 45 (`str_bres` 24 when the enemy block the way, the fight goes
  on); the field comes back.
- *Defend, items, backup, monsters' flight.* `Event::Defended` says action 3's opening line
  (`actmsg` 10, "Hero defends."). `Event::ItemUsed` precedes the item's own action: its
  opening line (`actmsg` 70, "Hero uses a medicinal herb.") names the item the member chose
  and its result line follows ("Hero's wounds are healed."). Only the selected held slot is
  consumed, and only on execution of an item whose record sets `+0x08` bit 19 (used up).
  Cancelling or falling before the turn spends nothing; reusable items such as the sage's
  stone remain. The bag is unchanged. Slot compaction and the flag's evidence are in
  `re/battle.md`, "Defend, guard level and items". A call for backup
  (action kind 12) says its opening, then the record's line: "A bag o' laughs shows up." when
  the engine added the monster (`Event::Summoned`, the scene gets its model through
  `BattleScene::add_monster` and the monsters take their places again), "But nobody shows up."
  otherwise. A monster that flees (Flee `0xE1`, `0x395`: `Event::Acted` on itself) says its
  record's opening ("The slime flees.") and is removed from the scene; it pays nothing
  (`Battle::has_fled`, no kill).
- *Fullheal.* The core kind-2 handler applies the original max-HP override after the normal
  amount calculations and RNG draws (`ov_024::021db3a4`), not physical-strength-based healing.
  A ROM-backed `Battle::round` smoke restored HP 37 → 321 and spent MP 40 → 16
  (`build/evidence/j4-full-heal/smoke.log`, `2d4fc32e2122`); no interactive spell-menu claim.
  Omniheal's MP128 policy remains unsupported. Details and regression:
  [`re/battle.md`](re/battle.md#final-damage-ov_024021e6a90-eu-decompilation-read).
- *Back.* The screens go to black, the field is restored (§ "Battle scene"), trigger event 15
  (won) or 16 (lost) of the scripted battle fires, a flight fires none.
- **Wipe-out** (`func_02010604`, wiki *Travel* "A wipe-out"): the fallen members get up with
  full HP and MP, the purse is **halved rounding down** (the bank is spared), and the party
  comes round on the revival map at its start point: **1106** (Angel Falls' church) after a
  new game. The trigger 16 of the battle runs **before** (the Hexagoon's, `D01` room 7105,
  sets track flag 4). Not modelled: what the priest says there (`chur_messet.bin` picks the
  voice, wiki *Travel*), the revival map a save carries (a church saved in, Stornway's), a
  set battle's own revival map (trigger action 180, the Magmaroo and Gortress only), the
  jingle `ME_009`. A wipe-out during a running scene (an event queued by trigger 16) leaves
  the party on the map.

**Controls** (as the conversation window): `Enter`, `Space` or `Z` confirm and close the
window; `Backspace` or `X` cancel (back one menu, or to the previous member); the arrow keys
or `W`/`A`/`S`/`D` move the cursor (left and up go back, right and down forward); `M` mute,
`R` layout; `Esc` cancels command menus and still closes the application on the field. No
touch. With `--layout top` the bottom screen's windows are laid over the top screen.

**Headless and parity.** The 615 checkpoints and the recorded traces pin battle 26's "won at
once" timeline, so a headless run, `--replay` and the application's tests keep the stand-in
unless `--fight` is given; the window and the Web always fight. `--fight` plays the battle in
the same fixed step (no step is spent on a presentation) with the automatic policy (Attack
the first standing monster, as holding confirm through the menus does: a test asserts the
two play the same fight) and the world's generators on `--seed`. Follow-up: re-record the
references with real battles (`scripts/parity.ps1` still compares the stand-in; the loot of
`--fight` depends on the seed: seed 7 drops nothing now that the fleeing monsters pay nothing).

**Open and [hypothesis].**

- Layout of the windows is not read from the overlay: the message window is the field's size
  (252 × 70) put at the top of the top screen (the field's own, `y = 0x74`, would sit on the
  fighters); the status window (`6, 6`, 244 wide, one 22-pixel row a member: name, `HP`,
  `MP`), the command menu (`6`, below it, 116 wide, 20-pixel rows) and the target list (`128`,
  122 wide) are laid out here. The colours are the conversation window's.
- Window timing: 40 steps per window (the opening's `<TIME=45>` and the flight's 30 are the
  game's), a key closes it after 10; results wait for a key (wiki *Battle-Stages*). The scene's
  actual loaded action/reaction/death/flight motion is also a barrier: the window and pad
  wait, so a damage number, next actor or result cannot cut the presentation. Which lines
  go in one window (here: one a line) and multi-target line ordering remain [hypothesis].
- Party names carry no article (`grammar` 0): "Hero attacks."; `IF_SOLO` makes the victory
  lines read "Hero obtains 8 gold coins!" and the wipe-out "Hero is wiped out!".
- Treasure: one chest pair (17, 18) per drop, the monster named is the first fallen one whose
  record holds the item.
- Several kinds of monsters: one `strbtl` 6 line each, not message 7 (two sentences).
- **Rules the engine still refuses.** `Battle::round` returns `BattleError::Unsupported` for
  the whole round and leaves the battle unchanged (an AI mode, a targeting handler…): the
  fight then ends as a flight with a logged warning, since the same round would fail again.
  Over twenty seeds, every chapter 2 monster of `re/battle.md` plays to its end (Flee, Buff,
  Accelerate, Dazzle, Weird Dance, a call for backup), so none stops today. The menus ask the
  engine first: a spell or an item whose action `validate` refuses is greyed.
- Not played: abilities, Examine, Misc., the guard's effect on who monsters pick, party
  members beyond the hero on the scene, the full action-script camera/effects interpreter
  (the opening's wide shot stays), fanfares and battle sound effects. Using worn equipment
  from Items is not offered. Lunge interpolation/return timing and the window grouping
  remain [hypothesis]; playing the ROM motions does not certify the full presentation.
- Held-item evidence (`j4-battle-held-items`): offscreen rasterization of the real UI quads,
  ROM atlas and font, not a native-window or browser interaction:
  `build/evidence/j4-battle-held-items/item-menu.png` (`9e2260ec767e`, HP 5/20) and
  `item-used.png` (`f4baed36c8c8`, healing). At capture, HP increased, the held herb was gone,
  and the bag retained its two herbs. Regression tests cover a selected second duplicate,
  cancellation, a skipped fallen actor and a reusable sage's stone.
- Motion evidence: native ROM captures `j4-battle-motions/battle26-swing.png`
  (`780cc8be2a0a`) and `hexagoon-swing.png` (`1d5f145219cd`), actual local browser battle 26
  on seed 7 `j4-battle-gpu-sync/web-after-lunge.png` (`c461e1d32686`) and
  `web-after-victory.png` (`c39b70b7ba1a`): hero lunges, returns, and defeated/fled monsters
  disappear; victory remains 4 rounds, 2 XP, 4 gold. Extra confirmation and stacked → wide
  retain the finished preview without restarting. Console: `web-after-console.json`
  (`e616bcfb24af`). The live GPU world upload regression failed before the fix (300 pixels)
  and now passes lunge/hide readbacks; full ROM gate 5/5, 109.4 s, app 103/103.

**Evidence** (local, `build/evidence/j4-battle-play/`, sha256 prefixes): `fight-run.log`
(`743b4a58e0bd`, `--new-game --fight --seed 7 --headless --auto --pilot`: battle 26 won in 4
rounds, 2 experience, 4 gold, no drop, the slime and the cruelcumber that had not fallen
fleeing; the party line and every window of the fight are in the report), `menu.png`
(`50124557cf7c`), `damage.png` (`dd23ba975c12`), `victory.png` (`d00681832a18`),
`hexagoon-lost.png` (`841dac2ee401`, `--battle 2 --seed 3`), `parity.log` (`6bffa8a68372`: 2
of 2, unchanged), `events-report-2.1-1.txt` (`0475ab6cf0dc`: 47 of 50 complete, unchanged),
`verify.log` (`aa44613207e9`). Current tests: `battle/tests.rs` (menus against the automatic
policy, cancel, flight by seed, a lost Hexagoon, level-up, Defend, held-item use and bag-item
refusal, fleeing monsters, a call for backup) and `app/tests.rs` (battle
26 fought and paid, the lost Hexagoon's trigger 16 and wake-up in map 1106, an encounter, the
stand-in untouched, a monster joining the scene, the window's path driven by the pad).
The original `j4-battle-play` evidence drove phases without a window; newer native interactive
checks are recorded under Field encounters, and Web checks under Verification.

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

**Scripted battles.** A headless run does not play the battle unless `--fight` is given:
`scenario::stand_in` declares it won, which the log states explicitly ("stand-in, no battle
system yet"), so the recorded input traces and the 615 checkpoints of `scripts/parity.ps1`
keep their timeline. Action `0x78` (and native 547 in a cutscene) shows the **battle scene**
first — the stage, the monsters and the party on the grid, under the battle camera (§
"Battle scene") — then the battle is settled and trigger 15 (or 16) starts what follows. In
a window, on the Web and with `--fight` the settling is the played fight of § "Battle": the
party is paid (experience, gold, drops) and its HP persists.

**Result** (`--new-game --headless --auto`, EU ROM, English text; game time at 60 Hz):

| Time | Map | Cutscene / fact |
|---:|---|---|
| 0 s | `M01M12` | `21500` "stage_opening" (80 s), then `21507` (43 s) via native 538 |
| 123 s | — | trigger 11 of `21507` → **scripted battle 26** (monsters 290, 292, 290; arena 30116), won by stand-in (`--fight`: played by the engine, § "Battle") → trigger 15 → `21520` |
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

- The driver keeps the battle stand-in by default (`--fight` plays them with the engine, § "Battle");
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

## Running in a browser

### Hosted demo (GitHub Pages)

Open **https://fablrc.github.io/dq9-rust/?newgame=1**, choose your European ROM `YDQP01`:
the "New game" button becomes active as soon as the ROM is read, and the keyboard is given back to the game
without an extra click. Arrows: walk; Enter/Space: talk and advance messages;
Q/E: camera; M: sound. A DS save is optional.
Files are read locally: no ROM or save is sent to GitHub.
A computer with a keyboard and a recent browser with WebGPU or WebGL 2 are recommended.
Battles are interactive; standalone battle 26 has been played to victory in the browser
(Verification below). Roaming monsters are visible and start fights; field-menu/facility
screens are still missing;
effects and battle motions are incomplete. The continuous chapter 2 route is not certified.

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

Open `http://localhost:8000/` (parameters `map`, `time` (name or 0 to 3), `clock`, `layout` (`stacked`, `side`, `top` or `wide`), `chapter`, `step`,
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

Each Orca worktree has its own Cargo target: sharing one is **not** safe. Observed in the r1
wave (integration of `r1-formats-bytes`): with one `CARGO_TARGET_DIR` for every worktree,
Cargo considered the `nds-rom` rlib built from another worktree's sources fresh, and the
doctests of `dq9-formats` failed against it (`module bytes is private`) although the branch
was correct; the same mechanism can produce a false pass. The target stays outside the
repository (`build/` is only for project outputs: screenshots, extracted ROM), and its
directory name carries the batch, so two worktrees never share a lock either.

```powershell
$env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\dq9-target-<batch>"   # cmd: %LOCALAPPDATA%\dq9-target-<batch>
```

A test binary can outlive the sources it was built from (branch change): the tests read
`CARGO_MANIFEST_DIR` at runtime (`crates/testrom`), and `verify.ps1 -RequireRom` passes the
found ROM to the tests through `DQ9_ROM`.

A private target costs little: a full `cargo test --workspace --no-run` into an empty target
takes ~96 s on the development machine (measured after the r1 wave). `sccache` was measured
too and is not used: a second empty target got 0 cache hits (the keys contain the target
path), and even with perfect hits it could save at most those ~1.5 min per batch, for a
cache of several GB.

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
- **Billboards of models** (G3D render commands `0x07` BB and `0x08` BBY: trees, bushes and
  other flat scenery, `j4-billboards`, [`re/g3d.md`](re/g3d.md) § Billboards): turned toward the
  render camera every frame on the CPU, one model copy per billboard placement, with a static
  translucent sort key. 342 of 740 `.amdj` archives hold them (3,466 commands, 2,761 BBY);
  `F01` none (how the field vegetation is drawn is not established). Before this, such quads kept
  their authored orientation and vanished edge-on or behind. Checked by `--screenshot` only: the
  interactive window runs the same per-frame code but was not driven by hand.
- **Not rendered**: antialiasing (the wide presentation's FXAA is a post-process, not the DS
  engine), shadow polygons (mode 3) and wireframe (alpha 0), fog
  alpha-only mode and `FOG_COLOR` alpha, DS precision
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

The window shows the two DS screens (`crates/dq9-app/src/screens.rs`), or a single 16:9 field
in the `wide` presentation:

- **Top**: the 3D field, with the game's camera and projection (`0x0202df78`, vertical
  half-angle of 15°). The screen is 256×192; only the rectangle size varies with the window.
- **Bottom**: plain background (`dq9_render::Panel`) in the field, no content: the touch
  screen content is not identified **[unknown]**; no ROM data is displayed there. During a
  fight it holds the party's status and the command menus (§ "Battle", layout
  **[hypothesis]**).
- **Message window**: drawn on the top screen (`talk.rs`, `dialogue.md`); the battle's own is
  at the top of the screen (`battle/ui.rs`).

Layouts: `stacked` (default, top screen above the bottom one), `side` (side by side),
`top` (top screen alone) and `wide` (one 16:9 field, letterboxed in the window). The first three
are centered and scaled together to the maximum of the available space, without distorting
their 4:3 ratio; margins stay black, independently of sky, fog and fades. On the Web, the
canvas follows the available size below the controls and the screen's pixel density. Native:
`--layout`, and `R` switches to the next layout. Web: the "Screens" select or `?layout=`. The
old `--ds-aspect` and `ds=` are removed.

### Wide 16:9 (cinematic presentation)

`--layout wide` (Web: `?layout=wide`) is the graphics tech demo, not a DS reconstruction:

- The field fills a 16:9 rectangle centred in the window (black bars elsewhere); the camera
  keeps the game's 15° vertical half-angle, so the widescreen field widens the horizontal
  view instead of cropping it.
- `Renderer::new_with(..., true)` uploads mipmapped textures and samples them linearly
  (trilinear), instead of the DS's nearest filtering with one mip level.
- The field is rendered to an offscreen target and composed by `post.wgsl`: FXAA-lite,
  screen-space contact shadows from the depth buffer, threshold bloom, filmic tonemapping mixed
  with the DS colours, cool-shadow/warm-highlight grading, vignette and dithering. Edge marking
  stays as a single-pixel accent. The shaders are parsed and validated by
  `crates/dq9-render/tests/shaders.rs` without a GPU; this does not certify backend support.
- The interface is a 960×540 virtual canvas (`hud.rs`): location, map code and time of day,
  the leader's HP/MP bars and purse, and the game's message window scaled into a bottom-left
  glass panel. Battle windows keep their 4:3 shape in the upper and lower left corners.
- The renderer is rebuilt when `R` or the select enters or leaves `wide`, because texture
  filtering and the post pass differ; the default layouts are unchanged.

Screenshot of the demo:

```powershell
cargo run -p dq9-app --release -- --map M01 --layout wide --size 1920x1080 `
    --eye=-5,20,40 --target=-20,0,5 --screenshot build/scratch/app/wide.png
```

Integration `49843de` passed `verify.ps1 -RequireRom` (5/5, 150.3 s; app 86/86).
Native Vulkan offscreen captures and an actual local-browser M01 session exercised wide →
stacked → wide, including the HUD and renderer recreation without reported shader errors.
Evidence: `build/evidence/add-graphics-tech/native-wide.png` (`684fb0b30e4c`),
`web-wide-restored.png` (`5b598af7e34f`), `web-console.json` (`ac1c4bafeb61`).
The browser smoke used the gate's debug wasm; forced WebGL 2 fallback and a continuous
new-game route were not exercised.
Published build `1e0326d`: [CI](https://github.com/FabLrc/dq9-rust/actions/runs/42831807723)
and [Pages](https://github.com/FabLrc/dq9-rust/actions/runs/42831807515) passed.

## Verification

- Published roaming smoke (November 10, 2026), Pages
  [`42812911849`](https://github.com/FabLrc/dq9-rust/actions/runs/42812911849), build `6ecf718`.
  Local ROM, standard keyboard input, no game-state injection. F01 models were visibly
  animated. Direct entry on D01M02 produced natural contact with monster 4, company 4×2,
  stage 30214; Esc cancelled a command submenu; flight ended after one round without rewards.
  A different monster (20) then contacted the party; four rounds of Attack ended in defeat,
  followed by a visible revival on church map 1106. Evidence:
  `build/evidence/j4-combat-web-final/hexagon-contact.png` (`2ca872e55958`),
  `escape-cancel.png` (`4be4d98a7866`), `revival-church.png` (`430671952e62`),
  `console.json` (`2798e51a2364`).
  Standalone battle 26 (`?battle=26&seed=7`) won once in four rounds, 2 XP and 4 gold.
  Its residual no-field rendering after closure was found in this run and corrected by
  `749ae40`: completed previews retain their stage/camera instead of stale GPU geometry;
  a dialogue also suspends roaming until it closes. These direct-map checks do not certify
  the continuous new-game route or original-engine flight resumption.
  Final preview check on build `ccb1811`, Pages
  [`42815645558`](https://github.com/FabLrc/dq9-rust/actions/runs/42815645558):
  same seeded victory, reward closure and extra confirms leave the complete framed stage
  without restarting; `build/evidence/j4-battle-lifecycle/preview-finished-after.png`
  (`50eadc1365fe`). CI [`42815645466`](https://github.com/FabLrc/dq9-rust/actions/runs/42815645466)
  is green on that build.

- Browser battle on Pages (November 10, 2026): deployment
  [`42800444183`](https://github.com/FabLrc/dq9-rust/actions/runs/42800444183),
  commit `b91f43b`, URL `https://fablrc.github.io/dq9-rust/?battle=26&mute=1`.
  Local European ROM; Orca embedded browser, real focus and Enter presses, no game-state
  injection. Commands, targeting, damage, victory and reward windows exercised: 4 rounds,
  5 experience, 9 gold, one drop, hero HP 10/20 at victory. Evidence:
  `build/evidence/j4-combat-web-baseline/report.md` (`44aa2de407fe`),
  `victory.png` (`981849bfdefc`), `gold.png` (`b131f23efcf2`).
  **Defect on that build**: closing rewards started standalone battle 26 again;
  `restarted.png` (`637807d124a5`). Fixed in `2801473` by removing the explicit preview
  restart branch; a regression checks no second battle or reward after closure.
  This proves neither natural field contact/return nor continuous new-game progress.

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
