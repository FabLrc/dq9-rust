# Battle text: the five message files and the inflected name

Implementation: `crates/dq9-formats/src/battle/text.rs` (readers and the `BattleText` builder)
and `message.rs` (`Declension`, `article_number`, `inflect`: `FUN_020e4968`). Inspection with
`cargo dq9re battle-text <file> <number> [--lang] [--actor] [--target] [--item] [--action]
[--val] [--str] [--count] [--solo]`. Real-ROM checks: `crates/dq9-formats/tests/battle_text_rom.rs`.

Sources: the DQIX wiki *Battle Text*, *System Strings*, *Articles*, *Text Markup*, *Monsters*
(local checkout, facts only), the USA decompilation for the names of the ARM9 functions, and
the European ROM for every count. Counts and addresses below were measured or read on this
ROM; a value the wiki only inferred is marked **[hypothesis]**.

## Files

| file | archive / member | messages | layout |
|---|---|---|---|
| `strbtl` | `data/bin/strbtl.gp2/strbtl_<lang>.nat` | 17 | system strings |
| `actmsg` | `data/prm/actmsg.gp2/actmsg_<lang>.nat` | 591 | system strings |
| `str_bres` | `data/bin/str_bres.gp2/str_bres_<lang>.bin` | 44 | Level-5 record table |
| `str_btl` | `data/bin/menu/str_btl.gp2/str_btl_<lang>.nat` | 106 | system strings |
| `str_tm` | `data/bin/menu/str_tm.gp2/str_tm_<lang>.nat` | 329 | system strings |

The **system-strings layout** is the one of `strstd`: a head word (bits 0–11 the record count,
12–27 the string section's size), then `(u32 number, u32 string offset)` records, then the
strings, ending the file exactly. `battle::FixedRecords` splits it and checks every offset
starts a string; the five files pass on all five languages. The high halfword of a record's
number is flags on `actmsg`'s record 0 (numbered 0, carrying `0x39`); the number is the low
halfword.

`str_bres` is a **Level-5 record table** (`table.rs`): 47 records, of which 44 are kind
`0x67`, each holding the message number and the string as its first two fields. Record 2
(kind `0x66`) holds 44, the message count. Four of the 44 carry a file path instead of a
sentence (`data/prm/level%d.bin`, `data/bin/skilltable.bin`, `str_gskl_<LG>.bin`,
`data/prm/str_gskl.gp2`); they are numbers 0, 14, 33 and 43 (`0x2b`) and are read like the
rest, by number.

**French `str_bres` 6 is defective in the ROM**: the record holds `<Cap><str_1> obtient
<val_1> point` then the whole English messages 6 to 9 (`<str_1> earns …`, `<str_3>`…) and at
its end `d<1>exp<'e>rience<!>`. The pipeline prints it as authored; the clean two-member line
is 7 (`Hero obtient 3 points d<1>exp<'e>rience<!>`). No test depends on 6.

Messages that matter to chapter 2, measured:

- `actmsg` **1** `<Cap><DEF_ART_ACTOR> attacks.`, **2** `… takes <val_1> point(s) of damage.`,
  **9** defeated, **10** defends, **22** `… <1>s wounds are healed.`, **46** `… casts
  <ACTION>.`, **140** `Critical hit!`, **141** a spell gone haywire.
- `strbtl` **6** `A slime draws near!` / `Some slimes draw near!` (the singular branch on
  `<IF_SING val_1>`, `<val_1>` being the number of monsters), **8** `Mortamor draws near!`,
  **13** the monster strikes first.
- `str_bres` **6** to **9** experience (one to four members, each with `<str_n>` and
  `<val_n>`), **10** level up, **13** skill points, **25**/ **26** shared experience,
  **20** (`0x14`) wipe-out.

`itemname.gp2/itemname_<lang>.nat` is in the same head-word layout with 16-byte records:
singular name, plural name, grammar word, item id (`+0x0C`). 1,178 records per language; the
ids are the `itemdt` ids (the medicinal herb is 22000). `article.gp2/article_<lang>.nat` is a
system-strings file: 38 articles in English, 64 French, 68 German, 122 Italian and 39 Spanish.

## Which message an action says

`actdt_a`/`actdt_b` record field `+0x20`: bits 10–19 the **opening** line, bits 20–31 the
**result** line. The wiki marked this INFERRED; it is checked here on the chapter's actions:
the plain attack 1 says 1 then 2 (`attacks.`, `takes N points of damage.`), Frizzle 10 says
46 then 2 (`casts <ACTION>.`), Heal 30 says 46 then 22 (`casts Heal.`, `… wounds are
healed.`), Lightning Storm 74 says 179 then 2, and the medicinal herb 255 says 70 then 22
(`uses <INDEF_ART_SGL_I_NAME>.`, `… wounds are healed.`). Each pair is an opening and a
result, never two results, on every one. **Fact** for these numbers.

The critical line is not in `+0x20`: 140 is said by the resolution when a blow rolls a
critical.

## The inflected name — `FUN_020e4968`

`Msg_ExpandNames` (`0x02069244` EU) collects the parameters of a grammar tag
(`DEF_`/`INDEF_` article, `ART_`, `SGL_`/`PLR_`, the case, the entity) and calls
`FUN_020e4958` (EU `0x020e4968`, **[hypothesis: `Msg_InflectName`]**) with the entity's name
record. The record is the triple singular/plural/grammar word at `+0x00`, `+0x04`, `+0x08`
of a monster's or an item's name record.

`FUN_020e4968` chooses one of **six builders** from the table at `0x020eea00` (EU), by
`number × 3 + (1 − article) + ART_` (`number` 0 singular, 1 plural; `article` 0 definite,
1 indefinite, absent counted as 1; `ART_` 0 or 1):

| row | builder | article number |
|---:|---|---|
| 0 | singular, no article | — |
| 1 | singular indefinite | `100 + bits 0–5` |
| 2 | singular definite | `bits 12–17` |
| 3 | plural, no article | — |
| 4 | plural indefinite | `300 + bits 6–11` |
| 5 | plural definite | `bits 18–23` |

So `<DEF_ART_ACTOR>` is row 2, `<INDEF_ART_SGL_I_NAME>` row 1, `<INDEF_ART_PLR_M_NAME>` row 4,
`<DEF_ART_PLR_M_NAME>` row 5, `<SGL_M_NAME>` row 0 and `<PLR_M_NAME>` row 3. The article is
looked up in `article_<lang>.nat` by that number. `func_020e462c` (EU `0x020e463c`) then
prints the article, a **space** unless the article ends with the apostrophe glyph `<1>`
(French `l'`, Italian `l'`), and the singular or plural. An empty article (number 0) prints
nothing, not even the space.

The measured English slime (number 1) carries grammar `0x02041041`: bits 0–5 `1` (article 101
`a`), 6–11 `1` (301 `some`), 12–17 `1` (1 `the`), 18–23 `1` (201 `the`), gender 2. Hence
`A slime draws near!`, `Some slimes draw near!`, `The slime attacks.` The French slime
(`gluant`) carries `0x43042` and gives `Le gluant subit 5 points de d<'e>g<^a>ts.`

**Gender** (bits 24–25) is read by the `IF_ACTOR_M`/`_F`/`_N`, `IF_TARGET_…`,
`IF_M_NAME_…`, `IF_I_NAME_…` and `IF_ACTION_…` branches of the message pipeline. The bit
position is the wiki's INFERRED one; the engine's condition handlers were not decompiled here,
so the position is **[hypothesis]** — the values on the game's names agree with the text
(the named men are 0, most monsters 2).

German's names and articles are **templates**: `Schleim[gs]`, `[defm]`, `[dn]`, `[adjf]`…
They carry no article word in the text table. `FUN_020e4968` expands the article tags with the
table at `0x020eea60` (7 entries, four case bytes, a name pointer, a length) and the words at
`0x020eea18` (12 six-byte words, `der` to `einer`); then `FUN_020e4864` (EU `0x020e4874`)
expands the suffix tags with the tables at `0x020f2d2c` (13 11-byte tags), `0x020eeac0` (13
rows of two bytes per case: definite, indefinite) and `0x020ee9e8` (8 three-byte endings).
The case is the tag's (`NOM_` 0 … `ACC_` 3); in German only (`func_0200fb10` returns the
language, 3) a tag with no case gets the nominative. Measured: `Der Schleim nimmt 5 Punkte
Schaden.` The tables are read from the ARM9 at those addresses, so no German word is copied
into the repository.

`str_bres`'s "The slime<1>s level increases to…" shows the apostrophe as the font's glyph
markup `<1>`; the drawn window substitutes the glyph.

## `dq9re battle-text`

```
cargo dq9re battle-text actmsg 2 --target slime --val 5
The slime takes 5 points of damage.
cargo dq9re battle-text actmsg 1 --actor 1 --lang fr
Le gluant attaque.
cargo dq9re battle-text strbtl 6 --target 1 --val 2 --lang de
Schleime n<:a>hern sich!
```

`--actor` and `--target` take a monster number, code or name (localised) or `hero`;
`--item` an item id or name; `--action` an action number; `--val`, `--str` repeat; `--count`
is the target count and `--solo` the one-member party. The command prints only the drawn
text: control codes draw nothing, a line break is a newline.

## Evidence

- `crates/dq9-formats/tests/battle_text_rom.rs` asserts the counts above, the singular and
  plural draw-near lines, attack/damage with the hero and a slime, the critical, the level-up,
  experience and skill-point lines, an item message, the French article and the German case
  expansion, and the action→message pairs on five chapter 2 actions.
- Ghidra queries (EU addresses): `decomp 0x020e4968` (`FUN_020e4958`), `decomp 0x020e4874`
  (`FUN_020e4864`), `decomp 0x020e463c` (`func_020e462c`), `decomp 0x02069244`
  (`Msg_ExpandNames`), and the tables read from `arm9.bin` at `0x020ee9e8`, `0x020eea18`,
  `0x020eea60`, `0x020eeac0`, `0x020f2d2c`, `0x020eea00`, `0x020e4b34`…
- `build/evidence/j4-battle-text/verify.log` (sha256 `2abf1fca2092`): `verify.ps1
  -RequireRom`, all steps pass.
- `build/evidence/j4-battle-text/battle-text-rom.log` (`06fe623b36b3`): the nine real-ROM
  tests.
- `build/evidence/j4-battle-text/samples.txt` (`722a0b6505cf`): the chapter 2 messages in
  English and French, and the French two-member experience line.

## Open

- **[unknown]** the high halfword `0x39` of `actmsg`'s record 0, and the meaning of the four
  `str_bres` records that hold paths.
- **[unknown]** grammar bits 26–31 (the wiki leaves them open).
- **[hypothesis]** the gender bits 24–25: the wiki's reading, not the condition handlers'
  code.
- Not modelled: `[WPN…]`/`[ACTER…]` (`Name::Weapon`, `Name::Actor`) keep the actor's name;
  no chapter 2 battle message uses them.
