# Verification

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
  five tables (details and limits: [`re/lighting.md`](../re/lighting.md) § "Comparison with melonDS",
  `scripts/melonds-capture.ps1`). DS lights, sky and advanced-zone fade not compared.
- Browser: wasm bundle rebuilt with `wasm-bindgen`, ROM chosen locally in
  the page; `M01` and `C01M09` display the map and NPCs, with no JavaScript error observed.
  Native screenshots of frames 0 and 60 of `M01` differ on 41,143 pixels with an identical
  camera. Rendering has not yet been compared frame by frame with the DS's.
