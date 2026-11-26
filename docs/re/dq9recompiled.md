# DQ9Recompiled: European hooks, names and structure offsets

[`birabittoh/DQ9Recompiled`](https://github.com/birabittoh/DQ9Recompiled) (MIT, C++) statically
recompiles **our exact ROM** (Europe `YDQP`, SHA-1 `ff761d349709f329c8ac4bd0023fdce21861e8a1`,
checked on our dump) into native code with [`ndsrecomp`](https://github.com/mstan/ndsrecomp) and
patches the result through **hooks at European addresses**. Its hook manifests and handlers name
functions, call sites and structure offsets that we would otherwise have to find in Ghidra.

This note imports those **facts** (addresses, roles, offsets), cross-checks them against our own
corpus, and states what we did not import (code) and why. It is the record for
`re/symbols.txt` § "Cross-checked against DQ9Recompiled".

Read: `f6e8571ad3ce2f096f657443825ce584b4091e6a` (2026-11-18), submodules `ndsrecomp`
`2b27915dcca31b551de0942833a6faf648cff153` and `recomp-net`
`7a4d330ecf995b881d167234da3c0b01675fd512` (clone `--depth 1`, without submodules; the
checkout under `tools/dq9recompiled` still answers `git rev-parse HEAD` with the same commit).
The ROM SHA-1 in `game.toml` and `.recomp.json` matches ours byte for byte
([`tools/dq9recompiled/.recomp.json`](../../tools/dq9recompiled/.recomp.json), the `original`
entry).

## 1. What it is, and what it is not

- **Static recompilation, not emulation per title**: ARM9 main, ARM7, the overlays (35 slots in
  `game.toml`) and a recompiled FreeBIOS are lifted to C ahead of time; interpreted tiers only
  cover code the guest copies into RAM (`game.toml`, `CMakeLists.txt`). There is no
  per-instruction fetch of the immutable banks.
- **Same addresses as ours**: every one of the 155 ARM9 hook points lands in a function of the
  **same overlay number** in `re/functions.txt`. Our `ov_NNN` numbering (from the USA
  decompilation) therefore matches the ROM's overlay index, and no address conversion is
  needed between the two projects. This is checked mechanically (see § 3).
- **Hooks are a build-time mechanism**: `nds_recompile --hooks <bank>.toml` bakes a direct C
  call at a guest instruction (`ndsrecomp/docs/midasm-hooks.md`). Nothing is patched at run
  time, and a hook costs one call; `on_true` / `on_false` / `jump_address` can also skip or
  bend the guest code. Changing a hook means rebuilding that bank.
- **A player-facing port**: 60 FPS modes, mouse menus, language switching, netplay, Discord
  presence. Those features are interesting as *evidence of where the code is*, not as a design
  to copy.

## 2. Licence boundaries

- The repository's source is **MIT**; facts and short paraphrases are reusable with attribution
  (this note, the `# DQ9Recompiled <file>` symbol comments).
- A **built runner links GPL-3.0-or-later melonDS** (`LICENSE-GPL`, README § License). We never
  copy the runner, never vendor its hooks, never commit generated banks: the banks are code
  derived from the ROM, and the hook sources embed ROM-derived tables (`scripts/gen-map-names.py`,
  `res/lang/*.toml` are text data).
- We reimplement from the ROM. Where a handler implements game logic (damage, EXP, drops), the
  fact we take is the *address and the field it reads*; the implementation stays ours
  (`crates/dq9-core`), as with every other source in `docs/external-resources.md`.

## 3. How the cross-check was done

1. Read all 15 manifests (`config/hooks/*.toml`, 157 `[[hook]]` entries in total) and the
   comments of the handler sources that carry those hooks (`src/hooks/*.cpp|h`), plus
   `dq9_scene.h`, `dq9_text.h`, `options_model.h`, `save_image.h`.
2. Extracted every entry (address, handler, anchor instruction, file) into a local `hooks.tsv`
   and the grouped table of § 4; the extraction is reproducible from the manifests under
   `tools/dq9recompiled/config/hooks/`, which the counts above were re-read against.
3. Resolved each address against `re/functions.txt` (is it a function start? which function
   contains it?) and `re/symbols.txt`.
4. Re-read 11 functions in the decompilation corpus (`scripts/decomp-find.ps1`, which prints
   them on demand from the main checkout's `build/decomp`). These confirm the
   instruction-level claims sampled below, including the OS/owner-info layout, the quest-log
   remove, the 2D-window redraw, the VBlank wait, the gold share and the reward apply.

**Counts**

| | |
|---|---:|
| Manifests read | 15 |
| Hook points (distinct `(bank, address)`) | 157 |
| ARM9 hook points resolved to a function of the same overlay | 155 / 155 |
| ARM7 hook points (ARM7 is not in `re/functions.txt`, which is ARM9 only) | 2 |
| At an address we already name exactly (`re/symbols.txt`) | 3 |
| Inside a function we already name (a `re/symbols.txt` name or a C++ name from the USA decompilation) | 10 |
| New location (only a `func_…` placeholder, or not listed) | 144 |
| Contradictions with our notes or code | 0 |
| Symbols added (`re/symbols.txt`) | 28 (20 functions, 8 `g_…` data) |

The three exact hits are `Gx_SwapBuffers` (`0x020d86e0`), `Font_Kerning` (`0x020425f4`) and
`Field_UpdateTask` (`ov_017::0218cbd4`). The ten semantic-container hits include `main`
(`0x02000e2c`, `0x02000fd8`), `LightingManager::RecomputeAdvancedLighting` (fog hooks),
`SafeAllocator::Allocate`, `KeyManager::RefreshKeys`, `VRAMStagingManager::GetStagingSpaceAllocation`
(ITCM), `SubmitCommandToGeometryFifo`, `Model3D::TestVisible` and the two getters of § 7.

## 4. Hook points

The anchor is the instruction the source says it hooks; hook names are the handler identifiers.
"Status" follows § 3: **confirmed** = the address is at or inside a function we already name;
it does **not** mean the role was known. Grouped by handler, all addresses listed.

| Hook | Address(es) | Anchor read in the source | File | Status |
|---|---|---|---|---|
| `Dq9HeapBlockLookup` | `0x01ff88c4` | function entry (r1 = pointer, lr = return address) | dq9_arm9.toml | confirmed |
| `Dq9LanguageReload` | `0x02000e2c` | mov r0, r7 (main loop head) | dq9_arm9.toml | confirmed |
| `Dq9SkipOpeningMovie` | `0x02000fd8` | cmp r1, #0 ; bne, r1 = ctx+0x7f72 | dq9_arm9.toml | confirmed |
| `Dq9MessageTopClick` | `0x0201232c` | strh r0, [r5] (keys held this frame) | dq9_arm9.toml | confirmed |
| `Dq9BoxTestWait` | `0x020173f8`, `0x0203f6f8`, `0x0207ea14` | poll loop head, three box-test sites | dq9_arm9.toml | new |
| `Dq9TopMapPan` | `0x02024794` | str r5, [r4, #0x44] (map centre x/y) | dq9_arm9.toml | new |
| `Dq9CameraDistanceNear` | `0x0202e138` | bl MTX_PerspectiveW | dq9_arm9.toml | new |
| `Dq9CameraDistance` | `0x0202e4f8` | ldr r0, =0x0210a250 (before publishing the position) | dq9_arm9.toml | new |
| `Dq9TitleTextAlloc` | `0x02032554` | push {r3-r5, lr} (heap, size) | dq9_arm9.toml | confirmed |
| `Dq9StylusDeadZone` | `0x02038324`, `0x02096a1c` | cmp r0, #0x8000 (pen distance, walk / arrow) | dq9_arm9.toml | new |
| `Dq9AnalogMovement` | `0x020383f0` | cmp r3, r2 (direction code, angle + yaw) | dq9_arm9.toml | new |
| `Dq9KerningLookup` | `0x020425f4` | function entry (previous char, char, font) | dq9_arm9.toml | confirmed |
| `Dq9TextWindowHalfRate` | `0x020439dc` | beq 0x02043e94 (2D window up) | dq9_arm9.toml | new |
| `Dq9TextWindowSkipUpload` | `0x02043c2c` | beq 0x02043c98 (tile buffer descriptor) | dq9_arm9.toml | new |
| `Dq9TextAdvanceRightClick` | `0x02044168` | cmp r0, #0 (advance flag) | dq9_arm9.toml | new |
| `Dq9TextBoxBegin` | `0x02044514`, `0x02044534` | before bl 0x02066d00 (target and plain paths) | dq9_arm9.toml | new |
| `Dq9TextBoxEnd` | `0x02044530`, `0x0204453c`, `0x0205cb5c` | pop {r4, pc} / single exit | dq9_arm9.toml | new |
| `Dq9Blit4bpp` | `0x0204e1d8` | function entry, 4bpp glyph/frame blitter | dq9_arm9.toml | new |
| `Dq9CameraDistanceFixedFogPreset` | `0x02051b78` | bl G3X_SetFog (fixed preset) | dq9_arm9.toml | confirmed |
| `Dq9CameraDistanceFixedFog` | `0x02051d64` | beq 0x02052238 (fog mode 1) | dq9_arm9.toml | confirmed |
| `Dq9CameraDistanceFog` | `0x02052208` | ldr r1, [r7, #0x50] (fog mode) | dq9_arm9.toml | confirmed |
| `Dq9MouseMenuListPress` | `0x0205bc70` | cmp r0, #0 (press edge, widget) | dq9_arm9.toml | new |
| `Dq9MouseMenuListPosition` | `0x0205bc8c` | ldr r1, [sp, #4] (pen x/y) | dq9_arm9.toml | new |
| `Dq9MouseMenuListSame` | `0x0205bcc4` | movne r0, #0 (same entry as before) | dq9_arm9.toml | new |
| `Dq9MouseMenuListDone` | `0x0205c65c` | b 0x0205c6c4 (cursor moved) | dq9_arm9.toml | new |
| `Dq9ChoiceWindowBegin` | `0x0205c984` | mov r10, r0 (choice list draw entry) | dq9_arm9.toml | new |
| `Dq9QuestFullCheck` | `0x020617f0` | cmp r1, #0 (count, which question) | dq9_arm9.toml | new |
| `Dq9QuestSlotBase` | `0x02062cb4`, `0x0209522c`, `0x02096164`, `ov_003::02157540`, `ov_023::021f3234`, `ov_023::021f3380`, `ov_023::021f34d4`, `ov_023::021f360c`, `ov_024::021df770` | every walk of the quest log slots (`mgr+4`) | dq9_arm9.toml, ov003, ov023, ov024 | new |
| `Dq9TextInstant` | `0x02065d14` | add r2, r3, r4, lsl #4 (characters per frame) | dq9_arm9.toml | new |
| `Dq9MouseMenuTapPosition` | `0x02080de4` | entry of the layout-menu tap routine (x, y) | dq9_arm9.toml | new |
| `Dq9MouseMenuTapChoose` | `0x02080e28` | tst r0, #4 (item touched bit) | dq9_arm9.toml | new |
| `Dq9QuestAcceptLimit` | `0x020961d4` | cmp r0, #8 (accepted count) | dq9_arm9.toml | new |
| `Dq9QuestInsert` | `0x02096318` | cmp r0, #8 (insert refuses at 8) | dq9_arm9.toml | new |
| `Dq9QuestRemove` | `0x0209646c` | function entry (mgr, quest id) | dq9_arm9.toml | new |
| `Dq9StylusArrowStub` | `0x02096d80` | str r0, [sp] (stub length) | dq9_arm9.toml | new |
| `Dq9StylusArrowPiece` | `0x02096e18`, `0x02096f10`, `0x02096f48` | ring, head, dot draws | dq9_arm9.toml | new |
| `Dq9StylusArrowShaft` | `0x02096eb0` | ldr r0, [sp] (shaft length) | dq9_arm9.toml | new |
| `Dq9StylusArrowLength` | `0x02096ebc` | bl 0x020c2c04 (length - 5 px) | dq9_arm9.toml | new |
| `Dq9UnlockCameraRotation` | `0x020a2364`, `0x020a23c8` | cmp r6, r1 (yaw clamp, L and R) | dq9_arm9.toml | new |
| `Dq9QuestSave` | `0x020aa928` | bl 0x02075adc (save the 0xa0-byte block) | dq9_arm9.toml | new |
| `Dq9QuestLoad` | `0x020ab104`, `0x020ab4c0` | after the 0xa0-byte copy into mgr | dq9_arm9.toml | new |
| `Dq9GeBufferCommand` | `0x020b6adc` | function entry, geometry command buffer | dq9_arm9.toml | confirmed |
| `Dq9Language` | `0x020c99f8` | strb r2, [r4] (firmware language) | dq9_arm9.toml | new |
| `Dq9GxFifoSend` | `0x020ca440` | function entry (src, dst, bytes) | dq9_arm9.toml | new |
| `Dq9WindowLayers` | `0x020d86e0` | Gx_SwapBuffers entry | dq9_arm9.toml | confirmed |
| `Dq9MouseMenuChoicePress` | `0x020e1e70` | cmp r1, #0 (press edge, hit-test) | dq9_arm9.toml | new |
| `Dq9MouseMenuChoicePosition` | `0x020e1e84` | ldr r1, [sp, #4] (pen x/y) | dq9_arm9.toml | new |
| `Dq9MouseMenuChoiceSame` | `0x020e26cc` | cmp r2, r0 (cursor vs entry) | dq9_arm9.toml | new |
| `Dq9MouseMenuChoiceTouched` | `0x020e26d8` | cmp r1, #0 (touched before) | dq9_arm9.toml | new |
| `Dq9BattleMultiplierSnapshot` | `ov_000::0215ce84` | mov r5, r0 (battle init) | dq9_arm9_ov000.toml | new |
| `Dq9CampOptionsList` | `ov_002::02156154` | the Misc. list is built | dq9_arm9_ov002.toml | new |
| `Dq9MouseMenuListGreen` | `ov_002::0215bbe4`, `ov_003::0215c124`, `ov_003::0215db64`, `ov_003::0216aa60`, `ov_005::02158f0c`, `ov_012::02185d34`, `ov_023::021da214` | beq (pen not down: hover green) | ov002, ov003, ov005, ov012, ov023 | new |
| `Dq9CampOptionsSize` | `ov_002::0215bed8` | ldrb r0, [r6] (window size) | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsTitle` | `ov_002::0215f53c` | mov r1, r0 (list window title) | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsLabel` | `ov_002::0215f5e4` | mov r2, r0 (entry text) | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsDescription` | `ov_002::0215f690` | mov r1, r0 (entry description) | dq9_arm9_ov002.toml | new |
| `Dq9ZoomIndoors` | `ov_002::02165cdc` | cmp r7, #2 (Zoom kind) | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsOpen` | `ov_002::021661b0` | sub-state 0, cursor reset | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsFrame` | `ov_002::021661e0` | sub-state 1, every frame | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsRedraw` | `ov_002::0216627c` | cmp r0, #0 (redraw the list) | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsChoose` | `ov_002::02166344` | cmp r0, #8 (chosen entry code) | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsBack` | `ov_002::02166580` | cmp r0, #0 (B closes) | dq9_arm9_ov002.toml | new |
| `Dq9CampOptionsExit` | `ov_002::0216659c` | end of the Misc. step | dq9_arm9_ov002.toml | new |
| `Dq9MouseMenuTapPress` | `ov_003::02155150`, `ov_003::021608dc`, `ov_003::02176628`, `ov_006::02158ac4`, `ov_014::02186f44`, `ov_017::021ab0f4` | cmp r1, #0 (press edge, six scenes) | ov003, ov006, ov014, ov017 | new |
| `Dq9PowerOffScreen` | `ov_003::0215b1dc` | push {r3-r7, lr} (power-off screen) | dq9_arm9_ov003.toml | new |
| `Dq9MouseMenuKeyPress` | `ov_003::0215f350` | cmp r1, #0 (keyboard press) | dq9_arm9_ov003.toml | new |
| `Dq9MouseMenuKeyPosition` | `ov_003::0215f364` | after bl 0x02012a94 (pen x/y) | dq9_arm9_ov003.toml | new |
| `Dq9MouseMenuKeyFound` | `ov_003::0215f3f0` | cmp r7, #0 (key under the pen) | dq9_arm9_ov003.toml | new |
| `Dq9OptionsRowText` | `ov_004::0216b04c` | after bl 0x021f89f4 (cell) | dq9_arm9_ov004.toml | new |
| `Dq9SaveRowCell` | `ov_004::0216b498` | entry of the rows' builder | dq9_arm9_ov004.toml | new |
| `Dq9OptionsRowHideIcon` | `ov_004::0216c904`, `ov_004::0216ca70`, `ov_004::0216cc1c` | modes 1, 2, 5 (WFC icon) | dq9_arm9_ov004.toml | new |
| `Dq9SaveRowLayout` | `ov_004::0216cedc`, `ov_004::0216cf18`, `ov_004::0216cf5c` | bl 0x021f7eb8 (panel calls) | dq9_arm9_ov004.toml | new |
| `Dq9OptionsMenu` | `ov_004::0216d6a0` | handler 10 (WFC setup entry) | dq9_arm9_ov004.toml | new |
| `Dq9MouseMenuEquipPress` | `ov_005::021562ec` | cmp r1, #0 (equipment pen press) | dq9_arm9_ov005.toml | new |
| `Dq9MouseMenuEquipClosing` | `ov_005::02156338` | beq (page not closing) | dq9_arm9_ov005.toml | new |
| `Dq9MouseMenuEquipSkipModel` | `ov_005::021563e0` | bl 0x0215690c (model hit-test) | dq9_arm9_ov005.toml | new |
| `Dq9MouseMenuEquipCell` | `ov_005::02156828` | cmp r3, r1 (cell item, -1 empty) | dq9_arm9_ov005.toml | new |
| `Dq9MouseMenuEquipPicked` | `ov_005::02156880` | after bl 0x0205bb14 (picked) | dq9_arm9_ov005.toml | new |
| `Dq9MouseMenuEquipCategory` | `ov_005::0215b3c8` | after bl 0x0205bf68 (category row) | dq9_arm9_ov005.toml | new |
| `Dq9MouseMenuEquipStep` | `ov_005::0215be84`, `ov_005::0215bee8`, `ov_005::0215bf6c` | cmp press edge (three page steps) | dq9_arm9_ov005.toml | new |
| `Dq9MouseMenuCharPress` | `ov_009::02188f24` | cmp r1, #0 (character creation) | dq9_arm9_ov009.toml | new |
| `Dq9MouseMenuCharHit` | `ov_009::02188f44` | add r0, r6, #3 (what the pen hit) | dq9_arm9_ov009.toml | new |
| `Dq9ChatForceClose` | `ov_012::02184c4c` | cmp r0, #0 (pulled out of the menus) | dq9_arm9_ov012.toml | new |
| `Dq9ChatClose` | `ov_012::02184cd4` | add r2, r6, #0x1000 (closing all) | dq9_arm9_ov012.toml | new |
| `Dq9ChatProfileMenu` | `ov_012::02186b0c` | cmp r0, #0 (menu entry chosen) | dq9_arm9_ov012.toml | new |
| `Dq9ChatGreetingText` | `ov_012::02188d34` | after the greeting was copied in | dq9_arm9_ov012.toml | new |
| `Dq9ChatGreetingBack` | `ov_012::02188f84` | cmp r0, #0 (B on the text) | dq9_arm9_ov012.toml | new |
| `Dq9ChatGreetingStep` | `ov_012::02188fbc` | cmp r0, #0xb (keyboard result) | dq9_arm9_ov012.toml | new |
| `Dq9ChatGreetingSave` | `ov_012::02189140` | bl 0x020426cc (save the greeting) | dq9_arm9_ov012.toml | new |
| `Dq9ChatGreetingPrompt` | `ov_012::0218ab84` | mov r1, #1 (keyboard title) | dq9_arm9_ov012.toml | new |
| `Dq9FrameRate` | `ov_017::0218c78c` | b 0x0218c798 (VBlank wait bound) | dq9_arm9_ov017.toml | new |
| `Dq9TopScreenClick` | `ov_017::0218cbd4` | field step entry | dq9_arm9_ov017.toml | confirmed |
| `Dq9InteractPromptTouch` | `ov_017::0219acb0` | cmp r3, r5 (prompt touch area) | dq9_arm9_ov017.toml | new |
| `Dq9InteractPrompt` | `ov_017::0219b088` | bl 0x0204725c (prompt animation) | dq9_arm9_ov017.toml | new |
| `Dq9ChatFieldMenu` | `ov_017::021a53a0` | cmp r8, #0 (X in the field) | dq9_arm9_ov017.toml | new |
| `Dq9ChatQueueTask` | `ov_017::021a53fc` | ldr r1, [r1, #0x71c] (camp menu task) | dq9_arm9_ov017.toml | new |
| `Dq9WorldIconBegin` | `ov_017::021aed10`, `ov_017::021aedd4`, `ov_017::021b6150`, `ov_017::021bfd94` | bl 0x020473d8 (world-point icons) | dq9_arm9_ov017.toml | new |
| `Dq9WorldIconEnd` | `ov_017::021aed14`, `ov_017::021aedd8`, `ov_017::021b6154`, `ov_017::021bfd98` | after the draw | dq9_arm9_ov017.toml | new |
| `Dq9ChatProfileTask` | `ov_017::021c17fc` | entry of the Profile Settings task | dq9_arm9_ov017.toml | new |
| `Dq9SkipBootLogos` | `ov_020::0218bf38` | cmp r0, #0 ; bne (boot phase) | dq9_arm9_ov020.toml | new |
| `Dq9BattleExpScale` | `ov_023::021ee4f4` | ldr r1, [r7, #0x29c] (EXP per slot) | dq9_arm9_ov023.toml | new |
| `Dq9BattleDropPercent` | `ov_023::021f47e0` | ldr r0, [sp, #0x14] (0..99 roll) | dq9_arm9_ov023.toml | new |
| `Dq9BattleDropOdds` | `ov_023::021f4a00`, `ov_023::021f4ac8` | mov r0, r1 (rare and common 1/N) | dq9_arm9_ov023.toml | new |
| `Dq9BattleGoldScale` | `ov_023::021f51ec` | str r0, [r7] (gold share) | dq9_arm9_ov023.toml | new |
| `Dq9TitleWidgets` | `ov_023::021f68dc` | function entry (title widgets) | dq9_arm9_ov023.toml | new |
| `Dq9TitleLogoHeap` | `ov_023::021f7ad0` | movs r5, r0 (logo group node) | dq9_arm9_ov023.toml | new |
| `Dq9MouseMenuGridPress` | `ov_023::021f9384` | cmp r1, #0 (grid menu press) | dq9_arm9_ov023.toml | new |
| `Dq9MouseMenuGridPosition` | `ov_023::021f9398` | ldr r2, [sp, #0x34] (pen) | dq9_arm9_ov023.toml | new |
| `Dq9MouseMenuGridArrows` | `ov_023::021f93ac` | movs r6, r0 (page arrow) | dq9_arm9_ov023.toml | new |
| `Dq9MouseMenuGridSelected` | `ov_023::021f9608` | after the cell is selected | dq9_arm9_ov023.toml | new |
| `Dq9MouseMenuGridChoose` | `ov_023::021f961c` | moveq r0, #1 (cell is the cursor) | dq9_arm9_ov023.toml | new |
| `Dq9TitleTextHeap` | `ov_023::021fa670` | mov r4, r0 (string table group node) | dq9_arm9_ov023.toml | new |
| `Dq9BattleDamageScale` | `ov_024::021ec7ac` | str r0, [sp, #0x54] (final damage) | dq9_arm9_ov024.toml | new |
| `Dq9WmInternalDisconnect` | `0x027e8ad0` | ldr r12, =0x027e45dc (disconnect) | dq9_arm7.toml | new |
| `Dq9WlSetStatusFlag` | `0x027eb384` | push {r4, lr} (WL status bits) | dq9_arm7.toml | new |

## 5. Structure offsets and data addresses

Offsets are added to the pointer the source names. `ctx` is `g_GameState` (`0x020f33d8`).

| Base | Offset | Meaning | Evidence |
|---|---:|---|---|
| `ctx` | `+4` | main-loop mode byte: 0/4/5/9 game, 6 boot and title | `dq9_arm9.toml`, `text_box_size.cpp` |
| `ctx` | `+5` | text language the game last read (2–5, else 1) | `dq9_arm9.toml` |
| `ctx` | `+8 + 4·id` | party/member entity pointer (ids 0–0xe8) | `discord_presence.cpp`, `0x0200fe00` |
| `ctx` | `+940` (`+0x3ac`) | this machine's player index (0 alone or hosting) | `discord_presence.cpp`, our `melonds-state.md` |
| `ctx` | `+0x3b4`, `+0x3b8` | milliseconds of the last pass | `frame_rate.cpp` |
| `ctx` | `+0x3c4` | VBlanks at the last update (`GameState_GetTickCount`) | `frame_rate.cpp`, our symbol |
| `ctx` | `+0x3c8` | VBlanks since the last main-loop pass | `frame_rate.cpp`, verified in `ov_017::0218b688` |
| `ctx` | `+0x63d6` | set/cleared around the opening movie | `dq9_arm9.toml` |
| `ctx` | `+0x7f6c` | boot phase (0 = cold boot, logos shown) | `dq9_arm9_ov020.toml` |
| `ctx` | `+0x7f70`, `+0x7f72` | post-logo bookkeeping; `+0x7f72` set = skip the opening movie | `dq9_arm9.toml`, `dq9_arm9_ov020.toml` |
| camera | `+0x58`, `+0x5c..+0x6c` | field of view angle, its sine/cosine, aspect, near, far | `dq9_arm9.toml` |
| camera | `+0x68` | near plane used by `MTX_PerspectiveW` | `dq9_arm9.toml` |
| camera | `+0x120`, `+0x12c` | position, target (fx32 triples) | `dq9_arm9.toml` |
| camera | `+0x244` bit 1, `+0x245` bit 1, `+0x246` | map lock (our unread bit), L+R ease flag, default yaw | `dq9_arm9.toml` |
| environment block | `+0x410` | fog mode: 2 field (per preset), 1 fixed preset | `dq9_arm9.toml` |
| environment block | `+0x288 + 0x38·i` | fixed fog presets, 56 bytes each | `dq9_arm9.toml` |
| environment block | `+0x4c..+0x64` | fog mode, shift, offset, colour, alpha, 32 densities | `dq9_arm9.toml` |
| game/field manager | `+0x36fc` | field task queue (`gameres+0x36fc`, already in `facility.md`) | `dq9_arm9_ov017.toml` |
| game/field manager | `+0x371c` | camp menu task | `dq9_arm9_ov017.toml` |
| game/field manager | `+0x3b54` | Profile Settings task (type `0x2f`, state `+28`, from-menu `+29`, scene `+32`) | `dq9_arm9_ov017.toml`, `game_chat.cpp` |
| field | `+0x36b5` | best interaction target's prompt icon index | `dq9_arm9_ov017.toml` |
| hero | `+0x250`, `+0x255`, `+0x256` | direction code 8–15, path-snap cache code / angle | `dq9_arm9.toml` |
| hero | `+0xac`, `+0xc4` | busy counter (s16), action (u16, low 15 bits) | `game_chat.cpp` |
| entity | `+308` (`+0x134`) | stats record: name, max HP `+48`, max MP `+50` | `discord_presence.cpp` |
| entity | `+336` (`+0x150`) | character record: vocation `+2384` (`+0x950`), levels `+364` (`+0x16c`), EXP `+0x138` | `discord_presence.cpp`, verified in `ov_023::021f5150` |
| entity | first halfword | bit `0x800` = returned by `0x0200fe00`; bit `0x100` = returned by `0x0200ff2c` | decompilation, § 7 |
| message manager | `+0x34`, `+0x38` | tile-buffer descriptor, render target | `dq9_arm9.toml` |
| message manager | `+0x6c`, `+0x78`, `+0x80` | text length, next character, 4-bit-fraction accumulator | `dq9_arm9.toml` |
| message manager | `+0x90` | inline choice list | `dq9_arm9.toml` |
| message manager | `+0x19cc` | 2D window is up | `dq9_arm9.toml`, verified in `0x020439c0` |
| touch manager | `+0x24`, `+0x55`, `+0x38/+0x3c` | touching, press edge, release position | `dq9_arm9.toml` |
| quest log (`0x02109418`) | `+0`, `+4`, `+0x4a8` | count (u8), 8 × 16-byte slots (id in bits 0–8), seen bits | `dq9_arm9.toml`, verified in `0x0209646c` |
| battle ctrl `ctrl` | `+22360` (`+0x5758`) | EXP per party slot, `u32[4]` | `dq9_arm9_ov023.toml`, `battle_multipliers.cpp` |
| battle ctrl `ctrl` | `+22376` (`+0x5768`) | gold shown by the results box | `battle_multipliers.cpp` |
| battle struct | `+0x58c4`, `+0x58cc`, `+0x58ce` | gold share: u32 × u16 / u16 (verified) | `0x021f44d0` |
| action executor stack | `+0xd0`, `+0xd2`, `+0x54` | target actor id, attacker actor id (s16), final damage of one hit | `dq9_arm9_ov024.toml` |
| overlay loader (`0x020a1950`) | `g_OverlayLoadBank`, `g_OverlaySlots`, `g_OverlaySlotOf` | bank index (`0x01ffd348`), `s8[6]` ids per bank (`0x01ffd364 + 6·bank`), slot of overlay n (`0x020e8f30 + 8n`); slots 0 = ov 0–6, 1 = 7–14, 2 = 15–21, 3 = 22–30, 4 = 31–32, 5 = 33–34, −1 none | `dq9_scene.h` |
| save | `[0x10, 0x6ff4)`, `[0x8010, 0xeff4)` | the two 0x6fe4-byte copies the game writes; checksum CRCs at `+16` (16-byte header) and `+132` (28508-byte body) | `save_image.h`, agrees with `save.md` |
| ARM7 WM | `0x027f8c5c + 0x550` | work area: `+0` state u16, `+0x738` last-heard u64, `+0x7b8` lifetime u64; `wl+0x3f4` status bits; request `+8` disconnect reason | `local_wireless_diag.cpp`, `dq9_arm7.toml` |

The overlay ids in `dq9_scene.h` also agree with our RAM observation: field = overlay 11 in slot 1,
battle = its own overlay in slot 3, title = overlay 4 in slot 0 (`melonds-state.md`).

## 6. What our corpus already had (confirmations)

These are not new, but they are independent confirmation from a second project reading the same
European ROM, and they are worth recording as such:

- `0x02032380 RandBelow` (C-library generator, `(int)(n·(rand−1)/RAND_MAX)`) and
  `0x0207430c NextRandomMax`: the drop and treasure-map rolls use them exactly as
  `battle.md § 1` and `encyclopedia-audit.md R11` describe.
- `0x021e6a90 CalculateFinalDamage`, `ov_023::021f454c RollDrops`, `ov_024::021eb5d0` (action
  executor) and the `sp+0xd0/0xd2/0x54` triple match `battle.md § Final damage`.
- The party layout: EXP at member `+0x138`, vocation at `+0x950`, level at `+0x16c` — the same
  offsets as `party.md` and `save.md`; `0x021f5150` adds each share to
  `member + 0x138 + 4·vocation`.
- `0x020f33d8 g_GameState`, `0x020fb3f0 g_Zone3D`, `0x02109418 g_QuestLog`,
  `0x02108844 g_QuestManager`, `0x02114e54` touch manager, `0x02012a94 TouchInput::GetTouchPoint`.
- The save write ranges are exactly our `0x6fe4` bytes at `0x10`/`0x8010`.
- `0x020d86e0 Gx_SwapBuffers`, `0x020c54b4 Gx_SetFog`, `0x020c56c0 G3_GetBoxTestResult`,
  `0x02066d00 MsgWin_DrawGlyphs`, `0x020424f4 Font_FindGlyph`, `0x02030f40 fix32_ReduceAngle0To2Pi`.
- The field overlay's per-update task is `ov_017::0218cbd4` (`Field_UpdateTask`), as we name it.

**One address that is not a function start**: the "field main loop" of the source is
`ov_017::0218b710`, which lies inside `func_ov017_0218b688` (size `0x1544`). It must not be
used in a `//! port:` marker; cite `0x0218b688` (or the hook address as evidence only).

## 7. Conflicts and discrepancies

No contradiction was found. Three points need care before they become our facts:

1. **`0x02066d00` is wider than our name.** We call it `MsgWin_DrawGlyphs`; the source says the
   field draw calls it and it draws the frame (`0x0202f414` → `0x0202f46c`), the glyphs
   (`0x020417c0`), the inline choice list (`0x02044540` → `0x0205c97c`) and the prompt.
   **[hypothesis]** it is the window's whole 2D draw, glyphs being only one part; do not rename
   until checked. Both readings are compatible, the name is narrower.
2. **Two member getters, two bits.** `0x0200fe00` (USA `GetPartyMemberByIndex`) returns the
   entity only when bit `0x800` of its first halfword is set; `0x0200ff2c` requires bit `0x100`.
   Our `party.md` quotes the second one for "the actor record exists". Both are true and they
   are different predicates; the source's "live" is the `0x800` one. **[hypothesis]** bit
   `0x100` means "in the active battle/party" and `0x800` "allocated"; no test yet.
3. **Main-loop cadence.** The source reads a pass as exactly two VBlanks (the wait bounds
   `r7 = 2 − ctx+0x3c8`, then `Gx_SwapBuffers` and one more wait; verified in
   `ov_017::0218b688`), i.e. 30 passes a second, and the field's task runs once a pass. That
   **agrees** with `docs/architecture.md` (field at 30 Hz) but the continuous measurement
   promised in `movement.md` ("4 VBlanks ⇒ …") is still missing; treat the 30 Hz reading as a
   source reading, hence **[hypothesis]** as a cadence fact until the check runs.

## 8. What this unlocks

| Area | Addresses / offsets | Our module that could use it |
|---|---|---|
| Battle rewards (EXP, gold, drops) | `ov_023::021ee4f4`, `021f44d0`, `021f5150`, `021f4a00`, `021f4ac8`, `021f47e0`; battle `+22360`, `+22376` | `crates/dq9-core/src/battle/end.rs`, `party.rs`; a real oracle for chapter 2 (see § 9) |
| Per-hit damage | `ov_024::021e6a90`, `021ec7ac`, `sp+0xd0/0xd2/0x54` | `battle/resolve.rs`; a hook could log every hit of a battle |
| Main-loop pacing | `ov_017::0218b710` (inside `0218b688`), `0218c780..0218c78c`; `ctx+0x3b4/0x3b8/0x3c4/0x3c8` | `dq9-app` loop, `docs/architecture.md`; closes the cadence [hypothesis] |
| Field task queue | `ov_017::021a51a4`, `021a53a0`, `021a53fc`; `mgr+0x36fc`, `+0x371c`, `+0x3b54` | `dq9-core/src/field`, `state.rs`; menu/task gating |
| Interaction prompt | `ov_017::0219b088`, `0219acb0`, `0219ad84`; `field+0x36b5` | field rendering and talk/check input in `dq9-core`, `dq9-app` |
| Text pipeline | `0x020444e4`, `0x020439c0`, `0x02066d00`, `0x02065d14`, `0x020425f4`, `0x0204e1d8`; `mgr+0x6c/0x78/0x80/0x19cc` | `Dialogue`, text pacing (`docs/re/dialogue.md`, `text.md`, `font.md`) |
| Menus | grid `ov_023::021f8e1c`; lists `0x0205c5c0`, `0x0205bc3c`; choices `0x020e26ac`; layouts `0x02080de4`; keyboard `ov_003::0215f000`; equipment `ov_005` | `dq9-app` menus, `j4-field-menu-rules` work |
| Camera and fog | `0x0202e0b4`, `0x0202e138`, `0x02051d38`, `0x02051b78`, `0x02051d64`, `0x02052208`; `cam+0x58..0x6c`, `+0x120/+0x12c`, `+0x244/+0x245/+0x246` | camera lock (F-map) and fog; `docs/re/lighting.md`, `stb-natives.md` |
| Overlay loader | `0x020a1950`; `g_OverlayLoadBank`, `g_OverlaySlots`, `g_OverlaySlotOf` | `dq9re state` could report loaded overlays by direct read instead of code comparison |
| Quests | `0x02109418` layout, seen bits `+0x4a8`, `0x0209646c` insert/remove, `0x020961c0` accept | quest rules; `save.md`, `trigger.md` |
| Netplay | `g_CommSession`, `g_PlayerIndex`, ARM7 `0x027e8ad0`, `0x027eb384` | only if netplay is ported; not needed now |

## 9. Oracle feasibility

Question: can the recompiled runner replace or complement our melonDS oracle
(`docs/re/melonds-state.md`, `scripts/melonds-*.ps1`, `scripts/melondsds_host.py`)?

### Answers

| Property | Verdict | Evidence |
|---|---|---|
| **Headless** | **Yes** | `--serve` runs without a visible window and serves a JSON-line debug protocol on `--port` (default 19842); `NDS_FRONTEND_HIDDEN=1` hides the SDL windows; the runner also builds with `-DNDS_SDL_BACKEND=NONE` for a no-frontend binary (`ndsrecomp/runner/CMakeLists.txt`) |
| **Memory / registers at a frame** | **Yes** | `regs` (both CPUs), `read_mem`/`write_mem` (writes only canonical main RAM `0x02000000..0x023fffff`), `state_save`/`state_load` (exact build + ROM gated), `capture_snapshot`, `frame_digests`, `cart_save`; advance with `run_vblanks`, `run_to_event`, `run_to_pc` (serve mode only; the SDL play mode refuses these) |
| **Recorded inputs** | **Partly** | There is **no input-file replay flag**. Input is injected live (`frontend_input` actions `key`, `touch`, `mouse`, `motion`) between `run_vblanks` calls, so a driver script is the recording. Network has a packet capture/replay (`--net-capture-in/out`); gameplay input does not. Our melonDS libretro host already has JSONL inputs and `--watch` |
| **Deterministic** | **By design, with caveats** | Virtual time is independent of host wall time; the framework ships a determinism probe (repeat `reset` + `run_to_event`, compare PCs and counters) and exact-build savestates. Caveats: host RTC (`--rtc-host`) must stay off or be pinned, audio/GPU threads must not feed guest state, 60 FPS "Accurate" overclocks the ARM9 (keep `Off`), network and local wireless must be off, and hook handlers must be pure functions of guest state (`midasm-hooks.md` says why) |
| **RNG / per-turn battle logging** | **Yes, by hooks or by memory reads** | A `before` hook at `0x02032380` logs N; the drawn value comes from an `after` hook on the epilogue instruction before `bx lr` (an `after` hook cannot follow the `bx lr` itself) or from the call sites (e.g. `0x021f4a00`). The same for `0x0207430c`; hooks at `ov_024::021ec7ac` (damage) and `ov_023::021f5150` (rewards) log the battle's numbers. Without a rebuild, poll `read_mem` on the battle ctrl/struct each VBlank |

### Build and cost

Requirements per README and `scripts/build.py`: Python 3 with `ndspy==4.2.0` + Pillow in a venv,
CMake ≥ 3.20, Ninja, **clang/clang++ in PATH** (mandatory on Windows; MSVC is refused), and
**SDL3 with `SDL3Config.cmake`** (or a manual `-DNDS_SDL_BACKEND=NONE` configure); submodules must
be initialised (`--recursive`, not `--depth 1` only). The bank build compiles roughly **1 GB of
generated C**; `--jobs` defaults to `min(CPUs, 6)`.

On this workstation: Python 3.12, CMake 4.4.4, Ninja and git are present; **clang and SDL3 are
missing**; `C:` has 28.9 GB free. [estimate] A first full build is 30–90 min on 6 jobs and
3–6 GB of build tree on top of the ~1 GB of generated C; a hook change recompiles the affected
bank (minutes for the ARM9 main bank). The 28.9 GB are enough only if the spare slot's cargo
targets are not filled in parallel.

### Comparison with the melonDS oracle

| | Recompiled runner | melonDS + states/libretro |
|---|---|---|
| Observation point | any instruction, through a built-in hook | frame boundary (state or per-frame watcher) |
| Setup before reading | 30–90 min build, then `--serve` | ~40 s state script or libretro host |
| Editing what is observed | edit a manifest/handler, rebuild the bank | edit a script, no rebuild |
| Determinism | virtual time, exact-build states, but see caveats | wall-clock input posting; libretro input log is frame-exact |
| Debug surfaces | trace rings, bus/IPC/IRQ rings, state save/load, memory reads | states, `--watch` bytes, GDB stub (flaky here) |
| What it adds for us | RNG draws and intermediate battle values *inside* a turn, no window | full 4 MiB RAM, already wired into `dq9re state` |

### Recommendation: **try on a spare slot** (not adopt)

Adopt only if it produces the chapter-2 reference we lack (`status.md`, "no original reference
for chapter 2") cheaper than extending melonDS. Try it in a dedicated batch, on a spare slot,
with a hard time box.

Exact steps and estimate if "try":

1. Install LLVM (clang/clang++ in PATH) and SDL3 (`SDL3Config.cmake`); free ≥ 10 GB on `C:`.
2. Clone `birabittoh/DQ9Recompiled` with submodules at the commits of the introduction (README
   `--recursive`); put our ROM at the repo root as `dq9.nds` (ignored by git; never commit).
3. `python scripts/build.py -j 6`; the runner lands as `DQ9Recompiled.exe`.
4. Start headless: `DQ9Recompiled.exe --serve --port 19842 --rom dq9.nds
   --save-path <a copy of the .sav> --startup-mode automatic --boot direct --generated-firmware
   --freebios --network off --local-wireless off` (`--no-save` instead of `--save-path` when
   the run must not write), with `SDL_AUDIODRIVER=dummy` and no `--rtc-host`; keep 60 FPS at
   Off (default `game.toml`).
5. First goal, no rebuild: script `run_vblanks` + `frontend_input` to replay the prologue to
   battle 26, then `state_save` and `read_mem` the battle struct (`+22360`, `+22376`) and
   the party members; compare with the pinned values (HP 16/20, XP 7, gold 13).
6. Second goal, one rebuild: add MIT-style probe handlers in `src/hooks/` and manifests for
   `0x02032380`, `0x0207430c`, `ov_024::021ec7ac` and `ov_023::021f5150`; log draws and rewards
   to stderr. This is the part melonDS cannot do without a GDB-level oracle.
7. Stop rule: if the build does not boot to the field within the time box, or the debug server
   cannot inject input deterministically, record the failure and keep melonDS as the oracle.

## 10. Open questions

- The fork's `dq9` branch is 211 commits ahead of the fork's `main` and 5 behind it; its debug
  surface is the upstream one plus additions we did not audit.
- Hooks are baked in: there is no runtime hook injection, so routine observation is either a
  rebuild or frame-level `read_mem`.
- The runner's determinism under injected input is asserted by the framework's probe, not by a
  game-level replay; we have not run it (clang/SDL3 absent here).
- The "60 FPS Accurate" overclock and the 2D-window half-rate hook change guest-visible timing;
  any oracle run must keep the defaults (Off) and no hooks that alter game state.

## 11. The reward and damage anchors, read against our code (batch `v3-reward-hooks`)

The manifests' comments at `dq9_arm9_ov023.toml` / `dq9_arm9_ov024.toml` name the data flow
around four hooks; each was resolved in the EU code and compared with `dq9-core`
(`docs/re/battle.md` § "End", `docs/re/party.md` § "A won battle's award"). Evidence:
`build/evidence/v3-reward-hooks/decomp-ov_023__021edf54.txt`, `…/decomp-ov_023__021f454c.txt`,
`…/decomp-ov_023__021f44d0.txt`, `…/decomp-ov_023__021f5150.txt`,
`…/disasm-021f454c.txt`.

| hook | anchor | what the code does | our code | verdict |
|---|---|---|---|---|
| `Dq9BattleExpScale` | `0x021ee4f4` (`ldr r1,[r7,#0x29c]`) | `ov_023::021edf54` computes `(u32)(float(0x8e28) × 0x8e3c)` and, per slot of the battle's mask, `func_ov023_021f4098(ctrl, slot, 0)` into `ctrl+0x5758+4·slot` (`0x021ee140`); every machine reaches the anchor once that array is filled (leader computes, guests receive it with message 106, `0x021ee4b0` waits on flags 0x2000/0x4000/0x8000) | `Battle::finish` reports the sums and `Party::award_with` splits them with `func_ov023_021f4098`'s formula and the battle counter at the fight's close | agree in numbers and order (the snapshot follows the share) **for one player**; the routine's connected-player factor `1 + (n − 1) ÷ uRam021f442c` is absent from `award_with` (multiplayer unmodelled) |
| `Dq9BattleGoldScale` | `0x021f51ec` (`str r0,[r7]`) | `func_ov023_021f44d0` returns `(u32)max(ctrl+0x58c4 × (0x58ce ÷ 0x58cc), 1.0)` for a positive share (0 stays 0, `0x021f4520`–`0x021f453c`); `0x021f51ec` stores it at the caller's `ctrl+0x5768` (the results amount) and `0x021f51f0`–`0x021f5208` add it to the party's gold, held to `0x0098967F` | the total, added whole and held to `GOLD_MAX` | agree for one player: `0x58cc` = turn count, `0x58ce` = turns with a member `func_02010088` answers 0 for, so the ratio is 1 (`bat6.ml1`: 13 = the monsters' 13); the split's multiplayer meaning is **[hypothesis]** |
| `Dq9BattleDropOdds` | `0x021f4a00`, `0x021f4ac8` (`mov r0,r1`) | `ov_023::021f454c`: the leader's pass, then one pass per **party slot** that is in the battle, is not held by the `live+0xA8` bit 0 gate, holds ≥ half the battle counter and carries flag `0xA4` (`live+0x900` bit 4); that pass's `1/N` is `N × 100 ÷ level`, and a byte-0 step ("one in one") draws nothing there (`0x021f49e0`, `0x021f4aa8`); rare (`+0x03`/`+0x06`) before ordinary (`+0x02`/`+0x04`), eight items at most, no HP test | `Battle::finish` walks the same passes, `PartyMember::item_finder` carrying the flag and level | the manifests only called the code leader-only; the **second member's flag does add a pass** (0x021f4628–0x021f46b4), so the batch's premise was refuted and the read rule implemented; the challenger's review caught the byte-0 suppression, now pinned by `an_extra_pass_never_replays_a_certain_drop` |
| `Dq9BattleDamageScale` | `0x021ec7ac` (`str r0,[sp,#0x54]`) | the action executor stores `ov_024::021e6a90`'s result — after resistance, block/dodge, the coin, the combo table and the cap — at `sp+0x54`; `sp+0xD2`/`sp+0xD0` are the actor ids | `resolve_action` truncates once into `TargetOutcome::amount`, subtracts exactly it from HP, and reports it | agree: one quantity applied and shown |

The manifests' group-force flags (kind word bit 14 forces the rare, bit 15 the ordinary) and the
treasure-map percentage branch of the same function remain unmodelled; they need fields our
`Kind` does not carry and a grotto battle the engine cannot set up.
