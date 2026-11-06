# Scripts d'événements (`evNNNNN.stb`, `SB2`)

Implémentation : `crates/dq9-formats/src/stb.rs` (`Script::parse`, `Instr`,
`Function::native_calls`). Outil : `cargo dq9re stb data/event/ev03030.gp2 [--lang fr]`
désassemble le script et affiche le texte des messages à côté des appels de la native 400.
L'interpréteur qui exécute ces scripts est dans `dq9-core` (§ [Interpréteur](#interpréteur-dq9-core-j3)) ;
leurs effets sont joués à l'écran par `dq9-app` (§ [Exécution à l'écran](#exécution-à-lécran-dq9-corecutscene-dq9-app)).

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
contrairement à 210. Unités des durées : `stb-natives.md`, § Temps et unités (fondu en ticks
de 60 Hz, mouvements en paires de ticks, soit 30 Hz nominal).

Bilan des cinq natives les plus fréquentes (ROM EU, source USA clonée, wiki) :

- **206, 208, 210** : handlers EU localisés (`ov_001`, `0215bfd4`, `0215c138`, `0215c298`,
  noms dans `re/symbols.txt`). Absents de la décompilation USA clonée (elle ne contient ni
  table native ni VM SB2) ; rôles établis par la ROM EU (commandes de file, `stb-natives.md`).
  Les lectures du wiki (208 en radians, 210 « bit 1 ») sont **[hypothèse] EU**. Conflit
  ouvert sur les bits de 210 (`0x01` contre « bit 1 »).
- **570** : établie. `570(slot, b)` cache (`b=0`) ou affiche (`b≠0`) l'objet lié au slot :
  le handler positionne le masque `1` de `Object3D::flags_`, soit `OBJECT3D_FLAG_HIDDEN`
  (source USA `Object3D.cpp`). Ce n'est pas un prédicat « polled » : elle n'écrit rien par
  adresse. 201 agit sur le même bit par identifiant de personnage.
- **571** : mêmes slots, masque `0x10` de `flags_` (bit 4, sans lecteur dans la source USA)
  et bit `0x40` d'un placement. Toujours appelée avec 570 ; sens **[inconnu]**.

Aucune de ces cinq natives n'est marquée `port:`.

400 affiche un message par identifiant ou chaîne ; 405 expose l’état d’attente
(`ev03030:fn@0xe3c` boucle tant qu’il vaut 1), et 558 lit séparément la réponse.
Le chemin YESNO documenté utilise 0=oui, 1=non, sans généralisation aux variantes.
La caméra orbitale prend azimut, hauteur relative et distance totale, pas un pitch.
726–728 chargent/restaurent des ressources sonores et jouent un index SE, pas des flags.

Les fonctions utilitaires sont compilées dans le fichier, pas fournies par le moteur ; dans
`ev03030` : attente de `n` images (`while (n >= 0) { n -= 1; yield; }`, `fn@0x58`) et
affichage d'un message avec attente de sa fin (`fn@0xe3c`).

## Interpréteur (`dq9-core`, J3)

Implémentation : `crates/dq9-core/src/script.rs` et `script/` ; essai headless :
`cargo dq9re script <archive|événement|étape> [--quiet]` (`data/event/ev01130.gp2`, `1130`,
`1.1-3`), qui joue les scripts avec une application factice qui termine toute attente
aussitôt.

| Module | Rôle |
|---|---|
| `script/vm.rs` | la machine (`Stb_Run` et ses auxiliaires), sans natives ; marqueurs `port:` |
| `script/natives.rs` | numéro de native → `Event`, et l'état que les scripts interrogent |
| `script/event.rs` | les `Event` : fondus, messages, acteurs, caméra, son, déclarations de distribution |
| `script/load.rs` | numéro d'événement → archive, entrées et textes ; étapes du scénario |
| `script.rs` | `Script` (entrées jouées une à une, `step`, `signal`) et `Director` (tâche d'événement) |

**Machine.** Traduction fonction par fonction de `ov_017::021d4980…021d4e38` (les adresses sont
dans les commentaires et dans le `port:` de `vm.rs`) : valeurs `(type, mot)` de 8 octets, une
seule zone pour globales, pile et cadres, les types 3 (adresse) étant ici des indices de cette
zone, si bien qu'une native écrit son résultat comme `StbArg_SetInt` : par l'adresse reçue,
sans toucher au type. Limites de la ROM : pile de 0x80 valeurs, globales comprises, et 0x200
cadres (`Event_InitStb`, `ov_001::021538d4`). Le jeu n'en vérifie pas la première ; ici un
dépassement, un type incorrect (`FUN_0200159c(-1)`, l'erreur fatale du jeu), une division
entière par zéro, une adresse hors zone ou une boucle de plus d'un million d'instructions sans
`yield` arrêtent le script par une `Fault`, jamais par une panique. **[inconnu]** : l'effet
d'une division par zéro sur la DS (la routine `_s32_div_f` n'est pas analysée) ; **[hypothèse]**
`sin`/`cos` : le jeu convertit en double et appelle une routine logicielle (`0200c578`,
`02009424`, `02008dcc`), reproduite par `f64::sin`, qui peut différer au dernier bit.

L'interpréteur est une machine à reprise : `Vm::run` s'arrête au `yield` (0x17) et repart à
l'instruction suivante, comme `Stb_Resume`. Les opérations 0x10 à 0x12, 0x15 et 0x17 sont sans
effet tant que le mot `+0x40` est positionné (`Vm::set_skip`) ; l'op 0x1c le remet à zéro et
suspend. Aucun code ne positionne `+0x40` hors de l'interpréteur dans ce qui a été lu :
**[hypothèse]** c'est le saut d'une scène (le test de saut `ov_001::02156858` fait passer
`EventState_Run` à l'état 0xe et pose `+0x15c`, lu par la native 595). Non utilisé par
l'application.

**Tâche d'événement.** Les fonctions du pilote sont dans `ov_001` (noms dans
`re/symbols.txt`) : `Event_InitStb` crée la machine, `Event_StartEntry` (`Stb_Start` puis
`Event_UpdateActors`), `Event_Resume` (`Stb_Resume`, puis files des 24 acteurs et de la
caméra ; à la fin `Event_End`). L'ordre établi par les données (les 30 scripts de 1.1 à 1.3) :

1. entrée **200** (`prepare`) : seule à appeler 566/567/568 (74 + 25 + 20 appels) ;
2. `Event_QueueLoads`, puis `Event_BuildScene` construisent la distribution déclarée ;
3. entrée **100** (`main`, champ `+0x4a`, `EventState_Build`) : toute la mise en scène ;
4. entrée **300** (`finish`, champ `+0x4e`, `EventState_Finish`) : remises en état.

Les trois numéros viennent de l'enregistrement de l'événement (valeurs 9 à 11 de la liste, 100,
200, 300 partout). **[inconnu]** : l'appel de l'entrée 200 n'a pas été trouvé (`Stb_Start` n'a
que deux appelants dans `ov_001`, pour 100 et 300) ; son rang avant la construction est établi
par les données seulement. Une entrée absente du script (le plus souvent 300) ne fait rien
(`Stb_Start` rend -1). La native 538 enchaîne l'événement suivant (`EventTask_Update`) : le
`Director` le rejoue avant les autres événements en attente, et `Event::Ended` signale à
l'application qu'elle doit tirer le déclencheur 11, comme le fait la tâche.

**Attentes.** Aucune native ne bloque : le script boucle sur une sortie adresse avec `yield`.
Le script lève donc lui-même ce qui devient occupé en enfilant du travail (405 après 400, 204
après une commande d'acteur, 301 après une commande de caméra, 507 après 506) et
l'application l'abaisse par un `Signal` (`MessageClosed`, `ActorIdle`, `CameraIdle`,
`Preloaded`, `Answer`) ; les sorties que seule l'application connaît (pose des acteurs 543/544,
animation en cours 234) sont dans `Runtime`. Les fondus n'ont aucun prédicat de fin : les
scripts comptent des mises à jour (840 renvoie `Frame::ticks`, 2 par défaut, voir
[stb-natives.md](stb-natives.md#temps-et-unités-j0)). Un message dont le numéro manque à la table
du script n'affiche rien et n'attend rien (`ov_001::0215e2a4`) : cas réel dans `ev21595`
(message 1).

**Natives.** 96 des 120 natives du prologue sont traitées : celles qui agissent donnent un
`Event` typé (arguments décodés comme les gestionnaires, unités du jeu), celles que les scripts
interrogent répondent depuis `Runtime` ; 502, 503, 512, 587 et 724, qui ne
gèrent que la mémoire ou le matériel de la DS (comme 706 à 709), sont ignorées sans
rapport ; les autres sont rapportées **une fois** (`Event::Unimplemented`) et comptées, jamais
fatales. Les identifiants hors des plages de la table (`0–9`, `100–122`, `200–240`, `300–328`,
`400–421`, `500–603`, `700–738`, `800–845`) sont ignorés comme dans `Stb_CallNative`. 596
(`StbNat_GetActorStatus`, `ov_001::02161ab8`) cherche la déclaration de 566 dont le second
octet vaut l'argument : catégorie (`octet0 >> 4`, 1 héros local, 2 autre personnage, 0 sinon)
et état ; absente : -1 et 2. L'état d'un personnage déclaré (0 ici) vient de son contrôleur
d'animation **[hypothèse]**, les modes 9 et 10 de 566 ne sont pas modélisés **[inconnu]**.

**Résultat sur le prologue** (`cargo dq9re script 1.1-3`, test `prologue_scripts_run_to_completion`) :
les 30 scripts vont à leur terme, sans erreur de la machine, de 56 mises à jour (`ev01140`) à
3 579 (`ev21500`, 1 023 événements, environ deux minutes à 30 Hz ; il enchaîne `ev21507` par la
native 538) ; chaque message `Text::Id`
existe dans la table du script ; 8 natives restent sans effet modélisé dans `dq9-core` : 547,
559, 560, 571, 581, 582, 809 et 815 (les mêmes que « non identifiées » dans
[stb-natives.md](stb-natives.md)). Ce n'est
pas une validation sous émulateur : l'ordre et les durées ne sont pas comparés à melonDS.

## Exécution à l'écran (`dq9-core::cutscene`, `dq9-app`)

Implémentation : `crates/dq9-core/src/cutscene.rs` (état de la scène, sans E/S, tests),
`crates/dq9-app/src/event.rs` (`EventPlayer` : `Director` + rendu), acteurs et mouvements dans
`dq9-render` (`AnimatedScene::add_actor`…) ; usage et résultat par scène dans
[`app.md`](app.md), § « Événements ». Aucune fonction du jeu n'est portée en entier
(pas de marqueur `port:`) : ce sont des modèles de comportement tirés des natives
([`stb-natives.md`](stb-natives.md)) et des données. Les points ci-dessous sont ce que les
30 scripts du prologue imposent ; ce n'est pas une comparaison à melonDS.

**Établi par les données** (contrôles sur les 30 scripts, `cargo dq9re script`) :

- Liste des événements (`eventlist6.bin`, `eventlist_lv5.bin`, enregistrements 102) : la valeur 4
  est la carte de la scène (`ev21500` → 1112 `M01M12`, `ev21520` → 20101 `F01M01`, `ev01130` →
  4105 `X01M05`), les valeurs 12 à 15 la position (x, y, z) et l'angle de départ du héros, tous
  nuls (ou entiers) quand le script place lui-même les personnages (`dq9_core::script::load::place`).
  Le premier enregistrement de l'étape 1.1 dans `eventlist_lv5.bin` est `21500`
  (`stage_opening`).
- Les déclarations 566 numérotent les membres de la distribution par leur champ `attach` : le
  membre `n` est celui des créneaux de liaison et des acteurs de numéro `n` (`ev01130` : acteur 0
  = héros, `attach 0` mode 0 ; acteur 1 = PNJ 105, `attach 1` mode 5 ; acteur 2 = modèle
  `effect/ev999990700.chr`, `attach 2` mode 2 ; `ev01330` déclare 3, 4, 2, 1 dans cet ordre).
  Modes : 0 le héros, 2/3/6/7 un modèle (ou sprite) de chemin donné, 4/5 un PNJ de la carte par son
  numéro (`reference`). Le mode 4 aussi : `Event_BuildScene` le cherche dans la table des PNJ
  (`NpcTable_FindById`) et lui réserve un emplacement `Chara_Get` ([stb-natives.md](stb-natives.md#effets-exécutés-à-lécran-j3)) ;
  ses mouvements (`stand`, `walk`, `talk_loop`) viennent de l'archive du PNJ et des paquets de
  l'événement.
- Une liaison explicite (202) vise un personnage par son type : 4 avec le personnage 0 est le
  héros, 1 avec un identifiant négatif un modèle chargé par 200/228/233, 2 un objet de carte.
- Un clone (228) voit les paquets d'animation ajoutés ensuite à son modèle d'origine : en `ev21507`
  le personnage −5 (clone de −4) joue des mouvements que seul −4 reçoit (`LoadModelArchive`),
  de même −12 en `ev21500`. `RemoveAnimationPackage` est appelée pour chacun.
- La caméra de l'ouverture (572, `eye`/`lookat`) est portée par un modèle sans polygone
  (`ev01000cc000.chr`, `ev01000c000.chr`, mouvement `cut_00`/`cut_01` d'une tranche du fichier
  `nsbca` du paquet de type 0) : l'œil et la cible sont les os de ce nom du modèle posé, dans le
  repère de l'acteur. Avec l'échelle 0x10a/4096 du sélecteur 0, l'œil et la cible de `cut_01`
  tombent à quelques unités des personnages que le script place un peu avant (œil
  (−11,75 ; 12,35 ; −14,58), cible (−27,40 ; 0,65 ; −9,20), personnages vers (−27 ; −0,3 ; −9)) ;
  avec l'échelle 1 ils seraient à 400 unités.
- Les messages d'une scène sont fermés tantôt par le joueur (le script boucle sur 405), tantôt par
  la native 401 après une durée (textes `<CEN>` des cinématiques, cadre retiré par 410).

**Hypothèses** (marquées dans le code) :

- Un modèle chargé par 200 avec le sélecteur 0 a l'échelle des personnages de la carte
  (0x10a/4096, `NpcTable_Build`) ; avec le sélecteur 1 il a l'échelle 1, le script fixant la
  sienne par 211. Le clone (228) a 0x10a. **[hypothèse]** : le sens du sélecteur est inconnu
  (`stb-natives.md`, 200) ; elle s'accorde avec les caméras et les modèles attachés du prologue.
- Les commandes d'un acteur sont groupées en six canaux (position, rotation, animation, échelle,
  alpha, rayon) ; les huit files du jeu n'ont pas été reliées une à une. Une commande instantanée
  ne prend pas de tick, l'acteur est au repos quand tous les canaux sont vides ; 210 ne bloque
  pas, 224 et 213 attendent la fin du mouvement en cours (fin = la fin de la tranche atteinte
  une fois, y compris pour un mouvement qui boucle).
- Les durées de la caméra (304 à 322) et du chemin (217) sont comptées comme celles des acteurs
  (2 ticks par unité) ; le demi-angle de champ de 532/580 est celui du jeu (15° par défaut) ; le
  roulis 327 tourne le vecteur haut autour de la visée, positif vers la droite de l'image.
- `Event_BuildScene` ne lie pas elle-même la position de départ d'un PNJ de la carte : il garde
  sa place jusqu'à la première commande, et la retrouve à la fin de la scène.
- Fondus : le moteur « principal » est l'écran 3D ; un moteur verrouillé (112, 114, 116) ignore
  les fondus simples (100, 101, 105, 106, 107) jusqu'à un fondu qui le déverrouille
  (117 à 122) ; l'état revient à la normale quand la dernière scène finit.
- Lumière : la durée de 578 est comptée comme celle des acteurs (2 ticks par unité, le jeu la
  multiplie par `s16(02010218(save))`, ≈ 33 ms) ; la phase forcée par 588 est prise au début de la
  phase, sans fondu vers la suivante ; 579 et 808 changent l'horloge du jour de l'application
  (rechargement d'une carte ensuite). La phase des cartes de base se règle comme à l'entrée
  d'une zone (`settle_basic_entry`) seulement quand le script ne la force pas.
- Un modèle accroché à un os (235) est dessiné dans le repère du parent × la matrice de l'os
  (`HeroRig::bone`, `actor_bone`), sa propre pose étant relative à cet os ; l'échelle du parent
  est héritée **[hypothèse]**, que les scripts remettent à 1 pour le modèle accroché.
- La bulle (541) est un sprite posé sur le rayon qui passe par le pixel de l'écran DS (projection
  de l'acteur décalée de `(−6, −76)`), un peu devant l'acteur, à l'échelle d'un texel par pixel
  DS **[hypothèse]** : le jeu la dessine en 2D par-dessus l'image.
- Placements 574/575 : `Scene::build` construit une instance pour chaque placement de modèle ou
  d'objet, y compris ceux que l'heure choisie cache (`Instance::hidden`, bit `0x4` de `+2`).
  L'instance cachée a une échelle nulle mais garde sa matrice d'origine (`SceneInstance::home`,
  `0x02013168`) ; 574 et 575 changent l'échelle et la translation d'après cet état : un 575
  seul déplace le placement sans le montrer, 574 le montre à sa nouvelle position (dans un ordre
  ou l'autre), jusqu'au rechargement de la carte, comme le jeu qui garde les champs de
  l'enregistrement. Les placements de collision n'ont pas d'instance : leur changement de
  murs/sol n'est pas appliqué et le rapport le compte (`collision placement`) depuis le lot
  « scènes restantes ».
- Légendes (409/411/413) : `W+0x9a8` donne le maintien ; `020658b8` fait varier `W+0x9b0` de
  `0x8444` par image jusqu'à `0x1f0000` (60 images par fondu), `02066d00` en tire l'alpha du
  texte. `0206b6c8` déplace la fenêtre à `(192 − (lignes − 1) × 20 − 8) / 2 − 16` (411) et
  `02045608` coupe le bip quand `W+0x19b2 == 0` (413). Exécuté par `Talk` : alpha des quads,
  texte centré quand 410 a retiré le cadre, blips filtrés ; le compteur de message est
  **gelé** pendant la légende (`020657c8` : l'action ne ferme pas avant 401, 405 reste 1).
- Caméra à trois os (552) : `0204a170` suit `eye` et `lookat` de la source et pose la cible sur
  `foot` ; celle-ci est tournée tous les trois updates vers son déplacement. Exécuté par
  `Camera::ThreeBoneCamera` et `EventPlayer::drag_three_bone` ; sa capture à l'image 1820 de
  `21507` montre le changement de point de vue, sans comparaison melonDS.
- Porte (584) : `02018b44` retrouve le placement `(entrée de bloc, identifiant)` de la zone ;
  `02018310` fixe l'angle cible à `+6434` (90°) et la vitesse à `409` par update. Exécuté par
  `Event::OpenDoor` : rotation du placement visible aux images 2900→2960 de `21500` ; le son
  (index de `g_DoorSeTable` dépendant du quartet bas de la zone) reste rapporté.

**Exécuté depuis J3** : lumière et heure (578, 579, 588, 589, 548/549, 808) et brouillard (804)
par `dq9_core::cutscene::SceneLight` ; attachements d'os (235, 236) ; mouvements nommés des PNJ de
la carte ; sprites 2D de script (521/522) ; bulles (541/542/561) ; placements (574/575) ;
mouvements 545/546 ; palette (238) ; retour de la caméra (328) — détail et preuves dans
[stb-natives.md](stb-natives.md#effets-exécutés-à-lécran-j3).

- Échange d'écrans (109/110) : `Cutscene::screen_swapped` conserve le bit `POWCNT1` 15 entre
  événements ; `screens::field/overlay` placent le champ et la fenêtre sur l'autre écran.

**Non exécuté** (rapporté par `--events-report`) : native 547 (transition de combat, hors v0.1) ;
le son de porte (584) ; l'échange d'écrans tant que le layout de `app.rs` n'applique pas
`screen_swapped()` ; 559, 560, 571, 815, 581, 582 et 809 sont sans effet visible établi ou
sans objet dessiné.

## Enchaînement du prologue (`dq9-core::scenario`)

Ce qui enchaîne les scènes est établi (ROM EU, Ghidra, `cargo dq9re triggers`) :

- `ev21500` → `ev21507` : native 538 du script (`Director`).
- `ev21507` → `ev21520` : la fin de `ev21507` tire le trigger 11 de `M01` (`8 : 21507`), dont
  l'action `0x78(26)` est **un combat scripté**, pas une tâche inconnue (`trigger.md`, § Combats
  scriptés : `Trig_RunDeferred` cas `0x78` remplit la tâche de combat `+0x3718`, numéro 26 dans
  `eventbattle.bin`). La victoire tire le trigger 15 (`12 : 26`), qui lance `ev21520`
  (`triggerM01.bin` et `triggerF.bin`). Il n'y a pas de combat avant J4 : l'application le
  déclare gagné (`scenario::stand_in`).
- `ev21520 → 21521 → 21522 → 21596` : natives 538 ; changements de carte (`F01M01`, `X01`,
  `X01M01`, `X01M05`) demandés par les scripts d'après l'enregistrement de la liste des
  événements. À la fin de `21596`, le trigger 11 de `X01M05` lève le drapeau 337, donne l'objet
  22159 et rend la main au joueur.
- Ensuite, tout vient des triggers : conversations (trigger 0 puis 1 à la fermeture :
  `Field::talk_ended`), aires (2), portes et warps (17, 6), arrivée (20, 3), et le trigger 11 des
  scènes (suite, étape `0x84`, transport `0xe1`/`0x85`). Exemple : `21591` (descendre à Angel
  Falls) part de la conversation avec le PNJ 101 (`6 : 101`, `11 : 193`, `16 : 0` → `0x77(21591)`),
  se termine par `0xe1(1100)` ; `21592` part du trigger d'arrivée (3) de `M01` à l'étape 1.2 ;
  `21593` d'une conversation (PNJ 99) et se termine par `0x84(1, 4, 1)` (étape 1.4) et `0x85(4105)`.
- Parcours complet : `dq9-app --new-game --headless --auto [--pilot]` (`app.md`, § « Nouvelle
  partie »).

## Questions ouvertes

- Natives encore sans rôle (571, 559, 560, 547, 815) : tableau « non identifiées » de
  [stb-natives.md](stb-natives.md#prologue-dangel-falls--natives-employées-j0).
- Palette de 238 : les modes 1 et 2 ne sont pas observés ; objets de 809 et ombres de 581/582
  **[hypothèse]**.
- Nature (30 Hz ou 60 Hz) de `currentNumTicks_` : [hypothèse] VBlank, non démontrée.
- Sens de l'op 0x16, de l'op 0x1c et du deuxième mot des enregistrements de fonction.
- Champs inconnus des listes d'événements.
- Appelant de l'entrée 200 et origine de `+0x40` (saut) dans la tâche d'événement.
- Porte 584 : quartet bas de la zone (index du son 200–204) non lu, branche coulissante (bit
  `0x20`) et sens inverse non modélisés. 571 reste non identifiée (bit `0x10` sans lecteur,
  sans effet visible établi) ; 839 (`+0x265`) n'apparaît pas dans le prologue (seulement
  `ev28920`) et n'est donc pas piloté en v0.1.
