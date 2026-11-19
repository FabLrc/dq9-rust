# Docs

Index of the documentation. Rules live in [`../AGENTS.md`](../AGENTS.md), the current state in
[`status.md`](status.md) (the only place it is recorded), the plan in [`roadmap.md`](roadmap.md).

## Where to start

| Task | Read first |
|---|---|
| Understand the port | [`architecture.md`](architecture.md): crates, rules (determinism, traceability), verification strategy. |
| Run and drive the app | [`app.md`](app.md), the index of one note per subsystem: [`app/native.md`](app/native.md) for the commands, [`app/dev-loop.md`](app/dev-loop.md) for the build loop. |
| Change `dq9-app` / `dq9-render` | [`app-structure.md`](app-structure.md) (module map, extension points), then the note of the subsystem in [`app/`](app/). |
| Reverse-engineer a format or a routine | [`re/`](re/) (one note per topic), `re/symbols.txt` for names, [`re/decomp-corpus.md`](re/decomp-corpus.md) for the Ghidra corpus, [`external-resources.md`](external-resources.md) for the published work. `AGENTS.md` § "Where to look before analysing" gives the order. |
| Run or supervise batches | [`orchestration/README.md`](orchestration/README.md) (coordinator), [`orchestration/worker-rules.md`](orchestration/worker-rules.md) (workers), [`orchestration/batch-template.md`](orchestration/batch-template.md) (spec). |
| Check what was verified | [`app/verification.md`](app/verification.md) and each note's evidence paths (`build/evidence/<batch>/`, local). |
| Speed up the Windows loop | [`build-speed.md`](build-speed.md): measured link and test times, the settings adopted from them. |

`docs/app.md` is 120 lines at most: it lists the topics of `docs/app/`. A section that moved
there is cited by its file (`docs/app/battle.md`), not by an anchor of `docs/app.md`.
