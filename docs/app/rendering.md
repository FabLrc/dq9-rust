# Rendering

What `dq9-render` does, and what deviates from the DS 3D engine.

Faithful to what the game reads:

- **Instances**: one per model and object placement (`.nsbmd`), including those the chosen
  time hides (`Instance::hidden`): the hidden instance has a zero scale and keeps its
  original matrix (`SceneInstance::home`), so that natives 574 and 575 can show or
  move it; world matrix = translation, `y` rotation, scale from `Instance::world`
  (`0x02013168`). `dq9-core` decompresses the `.col2`, transforms the visible
  faces and uses them for floor and walls (see `re/movement.md`).
- **Textures**: all the `.nsbtx` of the `.ambl` (`Zone3D+0x418`), bound to materials by
  texture and palette name. When several files have the same name, the first in the
  list (the most recent) wins **[hypothesis]**. A texture that no file provides
  (30 in the ROM, `Scene::unbound_textures`) leaves the material untextured **[hypothesis]**.
- **Materials**: vertex colors, polygon alpha (`POLYGON_ATTR` bits 16-20), culled
  faces (bits 6-7, front faces counterclockwise), modulation and decal modes
  (bits 4-5; all map materials are modulation), repeat and mirror
  (`TEXIMAGE_PARAM` bits 16-19), material texture matrix (`Model::tex_matrices`),
  nearest filtering, no alpha-0 pixel.
- **Passes**: opaque polygons then translucent ones (alpha < 31 or texture with partial alpha,
  A3I5/A5I3); alpha-31 texels of an alpha-31 polygon are drawn in the opaque
  pass, like the DS's opaque pixels; translucent pixels only write
  depth with bit 11; "equal" depth test (bit 14) after the other
  opaques.
- **Camera**: the game's by default (`0x0202df78`): vertical half-angle of 15°
  (`0x0202e9b4`, sine and cosine for `NNS_G3dGlbPerspective`), planes at 1 and 400; free in
  orbit or flight; 256×192 option.
- **Hero** (`dq9_render::hero`, `AnimatedScene::add_hero`): body and legs on the
  14-bone template, animated by `mp0200n`/`mp0200ne` (`stand`, `walk`, `run`), face, hair and headgear
  placed on the `head` bone, arms and shoes by texture files, merged into one model;
  oriented by heading (`angle_of_yaw`), scale `0x10a × build`; the gait comes from the
  distance covered per step (`Gait::from_speed`: stand if still, run otherwise). Appearance: the first character of the
  save (`--save`), otherwise a default hero **[hypothesis]**; no weapons, no shields, no tints
  from `palette.bin` ([`re/characters.md`](../re/characters.md)). In his winged form (Angel Falls,
  story 1.2 to 1.3) the application adds the pink palette operation and two scripted models hung
  on his `chest` (wings) and `head` (halo) bones each frame, `--screenshot` included
  ([`re/mortal-veil.md`](../re/mortal-veil.md) § 7).
- **NPCs**: cast and placements from `cast::load`, `.spr` sprites decoded once
  (cut-out images), `stand_*` names chosen by `cast::sprite_stand`, billboards facing the
  camera; seated `sleep_*` variants with mirroring and the lying-back quad below. Game scales:
  sprites `0x8f/4096` per pixel, models `0x10a/4096`; positions `fx32/4096`, rotation in
  radians. `.chr` archives, motion from `cast::initial_motion` (including the extra package of
  type 19), monsters from the `.mon` member of `enemy.gp2` then first `.cchr`.
- **Seated sprites** (flag `0x800`, sheet `<name>_s.spr`; `SpriteAnimation::face`,
  `seated_frame`; `j4-field-facing`). The sprite draw `0x0203f54c` (USA `func_0203f53c`) puts the
  pivot at the middle of the sheet like a standing one, but a seated sheet is **not a plain
  billboard**: `NpcSprite_FaceCamera` (`0x0203cbe8`) stores `+0x54 = −θ`, with `θ` = the NPC's
  angle plus the type 15 angle (`+0x90`) minus the camera's, in `[0, 2π)`, and the draw multiplies
  the matrix by `RotX33(a) · C` (`MTX_Concat33`, `0x020c15b4`), then `RotZ(r)` (`0x02031288`):
  `C` is the camera's inverse-view rotation (`Camera+0x144`, rows right, up, back),
  `a = −|cos θ| / 2` (`fix32abs(fix32cos(θ)) >> 1`, negated), and `r` is `θ` reduced by whole
  `π/4` (`0xc91`) to `[−π/8, π/8)`: the eight-sector table `0x020e7938` (`sleep_down`,
  `sleep_r_down`, `sleep_right`, `sleep_r_up`, `sleep_up`, then `sleep_r_up`, `sleep_right`,
  `sleep_r_down` mirrored) picks the picture, the roll takes up the rest. `MTX_RotX33_`
  (`0x020c1274`) stores `_11 = cos`, `_12 = sin`, `_21 = −sin`, `_22 = cos`, so the sheet's up
  goes to `cos a · up + sin a · back`:
  with `a < 0` the **top leans away from the eye** (up to 0.5 rad, none seen from the side,
  where `cos θ = 0`), the quad lying back as a sleeper in a bed seen from above, and it rolls
  in its own plane by `r`. The port tilted the other way (`forward · sin a`, the top towards the
  eye) from a guess before this was read; `animation/sprite.rs` tests transcribe the DS chain
  and `animation/seated_rom.rs` checks `C01M15`'s sleeper over 16 yaws. **Challenger pending**:
  read from the code and its machine code (`0x0203f730..0x0203f84c`: `fix32cos`/`fix32abs`
  of `θ`, the reduction loop, `MTX_RotX33_(out, sin a, cos a)` through `blx 0x020c1274`,
  `MTX_Concat33(RotX, C, out)` through `bl 0x020c15b4`, `translate(0, −h/2, 0)`, `MULT_3x3` =
  GX command `0x1a` (`0x020c51d0`), `RotZ(r)`, `translate(0, +h/2, 0)`; the object's own rotation
  `0x020e79f8` is the identity and its scale 1); not compared with a melonDS capture of a seated
  NPC at several camera angles (none taken). **Open**: the same
  draw has a second billboard mode, sprite flag `+0x50` bit 4: `RotY(camera angle)`
  (`0x02031244`) instead of the camera's matrix, an upright turn like a `BBY` command; which
  sprites set it was not read **[unknown]**, and `face` always uses the full camera frame for
  standing sheets.
- **Field monsters** (`<code>_f.mon` of the roamers, `app/roaming.rs`): the model's world matrix is
  `T(position) · RotY(heading) · S(scale)`, the object's own `+0x54`; **nothing turns it to the
  camera**. `Object3D::PopulateRenderConfigWorld` (`0x0203575c`) takes the camera's matrix only
  when flag `0x80000` of `+0x6c` is set, and it is clear on all 50 live roamers of the ten
  original RAM images read (`build/oracle/field-F02/run/w-*-ram.bin`: `0x4`, `0x84`,
  `0x10000004`, `0x10000084`); no
  roamer state handler (`0x02076d44..0x02078e60`) is among the eleven callers of the camera's
  angle getter (`0x0202e800`, `calls.txt`); the field draw loop (`ov_017::021a1d98`) draws each
  visible roamer with `0x02076bfc`, which goes through `Character::Draw` (`0x02032fec`) and
  `Object3D::Draw` (`0x02035a3c`) with that rotation. 27 of the 397 field bodies that build
  carry model billboard commands (`z011a`, `z014a`, `z029a`…): those parts are already turned by
  `face_camera` like a map's. The port's roamer drawn at the position, heading and camera of six
  original states (`w-c60`, `w-c190`, `w-c395`, `w-c410`, `w-contact2`, `w-contact3`) shows the
  same side as the original's capture (five readable): the roamers are drawn like the original,
  and `a_roamers_model_keeps_its_own_heading_whatever_the_camera_yaw` (`app/roaming/tests.rs`,
  eight camera yaws) pins it, so no change can add a camera turn unseen. The one camera-facing
  thing a roamer draws is a 2D marker during its notice state, **not modelled**
  ([`re/encounter.md`](../re/encounter.md) § Not modelled / open).
- **Animations**: files explicitly selected by the flags of the map
  parts, G3D pose (`JNT0`), texture matrices (`SRT0`), colors/alpha (`MAT0`) and
  texture/palette changes (`PAT0`). Simple models loop over the full
  frame count (`Animation3D::AdvanceTimer`), objects with a BCFG package use the
  bounds and speed of the first motion; per-motion `.chr` use its speed
  and the file's range `[0, frames−1[`, not the old BCFG slices
  (see `characters.md`).
  Display lists are decoded once; poses only rewrite the
  vertices, without rebuilding indices or reallocating GPU resources. SRT/PAT alone do not
  redo the geometry. The animated CPU state is separate from the static scene API.

Approximated or absent:

- **Translucent sorting**: back to front by the depth of the center of
  each mesh × instance; the DS sorts by on-screen `Y` (or keeps submission order).
- **Lighting**: map models are lit per vertex by the two DS lights of the
  `.bats` (GBATEK equation: emission + per light `color × (diffuse·max(0, −L·N) + ambient
  + specular·shininess)`, on 5 bits; bits 0-3 of `POLYGON_ATTR`), model normals, zone ambient
  for the terrain in advanced mode. Base zones have no light: their
  vertex colors are transformed (hue, brightness, contrast); sprites and characters
  are multiplied by the zone's diffuse; characters keep an extra fixed light (the
  real DS lights go through their own matrix, not ported). Background: vertical gradient
  centered on the horizon; DS fog (offset, shift, table of 32 densities, color) on
  materials with bit 15 of `POLYGON_ATTR` and sprites; edge marking. The shininess
  table (`SetShininessTable`, bit 15 of `SPE_EMI`) is not ported.
- **Billboards of models** (G3D render commands `0x07` BB and `0x08` BBY: trees, bushes and
  other flat scenery, `j4-billboards`, [`re/g3d.md`](../re/g3d.md) § Billboards): turned toward the
  render camera every frame on the CPU, one model copy per billboard placement, with a static
  translucent sort key. 342 of 740 `.amdj` archives hold them (3,466 commands, 2,761 BBY);
  `F01` none (how the field vegetation is drawn is not established). Before this, such quads kept
  their authored orientation and vanished edge-on or behind. Checked by `--screenshot` only: the
  interactive window runs the same per-frame code but was not driven by hand.
- **Not rendered**: antialiasing (the enhanced presentation's FXAA is a post-process, not the DS
  engine), shadow polygons (mode 3) and wireframe (alpha 0), fog
  alpha-only mode and `FOG_COLOR` alpha, DS precision
  (6-bit colors, W depth; fog depth is recomputed on 24 bits).
- **NPCs, limits**: NPCs neither stop nor turn toward the hero while he talks
  to them (sprites follow the camera; model orientation has no API in
  `AnimatedScene`); NPC heights are those of the placements; ground snapping and
  hero movement are approximate (no acceleration or radius).
  Doors, areas and warps follow `dq9_core::field` (compared with
  melonDS on the Stornway church) without the door animation; zone triggers are
  evaluated; their event requests are executed (§ [Events](events.md)), the other requests
  only logged, and stairs, falls and NPC paths are not executed. Network
  visitors, the `s069` accessory and additive palette tint (type 22) remain
  absent. Blending of several poses, `.beff`/`.bact` effects/actions and sprite
  modes other than standing/seated are not executed. MAT colors are evaluated;
  vertices with normals remain subject to the fixed light described above, not to the DS lights.
