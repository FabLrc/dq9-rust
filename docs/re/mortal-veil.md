# The hero's winged (Celestrian) form in the field

Batch `j4-mortal-veil`. **Status: the visual and its gate are read from the code and the
scenario data; the hero in the field is drawn with the original's palette operation, and with
the wings and halo the original hangs on him (`j4-winged-form`, § 7). What was not compared
with an original capture is listed in § 6.**

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
- (Challenger, not re-run here.) Native 238 also carries `(8, 6, 16)` in `ev21520`, `ev21521` and
  `ev29306`; in `ev21593`/`ev21595` it targets slot 1 (the event-loaded Aquila `s019n`), not the
  hero, who keeps the field's tint. `021bc77c` (event task end) calls `MakeVisible` on the wings
  object (`021bdbc0`), so an event can hide them (native 809) and the field shows them again at
  its end.
- Mode: `+0x9e` is 0 (add) without a fifth argument of 238 and nothing writes it between a
  script and the field **[hypothesis]**. The result (brown hair → lavender, orange tunic →
  pink) matches the user's word, but it was not compared with an original capture of the
  field.
- The objects `GameResources+0x4334` / `+0x4338` loaded by that task are shown or hidden
  by native 809 (`ov_001::02162174`); `+0x433c`, `+0x4340` are loaded too, unread. Which files
  they are is read in § 7 (the wings and the halo).

## 3. What gates it (read from the scenario data)

Trigger action **`0x97(arg)`** is `Trig_RunAction` case `0x33` (`0x02061c14`, arm9.c:80415):
`021bdbf0(task)`, `task+0x11 = arg`, queue the task. `arg` 1 enters the winged form, 2 leaves
it, 0 leaves it and drops the objects. All 76 scenario trigger files were scanned
(`dq9re triggers`, evidence `triggers-0x97.txt`, `8bf7246dc82e`); only `M01` and `X01` use it:

| Rule | Effect |
|---|---|
| `triggerM01`, rooms **1100..=1110**, 1.2..1.3, event 3 (arrival) | `0x97(1)`: winged (1100 at 1.2 first with `0x97(1) 0x77(21592)`; 1100 and 1106 repeat it) |
| `triggerM01`, room 1100, 1.3..1.3, event 11 after `21593`, or after `21595` with `0x1f(1)` | `0x97(2)` then `0x84(0,1,4,1)` and the warp to 4105 |
| same, event 11 after `21595` with `0x1f(0)` | `0x68(1)`, `0x84(0,1,3,2)`, `0x8d(1)`: **no `0x97`**, the form is kept at 1.3.2 (`dq9re triggers M01 --room 1100 --story 1.3 --event 11`) |
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

- The original's field frame at map 1100, to confirm the mode (`+0x9e`), the exact pink, the
  halo and the wings' size and place; a state read with `dq9re state --peek` at the leader's
  object (`+0x18c` bit 6, `+0x9c`, `+0x9e`). **No original field capture exists in the
  repository**: the port's frames were compared with the user's description only (rose hero,
  halo above the head, two small white wings on the back, opaque), not with an image.
- During an event the script's own tint and attachments rule; whether the field's form is kept
  under a cutscene that does not call 238 or attach wings (`ev21592`) was not established.
- The hero model's own ring (outfit item 12805 of `HeroLook::new_game`, drawn always, also
  outside the form) is a **[hypothesis]** of `hero.rs`: `docs/re/party.md` reads no headgear on
  the party's leader. In the form it coincides with the halo of § 7 at rest, so one ring is
  seen; whether the original draws a ring outside the form is **[unknown]**.

## 7. The wings and the halo (`j4-winged-form`)

**What the form loads (read).** `ov_017::021bdc34` queues four files (the string table at
`0x021be080..0x021be090`, read from the overlay): `chara_sub/s023n2.chr` → `GameResources+0x4334`,
`chara_sub/s069.chr` → `+0x4338`, and `d_wing.chr` / `d_wa.chr` of `pack_lv5/chara_pd.gp2` →
`+0x433c`, `+0x4340` (an earlier note of this file and `characters.md` named the rig `d_hane`:
that was not read from this code; the two small archives are the ones above). Each is built with
`Object3D::LoadFromCCHROrCMOTArchive`. `s023n2.chr` is a model of the same size as `s023.chr`
(the wings the cutscenes hang on the hero's chest in `ev21507`) with `stand` and `run`;
`s069.chr` is the halo the same cutscenes hang on the head, with `stand`.

**How the field draws them (read).** `ov_017::0218eafc` (per party member, `param_2` = member)
draws, for member 0 only: the halo (`021bdbcc`, `+0x4338`) with the matrices of the hero's bone
slot 0, right after his equipment and before his children; then, after `DrawChildren`, the wings
(`021bdbc0`, `+0x4334`) with bone slot 2. The slots are `func_02053e10` (EU `0x02053e20`): slot
0 `head`, 1 `waist`, 2 `chest` (strings at `0x02053ea8..`, EU), read by `func_02053eb4` (EU
`0x02053ec4`). Both are drawn with `GetModelBonePositionAndDirectionMatrices(…, bone)` then
`Object3D::Draw`: the same attachment as the scripts' (`head` for the halo, `chest` for the
wings). `0218eafc` runs for the hero in the field, so the form is drawn as part of the hero,
not as a separate task. `ov_017::0218fd34` includes slot `0xff` (`+0x4334`, the wings) in the
models it recolours, not `+0x4338`: **the wings carry the pink, the halo does not**.
`0218e674` calls `Object3D::AdvanceEffects` on the wings each field update.

**Motion.** `021bdc34` sets `stand` (`0x021be094`) on the wings when the form is entered, and
`run` (`0x021be098`) on the way out (`+0x11 == 2` with the entering arg). Nothing in
`0218eafc`/`0218e674` switches it with the hero's gait. The port plays `stand` in a loop from the
model's first motion record and never changes it. Whether the object's own animation advances
each field frame in the original (so that the wings flap) is **[unknown]**: the port loops it.

**Drawn (port).** `App::sync_winged` (`app/draw.rs`) adds the two models to the scene the first
time `hero_winged` holds (as scripted actors, `AnimatedScene::add_actor`), hangs them every
frame on the hero's `chest` and `head` bones (`hero_world × hero_bone`, the matrix the cutscenes'
attachments give them), gives the wings `WINGED_TINT` like his models, and hides both when the
gate fails, a script runs (the script's own attachments are drawn: the Aquila scene is
unchanged), or the hero is hidden. The model's own scale and offset inside the attachment are the
identity **[hypothesis]**: the draw code was not followed into `GetModelBonePositionAndDirection
Matrices`' use of the object's scale (`+0x5c`).
Wings and halo are removed with the gate: `021bdc34`'s leave case zeroes `+0x4334..0x4340`, and the
port hides them (the models stay in the scene for the map's lifetime).

**Not resolved: the wings' own gate.** The port shows wings and halo exactly while `hero_winged`
holds (the tint's gate). The code leaves a doubt: `ov_017::021bdc34` also *loads* the objects for
`0x97(2)` (it only clears the bit and the palette, then plays `run` on the wings instead of
`stand`), `ov_017::0218b688` runs that task with arg 2 in one branch of the field's
initialisation (game mode 2 of `func_0200fb9c`, line ~494), `X01` arrival rules call `0x97(2)`, and
`0218eafc` draws the objects whenever they exist and are visible (`Object3D::Draw` returns when the
hidden bit `+0x6c & 1` is set or the alpha is 0; `Object3D::Initialize` clears the flags, so a
freshly loaded object is visible). Nothing read hides them under arg 2. The `run` motion is not
a hide: it poses the wings narrower (model-space width about 11 against 21 at `stand` frame 3,
`dq9re model` of `s023n2.chr` with `run.nsbca` / `stand.nsbca`, evidence `wing-extents.txt`). So
**[unknown]** whether the original draws (narrow) wings on the hero outside rooms 1100..=1110 /
1.2..1.3 (the Observatory arrival with `0x97(2)`, the first field after a new game). The user's
screenshot only shows the form at Angel Falls. The port keeps the gate the batch fixed.

**Tint, checked against the challenger's reading.** `0218fd34` recolours Chara entries 0 (hero),
`0x13`, `0x14`, `0x16..=0x1d`, `0xc8` and `+0x4334`, never `+0x4338` (halo, `s069`) nor
`+0x433c`/`+0x4340`: wings pink, halo white, as the port draws them (the halo's own colours).
Entry `0x15` is skipped by `0218fd34` and by the restore `0218f80c`; what it is **[unknown]**
(`0218eafc` draws it between the scaled `+0x14` object and the `+0x1b` one). Every material of
the port's merged hero model gets the tint, including the model's ring (item 12805, see § 6): all
nine materials of the hero's drawn model use paletted textures (probe in the app test harness,
discarded), so none is a direct-colour texture a palette add could not reach. The ring being a
`HeroLook` hypothesis, the pink on it is the port's; the original's halo is `s069` and white.

**Test.** `app::draw::tests::the_winged_hero_wears_wings_and_a_halo_only_in_his_form` (real
ROM): at 1100 and 1.2.1 and at 1.3 both are drawn, within 0.5 world units of the hero's back and
head, the wings recoloured; at 1.4 and 1.1 they are hidden (and `a_hero_outside_the_form_has_no_
wings_in_the_scene`: never loaded). Headless, pilot and parity are untouched (draw-only).

**Captures** (`--screenshot`, now with the form: `native.rs::screenshot` calls
`App::winged_in_scene`, the window's own code; no local hack): in
`build/evidence/j4-winged-form/` (`manifest.json` has the hashes): `before-1100-back-1.4.png`
(1.4: normal hero), `after-1100-back.png` and `after-1100-side.png` (1.2.1, the bridge),
`after-1100-run.png`, `house-1105-step2.png` / `-step4.png` (a house, 1.2 against 1.4).
**What still differs from the user's original screenshot: unknown** (no original field capture to
compare with): the wings' exact size and angle, the pink's exact value and the halo's height are
the port's, from the attachment mechanism above.
