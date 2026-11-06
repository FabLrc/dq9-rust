# Sauvegarde DS (`.sav`)

Module `dq9_formats::save`, commande `dq9re save <fichier> [-v]` (sans ROM).

## Sources et vérification

1. Disposition des champs : éditeur de sauvegarde [DQIX/editor](https://github.com/DQIX/editor)
   (`src/game/layout.js`, `src/saveManager.js`, `pattern.hexpat`, **sans licence** : lu,
   réimplémenté, rien copié ; sa sauvegarde de démonstration n'est pas versionnée).
2. **Code du jeu** (ARM9 européen, lu dans Ghidra) : le sous-système de sauvegarde occupe
   `0x020a94f8..0x020ac9bc`. Il confirme la structure ci-dessous et donne la table complète
   des blocs (§ « Sérialisation »). Noms dans `re/symbols.txt`, section *Save file*.
3. Contrôle sur la sauvegarde de démonstration (64 Kio) : les 6 sommes se recalculent ;
   réécrire (recalcul des sommes) redonne le fichier octet pour octet ; version `save013`,
   chapitre 19, étape 2, sous-étape 1, héros Sage niveau 91, 98 h 32 min 44 s, 38 754 or.

Les décalages `save +0x…` de `re/symbols.txt` (`Save_Chapter`…) sont ceux de `GameState` en
RAM, **pas** ceux du fichier : `Save_Serialize` copie des blocs de `GameState` vers des
décalages fixes du fichier. Exemple : étape du scénario en `GameState+0x5cb4`, en `slot+11572`
dans le fichier.

## Conteneur

Image EEPROM de **0x10000 octets** = 2 emplacements de **0x8000** (0x6fe4 octets utiles chacun).

| Décalage | Contenu | Preuve |
|---:|---|---|
| fichier 0 | `DRAGON QUEST IX` + octet 0, **une seule fois pour tout le fichier** | `Save_CheckMagic` (`0x020ac920`) lit 16 octets à 0, compare, sinon efface et réécrit |
| slot+16 | CRC A, couvre `+20..+36` | `Save_ReadHeaderA` (`0x020a9508`) : lit 0x14 octets à `+0x10`, CRC-32 des 0x10 suivants |
| slot+20 | octet « intermission » (1 = sauvegarde de suspension) | `Save_SlotStatus` (`0x020ab9d0`) : statut 4 |
| slot+21 | octet « existe » (0 = emplacement vide) | même fonction : statut 3 |
| slot+24 | version `save013\0` (`save011` est rejeté) | `Save_ReadHeaderA`, `Save_WriteHeaderA` (`0x020aba6c`) |
| slot+36 | CRC C, couvre `+40..+132` (0x5c octets) | `Save_ReadHeaderC` (`0x020ac874`) ; les 0x54 octets `+36..+120` (**CRC compris**) sont copiés vers `GameState+0x6380` (vu en RAM) |
| slot+132 | CRC B, couvre `+136..+28644` (0x6f5c octets) | `Save_LoadSlot` (`0x020aaf94`) : lit 0x6f60 octets à `0x84` / `0x8084` |
| slot+136 | personnages (13 × 572) puis le reste (§ suivant) | `Save_Serialize` (`0x020a9ec8`) |

CRC-32 standard (IEEE, valeur initiale 0). Statuts de `Save_SlotStatus` : 0 valide, 1 illisible,
2 CRC A faux, 3 vide, 4 intermission ; en mode normal, la touche testée à `0x2000` (maintenue)
force 2 [inconnu : sans doute une réinitialisation volontaire].
Une erreur de chargement pose `GameState+0x5cc8` à 2 (emplacement 0) ou 3 (emplacement 1).

**Le magic n'est pas par emplacement.** L'emplacement 1 de la démo est la copie de
l'emplacement 0 sauf les 16 premiers octets (`0xff`), ce qui s'explique : le magic n'existe
qu'à l'offset 0 du fichier.

## Rôle des deux emplacements et protocole d'écriture

`Save_LoadSlot` (`0x020aaf94`) reçoit `intermission` (0 ou 1) : **emplacement 0 = sauvegarde
normale, emplacement 1 = sauvegarde de suspension** (« intermission »). Il lit l'en-tête à
`0x10` ou `0x8010`, les blocs à `0x84` ou `0x8084`. Un échec de somme B pose
`GameState+0x5cc8` à 2 (emplacement 0) ou 3 (emplacement 1) ; il n'y a **pas de repli**
automatique sur l'autre emplacement dans cette fonction.

`Save_WriteStep` (`0x020aad2c`, un état par appel, `*param_1` = état) :

| État | Action |
|---:|---|
| 0 | teste le périphérique (lit 1 octet à 0) ; **lit tout l'emplacement 1** (`0x8000`, `0x6fe4` octets) dans le tampon (pour garder ce que la sérialisation ne réécrit pas) → 1 |
| 1 | attend la fin, `Save_Serialize`, puis écrit `0x6fe4` octets à `0x10` (emplacement 0, sauvegarde normale) ou à `0x8010` (suspension) → 2 |
| 2 | sauvegarde de suspension : terminé. Sauvegarde normale : réécrit **la même image à `0x8010`** → 3 |
| 3 | attend la fin, relâche les verrous → 4 (terminé) |

Une sauvegarde normale écrit donc **deux copies identiques** (emplacement 0 puis 1), ce qui
explique la démo (emplacement 1 = copie de 0). L'octet intermission de l'en-tête n'est à 1
que dans la copie écrite par une sauvegarde de suspension ; seul `Save_SlotStatus` le lit.
`Save::current_slot` prend le premier emplacement de statut valide (0 avant 1).

## Sérialisation (`Save_Serialize`, `0x020a9ec8`)

Les offsets de la fonction sont relatifs à un tampon décalé de `-0x84` : ajouter 0x84 pour
l'offset d'emplacement. Les 16 blocs ci-dessous **pavent exactement** la zone de la somme B
(`136..28644`, test `blocks_tile_checksum_b_exactly`) : taille = distance au bloc suivant,
recoupée avec les longueurs de copie du code. Accès brut : `Slot::block(Block)`.

| `Block` | Offset | Taille | Contenu | Lecture dans le jeu |
|---|---:|---:|---|---|
| `Main` | 136 | 11432 | personnages (13 × 0x23c), effectifs, ordre du groupe, objets portés, or, banque, médailles, zone, réglages | `Save_UnpackMain` (`0x020a95b4`) |
| `Progress` | 11568 | 836 | 4 mots (chapitre, étape, sous-étape, 1 mot) + bloc d'événements (0x334) | `Save_UnpackProgress` (`0x020a9a60`) |
| `QuestLog` | 12404 | 160 | `QuestLog_Get()` | `memcpy` direct |
| `QuestTimers` | 12564 | 816 | `QuestLog+0x178` | `memcpy` direct |
| `Hints` | 13380 | 404 | 1 octet (`func_020120d4`) + 400 octets → `GameState+0x5cdc` | `Save_UnpackHints` (`0x020a9b08`) |
| `Monsters` | 13784 | 1232 | `GameState+0x75f0` | `memcpy` direct |
| `Alchemy` | 15016 | 948 | `GameState+0x7ac0` | `memcpy` direct |
| `Accolades` | 15964 | 236 | `GameState+0x7504` | `memcpy` direct |
| `Zone` | 16200 | 6976 | 0x1b30 octets de l'objet de zone (`func_02012fe4()+0x840`), puis 5 champs en `+0x1b30..+0x1b3f` (→ `+0x2374..0x23a1`) | `Save_UnpackZone` (`0x020a9b3c`) |
| `DqvcMessages` | 23176 | 1028 | 2 × 512 octets → `GameState+0x5f80`, `+0x6180` ; 1 mot → `+0x5f7c` | `Save_UnpackDqvcMessages` (`0x020a9bb4`) |
| `TreasureMaps` | 24204 | 2776 | 0xad6 octets → `GameState+0x64f4` : 99 cartes de 0x1c octets (`TreasureMapMetadata`), après un en-tête de 2 octets ; reconstruit ensuite la grotte courante | `Save_UnpackTreasureMaps` (`0x020a9c24`) |
| `Profile` | 26980 | 124 | `GameState+0x569c` | `memcpy` direct |
| `Warp` | 27104 | 28 | destination de la reprise : carte u16, angle s16, position (3 × fx32), § « Position à la reprise » ; **sauvegarde de suspension seulement** | `Save_UnpackWarp` (`0x020a9d88`) |
| `Dqvc` | 27132 | 708 | `GameState+0x71fc` | `memcpy` direct |
| `Misc` | 27840 | 272 | `GameState+0x5e6c` | `memcpy` direct |
| `Suspend` | 28112 | 532 | suspension : 0x200 octets → `GameState+0x6fcc`, 10 octets (`+0x204`) → `+0x5cd0`, un mot (`+0x200`) ; sauvegarde normale : le même bloc se lit comme `u16` présence, `u16` n, puis n × 0x14 octets → `func_02010828()+0xf2c` (3 au plus) | `Save_UnpackSuspend` (`0x020a9e40`) |

`Main` en détail (offsets d'emplacement) : personnages `136 + 572 n` ; `7572` personnages en
réserve ; `7573` ordre du groupe (4) ; `7577` effectif du groupe ; `7578 + 18 n` objets portés
(8 × u16 + 2 octets) ; `11448` or en main ; `11452` or en banque ; `11460` mini-médailles ;
`11476` id de carte courante (109 dans la démo ; `Save_LoadSlot` le copie à
`GameState+0x7e74`, et la carte observée `C01M09` porte l'id 109) ; `7652` sac (0x1d4 octets,
voir « Inconnu ») ; réglages et données de départ `11488..11519` ; drapeau d'intermission
`11530` (tampon `+0x2ca2`).

La somme C (`40..132`) ne couvre pas ces blocs : elle protège le petit bloc copié vers
`GameState+0x6380` (avec le mot de CRC lui-même).

## Position à la reprise

`Save_LoadSlot` ne place personne : il remplit `GameState`. Le groupe est posé plus tard par
`Party_Place` (`ov_017::0219c598`), appelée à l'état 3 de la machine d'entrée de carte
(`Field_EnterMap`, `ov_017::0219e384`), après le chargement de la carte. Sans position
explicite, elle choisit :

- **octet `Save_Warp+7` nul** (sauvegarde normale, nouvelle partie) : le **point de départ de
  la carte**, l'enregistrement `0x6e` de son `.bmbl` (`BlockList_RecStart`, `0x0201d4a4`) :
  trois flottants × 4096 → `BlockList+0x70` (= `g_Zone3D+0xdc`), un quatrième × 4096, ramené
  à `0..2π` (`0x02030f40`), → `+0x7c` (= `g_Zone3D+0xe8`) ; lus par `BlockList_GetStart`
  (`0x0201e81c`) et `BlockList_GetStartAngle` (`0x0201e830`) ;
- **octet `+7` non nul** (sauvegarde de suspension, posé par `Save_UnpackWarp`) : le vecteur
  `Save_Warp+0x10` et l'angle (`s16`) `+0x1c`, soit les champs du bloc `Warp` ci-dessous.

Chaque membre du groupe (ordre `GameState_Party+0xf78`, effectif `+0xf7c`) est posé par
`Object_Place` (`0x020399c0`) : carte (`Object3D` champ 6), position `+0x44`, angle `+0x54`,
sans recalage au sol. L'état 9 de `Field_EnterMap` copie ensuite la position du héros en
`GameState+0x7f60` (vu en RAM).

Bloc `Warp` (`slot+27104`, 28 octets, lu par `Save_UnpackWarp` `0x020a9d88`) :

| Décalage | Type | Destination | Sens |
|---:|---|---|---|
| +0 | u16 | `Save_Warp+0` | id de la carte |
| +2 | s16 | `Save_Warp+0x1c` | angle, fx32 radians |
| +4 | 3 × fx32 | `Save_Warp+0x10` | position dans le monde |
| +0x10, +0x12 | u16 | deux champs de `g_Zone3D` (`0x0201bc88`, `0x0201bc7c`) | [inconnu] |
| +0x14 | u8 | bit 3 d'un drapeau de la zone | [inconnu] |
| +0x15 | u8 | `GameState+0x7e7e` | [inconnu] |
| +0x16, +0x18, +0x1a | u16 | `GameState+0x7e78..0x7e7c` | [inconnu] |

Sur une sauvegarde **normale** ce bloc contient des octets périmés que personne ne lit
(démo : `21 4e f7 57…`) ; `Slot::warp` ne le décode que pour une sauvegarde de suspension.

**Vérifié sur la RAM** (`Party_Place` sans position, octet `+7` nul) : sur la démo, l'état
`before.ml1` (carte 109 `C01M09`) a les quatre membres en (860 ; 491 ; −5 652) en fx32 avec
l'angle `0x323d` en `+0x54`, exactement l'enregistrement `0x6e` de `C01M09.bmbl`
(0,21 ; 0,12 ; −1,38 ; 3,14) lu par notre code, et ce même triplet est en `g_Zone3D+0xdc`
(`0x020fb4cc`). Même sauvegarde, id de carte patché à 100 (`C01`, sommes recalculées) puis
chargée dans melonDS : le héros est en x = −819, z = 113 664, angle `0x3245`, identiques à
l'enregistrement de `C01.bmbl` (−0,2 ; 0,1 ; 27,75) ; la hauteur est 821 au lieu de 409 :
le jeu pose le héros `0x199` au-dessus du sol qu'il trouve (`Zone3D_GroundHeight`) après
coup ; sur le sol de l'église la hauteur écrite est déjà celle-là (0,12 = 0,02 + 0,1).
Test : `dq9-core/tests/real_rom.rs`, `save_start_matches_melonds_ram` et
`patched_save_start_matches_melonds_ram` (`DQ9_SAVE`, `DQ9_STATE`, `DQ9_STATE_MAP100`).

**Non vérifié sur la RAM** : la sauvegarde de suspension (aucune sauvegarde de ce type, et le
script de `melonds-state.ps1` ne pilote que « Continue ») ; l'état 8 de `Field_EnterMap`
peut encore remplacer la position du héros (`ov_017::021a01f4`, table indexée par carte et
moment de la journée) **[inconnu : quelles cartes]** ; une carte sans enregistrement `0x6e`
place le groupe en (0, 0, 0).

## Heure et temps de la journée

L'en-tête A porte l'heure : un `f32` en `slot+32` (secondes, jour de 420 s ;
`Save_Serialize` y écrit `GameState::GetDayTimer`). `Save_LoadSlot` le passe à
`GameState::SetDayTimer` (`0x02010298`, `GameState+0x3cc`), qui dérive le moment (`+0x3dc`) :
nuit 0 (< 180 s), matin 1 (≥ 180), jour 2 (≥ 210), soir 3 (≥ 390) ; seuils lus en RAM
(`0x020f33b8..0x020f33c4` : 420, 390, 210, 180). La démo : 42,649 s, nuit, identique en RAM ;
le drapeau « horloge en marche » (`+0x3d8`, 1 en RAM) n'est pas sauvegardé. `dq9_core::state`
porte `SetDayTimer`, `AdvanceDayTimer` (`+0x3c4` ticks × `+0x3d4` = 1/60 s), `SetTimeOfDay`.

## En RAM après un chargement

Contrôlé sur un état melonDS pris dans le jeu après « Continue » sur la démo
(`dq9re state … --save`, [`melonds-state.md`](melonds-state.md)) : chaque copie simple de
`Save_LoadSlot` (`0x020aaf94`) et de ses fonctions se retrouve **octet pour octet** à
l'adresse lue dans le code. Table complète : `melonds::SAVE_COPIES`.

| Données | Destination | Vu |
|---|---|---|
| en-tête C (`slot+36`, 0x54) | `GameState+0x6380` | identique |
| sac (`7652`, 0x1d4), puis `+0x2b08` (0x128) | `GameState_Party` (`0x02010838` → `GameState+0x2a04`) `+0`, `+0xe04` | identiques |
| banque, or, mot `11456`, mini-médailles | `GameState+0x396c`, `+0x3970`, `+0x3974`, `+0x3978` (partie `+0xf68..+0xf74`) | identiques |
| ordre (4) et effectif du groupe | `GameState+0x397c`, `+0x3980` | identiques |
| id de carte (`11476`, u16) ; `11490`, `11491` | `GameState+0x7e74` (également partie `+0x2c94`) ; partie `+0xf7d`, `GameState+0x63dc` | identiques ; l'id 109 correspond à la carte `C01M09` dans la liste |
| `11492`, `11494` (u16, ordre inversé), vecteur `11496` (3 × fx32) + mot `11508`, `11512`, `11516`, `11519` | `g_Zone3D+0x2786`, `+0x2784`, `+0x2774..+0x2784`, `+0x2794`, `+0x2788`, `+0x27d0` | identiques ; le vecteur n'est pas la position du héros à la reprise (§ « Position à la reprise ») |
| membres du groupe (572 octets chacun) | convertis par `PartyMember_LoadRecord` (`0x02082d7c`) vers `GameState_PartyMember` (`GameState+0x474 + i×0x964`), § suivant | tous les champs comparés identiques |
| personnages en réserve | `GameState+0x3984` (n × 572, copie directe) | démo : 0 en réserve |
| mots de progression (16) | `GameState+0x5cb0` | identiques |
| bloc d'événements | `g_QuestManager` (`0x02108844`, rendu par `0x0205ec44`) | identique sauf `+0x180..+0x1cf` remis à zéro |
| `QuestLog`, `QuestTimers` | `g_QuestLog` (`0x02109418`), `+0x178` | identiques |
| `Hints` (`+4`, 400) | `GameState+0x5cdc` | 180 octets changés par le jeu |
| `Monsters`, `Alchemy`, `Accolades`, `TreasureMaps`, `Profile`, `Dqvc`, `Misc`, messages DQVC | `GameState+` décalages du tableau ci-dessus | identiques |
| `Zone` (0x1b30) | `g_Zone3D+0x840` (`0x020fb3f0`, rendu par `0x02012ff4`) | identique |

Toute la sauvegarde chargée est en `.bss` de l'ARM9 (`0x020f2e60..0x021536e0`) : adresses fixes.
Le tampon de lecture de l'emplacement est aussi en RAM (`slot+0x84` en `0x0214633c`) : une
recherche d'octets de la sauvegarde y tombe d'abord.

### Membres du groupe

`Save_UnpackMain` traite le i-ᵉ personnage du groupe (fiche `standby + i`) : il cherche parmi
les 4 objets membres celui dont le mot `+0x568` vaut `ordre[i]` (sinon un objet libre, mot
négatif), y charge la fiche, écrit `ordre[i]` en `+0x568`, copie les 8 objets portés
(`7578 + 18 i`, 16 octets) en `+0x454`, puis retire l'objet `0x5612` des objets portés
(`PartyMember_DropHeldItem`, `0x020838d4` : la première copie devient `0xffff`, puis **une
seule passe** décale chaque trou d'un rang) et du sac (`Bag_Remove(…, 0x5612, 99)`). Le membre
d'id 0 (le héros sur la démo) reçoit 5 en `+0x94c`. L'ordre du groupe n'est donc **pas** une
liste d'indices de fiches : c'est l'id de l'objet membre de chaque position.

`PartyMember_LoadRecord` (fiche → objet membre) :

| Fiche | Objet membre | Sens connu |
|---|---|---|
| `+1` bits 0-3 | `+0x56a` | [inconnu] |
| `+1` bit 4 | `+0x78` bit 30 | [inconnu] |
| `+1` bits 5-7, `+0x13f` bits 0-2 | `+0x56b` bits 0-2, 4-6 | [inconnu] |
| `+2` (13 octets) | `+0x16c` (13 × u16) | niveaux par vocation |
| `+0xf` (13) | `+0x186` | renaissances |
| `+0x1c` (13 × u32) | `+0x138` | expérience |
| `+0x50` (u32), `+0x54` (u16) | `+0x950`, `+0x954` | vocation courante (sert à charger le fichier de la vocation, `0x02082fd4`) ; [inconnu] |
| `+0x58` (13 × 12) | `+0x850` | graines |
| `+0xf4` (u16) | `+0x564` | points libres |
| `+0xf6` (27), `+0x111` (9), `+0x11a` (0x24) | `+0x464`, `+0x910`, `+0x8ec` | compétences / maîtrises [hypothèse] |
| `+0x13e` bits 0-2 | `+0x18` bits 0-2 (posés, jamais effacés) | [inconnu] |
| `+0x13f` bits 3-7 | `+0x94c` | [inconnu] (5 sur la démo) |
| `+0x140` (nom) | `+0x3c`, texte (indices de glyphes convertis par `func_02042774`) | nom |
| `+0x154` (u32) | `+0x1c` = bits 10..19, `+0x1e` = bits 20..29 | [inconnu] |
| `+0x160` (0x1c) | `+0x488` | équipement et apparence |
| `+0x17c` (0xc0) | `+0x4a4` | [inconnu] |

Contrôlé sur la démo : les 4 membres, tous ces champs sauf le nom et les bits de `+0x18`
(`melonds::State::compare_party`, `dq9re state --save`). Les noms sont bien en ASCII en
`+0x3c` (« Adenine »…).

### Écarts dus au jeu

[inconnu : quelle fonction]

- Drapeaux globaux `+0x180..+0x1cf` du bloc d'événements (indices 1 952 à 2 591, 80 octets)
  remis à zéro dans le jeu (la démo en avait 211 posés). [hypothèse] drapeaux temporaires
  de zone, effacés au chargement d'une carte.
- `Hints` : des mots `1e281ea2` deviennent `012800a2` (octets 0 et 2 réécrits).

L'écran titre lit déjà l'emplacement (journal d'aventure) et pose chapitre/étape/sous-étape
dans `GameState`, sans le reste : un état pris sur l'écran titre a le bon scénario mais ni
l'or ni les bestiaires (`ov_004` chargé au lieu de `ov_003`).

## Bloc d'événements (`slot+11584`, 0x334 octets)

`func_020a99d0` le copie en cinq morceaux (`0x8c`, `0x234`, `0xc`, `0x66` octets, puis 1) ;
`func_020a9a50` fait l'inverse et fixe d'abord chapitre/étape/sous-étape courants
(`0x02010774`, `0x020107a8`, `0x020107dc`, `0x02010810`). Le bloc est le début du gestionnaire
de quêtes de `trigger.md` : ses décalages valent tels quels.

| Décalage | Taille | Contenu |
|---:|---:|---|
| `0x000` | 5 × 0x1c | pistes : octets chapitre, étape, sous-étape, 13 octets de drapeaux de chapitre, 12 octets de drapeaux d'étape |
| `0x08c` | 0x234 | drapeaux globaux (4 512 bits, bit de poids faible d'abord : `Flags_Test`) ; l'indice `>= 0x400` des scripts est décalé de `0x6fa` (`Flags_TestExt`) |
| `0x2c0` | 0xc | [inconnu] |
| `0x2cc` | 0x66 | 204 quêtes, deux quartets par octet (état et drapeaux, voir `dialogue.md`) |
| `0x332` | 1 | piste courante (0..4) |

Vérifié sur la démo : piste courante 0, avec 19 / 2 / 1 = chapitre/étape/sous-étape des mots
d'en-tête ; 1 618 drapeaux globaux posés. Accesseurs : `Slot::event_flags`, `EventFlags`,
`Track`, `test_bit`.

Une **nouvelle partie** ne part pas d'un bloc vide : les drapeaux de scène
(`EventTask_Update`) et de carte visitée (`Flags_SetMapVisited`) sont posés par le moteur
avant et pendant le prologue, sans passer par un chargement de sauvegarde. Mécanismes,
preuves en RAM et drapeaux encore inexpliqués : [`trigger.md`](trigger.md), § « Drapeaux
globaux posés à l'ouverture ».

## Champs typés par `dq9_formats::save`

Voir les accesseurs de `Slot` et `Character` ; tous sont vérifiés sur la démo. Personnage
(572 octets) : niveaux `+2` (13 octets, vocation 0 = Gardien), renaissances `+15`, expérience
`+28` (13 × u32), vocation `+80`, graines `+88` (12 octets par vocation : 3 mots de 3 × 10
bits : force, agilité, résistance / adresse, charme, guérison magique / puissance magique,
PV max, PM max), points libres `+244`, maîtrises `+282`, nom `+320` (10 octets, **un octet par
caractère = indice dans la table de glyphes de la police `me`** (`font.md`), terminé par 0 :
`12 2f 30 39 34 39 30` = A d e n i n e ; la démo donne quatre noms lisibles), équipement
(u16) torse `+352`, jambes `+354`, bras `+360`, pieds `+362`, tête `+364`, arme `+366`, bouclier
`+368`, accessoire `+370`, visage `+356`, coiffure `+358`, teint/yeux/sexe `+372`, cheveux
`+373`, morphologie `+376`/`+378`.

## Inconnu

- Noms : voir plus haut (indices de glyphes) ; les caractères hors police `me` ne sont pas
  contrôlés.
- Signification exacte du vecteur `11496` : copié en `g_Zone3D+0x2774`, ce n'est **pas** la
  position du héros à la reprise (§ « Position à la reprise »). **[hypothèse]** point de
  retour sur la carte de campagne (l'id `11492` vaut 20002 = `F02` sur la démo, le vecteur
  (−57 ; 0,1 ; −31,5) est une position de cette carte).
- Bits de `lieux visités` (`0xfe77ffff` sur la démo, l'éditeur ne nomme que les bits 1..=18).
- Sens des drapeaux eux-mêmes (indices) : le bloc est typé, pas les bits. Le bloc `+0x2c0`
  (12 octets) n'a pas de rôle établi.
- Contenu interne des blocs `QuestLog`, `QuestTimers`, `Hints`, `Monsters`, `Alchemy`,
  `Accolades`, `TreasureMaps`, `Profile`, `Dqvc`, `Misc` : tailles et destinations en RAM
  établies, **champs non décodés**. Le sac (`Main`+7652) idem.
- Le protocole d'écriture est compris (§ « Rôle des deux emplacements ») mais **pas porté** :
  il pilote le matériel (`Save_BackupWrite`) et un moteur d'état de chargement, donc aucun
  marqueur `port:` ; `dq9_formats::save` écrit seulement l'image (`Save::seal`).
- Nombre et ordre des allocations de compétence (`+247`).
