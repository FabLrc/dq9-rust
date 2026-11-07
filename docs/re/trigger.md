# Triggers de scénario (`data/scenario/trigger<carte>.bin`)

Implémentation : `crates/dq9-formats/src/trigger.rs` (format : `parse`, `Trigger::loaded`,
`Trigger::talk`, `action_name`) et `crates/dq9-core/src/trigger.rs` (état et exécution :
`first_match`, `fire`, `run_action`, `run_deferred`, `State`) ; les événements des régions de
carte (2, 3, 5, 6, 17, 20) sont déclenchés par `crates/dq9-core/src/field.rs` (voir « Zones »
plus bas). Outil : `cargo dq9re talk`
déclenche les triggers de conversation de la carte, exécute leurs actions, puis choisit la
réplique (`dialogue.md`).

Règles « si conditions alors actions » déclenchées par des événements du jeu. Ce sont elles
qui fixent le **contexte de conversation** 192–223 qu'attendent la moitié des répliques des
PNJ (offres et récompenses de quêtes), lancent les événements scriptés (`stb.md`), font
avancer le scénario, donnent objets et or.

## Chargement (`Trig_LoadMap`, `0x0206462c`)

Au chargement d'une salle : fichier `data/scenario/trigger%s.bin`, `%s` = trois premiers
caractères de la carte (`M02`), ou `F` seul pour les cartes de terrain `F*`. C'est une table
d'enregistrements (`table.md`) lue par `Tbl_*` avec `g_TrigRecordHandlers` (`0x020f05bc`) :
types 100 et 101 sans effet, type 1 = un trigger (`Trig_LoadRecord`, `0x0205f9dc`) :

```
salle, chapitre_min, étape_min, chapitre_max, étape_max, événement, mots…
```

Il n'est gardé que si `salle` est la salle courante et
`chapitre_min*1000 + étape_min <= chapitre*1000 + étape <= chapitre_max*1000 + étape_max`
(`Save_Chapter`, `Save_ScenarioStep`, c'est-à-dire la progression de la piste courante,
voir plus bas). `Trig_Register` le range dans la chaîne de son type d'événement, dans
l'ordre du fichier.

76 fichiers, 5 895 triggers, tous de type 1 (test `trigger_files_compile`).

## Pistes de scénario

Le gestionnaire de quêtes commence par cinq blocs de `0x1c` octets :
`[chapitre, étape, sous-étape, drapeaux 0 (13 octets, +3), drapeaux 1 (12 octets, +0x10)]`.
L'octet `+0x332` choisit le bloc courant ; `FUN_02064ba8` le fixe d'après la salle à
l'entrée d'une carte puis recopie sa progression dans la sauvegarde (`+0x5cb0..+0x5cb8`,
`Save_Chapter`…) :

| Piste | Salles (cartes) |
|---|---|
| 1 | 4200–4202 (X02), 9000–9008 (T01) |
| 2 | 1700–1706 (M07), 1800–1808 (M08), 6000–6001 (S10), 7700–7709 (D07) |
| 3 | 200–219 (C02), 7802–7809 (D08) |
| 4 | 2100–2109 (M11), 8301–8303 (D13) |
| 0 | toutes les autres |

**[hypothèse]** Ce sont les arcs du scénario que le joueur peut suivre dans l'ordre de son
choix : chacun garde sa propre progression.

La progression d'une piste n'avance jamais en arrière (`FUN_020703d8` compare
`chapitre*10000 + étape*100 + sous-étape`). Un changement de chapitre efface les deux jeux
de drapeaux de la piste (`FUN_0206e090`), un changement d'étape le jeu 1 (`FUN_0206e0e0`).
Les « drapeaux par joueur » des notes précédentes sont donc des **drapeaux locaux à la
piste** : le jeu 0 vaut pour le chapitre en cours, le jeu 1 pour l'étape en cours
(conditions 2–5, actions 0x66–0x69, 0x86/0x87).

## Événements

`Trig_Fire(gestionnaire, événement, paramètres)` (`0x020649c0`) : premier trigger valide de
la chaîne, puis ses actions ; les actions différées ne sont exécutées
(`Trig_RunDeferred`, `0x0206f82c`) que si la liste est allée à son terme. Les paramètres sont
un bloc de mots que comparent les conditions 6 à 0xC (mots 0 à 6), 0x1E (mot 8), 0x4B/0x4C
(mot 9), 0x1D (mots 7 et 10) et 0x20/0x21 (octets `+0x2a..+0x2d`).

Chaque appelant passe une constante (relevée dans `r1` avant chaque appel) :

| Événement | Triggers | Déclencheur | Paramètres |
|---|---|---|---|
| 0 | 2 831 | début de conversation (`Talk_Start`) | 0 = PNJ, 5 = zone de conversation |
| 1 | 1 143 | fin de conversation (`TalkTask_Update`, états de fermeture) ; branché : `Field::talk_ended` | 0 = PNJ, 5 = contexte de la conversation (celui qui a choisi la réplique), `+0x954` = réponse au dernier `<YESNO>` (0 sans question) |
| 2 / 5 | 110 / 0 | le meneur entre dans / sort d'une zone (`ov_017::0219814c`, `02198e30`) | 1 = zone : objet de carte de type 3 ou zone ajoutée par l'action 0x8f |
| 3 | 249 | arrivée dans une salle après une transition (`ov_017::0219e384`, `0219fc88`, via `FUN_02064a04`) | 3 = salle |
| 6 | 35 | le meneur entre dans la boîte d'un warp `0x72` (`Field_CheckWarps`, `ov_017::0219ca88`) : le transport est prêt (`Save_Warp`), le trigger s'exécute avant le changement de carte | 3 = salle quittée |
| 9 / 19 | 0 / 11 | `FUN_02064a50` (personnage local) / `FUN_02064aac` (meneur), appelé par `FUN_02053644` quand un compteur d'un personnage atteint sa cible | 1 = zone courante ; octets `+0x2a..+0x2d` = quatre octets du personnage (`+0x178`) **[sens inconnu]** |
| 11 | 669 | fin d'un événement scripté (`ov_017::021bc77c`) | 2 = numéro de l'événement |
| 15 / 16 | 47 / 33 | fin d'un combat scripté (`BattleTask_Update`, `ov_017::021b790c` → `Trig_FireBattleEnd` `0x02064a18` / `Trig_FireBattleEndAlt` `0x02064a34`) : 15 quand il est gagné, 16 quand l'octet `+0x8e14` de la structure de combat vaut 2 (défaite) ; branché : `Field::battle_ended` (§ « Combats scriptés ») | 6 = numéro de combat (champ 0 de `data/event/eventbattle.bin` : les 40 valeurs y figurent) |
| 17 | 504 | porte de type 2 touchée : le meneur a marché 3 images contre son orientation, une quatrième la prend (`Field_TouchDoorways`, `ov_017::02198f84`, liste `+0x36bc`) | 7 = (classe `+0x2c`, numéro `+0x2d`), 10 = orientation `+0x20` |
| 20 | 195 | fin du chargement des triggers de la salle (`Trig_LoadTable`) | 3 = salle |
| 22 | 41 | overlay 2 (`ov_002::02155960`), cartes H18 et M08 **[sens inconnu]** | — |
| 23 | 13 | `ov_017::02199138`, carte S08 **[sens inconnu]** | 3 = valeur globale, 9 = premier objet de `+0x36bc` |
| 27 | 12 | `ov_017::021a9bf8`, après le journal de quêtes **[sens inconnu]** | — |
| 30 | 2 | même tâche que l'événement 3, M11 | 3 = salle |
| 10, 12, 18, 24, 25, 26, 29 | 0 | overlays 1, 3, `ov_017` (sessions sans fil pour 26/29) | — |

Constats sur les données : l'événement 1 porte les suites des répliques (actions 0x72/0x73
objets, 0x77 événement, 0x7d/0x7f quêtes, conditions 0xB sur le contexte et 0x10 sur la
réponse oui/non) ; l'événement 20 prépare la salle (0x95/0x96 objets, 0x8f zones) ;
l'événement 17 marque l'objet (0x6c) puis fait parler.

## Zones : aires, portes et warps (`dq9_core::field`)

Les événements 2, 5, 6, 17 et 20 ne viennent pas de conversations mais des régions de la
carte (`.bmbl`, `map-archive.md`, « Portes, zones et changements de carte ») et de l'arrivée
dans une salle. `Field::step` les évalue à chaque pas fixe avec la position du meneur ;
`Field::enter` joue 20 puis, après une transition, 3. Les triggers sont relus du fichier à
chaque événement, filtrés par `Trigger::loaded` d'après la progression de la piste de la salle
(`FUN_02064ba8` la choisit à l'arrivée, `track_of_room`). Un trigger qui rend un transport
(actions `0x79`/`0x7a`/`0x7b`…) devient un changement de carte ; les autres demandes
(`Request::Talk`, `Event`…) sont rendues à l'application, qui les journalise jusqu'à J3.

**Aires de la carte** (zones de type 3, `Area_CheckLeader`, `ov_017::0219814c`) : on cherche
la première aire de la liste (la plus récemment définie d'abord) qui tient le meneur
(`Zone_Holds`). Si l'aire change, événement 5 avec le mot 1 = numéro de l'aire quittée, puis 2
avec celui de l'aire atteinte. Le jeu mémorise l'aire (pointeur) sur l'objet du meneur ; deux
aires de même numéro sont donc distinctes. 22 sur la ROM, 15 cartes.

**Aires ajoutées par l'action `0x8f`** (`Trig_RunAction`, cas `0x2b`, `Trig_AddArea`
`0x02064b08`) : `0x8f(n)` puis coin haut (x y z), coin bas (x y z), angle en degrés. Structure
de 0x30 octets ajoutée **en fin de liste** : `+0` n, `+2` angle `degrés × π / 180 × 4096`
tronqué en 16 bits puis ramené dans `[0, 2π]` (`0x6488`), `+4` centre = (haut + bas) >> 1,
`+0x10` rayon² = (largeur/2)² + (**hauteur**/2)² (pas la profondeur : un rectangle haut et
mince refuse ses extrémités quand il est tourné), `+0x14` coin haut, `+0x20` coin bas. Les
flottants sont arrondis (demi unité loin de zéro). `ov_017::02198e30` teste le meneur dans
chacune, la première qui convient l'emporte ; l'aire courante est `+0x491` de l'objet des
triggers (−1 = aucune), l'événement 5 porte l'aire quittée en mot 1, 2 l'aire atteinte, et
`+0x4446` du gestionnaire de champ mémorise la dernière atteinte (lue par les actions `0xd1` et `0xd2`). Les 116 occurrences
sont surtout dans des enregistrements de l'événement 20 (80 d'après le wiki). Sur le prologue
d'Angel Falls (1.1), l'événement 20 de `M01` ajoute l'aire 10 et aucun trigger de zone n'y
répond.

**Porte touchée** (événement 17, `Field_TouchDoorways`, `ov_017::02198f84`) : mot 7 = classe
`+0x2c` (moitié basse) et numéro `+0x2d` (moitié haute), mot 10 = orientation `+0x20`
(la condition `0x1d` les compare : arg = classe, opérande 0 = numéro, opérande 1 =
orientation en degrés). Si le trigger a posé un transport, il remplace la destination, la
position et l'orientation d'arrivée de la porte ; sinon la porte part vers la sienne. Un
trigger qui s'arrête (action sans cas) abandonne ses actions différées : la porte garde sa
destination.

**Arrivée** : la salle chargée joue l'événement 20 (fin du chargement des triggers : mot 3 =
salle, c'est là que les triggers ajoutent leurs aires et marquent leurs portes), puis, après
une porte ou un transport, l'événement 3 (mot 3 = salle).

**Vérification sur une sauvegarde** : à Stornway (`C01`, 19.2, nuit), l'événement 20
(condition `0x11(1)` : nuit) joue `0x6c` pour les numéros 24 et 22 ; la RAM d'un état melonDS
y montre exactement ces deux portes de type 2 (zones 12 et 13, les deux accès de `C01M14`) avec
les drapeaux 12 et 14 au lieu de 8 et 10, soit le bit `0x4` (`dq9re doors --state`, test
`stornway_at_night_marks_its_two_locked_doorways`). Les portes de l'église (numéros 18 et 20)
restent libres, et le héros les prend.

**Non modélisé** : l'animation de la porte et la marche automatique, les gardes de
`ov_017::0219755c`, les zones des autres types, et l'application par la fenêtre des objets de
carte `0x6d`, `0x99`… Pour `0x95`/`0x96`, l'émission est faite (`Request::SetPlacementHidden`,
voir le tableau des actions) ; l'application au rendu et aux collisions appartient à
`dq9-app`.

## Mots (`Trig_CompileWords`, `0x0205ec80`)

Chaque mot vaut `code << 16 | arg` et est suivi d'un nombre fixe de champs d'opérandes, propre
au code (table dans `dq9_formats::trigger::operand_count`). Les codes 100 à 499 sont des **actions**, les
autres des **conditions** ; le compilateur les sépare en deux listes. Contrôle : les mots des
5 895 triggers tombent exactement sur la fin de leur enregistrement.

Les nœuds compilés rangent l'argument et les moitiés des opérandes dans des demi-mots ; les
dispositions, relevées sur les deux tables de sauts (`0x0205ed1c` conditions,
`0x0205f1f4` actions), sont reproduites par `dq9-core/src/trigger.rs`. Cas notables :

- 0x1D et les actions 0x6c/0x6d/0x88/0x98–0x9a : le second opérande est un angle en degrés,
  converti en `(π·x/180 ramené une fois dans [0, 2π]) × 4096` tronqué ;
- 0x32/0x52 : un mot donne deux nœuds (voir conditions) ;
- 0x4A : l'argument est perdu, seule la moitié basse de l'opérande est gardée ;
- 0x7a/0x7b : flottants × 4096 arrondis (positions, direction) ; 0x8b/0x8c : opérande entier
  sur 32 bits ;
- 0x8f : sept flottants (zone).

## Choix (`Trig_FindMatch`, `0x020644a0`)

Pour un événement, les triggers de sa chaîne sont essayés dans l'ordre ; le premier dont
**toutes** les conditions sont vraies (évaluées dans l'ordre, arrêt au premier faux) est
retenu. Deux bizarreries conservées : le masque d'équipement de 0x32/0x52
(`+0x492`) n'est remis à zéro qu'une fois par événement, et le booléen « tout est vrai »
n'est pas réinitialisé entre deux triggers (sans effet sur les données : aucun trigger n'a
zéro condition). Si aucun trigger ne convient, le jeu suit son comportement par défaut
(conversation : contexte = zone).

## Conditions (`Trig_EvalCondition`, `0x0205fb04`)

Toutes les conditions présentes dans les données sont réimplémentées (test
`trigger_conditions_are_modelled`, `dq9-core/tests/real_rom.rs`) ; seules 0x2C, 0x45, 0x4F, 0x53 et 0x5B, absentes des données,
lisent un état non modélisé (`dq9_core::trigger::UNMODELLED`). « Personnage `n` » = entrée `n` de la
table des personnages (`FUN_0200fd80`, 0 = le héros), « équipe » = liste à `+0x397c` de la
sauvegarde (le premier est le meneur).

| Code | Vrai si |
|---|---|
| 0 / 1 | drapeau de scénario `arg` levé / baissé (tableau de bits `+0x8c` du gestionnaire de quêtes) |
| 2 / 3, 4 / 5 | drapeau `arg` du jeu 0 / 1 de la piste courante levé / baissé |
| 6…0xC | mot `code-6` des paramètres == `arg` |
| 0xD / 0xE / 0xF | nombre de personnages 0 à 3 présents ≥ / ≤ / == `arg` |
| 0x10 | `+0x954` de la fenêtre de messages == `arg` : **réponse au dernier `<YESNO>`**, 0 = oui, 1 = non (M05, PNJ 7 : la branche « non » donne l'indice et le trigger 0x10(1) enregistre la quête 201) |
| 0x11 | `arg` = 1 : nuit, sinon jour (`+0x3dc` nul la nuit) |
| 0x12 / 0x13 | l'équipe possède / ne possède pas l'objet `arg` |
| 0x14, 0x16, 0x19, 0x1C | quête `arg` dans l'état 2, 3, 1, 0 (et permise à un invité) |
| 0x15 | bit 2 de la quête `arg` |
| 0x17 | mode de session `arg` : 0 solo, 1 session, 2 solo ou drapeau de pair nul, 3 session et drapeau de pair |
| 0x1A / 0x1B | drapeau étendu (`arg >= 0x400` décalé de `0x6fa`) levé / baissé |
| 0x1D | mot 7 == (`arg`, opérande 0) et mot 10 == angle de l'opérande 1 (objet de l'événement 17) |
| 0x1E | mot 8 == `arg` |
| 0x1F | octet signé `+0x490` du gestionnaire == `arg` **[sens inconnu]** |
| 0x20 / 0x21 | octets de l'opérande (poids fort d'abord, 0 = indifférent) == octets `+0x2a..+0x2d` des paramètres, dans l'ordre / chacun à une place distincte |
| 0x22 | `arg` ≠ 0 : au moins deux membres dans l'équipe ; `arg` = 0 : moins de deux |
| 0x23 | sous-étape de la piste == `arg` |
| 0x24 | PV du héros (`+0x130 → +4`) nuls si `arg` ≠ 0, non nuls si `arg` = 0 (voir plus bas) |
| 0x25 / 0x26 | bit de la quête dans le tableau `+0x84` du journal levé / baissé **[sens inconnu]** |
| 0x27 / 0x28, 0x43 / 0x44 | drapeau `arg + 0x776`, `arg + 0x1136` |
| 0x29 | le meneur est (`arg` ≠ 0) / n'est pas (`arg` = 0) dans une zone de conversation du PNJ du mot 0 (rectangles du PNJ, bornes strictes, comme `Talk_NearestNpc`) ; faux si le PNJ n'est pas sur la carte |
| 0x2A | vocation du héros (`+0x150 → +0x950`) == `arg` (0 Gardien, 1 Guerrier… 12 Rôdeur, ordre du menu `str_sta`) |
| 0x2B | bits 19–29 de `+0x56a0` de la sauvegarde == `arg` **[sens inconnu]** |
| 0x2D / 0x2E | six champs de 10 bits du héros (`+0xc`, `+0x10`, `+0x14`) : le champ `arg` est le plus grand / dépasse (ou non) d'une limite le champ de `+4` **[sens inconnu]** |
| 0x2F | nombre d'emplacements du meneur (11 × 32 octets à `+0x194`) dont le bit 18 du mot `+8` est levé, comparé à l'opérande |
| 0x30 | au moins `opérande` exemplaires de l'objet `arg` |
| 0x31 | un membre de l'équipe (bit 0 d'état nul, PV non nuls) a la vocation `arg` et au moins le niveau `opérande` dans cette vocation |
| 0x32 / 0x52 | équipement : nœud 1 (arg, opérande 0, moitié haute de l'opérande 1 ; < 1 = indifférent) comparé aux emplacements 0, 1, 5, 6 de chaque membre (0x32) ou du personnage local (0x52), les membres acceptés vont dans le masque ; nœud 2 (moitié basse de l'opérande 1, opérande 2, moitié basse de l'opérande 3 ; 0 = indifférent) comparé aux emplacements 7 à 10 des membres du masque |
| 0x33 | l'entrée de la quête `arg` dans le journal a son bit 15 **[sens inconnu]** |
| 0x34 | PNJ == `arg`, drapeau `haut` du jeu 1 de la piste baissé, mode de session `bas` |
| 0x35 | PNJ == `arg`, condition de quête, zone == `haut` de l'opérande 1, réponse (0x10) == `bas` |
| 0x36 | PNJ == `arg`, condition de quête, l'équipe possède l'objet `haut` de l'opérande 1 |
| 0x37 | PNJ == `arg` et condition de quête (`haut` = quête, `bas` = condition codée comme `b` des dialogues, `Trig_QuestCondition`) |
| 0x38–0x3D | 0x37 puis : 0x24 (0x38), drapeau étendu et/ou mode de session (0x39–0x3D) |
| 0x3E / 0x3F | PNJ == `arg`, drapeau `haut` levé / baissé, mode de session `bas` |
| 0x40 / 0x41 | jusqu'à quatre drapeaux (`arg` et trois moitiés d'opérandes, négatif = absent) levés / baissés |
| 0x42 | plus de `bas` exemplaires de l'objet `haut` si `arg` ≠ 0, au plus `bas` sinon |
| 0x46 | bit 18 de l'entrée de la quête `arg` == (opérande ≠ 0) |
| 0x49 | demi-mot signé `+0x2ca` du gestionnaire ≥ opérande si `arg` ≠ 0, < sinon (remis à 0 par l'action 0xbb) |
| 0x4A | demi-mot `+0x71dc` de la sauvegarde == opérande |
| 0x4B / 0x4C | mot 9 == opérande |
| 0x4D | drapeaux 0x364 et 0x365 levés |
| 0x4E | valeur de l'entrée de journal de la quête `arg` (champ `haut`, demi-mots `+4..+0xe`) comparée à `bas` : 0 ≥, 1 <, 2 == (mode = opérande 1) |
| 0x50 | un membre (bit 0 d'état nul, PV non nuls) a au moins le niveau `arg` dans sa vocation actuelle |
| 0x51 | session active si `arg` ≠ 0, sinon inactive |
| 0x54 | octet `+0x64de` de la sauvegarde == `arg` |
| 0x55 | or (`+0x3970`, plafonné à 9 999 999 par l'action 0x8b) < opérande si `arg` = 0, ≥ sinon |
| 0x56 | le premier des personnages 0 à 3 dont l'octet `+0x56b` a un quartet bas non nul a des PV non nuls (sans tel personnage : le personnage 0xce existe) : `arg` = 1 si oui, 0 si non |
| 0x57 | journal de quêtes : moins de 8 quêtes si `arg` = 0, plein (8) sinon |
| 0x58 / 0x59 | drapeau `arg + 0x33e` levé / baissé (`arg` < 0x49) |
| 0x5A | date de l'horloge de la console == (mois `arg`, jour opérande) : 24 et 25 décembre dans les données |
| 0x5C | bit 9 de l'entrée de la quête `arg` nul |
| 0x5D / 0x5E / 0x5F | bit `arg` de `+0x238c` / mot `+0x237c` / demi-mot `+0x27b4` d'une structure globale (`FUN_02012ff4`) == `arg` **[sens inconnu]** |
| 0x60 | au moins `arg` vocations au niveau 99 pour le personnage local |
| 0x62 / 0x63 | octet global `+0x27d0` ≤ / ≥ `arg` |
| 0x1F4 | le premier membre dont le quartet haut de `+0x56b` vaut `arg` a son bit 0 d'état nul si opérande = 1, levé si 0 |
| 0x1F5 | `+0x3dc` (moment de la journée) == `arg` (0, 2, 3 dans les données) |
| 0x1F6 | octet global `+0x23a1`, remis à zéro par la lecture **[sens inconnu]** |
| 0x1F7 | drapeau 0x798 levé si `arg` ≠ 0, baissé sinon (écrit par l'action 0xe0) |
| 0x1F8 | date de l'horloge == date enregistrée à `+0x569c` (mois bits 12–15, jour 16–20) : « déjà fait aujourd'hui » **[hypothèse]** |
| 0x1F9 | bit 0 de `+0x63dc` de la sauvegarde |
| 0x1FA | bit 3 de la quête `arg` |
| 0x1FB | salle de destination (`+0x3f8`, écrite par les actions de transport) == `arg` |
| 0x18, 0x47, 0x48, 0x61, autres | toujours faux |

### Le « mot du héros » : PV du héros

0x24, 0x38 et les répliques de types 5/6 (`dialogue.md`) lisent le demi-mot
`personnage 0 → +0x130 → +4`. Ce sont les **PV** du héros **[hypothèse étayée]** :

- l'action 0xcb ne touche que les membres dont ce demi-mot vaut 0 et l'écrit avec
  `+0x134 → +0x30` (PV max), sa moitié ou 1 selon `arg` : une résurrection ;
- 0x31 et 0x50 ne comptent que les membres où il est non nul ;
- les répliques qui l'exigent nul s'adressent à un autre membre : la gardienne des bains de
  Gleeba (C02, PNJ 103) dit « you who saved our monarch » quand il est non nul, « you,
  friend of `<HERO>`… Be thankful that you know such a great one as `<HERO>` » quand il est
  nul ; 12 % des répliques de type 5 sont des silences `*: ......` (1 % pour le type 1),
  ce qui convient aux esprits que seul le héros entend.

## Actions (`Trig_RunAction`, `0x02061c14`)

`Trig_RunActions` (`0x02064540`) exécute la liste dans l'ordre et s'arrête à la première
action qui rend 0 ; les actions **différées** sont ajoutées à la liste `+0x30` du bloc de
paramètres (`Trig_Defer`, `0x0206446c`) et ne sont exécutées par `Trig_RunDeferred` que si
la liste est allée au bout. `dq9_core::trigger::run_action` reproduit le déroulement et les effets
sur `State` (drapeaux, pistes, quêtes, journal, or, objets, moment de la journée) ; les
effets sur ce que `State` ne contient pas (objets de carte, PNJ, menus, son,
synchronisation sans fil, compteurs de conversation, PV) sont ignorés.

`haut` / `bas` = moitiés de l'opérande 0 sauf mention.

| Code | Effet | Occurrences |
|---|---|---|
| 0x64 / 0x65 | lève / baisse le drapeau `arg` | 429 / 330 |
| 0x66 / 0x67, 0x68 / 0x69 | lève / baisse le drapeau `arg` du jeu 0, 1 de la piste | 57 / 8, 251 / 21 |
| 0x6a | remet à zéro les deux compteurs de conversation du PNJ `arg` | 55 |
| 0x6b | hôte d'une session : drapeau 0x113a | 53 |
| 0x6c / 0x88 | lève / baisse le bit « utilisée » (`0x4` des drapeaux) des portes de type 2 de classe `arg`, de numéro l'opérande 0 et d'orientation l'opérande 1 (degrés) ; une porte utilisée n'est plus touchée ; `0x6c` n'agit pas sur une porte au bit `0x1` (`Trig_RunAction`, cas 8 et `0x24`). Observé en RAM (« Zones ») | 502 / 28 |
| 0x6d | retire l'objet de carte | 179 |
| 0x6e | moment de la journée (`+0x3dc`) = `arg` (< 4) | 47 |
| 0x6f–0x71, 0x98, 0x9a, 0xa3, ≥ 0xea | aucun cas : **arrête** la liste | — |
| 0x72 / 0x74 † | donne l'objet `arg` (1 / `haut` exemplaires, avec message) et pose son drapeau `0xc76 + compendium` au premier exemplaire (`func_0207cc18`, [`item.md`](item.md)) | 378 / 4 |
| 0x73 / 0x75 † | retire l'objet `arg` (1 / `haut`), sac d'abord puis équipement | 163 / 26 |
| 0x76 † | conversation avec le PNJ `arg`, contexte `haut` | 3 378 |
| 0x77, 0x80 † | lance l'événement `arg` : les 524 valeurs de 0x77 sont des `data/event/evNNNNN.gp2`, `data/evspt_lv5/evNNNNN.gp2` ou des entrées de `eventlist6.bin` / `evl_quest.bin` (`stb.md`) | 622 / 0 |
| 0x78 † | lance le combat scripté `arg` (enregistrement d'index `arg` de `data/event/eventbattle.bin`), § « Combats scriptés » | 65 |
| 0x79, 0x89, 0xe1 † | transport vers la salle `arg` (structure `+0x3f8` de la sauvegarde) | 163, 0, 22 |
| 0x85, 0x8a, 0xe2 † | idem avec un point d'entrée `haut` | 30, 1, 5 |
| 0x7a / 0x7b † | position (3 flottants × 4096) / direction du transport | 179 / 33 |
| 0x7c † | retire le PNJ `arg` (drapeau 0x8000 de son objet) | 39 |
| 0x7d | inscrit la quête `arg` au journal ; **arrête** la liste si le journal est plein ou si la quête n'est pas proposée ; drapeau 0x119d | 258 |
| 0x7e | si la quête `arg` est dans l'état 2 : fenêtre de quête `+0x3bbc` **[sens inconnu]** | 1 |
| 0x7f | quête `arg` terminée : date enregistrée (journal `+0x178`), état 3, retrait du journal **[hypothèse pour le retrait]** | 209 |
| 0x81 | quête `arg` dans l'état 1 (proposable), bits 2–3 conservés | 2 |
| 0x82 / 0x83 | lève / baisse le drapeau étendu `haut` (`arg` = quête, pour la synchronisation) | 294 / 148 |
| 0x84 † | fait avancer la piste courante à (chapitre, étape, sous-étape) = moitiés basses des opérandes ; avant de différer, efface ses drapeaux si chapitre ou étape diffèrent | 217 |
| 0x94 † / 0xd6 † | idem pour les cinq pistes / pour la piste `arg` | 2 / 12 |
| 0x86 / 0x87 | lève / baisse le drapeau `arg` du jeu `haut` (3 scénario, 4 jeu 0, 5 jeu 1 de la piste) | 59 / 0 |
| 0x8b / 0x8c | ajoute / retire l'opérande (32 bits) à l'or, plafond 9 999 999, plancher 0 | 13 / 0 |
| 0x8d † | tâche `+0x3b58` si `arg` ≠ 0 **[sens inconnu]** | 236 |
| 0x8e | drapeau `0xbf1 + rang` de `arg` dans une table de 32 valeurs (`0x020e87d0`), rangs 17 à 31 seulement | 14 |
| 0x8f | ajoute une aire (événements 2/5), voir « Zones » ; s'exécute au fil de la liste, sans être différée | 116 |
| 0x90 | bits 11–13 de l'entrée de la quête `arg` = `haut` (quête dans l'état 2, pas invité) | 155 |
| 0x91 † | fenêtre choisie par `arg` **[sens inconnu]** | 49 |
| 0x92 / 0x93 | drapeau `arg + 0x776` | 0 |
| 0x95 / 0x96 | cache / montre **immédiatement** l'objet de carte : bloc = `arg & 0xff`, placement = moitié haute de l'opérande 0. `Trig_RunAction` cas 0x31/0x32 (`0x02061c14`) : `func_02019508(manager, arg, opérande)` puis `func_02013380(objet, arg, 0/1)`; si l'objet n'est pas chargé, `func_0206ea8c(manager, arg & 0xff, (opérande + 0x80) >> 16, …)` note l'état dans `+0x41b`. Émis en `Request::SetPlacementHidden { block, placement, hidden }`, dans l'ordre de la liste et avant les actions différées. `0x95(0, 0x100000)` = bloc 0, placement 16, caché (tour `X01M01`) | 342 / 97 |
| 0x97 | fenêtre `+0x3b60` avec `arg` **[sens inconnu]** | 23 |
| 0x9b † | drapeau `haut` du jeu 1 puis lance l'événement `arg` | 14 |
| 0x9c † | termine la quête `arg`, reprend l'objet `haut`, donne l'objet `bas` | 15 |
| 0x9d † | quête `arg` : inscription (`haut` = 0), fin (2) ou proposition (3), puis donne l'objet `bas` | 28 |
| 0x9e / 0x9f † | diffère 0x75 / 0x74 avec les mêmes valeurs | 0 |
| 0xa0, 0xca / 0xaf | lève `arg + 0x113f`, `arg + 0x1198` / baisse `arg + 0x1136` | 6, 6 / 7 |
| 0xa2 | drapeau 0x113b = (`arg` = 0) | 6 |
| 0xb0 | bit 17 de l'entrée de la quête `arg` = `haut` | 27 |
| 0xb1 | moment de la journée = `haut` | 16 |
| 0xba | drapeau 0x1138 = (`arg` ≠ 0) | 4 |
| 0xbb | `+0x2ca` du gestionnaire = 0 (condition 0x49) | 7 |
| 0xbc | valeur `haut` de l'entrée de la quête `arg` : = / + (plafond 0xffff) / − (plancher 0) `bas` selon l'opérande 1 | 29 |
| 0xbe / 0xbf | valeurs aléatoires du journal ; **arrête** la liste si la quête n'y est pas | 0 |
| 0xc5 | `+0x5cbc` de la sauvegarde = `arg` **[sens inconnu]** | 178 |
| 0xcb | ranime les membres à 0 PV : PV max (`arg` = 0), moitié (1) ou 1 | 2 |
| 0xce | drapeau 0x79a | 1 |
| 0xd3 | lève le bit `arg` de `+0x238c` (condition 0x5d) | 0 |
| 0xd8 | hôte : `+0x27b4` = `arg` (condition 0x5f) | 13 |
| 0xde | `+0x27d0` = 0 (conditions 0x62/0x63) | 3 |
| 0xdf / 0xe0 | drapeau 0x799 / 0x798 = (`arg` ≠ 0) | 2 / 4 |
| 0xe6 | drapeau `arg + 0xbea` = (`haut` ≠ 0) | 1 |
| 0xe8 | type de conversation du PNJ `arg` (bits 3–5 de l'octet 10 de sa définition) = `haut` | 1 |
| 0x99 | lève le bit 0x100 de l'objet de carte (type, numéro, angle) puis le retire | 13 |

† différée. Les autres codes présents dans les données (0xa1, 0xa4, 0xa6, 0xa7, 0xa9,
0xb2, 0xb4, 0xb9, 0xbd, 0xc7, 0xcc, 0xcd, 0xcf, 0xd0, 0xd4, 0xd5, 0xd7, 0xda–0xdd, 0xe3,
0xe5, 0xe7, 0xe9) touchent des fenêtres, le son, les sessions sans fil, l'équipement ou des
structures non identifiées ; ils ne changent pas le déroulement de la liste, sauf 0xdb (rend
le résultat d'une opération sur les objets, non modélisée). Absents des données, 0xc6 et
0xd1 l'arrêtent selon le nombre de joueurs d'une session.

Différées, dans l'ordre de la liste (`dq9_core::trigger::run_deferred`) : objets et progression sont
appliqués à l'état, le reste est rendu sous forme de demandes (`Request::Talk`, `Event`,
`Warp`, `RemoveNpc`). Une progression qui change recharge les triggers de la carte
(`Request::ReloadTriggers`).

## Drapeaux globaux posés à l'ouverture

Plusieurs mécanismes distincts écrivent des drapeaux globaux (`g_QuestManager+0x8c`) pendant
le prologue : la scène, la carte visitée, l'acquisition d'un objet et la touche tenue, ainsi
que deux actions de trigger (objet donné et hôte de session).

### Drapeau de scène (`EventTask_Update`, `ov_017::021bc77c`)

**Trouvé** en décompilant `EventTask_Update` (Ghidra, adresses EU), puis contrôlé sur les
états melonDS du premier contrôle (`build/orca/oracle/`, `build/orca/oracle-session-1711efc/`) :

- Au premier passage qui n'est pas bloqué par le chargement des fichiers de la scène (retour
  immédiat tant que `BackgroundLoader::GetNumQueuedTasks() > 0`, `0x020303a0`), la tâche pose
  deux drapeaux facultatifs sur son contexte `ctx` (pointeur `GameResources+0x3734`,
  `ov_017::0218b5b0`) :
  - `Flags_Set(manager, manager+0x8c, s16(ctx+0x52) + 0x38e, 1)` si `s16(ctx+0x52) >= 0` ;
  - `Flags_Set(manager, manager+0x8c, s8(ctx+0xa0) + 0xbea, 1)` si `s8(ctx+0xa0) >= 0` et
    `ctx+0x9c != 0`.
  Les deux champs valent `-1` à l'initialisation (`func_ov017_021bba08`, `ov_017::021bbae4`).
- L'index de scène vient du **rang de l'enregistrement dans `eventlist_lv5.bin`** : le lecteur
  de tables (`EventRecord_Parse`, `0x02071498`) ajoute à `ctx+0x52` la base 0x21c (540) pour
  les événements 21000..39999. Une seconde base 0x302 (770) existe dans le code de
  `Flags_SetStoryBases` (`0x02071698`, appelée par `main`) pour les autres enregistrements,
  mais elle n'est **pas observée** sur le prologue : les états originaux après 1130, 1150,
  1160 et 1600 ne portent ni 1680 ni 1681. Le rang (0, 1, 2…) vient du compteur de
  `Tbl_Run` incrémenté par l'enregistrement précédent (`EventRecord_Build`, `0x02071218`,
  `+4`), que `EventRecord_Parse` recopie en `ctx+0x46` lorsque l'enregistrement le demande,
  puis auquel il ajoute la base.
- Contrôlé en RAM sur les états réels : `ctx+0x52` vaut 540, 541, 542, 543, 544 et 551 pour
  `ev21500`, `ev21507`, `ev21520`, `ev21521`, `ev21522` et `ev21596`, soit les drapeaux
  **1450 à 1454 et 1461** ; ce sont les six drapeaux de scène du premier contrôle
  (`bat6.ml1`). L'état `pro1.ml1` montre déjà 540 dans `ctx+0x52` mais **pas** 1450 : la tâche
  était encore bloquée sur ses chargements, ce qui confirme que le drapeau n'est posé qu'au
  premier passage utile et non à la création du contexte.
- **Réimplémentation** (`dq9-core`) : `script::load::scene_flag` lit le rang dans
  `eventlist_lv5.bin` et ajoute la base puis `0x38e`, uniquement pour la plage
  d'ouverture observée — test `scene_flags_match_the_oracle_checkpoints` sur la ROM
  (`21500`→1450 … `21596`→1461) et test synthétique qui refuse 1130/1160/1600, sans liste
  de constantes d'événements. Le drapeau
  voyage sur `EventScript` (même lecture de la liste que le script, pas de parse séparé) ; le
  branchement est fait dans `dq9-app::begin_event`, après `EventPlayer::prepare` et avant la
  première mise à jour du script (jamais dans `GameState::new_game`) : c'est un effet de
  l'exécution des scènes.

### Drapeau de carte visitée (`Flags_SetMapVisited`, `0x0201ba78`)

**Trouvé** en décompilant le code appelé au chargement d'une carte (`0x02015448+200`) :
`Flags_SetMapVisited(Zone3D*)` lit l'identifiant de carte en `*Zone3D` et pose, dans le
gestionnaire de quêtes :

- `99 < id <= 9999` : drapeau `0xa96 + id / 100` (base 2710). Contrôlé sur les états : carte
  1112 (`M01M12`) → 11 → **2721**, présent dès `pro1` ; carte 4100 (`X01`) → 41 → **2751**,
  absente avant le combat 26 et présente après le chargement de `X01`.
- `20000 <= id <= 29999` : drapeau `0xa5a + f(id - 20000)` (`0x02012f78`, remappage 0x3c..0x3f,
  `-1` au-delà de 0x3b → aucun drapeau). La valeur exacte de `f` pour les cartes de champ du
  prologue n'est pas contrôlée.
- `func_0201b588()` (condition **[inconnue]**) pose le drapeau **1949** (`0x0201bb84`) : ce
  n'est pas un identifiant de carte, et `map_visit_flag(1949)` suit donc la branche générale
  (`2729`). La branche `0x02012f24` des cartes hors `99..=9999` et `20000..=29999` reste
  **[inconnue]**.
- Contrôlé : **2774** = 2710 + 64 est déjà posé en `pro1`, ce qui correspond au groupe de
  cartes 6400 (`S14`, Starflight Express). Le harnais libretro donne le chaînon : après la
  création du personnage, l'image 3762 a pour carte **6405** (`S14M05`, « For Events ») et
  area 6401, l'image 3764 porte 2774, et l'image 3864 est sur `M01M12` (1112). La carte du
  train est donc bien chargée avant le prologue ; la scène qui s'y joue n'est pas modélisée.
- **Réimplémentation** (`dq9-core`) : `state::map_visit_flag` reproduit la branche
  `99 < id <= 9999` et rend `None` pour les autres (non établies). Branchement : à l'entrée
  de carte, avant les triggers (`dq9-app::change_map`), pas dans `GameState::new_game` ;
  `dq9-app::start_new_game` charge `S14M05` (6405) avant de lancer la première scène, comme
  la course originale.

### Drapeau d'objet obtenu (`func_0207cc18`, `0x0207cc28`)

**Trouvé** en décompilant `func_0207cc18`, appelée par `Items_Give`, l'ajout au sac
(`func_0207cf30`) et l'équipement (`func_0207ccf0`) : au **premier** exemplaire d'un objet,
le jeu lit l'**indice de compendium** de son enregistrement
(`(record[0xc] & 0x7fffff) >> 0xc`, table `data/prm/itemdt_<catégorie>.gp2`) et pose
`Flags_Set(g_QuestManager, +0x8c, 0xc76 + indice, 1)`. Indice 0 : aucun drapeau.
Détail du format et table des objets : [`item.md`](item.md).

- **Équipement initial** : les quatre drapeaux posés ensemble à l'image 3740 de la course
  originale sont l'épée de cuivre (20004 → indice 1 → **3191**), le costume céleste (13007 →
  585 → **3775**), les bas célestes (16215 → 765 → **3955**) et les souliers célestes (17120 →
  856 → **4046**). L'épée vient de la table de création `ov_009` `0x0218636c` ; les trois
  pièces de tenue, de `HeroLook::new_game`.
- **Butin d'événement** : le trigger `triggerX01.bin`, room 4105, événement 11 de `21596`
  (`0x68(set_track_flag) 0x64(set_flag 337) 0x72(give_item 22159) 0x6b(0)`) donne la
  benevolessence (22159 → 1093 → **4283**), observée à l'image 21142 ; le même trigger pose
  337 et, par `0x6b`, **4410** (ci-dessous).
- **Réimplémentation** : `dq9-formats::item::ItemTable` lit les neuf tables ; `dq9-app` la
  remplit depuis la ROM ; `dq9-core::trigger::State::obtain` applique `0xc76 + compendium` au
  premier exemplaire (actions `0x72`/`0x74`, `0x9c`/`0x9d`), et `start_new_game` donne
  l'équipement de départ par la même règle. Aucune constante de drapeau n'est copiée : la
  table d'objets fournit l'indice.

### Drapeau de touche tenue (`KeyManager::RefreshKeys`, `0x020122ac`)

**Trouvé** dans `KeyManager::RefreshKeys` (décompilation USA `src/Input/Input.cpp`, adresse EU
`0x020122ac`) : à chaque image, si l'entrée est active, l'écran ouvert et la luminosité
suffisante, il lit les douze boutons ; si `currentKeys != 0`, il pose
`Flags_Set(g_QuestManager, +0x8c, 0x113e, 1)` — le drapeau **4414**. La course originale le
montre à l'image 1501, une image après le premier appui.

**Réimplémentation** : `dq9-app::run_steps` pose `state::KEY_REFRESH_FLAG` (0x113e) dès qu'une
touche est tenue (d-pad, touche d'action, pilotage `--auto` ou rejeu). Les conditions
d'écran/luminosité n'ont pas d'équivalent et passent toujours.

### Action `0x6b` : hôte de session (`Trig_RunAction`, cas 7)

**Trouvé** dans `Trig_RunAction` (`0x02061c14`, cas 7 = code `0x6b`) : si le prédicat de
session (`func_0202c508`, `0x0202c518`) est vrai, `Flags_Set(g_QuestManager, +0x8c,
*(u32*)0x02063008)` ; le mot lu en RAM vaut **0x113a**. En solo le runtime est hôte : le
prédicat est considéré vrai. Le trigger de fin de `21596` l'exécute avec l'argument 0, ce qui
donne **4410** à l'image 21137, en même temps que 337 et 1461.

### Reste inconnu

**Mesure après branchement** (`--new-game --headless --auto`, comparateur strict sur
`bat6.ml1` et sur l'image RAM `f000090.bin` du premier contrôle) : le runtime pose les
**17** drapeaux `337, 1450–1454, 1461, 2721, 2751, 2774, 3191, 3775, 3955, 4046, 4283, 4410,
4414` ; carte, piste, progression et `step_flags {1}` identiques. Écart : **4135** seul face à
`f000090.bin` (course féminine), **4212** seul face à `bat6` — jamais les deux.

- **4135** (objet 22000, herbe médicinale) et **4212** (objet 22087, slimedrop) sont du
  **butin de combat aléatoire** : trois courses réelles aux entrées identiques sauf le sexe
  donnent l'herbe, ou aucun des deux ; `bat6` donne le slimedrop. Le runtime substitue le
  combat 26 et ne tire pas de butin (`melonds-state.md`, § « Oracle différentiel ») : aucun
  drapeau n'est forcé pour le faire coïncider.
- La provenance des autres drapeaux est établie ci-dessus (équipement, carte, touche,
  trigger) ; il n'y a plus de constante copiée d'un checkpoint.

## Combats scriptés (action `0x78`)

**Trouvé** en lisant `Trig_RunDeferred` (`0x0206f82c`), cas `0x78` (Ghidra, adresses EU). Le
wiki DQIX (`Triggers.md`, opération 120) le dit aussi (« starts set battle *n* ») : nous l'avons
vérifié sur le code et les données avant de le reprendre.

- Le cas prend la tâche `+0x3718` du gestionnaire de champ, l'initialise (`BattleTask_Init`,
  `ov_017::021b6f18`), remplit une requête de `0x18` octets (octet 0 = 1, octet 1 = personnage local,
  demi-mot `+4` = `arg + 0x8005`, mot `+8` = `arg`, `+0xc`/`+0xd` = états lus sur le monstre de
  terrain et le type de carte au trésor par `ov_017::021b6e70`, octet `+0x12` = moment de la journée)
  puis la copie dans la tâche (`BattleTask_Setup`, `021b7104` : numéro de combat en `+0x28`,
  `arg + 0x8005` en `+0x24`) et la met en file (`0x020469b4`).
- La tâche (`BattleTask_Update`, `ov_017::021b790c`) conduit le combat. À sa fin, si `+0x28 ≥ 0`,
  elle construit le bloc de paramètres de `Trig_Fire` (`0x34` octets, **mot 6 = `+0x28`**) et appelle
  `Trig_FireBattleEnd` (`0x02064a18`, événement 15) ou, quand l'octet `+0x8e14` de la structure de
  combat vaut 2, `Trig_FireBattleEndAlt` (`0x02064a34`, événement 16), puis `Trig_RunDeferred`. C'est
  le mot 6 que lit la condition `0xc` (`12 : n` du wiki). Ce que vaut `arg + 0x8005` (identifiant
  de la tâche, comparé à `+0x44ae` des ressources de jeu à la fin) reste **[inconnu]**.
- Vérification sur les données (`tests/scenario_rom.rs`) : toutes les actions `0x78` des fichiers de
  triggers (65) nomment un enregistrement de `eventbattle.bin`, et chacune a, dans le même fichier
  et pour la même salle, un trigger d'événement 15 sur ce numéro, sans exception (le wiki en
  comptait 65 sur 66 par fichier).

Dans `dq9-core` : `Request::Battle(n)` (`run_deferred`), `scenario::battle(files, n)` lit
l'enregistrement (monstres, musique, arène), `Field::battle_ended(n, gagné, état)` tire 15 ou 16.
**Il n'y a pas de système de combat avant J4** : l'application ne joue pas le combat, elle le
déclare gagné aussitôt (`scenario::stand_in`, visible dans le journal et sur la sortie d'erreur) puis
tire l'événement 15, comme le ferait la tâche. Ce n'est pas un modèle du combat : il sert à atteindre
les scènes qui le suivent. Exemple du prologue : fin de `ev21507` (`M01`, salle 1112, trigger 11
`8 : 21507` → `0x78(26)`), combat 26 (monstres 290, 292 et 290, musique 23, arène 30116), victoire
(`triggerM01.bin` et `triggerF.bin`, événement 15, `12 : 26` → `0x77(21520)`).

## Portes sans destination et portes internes (native 584)

Une porte de type 2 dont l'action `0x74` n'a pas le drapeau `0x8` ne change pas de carte :
`Field_TouchDoorways` (`ov_017::02198f84`) appelle quand même la routine de porte
(`0x02018b44`, la même que la native 584). Elle trouve le placement dont l'entrée de bloc vaut
la **classe** de la zone et l'identifiant vaut son **numéro**, puis tourne ce placement et tout
son sous-arbre de `0x1922` (+90°) autour de son origine : 16 mises à jour de 2 images, `409` par
mise à jour (`Door::SPEED`, `Door::QUARTER_TURN` de `dq9_core::cutscene`). La collision enfant
suit la rotation (`CollisionWorld::set_door_angle`, delta `fx32`), sinon le passage reste fermé.
L'événement 17 part avant la rotation ; l'événement 3 (arrivée) part quand elle est finie. Le
son est `g_DoorSeTable[quartet bas de la valeur 3 du 0x74]`, effets de base **200 à 204**.
Preuve directe de la table : `dq9re state build/orca/oracle/bat6.ml1 --peek 0x020e6e84:14`
donne `c8000000 c9000000 ca000000 cb000000 cc000000` (200, 201, 202, 203, 204) ; le pointeur
`0x020e6e84` vaut `0x02018754`, `02018310` copie exactement cinq entrées et indexe
`flags & 0xf` : `200 + quartet bas` est juste pour les indices 0 à 4.

Preuves sur la ROM :

- X01M07 (bibliothèque) a **deux** zones au même volume `(-8.6, 0.5, -7.2)`, `1.5 × 2 × 2` :
  classe 0 numéro 2 (drapeaux `0x10`, son 2) et classe 0 numéro 3 (drapeaux `0x12`, son 2).
  Leurs placements 2 et 3 (entrée de bloc 0) portent chacun un enfant de collision (4 et 5).
  Le test `x01m07_inner_door_opens_the_way` : fermée, le héros s'arrête à `z = -6.45` ; après
  `set_door_angle(0, 2, +0x1922)` **et** `set_door_angle(0, 3, +0x1922)`, il traverse
  (`z < -8`). Un seul vantail ne suffit pas : les deux zones sont touchées ensemble.
- X01M01, X01M07, X01M05 : les zones `0x73` de type 1 (43 sur la ROM) sont **inertes** à
  l'exécution — aucune routine de `Field_CheckRegions` ne lit leur liste — ; ce ne sont ni un
  monte-charge ni une tâche 8. Voir la limite du parcours dans `app.md`.

## Fin de conversation (événement 1)

`Field::talk_ended(pnj, contexte, réponse, état)` tire l'événement 1 quand la fenêtre d'une
conversation se ferme : mot 0 = PNJ, mot 5 = contexte qui a choisi la réplique (celui du trigger
d'événement 0, `0x76(pnj, contexte)`, sinon la zone), `+0x954` = réponse au dernier `<YESNO>`
(0 = oui ; 0 sans question, car la fenêtre le met à 0 à l'ouverture). C'est la suite des répliques :
`0x6(101) 0xb(193) 0x10(0)` → `0x77(21591)` (descendre vers Angel Falls après avoir répondu oui au
gardien de l'Observatoire). Branché dans `dq9-app` (`Talk::take_ended`). Ce trigger est rangé
sous **1.2..1.2** (`dq9re triggers X01 --with 21591`) : il n'est pas chargé à la sous-étape
1.1.2. La suite d'origine passe par `4101 → 4103 → 4109 → 4103 → 4104`, l'événement `21510`
(1.2.1) puis `21591` ; une route directe `21591` après 1.1.2 est exclue par les conditions
d'origine.

## Chaîne du prologue d'Angel Falls (1.1.2 → 1.4.1)

Établie sur la ROM européenne avec `dq9re stage-events 1.1-3`, `dq9re triggers`, `dq9re cast`,
`dq9re talk`, `dq9re stb` et `dq9re script 1.2-3` ; les dumps et traces sont sous
`build/v01-20261107/village/` (ignoré). Chaque transition ci-dessous est la **seule** action
`0x84` qui atteint ce point d'histoire : un point d'histoire n'avance que par une action de
trigger, jamais par une scène. Les scènes ne sont que des `0x77` ; elles ne modifient la
progression que par le trigger de l'événement 11.

### 1.1.2 → 1.2 : Yggdrasil et l'Observatoire

| Étape | Preuve (`dq9re triggers`) | Interaction |
|---|---|---|
| 1.1.2, `X01M04` (4104), Yggdrasil | `room 4104 1.1..1.1 event 1: if 0x6(199) 0xb(80) 0x10(0) then 0x77(21510)` | PNJ 199 de type 1 (point d'examen, invisible) à `(0, 9.1, -3.04)`, zone 80 ; répondre **oui** à `Offer the benevolessence up to Yggdrasil?` |
| fin de `21510` | `room 4104 1.1..1.1 event 11: if 0x8(21510) then 0x73(22159) 0x84(0, 1, 2, 1) …` | retire la benevolessence, passe en **1.2.1** |
| 1.2.1, `X01M07` (4107), Apus Major | `room 4107 1.2..1.3 event 0: if 0x6(139) 0x5(0) then 0x76(139, 192) 0x68(0)` | dialogue 192 (« Well done… »), puis contexte 193 (« Make ready, and then speak with the female Celestrian ») ; l'action `0x68(0)` lève le drapeau **jeu 1 n° 0** |
| `X01M05` (4105), gardienne 101 | `room 4105 1.2..1.2 event 0: if 0x6(101) 0x4(0) 0x52(13007, …) then 0x76(101, 193)` ; `event 1: if 0x6(101) 0xb(193) 0x10(0) then 0x77(21591)` | la condition `0x52(13007)` exige la tenue céleste, portée depuis la création ; la ligne 193 est la question « descendre vers le Protectorat ? » ; répondre **oui** |
| fin de `21591` | `room 4105 1.2..1.2 event 11: if 0x8(21591) then 0xe1(1100)` | transport vers la carte **1100** (`M01`) |

### 1.2 : arrivée et tâches du village

À l'arrivée avec transition, `Field::enter` joue l'événement 20 puis l'événement 3 :

- événement 20 (`room 1100 1.1..1.2`) : `0x95(0, 0x1a0000)` (cache le placement 26),
  `0x96(0, 0x1b0000)` (montre 27), `0x95(0, 0x1c0000)` (cache 28), puis
  `0x8f(10, -2, -1, 21, -10, -2, 15, 0)` : aire **10**, autour de l'entrée du village ;
- événement 3 (`1.2..1.2`, drapeau jeu 1 n° 1 baissé) : `0x97(1)`, `0x77(21592)`,
  `0x68(1)`, `0xc5(3)`. `21592` est la scène d'Ivor (trois messages `//Ivor//`) ; elle se
  termine sans autre trigger.

Le point de départ de `M01` est `(-6.45, -1.95, 13.55)`, hors de l'aire 10 ; en marchant vers
le nord (l'entrée à `z ≈ 18`) on entre dans l'aire :

| Étape | Preuve | Interaction |
|---|---|---|
| aire 10 atteinte | `room 1100 1.2..1.2 event 2: if 0x7(10) then 0x76(230, 192)` | conversation automatique avec le PNJ invisible 230 (`-6.51, -1.85, 17.91`) |
| fin de cette conversation | `event 1: if 0x6(230) 0xb(192) then 0x77(1310)` | scène 1310 (fondu, caméra, héros ramené à `(-6.45, -1.85, 13.55)`) |

Les trois tâches qui font avancer 1.2 (drapeaux **jeu 1** 3, 4 et 5) :

| Tâche | Preuve | Effet |
|---|---|---|
| **Anneau de l'esprit** : parler au PNJ 210 (`s097`, `26.91, -1.05, 2.16`) | `room 1100 1.2..1.2 event 0: if 0x6(210) 0x5(3) then 0x77(1390)` ; `event 11: if 0x8(1390) then 0x72(22158) 0x68(3) 0x7c(210)` | scène 1390 (`<HERO> acquires a keepsake ring`), objet **22158**, drapeau jeu 1 n° 3, PNJ 210 retiré |
| **La veuve** : parler au PNJ 12 de `M01M06` (1106), `1.50, 0.15, 0` | avec le drapeau 3 levé : `event 0: if 0x6(12) 0x4(3) 0x5(4) then 0x76(12, 193)` ; `event 1: if 0x6(12) 0xb(193) 0x10(0) then 0x77(1330) 0x73(22158) 0x72(22159) 0x68(4)` | ligne 193 (« Please, Guardian… help to reunite my husband's ring ») avec choix ; répondre **oui** ; scène 1330 (l'anneau glissé dans le manteau), l'objet 22158 est pris, la benevolessence **22159** donnée, drapeau jeu 1 n° 4 |
| **L'écurie** : parler aux PNJ 211 à 215 de `M01M04` (1104) | `event 0: if 0x6(211) 0x1(316) then 0x76(211, 192)` ; `event 1: if 0x6(211) 0x1(316) 0x0(317) 0x0(318) 0x0(319) 0x0(320) 0x5(5) then 0x72(22079) 0x64(316) 0x7c(211) 0x77(1340)` ; variante sans les quatre autres drapeaux pour les quatre premiers | chacun des cinq donne **22079**, lève son drapeau (316…320) et est retiré ; le **cinquième** (les quatre autres drapeaux levés, drapeau jeu 1 n° 5 baissé) joue 1340 : le palefrenier remercie, la benevolessence 22159 est donnée (`event 11: if 0x8(1340) then 0x68(5) 0x64(315) 0x72(22159)`) |

La **sortie de maison** est le verrou de 1.2 → 1.3 : la zone `0x72` de 1104 ou de 1106
déclenche l'événement 6 (`room 1104 1.2..1.2 event 6: if 0x9(1104) 0x4(4) 0x4(5) then
0x66(1) 0xb1(1, 0) 0x84(0, 1, 3, 1)`, variante 1106 avec `0x66(0)`). Il faut donc les
drapeaux jeu 1 n° 4 (1330) **et** n° 5 (1340) ; le drapeau jeu 0 (0 ou 1 selon la maison)
choisit la scène d'arrivée 1350 ou 1360. `0xb1(1, 0)` passe le moment de la journée à **0
(nuit)**.

Scène **non obligatoire mais d'histoire** : le chien 95 (`n097a`, `20.29, -1.05, -0.67`) →
`if 0x6(95) 0x5(2) 0x5(3) then 0x77(1320)` ; la fin de 1320 fait `0x68(2)` et avance en
**1.2.2** (`0x84(0, 1, 2, 2)`). Aucun verrou de 1.2 → 1.3 ne teste la sous-étape : la scène
peut être vue à tout moment avant de sortir, et son oubli ne bloque pas la transition.

### 1.3 : nuit, âme errante et retour

- événement 20 (`room 1100 1.3..1.4`) : `0x96(0, 0x1a0000)`, `0x96(0, 0x1b0000)`,
  `0x95(0, 0x1c0000)`, puis `0x8f(12, 3, 0, 0, 0, -1, -4, 17)` : aire **12** ;
- événement 3 : `0x2(0)` (drapeau jeu 0 n° 0 levé) → `0x77(1350)` ; `0x2(1)` → `0x77(1360)` ;
  la variante jouée lève le drapeau jeu 0 n° 2 et baisse le drapeau global 315 ;
- aire 12 (x `0..3`, z `-4..0`) → `room 1100 1.3..1.3 event 2: if 0x7(12) 0x5(0) then
  0x77(1410)` : leçon d'Aquila sur les âmes errantes ; sa fin lève le drapeau jeu 1 n° 0 ;
- PNJ 94 (`n007g`, `15.76, -1.05, -6.78`) →
  `room 1100 1.3..1.3 event 0: if 0x6(94) 0x5(1) then 0x77(21595) 0x72(22159)` : la scène de
  l'âme errante. Elle n'ouvre aucune conversation (pas de `0x76`), donc aucun fichier de
  répliques `094_en.bin` n'est nécessaire.

Fin de `21595`, cas normal de l'original : le script appelle la native **559**
(`ov_001::021609e4`) avec 1 sur le chemin normal (`559(1)` juste avant le fondu final de
`mark(13)` ; `559(0)` seulement sur la branche de `mark(12)`), ce qui écrit `+0x490` du
gestionnaire de quêtes. Le trigger de fin prend donc la branche
`event 11: if 0x8(21595) 0x1f(1) then 0x84(0, 1, 4, 1) 0x97(2) 0x85(4105, …)`: retour
direct à l'Observatoire (`4105`) en **1.4.1**. Le runtime ne modélise pas 559 : `+0x490`
reste 0 et il prend `0x1f(0)` (`0x68(1) 0x84(0, 1, 3, 2) 0x8d(1)`), puis le repli
d'origine : parler à Aquila 99 (`s019n`, `4.46, -1.05, -2.06`), ligne de sous-étape 2 avec
choix, `event 1: if 0x6(99) 0xb(0) 0x10(0) 0x4(1) then 0x77(21593)` ; la fin de 21593 fait
`0x84(0, 1, 4, 1) 0x97(2) 0x85(4105, …)`. Les deux routes atteignent 1.4.1, mais l'original
ne joue pas 21593.

### Tâches d'histoire et quêtes numérotées

- Les tâches 1330 et 1340 sont des **tâches d'histoire**, pas des quêtes numérotées : aucune
  action de journal (`0x7d`, `0x7f`, `0x90`, `0x9c`/`0x9d`) ; leur état tient dans les
  drapeaux jeu 1 n° 3–5 et les drapeaux globaux 315 et 316–320.
- Les quêtes numérotées des mêmes cartes (quête 7 chez le PNJ 1, quête 103 chez le PNJ 1,
  quête 193 à l'auberge d'Ivor) sont des conditions `0x14`/`0x19`/`0x1a` de triggers
  `1.1..19.99` : elles restent disponibles sans jamais intervenir dans le verrou
  1.2 → 1.3. `dq9re triggers M01 --with 7` les montre à côté des lignes du prologue.
- La scène 1610 (`X01M07`, PNJ 130, réponse **non**) est du lore (la disparition d'un
  Guardian) ; elle ne touche ni progression ni quête.
- `1370` (Ivor, choix) et `1380` (PNJ 2) sont des scènes de couleur locale : `0x68(13)` et
  `0x66(7)` respectivement, aucune action `0x84`.

### Écarts connus sur ce chemin

1. **Native 559** (`ov_001::021609e4`) : écrit l'octet `+0x490` lu par la condition `0x1f`.
   Sans elle, 21595 prend le repli 1.3.2 + 21593 au lieu du retour direct 1.4.1. Contrat
   proposé : un `Event` du script (ou une commande de type `Raw`) écrit `Raw::Quest490` dans
   l'état de l'application avant que `Field::event_ended(21595)` ne soit appelé ; fichiers
   nécessaires : `dq9-core/src/script/natives.rs`, `dq9-core/src/cutscene.rs`,
   `dq9-app/src/app.rs` (et `dq9-app/src/event.rs`, propriété de la tour). L'assertion doit
   vérifier `Raw::Quest490 == 1` après la scène et le warp `4105` tiré par le trigger 11.
2. **Moment de la journée du rendu** : `0xb1(1, 0)` met `state.time_of_day` à 0 avant le
   changement de carte, et `story_from_state` en dérive le cast nuit, mais `dq9-app` garde son
   propre `self.time`/`DayClock` pour l'éclairage (`load::load(..., self.time, ...)`) : la
   scène 1.3 est donc éclairée de jour. Contrat proposé : synchroniser `self.day`/`self.time`
   (sans recharger la carte, `set_time` recharge) quand `state.time_of_day` a changé entre
   deux lots d'événements, en ne touchant pas aux options `--time`/`--clock` explicites.
   Fichiers : `dq9-app/src/app.rs` (+ `lighting.rs` déjà fourni).
3. **Retrait de PNJ (0x7c)** : `Request::RemoveNpc` n'enlève le PNJ que de la liste de
   conversation (`Talk::remove_npc`) ; l'instance du modèle reste dans la scène rendue. Les
   cinq tas de l'écurie et l'esprit 210 restent donc visibles après leur scène. Assertion
   proposée : après application de `RemoveNpc(210)` puis de 1390, l'instance du cast 210 est
   masquée (et idem pour 211–215 après 1340).
4. `0x8d` (`Request::Other(141)`), `560` et `571` ne changent pas le déroulement observé :
   les scènes 1.2–1.3 vont à leur terme (`dq9re script 1.2-3`) ; aucun correctif n'est
   établi pour l'instant.

### Reproduction

```powershell
$ROM = "rom/Dragon Quest IX - Sentinels of the Starry Skies (Europe) (En,Fr,De,Es,It).nds"
dq9re --rom $ROM stage-events 1.1-3
dq9re --rom $ROM triggers X01 --story 1.2
dq9re --rom $ROM triggers M01 --story 1.2
dq9re --rom $ROM triggers M01 --story 1.3
dq9re --rom $ROM triggers M01 --with 21510
dq9re --rom $ROM triggers M01 --with 1330
dq9re --rom $ROM script 1.2-3 --quiet
# diagnostic isolé (ne prouve pas le parcours continu) :
dq9-app --rom $ROM --events-report 1.2-3 --mute
dq9-app --rom $ROM --event 21591 --chapter 1 --step 2 --substep 1 --headless --headless-steps 4000 --press-every 30 --mute
dq9-app --rom $ROM --event 21595 --chapter 1 --step 3 --substep 1 --night --headless --headless-steps 6000 --press-every 20 --mute
```

### Assertions de régression proposées

- **Émission 0x95/0x96** (faite, `field_triggers_emit_placement_hide_and_show`) : un trigger
  synthétique porte `0x95(0x1234, 0x00100000)` puis `0x96(0x01ff, 0x00070000)` et un
  `0x77(5000)` différé ; `Field::event_ended` rend
  `[SetPlacementHidden{block:0x34, placement:16, hidden:true},
  SetPlacementHidden{block:0xff, placement:7, hidden:false}, Event(5000)]`.
- **Verrou de sortie** : état 1.2.x, drapeaux jeu 1 4 et 5 levés, meneur dans la zone `0x72`
  de 1104 → un pas rend `MapChange{destination:1100, cause:Warp}`, `state.time_of_day == 0`
  et la piste (1, 3, 1) ; avec un seul des deux drapeaux, aucun changement de carte.
- **1330** : état 1.2.1 avec drapeau jeu 1 n° 3, événement 1 du PNJ 12, contexte 193,
  réponse 0 → `Event(1330)`, 22158 retiré, 22159 donné, drapeau 4 levé (et drapeau
  d'obtention 4283).
- **1340** : événement 1 du PNJ 211 avec les drapeaux 317–320 levés et 316 baissé →
  `Event(1340)` et `RemoveNpc(211)` ; avec 317 seulement, pas d'événement mais 22079 donné,
  316 levé et `RemoveNpc(211)`.
- **559** : après exécution de 21595, `Raw::Quest490 == 1` et le trigger 11 rend le warp
  4105 avec la piste (1, 4, 1), sans 21593.

## Exemples

George à Stornway (`triggerM02`, salle 1200, chapitre 15 = lettre M) quand la quête 11 est
dans l'état 1 (test `talk_triggers_force_the_dialogue_context`) :

```
si 0x37(18) [quête 11, cond. 3]  0x57(0)  0x50(25)  alors 0x76(18) contexte 202  → offre de la quête
si 0x37(18) [quête 11, cond. 3]                     alors 0x76(18) contexte 203  → « if only there was someone… high enough level »
```

Brunhild à Gleeba (`triggerC02`, salle 201, piste 3) :
`si 0x6(210) 0x19(30) 0x31(9)[15] alors 0x76(210) contexte 214` → « So, you have reached
the fifteenth level as a paladin » (vocation 9 = Paladin, niveau 15).

Panneau 137 du château de Stornway (`triggerC01`, salle 100) : `si 0x29(1) 0x6(137) alors
0x76(137) contexte 202` → « `<LEADER>` reads the sign » seulement depuis la zone du
panneau.
