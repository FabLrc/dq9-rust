# DQ9 Rust

[![CI](https://img.shields.io/github/actions/workflow/status/FabLrc/dq9-rust/ci.yml?branch=main&label=CI&logo=github)](https://github.com/FabLrc/dq9-rust/actions/workflows/ci.yml)
[![Web demo](https://img.shields.io/github/actions/workflow/status/FabLrc/dq9-rust/pages.yml?branch=main&label=web%20demo&logo=webassembly)](https://fablrc.github.io/dq9-rust/?newgame=1)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue)](LICENSE)
[![Rust 1.99 | edition 2024](https://img.shields.io/badge/rust-1.99%20%7C%20edition%202024-orange?logo=rust)](rust-toolchain.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux%20%7C%20Web-informational)](docs/app.md)
[![ROM](https://img.shields.io/badge/ROM-YDQP01%20(Europe)-lightgrey)](rom/README.md)
[![Last commit](https://img.shields.io/github/last-commit/FabLrc/dq9-rust)](https://github.com/FabLrc/dq9-rust/commits/main)

A Rust reimplementation of *Dragon Quest IX – Sentinels of the Starry Skies* (Nintendo DS) for
Windows, macOS, Linux and the Web, with the long-term goal of making the DS multiplayer
playable online. The game is reverse engineered and its assets are read from **your own ROM**;
no game data is included in this repository.

**[Try DQ9 Rust in your browser](https://fablrc.github.io/dq9-rust/?newgame=1)** — select
your own European ROM (`YDQP01`): the "New game" button becomes active once the ROM is read and
the game grabs the keyboard automatically. ROM and save files stay local: nothing is uploaded.
The demo is public, experimental and needs a recent browser with WebGPU or WebGL 2.

<!-- coverage:start -->
[![Ported](https://img.shields.io/badge/ported-2.3%25%20of%20functions-orange)](docs/roadmap.md)

**Progress: 2.3%** of game functions ported (287 / 12,056), 2.9% of the code (75 KB / 2,505 KB).
Computed by `cargo dq9re coverage` from the `port:` markers in the code (definition in `docs/roadmap.md`).

<details><summary>Per-module detail</summary>

| Module | Functions ported | Code ported |
|---|---:|---:|
| ARM9 | 255 / 5,464 (4.6%) | 65 KB / 826 KB (7.8%) |
| ITCM | 0 / 34 (0.0%) | 0 KB / 6 KB (0.0%) |
| ov_000 | 1 / 826 (0.1%) | 1 KB / 190 KB (0.1%) |
| ov_001 | 1 / 515 (0.1%) | 1 KB / 70 KB (0.0%) |
| ov_002 | 0 / 213 (0.0%) | 0 KB / 101 KB (0.0%) |
| ov_003 | 0 / 510 (0.0%) | 0 KB / 176 KB (0.0%) |
| ov_004 | 0 / 532 (0.0%) | 0 KB / 113 KB (0.0%) |
| ov_005 | 0 / 89 (0.0%) | 0 KB / 38 KB (0.0%) |
| ov_006 | 0 / 132 (0.0%) | 0 KB / 51 KB (0.0%) |
| ov_008 | 0 / 63 (0.0%) | 0 KB / 29 KB (0.0%) |
| ov_009 | 0 / 43 (0.0%) | 0 KB / 26 KB (0.0%) |
| ov_010 | 0 / 3 (0.0%) | 0 KB / 2 KB (0.0%) |
| ov_011 | 0 / 186 (0.0%) | 0 KB / 18 KB (0.0%) |
| ov_012 | 0 / 71 (0.0%) | 0 KB / 28 KB (0.0%) |
| ov_013 | 0 / 40 (0.0%) | 0 KB / 15 KB (0.0%) |
| ov_014 | 0 / 69 (0.0%) | 0 KB / 21 KB (0.0%) |
| ov_015 | 0 / 103 (0.0%) | 0 KB / 34 KB (0.0%) |
| ov_016 | 0 / 87 (0.0%) | 0 KB / 22 KB (0.0%) |
| ov_017 | 30 / 1,194 (2.5%) | 10 KB / 301 KB (3.2%) |
| ov_020 | 0 / 18 (0.0%) | 0 KB / 9 KB (0.0%) |
| ov_021 | 0 / 5 (0.0%) | 0 KB / 2 KB (0.0%) |
| ov_023 | 0 / 827 (0.0%) | 0 KB / 147 KB (0.0%) |
| ov_024 | 0 / 573 (0.0%) | 0 KB / 152 KB (0.0%) |
| ov_025 | 0 / 304 (0.0%) | 0 KB / 90 KB (0.0%) |
| ov_026 | 0 / 24 (0.0%) | 0 KB / 24 KB (0.0%) |
| ov_027 | 0 / 87 (0.0%) | 0 KB / 20 KB (0.0%) |
| ov_028 | 0 / 34 (0.0%) | 0 KB / 5 KB (0.0%) |
| ov_033 | 0 / 5 (0.0%) | 0 KB / 3 KB (0.0%) |
| ov_034 | 0 / 5 (0.0%) | 0 KB / 3 KB (0.0%) |

</details>
<!-- coverage:end -->

<img width="1467" height="801" alt="image" src="https://github.com/user-attachments/assets/55868b8e-93d1-4090-8143-bef37e079afb" />

## Status

**v0.1 works natively and is partly certified in the browser; v0.2 (chapter 2 up to the
landslide cleared, every battle fought) is in progress**; details, limits and next steps in
[`docs/status.md`](docs/status.md).
`cargo run -p dq9-app --release -- --new-game` runs the opening, chains the prologue, the
Observatory and the return to Angel Falls, then the village scenes. `--map M01`, or a save
passed with `--save`, opens Angel Falls directly.
What works:

- both DS screens (stacked, side by side, or top screen only);
- textured maps, lighting, fog and time of day;
- animated NPCs and an animated hero;
- movement ported in fixed-point, with collisions and the L/R camera;
- doors, houses and scripted warps, conversations in the game's message window;
- scripted cutscenes: `.stb` interpreter, characters, camera, fades, effects, night; the
  chapter 2 scenes play (`--events-report 2.1-1`: 47 of 50 complete);
- village tasks 1.2: ring, widow, stable, exit lock, return 1.4.1;
- music, sound effects and jingles;
- starting from a DS save (`--save`); the game state restores the party, bag and gold;
- battles played with the engine in the window and on the Web (command menus, the game's
  messages, carried-item use, experience, level-ups, wipe-out; `--fight --seed N` headless) and a battle scene on its
  stage (`--battle 2`, `--battle-stage 30116 --monsters 1x3`); in `dq9-core` a battle engine checked
  against BattleEmulator, party levels and rewards, battle messages in five languages,
  roaming-monster rules checked on melonDS field states;
- visible animated roaming monsters and contact-triggered battles, with the field restored
  afterwards; native F01 and Hexagon smoke checks (`docs/app.md`, Field encounters);
- trees and scenery that face the camera.

Still missing:

- headless runs still count scripted battles as won unless `--fight` is given, and roaming
  is disabled there to preserve the parity traces;
- attack animations in battle, the field menu, shops, inn and church screens (their rules exist in
  `dq9-core`);
- end-to-end browser certification (standalone battle 26 has been played to victory, but not
  the continuous route) and comparison with the original over the whole route
  (see [`docs/status.md`](docs/status.md)).

Status, limits and evidence: [`docs/status.md`](docs/status.md); plan and milestones:
[`docs/roadmap.md`](docs/roadmap.md); running and commands: [`docs/app.md`](docs/app.md).

## Quick start

You only need Rust (via [rustup](https://rustup.rs); `rust-toolchain.toml` pins the version) and
your own ROM. Ghidra, melonDS and the other reverse-engineering tools are optional.

```sh
cargo run -p dq9-app --release -- --new-game   # play from the opening
cargo dq9re info                               # check the ROM
cargo test --workspace                         # tests (ROM-based ones skip without a ROM)
```

## Repository layout

| Path | Role |
|---|---|
| `crates/nds-rom` | DS ROM: header, FNT/FAT, overlays, ARM9 (BLZ), LZ10/LZ11/Huffman/RLE, NARC, G3D models and animations. No I/O, builds for wasm. |
| `crates/dq9-formats` | DQ9 Level-5 formats: `.gp2` archives, tables, fonts, messages, dialogue lines, triggers, `.stb` scripts, maps, characters, lighting, save, SDAT. No I/O, builds for wasm. |
| `crates/dq9-core` | Deterministic game logic: state from a save or a new game, fixed-point movement and collisions, camera, doors and zones, triggers, conversations, `.stb` interpreter, cutscenes, scenario chaining. No I/O, builds for wasm. |
| `crates/dq9-render` | wgpu renderer close to the DS 3D engine: maps, NPCs, hero, lighting, fog, 2D interface. No I/O, builds for wasm. |
| `crates/dq9-audio` | SSEQ sequencer, banks, waves and mixing of the DS sound. No I/O, builds for wasm. |
| `crates/dq9-app` | Application (winit + wgpu, native and Web): `cargo run -p dq9-app --release -- --new-game` ([`docs/app.md`](docs/app.md)). |
| `crates/dq9re` | Reverse-engineering CLI (`cargo dq9re --help`). |
| `crates/testrom` | Locates the ROM for real-data tests. |
| `re/symbols.txt` | Recovered names, source of truth applied to the Ghidra project. |
| `re/ghidra_scripts/`, `scripts/` | Ghidra and PowerShell scripts: tool installation, import, queries, symbols, melonDS states. |
| `docs/` | Roadmap, architecture, external resources, reverse-engineering notes (`re/`). |
| `rom/` | Your ROM (ignored by git). |

## Setup

Expected ROM: European release `YDQP01` (see [`rom/README.md`](rom/README.md)), placed in `rom/`,
at the repository root, or given with `--rom <path>` / `DQ9_ROM`.

Requirements: Rust via rustup (version pinned by `rust-toolchain.toml`, `wasm32-unknown-unknown`
target included). On Windows, host `x86_64-pc-windows-gnu` with MinGW-w64 (WinLibs) in the
`PATH`. The scripts are PowerShell; on macOS and Linux run them with
[`pwsh`](https://learn.microsoft.com/powershell/scripting/install/installing-powershell) (CI
does so on Linux). The reverse-engineering tool installer is Windows-only.

```powershell
cargo dq9re info                 # check the ROM
cargo dq9re extract --unpack     # code + data in build/rom, .gp2 archives unpacked

# Reverse-engineering tools (Windows): JDK, Ghidra, melonDS in ./tools, SHA-256 verified
powershell -ExecutionPolicy Bypass -File scripts/setup-re-tools.ps1
powershell -ExecutionPolicy Bypass -File scripts/ghidra-import.ps1   # re/ghidra project (~3 min)
powershell -ExecutionPolicy Bypass -File scripts/ghidra.ps1          # graphical interface
powershell -ExecutionPolicy Bypass -File scripts/melonds-state.ps1 -Save game.sav   # verified .ml1 state (~40 s)
```

## Development

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1              # fmt, clippy, tests, wasm, progress
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Fast        # fmt and clippy only
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Only test   # selected steps
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -RequireRom  # fails without a ROM
powershell -ExecutionPolicy Bypass -File scripts/parity.ps1              # comparison with the original
```

`scripts/verify.ps1` is the single source of the verification commands (CI calls it): it runs
fmt, clippy, tests (the real-ROM ones when a ROM is present), the wasm build and the progress
block above, and stops at the first failure; the per-worktree Cargo target and the optional
`cargo nextest` are described in [`docs/app.md`](docs/app.md#development-loop).

Two diagnostic tools, with no effect on the game:

```powershell
# List (then delete with -Apply) old emulator states under build/
powershell -ExecutionPolicy Bypass -File scripts/prune-build.ps1
# Compare two runs by story point, not by tick
powershell -ExecutionPolicy Bypass -File scripts/compare-runs.ps1 `
    -Original build/oracle/original.jsonl -Runtime build/oracle/runtime.jsonl
# Compare two states (named delta) and name the changes of a watch frame by frame
cargo dq9re state diff bat6.ml1 bat4.ml1
cargo dq9re state watch --file build/oracle/watch.jsonl
```

`prune-build.ps1` touches neither `.sav`, `.nds`, JSON nor image files, and refuses the
repository root. `compare-runs.ps1` aligns the JSONL checkpoints on the current map and track,
preferring the same `kind` (`control`, `after`) and otherwise the runtime checkpoint: a map
load is not compared with a player-control handoff. It then compares progress, flags and
quests; it complements `oracle-compare.ps1`, which starts from a single melonDS state.

CI runs `scripts/verify.ps1` on Linux (fmt, clippy, tests, wasm) without ever having access to
the ROM, then computes the progress and fails if the block in this README is stale: refresh it
locally with `verify.ps1`. A commit that only touches documentation, `re/symbols.txt`,
`re/ghidra_scripts/` or `rom/` does not rerun CI (a change to `re/functions.txt` does); the
Pages deployment is only triggered by paths that change the web binary (`crates/**`,
manifests, `rust-toolchain*`).

## Reverse engineering

- **`cargo dq9re`**: `ls data/map/`, `gp2 <archive> --out <dir>`, `table <archive>
  <member>`, `msg`, `font`, `narc <file> -d`, `cat <file> -d`,
  `talk data/scenario/M02M0.gp2 018_en.bin --quest 11=1 --level 25` (triggers, then the line
  chosen for a given state), `stb data/event/ev03030.gp2` (disassembled event script with its
  texts), `map X01M07 --image build/scratch/x01m07.png --scale 16` (top view of the
  collision), `map X01M07 --reach 0 0.17 6.8 --to 0 0.17 17.5 --reach-image
  build/scratch/reach.png` (actual reach in 3D, see
  [`docs/re/map-archive.md`](docs/re/map-archive.md#viewing-the-collision-dq9re-map---image---reach)).
- **Oracle states**: `cargo dq9re state state.ml1` (sections, VBlank, scenario, code compared
  with the ROM), `--json` (the story point the runtime compares), `--peek 020f33d8:40`, and
  `state --ram ram.bin` on a raw 4 MiB main RAM image (`--dump-ram` of the harness), or a file
  name without `--ram`; `cargo dq9re states index build/oracle --out
  build/oracle/index.json` then `states find --story 1.1.2 [--map 4101]` to find a state
  without opening it ([`docs/re/melonds-state.md`](docs/re/melonds-state.md)).
- **libretro harness**: `python scripts/melondsds_host.py --rom <ROM> --core
  tools/build/melondsds_libretro.dll --input run.jsonl --frames 23050 --checkpoints
  build/oracle/checkpoints.jsonl --dump-ram build/oracle/ram.bin --state-out
  build/oracle/core.bin --shot build/oracle/control.png` runs the ROM in the melonDS DS core,
  without a window or focus: frame-by-frame inputs (`pad`, `touch`, `checkpoint`, `shot`),
  RAM, states and PNG; it reaches the first player control in the Observatory on the original
  (input format and limits in [`docs/re/melonds-state.md`](docs/re/melonds-state.md)).
- **Ghidra**: ARM9 in the `arm9` block, ITCM/DTCM at their run-time addresses, each overlay in
  its own `ov_NNN` space. Headless queries:
  `scripts/ghidra-query.ps1 decomp|xrefs|calls|disasm <address>` (`0x020d91ec` or
  `ov_017::0218c0f4`); a query locks the project, running several needs copies of
  `re/ghidra` designated by `DQ9_GHIDRA_PROJECT`. Results are cached in
  `build/ghidra-cache/` (key: command, addresses, ROM, SHA-256 of `Query.java`); `-Refresh`
  forces the query.
  Imports and `scripts/ghidra-apply-symbols.ps1` apply the names and bounds of
  `re/functions.txt`, then the overrides of `re/symbols.txt`; details and limits in
  [`docs/re/rom-layout.md`](docs/re/rom-layout.md#names-and-bounds-in-ghidra).
- **melonDS** (`tools/melonds`): visual reference and dynamic analysis through its GDB stub.
- **Before analysing**: symbols and code of the
  [USA decompilation](https://github.com/DQIX/dqix-decomp) (addresses to convert for the
  European ROM) and the [DQIX formats wiki](https://github.com/DQIX/wiki), see
  [`docs/external-resources.md`](docs/external-resources.md).

Recording a finding:

1. Name functions and data in `re/symbols.txt` (`<address> <name> # comment`), then
   `scripts/ghidra-apply-symbols.ps1` (project closed in the interface). Local names take
   precedence over those of the decompilation; the Ghidra project is disposable.
2. Describe the format or behaviour in `docs/re/<topic>.md`: evidence (addresses, checks on
   data), hypotheses marked **[hypothesis]**.
3. Implement it in the matching crate with a test on synthetic data and, when relevant, an
   invariant on the real ROM (`tests/real_rom.rs`).

### Quick access to the original code

`scripts/decomp.ps1` maps a European address or name to the source file of the USA
decompilation (Unlicense, uncommitted checkout, expected in `tools/dqix-decomp`):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250   # GameState::AdvanceDayTimer
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 GameState::SetDayTimer -Context 80
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 -List SetDayTimer
```

It prints the European address and name, the USA address, the source path and an excerpt;
`-Decomp PATH` selects another checkout. Details, Ghidra cache and limits:
[`docs/re/tools.md`](docs/re/tools.md).

## Documentation

| File | Content |
|---|---|
| [`docs/status.md`](docs/status.md) | current status, active batches, blockers and next steps (single source) |
| [`docs/roadmap.md`](docs/roadmap.md) | milestones up to the full port, definition of "100%" |
| [`docs/architecture.md`](docs/architecture.md) | target crates and rules (portability, determinism, traceability) |
| [`docs/orchestration/`](docs/orchestration/README.md) | kit for working with agents: coordinator, worker rules, batch template |
| [`docs/app.md`](docs/app.md) | running `dq9-app` (native, browser), options, commands, events, rendering and its approximations |
| [`docs/external-resources.md`](docs/external-resources.md) | decompilation, wiki, PC port, DS tools, licenses |
| [`docs/re/rom-layout.md`](docs/re/rom-layout.md) | ROM structure, overlays, file systems |
| [`docs/re/nitro-sdk.md`](docs/re/nitro-sdk.md) | identified ARM9 NitroSDK routines (classified Platform, not ported) |
| [`docs/re/gpc2.md`](docs/re/gpc2.md), [`table.md`](docs/re/table.md) | Level-5 archives and compression, record tables |
| [`docs/re/text.md`](docs/re/text.md), [`font.md`](docs/re/font.md) | texts, tags, message pipeline, fonts |
| [`docs/re/dialogue.md`](docs/re/dialogue.md), [`trigger.md`](docs/re/trigger.md), [`stb.md`](docs/re/stb.md) | dialogue lines and message window, scenario triggers, event scripts and their execution |
| [`docs/re/item.md`](docs/re/item.md) | items: `itemdt_*` tables, compendium indices, "item obtained" flags |
| [`docs/re/stb-natives.md`](docs/re/stb-natives.md) | inventory of the 228 natives used: arguments, established effects, addresses and unknowns |
| [`docs/re/maplist.md`](docs/re/maplist.md), [`map-archive.md`](docs/re/map-archive.md), [`g3d.md`](docs/re/g3d.md) | map list, map assembly, doors and zones, G3D models and animations |
| [`docs/re/characters.md`](docs/re/characters.md) | NPCs, cast per story step, sprites, models, hero |
| [`docs/re/collision.md`](docs/re/collision.md), [`movement.md`](docs/re/movement.md) | `.col2` collisions, hero movement and camera |
| [`docs/re/lighting.md`](docs/re/lighting.md) | `.bats` lighting, fog, time of day |
| [`docs/re/sdat.md`](docs/re/sdat.md) | sound: SDAT, sequences, effects, jingles, map music |
| [`docs/re/save.md`](docs/re/save.md) | DS save: container, checksums, typed fields, unknowns |
| [`docs/re/melonds-state.md`](docs/re/melonds-state.md) | melonDS oracle: `.ml1` states and RAM images, catalogue, libretro harness, useful addresses, remaining verification |
| [`docs/re/tools.md`](docs/re/tools.md) | Ghidra query cache, European address to USA decompilation source |
| [`AGENTS.md`](AGENTS.md) | context and rules for development agents |

## Legal

This project contains and distributes **no game files**: no ROM, no extracted assets, no
game code. It reads the data from a copy you provide at run time. Use only a copy of the game
you legally own.

DQ9 Rust is an unofficial fan project. It is not affiliated with, endorsed or sponsored by
Square Enix, Level-5, Armor Project, Bird Studio or Nintendo. *Dragon Quest* is a registered
trademark of Square Enix Holdings Co., Ltd.; *Nintendo DS* is a trademark of Nintendo. All
other trademarks belong to their respective owners and are used here only to identify the
game this project is compatible with.

## License

Copyright © 2026 Fabien Laurence and DQ9 Rust contributors.

Licensed under the [GNU Affero General Public License v3.0 or later](LICENSE)
(`AGPL-3.0-or-later`). Any modified version you distribute, or make available to users over a
network (for example a hosted web build or an online server), must be released under the same
license with its complete source code.
