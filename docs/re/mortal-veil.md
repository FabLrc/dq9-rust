# The hero's winged (Celestrian) form in the field

Batch `j4-mortal-veil`. **Status: the visual and its gate are read from the code and the
scenario data; the hero in the field is drawn with the original's palette operation. The
wings are not drawn (open, § 6).**

## 1. Observation (user, hand-played, ground truth)

On arriving at Angel Falls (`M01`, map 1100, story 1.2.1) the original draws the hero "rosé":
his whole model (hair, skin, clothes) in a pink cast, a halo above his head and small white
wings; a normal villager next to him keeps his colours. He is opaque, not translucent. In our
port the opening scene with Aquila (the flight, `ev21507`) already showed the same cast with
wings and halo, but the field at Angel Falls drew the hero normally.

## 2. What makes the pink (read, with addresses)

It is a **palette operation on the hero's models**, not alpha, not lighting, not an outline.

- Native 238 (`stb-natives.md`) and the field share the code `ov_017::0218fcf8` →
  `0218fd34(res, member, rgb555)`: for the member's character objects (slots from
  `0x13 + 12·member`, plus the wing object `0xff`) it re-reads each model's **original
  palette** (`Model3D::GetTEX0`) and writes a new one to VRAM (`StageMemoryToVRAM`): mode
  `Object3D+0x9e` = 0 **adds** the colour to every entry (clamped at 31 per channel), 1
  replaces it, 2 multiplies by `c/31`.
- The winged form is the task type `0x32` (`ov_017::021a3544` case `0x32` → `021bdc34`).
  Entering it (`+0x11 == 1`, set by `021bdbf0`) loads four files, sets bit **`0x40` of the
  leader's `+0x18c`**, and rebuilds his models (`0218f5a4`); at the end of the load,
  `021bdb78` stores the literal **`0x40c8`** (read at `0x021bdbbc`, loaded at `0x021bdba4`) in
  `Object3D+0x9c` and calls `0218fcf8(res, 0, 0x40c8)`. `0x40c8` as 5/5/5 is **(R 8, G 6,
  B 16)**. Leaving it (`+0x11 == 2`) clears the bit, writes `+0x9c = 0` and calls
  `0218fcf8(res, 0, 0)`: the original palettes come back. `02191f4c` re-applies `021bdb78`
  to a member whose bit `0x40` is set each time its models are rebuilt (equipment).
- **Falsification, a second source:** every script that shows the hero winged calls native 238
  with exactly this colour: `238(slot, 8, 6, 16)` in `ev21500` (11 calls), `ev21507`,
  `ev21593`, `ev21595` (`dq9re stb data/evspt_lv5/ev21507.gp2`, evidence `stb-238.txt`, `9779061b4fd8`). The
  scripts and the field use the same operation and the same constant. In our port the
  cutscenes already apply it (`Event::Tint`, `TexelOp`), which is why the Aquila scene was
  "already rosé".
- Mode: `+0x9e` is 0 (add) without a fifth argument of 238 and nothing writes it between a
  script and the field **[hypothesis]**. The result (brown hair → lavender, orange tunic →
  pink) matches the user's word, but it was not compared with an original capture of the
  field.
- The two objects `GameResources+0x4334` / `+0x4338` loaded by that task are shown or hidden
  by native 809 (`ov_001::02162174`); they are the wings and the halo **[hypothesis]**.
  `+0x433c`, `+0x4340` are loaded too, unread.

## 3. What gates it (read from the scenario data)

Trigger action **`0x97(arg)`** is `Trig_RunAction` case `0x33` (`0x02061c14`, arm9.c:80415):
`021bdbf0(task)`, `task+0x11 = arg`, queue the task. `arg` 1 enters the winged form, 2 leaves
it, 0 leaves it and drops the objects. All 76 scenario trigger files were scanned
(`dq9re triggers`, evidence `triggers-0x97.txt`, `8bf7246dc82e`); only `M01` and `X01` use it:

| Rule | Effect |
|---|---|
| `triggerM01`, rooms **1100..=1110**, 1.2..1.3, event 3 (arrival) | `0x97(1)`: winged (1100 at 1.2 first with `0x97(1) 0x77(21592)`; 1100 and 1106 repeat it) |
| `triggerM01`, room 1100, 1.3..1.3, event 11 after `21593` / `21595` | `0x97(2)` then `0x84(0,1,4,1)` and the warp to 4105 |
| `triggerX01`, rooms 4101, 4103..4107, 4109, 1.1..1.4, event 3 | `0x97(2)`: not winged in the Observatory field |
| `triggerM01`, room 1100, 2.1..2.1, event 3 | `0x97(0)` with `0x77(22590)`: gone for good |

The bit lives on the leader's object, so it persists across the maps of the stage (houses
1104, 1106, the village) and is cleared by the first Observatory arrival or the 2.1 arrival.
Ranges are `chapter·1000 + step` inclusive (`Trigger::loaded`). So the form is **on in rooms
1100..=1110 from 1.2.1 to the end of 1.3** and nowhere else in the field. It is the story's
"invisible to the mortals" state; how the villagers react to it is not in this note
(**[unknown]**: not read; their talk rules use no `0x97`).

`dq9_core::state::hero_winged(room, chapter, step)` is that rule; a unit test pins its
boundaries, and a real-ROM test checks it against every event-3 rule with `0x97` of
`triggerM01` and `triggerX01` over its whole range (`0x97(1)` ⇒ winged, `0x97(2)`/`0x97(0)` ⇒
not).

The outfit condition `0x52(13007, …)` of guardian 101 (`trigger.md`) is the equipment test
for the Celestrian costume (item 13007) and is **unrelated** to this state: the costume is
worn from character creation, the form comes from `0x97(1)`.

## 4. Implementation

- `dq9_core::state`: `WINGED_TINT = [8, 6, 16]`, `hero_winged`.
- `dq9-app::app::draw`: `App::sync_winged_tint`, called every frame before the upload: when the
  gate holds and no event is playing, the hero's materials get the palette operation
  (`TexelOp::new(0, WINGED_TINT)` → `TexelTint`, the mechanism of native 238); otherwise his own
  colours. Draw-only: no game-logic state reads it, so headless, pilot, parity and the route
  digest are untouched.
- Test `app::draw::tests::the_hero_arrives_at_angel_falls_winged_and_is_normal_before_and_after`
  (real ROM): at 1100 and 1.2.1 the hero's materials carry the tint `(8, 6, 16)/31`; at 1.3 too;
  at 1.4 and at 1.1 they carry none.
- Capture: `build/evidence/j4-mortal-veil/hero-normal-vs-winged.png` (`0856182a7f92`; left: the hero as drawn
  before; right: with the operation). The `--screenshot` CLI path (`native.rs::screenshot`)
  does not call `sync_winged_tint`, so this image was made with a local, uncommitted
  one-line `tint_hero` in that function; the window and the app tests use the real path.

## 5. Negative results (kept)

The first pass looked for an alpha veil and found none, which stands: the stage-1.2 scripts
(`ev01310..01390`, `ev01610`, `evspt 21500, 21507, 21510, 21591..21597`) never call native 219
or 220 (set / fade actor alpha); 570/571 only toggle slot visibility and the shadow bit.
The three map-entry writers `ov_017::0219bfb4`, `0219d250`, `0219dac4` set the party's inherited
alpha to `0x1f` (leader) or 0; `ov_017::021d2c78` is wireless command `0x24` (table at
`0x021d8028`, next to `0x23`/`0x25`). The hero is opaque, as the user's capture shows.

## 6. Still open

- **Wings.** The original shows wings in the field; the port draws only the halo (the outfit
  item 12805, always, including outside 1.2–1.3: whether the original draws it outside the
  form is **[unknown]**). The wings are the rig `d_hane` (`chara_pd.gp2`, 21 bones) loaded by
  `021bdc34` (`+0x4334`), animated with the motions at `0x021be094` / `0x021be098`. Drawing
  them is in `crates/dq9-render/src/hero.rs` (another batch) and was not touched.
- The original's field frame at map 1100, to confirm the mode (`+0x9e`), the exact pink and
  the halo; a state read with `dq9re state --peek` at the leader's object (`+0x18c` bit 6,
  `+0x9c`, `+0x9e`).
- During an event the script's own tint rules; whether the field's form is kept under a
  cutscene that does not call 238 (`ev21592`) was not established.
- `--screenshot` (`native.rs`) should call `sync_winged_tint` (needs the story point and the map
  in `prepare`); it is outside this batch's files.
