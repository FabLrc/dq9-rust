# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 27, 2026 (night)

**v0.1 functional natively; v0.2's native, browser and headless criteria are met by the pilot, the
hand-played one is not.** The pilot plays a new game with the real battle engine to **event 2750**
(story **2.6.5**, 964 checkpoints, digest `d721c11f…`), natively, in Edge and on the hosted page;
the route passed a full `-Only long` run on the wave-3 tree (`12a85b9`), and `v4-metal-rules` ran it
again on its own tree — the last two batches only touch weapons the pilot does not carry and the
save loader the route does not use, so no checkpoint can move. **v0.3 is under way and saving works
in play**: the church's Confession and the field menu's Quick Save write the DS slots (normal and
suspend), a written container loads in melonDS, and the suspend slot resumes at its warp.

## Integrated on main (every batch passed `verify.ps1 -RequireRom`)

| Area | Batches (commit) |
|---|---|
| Screen and map | `v3-map-screen-data` 3475c61, `v3-map-screen-draw` c634a0e (picture, strips, dots, tab), `v3-map-screen-details` 7095714 (backdrop; marks decoded, not drawn) |
| Field | `v2-door-task` 1d4e76b + `v2-door-task-fix` 82b0954 (type-1 warp task), `v3-party-companions` 7c302ef (the 16-point follow rule, rigs, dots at each place) |
| Battle | `v4-battle-presentation` 9854488, `v4-battle-intro` fe8b020 (opening camera, 47 kind shots, eight-slot cap), `v4-metal-rules` abb523e (miss/zeroing/exempt actions), `v4-metal-equipment` 025228f (the metal/needle bits from the equipped weapon, mapping settled on the ROM) |
| Encounter | `v3-encounter-flight` 94b95de (recent ring, battle-exit reset) |
| Dialogue | `v3-dialogue-cast` da52e20, `v3-dialogue-rturn` 4a620cb, `v3-balloons` 363579d (60-tick balloon; preview hook d70e966) |
| Hero and touch | `v3-hero-skin` 88f5588, `v3-touch-ui` 343d9a1 (tap routes to the window or the measured X Menu box) |
| Saving and church | `v3-save-writer` 91b0095, `v3-save-bind` 70c8109 (melonDS-loaded), `v3-save-menu` bcb37ef (Confession), `v3-suspend-save` a1dedec (Quick Save into slot 1), `v3-suspend-resume` d92ee2f (boot choice, log deletion) |
| Web and rendering | WGSL fix 05cc6e5 (hosted map screen), mipmapped-texture option removed on request b4a4bcc |
| Docs | `v3-docs-consistency` 9d9d313, route re-pin 794829d, README/roadmap/status updates |

## Limits recorded, not hidden

- **Route**: re-pinned after the door/encounter timeline shift; up to checkpoint 701 only ticks
  differ from the older artifact, the first state difference is one extra battle's 25 experience
  before the Hexagoon (`build/evidence/v3-route-after-door/first-difference.txt`).
- **Suspend save**: the port resumes the **normal** slot first where the title ranks a quick save
  above it (the menu consumer is **[unknown]**); `--save-out` on a suspend-only container refuses;
  the Web keeps its uploaded bytes; DS acceptance of a written suspend slot is **[hypothesis]**.
- **Metal**: the needle's accuracy auto-hit, the mask `0xfff` claim of `func_0209a088` against
  `battle-data.md`/`equipment.rs` (same count on this file), and the weapons' names stay open.
- **Map screen**: marks decoded but not drawn (no record names a marker); scrolling, `.obg
  unknown_03` and the tab's draw function stay `[unknown]`.
- **Companions**: scene formation, an airborne follower's angle, the state-2 mirror, the
  `func_02046b08` gate and the live status word behind the down flags remain unmodelled.
- **Church**: no window capture of Confession; trigger `0xC` not fired; base attributes
  (`+0x14c` 0–6) and current HP/MP are derived, not written.
- **Balloons**: synthetic capture (no prologue/chapter-2 line carries `<EXC>`/`<QES>`).
- **Parity**: two original references, both prologue; chapter 2 has none.

## Next

(1) The user's full hand-played run to 2750, recorded, and his test of `--screens original` as the
default; (2) v0.3: the title screen/Continue (with the quick-save ranking), `--save-out` after a
suspend resume, character creation; (3) the map screen's marks/scroll and window captures of the
dots and the Confession; (4) more original references and the remaining `re/*.md` unknowns.
Pages deploys on each push to `main`; evidence stays local under `build/evidence/<batch>/`.
