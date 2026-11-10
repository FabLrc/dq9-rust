# melonDS oracle: save states (`.ml1`)

Module `dq9_formats::melonds`, command `dq9re state <file>` (the ROM, if found, is used to
compare resident code), script `scripts/melonds-state.ps1` to produce a state.

Purpose: read the memory of a game running in melonDS to compare it with what our
reimplementation computes (trigger state, party, position, progress). The format is the
melonDS 1.x one ("`MELN`"); its description comes from
[ndskit](https://github.com/vs-sr-dev/ndskit) (`melonds.py`, MIT, checked by its author on a
2025 version): **reimplemented, not copied**; the melonDS code (GPL) was not read.

## Format

A 16-byte header (`MELN` then version and size), then consecutive sections: 4-byte name, u32
length **including the 16-byte header**, 8 more header bytes, then the data. Only these are
used:

| Section | Offset from the start of the section header | Content |
|---|---:|---|
| `NDSG` | `0x14` | main RAM (4 MiB used; the section reserves 16 for the DSi) |
| | `0x14 + 0x1000000` | shared WRAM (32 KiB), then ARM7 WRAM (64 KiB) |
| `CP15` | `0x1c` | ITCM (32 KiB), then DTCM (16 KiB) |

A truncated state, or one without long enough `NDSG`/`CP15`, is rejected (never a panic).

## Useful addresses

| Address | Meaning | Source |
|---|---|---|
| `0x027ffc3c` | SDK VBlank counter: used to align a state and an execution on the same frame | ndskit |
| `0x020f33d8` | `GameState` instance: the value is the literal word read by `GameState::GetInstance` (`0x0200f398`) | European ARM9, checked in RAM |
| `GameState+0x5cb0 / 4 / 8` | chapter, step, substep (`Save_Chapter`, `Save_ScenarioStep`, `Save_ScenarioSubstep`) | `re/symbols.txt` |
| `0x02108844`, `0x02109418`, `0x020fb3f0` | `g_QuestManager` (event block), `g_QuestLog`, `g_Zone3D` | `save.md` § "In RAM" |
| `GameState+8` | table of 0xe9 game object pointers (party, NPCs, monsters): `Chara_Get` (`0x0200fd80`) | European ARM9; `GameState.h` from the USA decompilation |
| `GameState+0x3ac` | index of the protagonist's object (`GameState::GetProtagonist`, `0x0200fddc`) | same |
| object `+0x44` | world position, 3 × fx32 (`Object3D::position_`) | USA decompilation (`Object3D.h`), checked by moving the hero |

`State::read(addr, len)` addresses main RAM (mirrored every 4 MiB), DTCM at `0x027e0000` and
ITCM (mirrored every 32 KiB); a read does not span two memories.

## Usage

```powershell
cargo dq9re state game.ml1                         # sections, VBlank, scenario, code vs ROM
cargo dq9re state game.ml1 --save game.sav         # + RAM compared with the loaded save
cargo dq9re state game.ml1 --peek 020f33d8:40      # bytes in hexadecimal (address:length)
cargo dq9re state game.ml1 --json                  # story checkpoint (JSON, see below)
cargo dq9re state --ram ram.bin --json             # same read on a 4 MiB RAM image
cargo dq9re states index build/oracle --out build/oracle/index.json
cargo dq9re states find --story 1.1.2 --map 4101   # catalog: which files, without opening them
```

### Neutral state: main RAM image

`--ram FILE` reads a **raw main RAM image**: exactly 4 MiB, based at `0x02000000`, no header
(what `scripts/melondsds_host.py --dump-ram` writes, and what
`retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM)` exposes). A path passed without `--ram` is
also accepted: a file that does not start with `MELN` and is exactly 4 MiB is read as a RAM
image (content decides), which lets `scripts/oracle-compare.ps1` work on a dump without
converting it.

Both containers share the same memory view (`dq9_formats::melonds::Memory`): `read`, `u32`,
VBlank, scenario, event block, map, quests, objects and positions exist only once, so
`--json` and `--peek` give **the same result** on a `.ml1` and on its RAM image (checked on
first player control: `--peek 020f33d8:10` identical byte for byte). A RAM image contains no
ITCM, DTCM or overlay: reads in those memories fail, and `dq9re state` says so instead of
comparing resident code; `--save` remains available (all `SAVE_COPIES` copies are in main
RAM).

### Catalog (`dq9re states`)

`states index DIR [--out index.json]` reads each file in the directory (not subdirectories):
a `.ml1` is recognized by `MELN`, a RAM image by its exact 4 MiB, everything else is listed in
`invalid` with its error instead of failing the scan. Each entry gives `file`, `kind`,
`vblank`, `map`, `area`, `track`, `story` (`CHAPTER.STEP.SUBSTEP`), `flags` (number of global
flags) and `hero` (`x`, `y`, `z`, `yaw` in world units). A core resume state
(`retro_serialize`, 19 MiB) also starts with `MELN` and is therefore read like a `.ml1`.

`states find [--story 1.1.2] [--map 4101] [--index index.json]` filters this catalog
(`build/oracle/index.json` by default) and prints one line per state: it answers "which state
is at 1.1.2?" without loading each file. Real example, on the states kept from 2026-11-06:

```
$ cargo dq9re states find --index build/oracle/index-preserved.json --map 4105
1.1.1     savestate map  4105 area  4100  (0.660, -0.250, -3.610) flags 18  vblank 31578    bat10.ml1
1.1.1     savestate map  4105 area  4100  (0.660, -0.250, -3.610) flags 18  vblank 23782    bat4.ml1
1.1.1     savestate map  4105 area  4100  (0.660, -0.250, -3.610) flags 18  vblank 26379    bat6.ml1
3 state(s) in …/build/orca/oracle
```

With the ROM, `dq9re state` compares the static ARM9 module, each autoload block (ITCM, DTCM)
and each overlay against the ROM content, and lists the overlays present (≥ 90% identical
bytes; overlays at the same address are never that similar). `ov_003` present = in game;
`ov_004` = title screen.

`--save` compares each copy of `melonds::SAVE_COPIES` (where loading a normal save puts its
bytes, `save.md` § "In RAM") with the current slot of the `.sav` and reports differences; two
copies are known to change during play ("changed by the game"). It also compares each party
member with its object in RAM (`State::compare_party`, fields of `save.md` § "Party members").
Several unexpected differences = the game was not loaded from this save (on the title screen:
no member object has the right ids).

To drive the game (movement), first bind keys in `melonDS.toml` (`[Instance0.Keyboard]`, Qt
codes: `Up = 16777235`, `Down = 16777237`, `Left = 16777234`, `Right = 16777236`, `A = 88`
(X), `B = 90` (Z)); `WM_KEYDOWN`/`WM_KEYUP` posted to the window are then enough, without
foreground. Modifiers (Shift+F1) do not get through this way.

### Producing a state

```powershell
powershell -ExecutionPolicy Bypass -File scripts/melonds-state.ps1 -Save build/sav/demo.sav -Out build/melonds/demo.ml1
```

The script (~40 s) prepares a portable melonDS in `build/melonds/` (copy of
`tools/melonds/melonDS.exe`, `portable` file, ROM as hard link, copy of the `.sav`), presses
START on the title screen (detected by the button color), then "Continue your adventure"
**until the screens change**, waits `-Settle` seconds (8 by default) in game, saves state 1
via *File → Save state → 1*, closes melonDS (`-Keep` leaves it open) and runs
`dq9re state --save`. Inputs are messages posted to the window: it can stay behind other
windows (Windows refuses foreground to a script while the user works elsewhere, which made
Shift+F1 unreliable). It assumes the default window (scale 1, screens stacked vertically).
melonDS 1.1 binds no DS key by default: to play with the keyboard, set them once in
*Config → Input and hotkeys* (kept in `build/melonds/melonDS.toml`). The melonDS 1.1 GDB stub
(enabled in the `.toml`) does listen on 3333 but dropped the connection on the first packet of
a minimal RSP client under Windows; not investigated, states are enough.

## Verification

On real states produced from the demo save (`save.md`, melonDS 1.1, direct boot, free BIOS):

- The ndskit offsets are correct: static ARM9 module (`0x02000000`, `NDSG` main RAM) and ITCM
  (`0x01ff8000`, `CP15`) 100% identical to the ROM; DTCM at 92.7% (the rest is written at run
  time); loaded overlays 100% identical (`ov_003`, `ov_011`, `ov_017`, `ov_023`, `ov_032`,
  `ov_033` in game).
- `GameState` is indeed at `0x020f33d8`, and all `SAVE_COPIES` copies are found byte for byte
  (gold, bag, bestiary, alchemy, zone, treasure maps…), except the two areas the game modifies
  (`save.md` § "In RAM"). The differences are the same from one state to the next.
- VBlank counter consistent with 60 frames/s (~2,100 after ~35 s).
- Positions: on arrival in the Stornway church (`C01M09`, id 109, identified by
  `dq9re maps --state`), the 4 party objects (indices 0 to 3, hero = 0) are at
  (0.210, 0.120, −1.380). After ~1 s down then ~0.6 s left (posted keys), the hero is at
  (−2.853, 0.106, 3.854) and the other three follow in a line; `dq9re state` displays them.
  Objects are allocated on the heap (`0x022bbbdc`, then every `0x116c0`), hence going through
  the table. The table index is the party order id (`save.md` § "Party members"): on the demo,
  order `[0, 3, 2, 1]`, and id 3 (2nd character) points to the 2nd allocated object.
- The save vector copied to `g_Zone3D+0x2774` (−57, 0.1, −31.5) is not the hero's position in
  the church: that is the map's starting point (`.bmbl`, record `0x6e`), `save.md`
  § "Position on resume". Automated comparison: `dq9-core/tests/real_rom.rs` (`DQ9_SAVE`,
  `DQ9_STATE`).

**Correction**: the first version of the script sometimes lost the "Continue" confirmation
and saved the **title screen** (`ov_004`); scenario 19/2/1 is already set there by reading the
adventure log, which made it pass for a loaded game. Gold and the other blocks are not there:
hence the `--save` comparison, which tells the two cases apart (15 differing copies on the
title screen).

The tests (`melonds::tests`) use a synthetic state built according to this format.

## Differential oracle (v0.1)

Goal: compare the story state computed by the reimplemented runtime with the real game
(melonDS), at the **same points** — progress of the five tracks, their two flag sets, global
flags, quests — plus the map and the hero position. The v0.1 criterion
([`roadmap.md`](../roadmap.md), J3) is strict: same flags at the same points; it cannot be
inferred from `--pilot` alone nor from a fabricated save.

### Checkpoint (`dq9re state --json`)

The event block is read in RAM at `g_QuestManager` (`0x02108844`): same layout as the save
(`save.md`, § "Event block"), so tracks at `+0`, global flags at `+0x8c`, quests at `+0x2cc`,
current track at `+0x332`. The map comes from `Zone3D+0`/`+4`, the frame number from the
VBlank counter (`0x027ffc3c`).

```json
{"vblank":26379,"map":4105,"area":4100,"track":0,"daytime":true,
 "tracks":[{"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[1]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]}],
 "flags":[337,1450,...],"quests":{}}
```

`flags` and a track's two sets are **bit indices** (least significant bit first,
`Flags_Test`); `quests` lists only non-zero nibbles. Without `--json`, the same thing is
printed as text, with the position of the party objects.

### Diff of two states (`dq9re state diff`)

```powershell
cargo dq9re state diff bat6.ml1 bat4.ml1          # same story point -> code 0
cargo dq9re state diff pro1.ml1 bat6.ml1          # exit 1: five named differences
cargo dq9re state diff bat6.ml1 ram23050.bin --json
```

Both inputs may be a `.ml1` or a 4 MiB main RAM image (`--dump-ram`): the type is recognized
by the `MELN` header or by size, never guessed. The reader is the one of `dq9re state`
(`Memory`), so the same addresses and mirrors apply.

The first block of the report names the **game** differences: map and area, current track,
story (`chapter.step.substep`), time of day, hero pose (position and `fx32` angle), progress
and both flag sets of each of the five tracks (bits added/removed), global flags
(added/removed) and quests (modified nibbles). `--json` gives them in `differences`, one typed
entry per field (`{"field":"global_flags","change":{"added":[…],"removed":[…]}}`).

The second block summarizes **the other bytes** of main RAM, outside the event block (already
described above): number of modified bytes, then ranges `start`, `length`, `changed`. Equal
bytes separating two changes less than 16 bytes apart merge into a single range, so a
structure reads as one block; the `changed` count stays exact. In text, only the first 200
ranges are printed (`--json` keeps them all). The VBlank counter, clocks and heap show up
there: these are transient differences, not game differences.

Exit codes: **0** if the first block is empty (identical game state, even if transient bytes
differ), **1** if at least one game field differs, **2** if an input is unreadable or is
neither a melonDS state nor a 4 MiB image. This is what the 2026-11-06 smoke checks:
`bat6.ml1` against `bat4.ml1` (same first player control, different frames) gives 0 and 136
transient ranges; `pro1.ml1` against `bat6.ml1` gives 1 with map, area, pose, `step_flags` and
global flags named.

Limit: the diff sees the bytes that change and the fields they make up; it does not say
**who** wrote them. A flag set directly outside `Flags_Set` is therefore seen, its author is
not (see `state watch` below for frame-by-frame provenance).

### Frame-by-frame tracing (`--watch` + `dq9re state watch`)

The libretro harness (`scripts/melondsds_host.py`, § "libretro harness") can watch main RAM
bytes and write only their changes:

```powershell
python scripts/melondsds_host.py --rom <ROM> --frames 12000 `
    --input build/oracle/inputs/prologue.jsonl `
    --watch 0x02108844:0x334 --watch-out build/oracle/watch.jsonl
cargo dq9re state watch --file build/oracle/watch.jsonl
cargo dq9re state watch --file build/oracle/watch.jsonl --json
```

`--watch ADDR:LEN` is repeatable, in hexadecimal, and overlapping areas are merged. Each
modified byte produces a line `{"frame":N,"addr":"0x...","old":0,"new":1}`; `frame` counts the
frames executed since start, the first snapshot being taken before frame 1. Only watched areas
are copied each frame (a few KiB), not the 4 MiB.

`dq9re state watch` names each change when the address falls in the event block
(`g_QuestManager`, `0x02108844`): global flag bit, bit of one of a track's two sets,
chapter/step/substep, quest nibble, current track. Other addresses stay RAM bytes named by
their address. The text gives one line per change and a total; `--json` returns an array of
typed objects (`kind`: `global_flag`, `track_flags`, `track_progress`, `quest`,
`current_track`, `ram`). An invalid line makes the command fail (code 2) without inventing
anything.

Example output on the event block:

```text
frame 1450: global flag 1450: 0 -> 1
frame 7401: global flag 1451: 0 -> 1
frame 9000: track 0 step_flags flag 1: 0 -> 1
frame 9001: current track: 0 -> 1
```

Limits: the log sees the **change**, not the instruction that wrote it; for the `LR` of the
write, DQIX Enhanced's `DQIX_WATCH` is the documented lead in [`tools.md`](tools.md). A byte
that changes several times between two frames keeps only the last value; `--checkpoint-every`
and `--dump-ram` give the full state at chosen frames.

### Runtime side

`dq9-app --new-game --headless --auto` stops at first player control and prints the log then,
for each track, `track i: c.s.b flags [{...}, {...}]`, `global flags: {...}` and the hero
position. `--pilot` now continues with real walking and interactions, without teleporting:
the verified route reaches 1.1.2 in `X01M01`, not yet chapter 2. `--checkpoints FILE` writes
the JSONL (same field names plus `tick`/`seconds`/`kind`/`event`/`hero`); `-RuntimeJson`
compares one object from that file. `--trace-inputs FILE` and `--replay FILE` reproduce the
driver's route with camera angle 0; this evidence is neither a manual-camera replay nor
melonDS parity.

### Comparator

```powershell
# text report: the report does not name the current track, it must be given
powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 `
    -State build/melonds-oracle/observatory.ml1 -Report build/oracle/runtime-auto.txt -TrackIndex 0
# runtime JSONL contract: a single object (one line of --checkpoints)
powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 `
    -State build/melonds-oracle/observatory.ml1 -RuntimeJson build/oracle/checkpoint.json
```

It runs `dq9re state --json` on the state, then **strictly** compares the map, the current
track index, the number of tracks, the progress and both flag sets of each track, the global
flags, and — only when `-RuntimeJson` provides them — `daytime` and the quests (the text
report does not contain them). A current track missing from the text report **or from the
JSON object** is a difference (non-zero code), never an `OK`: `-TrackIndex` or `-RuntimeJson`
provides it. Non-zero quests on the melonDS side with no quests on the runtime side are also a
difference. Non-zero exit code and one `DIFF` line per difference; empty sets are valid (no
truthiness test on a set).

### Driving melonDS (`scripts/melonds-play.ps1`)

The script copies `melonDS.exe` into `build/melonds-oracle/` (with `portable`, a hard link to
the ROM, `dq9.sav`), writes a valid `melonDS.toml` (no BOM) with the keys
(`Up`/`Down`/`Left`/`Right`, `A = X`, `B = Z`, `Start = Retour`), then posts messages to the
window (no foreground, window not resized). Actions: `waitstart` (orange START button), `tap`,
`key`, `hold:up+left:2`, `spam`/`play` (A every N seconds, states every M), `save`,
`loadstate`, `shot`, `pause`/`frame:N` (melonDS hotkeys, see limits). Reference new-game
example:

```powershell
# title screen -> menu -> character creation (A x10, touch OK, "Yes") -> prologue
powershell -ExecutionPolicy Bypass -File scripts/melonds-play.ps1 -Fresh -Actions `
  "waitstart,tap:127:377,wait:8,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,tap:221:144,wait:3,key:a,wait:8,play:260:2:20:pro,shot:end"
# resume from a state (File > Load state > 1) then battle/cutscenes
powershell -ExecutionPolicy Bypass -File scripts/melonds-play.ps1 `
  -LoadState build/melonds-oracle/pro11.ml1 -Actions "wait:3,loadstate,wait:2,play:240:1.5:20:bat"
```

Character creation is real (no fabricated save): defaults are accepted, the name is one
character typed on the touch keyboard, then "Begin your adventure"; battle 26 is played (A)
and won normally. States are stable story points, not exact frames.

### melonDS DS core harness (libretro)

Standalone melonDS requires a focusable window and its GDB stub disconnects; the official
[melonDS DS](https://docs.libretro.com/library/melonds_ds/) core runs the ROM through
callbacks, without window or posted messages. `scripts/melondsds_host.py` calls it through its
C ABI (ctypes): `retro_set_environment`/`video_refresh`/`audio_sample_batch`/`input_state`,
`retro_run` **one frame at a time**, `retro_serialize`/`retro_unserialize`,
`retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM)`. The core (GPL, outside the repo) and the ROM
stay outside the worktree: `--core tools/build/melondsds_libretro.dll` (reference archive,
SHA-256 `b2873cc3…`), `--rom` or `DQ9_ROM`.

```powershell
python scripts/melondsds_host.py --rom <ROM> --core <melondsds_libretro.dll> `
    --input run.jsonl --frames 23050 --checkpoints build/oracle/checkpoints.jsonl `
    --dump-ram build/oracle/ram.bin --state-out build/oracle/core.bin --shot build/oracle/control.png
```

Options: `--rom`, `--core`, `--sysdir`, `--savedir`, `--input`, `--frames`, `--checkpoints`,
`--checkpoint-ram-dir` (keeps the RAM images of each point), `--checkpoint-every N`, `--dq9re`
(binary, otherwise `target/release` then `target/debug`), `--dump-ram` (4 MiB),
`--state-out`/`--state-in` (core state, so a real run can be resumed), `--shot`,
`--shot-every N`/`--shot-dir`, `--core-log` (core log, otherwise discarded). The firmware is
the core's (`melonds_sysfile_mode = builtin`), no BIOS is required.

Inputs (`--input`, JSONL, one object per line, **increasing by frame index**, held until the
next line that changes it — so a sparse file is enough, and one line per frame gives
frame-by-frame control):

```json
{"frame": 1300, "pad": ["start"]}
{"frame": 1310, "pad": []}
{"frame": 1700, "touch": [128, 305]}
{"frame": 1710, "touch": null, "checkpoint": "creation"}
{"frame": 2200, "shot": "build/oracle/creation.png"}
```

- `frame`: number of `retro_run` calls already made (omitted = previous line + 1);
- `pad`: `a b x y l r start select up down left right`, replaces the held set (`[]`
  releases);
- `touch`: `[x, y]` in **image pixels** (256×384, top screen then bottom; the harness
  converts to libretro's pointer range `-0x8000..0x7fff`, without which the core receives no
  press), or `null` to lift the stylus;
- `checkpoint`: captures a point before the frame runs (`true` or a `kind` name);
- `shot`: PNG before the frame runs.

`--checkpoints` writes the JSONL of the `dq9-app --checkpoints` contract: `tick` (frame index),
`seconds` (`tick/60`), `kind`, then the `dq9re state --json` fields (`vblank`, `map`, `area`,
`track`, `daytime`, `tracks`, `flags`, `quests`, `hero`), `event` staying `null` (the original
does not expose the cutscene id). Each point goes through `dq9re state --ram` on a temporary
RAM image: **a single** implementation of the offsets, and `--checkpoint-ram-dir` keeps them
for the catalog or for `oracle-compare.ps1`.

Limits: the captured video frame is that of the **last** executed frame; only joypad and
stylus are provided (no mic, no Wi-Fi, no set RTC). Looping A does close messages, but it can
**reopen** a repeatable NPC conversation once the cutscene is over: story progress then stays
identical, it is not a message window lock (evidence on 2026-11-07 below).

### State resume, message window and touch coordinates (2026-11-07)

`--state-in` exactly resumes a file written by `--state-out` (same core): the milestones of a
run split into segments reproduce in a single continuous run (§ "Continuous original run"
below). The harness also exposes main RAM, so the message window state is read **without a
screenshot**: `W = 020421b0()`, i.e. `u32[u32[0x020421bc] + 0x1c]`; on 2026-11-07,
`W = 0x021152f4` in both states `build/orca/route/idle-state.bin` and
`build/v01-20261107/oracle/end1600-state.bin`.

| Field | Meaning |
|---|---|
| `W+0x998` | window active (1) or closed (0) |
| `W+0x9a0` | `MsgWin_Update` wait state (0 waits for key, 3 `<ADD>`, 5 typing, 6 choice) |
| `W+0x19bd` | byte returned by native 405 (`get_message_state`): 1 as long as the message is not closed |
| `W+0x912` | 3 = wait arrow shown; 5 alternates during typing |
| `W+0x954` | answer to the last `YESNO`: −1 no answer, 0 yes, 1 no |

Measurement: from `idle-state.bin`, an A at frame 61 makes `W+0x998` and `W+0x19bd` go from 1
to 0 at frame 67; the cutscene then displays its next message at frame 110 and ends at frame
238. The A press is therefore received and the text closes.

**False dead end after 1160.** The message "Stop! Benevolence is not a toy!" does not belong to
event 1160: it is entry 169 of `data/event/ev01600/ev01600_en.bin` (keys 168, 169, 170), the
Observatory cutscene where a companion asks to receive benevolence
(`data/scenario/X01A0/130_en.bin`, NPC 130 of `X01M07` at (−9.27, 0.25, 1.16)). Once 1160 is
over, the hero is in free control; looping A reopens the repeatable conversation of NPC 130,
whose cutscene 1600 replays (the "Yes" choice then 168 → 169 → 170) every 60 to 80 frames,
and the story stays at 1.1.1. No state was stuck: one had to **stop pressing A and walk**.
Evidence: an A at frame 61 closes the window and leaves free control at frame 238;
`--watch 0x021152f4:0x1a00` shows 998 going back from 0 to 1 at each new conversation.

**Touch coordinates.** `--input` expects `[x, y]` on the **stacked 256 × 384** image (top then
bottom), as documented above; the touch screen is the bottom part, so `y` must be in
`192..383`. Earlier attempts `[128, 160]` targeted the top screen and could not reach the
stylus; they prove nothing about text speed or a touch wait.

### Continuous original run: 1160 → 1170 → 1.1.2 (2026-11-07)

Inputs and evidence (ignored by git): `build/v01-20261107/oracle/`. The run starts from
`build/orca/route/idle-state.bin`, closes cutscene 1600, walks to `X01M01`, talks to NPC 127
(trigger `room 4101, event 0 : 0x6[?](127) 0x4[?](3) 0x5[?](4) → 0x77(1170)`), plays 1170,
then stops at the first frame of 1.1.2.

```powershell
python scripts/melondsds_host.py --rom <ROM> --core tools/build/melondsds_libretro.dll `
    --state-in build/orca/route/idle-state.bin `
    --input build/v01-20261107/oracle/continuous.jsonl --frames 3966 `
    --checkpoints build/v01-20261107/oracle/continuous-cp.jsonl `
    --state-out build/v01-20261107/oracle/continuous-state.bin `
    --dump-ram build/v01-20261107/oracle/continuous-ram.bin `
    --shot build/v01-20261107/oracle/continuous-112.png
cargo dq9re map X01M01 --state build/v01-20261107/oracle/continuous-state.bin
cargo dq9re state build/v01-20261107/oracle/continuous-state.bin --json
```

Run landmarks (frame, hero pose, map):

| Frame | Pose | Point |
|---:|---|---|
| 800 | (−8.275, 0.169, 1.257) | free control after 1600, `X01M07` 1.1.1 |
| 1100 | (−8.275, 0.169, 5.957) | straight south descent |
| 1200 | (0.985, 0.169, 5.957) | passage to the east |
| 1440 | (1.177, 0.169, 6.5) | south wall |
| 1940 | (−7.32, 0.169, 14.98) | west then south corridor |
| 2200 | (0, 4.25, 31.72) | entering `X01M01` through the `X01M07` door |
| 2590 | (−3.509, 7.843, 28.514) | south-west ramp |
| 3010 | (2.704, 11.249, 22.103) | upper platform |
| 3230 | (10.125, 11.249, 15.937) | east side of the platform |
| 3400 | (1.68, 11.249, 21.919) | back to the center |
| 3480 | (1.303, 11.249, 18.651) | A in front of NPC 127 |
| 3571 | (1.303, 11.249, 18.651) | message "Apus Major has sent you…" |
| 3965 | (2.77, 11.25, 18.19) | **first frame of 1.1.2** |

At frame 3965, `dq9re state --json` gives map `4101`, area `4100`, track 0,
`chapter/step/substep = 1.1.2`, `step_flags {1,2,3,4}`, global flags unchanged
(`1450–1454,1461,2721,2751,2774,3191,3775,3955,4046,4135,4283,4410,4414`), hero
(2.77, 11.25, 18.19), heading 1.888.

The 1170 end trigger runs `0x68(4)` (track flag), `0x95(0, 0x100000)` then `0x84(0,1,1,2)`
(advances to 1.1.2). In the original, `dq9re map X01M01 --state` names the effect of `0x95`:
on `continuous-state.bin`, "block 0 (X01M0100.bmdj): instance 16 hidden"; on
`talk127b-state.bin` (1.1.1, before 1170), "the loaded blocks match the assembly". Instance 16
is therefore indeed **hidden at 1.1.2** by the game: a runtime that ignores `0x95` keeps the
thin wall S1 and cannot claim the same state.

**3965 is a progress boundary, not player control.** The hero stays locked about 60 to 80
frames after the track advance (no direction moves him at frame 3965); this is why the
progress checkpoint must not be compared with a runtime player-control point.
`control112-state.bin` is the **controllable** state after 90 frames without input: same
pose, message window closed, and `Left` for 40 frames moves the hero to
(−0.875, 11.65, 17.853).

#### Wall S1 and tower `X01M01` → `X01M03` → `X01M09`

The hidden piece 12 is `X01A01S1.col2` (2 triangles, `184` bytes): a thin wall at
x `0.0186..0.1055`, y `9..14`, z `15.42..18.99`, in map coordinates.

- **1.1.1 (wall present)**: from `seg11-state.bin` (1.68, 11.25, 21.19), `Up` 44 frames then
  `Left` stops at (0.947, 11.25, 18.557); 60 more frames of `Left` no longer move the hero.
- **1.1.2 (wall hidden)**: from `control112-state.bin`, `Left` crosses x = 0.9 and the hero
  climbs: (−0.144, 11.459, 17.944) at frame 4096, then (−7.432, 13.727, 16.402) when the map
  switches to `4103` (`X01M03`). He arrives at (−9.75, 4.25, 17.62), then `Up` reaches `4109`
  (`X01M09`) at frame 4376 at (−9.236, 4.25, 11.727). The map id changes a few frames before
  the pose moves: the stable arrival point is (−9.75, 4.25, 17.62), to be compared after the
  transition.

Extended continuous run (`continuous2.jsonl`, same start `idle-state.bin`):

```powershell
python scripts/melondsds_host.py --rom <ROM> --core tools/build/melondsds_libretro.dll `
    --state-in build/orca/route/idle-state.bin `
    --input build/v01-20261107/oracle/continuous2.jsonl --frames 4416 `
    --checkpoints build/v01-20261107/oracle/continuous2-cp.jsonl `
    --state-out build/v01-20261107/oracle/continuous2-state.bin `
    --dump-ram build/v01-20261107/oracle/continuous2-ram.bin `
    --shot build/v01-20261107/oracle/x01m09-4415.png
```

| Frame | Map | Pose | Point |
|---:|---:|---|---|
| 3965 | 4101 | (2.77, 11.25, 18.19) | 1.1.2, control still locked |
| 4055 | 4101 | (2.77, 11.25, 18.19) | real control, message closed |
| 4096 | 4101 | (−0.144, 11.459, 17.944) | crosses the hidden wall S1 |
| 4196 | 4103 | (−7.432, 13.727, 16.402) | map `X01M03` (pose not yet moved) |
| 4236 | 4103 | (−9.75, 4.25, 17.62) | stable arrival in `X01M03` |
| 4376 | 4109 | (−9.236, 4.25, 11.727) | `X01M09` |
| 4415 | 4109 | (−7.36, 0.199, 10.22) | end of the replayed run |

SHA-256 fingerprints: `continuous.jsonl` `3d0fd18d…`, checkpoints `469404ea…`, state
`496fec9e…`, RAM image `cc30d043…`, screenshot `fd0c728e…`, `idle-state.bin` `cad28ef0…`,
1.1.1 state `talk127b-state.bin` `efd0ff3e…`, `continuous2.jsonl` `76e11b30…`, checkpoints
`d0ef2a09…`, state `11411a78…`, RAM image `be2765ce…`, `control112-state.bin` `3714213d…`,
`X01M09` screenshot `b85b8730…`.

#### Return doors and limit `X01M01` → `X01M05` (2026-11-07)

The `X01M01` doors (`dq9re doors X01M01`) place two distinct volumes below the arrival
platform (0, 4.25, 31.72):

| Door | `0x73` (volume) | Destination |
|---|---|---|
| `X01M05` | `(0, 1, 35)`, half-sizes `2.5 × 3 × 2` | `X01M05` (2.15, 22.4) |
| `X01M07` | `(0, 5, 30.5)`, half-sizes `3 × 2` | `X01M07` (0.17, 16.9) |

From the arrival (0, 4.25, 31.72), `Down` for 40 frames stops at (0, 4.25, 34.5) without a map
change; the height does not go down to 1 and `Up`/`Left`/`Right` only come back or slide. The
`X01M05` volume is therefore **below** the arrival platform: it is not a same-level path to
(0, 1, 34), and arriving through the `X01M07` door (0.17, 16.9) is another path. The observed
physical path climbs `X01M01` up to `X01M03`/`X01M09`; the low return (y = 1) is not yet
obtained in the original and remains **[unknown]**, not to be inferred from a mere X/Z.

**Native segment replayed as is: falsified.** On 2026-11-07, the coordinator provided
`build/v01-20261107/oracle/native-4103-to21510-relative.jsonl` (57 pad changes, A, 828
frames) starting from `X01M03` (−18.62, 4.2, 4.25), appyaw −2.374. The original is placed at
the same spot by `m03-westentry-state.bin` (−18.408, 4.25, 4.216). Replayed **as is**, the
native stream gives at its relative frame 541 (ours 546 after 5 frames of `Down+Left` to align
the heading) `X01M03` (−11.4, 4.25, 11.7), **not** `X01M04` (0, −0.01, 19.62): divergence
starts around frame 120. The native pads assume the runtime camera (yaw 0); those of the
original do not transpose directly. **Native frame-by-frame positions** (or world waypoints)
would be needed, not just the pads, to redo the physical path. Fingerprints:
`m03-westentry-state.bin` `43c5b351…`, `native-replay.jsonl` `922b1077…`,
`native-replay-cp.jsonl` `81e8509e…`, `native-replay2.jsonl` `94bfc6f8…`,
`native-replay2-cp.jsonl` `975fa115…`.

### Measurement of 2026-11-06 (EU ROM, melonDS 1.1, free BIOS)

Reference: real new game, battle 26 won, states taken every 20 s. The first player control at
the Observatory gives (vblank 26379, `bat6.ml1`):

| | melonDS | runtime `--new-game --headless --auto` (210.42 s) |
|---|---|---|
| map | `4105` (`X01M05`) | `4105` |
| hero | (0.660, −0.250, −3.610) | (0.6599, −0.25, −3.6099) |
| story | 1.1.1, track 0 | 1.1.1, track 0 |
| `step_flags` track 0 | `{1}` | `{1}` |
| global flags | 18 | 1 (`337`) |

Identical: map, position (within 10⁻³), progress and track flags. **Measured difference**: 17
global flags present in melonDS and missing from the runtime —
`1450,1451,1452,1453,1454,1461,2721,2751,2774,3191,3775,3955,4046,4212,4283,4410,4414`;
`337` is shared. `GameState::new_game` leaves the event block empty (`state.rs`, "the game's
initial tracks are not read"): the difference therefore covers new-game initialization,
cutscenes 21500–21596 and the battle 26 victory (substituted on the runtime side). Reported to
the flags and route workers with the state and the comparator; accepted as ground truth, not
fixed here.

**Since then (November 6, 2026), the causes are wired**: initial equipment (item
`0xc76 + compendium`), loading `S14M05` before the prologue, `0x72(22159)`/`0x6b` from the
`21596` end trigger, held key. The runtime sets 17 flags; only the random loot of battle 26
(`4135` medicinal herb, or `4212` slimedrop depending on the run) is still missing
([`trigger.md`](trigger.md), [`item.md`](item.md)).

During the opening (`M01M12`, 1.1.1) the same 7 base flags are already set in the first state
(vblank 5217) and `1450`, then `1451`, appear during the cutscene: these are
"initialization/scripts" candidates, not track flags.

The runtime delivered its checkpoints (`--checkpoints`, JSONL, same field names). Landmarks
during the opening: **the original states below are not aligned with the runtime's event
ends**. The same 1.1.1 progress is not enough to establish that they describe the same instant
of the cutscene; this table does not validate checkpoint parity.

| point | runtime | melonDS |
|---|---|---|
| end of `21500` (tick 4837, `M01M12`, 1.1.1) | `[]` | `pro1`: `2721,2774,3191,3775,3955,4046,4414` |
| end of `21507` (tick 7401, same map) | `[]` | `pro7`: the 7 + `1450,1451` |
| first player control (tick 12625, `X01M05`) | `[337]` | `bat6`: 18 flags |

Part of the difference therefore exists from the opening, before battle 26; the origin of the
other flags remains to be established. The runtime then clears `337` at `X01M07` (tick 18482);
no melonDS state at that spot to settle it.

**Getting past first player control (real recording, reproducible).** From
(0.660, −0.250, −3.610): `Up` 8 s (stairs), `Left` 4 s → (−16.45, 2.25, −17.47), `Down` 4 s
(balcony) → (−16.45, 2.25, 5.75), `Right` 1.1 s + `Down` 0.3 s → (−10.77, 2.25, 8.16), then A:
event 1140 plays with NPC 111, at (−11.27, 2.25, 9.04). The conversation closes after **8** A
presses (the 9th reopens it); the left balcony stops at z = 9.5, NPC 105 is below at
(−18.5, 0.45, 20.31). These coordinates are shared with the route worker; the rest (tower
`X01M03`, warp y = 15.5) remains to be crossed by real walking.

#### First player control via the libretro core (2026-11-06)

Real run replayed by `scripts/melondsds_host.py` (EU ROM, core 1.4.0, built-in firmware):
title screen (`start` at frame 1300), menu, character creation, "Yes", then A every 24 frames
until frame 23050. The input file has 1698 lines; the pad presses are `start` (10 frames) then
`a` (6 frames every 24), and a single touch press (`[128, 305]`, the "Female" line, frames
1700–1710). The creation fields are therefore left at their current values, except the
gender, and the name becomes `11111` (one A per character). Command:

```powershell
python scripts/melondsds_host.py --rom <ROM> --core <melondsds_libretro.dll> `
    --input build/tmp/in11.jsonl --frames 23050 `
    --checkpoints build/oracle/checkpoints.jsonl --dump-ram build/oracle/ram23050.bin `
    --state-out build/oracle/core23050.bin --shot build/oracle/control.png
cargo dq9re state --ram build/oracle/ram23050.bin --json
```

| | `bat6.ml1` (melonDS 1.1) | core RAM image (vblank 23033) |
|---|---|---|
| map, area | `4105`, `4100` | `4105`, `4100` |
| story, track | 1.1.1, track 0 | 1.1.1, track 0 |
| `step_flags` track 0 | `{1}` | `{1}` |
| hero (fx32 → world) | (0.660, −0.250, −3.610), heading 4.86 | (0.660, −0.250, −3.610), heading 4.86 |
| global flags | 18 | 18 |

Identical: map, area, progress, track flags, hero position and heading (same fx32), and the
number of global flags. `scripts/oracle-compare.ps1` on the RAM image and this checkpoint
answers `OK: story point and flags identical`; against `bat6.ml1` it reports only **two**
differences, one bit on each side:

```
DIFF global flags only in melonDS: 4212
DIFF global flags only in the runtime: 4135
```

`4135` is not a constant: two other real runs, identical except for the hero's gender (no
touch press, or a press on "Male"), have **17** flags, with neither `4135` nor `4212` — member
0's appearance there is byte for byte that of `bat6` (`cf32573f40232823…`). `4212`, however,
is present in all three `bat*.ml1` states (18 flags, vblank 23782/26379/31578) and already in
`pro11.ml1` (vblank 18384, map 20101), so it is set early in the prologue of the standalone
run: the branch that sets it is not yet established and **no flag was forced** to make it
match. `bat6` was also taken 3346 frames (VBlank) later than the frame above, in the same
cutscene: the two points are not the same instant.

### Field states with roaming monsters (2026-11-10, `scripts/melonds-field.ps1`)

```powershell
powershell -ExecutionPolicy Bypass -File scripts/melonds-field.ps1 -Out build/field
$env:DQ9_FIELD_DIR = (Resolve-Path build/field/run).Path; $env:DQ9_FIELD_REPORT = 1
cargo test -p dq9-core --test encounter_rom -- --nocapture   # reads the run, writes roamers-*.txt
```

What the script does, and what was learned doing it:

- **The save reaches the core through `SAVE_RAM`.** `scripts/melondsds_host.py` boots the ROM but
  does not load a `.sav`: a `demo.sav` placed in the save directory (even under `melonDS DS/`) is
  ignored and the title screen offers only "Create a new adventure log". The core exposes the
  save as `retro_get_memory_data(0)` (65536 bytes, the save's size): copying `demo.sav` there right
  after `retro_load_game` makes the title menu offer **"Continue your adventure"** with the
  adventure log (party level 91, Stornway). The script imports the host as a module and does
  exactly this; nothing else is written to the emulated machine.
- **Route** (60 Hz frames, all real key presses): START at frame 1300 (10 frames), A at 1500
  (Continue), 600 frames, A twice (the greeting), `Down` 180 frames: out of the church
  (`C01M09`, map 109) onto `C01` (map 100, (14.85, −10.05)); a 22-segment route found by a
  best-first search over savestates (the hero stays on the walkable ground; each segment a held
  direction, 16 frames per search step) reaches the south door (`0x72` volume at (−0.22, 34),
  `C01` → `F02`) and `Down` crosses it: **`F02` (map 20002), frame 3722, (−39.5, 0.43, −14.5),
  night** (day clock 45.87 s, phase 0). `Down` from the church door is the whole of the church
  part; the town route is the `hold` lines of the recipe embedded in the script.
- **Reproducible**: the recipe run twice gives main RAM images that differ in two bytes
  (`0x023ffded..e`, the end of main RAM, not the game) and the same core state otherwise.
- **Where the roamers are**: `func_02027ca4` returns `0x020fdd44` (EU; four slots of `0x318`
  bytes); slot 0 holds the map (`+0` 20002, `+2` flags `0xc`), `+6` is the **number of spawns
  since the map was loaded** (it never goes down), `+8` the spawn timer. The slot's group-0
  objects are `GameState+8` entries `0x70..0x7b`, contiguous, `0x198` bytes each. Fields read
  (checked against the spawn code `0x020733e8`, `ov_017::021a2128`): `+2` the **monster number**,
  `+0x44` position, `+0x54` angle, `+0xb8` the graph node index, `+0x130` state, `+0x14c` /
  `+0x154` state / awake ms, `+0x164` the destination node id, `+0x168` the **encfld zone id** the
  monster was drawn from, `+0x138..+0x13a` wander, walk class, vision. A free object has `+2 < 0`.
  The day clock is `GameState+0x3cc` (seconds, float) and the phase `+0x3dc` (0 night … 3 evening).
  The reader is `crates/dq9re/src/roamers.rs` (included by `tests/encounter_rom.rs`).
- **The watch loop** (stage 2 of the script, a seeded random walk): reads the twelve objects each
  frame and writes `S` (spawn), `T` (state change), `C` (contact) lines of numbers and a JSONL;
  on a contact it captures the battle screen and rolls back to an older snapshot of its own to
  keep patrolling. A full run covers the day clock from 45 s to its wrap and again to 55 s
  (night, morning, day, evening, night).


### Limits

- Character creation is driven in real time (A, touch OK): the sequence is reproducible but
  not frame-exact; checkpoints are read from RAM, never inferred from a screenshot. With the
  libretro harness it is frame-exact, but the A cadence (24 frames) remains a choice, not a
  player's rhythm.
- Looping A advances dialogues but **reopens** the conversation at the end (the hero still
  faces the NPC): it is not enough to progress. Beyond first player control, reaching Angel
  Falls requires the **real physical path** (stairs, doors, NPCs to talk to). The field
  worker's real route inputs will be replayed in melonDS; no teleport will serve as reference.
- Battle 26 is a real battle (no bypass), but the runtime substitutes it: flags set by the
  battle/engine may be missing (this is the difference above).
- No quest nibble at these points; the "village quests" and "end of prologue" checkpoints
  await the physical path.
- Saving state goes through the menu (about 2 s, real time); these are stable story points,
  not aligned frame numbers.
- Stepping (`pause` + `frame:N`, hotkeys `HK_Pause`/`HK_FrameStep`) works but melonDS 1.1
  advances only about **0.5 frame per press** under posted messages (measured: +3 VBlank for
  10 presses, +25 for 50): the real frame count is read from the VBlank counter, and it is not
  yet exact enough for the J2 "same number of frames" criterion.

## Next

- Replay the real route inputs (first player control → Angel Falls → quests → end of
  prologue) in the **libretro core**: the harness provides the same keys frame by frame, so
  the J2 "same number of frames" criterion becomes reachable by translating a
  `dq9-app --trace-inputs` trace into input JSONL. Standalone melonDS 1.1, for its part,
  advances only ~0.5 frame per press under posted messages (`pause`/`frame:N`).
- `4212` and `4135`: **provenance established** (November 6, 2026). They are item-obtained
  flags (`func_0207cc18`, `0xc76 + compendium index`): medicinal herb 22000 (index 945 → 4135)
  in the female run, slimedrop 22087 (index 1022 → 4212) in `bat6`. Battle 26 loot is
  **random**: three runs with identical inputs except gender give the herb or neither. The
  runtime substitutes the battle and draws no loot ([`item.md`](item.md),
  [`trigger.md`](trigger.md)).
- The state also contains the ARM7 and sound (`SPU.`): out of scope until audio is ported.
