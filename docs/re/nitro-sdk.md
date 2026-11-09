# NitroSDK ARM9: first and second verified batches (86 routines)

These **86 routines of the European `Module::Main`** are classified **Platform**, not
ported: they will be replaced, not translated. No SDK or ROM code is
reproduced here. The translated-function counters do not increase.

## Sources and scope

- [Pinned DQIX](https://github.com/DQIX/dqix-decomp/tree/b5c617aa3113a57baba07465ede3be7a0be5328f): source names and EUR mappings from `re/functions.txt`.
- [Pinned reference NitroSDK](https://github.com/ntrtwl/NitroSDK/tree/ccbbcc8b6db4c4dbba3d503d2026978d78977840): the paths and lines in the table are relative to this reference.
- [Pinned pret/pokeheartgold wrapper](https://github.com/pret/pokeheartgold/tree/e1d5a4606640e31c44d36714ad0b5cdd9c8763fd) and its NitroSDK dependency: provenance of the reference, not DQ9 binary evidence.

DQ9 uses **4.2.7539**, the reference **4.2.30001**. The identification is
**functional/structural**, never a cross-version byte-match. Each routine
was examined individually: **D** = European decompilation, **I** =
instructions, **P** = pools read in the EUR ARM9. The table associates these observations
with the reference file and a structural marker; the ignored temporary files
under `build/` are not the only published evidence.

The whitelist simultaneously requires **Main module + EUR address + size + exact source
name**. `re/functions.txt` keeps the original name; `re/symbols.txt` records
the canonical one without inventing an ARM/Thumb mode. **`_`** suffixes are kept:
they denote the implementations, unlike the inline public wrappers.
No extension by prefix, neighbourhood or call: overlays, ITCM, CP helpers and
other callees are not absorbed. The helpers of
[`ExtendedVectorMath.cpp`](https://github.com/DQIX/dqix-decomp/blob/b5c617aa3113a57baba07465ede3be7a0be5328f/src/Graphics/ExtendedVectorMath.cpp#L1-L146)
stay **Game**; composing SDK calls or sharing a prefix does not prove the
identity of a standalone SDK routine. Neighbouring anonymous routines remain unidentified.

## Individual mappings

| EUR address | Size | Source name → SDK canonical | Reference file:lines | Structural evidence |
|---|---:|---|---|---|
| `0x020c112c` | `0xc` | `Mat2x2_WriteRotation` → `MTX_Rot22_` | `libraries/fx/src/fx_mtx22.c:44-52` | D/I — cosine at both diagonal positions, sine then negative sine off the diagonal. |
| `0x020c1138` | `0x58` | `Mat2x2_ApplyScale` → `MTX_ScaleApply22` | `libraries/fx/src/fx_mtx22.c:56-70` | D — Four signed 64-bit products, X factor for the first two elements, Y for the next two; Q12 reduction without rounding addition. |
| `0x020c1190` | `0x24` | `Mat3x3_WriteIdentity` → `MTX_Identity33_` | `libraries/fx/src/fx_mtx33.c:12-23` | D — This particular order matches the source assembly. |
| `0x020c11b4` | `0xc0` | `Mat3x3_ApplyScale` → `MTX_ScaleApply33` | `libraries/fx/src/fx_mtx33.c:87-108` | D — No rounding added. |
| `0x020c1274` | `0x1c` | `Mat3x3_WriteRotationX` → `MTX_RotX33_` | `libraries/fx/src/fx_mtx33.c:112-129` | D — Matching write order. |
| `0x020c1290` | `0x1c` | `Mat3x3_WriteRotationY` → `MTX_RotY33_` | `libraries/fx/src/fx_mtx33.c:133-150` | D — Cosine at indices 0/8 written first, sine at indices 6/2 with opposite sign, unit at index 4; same zeros and order. |
| `0x020c12ac` | `0x18` | `Mat3x3_WriteRotationZ` → `MTX_RotZ33_` | `libraries/fx/src/fx_mtx33.c:154-167` | D — Rotation block at indices 0/1/3/4, matching signs, index 8 at `0x1000`; same zero positions. |
| `0x020c12c4` | `0x2f0` | `Mat3x3_Invert` → `MTX_Inverse33` | `libraries/fx/src/fx_mtx33.c:212-274` | D — Nine-word temporary if input=output; three cofactors and determinant with `0x800` added before Q12 reduction; returns −1 if determinant is zero; asynchronous reciprocal, other cofactors computed during the wait, nine signed results; final copy if temporary, returns 0. |
| `0x020c15b4` | `0x220` | `Mat3x3_Multiply` → `MTX_Concat33` | `libraries/fx/src/fx_mtx33.c:276-331` | D — Temporary only if output=second operand; nine three-term dot products, Q12 without rounding; same unusual result order `00,01,02,12,11,10,20,21,22`, column reuse, final nine-word copy. |
| `0x020c17d4` | `0x7c` | `Mat3x3_ApplyToVector` → `MTX_MultVec33` | `libraries/fx/src/fx_mtx33.c:333-347` | D — The three coordinates are read first; three dot products successively using columns 0/3/6, 1/4/7, 2/5/8; Q12 reduction, no translation. |
| `0x020c1850` | `0x28` | `Mat4x3_WriteIdentity` → `MTX_Identity43_` | `libraries/fx/src/fx_mtx43.c:9-21` | D — diagonal 0/4/8 at `0x1000`, other elements zero, including the three translation components; matching order. |
| `0x020c1878` | `0x34` | `Mat4x3_ConvertTo4x4` → `MTX_Copy43To44_` | `libraries/fx/src/fx_mtx43.c:23-38` | D — Copy of four groups of three words; successive insertion of 0,0,0,`0x1000` in the fourth column; same groupings and pointer progression. |
| `0x020c18ac` | `0xac` | `Mat4x3_ApplyTranslation` → `MTX_TransApply43` | `libraries/fx/src/fx_mtx43.c:55-73` | D — input≠output branch with a call copying the linear block; three Q12 dot products of the requested translation with the matrix columns, added to the existing translation. |
| `0x020c1958` | `0x34` | `Mat4x3_ApplyScale` → `MTX_ScaleApply43` | `libraries/fx/src/fx_mtx43.c:94-101` | D — Direct call to the 3×3 routine identified at `0x020c11b4`, then unchanged copy of the three translation words at offsets `0x24/0x28/0x2c`. |
| `0x020c198c` | `0x20` | `Mat4x3_WriteRotationX` → `MTX_RotX43_` | `libraries/fx/src/fx_mtx43.c:105-123` | D — Sine and its negation written first at indices 5/7; unit at 0, cosine at 4/8; zero translation and same other zeros. |
| `0x020c19ac` | `0x1c` | `Mat4x3_WriteRotationY` → `MTX_RotY43_` | `libraries/fx/src/fx_mtx43.c:127-143` | D — Sine at index 6 written first, negation at 2, cosine at 0/8, unit at 4; zero translation; matching order. |
| `0x020c19c8` | `0x1c` | `Mat4x3_WriteRotationZ` → `MTX_RotZ43_` | `libraries/fx/src/fx_mtx43.c:147-162` | D — Same Z block as the 3×3 version, unit at 8 then three zero translation words; matching positions, signs and order. |
| `0x020c19e4` | `0x38c` | `Mat4x3_Invert` → `MTX_Inverse43` | `libraries/fx/src/fx_mtx43.c:175-248` | D — Cofactor inversion of the linear block with `0x800` rounding, determinant test and asynchronous reciprocal; twelve-word temporary on alias; inverse translation by three negated dot products; matching final copy and −1/0 returns. |
| `0x020c1d70` | `0x2d4` | `Mat4x3_Multiply` → `MTX_Concat43` | `libraries/fx/src/fx_mtx43.c:250-317` | D — Same alias branch with the second operand; nine results of the linear block, then translation computed in order 32/31/30 with the second operand's added; Q12 without rounding and final twelve-word copy. |
| `0x020c2044` | `0xa0` | `Mat4x3_ApplyToVector` → `MTX_MultVec43` | `libraries/fx/src/fx_mtx43.c:319-339` | D — Three coordinates captured before writes; Q12 dot products identical to the 3×3 version, then separate addition of each translation component. |
| `0x020c20e4` | `0x108` | `Mat4x3_WriteViewMatrix` → `MTX_LookAt` | `libraries/fx/src/fx_mtx43.c:341-375` | D — Complete matching sequence. |
| `0x020c21ec` | `0x2c` | `Mat4x4_WriteIdentity` → `MTX_Identity44_` | `libraries/fx/src/fx_mtx44.c:13-26` | D — Sixteen writes with Q12 unit at indices 0/5/10/15 and zeros elsewhere, in the progression order of the source assembly. |
| `0x020c2218` | `0x34` | `Mat4x4_ConvertTo4x3` → `MTX_Copy44To43_` | `libraries/fx/src/fx_mtx44.c:42-57` | D — Four successive three-word copies, systematically skipping the fourth word of each source row; same groupings and progression. |
| `0x020c224c` | `0x664` | `Mat4x4_Multiply` → `MTX_Concat44` | `libraries/fx/src/fx_mtx44.c:281-360` | D — Sixteen-word temporary only if output=second operand; sixteen four-term dot products, Q12 without rounding; matching output order and column reuse, including first row 00/01/03/02; complete final copy. |
| `0x020c28b0` | `0x14c` | `Mat4x4_MaybeWriteFrustum` → `MTX_PerspectiveW` | `libraries/fx/src/fx_mtx44.c:416-460` | D/I/P — Pool `0x020c29f8 = 0x04000290`. |
| `0x020c29fc` | `0x208` | `Mat4x4_WriteProjectionUnknown` → `MTX_OrthoW` | `libraries/fx/src/fx_mtx44.c:462-511` | D/I/P — Pool `0x020c2c00 = 0x04000290`. |
| `0x020c2c04` | `0x10` | `fix32_Divide` → `FX_Div` | `libraries/fx/src/fx_cp.c:14-18` | D — queues the quotient then fetches the rounded result. |
| `0x020c2c14` | `0x34` | `fix32_Sqrt` → `FX_Sqrt` | `libraries/fx/src/fx_cp.c:38-48` | D+P — sign test, positive path configuring SQRT in 64-bit mode, operand `x<<32`, then call to the FX reader. |
| `0x020c2c48` | `0x24` | `GetHardwareDividerResult` → `FX_GetDivResultFx64c` | `libraries/fx/src/fx_cp.c:69-72` | D+P — wait on busy `0x8000`, returns both quotient words without rounding/shift. |
| `0x020c2c6c` | `0x38` | `fix32_GetDivisionResult` → `FX_GetDivResult` | `libraries/fx/src/fx_cp.c:74-77` | D+P — same DIV wait, adds `0x80000` with carry propagation, extraction after a 20-bit shift. |
| `0x020c2ca4` | `0x30` | `fix32_QueueComputeReciprocal` → `FX_InvAsync` | `libraries/fx/src/fx_cp.c:79-85` | D+P — DIV mode `1`, 64-bit numerator whose low/high words are `0 / 0x1000`, argument denominator then zero high word; no wait. |
| `0x020c2cd4` | `0x2c` | `fix32_GetSqrtResult` → `FX_GetSqrtResult` | `libraries/fx/src/fx_cp.c:107-110` | D+P — SQRT wait, result read, adds `0x200`, 10-bit shift. |
| `0x020c2d00` | `0x28` | `fix32_QueueComputeQuotient` → `FX_DivAsync` | `libraries/fx/src/fx_cp.c:112-118` | D+P — DIV mode `1`, numerator `numer<<32`, 32-bit denominator with zero high word, immediate return. |
| `0x020c2d28` | `0x3c` | `FastIntDivide` → `FX_DivS32` | `libraries/fx/src/fx_cp.c:120-127` | D+P — DIV mode `0`, argument writes, clears the denominator's high word, busy wait, reads quotient at `0x040002a0`. |
| `0x020c2d64` | `0x3c` | `FastIntModulus` → `FX_ModS32` | `libraries/fx/src/fx_cp.c:129-136` | D+P — independent setup identical to the integer division, but reads the remainder at `0x040002a8`. |
| `0x020c2da0` | `0x34` | `Vector3fix_Add` → `VEC_Add` | `libraries/fx/src/fx_vec.c:7-16` | D — three 32-bit additions at offsets `0,4,8`, written to the destination; matching structure and component order. |
| `0x020c2dd4` | `0x34` | `Vector3fix_Subtract` → `VEC_Subtract` | `libraries/fx/src/fx_vec.c:18-27` | D — three subtractions `a-b` at offsets `0,4,8`, without conversion or rounding. |
| `0x020c2e08` | `0x3c` | `Vector3fix_InnerProduct` → `VEC_DotProduct` | `libraries/fx/src/fx_vec.c:53-59` | D — three widened signed products, 64-bit accumulation, a single `0x800` rounding addition, extraction shifted by 12 bits. |
| `0x020c2e44` | `0x84` | `Vector3fix_CrossProduct` → `VEC_CrossProduct` | `libraries/fx/src/fx_vec.c:73-88` | D — six widened signed products, three cyclic differences, 64-bit carries/borrows, adds `0x800` then Q12 extraction for each component. |
| `0x020c2ec8` | `0x60` | `Vector3fix_Length` → `VEC_Mag` | `libraries/fx/src/fx_vec.c:112-130` | D+P — 64-bit sum of the three squares, multiplied by four, SQRT64, wait, result rounded by adding one then dividing by two. |
| `0x020c2f28` | `0x118` | `Vector3fix_Normalize` → `VEC_Normalize` | `libraries/fx/src/fx_vec.c:151-178` | D+P — sum of squares; DIV64/64 in mode `2` with numerator `2^56`; SQRT64 of four times that sum; fetch of both results; reciprocal×root multiplication; three scalings with rounding matching the 45-bit extraction. |
| `0x020c3040` | `0x7c` | `Vector3fix_Distance` → `VEC_Distance` | `libraries/fx/src/fx_vec.c:230-251` | D+P — three 32-bit component differences, sum of their widened squares, factor four, SQRT64 and final rounding as in `VEC_Distance`. |
| `0x020c339c` | `0x1b8` | `fix32_Atan2` → `FX_Atan2` | `libraries/fx/src/fx_atan.c:158-244` | D+P — same partition by quadrants, axes and diagonals; constants `±3217`, `±6434`, `±9651`, `±12868`; division of the smaller magnitude by the larger, index `>>5`, signed 16-bit read, addition/subtraction by quadrant, truncated signed 16-bit result. |
| `0x020c7d44` | `0x4c` | `SaveContext` → `OS_SaveContext` | `libraries/os/src/os_context.c:68-96` | I+P — CP call with context `+0x48`, CPSR save, switch to SVC `0xd3`, SP_SVC save, mode restore, save of `r0-r14` with resume value `1`, synthetic PC+4 and initial return `0`. |
| `0x020c7d90` | `0x44` | `RestoreContext` → `OS_LoadContext` | `libraries/os/src/os_context.c:100-130` | I+P — CP call at `+0x48`, masked switch to SVC, loads SPSR, SP_SVC and PC+4, restores registers with `^` suffix, no-op instruction, exception return subtracting four from the saved PC. |
| `0x020ca5a4` | `0xbc` | `VectorizedMemset` → `MI_CpuFill8` | `libraries/mi/src/mi_memory.c:402-509` | D — non-small variant; odd destination handled by preserving the adjacent byte, value duplication, halfword then word alignment, 32-bit loop, 16/8-bit tails preserving the outer byte. |
| `0x020ca660` | `0x180` | `VectorizedInvertedMemcpy` → `MI_CpuCopy8` | `libraries/mi/src/mi_memory.c:526-755` | D — source then destination; odd-destination prologue, split by alignment XOR bit0 then bit1, halfword reconstruction on opposite parities, halfword/word paths, tail preserving the neighbouring byte. |
| `0x020ca96c` | `0xb4` | `DecompressC` → `MI_ReadUncompRL8` | `libraries/mi/src/mi_uncomp_stream.c:43-102` | D+I — destination/remaining/flags/length context at offsets `0,4,0xb,0xc`, literals or repeats by bit7, lengths `+1` or `+3`, stops on input exhaustion and saves the context. |
| `0x020caa20` | `0x184` | `DecompressA` → `MI_ReadUncompLZ8` | `libraries/mi/src/mi_uncomp_stream.c:173-295` | D+I — context at offsets `0,4,8,0xf,0x10,0x11,0x12`, eight flags, partial-length resume, extended-format distinction, constants `0x110/0x1110`, 12-bit distance plus one, copy from earlier output. |
| `0x020caba4` | `0x16c` | `DecompressB` → `MI_ReadUncompHuffman` | `libraries/mi/src/mi_uncomp_stream.c:445-542` | D+I — same nine context fields, progressive tree loading, root `context+0x1d`, navigation by six-bit offset and branch bit, terminal mark `0x80`, input/output accumulators, 32-bit emission. |

## Corrections and build limits

- `Mat4x4_MaybeWriteFrustum` is **`MTX_PerspectiveW`**, not `MTX_FrustumW`:
  seven parameters, cosine/sine division and cotangent; that cotangent is
  multiplied by `scaleW` in **32 bits**, not 64 bits. The second projection is
  `MTX_OrthoW`. Sizes include the trailing pool words.
- `DecompressC/A/B` are the **streaming RL8/LZ8/Huffman** readers of
  `mi_uncomp_stream.c`, not the single-block decompressors of `mi_uncompress.c`.
  RL8 and LZ8 use `SWPB`, the branch without `MI_USE_STRB`. In DQ9, the conditional
  RL8 addition and the Huffman root/saturation update are expanded into branches
  rather than conditional instructions: no binary identity.
- The contexts match the **ARM9 variant with `SDK_CONTEXT_HAS_SP_SVC`**;
  `RestoreContext` gets the canonical **`OS_LoadContext`**.
- The memory routines match the **non-small** variants; FX/VEC the production
  paths without assertions. These build choices and the version differences
  forbid concluding an identical compilation.
- Hardware pools are observed, not inferred from names. The body of `FX_Atan2`
  is identified, but its full table has not been compared. Degraded decompilation
  signatures are not presented as reliable prototypes.

## Effect on the inventory

Measured after the second batch, `cargo dq9re coverage`: **Game 12,056**, **Platform 2,718**,
total **14,780**, **6 exclusions**. The first batch expected 2,682 in Platform; the second
adds 36 routines (2,682 + 36 = 2,718), and each added row matches the size and
source name of the `VERIFIED_SDK` table. Totals stay those of the parent: only
routines already counted in Game move to Platform.

## Second batch: hardware primitives identified by behaviour

This batch adds **36 routines** (9 CP15, 27 G3X/display), i.e. **1,228 bytes** of code.
Each is a leaf routine whose body is only a hardware primitive: no call
into Level-5 code, no game structure. They are classified `Platform` in
`crates/dq9re/src/coverage.rs` (`VERIFIED_SDK`), with the key `func_<USA address>`: the
comparison is not made on the descriptive name. Descriptive names are in
`re/symbols.txt`, section "behaviour-identified hardware primitives".

Probable SDK canonical names are **[hypothesis]**: the routines are not
identified by reference to the source, only by their observed behaviour.

### Method

- Capstone disassembly (ARM and Thumb) of the ARM9 Main section at EUR addresses, then
  a sweep of 80 leaf candidates. Criterion: no `bl`/`blx` instruction, no exit
  outside the routine, and an I/O access (literal in `0x04000000..0x04FFFFFF`), `p15`,
  `swi` or `msr`.
- Each retained candidate was disassembled in full. Bodies ending in `bx ip` via a
  literal (tail calls) are not leaves under the rule and are excluded.
- EUR addresses: USA + `0x8` or + `0x10` depending on the range (offsets from `AGENTS.md`, checked against `re/functions.txt`).
- Semantics: from the register encoding. GBATEK bits marked `[hypothesis]`
  are not verified point by point in this batch; `[unknown]` flags an unresolved bit.

### CP15 routines (9)

| EUR address | Size | Source | Descriptive name | Encoding | Semantics | Leaf |
|---|---|---|---|---|---|---|
| `0x020c89d4` | `0x10` | `func_020c89c4` | `Cp15_EnableIRam` | control register, `orr #0x40000` | bit 18: I-RAM enable | yes, no `bl`, returns `bx lr` |
| `0x020c89e4` | `0x10` | `func_020c89d4` | `Cp15_EnableDRam` | control register, `orr #0x10000` | bit 16: D-RAM enable | yes, no `bl`, returns `bx lr` |
| `0x020c89f4` | `0x14` | `func_020c89e4` | `Cp15_GetDtcmBase` | `mrc p15,0,r0,c9,c1,0`, `and #0xfffff000` | DTCM region base | yes, no `bl`, returns `bx lr` |
| `0x020c8a08` | `0x10` | `func_020c89f8` | `Cp15_EnableProtectionUnit` | control register, `orr #1` | bit 0: protection unit enable `[hypothesis]` | yes, no `bl`, returns `bx lr` |
| `0x020c8a18` | `0x10` | `func_020c8a08` | `Cp15_DisableProtectionUnit` | control register, `bic #1` | bit 0 cleared `[hypothesis]` | yes, no `bl`, returns `bx lr` |
| `0x020c8a28` | `0x14` | `func_020c8a18` | `Cp15_UpdateDataPermission` | `mrc p15,0,r2,c5,c0,2`, `bic r2,r2,r0`, `orr r2,r2,r1`, `mcr p15,0,r2,c5,c0,2` | read-modify-write of the data access permission register: clears the bits of r0, sets those of r1 | yes, no `bl`, returns `bx lr` |
| `0x020c8a3c` | `0x8` | `func_020c8a2c` | `Cp15_WriteMpuDataRegion1` | `mcr p15,0,r0,c6,c1,0` | data protection region 1 | yes, no `bl`, returns `bx lr` |
| `0x020c8a44` | `0x8` | `func_020c8a34` | `Cp15_WriteMpuDataRegion2` | `mcr p15,0,r0,c6,c2,0` | data protection region 2 | yes, no `bl`, returns `bx lr` |
| `0x020c9c00` | `0xc` | `func_020c9bf0` | `Cp15_WaitForInterrupt` | `mov r0,#0`, `mcr p15,0,r0,c7,c0,4` | wait for interrupt | yes, no `bl`, returns `bx lr` |

### G3X and display routines (27)

| EUR address | Size | Source | Descriptive name | Registers | Semantics | Leaf |
|---|---|---|---|---|---|---|
| `0x02020978` | `0x34` | `func_02020968` | `Disp_WriteBg0CntB` | `0x04001008`: `ldrh`, `and #0x43`, OR of r0..r4 at shifts 14/7/8/2/13, `strh` | packing of engine B BG0CNT: size, colour, screen base, character base, overflow | yes, no `bl`, `push`/`pop` |
| `0x020209ac` | `0x34` | `func_0202099c` | `Disp_WriteBg1CntB` | `0x0400100a`, same packing | engine B BG1CNT | yes, no `bl`, `push`/`pop` |
| `0x0208c9dc` | `0x34` | `func_0208c9cc` | `Disp_WriteBg0CntB_Alt` | `0x04001008`, `and #0x43`, `push {r4,lr}`/`pop {r4,pc}` | same register as `Disp_WriteBg0CntB`; argument layout not traced `[unknown]` | yes, no `bl`, `pop {r4,pc}` |
| `0x020938dc` | `0x24` | `func_020938cc` | `Disp_IsPowcnt1Bit15Clear` | reads `0x04000304` bit 15 | returns 1 if the bit is 0 (inverted) | yes, no `bl`, returns `bx lr` |
| `0x020c384c` | `0x28` | `func_020c383c` | `Disp_SetDispstatLyc` | writes `0x04000004`: LYC bits 8–15, bit 7 from r0 bit 8 | VCount compare line; bit 7 `[unknown]` | yes, no `bl`, returns `bx lr` |
| `0x020c3874` | `0x34` | `func_020c3864` | `Disp_SetDispstatVBlankIrq` | `0x04000004` bit 3 (`orr`/`bic #8`) | VBlank IRQ enable `[hypothesis]` | yes, no `bl`, returns `bx lr` |
| `0x020c3994` | `0x1c` | `func_020c3984` | `Disp_SetDispcntBMode` | `0x04001000`: `bic #7`, then `orr r0` | BG mode, DISPCNT B bits 0–2 | yes, no `bl`, returns `bx lr` |
| `0x020c5598` | `0x28` | `func_020c5588` | `G3_SetClearColorDepth` | `0x04000350` (CLEAR_COLOR), `0x04000354` (CLEAR_DEPTH) | 3D clear colour and depth; bit 15 = fog flag `[hypothesis]` | yes, no `bl`, returns `bx lr` |
| `0x020c5660` | `0x30` | `func_020c5650` | `G3_GetGxstatMatrixLevel` | reads `0x04000600` (GXSTAT) | -1 if bit 14; otherwise matrix stack level, bits 8–12 `[hypothesis]` | yes, no `bl`, returns `bx lr` |
| `0x020c5690` | `0x30` | `func_020c5680` | `G3_GetGxstatBit13` | reads `0x04000600` | -1 if bit 14; otherwise bit 13 `[unknown]` | yes, no `bl`, returns `bx lr` |
| `0x020c56c0` | `0x2c` | `func_020c56b0` | `G3_GetBoxTestResult` | reads `0x04000600` | -1 if bit 0; otherwise bit 1 (box test result `[hypothesis]`) | yes, no `bl`, returns `bx lr` |
| `0x020cedf0` | `0x1c` | `func_020cede0` | `Disp_IsPowcnt1Bit0Set` | reads `0x04000304` bit 0 (`tst #1`, `movne #1`) | returns 1 if POWCNT1 bit 0 is 1; hardware role `[unknown]` | yes, no `bl`, returns `bx lr` |
| `0x020daf30` | `0x1c` | `func_020daf20` | `Disp_SetDispcntBBits8To12` | writes `0x04001000` bits 8–12 | engine B BG enables | yes, no `bl`, returns `bx lr` |
| `0x020daf64` | `0x18` | `func_020daf54` | `Disp_GetDispcntBBits8To12` | reads `0x04001000` bits 8–12 | reads the same range | yes, no `bl`, returns `bx lr` |
| `0x020dc018` | `0x18` | `func_020dc008` | `Disp_GetDispcntBBits8To12_Copy` | reads `0x04001000` bits 8–12 | body identical to `Disp_GetDispcntBBits8To12` | yes, no `bl`, returns `bx lr` |
| `0x020e2b88` | `0x1c` | `func_020e2b78` | `Disp_SetDispcntBBits13To15` | writes `0x04001000` bits 13–15 | engine B WIN0, WIN1, OBJWIN windows | yes, no `bl`, returns `bx lr` |
| `0x020e2bbc` | `0x18` | `func_020e2bac` | `Disp_GetDispcntBBits13To15` | reads `0x04001000` bits 13–15 | reads the same range | yes, no `bl`, returns `bx lr` |
| `0x020e29c8` | `0x34` | `func_020e29b8` | `Disp_SetWin1RectB` | H `0x04001042`, V `0x04001046` (engine B) | WIN1 rectangle, arguments x1,y1,x2,y2 | yes, no `bl`, returns `bx lr` |
| `0x020e29fc` | `0x34` | `func_020e29ec` | `Disp_SetWin0RectB` | H `0x04001040`, V `0x04001044` (engine B) | WIN0 rectangle | yes, no `bl`, returns `bx lr` |
| `0x020e2a30` | `0x34` | `func_020e2a20` | `Disp_SetWin1RectA` | H `0x04000042`, V `0x04000046` (engine A) | WIN1 rectangle | yes, no `bl`, returns `bx lr` |
| `0x020e2a64` | `0x34` | `func_020e2a54` | `Disp_SetWin0RectA` | H `0x04000040`, V `0x04000044` (engine A) | WIN0 rectangle | yes, no `bl`, returns `bx lr` |
| `0x020e2a98` | `0x28` | `func_020e2a88` | `Disp_SetWininB1Bits` | WININ B `0x04001048` bits 8–13 | WIN1 masks, effect flag bit 13 | yes, no `bl`, returns `bx lr` |
| `0x020e2ac0` | `0x28` | `func_020e2ab0` | `Disp_SetWininB0Bits` | WININ B `0x04001048` bits 0–5 | WIN0 masks, effect flag bit 5 | yes, no `bl`, returns `bx lr` |
| `0x020e2ae8` | `0x28` | `func_020e2ad8` | `Disp_SetWininA1Bits` | WININ A `0x04000048` bits 8–13 | WIN1 masks (engine A) | yes, no `bl`, returns `bx lr` |
| `0x020e2b10` | `0x28` | `func_020e2b00` | `Disp_SetWininA0Bits` | WININ A `0x04000048` bits 0–5 | WIN0 masks (engine A) | yes, no `bl`, returns `bx lr` |
| `0x020e2b38` | `0x28` | `func_020e2b28` | `Disp_SetWinoutBBits` | WINOUT B `0x0400104a` bits 0–5 | outside-window masks (engine B) | yes, no `bl`, returns `bx lr` |
| `0x020e2b60` | `0x28` | `func_020e2b50` | `Disp_SetWinoutABits` | WINOUT A `0x0400004a` bits 0–5 | outside-window masks (engine A) | yes, no `bl`, returns `bx lr` |

### Remaining candidates (unclassified)

- `0x020c9bfc`: WRAMCNT bank control (`0x04000247`), outside G3X and display.
- `0x020292ec` (0x98): G3 geometry writes `0x04000444..0x040004C0`, multiple
  fixed-point arguments; undecided.
- `0x020c54a4`: two register paths plus a branch on `r0`; composite.
- `0x020db9a8`: reads field `[r0,#0x54]` of a game structure.
- `0x02074b04`, `0x02074b64`: save BG state into a structure.
- `0x02074c18`, `0x02074e54`: clear a 0x23c-byte structure.
- Tail calls via literal (`bx ip`): `0x020c516c` (sound FIFO), `0x020c555c`
  (`0x04000330`), `0x020c5574` (`0x04000360`), `0x020dbfe0` (`0x0400106c`), `0x020dbff4`
  (`0x0400006c`). These are primitive wrappers, not leaves under the rule.
- Sound writes `0x04000400..0x040004FF`: outside G3X and display.
- `0x020cd5a4`, `0x020cd5e4` (DIVCNT), `0x020d1478`, `0x020d14c0` (EXMEMCNT), `0x020d6cac`
  (IME), `0x020d86e0` (`0x04000540`): hardware registers outside this batch's scope.
- `swi`/`msr` routines: `0x0200bbe0`, `0x0200bc78`, `0x0200bd10`, `0x0200bdb4`,
  `0x0200bedc`, `0x0200d894`, `0x0200d8a8`, `0x0200d8bc`, `0x020c0338`, `0x020c08ac`,
  `0x020c8b54`, `0x02000970`, `0x02000ab0`: to classify after reading the call context.

These candidates remain unclassified. The Platform count only increases by the routines
listed above.
