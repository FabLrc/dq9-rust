# Tools: Ghidra cache and USA source

Two scripts speed up reverse engineering: `scripts/ghidra-query.ps1` caches Ghidra's
answers and `scripts/decomp.ps1` maps a European address or name directly to the source
code of the American decompilation. Both work from a linked worktree: the tools, the Ghidra
project and the ROM are then read from the main repository when the worktree does not
contain them (they are ignored by git).

## `scripts/decomp.ps1` — EU address to USA source

The [DQIX/dqix-decomp](https://github.com/DQIX/dqix-decomp) decompilation is under the
Unlicense, but its checkout is not committed: it is expected in `tools/dqix-decomp` (or
designated by `-Decomp`). `re/functions.txt` provides the European functions (address,
size, category, decompilation name); the script finds the matching symbol in
`config/usa/arm9/**/symbols.txt`, then the source file in the module's `delinks.txt`. For
ARM9, the USA address is obtained by the inverse conversion of `usa_to_eur`
(`crates/dq9re/src/coverage.rs`): `-8` on `0x0200F3C0..0x0200FB30`, `-0x10` on
`0x0200FB30..0x020EEBE0`, unchanged elsewhere. Overlays and ITCM have the same addresses in
both versions.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 GameState::SetDayTimer
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 ov_017::0218b5a0 -Context 20
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 -List SetDayTimer
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250 -Decomp <path-to-dqix-decomp>
```

- An address is looked up as in Ghidra: largest start address less than or equal, as long
  as it precedes the next start. An overlay address requires the `ov_NNN::` prefix; an ARM9
  address that falls after the last function reminds you of it.
- A name is looked up first exactly (raw symbol, demangled name, then last component), then
  by substring. Several results give the list of candidates and exit code 2; `-List` prints
  addresses and names without an excerpt.
- `-Context N` prints N lines from the definition (40 by default). Non-demangled symbols are
  rendered by a minimal Itanium demangler (`_ZN...E`, constructors, destructors); an unknown
  name is printed as is.
- If the checkout, the symbol or the source is missing, the script says which and exits with
  an error (code 1): it does not invent a path. Many overlay functions do not yet have source
  in the decompilation, only a symbol.

Examples verified on November 6, 2026 on checkout `b5c617a`:

```text
$ scripts/decomp.ps1 0x02010250
eu    0x02010250  0x40  game  GameState::AdvanceDayTimer
usa   0x02010240  _ZN9GameState15AdvanceDayTimerEv
src   ...\tools\dqix-decomp\src\GameState\GameTime.cpp:28
    28: void GameState::AdvanceDayTimer()

$ scripts/decomp.ps1 ov_017::0218b5a0 -Context 4
eu    ov_017::0218b5a0  0x10  game  SetGameResources
usa   0x0218b5a0  _Z16SetGameResourcesP13GameResources
src   ...\src\Resource\GameResources.cpp:16
    16: void SetGameResources(GameResources* gameResources)
```

`0x02010250` is indeed `AdvanceDayTimer` in both versions (USA `0x02010240`);
`GameState::SetDayTimer` starts at European address `0x02010298` (USA `0x02010288`) and is
found by its name. The ITCM case is covered by the same path: `0x01ff8810` gives
`VRAMStagingManager::ZeroInitialize` in `src/Graphics/itcm/VRAMStagingITCM.cpp`.

## `scripts/ghidra-query.ps1` — Ghidra query cache

An `analyzeHeadless` query costs 5 to 30 s. The filtered result is now written to
`build/ghidra-cache/<key>.txt` and read back as is on the next identical call. The SHA-256
key covers:

- the command and its arguments;
- the ROM identity: canonical path, size and modification date (`DQ9_ROM`, otherwise
  `rom/*.nds` then the root, in the worktree or the main repository);
- the SHA-256 of `re/ghidra_scripts/Query.java`.

`cache: hit` or `cache: miss` is written to stderr; stdout stays that of `Query.java`.
`-Refresh` ignores the existing entry and rewrites it. An `analyzeHeadless` failure, or an
empty output (invalid arguments, locked project), is not cached.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 decomp 0x02010250
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 decomp 0x02010250   # cache: hit
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 xrefs 0x02010250 -Refresh
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 calls 0x02010250 0x02010298
```

Measured on November 6, 2026 on the local ROM and project, two identical calls to
`decomp 0x02010250`: `cache: miss` in 5,193 ms, then `cache: hit` in 289 ms, identical
outputs. The other forms (`xrefs`, `calls` with several addresses) ran successfully before
and after caching.

Limits:

- `re/symbols.txt` or the project analysis can change without changing the key: after
  applying symbols, `-Refresh` is required. `Query.java` is versioned by its content, the
  Ghidra project is not.
- The cache is local to the calling worktree (`build/` is ignored); it is never committed
  and contains only Ghidra's output. The printed excerpts remain game data: do not copy them
  into the repository beyond what evidence requires.
- A query still locks the project while it runs; for parallelism, keep one copy of
  `re/ghidra` per query and `DQ9_GHIDRA_PROJECT`, the cache changes nothing there.

## Memory tracing: `DQIX_WATCH` from DQIX Enhanced

[DQIX Enhanced](https://github.com/sackchief/dqix-enhanced) (GPL-3.0, v33, commit
`86ff9b1`) is a static recompilation of the European code. It is an **external tool**: it
may be run to observe the game, never copied or linked into the Rust crates. It has **no
prebuilt binary**; building from the ROM is mandatory (MSYS2 UCRT64, 10 to 20 min the first
time, ~3 GB):

```sh
./tools/build-game.sh /c/path/YDQP.nds   # game/dqix.exe, game/data/code, game/dqixcore.dll
```

Its headless mode creates no window and logs ARM9 writes to `stderr`, which makes it the
memory tracer for flags whose writer is not yet known:

```powershell
$env:DQIX_NOWINDOW   = "1"
$env:DQIX_NO_SETTINGS = "1"
$env:SDL_AUDIODRIVER = "dummy"
$env:DQIX_RTC        = "1760000000"       # fixed clock, reproducible
$env:DQIX_INPUT_FILE = "build/watch/inputs.txt"
$env:DQIX_RAMDUMP    = "build/watch/ram,12000,12100"
$env:DQIX_WATCH      = "02108ADC"         # a single ARM9 word, 4-byte aligned
$env:DQIX_WATCHREGS  = "1"
$env:DQIX_TRACEN     = "64"
.\game\dqix.exe <ROM> game\data\code 12201 1 build\watch\eval.sav 2> build\watch\watch.log
```

Each write produces `[f<frame> l<vblank>] watch: write32 <address> = <value> (lr <lr>)`,
followed by `r0..r12` with `DQIX_WATCHREGS`, then the last ARM9 functions entered (names
with `-DRECOMP_TRACE`). `DQIX_RAMDUMP` writes raw 4 MiB images at the requested frames,
readable by `dq9re state --ram` once that mode ships. Limits: a single watched word per run,
no PC or full stack, no value before the write. The full reasoning and the code paths (v33,
`runtime/mem.c` l. 157-188, `runtime/io.c` l. 8-19) are in the local report
`build/eval-dqix-enhanced/RAPPORT-watch-drapeaux.md`; as of November 6, 2026, the units
`mem.c`, `dispatch.c` and `frontend_none.c` compile with MinGW-W64, but the full link with
the ROM remains to be run.

## Comparing two runs: `scripts/compare-runs.ps1`

The runtime (`dq9-app --checkpoints`) and the libretro harness write the same JSONL: one
object per point, with `map`, `track`, `tracks` (chapter, step, sub-step and their flags),
`flags`, `quests`, `daytime`, `hero` and a free `kind`. The two runs do not share a time
base: melonDS advances at real-time clock and the runtime compresses waits. The script
therefore aligns by **story point** (`map:chapter.step.substep`) and by `kind`, not by
frame number.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/compare-runs.ps1 `
    -Original build/oracle-host/checkpoints.jsonl `
    -Runtime  build/oracle/runtime.jsonl -Limit 100
```

Alignment rules (fixed on November 7, 2026; the batch is under validation, the `main`
script before this batch reused runtime points and exited 0 on zero alignment):

- a runtime checkpoint is used **only once**: two original checkpoints with the same key
  cannot be "proven" by the same runtime point;
- same `kind` first; sole exception, an original `after` (captured after player control)
  may pair with the runtime `control`;
- a `kind` with no counterpart (the `f012625` movement samples of a route run, for example)
  stays **unpaired**: comparing it to a `control` of the same step would compare two
  different instants;
- `aligned 0` is never a success: the script prints `NO PARITY` and exits with 1.

The output gives the number of pairs and the number of **distinct** runtime checkpoints
used, then the unpaired ones. Exit codes: `0` all aligned pairs are identical and at least
one exists; `1` a difference or no pair; `2` unreadable input or non-JSON line. A difference
does not change the code (it stays 1, it no longer equals the number of differences). A
`daytime` or a `hero` present on only one side fails instead of being ignored when the
comparison requests it (`-HeroTolerance`).

Cases measured on the November 6, 2026 evidence (throwaway pairs under
`build/v01-20261107/parity/throwaway/`):

| Input | Before | After |
|---|---|---|
| disjoint keys | `aligned 0 ... OK` (code 0) | `NO PARITY` (code 1) |
| 2 identical originals, 1 runtime | `aligned 2 ... OK` (1 runtime reused) | `aligned 1 ... with 1 distinct`, 1 unpaired |
| 2 distinct originals, 2 distinct runtime | difference reported (the first runtime was reused) | `aligned 2 ... with 2 distinct`, OK |
| `oracle-host` (2) vs fresh runtime first control (19) | `aligned 2`, code 2 | `aligned 1`, 1 unpaired, code 1, only difference `4135` |
| `route1` (62 samples) vs fresh runtime | `aligned 62` against a single `control`, code 62 | `NO PARITY`, code 1 |

`route1-checkpoints.jsonl` is a movement sampling (one point every 120 frames), not a
sequence of story points: it must not be counted as 62 pieces of evidence.

## Comparing a melonDS state to a point: `scripts/oracle-compare.ps1`

This strict comparator takes **one** melonDS state and **one** runtime point, either a JSON
object (`-RuntimeJson`, a single line, not a JSONL), or the `--auto` text report (`-Report`
with `-TrackIndex`). It compares map, current track, progress and flags of the five tracks,
global flags, quests and `daytime`.

```powershell
target/release/dq9re.exe state build/orca/oracle/bat6.ml1 --json
# then, with the last line of --checkpoints alone in a file:
powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 `
    -State build/orca/oracle/bat6.ml1 `
    -RuntimeJson build/v01-20261107/parity/runtime-first-control-object.json `
    -Dq9re target/release/dq9re.exe
```

At first player control in the Observatory (`bat6.ml1`, vblank 26379, map 4105, 1.1.1):

```text
checkpoint: vblank 26379, map 4105, track 0 1.1.1
runtime: map 4105, tracks 5, global flags 17
DIFF global flags only in melonDS: 4212
```

Only the random loot of battle 26 differs: `4212` (slimedrop) in `bat6.ml1`, `4135`
(medicinal herb) in `build/oracle-host/checkpoints.jsonl`. It is not an initialization
failure and it is neither forced nor masked. Since November 7, 2026, a runtime object
without a `daytime` or `quests` field produces an explicit difference (an absent field is
not an empty value), and an out-of-bounds melonDS track index is rejected. The script exits
with 0 or 1, no longer with the number of differences.

**Yaw convention**: `dq9re state --json` writes the game's raw angle (`Object3D+0x54`,
`/4096`), whereas `dq9-app --checkpoints` writes the application yaw. The two differ by
`π`: at first player control, `4.860` raw and `-1.718` application, with
`π - 4.860 = -1.718`. Positions are identical; only the `yaw` field is not in the same
convention. `compare-runs.ps1` only compares `x`, `y`, `z` (`-HeroTolerance`), so it is not
affected; a yaw comparison will have to convert one of the two.

## Automated parity: `scripts/parity.ps1`

Parity evidence is no longer prose: each proven route is a **reference** replayed on the
headless runtime and compared to the original by `compare-runs.ps1`.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/parity.ps1
powershell -ExecutionPolicy Bypass -File scripts/parity.ps1 -References <file.json> -Rom <rom.nds>
```

By default, `-References` is `build/evidence/parity/references.json` (local, ignored: it
designates original checkpoints, game data) and the ROM follows the `crates/testrom` lookup
(`DQ9_ROM`, otherwise the only `*.nds` in `rom/`, otherwise in the root). The script:

1. checks each file (runtime inputs, original checkpoints) against its sha256 **before**
   use: missing file or different hash = reference failure;
2. builds `dq9-app` in release only once;
3. replays each input set (`--rom … <args> --headless --replay <inputs> --checkpoints
   <output>`), only once per inputs + options pair;
4. removes each accepted global flag from the copy of the declared side, then compares via
   `compare-runs.ps1` (the reference's `-HeroTolerance`, `-Limit 100`).

It only writes under `build/parity/`: `run<N>-checkpoints.jsonl` and the `dq9-app` logs, and
`<reference>/original.jsonl` / `runtime.jsonl` (the compared copies); cargo writes to its
target directory. Output: one `ok` or `FAILED` line per reference, then its discrepancies
(`DIFF` lines from `compare-runs.ps1`, hash, missing file), and a summary. Code 0 if all
references pass, 1 otherwise.

Stricter than `compare-runs.ps1` alone: an **unpaired** original checkpoint fails the
reference (it would prove less than it claims), and an accepted discrepancy **absent** from
its side is stale and fails too (remove it from the reference).

### `references.json` format

```json
{
  "version": 1,
  "references": [
    {
      "name": "control-1.1.2",
      "description": "Free control after 1170 in X01M01 (4101), story 1.1.2.",
      "inputs":   { "path": "inputs/combined-final-inputs.jsonl", "sha256": "5394e04e…", "source": "…" },
      "original": { "path": "original/control-1.1.2.jsonl", "sha256": "22554295…", "source": "…" },
      "args": ["--new-game", "--mute"],
      "heroTolerance": 0.01,
      "accepted": [
        { "flag": 4135, "only": "original", "reason": "random loot of battle 26…" }
      ]
    }
  ]
}
```

- `name`: `[A-Za-z0-9._-]+`, unique (name of the output folder).
- `inputs`: runtime `--trace-inputs` file; `original`: original JSONL checkpoints (same
  format as `dq9re state --json`, with a `kind` that aligns with the runtime's, usually
  `control`). `path` is relative to the references file's folder; `sha256` full (64 hex);
  `source` describes provenance (free text, not read).
- `args`: `dq9-app` launch options; `--rom`, `--headless`, `--replay`, `--checkpoints`,
  `--auto`, `--pilot`, `--trace-inputs` are rejected there (the script sets them).
- `heroTolerance`: tolerated deviation on the hero's `x`/`y`/`z`; `0` or absent = not
  compared. Yaw is never compared (different conventions, above).
- `accepted`: accepted discrepancies, each **one** global flag present on only one side
  (`only`: `original` or `runtime`) with its mandatory justification `reason`. The flag is
  removed only from that side and only from global flags: any other difference fails, and so
  does the flag appearing on the other side. No other key is allowed (a typo is an error,
  not an ignored option).

### Current references (November 8, 2026)

Batch `build/evidence/parity/` (`manifest.json` via `scripts/evidence.ps1 -Batch parity`):

| File | sha256 | Provenance |
|---|---|---|
| `inputs/combined-final-inputs.jsonl` | `5394e04ed1cb` | `build/v01-20261107/combined/final-inputs.jsonl`, combined native route (40,432 steps, first player control at step 12625, control 1.1.2 at step 20068) |
| `original/first-control-1.1.1-male.jsonl` | `1fe8b98dfbc6` | `dq9re state --ram build/oracle-host/ram_male.bin --json` + `kind: control` (male libretro run, no loot) |
| `original/control-1.1.2.jsonl` | `2255429503fd` | `build/v01-20261107/oracle/control112-as-control.jsonl` = `control112-cp.jsonl` (`d357eb188e0b`) with `kind` renamed `control` |

- `first-control-1.1.1-male`: first player control (4105, 1.1.1), no accepted discrepancy.
- `control-1.1.2`: free control 1.1.2 (4101, VBlank 58392), accepted discrepancy `4135`
  (medicinal herb, random loot of battle 26 of the female run; the runtime substitutes the
  battle and does not roll loot, no forced flag: [`melonds-state.md`](melonds-state.md),
  [`item.md`](item.md)).

Output on `5b32cf1` (modified working tree, European ROM):

```text
ok      first-control-1.1.1-male  aligned 1/1 original checkpoint(s), runtime 615
ok      control-1.1.2  aligned 1/1 original checkpoint(s), runtime 615; accepted: flag 4135 original only (1 checkpoint(s))
parity: 2 reference(s), 2 ok, 0 failed
```

Falsification: without the `4135` entry, `control-1.1.2` fails on
`DIFF 4101:1.1.2 control global flags: original [… 4135 …], runtime […]`; an altered
original fails on the hash, then, hash updated, on
`DIFF 4101:1.1.2 control track 0 step flags: original [1,2,3], runtime [1,2,3,4]`.

Missing data, not fabricated: no original checkpoint exists beyond 1.1.2 (Yggdrasil 1.2.1,
Angel Falls, village tasks, 1.4.1); these points remain without a reference.
`combined/full-inputs.jsonl`, cited as evidence of the native route, no longer goes through
1.1.2 on `5b32cf1` (25 checkpoints, no 1.1.2 control): only `final-inputs.jsonl` serves as
a reference. The female first-control run (`build/oracle-host/checkpoints.jsonl`) is not
reused: its second `after` point has no second runtime control to pair with and its only
discrepancy (`4135`) is already covered at 1.1.2.

### Adding a reference

**Rule**: every new route proven against the original becomes a reference in this file or a
`real_rom` test, never only prose in the roadmap or the docs.

1. Replay or produce the runtime inputs (`--trace-inputs`) and the original checkpoints
   (libretro harness + `dq9re state --json`) at the same story point, with a `kind` that
   aligns (`control` for player control).
2. Copy both files under `build/evidence/parity/` (`inputs/`, `original/`), without
   modifying their content; note any transformation (`kind` renaming) in `source`.
3. Add the entry with the sha256 (`Get-FileHash -Algorithm SHA256`); an accepted
   discrepancy requires an established and documented cause, never a flag forced to pass.
4. Run `scripts/parity.ps1` (all `ok`), then `scripts/evidence.ps1 -Batch parity` for the
   manifest; cite the files with the first 12 characters of their sha256.

## Movement at equal frame count

The field updates at 30 Hz; `App::run_steps` now runs the field (`Hero::step(..., 2)`,
`Field::step`, gait) every other VBlank and holds the pose on the intermediate frame
([`movement.md`](movement.md#what-is-ported)). `--walk` goes through `run_steps` and thus
follows this cadence. The measurements in this section date from the 60 Hz application
(`ticks = 1`): they keep their historical comparison value, but the conclusion has changed
and strict equality is now verified by a native test on the real ROM
(`field_cadence_holds_the_pose_and_matches_the_original_every_vblank`, `crates/dq9-app/src/app/tests.rs`).
Comparing two engines at the same frame requires starting from the **same state**: same
position, same speed (stopped) and **same orientation**.

First attempt, from first player control without aligning orientation
(`build/v01-20261107/parity/movement-up/`): original `core23050.bin` (vblank 23033) with
`up` held, RAM captured every frame; runtime with `--start 0.66,-3.61` and 120 one-frame
`-z` segments. Movement over two frames is identical within 0.001 on 47 of the 51
intervals from frame 20 to 120 (maximum deviation 0.016 at slope breaks; stable step 0.192
unit = 786 in fixed-point), but at frame 120 the original is at `z = -13.565` and the
runtime at `z = -13.495`. This deviation came from the start: `--walk` has no yaw option and
places the hero facing `+z` (raw angle 0) whereas the original faces 4.860; the initial
rotation and the acceleration did not end at the same moment. This measurement therefore
did not prove a cadence defect.

Second experiment, identical orientation
(`build/v01-20261107/parity/movement-facing/`):

```powershell
# original: turn to raw angle 0 (down), stop, then hold up at frame 70
python scripts/melondsds_host.py --rom <ROM> `
    --core tools/build/melondsds_libretro.dll `
    --state-in build/oracle-host/core23050.bin `
    --input build/v01-20261107/parity/movement-facing/prep.jsonl --frames 191 `
    --checkpoints build/v01-20261107/parity/movement-facing/original-prep.jsonl `
    --checkpoint-every 1 --dq9re target/release/dq9re.exe

$S = (1..120 | ForEach-Object { '-z:0.0166667' }) -join ','
target/release/dq9-app.exe --rom <ROM> --map 4105 --start 1.756,-3.041 `
    --walk $S --mute
```

The original stops at `(1.756, -0.250, -3.041)`, raw angle 0, zero speed at frame 70;
`--start 1.756,-3.041` sets exactly the same state (raw angle 0, zero speed). Result at the
same frame, over 120 frames of `up`:

- the trajectory is the same: `x` identical, `y` within 0.003, `z` within 0.003 at frames
  50, 100 and 120; the hero stops at the same boundary `z = -11.299`;
- while walking, the deviation follows the 30/60 Hz phase: the original only moves every
  other frame, the application advances every frame. The maximum deviation is 0.191, exactly
  one 30 Hz step (786/4096 = 0.192), and it vanishes as soon as both positions are blocked
  at the same wall. No cumulative drift;
- on even frames (two 60 Hz steps = one 30 Hz update), the deviation stays under 0.10, the
  difference coming from collision rounding on slopes.

Conclusion of the historical measurement: the 60 Hz application followed the original at the
30 Hz step, with no cumulative drift, but **strict equality at every VBlank was not
reached**: the application advanced every frame, the original holds its position every
other frame. The measured deviation was bounded by one field step (0.191 maximum, 0.097 on
even frames, 0.003 when stopped).

The v0.1 criterion "movement at controlled frame count" is now **closed for this segment**:
the native test on the real ROM replays exactly the starting state above
(`(1.756; −3.041)`, raw angle 0, speed 0) and checks at every VBlank the exact raw angles 0,
1608, 3216, …, 12868, the fx `z` values of the original capture (tolerance 2), the
byte-for-byte hold on even frames and the camera that keeps following the held pose. The
phase is the one from the review: the first VBlank only reads the pad, the first movement is
at the third field step (VBlank 5), the U-turn settles at VBlank 19. The cadence of
`Field::step` (doors) remains **[hypothesis]** — 4 calls at 30 Hz = 8 VBlanks — until the
melonDS measurement described in [`movement.md`](movement.md) is done. No CLI setting was
added.

Limits: `--walk` cannot start from an arbitrary yaw, hence the original preparation above;
only first player control points have an original reference in the existing evidence;
orientation is compared after conversion (original raw angle, application yaw).
