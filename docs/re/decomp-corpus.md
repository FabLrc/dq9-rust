# Decompilation corpus (`build/decomp/`)

The Ghidra project answers one question per run: `scripts/ghidra-query.ps1` costs 5–17 s
(`analyzeHeadless` start, project open, one decompilation), locks `re/ghidra` while it runs,
and cannot be parallelised without a copy of the project (104–156 MB). The corpus is the other way
round: **decompile once, grep forever**. One file per module, one block per function, plus an
address index and a call-edge list — answers come from `rg` in milliseconds, in any number of
agents at once, with no Ghidra and no lock.

Tools: `scripts/ghidra-corpus.ps1` (build), `scripts/decomp-find.ps1` (one function),
`re/ghidra_scripts/DumpCorpus.java` (the decompiler driver).

## Contents

The `game` functions of `re/functions.txt` — the same map `ApplyFunctions.java` applies to the
project — decompiled once per module into `build/decomp/<module>.c`:

| Module | Game functions | `.c` size |
|---|---|---|
| `arm9` | 5,498 | 4.5 MB |
| `ov_017` | 1,194 | 1.6 MB |
| `ov_023`, `ov_000`, `ov_024` | 827, 826, 573 | 0.8, 1.0, 0.8 MB |
| `ov_004`, `ov_001`, `ov_003` | 532, 515, 510 | 0.5, 0.4, 0.9 MB |
| 20 other overlays (3–304 each) | 1,581 | 2.7 MB |
| **total, 28 modules** | **12,056** | **13.1 MB** |

Coverage: every `game` entry of the map, 12,056 of 12,056 (the map's 2,718 `platform` and 6
`excluded` entries are not dumped: they are the Nintendo libraries `re/symbols.txt` already
names). `failures.txt` is empty.

Next to them, all rebuilt at the end of every run:

| File | Lines | Size | Content |
|---|---|---|---|
| `index.txt` | 12,056 | 570 KB | `<addr> <name> <file>:<line>` |
| `calls.txt` | 54,268 | 1.5 MB | `<caller addr> <callee addr or name>`, deduplicated, sorted |
| `failures.txt` | 0 | 0 B | `<addr> <name> <file> <reason>`, one line per failed block |
| `logs/` | — | — | the `analyzeHeadless` log of each module (ignored, kept for diagnosis) |

A block is the decompiled text with its first line as header — that is the only structure the
consumers rely on:

```
// ov_017::0218b688 0x1544 func_ov017_0218b688

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void func_ov017_0218b688(int param_1)
{
  ...
```

`<addr>` is the map's address (`0x020d91ec` for the ARM9, `ov_017::0218b688` for an overlay
space), `<size>` the map's size, `<name>` the map's name. The C body carries the program's
names: `re/symbols.txt` wins, so bodies call `Gpc2_LoadIndex` where a header shows
`_Z42LoadAndDecompressGPCHeaderAndInnerFileInfoPP7GPCFileR15ExtendedNitroVMPKcPvRjjbPb`.

## Using it

```powershell
powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 ov_017::0218c0f4   # containing function
powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 Gpc2_LoadIndex      # re/symbols.txt name
powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 0x020d91ec -Callers # who calls it
powershell -ExecutionPolicy Bypass -File scripts/decomp-find.ps1 0x020d91ec -Callees # what it calls
```

A name is an index name (`re/functions.txt`) or a `re/symbols.txt` name (`Gpc2_LoadIndex`); an
unknown name is matched as a substring and accepted when it hits exactly one function, otherwise
the candidates are listed. An address *inside* a function prints the containing function and
says where it is (`+0xa6c`), like `ghidra-query.ps1 decomp` does. `-Callers`/`-Callees` print the block and then
one line per related function (`<addr> <name> <file>:<line>`), so the next query is a copy of
an address.

Everything else is a plain `rg` over `build/decomp/*.c`, which is why `consts.txt` was not
written: `rg -n '\b0x400\b' build/decomp/*.c` answers "where is this constant" in 0.01 s
(359 hits), with the block header above each match for attribution.

The corpus is ROM-derived data: it lives under `build/` (ignored), is never committed and
never copied into committed evidence.

## Building it

```powershell
powershell -ExecutionPolicy Bypass -File scripts/ghidra-corpus.ps1              # resume, -Jobs 3
powershell -ExecutionPolicy Bypass -File scripts/ghidra-corpus.ps1 -Only ov_017 # one module
powershell -ExecutionPolicy Bypass -File scripts/ghidra-corpus.ps1 -Refresh     # everything again
```

- `-Out` defaults to `<main checkout>\build\decomp`, so a linked worktree writes the corpus the
  other worktrees read (ROM-derived text is not duplicated per worktree).
- **Resumable per module**: a module is skipped when its `.c` exists and is newer than
  `re/functions.txt`, `re/symbols.txt`, the ROM and `DumpCorpus.java`; `-Refresh` ignores that.
  A run interrupted mid-module leaves `<module>.c.tmp`, which is deleted on the next attempt
  (the `.c` only appears when the module completed).
- `-Jobs N` copies `re/ghidra` N times under `build/ghidra-jobs/` (104–156 MB each, Ghidra
  rewrites its database; the run prints
  the size and the free space first, and refuses to start below 1 GB of margin), points each
  `analyzeHeadless` at its copy and deletes the copies at the end: that is the only way to
  decompile in parallel, since Ghidra locks a project per run. `-Jobs 1` uses the project in
  place.
- `-Timeout` (default 60 s) is the per-function decompiler deadline: one pathological function
  is recorded as a failure and the module continues. `-ModuleTimeout` (default 1800 s) kills a
  whole module run.
- The indexes are rebuilt from the `.c` files at the end of every run (≈6 s over 13 MB), so
  they always describe the corpus directory, including modules skipped by `-Only`.

Regenerate when `re/functions.txt`, `re/symbols.txt` or the ROM changes — the mtime check
does it on its own. **Measured build**: 2.5–5 min wall for the 28 modules, `-Jobs 3` on 20
cores (two clean runs: 135 s and 259 s inside the script, 178 s and 371 s of decompilation
summed over the 3 workers — the disk, not the CPU, moves that spread), 0 failed functions.
`arm9` alone is 5,498 functions, 4.5 MB and 50–116 s of decompilation; a small module costs
6–20 s wall, of which ~6 s is `analyzeHeadless` start-up.

## Cost, measured

Five questions a worker actually asks, run as a user runs them
(`powershell -ExecutionPolicy Bypass -File …`), best of two series for the Ghidra side (with
`-Refresh`, so every "before" is a real `analyzeHeadless` run) and best of three runs for the
corpus. A busy machine moves both sides (one `xrefs` run took 64 s); the ratio does not change:

| Question | Before (`ghidra-query.ps1`) | After (`decomp-find.ps1`) |
|---|---|---|
| block of `0x020d91ec` | 5.5 s | 0.60 s |
| block containing `ov_017::0218c0f4` (5.4 KB function) | 10.8 s | 0.62 s |
| callers of `0x020114a4` (18 references) | 7.6 s | 0.62 s |
| callees of `0x020d91ec` (8 references) | 6.0 s | 0.65 s |
| where is constant `0x400` used | arm9 dump first (`ghidra-dump.ps1` over the ARM9: 50 s, 210 s when three jobs contend; 4.5 MB written) | `rg` over the corpus: 0.01 s |

`decomp-find.ps1` is ~0.6 s wall including PowerShell start-up; the block read itself is
15–60 ms. The Ghidra side is 5–17 s **per question**, and 8–30 s on a cold project.

## Call edges: what `calls.txt` counts

`calls.txt` is extracted from the decompiled C, over every block *after* its preamble (blank
lines, `/* WARNING */` comments and the C signature, which is a declaration and not a call —
counting it added 959 edges, one per function definition):

1. `name(` tokens, minus C keywords and decompiler macros;
2. `name` → address with both maps (`re/symbols.txt` first, then `re/functions.txt`); a name
   that maps to two different addresses is dropped rather than guessed;
3. `func_0x<hex>` / `FUN_[ov_NNN__]<hex>`, the decompiler's name for a call whose target it
   could not name — resolved **when exactly one listed function starts at that address**.
   0x021536e0 is the base of `ov_000`–`ov_006`: the ambiguity is kept as a name (245 edges)
   instead of picking a module;
4. all-caps tokens that are in neither map are decompiler intrinsics (`SBORROW4`, `CONCAT44`,
   `CARRY4`, `LZCOUNT`) and are dropped.

Result: 54,268 edges from 8,371 distinct callers to 8,884 distinct callees; 46,673 (86 %) end
on a corpus function, 551 (1.0 %, 73 distinct names) keep a name — mainly the shared overlay
bases and targets outside the map. Indirect calls (`(**(code **)(x))()`) are not edges:
`calls.txt` is the graph of *direct* call sites.

**Recall measured against `ghidra-query.ps1 xrefs`** on ten functions (five ARM9, five
overlays, 5–21 expected callers): 94 of 94 `UNCONDITIONAL_CALL` references are present, none
missing. The corpus also reports 28 edges `xrefs` does not: a call compiled into an overlay
targets a raw address in its own space (`Gpc2_LoadIndex` at `0x020d91ec` from `ov_033`/`ov_034`,
`func_0x02153710` from twenty `ov_024` functions). Ghidra has no reference there — the address
has no function in that space — while the corpus resolves the number through the map. Those
edges mean "the function the map lists at that address **when that module is resident**", which
is what the machine executes; they are why `-Callers` can list an overlay function as a caller
of an ARM9 one.

## Limits

- **Names**: headers carry `re/functions.txt` names, bodies carry the program's names
  (`re/symbols.txt`). A `func_XXXXXXXX` name keeps its *USA* address while the header address
  is the European one (`func_020727d8` sits at `0x020727e8`): the ARM9 module is shifted by
  `+0x10` (AGENTS.md), the name predates the shift. Search by address or by header name.
- **Cross-space edges** (previous section) are resolved by address, not by Ghidra references.
- **No `.bss`/data layout, no struct recovery**: only the decompiled C. Types are Ghidra's
  (`undefined4`, `undefined1 *`).
- **Missing functions**: a map entry the analysis never created is recorded as a failure
  (`failures.txt`, 0 of 12,056 here) instead of being decompiled from inferred code.
- **Staleness**: the corpus describes `re/functions.txt`/`re/symbols.txt` at build time. A new
  name in `symbols.txt` needs a rebuild of the touched modules (`-Only ov_017 -Refresh`).

## Evidence

`build/evidence/k1-ghidra-corpus/`: `timings.txt` (the table above), `recall.txt` (the ten
functions, xref counts, missing and extra edges), `corpus-summary.txt` (build output and
sizes), `failures.txt` (0 of 12,056 blocks failed), `manifest.json`.
