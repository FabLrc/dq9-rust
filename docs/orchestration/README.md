# Orchestration — coordinator workflow

Documents: this file (coordinator), `worker-rules.md` (rules shared by all workers),
`batch-template.md` (batch spec, copied to `build/batches/<batch>.md`), `prompt-dispatch.md`
(worker launch prompt). Current state: `docs/status.md` only.

## Harness and models

Always the **OMP** harness: Orca workers launched with OMP, subagents through OMP's `task`
tool. The model is chosen when the worker is launched and recorded in the batch spec.

| Model | Id | Provider | Use |
|---|---|---|---|
| DeepSeek V4.1 Flash | `opencode-go/deepseek-v4.1-flash` | OpenCode Go | **default**: docs, mechanical scripts, data collection/import, tests against a given contract |
| Sonnet 5.5 | `anthropic/claude-sonnet-5-5` | Anthropic | **only when the task needs stronger reasoning**, chosen from the start (no Flash attempt first): RE that must establish a cause, design and structural refactors, non-trivial debugging, scripts that merge, rebase or delete |
| Opus 5.5 | `anthropic/claude-opus-5-5` | Anthropic | last resort after a documented Sonnet failure; justification in `docs/status.md` |

Escalation: Flash fails twice or exceeds its cap → Sonnet; Sonnet fails → Opus,
only with justification. No other model.

**Standing instruction (user): Sonnet 5.5 may be used again (lifted 2026-11-18 after a short
no-Sonnet period).** Opus stays the last resort.

OMP agents: `scout` has neither shell nor git (file reading only); any branch comparison,
`git cherry`/`range-diff` or build command goes to a `task` agent.

## Roles

- **Coordinator**: writes the specs, launches the workers, integrates. The only one writing
  `docs/status.md`, `docs/roadmap.md`, `README.md`.
- **Worker**: one batch each, in its own worktree, following `worker-rules.md`.
- **Challenger**: fresh agent (Sonnet). Receives the claim and the means to check it
  (original state, test, addresses, evidence), **not** the author's reasoning; tries to
  refute it. Mandatory for any route, height, transition or flag cause before it is
  written down as fact.

## Wave = DAG

RE → implementation → challenger → integration → final "docs consistency" batch.

- "Docs consistency" batch (Flash): check `AGENTS.md`, `docs/architecture.md`,
  `README.md`, `docs/status.md` against the code (grep for constants such as the tick rate,
  list of crates…).
- At most 4-5 concurrent workers.
- One owner per file. Shared files (`crates/dq9-app/src/app.rs` — state, constructor and step
  loop only; `app/input.rs`; `app/draw.rs::present_frame`; `crates/dq9-app/src/event.rs` +
  `event/**`, `crates/dq9-core/src/field.rs`, `crates/dq9-core/src/state.rs`,
  `crates/dq9-core/src/trigger.rs` + `trigger/**`, `Cargo.toml`, `re/symbols.txt`) appear in the
  ```owns block of a single batch at a time. A feature owns its module under
  `crates/dq9-app/src/app/` (`docs/app-structure.md`): the swirl, text speed or record/replay no
  longer queue behind each other, and the one-line hook in the loop is written by the batch that
  owns the file (or by the module's batch when it is alone on it).

## Batch cycle

1. Write `build/batches/<batch>.md` from `batch-template.md` (model, base SHA, ```owns, cap).
2. `./scripts/queue.ps1` prints the state of every spec (running, integrated, waiting,
   ready…) and the next wave; `./scripts/dispatch.ps1 -Batch <batch>` launches one worker. It
   reads the spec's `Model:`, ```owns and `Depends:`, refuses an ```owns overlap with a running
   batch of `build/batches/runs.json` and a dependency that is not integrated, picks the lowest
   free Cargo target slot, fills `prompt-dispatch.md` into
   `build/batches/<batch>.prompt.txt` and runs `orca orchestration worker-start
   --spec-file …` with `--base-branch main`, `--agent omp`, `--setup skip`; `-DryRun` prints
   everything and launches nothing. The branch is `FabLrc/<milestone>-<batch>`.
3. Wait for `worker_done`; read the report and the evidence.
4. Checks, then integration from the main checkout on `main`, clean tree. One run takes the
   whole list of settled branches in integration order and runs **one**
   `verify.ps1 -RequireRom` on the combined result:

   ```powershell
   ./scripts/integrate.ps1 -Branch FabLrc/j3-a,FabLrc/j3-b -Rebase                      # dry run
   ./scripts/integrate.ps1 -Branch FabLrc/j3-a,FabLrc/j3-b -Rebase -Apply `
       -Message 'audio: play field sound effects','formats: read the SDAT banks' -Cleanup
   ./scripts/integrate.ps1 -Branch FabLrc/j3-a -Cleanup                                 # cleanup only
   ```

   `-Spec` defaults to `build/batches/<batch>.md`; `-Message` is matched to the branches by
   position, and a spec line `Commit: domain: summary` supplies the subject when `-Message`
   has no entry for that branch. Commas are not split in `-Message`, so under
   `powershell -File` use the `Commit:` lines.

   Per branch, the cheap checks run first: branch contains `main` (otherwise rebase, or
   `-Rebase`), changed files within ```owns (except the `README.md` progress block), no
   forbidden file (`rom/`, `build/`, `tools/`, `*.nds`, `*.sav`), worktree clean. Then the
   combined result is built in a **temporary integration worktree** under
   `%LOCALAPPDATA%\dq9-integrate-wt-*` (never the main checkout or a batch worktree): each
   branch is squash-merged there as its own commit (a conflict names the branch), and
   `verify.ps1 -RequireRom` runs once on the combined tree. Green: `-Apply` fast-forwards
   `main` to those commits, without pushing. Red: the prefixes of the ordered list are
   verified by binary search, the first culprit is named, `main` stays untouched (a dry run
   costs the same single verify as an apply, so an `-Apply` straight away is fine).

   The README progress block that `verify.ps1` rewrites is **folded into the last integration
   commit** (never into a worker branch, never a commit of its own); any other change left by
   verify fails the run.

   `-Cleanup` (with `-Apply` or alone, branches already integrated) does, per branch:
   `orca orchestration worker-release --dispatch <id>` when `build/batches/runs.json` has an
   entry for the batch (`$env:DQ9_ORCA` replaces `orca`; a failed release is a warning), then
   removes the junctions without following their target, the worktree (never forced) and the
   branch, then the `runs.json` entry. The release comes first because the worker's open
   terminal is what made `git worktree remove` fail (exit 255) in wave v2-1.
   `dq9-target-slot<N>` (the long-lived targets reused by successive workers, cleaned of their
   workspace members by the next dispatch) and `dq9-target-integrate` are never deleted. Do not clean a
   branch up after another batch has touched the same shared file: the "integrated" check
   compares `main` with the branch on the files it changed.

   `runs.json` (ignored by git, main checkout): `{ "<batch>": { "dispatch", "slot", "model",
   "attempt", "started", "branch" } }`, written by `dispatch.ps1`, entry removed by `-Cleanup`.

   During a wave, `main` moves between batches: `-Rebase` runs `git rebase main` inside each
   batch worktree before the checks (a conflict aborts the rebase and fails with "rebase
   conflict, ask the worker"). The verify step always uses the private target
   `%LOCALAPPDATA%\dq9-target-integrate` and the main checkout's `*.nds` as `DQ9_ROM` when the
   variable is empty. A crash can leave a `dq9-integrate-wt-*` directory: remove it with
   `git worktree remove --force <path>`.

   Never run `orca worktree rm` on a worktree that still has a `rom` (older worktrees) or
   `tools` junction (they lead to the only copy of the ROM): first `integrate.ps1 -Cleanup`,
   which removes the junctions without following their target. A worktree with no junction and
   no commit can be removed cleanly with `orca worktree rm --worktree path:<path> --delete-branch`.
5. Rewrite `docs/status.md` (< 60 lines, rewritten, not appended to), commit, push. The status
   file is rewritten **once per wave**: no `docs:` commit that only records a green CI run or
   a deployment.

## Workflow optimisations (wave r1)

- **One Cargo target per worktree, from a pool of slots** (`worker-rules.md` § 0,
  [`app/dev-loop.md`](../app/dev-loop.md)): with a shared target, Cargo took the `nds-rom` rlib
  built from another worktree's sources as fresh and the `dq9-formats` doctests failed
  (`module bytes is private`) on a correct branch; the same mechanism can produce a false pass.
  So each worker gets `%LOCALAPPDATA%\dq9-target-slot<N>` (1..5), reused by successive workers:
  `dispatch.ps1` cleans a slot's workspace members when it last served another batch and keeps
  the third-party cache, a worker never deletes its slot, and `integrate.ps1 -Cleanup` leaves
  every slot and `dq9-target-integrate` alone. The numbers (cold ~105 s with the adopted
  settings, 17.6 s per incremental loop, ~4-5 GB per target) are in
  [`build-speed.md`](../build-speed.md); `sccache` was measured and dropped (0 hits across
  targets).
- **`DQ9_ROM` instead of a `rom` junction**: `rom/` is tracked (it holds `rom/README.md`), so
  the junction could never be created in a fresh worktree.
- **`integrate.ps1` owns its verify environment**: it always sets
  `%LOCALAPPDATA%\dq9-target-integrate` and finds the main checkout's `*.nds` when `DQ9_ROM`
  is empty; it integrates a whole wave in one run (one verify, one commit per branch, README
  block folded into the last one) and `-Rebase` handles a moved `main`.
- **No ceremony around a run** (k1-integrate): no manual rebase/verify per branch, no
  regeneration of the README block before `-Apply`, no hand `worker-release`, no hand target
  removal. The one-verify-per-run saving is the point: a verify costs 84-354 s (median ~110 s).
- **Parity references fall back to the main checkout** (`scripts/parity.ps1`): `build/` is
  ignored by git and absent from a linked worktree, so the references file of the main
  checkout is used when `-References` is not given.
- **Dispatch flags and prompt** (`prompt-dispatch.md`): `--base-branch main` starts from the
  local integrated `main` (not the lagging `origin/main`), `--agent omp` selects the harness,
  `--setup skip` avoids the `pnpm install` hook, and the prompt carries the slot target the
  dispatch picked (`dispatch.ps1` writes it, never a human).

## Scripts (contract interfaces)

| Script | Example |
|---|---|
| `scripts/queue.ps1` | `./scripts/queue.ps1` — read-only: every spec's state and the next wave, each line the `dispatch.ps1` command to run |
| `scripts/dispatch.ps1 -Batch <batch> [-Model <id>] [-Attempt <n>] [-RetryOf <id>] [-DryRun]` | `./scripts/dispatch.ps1 -Batch k1-linker` — spec checks, slot, prompt, `runs.json`, `worker-start`; tests: `scripts/tests/dispatch.tests.ps1` |
| `scripts/verify.ps1 [-Fast] [-Only <step[,step]>] [-RequireRom]` | `./scripts/verify.ps1 -Only fmt,clippy` — steps `fmt`, `clippy`, `test`, `wasm`, `coverage`, plus the opt-in `long`; only source of the verification commands |
| `scripts/integrate.ps1 -Branch <b[,b…]> [-Spec <batch>[,…]] [-Message <subject>[,…]] [-Apply] [-Rebase] [-Cleanup]` | see cycle above; tests: `scripts/tests/integrate.tests.ps1` |
| `scripts/evidence.ps1 -Batch <batch> [-Command <text>] [-Note <text>]` | `./scripts/evidence.ps1 -Batch j3-sound-effects -Command 'cargo run -p dq9-app -- --headless' -Note 'capture'` → `build/evidence/<batch>/manifest.json` |
| `scripts/prune-build.ps1` | `./scripts/prune-build.ps1` — never deletes a file listed in an evidence manifest |
| `scripts/prune-branches.ps1 [-Apply] [-Remote]` | `./scripts/prune-branches.ps1 -Remote` (list), then `-Apply`; never deletes a unique branch |
| `scripts/parity.ps1 [-References <json>] [-Rom <path>]` | `./scripts/parity.ps1` — default `build/evidence/parity/references.json`; non-zero on a non-accepted difference |

Docs cite evidence as `build/evidence/<batch>/<file>` + the first 12 characters of the
sha256.

`long` is the route's acceptance run as a test:
`cargo test -p dq9-app --release -- --ignored the_route_reaches_event_2750` (~19 min in
release): the prologue and chapter 2 played by the pilot to event 2750, with the story, the
Hexagoon outcomes, the checkpoint count and the SHA-256 of the checkpoint JSONL pinned. It is
never part of the default run nor of `-Fast`; CI has no ROM, and `integrate.ps1` does not run
it. The **coordinator** runs `./scripts/verify.ps1 -Only long` (with `DQ9_ROM` set) before
declaring a milestone.

## End of wave

```powershell
./scripts/prune-branches.ps1          # review the list
./scripts/prune-branches.ps1 -Apply
./scripts/prune-build.ps1
```

Then launch the "docs consistency" batch.

## Caps

- Each batch has a cap (duration or budget) in its spec. Exceeded → one single `ask` to the
  coordinator, then stop; no status loop.
- An open-ended RE task ("continue the reference") is always split into verifiable goals
  before being handed out.

## CI

`.github/workflows/ci.yml` calls `verify.ps1 -Only …` with `RUSTFLAGS=-D warnings`; the local
`wasm` step sets the same flag (a wasm-only `dead_code` warning once passed locally and failed CI).
No more bot commits on `main`: the `README.md` progress block is updated locally by `verify.ps1`
(`coverage` step); the coverage job fails if the block is stale.

A worker never commits the README progress block (`worker-rules.md` § 2): the integration run
regenerates it and folds it into the last integration commit. A block already committed on a
worker branch conflicts when two branches carry it: drop it with `git reset --hard <last code
commit>` in that worktree.
