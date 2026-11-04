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

**[inconnu]** Rôle exact des deux emplacements. Indices : `Save_Serialize` prend un argument
`intermission` ; quand il est vrai, les blocs partent à `slot 1` (`+0x8000`) et l'en-tête
écrit à `0x10` (emplacement 0) porte l'octet intermission à 1 ; le bloc « position » en
`slot+27104` n'est écrit que dans ce cas. Lecture la plus probable : emplacement 0 = sauvegarde
normale, emplacement 1 = sauvegarde de suspension. `Save::current_slot` prend le premier
emplacement de statut valide.

## Sérialisation (`Save_Serialize`, `0x020a9ec8`)

Les offsets de la fonction sont relatifs à un tampon décalé de `-0x84` : ajouter 0x84 pour
l'offset d'emplacement. `Save_LoadSlot` fait l'inverse. Blocs retrouvés (offset d'emplacement,
taille, source en mémoire) :

| Offset | Taille | Contenu | Source |
|---:|---:|---|---|
| 136 | ≈ 0x2ca3 | personnages 13 × 0x23c, effectifs, sacs, or, zone, réglages | tampon assemblé par la fonction |
| 7572 | 1 | personnages en réserve (`0x1d0c` du tampon) | |
| 7573 | 4 | ordre du groupe | |
| 7577 | 1 | effectif du groupe | |
| 7578 | 4 × 18 | objets portés (8 × u16 + 2 octets) | `char+0x454` (0x10 octets) |
| 7652 | 0x1d4 | sac (`iVar11+0`, structure de 0x1d4 octets) | |
| 11448 / 11452 | 4 / 4 | or en main / en banque | `+0xf6c` / `+0xf68` |
| 11460 | 4 | mini-médailles remises | `+0xf74` |
| 11476 | 2 | zone (`+0x2c94`, mot courant, ou `0x6d` pour le point de départ) | |
| 11568 | 0x344 | **progression** : chapitre, étape, sous-étape (`Save_Chapter`, `Save_ScenarioStep`, `Save_ScenarioSubstep`), 1 mot (`0x0201081c`), puis le bloc d'événements (0x334 octets, voir plus bas) | `GameState`, copié par `func_020a99d0` (écriture) / `func_020a9a50` (lecture) |
| 12404 | 0xa0 | état du journal de quêtes (`QuestLog_Get`) | |
| 12564 | 0x330 | durées de quêtes (`QuestLog+0x178`) | |
| 13380 | 0x194 | `GameState+0x5cda` (1 octet) puis `+0x5cdc` (400 octets) | |
| 13784 | 0x4d0 | monstres vaincus | `GameState+0x75f0` |
| 15016 | 0x3b4 | alchimie | `GameState+0x7ac0` |
| 15964 | 0xec | accolades | `GameState+0x7504` |
| 16200 | 30 × 232 (l'éditeur) | invités recensés ; le code écrit `uRam020aad18` octets de `func_02012fe4()+0x840`, puis un second bloc de `uRam020aad1c` octets à `+0xd98` | `func_02012fe4()+0x840`, `+0x15d8` |
| 23176 | 0x200 | message DQVC (éditeur) ; le code y écrit `GameState+0x5f80` | `GameState+0x5f80` |
| 26980 | 0x7c | profil du joueur | `GameState+0x569c` |
| 27104 | 0x1c | position (sauvegarde de suspension seulement) | objet du héros |
| 27132 | 0x2c4 | contenu téléchargeable (DQVC) | `GameState+0x71fc` |
| 27840 | 0x110 | (sens inconnu) | `GameState+0x5e6c` |

Les lignes sans « Source » viennent du seul éditeur (non revérifiées dans le code). Le bloc
« 136 » est assemblé dans un tampon puis écrit d'un coup : dans ce tampon, `+0x1d0c` à `+0x1d12 + 18n`
(effectif, ordre, groupe, objets portés), `+0x2c30..0x2c3c` (or, banque, médailles),
`+0x2c4c` (zone), `+0x2c58..0x2c77` (réglages, position de départ), `+0x2ca2` (drapeau
d'intermission).

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
PV max, PM max), points libres `+244`, maîtrises `+282`, nom `+320` (10 octets), équipement
(u16) torse `+352`, jambes `+354`, bras `+360`, pieds `+362`, tête `+364`, arme `+366`, bouclier
`+368`, accessoire `+370`, visage `+356`, coiffure `+358`, teint/yeux/sexe `+372`, cheveux
`+373`, morphologie `+376`/`+378`.

## Inconnu

- Encodage des noms (`12 2f 30 39…` pour le héros de la démo) : à rapprocher de `text.md`.
- Numérotation de la zone (`109` sur la démo) ; sens des mots de position.
- Bits de `lieux visités` (`0xfe77ffff` sur la démo, l'éditeur ne nomme que les bits 1..=18).
- Sens des drapeaux eux-mêmes (indices) : le bloc est typé, pas les bits. Les blocs de
  `+0x2c0` (12 octets), `+0x2cc` (0x66 octets) et l'octet `+0x332` hors sélecteur de piste
  n'ont pas de rôle établi.
- Écritures : protocole de `Save_WriteStep` (`0x020aad2c`, 4 états : lecture de l'emplacement
  1, sérialisation, écriture, vérification) et rôle des écritures redirigées vers un tampon
  (`puRam020aaf8c`) ; pas encore porté, donc aucun marqueur `port:`.
- Nombre et ordre des allocations de compétence (`+247`).
