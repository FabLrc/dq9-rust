# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 26, 2026 (night)

**v0.1 functional natively; v0.2's native, browser and headless criteria are met by the pilot, the
hand-played one is not.** The pilot plays a new game with the real battle engine to **event 2750**
(story **2.6.5**, 964 checkpoints, digest `d721c11f…`, Hexagoon lost Lv 5–8 then won at Lv 9 after
this wave's logic changes), natively, in Edge and on the hosted page. **v0.3 has started**: a played
session is written into a DS save's normal slot, sealed and mirrored, and melonDS loaded the written
container and continued into Stornway — saving from a run is real and DS-acceptable.

## Integrated on main (every batch passed `verify.ps1 -RequireRom`)

| Area | Batches (commit) |
|---|---|
| Screen and map | `v3-map-screen-data` 3475c61 (NCLR/NCGR/NCER, `minimap.gp2`, `.bmmp`/`.obg`/`.pac`), `v3-map-screen-draw` c634a0e (picture, strips, dots, tab in `--screens original`, native), `v3-map-screen-details` 7095714 (backdrop; marks decoded, not drawn) |
| Field | `v2-door-task` 1d4e76b + `v2-door-task-fix` 82b0954 (warp type-1 task: confirm edge, arming, 15-tick fades, map change at black; wiring in the doors module) |
| Battle | `v4-battle-presentation` 9854488 (12-frame contact freeze, 94/95 load blacks, 15-update delay, ROM row formation), `v3-reward-hooks` ac711d5 (item-finding drop passes, four reward anchors pinned, two challengers) |
| Encounter | `v3-encounter-flight` 94b95de (four-slot recent ring with cursor, battle-exit reset/hide, only a flight keeps state 9; facing/surprise tests) |
| Dialogue | `v3-dialogue-cast` da52e20 (partner held and turned; sprite/model rates), `v3-dialogue-rturn` 4a620cb (`<R_TURN>`/`<END_R_TURN>` executed), `v3-balloons` 363579d (60-tick balloon from both codes, DS window and enhanced HUD, synthetic capture) |
| Hero | `v3-hero-skin` 88f5588 (face/arm palette recolour from `palette.bin`; the ROM falsified the "painted palette" assumption) |
| Saving | `v3-save-writer` 91b0095 (every reader field writable, `dq9re save --set/--out`, mirror), `v3-save-bind` 70c8109 (GameState → slot, `--save-out`, melonDS load) |
| Web | `v3-web-screens` 9fe47ab (defect report) + 05cc6e5 (WGSL uniform-control-flow fix, naga source test); hosted page redraws the map screen (328,434 non-black px, 0 before) |
| Docs | `v3-docs-consistency` 9d9d313 (10 docs aligned), route re-pin 794829d, roadmap/links fixes |

## Limits recorded, not hidden

- **Route**: the re-pin's cause is the door/encounter timeline shift; up to checkpoint 701 only ticks
  differ from the previous artifact, the first state difference is one extra battle's 25 experience
  before the Hexagoon (`build/evidence/v3-route-after-door/first-difference.txt`).
- **Map screen**: marks are decoded but not drawn (no record names a marker); the scroll of a picture
  larger than 256×192, the `.obg` `unknown_03` and the area tab's draw function stay `[unknown]`;
  party members' own field positions are not modelled, so their dots coincide.
- **Balloons**: the 60-tick timer and the `(-12,-62)` projection are drawn in the window and the Web;
  no prologue/chapter-2 line carrying `<EXC>`/`<QES>` is reachable, so the capture is synthetic, and
  the `--screenshot` preview does not feed the camera (one-line hook in `re/dialogue.md`).
- **Saving**: base attributes (`+0x14c` fields 0–6) and current HP/MP are not written (derived at
  load); the DS acceptance is proven once (melonDS), not per field; no save menu in play yet.
- **Battle**: intro camera, the row's >8-monster case and the backup row-flag call remain
  unmodelled; the live+0xA8 item-finder gate and multiplayer share semantics stay hypotheses.
- **Cast/screens**: the notice marker over a spotting roamer is not drawn; the `.bmmp 0x6a` backdrop
  layout, the touch UI ("X Menu", stylus) and the battle bottom screen's exact bands are open.
- **Parity**: two original references, both prologue; chapter 2 has none.

## Next

(1) The user's full hand-played run to 2750, recorded (`--record`), and his test of
`--screens original` as the default; (2) v0.3: a save/quick-save menu in play, title screen and
character creation, church Confession; (3) party companions on the field (the map screen's dots and
the field's other members), then the minimap marks/scroll; (4) more original references
(DQ9Recompiled runner or melonDS states); (5) the `--screenshot` balloon hook and the remaining
`re/dialogue.md` items. Pages deploys on each push to `main`; evidence stays local under
`build/evidence/<batch>/`.
