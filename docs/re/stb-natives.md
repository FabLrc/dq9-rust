# Inventory of the STB natives in use

## Scope and how to read

Static inventory of the **228 identifiers used** in the reached code of the 689 scripts
(80,237 calls), out of the 304 entries of the `ov_001::02164d6c` table.
It consolidates ten handler surveys, their engine functions and the STB contexts.
It redistributes no game scripts, texts or resources and is not an
emulator validation. The [STB ABI and format](stb.md#natives) remain the reference.

Handler addresses `0215…`/`0216…` are in **ov_001**, helpers
`020…` in ARM9; any other overlay is stated explicitly. These are virtual
addresses of the analysed ROM, not file offsets. Offsets `ev…:0x…`
are relative to the script code. English names are descriptive, not recovered original
symbols. **—: keep the numeric ID**. An established field write
is not enough to identify its gameplay function. `[INFERENCE]` remains a hypothesis.

Arguments exclude the native number; `&` = output through address; `[]` = optional only
when the handler checks it. A scripted arity that is too short establishes no default.
The handler's C return value is ignored; no returned state implies a suspension.

In the tables, `a`/`actor` denotes the STB controller, `b`/`slot` a binding
unless stated otherwise, `id`/`character_id` a character. `F=ov_017::0218b5b0()`,
`E=*(F+0x3734)`, `G=0200f398()`, `R=02012ff4()`. A field without an explicit
prefix belongs to the object described in its row.

The evidence is the decompilations of the table's handlers and their engine relays,
checked against the STB disassemblies; [the versioned symbols](../../re/symbols.txt)
pin the already named addresses. Generated local results are not required
to read this inventory; no new dynamic capture is claimed.

For 566, the signature depends on the mode: 0/1/9/10/11/12/13 `(mode,n)`;
2/7 `(mode,path,n[,option])`, 6 likewise with a `.mon` resource;
3 `(3,path,n[,option])` under `data/ani/`; 4/5 `(mode,reference,n)`;
8 `(8,n[,option])` with a fixed resource. The full consumption of these
declarations remains unknown; no general gameplay role assigned.

### Distinctions to preserve

- **Character/model**: table `Chara_Get` (`0200fd80`), indices 0–0xe8; conversion
  `0215ad2c(id)`: positives unchanged, negatives `0x9f-id` (`-1→0xa0`).
  **Binding**: 32 entries `{type,id,state,pointer}`, stride 0x10, configured by 202.
  **STB actor**: 24 controllers 0–23, stride 0x588, accessible through `0215ab20`;
  205 selects their binding. `ev21500:fn@0xeb0` chains 202, 203, 205.
- Actor commands are spread over **eight queues** (`0215a134`); even a
  direct assignment waits for its queue to be processed. Timed 207/209/211
  compare against `2×duration`, with engine step `numTicks_` (840). Units: §Time and units.
  `ev03030:0x1e1c/0x1eb8` uses durations 30/10 then matching waits.
  218 and 227 count differently; 838 exposes a separate counter.
- **210 ≠ 224**: 224 waits for the end of the current animation, 210 does not.
  210 flags: `0x01` stops without looping, `0x04` reverses progression,
  `0x08` forces restart, `0x10` prepares the transition; engine evidence
  `02036e44`, `02034ea0`, `020352f8`. Bit 2 and the effect of the fourth float unknown.
- Camera: eye `E`, target `T`, azimuth `θ` in radians, **relative height `h` (not pitch)**,
  total distance `r` (not horizontal radius). `0202e5e8` rebuilds
  `E=T+Ry(θ)·(0,h,sqrt(r²-h²))`. Commands are queued; predicate 301
  only examines four queues whereas the update processes five.
- Fades: **C0/C1 are not demonstrated to mean top/bottom nor main/sub**; controller
  locks +0x24/+0x25. Duration and rate not established; the handlers do not block.
- Messages: `W=020421b0()`. 405 exposes byte `W+0x19bd` (1: wait in progress,
  0: reset; full enum unknown), **558 reads the answer `W+0x954`**.
  For the documented **YESNO path only: 0=yes, 1=no**; evidence:
  `ev01160:0x20f8` reads 558 after message 210, `0x211c` tests zero,
  and [trigger condition 0x10](trigger.md), [YESNO/NOYES codes](text.md).
  No generalisation to NOYES, variants, or cancellation.
- **726–728 are sound**, not flags/quests: group + SE archive, heap
  restore, then local index. `ev03030:0x16b4` selects 231; `0x2b84`
  selects 237 and `0x2bc0` plays index 1. Not to be confused with a BGM ID.

## Time and units (J0)

Facts established by the code and the USA source (`GameTime.cpp`, `Brightness.cpp`):

- **Tick**: `GameState::numTicks_` (`GameState+0x3c4`, read by 840) = number of ticks
  elapsed since the previous update, **capped at 3**. **Measured: 2** in the five
  supplied melonDS states (`before`, `moved`, `field`, `demo`, `demo20`: `+0x3b4` = effective
  delta **33 ms**, `+0x3bc` = game speed `0x1000` = 1.0, `+0x3c0` = 7951 =
  `4096×33/17`). The game loop therefore updates **≈ every 33 ms** (30 times per
  second), i.e. **2 ticks of ≈ 16.67 ms** per update. That `currentNumTicks_`
  (`+0x3c8`) counts VBlanks is a **[hypothesis]**, consistent with these values.
- **Fades 100–122**: `SetXBrightness(res, target, d)` stores `d × 16.667` ms
  (`(int)(duration * 16.667f)`), decremented by the **effective delta** in ms
  (`UpdateBrightnessTransitions`); `d=0` applies immediately. The unit of `d` is therefore
  **the 60 Hz tick (16.67 ms)**, independent of the number of ticks per update.
  Level: −16 black … 0 normal … +16 white (`MASTER_BRIGHT` register).
- **Timed movements 207/209/211**: counter incremented by `numTicks_` on each
  execution, compared to `2×duration`: a duration of `n` equals `n` updates of ≈ 33 ms at
  nominal rate (**≈ n/30 s**), hence `n` frames **at 30 Hz**, not 60 Hz. Same unit
  as the wait in the `evspt_lv5` scripts (budget `2n` minus 840 per pass); the wait
  in the `event` scripts (`fn@0x58`) counts `n` VM passes.
- **580**: duration multiplied by **33** (ms per update); **715/721** and the camera
  (304–326, 317): units not rechecked here, see their rows.
- Consequence: fade `d` ≈ `d/2` updates, movement `n` ≈ `n` updates. An
  interpreter must keep both conventions.

## Angel Falls prologue: natives used (J0)

**Studied set.** Cutscenes whose stage (values 0 and 1 of the `0x66` records, tag 102,
of `eventlist6.bin` and `eventlist_lv5.bin`, see `stb.md`) is **1.1 to 1.3**: 30 scripts
(Angel Falls and its observatory, `ev01130…ev01610` and `ev21500…ev21597`). The two quest
cutscenes 53010/53011 add no native. Command:
`cargo dq9re stb-natives --stage 1.1-3 [--unnamed]` (test `prologue_scripts_use_120_natives`).
Result: **120 distinct natives** out of the 228 used in total. The 120 are either
named in `dq9re stb`, or listed below with what was attempted.

**Identified (name in `dq9re stb`) in this pass**, evidence in their row: fades
105–107, 114, 116, 119–122 (USA source `Brightness.cpp` + dispatcher `0215b074`), path
214–217 (`02157908…02157ae0`), 226–230 (`Object3D::SetRadius`, `ShallowCloneTo`,
`LoadFromCHRArchive`, `RemoveAnimationPackageByID`), 327 roll, 410, 502/503
(`TextureArena`), 512, 531, 532, 538 (`EventTask_Update`), 548/549, 566–568 (scene
construction and teardown), 570 (`OBJECT3D_FLAG_HIDDEN`), 572, 579, 580, 587, 589, 730–732, 804, 840.

**Not identified or partially, with what was done** (Ghidra EU decompilation of the
handler and its direct callees, USA source, wiki `Engine-Functions`/`Event-Scripts`):

| ID (calls) | Established | Why no name |
|---|---|---|
| 571 (172) | mask `0x10` of `Object3D::flags_` (bit 4: `OBJECT3D_FLAG_4`, **no reader in the USA source**) and bit `0x40` of the first word of a placement (type 2), set if `b=0`; always called with 570 (`570(s,b); 571(s,b)`) | consumer of the bit not found (no reference to mask `0x10` in `Object3D.cpp`; readers outside the source were not searched) |
| 574 (68), 575 (5) | map placement record `(group,id)`: `02019518` walks the map's blocks (`Zone3D+0x41c`, field `+0` = block entry identifier, here "group", always 0) then its placements (`0x70` bytes, `u16` at `+0` = identifier); 574: bit `0x4` of `+2` ("hidden at this time") set if `b=0`, bit `0x40` always; 575: position at `+8/+0xc/+0x10` (`fx32`, float value × 4096). **Executed**: a drawn placement is hidden or moved (`Event::Placement`, `PlacementMove`); in the data (`M01M12`, `X01M04`, `M01M06`) most target **collisions** (`part` of type 1), which are not drawn | placement hidden at the chosen time: built with zero scale and its original matrix, so 574 can show it and 575 move it until the map reloads; result not compared to melonDS |
| 541 (14), 542 (14), 561 (14) | 541 `(actor, kind, [p, q])`: `0219b43c` picks a 2D sprite `SPRSprite` (index `kind`: 2→2, 5→3, 6→4, 14→0xc, 15→0xd, 16→0xe, array of `0x88` bytes at `ctx+0x2b90`), places it at the screen projection of the actor's position (`0202ec84`) offset by `(−6 or −12, −76)` pixels `+(p,q)`; 542: `ctx+0x3630=0`; 561 `(a, b)`: offsets this sprite by `(a,b)` pixels (`0219b578`). **Executed**: indices 12 to 16 are the field sheets (`fieldSpriteFiles`, wiki `Sprites.md`): 13 `ev_mark` (the "!" of the scripts), in `data/bin/icon.nsarc`; drawn by `add_sprite` (overlay without fog) at the DS screen position | indices 2 to 4 (`fuki_*`) are described but the prologue scripts only use 15 (→ 13) |
| 545 (8), 546 (7) | characters: animation `run` / `stand` (`MaybeSetRegularAnimation`); placements 2/6: bit 8 of the sprite's `+0x5c` and speed `value` / `1.0`. **Executed** (`Event::SlotMotion`): hero, NPCs, script models; sprites (NPC or 521): `walk_*` (4 sectors, `NpcSprite_FaceCamera`, bit 3 of `+0x5c`) instead of `stand_*` (8 sectors) | polymorphic depending on the slot type |
| 238 (32) | `(slot, c0, c1, c2, [mode])`, 5/5/5 colour `c0\|c1<<5\|c2<<10`, mode at `Object3D+0x9e` (`SetField9e`, 0 without a fifth argument), then `0218fcf8` (ids 0–3: the hero and his 11 equipment objects `Chara_Get(0x13+12·id…)`) or `0218f92c` after the test `02191fec`. Both reread the model's **original palette** (`GetTEX0`): mode 0 **adds** the colour to each entry (capped at 31 per channel), mode 1 replaces each entry, mode 2 multiplies each channel by `c/31`. **Executed** (`Event::Tint`, `TexelOp`): operation done on the texels, in the fragment (`tint_mul`/`tint_add` of the material), on the hero, actors and model NPCs; `238(s,0,0,0)` restores the colours | the 32 prologue calls have only four arguments (mode 0, adding `(8, 6, 16)/31`, a bluish night glow); modes 1 and 2 are not observed |
| 554 (11) | writes `W+0x99c` | reader not found in `020439c0`/`0204501c`; wiki: selection of the "voice" sound **[hypothesis]** |
| 409, 411, 413 (5 each) | 409: timed caption (`W+0x195b |= 4`, `W+0x9a8 = n`; alpha `W+0x9b0`, step `0x8444` per tick, 60 ticks per fade); 411: vertical centring of the window (`W+0x195b |= 0x40`, `0206b6c8`); 413: silent text (`W+0x19b2 = 0`, `02045608`) | **Executed**: text/window alpha (60-tick fade, hold `n` ticks, 60 fade), vertical centring with 410, beep suppression (`Talk`); stopping the message counter during the caption is not modelled **[hypothesis]** |
| 559 (2) | byte `+0x490` of the singleton `0205ec44`; handler `ov_001::021609e4` | **Modelled**: `Event::SetQuestSignal(u8)`, applied before the end trigger; condition `0x1f` rereads this signed byte. At the end of 21595, value 1 selects 1.4.1 and the return to the Observatory instead of the 1.3.2 fallback. Evidence: [`trigger.md`](trigger.md), § "1.2: arrival and village tasks". |
| 560 (9) | `1` if `0202b7e8()==0` or `0202c1b4()==0` | unknown subsystem; wiki: "cutscene chained to a conversation" **[hypothesis]** |
| 547 (1) | scripted battle transition (object of the type-1 slot made visible, mask `0x40000000`) | outside v0.1 (J4): battle does not exist, the effect stays reported |
| 552 (1) | three-bone camera (`02160768`, `0204a170`), same `ctx+4` as 572/531; **executed** (eye/aim by bones, target carried by `foot`) | byte `+0x265` (839) is not driven |
| 581 (2), 582 (4) | bit 1 (value 2) of `NpcTable+0x98` (`0203cf5c`): `581(0)` sets it, `581(1)` and `582` clear it, `582` also clears `0x10000` of the 32 NPCs ("event cast NPCs", which 566 modes 4 and 5 set). Only reader found: `0203de94` (NPC loop towards `0208f87c`, with `zone+0x38/+0x3c`) only processes an NPC if `+0x98 = 0` or if it carries `0x10000`. Bit 0 of `+0x98` instead cuts sprite drawing (`0203dda4`, `0203ddf0`) | **[hypothesis]**: `0208f87c` draws the NPCs' shadow; the engine here draws none, so 581/582 have no visible effect |
| 584 (1) | door opening: placement `(block entry, identifier)`, angle ±6434 (90°), speed 409 per update, sound `g_DoorSeTable[flags & 0xf]`; **executed** (rotation of the placement, subtree collision included) | the sound is read (low nibble of the zone, base effects 200 to 204) and played; the sliding/reverse branches are not read |
| 328 (1) | enqueues type 3 (eye and target movement, like 304) towards the pose that `020a2b48` computes for the field camera (`GameState+0x3b0`, flag 4 of `0202e09c`) by resetting it to its default angle, then restores it: return of the game camera to the hero, over `N` updates. **Executed** (`Event::CameraToField`): eye and target of the field camera relative to the hero, as at the start of the event **[hypothesis]** | the game recomputes the pose from its camera state, which we do not port |
| 809 (9) | shows (`enable≠0`) or hides object `GameResources+0x4334` (mask 0 or bit 0) and `+0x4338` (bit 1); these four `Object3D` (`+0x4334..0x4340`) are loaded from four files by `ov_017::021bdc34` (state function triggered by flag `0x40` of `PartyMember+0x18c`, which calls `0218fcf8`, the colour reset, on exit): **objects of the hero's winged form** (wings, halo) **[hypothesis]**. Called with `1` everywhere in the data, at the end of a cutscene | these objects are not drawn (the hero has neither wings nor halo): `809(1)` has no effect here |
| 815 (2) | writes 1 then 0 if three probes of overlay 29 return the expected constants (wiki: integrity check) | overlay 29 obfuscated |
| 712 (48) | without output: base archive `+0xb4`; with output: archive `+0xc4`, handle written | two behaviours, no single name; played by `audio.rs` since the J3 sound work (`re/sdat.md`, § "The game's sound manager") |
| 724 (1) | stub: `return 1`, no effect | has no role |

The wiki counts 570 at millions of calls at runtime; the handler returns nothing through
an address (two integers read, return 1): it is **not** a predicate polled in a loop, but
a command repeated by the scripts.

### Effects executed on screen (J3)

`dq9-app` plays these effects (`docs/re/stb.md`, § On-screen execution; `cargo dq9re script`
decodes them as `Event`). Evidence: rows of the natives below and of the previous table.

| Natives | On-screen effect | Module |
|---|---|---|
| 578, 548/549, 588, 589, 804 | light scale (`lightRGBScale_`, `BeginFade`: linear interpolation over `2 × duration` ticks, applied to both lights, the background, the fog), forced phase (588: start of the phase; 589: phase from the save), forced lighting index, fog cut or restored | `dq9_core::cutscene::SceneLight`, `dq9_app::lighting::scene_environment` |
| 579, 808 | day clock stopped or restarted (579: `Running(!stop)`), set to the start of a phase (808: `SetTimeOfDay`); change the phase of maps loaded afterwards | `ClockCommand`, `App::apply_event_lighting` |
| 235, 236 | a model (or an actor sprite) attached to a bone of another: parent world × bone matrix (hero via `HeroRig::bone`, actors via `actor_bone`); without a name, the bone of the previous attachment | `Node::attached`, `EventPlayer::attachment_base` |
| 238 | palette operation on a model's colours (see its row) | `TexelOp`, `MaterialLight::texel_tint` |
| 328 | return of the camera to the field pose around the hero | `Event::CameraToField` |
| 521, 522 | a `.spr` sheet from `data/ani` created by handle; slots of type 2 and 6 designate it; drawn like a map NPC (billboard, `stand_*` or `walk_*`) | `Target::Object`, `AnimatedScene::add_sprite` |
| 541, 542, 561 | "!" balloon above an actor | `Balloon`, `EventPlayer::present_balloon` |
| 545, 546 | `run` / `stand` of a character or a sprite | `Event::SlotMotion` |
| 574, 575 | map placement hidden, shown or moved | `PlacementChange`, `Scene::placements` |
| 409, 411, 413 | caption: text alpha (60-tick fade, hold, fade), vertical centring with 410, typing beep cut | `Caption`, `EventPlayer::step`, `Talk::set_event_caption_alpha/centered/silent` |
| 552 | three-bone camera: eye/aim by two bones of the source, target placed on the third | `Camera::ThreeBoneCamera`, `EventPlayer::three_bone_camera/drag_three_bone` |
| 584 | door opening: 90° rotation of the placement `(block entry, identifier)` over 16 updates | `Event::OpenDoor`, `Door`, `EventPlayer::apply_placements` |
| 566 (modes 4, 5), 210/224 on a map NPC | named motions of a model NPC (archive, extra package, event packages 229/567) | `AnimatedScene::play_npc`, `add_npc_package` |

**Mode 4 of 566 (resolved).** `Event_BuildScene` (`ov_001::021539d8`, case 4) looks up the
map NPC with number `reference` in the NPC table (`NpcTable_FindById`, `0x0203df88`: the
32 entries of `g_NpcTable`, comparison of the member number of `<map>.npc`), stores it in
the reserved `Chara_Get` slot (`0xa0 + counter`, `short` at `+2` of the descriptor) and sets
flag `0x10000` on it; mode 5 does the same without a reserved slot (it uses the
number itself). Numbers 99, 139, 130… of 1150, 1160, 1600 are therefore **map
NPCs** (`dq9re cast X01M07`: `99 s019`, `139 s018`): the `stand`/`walk`/
`talk_loop` motions apply to their model. What the wiki read as a "character
table" is this NPC table.

**No effect in rendering** (the report does not count them against a cutscene): 560,
571, 815 (see their rows; 554 and 712 are sounds, played by `audio.rs`), 581/582 (NPC shadows **[hypothesis]**, not drawn),
809 (objects of the hero's winged form **[hypothesis]**, not drawn), and motions whose name no package of the model
has: `MaybeSetRegularAnimation` (`02036e44`) looks up the name in the BCFG tables and returns 0 without
changing anything (`ev010000300.chr`, `ev180003410.chr`, `ev01000s019.chr` have neither `1` nor
`nozoki_loop`). Placement changes that target a **collision** are now
counted: walls and floor change even if no image shows them.

**Executed since the "remaining cutscenes" batch**: captions 409/411/413 (alpha, centring,
silence), three-bone camera 552 and door 584. The door sound is played: the low nibble of the
zone (`Zone::sound`) gives `g_DoorSeTable`, base effect 200 + nibble, once per
opening.

**Still not executed**: 547 (battle transition, outside v0.1 without battles). Screen
swapping (`SwapScreens`) is executed (redraw and captures).

## Count by family

| Family | IDs used |
|---|---:|
| System and display 0–122 | 24 |
| Actors 200–219 | 20 |
| Actors 220–240 | 18 |
| Camera 300–328 | 17 |
| Messages 400–421 | 12 |
| Resources and misc 500–549 | 24 |
| Event and objects 550–575 | 23 |
| Cutscene and flags 576–603 | 23 |
| Sound 700–738 | 26 |
| Misc 800–845 | 41 |
| **Total** | **228** |

Unused entries are outside the inventory, notably 590/592–594/601; no
entry 818 is present in the table survey. "Used" does not mean
"identified": no-op operations and unknown fields are still listed.

## System and display (0–122)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 0 | `count_face_buttons_held` | `(&out)` | Refreshes the button state then writes **the sum of four booleans**, so 0–4, for bits `0x800`, `0x400`, `1`, `2`: Y, X, A, B. It is neither a mask nor simply a boolean. `0215ae1c`; . Transcript: decompilations of `020122ac`, `0201242c`, `02012418`, `020123f0`, `02012404`. `ev53174`, instruction `0x22e0`. The state is filtered by `020122ac`: do not present it as a raw hardware read. |
| 2 | — | `(&out)` | Writes `1` if `byte[0x02114e54+0x54] != 0` **and** `u32[+0x44] <= 9`, otherwise `0`. Used as an alternative to native 0 in a wait for user input. `0215aeb8`; . Block address obtained in the transcript by reading the literal pool `0215aeec`. `ev53174`, instruction `0x234c`. **[INFERENCE]** touch input / recent touch. The exact meaning of the boolean and of threshold 9 has not been established: no `is_touch_pressed` name at this stage. |
| 3 | — | `(mode:int)` | Reads `mode` and obtains the flag block via `020d6c10`. If zero: calls the flag setter with mask `1`. Otherwise: calls the clearer then `020dc2e0(0)`, which recomputes an engine state and performs other updates. `0215aef0`; . Transcript: decompilations of `020466f4`, `02046704`, `020dc2e0`, `020dc224`. `evluida`, instruction `0x5dcc`, before 117(30). Controlled feature unknown. The decompilation omits the clearer's arguments in this branch: do not turn this into a certain gameplay signature. |
| 7 | `random_int` | `(min:int, max:int, &out)` | Writes a random integer **bounds inclusive**: `min + random_below(max-min+1)`. `0215afec`; . `02074488` and `0207430c` decompiled in the transcript, also . Four sites in `evluida`. No established guarantee on the exact distribution or on swapped bounds. The handler's guard only checks `argc >= 2` while it writes to the third argument; the scripts do pass three arguments. |
| 8 | `enable_event_chaining` | `()` | Sets to `1` the marker `block_returned_by_0200f398 + 0x5cac`, for continuity between events. `0215b040`; . `ev11600:263-268`: message "connection to the next event via a trigger", immediately followed by 8. : this marker is propagated to the warp context. The name describes the established use; it is not a general scenario-flag native. Not all effects of this continuity are identified. |
| 9 | `disable_event_chaining` | `()` | Resets this same marker `+0x5cac` to `0`. `0215b058`; . `ev01520:29-40`: call in the initialisation entry 200. Same caveat on indirect effects as for 8. |
| 100 | `fade_in_both` | `(d:int)` | `SetBrightness(res, 0, d)`: target brightness `0` (normal) on the main **and** sub engines. `0215b2cc`, → `0203b238`. Non-blocking. Duration: **d × 16.667 ms** (§Time). A possible second argument is read but ignored. |
| 101 | `fade_both` | `(d:int [, b:int=-16])` | Requests target `b` on both channels. Default black `−16`; the three explicit targets observed are `+16`. `0215b318`,  → `0203b238`. Also writes an event flag `+0x102` under a condition, described below; meaning unknown. |
| 102 | — | `(a1:int [, a2,a3,a4])` | **Only `a1` is read.** Via `020708d8`, writes a value derived from `a1` at `object+8`, then `object[4]=1` and `object[0]=1`. `0215b384`, . Transcript: `0207050c` → block `0x02108d5c`, decompilation of `020708d8`. **[INFERENCE]** return/clearing of a colour effect. The three extra arguments are ignored; do not attribute to it the RGB signature of 103. |
| 103 | — | `(r:int,g:int,b:int,d:int)` | Via `02070910`, writes the 16-bit colour `r \| (g<<5) \| (b<<10)`, a value derived from `d` at `+8`, then `object[4]=0x1f` and `object[0]=1`. `0215b3ac`, ; decompilation of `02070910` in the transcript. RGB15 colour established; **[INFERENCE]** colour fade/overlay. Display target and interpolation not recovered. Values `255` exist: no explicit clamp in this helper. |
| 105 | `fade_sub` | `(d:int [, b:int=-16])` | `SetSubBrightness(res, b, d)`: **sub** engine (`REG_MASTER_BRIGHT_SUB`, `0x0400106C`), target `b`. `0215b454`, → `0203b1ac`. Same `+0x102` side effect as 101. Lock `+0x25`: no effect if it is set. |
| 106 | `fade_in_main` | `(d:int [, ignored:int])` | `SetMainBrightness(res, 0, d)`: **main** engine (`REG_MASTER_BRIGHT`, `0x0400006C`), fixed target `0`. `0215b4c0`, → `0203b120`. Second argument always `16`, **read but ignored**. Lock `+0x24`. Physical screen: see 109/110; **[hypothesis]** main is the top screen by default (wiki). |
| 107 | `fade_main` | `(d:int [, b:int=-16])` | `SetMainBrightness(res, b, d)`: **main** engine, target `b` (−16 black … +16 white; other values are used). `0215b50c`, → `0203b120`. |
| 109 | `set_display_swap` | `()` | Sets **bit 15 of `POWCNT1` (`0x04000304`)** to 1, without touching the other bits. `0215b5b4`,  → `0215b598(1)`. Transcript: helper decompiled and literal address `0215b5b0` read. The hardware operation is established. Avoid a "top screen" name without making the main/sub convention explicit. **Executed**: `Cutscene::screen_swapped`, applied by `screens::field/overlay` (window and capture); the state survives chained events. |
| 110 | `clear_display_swap` | `()` | Clears this bit 15. `0215b5c8`,  → `0215b598(0)`. Same caveat as 109. **Executed** like 109. |
| 112 | `fade_both_and_lock` | `(d:int)` | Dispatch mode **7**: fade of both channels to `−16` by default, then locks both. `0215b5f8`,  → `0215b074(7,…)` → `0203b290`. The common helper accepts an optional target; no two-argument call observed for 112. Meaning of the secondary event flag unknown. |
| 114 | `fade_sub_and_lock` | `(d:int)` | Mode **9**: `SetAndLockSubBrightness(res, −16, d)` (name confirmed by the Ghidra decompilation), then lock `+0x25`. `0215b630`, → `0215b074(9,…)`. Event side effect `+0x102` under a condition. |
| 116 | `fade_main_and_lock` | `(d:int [, b:int=-16])` | Mode **11**: `SetAndLockMainBrightness(res, b, d)`, then lock `+0x24`. `0215b668`, → `0215b074(11,…)`. |
| 117 | `unlock_and_fade_in_both` | `(d:int [, ignored:int])` | Mode **12**: unlocks both channels, then requests fixed target `0`. `0215b684`,  → `0215b074(12,…)` → `0203b2f0(obj,0,d)`. The second argument is **ignored**, even though the dispatcher reads it; its observed values are 16 and 8. |
| 118 | `unlock_and_fade_both` | `(d:int)` | Mode **13**: unlocks both channels, then requests `−16` by default. `0215b6a0`,  → `0215b074(13,…)` → `0203b2f0(obj,b,d)`. Optional target accepted by the helper, not used here. |
| 119 | `unlock_and_fade_in_sub` | `(d:int)` | Mode **14**: `UnlockAndSetSubBrightness(res, 0, d)`. `0215b6bc`, → `0215b074(14,…)` → `0203b2dc`. |
| 120 | `unlock_and_fade_sub` | `(d:int [, b:int=-16])` | Mode **15**: `UnlockAndSetSubBrightness(res, b, d)`. `0215b6d8`, → `0215b074(15,…)`. Do not call this native a wait. |
| 121 | `unlock_and_fade_in_main` | `(d:int [, ignored:int])` | Mode **16**: `UnlockAndSetMainBrightness(res, 0, d)`. `0215b6f4`, → `0215b074(16,…)` → `0203b2c8`. Second argument read but **ignored**. |
| 122 | `unlock_and_fade_main` | `(d:int,b:int)` | Mode **17**: `UnlockAndSetMainBrightness(res, b, d)`. `0215b710`, → `0215b074(17,…)`. Helper default: `−16` if the target is missing, but the only call supplies `+16`. |

## Actors (200–219)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 200 | `load_character_model` | `(path:string, id:int, [selector:int=0])`. Loads in particular a `.chr`, allocates an object of `0xac` bytes and registers it in the character/model table. | **0215b72c** → `0200fd48`. The converted identifier must be `0xa0..0xbf`, i.e. normally `id=-1..-32`. The last integer **selects the allocator** (`021658b8[selector]`); it is not a duration and does not change the size. Ends with `Object3D::SetScale(0x10a, 0x10a, 0x10a)` (constant `0215b9c0`) whatever the selector. Does not by itself bind an STB slot. |
| 201 | `set_character_visible` | `(id:int, [value:int=1])`. Accesses `Chara_Get` directly; `value=0` hides the model, otherwise shows it. | **0215bd5c** → `02037418` (`Object3D::MakeHidden`) / `02037408` (`MakeVisible`), names from [the USA decompilation](../../tools/dqix-decomp/src/World/Object3D.cpp) (bit 0 of `flags_` = `OBJECT3D_FLAG_HIDDEN`, "if set, no drawing"). Unlike 200/212, no conversion of negative identifiers in the observed wrapper. |
| 202 | `bind_actor_object` | `(b:int, id:int, type:int)`. Resets a binding then records in it the type, converted identifier and pointer to the existing object. | **0215bdd0** → `02164214` / `02164248`. Table of **32 entries of 16 bytes**. Types 0/1/4/5: character table; 2/6: other object table, setting bit `0x10000` on the object. Details §4. |
| 203 | `reset_actor` | `(a:int)`. Clears the actor's queues and counters; removes its binding (`actor+0x70=-1`) and restores the default values. | **0215bf34** → `02159d9c`. Reset of the STB controller, **not destruction of the model**. |
| 204 | `get_actor_busy` | `(a:int, out:&int)`. Writes **1 if commands remain pending**, 0 otherwise. | **0215bf58** → negation of `0215a094`, which examines eight queues. Includes special handling of the last animation marker. It is not a simple "character moving" test. |
| 205 | `set_actor_binding` | `(a:int, b:int)`. Assigns the binding entry used by the actor controller. | **0215bf94** → `02159ea8`: writes `actor+0x70`. Allocates and loads no model. |
| 206 | `set_actor_position` | `(a:int, x:float, y:float, z:float)`. Enqueues an absolute position assignment. | **0215bfd4** → `0215a330`, command **1**, executed by **0215929c**. Copies to `actor+0x74`, then to the object. The fifth arguments of two calls are **ignored by this wrapper**, not durations. **USA source: not found** in `tools/dqix-decomp` (no native table or SB2 VM in the clone). **Wiki** (`Engine-Functions.md` l.40, 49; `Event-Scripts.md` l.153): "places a character", **[hypothesis] EU, not verified**. |
| 207 | `move_actor_to` | `(a:int, x:float, y:float, z:float, duration:int, [arrival_animation:string])`. Enqueues a linear interpolation to an absolute position. | **0215c06c** → `0215a364`, command **2**, processing **021592bc**. The wrapper sets the movement mode to 0. The optional string is used at the end. Exact time unit §3. |
| 208 | `set_actor_rotation` | `(a:int, rx:float, ry:float, rz:float)`. Enqueues an absolute assignment of the three angles. | **0215c138** → `0215a44c`, command **7**, processing **021594ac**. Angles in **radians**; middle component = rotation around Y, commonly used for orientation. Full Euler convention not established. **USA source: not found** in the clone. **Wiki** (`Engine-Functions.md` l.36; `Event-Scripts.md` l.180): radians, y = orientation; consistent with 544 in radians, **[hypothesis] EU**. On the USA side, `Object3D` (`World/Object3D.h` l.110–113) carries `rotation_` with no proven link to this native. |
| 209 | `rotate_actor_to` | `(a:int, rx:float, ry:float, rz:float, duration:int, [direction:int=0])`. Timed rotation; with a negative duration, repeated addition of the three angles. | **0215c1d0** → `0215a480`, command **8**, processing **021594cc**. Direction 0: positive progression modulo one turn; non-zero: negative. **Not necessarily the shortest angular path.** Seventh argument observed but ignored. |
| 210 | `play_actor_animation` | `(a:int, name:string, [flags:int=0], [value:float=1])`. Enqueues the choice of a named animation: `stand`, `walk`, numeric names, etc. | **0215c298** → `0215a550`, command **10**, processing **02159a00** → `02164578` → `02036e44`. The third argument is a **bit mask**, not a duration. The fourth is converted and stored, but **its effect on speed is not established**: the decompiled relay does not consume it explicitly. **USA source: not found** in the clone. **Conflict to resolve**: the bits above (`0x01` stop without looping) differ from the wiki (`Event-Scripts.md` l.181, EU, **[hypothesis]**) which speaks of "bit 1" = play once. The two readings are not settled; the wiki is not retained. |
| 211 | `set_actor_scale` | `(a:int, sx:float, sy:float, sz:float, [duration:int=0])`. Per-axis scale; direct assignment if duration ≤0, interpolation if positive. | **0215c330** → `0215a608`, commands **16/17**, processing **02159b5c / 02159bbc**, then `02164754` → `020374f0`. Relative scale, not position; model fields at `+0x5c/+0x5e/+0x60`. |
| 212 | `release_character_model` | `(id:int)`. Converts the identifier, frees/resets the model's resources, removes its pointer from the table, then invalidates type-1 bindings targeting this identifier. | **0215b9c4** → `02037340`, `0200fd58`, `0215acb4`. **Not an STB slot as argument.** The exact memory release of the `0xac`-byte object has not been traced. |
| 213 | `wait_actor_animation` | `(a:int)` | Enqueues an end wait in the animation channel, without directly blocking the script. `0215c3e8` → `0215a5d8`, command 12 → `02159b04` → `02164624` → `02037180` (`object+0x40 & 1`). The "end" meaning is corroborated by `02034ea0`/`020352f8` and predicate 234. |
| 214 | `reset_actor_path` | `(a:int)`. Enqueues command 3. | **0215c40c** → `0215a3c0` → **02159408** → `02157908`: clears the **16 points** (`12` bytes) of object `actor+0x11c`, the point counter (`+0xc4`), the speed (`+0xc0`) and the path indicators. Established. |
| 215 | `set_actor_path_speed` | `(a:int, value:int)`. Enqueues command 4. | **0215c430** → `0215a3d8` → **0215941c**: writes `actor+0x1dc` = path `+0xc0` (`actor+0x11c+0xc0`). `021579ac` divides the length of each segment by this value (`FX_Div(length, value<<12)`): it is the **length → duration divisor** ("speed"), duration unit **[unknown]**. |
| 216 | `add_actor_path_point` | `(a:int, x:float, y:float, z:float)`. Enqueues command 5; values ×4096. | **0215c468** → `0215a3f8` → **0215942c** → `02157964`: adds a point to the table of 16 (`12` bytes), **silently ignored** beyond that (non-zero return). The fifth float of the 42 calls is ignored. Established. |
| 217 | `run_actor_path` | `(a:int, [value:int=0])`. Enqueues command 6. | **0215c4fc** → `0215a42c` → **0215944c**: on the first pass, `021579ac` computes the per-segment durations, `02157acc` activates the path (`+0x464=1`); on each update `02157ae0` samples the curve into `actor+0x74` (position) and sets the Y angle `actor+0x84` via `FX_Atan2` of the tangent. The channel is **held** while `actor+0x580 != 0`: it is a walk that blocks the actor until the end. Curve passing through the points; spline type **[hypothesis]** (Catmull-Rom, wiki). The optional integer is not read. |
| 218 | `delay_actor_animation` | `(a:int, count:int)`. Adds a wait **in the animation channel**, without directly suspending the script or the other channels. | **0215c544** → `0215a530`, command **9**, processing **021599dc**. Increments a counter by 1 per pass while it is below `count`, then finishes and resets. Not to be confused with the `2×duration` timing of 207/209. |
| 219 | `set_actor_alpha` | `(a:int, alpha:int)`. Enqueues the alpha assignment. | **0215c57c** → `0215a4c8`, command **18**, processing **02159804** → `02164908` → `02037428`. The latter stores the alpha in the **upper 5 bits of `object+0x40`**. Usual values 0/31; no clamp observed, and scripts also pass 255. |

## Actors (220–240)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 220 | `fade_actor_alpha` | `actor:int, alpha:int, duration:int` | Handler **0215c5b4** → **0215a4e8**, command `0x13` → executor **02159820**: interpolation of the actor field `+0x8c`, then application via **02164908 → 02037428**. The latter stores the alpha on 5 bits. **9 calls have only two arguments**, while the handler reads the third without a guard: do not invent a default duration. |
| 221 | `turn_actor_towards_actor` | `actor:int, target_actor:int, duration:int [, direction:int]` | **0215c5fc** computes an orientation from `Δx, Δz`, keeps the other two rotation components and enqueues command `8` via **0215a480**. Executor **021594cc**: interpolated rotation with angular wrap-around. Without arg4, shortest path chosen, threshold `0x3244`. Physical left/right meaning of the direction values not established. One call has five arguments; the fifth is not read. Some calls supply floats to the integer readers: anomaly kept. |
| 222 | `stop_actor_animation` | `actor:int` | **0215c70c → 0215a5f0**, command `0xe` → **02159b34 → 021646b8 → 02037148**. Clears the current animation resource, the index and the transition state. **Queued** command, not immediate clearing of the whole animation queue. |
| 223 | — | `slot:int, enabled:int` | **0215c730**: only if the slot type is `2`. `0` calls **020406c0**, non-zero **02040678**. The kept decompilations show respectively setting/clearing bit `1` of the rendered object, or calls to the character variants **02037418/02037408**. Certainly toggles a state of the object, but **visibility versus exact activation not demonstrated** in the survey. Do not name it `set_visible` without evidence of the bit's consumer. |
| 224 | `queue_actor_animation` | `actor:int, animation:string [, flags:int=0 [, value:float=1.0]]` | **0215c788 → 0215a594**, command `0xb` → **02159a74**. **Waits for the end of the current animation**, then calls **02164578 → 02036e44** with the name and flags. The fourth argument is converted to fixed-point ×4096; no observed call uses it. Do not present it as a simple alias of 210: **210 starts without waiting for the end**, unlike 224. The actual propagation of the parameter named `speed` to the engine is not fully established; . |
| 226 | `set_actor_radius` | `actor:int, value:float` | **0215c858 → 0215a654**, command `0x15` → **021598e0**: writes `value ×4096` into actor `+0x90`. Application **021649d4 → 020377d4 = `Object3D::SetRadius`**: field `radius_` (`+0x64`, collision/bounding radius, in fx32), only for a type `1` object. Name established by the USA decompilation. |
| 227 | `fade_actor_radius` | `actor:int, target:float, duration:int` | **0215c8a4 → 0215a674**, command `0x16` → **021598f0**: interpolation of the same actor field `+0x90`, then same application (`SetRadius`). Duration 0: nothing is written. The counter advances by `1` per execution; not to be confused with the ×2 counters of movements/rotations. |
| 228 | `clone_character` | `source_character_id:int, destination_character_id:int [, allocation_selector:int=0]` | **0215ba64**: normalises the identifiers, allocates `0xac` bytes, `Object3D::Initialize`, **`Object3D::ShallowCloneTo(source → copy)`** (same model and animations, without copying dynamic memory), registers the copy through **0200fd48**, applies `SetScale(c,c,c)` with a single constant (wiki: `0x10a`) and `MaybeSetBCFGAnimation(0,0)`. The third argument indexes the allocator table `021658b8`. Established (the wiki notes that no script passes the third argument). |
| 229 | `load_character_archive` | `resource_path:string, character_id:int [, allocation_selector:int=0 [, resource_tag:int=3]]` | **0215bb3c**: retrieves an existing character; `data/%s`; file already loaded (`BackgroundLoader::GetLoadedFileByName`) or force-loaded (`AddLockGlobal`/`0215a750`); depending on bit 0 of the first word of the attached resource: **`Object3D::LoadFromCHRArchive`** or **`LoadFromCCHROrCMOTArchive`** with an `ObjectArchiveLoadInfo` carrying the tag (default **3**), then `MaybeSetBCFGAnimation(0,0)`. Same calls as scene construction for 567: **loads animations/resources into an existing character, identified by the tag** (removed by 230). Whether such an archive also replaces the model is **[unknown]**. |
| 230 | `remove_character_animation_package` | `character_id:int [, resource_tag:int=3]` | **0215bcc0**: remembers the name and indicators of the current animation (`+0x14`/`+0x19`), **`Object3D::RemoveAnimationPackageByID(tag)`**, then restores the animation through `MaybeSetRegularAnimation`; if the name no longer resolves, plays **`stand`**. Established (names from the USA decompilation). |
| 232 | `move_actor_on_ground` | `actor:int, x:float, z:float, duration:int [, animation:string]` | **0215c994 → 0215a364**, command `2`, mode `1`, coordinates ×4096. Executor **021592bc**: interpolation of X/Z, recomputation of Y by **02018fcc**, then optional animation on arrival. Arg5 supported but never used in the statistics. The handler initialises Y to `0xa000`; at the end, ground mode only copies back X/Z. Internal time threshold `duration ×2`. |
| 233 | `load_monster_character` | `monster_path:string, character_id:int [, allocation_selector:int=0]` | **0215ca4c**: looks up in **`data/pack_lv5/enemy.gp2`**, extracts **`.cchr`**, builds the character via **02075664 / 02036814**, registers it through **0200fd48**, then `Object3D::SetScale(0x10a, 0x10a, 0x10a)` (constant `0215ccd8`). Examples: `enemy/z000a.mon`, `enemy/b012a.mon`. The selector picks the allocator (`021658b8`, like 200). |
| 234 | `get_animation_active` | `slot:int, out:address` | **0215ccdc**: for types `0,1,4,5,6` with a non-null pointer, writes **`02037180(pointer) == 0`**. **02037180** reads the animation end bit; the updates **02034ea0 / 020352f8** establish this meaning. For other types, writes `1`. **For an accepted type with a null pointer, does not touch the output.** It is therefore not a uniform predicate over all slots. |
| 235 | `attach_character_to_bone` | `child_slot:int, parent_slot:int [, bone:string]` | **0215cd50 → 020378a0**: resolves both pointers, looks up the named node on the parent's render through **0207ea48**, stores the index in child `+0x92`, then calls **02037868**. `head` accounts for 280 calls; others: `arm1R`, `chest`, etc. All calls supply the name. Without a name, reuses the already stored index. The function refuses a new attachment if child `+0x94` is already non-zero. |
| 236 | `detach_character` | `slot:int` | **0215cdf8 → 020378ec**: modifies the links `+0x94/+0x98`, then resets via **02037850**. **If no previous link is present, also walks the following links**: do not reduce its semantics to detaching only the current object. |
| 238 | — | `slot:int, c0:int, c1:int, c2:int [, mode:int=0]` | **0215cf98**: packs **`c0 \| c1<<5 \| c2<<10`**, writes the mode through **02037af8** into character `+0x9e`, then calls **0218fcf8** for identifiers 0–3, **0218f92c** otherwise. 5/5/5 colour very strongly indicated; **exact destination — tint, lighting, outline… — unknown**. Meaning of the mode unknown; 15 calls pass `2`. No semantic name retained. |
| 239 | — | `first_slot:int, second_slot:int` | **0215d08c**: looks up the engine object matching identifier `first_slot.+4` through **0203dcf4**, then writes `second_slot.+0xc` into the found object at **`+0x10`**. **Role of the `+0x10` link unknown**. It is not the same attachment as 235. |
| 240 | — | `slot:int` | **0215d0f4**: same resolution as 239, then sets **`object.+0x10 = 0`**. Counterpart of 239 established; semantics of the link unknown. |

## Camera (300–328)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 300 | `reset_camera_commands` | None | Handler `0215d134` → `021588e4`: empties the controller's queues, counters and states. Also enables camera processing by setting global flag `+0xe4` to 1 if it was zero. , , . **Does not restore a default view**: it is not a simple "reset camera position". |
| 301 | `get_camera_busy` | `out busy: &int` | `0215d168` → `02158ab8`, then writes the inverse of the result: `busy=1` if at least one queue head `+8`, `+0x10`, `+0x18`, `+0x20` is non-null; otherwise 0. , . The update loop processes **five** queues, but this predicate only examines four. Actor tracking can keep it at 1. |
| 302 | `set_camera_position` | `x,y,z: float` | `0215d19c` → `02158efc`: enqueues type 1; executor `02157bfc` copies the vector into controller `+0x4c`, then camera `+4`: `E`. , , . Immediate **when the queue is processed**, not a synchronous write by the handler. |
| 303 | `set_camera_target` | `x,y,z: float` | `0215d234` → `02158f30`: type 2; `02157c1c` writes controller `+0x58`, then camera `+0x10`: `T`. , . Cartesian coordinates, not angles. |
| 304 | `move_camera_position_and_target` | `eye_x,eye_y,eye_z,target_x,target_y,target_z: float`, `N: int`, `[snap: int=0]` | `0215d2cc` → `02158f64`: type 3; `02157c3c` interpolates both vectors at constant speed, separately. The handler also forces the internal field `+0x20=1`. , . `snap!=0` forces the exact destinations at the end. This eighth argument is **not an easing**. |
| 305 | `move_camera_target` | `target_x,target_y,target_z: float`, `N: int` | `0215d418` → `02158fc4`: type 4; `02157e18` interpolates only `T`. , . No explicit final forcing in this executor. |
| 306 | `move_camera_position` | `eye_x,eye_y,eye_z: float`, `N: int` | `0215d4bc` → `02159004`: type 5; `02157f10` interpolates only `E`. , . Same caveat on final rounding. |
| 310 | `set_camera_orbit` | `θ,h,r: float` | `0215d608` → `0215913c`: type 13; `02158038` sets the three orbital parameters and requests their application. , , `cam5.txt`, function `0202e5e8`. **The second float is a height, not a pitch**; the third is the **total** distance, not the horizontal radius. One call has a fourth argument, ignored by the handler. |
| 311 | `move_camera_orbit` | `θ,h,r: float`, `N: int`, `[dir: int=-1]`, `[snap: int=0]` | `0215d684` → `02159168`: type 14; `02158080` interpolates the three scalars, with circular handling of `θ`. , . `dir=-1`: shortest path; `0`: decreasing angle; other value: increasing. `snap!=0` forces the three final values. |
| 317 | `shake_camera` | `amplitude_x,amplitude_y,amplitude_z: float`, `N: int` | `0215d8f4` → `021591d0`: type 16; `02158494` offsets **eye and target together**, alternating addition/subtraction according to the counter modulo 4. Saves/restores the base pose between updates. , . `N>=0`: duration in number of shake update calls; `N<0`: persistent. The amplitude is recomputed during the command; **do not claim a simple linear decay**. |
| 321 | `move_camera_target_preserving_orbit` | `target_x,target_y,target_z: float`, `N: int` | `0215dbd0` reads the current orbital parameters, rebuilds the eye−target offset, computes `E_destination=T_destination+offset`, then enqueues the same type 3 as 304. . Orbital parameters captured **when the native is called**, not when its command later starts. |
| 322 | `move_camera_target_and_orbit` | `target_x,target_y,target_z,θ,h,r: float`, `N: int`, `[dir: int=-1]` | `0215dd58` enqueues a type 3 on the Cartesian queue and a type 14 on the orbital queue, with the same duration. . Type 3 receives an eye destination `(0,0,0)`, not a final eye computed here; the orbital parameters then rebuild the eye. Do not present this native as a simple linear interpolation between two full poses. |
| 324 | `follow_actor_with_camera_target` | `actor: int`, `[offset_x,offset_y,offset_z: float=0]`; four arguments in the scripts | `0215dfc4` resolves `Stb_Actor`, then `02159250` stores the actor in controller `+0xa20` and enqueues type 18. `021587c4` writes on each update `T=actor.position(+0x74)+offset`. , . Follows **the target**, not directly the eye. Missing actor: handler returns 0. The command stays active while the followed pointer is non-null. |
| 325 | `stop_camera_target_follow` | None | `0215e0c0` → `02159290`: sets controller `+0xa20` to zero; type 18 ends on its next processing. , . Does not cancel all camera commands. |
| 326 | `shake_camera_target` | `amplitude_x,amplitude_y,amplitude_z: float`, `N: int` | `0215d984` → `02159210`: type 17; `02158654` applies the oscillation to **T only**, and forces reapplication of the orbital parameters. , . The eye can therefore be rebuilt around the modified target. Same `N>=0` / `N<0` distinction and same caveat on the envelope as 317. |
| 327 | `set_camera_roll` | `angle_degrees: number`, read as float | `0215e0dc` converts to fx32, multiplies by `0x47/4096` (game degree), normalises via `fix32ReduceAngle0To2Pi`, truncates to 16 bits and calls `0202e884`, which writes camera `+0x7c` and resets `+0x1ec/+0x1ee` to zero. **Reading of the field**: the view matrix builder `0202e0b4` takes the up axis `(0,0x1000,0)` as is when `+0x7c==0`, otherwise rotates it by `RotationMatrixZ(+0x7c)` before the heading rotation: it is a **roll** (tilt around the view axis), established. Not a field of view (532). |
| 328 | — | `N: int` | `0215e14c` obtains a camera via `02010108`, conditioned by bit 4 of `0202e0ac`, calls `020a2b48` to obtain vectors, then enqueues type 3, duration `N`, internal fields `+0x20=0`, `+0x24=0`. , `cam5.txt`, function `02010108`. **Destination not identified**: no kept decompilation of `020a2b48`. "Return to normal camera" remains a hypothesis, not an established name. |

## Messages (400–421)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 400 | `show_message` | `message_id: int` or `text: string`, `[flags: int]` | `0215e2a4`. **Integer**: linear search of the key in the event's `{key, text}` table (8-byte stride) (`02153884`); key missing or null text: **nothing is displayed, return 0**. **String** (tag 2): direct text, no table. In both cases: `W+0x1838 = -1`, `0204501c(W, text, p, 0xe3, …)`, then `W+0x998 = 1`. `p` is 1 unless **bit 0** of the 2nd argument (integer branch only) is set, in which case `p=0`: the bit is therefore **inverted**. Role of `p` in `0204501c` **[unknown]**. No string call in the scripts. |
| 401 | `close_message` | none | `0215e398`, . Calls `02043134` then `02043214`: deactivation/reset of the message. Example `ev28820`, `0x11700`, after message 112 and a 60-frame wait. It is not simply clearing the text: also resets the state and some graphics resources. |
| 405 | `get_message_state` | `&out: int` | `0215e374`, . Copies **byte `W+19bd`**, not `W+998` nor `W+9a0`. `ev03030`, helper `fn@0xe3c`, waits while result `== 1`. **1: message in progress/wait not finished; 0: state reset.** Exhaustive list of values and natural transitions not recovered. |
| 409 | — | `value: int` | `0215e4b4`: `W+195b \|= 4`, `W+9a8 = value`. **Rendering** (`02066d00`): if bit 4 is set, the text alpha is `W+9b0 >> 16 & 0x1f`; the update (`020657c8`, step `0x8444` per tick at `0x020658b8`) moves `W+9b0` from 0 to `0x1f0000` (60 ticks = 1 s), holds `value` ticks, then goes back down. It is a **timed caption** (one-second fade, hold, fade), not an auto-close: the script closes with 401. Prologue values: 140, 180, 260. |
| 411 | — | none | `0215e514`: `W+195b \|= 0x40`. **Rendering** (`0206b6c8`): counts the line breaks and places the window at `(192 − (lines − 1) × 20 − 8) / 2 − 16`, text at the usual margin, instead of the box's `0x74`. Without 410, the box geometry is rewritten every frame (`020439b0`): the offset does not survive. **Vertical centring of the caption**. |
| 413 | — | none | `0215e564`: `W+19b2 = 0` (default 1 set by `0204501c`). **Rendering** (`02045608`): the typing-beep routine returns without playing when `W+19b2 == 0`. **Silent text** (`docs/re/sdat.md`: sounds 10/12/11). |
| 410 | `disable_message_box` | none | `0215e4f0`: `W+0x19b1 = 0`. The display routine `0204501c` initialises it to 1; the window update (`020439c0`) only draws the **frame** `(2, 0x74, 0xfc, 0x4a)` if `W+0x998 != 0` and `W+0x19b1 != 0` (or `W+0x19cb != 0`, see 420). 410 therefore removes the frame; the text remains. Established. |
| 414 | — | none | `0215e588`, . `W+195b \|= 0x80`. In the `+19ce` rendering branch, selects several offset draws of the glyph instead of the simple draw (). [INFERENCE] Outline/shadow effect; offsets and exact effect not established. |
| 417 | — | none | `0215e900`, . `W+19c0 = 1`, `W+195d = 30`. These fields are initialised respectively to 0 and 30 by `0204501c` in the transcript. Neither the flag's role nor the unit of 30 demonstrated. |
| 418 | — | `value: int`, truncated to a byte | `0215e938`, . `W+19ae = value`. Only argument observed: **0**. `ev29306`, helper `fn@0x2bfc`: after 400, repeats `418(0)` every frame for a duration, then 401 (`0x2c64–0x2da8`). [INFERENCE] Prevents a progression/action during the wait; exact consumer missing. |
| 420 | — | `enabled: int` → boolean | `0215e97c`, . `W+19cb = enabled != 0`. In `02066d00`, this boolean forces entry into the window rendering block independently of `W+998` and `W+19b1` (). `ev29320`: enabled during "Saving adventure log…", then disabled (`0x1324`, `0x14c8`). "Keep/force window display" supported; do not extrapolate an input lock or a save-specific role. |
| 421 | — | none | `0215e920`, . `W+19c1 = 1`. `ev29120`, helper `fn@0x2674`: 400, 421, then wait on 405 (`0x26dc–0x27cc`); three other calls in `ev29190`. Consumer and difference with 417 unknown. |

## Resources and misc (500–549)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 502 | `restore_texture_arena` | `int index, [int option=1]` | `0215eab0`. Selects a `TextureArena` (`0x70` bytes): `GameResources+0x2cc+index*0x70`, or `+0xe9c` if the index is negative (= index 27: `0x2cc+27×0x70`, default value of 521). If `option` is non-zero (default 1) calls `TextureArena::Reset`, then in all cases `TextureArena::Restore` (names and semantics from [`TextureArena.h`](../../tools/dqix-decomp/include/Graphics/TextureArena.h): state of the texture/palette VRAM pool committed in the SDK). Index unbounded. Opens a VRAM allocation scope: to be closed by 503. |
| 503 | `save_texture_arena` | `int index, [int option]` | `0215eb3c`. Same selection as 502; calls `TextureArena::Save` (rereads the SDK allocation state into the arena, i.e. closes the scope opened by 502). Non-zero `option`: computes state differences (absolute values) whose result is **discarded**; no effect. |
| 506 | `preload_resources` | `string paths...`; 1–12 observed | `0215ed2c`, . Resets the tracking counter, submits each path via `0215ec94` and keeps the handles. The transcript shows requests per file or archive member. Asynchronous preload established; it is not actor creation. The call resets the list without itself freeing its old handles. |
| 507 | `get_preload_pending` | `&int pending` | `0215edb8`, . Writes **1** if a tracked handle returns state 0 to `0202fde0`, otherwise **0**. **0 does not guarantee success**: the state reader can return `-1` for a missing handle or certain failure states, which 507 considers not pending. |
| 508 | `release_preloaded_resources` | none | `0215ef44`, . Calls `0215aac8`: removes each tracked handle via `020301d8`, replaces the entries with `-1`, resets the counter. Frees the tracked requests/handles; do not generalise to "destroys all actors" or "empties all caches". |
| 509 | `set_character_flags` | `int character, int mask, int enabled` | `0215eeb0`, . Resolves the character via `0215ad2c` then `Chara_Get`. `enabled!=0`: OR of the mask into `character+0x6c`; otherwise AND with its complement. Evidence of the operations: . Meaning of the bits unknown. A negative identifier is turned into `0x9f-id`, **not** into an STB actor slot. |
| 512 | `set_resource_flags_04` | `int mask, int enabled` | `0215f1b0`. `enabled==0`: `ClearFlags04(GameResources, mask)`; otherwise `SetFlags04`. Word `GameResources+0x04`: a set bit **suspends** the update of a subsystem (≈36 sites `test; bne skip`, including those of 568). Common masks: `0x2000`, `0x40`, `2`. Identity of the subsystems per bit **[unknown]** (0x8 and 0x400: see 536/568). |
| 521 | `create_sprite` | `string name, &int handle, [int heap_index=0], [int block_index=27]` | `0215f3fc`, . Reserves a slot `<32`, writes its number at the supplied address, loads a `.spr` resource, builds the object and inserts it via `0203dd20`. Recovered paths: `data/ani/%s` or `data/ani/%s.spr`. `heap_index` selects a table entry; do not give it a more precise meaning. The last index selects a `0x70`-byte block of the same family as 502/503. |
| 522 | `destroy_sprite` | `int handle` | `0215f740`, . Finds the object by handle, frees its components, calls `02040514`, clears the slot via `0203dd20(...,0)` and resets the corresponding type 2/6 STB references. The object may follow two internal variants; component ownership details not established. |
| 531 | `restore_saved_camera` | none | `0215f930`. If `ctx+4` (`ctx=02010778()`, former `+4` of the event state) is non-zero, reinstalls this object through the setter `020100d4` of `GameState::unknown_3b0_` ("probably a high-level camera" in the USA decompilation) then resets `ctx+4` to 0. 572 and 552 write `ctx+4 = 020100cc(save)` before installing their camera: 531 is the counterpart, **established** by the code of the three handlers. Has no argument. |
| 532 | `set_camera_fov` | `int angle`; handler also accepting `float` | `0215f968`. Integer: `angle<<12`; float: `×4096`; calls `0202e9b4` on camera `020100cc(save)`, which writes `+0x58` and the sine/cosine at `+0x5c/+0x60`. The view builder `0202e0b4` passes `+0x5c/+0x60` to `MTX_PerspectiveW`: it is indeed the projection angle. **Unit: degrees** (same `0x47/4096` conversion as 327). **[unknown]**: half-angle or full field (the Nitro SDK expects the sine/cosine of the vertical half-angle; the engine default `60` argues for a full field). |
| 536 | — | `int selector, int value` | `0215ee2c`, . Selector 0 → mask `8`, selector 1 → mask `0x400`. Zero value: calls `0203b4e8`; otherwise `0203b4f8`. Meaning of the selectors unknown. Call polarity inverse of 512. |
| 537 | — | `&int result` | `0215ef20`, . Writes the result of `020100b8`, reader of field `save+0x3ac` (). This result is used elsewhere by `Chara_Get` and `Party_List`, but its exact identity is not established by the source: do not impose "hero" or "leader". |
| 538 | `set_next_event` | `int id`, truncated to 16 bits | `0215f9dc` writes `ctx+0x11a` (`ctx=*(0218b5b0()+0x3734)`) through `021bbbec`. `EventTask_Update` (`ov_017::021bc77c`): when the script ends and `ctx+0x11a != 0`, it copies this identifier into the current event (`+8`), arms `+0xfd`, re-initialises the VM (`021bb0c4`) and returns "not finished": **the context (cast, camera) is kept and another script is chained**. 0 = no chaining. Identifier not validated. Established. |
| 540 | — | `int key_a, int key_b, [int p=1], [int q=1]` | `0215fa40`, . Walks a type-2 list of context `02012ff4()+0x6c`. Filters on bytes `+0x2c/+0x2d` and a bit of `+0x2e`, modifies the flags then calls `02018310(...,p,q,0)`; then sets bit `0x40` of an associated structure. Nature of the entries and visible effect unknown. `key_a=0` in all observed calls. |
| 541 | — | `int actor, int kind, [int p=0], [int q=0]` | `0215fe50`, . Reads the triplet `actor+0x74..0x7c` then calls `0219b43c(context,kind,&triplet,p,q)`. `kind` is 15 or 16 in the scripts. **[INFERENCE]** Triggers an effect positioned on the actor; exact nature, parameters and units unknown. The last two arguments are read as integers, even though some scripts supply floats. |
| 542 | — | none | `0215fef4`, . Writes zero into `context+0x3630`, context obtained through `0218b5b0`. Meaning of the field unknown; precise link with 541 not demonstrated. |
| 543 | `get_actor_position` | `int actor, &float x, [&float y], [&float z]` | `0215ff10` exports `actor+0x74/+0x78/+0x7c`, divided by 4096. XYZ position confirmed by the writes of 206 and the executor `0215929c`; physical unit not established. |
| 544 | `get_actor_rotation` | `int actor, &float rx, &float ry, [&float rz]` observed | `0215ffc0` exports `actor+0x80/+0x84/+0x88`, divided by 4096. Rotation confirmed by 208 and `021594ac`, angles in radians; Euler order not established. One observed argument is classed as a variable rather than an address. |
| 545 | — | `int slot, float value`; two calls observed with only `slot` | `02160070`, . Types 2/6: sets bit 8 of `object+0x5c` via `0203ced4`, then passes `value` to `0203fc6c`. Types 0/1/4/5: calls `02036e44(object,"run",0)`. Polymorphic behaviour established, but exact slot/type/object correspondence still open at the interruption. Do not reduce to "set animation speed": the character branch does not pass `value`. |
| 546 | — | `int slot`; 14 calls with an ignored second argument | `0216013c`, . Types 2/6: clears the same bit 8 and passes **1.0f** to `0203fc6c`. Types 0/1/4/5: plays `"stand"` via `02036e44`. Contrary to the initial statement, **this handler does not read a second float**. Do not name it "stop movement": no stop of movement is demonstrated. |
| 547 | — | `int slot, [int value=0xffff]` | `02163ccc`, . Positive slot: requires type 1, retrieves the object, calls `02037408` and sets mask `0x40000000`. Then links the object to a structure via `021b621c`, writes `value` at `+0xfe`, calls `02046a08` and resets two state fields. Nature of the structure/link unknown. The handler also handles zero arguments, not observed in the statistics. |
| 548 | `set_lighting_index_override` | `int mode`; always 4 observed | `02160338`. If `02012ff4()+0x834` is zero, writes `mode` into `LightingManager+0x90` (`lightingIndexOverride_`: if non-zero, `LightingManager` reads the lighting colour arrays at this index, USA source `LightingManager.cpp:397`). If `+0x410==1`, re-applies to list `+0x41c` (`021601f8`) and sets `+0x834=1` (done once per zone). |
| 549 | `clear_lighting_index_override` | none | `021603b0`: `LightingManager+0x90 = 0`. Reads its argument (a single call to `StbArg_Int`) and overwrites it immediately. |

## Event and objects (550–575)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 550 | — | `int i` (always 0), `int b` (0/1) | For the entries `Chara_Get(12*i+0x1c)` and `Chara_Get(12*i+0x1d)` that are present and whose signed halfword `+2 ≥ 0`: `b=0` sets bit 1 at `+0x6c`, otherwise clears it. **[INFERENCE]** visibility of two accessories; "weapon/shield" identification not established. `021605d4`; , `q2:109–120` |
| 551 | — | `int id`; real call with **−2** | Converts the negative identifier, loads `data/map/%s.chr` from the event's map field, builds a `0xac`-byte object and registers it in the `Chara_Get` table. Exact nature of this object not established. `0216066c`; , `q3:134–163`; `inevent.txt:0xff8` |
| 552 | — | `int source`, `str s1`, `str s2`, `str s3`, `int target` | **`02160768`**: resolves `source` and `target` in `Chara_Get`, allocates `0x268` bytes, stores the current camera in `ctx+4` (like 572), copies `s1`/`s2`/`s3`, attaches `source` then `target`, enables bits 1 and 0x20 of `source` and installs it at `GameState+0x3b0`. **Update `0204a170`**: three bones of `source` tracked (`TrackBone`, `0x38` matrices at `0x02107874/ac/e4`); eye = `source` × bone `s1`, aim = `source` × bone `s2`; the **target** is placed on bone `s3` (`Vector3` at `+0x44`) and, every three updates, turned towards its movement (`(rotation + atan2(dx,dz) + 1) >> 1`), unless byte `+0x265` (native 839) forbids it. Observed strings: `eye`, `lookat`, `foot`/`pos`/`eff`. **Executed** (`Camera::ThreeBoneCamera`, `EventPlayer`): eye/aim by bones and target moved; byte `+0x265` (native 839) is not driven. |
| 553 | — | `&int a`; second output `&float b` present in one of the two calls | From the structure `E+0xa0`, writes the signed byte `+0x14` and the float word `+0x18`, unless its first byte is negative. **Anomaly: the handler accesses both arguments without testing their count**, while one call supplies only one. Meaning of the values unknown. `0216089c`;  ; `inevent.txt:0x1064, 0x1630` |
| 554 | — | `int v`; constants 0, 1, 2 or variable | Writes `v` directly into **message window `+0x99c`**. Called after 400 in the dialogue wrappers. It is not a wait; nature of the setting unknown. `021608e8`; , `q2:300–304`; `ev03030.txt:25–56` |
| 556 | — | `int slot` (always 0), `int b` (0/1) | Accepts binding types 0, 4 and 5; passes the associated identifier and `b != 0` to `ov_017::021917f0`. This helper changes the attachment and transformations of object `12*id+0x1c` according to two configurations. **[INFERENCE]** position of a weapon, possibly drawn/sheathed; not confirmed. `02160938`; , `q2:212–299` |
| 557 | — | None | Calls `ov_017::021bd5d0`, retrieves the object of the first member of the party list, then calls `020397dc(object,1)`, which notably resets its bytes `+0x252/+0x253` to zero. Meaning of the overall operation unknown. `021609a0`; , `q3:164–226` |
| 558 | `get_message_answer` | `&int result` | Copies `MsgWin_Answer` (`020457f0`), word `W+0x954`. Handler `021609c0`. Documented YESNO path: 0=yes, 1=no; variants not established (evidence above). |
| 559 | — | `int b` (low byte) | Writes the low byte of the argument to field `+0x490` of the singleton returned by `0205ec44`. Trigger condition `0x1f` rereads it signed (`Raw::Quest490`); `ev21595` writes 1 before its end trigger. `ov_001::021609e4`; `q2:164–168`. |
| 560 | — | `&int result` | Writes 1 if `0202b7e8(singleton)==0` **or** `0202c1b4(singleton)==0`, otherwise 0. The first helper tests the singleton's first word; the second reads a byte indexed from `+0x1038`. Subsystem and meaning of the predicate unknown. `02160a00`; , `q2:169–185` |
| 561 | — | `int a`, `int b`; `a` usually 0 | Passes both integers to `ov_017::0219b578`, which combines them with fields `+0x1c/+0x20` of object `context+0x3630`, then calls `0203a47c`. **[INFERENCE]** two-component movement/offset; nature of the object not established. `02160a48`; , `q2:186–211` |
| 562 | — | Four `&int` observed | Exports the bytes of `E` in the order **`+0xcd`, `+0xcf`, `+0xce`**, then an indicator derived from `+0xcc`: 2→0, 1→1. Meaning of the fields unknown; other values of `+0xcc` undetermined in the decompilation. All observed calls are in `evluida`. `02160a7c`;  ; `evluida.txt:0x1144, 0x36c0, 0x4140` |
| 563 | — | `int a` (always 0), `int b`; third integer read if supplied, none observed | Walks a list of singleton `02012ff4`, selects entries by bytes `+0x2c/+0x2d` and a flag of `+0x2e`, modifies their flags then calls `0201875c`. Role of the list and final effect unknown. `02160b08` |
| 566 | `declare_cast_member` | **2, 3 or 4 arguments**, depending on the mode | Adds a `0x38`-byte descriptor to list **`E+0xd0`** (link `+0x34`); returns 0 if `0xa0 + counter > 0xbf`, the counter being the five high bits of `E+0xf8`. `byte0 & 0xf` = mode, `byte1` = second argument (attachment identifier shared with 567). Modes 4, 6, 7 and 8 reserve a `Chara_Get` slot (`0xa0+counter`) and increment this counter; mode 5 copies a reference without incrementing it. Modes 0, 1, 9–13: **already existing** characters (identifiers read or fixed 3/2/1 for 11/12/13); 2, 6, 7: file (`data/…`, `.mon` for 6); 3: `data/ani/…`; 8: fixed resource. **Established consumption**: scene construction `ov_001::021539d8` walks the list, loads the resources and creates the objects, then applies the archives declared by 567: **deferred declaration**, no model is loaded during the call. Fine semantics of each mode **[unknown]**. `02160da8` |
| 567 | `declare_character_archive` | `str path`, `int n`; third integer 1 in three calls | Adds to **`E+0xd4`** (link `+0x28`) a `0x30`-byte descriptor: `"data/"+path`, `n` (byte), load task `-1`, state 0. **Established consumption**: `021539d8` loads these files through the `BackgroundLoader`, then, for each character declared by 566 whose `byte1 == n`, calls `Object3D::LoadFromCHRArchive` or `LoadFromCCHROrCMOTArchive` (depending on bit 0 of the resource's first word) with tag **3**: same calls as 229. Third argument not read. `021612c8` |
| 568 | `add_scene_suppress_flags` | One `int mask` observed; variadic handler | `E+0xf8 = (E+0xf8 & 0xf8000000) \| (E+0xf8 & 0x7ffffff) \| (arg & 0x7ffffff)` for each argument: OR of the **27 low bits**, five high bits preserved (566 counter). **Established consumption**: `021539d8` (construction) reads bits 0x1, 0x2, 0x8 → `SetFlags04(0x8/0x10/0x400)` (suspends these subsystems), 0x4 → `EnableFlag(1)` (hides) objects 0–3 outside the cast, 0x20 → bit 2 of `+0x98` of the placement manager; `ov_001::02154974` (teardown) does the reverse (`ClearFlags04`, `DisableFlag`). Bit 0x10: read to condition the group of 566 mode 0/1 and `0202c508`. Gameplay meaning of the subsystems **[unknown]**. `02161374` |
| 569 | — | `str bin_path` | Records `"data/"+path` in buffer **`E+0xd8`**. The observed paths are event `.bin` files. Content and later use unknown. `021613d0` |
| 570 | `set_slot_visible` | `int slot`, `int b` (0/1) | `b=0`: **hides**, `b≠0`: **shows**. Evidence: handler `02161408` sets mask `1` of `Object3D::flags_` (+0x6c) with `EnableFlag` when `b==0`, clears it (`DisableFlag`) otherwise; this mask is `OBJECT3D_FLAG_HIDDEN` ("if set, no drawing", USA source `Object3D.cpp`). Type 2: toggles via `020406b0`/`02040668` (placement); type 6: nested object `+0x18` and placement; types 0/1/4/5: direct object; type 3: no effect; null object: return 0. Usage: `570(s,0)` before repositioning (206), `570(s,1)` after (`ev01130`). Natives 201 and 570 therefore act on the same bit (by character identifier or by binding slot). |
| 571 | — | `int slot`, `int b` (0/1) | Same slot selection as 570, but controls **`0x10` in character field `+0x6c`** and/or **`0x40` in the first word of the associated object**: set if `b=0`, cleared otherwise. Functional meaning unknown; do not confuse with 570. `02161524`; , `q2:59–70`. **USA source: not found** in the clone. **Wiki**: no mention. |
| 572 | `install_bone_camera` | `int slot`, `str s1` (`"eye"`), `str s2` (`"lookat"`) | Requires a bound slot of type 1. Allocates the `0x268`-byte object (`0204a120`), **stores the current camera** in `ctx+4` (read by 531), attaches the character to it (`0204a3f0`), copies the names of the two bones (`0204a440/0204a464`), sets bits `1` and `0x20` of the character, then installs it as `GameState::unknown_3b0_`. Its update (`0204a180`) copies the character's position/scale and tracks bone matrices (`MaybeUpdateBonePositions`). Established: **camera driven by two named bones**; the precise role of each bone (eye/target) follows from the names. `02161650` |
| 573 | — | `int slot`; second integer 0 in eight calls | For binding types 2 or 6, finds the associated object, performs its cleanup, resets its internal pointers and removes its entry from the 32-object registry. The second argument is not read. Exact memory release not established. `02161754`; , `q3:56–116` |
| 574 | — | `int group` (always 0), `int id`, `int b` (0/1) | Looks up a record via `02019518(group,id)`. In its halfword `+2`, sets `0x4` if `b=0`, clears it otherwise, and always sets `0x40`. **This is not the slot table of 570/571.** Nature of the records and meaning of the bits unknown. `02163dec`; , `q2:121–163` |
| 575 | — | `int group` (always 0), `int id`, three numbers converted to floats | Same lookup as 574; converts the three values after scaling and writes the words at offsets **`+8`, `+0xc`, `+0x10`**. **[INFERENCE]** three-component position; nature of the target not established. `02163e50`;  ; constant `02163ef0` recovered in the transcript |

## Cutscene and flags (576–603)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 576 | — | `int a, int b, float v`; 3 calls add two ignored floats | **`02163ef4`**: looks up a record with `02019518(map,a,b)`, writes `s16(record+6)=trunc(v×C)`. **[INFERENCE]** rotation of a map node; unit and constant not recovered. |
| 577 | — | `int a, int b, float x, float y, float z` | **`02163f60`**: same lookup; writes the three converted values at `+0x14,+0x18,+0x1c`. **[INFERENCE]** XYZ scale, consistent with the often identical values; meaning of the fields and constant not demonstrated. |
| 578 | `set_light_intensity` | `float intensity, int duration=0` | **`02164004`** → `02052894`: sets/interpolates a lighting factor; duration multiplied by `s16(02010218(save))`. The transcript of `02051d38` shows this factor applied to the RGB555 components. Float→integer constant not recovered. |
| 579 | `set_day_timer_stopped` | `int stop` observed at 0/1 | **`02164080`** → `GameState::SetDayTimerRunning(stop==0)`: **0 starts** the day timer, non-zero stops it (`AdvanceDayTimer` no longer advances `dayTimer_`, USA source `GameTime.cpp`). |
| 580 | `move_camera_fov` | `float value, int count`; first argument encoded as integer in the scripts | **`021640b4`** → `0202f0d4(cam, value×4096, count×33)`: target at `+0x1f0`, duration at `+0x1f4`; if the duration is zero, applies immediately through the setter of 532 (`0202e9b4`). Same unit as 532 (degrees). The factor 33 is the engine's time unit (see §Time). |
| 581 | — | `int enabled` | **`0216184c`**: on the object of `0203cf5c`, sets mask `0x2` of `+0x98` if the argument is zero, clears it otherwise. Function of the mask unknown. |
| 582 | — | none | **`02161880`**: clears this mask `0x2`, then clears `0x10000` in the first word of entries 0–31 returned by `0203dcf4`. Exact nature of the manager unknown. |
| 583 | — | `int value` observed at 4/8/12 | **`021618d0`** → `020115c4`: writes the low byte at `save+0x63d6`. Semantics unknown. |
| 584 | — | `int type, int number, int sound=1, int option=1`; 2 or 3 arguments observed | **`0215fb44`**: walks the type-2 door zones (`02012ff4()+0x6c`) and keeps the one whose `+0x2c == type` and `+0x2d == number`; adds `0x280` to the logical flags (`u16(+0x2e)>>4`), calls `02018310` then sets `0x40` in halfword `node+2`. **`02018310`**: if bit `0x20` is absent (swinging door), the angle target is `placement angle ± 6434` (`0x1922` = 90° in units of 4096) and the speed `409` per update (`+0x34`, `+0x36`), the direction coming from bit 2 of the flags; plays `g_DoorSeTable[flags & 0xf]` (200–204) if `sound`; the node is the **placement** `(block entry = type, identifier = number)` (`02018b44` compares `+0x2c` with the block's first word and `+0x2d` with the placement identifier). **Executed** (`Event::OpenDoor`): rotation of the placement (90° over 16 updates, visible animation); the sound and the other branches (sliding door, reverse direction) remain reported/not modelled. |
| 585 | — | same arguments/defaults; 2 or 3 observed | **`0215fc48`**: same mechanism, adds `0x300` to the logical flags. **[INFERENCE]** other door-opening variant; exact distinction not established. |
| 586 | — | same arguments/defaults; single call `(0,2,0)` | **`0215fd4c`**: same mechanism, adds `0x380` to the logical flags. Same caveat on the semantics of the mode. |
| 587 | `free_allocator_front` | `int index`, always 1 | **`02161900`**: `table[index]` (`021658b8`), if the entry exists: `HMRFAllocator::Free(*entry, 1)` = `FreeFront` (rewinds the start pointer: frees all "front" allocations and saved states, USA source `HMRFAllocator.cpp`). Index and allocator not verified. Allocator 0 is the event system's: the actual scripted index **[unknown]**. |
| 588 | `set_scene_time_of_day` | `int time` observed at 0–3 | **`021603d8`**: sets `lighting+0x98`, selects the associated profile at `+0x94`, prepares/reapplies the lighting and marks map states. **Does not go through `Save_SetTimeOfDay`**: do not present this call as a write of the saved time. |
| 589 | `sync_lighting_to_time_of_day` | none | **`02160504`**: reads `GameState::GetTimeOfDay` (0–3), writes it into `LightingManager+0x98` (`timeOfDayIndex_`), sets the lighting clock `+0x94` to the start of this phase (phase table), then reapplies to the zone's list if `+0x410==1`. Only once per zone (`+0x834`). Differs from 588: 588 imposes a given phase without touching the saved time. |
| 591 | — | `int enabled` | **`02161988`**: sets bit 0 of `map+0x105` if the argument is zero, clears it otherwise. Function of the bit unknown. |
| 595 | `is_event_skipped` | `&result` | **`02161a90`**: copies `event+0x15c`. Strong script evidence: `ev04051`, call `0x192c`, branch on non-zero result at `0x1aac`, then Japanese trace **"イベントをスキップした"**, "event skipped". Exact values other than zero/non-zero not established. |
| 596 | `get_actor_status` | `int slot, &kind, &state`; 12 calls have only `slot,&kind`. A fourth output is provided by the handler, never observed | **`02161ab8`**: searches the event's actor declarations, returns category and state; . Observed slots: **12,13,14**, each 204 times. Do not reduce the function to an "NPC present" boolean. |
| 597 | `get_scene_time_of_day` | `&time` | **`02161ce8`**: copies `lighting+0x98`, field written by 588 and 589. **Scene time**, not a direct read of the saved field. |
| 598 | `get_party_leader_id` | `&character_id` | **`02161d08`** → `020100c0`: reads `u8(save+0x397c)`, first ID of the party list. Consistent with `Party_List`, `re/symbols.txt:221`. |
| 599 | — | `int value` observed at 0/1 | **`02161d2c`**: writes `map+0x274c`. Function of the field unknown. |
| 600 | `test_global_flag` | `int index, &result` | **`021632e4`**: `Flags_Test(manager,manager+0x8c,index)`, result normalised to 0/1. |
| 602 | `test_step_flag` | `int index, &result` | **`02163390`**: tests **set 1 of the current track**, base `manager + current_track×0x1c + 0x10`, result 0/1. Set 1 is cleared on stage change: `docs/re/trigger.md:32-56`. **Not a "per-player" flag.** |
| 603 | `test_story_flag` | `int index, &result` | **`021633ec`**: `Flags_TestExt(manager,index)`, result 0/1. Reuse the existing remapping of `Flags_TestExt`, not a naive direct test. |

## Sound (700–738)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 706 | — | `int, int` | `021634f8`: returns 1 immediately, ignores the arguments. Observed pairs: `(232,232)`, `(241,241)`, `(233,233)`, `(236,236)`. Former intent unknown; **no effect in this binary**. |
| 707 | — | none | `02163500`: returns 1 immediately. Former intent unknown. |
| 708 | — | `int`: 0 or 4 | `02163508`: returns 1 immediately, ignores the argument. Former intent unknown. |
| 709 | — | `int`: 2 or 5 | `02163510`: returns 1 immediately, ignores the argument. Former intent unknown. |
| 712 | — | `int index[, &out_handle]` | `02163564`: without output, `0205eaa0(manager, index, 0)` (base archive `+0xb4`). With output: `0215a818` looks for a free handle then `0205eb90` (archive `+0xc4`, like 728), and the number is written through the address. **Two behaviours** depending on arity: no single name. |
| 713 | `select_bgm` | `int sequence_id`; handler also accepts `(int ignored, int sequence_id)` | `02163640` → `0209c3c4(id)`, then `0209c688(0)`. Selects/loads a BGM sequence then stops it immediately; 714 lets it restart. Do not simply call this native "play the music". Negative value refused. The two-argument form is not observed. |
| 714 | `restart_bgm` | none | `021636a8` → `0209c5f8`, then `0209c2f0(127,0)`. Restarts the selected sequence and resets the requested volume to 127. Internal guards can prevent the restart; an engine condition instead causes a stop. Resuming at the previous position **not established**. |
| 715 | `set_bgm_volume` | `int volume[, int duration=0]` | `021636d0`: clamps the volume to 0–127, then `0209c2f0`. The latter adjusts the BGM handles and the volumes of players 5, 7 and 6. Exact duration unit not established in the recovered files. Two calls use 30. |
| 720 | — | `int sequence_id` | `02163744` → `0209c840`: writes the ID at `manager+0xce`, sets `+0xc9=1`. Deferred request for a music sequence; 725 queries these fields. "Jingle" naming plausible, but the request's consumer not decompiled in this session. |
| 721 | `stop_bgm` | `[int duration=30]`; all observed calls pass an argument | `0216393c` → `0209c688` → `0203ac20` → `020bc028`: immediate or faded stop of the active BGM handle. Duration unit not established; stop subject to the lock at `+0xc8`, bit 2. |
| 723 | `stop_sound_handle` | `int handle[, int duration=0]` | `021635d0`: requires an index 0–15 and a non-null cell; calls `0203acd8`, then `020bc088` to clear the cell. Kinship with the handles produced by 712 supported; equality of all array bases not verified by the overlay literals. |
| 724 | — | none | `02163764`: returns 1 immediately. No effect; historical intent unknown. |
| 725 | — | `&out_bool` | `0216376c` → `0209ca3c`: writes 0 if `+0xce<0`, 1 if a request is pending (`+0xc9!=0`), otherwise tests whether a sequence with this ID is still active via `020bc0b4`. **Not only "currently playing": includes the pending state.** "Jingle" name not retained. |
| 726 | `load_se_group` | `int group_id[, int sequence_archive_id=group_id]` | `021637ec` → `0205eb64`: restores the heap to level `+0xc0`, loads the group, writes the selected archive at `+0xc4`. All observed calls have one argument. Values 205–367 are **not BGM track indices played directly**. Individual mapping of the SDAT entries not extracted. |
| 727 | `restore_se_heap` | none | `02163848` → `0205eb90` → `0203aa90(manager, manager[+0xc0])`. Return to the heap level preceding the load performed by 726. Does not explicitly clear `+0xc4` in the recovered body. Do not artificially add this reset. |
| 728 | `play_se` | `int sequence_index[, &out_handle]` | `02163860` → `0205eba0` → `0203ac50` → `020be7b8`. Plays an index of archive `+0xc4`; without output uses the internal handle, otherwise looks for a free cell among 16 and writes its index. Some second arguments are not addresses according to the statistics: 3 variables and the constant 20. Handle exhaustion: local output potentially uninitialised in the decompilation; no fallback behaviour established. |
| 729 | `stop_se` | `int handle[, int duration=0]`; two arguments everywhere in the scripts | `021638cc`: validates 0–15 and non-null cell; `0205ebc4` → `0203acd8`, then frees the cell through `020bc088`. Cell freed immediately even if a fade is requested. |
| 730 | `load_secondary_se_group` | `int group_id` | `02163970` → `0205eae0(id,id)`: restores the heap to level `+0xb8`, loads the group, writes the archive at `+0xbc`, then records the new state at `+0xc0`. Sound role established, but functional name of this second resource level unknown. Do not invent "ambience" or "music". |
| 731 | `restore_secondary_se_heap` | none | `02163994` → `0205eb18`: restores level `+0xb8` and copies this state into `+0xc0`. Counterpart of 730 (like 727 of 726). Does not explicitly reset the archive IDs to −1. |
| 732 | `play_secondary_se` | `int sequence_index, &out_handle`; handler also accepts a single argument | `021639ac` → `0205eb34` → `0203ac50`. Plays from archive `+0xbc`, selected by 730. Same 16-handle mechanism as 728. Semantic name of this level unknown. Observed indices: 0, except one call with 1. Same caveat on handle exhaustion. |
| 733 | — | `int handle[, int ignored]` | `02163a18`: only validates 0–15, then `0205eb58` → `0203acd8(..., duration=0)`. Stops the handle immediately. **Does not read the second argument and does not clear the cell.** Three calls pass 364, so fail validation: do not reinterpret them as group loads. |
| 734 | — | `int mode` | `02163a5c`: mode 0 resets BGM with heap `0x57000`, then opens the normal SEs with ID 100; non-zero mode closes the SEs and resets BGM with heap `0xa2000`. "Audio memory mode" usage established; precise scenario of each mode unknown. Both BGM configurations use the same file. |
| 735 | — | `int enable`: 0 or 1 | `02163ab4`: bit 2 of `manager+0xc8` is `(enable==0)`. This bit notably inhibits BGM selection, restart and stop in the recovered functions. Do not call it "mute": it blocks operations, not directly the volume. Reach over all music paths not established. |
| 736 | — | none | `02163aec`: obtains an ID from `02012ff4`, transforms it through `0209cd60`, then calls `0209c3c4`. Selection of the current location's BGM, with possible conditional replacement. The "location/room" meaning is **[INFERENCE]** from the tables and the access chain. Does not explicitly make 714's restart call. |
| 737 | `play_music_sequence` | `int sequence_id`: 67 or 55 | `021637a8` → reset `0209c21c`, then `0209c6e8`: loads and starts the sequence on handle `manager+0xc4`, stores its ID at `+0xce`. Use as a "jingle" likely but not established by identifying the tracks; deliberately generic name. |
| 738 | `reset_bgm` | none | `021637d4` → `0209c21c`: stops the relevant audio paths, restores the initial heap, resets the state to 0, the main IDs to −1, detaches the handles and restores the volume to 127. Broader than a simple stop; does not itself destroy the heap like `0209c288`. |

## Misc (800–845)

| ID | Descriptive name | Arguments | Established effect, evidence and limits |
|---|---|---|---|
| 800 | — | `&out` | Writes the inverse of bit 0 at `+0x49c` of a structure obtained via `0200fddc`, then `02053c7c`. Identity and meaning of the bit unknown. `02161d54` |
| 801 | — | `&out` | Writes the result of `0202b7e8` after `0202ae28`: test of the first word being non-zero. Subsystem not identified. `02161d8c` |
| 802 | — | `int id`: 1 or 3 | Walks the chain `E+0xd0`, link `+0x34`; looks for byte `+1 == id`, then sets bit 0 of byte `+4`. Nature of the elements unknown. `02161db0` |
| 803 | — | `int enable`, two numerics decoded as floats; calls observed with 1 or 3 arguments | Modifies the structure `*(G+0x3b0)`: byte `+0x1f8`, then parameters `+0x200/+0x204` if enabled; the last one goes through multiplication and integer conversion. Effect unknown. **The decompiled code reads all three arguments even to disable**, despite a single-argument call. `02161e00` |
| 804 | `set_fog_enabled` | `int enable`: 0/1 | `02161e8c`: `LightingManager+0x85 = enable`, then `020c54a4(enable, type, depthShift, offset)` with the parameters kept at `+0x4c` (zeros if disabled). It is the same call as `LightingManager.cpp:818` of the USA decompilation, which passes it `fog->type, depthShift, offset`: **3D fog established**. |
| 805 | — | `int value`: 0/1 | Assigns `R+0x2750`. Meaning unknown. `02161f3c` |
| 806 | — | none | Sets byte `R+0x264d` to 1. Effect unknown; used in `ev20741`, `ev20931`. `02161f64` |
| 807 | — | `int room`, three position floats, one orientation float, `[int entry = -1]`; 5 or 6 arguments | Prepares a teleport destination in `Save_Warp()`: room `+0`, position `+0x10`, orientation `+0x1c`, entry `+0x20`, then calls `0200fd0c`. Coordinates and orientation are multiplied then converted to integers. **Units and scale constant not recovered.** `02161f80`,  `Save_Warp` established in `re/symbols.txt:L218` |
| 808 | `set_time_of_day` | `int value`: 0, 1, 2 | Calls `Save_SetTimeOfDay(G, value)` directly. The individual meanings of the values are not established here. `02162148`,  engine `02010374` |
| 809 | — | `int enable`: always 1; `[int mask = 0]` decoded | Selects components `F+0x4334/+0x4338` according to the mask; the helpers set or clear bit 0 of a field `+0x6c`. `enable != 0` calls the clearing helper. Nature of the components unknown; pointer passing incomplete in the decompiled code. `02162174` |
| 810 | — | none | If `u16(E+0x11c) != 0`, calls the setter of `E+0x11a`, then sets `E+0x11c` to zero. **Value passed to the first setter not recovered.** `02162210` |
| 811 | — | none | Pushes mode 5 via `020a1f00`, sets `F.flags0 & 0x1000`, then calls `021d96bc`. Used in `ev29350`. **[INFERENCE] Start of the credits timing**, related to 812/838; overlay not confirmed. `0216224c` |
| 812 | — | none | Clears `F.flags0 & 0x1000`, calls `021d9714`, then pops a mode via `020a1f5c(1)`. Used in `ev29373`. Counterpart of 811; timing meaning still inferred. `0216227c` |
| 815 | — | `&out`, `[int mode]`: 1 or 2 | Initially writes 1; writes 0 if the mode is absent or different from 2. In mode 2, may write 0 if three results match the expected constants and a global counter exceeds 5. Wraps the calls with `020a1950/020a1bc4`. **Meaning of the checks and overlay of the three calls not established.** `02162338` |
| 816 | — | none | Calls `ov_017::0219ff58(F, 0, 1, 0, …)`. All uses in `ev29450`. Effect unknown; last parameter decompiled as an incoming register. `02162468` |
| 819 | — | none | If bit 0 of `G+0x63dc` is set, calls `ov_017::0219c520`, which prepares object `F+0x3b4c` and passes it to `020469c4`. Meaning of the bit and of the operation unknown. `021624a8` |
| 820 | — | `string path` to `chara_sub/*.pac` | Looks up or loads the PAC, extracts its resources and configures their graphics transfer. Credits files: `horii`, `toriyama`, `sugiyama`, `hino`, `fujisawa`, `ichimura`, `tobe_<LG>`. **Exact display and helper names not established.** `021624cc`,  `ev29350`, instructions `0x143c`, `0x1eb4`… |
| 821 | — | `int selector`: always 0; branch 1 present | Saves a graphics state: value returned by `020c4b48/020c4ba8`, four register halfwords and bits `DISPCNT[12:8]`. Matches 822. `0216297c` |
| 822 | — | `int selector`: always 0; branch 1 present | Restores the graphics state saved by 821, with the corresponding helpers and register masks. **Do not generalise to a full save of the whole display.** `02162a90` |
| 823 | — | `&out` | Tests bit 0 of `G+0x63dc`. If set, writes 1; **otherwise does not modify the output**. It is therefore not a total boolean getter. `021626a4` |
| 824 | — | `string path`, `int selector`: `"chara_sub/company.pac"`, 0 | Variant of 820 with selection of a graphics configuration; alternative branch if the selector is non-zero. Precise meaning of the selector unknown. `021626e4` |
| 825 | — | `&out` | Writes exactly `Save_Chapter() > 18`. **The "postgame" naming was not established by the session.** `02162d3c`,  engine `020107ac` |
| 826 | — | `int selector`: always 0 | Prepares two zeroed buffers of `0x20` and `0x600` bytes and calls the graphics routines matching the selector. **[INFERENCE] Clearing of graphics data**, but destinations not established. `02162900` |
| 827 | — | `&out` | Obtains a structure through `0200fe00(G,0)`, then applies the same bit test as 800 after `02053c7c`. If the structure is missing: returns 0 **without writing the output**. Identity of the target unknown. `02162d68` |
| 828 | — | two integers: always `(0,6)`; `[int mode = 0]`, explicitly 1 in six calls | Modifies data associated with a character and index 6, keeps values in `E+0x180/+0x182`, clears bit `F.flags0 & 0x10`, then calls `0218f5a4`. **[INFERENCE] Equipment modification/restoration**, without demonstrated identification of the slot or the mode. `02162db0` |
| 829 | — | `&out` | If `F.flags0 & 0x10` is set: writes 0. Otherwise compares the result of `02046b34(*(F+0x3700))` with `0x13`, writes this equality and sets bit `0x10` again if different. Same set of seven scripts as 828. **"Finished/busy" meaning not established.** `02162f8c` |
| 830 | — | none | Allocates 8 bytes and `0x8000` bytes, stores the pointers at global offsets `+0x4b4/+0x4b8`, initialises the second buffer and sets a flag via `Flags_Set`. Purpose and flag ID unknown. `02163008` |
| 831 | — | `&out` | Uses the buffers of 830 with `020aad2c(...,10,0)`; writes `result == 0`, modifies mask `0x80` via `020466f4/02046704`. **No write if the initial pointer is null.** Possible graphics capture, not established. `02163070` |
| 832 | — | `int enable`: 1 then 0 | Assigns byte `E+0xfe` to `enable != 0`. Effect unknown; used in `ev29320`. `021630f4` |
| 833 | — | `int enable`: 0/1 | Argument 0: sets bit `0x800` of the **second** flag word of `F`; non-zero argument: clears it. Meaning unknown. `02163128` |
| 834 | — | `&out` | Writes `u16(E+0x11c) != 0`. Associated with 810 in scripts `ev29500`, `29503`, `29506`, `29509`, `29512`, `29515`, `29516`. Nature of this state unknown. `02163164` |
| 836 | — | `&out` | Reads byte `E+0x105`, resets it to zero and writes the old value. **Destructive read**, meaning unknown. `021631d0` |
| 837 | — | `&out` | Reads byte `E+0x107`, without reset. Meaning unknown. `02163200` |
| 838 | — | `&out` | Writes the result of `021d9748`. The credits scripts wait for this value to reach an absolute threshold. **[INFERENCE] Elapsed time, probably in milliseconds**; unit and definitive attachment to `ov_028` not proven. `02163228`,   ; `ev29373`, `fn@0x1294` |
| 839 | — | `int value`: 0 | If `*(G+0x3b0)` exists, assigns its byte `+0x265` to `value == 0`. Effect unknown. `02163248` |
| 840 | `get_tick_count` | `&out` | `02163284`: writes `GameState::GetTickCount()` = `numTicks_` (`GameState+0x3c4`), number of ticks elapsed since the previous update, capped at 3 (`CalculateDeltaTime`, USA source `GameTime.cpp`). **Measured: 2** in the five melonDS states (effective delta 33 ms, `+0x3b4=0x21`). In `ev21500`, the wait initialises a budget `2*n`, subtracts this value on each pass then `yield`: a wait of `n` therefore lasts `n` updates of ≈33 ms at nominal rate (see §Time). |
| 841 | — | `int enable`: 0/1 | Clears or sets bit `0x4000` of the first flag word of `F`. Meaning unknown. `021632a8` |
| 842 | — | none | Prepares a `Save_Warp()` destination from fields `R+0x23bb/+0x23c0/+0x23cc`; room computed `s8(R+0x23bb)-0x63c0`, position, orientation, byte `warp+0x68=1`, then `0200fd0c`. **[INFERENCE] Return to a stored destination**; context and room formula to be kept without normalisation. `021620bc` |
| 843 | — | two integers: `(0,28)`, `(0,30)`; third decoded if present, but no use observed | Walks a list of `R+0x6c`, filters on bytes `+0x2c/+0x2d` and a flag of `+0x2e`, modifies the flags then calls `02018928`. **Nature of the objects and final effect unknown.** `02160bc0` |
| 844 | — | none | Calls `02017c68`, then writes 1 at `*(F+0x3b5c)+8`. Effect unknown; `ev29220`, `ev29230`. `02163dc8` |
| 845 | — | `.chr` strings; 1, 2 or 4 arguments observed | `0215ed74` calls the preload helper `0215ec94` (like 506), without resetting the global counter `+0x88`. Loops from this counter up to `argc`, reading the argument at the same index, without offset. It is therefore not a general "add resources" primitive. Uses in `ev28820`. |
