# ROM structure (YDQP01, Europe)

Facts observed with `cargo dq9re info|stats|extract` on the European ROM
(268,435,456 bytes, header CRC `0xF9CC` valid). Every line can be checked
with the tool; hypotheses are marked **[hypothesis]**.

## Binaries

| Item | Value |
|---|---|
| ARM9 | loaded at `0x02000000`, entry `0x02000800`, 0x9BD08 bytes in ROM → 0xF4618 after BLZ decompression |
| Nitro SDK | `0x04027539` (`sdk_version` field of the module params) |
| Static module | `0x02000000..0x020F2E60` (then autoloads) |
| Static .bss | `0x020F2E60..0x021536E0` |
| ITCM autoload | `0x01FF8000`, 0x1740 bytes + 0x5B40 bss |
| DTCM autoload | `0x027E0000`, 0x60 bytes + 0x20 bss |
| ARM7 | `0x02380000`, 0x28FC4 bytes (uncompressed, probably the standard SDK ARM7 binary **[hypothesis]**) |

## ARM9 overlays

35 overlays, all BLZ-compressed. They share 6 memory slots (overlays
of the same slot are mutually exclusive):

| Address | Overlays (decompressed size) |
|---|---|
| `0x021536E0` | 0 (0x30B40), 1 (0x12120), 2 (0x19CC0), 3 (0x2D5C0), 4 (0x1D0E0), 5 (0x9740), 6 (0xCCA0) |
| `0x021842A0` | 7 (0x20), 8 (0x7300), 9 (0x6AC0), 10 (0x820), 11 (0x48E0), 12 (0x7080), 13 (0x3C40), 14 (0x5560) |
| `0x0218B5A0` | 15 (0x8FC0), 16 (0x11B20), 17 (0x4CD40), 18 (0x20), 19 (0xD80), 20 (0x2780), 21 (0x640) |
| `0x021D8A40` | 22 (0x20), 23 (0x26FA0), 24 (0x27700), 25 (0x16F20), 26 (0x64C0), 27 (0x4EE0), 28 (0x10A0), 29 (0x1EA0), 30 (0x1220) |
| `0x02200160` | 31 (0x4C820), 32 (0x20) |
| `0x022A2180` | 33 (0x8A0), 34 (0x8E0) |

Overlays 7, 18 and 22 contain only 0x20 bytes (one static initializer table
entry): "empty" slots **[hypothesis]**. Overlay 32 is also
0x20 bytes but reserves 0xA2000 bytes of .bss at `0x02200180` (buffer/heap **[hypothesis]**).

Ghidra's initial auto-analysis had detected 12,874 functions (5,692 in the
ARM9 static module, 1,790 in overlay 31, 1,068 in overlay 17, 622 in overlay 0).
The imports now also apply the bounds and names from the decompilation:
see below. These detection counts do not measure reimplemented code.

### Names and bounds in Ghidra

Order of `scripts/ghidra-import.ps1`:

1. `NdsLayout.java` builds the blocks; Ghidra runs its auto-analysis.
2. `ApplyFunctions.java` applies `re/functions.txt`: European addresses, size in
   bytes, category and name, taken from the symbols of the
   [USA decompilation](https://github.com/DQIX/dqix-decomp) (Unlicense; address
   mapping in `../external-resources.md`).
3. `ApplySymbols.java` applies `re/symbols.txt`: **our names and comments take precedence**.

Step 1 is a first headless invocation, saved before steps 2 and 3,
which run in a second invocation with `-noanalysis`. Without this split,
Ghidra runs an analysis after the post-scripts and may widen bodies again:
observed on `ov_017::021b9e30` during a fresh import.

For an existing project, `scripts/ghidra-apply-symbols.ps1` runs steps 2 and 3
without reanalysis. Both launchers accept `DQ9_GHIDRA_PROJECT`, like the queries,
to work on a copy. Close the project in the UI before use.
Warning: the full import uses `-overwrite`; it rebuilds `arm9.bin`.

Sizes describe `[entry, entry + size)` intervals that may include
constant pools, not only instructions. The importer validates the whole
file (syntax, categories, unique addresses, ranges inside an initialized block), then
applies the bodies. It keeps function metadata at known entries and
functions outside the imported ranges. Analysis-inferred functions whose
entry lies inside another imported range are removed to free
that range; their instructions are not removed.

Explicit exceptions in the current inventory:

- Two zero sizes at `ov_016::021909cc` and `ov_016::0219c514`: apply the name without
  inventing a body (label if no function exists yet).
- The declared range of `0x020c0eac` ends at `0x020c1140`, after the next entry
  `0x020c112c`. Ghidra does not allow shared function bodies: the importer clamps
  this range to `0x020c112c` exclusive and reports it. This import choice does not settle
  the origin of the overlap in the source map.
- A pre-existing local label at the same place and with the same name as an imported function
  is replaced by the function name when local symbols are applied.

The file does not state ARM/Thumb: **no mode is guessed, no additional
disassembly is started**. A defined data item or decoded instruction straddling
a known entry is cleared as a listing unit to allow creating
the function, without changing bytes or processor context. The presence of an
imported body is therefore not evidence of complete disassembly nor of correct
decompilation. `port:` markers remain reserved for actual reimplementations.

Check on a copy of the European project and after a fresh import: 14,778 bodies and
two unbounded names applied, then 332 local symbols. Re-read of all saved names
and bodies, including local priorities; second application succeeded,
with no new removal of inferred functions. A file whose second entry
falls outside the memory blocks is rejected before any mutation of the first entry.
The launchers require the success summaries and refuse Ghidra errors, which can
occur even with a native exit code 0.

## File system

7,481 named files (7,516 FAT entries, the first 35 being the overlays), all under
`data/` except `dwc/` (Nintendo Wi-Fi Connection). Level-5 engine.

| Ext. | Files | Bytes | Magic | Notes |
|---|---:|---:|---|---|
| gp2 | 1,671 | 76 M | `GPC2` | Level-5 archives of named, compressed files, 53,639 members (texts, tables, models…) — see `gpc2.md` |
| sdat | 3 | 69 M | `SDAT` | standard Nitro sound (SSEQ/SBNK/SWAR/STRM) |
| chr | 2,683 | 38 M | `NARC` | characters/effects; e.g. `data/chara_sub/s158.chr` = `.bcfg`, `BMD0` model (`.nsbmd`), `BCA0` animations (`.nsbca`), uncompressed |
| ambl | 681 | 28 M | `NARC` | maps; named members (`.bmbl`, `.dat`, `.nsbtx`) LZ10-compressed |
| amdj | 669 | 22 M | `NARC` | maps (additional data) |
| mods | 6 | 11 M | `MODS` | Actimagine Mobiclip videos |
| spr | 1,300 | 2.9 M | `01 00 03 00` | 2D sprites |
| pac | 126 | 2.3 M | — | archives with plain-text names |
| bin | 162 | 2.2 M | various | data tables |
| stb | 46 | 1.4 M | `SB2.` | **[unknown]** |
| nsarc / npc / mse | 93 | 1.1 M | `NARC` | |
| nat | 10 | 245 K | — | **[unknown]** |
| sadl | 4 | 115 K | text `#def` | |
| lia | 8 | 14 K | `LI5.` | **[unknown]** |
| NCGR/NCLR/NCER/NANR | 11 | 14 K | standard Nitro | |

NARC members are often LZ10-compressed (first byte `0x10`): e.g.
`data/map/F01M01.ambl` member 2 decompresses to `BTX0` (Nitro texture).

## Formats to document (by porting priority)

1. Contents of `gp2` archives: texts (`.bin` of `scenario/`, `.mes`), tables, scripts.
2. Nitro 3D models/textures (`BMD0`/`BTX0`/`BCA0`) in the `chr`/`ambl` NARCs.
3. `bin` data tables (items, monsters, spells) and texts.
4. `spr`, `pac`, `stb`, `nat`, `lia`.
