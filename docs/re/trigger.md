# Triggers de scénario (`data/scenario/trigger<carte>.bin`)

Implémentation : `crates/dq9-formats/src/trigger.rs` (`parse`, `Trigger::loaded`,
`first_match`, `eval`). Outil : `cargo dq9re talk` passe d'abord par les triggers de
conversation de la carte (`dialogue.md`).

Règles « si conditions alors actions » déclenchées par des événements du jeu. Ce sont elles
qui fixent le **contexte de conversation** 192–223 qu'attendent la moitié des répliques des
PNJ (offres et récompenses de quêtes).

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
(`Save_Chapter`, `Save_ScenarioStep`). `Trig_Register` le range dans la chaîne de son type
d'événement, dans l'ordre du fichier.

76 fichiers, 5 895 triggers, tous de type 1 (test `trigger_files_compile`).

| Événement | Triggers | Sens |
|---|---|---|
| 0 | 2 831 | parler à un PNJ (`Talk_Start` : paramètres `[pnj, 0, 0, 0, 0, zone]`) |
| 1 | 1 143 | **[inconnu]** |
| 11, 17, 3, 20 (0x14, déclenché à la fin du chargement), 2… | 1 921 | **[inconnus]** |

## Mots (`Trig_CompileWords`, `0x0205ec80`)

Chaque mot vaut `code << 16 | arg` et est suivi d'un nombre fixe de champs d'opérandes, propre
au code (table dans `trigger::operand_count`, extraite du `switch` d'actions et de la table
de sauts des conditions à `0x0205ed1c`). Les codes 100 à 499 sont des **actions**, les autres
des **conditions** ; le compilateur les sépare en deux listes. Contrôle : les mots des 5 895
triggers tombent exactement sur la fin de leur enregistrement.

Beaucoup d'opérandes portent deux valeurs de 16 bits : moitié haute (signée) et moitié basse.

## Choix (`Trig_FindMatch`, `0x020644a0`)

Pour un événement, les triggers de sa chaîne sont essayés dans l'ordre ; le premier dont
**toutes** les conditions sont vraies (évaluées dans l'ordre, arrêt au premier faux) est
retenu et ses actions exécutées (`Trig_Fire`, `0x020649c0`). Si aucun ne l'est, le jeu suit
son comportement par défaut (pour une conversation : contexte = zone).

## Conditions réimplémentées (`Trig_EvalCondition`, `0x0205fb04`)

96 % des 7 328 conditions des triggers de conversation (2 533 triggers sur 2 831 entièrement).
`first_match` ne renvoie une erreur que si une condition non réimplémentée est réellement
atteinte.

| Code | Vrai si |
|---|---|
| 0 / 1 | drapeau de scénario `arg` levé / baissé (tableau de bits du gestionnaire de quêtes `+0x8c`) |
| 2 / 3, 4 / 5 | drapeau `arg` d'un des deux jeux par joueur levé / baissé **[sens inconnu]** |
| 6…0xC | paramètre `code-6` de l'événement == `arg` (6 : PNJ, 0xB : zone) |
| 0x11 | `arg` = 1 : nuit, sinon jour |
| 0x12 / 0x13 | l'équipe possède / ne possède pas l'objet `arg` |
| 0x14, 0x16, 0x19, 0x1C | quête `arg` dans l'état 2, 3, 1, 0 (et permise à un invité) |
| 0x15 | bit 2 de la quête `arg` |
| 0x17 | mode de session `arg` : 0 solo, 1 session, 2 solo ou drapeau de pair nul, 3 session et drapeau de pair |
| 0x1A / 0x1B | drapeau étendu (`arg >= 0x400` décalé de `0x6fa`) levé / baissé |
| 0x1E | paramètre 8 de l'événement == `arg` |
| 0x23 | `Save_ScenarioSubstep` == `arg` |
| 0x24 | mot du héros nul si `arg` ≠ 0, non nul si `arg` = 0 |
| 0x25 / 0x26 | bit de la quête dans le journal `+0x84` levé / baissé **[sens inconnu]** |
| 0x27 / 0x28, 0x43 / 0x44 | drapeau `arg + 0x776`, `arg + 0x1136` |
| 0x30 | au moins `opérande` exemplaires de l'objet `arg` |
| 0x33 | l'entrée de la quête `arg` dans le journal a son bit 15 **[sens inconnu]** |
| 0x34 | PNJ == `arg`, drapeau joueur 1 `haut` baissé, mode de session `bas` |
| 0x37 | PNJ == `arg` et condition de quête (`haut` = quête, `bas` = condition codée comme `b` des dialogues, `Trig_QuestCondition`) |
| 0x38–0x3D | 0x37 puis : mot du héros (0x38), drapeau étendu et/ou mode de session (0x39–0x3D) |
| 0x3E / 0x3F | PNJ == `arg`, drapeau `haut` levé / baissé, mode de session `bas` |
| 0x40 / 0x41 | jusqu'à quatre drapeaux (`arg` et trois moitiés d'opérandes, négatif = absent) levés / baissés |
| 0x50 | un membre actif de l'équipe a au moins le niveau `arg` (dans sa vocation actuelle) |
| 0x51 | session active si `arg` ≠ 0, sinon inactive |
| 0x57 | journal de quêtes : moins de 8 quêtes si `arg` = 0, plein (8) sinon |

Non réimplémentées pour l'instant (ordre de fréquence dans les triggers de conversation) :
0x29 (héros dans une zone de conversation du PNJ), 0x31, 0x2A (vocation), 0x36, 0x1F7,
0x1F4 (500), 0x52, 0x32, 0x49…

## Actions

Seule l'action 0x76 est réimplémentée : **commencer la conversation avec le PNJ `arg`**, avec le
contexte = moitié haute de l'opérande (`Trig_RunDeferred`, `0x0206f82c` : écrit `+0x114`
et `+0x124` de la tâche de conversation). Les contextes de toutes les actions 0x76 sont 0, 80–131,
160, 176 et 192–223 : des groupes (0, 80) ou des emplacements exacts des fichiers de dialogue.

Exemple (test `talk_triggers_force_the_dialogue_context`), George à Stornway
(`triggerM02`, salle 1200, chapitre 15 = lettre M) quand la quête 11 est dans l'état 1 :

```
si 0x37(18) [quête 11, cond. 3]  0x57(0)  0x50(25)  alors 0x76(18) contexte 202  → offre de la quête
si 0x37(18) [quête 11, cond. 3]                     alors 0x76(18) contexte 203  → « if only there was someone… high enough level »
```

L'état 1 signifie donc « quête disponible », et non « proposée » : c'est un trigger qui vérifie
le niveau (25) avant de faire proposer la quête.
