# Worker dispatch prompt

The prompt is deliberately short: it points to the versioned files (`worker-rules.md`,
batch spec) instead of copying the rules, so it never drifts from them. Replace the fields
`<batch>`, `<model>`, `<base sha>`, `<absolute spec path>`, then copy the block as is.

```text
Batch <batch> — OMP harness, mandatory model: <model> (do not change it; subagents via `task`).

1. First read docs/orchestration/worker-rules.md: it prevails in case of conflict.
2. Read the batch spec, read-only, by absolute path (file ignored by git, absent from the
   worktree): <absolute spec path>
   (e.g. <main-checkout>\build\batches\<batch>.md).
3. Read AGENTS.md then docs/status.md. Do not read the whole docs/roadmap.md.

Rules:
- Start from <base sha>. Rebase on main only if the coordinator asks for it.
- Set `$env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\dq9-target-<batch>"` for every
  cargo/verify.ps1 call: never share a target between worktrees (`docs/app.md`,
  "Development loop").
- Only change files covered by the spec's ```owns block. Never rom/, tools/,
  build/ (except evidence), *.nds, *.sav, docs/status.md, docs/roadmap.md, README.md.
- While working: ./scripts/verify.ps1 -Fast. Once at the end:
  ./scripts/verify.ps1 -RequireRom (must pass). Do NOT commit the README.md progress block:
  the coordinator regenerates it at integration (a committed block conflicts on rebase,
  waves v2-1/v2-2).
- Evidence under build/evidence/<batch>/, then
  ./scripts/evidence.ps1 -Batch <batch> -Command '<command>' -Note '<context>'.
- Commits on the worktree branch only; do not push, do not merge.
- Blocked or cap reached: one single `ask` to the coordinator, then stop.
- Finish with `worker_done` in the format of worker-rules.md § 5.
```

Launch (from the main checkout):

```powershell
orca orchestration worker-start --spec "<filled prompt>" --worktree new-top-level `
  --repo id:8ef45a12-a7ec-4e6c-b9dc-c85007817f63 --name <batch> --model <model> `
  --base-branch main --agent omp --setup skip
```

- `--base-branch main`: without it Orca creates the worktree from `origin/main`, which lags
  behind the locally integrated `main` during a wave.
- `--agent omp`: the harness of `AGENTS.md`.
- `--setup skip`: the repo-level Orca setup hook runs `pnpm install`, irrelevant for this Rust
  repository.
