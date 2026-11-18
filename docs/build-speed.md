# Build speed: the native Windows loop

Numbers for the edit→test loop on the development machine, and the settings adopted from them.
`docs/app.md` ("Development loop") owns the worktree/target rules; this note owns the link.

## What is measured

The loop this project actually runs on Windows:

```
touch crates/dq9-core/src/lib.rs; cargo test -p dq9-app --no-run
```

`dq9-core` is a leaf of everything, so the touch rebuilds it, `dq9-app`, and relinks the test
binaries. Second measure: `cargo clippy --workspace --all-targets -- -D warnings` after the
same touch (the `-Fast` step of `scripts/verify.ps1`). A third, cold `cargo test -p dq9-app
--no-run` into an empty target gives the cost of a fresh per-batch target.

Timings are wall time on
`x86_64-pc-windows-gnu`, rustc 1.99.1, LLVM 23.1.1, MinGW-w64 gcc 16.2.0, 20 logical CPUs,
Windows 11 26200 (`build/evidence/k1-linker/environment.txt`). Five runs per configuration,
medians reported; every raw run is in `build/evidence/k1-linker/timings.txt`.

**The machine's noise is large** (`build/` targets are scanned by the antivirus, other
worktrees build at the same time): a single baseline run ranged from 45 s to 100 s for the
same command. That is why each candidate is compared on the median of five runs, and why a
candidate had to beat the baseline by a margin far larger than the spread to be adopted.

## Results

Median of five runs. "Cold" is one run into an empty target directory; the last column sums the
seconds the test binaries themselves reported, warm.

| Configuration | cold `test --no-run` | loop `test -p dq9-app --no-run` | loop `clippy --workspace --all-targets` | test suites (sum of `finished in`) |
|---|---|---|---|---|
| baseline (`main`, gcc/MinGW `ld`) | 488 000 | 66 900 | 32 600 | 8.5 s |
| `linker = "rust-lld"` | 179 400 | 24 500 (−63 %) | 10 300 | — |
| + `[profile.test] debug = "none"` | 105 000¹ | 17 600 (−28 %, −74 % vs baseline) | 8 900 | 8.0 s |
| + `[profile.test] opt-level = 1` | 116 000¹ | 20 700 (−16 % vs lld) | 8 700 | 8.9 s |

¹ Not a cold build: the same warmed target with a changed profile, so all units rebuild. It
shows the rebuild cost of each setting, not the empty-target cost.

The adopted combination is rows 2+3: `24 500 → 17 600 ms` per incremental iteration
(median; runs 16.9–23.9 s versus 19.1–29.2 s for LLD alone), and `488 → ~105 s` for a fresh
per-batch target.

Raw runs, per configuration, in `build/evidence/k1-linker/timings.txt`. The spread is why the
`opt-level = 1` row is not adopted: its runs (19.3–25.9 s) overlap the LLD-only runs
(19.1–29.2 s), so the median difference is not a result, and its test suites are marginally
slower (8.9 s vs 8.5 s).

## Adopted

### `rust-lld` as the Windows linker

`.cargo/config.toml`:

```toml
[target.x86_64-pc-windows-gnu]
# The toolchain ships LLD; the MinGW `ld` was the dominant cost of every relink.
linker = "rust-lld"
```

Facts that make this portable, not machine-specific:

- `rust-lld` is part of the Rust toolchain (`<sysroot>/lib/rustlib/x86_64-pc-windows-gnu/bin/rust-lld.exe`)
  and is **not** on `PATH` here: rustc resolves the name from the toolchain itself
  (`build/evidence/k1-linker/environment.txt`). No absolute path, no extra install.
- The gcc-driver route (`rustflags = ["-Clink-arg=-fuse-ld=lld"]`) also works but requires
  `-Clink-arg=-B<sysroot>/lib/rustlib/x86_64-pc-windows-gnu/bin/gcc-ld`, i.e. an absolute
  path inside a rustup toolchain: rejected as non-portable, the direct `linker` key gets the
  same speed without the path.
- The key is scoped to the `x86_64-pc-windows-gnu` triple: Linux CI (`x86_64-unknown-linux-gnu`)
  and the wasm build (`wasm32-unknown-unknown`) never read it, and `cargo metadata` parses it
  on any host (checked: `cargo --config … metadata --no-deps` succeeds).

### No DWARF in test binaries

`Cargo.toml`:

```toml
[profile.test]
opt-level = 2
debug = "none"
```

Test binaries are compiled and linked, run once, and thrown away; their debug info was 28 % of
the loop (24.5 → 17.6 s). What is lost: backtraces and line-level stepping in a debugger for
test binaries — `PanicInfo` still prints the failing `file:line`, and a need for DWARF is one
environment variable away (`CARGO_PROFILE_TEST_DEBUG=line-tables-only`). `[profile.dev]` keeps
`line-tables-only`, and `[profile.release]` is untouched, so the `--screenshot` runs and the
long route test are unchanged.

## Rejected

- **`[profile.test] split-debuginfo`**: cargo and rustc accept `"unpacked"` on
  `x86_64-pc-windows-gnu` but no split object file is produced (0 `*.o` in a scratch target),
  so there is nothing to gain: the debug info is still written into the binary.
- **`opt-level = 1` for the workspace crates in `[profile.test]`**: 20.7 s median, but its five
  runs (19.3–25.9 s) overlap LLD-only runs (19.1–29.2 s), so the median is not a measurable
  win; at the same time real-ROM tests get slower (8.9 s vs 8.5 s of summed suite time), since
  they walk ~270k strings and 53k archive members, and `opt-level = 2` exists for them.
  `codegen-units` and `incremental` were left alone: `[profile.test]` already defaults to 256
  units and incremental builds.

## How to re-measure

One target per worktree (`docs/app.md`, "Development loop"), then the loop itself:

```powershell
$env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\dq9-target-<batch>"
cargo test -p dq9-app --no-run                      # warm the target
1..5 | ForEach-Object {
    (Get-Item crates/dq9-core/src/lib.rs).LastWriteTime = Get-Date
    Measure-Command { cargo test -p dq9-app --no-run } | Select-Object TotalSeconds
}
# and the same touch followed by: cargo clippy --workspace --all-targets -- -D warnings
```

Compare medians of five runs, never a single run: the same command ranged from 45 s to 100 s
here because of the antivirus and other worktrees building at the same time. The cold number is
one `rm -rf "$CARGO_TARGET_DIR"` followed by a timed `cargo test -p dq9-app --no-run`.

The batch's own scripts (`measure.sh`, `runtime.sh`) and every raw run stay in
`build/evidence/k1-linker/`; they are evidence, not tooling.

`cargo --timings` is **not** a way to split the loop: on cargo 1.99 it builds
non-incrementally (the same touch measured 118 s against 17.6 s incremental) and moreover
throws the incremental cache away, so the next plain run pays 50.8 s of rebuild. All numbers
above are end-to-end wall time, measured with `cargo` alone.

### Known drift, not explained

After those `--timings` runs the same configuration on the same target cost 40–73 s per touch
in five consecutive runs, and `debug/incremental` stayed empty while `cargo build -v` passed no
`-C incremental` to rustc, i.e. incremental compilation was not in effect at all (a
`cargo clean -p dq9-core -p dq9-app` did not bring it back). The raw values are in
`build/evidence/k1-linker/timings.txt` (`adopted plain re-run`, `adopted steady run1/2`,
`post-clean incremental run1/2`). Two consequences: the **cold** numbers (488 → 179 → 105 s)
are the ones to trust, since no cache state can affect them; and the warm-loop medians above
describe a target whose incremental cache is warm, which is the state after any
`verify.ps1` run — re-measure before relying on them for a different target. **[unknown]** why
incremental compilation is currently inactive here.
