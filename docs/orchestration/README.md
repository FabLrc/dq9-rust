# Orchestration — coordinator workflow

Documents: this file (coordinator), `worker-rules.md` (rules shared by all workers),
`batch-template.md` (batch spec, copied to `build/batches/<batch>.md`), `prompt-dispatch.md`
(worker launch prompt). Current state: `docs/status.md` only.

## Harness and models

Always the **OMP** harness: Orca workers launched with OMP, subagents through OMP's `task`
tool. The model is chosen when the worker is launched and recorded in the batch spec.

| Model | Id | Provider | Use |
|---|---|---|---|
| DeepSeek V4.1 Flash | `opencode-go/deepseek-v4.1-flash` | OpenCode Go | **default**: docs, mechanical scripts, data collection, tests against a given contract |
| Sonnet 5.5 | `anthropic/claude-sonnet-5-5` | Anthropic | RE, challenger, design, non-trivial debugging, scripts touching user files |
| Opus 5.5 | `anthropic/claude-opus-5-5` | Anthropic | last resort after a documented Sonnet failure; justification in `docs/status.md` |

Escalation: Flash fails twice or exceeds its cap → Sonnet; Sonnet fails → Opus,
only with justification. No other model.

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
- One owner per file. Shared files (`crates/dq9-app/src/app.rs`,
  `crates/dq9-app/src/event.rs`, `crates/dq9-core/src/field.rs`, `crates/dq9-core/src/state.rs`,
  `crates/dq9-core/src/trigger.rs`, `Cargo.toml`, `re/symbols.txt`) appear in the
  ```owns block of a single batch at a time.

## Batch cycle

1. Write `build/batches/<batch>.md` from `batch-template.md` (model, base SHA, ```owns, cap).
2. Orca dispatch (OMP worker, spec model, branch `FabLrc/<milestone>-<batch>`): the
   `prompt-dispatch.md` prompt filled in, launched with the `orca orchestration worker-start`
   command given in that file.
3. Wait for `worker_done`; read the report and the evidence.
4. Checks, then integration from the main checkout on `main`, clean tree:

   ```powershell
   ./scripts/integrate.ps1 -Branch FabLrc/j3-sound-effects -Spec build/batches/j3-sound-effects.md
   ./scripts/integrate.ps1 -Branch FabLrc/j3-sound-effects -Spec build/batches/j3-sound-effects.md -Apply -Message 'audio: play field sound effects'
   # release the worker on the Orca side, then:
   ./scripts/integrate.ps1 -Branch FabLrc/j3-sound-effects -Spec build/batches/j3-sound-effects.md -Cleanup
   ```

   The checks verify: branch contains `main` (otherwise rebase), changed files within
   ```owns (except the `README.md` progress block, always accepted), no forbidden file
   (`rom/`, `build/`, `tools/`, `*.nds`, `*.sav`), clean worktree,
   `verify.ps1 -RequireRom` green and worktree still clean afterwards. `-Apply` does a
   `git merge --squash` + one `domain: summary` commit, without pushing. `-Cleanup` removes
   the junctions without following their target, then the worktree and the branch.

   Never run `orca worktree rm` on a worktree that still has `rom`/`tools` junctions
   (they lead to the only copy of the ROM): first `integrate.ps1 -Cleanup`, which removes the
   junctions without following their target. A worktree with no junction and no commit can be
   removed cleanly with `orca worktree rm --worktree path:<path> --delete-branch`.
5. Rewrite `docs/status.md` (< 60 lines, rewritten, not appended to), commit, push.

## Scripts (contract interfaces)

| Script | Example |
|---|---|
| `scripts/verify.ps1 [-Fast] [-Only <step[,step]>] [-RequireRom]` | `./scripts/verify.ps1 -Only fmt,clippy` — steps `fmt`, `clippy`, `test`, `wasm`, `coverage`; only source of the verification commands |
| `scripts/integrate.ps1 -Branch <b> -Spec <batch> [-Message <subject>] [-Apply] [-Cleanup]` | see cycle above |
| `scripts/evidence.ps1 -Batch <batch> [-Command <text>] [-Note <text>]` | `./scripts/evidence.ps1 -Batch j3-sound-effects -Command 'cargo run -p dq9-app -- --headless' -Note 'capture'` → `build/evidence/<batch>/manifest.json` |
| `scripts/prune-build.ps1` | `./scripts/prune-build.ps1` — never deletes a file listed in an evidence manifest |
| `scripts/prune-branches.ps1 [-Apply] [-Remote]` | `./scripts/prune-branches.ps1 -Remote` (list), then `-Apply`; never deletes a unique branch |
| `scripts/parity.ps1 [-References <json>] [-Rom <path>]` | `./scripts/parity.ps1` — default `build/evidence/parity/references.json`; non-zero on a non-accepted difference |

Docs cite evidence as `build/evidence/<batch>/<file>` + the first 12 characters of the
sha256.

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

`.github/workflows/ci.yml` calls `verify.ps1 -Only …`. No more bot commits on `main`:
the `README.md` progress block is updated locally by `verify.ps1` (`coverage` step) and
committed with the batch; the coverage job fails if the block is stale.
