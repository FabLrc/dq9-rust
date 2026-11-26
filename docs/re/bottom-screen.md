# The two DS screens: what the original draws, field and battle

What each LCD of the original shows, in the field (free control) and in battle, and what
touches there do. Read from the European ROM (`YDQP`) with Ghidra and melonDS, with the
[DQIX wiki](../external-resources.md) and the USA decompilation as hints; USA ARM9 addresses
are moved to European ones as `AGENTS.md` gives (`+0x8` up to `0x0200FB20`, `+0x10` after),
overlay addresses are identical. Data, code and layout below were checked on this ROM;
**[hypothesis]** and **[unknown]** mark what was not.

This note corrects the assumption in `docs/app/screens.md` that the field's 3D view is the top
screen and that the bottom one is empty: **the field draws its 3D view, message window and
touch UI on the bottom (touch) screen, and the map screen on the top one; the battle does
the opposite.**

## 1. Which LCD shows what, and why

The DS's two display engines are fixed to the 3D core (engine A, "main") and the 2D screens
(engine B, "sub"). Which LCD each feeds is set by `POWCNT1` (`0x04000304`) bit 15.

| Scene | Engine A (3D + message window) | Engine B (2D) | `POWCNT1` bit 15 |
|---|---|---|---|
| Field (free control and field cutscenes) | **bottom LCD**, touch | top LCD (map screen) | **1** |
| Battle | top LCD | bottom LCD (party panels, commands) | 0 (as the SDK leaves it) |
| Menus, title, facilities | top LCD | bottom LCD | 0 |

Evidence, two independent chains:

1. **Code.** `func_ov017_021b1ca8(bit)` (`ov_017::021b1ca8`) is a two-instruction setter of
   `POWCNT1` bit 15 (`ldrh`, `bic #0x8000`, `orr r0, r0, lsl #15`, `strh`). The field's
   screen-setup task calls it **with 1 when the field starts** (`mov r0, #1` at
   `ov_017::021b17ec`) and **with 0 when the field is left** (`mov r0, #0` at
   `ov_017::021b19e4`). Its screenshot-related state machine also saves and restores
   `DISPCNT` (main engine) bits 8-12 around the field (`param_1+0x34/+0x38`). The 3D field is
   engine A, so engine A is on the LCD that shows the 3D view: the bottom one, since the
   battle leaves bit 15 clear and shows the 3D view on the top LCD.
2. **Captures.** melonDS states of the original (local evidence):
   - field: the map screen and the four party name strips are in the **top** half, the 3D
     view, the message window and the "X Menu" affordance in the **bottom** half
     (`build/evidence/j4-bottom-screen-re/field-church.png`, `build/evidence/j4-bottom-screen-re/field-western-stornway.png`);
   - the touch is the bottom half: a tap at image `(128,100)` (top half) does nothing, a tap
     at `(128,300)` advances the message (`build/evidence/j4-bottom-screen-re/field-church-tap-top.png` vs
     `build/evidence/j4-bottom-screen-re/field-church-tap-bottom.png`, same state);
   - battle: 3D view and battle text in the top half, party panels and commands at the
     bottom (`build/evidence/j4-bottom-screen-re/battle-bottom.png`).

The NitroSDK's `GX_Init` (ARM9 `0x020c3700`) also sets bit 15 before powering the engines;
the field's own call is what makes the field differ from everything else.

**Consequence for the port.** `screens::field(swapped)` in `dq9-app` puts the 3D view on the
bottom only when a cutscene's native 109 set the swap; the field itself must run swapped.
`talk.rs` draws the message window on the same screen as the 3D view, which is therefore the
bottom one in the field. Not changed here (documentation batch).

## 2. Field

### 2.1 Top LCD (engine B): the map screen

Asset loading and drawing live in an ARM9 module, `0x02020b98`-`0x02022dd8`. One object
holds the screen state: `+0x9c1` = mode, `+0x9c2` = load step, `+0x9c3` = active,
`+0x94` = an `OBJSpriteSet`, `+0x9fc`/`+0xa00`/`+0xa04` = `BackgroundLoader` tasks,
`+0xa45` = the map code (e.g. `M01M0001`), `+0xa55` = the map section's name.

| Function (EU) | Role |
|---|---|
| `0x02021500` | **`MapScreen_Load`**: `func_020214f0`, dispatches five loaders on `+0x9c1` and returns while a brightness transition runs |
| `0x02022d88` | **`MapScreen_BeginLoad(obj, mode)`**: fades the **sub** screen to −16 (`SetSubBrightness`, `0x0203b1ac`), stores `mode` at `+0x9c1`, restarts the load (`+0x9c2 = 0`, `+0x9c3 = 1`) |
| `0x02020b98` | mode-0 first load helper: opens `/data/pack_lv5/minimap.gp2` |
| `0x02021578` | mode 0: `minimap.gp2` + `/data/ani/obj_mm.pac` (party panels, dots) |
| `0x02021b84` | mode 1: `minimapt.gp2` + `/data/menu/tmap_base.pac` + `obj_mm_tmap_f.pac` |
| `0x02021f88` | mode 3: `obj_mm_tmap.pac` + `minimapbg2` + `minimap.gp2` |
| `0x020227dc` | mode 4: `obj_mm_w.pac` + `minimap.gp2` (world map) |
| `0x02020e58` | BG staging: `BGLayer` reset, text mode, priority 3, offset (0, 0), `.bnsc` staging |
| `0x02020aa0` | VRAM: `DisableSubBGVRAMBanks` then `MapVRAMBanksToSubBG(0x80)` — engine **B** |

Mode 0 (`func_02021578`, `minimap.gp2` + `obj_mm.pac`) is the field's own screen: it is the
only one of the five that loads the party panel pack, and the map the field shows is the
`.obg` picture of `minimap.gp2` (format in the wiki's *Mini-Map* note). **[hypothesis]** that
the field picks mode 0 rather than 1/3; the mode setter's callers are reached through the
field task's indirect calls and were not resolved.

| Element | Purpose | Draw / data | Layout (screen pixels) | State read | Confidence |
|---|---|---|---|---|---|
| Map picture | area plan, `.obg` of `minimap.gp2` drawn as a BG tilemap (`z01`-`z05` palettes) | mode-0 loader above; `.bncg`/`.bncl`/`.bnsc` staging `0x02020e58` | full 256×192, scrolled; `.obg` scale and corner come from the map's `.bmmp` (`0x69`, `0x64`) | map id → `.bmmp` picture | code + capture; scrolling **[unknown]**; port draws the picture at its own top-left (a larger one is cropped) |
| Backdrop | paper pattern under the map picture | the layout's `.bmmp` `0x6a` name → `minimapbg2`–`4` `.obg` of `minimap.gp2` (8×8 cells) | the port repeats it at one texel per pixel over the picture's own rectangle, from its top-left (stated choice, DS layout **[unknown]**) | `.bmmp` `0x6a` | data + code; `221 of 283` layouts name one (`re/minimap.md` § 4.1) |
| Area tab | the map section's name ("Stornway", "Western Stornway", "Observatory - L1") | text; `TMAP_SEC` is the same name inside dialogue (`MsgSubst_TMAP_SEC`, `0x020691d0`) | x 200..253, y 0..15 on `field-church.png` (dark tab, light edge); the port keeps the right edge and grows the tab left when the name is wider | map id → map list name (`maplist9` label, the port's `loaded.label`) | capture; drawing function and palette source **[unknown]** |
| Party strips | one name strip per party slot | the 64×64 panel of `obj_mm.pac` (cell 2 of `obj_minimap.NCER`, name strip = rows 0..15 only) | 4 × **64×16** at **y = 176**, x = **0, 64, 128, 192**; only as many as the party holds | party order `GameState+0x397c`, count `+0x3980`; per-member colour | capture + decomp; colour source **[unknown]**; the port uses the file's palette slots 0–3 (blue, green, pink, orange) **[hypothesis]** |
| Party dots | one coloured dot per member on the map | `obj_minimap.NCER` cells 3..7 / the dots of `obj_mm.pac` | on the party's position, projected by the `.bmmp` scale/corner | hero and members' x/z | wiki + capture; colour choice **[unknown]**; the port draws cells 3..7 and the members' positions coincide (only the hero's is modelled) **[hypothesis]** |
| Floor badge | a small box with the floor (`1` in the Observatory capture) | `tmap_field_obj.NCER`/`.NCGR`/`.NCLR` **or** the same cells as the tab **[hypothesis]** | x≈0..24, y≈3..19 (Observatory capture) | map id | capture only |
| Map markers | doors, houses, marks | `.bmmp` marks (`0x6c`), `obj_mappoint.pac` | from the map position × scale − corner × 8 | `.bmmp` | wiki; which `marker0`–`4` a mark takes **[unknown]** |
| Clock / time of day | **nothing** is drawn in the field HUD; the day clock only drives lighting, rosters and encounters | — | — | `GameState` clock | capture |

The party strips measured in `field-church.png`: row 176 is the black border, rows 177-190
the dark strip with the white name and the two coloured ends, row 191 white. The four slots
are 64 px wide each, in party order, irrespective of colour.

**What the port draws** (`dq9_render::map_screen`, wired in `app/draw.rs` for `--screens
original`): the `.obg` picture the map's `.bmmp` names, one pixel per texel with its top-left
at the screen's top-left; under it, the backdrop the layout's `0x6a` names (`minimapbg2`–`4`,
8×8 cells of paper) repeated at one texel per pixel over the picture's own rectangle, from its
top-left — how the DS lays it out is **[unknown]**, so the rectangle is a stated choice; the
name strips cut from cell 2 of `obj_minimap.NCER` (rows 0..15) with palette slots 0..3 and each
member's name from the record's glyph indices in the message font, centred in the slot; one
dot per member (cells 3..7, palette slot 4) at the projected hero position, so a full party's
dots coincide; and the `maplist9` label in the tab, right aligned. Not drawn, because the data
does not establish them: the scrolling of a picture larger than 256×192, the `.bmmp` marks
(`0x6c`; decoded and projected, but no record says which of the five `marker0`–`4` dots —
blue, green, pink, yellow, red — an ordinary door or house mark takes, and
`obj_mappoint.pac`'s role is **[unknown]**), the original's palette source for the strips and
dots, and the floor badge. A map without a `.bmmp` layout (592 of the map list's 1,010
records, mostly dungeons) leaves the placeholder and the status line says so. Captures:
`build/evidence/v3-map-screen-draw/` and, with the backdrop, `build/evidence/v3-map-screen-details/`.

### 2.2 Bottom LCD (engine A): the 3D view and the touch UI

| Element | Purpose | Draw / input | Layout | State | Confidence |
|---|---|---|---|---|---|
| 3D field | the world, the hero, NPCs, roamers | per-frame field draw `ov_017::021960b8` (sets the projection, `0x020c52e8`/`0x020c5414`, lighting, background gradient, `Object3D` draws); camera `0x0202df78` | whole 256×192 | camera, hero, map | capture + decomp |
| Message window | NPC lines and captions | `MsgWin_Show` `0x0204501c`, `MsgWin_Update` `0x020659a0`, `MsgWin_SetRect` `0x02042ba8`; frames `data/ani/win0..7.spr` (`MsgWin_LoadFrames` `0x0202f25c`) | rect `(2, 0x74, 0xfc, 0x4a)`, i.e. x 2..254, y 116..190; text origin y + 8; only on the 3D screen | window `W = 0x021152f4`, `+0x998` active, `+0x9a0` wait state | decomp + capture |
| "X Menu" affordance | shows that X (or a touch) opens the field menu | not identified as a function **[unknown]**; drawn in the field's engine-A BG1/BG2, whose screens are cleared at field exit (`LoadToMainBG{1,2}{Character,Screen}Data`, `ov_017::021b1c..`); asset **[unknown]** | top-right: the label ("×" icon then "Menu", white on a dark rim) **x 214..243, y 28..35** on the bottom half of `field-western-stornway.png`, its icon's blue rim reaching x 213 and row 27 (the port's touch box: x 213..243, y 27..35) | — | capture; re-measured (§ 2.2 below, `build/evidence/v3-touch-ui/menu-affordance-measured.png`): the earlier `202..255 × 14..28` took the hand for the label; code path of the text/icon **[unknown]** |
| Stylus hand | white gloved pointer at the touch point / on the Menu affordance | **[unknown]** (a game sprite, not melonDS's mouse) | 16×16 around the touch point; in the capture the glove is x 224..234, y 15..26 and its blue outline x 221..236, y 14..26 | touch coordinates | capture |
| Touch actions | advance/close a message, pick up a spot, walk, climb a ladder (drag), steer the ship (drag), press the Menu affordance | `TouchInput::GetTouchPoint` `0x02012a94` (callers `ov_017::021960b8`, `ov_017::021ab0c0`), `TouchInput::IsPressed` `0x02012abc`, `TestHoldInRectangle` `0x02012744` | — | `TouchInput` state, field flags | callers + wiki (Gathering, Getting-Around, Ship) |

The Menu affordance is absent in interiors with a dialogue open (Stornway church capture);
whether it is hidden by the window or by the map is **[unknown]**.

**Touch in the port** (`crates/dq9-app/src/app/input.rs`, `screens.rs`; `docs/app/screens.md`
§ Touch). A left press released after at most 8 window pixels of pointer travel is a tap — the
travel is the path length, so a back-and-forth inside those 8 px is a drag too; anything longer
rotates the camera and fires nothing, so the two gestures never overlap. (A press that arrives
before the window has seen any pointer motion is ignored: `winit`'s button events carry no
position and guessing one would tap the wrong place.) The tap is mapped from its window pixel
to the 256×192 DS screen the field draws on — the bottom screen in `original` mode and after a
cutscene's 109, the top one in `port`, the only screen in `top` and the 16:9 rectangle in the
enhanced layouts (proportional there: a 16:9 field has no DS screen shape **[hypothesis]**) —
and a tap on the other LCD, the letterbox or a black margin does nothing.

On the field's screen it does two things and only two. With a message window open it presses
the action button: the note's `field-church-tap-bottom.png` (256×409, a 25-row melonDS menu bar
above the two LCDs) advances the line from an image tap at `(128,300)` — bottom-screen row 83,
above the window's own rectangle `y 116..190`; the capture helper's tap coordinates are
bottom-relative and would make it row 100. Either way the tap was outside the window, so the
whole 3D screen is the target; the exact point is **[unknown]**, and reading the touched row of
a prompt is not established and is not done. Without a window, a tap on the label's measured
rectangle (x 213..243, y 27..35, **[hypothesis]**: the game's own rectangle
`TestHoldInRectangle` `0x02012744` was not resolved for it) opens the field menu where the key
`I` does, and nowhere else. The original's other handlers (walk and climb by drag, pick a spot,
steer the ship) are not modelled, so the port routes no tap to them. A recorded session replays
keys and camera deltas only, so a tap is ignored while recording or replaying. Neither the
label nor the stylus hand is drawn: their asset is **[unknown]**, and `bg1`/`bg2` are not
modelled, so an invention would replace a missing element rather than reproduce one. Evidence:
`build/evidence/v3-touch-ui/` (`menu-affordance-measured.png`, `tap-trace.txt`, `tests.txt`).

## 3. Battle

| Screen | Element | Draw / input | Layout | Confidence |
|---|---|---|---|---|
| Top (engine A) | 3D battle stage, fighters, action effects | battle camera and presentation in `docs/re/battle.md`, overlays 0/24/25/26 | full 256×192 | capture |
| Top | battle text ("A hammerhood draws near!", damage, results) | the battle's own message window, at the top of the screen (`battle/ui.rs` in the port) | window over the upper part | capture + port |
| Bottom (engine B) | **member bands** (party status) | panel part and palette per member; level `func_ov000_02174738` right-aligned in 16 px at (44, 22) of the panel; acting member's pulse `func_ov000_02170b0c` colours palette entry 15; placement `func_ov000_021811f4`; HP/MP bars and `:Lv` from the panel sprite | 256 px wide bands: hero **y 0..71** (a **64×72** status block at x 0), members 2-4 **y 72..111, 112..151, 152..191**; status block at x≈0..63 (icon, name, Lv, HP, MP), command/tactic window x≈80..238 | capture + wiki |
| Bottom | **command / tactic window** | overlay 0 command UI (`func_ov000_0217df08` the party menu's second row, `func_ov000_02157e1c`, `func_ov000_02154f30`); "Follow Orders" when the member's tactic is not the player's | right part of each member band | capture + wiki |
| Bottom | target choice, item/skill lists | overlays 0 and 26; the same window area | replaces the command window | wiki (Battle-Commands) |
| Bottom | touch | a command is chosen by touching it (wiki Battle-Commands); `TouchInput` `0x02012a94` callers in overlay 23 (`ov_023::021e6448`, `ov_023::021f8970`) | — | wiki + callers |

The battle therefore keeps the SDK's engine assignment: 3D and its text on top, all 2D UI at
the bottom. Coordinates are measured on `build/evidence/j4-bottom-screen-re/battle-bottom.png` (a real chapter-1 fight) and are
consistent with the wiki's panel reading.

## 4. Implementation plan (v0.3), ranked

1. **Field screen assignment (no new data).** Render the field's 3D view and message window
   on the bottom screen and leave the top to the map screen — the original's `POWCNT1` bit 15
   is set for the whole field (`ov_017::021b1ca8`, entry/exit calls above). This is the only
   element that changes the look of every field frame, needs no reader, and also fixes where
   the message window goes. It touches `dq9-app` only (`screens.rs`/`app.rs`, the capture
   path), not the deterministic core. Implemented by `v3-screens-*` (`--screens original`).
2. **Party name strips** (top, four 64×16 strips at y 176, x 0/64/128/192). Data: party order
   and count (already read by `dq9-parity-party`), the panel part of `obj_mm.pac` /
   `obj_minimap.NCER` with its palette. Implemented: `dq9-formats` decodes the
   `obj_minimap` NCER/NCGR/NCLR (`re/minimap.md` § 5) and § 2.1 draws the strips.
3. **Map picture** (the largest element, and what the top screen is for). Data: `.bmmp` per
   map (record table, `docs/re/table.md`), the `.obg` picture and `z01`-`z05` palettes of
   `minimap.gp2` (`.obg` layout in the wiki's *Mini-Map*), then the party dots. Implemented:
   the `.obg`+`.bmmp` reader (`v3-map-screen-data`) and the backdrop (`v3-map-screen-details`).
4. **Area tab** (name over the map) — the map list's name, which the text pipeline already
   resolves as `<TMAP_SEC>`. Cheap once step 3 places the screen.
5. **Menu affordance and touch** on the field: draw "X Menu" (asset still **[unknown]**) and
   hit-test the touch rectangle; the field menu already exists (`field-menu.png`). Also route
   the existing tap-to-advance and drag actions through the same handler. Implemented
   (`v3-touch-ui`): a tap routes to the message window's action button and to the label's
   measured rectangle, the camera drag suppresses it, and neither the label nor the hand is
   drawn (their asset stays **[unknown]**, § 2.2 "Touch in the port").
6. **Battle bottom-screen check**: the port already draws party panels and command windows
   there; compare the bands' geometry (hero band 72 px, others 40 px) with the port's layout.

Minimum for "not a blank background" in the field: 1 (screen assignment) and 2 (strips);
1 and 3 for the screen to look like the original. Steps 1 to 5 are implemented
(`v3-screens-*`, then `v3-map-screen-data`, `v3-map-screen-draw` and
`v3-map-screen-details`, § 2.1 "What the port draws"; `v3-touch-ui` for the tap grammar of
§ 2.2); step 5's assets stay **[unknown]**, so its label and stylus hand are not drawn.

## Evidence

Local, never committed (`build/evidence/j4-bottom-screen-re/`):

| File | What | sha256 (12) |
|---|---|---|
| `field-church.png` | free field in Stornway's church, original (state `pro1`) | `9fcd2ead5d31` |
| `field-church-tap-top.png`, `field-church-tap-bottom.png` | same state after a tap in the top half (nothing) and the bottom half (message advanced) | `f0b813c63eb1`, `187019365aa1` |
| `field-western-stornway.png` | map screen and "X Menu" + hand, Western Stornway (roaming run, copied from `build/evidence/j4-roaming-challenge/`) | `9c0d549c62e5` |
| `battle-bottom.png`, `battle-bottom-2x.png` | a real chapter-1 fight: 3D top, panels and "Follow Orders" at the bottom | `7c8128836dfd`, `a7404d5d80c3` |
| `field-observatory.png` | the Observatory map with the floor badge (state `bat6`) | `c0fe94e7d2db` |
| `field-pro11.png` | a field cutscene state: the option screen and window | `ccb7e2dccb8d` |

Map screen implemented (`build/evidence/v3-map-screen-draw/`, then the backdrop — and the marks,
decoded but not drawn — in `v3-map-screen-details/`): `original-M01.png` (`13d50e43d010` /
`b319a0f1adbd` with the backdrop), `original-F01.png` (`b97b361c63cf`) and `original-S01M01.png`
(`08033af76f1e`), or `original-F01M01.png` (`127ab848910c`, the placeholder) and `original-S01M01.png`
(`72ddf077c467`) from the details batch; window captures of `--map <code> --save …
--layout stacked --screens original`; `sweep.txt` (`9c1ec9f9e913`, `b25081e83a2a`), the
283-layout and 1,010-map resolution test (418 draw, 592 fall back); `backdrops.txt`
(`db3f23c110a2`) and `obg-members.txt` (`9538d3e5c321`), the archive's backdrop and marker
members; the tab measured on `field-church.png` at x 200..253, y 0..15 (dark `97,65,40`, edge
`211,146,48`, white name); the port's `--screens port` reference is byte-identical to the
parent commit (`3475c61`).

Touch UI implemented (`v3-touch-ui`, `build/evidence/v3-touch-ui/`):
`menu-affordance-measured.png` (`b21563f6ebbc`, the measured label box in green, the note's
earlier box in red over the stylus hand), `tap-trace.txt` (`d16a4a0b6814`, the `X01M07`
conversation leaving its wait on a tap at DS `(128,96)`: arrow at tick 105, `-` at 106, the
prompt at 158) and `tests.txt` (the mapping, gesture, menu and tap tests).

Ghidra queries (cache under `build/ghidra-cache/`): `decomp ov_017::021b1564`,
`ov_017::021960b8`, `0x020214f0`, `0x02022d88`; `calls 0x02021500`, `0x02012a94`,
`0x02012abc`; `xrefs` on the asset strings `0x020ef4b0`, `0x020ef5a5`, `0x020ef5c0`,
`0x020ef5df`, `0x020ef5fa`, `0x020ef649`, `0x020ef67c`.

## Not established

- The field's map-screen mode (0 assumed) and which element draws the area tab, the floor
  badge and the "X Menu" text/hand (asset and function).
- How the `.obg` picture scrolls (the port crops a larger one at its top-left) and the
  colours of the party's dots; their placement is the projection of § 2.1 (the members'
  own positions are not modelled, only the hero's, so their dots coincide).
- How the DS lays out the backdrop (`0x6a`): the port tiles it over the picture's own
  rectangle (`re/minimap.md` § 4.1, § 6).
- Which of `marker0`–`4` an ordinary door or house mark takes, and what `obj_mappoint.pac`
  holds (`re/minimap.md` § 4.1).
- Whether the Menu affordance is a touch target (it is drawn like one) or only an X hint: the
  port hit-tests the label's measured box (a decision, **[hypothesis]**), not the game's own
  rectangle (`TestHoldInRectangle` `0x02012744` was not resolved for it).
- The party colour source (not a paletted entry of the map sprite set).
- What the sub-screen fade (105/114) does while the field is swapped: the field's fade code
  treats the two engines by the logical swap flag (`0x020db9a8`, `0x020db360`), not by LCD.
