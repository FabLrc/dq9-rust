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
  tables at `0x02183108` (party) and `0x02183118` (monsters): one hero → 58 (0, 4.5), three
  monsters → 29, 22, 32 (−3.897, −2.25), (0, −4.5), (3.897, −2.25). Every fighter stands
  `0xcc` (0.05) above the stage. A set with more than eight monsters (the tables stop at
  eight; the ROM's records ask at most eight) gets free slots after them **[hypothesis]**.
- **Formation.** A battle starts on the grid; the row (`0x02167e6c`) is only used by a few
  action scripts, so `set_formation(Formation::Row)` asks for it: party at z 2.5, 1.5 apart and
  centred; monsters at z −2.5 by their width (radius ×4, gap 0.7, narrowed as `0x02167b5c`
  does). The turn of the party's ends (±0.2) and the five narrowed monster kinds are
  **[hypothesis]**: the angle table and the kind list were not read.
- **Models and motions.** The party is the hero model (`AnimatedScene::add_hero`); monsters
  load their `.cchr` bodies and separate `.cmot` motions from `<code>.mon` in
  `data/pack_lv5/enemy.gp2` (`LoadMonsterCharacter`, native 233). ROM `.bact` action sections
  select attack, magic, guard, call and escape motions; the hero takes its equipped weapon's
  `mp02NNbe` set. Damage reacts at the script's hit percentage; death/flight play once then
  hide the model. The original alpha fade and camera close-up size remain unmodelled.
  Timing, dispatch and limits: [`re/battle-data.md`](../re/battle-data.md#action-scripts-and-battle-motions).
- **Camera.** `Shot::Opening` is the opening (`func_ov000_0216118c`): the wide side shot
  (22° half-angle), from the party's side; `Shot::Side(side)` is the 15° framing of
  `func_ov000_0216d600`. Distance is the larger of width × cos ÷ (sin × 2.2) and
  depth × cos ÷ (sin × 1.5), less 2.5 (at least 6.5, 3 wide), looking at (0, depth/2 clamped
  1.2–2, 0). `func_ov000_0216f2b8` then keeps the eye at most 17 from the target and at most 5
  high. Which sign "side 1" is, the opening's eased approach (0.95 a frame) and the fixed
  per-monster-kind shots of `data_ov026_021de87c` are **[unknown]/[hypothesis]**: the first
  is taken as the party's side.
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
  a following battle gives the field back after 110 steps. Frames, the load-bound black
  periods (94 and 95 frames) and the intro camera: [`re/battle-transition.md`](../re/battle-transition.md);
  the load blacks, the 12-frame freeze after contact and the 15-update delay of a set battle
  are not modelled.
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
(`camera: eye … target … half-angle …`). Captures (`build/evidence/j4-battle-scene/`):
`hexagoon.png` (`dd025965b166`: the Hexagoon on `B02M15`, the hero facing it),
`slimes-f01.png` (`3e1ba9308ed1`: three slimes on `B01M16`, the middle one hidden behind the
hero), `battle26.png` (`c7ed26e23bcb`: battle 26's record, monsters 290/292/290) and
`report-2.1.txt` (`fe41a9bcc437`, `--events-report 2.1-1`: no 547 left).
Real-ROM tests in `app/tests.rs` assert the grid of a
three-slime scene and of the Hexagoon's set battle, the eye clamp, `fighter_screen_pos`, and
that scripted battle 26 enters the scene from `--new-game` and gives the same map and pose
back.
