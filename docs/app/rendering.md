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
  camera; seated `sleep_*` variants with mirroring. Game scales: sprites `0x8f/4096`
  per pixel, models `0x10a/4096`; positions `fx32/4096`, rotation in radians.
  `.chr` archives, motion from `cast::initial_motion` (including the extra package of
  type 19), monsters from the `.mon` member of `enemy.gp2` then first `.cchr`.
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
