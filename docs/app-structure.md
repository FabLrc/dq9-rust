# Structure of `dq9-app`

How the application is split so that a feature owns a **module** and reaches the loop through a
small, named extension point, instead of editing one central file. Behaviour is unchanged by
the split (batch `k1-app-split`): the digest of the long route test, `parity.ps1` and a
headless screenshot are identical before and after. The behaviour itself is described in
[`app.md`](app.md); this page is the map of the code.

Sizes after the split (lines): `app.rs` 637, `app/*.rs` 67 to 922 except the two the batch
left whole, `app/roaming.rs` 922 (one subsystem) and the pilot's `app/auto/route.rs` 1100; no
test file above 1000 (`app/auto/tests.rs`, `app/services/tests.rs`).

## Rules of the split

- `App` (`app.rs`) is the only owner of the whole state. Each subsystem keeps **its own state
  struct** in its own module, held by one field of `App`, and implements its behaviour as an
  `impl App` block in that module. A child module of `app` sees `App`'s private fields, so a
  module touches the state it does not own only through the fields listed in its code, never
  through a new accessor.
- A method another module calls is `pub(super)` (visible in `app` and below); everything else
  is private to its module. Items other crates use (`pub`, `pub(crate)`) kept their paths:
  `app::App`, `app::Options`, `app::UserEvent`, `app::status`, `app::STEP`,
  `app::story_of`, `app::BattlePlay`, `app::fight_regions`, `app::run_to_phase`.
- Tests sit in `app/<module>/tests.rs`, declared `#[cfg(all(test, windows))] mod tests;`
  at the end of their module (CI compiles them on Windows only, as before, so an item used only
  by tests needs `#[cfg_attr(not(all(test, windows)), allow(dead_code))]`, not `not(test)`:
  `docs/orchestration/worker-rules.md` §2). What several test modules share is in
  `app/test_support.rs` (also Windows-test only).
- The wasm allowances moved with their code: `#[cfg(target_arch = "wasm32")]` fields and
  methods are in the module that owns them (`recording.rs` for the page's record commands,
  `window.rs` for the `UserEvent` variants only the page sends).

## Module map

| Module | State it owns (field of `App`) | What it does | Called by |
|---|---|---|---|
| `app.rs` | `App` itself: `options`, `rom`, `maps`, `current`, `runtime`, `collision`, `field`, `state`, `hero`, `party`, `talk`, `events`, `journal`, `tick`, `steps`, clocks | the struct, `new`/`with_proxy`, `set_rom`/`set_save`/`reset_session`, `update` (accumulator, replay frame, pilot session) and `run_steps` (one fixed step, below) | `window.rs`, `headless.rs`, `auto/*` |
| `options.rs` | `Options` (start-up options, not an `App` field) | `Options`, `Options::from_record`, `story_of`, `item_table` | `native.rs`, `web.rs`, `recording.rs` |
| `window.rs` | none | `UserEvent` and the `winit` `ApplicationHandler` (`resumed`, `user_event`, `window_event`, `about_to_wait`, `exiting`) | `native.rs`, `web.rs` |
| `input.rs` | `Controls` (`held`, `cursor`, `dragging`, `rotate`, `zoom`) | `key` (all key handling by mode: fight, facility screen, talk, field), `axis`, `mouse`, `focus_lost`, `pad_input`/`pad_direction`/`input_bits` | `window.rs`, `recording.rs` (replayed inputs), `run_steps` |
| `camera.rs` | `View` (`camera`, `field_camera`, `camera_home`, `clock_ms`, `framed`) | the game's `L`/`R` turn (`turn_camera`), `look_at` | `run_steps`, `map.rs`, `encounter.rs`, `script.rs` |
| `draw.rs` | `Presentation` (`screens`, `fx`, `panel`) | `upload`, `redraw`, `present_frame` (3D screen, panel, HUD or message window, facility screen, fight UI), `set_layout`, `set_screens`, `set_fx`, `set_text_speed`, `update_title` | `window.rs`, `map.rs`, `encounter.rs`, `facility.rs` |
| `map.rs` | none (`App.current`, `field`, `runtime`) | `load`/`load_map` (map assembly, cast, collision, containers, swirl disc, entry triggers), `change_map`, `cycle_map`, `reload_cast`, `apply_events` (what the field's triggers ask for) | `run_steps`, `script.rs`, `flow.rs`, `input.rs` |
| `script.rs` | none (`App.events`) | `begin_event`, `step_event` (the event task's outcomes), `step_sound` (jingle waits), `drain_placement_changes` | `run_steps` |
| `doors.rs` | `Doors` (`opened`, `angles`) | `open_door`, `play_door_sound`, `step_doors` | `run_steps`, `map.rs`, `script.rs` |
| `daytime.rs` | none (`App.day`, `time`, `environment`) | `set_time`, `sync_day_phase`, `sync_clock_from_day`, `apply_event_lighting` | `input.rs`, `map.rs`, `facility.rs`, `script.rs` |
| `facility.rs` | none (`App.services`, `containers`) | opening the field menu and a facility, the action key on a container, the command keys, the hero's look after an equipment change | `input.rs`, `run_steps` |
| `services.rs` + `services/` | `Services` (menu, shop, inn, church, priest) | the screens' state machine: `open.rs` (opening), `actions.rs` (using, buying, rows), `draw.rs` (quads), `tables.rs` (ROM tables), `transfer.rs` (bag, held slots, equip), `containers.rs` | `facility.rs`, `draw.rs` |
| `encounter.rs` | `Encounter` (`battle`, `battle_change`, `saved_field`, `battle_cover`, `swirl`, `last_battle`) | the transition and scene phases (`BattleChange`), the swirl on the field, set-aside and restore of the field, `battle_transition`, `start_battle` | `run_steps`, `draw.rs`, `fight.rs`, `roaming.rs`, `map.rs` |
| `fight.rs` | `Combat` (`fight`, `battle_pad`, `world`, `world_seeded`, `last_fight`, `revival_map`) | the fight in play: `make_fight`, `drive_fight`, `settle_battle`, wipe-out, party report | `encounter.rs`, `roaming.rs`, `flow.rs` |
| `roaming.rs` | `Roaming` | the field's roaming monsters: spawn, step, contact, flight | `run_steps`, `map.rs`, `draw.rs` |
| `flow.rs` | none | the story's flow: `start_new_game`, `run_battle` (scripted), `wipe_out_arrival`, `talk_to`, `current_story` | `map.rs`, `window.rs` |
| `recording.rs` | `Recording` (`recorder`, `replay`, `jingle_ended`, `replay_pad`, `input_trace`, page flags) | the input log, `state_digest`, `step_begin`, record/replay start and end, the page's record commands | `run_steps`, `input.rs`, `window.rs`, `headless.rs` |
| `headless.rs` | none | native-only and checkpoint drivers (`--walk`, `--replay`, `--play-record`, checkpoints, input trace) | `native.rs`, `web.rs` |
| `auto.rs` + `auto/` | `Session` (the page's pilot run) | the pilot: `auto.rs` (loop, report), `frame.rs` (one frame), `pursue.rs` (goals), `route.rs`, `nav.rs`, `approach.rs`, `session.rs` | `native.rs`, `web.rs`, `update` |

### One fixed step (`run_steps`, the loop's only list of subsystems)

```
step_begin (recording)            replay inputs, digest, steps += 1
step_sound (script)               jingle waits
battle_is_active?  -> step_battle (encounter) ; tick += 1 ; next step
step_doors (doors)
tick += 1 ; journal progress
event running?     -> step_event (script) ; next step
cast_reload?       -> reload_cast (map) ; next step
talk / container   -> examine_container (facility), Talk::begin
open_service / talk step / talk_ended -> field events
input -> View::camera.step, turn_camera (camera), hero step, Field::step
apply_events (map) -> change_map / battle / door / talk requests
step_roaming (roaming) -> begin_encounter (fight, encounter)
runtime.step (animation)
```

The frame (`redraw`, in `draw.rs`) calls `update` (`app.rs`), then `draw_battle` or the field
upload (`encounter.rs`/`draw.rs`) and `present_frame`.

## Extension points

A feature adds its module (or a function in its own) and **one line** in the central place
below; it does not edit `app.rs` otherwise.

| To add | Put the behaviour in | The one hook |
|---|---|---|
| a per-step behaviour (a new task, a new effect on the field) | a method in your module | one call in `run_steps` (`app.rs`), at the position of the original task order |
| a key | your module's handler | one arm in `input.rs::key` (the match is by mode: fight, facility, talk, field) |
| a page or window command | your module | a `UserEvent` variant and its arm in `window.rs::user_event` |
| a start-up option | `options.rs` (`Options`) + `native.rs`/`web.rs` (parsing, **out of this module's scope**) | the field of `Options` |
| a phase of the battle transition | `encounter.rs` (`BattleChange` variant and its arm in `step_battle`) | none |
| a draw layer (an overlay, a transition cover, a HUD part) | your module's `draw_*` | one call in `draw.rs::present_frame` (after the layer it covers) |
| a trigger request (`Request::...`) | your module | one arm in `map.rs::apply_events` |
| an event native (`Outcome::...`) | `event.rs`/`event/` | one arm in `script.rs::step_event` |
| a facility | `services/` (a screen and its rows) | a `ServiceTag` arm in `services/open.rs` |
| state that must survive a map change or a battle | your module's struct | a field of `App`, initialised in `app.rs::with_proxy` through `YourState::new()` |

## Who owns which feature

| Feature | Owner module(s) | Notes |
|---|---|---|
| Battle transition (swirl, black cover, scene swap, way back) | `encounter.rs`, `battle/transition.rs` | wiring the swirl into the loop is a `step_battle`/`draw.rs` change only |
| Fight rules, outcomes, wipe-out | `fight.rs`, `battle/fight.rs` | `Combat` |
| Roaming monsters and contact | `roaming.rs` | |
| Text speed and dialogue pacing | `draw.rs::set_text_speed` (switch), `talk.rs` (pacing, out of scope) | `--text-speed`, `?textspeed=` |
| Field menu, shops, inn, church, containers | `facility.rs` (keys, opening), `services/` | |
| Record, replay, digests, page recording | `recording.rs`, `record.rs`, `headless.rs` (native drivers) | `Recording` |
| Camera (the game's turn, framing, swirl roll) | `camera.rs`; the event camera is `event/camera.rs` | `View` |
| Screens, layout, enhanced effects | `draw.rs`, `screens.rs`, `hud.rs` | `Presentation`, `Options::layout` |
| Time of day, lighting, day clock | `daytime.rs`, `lighting.rs` | |
| Doors | `doors.rs` | |
| Map loading, cast, placements | `map.rs`, `load.rs` | `load.rs` is out of scope |
| Story flow (new game, scripted battles) | `flow.rs` | |
| Pilot (`--auto`, `--route`, the page's pilot) | `auto.rs` + `auto/` | |
| Keys and pointer | `input.rs` | `Controls` |

## Later splits (out of scope of this batch)

- `native.rs` (1261): argument parsing, the run modes (window, headless, screenshot, replay,
  record) and their reports, as `native/args.rs`, `native/run.rs`, `native/screenshot.rs`.
- `web.rs` (828): URL parameters, file pickers, the recording UI and the canvas, as
  `web/params.rs`, `web/files.rs`, `web/record.rs`.
- `event.rs` (1219): the player's state, the script VM's natives and the stage as `event/player.rs`,
  `event/natives.rs` (it already has `camera`, `guest`, `placement`, `scan`).
- `talk.rs` (1342): the message window's geometry and drawing, the text typing, and the
  service tags as `talk/window.rs`, `talk/typing.rs`, `talk/service.rs`.
- `battle.rs` (1373): the scene build, the fighter grid and the motions as `battle/scene.rs`,
  `battle/grid.rs` (it already has `data`, `fight`, `motion`, `narrate`, `transition`, `ui`).
- `app/auto/route.rs` (1100) and `app/roaming.rs` (922) stay whole: each is one subsystem.

## Proposed replacement of the "shared files" list (`AGENTS.md`, `docs/orchestration/README.md`)

The coordinator applies it; this batch does not edit those files.

> - **One owner per shared file.** `dq9-app/src/app.rs` (the state struct, the constructor and
>   the step loop only: a feature adds **one call** there), `dq9-app/src/app/input.rs` (the key
>   match), `dq9-app/src/app/draw.rs::present_frame` (the draw order), `dq9-app/src/event.rs`
>   + `event/**`, `dq9-core/src/{field,state}.rs`, `dq9-core/src/trigger.rs` + `trigger/**`,
>   `Cargo.toml`, `re/symbols.txt`: in the ```owns block of a single batch at a time. A feature
>   owns its module (`app/<module>.rs` + `app/<module>/**`, see `docs/app-structure.md`): two
>   batches on different modules of `crates/dq9-app/src/app/` do not conflict, and the hook
>   they add in `app.rs`/`input.rs`/`draw.rs` is one line, which the batch that owns that file
>   at that time writes (or the module's batch, if it is the only one touching the file).
