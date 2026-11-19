# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 19, 2026 (evening)

**v0.1 functional natively; v0.2's native and browser criteria are met by the pilot, the
hand-played one is not.** The pilot plays a new game with the real battle engine to **event
2750** (story **2.6.5**, **3686.9 s** of game time, 972 checkpoints, digest `f33a42d4…`), in the
window, in Edge and on the hosted page. The user hand-played a session, stopped it at the first
defects and replayed it after their fixes (wave below); a full hand-played run to 2750 remains.
Fixed 60 Hz loop; hero, field and model pose at 30 Hz.

## Integrated on main (every batch passed `verify.ps1 -RequireRom`)

| Area | Batches (commit) |
|---|---|
| First session fixes | `j4-message-flow` b10e337 (`<ADD>` waits for a key, one-press Yes/No), `j4-message-arrow` 749a03e (32/32 wait arrow, A/B/X/L/R/d-pad), `j4-stair-run` ed5108f (run on stairs), `j4-angel-falls-cast` eb45d8f (Ivor once, default motion), `j4-benevolessence` 7854d2b (effect billboards), `j4-scene-camera` 9af9908 (map home yaw, four framings), `j4-mortal-veil` b476940 + `j4-winged-form` 9866b5a (palette add `0x97(1)`, rooms 1100–1110 at 1.2–1.3; wings `s023n2`, halo `s069`) |
| Second session fixes | `j4-outline-width` 3434887 (edge 1 DS px at 17/32), `j4-contact-shadow` 430688f (halo off in the enhanced default), `j4-cast-scenes` d522143 (568 bit `0x20`; a bound NPC a scene moved stays), `j4-field-facing` cfc830e (seated sprites lean away; roamer headings pinned) |
| Tooling k1/k2 | `k1-fast-test` a713ea8, `k1-linker` 10f94e1, `k1-integrate` dc99dbb + `k1-queue-fix` b6c76e7, `k1-dispatch` 5bbc5e5 + 569d1bf, `k1-ghidra-corpus` d184e3f, `k1-app-split` a15a406, `k1-docs-split` 0955548, `k2-recomp-facts` cc290ae |
| Earlier v0.2 | pilot and web routes, parity (2/2), battle rules and UI, field services and containers, chapter 2 story, enhanced presentation, text speed, human record/replay |

## Limits recorded, not hidden

- **Parity**: two original references, both in the prologue; the original's RNG drew another
  battle-26 monster set (three `accepted` entries). Chapter 2 has no comparison with the original.
- **Timing**: shown and instant runs are not tick-comparable; any change of dialogue or transition
  timing re-pins the route digest. A `dq9rec` is replayed in its presentation.
- **Text**: `<AUTO=n>` unmodelled (no prologue text carries it); codes the port does not model cost
  0 units against 1; two states of the window probe still print "arrow".
- **Battle**: the post-contact freeze, load blacks and formation (`79`) are unmodelled; enrage
  draws for heal, metal rules, tension and the skill-tree equip rule are later milestones.
- **Cast**: the 568 rule is read from code and two states, not observed inside the scenes; the
  "second room" is an inference (`X01M01`); natives 581/582 and the sprite billboard bit 4 are
  unmodelled; the notice marker over a roamer that spotted the party is not drawn.
- **Seated sprites**: the lean's sign rests on the machine code (a 45° capture cannot resolve
  under a pixel); the wings' own gate under `0x97(2)` and tint part `0x15` are `[unknown]`.
- **Screens**: `--screens original` leaves the top screen a placeholder (map and party strips need
  `re/bottom-screen.md` steps 2–3).
- **Wipe-out**: the save-carried revival map, trigger 180 and `ME_009` are unmodelled.

## Next

(1) The user's full hand-played run to 2750, recorded (`--record`), and his answers on the
"second room" and on the roaming monsters; (2) the top screen's map and party strips, then
`--screens original` as default after the user's test; (3) more original references: the
DQ9Recompiled runner on a spare slot (time-boxed, `re/dq9recompiled.md` § 9) or more melonDS
states; (4) its 144 hook anchors for battle rewards, per-hit damage and the 30 Hz cadence;
(5) an autonomous coordinator over `queue.ps1`/`dispatch.ps1` is **not enabled**: it waits for the
user's go. Pages deploys on each push to `main`. Evidence stays local under `build/evidence/<batch>/`.
