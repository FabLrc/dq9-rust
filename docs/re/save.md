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
| slot+36 | CRC C, couvre `+40..+132` (0x5c octets) | `Save_ReadHeaderC` (`0x020ac874`) ; 0x54 octets copiés vers `GameState+0x6380` |
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
| `Warp` | 27104 | 28 | destination (carte u16, vecteur à `+4`, drapeau), **sauvegarde de suspension seulement** | `Save_UnpackWarp` (`0x020a9d88`) |
| `Dqvc` | 27132 | 708 | `GameState+0x71fc` | `memcpy` direct |
| `Misc` | 27840 | 272 | `GameState+0x5e6c` | `memcpy` direct |
| `Suspend` | 28112 | 532 | suspension : 0x200 octets → `GameState+0x6fcc`, 10 octets (`+0x204`) → `+0x5cd0`, un mot (`+0x200`) ; sauvegarde normale : le même bloc se lit comme `u16` présence, `u16` n, puis n × 0x14 octets → `func_02010828()+0xf2c` (3 au plus) | `Save_UnpackSuspend` (`0x020a9e40`) |

`Main` en détail (offsets d'emplacement) : personnages `136 + 572 n` ; `7572` personnages en
réserve ; `7573` ordre du groupe (4) ; `7577` effectif du groupe ; `7578 + 18 n` objets portés
(8 × u16 + 2 octets) ; `11448` or en main ; `11452` or en banque ; `11460` mini-médailles ;
`11476` zone (mot courant, ou `0x6d` pour le point de départ) ; `7652` sac (0x1d4 octets, voir
« Inconnu ») ; réglages et position de départ `11488..11519` ; drapeau d'intermission
`11530` (tampon `+0x2ca2`).

La somme C (`40..132`) ne couvre pas ces blocs : elle protège le petit bloc de 0x54 octets
copié vers `GameState+0x6380`.

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
| `0x2cc` | 0x66 | [inconnu] |
| `0x332` | 1 | piste courante (0..4) |

Vérifié sur la démo : piste courante 0, avec 19 / 2 / 1 = chapitre/étape/sous-étape des mots
d'en-tête ; 1 618 drapeaux globaux posés. Accesseurs : `Slot::event_flags`, `EventFlags`,
`Track`, `test_bit`.

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
- Numérotation de la zone (`109` sur la démo) ; sens des mots de position.
- Bits de `lieux visités` (`0xfe77ffff` sur la démo, l'éditeur ne nomme que les bits 1..=18).
- Sens des drapeaux eux-mêmes (indices) : le bloc est typé, pas les bits. Les blocs de
  `+0x2c0` (12 octets), `+0x2cc` (0x66 octets) et l'octet `+0x332` hors sélecteur de piste
  n'ont pas de rôle établi.
- Contenu interne des blocs `QuestLog`, `QuestTimers`, `Hints`, `Monsters`, `Alchemy`,
  `Accolades`, `TreasureMaps`, `Profile`, `Dqvc`, `Misc` : tailles et destinations en RAM
  établies, **champs non décodés**. Le sac (`Main`+7652) idem.
- Le protocole d'écriture est compris (§ « Rôle des deux emplacements ») mais **pas porté** :
  il pilote le matériel (`Save_BackupWrite`) et un moteur d'état de chargement, donc aucun
  marqueur `port:` ; `dq9_formats::save` écrit seulement l'image (`Save::seal`).
- Nombre et ordre des allocations de compétence (`+247`).
