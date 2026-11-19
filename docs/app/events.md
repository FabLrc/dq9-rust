# Events (`.stb` cutscenes)

The game's cutscenes (prologue cinematics, scripted dialogues) are `.stb` scripts
([`re/stb.md`](../re/stb.md)) that `dq9-app` plays on screen:

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
  game update, [`re/stb-natives.md`](../re/stb-natives.md) § "Time and units"), the scene on
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
  view. A cutscene is framed in 4:3 on the field's screen (`screens.rs`: the top one by
  default, the bottom one with `--screens original`).
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
[`re/stb-natives.md`](../re/stb-natives.md#effects-executed-on-screen-j3)). **Collision
placements** (574/575) are no longer excluded: the report counts them since the "remaining
cutscenes" batch. Before the J3-effects work: **22 partial cutscenes out of 30** (18 report
rows); after: **2 partial cutscenes** (1 row). The battle scene batch executes native 547
(§ [Battle scene](battle-scene.md)): **no partial cutscene remains**, the 26 report rows are complete.

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
[`re/characters.md`](../re/characters.md), § The guest). Natives counted "no rendered effect" now
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
[`re/stb-natives.md`](../re/stb-natives.md#effects-executed-on-screen-j3)):

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
- **A model the script loads (native 200).** It starts in record 0 of its motion table
  (`stand`), as the game does, not in the bind pose; the map NPCs drawn with the same model
  are hidden while it is shown (`21592`: one Ivor, [`re/characters.md`](../re/characters.md)
  § A model a script loads).
- **Open (effect models).** Scripted effect models (sparkles, glows of 21510, 1330, 1340) lose
  their billboard commands in `AnimatedScene::add_actor`, so they never face the camera; a fix
  attempt did not make the quad face the eye and is not committed; the scene that shows the
  benevolessence is unconfirmed ([`re/characters.md`](../re/characters.md) § Open: effect models
  and billboards). Ivor's duplicate case 2 (sprite NPC 8 `n001a` against the scene's `s064`) is
  also open.
- **Doors by 540, show/hide of a sprite (223), placement scale (577).** 540 opens the doorway
  `(block entry, identifier)` like 584; 223 shows or hides a script sprite; 577 scales a map
  placement's drawn instance (not its collision).
- **Screen swap (109/110).** The `POWCNT1` bit 15 state is shared with the layout; six
  swaps in the opening move the field and the window from one screen to the other. Screenshots
  `stacked`/`side`/`top` before/after the first 109 (frames 100 and 5000 of 21500). In
  `--screens original` the field already runs with the bit set, so the script's swap changes
  no screen there **[hypothesis]**: [`re/bottom-screen.md`](../re/bottom-screen.md) § 1 gives the
  bit for the whole field and says nothing of a 110 inside a field cutscene.

`--eye`/`--target` with `--event` and `--screenshot` look at the scene the script left
(they replace the script's camera): useful to inspect an actor.

**Limits and hypotheses.** See also [`re/stb.md`](../re/stb.md), § "On-screen execution": default binding
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
(`0x78`, or `Request::Battle` of a trigger) shows the scene (§ [Battle scene](battle-scene.md)); its arguments
are not carried by `Event::Unimplemented`, so the transition plays with the ordinary track
(`0x17`) and the record's track comes with the request.
