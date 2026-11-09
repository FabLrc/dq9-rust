# External resources useful to the project

Survey made on 4 November 2026: projects, tools and documentation that can save time in
reverse engineering and porting. Each entry states what it brings, its license and how to use
it here. Links and figures come from the repositories themselves as of that date; anything not
verified is marked **[unverified]**.

## Key points

1. **An active DQ9 community already does the same work**, in public: the GitHub
   organization [DQI-haX](https://github.com/DQIX) (decompilation, format wiki, tools) and
   the Discord *The Quester's Rest* (<https://discord.gg/dqix>, "DQI-haX" thread).
2. **The USA decompilation symbols apply to our European ROM** with a simple address
   offset (verified below on our 193 symbols). 1,704 names and the bounds of
   ~20,000 functions, under the Unlicense (public domain).
3. **The [DQIX wiki](https://github.com/DQIX/wiki) documents, on the same `YDQP` ROM, a
   good part of our roadmap**: triggers, `.stb` scripts, `.nat` tables,
   maps, sprites, SDAT, NSBMD… Read it before tackling a format.
4. **[DQIX Enhanced](https://github.com/sackchief/dqix-enhanced)** (static recompilation
   to PC, GPL-3.0) has already solved online multiplayer, Tag mode, the DQVC shop and
   downloadable quests. Source of facts, **no code to copy** (see Licenses).
5. **[apicula](https://github.com/scurest/apicula)** is an NSBMD/NSBTX/NSBCA reader/viewer
   in Rust, under 0BSD: its readers are ported into `nds_rom::g3d`
   ([`re/g3d.md`](re/g3d.md)), credited at the top of the module.

## DQ9-specific projects

### dqix-decomp — "matching" decompilation (USA and JPN)

<https://github.com/DQIX/dqix-decomp> · Unlicense · tool [dsd](https://github.com/AetiasHax/ds-decomp)

- Rebuilds a byte-identical USA or JPN ROM from assembly and C++.
  Progress as of 2026-11-04: 9.18% of functions, 6.05% of bytes (USA).
- `config/usa/arm9/**/symbols.txt`: bounds and types (ARM/Thumb, size) of all
  functions and data, 1,704 explicit names (1,584 in the main ARM9 module),
  often mangled C++ names (`_ZN7GPCFile12GetFileByCRCEj`) that reveal the classes.
- Code already decompiled and directly useful to the port: grotto generator
  (`src/Grotto/Main/FloorMapGenerator.cpp`, `TreasureMapDataStructs.cpp`…), combat
  formulas (`src/Combat/…/BasicAttackCalculation.cpp`, `CritRateCalculation.cpp`,
  `TensionBonusCalculation.cpp`, `GetAttackBaseDamage.cpp`), GPC (`src/Filesystem/GPC.cpp`),
  randomness (`src/Util/Random.cpp`), play time, lighting, `.spr` sprites, `.pac`, `.bcfg`,
  and a large part of the NitroSystem G3D engine (`src/NitroSDK/NSBXX/`).
- `Decompiling.md` explains the workflow (Ghidra + [dsd-ghidra](https://github.com/AetiasHax/dsd-ghidra)
  to sync symbols, [decomp.me](https://decomp.me), objdiff).

#### USA → Europe mapping

Established by DQIX Enhanced's `tools/region.py` and **verified here**: the 177 ARM9-module
symbols and the 16 overlay-17 symbols of `re/symbols.txt` all land on a symbol start
in `config/usa/arm9` once the address is converted (`Gpc2_LoadIndex` ↔
`LoadAndDecompressGPCHeaderAndInnerFileInfo…`, `Gpc_FindEntryByHash` ↔ `GPCFile::GetFileByCRC`,
`Msg_Compile` ↔ `func_02069fec`…).

| USA address (`YDQE`) | Europe address (`YDQP`) |
|---|---|
| ARM9 module `0x0200F3B8..0x0200FB20` | + `0x8` |
| ARM9 module `0x0200FB20..0x020EEBD0` (code and constructor table) | + `0x10` |
| rest of the ARM9 module (data after the code), ITCM, DTCM, **all overlays**, ARM7 | identical |
| secure area `0x02000000..0x02000800` (BIOS call stubs) | different order: find each stub by its `swi` number |

The 16 bytes come from two functions (`func_0200f3a4`, `func_0200fb08` on the USA side) that
choose the language from the console settings.

**Recommended use**: a converter (script or `dq9re` subcommand) that reads dsd's
`symbols.txt`, applies the table above, keeps only explicit names (not
`func_…`/`data_…`) and outputs a file in the `re/symbols.txt` format, applied before our
own names by `ghidra-apply-symbols.ps1`. The decompilation's function bounds also
complete the functions Ghidra's auto-analysis misses (code reached only through pointer
tables).

### DQIX format wiki

<https://github.com/DQIX/wiki> · **no license** (reuse the facts citing the page,
do not copy the text)

88 pages, observations made on the **European `YDQP` ROM** (ours), code addresses
often given in USA with the decompilation name. Same discipline as our
`docs/re/`: "confirmed", "INFERRED", "Not established", evidence by file and offset.
Beware: trigger and engine-function codes are given **in decimal** there (our
`trigger.md` writes them in hexadecimal).

| Topic | Wiki pages |
|---|---|
| Triggers (done on our side, to cross-check) | [Triggers](https://github.com/DQIX/wiki/blob/main/Triggers.md): conditions and actions in decimal (41 = our `0x29`, 119 = `0x77`), event types ("Kinds"), oriented zones; [Story-Threads](https://github.com/DQIX/wiki/blob/main/Story-Threads.md), [Time-Of-Day](https://github.com/DQIX/wiki/blob/main/Time-Of-Day.md); may resolve some **[unknown meaning]** in `trigger.md` |
| `.stb` event scripts and **natives** | [Event-Scripts](https://github.com/DQIX/wiki/blob/main/Event-Scripts.md) (stack machine, shared library of 18 routines); **[Engine-Functions](https://github.com/DQIX/wiki/blob/main/Engine-Functions.md): role of most natives 100–845** (characters 200, camera 300, messages 400, cutscene 500, sound 700, ending 800); [Event-Lists](https://github.com/DQIX/wiki/blob/main/Event-Lists.md), [Event-Text](https://github.com/DQIX/wiki/blob/main/Event-Text.md) |
| 3D graphics and maps | [NSBMD](https://github.com/DQIX/wiki/blob/main/NSBMD.md), [NSBTA-and-NSBMA](https://github.com/DQIX/wiki/blob/main/NSBTA-and-NSBMA.md), [NSBTP](https://github.com/DQIX/wiki/blob/main/NSBTP.md), [Map-List](https://github.com/DQIX/wiki/blob/main/Map-List.md), [Map-Archive](https://github.com/DQIX/wiki/blob/main/Map-Archive.md), [Map-Collision](https://github.com/DQIX/wiki/blob/main/Map-Collision.md), [Map-Objects](https://github.com/DQIX/wiki/blob/main/Map-Objects.md), [Map-Textures](https://github.com/DQIX/wiki/blob/main/Map-Textures.md) (`.bmbl`), [Doors](https://github.com/DQIX/wiki/blob/main/Doors.md), [Character-Parts](https://github.com/DQIX/wiki/blob/main/Character-Parts.md), [Motion-Tables](https://github.com/DQIX/wiki/blob/main/Motion-Tables.md) |
| Game and combat tables | [Items](https://github.com/DQIX/wiki/blob/main/Items.md), [Monsters](https://github.com/DQIX/wiki/blob/main/Monsters.md) (`mon_btldata.nat`…), [Actions](https://github.com/DQIX/wiki/blob/main/Actions.md) (`actdt_a/b`), [Spell-Table](https://github.com/DQIX/wiki/blob/main/Spell-Table.md), [Articles](https://github.com/DQIX/wiki/blob/main/Articles.md) (grammatical inflection of names), [System-Strings](https://github.com/DQIX/wiki/blob/main/System-Strings.md) (text `.nat` format), [Encounters](https://github.com/DQIX/wiki/blob/main/Encounters.md), [Battle-Resolution](https://github.com/DQIX/wiki/blob/main/Battle-Resolution.md), [Battle-AI](https://github.com/DQIX/wiki/blob/main/Battle-AI.md), [Alchemy-Recipes](https://github.com/DQIX/wiki/blob/main/Alchemy-Recipes.md), [Level-Tables](https://github.com/DQIX/wiki/blob/main/Level-Tables.md) |
| Interface (map, dialogue box, menus) | [Bitmap-Font](https://github.com/DQIX/wiki/blob/main/Bitmap-Font.md), [Sprites](https://github.com/DQIX/wiki/blob/main/Sprites.md) (`.spr`), [Menu-Backgrounds](https://github.com/DQIX/wiki/blob/main/Menu-Backgrounds.md), [2D-Graphics](https://github.com/DQIX/wiki/blob/main/2D-Graphics.md), [Mini-Map](https://github.com/DQIX/wiki/blob/main/Mini-Map.md), [Area-Cast](https://github.com/DQIX/wiki/blob/main/Area-Cast.md) (`.npc`) |
| Non-localized tags | [Text-Markup](https://github.com/DQIX/wiki/blob/main/Text-Markup.md) (tag table, `<ADD>`, `<N_TURN>`, `<TIME=n>`, `<ME_n>`/`<SE_n>`…), [Text-Events](https://github.com/DQIX/wiki/blob/main/Text-Events.md) |
| Formats still **[unknown]** in `rom-layout.md` | `.nat` (game tables above), `.npc` ([Area-Cast](https://github.com/DQIX/wiki/blob/main/Area-Cast.md)), `.pac` ([Pac](https://github.com/DQIX/wiki/blob/main/Pac.md)), tagged tables `.dat/.bats/.bcfg/.bpos/.bmed/.bmbl` ([Tagged-Data-Table](https://github.com/DQIX/wiki/blob/main/Tagged-Data-Table.md)); `.lia` not read (listed as unknown in Menu-Backgrounds) |

What we have that the wiki lacks (to contribute back): NPC line selection
(`dialogue.md`), the 6-pass message pipeline (`text.md`), font layout
(`font.md`).

### DQIX Enhanced — PC port by static recompilation

<https://github.com/sackchief/dqix-enhanced> · GPL-3.0-or-later · v33 stable

Different approach from ours: the ARM code is translated to C and the DS hardware is emulated
around it (3D rendering, sound and Wi-Fi based on melonDS). It already works end to end, which
makes it an **oracle**: same input → same result as the DS, "Classic" mode verified pixel
by pixel against an emulator. Facts documented in
[`docs/TECHNICAL.md`](https://github.com/sackchief/dqix-enhanced/blob/main/docs/TECHNICAL.md)
and [`ROADMAP.md`](https://github.com/sackchief/dqix-enhanced/blob/main/ROADMAP.md):

- **Online multiplayer** (Tag mode, visits via Pavo, 4 players): the DS requires radio
  responses within microseconds, so *each PC runs the DSes of all players*; only
  inputs travel (TCP via the host, port 27900, input delay chosen from ping),
  with memory fingerprints every 2 s to detect desync. Cloudflare relay
  for NATs, UPnP/NAT-PMP/PCP. A reimplementation can do something simpler
  (high-level protocol), but must reproduce the **data exchanged** in Tag mode.
- **DQVC and downloadable quests**: built-in Wi-Fi Connection server. Files encrypted with
  RC4 using the key `XENLONPROJECTKEY`; the last 4 bytes make the encrypted file,
  read as a number, divisible by `0xC2A030D4`. The 64 quests and 23 special guests are
  on the cartridge; the connection only enables them in the save. Schedule of the
  52 DQVC sales (`runtime/dqvc_schedule.h`, built from yab's lists).
- The DS connection to Nintendo goes over HTTPS; the community's "NoSSL" patches
  switch the addresses to `http://`, and the login server must return a `Date` header.
  In October 2026, neither AltWFC nor Wiimmfi served DQIX anymore.
- Save: 64 KB EEPROM, DS format kept (compatible with editors).
- Fixes required in the game itself: anti-piracy memory leak in the file
  loader (freeze after the first battle), encrypted anti-piracy code run by an
  interpreter (dqix-functions places the protection in overlay 29 **[unverified]**).

### Other DQI-haX projects

| Project | Contribution | License |
|---|---|---|
| [dqix-functions](https://github.com/DQIX/dqix-functions) | JPN symbols (resymgen format), overlay map with their **internal names** (`00 battle`, `01 event`, `17 gamemain`, `23 sub_menu`, `31 wifi`, `32 sound`…), glossary of the three random generators (see below) | GPL-3.0 |
| [dqix-script-disasm](https://github.com/DQIX/dqix-script-disasm) | disassembler/assembler of binary initialization scripts (lighting, atmosphere, chest contents) to Lua; contexts per script type | MIT |
| [ArchiveTool](https://github.com/DQIX/ArchiveTool) | extraction/rebuild of `.gp2`: cross-check for `gpc2.rs` | MIT |
| [desmume-scripts](https://github.com/DQIX/desmume-scripts) | DeSmuME Lua scripts: JPN/USA/EUR random tables, hitbox display, collisions, monster spawn nodes, player coordinates | none |
| [editor](https://github.com/DQIX/editor) ([online](https://dqix.github.io/editor/)) | full save editor; `pattern.hexpat` describes the save for ImHex: basis for the port's save | none |
| [Grotto-Searcher](https://github.com/DQIX/Grotto-Searcher) | grotto generation from a treasure map (link `?id=RRSSSS`): test oracle for a reimplemented generator | unspecified |
| [BattleEmulator](https://github.com/DQIX/BattleEmulator) | solo battle simulator (C++/wasm), formulas and random rolls faithful to the game: oracle for `dq9-core` | MIT |
| [auction](https://github.com/DQIX/auction) | generator for the DLC file `auction.bin` (DQVC stock) | none |
| [Collapsus](https://github.com/DQIX/Collapsus) | database (recipes, quests, monsters, grottoes) extracted from game structures | GPL-3.0 |
| [Save-Editor-Decomp](https://github.com/DQIX/Save-Editor-Decomp) | decompilation of asa-o's historical save editor | archived |

Random generators (from the dqix-functions glossary):

| Name | Algorithm | Known uses |
|---|---|---|
| AT | 32-bit LCG: `r = r * 0x41C64E6D + 0x3039` | monster spawns, terrain, animations, chest loot |
| BT ("Hoimi table") | 64-bit LCG: `s = s * 0x5D588B656C078965 + 0x269EC3`; same seed as AT | critical alchemy, fleeing, Zaoral, boss loot, encounters |
| CT | same 64-bit LCG, seed drawn at each battle | battle actions, camera |

## Generic Nintendo DS tools

### Code analysis

| Tool | Value for us | License |
|---|---|---|
| [ds-decomp (`dsd`)](https://github.com/AetiasHax/ds-decomp) | DS decompilation toolkit **in Rust** (crates [`ds-decomp`](https://crates.io/crates/ds-decomp), [`ds-rom`](https://lib.rs/crates/ds-rom)); `dsd init` produces a full map (functions, relocations, overlays) of a ROM: this is how DQIX Enhanced mapped the European version | MIT |
| [dsd-ghidra](https://github.com/AetiasHax/dsd-ghidra) | Ghidra loader + symbol sync from a `dsd` config | MIT |
| [objdiff](https://github.com/encounter/objdiff), [decomp.me](https://decomp.me) | function-by-function comparison; useful only if contributing to the decompilation | Apache-2.0 |
| [ndskit](https://github.com/vs-sr-dev/ndskit) | generic DS porting kit (Python + C++): ARM/Thumb function discovery, interpreter, **memory and sound-channel comparison against a melonDS savestate** (`python -m ndskit.melonds STATE.ml1 --compare`): possible basis for our differential comparison | MIT |
| [libntr](https://github.com/cybervisi0n/libntr) | NitroSDK replacement for PC (used by the Pokémon Platinum PC port): reference for Nintendo library behavior outside the DS | MIT |
| [pret/pokeplatinum](https://github.com/pret/pokeplatinum), [pret/pokediamond](https://github.com/pret/pokediamond) | names and code of NitroSDK/NitroSystem functions, to recognize libraries in our binary (SDK `0x04027539`) | none |

### Emulators and dynamic analysis

- **melonDS** (already installed): GDB stub. Its research (3D rendering, sound, Wi-Fi) is the
  reference followed by DQIX Enhanced. GPL-3.0.
- **DeSmuME**: built-in debugger and **Lua scripts**; the random tables of the DQI-haX
  scripts above exist for the European version, the other scripts are mostly JPN.
- **No$GBA**: debugger; [GBATEK](https://problemkaputt.de/gbatek.htm) remains the hardware
  reference (3D engine, `BMD0/BTX0/BCA0` formats in "DS Files - 3D Video").

### Standard Nintendo formats

| Need | Resource | License |
|---|---|---|
| Nitro models/textures/animations (stage 3) | [apicula](https://github.com/scurest/apicula) (Rust: reading, viewer, glTF/COLLADA export) and its docs [nsbmd_docs](https://github.com/scurest/nsbmd_docs) | 0BSD |
| NNS G3D rendering in a browser | [noclip.website `src/nns_g3d`](https://github.com/magcius/noclip.website/tree/main/src/nns_g3d) (TypeScript, Mario Kart DS, Pokémon Platinum/HGSS) | custom, to check |
| SDAT sound (`dq9-audio` crate) | [ndspy SDAT docs](https://ndspy.readthedocs.io/en/stable/appendices/sdat-structure.html), [Nitro Studio 2 specs](https://gota7.github.io/NitroStudio2/specs/), the DQIX wiki [SDAT](https://github.com/DQIX/wiki/blob/main/SDAT.md) page (the game's three `.sdat`); sequencers [SSEQPlayer](https://github.com/kode54/SSEQPlayer) (C++) and [FSS](https://github.com/fincs/FSS) | WTFPL |
| Modern SDAT player | [DualRip](https://github.com/TetraSsky/DualRip) (emulates the DS sound driver) | MIT |
| Mobiclip videos (`.mods`, 6 files) | FFmpeg (`mobiclip` decoder, [`mods`](https://www.ffmpeg.org/doxygen/7.0/mods_8c.html) demuxer); [Mobiclip format documentation](https://gist.github.com/quatric/3e2f0caa5c22a9a8b24d9cf1ffdfe860) | LGPL (FFmpeg) |
| Level-5 compression / Level-5 formats in general | [Kuriimu2](https://github.com/FanTranslatorsInternational/Kuriimu2) | GPL-3.0 |

## Online servers and downloadable content

- [DLS1-Files](https://github.com/InternalLoss/DLS1-Files) (mirror
  [GitLab](https://gitlab.com/internalloss/dls1-files)): files served by Nintendo's
  download server (`dls1.nintendowifi.net`), preserved by the community, including
  DQIX's per region (`YDQE`, `YDQP`, `YDQJ`: quests, DQVC stock, news
  message). These are game data: read them from the player's copy, never
  redistribute.
- NAS/DLS1 protocols: [Tockdom wiki](https://mkwiiki.org/wiki/DLS1).
- Wiimmfi and AltWFC: Nintendo WFC replacements, but no longer served DQIX in October
  2026 (source: DQIX Enhanced).

## Game mechanics and reference data

Useful for writing tests or invariants against the real ROM:

- [Dragon Quest Wiki](https://dragon-quest.org/wiki/Dragon_Quest_IX:_Sentinels_of_the_Starry_Skies)
  (dragon-quest.org) and [Fandom](https://dragonquest.fandom.com/): lists of items, monsters,
  grotto locations.
- GameFAQs: [Grotto Mechanics Guide](https://gamefaqs.gamespot.com/ds/937281-dragon-quest-ix-sentinels-of-the-starry-skies/faqs/61151)
  (grotto generation, wandering monster bug).
- [Dragon's Den](https://www.woodus.com/den/games/dq9ds/): recipes, legendary boss loot,
  historical save editor (asa-o, v0.7).
- Japanese: [daisukedaisuke's analysis blog](https://daisukedaisuke.hatenablog.com/)
  (encounters, companion tables, undocumented specifications),
  [Qiita article on the Hoimi table](https://qiita.com/DaisukeDaisuke/items/9d829d4d2878d8ab82c4),
  [ドラゴンクエスト9攻略Wiki](https://dragonquest9.com/), [hossy.info/dq9](https://hossy.info/dq9).

## Licenses: what we can integrate

The repository is licensed under `AGPL-3.0-or-later`. Unlicense, MIT, 0BSD, Apache-2.0 and
GPL-3.0-or-later code may legally be combined with AGPL-3.0; the project rule below still
keeps GPL and unlicensed sources as read-only references.

| Status | Projects | Rule |
|---|---|---|
| Integrable (with attribution) | dqix-decomp (Unlicense), apicula (0BSD), ds-decomp/ds-rom, dsd-ghidra, ndskit, libntr, dqix-script-disasm, ArchiveTool, BattleEmulator, DualRip (MIT), objdiff (Apache-2.0), SSEQPlayer/FSS (WTFPL) | reusable code and data; cite the source |
| Read-only | DQIX Enhanced, melonDS, Kuriimu2, dqix-functions, Collapsus (GPL-3.0), FFmpeg (LGPL) | learn the facts, reimplement yourself; do not copy or translate the code |
| No license | DQIX wiki, editor, desmume-scripts, auction, pret | facts reusable citing the page; do not copy text or code |

Facts about the game (offsets, formulas, constants) are not protected by these licenses;
code and prose are. In `docs/re/`, cite the external source of a fact and
verify it on the ROM like our own findings.

## Implementation

These resources are scheduled in `roadmap.md`: milestone J0 (decompilation symbols,
native reading, DS saves, oracle) then, for each milestone, the "Reuse" column.
