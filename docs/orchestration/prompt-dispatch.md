# Worker dispatch prompt

`scripts/dispatch.ps1 -Batch <batch>` fills the ```text block below and passes it with
`orca orchestration worker-start --spec-file`; `scripts/queue.ps1` shows what is ready. The
prompt stays deliberately short: it points to the versioned files (`worker-rules.md`, the batch
spec) instead of copying the rules, so it never drifts from them.

Placeholders, filled from the batch spec and the main checkout:

| Placeholder | Value |
|---|---|
| `<batch>` | batch name: spec `build/batches/<batch>.md`, branch `FabLrc/<batch>` |
| `<model>` | the spec's `Model:` line (or `-Model`), one of the three ids of `batch-template.md` |
| `<base sha>` | `git rev-parse --short main` at launch |
| `<absolute spec path>` | `<main checkout>\build\batches\<batch>.md` |
| `<main checkout>` | the main checkout's root |
| `<slot target>` | `%LOCALAPPDATA%\dq9-target-slot<N>`, the slot the dispatch picked |

The block is copied into `build/batches/<batch>.prompt.txt` (ignored by git): never fill it by
hand any more, and keep it the only source of the prompt.

```text
Batch <batch> — OMP harness, mandatory model: <model> (do not change it; subagents via `task`).

1. First read docs/orchestration/worker-rules.md: it prevails in case of conflict.
2. Read the batch spec, read-only, by absolute path (file ignored by git, absent from the
   worktree): <absolute spec path>
   (e.g. <main checkout>\build\batches\<batch>.md).
3. Read AGENTS.md then docs/status.md. Do not read the whole docs/roadmap.md.

Rules:
- Start from <base sha>. Rebase on main only if the coordinator asks for it.
- Set $env:CARGO_TARGET_DIR = "<slot target>" for every cargo/verify.ps1 call: that slot is
  yours (never shared with a concurrent worker, cleaned before the launch); never delete it,
  and never share a target between worktrees (docs/app.md, "Development loop").
- Set $env:DQ9_ROM to the ABSOLUTE path of the ROM in the main checkout (worker-rules.md
  section 0): a relative path makes every ROM test panic.
- Only change files covered by the spec's ```owns block. Never rom/, tools/,
  build/ (except evidence and what the spec allows), *.nds, *.sav, docs/status.md, docs/roadmap.md, README.md.
- While working: ./scripts/verify.ps1 -Fast and targeted tests only. At the end,
  ./scripts/verify.ps1 -RequireRom must pass. Do NOT commit the README.md progress block.
- Evidence under build/evidence/<batch>/, then
  ./scripts/evidence.ps1 -Batch <batch> -Command '<command>' -Note '<context>'.
- Commits on the worktree branch only; do not push, do not merge.
- Blocked or cap reached: one single `ask` to the coordinator, then stop.
- Finish with `worker_done` in the format of worker-rules.md section 5.
```

## Launch

From the main checkout:

```powershell
./scripts/queue.ps1                              # states of every spec, then the next wave
./scripts/dispatch.ps1 -Batch <batch> -DryRun    # what it would write and launch
./scripts/dispatch.ps1 -Batch <batch>            # prompt, slot, runs.json, worker-start
```

`dispatch.ps1` refuses an unknown model, a `Depends:` batch that is not integrated and an
```owns overlap with a batch of `build/batches/runs.json`; it picks the lowest free slot of
five, cleans that slot's workspace members when it last served another batch, writes the
prompt file and records the dispatch in `runs.json` (`integrate.ps1 -Cleanup` releases the
worker and removes the entry). `-RetryOf <dispatch id>` and `-Attempt <n>` record a retry.

The launch flags are fixed by the script (or by its `-Model`/`-Attempt`/`-RetryOf` arguments):
`--worktree new-top-level --name <batch>` opens the worker in a fresh worktree,
`--model <id>` is the spec's model, `--base-branch main` starts from the local integrated
`main` (not the lagging `origin/main`), `--agent omp` selects the harness of `AGENTS.md`,
`--setup skip` avoids the `pnpm install` hook, and `--repo` names this repository in Orca.
