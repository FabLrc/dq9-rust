# Project status

Rewritten by the coordinator at integration; no history log.
Plan: [`roadmap.md`](roadmap.md); rules: [`../AGENTS.md`](../AGENTS.md).

## Status as of November 27, 2026 (evening)

**v0.1 functional natively; v0.2's native, browser and headless criteria are met by the pilot, the
hand-played one is not.** The pilot plays a new game with the real battle engine to **event 2750**
(story **2.6.5**), natively, in Edge and on the hosted page; the full `-Only long` route passed again
on the final tree of this stretch (`359c540`, 1045 s, pinned checkpoints).

**v0.3's port-side criteria are met**: the title screen ranks and resumes the DS slots like
`ov_004::0216dad0`, character creation asks the name and sex (the sex's look defaults) and writes them
with the hero's appearance, the church's Confession and the field menu's Quick Save write the normal
and suspend slots, a suspend-only container resumes at its warp and can later save a normal slot.
The **normal** save was loaded by melonDS and continued; the **suspend** slot's DS acceptance is the
remaining [hypothesis] (the port resumes it).

## Integrated on main (every batch passed `verify.ps1 -RequireRom`)

| Area | Batches (commit) |
|---|---|
| Title and creation | `v3-title-screen` 7ab28ae (ranked Continue, 110/4702 return), `v3-character-creation` b13104c (name/sex, 8 glyphs), `v3-save-appearance` 33694a6 (the look written) |
| Saving | `v3-suspend-saveout` 4f084cb, `v3-suspend-resume` d92ee2f, `v3-base-attributes` d80a00a + `55fc1b6` (fields 7/8 current HP/MP, 9/10 maxima; a wounded party round-trips), `v3-real-save-tests` 5fcc66f |
| Battle | `v4-needle-accuracy` a1e5755, `v4-metal-equipment` 025228f, `v4-battle-data-mask` e9d53fb, `v4-family-bonus` d2893c1, `v4-attack-buff` 71022a0, `v4-multitarget-falloff` b83c9cf, `v4-tension` 34d7123, `v4-psyche-up` 3971480, `v4-party-weapon-strikes` 02515d9, `v4-party-level` 359c540 |
| Formats | `v4-item-names` df42ae9 (the tables' names block; the metal weapons' names) |
| Map screen | `v3-map-marks` 48b5b98 (marks are metadata, no marker tile), `v3-map-scroll` 92b53cb (BG0 scroll, dead zone, fixed BG1 backdrop) |

## Limits recorded, not hidden

- **Suspend save**: the port's boot `--save` still takes the normal slot first where the title ranks
  the quick save above it (the title's own Continue uses the right ranking); `--save-out` on a
  suspend-only container now writes slot 0; DS acceptance of a written suspend slot is
  **[hypothesis]**; the Web page keeps its uploaded bytes.
- **Character creation**: name (8 glyphs, no space — glyph 0 terminates) and sex only; the
  face/hair/skin/eye pages are not modelled and the full appearance comes from the sex defaults
  (written into the save).
- **Battle**: B13 (tension 100 immunity) is not portable — the port has none of its five statuses;
  Egg On (168/330) and action `0xDB` stop at their unestablished reach 8; the gone-target filter and
  the acting-unit redirect are not modelled; the combo gate's second bit, action `0x61`'s second blow
  and the 66 damage handlers past slot 0 stay named/refused.
- **Map screen**: marks place sprites, never a marker tile (no glyph exists); draw modes 1 and 3 are
  not modelled (kind 0/7 stay fixed); the field's loader-mode-0 assumption is a hypothesis.
- **Companions, church, balloons**: scene formation, an airborne follower's angle, the state-2
  mirror, the Confession's window capture and its trigger `0xC`, and a real balloon line stay open.

## Next

(1) The user's full hand-played run to 2750, recorded, and his test of `--screens original` as the
default; (2) melonDS acceptance of a written **suspend** slot (title → quick save → warp) and a
browser pass on the hosted page's title/New Game; (3) v0.3 facilities and menus beyond chapter 2
(bank, shops, tactics, skills) and the creation's appearance pages; (4) more original references and
the remaining `re/*.md` unknowns.
Pages deploys on each push to `main`; evidence stays local under `build/evidence/<batch>/`.
