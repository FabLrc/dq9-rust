# Running in a browser

## Hosted demo (GitHub Pages)

Open **https://fablrc.github.io/dq9-rust/?newgame=1**, choose your European ROM `YDQP01`:
the "New game" button becomes active as soon as the ROM is read, and the keyboard is given back to the game
without an extra click. Arrows: walk; Enter/Space: talk and advance messages;
Q/E: camera; M: sound. A DS save is optional.
Files are read locally: no ROM or save is sent to GitHub.
A computer with a keyboard and a recent browser with WebGPU or WebGL 2 are recommended.
Battles are interactive; standalone battle 26 has been played to victory in the browser
([Verification](verification.md)). Roaming monsters are visible and start fights; field-menu/facility
screens are still missing;
effects and battle motions are incomplete. The route mode and the pilot also run on the
hosted page (deployment `2e580a1`): a `battles=show` session there played the chapter 2 route
to event 2750 (§ "Route mode in the browser"). The page also records and replays a session
inside the browser (`?record=1`, `?play-record=1` or the "Play recording" field): § [Recording a human session](record.md).

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

## Build and local server

```powershell
cargo build -p dq9-app --release --target wasm32-unknown-unknown
# Once: the wasm-bindgen-cli version must match Cargo.lock.
cargo install wasm-bindgen-cli --version 0.2.130 --locked --root tools/wasm-bindgen
tools/wasm-bindgen/bin/wasm-bindgen --target web --no-typescript `
    --out-dir crates/dq9-app/web/pkg target/wasm32-unknown-unknown/release/dq9-app.wasm
python -m http.server 8000 -d crates/dq9-app/web     # or any static file server
```

Open `http://localhost:8000/` (parameters `map`, `time` (name or 0 to 3), `clock`, `layout`
(`enhanced` (default), `enhanced-reactive`, `stacked`, `side` or `top`), `fx`/`nofx` (the
enhanced effects to switch on/off, § [Enhanced 16:9](screens.md#enhanced-169)), `screens` (`port` or `original`: where the field
and its message window are drawn, § [DS screens](screens.md)), `chapter`, `step`,
`substep`, `night=1`, `mute=1`, `newgame=1`, `seed`, `record` and `play-record` (§ [Recording a human session](record.md)), and the pilot session's `pilot`, `fight`, `route`, `speed`, `budget`,
`render`, `battles`: § "Route mode in the browser"), then choose the ROM and, optionally, a `.sav` save in the
page. Both files are read locally in the browser, never sent or hosted;
an explicit `map` takes precedence over the save's map. The "New game" button (or `?newgame=1`)
launches the prologue (§ [New game](new-game.md)); it stays disabled until the ROM is read and
the keyboard returns to the canvas afterwards. The page reports errors in red and names the
rejected file.
WebGPU when the browser offers it, otherwise WebGL 2. `crates/dq9-app/web/pkg/` is generated
(ignored by git).

## Route mode in the browser (`pilot=1`)

The pilot and the route mode run in the page: the same `App::auto_step` the native
`--headless --auto` loop calls (`app/auto.rs`), driven one animation frame at a time by
`app/auto/session.rs`. The step stays the 60 Hz fixed step; nothing reads the wall clock for
the game, so a session is deterministic from the ROM, the seed and the options.

| Parameter | Effect |
|---|---|
| `newgame=1` | the new game starts when the ROM is read (else click "New game") |
| `pilot=1` | the autopilot plays the new game (`--pilot`); implied by `route` |
| `route=2750` | route mode (`--route 2750`): roaming on, grind, inn and herbs; stops when the event ends; implies `fight` |
| `fight=1` | the engine plays the battles that a native headless run settles (`--fight`); without it they are the J4 stand-in |
| `seed=7` | the world's generators (`--seed`) |
| `speed=N` | `N` fixed steps per animation frame; real time (60 steps/s) without it |
| `budget=40` | wall milliseconds after which a frame stops running steps (default 40; checked every 32 steps): it bounds a frame, never changes what the run does |
| `render=0` | no frame is drawn; the logic and the checkpoints run unchanged |
| `battles=show` | presents battles (transition, scene, windows; the confirm key is held, the policy of `Fight::play_out`) instead of settling them in the step that starts them, which is the native headless behaviour and the default of a session |
| `mute=1` | no sound device |

What the page offers: `#session` shows `session: <game seconds> (<steps>), map, story, level,
HP, gold, fights` every 10 s of game time, the console carries the engine's status lines,
and the run ends on `stopped: event 2750 ended (route reached) after … s of game time`
in both. `window.dq9_session` holds `line`, `tick`, `seconds`, `wall`, `event`, `attempts` (the
set battles with the level they were fought at), `checkpoints` (the JSONL lines of the native
`--checkpoints`, from the same `capture_checkpoint`), `done`, `stop` and `report` (the journal
and the final state, as `--headless --auto` prints it).

```powershell
# Builds the release bundle (as pages.yml does), serves it locally, drives Edge/Chrome over the
# DevTools protocol (web/session-driver.mjs, Node 22+, no npm package), chooses the ROM through
# the page's <input type=file> and records console.log, progress.log, checkpoints.jsonl,
# report.txt, shots/*.png and result.json under -Out:
powershell -File scripts/web-session.ps1 -Out build/evidence/j4-web-pilot/run-instant `
    -Query 'newgame=1&pilot=1&fight=1&route=2750&seed=7&mute=1&speed=60'
# The first differing checkpoint against a native run:
node web/compare-checkpoints.mjs native-checkpoints.jsonl build/evidence/j4-web-pilot/run-instant/checkpoints.jsonl
# The same session on a page served elsewhere (here the hosted Pages deployment): nothing is
# built or served locally, -Query becomes that page's query string.
powershell -File scripts/web-session.ps1 -Out build/evidence/j4-web-show/hosted-show `
    -Url https://fablrc.github.io/dq9-rust/ `
    -Query 'newgame=1&pilot=1&fight=1&route=2750&seed=7&mute=1&speed=60&battles=show'
```

Settling versus showing. A default session settles a battle inside the step that starts it, as
the native headless run does, so the browser's timeline is the native one tick for tick and the
checkpoints can be compared. `battles=show` plays the battles on screen: they take fixed steps
(transition 35, fight, leave 45), so the game's clock, the parity of its 30 Hz field step and
the pilot's key presses no longer line up with the native run: its checkpoints are compared by
state (story, flags, maps, events), never by tick (below). In both modes the route reads a
roamer's fight when it is settled (`route_observe` is skipped while a battle is shown).
A headless `App` can present its battles too: a session decides on its own
(`App::battles_instant`), so `set_session` with `show_battles: true` on `App::new_headless`
runs the Web's shown route natively, which is how the `battles=show` stop below was reproduced.

**Run on `FabLrc/j4-web-pilot`** (Edge 157, wasm release bundle, European ROM chosen through the
page's file input, `newgame=1&pilot=1&fight=1&route=2750&seed=7&mute=1&speed=60`, rendered, one
continuous session, 1859 s of wall time): `stopped: event 2750 ended (route reached) after 3057.2 s
of game time`, 183,429 fixed steps, story 2.6.5, Lv 8, 115 roamer fights won, 2 lost; the
Hexagoon (set battle 2) lost at Lv 4, 5, 6 and 7, won at Lv 8 in 7 rounds. **The 972 checkpoints
are byte-identical to the native acceptance run** (`cmp` on `accept-2750-checkpoints.jsonl`,
`a0d6200bb7b8`), and the report matches `accept-2750.out` line for line up to the native-only
"last fight" log. The wasm runtime therefore reproduces the native float and RNG sequence for
the whole route. The page made 3,000 to 5,000 fixed steps per second in scenes and 30 to 900
(about 100 on average) while the pilot probes a map's walkable grid. The only console error is
the browser's `favicon.ico` 404. Evidence: `build/evidence/j4-web-pilot/run-instant/` (`console.log`
`a566bf824b31`, `checkpoints.jsonl` `a0d6200bb7b8`, `report.txt` `f126e4558642`, `result.json`,
13 screenshots). The screenshots are taken by the driver at the first poll (1 s of wall time)
after the milestone, so they show the field some game seconds later (each carries its session
line in `result.json`); in this default mode a battle is settled in the step that starts it and
is never on screen.

`battles=show` (`run-show/`, same query plus `battles=show`, 1753 s of wall time) played the
battles on screen but stopped at 2489.2 s with `free control on map 1107 (story 2.4.5)`, Lv 4,
before any attempt on the Hexagoon. The pilot had entered `D01M05` (7105) at 2174 s and 2313 s
and walked back out 0.45 s later without talking to NPC 203, whose talk starts events 2535 and
22510 and the Hexagoon in the native run; it then circled the Angel Falls rooms, found nothing
left to try at 1107 and stopped (`Stop::Control`).

**Cause (`j4-show-battles`).** Reproduced natively with the same options (the headless session
above, on `ca074aa`): 817 checkpoints like the Web's, the last on map 1107, the same stop. The
shown and the instant runs have the same state sequence (maps, events, story, flags) for 715
checkpoints; they differ in the step counts (the first checkpoint, event 21507's end, is 110
steps later: the transition; the battle settles 1,386 steps after it) and, from checkpoint 715,
in a roamer fight: the instant pilot is wiped out in `D01M04` at about 1980 s, the shown one is
not and reaches 7105 (3 levels, hurt). A `DQ9_PILOT_DEBUG` line per `pursue` (not kept) showed
at that arrival `blocked false resume None`, no offer line, and `route_pursue` returning true
(`native-show2.err`, `369d27b8d2b3`): only `route_heal` can do that with no bar and no resume
room. The walk goes to the shop (herbs; no inn from 2.1 to 2.6): `route_visit` →
`route_travel` → `walk_back` → `start_exit`, which books `left` and `tried_exits` for every room
it leaves **at the story's digest**, with no memory of where the story was; the route mode
keeps that memory only after a fall (`Route::resume`, `route_observe`). After the herb (log: `talks
to the keeper of the Shop (NPC 19) ... HP [(3, 34)]`) the pilot walks the Angel Falls rooms
until `Stop::Control` and never goes back to `D01` (**[inference]** from the books: `bot.graph`
holds only the rooms of the current family, and `1100 → 20001` is already in `left` at this
digest, so `next_foreign_hop` finds nothing; the fix below removes the symptom, which is the
falsification that counts). The instant run never meets it: its party was whole on arrival at
7105, and its heals started with `resume` already set by a fall. Nothing in the game's state,
the battle engine or the presentation barriers was wrong; the defect is the pilot's.

**Fix.** `route_visit` sets `Route::resume` to the room the trip started from when it is not a
field and no resume room is set (`route.rs`); `route_resume` then walks the pilot back after the
herb and forgets that room's books, as it does after a fall. Regression tests:
`a_heal_trip_from_the_story_room_resumes_there_after_the_herb` (hurt party at 7105: the trip
sets the resume room, and at 7105 healed the pilot walks to NPC 203; it fails without the fix)
and `a_shown_battle_leaves_the_game_in_the_state_an_instant_one_does` (battle 26 shown and
settled leave the same party, story, flags and map).

**Result** (native, same ROM, `--new-game --fight --seed 7`, `build/evidence/j4-show-battles/`):
the instant run's 972 checkpoints are byte-identical before and after the fix (`135ddaa29dd9`:
the fix changes nothing where `resume` was already set, so the acceptance timeline stands: story
2.6.5, 182,129 steps). The shown run (`fixed2-show-checkpoints.jsonl`, `a7e4fa191432`) now plays
the Hexagoon (NPC 203 talked to four times against five, the set battle lost at Lv 5, 6 and 7,
won at Lv 8) and `stopped: event 2750 ended (route reached)` after 4,166.5 s of game time
(249,990 steps); its 971 checkpoints hold the same 21 story progress steps in the same order as
the instant run's 972, and the same events in the same order but for one fewer attempt at event
22510 (the pilot was stronger when it came: three lost attempts against four). A shown session
is therefore **not tick-comparable** with the instant one: the pilot's draws and the roamers'
fights differ from the first roamer contact on (**[hypothesis]**: the 110 steps of the first
transition move the field's 30 Hz parity and the roamer timers; unfalsified, since the world's
generators are shared by the battles and the roamers and the checkpoints do not carry them),
but it plays the same story.

**Hosted page (`j4-web-show`, November 17, 2026).** The same route was run once, with
`battles=show`, on the page GitHub Pages publishes (`scripts/web-session.ps1 -Url`,
`https://fablrc.github.io/dq9-rust/`): deployment `44132801037` of `2e580a1` (main's HEAD
`b11a38f` at the time is a documentation commit, which redeploys nothing), Edge
157.0.4341.60, the European ROM chosen through the page's file input, rendered, one
continuous session, 1026 s of wall time. It ended `stopped: event 2750 ended (route reached)
after 4166.5 s of game time`, 249,989 fixed steps, story 2.6.5, Lv 8, 116 roamer fights won
and 2 lost; the Hexagoon was lost at Lv 5, 6 and 7 (6, 4 and 5 rounds) and won at Lv 8 in 7
rounds (175 experience, 100 gold, HP 8/53 left), the same attempts as the native shown run. Its
971 checkpoints carry the same **state** as the native shown reference
(`build/evidence/j4-show-battles/fixed2-show-checkpoints.jsonl`, `a7e4fa191432`) at every one
of them: the same 21 story progress steps in the same order, the same events in the same
order, the same five set battles at the same levels, the same final flags, map, level and
gold. 915 lines are byte-identical; the other 56 are the `map` checkpoints of the walk that
follows each lost attempt (#776–841) and differ in `tick`/`vblank`/`seconds` alone, by +2
fixed steps, the next `event_start` realigning the clock: the shown battles' presentation
shifts the field by two steps locally, not the state. The driver captured a shown fight with
its message and HP/MP windows (`shots/02-battle-scene.png`, battle 26) and the Hexagoon with
its windows (`shots/09-hexagoon-scene-1.png`); the screenshots of the later attempts caught
the field, a frame before the scene was presented. The only console error is the browser's
`favicon.ico` 404. Evidence: `build/evidence/j4-web-show/hosted-show/` (`console.log`
`8d4c8d40b2bf`, `checkpoints.jsonl` `1e7f03b18fb6`, `report.txt` `f68b76e6377f`, 26
screenshots), the deployment and the served bundle in `build/evidence/j4-web-show/deploy.txt`
(`pkg/dq9-app_bg.wasm` `8962228cc9aa`).

The same page, same session, was then run in the default instant mode (1083 s of wall time):
`stopped: event 2750 ended (route reached) after 3035.5 s of game time`, 182,129 fixed steps,
story 2.6.5, Lv 8 (HP 36/53, MP 17/19, gold 648), 115 roamer fights won and 2 lost; the
Hexagoon lost at Lv 4, 5, 6 and 7 and won at Lv 8 — the native instant run's stop line and
story (`fixed-instant-checkpoints.jsonl`, `135ddaa29dd9`). Its 972 checkpoints carry the same
state as that run at every one of them: 904 are byte-identical and the 68 others are the `map`
checkpoints of the walks that follow a battle (#697–744, #777–796), where `tick`/`vblank`/
`seconds` differ by two fixed steps in either direction (0.03 s) and the next `event_start`
realigns the clock; the cause is **[unknown]**, the state those checkpoints carry does not
differ. Evidence: `build/evidence/j4-web-show/hosted-instant/` (`console.log` `6e2b7e6c6a7a`,
`checkpoints.jsonl` `8ce4af30e497`, `report.txt` `bfc5f168dfd3`, 13 screenshots); the only
console error is again the `favicon.ico` 404.
