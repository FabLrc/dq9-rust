# Running natively

```powershell
cargo run -p dq9-app --release -- --map M01            # ROM: --rom, DQ9_ROM, or the single *.nds in rom/
cargo run -p dq9-app --release -- --map C01M09 --time morning --layout side
cargo run -p dq9-app --release -- --map F01 --clock 389 --screenshot build/scratch/app/f01-dusk.png
cargo run -p dq9-app --release -- --save rom/dq9.sav   # map, position, heading and time from the save
# One offscreen PNG frame, no window, then exit:
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/m01.png
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/village.png `
    --size 1024x768 --eye=-5,20,40 --target=-20,0,5
cargo run -p dq9-app --release -- --map M01 --frame 60 --screenshot build/scratch/app/m01-f60.png
cargo run -p dq9-app --release -- --map C01M09 --chapter 19 --step 2 --substep 1 --night
# No window: walk following a script (world axes, seconds) and log maps
# and positions, doors included (enter a house in Angel Falls and leave it):
cargo run -p dq9-app --release -- --map M01 --start=-19.5,-9 --walk "-z:3,+z:3,idle:0.5"
# A screenshot after inputs: walk then talk to the NPC in front ("Conversations"):
cargo run -p dq9-app --release -- --map M01 --step 2 --start=5.4,2.5 `
    --input "+x:0.05,a,idle:1.2" --eye=4,3,14 --target=6,-2.5,2.5 --layout top `
    --screenshot build/scratch/app/hugo.png
```

With `--screenshot`, `--walk` and `--start` drive the hero during the `--frame N` fixed steps
(the script runs from step 0, then the hero stays still): the screenshot shows the gait
chosen from its speed, for example walking west from Angel Falls:

```powershell
cargo run -p dq9-app --release -- --map M01 --walk "-x:5" --start=-1.74,1.22 --frame 20 `
    --eye=-1.5,0,-3 --target=-3,-0.6,1.2 --screenshot build/scratch/app/hero-walk.png
cargo run -p dq9-app --release -- --map M01 --frame 10 `
    --eye=-1.74,0.3,-3.5 --target=-1.74,-0.5,1.22 --screenshot build/scratch/app/hero.png
```

`--walk` plays the **same fixed step as the window** (`App::run_steps`: `Hero::step` with the
D-pad keys the direction asks for, camera at yaw 0, then `Field::step`, doors and
field requests): `+x`, `-x`, `+z`, `-z` (two axes make a diagonal: `+x-z`) or
`idle`, a colon and a duration; `--start x,z` puts the hero on the ground at that point (also with
`--screenshot`, facing +z). Positions and segments are written to standard output;
map loads and door openings to standard error. If the map does not
load, the command fails instead of succeeding with "no map". An internal door
opens as in game, for example the double door of the library in `X01M07`:

```powershell
cargo run -p dq9-app --release -- --map X01M07 --start -8.6,-6 --walk=-z:2 --mute
# door 0/3 opens
# door 0/2 opens
# after step 0: map 4107 X01M07 hero (-9.307, 0.168, -9.381) yaw -0.000
```

## Conversations

In the window: `Enter`, `Space` or `Z` (A button) talks to the NPC in front of the hero (the most
"looked at" within 1.5 units, or inside its conversation zone: `dq9_core::talk::nearest`),
advances the text (while typing, shows up to the next wait) and closes at the
end; `Backspace` or `X` (B button) answers No; `↑`/`↓` or `W`/`S` move the cursor of
questions. The hero does not move during a conversation. The line is the one the game
chooses (`dialogue.md`) for the map, chapter, step, quests and time of day of the
application state, in English. On the Web the loaded ROM is enough: same keys.

`--input "SCRIPT"` (with `--screenshot`) plays inputs before the screenshot, with the same fixed
steps: `--walk` segments (`+x:0.5`, `idle:2`) and the buttons `a` (action: talk,
advance), `b` (cancel), `up`, `down`, one step each. The hero stays at its last position
(the orange marker in the screenshot); a script that changes map is an error. Example: the
Hugo screenshot above (step 2 of Angel Falls, `<N_TURN>//Hugo// …`).

The drawing of the frame, name, text and arrow comes from `dq9_render::ui`; the
layout constants are those of `MsgWin_Show` where they have been read, otherwise
hypotheses (`dialogue.md`, § "Message window"). The window is drawn on the field's screen
(the top one by default, the bottom one with `--screens original` or after a cutscene's 109),
within the area it occupies (`screens.rs`).

`--map`: code (`M01`, `C01M09`) or id (`1100`) from the map list; it takes precedence over the
map read from `--save`. Without either option, the map stays `M01`. `--save` reads a
local `.sav` and selects the map of the first valid normal slot; no data is sent
over the network. The hero starts at the position and heading the game gives after "Continue" (the
map's start point, `re/save.md` § "Position on resume"), placed on the ground; the
`--screenshot` output prints this pose (`hero: …`) and centers the camera on it.

`--save-out <file>` (needs `--save <file>`) writes the session's state into the save's current
normal slot at the end of any run with an application (`--headless-steps`, `--auto`, `--replay`,
`--walk`, `--play-record`, the window): the whole container goes through a temp file and a rename,
the input is never modified, and the option refuses to run without an input save or with an output
naming the input. What the state does not model (the record's base attributes, current HP/MP, the
opaque blocks) stays byte for byte; the written container is one melonDS loads and continues from
(`re/save.md`, § "Writing a session"; evidence `build/evidence/v3-save-bind/`).

`--suspend-out <file>` (needs `--save <file>`) names the destination of the field menu's **Misc.
→ Quick Save**: the session's state into the save's **suspend** slot (slot 1), with the
intermission header byte and the leader's current position and heading in the `Warp` block,
sealed and written through a temporary file and a rename. Slot 0 is never written, so the
normal adventure log the next "Continue" reads stays as it was. The destination is the `--save`
container **itself** when the option is absent, the way the DS writes its own cartridge; without
`--save` the entry says it has no file and writes nothing. It is a destination, not an
end-of-run write: nothing happens unless the entry is answered "Yes". Resuming it is the ROM
reader's rule (`GameState::from_slot` of slot 1 gives the saved map, story, party, gold and the
`Warp` pose); `--save` itself still resumes the first valid **normal** slot, so a quick save is
not yet a start-up path (`re/save.md`, § "The suspend slot").

`--time`: time of
day `night`, `morning`, `day`, `evening` or 0 to 3 (`LightingManager+0x98`: selection of
variants, of visible instances and of the lighting entry), by default the phase of the save's
clock, otherwise night; `--clock S`: second of the 420 s day (night 0, morning 180, day 210,
evening 390); its phase replaces `--time`, and the last two seconds of a phase blend
into the next one in an advanced zone (field); `--fog OFFSET,SHIFT,COLOR,D0..D31` replaces the
fog (comparisons with melonDS);
`--layout enhanced|enhanced-reactive|stacked|side|top`: the presentation ([DS screens](screens.md)); `enhanced` (default) is the single 16:9 field with the enhanced renderer and HUD,
`enhanced-reactive` the same field filling the window (experimental). `--fx LIST`/`--no-fx
LIST` switch the enhanced effects one by one ([Enhanced 16:9](screens.md#enhanced-169)). `--screens
port|original`: where the field's 3D view and its message window are drawn, `port` (default)
on the top screen or `original` on the bottom one ([DS screens](screens.md)). `--text-speed
slow|normal|fast` (Web `?textspeed=`): how fast dialogue text types; `normal`, the default, is the
original's rate of one message unit per 60 Hz frame (`re/dialogue.md`, § "Text speed"), `slow` is
half and `fast` double. A does not skip the typing, as in the original.

The cast follows the save's current scenario track when one is loaded:
progress, flags and quests are restored. Otherwise, it is that of the prologue `1.1.1` by day.
`--chapter`, `--step`, `--substep` explicitly replace the corresponding fields;
`--night` forces night placements and conversations, independently of `--time`.
With `--map`, the save provides neither the position nor the progress. The rest of the party and
the bag are not loaded. `--frame N` evaluates exactly N fixed steps from the start of
motions before the screenshot (0 by default), without depending on GPU load time.
Sprites are oriented toward the `--eye`/`--target` camera before this evaluation.

Controls: drag with the left button to rotate, wheel to zoom. In orbit mode,
`ZQSD`/`WASD` (physical keys) or arrows are the game's D-pad: the hero turns, accelerates and
walks at 5.76 units/s like `Character_Update`, blocked by walls and placed on the ground of the
map ([`re/movement.md`](../re/movement.md)); `Q`/`E` (physical `A`/`E` on AZERTY) are the
game's `L`/`R` buttons: they rotate the camera by at most ± 45° around its yaw on entering
the map, together they reset it. `Tab` switches to free flight, where the movement keys
move the camera and `Q`/`E` move the target down/up; `Shift` speeds up the camera. `F` restores
the terrain orbit around the hero (distance 11, relative height 6, target at mid-height), or
frames the whole map in free flight. `R` changes the presentation (`enhanced`,
`enhanced-reactive`, `stacked`, `side`, `top`); `T`
next time of day ("Time" list on the Web), `M` mutes/unmutes the music,
`PageUp`/`PageDown` previous/next map, `Enter`/`Space` talk, `Esc` quit on the field
(cancel while a fight is in play).

**Sound**: on each map load, `dq9-app` plays the `bgm.sdat` sequence the game
chooses for it (`re/sdat.md`: music slot of `maplist9.bin`, night variant, overrides from
`mapbgm.bin`), through `cpal` natively and through WebAudio on the Web (the browser only allows sound
after a gesture: the music starts on the first click or first key press). Event
scripts play their effects (natives 712 to 738), jingles (`ME_`, the music ducks then
resumes) and music changes; the message window plays the typing beep, `<EXC>`,
`<QES>` and the opening of choices. `--mute` (`?mute=1` on the Web) opens no audio
device; `dq9re sseq <n> --wav f.wav` renders a sequence, `dq9re se <n> --wav f.wav` an effect
without a window; `--event N --headless-steps S --sound-wav f.wav` writes the mixed sound of an event
(the log lists the effects requested by the scripts).
