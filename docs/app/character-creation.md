# Character creation

The title's **Create a new adventure log** opens this screen before the prologue: the hero's
**name**, **sex** and **appearance** — the hair, hair colour, face, skin tone and eye colour the
original's overlay 9 asks for on pages 3–7, and the two fields the message window, the scene and
the save consume ([`re/characters.md`](../re/characters.md) § "Character creation").

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

## The pages

One page at a time on the port's own 256×192 window (it draws a placeholder frame, no renderer),
with the game's message font (`App::creation_quads`):

| Page | Keys | Content |
|---|---|---|
| Name and sex | letters/digits/`.` `,` `'` `-` `/` `;` (Shift: upper case), Backspace; Left/Right: sex; Enter: confirm; Esc: title | the name with a caret, **Man** / **Woman** highlighted |
| Hair | Left/Right: option; Enter/Down: next; Esc/Up: back | `Hair: n / 10`, the ten options, the resolved part |
| Hair colour | same | `Hair colour: n / 10`, the ten options, the part and its texture |
| Face | same | `Face: n / 10`, the ten options, the resolved part |
| Skin tone | same | `Skin tone: n / 8` |
| Eyes | same | `Eyes: n / 8` |

Down opens the hair page from the name screen; Enter on the last appearance page returns to the
name screen, where Enter confirms and Escape returns to the title. Enter never confirms from an
appearance page. This is the port's navigation, not the original's: overlay 9 uses a list widget
moved with up/down, A advances the page and B goes back (`ov_009::02188ee8`), and page tabs jump
directly (`100 + page`). The option counts are the original's list lengths
(`func_ov009_02188c2c`): 10 hair, 10 hair colour, 10 face, 8 skin tone, 8 eye colour.

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

## The appearance options

Each appearance page edits one **per-sex column** of overlay 9 (`+0xda4`–`+0xdaf`, selected by
`+0xda3`); the port keeps the same two columns
(`dq9_formats::hero::CreationAppearance`). The tables are the original's
(`ov_009::02188b14`, ten shared entries `00..09` with a per-sex prefix):

| Page | Options | Man | Woman |
|---|---|---|---|
| Face (items) | 10 | 9024, 9020, 9021, 9022, 9023, 9025 … 9029 | 9021, 9020, 9022 … 9029 |
| Hair (items) | 10 | 9000 … 9009 | 9006, 9000 … 9005, 9007 … 9009 |
| Hair colour | 10 | `palette.bin` brows row 0–9 | same |
| Skin tone | 8 | `palette.bin` skin ramp 0–7 | same |
| Eyes | 8 | `palette.bin` eye pair 0–7 | same |

Left/Right **clamp**: the first and last options do not wrap (the original's list widget never
leaves its range). The **sex page's defaults** are the original's: face preset 0, hair preset 0,
hair colour 0 and skin tone 3 for both sexes, eye colour 0 for a man and 5 for a woman
(`ov_009::02185634`, `docs/re/characters.md`). Switching the sex switches the whole column
(`ov_009::02188ee8` case 1), so a sex you have not edited shows its own defaults and edits stay
with their sex; that is the "reset" the original performs on a sex change. The `0xd95` mode
flag's resets (face preset 1, the woman's hair 1, eye 0, hair colour 0) are **not modelled**:
what triggers the flag is **[unknown]**.

**The preview.** On each change the screen re-resolves the look through `Outfit::resolve` — the
same call `HeroRig::build` makes — and prints the parts the field rig will assemble (`p_f004`,
`p_h060a`, and on the hair colour page the `p_h0c0a.nsbtx` texture). It is a text preview: the
creation screen draws a placeholder frame and has no renderer, so no 3D model is shown here
(unlike the original's page 8 preview). The rig rebuild itself is the equipment change's
(`AnimatedScene::replace_hero`) and is exercised by `the_chosen_face_and_hair_rebuild_the_hero_rig`:
a chosen face and hair change the drawn geometry and restoring the default look restores it.

## What confirm does

`App::creation_confirm` → `app/flow.rs::start_created_game`:

1. the plain new game (`start_new_game`: story 1.1.1, the opening map `S14M05` / 6405, then
   the prologue), so the route and the pilot see exactly today's flow;
2. `Creation::look` (`CreationAppearance::to_look`) for the scene, **before** the opening map
   builds the hero's rig: the sex's defaults (skin tone 3, the opening outfit, the body build)
   under the pages' choices — face item, hair item, hair colour, skin tone and eye colour;
3. the party's hero record (`Character::name`, `Character::sex`, `Character::look`), which
   `GameState::write_into` serialises to `+0x140` glyphs, `+0x174` bit 0 and the appearance
   block at `+0x160` (face `+356`, hair `+358`, the appearance byte `+372`, the hair colour
   `+373` and the body words `+376`/`+378`) — so a save the game writes (the church's
   Confession through `--save`/`--save-out`, the port's `write_save_out`) carries the chosen
   name, sex and look. The creation start itself has no cartridge open, so in the window the
   Confession refuses ("no save file is open"); the write path is exercised by the tests below
   with the demo container;
4. the message identity (`Talk::set_identity`), which decodes the glyphs to text for `<HERO>`.

## Tests

`app/character/tests.rs` (real ROM, `DQ9_SAVE` for the save writes) and the title's
`new_game_opens_character_creation_before_the_field`:

- New Game opens the name page; Escape returns to the title without starting a game;
- title → creation → confirm loads the opening map 6405 and sets `options.new_game`;
- "Adenine" is stored as `12 2f 30 39 34 39 30`, zero-padded, and decodes back from the
  identity;
- the pages advance (name → hair → hair colour → face → skin tone → eyes → name) and go back,
  and Escape on the name page still returns to the title;
- Left/Right move the sex on the name page and clamp the option on an appearance page; the
  preview follows the choice (`p_h000a` → `p_h020a`);
- each sex keeps its own column: a man's face preset 5 stays when the sex is switched away and
  back, and the woman's column stays at her defaults;
- an empty name does not confirm, a space is not a name character, and the 8-glyph limit
  holds;
- `--new-game` keeps the default hero (no screen, male, empty name);
- a written save carries the created name, sex and look, then a chosen face/hair/colours:
  `+356` = 9025, `+358` = 9003, `+373` = 4, `+372` skin 6 / eyes 2, and the reloaded state reads
  the same look (`build/evidence/v3-character-appearance/`);
- the chosen face and hair rebuild the drawn rig (`replace_hero`);
- `dq9_formats::hero` pins the tables: the preset lists, the option counts, the clamping, the
  defaults equal to `HeroLook::for_sex`, and every option resolving to a `chara_pc.gp2` part on
  the cartridge.

## Limits

- The original's **face-offset page** (page 2, five options, `+0xda4`, only the preview sprite
  offset `ov_009::02188bf8`) and its **3D preview page** (page 8) are not modelled: the first
  stores nothing the save keeps and its meaning is **[unknown]**, the second needs the original's
  renderer.
- The `0xd95` mode-1 defaults and hidden pages are not modelled; the flag's trigger is
  **[unknown]**.
- The preview is the resolved part list, not a 3D model (above); the original's creation art,
  its model page, the option sprites and its sounds are not reproduced.
- The original's keyboard layout, its markup keys (`<capslock>`, `<shift>`, `<back>`, the
  accented letters) and the name filter (`ov_009::02189a4c`'s word lists) are **not modelled**;
  accents can be typed only through the letters the port maps.
