# Ressources externes utiles au projet

Recensement fait le 4 novembre 2026 : projets, outils et documentation qui peuvent faire
gagner du temps à la rétro-ingénierie et au portage. Chaque entrée dit ce qu'elle apporte,
sa licence et comment s'en servir ici. Les liens et chiffres viennent des dépôts eux-mêmes
à cette date ; tout ce qui n'a pas été vérifié est marqué **[non vérifié]**.

## L'essentiel

1. **Une communauté DQ9 active fait déjà le même travail**, en public : l'organisation
   GitHub [DQI-haX](https://github.com/DQIX) (décompilation, wiki des formats, outils) et
   le Discord *The Quester's Rest* (<https://discord.gg/dqix>, fil « DQI-haX »).
2. **Les symboles de la décompilation USA s'appliquent à notre ROM européenne** par un simple
   décalage d'adresse (vérifié ci-dessous sur nos 193 symboles). 1 704 noms et les bornes de
   ~20 000 fonctions, sous licence Unlicense (domaine public).
3. **Le [wiki DQIX](https://github.com/DQIX/wiki) documente, sur la même ROM `YDQP`, une
   bonne partie de notre feuille de route** : triggers, scripts `.stb`, tables `.nat`,
   cartes, sprites, SDAT, NSBMD… À lire avant d'attaquer un format.
4. **[DQIX Enhanced](https://github.com/sackchief/dqix-enhanced)** (recompilation statique
   vers PC, GPL-3.0) a déjà résolu le multijoueur en ligne, le mode Tag, la boutique DQVC et
   les quêtes téléchargeables. Source de faits, **pas de code à copier** (voir Licences).
5. **[apicula](https://github.com/scurest/apicula)** est un lecteur/visualiseur NSBMD/NSBTX/
   NSBCA en Rust, sous 0BSD : ses lecteurs sont portés dans `nds_rom::g3d`
   ([`re/g3d.md`](re/g3d.md)), crédités en tête du module.

## Projets consacrés à DQ9

### dqix-decomp — décompilation « matching » (USA et JPN)

<https://github.com/DQIX/dqix-decomp> · Unlicense · outil [dsd](https://github.com/AetiasHax/ds-decomp)

- Reconstruit une ROM USA ou JPN identique à l'octet près à partir d'assembleur et de C++.
  Avancement au 4/11/2026 : 9,18 % des fonctions, 6,05 % des octets (USA).
- `config/usa/arm9/**/symbols.txt` : bornes et types (ARM/Thumb, taille) de toutes les
  fonctions et données, 1 704 noms explicites (1 584 dans le module ARM9 principal),
  souvent des noms C++ décorés (`_ZN7GPCFile12GetFileByCRCEj`) qui donnent les classes.
- Code déjà décompilé et directement utile au portage : générateur de grottes
  (`src/Grotto/Main/FloorMapGenerator.cpp`, `TreasureMapDataStructs.cpp`…), formules de
  combat (`src/Combat/…/BasicAttackCalculation.cpp`, `CritRateCalculation.cpp`,
  `TensionBonusCalculation.cpp`, `GetAttackBaseDamage.cpp`), GPC (`src/Filesystem/GPC.cpp`),
  aléatoire (`src/Util/Random.cpp`), temps de jeu, éclairage, sprites `.spr`, `.pac`, `.bcfg`,
  et une grande partie du moteur G3D de NitroSystem (`src/NitroSDK/NSBXX/`).
- `Decompiling.md` explique le flux (Ghidra + [dsd-ghidra](https://github.com/AetiasHax/dsd-ghidra)
  pour synchroniser les symboles, [decomp.me](https://decomp.me), objdiff).

#### Correspondance USA → Europe

Établie par `tools/region.py` de DQIX Enhanced et **vérifiée ici** : les 177 symboles du
module ARM9 et les 16 de l'overlay 17 de `re/symbols.txt` tombent tous sur un début de
symbole de `config/usa/arm9` une fois l'adresse convertie (`Gpc2_LoadIndex` ↔
`LoadAndDecompressGPCHeaderAndInnerFileInfo…`, `Gpc_FindEntryByHash` ↔ `GPCFile::GetFileByCRC`,
`Msg_Compile` ↔ `func_02069fec`…).

| Adresse USA (`YDQE`) | Adresse Europe (`YDQP`) |
|---|---|
| module ARM9 `0x0200F3B8..0x0200FB20` | + `0x8` |
| module ARM9 `0x0200FB20..0x020EEBD0` (code et table des constructeurs) | + `0x10` |
| reste du module ARM9 (données après le code), ITCM, DTCM, **tous les overlays**, ARM7 | identique |
| zone sécurisée `0x02000000..0x02000800` (stubs d'appels BIOS) | ordre différent : retrouver chaque stub par son numéro de `swi` |

Les 16 octets viennent de deux fonctions (`func_0200f3a4`, `func_0200fb08` côté USA) qui
choisissent la langue d'après les réglages de la console.

**Usage recommandé** : un convertisseur (script ou sous-commande `dq9re`) qui lit les
`symbols.txt` de dsd, applique le tableau ci-dessus, ne garde que les noms explicites (pas
`func_…`/`data_…`) et produit un fichier au format de `re/symbols.txt`, appliqué avant nos
propres noms par `ghidra-apply-symbols.ps1`. Les bornes de fonctions de la décompilation
complètent aussi les fonctions que l'analyse automatique de Ghidra ne trouve pas (code
atteint seulement par des tables de pointeurs).

### Wiki des formats DQIX

<https://github.com/DQIX/wiki> · **pas de licence** (réutiliser les faits en citant la page,
ne pas recopier le texte)

88 pages, observations faites sur la **ROM européenne `YDQP`** (la nôtre), adresses de code
souvent données en USA avec le nom de la décompilation. Même discipline que nos
`docs/re/` : « confirmé », « INFERRED », « Not established », preuves par fichier et offset.
Attention : les codes de triggers et de fonctions moteur y sont **en décimal** (notre
`trigger.md` les écrit en hexadécimal).

| Sujet | Pages du wiki |
|---|---|
| Triggers (fait chez nous, à confronter) | [Triggers](https://github.com/DQIX/wiki/blob/main/Triggers.md) : conditions et actions en décimal (41 = notre `0x29`, 119 = `0x77`), types d'événements (« Kinds »), zones orientées ; [Story-Threads](https://github.com/DQIX/wiki/blob/main/Story-Threads.md), [Time-Of-Day](https://github.com/DQIX/wiki/blob/main/Time-Of-Day.md) ; peut lever des **[sens inconnu]** de `trigger.md` |
| Scripts d'événements `.stb` et **natives** | [Event-Scripts](https://github.com/DQIX/wiki/blob/main/Event-Scripts.md) (machine à pile, bibliothèque partagée de 18 routines) ; **[Engine-Functions](https://github.com/DQIX/wiki/blob/main/Engine-Functions.md) : rôle de la plupart des natives 100–845** (personnages 200, caméra 300, messages 400, scène 500, son 700, fin 800) ; [Event-Lists](https://github.com/DQIX/wiki/blob/main/Event-Lists.md), [Event-Text](https://github.com/DQIX/wiki/blob/main/Event-Text.md) |
| Graphismes 3D et cartes | [NSBMD](https://github.com/DQIX/wiki/blob/main/NSBMD.md), [NSBTA-and-NSBMA](https://github.com/DQIX/wiki/blob/main/NSBTA-and-NSBMA.md), [NSBTP](https://github.com/DQIX/wiki/blob/main/NSBTP.md), [Map-List](https://github.com/DQIX/wiki/blob/main/Map-List.md), [Map-Archive](https://github.com/DQIX/wiki/blob/main/Map-Archive.md), [Map-Collision](https://github.com/DQIX/wiki/blob/main/Map-Collision.md), [Map-Objects](https://github.com/DQIX/wiki/blob/main/Map-Objects.md), [Map-Textures](https://github.com/DQIX/wiki/blob/main/Map-Textures.md) (`.bmbl`), [Doors](https://github.com/DQIX/wiki/blob/main/Doors.md), [Character-Parts](https://github.com/DQIX/wiki/blob/main/Character-Parts.md), [Motion-Tables](https://github.com/DQIX/wiki/blob/main/Motion-Tables.md) |
| Tables de jeu et combat | [Items](https://github.com/DQIX/wiki/blob/main/Items.md), [Monsters](https://github.com/DQIX/wiki/blob/main/Monsters.md) (`mon_btldata.nat`…), [Actions](https://github.com/DQIX/wiki/blob/main/Actions.md) (`actdt_a/b`), [Spell-Table](https://github.com/DQIX/wiki/blob/main/Spell-Table.md), [Articles](https://github.com/DQIX/wiki/blob/main/Articles.md) (flexion grammaticale des noms), [System-Strings](https://github.com/DQIX/wiki/blob/main/System-Strings.md) (format `.nat` texte), [Encounters](https://github.com/DQIX/wiki/blob/main/Encounters.md), [Battle-Resolution](https://github.com/DQIX/wiki/blob/main/Battle-Resolution.md), [Battle-AI](https://github.com/DQIX/wiki/blob/main/Battle-AI.md), [Alchemy-Recipes](https://github.com/DQIX/wiki/blob/main/Alchemy-Recipes.md), [Level-Tables](https://github.com/DQIX/wiki/blob/main/Level-Tables.md) |
| Interface (carte, boîte de dialogue, menus) | [Bitmap-Font](https://github.com/DQIX/wiki/blob/main/Bitmap-Font.md), [Sprites](https://github.com/DQIX/wiki/blob/main/Sprites.md) (`.spr`), [Menu-Backgrounds](https://github.com/DQIX/wiki/blob/main/Menu-Backgrounds.md), [2D-Graphics](https://github.com/DQIX/wiki/blob/main/2D-Graphics.md), [Mini-Map](https://github.com/DQIX/wiki/blob/main/Mini-Map.md), [Area-Cast](https://github.com/DQIX/wiki/blob/main/Area-Cast.md) (`.npc`) |
| Balises non localisées | [Text-Markup](https://github.com/DQIX/wiki/blob/main/Text-Markup.md) (table des balises, `<ADD>`, `<N_TURN>`, `<TIME=n>`, `<ME_n>`/`<SE_n>`…), [Text-Events](https://github.com/DQIX/wiki/blob/main/Text-Events.md) |
| Formats encore **[inconnus]** dans `rom-layout.md` | `.nat` (tables de jeu ci-dessus), `.npc` ([Area-Cast](https://github.com/DQIX/wiki/blob/main/Area-Cast.md)), `.pac` ([Pac](https://github.com/DQIX/wiki/blob/main/Pac.md)), tables étiquetées `.dat/.bats/.bcfg/.bpos/.bmed/.bmbl` ([Tagged-Data-Table](https://github.com/DQIX/wiki/blob/main/Tagged-Data-Table.md)) ; `.lia` non lu (cité comme inconnu dans Menu-Backgrounds) |

Ce que nous avons et qui manque au wiki (à proposer en retour) : le choix des répliques de PNJ
(`dialogue.md`), le pipeline des messages en 6 passes (`text.md`), la mise en page des polices
(`font.md`).

### DQIX Enhanced — portage PC par recompilation statique

<https://github.com/sackchief/dqix-enhanced> · GPL-3.0-or-later · v33 stable

Approche différente de la nôtre : le code ARM est traduit en C et le matériel DS est émulé
autour (rendu 3D, son et Wi-Fi d'après melonDS). Il fonctionne déjà de bout en bout, ce qui
en fait un **oracle** : même entrée → même résultat que la DS, mode « Classic » vérifié pixel
par pixel contre un émulateur. Faits documentés dans
[`docs/TECHNICAL.md`](https://github.com/sackchief/dqix-enhanced/blob/main/docs/TECHNICAL.md)
et [`ROADMAP.md`](https://github.com/sackchief/dqix-enhanced/blob/main/ROADMAP.md) :

- **Multijoueur en ligne** (mode Tag, visites via Pavo, 4 joueurs) : la DS exige des réponses
  radio en microsecondes, donc *chaque PC fait tourner les DS de tous les joueurs* ; seules
  les entrées circulent (TCP via l'hôte, port 27900, délai d'entrée choisi d'après le ping),
  avec des empreintes mémoire toutes les 2 s pour détecter une désynchronisation. Relais
  Cloudflare pour les NAT, UPnP/NAT-PMP/PCP. Une réimplémentation peut faire plus simple
  (protocole de haut niveau), mais doit reproduire les **données échangées** en mode Tag.
- **DQVC et quêtes téléchargeables** : serveur Wi-Fi Connection intégré. Fichiers chiffrés
  RC4 avec la clé `XENLONPROJECTKEY` ; les 4 derniers octets rendent le fichier chiffré,
  lu comme un nombre, divisible par `0xC2A030D4`. Les 64 quêtes et 23 invités spéciaux sont
  sur la cartouche, la connexion ne fait que les activer dans la sauvegarde. Calendrier des
  52 soldes DQVC (`runtime/dqvc_schedule.h`, construit depuis les listes de yab).
- La connexion de la DS à Nintendo passe en HTTPS ; les patchs « NoSSL » de la communauté
  basculent les adresses en `http://`, et le serveur de login doit renvoyer un en-tête `Date`.
  En octobre 2026, ni AltWFC ni Wiimmfi ne servaient plus DQIX.
- Sauvegarde : EEPROM 64 Ko, format DS conservé (compatible avec les éditeurs).
- Correctifs nécessaires au jeu lui-même : fuite mémoire de l'anti-piratage dans le chargeur
  de fichiers (gel après le premier combat), code anti-piratage chiffré exécuté par un
  interpréteur (dqix-functions situe la protection dans l'overlay 29 **[non vérifié]**).

### Autres projets DQI-haX

| Projet | Apport | Licence |
|---|---|---|
| [dqix-functions](https://github.com/DQIX/dqix-functions) | symboles JPN (format resymgen), carte des overlays avec leurs **noms internes** (`00 battle`, `01 event`, `17 gamemain`, `23 sub_menu`, `31 wifi`, `32 sound`…), glossaire des trois générateurs aléatoires (voir plus bas) | GPL-3.0 |
| [dqix-script-disasm](https://github.com/DQIX/dqix-script-disasm) | désassembleur/assembleur des scripts binaires d'initialisation (éclairage, atmosphère, contenu des coffres) vers Lua ; contextes par type de script | MIT |
| [ArchiveTool](https://github.com/DQIX/ArchiveTool) | extraction/reconstruction des `.gp2` : contre-vérification de `gpc2.rs` | MIT |
| [desmume-scripts](https://github.com/DQIX/desmume-scripts) | scripts Lua DeSmuME : tables aléatoires JPN/USA/EUR, affichage des hitboxes, collisions, nœuds d'apparition des monstres, coordonnées du joueur | aucune |
| [editor](https://github.com/DQIX/editor) ([en ligne](https://dqix.github.io/editor/)) | éditeur de sauvegarde complet ; `pattern.hexpat` décrit la sauvegarde pour ImHex : base pour la sauvegarde du portage | aucune |
| [Grotto-Searcher](https://github.com/DQIX/Grotto-Searcher) | génération des grottes à partir d'une carte au trésor (lien `?id=RRSSSS`) : oracle de tests pour un générateur réimplémenté | non précisée |
| [BattleEmulator](https://github.com/DQIX/BattleEmulator) | simulateur des combats en solo (C++/wasm), formules et tirages aléatoires fidèles au jeu : oracle pour `dq9-core` | MIT |
| [auction](https://github.com/DQIX/auction) | générateur du fichier DLC `auction.bin` (stock DQVC) | aucune |
| [Collapsus](https://github.com/DQIX/Collapsus) | base de données (recettes, quêtes, monstres, grottes) extraite des structures du jeu | GPL-3.0 |
| [Save-Editor-Decomp](https://github.com/DQIX/Save-Editor-Decomp) | décompilation de l'éditeur de sauvegarde historique d'asa-o | archivé |

Générateurs aléatoires (d'après le glossaire de dqix-functions) :

| Nom | Algorithme | Usages connus |
|---|---|---|
| AT | LCG 32 bits : `r = r * 0x41C64E6D + 0x3039` | apparitions de monstres, terrain, animations, butin des coffres |
| BT (« table Hoimi ») | LCG 64 bits : `s = s * 0x5D588B656C078965 + 0x269EC3` ; même graine qu'AT | alchimie critique, fuite, Zaoral, butin des boss, rencontres |
| CT | même LCG 64 bits, graine tirée à chaque combat | actions de combat, caméra |

## Outils génériques Nintendo DS

### Analyse du code

| Outil | Intérêt pour nous | Licence |
|---|---|---|
| [ds-decomp (`dsd`)](https://github.com/AetiasHax/ds-decomp) | boîte à outils de décompilation DS **en Rust** (crates [`ds-decomp`](https://crates.io/crates/ds-decomp), [`ds-rom`](https://lib.rs/crates/ds-rom)) ; `dsd init` produit une carte complète (fonctions, relocations, overlays) d'une ROM : c'est ainsi que DQIX Enhanced a cartographié la version européenne | MIT |
| [dsd-ghidra](https://github.com/AetiasHax/dsd-ghidra) | chargeur Ghidra + synchronisation des symboles d'une config `dsd` | MIT |
| [objdiff](https://github.com/encounter/objdiff), [decomp.me](https://decomp.me) | comparaison fonction par fonction ; utile seulement si l'on contribue à la décompilation | Apache-2.0 |
| [ndskit](https://github.com/vs-sr-dev/ndskit) | kit générique de portage DS (Python + C++) : découverte de fonctions ARM/Thumb, interpréteur, **comparaison de la mémoire et des canaux son avec une sauvegarde d'état melonDS** (`python -m ndskit.melonds STATE.ml1 --compare`) : base possible pour notre comparaison différentielle | MIT |
| [libntr](https://github.com/cybervisi0n/libntr) | remplaçant du NitroSDK pour PC (utilisé par le portage PC de Pokémon Platinum) : référence sur le comportement des bibliothèques Nintendo hors DS | MIT |
| [pret/pokeplatinum](https://github.com/pret/pokeplatinum), [pret/pokediamond](https://github.com/pret/pokediamond) | noms et code des fonctions NitroSDK/NitroSystem, pour reconnaître les bibliothèques dans notre binaire (SDK `0x04027539`) | aucune |

### Émulateurs et analyse dynamique

- **melonDS** (déjà installé) : stub GDB. Sa recherche (rendu 3D, son, Wi-Fi) est la
  référence suivie par DQIX Enhanced. GPL-3.0.
- **DeSmuME** : débogueur intégré et **scripts Lua** ; les tables aléatoires des scripts
  DQI-haX ci-dessus existent pour la version européenne, les autres scripts sont surtout JPN.
- **No$GBA** : débogueur ; [GBATEK](https://problemkaputt.de/gbatek.htm) reste la référence
  du matériel (moteur 3D, formats `BMD0/BTX0/BCA0` dans « DS Files - 3D Video »).

### Formats Nintendo standard

| Besoin | Ressource | Licence |
|---|---|---|
| Modèles/textures/animations Nitro (étape 3) | [apicula](https://github.com/scurest/apicula) (Rust : lecture, visualiseur, export glTF/COLLADA) et sa doc [nsbmd_docs](https://github.com/scurest/nsbmd_docs) | 0BSD |
| Rendu NNS G3D dans un navigateur | [noclip.website `src/nns_g3d`](https://github.com/magcius/noclip.website/tree/main/src/nns_g3d) (TypeScript, Mario Kart DS, Pokémon Platinum/HGSS) | personnalisée, à vérifier |
| Son SDAT (crate `dq9-audio`) | [doc SDAT de ndspy](https://ndspy.readthedocs.io/en/stable/appendices/sdat-structure.html), [spécifications Nitro Studio 2](https://gota7.github.io/NitroStudio2/specs/), page [SDAT](https://github.com/DQIX/wiki/blob/main/SDAT.md) du wiki DQIX (les trois `.sdat` du jeu) ; séquenceurs [SSEQPlayer](https://github.com/kode54/SSEQPlayer) (C++) et [FSS](https://github.com/fincs/FSS) | WTFPL |
| Lecteur SDAT moderne | [DualRip](https://github.com/TetraSsky/DualRip) (émule le pilote son DS) | MIT |
| Vidéos Mobiclip (`.mods`, 6 fichiers) | FFmpeg (décodeur `mobiclip`, démultiplexeur [`mods`](https://www.ffmpeg.org/doxygen/7.0/mods_8c.html)) ; [documentation des formats Mobiclip](https://gist.github.com/quatric/3e2f0caa5c22a9a8b24d9cf1ffdfe860) | LGPL (FFmpeg) |
| Compression Level-5 / formats Level-5 en général | [Kuriimu2](https://github.com/FanTranslatorsInternational/Kuriimu2) | GPL-3.0 |

## Serveurs en ligne et contenu téléchargeable

- [DLS1-Files](https://github.com/InternalLoss/DLS1-Files) (miroir
  [GitLab](https://gitlab.com/internalloss/dls1-files)) : fichiers servis par le serveur de
  téléchargement de Nintendo (`dls1.nintendowifi.net`), conservés par la communauté, dont
  ceux de DQIX par région (`YDQE`, `YDQP`, `YDQJ` : quêtes, stock DQVC, message de
  nouvelles). Ce sont des données du jeu : à lire depuis une copie du joueur, jamais à
  redistribuer.
- Protocoles NAS/DLS1 : [wiki Tockdom](https://mkwiiki.org/wiki/DLS1).
- Wiimmfi et AltWFC : remplaçants de Nintendo WFC, mais ne servaient plus DQIX en octobre
  2026 (source : DQIX Enhanced).

## Mécaniques de jeu et données de référence

Utiles pour écrire des tests ou des invariants sur la vraie ROM :

- [Dragon Quest Wiki](https://dragon-quest.org/wiki/Dragon_Quest_IX:_Sentinels_of_the_Starry_Skies)
  (dragon-quest.org) et [Fandom](https://dragonquest.fandom.com/) : listes d'objets, monstres,
  emplacements des grottes.
- GameFAQs : [Grotto Mechanics Guide](https://gamefaqs.gamespot.com/ds/937281-dragon-quest-ix-sentinels-of-the-starry-skies/faqs/61151)
  (génération des grottes, bug des monstres errants).
- [Dragon's Den](https://www.woodus.com/den/games/dq9ds/) : recettes, butins des boss
  légendaires, éditeur de sauvegarde historique (asa-o, v0.7).
- Japonais : [blog d'analyse de daisukedaisuke](https://daisukedaisuke.hatenablog.com/)
  (rencontres, tables d'accompagnateurs, spécifications non documentées),
  [article Qiita sur la table Hoimi](https://qiita.com/DaisukeDaisuke/items/9d829d4d2878d8ab82c4),
  [ドラゴンクエスト9攻略Wiki](https://dragonquest9.com/), [hossy.info/dq9](https://hossy.info/dq9).

## Licences : ce qu'on peut intégrer

Le dépôt est sous `MIT OR Apache-2.0`.

| Statut | Projets | Règle |
|---|---|---|
| Intégrable (avec mention) | dqix-decomp (Unlicense), apicula (0BSD), ds-decomp/ds-rom, dsd-ghidra, ndskit, libntr, dqix-script-disasm, ArchiveTool, BattleEmulator, DualRip (MIT), objdiff (Apache-2.0), SSEQPlayer/FSS (WTFPL) | code et données réutilisables ; citer la source |
| Lecture seule | DQIX Enhanced, melonDS, Kuriimu2, dqix-functions, Collapsus (GPL-3.0), FFmpeg (LGPL) | apprendre les faits, réimplémenter soi-même ; ne pas copier ni traduire le code |
| Sans licence | wiki DQIX, editor, desmume-scripts, auction, pret | faits réutilisables en citant la page ; ne pas recopier texte ou code |

Les faits sur le jeu (offsets, formules, constantes) ne sont pas protégés par ces licences ;
le code et la rédaction le sont. Dans `docs/re/`, citer la source externe d'un fait et le
vérifier sur la ROM comme nos propres découvertes.

## Mise en œuvre

Ces ressources sont planifiées dans `roadmap.md` : jalon J0 (symboles de la décompilation,
lecture des natives, sauvegardes DS, oracle) puis, pour chaque jalon, la colonne « Réutiliser ».
