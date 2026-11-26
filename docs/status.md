# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 27, 2026 (night)

**v0.1 functional natively; v0.2's native, browser and headless criteria are met by the pilot, the
hand-played one is not.** The pilot plays a new game with the real battle engine to **event 2750**
(story **2.6.5**, 964 checkpoints, digest `d721c11f…`, Hexagoon lost Lv 5–8 then won at Lv 9),
natively, in Edge and on the hosted page — the route passed a full `-Only long` run after the last
logic changes. **v0.3 is under way and saving works in play**: the church's Confession writes the
adventure log into the save's normal slot (sealed, mirrored, atomic), the written container loads in
melonDS, and the party's other members follow the leader on the field.

## Integrated on main (every batch passed `verify.ps1 -RequireRom`)

| Area | Batches (commit) |
|---|---|
| Screen and map | `v3-map-screen-data` 3475c61 (NCLR/NCGR/NCER, `minimap.gp2`, `.bmmp`/`.obg`/`.pac`), `v3-map-screen-draw` c634a0e (picture, strips, dots, tab in `--screens original`), `v3-map-screen-details` 7095714 (backdrop; marks decoded, not drawn) |
| Field | `v2-door-task` 1d4e76b + `v2-door-task-fix` 82b0954 (warp type-1 task: confirm edge, arming, 15-tick fades, map change at black), `v3-party-companions` 7c302ef (the 16-point follow rule of `0x020391ac`, rigs per member, dots at each place) |
| Battle | `v4-battle-presentation` 9854488 (contact freeze, 94/95 load blacks, 15-update delay, row formation), `v4-battle-intro` fe8b020 (opening camera eased at `0xcd/4096` per 30 Hz update, the 47 kind shots, eight-slot cap), `v3-reward-hooks` ac711d5 (drop passes, reward anchors) |
| Encounter | `v3-encounter-flight` 94b95de (four-slot recent ring, battle-exit reset, only a flight keeps state 9) |
| Dialogue | `v3-dialogue-cast` da52e20, `v3-dialogue-rturn` 4a620cb (`<R_TURN>`/`<END_R_TURN>`), `v3-balloons` 363579d (60-tick balloon, window and Web; the preview hook  d70e966) |
| Hero and touch | `v3-hero-skin` 88f5588 (palette recolour), `v3-touch-ui` 343d9a1 (tap routes to the window or the measured X Menu box; nothing invented) |
| Saving and church | `v3-save-writer` 91b0095, `v3-save-bind` 70c8109 (`--save-out`, melonDS-loaded), `v3-save-menu` bcb37ef (Confession flow with `str_ch0`'s lines, 110 ends the session), fallback cleanup 46b1ba7 |
| Web and rendering | WGSL fix 05cc6e5 (hosted map screen draws), **mipmapped-texture option removed** on request b4a4bcc (textures are the DS's nearest, one level), map-dot doc f732f2f |
| Docs | `v3-docs-consistency` 9d9d313, route re-pin 794829d, README/roadmap/status updates |

## Limits recorded, not hidden

- **Route**: re-pinned after the door/encounter timeline shift; up to checkpoint 701 only ticks
  differ from the older artifact, the first state difference is one extra battle's 25 experience
  before the Hexagoon (`build/evidence/v3-route-after-door/first-difference.txt`).
- **Map screen**: marks are decoded but not drawn (no record names a marker); the scroll of a
  picture larger than 256×192, `.obg unknown_03` and the tab's draw function stay `[unknown]`.
- **Companions**: the follow rule is read, but the scene formation (`+0x26c`), an airborne
  follower's angle, the state-2 mirror, the `func_02046b08` gate and the live status word behind
  the down flags remain unmodelled or hypothesised; no window capture of the map dots.
- **Church**: no window capture of the Confession screen; its trigger `0xC` is not fired (no
  consumer known); a session without `--save`/`--save-out` has no container and says so.
- **Balloons**: no prologue/chapter-2 line carries `<EXC>`/`<QES>`, so the capture is synthetic.
- **Battle**: the party-count override of `func_ov000_0215fc60`, `0x115`'s variant counter and the
  kind-0 default remain open; the row's eight-slot capture path is not exposed to `--screenshot`.
- **Saving**: base attributes (`+0x14c` fields 0–6) and current HP/MP are not written (derived at
  load); the DS acceptance is proven once (melonDS).
- **Parity**: two original references, both prologue; chapter 2 has none.

## Next

(1) The user's full hand-played run to 2750, recorded (`--record`), and his test of
`--screens original` as the default; (2) v0.3: a suspend/quick save beside the Confession, the title
screen and character creation; (3) the map screen's marks/scroll and a window capture of its dots
and of the Confession; (4) more original references (DQ9Recompiled runner or melonDS states);
(5) the remaining `re/dialogue.md`, `re/battle.md` and `re/facility.md` unknowns. Pages deploys on
each push to `main`; evidence stays local under `build/evidence/<batch>/`.
