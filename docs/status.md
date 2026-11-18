# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 18, 2026

**v0.1 functional natively; v0.2's native and browser criteria are met by the pilot, the
hand-played one is not.** A new game played by the pilot with the real battle engine reaches
**event 2750** (Hexagoon lost at Lv 4–7, won at Lv 8, story **2.6.5**, **3627.8 s** of game time,
972 checkpoints; it was 3035.5 s before `d068809` slowed dialogue to the original rate). The same
route runs in the wasm build in Edge and on the hosted Pages page, instant and shown battles, with
the native run's state (`j4-web-pilot`, `j4-web-show`; ±2-step tick offsets unmodelled). Parity is
2/2 with party and gold compared (`fddd4c6`). The route is pinned by an `#[ignore]` real-ROM test
(`verify.ps1 -Only long`, ~16 min, run by the coordinator before a milestone). A human session can
be recorded and replayed (`--record`, `--play-record`, `j4-human-record`): **none recorded yet**.
Fixed 60 Hz loop; hero, field and model pose at 30 Hz.

## Integrated on main (every batch passed `verify.ps1 -RequireRom`)

| Area | Batches (commit) |
|---|---|
| Route and browser | `j4-chapter2-route` a647098, `j4-web-pilot` a861eaf, `j4-show-battles` 2e580a1, `j4-web-show` ead8d8b, `j4-route-test` 80943a1, `j4-human-record` 7e2e62a, `j4-parity-regress` fddd4c6 |
| Parity and oracle | `j4-parity-party` daa9553 (party/gold compared; fought ≠ stand-in on 611/615 lines), `j4-parity-regress` (native 560 = 1 moved event 1140's timeline; open-loop trace replaced by the pilot's) |
| Battle rules | `j4-full-heal`, `j4-audit-fixes` ed0974b (wear-off, flight 38–40), `j4-xp-share` ca074aa + `j4-xp-wire` 3f8422e, `j4-grudge-enrage` 4e0a0ed, `j4-item-spend` 2da4e51, `j4-spawn-block` e775c65 |
| Battle UI and scene | `j4-battle-motions`, `-gpu-sync`, `-skills`, `j4-abilities-capture` 97fd1e2, `j4-battle-worn-items` 0b5ceda, `j4-battle-transition` e973f6b (swirl read and clocked, **not wired**) |
| Field, services, story | `j4-field-services`, `-containers`, `j4-field-menu-rules` fcd135a, `j4-wipeout-priest` 615b24c, `j4-ch2-gaps` 2930280 (49/50), `j4-guest-slot`, `j4-gate-*`, `j4-aquila-bug` 5349f43 (face decal one tick late) |
| Presentation | `j4-enhanced-mode` 8189bf9 (`enhanced` default, ten `--fx`, `enhanced-reactive`), `j4-screens-original` 5ecb58e (opt-in), `j4-text-speed` d068809 + f516d79 (1 unit/frame, A does not skip, `--text-speed`) |
| Research | `j4-encyclopedia-audit` 236ea6a, `j4-bottom-screen-re` 2183df4 (the original runs the field with swapped screens), `j4-vegetation-billboards` bbe8bc3 (F-map bushes are fixed quads: normal), `j4-docs-consistency` 598cef3 |

## Limits recorded, not hidden

- **Parity**: the original's RNG drew another battle-26 monster set (HP 16/20 vs 13, XP 7 vs 2, gold
  13 vs 4); three `accepted` entries pin the runtime's value. Only two original references exist,
  both in the prologue: chapter 2 has no comparison with the original.
- **Timing**: shown runs are not tick-comparable with instant ones (draw divergence from checkpoint
  715, **[hypothesis]**). Any change of dialogue or transition timing re-pins the route digest.
- **Battle**: the swirl, the intro camera arc, the 12-frame post-contact freeze and the load blacks
  are read (`re/battle-transition.md`) but not wired into the app (`app.rs`/`load.rs`, hook in
  `build/evidence/j4-battle-transition/app-hook.md`). Formation (`79`), action-script cameras and
  companions on the scene are unmodelled. Enrage draws for heal/Zing actions, metal rules, tension,
  the weapon/shield skill-tree equip rule are later milestones. The xp counter's hold bit is **[unknown]**.
- **Screens**: `--screens original` leaves the top screen a plain placeholder (map and party strips
  need `re/bottom-screen.md` steps 2–3). The F-map camera does not rotate in the original (F02,
  4 states); our Q/E still rotates it; the lock (`+0x244` bit 1 in `FieldCamera_Update`) is unread.
- **Text**: codes the port does not model cost 0 units against 1 in the original; `<AUTO=n>` and the
  battle window's pacing are not played by `Dialogue`; no window capture of the new pacing.
- **Cutscenes**: bone cameras 552/572 read the current-frame bone; the hero's two halos diverge on
  odd ticks in 21507; `s019n`'s blank face in 21593/21595 is not examined; 2535+22510 stays partial.
- **Wipe-out**: the save-carried revival map, trigger 180 and `ME_009` are unmodelled.
- **Route mode** is a test driver; `D01M02`'s west stair being a dead end is **[hypothesis]**.
  Containers are runtime objects (nothing drawn); pending placement states (`+0x41b`) not modelled.

## Next

Nothing is unmerged. (1) The user's hand-played, recorded session, replayed by `--play-record`;
(2) wire the swirl and run the shown route before/after; (3) the F-map camera lock; (4) more
original references beyond 1.1.2 (melonDS states along the route); (5) the top screen's map and
party strips, then make `--screens original` the default after the user's test.
Pages deploys on each push to `main`. Evidence stays local under `build/evidence/<batch>/`.
