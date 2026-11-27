# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 27, 2026 (night)

**v0.1 functional natively; v0.2's native, browser and headless criteria are met by the pilot, the
hand-played one is not** (the pilot plays with the real battle engine to **event 2750**; the final
tree of this stretch, `bfe09ec`, passed the full `-Only long` route again, 1043 s).

**v0.3's criteria are met** (roadmap J5): the title screen ranks and resumes the DS slots like
`ov_004::0216dad0`; character creation asks the name, sex, face, hair, colours, skin and eyes; the
church's Confession and the field menu's Quick Save write the normal and suspend slots; the **DS
loaded** the normal save and **accepted and resumed the written suspend slot at its exact warp**
(melonDS, `v3-suspend-ds`), and the port's post-load slot image is **byte-identical to the DS's**
(`v3-suspend-backup`).

## Integrated on main (every batch passed `verify.ps1 -RequireRom`)

| Area | Batches (commit) |
|---|---|
| Title and creation | `v3-title-screen` 7ab28ae, `v3-character-creation` b13104c, `v3-save-appearance` 33694a6, `v3-character-appearance` e834c4f (face/hair/colours/skin/eyes pages) |
| Saving | `v3-suspend-saveout` 4f084cb, `v3-base-attributes` d80a00a + 55fc1b6 (current HP/MP 7/8, maxima 9/10; wounded round-trips), `v3-real-save-tests` 5fcc66f, `v3-suspend-ds` 0752ab0, `v3-suspend-backup` 8e357ae |
| Battle | `v4-needle-accuracy` a1e5755, `v4-metal-equipment` 025228f, `v4-battle-data-mask` e9d53fb, `v4-family-bonus` d2893c1, `v4-attack-buff` 71022a0, `v4-multitarget-falloff` b83c9cf, `v4-party-weapon-strikes` 02515d9, `v4-tension` 34d7123, `v4-psyche-up` 3971480, `v4-party-level` 359c540, `v4-gone-targets` 4e6d99d |
| Formats and screen | `v4-item-names` df42ae9, `v3-map-marks` 48b5b98, `v3-map-scroll` 92b53cb |

## Limits recorded, not hidden

- **Suspend save**: the title's Continue ranks the quick save first and writes the DS's backup; the
  `--save` boot path still takes the normal slot first (documented divergence); the Web page keeps
  its uploaded bytes; the DS's story-word refresh from the backed-up log (19.2.1 vs 19.3.1) and the
  corrupted-log refusals (codes 4, 6, 7) are not modelled.
- **Creation**: the face presses the overlay-9 options; the 3D preview page (overlay page 8) and
  page 2's face offset draw a resolved-part placeholder, and the original keyboard layout and the
  `0xd95` mode-1 defaults stay [unknown].
- **Battle**: B13 (tension at 100) is not portable (the port has none of its five statuses); Egg On
  (168/330) and action `0xDB` stop at their unestablished reach 8; the acting-unit redirect
  (`func_ov024_021e9f68`) is read but unreachable in the port's model; action `0x61`'s second blow,
  the combo gate's second bit and the 66 damage handlers past slot 0 stay named/refused.
- **Map screen**: marks place sprites, never a marker tile; draw modes 1 and 3 are not modelled
  (kind 0/7 stay fixed); the field's loader-mode-0 assumption is a hypothesis.
- **Companions, church, balloons**: scene formation, an airborne follower's angle, the state-2
  mirror, the Confession's window capture and its trigger `0xC`, and a real balloon line stay open.

## Next

(1) The user's full hand-played run to 2750, recorded, and his test of `--screens original` as the
default; (2) a browser pass on the hosted page's title/New Game; (3) the J5 backlog beyond the
milestone: shops beyond Angel Falls, the bank, tactics, skills, party and vocations (overlays 2, 3,
4, 5, 11, 13); (4) more original references and the remaining `re/*.md` unknowns.
Pages deploys on each push to `main`; evidence stays local under `build/evidence/<batch>/`.
