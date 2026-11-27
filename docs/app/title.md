# The title screen

The application boots on the title when it starts **interactively without a start-forcing
option**; every command-line driver and the Web page keep their own paths. The rows are the
port's, ranked by the original's slot check
([`re/title.md`](../re/title.md)); the title's own graphics are not decoded, so the window
draws plain rows with the game's font (`app/title.rs`).

## When it shows

`App::shows_title` (`app/title.rs`) gates the boot in `App::set_rom`:

- the app was opened by the window (`App::new`), not a headless driver (`App::new_headless`,
  `App::new_replay`, `--walk`, `--auto`, `--replay`, `--play-record` without `--window`);
- none of `--new-game`, `--save`, `--map`, `--event`, `--battle`/`--battle-stage` is present.

`--record` also keeps today's field start: its recorder is installed right after the app is
built, and `App::update` drops the title before the first step when a recorder is present, so
no step is taken on the title. The Web build is unchanged (the module is native only): the
page's `?newgame=`, save picking and pilot paths do not show it.

## The rows

| Row | When | What it does |
|---|---|---|
| **Continue your adventure** | a container has a slot `Save::title_slot` ranks (a suspend save, else a normal log on slot 0, checksums valid) | resumes that slot through `App::begin` |
| **Create a new adventure log** | always | the character-creation screen (`App::show_creation`), then the new game |

The container is `--save`'s file, re-read from disk when the title is shown (so a quick save
written during the session is seen), else the bytes the app started with. `--suspend-out`
writes elsewhere: a quick save written there is not the cartridge the title reads, as on the
DS. Continue is first when both rows are shown, because `ov_004::0216dad0` ranks a present
suspend save above a normal log.

**Continue** takes `Save::title_slot`, not the boot order: `load::start` and `GameState::from_save`
use `Save::resume_slot` (normal log first, the `--save` boot path), while the title ranks the
suspend save first ([`re/title.md`](../re/title.md), § "The slot check"). `app/title.rs`'s
`start_from_container` builds the same `load::Start` `load::start` would — `GameState::from_slot`
(the suspend slot's warp map), `Party::from_slot`, the hero's look and message identity — and
`App::begin` loads it. After a suspend resume, slot 1's header A is emptied and the container
replaced through a temporary file and a rename (`Save_WriteHeaderA`, `0x020aba6c`), as the
`--save` boot's `consume_suspend_log` does.

**Create a new adventure log** opens the character-creation screen (`app/character.rs`,
[`character-creation.md`](character-creation.md)): the hero's name and sex, then
`App::start_created_game` (story 1.1.1, the created hero, the opening map `S14M05`, then the
prologue). `options.new_game` is set only on confirm, so the field does not start while the
name is typed. `--new-game` keeps the default hero and skips the screen, which is the path the
route and the pilots take.

## Returns to the title

- **The church's Confession (110)** — `App::end_session` (`app/facility.rs`): the original
  stays on 110 and the console is switched off; the interactive port shows the title instead,
  which is the next switch-on. A headless run keeps `session_over` and its steps, so the
  facility tests and the pilots are unchanged.
- **The field menu's Misc. → Quick Save (4702)** — once the notice closes, the interactive app
  returns to the title (`App::after_service`, the `title_pending` flag), as `ov_002::02169efc`
  does. A headless run keeps the field menu.
- **Credits**: the port has no credits, so none returns here.

## Input and drawing

Up/down (arrows or `W`/`S`) move the cursor, `Enter`/`Space`/`Z` choose a row, a tap on a row
chooses it (`App::tap` maps the window point to the 256×192 screen the title draws on),
`M` mutes and `R` cycles the layout. `Escape` closes the native window, as on the field. The
creation screen's own keys are in [`character-creation.md`](character-creation.md).

The frame is `Panel` filling the window and the port's quads on the screen the field would use
(`App::draw_placeholder`, the title's quads from `App::title_quads`): a plain background, the
heading "Dragon Quest IX", the rows with the cursor highlighted and an "Enter: choose" hint. No scene or renderer is needed, so the app
boots without a map. The original's title art is **not decoded**: this is the port's own
placeholder, not a reproduction of the DS graphics.

## Tests

`app/title/tests.rs` (real ROM, `DQ9_SAVE` for the save paths):

- an interactive start with no option shows the title, a headless one does not;
- title → New Game opens the creation screen, and its confirm loads the opening map;
- title → Continue resumes the ranked suspend slot (its warp's map and party) and deletes the
  log from the container, while the normal log keeps the boot order (`resume_slot` = slot 0);
- a Quick Save's 4702 returns to the title, which then offers Continue;
- `end_session` returns the interactive app to the title and leaves a headless run's
  `session_over` set.

The ranking itself is `dq9-formats/src/save.rs`'s test (`title_slot`: suspend above normal,
normal when no suspend, nothing when both are empty, a stale checksum not ranked).

## Limits

- The menu consumer of `ov_004::0216dad0`'s entry codes is **[unknown]**; the DS observation
  (`v3-suspend-ds`) is one row, **Load quick save data.**, plus Cancel for a suspend save, where
  the port shows Continue (which resumes that slot) and New Game.
- The quick save's separate entry is folded into Continue; the corrupted-log refusals (codes 4,
  6, 7), the post-load backup write and the adventure-log display (party, play time) are not
  modelled.
- The title's graphics, sound and the DS START button are not modelled: the title appears
  directly, without the original's "press START" frame.
- The Web build keeps its page flows; it has no title screen.
