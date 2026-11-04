# Scripts d'événements (`evNNNNN.stb`, `SB2`)

Implémentation : `crates/dq9-formats/src/stb.rs` (`Script::parse`, `Instr`,
`Function::native_calls`). Outil : `cargo dq9re stb data/event/ev03030.gp2 [--lang fr]`
désassemble le script et affiche le texte des messages à côté des appels de la native 400.

Ce sont les scènes scriptées lancées par l'action de trigger 0x77 (`trigger.md`) :
caméra, déplacements des personnages, dialogues, combats.

## Fichiers

689 scripts, un par archive : 523 dans `data/event/evNNNNN.gp2`, 165 dans
`data/evspt_lv5/evNNNNN.gp2`, 1 dans `data/event_lv5`. Chaque archive contient
`evNNNNN.stb` et une table de textes par langue `evNNNNN_<lg>.bin` (`table.md`,
enregistrements `[numéro de message, texte]`, numéros à partir de 100).

Les 524 numéros d'événement passés à l'action 0x77 sont des archives de `data/event` ou
`data/evspt_lv5` (502) ou des entrées de `data/event/eventlist6.bin` et `evl_quest.bin`
(tables d'enregistrements de type 102). Champs observés de ces entrées :
`[chapitre, ?, ?, ?, salle, événement, archive, nom japonais, nom du script, 100, 200,
300, x, y, z, angle, ?, -1, ?, ?, police "font12eventA…", ?, ?]`, l'archive pouvant
différer de l'événement (1520 → `ev01515`) **[champs « ? » inconnus]**.

## En-tête

Mots de 32 bits, petit-boutiste (`ov_017::021d4c04` vérifie les 3 octets `SB2`) :

| Mot | Contenu |
|---|---|
| 0 | `SB2\0` |
| 1 | enregistrement de fonction exécuté sans numéro d'entrée (`ov_017::021d4ce4`, numéro < 0) |
| 2 | base du code : décalages de code, de saut, d'appel et de chaîne sont relatifs à elle |
| 3, 4 | table des entrées `(numéro, enregistrement)` × nombre (ex. 100, 200, 300) |
| 6 | nombre de variables globales du script (0 à 5) |
| 5, 7 | toujours 0 |

## Fonctions

Un enregistrement de `0x38` octets `[code, ?, cases du cadre, paramètres, zéros…]` est
suivi de son code : `enregistrement + 0x38 == base + code` pour toutes les fonctions
atteintes (contrôle `event_scripts_decode`). À l'appel (`ov_017::021d4af8`), les
`paramètres` dernières valeurs de la pile deviennent les premières cases du cadre, les
autres sont mises à zéro. Des chaînes constantes (noms d'animations `stand`, `walk`…,
marqueurs en Shift-JIS) sont rangées entre les fonctions : `Script::parse` ne décode que le
code atteint depuis les entrées (sauts et appels suivis), comme l'interpréteur.

## Machine (`ov_017::021d4e38`)

Instructions de trois mots `[op, a, b]`. Pile de valeurs de 8 octets `(type, valeur)` :
0 entier, 1 flottant, 2 chaîne, 3 adresse. Les opérations arithmétiques acceptent entier et
flottant mélangés (conversion de l'entier), sinon l'interpréteur s'arrête en erreur
(`FUN_0200159c(-1)`).

| Op | Effet |
|---|---|
| 0 | rien |
| 1 / 2 | empile la valeur / l'adresse de la variable `a` de portée `b` |
| 3 | empile la constante `b` : `a` = 1 entier, 2 flottant, 3 chaîne (décalage depuis la base) |
| 4 | dépile |
| 5 | affectation : dépile valeur et adresse, écrit, rempile la valeur |
| 6 / 7 / 8 / 9 / 0xa | `+` `−` `×` `/` `%` (0xa : entiers) |
| 0xb / 0xc / 0xd | négation, entier → flottant, flottant → entier |
| 0xe | comparaison `a` : 0x28 `==`, 0x29 `!=`, 0x2a `<`, 0x2b `<=`, 0x2c `>`, 0x2d `>=` ; empile 0 ou 1 |
| 0xf | retour avec la valeur du sommet ; au cadre le plus externe, fin du script (valeur gardée en `+0x4c`) |
| 0x10 | saut à `base + a` |
| 0x11 / 0x12 | dépile ; saute si faux / vrai en rempilant 0 / 1 quand `b` ≠ 0 (`&&`, `\|\|`) |
| 0x13 | appel de la fonction dont l'enregistrement est à `base + b` |
| 0x14 | dépile `a` valeurs et les passe à une sortie de débogage |
| 0x15 | appel natif : dépile `a` valeurs, la première est le numéro de la native |
| 0x16 | rien à l'exécution ; `a` = 0 en tête de boucle, 1 à sa sortie dans les fonctions observées **[sens exact inconnu]** |
| 0x17 | suspend le script jusqu'à l'image suivante (reprise à l'instruction suivante) |
| 0x18 / 0x19 / 0x1a | et / ou bit à bit, non logique (entiers) |
| 0x1b | fin du script |
| 0x1c | incrémente `+0x50` et reprend après une suspension **[sens inconnu]** |
| 0x1d / 0x1e | sinus / cosinus |

Portées des variables (`b` des op 1 et 2) : 1 case du cadre, 2 case `a + entier dépilé`,
4 élément `entier dépilé` du tableau dont l'adresse est dans la case `a`, 0x40 globale ;
8, 0x10, 0x20 et 0x200 sont les mêmes que 1, 2, 4 et 0x40 en retypant la case en flottant.
Les données n'emploient que 1, 8 et 0x40.

Statistiques (code atteint des 689 scripts) : 393 719 constantes, 80 237 appels natifs,
20 685 appels de fonctions, aucun opcode au-delà de 0x1e.

## Natives

Table `{fonction, numéro}` × 304 en `ov_001::02164d6c` (terminée par `{0, -1}`), numéros
0–9, 100–122, 200–240, 300–328, 400–421, 500–603, 700–738, 800–845 ; les fonctions sont en
overlay 1 (`ov_001::0215ae1c…`). `ov_017::021d4bc8` appelle la native avec un pointeur sur
ses arguments (sans le numéro) et leur nombre. Une native n'empile rien : elle rend ses
résultats par des arguments adresse (op 2).

Le retour C du handler est **ignoré** : `Stb_CallNative` (`ov_017::021d4bc8`) appelle
le pointeur puis revient ; le cas 0x15 de `Stb_Run` continue à l'instruction suivante.
Un retour 0 n'est donc ni une attente ni un résultat sur la pile. Les attentes sont
des boucles du script qui interrogent une sortie adresse puis exécutent `yield`.

Accès aux arguments (valeurs de 8 octets, indices ci-dessous sans le numéro de native) :

| Fonction (`ov_017`) | Effet |
|---|---|
| `021d60f4` (`StbArg_Int`) | lit le mot valeur ; convertit un flottant en entier par `0200c5fc` si type 1 |
| `021d6110` (`StbArg_Float`) | lit le mot valeur ; convertit un entier en flottant par `0200c670` si type 0 |
| `021d612c` (`StbArg_Str`) | lit le mot valeur (pointeur de chaîne) |
| `021d6134` / `021d6148` | écrivent le mot valeur de la case pointée si l'argument est de type 3 (adresse) ; ne modifient pas son type |

Ces faits proviennent de la décompilation et des conversions logicielles ARM9 ;
ils ne constituent pas une observation d'exécution sous émulateur.

Le compilateur écrit le numéro sous la forme `base + décalage` (`int 400; int 5; add`) :
plié, il est constant pour les 80 237 appels, qui emploient 228 natives
(`Function::native_calls`, test `event_scripts_decode`).

L’[inventaire des 228 natives employées](stb-natives.md) détaille, par famille,
arguments, effets établis, adresses des handlers/moteur et limites d’identification.
Les entrées encore incertaines restent numérotées.

206 enfile une position absolue, 208 une rotation (radians), 210 une animation
nommée ; ce ne sont pas trois variantes de déplacement. Les acteurs STB, leurs
liaisons et les personnages/modèles appartiennent à trois espaces distincts.
Les commandes sont en files par canal ; 224 attend la fin de l’animation courante,
contrairement à 210. Les cadences absolues des durées ne sont pas établies.

400 affiche un message par identifiant ou chaîne ; 405 expose l’état d’attente
(`ev03030:fn@0xe3c` boucle tant qu’il vaut 1), et 558 lit séparément la réponse.
Le chemin YESNO documenté utilise 0=oui, 1=non, sans généralisation aux variantes.
La caméra orbitale prend azimut, hauteur relative et distance totale, pas un pitch.
726–728 chargent/restaurent des ressources sonores et jouent un index SE, pas des flags.

Les fonctions utilitaires sont compilées dans le fichier, pas fournies par le moteur ; dans
`ev03030` : attente de `n` images (`while (n >= 0) { n -= 1; yield; }`, `fn@0x58`) et
affichage d'un message avec attente de sa fin (`fn@0xe3c`).

## Questions ouvertes

- Rôle des natives et champs encore inconnus, unités temporelles et variantes :
  voir les limites de l’[inventaire](stb-natives.md).
- Sens de l'op 0x16, de l'op 0x1c et du deuxième mot des enregistrements de fonction.
- Champs inconnus des listes d'événements.
