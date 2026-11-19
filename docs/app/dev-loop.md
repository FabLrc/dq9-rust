# Development loop

`scripts/verify.ps1` is the single source of verification commands: AGENTS.md, the docs
and CI call it. It chains five stages in this order and stops at the first failure:
`fmt` (`cargo fmt --all --check`), `clippy` (`cargo clippy --workspace --all-targets --
-D warnings`), `test` (`cargo test --workspace`), `wasm` (build of `nds-rom`,
`dq9-formats`, `dq9-core`, `dq9-render`, `dq9-audio` and `dq9-app` for
`wasm32-unknown-unknown`) and `coverage` (`cargo dq9re coverage --readme README.md`).

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1                    # the five stages
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Fast              # fmt + clippy
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Only test,wasm    # subset
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Only long         # the long route
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -RequireRom        # integration gate
```

`long` is the route's acceptance run as a test:
`cargo test -p dq9-app --release -- --ignored the_route_reaches_event_2750` plays from a new
game to event 2750 (~19 min in release) and pins the story, the Hexagoon outcomes, the
checkpoint count and the SHA-256 of the checkpoint JSONL. It is opt-in only (never in the
default run nor `-Fast`; CI has no ROM): the coordinator runs it before declaring a milestone.

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
markers. A worker never commits it: `scripts/integrate.ps1` regenerates the block and folds it
into the last integration commit (never a commit of its own), so it cannot conflict between
branches. CI recomputes the block, shows it in the job summary and fails if it differs from the
committed README (stale block); it never commits.

Branch integration (`scripts/integrate.ps1`), the evidence manifest
(`scripts/evidence.ps1`, protected by `scripts/prune-build.ps1`) and branch cleanup
(`scripts/prune-branches.ps1`) are described in `docs/orchestration/README.md`.

Each Orca worktree has its own Cargo target, and that target is a **slot**, not a directory per
batch. Five long-lived targets (`%LOCALAPPDATA%\dq9-target-slot1` to `slot5`) are reused by
successive workers: `scripts/dispatch.ps1` gives a batch the lowest free slot of
`build/batches/runs.json` and, when the slot's `.last-batch` marker names another batch, cleans
its workspace members with `cargo clean -p <member>` before the launch (the third-party cache,
the expensive part, stays). A worker keeps its slot, never deletes it and never touches another
one; `scripts/integrate.ps1` verifies a combined tree in its own
`%LOCALAPPDATA%\dq9-target-integrate`.

```powershell
$env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\dq9-target-slot1"   # cmd: %LOCALAPPDATA%\dq9-target-slot1
```

Never share one target between worktrees. Observed in the r1 wave (integration of
`r1-formats-bytes`): with one `CARGO_TARGET_DIR` for every worktree, Cargo considered the
`nds-rom` rlib built from another worktree's sources fresh, and the doctests of `dq9-formats`
failed against it (`module bytes is private`) although the branch was correct; the same
mechanism can produce a false pass. A target also stays outside the repository (`build/` is
only for project outputs: screenshots, extracted ROM).

A test binary can outlive the sources it was built from (branch change): the tests read
`CARGO_MANIFEST_DIR` at runtime (`crates/testrom`), and `verify.ps1 -RequireRom` passes the
found ROM to the tests through `DQ9_ROM`.

What the loop costs, measured on the development machine and detailed in
[`../build-speed.md`](../build-speed.md): a cold `cargo test -p dq9-app --no-run` into an empty
target takes ~105 s with the adopted settings (`rust-lld` as the Windows linker,
`[profile.test] debug = "none"`), an incremental `cargo test -p dq9-app --no-run` after a
`dq9-core` touch 17.6 s median (24.5 s with the MinGW `ld`), `cargo clippy --workspace
--all-targets` 8.9 s, and the test suites themselves ~8 s summed. `sccache` was measured and is
not used: a second empty target got 0 cache hits (the keys contain the target path), and even
with perfect hits it could not pay for a cache of several GB.

`cargo nextest` is optional too: it runs integration and unit tests
faster and names the slowest ones. CI keeps `cargo test`; workspace doctests are not
part of a nextest run and remain covered by `cargo test --doc`.

```powershell
cargo install cargo-nextest --locked
cargo nextest run --workspace
```
