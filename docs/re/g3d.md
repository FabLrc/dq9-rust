# NITRO 3D files (`BMD0`, `BTX0` and animations)

Map models and textures are G3D files of Nintendo's NITRO-System, stored
compressed (LZ10) in the NARCs `data/map/<code>.ambl` (textures `.nsbtx`) and
`data/map/<code>.amdj` (models `.nsbmd`, animations `.nsbca`/`.nsbta`/`.nsbtp`/`.nsbma`).
The character archives `data/**/*.chr` (NARC) store the same files **uncompressed**
(model, motions `.nsbca`, `.nsbma`, `.nsbta`, `.nsbtp`).
Implementation: `nds_rom::g3d` (no I/O, wasm); `cargo dq9re model <file or NARC>
[--member NAME] [--anim MEMBER --frame N] --out DIR` writes each texture as PNG and each
model as `.obj` + `.mtl`, posed and animated by `--anim` at frame `N`.

## Sources

- **[apicula](https://github.com/scurest/apicula)** (0BSD): ported module by module (see
  the header of `crates/nds-rom/src/g3d/mod.rs`): container, dictionaries, `MDL0`, `TEX0`,
  `PAT0`, render commands, "pivot" and "basis" matrices, texture decoding,
  GPU commands, primitive assembly, `JNT0` track layout.
- **USA decompilation** (Unlicense), `src/NitroSDK/NSBXX/`: the G3D library as the
  game links it (`JAC.cpp`, `MAT.cpp`, `MAM.cpp`, `PatternAnimation.cpp`,
  `RenderCommandProcs.cpp`, `RenderCommands.cpp`): curve evaluation, scaling rules,
  texture matrices. European addresses in `re/symbols.txt` (section "NITRO G3D
  animation library") and in code comments.
- **Ghidra on the European ARM9**: `VEC_Normalize` (`0x020c2f28`: hardware divider
  `2^56 / |v|²`, root `sqrt(4 |v|²)`) and `FX_DivAsync`/`FX_GetDivResult` (`0x020c2d00`,
  `0x020c2c6c`: `(a << 32) / b`, rounded to `2^-12`); pivot matrix index tables
  `0x020e9270` (models) and `0x020e9294` (animations), identical to each other and to apicula.
- **GBATEK** ("DS 3D Geometry Commands", "DS 3D Texture Formats", "DS 3D Texture
  Coordinates"): GPU commands, texture formats, texture matrix in "texture coordinate
  source" mode, hardware divider and root.
- Header field names (`posScale`, `boxPosScale`, `sbcType`…): NITRO SDK structures
  `NNSG3dResMdlInfo`, `NNSG3dResMatData`, as documented by apicula and
  its documentation [nsbmd_docs](https://github.com/scurest/nsbmd_docs).

All of this is Nintendo format and library code: cited by address, never
counted as a ported game function (no `port:` marker).

## What is read

| Item | Type | Contents |
|---|---|---|
| Container | `G3dFile` | NITRO header (`BMD0`, `BTX0`, `BCA0`, `BTA0`, `BTP0`, `BMA0`), sections; animation names (`AnimSection`) |
| Dictionary | `read_dict` | Patricia tree ignored, fixed-size entries + 16-byte names (`Name`) |
| `MDL0` | `Model` | full header (`up_scale`/`down_scale`, scaling rule, texture matrix mode, counts, bounding box and `box_pos_scale`), materials, meshes, objects, inverse bind matrices, render commands, hierarchy (`object_links`) |
| Material | `Material` | `DIF_AMB`, `SPE_EMI`, `POLYGON_ATTR` + mask, `TEXIMAGE_PARAM` + mask, palette base, texture size, `magW`/`magH`, texture matrix (`tex_srt()`, `tex_matrix()`), texture and palette names |
| Object | `Object` | translation, 3×3 or compressed "pivot" rotation, scale and its inverse; `matrix()` = T·R·S |
| Pose | `ObjectTransform`, `Model::rest_pose`, `Model::pose_matrices` | exact per-object transform then matrix according to the model's scaling rule |
| Render commands | `RenderOp` | load/store a matrix, multiply by an object, skin (`Skin`), `ScaleUp`/`ScaleDown`, bind a material, draw a mesh |
| Billboard commands | `BillboardCommand`, `Geometry::billboards` | render commands `0x07` (`BB`) and `0x08` (`BBY`): where they sit in the bytecode, their parameter byte, and the draws they leave to the camera ([§ Billboards](#billboards-render-commands-0x070x08)) |
| Display list | `DisplayList` → `GpuCmd` | every GBATEK GPU command; positions in exact fixed-point (`Fx12`), normals 1.0.9, texture coordinates 1.11.4, RGB555 colors |
| Geometry | `Geometry` | `build` (rest pose), `build_posed` (given object matrices): indexed triangles in model space, normals, colors, raw normalized UVs; `apply_tex_matrices` |
| `TEX0` | `Tex0` | textures (`TextureParams`, data, 4×4 block index) and palettes |
| Decoding | `decode_texture` | RGBA8 for the 7 formats: A3I5, 4, 16, 256 colors, compressed 4×4, A5I3, direct |
| `JNT0` | `JointAnimation` | per-object tracks, `track_transform`, `transforms`, `pose`/`pose_at` |
| `SRT0` | `TexSrtAnimation` | per-material tracks (scale, rotation, translation), `srt`, `matrix` |
| `MAT0` | `MaterialAnimation` | per-material tracks (diffuse, ambient, specular, emission, alpha), `colors` |
| `PAT0` | `Pattern` | texture/palette names and keyframes per material, `keyframe`, `at` |
| Texture matrix | `TexMatrix` | the 6 useful terms of the GPU matrix, `apply` (texels), `normalized` (normalized UVs) |

Differences from apicula:

- all values stay in exact fixed-point, `cgmath` is replaced by `Fx12` and
  `Mat4` (f32, apicula's column-vector convention); animations follow the library's
  32/64-bit arithmetic (interpolations, normalization, cross products);
- an unreadable section fails the file (apicula ignores it); no read is
  out of bounds: samples are read at evaluation, a read outside the file returns an
  error;
- `Material::params()` returns the material's `TEXIMAGE_PARAM` (apicula masks it with the
  `POLYGON_ATTR` mask, which looks like a bug);
- GPU matrix commands (`MTX_PUSH/POP/STORE/IDENTITY/LOAD/MULT/TRANS`) are also
  executed, and the palette of an out-of-palette color reads 0 as in apicula;
- apicula's curve sampling ("I made this up") is replaced by the
  library's; `SRT0` (incomplete in apicula) and `MAT0` (missing) are complete.

Not ported: blending of several joint animations on one object
(`ProcessJointAnimationsOnBoneMatrix`, `0x020b3bbc`), visibility animations (`V\0AV`,
absent from the files read here),
environment mapping (`TEXIMAGE_PARAM` generation 2 and 3), lighting,
COLLADA/glTF export, animation selection by the game.

## `up_scale`

`posScale` (`up_scale`) is only applied by render command `0x0b` (`ScaleUp`), which
the converter emits only when the scale differs from 1: models with `up_scale`
= 1 have none (M08M08F4, S14M01E1). M01M0003's vertices are stored at 1/8.

## Scaling rules (`scaling_rule`)

Render command `0x06` carries `(object, parent, flags)`; the library computes the
object's transform (`RenderCommand_6`, `0x020b4c9c`: animation or rest pose) then
submits it according to the model's rule (`BoneMatrixScaleCalculationProc_Type0/1/2` and
`BoneMatrixDataSubmissionProc_Type0/1/2`, `0x020b9a3c`..`0x020ba3a0`):

- 0 (standard): `T·R·S`;
- 1 (Maya, segment scale compensation): an object with flag 1 cancels the scale of
  its parent (`T·S(parent⁻¹)·R·S`), flag 2 stores the inverse of its own scale;
- 2 (Softimage, "classic scale off"): the accumulated ancestor scale is removed then
  reapplied in the local frame, `S(acc⁻¹)·T(t∘acc)·R·S(acc)·S`, following the
  hierarchy in command order; object 0 starts with an identity accumulated
  scale (`SetUpRenderCommandHandler`, `0x020b4078`).

The file stores each scale with its inverse (6 `fx32`). In the ROM: 6,113 models with
rule 2, 126 with rule 0, none with rule 1. Rule 2 explains the two models that
overflowed their header box with `T·R·S`: M10M0600 and S14M01E1 nest scaled
objects; with the rule, every model fits in its box.

## Billboards (render commands `0x07`/`0x08`)

Trees, lamps, cave lights and other flat scenery are quads that turn to face the camera.
The model says so with render commands `0x07` and `0x08`; they name no object. The opcode's
low five bits select the operation and the top three its modifier
(`ExecuteRenderCommands`, `0x020b4030`): `0x07` is `RenderCommand_7`
(EU `0x020b50fc`, USA `0x020b50ec`, 0x308 bytes) and `0x08` `RenderCommand_8`
(EU `0x020b5404`, USA `0x020b53f4`, 0x364 bytes) — the decompilation's comments name them
"free billboarding … noticeable with evac animation" and "axis-aligned billboarding —
noticeable on trees in Angel Falls & lights in cave grottos".

**The command's parameter byte is not an object index.** The plain form (modifier 0, the only
one on the cartridge) consumes two bytes and jumps over the second without reading it
(`numBytesConsumed = 2`); the byte is read as a matrix stack slot only in the modified forms
(`0x27`/`0x47`/`0x67`: store the result in, retrieve a matrix from, the position stack). No
render command hook is installed for commands 7 or 8 either: the game's only hook is command 6
(`SetModelRenderContextRenderCommandHook(context, &BoneTrackingRenderCommandHook, 0, 6, 3)`,
`Object3D.cpp`). The value grows with the command's place in the bytecode: 4 to 15 for the
twelve billboards of `M01M0003`, 0 to 40 over the cartridge. The library reads it and ignores
it. The billboard applies to **whatever the current matrix is** when the command runs: the
`0x06`/`0x03` commands before it leave one tree node in the stack.

What both variants do with that matrix:

1. set the projection to identity, push it, and read the current **clip** matrix
   (`func_020c54fc`), combined with the camera (`RenderConfig::GetCombinedWorldViewMatrix`,
   the inverse of the matrix `func_020b39ec` recomputes, in the decompilation's USA
   numbering): the object as the camera sees it;
2. keep its translation — the pivot, where the object stands — and the lengths of its three
   axes — the scale (uniform on every placement of the ROM, so the order does not matter);
3. replace its rotation:
   - `0x08` (`BBY`): the camera's `x` becomes the object's `x`, the normalized image of the
     object's `y` axis in camera space becomes its `y`, and `(0, −y_z, y_y)` its `z`
     (`data_020f1dc0` row 0 stays the identity's `(1,0,0)`). A quad standing on the ground
     therefore stays upright and turns around the vertical to face the camera;
   - `0x07` (`BB`): the rotation is the identity (`data_020f1d78` does not touch its 3×3
     block): the object is drawn screen-aligned at its pivot.

The result is loaded back with the inverse world-view (`GXFIFO_MATRIX_POP`,
`GXFIFO_MATRIX_MODE = 2`, `LoadMat4x3`, `MultiplyMat4x3`, `ScaleMatrix`), so the vertex of
the object at `v` ends up at `pivot + camera-facing rotation · scale · v` in camera space.
`RenderCommand_8` only runs that reconstruction when the image of `y` has a non-zero `y` or
`z`; otherwise it takes the `z` axis instead (`rows[1].y == 0 && rows[1].z == 0`: the up axis
lies along the camera's `x`, which no upright quad does).

On the ROM: the 740 `data/map/*.amdj` archives the map list names hold 342 with billboard
commands, 549 models, 3,466 commands (2,761 of them `0x08`); the 738 archive sets a scene
builds expose 550 such models and 3,576 commands (`crates/dq9-render/tests/real_rom.rs`), and
no model is placed twice. `M01` (Angel Falls) has 21, all `0x08`: 12 in `M01M0003` (one per
`tre` node), 8 in `M01M0004`, 1 in `M01M00L1` (the rainbow, hidden at this time of day).
`D01` has 9, all `0x08`, in `D01M0000` (its lamps). `F01`, the field map around the village,
has none: its vegetation is drawn without billboard commands, **[unknown]** how
(`F01M0100.nsbmd`, which carries 8 of them, belongs to the archive of `M01M12`, not to `F01`).

Implementation: `nds_rom::g3d::Model::billboards` keeps each command's position in the render
op list (`BillboardCommand`), `Geometry::billboards` the draws it leaves to the camera (pivot,
normalized axes, `y_only`), `dq9_render::Scene::face_billboards` rebuilds those vertices every
frame from the render camera. A model with a billboard is copied per placement, since the
result depends on the placement's world transform. **[hypothesis]** the DS sorts the affected
translucent quads by their on-screen depth per frame; our scene sorts draws by the *static*
model-space centre of each mesh, so a billboarded quad keeps the order of its rest pose.

## Joint animations (`JNT0`, block `J\0AC`)

Header: frame count (`u16`), track count (`u16`), flags (`u32`: bit 0 interpolate
between frames, bit 1 the last frame wraps to the first; seen: 0, 1, 3), offsets
of the pivot and "basis" matrices, then one `u16` offset per track. Track `i` animates
object `i`; the high byte of its flag word designates the track actually read (always
`i` in the ROM). Flags: bit 0 no component (identity); translation bits 1-2
(1: absent, 2: rest pose, none: animated) and 3-5 constant per axis; rotation bits 6-7
(absent, rest) and 8 constant; scale bits 9-10 and 11-13 constant per axis.

An animated value is a constant (translation `fx32`, scale `fx32` + inverse, rotation
an index) or a curve `(info, offset)`: `info` bits 16-28 last frame sampled
at the reduced step, 29 16-bit samples, 30-31 `log2` of the step (1, 2 or 4 frames). With a step
of 2 or 4, intermediate frames are the average of two samples or a 3:1 blend;
frames past the last reduced frame are stored one by one. A rotation
index designates a pivot matrix (bit 15) or a "basis" one (5 `u16`: two 13-bit rows,
the third by cross product). Blended rotations are renormalized by
`VEC_Normalize`. A fractional time (1.19.12 frames) is interpolated only if bit 0 is
set, otherwise it is rounded down to the lower frame; time is clamped to `[0, frames)`
(`JACAnimationProcessingCallback`, `0x020b78cc`).

## Texture matrices and `SRT0` (block `M\0AT`)

A material with `flags` bit 0 carries a scale, a rotation (`sin`, `cos`) and a translation
(in texture sizes); in the ROM, these materials (2,297) and only they have coordinate
generation 1 ("texture coordinate source"). The library turns this into the GPU matrix
according to `tex_mtx_mode` (`MaterialTextureMatrixLoadProc_Type0..3`); each specialized variant
(no rotation, no scale…) is the general formula with identity, hence one formula per
mode in `TexMatrix::new`. All 6,239 models of the ROM use mode 3 (Softimage XSI:
rotation then translation then scale, centered on the bottom-left corner). `magW`/`magH`
then multiply the first and second rows. Coordinates in texels:
`s' = a s + c t + e/16`, `t' = b s + d t + f/16`.

`SRT0`: frame count, header word (`0x300`, `0x301`, `0x303`; bits 0-1 as in `JNT0`
**[hypothesis]**, `0x300` **[unknown]**), then a dictionary of tracks named by material,
five curves `(info, value)`: scale s, scale t, rotation, translation s, t. `info`:
bits 0-15 last frame at reduced step, 28 16-bit samples, 29 constant (in
`value`), 30-31 step. The animation replaces the material transform and forces
generation 1 (`MATAnimationProcessingCallback`, `0x020b9668`).

## Colors (`MAT0`, block `M\0AM`)

Frame count, header word (0, 1, 3), per-material tracks: five words (diffuse,
ambient, specular, emission, alpha): bits 0-15 sample offset or constant
value, 16-28 last frame at reduced step, 29 constant, 30-31 step. RGB555 colors as
`u16`, blended channel by channel; alpha as `u8` (0..31). The result replaces `DIF_AMB`,
`SPE_EMI` (keeping bit 15) and the alpha of `POLYGON_ATTR` (`MaterialColors::apply`).

## Patterns (`PAT0`)

Per track: keyframe count (`u16`), an unused `u16`, a ratio (keyframes per
frame, 1.3.12) and the key offset. The search starts at `ratio × frame` then steps back and
forward to the last key starting no later than the frame (`0x020b77a8`); we
clamp this first guess to the key count, which the library does not do. A palette
index `0xFF` keeps the current palette.

## Checks on the ROM

`crates/nds-rom/tests/real_rom.rs`, `map_g3d_files` (every `.nsbmd`, `.nsbtx`, `.nsbtp`
of the NARCs `data/map/*.ambl`, `*.amdj`, decompressed):

- 4,328 `BMD0` (one model each, none with an embedded `TEX0`), 737 `BTX0`, 525 `BTP0`
  (525 pattern animations): everything parses;
- 36,632 meshes, 31,297 materials; every display list executes:
  1,365,561 vertices, 748,435 triangles;
- 22,827 textures, 22,822 palettes, all decoded (69,462,528 texels): 15,532 in 16
  colors, 3,498 A3I5, 3,445 A5I3, 331 in 256 colors, 16 in 4 colors, 5 direct,
  no compressed 4×4;
- a texture's palette: `<name>_pl`, otherwise the one a material or pattern
  animation associates with it; only `c04m01gds28` (C04M05) has none **[hypothesis]** unused
  texture;
- the header box (× `box_pos_scale`, within 2 fixed-point units) contains the
  rest-pose vertices of every model (scaling rule applied).

`animation_files` (every animation of the map NARCs and of the 2,683 `.chr`):

- maps: 299 `.nsbca`, 873 `.nsbta`, 512 `.nsbma`, 525 `.nsbtp` (compressed); `.chr`:
  5,347 `.nsbca`, 692 `.nsbta`, 1,594 `.nsbma`, 45 `.nsbtp` (uncompressed); one animation
  per file: 5,646 `JNT0`, 1,565 `SRT0`, 2,106 `MAT0`, 570 `PAT0`, 98,684 tracks;
- every frame of every track evaluates without error, plus the half-frame of interpolated
  animations: 6,732,887 evaluations;
- track `i` always reads its own data;
- every joint animation poses the models of its own archive that have enough
  objects: 2,965 poses (rule 2 with mode-3 texture matrices: 2,955; rule 0: 10),
  all built.

End-to-end runs:

- `cargo dq9re model data/map/M01.amdj --member M01M0003.nsbmd` (Angel Falls village)
  gives 17 objects, 39 materials, 50 meshes, 1,304 vertices, 692 triangles, box
  x −44.1..0.76, y −2.5..15, z −22.6..36 (world units); seen from above, the OBJ shows the
  paths, the grass and the blue roofs. The PNGs of `M01M00T1.nsbtx` (`.ambl`) are coherent
  textures (roof tiles, grass, door).
- `--member M01M0001.nsbmd --anim M01M0001.nsbca --frame N` (the waterfall: objects `fall`,
  `nami_*`, `shibuki*`, `spr_*`, 181 frames): frame 0 renders the rest pose (box
  x −10.46..18.51, z −21.70..−16.05); at frames 30 and 60, the `spr_*` sprays slide by
  several units (up to 5.9 between frames 0 and 60) and the box extends to
  z ..−15.84 then ..−14.98.
- `data/chara_sub/s000.chr --anim walk.nsbca` (skinned character, 15 objects, looping
  9-frame walk): rest pose arms spread (x ±9.39); frames 0 and 4 in symmetric
  strides (x −7.61..7.66 then −7.66..7.61), every vertex moves (up to
  5.9 units); frame 8 returns to frame 0.
