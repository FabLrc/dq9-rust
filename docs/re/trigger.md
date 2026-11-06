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

**Non modélisé** : les actions qui touchent les objets de carte (`0x6d`, `0x95`/`0x96`,
`0x99`…), l'animation de la porte et la marche automatique, les gardes de
`ov_017::0219755c`, les zones des autres types.

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
| 0x72 / 0x74 † | donne l'objet `arg` (1 / `haut` exemplaires, avec message) | 378 / 4 |
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
| 0x95 / 0x96 | cache / montre l'objet de carte (`arg`, `haut`), ou note l'état dans `+0x41b` s'il n'est pas chargé **[hypothèse sur le sens]** | 342 / 97 |
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

Deux mécanismes distincts écrivent des drapeaux globaux (`g_QuestManager+0x8c`) pendant le
prologue ; ils ne passent ni par les triggers ni par les natives des scripts.

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
  les événements 21000..39999, ou 0x302 (770) au-delà ; ces deux bases sont posées au
  démarrage par `Flags_SetStoryBases` (`0x02071698`, appelée par `main`). Le rang (0, 1, 2…)
  vient du compteur de `Tbl_Run` incrémenté par l'enregistrement précédent
  (`EventRecord_Build`, `0x02071218`, `+4`), que `EventRecord_Parse` recopie en `ctx+0x46`
  lorsque l'enregistrement le demande, puis auquel il ajoute la base.
- Contrôlé en RAM sur les états réels : `ctx+0x52` vaut 540, 541, 542, 543, 544 et 551 pour
  `ev21500`, `ev21507`, `ev21520`, `ev21521`, `ev21522` et `ev21596`, soit les drapeaux
  **1450 à 1454 et 1461** ; ce sont les six drapeaux de scène du premier contrôle
  (`bat6.ml1`). L'état `pro1.ml1` montre déjà 540 dans `ctx+0x52` mais **pas** 1450 : la tâche
  était encore bloquée sur ses chargements, ce qui confirme que le drapeau n'est posé qu'au
  premier passage utile et non à la création du contexte.
- **Réimplémentation** (`dq9-core`) : `script::load::scene_flag` lit le rang dans
  `eventlist_lv5.bin` et ajoute la base puis `0x38e` — test `scene_flags_match_the_oracle_checkpoints`
  sur la ROM (`21500`→1450 … `21596`→1461, sans liste de constantes d'événements). Le
  branchement (insertion dans l'état du champ au démarrage effectif de la scène) est fait dans
  `dq9-app`, pas dans `GameState::new_game` : c'est un effet de l'exécution des scènes.

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
- carte 1949 (constante `re/symbols.txt`) et branche `0x02012f24` : **[inconnu]**.
- Contrôlé : **2774** = 2710 + 64 est déjà posé en `pro1`, ce qui correspond au groupe de
  cartes 6400 (`S14`, Starflight Express) ; [hypothèse] une carte `S14` est chargée avant le
  prologue (scène d'ouverture), ce que le runtime ne rejoue pas.
- **Réimplémentation** (`dq9-core`) : `state::map_visit_flag` reproduit la branche
  `99 < id <= 9999` et rend `None` pour les autres (non établies). Branchement : à l'entrée
  de carte, avant les triggers (`dq9-app::change_map`), pas dans `GameState::new_game`.

### Reste inconnu

**Mesure après branchement** (`--new-game --headless --auto`, comparateur strict sur
`bat6.ml1`) : le runtime pose 337, **1450 à 1454**, **1461**, **2721** et **2751**. Écart
restant : **2774** et **3191, 3775, 3955, 4046, 4212, 4283, 4410, 4414**.

- **2774** est expliqué par la règle de carte visitée (`2710 + 64`, groupe `S14`) : la carte
  du train n'est pas chargée avant le prologue par le runtime, qui ne rejoue pas cette
  scène. Ce n'est pas un écrivain inconnu mais un chaînon de parcours manquant.
- Les huit autres n'ont **aucun écrivain trouvé** : ni dans les 41 appelants de `Flags_Set`
  décompilés, ni dans les 76 `trigger*.bin`, ni dans les `.bmbl`/`.bmdj`/`.npc`/`place.bin`
  (tous les `*.npc` du dépôt parcourus), ni dans les tables `eventlist*.bin`/`evl_quest.bin`.
  Contrôles négatifs supplémentaires : aucun bloc de 0x234 octets prérempli avec les
  drapeaux de `pro1`/`bat6` dans l'ARM9 ni les overlays (pas de modèle recopié) ; les bases
  d'objets/placement 0x212/0x33e/0x79e/0x776 ne donnent ces valeurs pour aucun
  enregistrement de `place.bin` (les correspondances trouvées sont des identifiants de PNJ) ;
  la base « objet obtenu » 0xc76 ne donne que 3192–3194 pour l'équipement de départ ; aucun
  enregistrement de scène ne porte l'indice négatif ou la grande valeur qu'il faudrait.
  Ne pas coder ces constantes : leur provenance n'est pas établie. Piste suivante :
  instrumentation de melonDS (GPL, outil seulement) pour journaliser l'appelant et l'indice
  de `Flags_Set` entre le boot et `pro1` ; le stub GDB de melonDS 1.1 coupe la connexion,
  aucune trace PC n'existe encore.

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
- X01M01, X01M07, X01M05 : les zones `0x73` de type 1 (43 sur la ROM) sont lues par le champ
  (tâche 8) mais **restent non modélisées** ; voir la limite du parcours dans `app.md`.

## Fin de conversation (événement 1)

`Field::talk_ended(pnj, contexte, réponse, état)` tire l'événement 1 quand la fenêtre d'une
conversation se ferme : mot 0 = PNJ, mot 5 = contexte qui a choisi la réplique (celui du trigger
d'événement 0, `0x76(pnj, contexte)`, sinon la zone), `+0x954` = réponse au dernier `<YESNO>`
(0 = oui ; 0 sans question, car la fenêtre le met à 0 à l'ouverture). C'est la suite des répliques :
`0x6(101) 0xb(193) 0x10(0)` → `0x77(21591)` (descendre vers Angel Falls après avoir répondu oui au
gardien de l'Observatoire). Branché dans `dq9-app` (`Talk::take_ended`).

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
