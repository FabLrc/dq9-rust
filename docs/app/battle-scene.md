# Battle scene

A battle is fought on a **stage**: an ordinary map of the map list (`B01M16`, "F01 - Field",
id 30116; `B02M15`, "D01 - Hexagoon", 30215…), assembled and lit like a field. The fight is
centred on the stage's origin, the party on `+z` facing π and the monsters on `−z` facing 0,
and starts everyone on the grid (`ov_000::02164d74`). `crates/dq9-app/src/battle.rs`
(`BattleScene`) builds it, `app.rs` shows it through the transition and gives the field back
afterwards. Facts from the wiki *Battle-Stages* and *Monsters*, rechecked on our ROM where the
tests could.

```powershell
# The Hexagoon's set battle (eventbattle.bin index 2: monster 300, stage 30215).
cargo run -p dq9-app --release -- --battle 2 --screenshot build/evidence/j4-battle-scene/hexagoon.png
# Three slimes on the field stage 30116.
cargo run -p dq9-app --release -- --battle-stage 30116 --monsters 1x3 `
    --screenshot build/evidence/j4-battle-scene/slimes-f01.png
# In the window: --battle 2, or ?battle=2 on the Web (`?battle-stage=30116&monsters=1x3`).
```

- **Stage.** `BattleScene::build` loads it with the field loader (map archives, textures, its
  own `.bats`), then keeps only the scene: the stage has no cast, no collision and no triggers
  worth stepping. A stage the record does not name, or id 0, is 30116, as
  `func_ov000_02166880` falls back. `--battle N` reads the record's stage, monsters and track
  (`dq9_core::scenario::battle`); `--battle-stage` takes an explicit stage and `--monsters`
  `number[xcount]` pairs.
- **Lighting.** The stage's `.bats` slot is taken **once**, at entry, from the day clock
  (`LightingManager+0x98` at the request, wiki *Battle-Stages*): the environment is computed
  once and never blended. The slot the record carries is not in `eventbattle.bin` (**[unknown]**).
- **Grid.** Slot *s* → column *s* mod 9, row *s* div 9; x = 2.598 × column − 10.392 (+1.299 on
  an odd row), z = 2.25 × row − 9 (`ov_000::0216f74c`). The slots by side size come from the
  tables at `0x02183108` (party: four byte rows of four) and `0x02183118` (monsters: eight byte
  rows of eight): one hero → 58 (0, 4.5), three monsters → 29, 22, 32 (−3.897, −2.25),
  (0, −4.5), (3.897, −2.25). Every fighter stands `0xcc` (0.05) above the stage. **The tables
  stop at four and eight** (`func_ov000::0215e9fc`/`0215eb1c` cap their counts at 4 and 8, and
  `func_ov000::02167cd4`/`021676a8` index `base + (count − 1) × size`): a ninth monster has no
  grid place and no row place, and the port builds the eight the battle's slots hold
  (`--battle-stage … --monsters 1x9` logs it).
- **Formation.** A battle starts on the grid (`ov_000::02164d74` places everyone on it and sets
  the flag to 2, `ov_000::02167dd8`); a few action scripts ask for the row
  (`ov_000::02167e6c` sets it to 1), so `set_formation(Formation::Row)` asks for it. The row's
  places are the ROM's: the party at z 2.5, 1.5 apart and centred, each turned by the table at
  `0x02183158` (`ov_000::021675a0`; one hero `0x323d` = the ROM's π, a four-hero row
  `0x3570 … 0x2f0a` = π ± 0x333/4096 = ±0.2, paired from +x down so both ends turn inward);
  the monsters at z −2.5, side by side by their **body radius** ×0.7 for the five wide
  monster numbers of `ov_000::021681f8` (`0xbd`, `0xbe`, `0xbf`, `0x110`, `0x155` — the
  drackals and Rover, once more than one fights) and
  by `ov_000::02167b5c`'s gap (0x199–0xb33 fx12, filled to 4.0 + one minimum gap per
  interval). The radius is the object's own, which the battle sets from `mon_data` `+0x0C` in
  1024ths (`Object3D_SetRadius(record[+0x0C] << 2)`, `func_020484f8`/`func_02048850`): a
  slime's 819 is 0.8,
  so three slimes stand at −1.5, 0 and +1.5. `func_ov000_021677fc` does the arithmetic on the
  fx12 grid — `(radius × factor + 0x800) >> 12`, the total and the gap summed, the first place
  at `−(total + gap × (n−1)) / 2`, each step `half = width / 2` — which the port mirrors in
  integers. A monster that joins recomputes the row (`ov_025::021d8c30` calls `0x021677fc`),
  and the port re-places the set and re-frames a running opening (**[hypothesis]** that a
  backup call also switches the flag to the row: `0x02167e6c` has no caller in the corpus).
  The layout is proved by `app/encounter/tests.rs::
  the_row_formation_places_one_hero_and_three_monsters` and the `battle/tests.rs` row cases
  (the table's values and pairing, the gap's arithmetic, the five numbers, the
  eight-monster limit); the row of nine is captured by
  `battle/tests.rs::the_opening_camera_and_the_nine_monster_row_are_captured`, since
  `--screenshot` cannot ask for the row (that flag lives in `native.rs`).
- **Models and motions.** The party is the hero model (`AnimatedScene::add_hero`); monsters
  load their `.cchr` bodies and separate `.cmot` motions from `<code>.mon` in
  `data/pack_lv5/enemy.gp2` (`LoadMonsterCharacter`, native 233). ROM `.bact` action sections
  select attack, magic, guard, call and escape motions; the hero takes its equipped weapon's
  `mp02NNbe` set. Damage reacts at the script's hit percentage; death/flight play once then
  hide the model. The original alpha fade and camera close-up size remain unmodelled.
  Timing, dispatch and limits: [`re/battle-data.md`](../re/battle-data.md#action-scripts-and-battle-motions).
- **Camera.** `func_ov000_0216d600(camera, side, wide, …)` frames **the side looked at**: side 1
  is the monsters (`+0x230`) from the party's `+z`; side 0 the party (`+0x228`) from `−z` (both
  `z` negated). Its extents are the **row's**, computed even when the battle stands on the grid
  (`ov_000::02164d74` calls `021677fc` before `02167cd4`): the party `0x1000 + 0x1800 ×
  (members − 1)` wide and `0x1800` deep (`ov_000::021675a0`), the monsters the sum of their
  radii plus one row gap per interval (the same `0x199`–`0xb33` as the row) and the tallest
  body + 0x800 (`ov_000::021677fc`). The party's count is forced to one for the battle types
  `func_ov000_0215fc60` answers (800/801), which the port does not carry. Half-angles 15°
  (`0x424`/`0x424 + 0xb50`) and 22° (`0x5fe`/`0xed6`), fx12 sin/cos at
  `0x0216d8f4`/`0x0216d8f8`; distance =
  max(width·cos ÷ (2.2·sin), depth·cos ÷ (1.5·sin)) − 2.5, floored at 6.5 (3.0 wide), with
  `FX_Div` and the `_ffix` of each divisor as the original does; look at (0, depth/2 at least
  1.2, 0) with the eye held at most 2 high. `func_ov000_0216f2b8` then keeps the eye at most 17
  from the stage's centre and at most 5 high.
- **Opening.** `func_ov000_0216118c(…, 1)` runs once per battle (`ov_026::021d8ba0` state 3,
  only if the camera's easing is idle): it asks the wide 22° side shot, then reads the eye's
  spherical parameters (`yaw`, `pitch`, `distance`, `func_0202eab8`) and sets the eye pulled back
  by `pitch + 0x800` and `distance + 0x3000` (`func_0202e5d8`). `func_ov000_0216f0d0` then eases
  the yaw (the short way, on a full turn of `0x6488` = 2π × 4096), the pitch and the distance
  back to that shot: each keeps `0xf33` of its difference (`(difference × 0xcd + 0x800) >> 12`,
  so 0.95 an update, `uRam02161260`) and the step snaps once every difference is under 0x28.
  The battle scene runs at **30 Hz** (`ov_017::0218b688`'s `CalculateDeltaTime` gives 2 VBlanks
  an update), so the ease moves on every second 60 Hz fixed step; the Hexagoon's opening
  settles at the 114th update (the 228th fixed step). The half-angle stays 22° throughout, and
  `ov_000::0216f2b8` clamps the eye on every step (a pulled-back eye may start beyond 17 and is
  cut to it).
- **Kind shots.** `data_ov026_021de87c` (overlay 026, 47 records of seven fx12 `int`s: kind, eye,
  target, then `−1`) gives the big monsters fixed shots: `func_ov026_021d8aac` takes the first
  record whose **kind** stands (`func_ov000_0215fc8c` compares `mon_data +0x10`, not the
  monster's number) and `func_ov000_0216d600`'s `param_8` applies it instead of the calculated
  eye and target. Read whole in `battle/shots.rs` (`0x101`…`0x133`): the Hexagoon (number 300)
  is kind `0x101`, the table's first record, eye (0, 1.060, 2.270) and target (0, 1.080, 0);
  `0x12c` is Estark's. Two details are not modelled: `0x115`'s three variants (picked by
  `battle +0x8e20` modulo 3, a battle counter) and the `kind 0` record (a battle type,
  `func_ov000_0215fc60` = 2); the first variant and no default are taken (**[hypothesis]**).
- **API**: `act(actor, action, targets, damaged)` starts the ROM-backed gesture;
  `presenting()` gates windows/input until it finishes. `kill`/`flee` hide at motion end;
  `remove` hides immediately. `upload` transfers geometry and every fighter world matrix,
  including hidden entries, to the existing GPU renderer. `set_shot` and
  `fighter_screen_pos` provide framing and damage-number positions on the 256×192 top screen.
- **Transition** (native 547, `ov_001::02163ccc`; a roaming contact through the battle task
  `ov_017::021b790c`, state 6). A scripted battle (`Request::Battle`, action `0x78`), 547 and a
  roamer's contact all run the original's swirl (`battle/transition.rs`,
  `ov_017::021b6290`/`0x0204701c`): the battle track (`eventbattle.bin` `+0x24`, else sequence
  `0x17`) plays at once, and the field is still drawn while the swirl's clock runs it — the
  camera rolls −8° and its half field of view drops by 0.4333° per 30 Hz update (15° → 7.2°
  over the 18 updates), and the swirl's disc (`data/effect/ev999991500.chr`,
  `dq9_render::swirl`) is pinned in front of that camera as the original draws it in an
  orthographic box (`ov_017::02195ecc`). Once the update counter passes 15 both screens fade
  to black over 20 steps (`SetMainBrightness(-16, 20)`); at the 38th fixed step the clock is
  done and the cover full, the scene is built on the black, `SwirlEffect_Release`
  (`0x02046f94`) gives the field camera back (roll 0, 15°) and hides the disc, and the scene
  comes up over 15 (`0x021d9d18`). The fight is played on it (§ [Battle](battle.md)), then the screens go
  to black again over 15, the field comes back and trigger event 15 or 16 fires. When no
  fight can be built, the stand-in holds the scene 90 steps and settles it as won. 547 without
  a following battle gives the field back after 110 steps.
  The phases and their ticks are `battle/entry.rs` (`BattleChange` runs them in order):
  - a **roamer's contact** freezes the picture 12 steps (its blocking load, measured) before
    the swirl's first update; a **set battle** (`Request::Battle`, number ≥ 1) spends the swirl
    task's 15 updates (30 steps) instead (`SwirlTask_Init`/`_Update`, `+0xfd`); a preview
    (`--battle`, `?battle=`) spends neither;
  - after the swirl's 38 steps the scene is built on the black, which then holds **94 frames**
    while the stage loads (measured), before the 15-step fade-in (`0x021d9d18`; the brightness
    log changes over ~13 frames, so the 15 is the chosen count, not a measured one);
  - the way back is 15 steps to black, **95 frames** black while the field reloads (measured,
    native 844's placements already restored) and 30 steps up (`SetMainBrightness(0, 30)`,
    state 5): 140 steps in all.
  Frames, the intro camera and the measurements: [`re/battle-transition.md`](../re/battle-transition.md).
  The port spends 144 steps from a contact to the first fade step against the original's 149
  (the five set-up frames before the freeze are not spent); the opening camera runs on the
  scene's own clock once it is built.
- **Headless** (`--headless`, `--auto`, `--replay`, tests): the scene is built and checked,
  then the battle is settled in the **same fixed step** (`App::last_battle` records its stage
  and fighter count). No step is spent on a presentation: the headless timeline is the
  story's, so the recorded input traces replay identically (`scripts/parity.ps1`). What
  settles it is the J4 stand-in (won) unless `--fight` plays it with the engine (§ [Battle](battle.md)).
  The window and the Web show the transition and play the fight.
- **Back to the field.** Native 844's rule is modelled by keeping the field scene aside
  (`App::save_field`): the same map, cast, doors, collision, party pose and lighting come back
  unchanged, so the placements are as the scene left them. The map is not reloaded: no entry
  trigger fires.
- **Events.** 547 no longer counts as "not executed": `EventPlayer` turns it into
  `Outcome::BattleTransition`, which the application executes. The script keeps running during
  the transition in the report playback (no field to freeze there); in the window the field
  waits for it, like the game's task order.

`--screenshot` prints each fighter's placed position and heading, and the camera it used
(`camera: eye … target … half-angle …`); `--frame N` steps the scene N fixed steps first, which
is how the opening is captured. `build/evidence/v4-battle-intro/captures/` holds it at ticks
0/60/240 of the Hexagoon's set battle (`opening-tick00-hexagoon.png` `eec1ec966add`,
`opening-tick60-hexagoon.png` `76326cdb173f`, `opening-tick240-hexagoon.png` `0630c6829616`,
its 241st step identical) and the row of nine slimes the game cannot hold
(`row-nine-monsters.png` `3ba5770d03c9`: eight places, no ninth). The camera's constants and
the whole kind table are in `build/evidence/v4-battle-intro/camera-tables.txt`
(`3533e89ea211`). Older captures
(`build/evidence/j4-battle-scene/`):
`hexagoon.png` (`dd025965b166`: the Hexagoon on `B02M15`, the hero facing it),
`slimes-f01.png` (`3e1ba9308ed1`: three slimes on `B01M16`, the middle one hidden behind the
hero), `battle26.png` (`c7ed26e23bcb`: battle 26's record, monsters 290/292/290) and
`report-2.1.txt` (`fe41a9bcc437`, `--events-report 2.1-1`: no 547 left).
Real-ROM tests in `app/tests.rs` assert the grid of a
three-slime scene and of the Hexagoon's set battle, the eye clamp, `fighter_screen_pos`, and
that scripted battle 26 enters the scene from `--new-game` and gives the same map and pose
back.
