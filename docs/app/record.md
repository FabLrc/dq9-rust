# Recording a human session (`--record`, `--play-record`)

Nobody plays 50 minutes on demand, and the pilot's trace (`--trace-inputs`/`--replay`) cannot
stand for a person: it records the pilot's d-pad and action key, in world directions with the
camera at yaw 0, so it has no camera turn, no cancel/menu keys, no field menu or battle
commands, no mouse. `--record` writes the **inputs the game logic reads from the window**, by
fixed step, and `--play-record` feeds them back through the same path a person uses (`App::key`
and the window's 60 Hz loop): the user plays once, and every later check (tests, regressions,
the parity script) replays that exact game.

```powershell
# Play: a window, a new game, a seed (picked and stored when --seed is absent). Close the
# window (or Esc from the field) to end; the file is also rewritten every 10 s of game time.
cargo run -p dq9-app --release -- --new-game --seed 7 --record build/records/session.dq9rec
# Replay headless (release): the stop line, the digests checked, the party; the exit code is 1
# on a mismatch. -Expect makes the run stop on an event and fail if it never ends.
powershell -File scripts/replay-record.ps1 -Record build/records/session.dq9rec -Expect 2750
# The same, by hand, with the story checkpoints (the `--checkpoints` format of the pilot runs):
cargo run -p dq9-app --release -- --play-record build/records/session.dq9rec `
  --expect-event 2750 --checkpoints build/records/session-checkpoints.jsonl
# Watch it in a window (the keyboard only closes it; M mutes, R changes the layout):
cargo run -p dq9-app --release -- --play-record build/records/session.dq9rec --window
# The ignored real-ROM test: DQ9_RECORD (the file), DQ9_RECORD_EVENT (default 2750),
# DQ9_RECORD_SAVE (the save it started from, if any).
$env:DQ9_RECORD = 'build/records/session.dq9rec'
cargo test --release -p dq9-app -- --ignored human_record_reaches_the_expected_event
```

Keep the file under `build/records/` (ignored by git): it holds no ROM data but is a personal
session. The start options are `--new-game` or `--save`/`--map`, `--seed`, `--time`/`--clock`,
`--chapter/--step/--substep`, `--stage`, `--event`, `--night`; `--play-record` takes them from
the file (and refuses to be given others). A session started from a save is replayed with
`--save <the same file>` (its hash is checked). `--record` needs a window and refuses
`--headless`, `--battle` and the offscreen modes.

**What is recorded** (`record.rs`): the keys `App::key` reads (the d-pad as arrows/WASD,
confirm Enter/Space/Z, cancel Backspace/X, Escape inside a fight or facility screen, `Q`/`E` the
camera's L/R, `I` the field menu, `H`/`T` the facility screens' keys, and the debug keys Tab,
`F`, `T` in the field, PageUp/PageDown), the **mouse drag and wheel** (a drag turns the camera
yaw, which is the walk direction), the **focus loss** (the held keys are forgotten) and the
**end of a jingle** a script waits for (`Audio::take_jingle_ended`: the sound device decides it
by wall time, so it is an input; a replay takes the recorded one, whatever its own, silent,
audio says). Mute, layout, screens, effects, window size and the Shift key never reach the logic
and are not recorded. Each input carries the number of fixed steps completed when the window
handled it (`App::steps`), so a slow or skipped frame, a window dragged for a second (the
accumulator clamps it at 0.25 s) or another machine cannot change what a step sees.

**File format** (`dq9rec` v1, UTF-8 text): a JSON header line (format, version, ROM identity,
start options, `battles: "show"`, `digest_every: 600`), then one line per event, in step order:

```text
{"format":"dq9rec","version":1,"rom":"YDQP01","rom_version":0,"rom_crc":"f9cc","new_game":true,"seed":7,...}
=0 map=6405 story=1.1.1 tick=0 hero=... party=1/0/0/0 gold=0 rng=eb68a9a21020d6bb
+170 KeyZ        a key pressed at step 170 ...
-171 KeyZ        ... and released: a key held for 43 steps is these two lines (a run)
!400             focus lost
j512             a jingle ended
m730 3d4ccccd 0 0   mouse drag (x, y) and wheel, f32 bits in hex
=600 map=1112 story=1.1.1 tick=600 hero=... party=... gold=0 rng=...
.2898            end: fixed steps completed
```

A file cut short (a crash) ends at its last line: the digests every 600 steps are rewritten as
they are taken. The ROM is identified by its header (game code and maker code `YDQP01`, version,
header CRC-16), a save by an FNV-1a hash of its bytes; a record played on another ROM is refused.

**Divergence guard.** Every 600 steps (and at the end) the recorder stores a digest: the map, the
story point, the tick, the hero's position and heading (f32 bits), each party member's
level/HP/MP/experience, the gold and a hash of the world's generators. The replay compares each
one as it falls due and stops at the first difference, naming the step and the field
(`digest mismatch at step 6600: hero recorded ..., replayed ...`; exit code 1). A dependency on
the wall clock, the hash order or uninitialised state shows up here.

**Presentation.** A window shows its battles and lets the field's monsters roam; the replay does
too (`App::new_replay`: no window, but `presented`), and a record's `battles` is always `show`:
shown and instant battles are not tick-comparable (`j4-show-battles`). A record is replayed
in the same presentation it was played in, whatever frames the window spent.

**Against `--trace-inputs`/`--replay`.** Those stay as they are for the pilot and the parity
references: one JSONL line per step with the pad bits and the action key, replayed through
`App::replay_step` in world directions (camera yaw 0). A record is the human's: logical keys
through `App::key`, camera turn, mouse, menus, digests. `scripts/parity.ps1` takes a record as
a reference's `inputs` with `"kind": "record"` (it then runs `--play-record`); the record
carries the start options, so the reference's `args` stay empty.

**Web.** The page records and replays the same files, through the same `App` hooks (a
recording is `App::recorder`, a replay `App::replay`):

```text
https://fablrc.github.io/dq9-rust/?record=1&newgame=1&seed=7&mute=1   record a session
https://fablrc.github.io/dq9-rust/?play-record=1&mute=1               replay a .dq9rec
```

`?record=1` records the session that starts when the ROM is read, from its first step: the seed
is `?seed=N`, else 7 — a record has to replay, so the page never picks a seed now — and the
header shows it (`#recording`, `● recording · seed 7 · 1200 steps`). "Save recording"
(`#recordsave`) is enabled while the page records and works at any moment: it downloads
`dq9-<seed>-<steps>.dq9rec`, built by the page as a `Blob` (`index.html`'s `dq9Download`; the
wasm side writes no file and uploads nothing), holding every input recorded so far plus a
closing digest of the step it is saved at (`Recorder::snapshot`: a digest is stamped with the
state its step starts from, like every other one), and the recording goes on. The step count is
the app's own (`App::step_begin` calls `web::recording_progress` once a second). The fields that
would replace the recorded session (ROM, save, time of day, "New game") are disabled while it
runs; layout, screens and effects never reach the logic and stay live. No `beforeunload` offer
is made: the download is the player's click.

The header is the native one (`battles: "show"`, the start options of `Options`: `new_game`,
`map`, the save's hash, `seed`, `time`/`clock`, chapter/step/substep, night, the event) and a
replay starts from it. Like a native record it holds no `textspeed`: a replay types dialogue at
the default (`normal`) pace, and `fight` is the presentation a session always has.
`?play-record=1`, or "Play recording" (`#recordfile`) on any page, reads a `.dq9rec` (native- or
Web-made) and then asks for the ROM, which must be the one the record was played on
(`Header::check_start`: game code, version, header CRC, save hash); the choice of ROM is the
page's normal one. A replay in the page is the native windowed one
(`App::replay_frame`): its inputs feed `App::key` at their step, the digests are compared as they
fall due, and the stop line goes to the console and to `#status` —
`stopped: event N ended after S s of game time`, `stopped: record ended after N steps …`, or
`stopped: digest mismatch at step N (recorded at M): field recorded X, replayed Y`. The page
starts the replay on its own clock (`App::start_page_replay` resets `steps` and `tick` before
loading the record's start options), so a record replays even when the page has already played
something. During a replay the keyboard changes only what is heard and seen (`M` mutes, `R`
changes the layout); a native window also closes on Escape, the page does not.

**Escape on the page.** From the field, Escape ends the session (`App::key` returns `true`): the
native window closes and writes its record. The page has no window to close, and its event loop
would stop with everything in it, the recording and "Save recording" included, so there Escape
does nothing in the field (`return !cfg!(target_arch = "wasm32")`, same in a replay) and the
page keeps its session. Inside a fight or a facility screen Escape is the game's cancel key on
both and is recorded like any other input; from the field neither its press nor its release is
recorded (`App::log_key`), so a record holds no unpaired key line.

Cross-compatibility is checked both ways (batch `j4-web-record`,
`build/evidence/j4-web-record/`): a browser session replayed by `--play-record`, and a record
made natively replayed in the page, every digest matching.

**Esc closes the window** from the field natively (it is the game's cancel key only inside a
fight or a facility screen), which ends the recording: close the window on purpose. On the page
Escape leaves the session running: download the record, then reload the page.

**Determinism hunt** (batch `j4-human-record`, evidence under `build/evidence/j4-human-record/`).
Observed:

- The pilot's `--route 2750` (instant battles, release) pins the 972 checkpoints of the long
  test: `stopped: event 2750 ended` at **3627.8 s** of game time, sha256 `3d27e89d9737`
  (`build/evidence/j4-parity-regress/route-2750-checkpoints.jsonl`; the run before the original
  message pacing of `d068809`/`f516d79` stood at 3035.5 s and `8ce4af30e497`, and the two files
  differ on 970 of the 972 lines in `tick`/`seconds`/`vblank` alone). Two runs of one build give
  the digest byte for byte (`route-instant-a-checkpoints.jsonl`, 1251 s of wall time each, the
  machine loaded). The headless `--replay` of the parity trace runs on the same `App` step and
  reaches the story points the trace was recorded on: since batch `j4-parity-regress` that trace
  is the pilot's own closed loop (173 checkpoints, `parity.ps1` 2/2, and the test
  `a_pilot_trace_replays_to_the_first_control_point`).
- The window itself was recorded with real key presses sent to it as scan codes (`window.dq9rec`:
  2898 steps, Z, Up, Q; `window-sound.dq9rec`: 6217 steps, sound device on, one recorded jingle
  end `j5802`), and both replay headless with every digest matching (6 and 12).
- The in-process session test (`a_recorded_session_replays_to_the_same_digests_at_any_pacing`):
  a scripted human (confirm taps, the field menu key, Tab, a held walk, the camera's Q, a mouse
  drag, a focus loss) played through `App::key` and `update()` with frames of 0 to 7 steps is
  replayed headless and through `update()` with other frames: the same digests, state and
  checkpoints. Without its inputs the replay fails at the first digest that differs.
- Inputs found that the logic reads and the first sketch missed: the jingle end (the sound
  device's wall time) and the mouse drag (the camera yaw). Nothing else read from the clock.

**Not established** (**[unknown]**): the shown route (`battles=show`, the window's presentation)
run twice, `the_shown_route_runs_twice_identically`: it is written and `#[ignore]`d but did not
finish in 35 minutes on the loaded machine and was stopped, so the window's shown battles are
covered only by `a_shown_battle_leaves_the_game_in_the_state_an_instant_one_does` and not by a
window recording; a recorded human battle and a long field session were not available to the
batch (the scripted keys never reached a fight).
`scripts/parity.ps1` shows `control-1.1.2` **FAILED** (no runtime checkpoint at its story point)
on `c035fa9` as well as on this branch, with identical runtime checkpoints: not caused by this
batch.
