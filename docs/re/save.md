# Sauvegarde DS (`.sav`)

Module `dq9_formats::save`, commande `dq9re save <fichier> [-v]` (sans ROM).

## Source et vérification

La disposition vient de l'éditeur de sauvegarde [DQIX/editor](https://github.com/DQIX/editor)
(`src/game/layout.js`, `src/saveManager.js`, `pattern.hexpat` ; **sans licence** : lu pour
comprendre, réimplémenté, rien copié ; sa sauvegarde de démonstration n'est pas versionnée).
Nous n'avons pas encore retrouvé le code de sauvegarde du jeu lui-même dans l'ARM9 : les
décalages ci-dessous sont des faits de **fichier**, pas la sérialisation de `GameState`
(les décalages `save +0x…` de `re/symbols.txt` sont ceux de la RAM et **diffèrent** : par
exemple le nombre de membres du groupe est en `+0x3980` en RAM, `+7577` dans le fichier).

Vérifié sur la sauvegarde de démonstration de l'éditeur (64 Kio) :

- les 3 sommes de contrôle de chaque emplacement se recalculent exactement (CRC-32 standard,
  polynôme IEEE, valeur initiale 0 ; `crc32fast::hash`) ;
- réécrire (recalcul des sommes) redonne le fichier octet pour octet ;
- les champs lus sont cohérents : héros Sage niveau 91, groupe de 4, 98 h 32 min 44 s,
  équipement dans les plages d'identifiants attendues, or et temps de jeu plausibles.

## Conteneur

Image EEPROM de **0x10000 octets** = 2 emplacements de **0x8000**. Dans un emplacement :

| Décalage | Contenu |
|---:|---|
| 0 | `DRAGON QUEST IX` + octet 0 (16 octets) |
| 16 | CRC A, couvre `20..36` |
| 20 | indicateur « sauvegarde intermédiaire » (quick save), nom de l'éditeur |
| 21 | bit 0 : l'emplacement contient une sauvegarde |
| 36 | CRC C, couvre `40..132` |
| 132 | CRC B, couvre `136..28644` |
| 136 | 13 personnages de 572 octets |

Les sommes ne couvrent **pas** le magic. Sur l'exemple réel, l'emplacement 1 est la copie
exacte de l'emplacement 0 sauf le magic, effacé à `0xff` : **[hypothèse]** le jeu écrit le
magic en dernier pour valider un emplacement (protection contre une coupure). Le portage
choisit donc le premier emplacement avec magic et sommes valides (`Save::current_slot`).
**[inconnu]** rôle exact des deux emplacements (alternance, copie de secours, ou emplacement
normal + intermédiaire) : à trancher en lisant le code ARM de sauvegarde.

## Champs typés

Décalages relatifs à l'emplacement.

| Champ | Décalage | Type |
|---|---:|---|
| personnages en réserve / dans le groupe | 7572 / 7577 | u8 |
| ordre du groupe | 7573 | 4 × u8 |
| objets portés | 7578 + 18·n | 4 × 8 × u16 (`0xffff` vide) |
| or en main / en banque | 11448 / 11452 | u32 |
| mini-médailles remises | 11460 | u32 |
| zone (sauvegarde normale) | 11476 | u16 |
| invités spéciaux, lieux visités | 11528, 11788 | u32 (champs de bits) |
| tours de passe-passe appris | 12108 | u32 (champ de bits) |
| vocations débloquées | 12276 | u16 (bits 6..=11 : Gladiateur … Rôdeur) |
| temps de jeu | 16024 | u16 heures, u8 minutes, u8 secondes |
| zone / position (sauvegarde intermédiaire) | 27104 / 27108 | u16 / 3 × i32 |

Personnage (572 octets) : niveaux `+2` (13 octets, vocation 0 = Gardien), renaissances `+15`,
expérience `+28` (13 × u32), vocation courante `+80`, graines `+88` (12 octets par vocation :
3 mots de 3 × 10 bits, ordre force, agilité, résistance / adresse, charme, guérison magique /
puissance magique, PV max, PM max), points de compétence libres `+244`, drapeaux de
maîtrises `+282` (36 octets), nom `+320` (10 octets), équipement (u16) torse `+352`, jambes
`+354`, bras `+360`, pieds `+362`, tête `+364`, arme `+366`, bouclier `+368`, accessoire
`+370`, visage `+356`, coiffure `+358`, octet de teint/yeux/sexe `+372` (bit 0 sexe, bits 1-3
peau, 4-7 yeux), couleur de cheveux `+373` (4 bits), morphologie `+376`/`+378` (u16).

## Inconnu

- **[inconnu]** Encodage des noms : ce n'est pas de l'ASCII (`12 2f 30 39…` pour le héros de
  l'exemple). À rapprocher de la table de caractères de `text.md`.
- **[inconnu]** Numérotation de la zone (`109` sur l'exemple) ; sens des mots de position.
- **[inconnu]** Bits de `lieux visités` : l'éditeur nomme les bits 1..=18 (villes) mais
  l'exemple en positionne d'autres (`0xfe77ffff`) ; exposé brut.
- **[inconnu]** Nombre et ordre des « allocations de compétence » (`+247`) ; exposé via les
  octets bruts seulement.
- **[inconnu]** Chapitre, étape et sous-étape du scénario, drapeaux d'événements, quêtes
  (l'éditeur place l'état des quêtes en `+12300`, 4 bits par quête), sacs, bestiaire, invités
  (`+16200`, 30 × 232), grottes (`+24206`, 28 octets) : non typés. Ils restent lisibles par
  `Slot::bytes`. L'état du scénario de `re/symbols.txt` (`Save_Chapter`…) n'a pas été
  retrouvé dans le fichier : la disposition fichier et RAM diffère.

## Suite

Ce module est le format, pas la logique : charger une sauvegarde dans `dq9-core` (J2)
demandera de typer les drapeaux d'événements et l'état du scénario, en retrouvant dans
l'ARM9 les fonctions qui (dé)sérialisent la RAM vers ce fichier.
