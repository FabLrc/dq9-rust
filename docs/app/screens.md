# DS screens

The window shows the two DS screens (`crates/dq9-app/src/screens.rs`), or a single enhanced
field in the `enhanced` presentation:

- **Top**: the 3D field, with the game's camera and projection (`0x0202df78`, vertical
  half-angle of 15°). The screen is 256×192; only the rectangle size varies with the window.
- **Bottom**: in the field the original draws its 3D view, message window and touch UI
  ("X Menu", stylus hand) there and the map screen on the top one; during a fight the party
  panels and command menus are at the bottom and the 3D view on top. Elements, draw
  functions, assets and coordinates: [`re/bottom-screen.md`](../re/bottom-screen.md).
- **Field screen assignment.** `--screens port` (default; Web: the "Field" select, or
  `?screens=port`) draws the field on the top screen, the port's own assignment. `--screens
  original` (Web: `?screens=original`) puts the 3D view and the message window on the bottom
  (touch) screen, as the original runs with `POWCNT1` bit 15 set (`ov_017::021b17ec`), and the
  top screen shows the original's map screen (`re/bottom-screen.md` § 2.1), drawn by
  `dq9_render::map_screen` from the map's `.bmmp`/`.obg` picture, the layout's backdrop
  (`.bmmp` `0x6a`, tiled under the picture) and the `obj_minimap` party strips and dots, with
  the area name in a dark tab from the game's font; what is implemented and what stays
  **[unknown]** (the scroll of a larger picture, the backdrop's own layout, the marks' colours,
  the party's colours) is listed there. A map whose layout or picture is missing keeps the plain
  placeholder (`dq9_render::Panel`, `panel.wgsl`) and the status line names the missing datum
  (`map screen (B08M03): no .bmmp layout for B08M03`); nothing stands in for an element the
  data does not establish. A field cutscene
  keeps the bottom screen as the note states; the script's 109/110 swap is not applied in
  `original` mode **[hypothesis]** (§ [Events](events.md), Screen swap). A battle is unchanged in both
  modes: 3D and battle text on the top screen, panels and command windows at the bottom. The
  field menu opens on the free screen, which is the top one in `original` mode, like the
  original's facilities.
- **Message window**: drawn on the field's screen (`talk.rs`, `dialogue.md`), so the top
  screen by default and the bottom one in `original` mode or after a cutscene's 109; the
  battle's own is at the top of the screen (`battle/ui.rs`).

Layouts: `enhanced` (default: one 16:9 field, letterboxed in the window, § "Enhanced 16:9"),
`enhanced-reactive` (the same field filling the window at any aspect, experimental),
`stacked` (top screen above the bottom one), `side` (side by side) and `top` (top screen
alone). The DS three are centered and scaled together to the maximum of the available space,
without distorting their 4:3 ratio; margins stay black, independently of sky, fog and fades.
Every layout works in both screen modes; `top` has one screen, so the field takes it in either
mode and the message window stays visible (no placeholder); the enhanced layouts have one
field and no second engine at all. On the Web, the
canvas follows the available size below the controls and the screen's pixel density. Native:
`--layout`, and `R` switches to the next layout; `--screens`. Web: the "Screens" select or
`?layout=`, the "Field" select or `?screens=`. The
old `--ds-aspect` and `ds=` are removed.

Captures of both modes (local, never committed, `build/evidence/j4-screens-original/`):
`after-port-m01.png` (`69b60fca0327`, `--map M01`, pixel-identical to the capture before the
option existed), `original-m01.png` (`dadac22d6bca`, 3D view at the bottom, the placeholder the
top screen kept until the map screen batch below), `original-m01-side.png` (`ebbca9caf9d0`),
`original-conversation.png` (`dd113de81d1c`, Hugo's window on the bottom screen with the 3D
view), `original-conversation-top.png`
(`49828f3975b9`: `top` keeps the field and its window on the only screen), and
`battle-original.png` (`dd025965b166`, identical to `battle-port.png`: a fight ignores the
option) and `wide-original.png` (`7867f9fee551`, identical to `wide-port.png`: one 16:9 field
has no second screen to move; those two files keep their names from that batch).

The map screen (batches `v3-map-screen-draw`, then `v3-map-screen-details` for the backdrop;
`build/evidence/v3-map-screen-details/`, local): `original-M01.png` (`b319a0f1adbd`, the village,
its backdrop outside the torn paper), `original-S01M01.png` (`72ddf077c467`, the pass) and
`original-F01M01.png` (`127ab848910c`, a map without a layout: the placeholder), window captures
of `--map <code> --save … --layout stacked --screens original`; the draw batch's
`original-M01.png` (`13d50e43d010`), `original-F01.png` (`b97b361c63cf`) and `original-S01M01.png`
(`08033af76f1e`) predate the backdrop. The portrait of the two screens compared with the original
is `re/bottom-screen.md` § 2.1. `port-m01.png` (`b6bd79252785`) and `battle-port.png`
(`019c69584c9d`) are the headless `--screenshot` references, byte-identical to the same
captures built from the parent commit `3475c61` (the port and a battle do not draw the map
screen). The sweep log `sweep.txt` (`b25081e83a2a`; `9c1ec9f9e913` in the draw batch) is the
map screen test: 283 layouts and 418 of the map list's 1,010 maps resolve, 592 fall back with
`no .bmmp layout for <code>`.

## Touch

The window routes the pointer to the DS touch screen (`app/input.rs`, `screens.rs`); the
original's touch elements and coordinates are in `re/bottom-screen.md` § 2.2.

- **A tap** is a left press released after at most 8 window pixels of pointer travel (the path
  length, so a back-and-forth counts as a drag). The camera gesture is the same button dragged,
  so a drag turns the camera and fires no tap. A press that arrives before the window has seen
  any pointer motion is ignored (`winit`'s button events carry no position).
- **Mapping.** The window pixel becomes a point of the 256×192 screen the field draws on: the
  bottom screen in `original` mode and after a cutscene's 109, the top one in `port`, the only
  screen in `top`, and the whole 16:9 rectangle proportionally in the enhanced layouts (a 16:9
  field has no DS screen shape **[hypothesis]**). A press on the other LCD, on a black margin or
  on the enhanced letterbox does nothing (`Screens::field_pointer`).
- **Routes.** With a message window open, the tap presses the action button: in the note's
  `field-church-tap-bottom.png` (256×409, a 25-row melonDS menu bar) a tap advances a line from
  an image row 300 — bottom-screen row 83, above the window's own rectangle `y 116..190` (row
  100 if the capture helper's bottom-relative coordinates were used) — so the whole 3D screen is
  the target; the exact point is **[unknown]**, and a prompt is not answered by the touched row
  (not established). Otherwise a tap on the "X Menu" label opens the field menu where `I` does
  (`screens::menu_affordance`, the label's measured box x 213..243, y 27..35, **[hypothesis]**:
  the game's own `TestHoldInRectangle` rectangle was not resolved). Nothing else: walking and
  climbing by drag, picking a spot and steering the ship are not modelled, and a battle scene,
  a played fight or a facility screen routes no tap.
- **Recording.** A recorded session replays keys and camera deltas only, so a tap would move the
  game without being recorded: taps are ignored while recording or replaying.
- **Not drawn.** Neither the "X Menu" label nor the stylus hand is drawn: their asset is
  **[unknown]** and the field's engine-A `bg1`/`bg2` layers are not modelled, so the port draws
  nothing there rather than an invented stand-in.

Tests (`cargo test -p dq9-app`): `a_window_pixel_maps_to_the_field_screens_touch_point`,
`the_menu_affordance_is_the_measured_label_rectangle`,
`a_press_released_in_place_is_a_tap_and_a_drag_is_not`,
`a_tap_on_the_menu_affordance_opens_the_field_menu`,
`a_synthetic_tap_advances_a_real_conversation_line` and
`taps_read_the_line_and_close_the_conversation` (real `X01M07` lines; the probe
`print_tap_trace` prints one trace, kept in `build/evidence/v3-touch-ui/`).

## Enhanced 16:9

`--layout enhanced` (Web: `?layout=enhanced`; the **default** on both) is the enhanced
presentation, not a DS reconstruction:

- The field fills a 16:9 rectangle centred in the window (black bars elsewhere); the camera
  keeps the game's 15° vertical half-angle, so the widescreen field widens the horizontal
  view instead of cropping it.
- Every effect is a boolean of one struct (`dq9_render::EnhancedOptions`,
  `crates/dq9-render/src/enhanced.rs`), every one on by default except the contact shadows
  (`contact-shadows` is opt-in), switched by `--fx LIST` /
  `--no-fx LIST` (names below, comma-separated, also `all` and `none`; `--no-fx` is applied
  after `--fx`) and, on the Web, by the "Effects" row of checkboxes or `?fx=`/`?nofx=`
  (`?nofx=bloom,vignette`). `--fx all` and `?fx=all` switch every effect on, contact shadows
  included; the default is `EnhancedOptions::DEFAULT` (`ALL` without `contact_shadows`).

| Name | Effect |
|---|---|
| `trilinear` | mipmapped, linearly sampled textures instead of the DS's nearest filtering with one mip level |
| `fxaa` | FXAA-lite |
| `contact-shadows` | screen-space contact shadows read from the depth buffer; **off by default**, `--fx contact-shadows` (or the page's checkbox) turns them on |
| `bloom` | threshold bloom |
| `tonemapping` | filmic (ACES) tonemapping mixed with the DS colours |
| `grading` | cool shadows, warm highlights, saturation and contrast |
| `vignette` | vignette |
| `dithering` | dither against the 8-bit steps of the sky gradients |
| `edge-accent` | edge marking as one physical pixel; off draws it at the DS's own width |
| `glass` | translucent panels behind the HUD and the field's message window |

- The contact shadows are the one opt-in effect, and the only difference between
  `EnhancedOptions::DEFAULT` and `ALL`. The original field has no post-process: the effect's
  soft dark rim around every entity (about 4 physical pixels at 1000×782) read as a thick
  outline on top of the single-pixel edge marking, whereas the DS draws no such halo. Measured
  at `X01M05`, night, 1000×782, frame 60: 19,859 pixels change by more than 30/765 when it is
  switched off, and the hero's silhouette goes from a soft black rim to the thin crisp edge
  (`build/evidence/j4-contact-shadow/`).
- The post effects travel to `post.wgsl` as a uniform bit mask (`features.x`,
  `PostBits` in the same module), read every frame: switching one rebuilds nothing. With
  every post effect off the field is drawn directly, the DS look (nearest filtering, DS-width
  edge marking) in the layout's frame. Only `trilinear` needs new textures, so only it
  rebuilds the renderer. The shaders are parsed and validated by
  `crates/dq9-render/tests/shaders.rs` without a GPU, including the mask bits the Rust side
  sends; this does not certify backend support.
- The field is rendered to an offscreen target (`Renderer`) and composed by `post.wgsl`. The
  target is at most 4096 px on its longest side (`renderer::MAX_SCENE`): a larger window is
  rendered at that resolution and stretched by the post pass, which keeps a high-DPI window
  under the adapters' texture limits.
- The interface is a 960×540 virtual canvas (`hud.rs`): location, map code and time of day,
  the whole party (up to four members: name, level, HP and MP bars with numbers), the purse,
  and the game's message window scaled into a bottom-left glass panel. Battle windows keep
  their 4:3 shape in the upper and lower left corners.
- The renderer is rebuilt when `R` or the select enters or leaves the enhanced layouts
  (texture filtering differs) and when `trilinear` changes; `--screens` has no effect: one
  field, no second DS screen and no placeholder.

Screenshots of the layout (a DS capture of the same map asks `--layout stacked`
explicitly: `stacked` used to be the default):

```powershell
cargo run -p dq9-app --release -- --map M01 --layout enhanced --size 1920x1080 `
    --eye=-5,20,40 --target=-20,0,5 --screenshot build/scratch/app/enhanced.png
cargo run -p dq9-app --release -- --map M01 --layout enhanced --no-fx all `
    --size 1920x1080 --screenshot build/scratch/app/enhanced-ds-look.png
cargo run -p dq9-app --release -- --map M01 --layout stacked --size 1280x720 `
    --screenshot build/scratch/app/ds-stacked.png
```

## Enhanced reactive (experimental)

`--layout enhanced-reactive` (Web: "Enhanced (reactive, experimental)",
`?layout=enhanced-reactive`) is the same renderer and effects, with a presentation that
follows the window:

- The 3D field fills the whole window at any aspect, portrait to ultra-wide, without bars
  (`screens::screens`); there is no second DS screen and no placeholder.
- The vertical half-angle stays the game's 15° in landscape; narrower than 16:9 the
  horizontal half-angle is held at its 16:9 value, so the view widens vertically instead of
  cropping the sides (`Camera::reactive_fovy`, `Camera::reactive_view_proj`).
- The offscreen target follows the window's size (device pixel ratio included) with the same
  4096-pixel cap; it is recreated when the size changes and the previous texture is dropped
  with the previous target, so a resize does not leak resources.
- The HUD's canvas takes the window's aspect with its shorter side at 540 canvas units
  (`hud::Canvas::for_window`): the canvas corners are the window's corners, the interface
  scales with the shorter side, and the message window and the battle windows are anchored
  to it instead of a fixed 960×540 canvas.
- Experimental: the layout math (aspect, field of view, anchors) is unit-tested for portrait,
  square, ultra-wide and tiny windows; no hand-driven window session nor browser resize was
  run for this batch **[unknown]**.

Captures below are local (`build/evidence/j4-enhanced-mode/`, sha256 prefix, `manifest.json`):
M01 at 1280×720 with every effect on, `enhanced-all.png` (`399b66298d10`), and with none,
`enhanced-none.png` (`1a24c4918d79`: the DS look in a 16:9 frame); each effect off in turn
`enhanced-no-trilinear.png` (`7cb1552e2ff8`), `-fxaa` (`03e65edac654`), `-contact-shadows`
(`65186b3d4a4e`), `-bloom` (`162817d0f6a7`), `-tonemapping` (`fd33cc2599be`), `-grading`
(`280b7d97a235`), `-vignette` (`825ea3399b2c`), `-dithering` (`c33495172196`), `-edge-accent`
(`30dc6d73e250`) and `-glass` (`a646593b06f1`), all pairwise different; the Hugo conversation
with its bottom-left glass panel, `enhanced-conversation.png` (`5b0e2ec49ce7`), and the same in
the reactive portrait, `reactive-conversation.png` (`4a7e8d7716d8`); reactive 700×1100
`reactive-portrait.png` (`f77739f719b0`) and 2400×800 `reactive-ultrawide.png`
(`1c4c2eedeb4d`); battle 26's menu windows, anchored and scaled to the window rather than
stretched, in `enhanced-battle.png` (`8ef8f66f7ccf`) and `reactive-battle.png`
(`ef2ccc682dbf`). The DS presentation is unchanged: `--layout stacked --size 1280x720` gives
`reference-m01-stacked-before.png` and `-after.png`, both `6b1d5597e9c1`. Those offscreen
captures carry no party values (a new game fills them at the first fight), so the four-member
panel is asserted by the `hud` unit tests, not seen in them. `verify.ps1 -RequireRom` passed
(5/5, 233.3 s; the README progress block did not change).
Earlier evidence for the presentation, when
`wide` was this layout's name (that build's captures):
`build/evidence/add-graphics-tech/native-wide.png` (`684fb0b30e4c`),
`web-wide-restored.png` (`5b598af7e34f`), `web-console.json` (`ac1c4bafeb61`); integration
`49843de` passed `verify.ps1 -RequireRom` (5/5, 150.3 s; app 86/86) and published build
`1e0326d` passed [CI](https://github.com/FabLrc/dq9-rust/actions/runs/42831807723) and
[Pages](https://github.com/FabLrc/dq9-rust/actions/runs/42831807515).
