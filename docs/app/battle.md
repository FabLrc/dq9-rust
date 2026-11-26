# Battle

A battle is **played**: the player picks the commands, the engine (`dq9_core::battle`)
resolves every round, the scene shows its fighters, and the messages of the game
(`dq9_formats::battle::text`, `re/battle-text.md`) say what happens. A victory pays the party
(`Party::award`: experience, gold, drops, level-up lines), a defeat is a wipe-out (trigger 16,
the purse halved, the party wakes in a church), a flight returns to the field.

**Field encounters.** Native and Web display the map's roaming monsters with their own
`<code>_f.mon` models and stand/walk/run/appear motions. `app/roaming.rs` steps the existing
core rules at 30 Hz, on the actual collision world; equipment contributes to party strength
and deftness, preset companies come from `eventbattle.bin`, and the field and battles retain
one continuous generator stream. Natural contact calls `start_encounter`; a won fight releases
the fought model at once (`Roamers::settle`, the original's reset + hide at `ov_000::021688dc`)
and its id leaves the four-id recent ring; a flight leaves the model standing where it was, in
state 9 (`FOUGHT`, which the contact check skips) and drops the id too. The ring's writer (field
message 61) and remover (message 83) are read from the code; the run never captured a battle's
exit, so that step is the code's reading (`re/encounter.md` § (d2), "Not modelled / open").
Field-menu/facility screens are still absent. Headless parity runs keep roaming disabled.

Native window evidence (`build/evidence/j4-roaming-play/`): natural F01 contact → victory
in 3 rounds (3 experience, 5 gold) → same field with no ghost model,
`field-restored-after-win.png` (`61a585913e52`), `smoke-win.log` (`0ec1f1917e68`);
`hexagon-field.png` (`fceee50be300`) shows field monsters on `D01M02`. ROM regressions cover
`D01M02`–`D01M04` and encounter-free `S01M01`/`D01M01`/`D01M05`.
Standalone `--battle`/`?battle=` previews now run once; `Esc` cancels battle commands.

```powershell
# The Hexagoon's set battle, played by hand (command menu, messages, defeat → trigger 16):
cargo run -p dq9-app --release -- --battle 2
# Three slimes on the field stage, to a victory with experience and gold messages:
cargo run -p dq9-app --release -- --battle-stage 30116 --monsters 1x3
# A new game whose battle 26 is played by the engine (an automatic policy attacks the first
# standing monster), the world's generators seeded with 7:
cargo run -p dq9-app --release -- --new-game --fight --seed 7 --headless --auto --pilot
# A capture of the windows: play with the confirm key held until the phase shows (`menu`,
# `damage`, `levelup`, `victory` or `lost`):
cargo run -p dq9-app --release -- --battle-stage 30116 --monsters 1x3 --fight --seed 7 `
    --battle-capture damage --screenshot build/evidence/j4-battle-play/damage.png
```

**Code.** `crates/dq9-app/src/battle/` (the scene is `battle.rs`): `data.rs` (the ROM tables
of a fight, decompressed once), `fight.rs` (`Fight`, the state machine: opening, command
entry, rounds, end; no scene, no GPU), `narrate.rs` (events → the game's messages and scene
cues), `ui.rs` (the windows as quads of the two DS screens), `tests.rs`. `app/fight.rs` is
the application's side: the party, the world's generators, the window's pad, the outcome's
effect on the story and the wipe-out.

**What is played.**

- *Entry.* `App::run_battle` (action `0x78`) and `App::start_encounter(groups, stage,
  opening)` (called by the field's roaming-monster contact) resolve the
  request, run the transition and build the scene (§ [Battle scene](battle-scene.md)), then the `Fight`:
  the party is `Party` (new game: the hero, level tables applied at the first fight; a save:
  its party), each member's `Character::fighter` from `ItemStats` and `itembtlprm.nat`; a
  scripted battle (`eventbattle.bin`) is a **set battle** (the table's HP, no flight), an
  explicit stage or an encounter draws the monsters' HP on the world's generator and allows
  flight. `Surprise` is the request's.
- *Generators.* The world's `Random` (monsters' HP, flight) and the C library's `CRand`
  (drops) are kept by the application across battles; the battle owns its own `Random`,
  seeded from the world's. `--seed N` (the Web's `?seed=N`) seeds them, else the game's clock
  and the fixed-step count at the first battle.
- *Opening.* `strbtl` 6 for each kind of monster ("Some slimes draw near!", "A cruelcumber
  draws near!"), 8 for a boss, then 14 (monsters strike first) or 11 (the party has the
  jump). Each window closes itself after 40 steps; a key shortens it.
- *Commands.* Per member, in the bottom screen: **Fight** (`str_btl` 30000) → Attack, Spells,
  Defend, Items (30004–30008); **Flee** (30001); Examine and Misc. are shown greyed. Attack,
  a single-target spell and an item open the target list (the monsters standing, or the
  party for an action aimed at its own side; the scene marks the monster under the cursor
  with the wait arrow). Spells are those the vocation has learned at the member's level in
  `spelltable.bin` (the Minstrel learns Heal at 3: a level 1 hero has none, so Spells is
  greyed). **Defend** is `Command::Defend` (no target); **Items** lists the acting member's
  held items with a supported battle action, one row per held slot (duplicates included).
  The party bag is unavailable in combat. Items is greyed when no held action is enabled;
  Flee is greyed in a set battle (`BattleError::FleeForbidden`).
- *Round.* `Battle::round`; its events become windows (`narrate.rs`): each action says its
  opening line (`actmsg`, by the record's `+0x20` bits 0–9 / 10–19 for a party / monster
  target), the critical line (`+0x28` bits 20–29), what it did (`+0x20` bits 20–29 at a
  party member, `+0x24` bits 0–9 at a monster, the failure lines `+0x24` bits 10–19 / 20–29
  on a dodge or a block) and the kill line (`+0x28` bits 0–9 / 10–19). The status window
  follows the HP as the windows come; damage and healing numbers float over the fighter at
  `fighter_screen_pos`. ROM action scripts select the attack and damage motions; death and
  flight hide the model after their one-shot motion. Windows wait for the loaded presentation.
- *End.* Victory: `str_bres` 1 (50 for several monsters), 25 (26 for several members), a
  level line (10) and the skill points (13) for each level gained, gold (16) and a treasure
  chest (17, 18) for each drop; every window waits for a key (wiki *Battle-Stages*). The
  drops roll on `CRand` (`Battle::finish`), the award goes to `Party::award`, each drop to
  the trigger state (`State::obtain`, which raises the item-obtained flag, so a drop of the
  medicinal herb raises flag 4135). Defeat: `str_bres` 20, then **the wipe-out**
  (below). Flight: `actmsg` 45 (`str_bres` 24 when the enemy block the way, the fight goes
  on); the field comes back.
- *Defend, items, backup, monsters' flight.* `Event::Defended` says action 3's opening line
  (`actmsg` 10, "Hero defends."). `Event::ItemUsed` precedes the item's own action: its
  opening line (`actmsg` 70, "Hero uses a medicinal herb.") names the item the member chose
  and its result line follows ("Hero's wounds are healed."). Only the selected held slot is
  consumed, and only on execution of an item whose record sets `+0x08` bit 19 (used up).
  Cancelling or falling before the turn spends nothing; reusable items such as the sage's
  stone remain. The bag is unchanged. Slot compaction and the flag's evidence are in
  `re/battle.md`, "Defend, guard level and items". A call for backup
  (action kind 12) says its opening, then the record's line: "A bag o' laughs shows up." when
  the engine added the monster (`Event::Summoned`, the scene gets its model through
  `BattleScene::add_monster` and the monsters take their places again), "But nobody shows up."
  otherwise. A monster that flees (Flee `0xE1`, `0x395`: `Event::Acted` on itself) says its
  record's opening ("The slime flees.") and is removed from the scene; it pays nothing
  (`Battle::has_fled`, no kill).
- *Fullheal.* The core kind-2 handler applies the original max-HP override after the normal
  amount calculations and RNG draws (`ov_024::021db3a4`), not physical-strength-based healing.
  A ROM-backed `Battle::round` smoke restored HP 37 → 321 and spent MP 40 → 16
  (`build/evidence/j4-full-heal/smoke.log`, `2d4fc32e2122`); no interactive spell-menu claim.
  Omniheal's MP128 policy remains unsupported. Details and regression:
  [`re/battle.md`](../re/battle.md#final-damage-ov_024021e6a90-eu-decompilation-read).
- *Back.* The screens go to black, the field is restored (§ [Battle scene](battle-scene.md)), trigger event 15
  (won) or 16 (lost) of the scripted battle fires, a flight fires none.
- **Wipe-out** (`func_02010604`, wiki *Travel* "A wipe-out"): the fallen members get up with
  full HP and MP, the purse is **halved rounding down** (the bank is spared), and the party
  comes round on the revival map at its start point: **1106** (Angel Falls' church) after a
  new game. Its church then says its priest's line before the party gets control:
  `chur_messet.bin` picks the voice for the map and story major (`str_ch<voice − 1>` message
  **1082**; voice 3 says nothing), shown in the church's mode-2 window and closed with a key.
  **No line says the purse was halved**: `func_02010604` only halves it. The trigger 16 of
  the battle runs **before** (the Hexagoon's, `D01` room 7105, sets track flag 4). Not
  modelled: the revival map a save carries (a church saved in, Stornway's; v0.3), a set
  battle's own revival map (trigger action 180, the Magmaroo and Gortress only), the jingle
  `ME_009` (the battle's state-10 presentation has no step here). A wipe-out during a
  running scene (an event queued by trigger 16) leaves the party on the map.

**Controls** (as the conversation window): `Enter`, `Space` or `Z` confirm and close the
window; `Backspace` or `X` cancel (back one menu, or to the previous member); the arrow keys
or `W`/`A`/`S`/`D` move the cursor (left and up go back, right and down forward); `M` mute,
`R` layout; `Esc` cancels command menus and still closes the application on the field. No
touch. With `--layout top` the bottom screen's windows are laid over the top screen.

**Headless and parity.** The recorded checkpoints and traces pin battle 26's "won at
once" timeline, so a headless run, `--replay` and the application's tests keep the stand-in
unless `--fight` is given; the window and the Web always fight. `--fight` plays the battle in
the same fixed step (no step is spent on a presentation) with the automatic policy (Attack
the first standing monster, as holding confirm through the menus does: a test asserts the
two play the same fight) and the world's generators on `--seed`. The parity references replay
the same inputs with `--new-game --fight --seed 7 --mute`, so battle 26 is the engine's:
"Won after 4 rounds, 2 experience, 4 gold, drops []" on the European ROM, in the same fixed
step (the run keeps its 615 checkpoints, byte for byte). The comparison reads the story,
flags, quests and the hero, not the party, the rewards or the fight's log. Seed 7 draws no
drop, so the female original's herb flag `4135` stays an `accepted` deviation
(`docs/re/tools.md`) and the runtime has none; a seed that draws the herb would set `4135` in
the runtime's flags and fail the male reference, which is why the seed is part of the
reference.

**Recording a reference.** The inputs are the pilot's own closed-loop route, and the originals
are real-game checkpoints aligned by story point (`scripts/compare-runs.ps1`), never a
frame-synced replay of our trace: the runtime's fixed steps and melonDS's VBlanks do not
share a base (1.1.1 at the reference's step 12625 against VBlank 23033; 1.1.2 at 20068 against
58392; the current trace has them at 13,785 and 23,008 at the original message pacing).
Everything below lives under `build/` (ignored); the file and its schema are documented in
`docs/re/tools.md`.

```powershell
# 1. Inputs: the pilot's own closed-loop trace. The current references pin the 60,000-step
#    pilot trace (inputs/pilot-inputs.jsonl, sha256 5bb6bc99b049..., batch j4-parity-regress)
#    and replay it as is; a fresh run records its own file, which the reference then pins by
#    sha256. The pilot re-walks the route, so a timeline change (a fade, a message tick) does
#    not leave the replay standing at a post.
cargo run -p dq9-app --release -- --new-game --fight --seed 7 --headless --auto --pilot `
  --headless-steps 60000 --trace-inputs build/route.jsonl --checkpoints build/route-checkpoints.jsonl

# 2. Runtime side: the same inputs with battle 26 played by the engine and seeded.
cargo run -p dq9-app --release -- --new-game --fight --seed 7 --mute --headless `
  --replay build/route.jsonl --checkpoints build/parity/run1-checkpoints.jsonl
#    The log (stderr) must say: battle 26: Won after 4 rounds, 2 experience, 4 gold, drops [].

# 3. Original 1.1.1 (male, first control): libretro run to VBlank 23033, then dq9re.
#    The harness and its frame-exact inputs are documented in docs/re/melonds-state.md.
python scripts/melondsds_host.py --rom <ROM> --core tools/build/melondsds_libretro.dll `
  --input build/oracle-host/inputs/in12.jsonl --frames 23050 `
  --dump-ram build/oracle-host/ram_male.bin --state-out build/oracle-host/core_male.bin
cargo run -p dq9re --release -- state --ram build/oracle-host/ram_male.bin --json
#    Add "kind":"control" to the object (the original does not expose a checkpoint kind).

# 4. Original 1.1.2: 90 frames without input from the continuous run's state, the checkpoint
#    captured at frame 89 by build/v01-20261107/oracle/control112.jsonl.
python scripts/melondsds_host.py --rom <ROM> --core tools/build/melondsds_libretro.dll `
  --state-in build/v01-20261107/oracle/continuous-state.bin `
  --input build/v01-20261107/oracle/control112.jsonl --frames 90 `
  --checkpoints build/control112-cp.jsonl --dump-ram build/control112-ram.bin `
  --state-out build/control112-state.bin
#    Then rename that checkpoint's "kind":"control112" to "control"; nothing else changes.

# 5. Compare every reference: exit 0 with 2/2.
powershell -ExecutionPolicy Bypass -File scripts/parity.ps1
```

**Open and [hypothesis].**

- Layout of the windows is not read from the overlay: the message window is the field's size
  (252 × 70) put at the top of the top screen (the field's own, `y = 0x74`, would sit on the
  fighters); the status window (`6, 6`, 244 wide, one 22-pixel row a member: name, `HP`,
  `MP`), the command menu (`6`, below it, 116 wide, 20-pixel rows) and the target list (`128`,
  122 wide) are laid out here. The colours are the conversation window's.
- Window timing: 40 steps per window (the opening's `<TIME=45>` and the flight's 30 are the
  game's), a key closes it after 10; results wait for a key (wiki *Battle-Stages*). The scene's
  actual loaded action/reaction/death/flight motion is also a barrier: the window and pad
  wait, so a damage number, next actor or result cannot cut the presentation. Which lines
  go in one window (here: one a line) and multi-target line ordering remain [hypothesis].
- Party names carry no article (`grammar` 0): "Hero attacks."; `IF_SOLO` makes the victory
  lines read "Hero obtains 8 gold coins!" and the wipe-out "Hero is wiped out!".
- Treasure: one chest pair (17, 18) per drop, the monster named is the first fallen one whose
  record holds the item.
- Several kinds of monsters: one `strbtl` 6 line each, not message 7 (two sentences).
- **Rules the engine still refuses.** `Battle::round` returns `BattleError::Unsupported` for
  the whole round and leaves the battle unchanged (an AI mode, a targeting handler…): the
  fight then ends as a flight with a logged warning, since the same round would fail again.
  Over twenty seeds, every chapter 2 monster of `re/battle.md` plays to its end (Flee, Buff,
  Accelerate, Dazzle, Weird Dance, a call for backup), so none stops today. The menus ask the
  engine first: a spell or an item whose action `validate` refuses is greyed.
- Not played: abilities, Examine, Misc., the guard's effect on who monsters pick, party
  members beyond the hero on the scene, the full action-script camera/effects interpreter
  (the opening's wide shot stays), fanfares and battle sound effects. Worn equipment with a use
  effect is offered in Items and never spent (`0b5ceda`). Lunge interpolation/return timing and the window grouping
  remain [hypothesis]; playing the ROM motions does not certify the full presentation.
- Held-item evidence (`j4-battle-held-items`): offscreen rasterization of the real UI quads,
  ROM atlas and font, not a native-window or browser interaction:
  `build/evidence/j4-battle-held-items/item-menu.png` (`9e2260ec767e`, HP 5/20) and
  `item-used.png` (`f4baed36c8c8`, healing). At capture, HP increased, the held herb was gone,
  and the bag retained its two herbs. Regression tests cover a selected second duplicate,
  cancellation, a skipped fallen actor and a reusable sage's stone.
- Motion evidence: native ROM captures `j4-battle-motions/battle26-swing.png`
  (`780cc8be2a0a`) and `hexagoon-swing.png` (`1d5f145219cd`), actual local browser battle 26
  on seed 7 `j4-battle-gpu-sync/web-after-lunge.png` (`c461e1d32686`) and
  `web-after-victory.png` (`c39b70b7ba1a`): hero lunges, returns, and defeated/fled monsters
  disappear; victory remains 4 rounds, 2 XP, 4 gold. Extra confirmation and stacked → wide
  retain the finished preview without restarting. Console: `web-after-console.json`
  (`e616bcfb24af`). The live GPU world upload regression failed before the fix (300 pixels)
  and now passes lunge/hide readbacks; full ROM gate 5/5, 109.4 s, app 103/103.

**Evidence** (local, `build/evidence/j4-battle-play/`, sha256 prefixes): `fight-run.log`
(`743b4a58e0bd`, `--new-game --fight --seed 7 --headless --auto --pilot`: battle 26 won in 4
rounds, 2 experience, 4 gold, no drop, the slime and the cruelcumber that had not fallen
fleeing; the party line and every window of the fight are in the report), `menu.png`
(`50124557cf7c`), `damage.png` (`dd23ba975c12`), `victory.png` (`d00681832a18`),
`hexagoon-lost.png` (`841dac2ee401`, `--battle 2 --seed 3`), `parity.log` (`6bffa8a68372`: 2
of 2, unchanged), `events-report-2.1-1.txt` (`0475ab6cf0dc`: 47 of 50 complete, unchanged),
`verify.log` (`aa44613207e9`). Current tests: `battle/tests.rs` (menus against the automatic
policy, cancel, flight by seed, a lost Hexagoon, level-up, Defend, held-item use and bag-item
refusal, fleeing monsters, a call for backup) and `app/tests.rs` (battle
26 fought and paid, the lost Hexagoon's trigger 16 and wake-up in map 1106, an encounter, the
stand-in untouched, a monster joining the scene, the window's path driven by the pad).
The original `j4-battle-play` evidence drove phases without a window; newer native interactive
checks are recorded under Field encounters, and Web checks under Verification.
