# Character creation

The title's **Create a new adventure log** opens this screen before the prologue: the hero's
**name** and **sex**, the two fields the message window, the scene and the save consume. The
original's overlay 9 has more pages (face, hair, colours); the port keeps the two the v0.3
milestone asks for and derives the rest from the sex's defaults
([`re/characters.md`](../re/characters.md) § "Character creation").

## When it shows

`App::show_creation` (`app/character.rs`), called by the title's New Game row
(`app/title.rs::title_new_game`):

- the app was opened by the window (`App::new`) and booted on the title, i.e. without
  `--new-game`, `--save`, `--map`, `--event` or `--battle`;
- `--new-game` (headless drivers, pilots, the route) keeps the default hero and **skips** the
  screen: `app/flow.rs::start_new_game` is unchanged for that path;
- the Web build has no title and no creation screen (the module is native only).

While the screen is up, `App::update` steps nothing (as on the title) and every key goes to
`App::creation_key`.

## The screen

One page, drawn on the 256×192 screen the field would use, with the game's message font
(`App::creation_quads`): the heading "Create your hero", the name with a caret, "Sex:" with
**Man** / **Woman** and the chosen one highlighted, and the key hints. It is the port's own
window, not the original's art.

| Key | Effect |
|---|---|
| a letter (Shift for upper case), a digit, `. , ' - / ;` | appends the message-font glyph |
| `Backspace` | removes the last glyph |
| `Left` / `Right` | moves the sex |
| `Enter` | confirms (refused while the name is empty) |
| `Escape` | back to the title, nothing started |

The name is stored as **message-font glyph indices** while typing (`Font::find`), so it is the
save's own representation from the start: at most **8 glyphs** (the original's entry widget
capacity, `*(entry + 0x14) = 8` in `ov_009::02186e00`), zero-padded to `NAME_LEN` = 10 bytes at
`+0x140`. "Adenine" types as `12 2f 30 39 34 39 30`, the demo save's own bytes. A name cannot
be empty, as in the original (`ov_009::02189a4c` never accepts one) — the port says so on the
status line.

Typing a character the message font has no glyph for does nothing (the original's keyboard only
offers glyphs it has). The port offers the letters, digits and a small punctuation set; the
original's full key layout is not decoded. **No space**: the font's space is glyph 0 (its string
is the markup `< >`), which is also the save field's zero terminator, so a stored space would
truncate the name. The port also refuses to confirm a name whose first glyph is 0.

## What confirm does

`App::creation_confirm` → `app/flow.rs::start_created_game`:

1. the plain new game (`start_new_game`: story 1.1.1, the opening map `S14M05` / 6405, then
   the prologue), so the route and the pilot see exactly today's flow;
2. `HeroLook::for_sex(sex)` for the scene, **before** the opening map builds the hero's rig:
   skin tone 3 and the opening outfit for both sexes, face 9024 or 9021, hair 9000 or 9006, eye
   colour 0 or 5, hair colour 0 or 3 ([`re/characters.md`](../re/characters.md), the creation
   table);
3. the party's hero record (`Character::name`, `Character::sex`, `Character::look`), which
   `GameState::write_into` serialises to `+0x140` glyphs, `+0x174` bit 0 and the appearance
   block at `+0x160` (face `+356`, hair `+358`, the appearance byte `+372`, the hair colour
   `+373` and the body words `+376`/`+378`) — so a save the game writes (the church's
   Confession through `--save`/`--save-out`, the port's `write_save_out`) carries the created
   name, sex and look. The creation start itself has no cartridge open, so in the window the
   Confession refuses ("no save file is open"); the write path is exercised by the test below
   with the demo container;
4. the message identity (`Talk::set_identity`), which decodes the glyphs to text for `<HERO>`.

## Tests

`app/character/tests.rs` (real ROM, `DQ9_SAVE` for the save write) and the title's updated
`new_game_opens_character_creation_before_the_field`:

- New Game opens the screen; Escape returns to the title without starting a game;
- title → creation → confirm loads the opening map 6405 and sets `options.new_game`;
- "Adenine" is stored as `12 2f 30 39 34 39 30`, zero-padded, and decodes back from the
  identity;
- Left/Right moves the sex, and the woman's look takes face 9021, hair 9006, eye 5, hair
  colour 3 (the man: face 9024, hair 9000);
- an empty name does not confirm, a space is not a name character, and the 8-glyph limit
  holds;
- `--new-game` keeps the default hero (no screen, male, empty name);
- a written save carries the created name, sex and look, replacing the demo container's
  "Adenine" and its face/body bytes (`build/evidence/v3-character-creation/`).

## Limits

- Only the name and the sex are asked: the face, hair, skin and eye pages are **not** modelled;
  their values are the sex's defaults from the creation table. Those defaults are written to
  the save like the live look of any member (`docs/re/save.md` § "Writing a session"), so a
  created hero's log no longer keeps the container's face, hair, skin, eye and body bytes.
- The sex-dependent defaults are established for the initial mode (`0xd95` = 0); the other mode
  changes both faces, the woman's hair and colours and what the pages offer, and **what chooses
  it** is `[unknown]`.
- The original's keyboard layout, its markup keys (`<capslock>`, `<shift>`, `<back>`, the
  accented letters) and the name filter (`ov_009::02189a4c`'s word lists) are **not** modelled;
  accents can be typed only through the letters the port maps.
- The original's creation art, preview model and sounds are not reproduced.
