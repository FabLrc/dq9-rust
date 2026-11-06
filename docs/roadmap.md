# Feuille de route

Objectif : DQ9 entièrement réimplémenté en Rust (natif et Web), avec une **première version
jouable le plus tôt possible**, en s'appuyant au maximum sur le travail public existant
(`ressources-externes.md`). Chaque étape se termine par une doc dans `docs/re/`, une
implémentation testée et un commit.

## Point de reprise — pause demandée le 6 novembre 2026

**Travail suspendu à la demande de l'utilisateur ; la v0.1 n'est pas validée.** Les agents
OMP (`opencode-go/deepseek-v4.1-flash`) sont arrêtés, leurs branches poussées et leurs
worktrees fermés après conservation des preuves locales. Ne pas confondre les lots livrés
avec un prologue intégralement jouable.

### Intégré et publié

- Dernier commit de jeu : **`445de2b`** (intégration `v01-field`) ; outillage : **`829db11`**.
  CI Linux [42274982535](https://github.com/FabLrc/dq9-rust/actions/runs/42274982535) et
  déploiement [Pages 42274980383](https://github.com/FabLrc/dq9-rust/actions/runs/42274980383)
  réussis.
- Runtime intégré : hooks `scene_flag`/`map_visit_flag`, NavGrid 3D, checkpoints/trace/replay.
  Le pilote réel atteint **1.1.2** à 369,78 s (arrêt 377,8 s sur `X01M01`) et ses 22 670 pas
  rejouent les 49 checkpoints **identiques** (`3da80263…`). Au premier contrôle il a 9 des 18
  drapeaux de l'original ; `compare-runs.ps1` préfère désormais le même `kind` pour ne pas
  confondre un chargement de carte avec une prise de contrôle.
- Harnais oracle libretro (outil GPL externe, hors crates) : création du personnage et
  **premier contrôle original** reproductibles sans GUI, `dq9re state --ram` sur les images
  RAM de 4 Mio, catalogue `states index/find`, `state diff` et `state watch`. Le premier
  contrôle donne carte 4105, area 4100, histoire 1.1.1, pose `fx32` identique à `bat6` et
  17 des 18 drapeaux (4135 au lieu de 4212). Preuves et entrées conservées sous
  `build/oracle-host/` (ignoré).
- Outillage (6 novembre 2026) : `scripts/verify.ps1` (fmt, clippy, tests, wasm), filtres de
  chemins CI/Pages vérifiés (un commit de documentation ne déclenche plus rien),
  `scripts/decomp.ps1` (adresse EU → source USA) et cache Ghidra (9,3 s → 0,96 s),
  `scripts/compare-runs.ps1` (alignement par point d'histoire), `scripts/prune-build.ps1`
  (purge sûre des états), `dq9re state diff/watch` (delta d'états et provenance image par
  image), `dq9re map --image/--reach` (rendu de collision et connectivité 3D réelle) et
  règles de propriété/preuve dans `AGENTS.md`. Le traceur mémoire dispose
  d'une piste vérifiée dans le code de DQIX Enhanced (`DQIX_WATCH`, outil GPL externe).
  Détails : [`docs/app.md`](app.md#boucle-de-développement),
  [`re/outils.md`](re/outils.md), [`re/map-archive.md`](re/map-archive.md), README.
- Ouverture, dialogues, légendes temporisées, caméra de scène et échange des écrans intégrés.
  Les deux vantaux de la bibliothèque de `X01M07` s'ouvrent et le héros les franchit :
  vérification native **et sur la page publiée**, par entrées clavier, sans téléportation.
- `--walk` utilise désormais le vrai pas fixe de l'application, plus un simulateur parallèle.
  Une carte invalide fait échouer la commande au lieu d'afficher « no map » avec succès.
- `e51a0bd` annule ensemble tâche, dialogue et état des portes lors d'un remplacement de
  session. Le test Windows avec ROM et sauvegarde réelles couvre un prologue en attente,
  puis actif avec légende ouverte, et 120 pas après chargement de `C01M09`. Clippy et wasm
  passent. La contre-vérification **visuelle complète sur Pages** de cette dernière correction
  reste à terminer : le défaut précédent, légende du prologue sur l'église, a été capturé.

### Branches poussées, non intégrées à `main`

| Branche | Dernier commit conservé | Contenu et portée |
|---|---|---|
| `FabLrc/v01-field` | `4534a0c` | Drapeaux de scène transmis par `EventScript` sans seconde lecture du catalogue, carte visitée avant les triggers, grille de navigation avec hauteur, trace/rejeu des entrées. Inclut le lot initial `04e417f` ; ne pas le réappliquer séparément. |
| `FabLrc/v01-init-flags` | `06f074d` | Provenance et traces des drapeaux d'ouverture, symboles et notes supplémentaires. Les nouvelles provenances ne sont pas encore réimplémentées. |
| `FabLrc/v01-height` | `45ad216` | RE et correction de conclusions erronées (`584535f`), plus `scripts/melondsds_host.py`, hôte libretro fonctionnel jusqu'aux menus et à la saisie du nom ; pas encore jusqu'au premier contrôle. |

Sur `4534a0c`, le parcours **réel à pied** atteint 1.1.2 à 369,8 s, puis s'arrête à 377,8 s
sur `X01M01`. Les 22 670 pas enregistrés rejouent les mêmes **49 checkpoints**, octet pour
octet. Ce n'est ni un parcours complet ni une preuve de parité avec melonDS.

### Blocages et ordre de reprise

1. **Reprendre depuis la référence outillée.** Le harnais, `state --ram`, le catalogue et le
   traceur sont intégrés. Produire les états originaux des points suivants (1.1.2, Angel
   Falls, quêtes, fin du prologue) avec le même harnais, puis les comparer au runtime par
   `state diff` et `compare-runs.ps1` aux mêmes points d'histoire. Ne pas réintégrer les
   anciennes branches d'outillage : elles sont déjà dans `main`.
2. **Poursuivre le parcours original au-delà du premier contrôle.** Le harnais libretro
   exécute la ROM sans focus (rendu logiciel, RAM et `retro_serialize` accessibles) et la
   création du personnage y est terminée. `bat6.ml1` n'est pas compatible avec
   `retro_unserialize` : repartir d'une **nouvelle partie réelle** et enregistrer un état
   (`--dump-ram`, `--state-out`, `--checkpoints`) à chaque point. Le prochain jalon est
   l'arrivée à 1.1.2 puis Angel Falls ; ne pas fabriquer d'état ni modifier les
   coordonnées/drapeaux pour contourner un blocage.
3. **Résoudre l'accès à la tour `X01M01` → `X01M03`.** La grille 3D plafonne à y = 11,25 ;
   le passage haut n'est pas atteint. Une route vérifiée seulement en X/Z était fausse.
   La proposition de contourner la tour via 21591 est également rejetée : ce trigger n'est
   chargé qu'en **1.2**, pas en 1.1.2 (`dq9re triggers X01 --with 21591`). Le baseline
   applicatif passe par `4103 → 4109 → 4103 → 4104`, puis 21510 avant 1.2.1.
   Comparer au vrai parcours original ; ne pas abaisser les murs ou les limites de marche.
   L'égalité de quelques constantes et placements ne prouve pas que tout le port est correct.
4. **Drapeaux de l'ouverture : causes établies et branchées.** Le premier contrôle du
   runtime pose désormais les 17 drapeaux `337, 1450..1454, 1461, 2721, 2751, 2774, 3191,
   3775, 3955, 4046, 4283, 4410, 4414`, chacun par sa cause : équipement initial (épée de
   cuivre + tenue céleste, `0xc76 + indice de compendium` lu dans `itemdt_*`), carte `S14M05`
   (6405) chargée avant le prologue (2774), `0x72(22159)` et `0x6b` du trigger de fin de
   `21596` (4283, 4410), touche tenue (`KeyManager::RefreshKeys`, 4414). Détail et preuves :
   [`re/trigger.md`](re/trigger.md) et [`re/item.md`](re/item.md).
   **Seul écart restant** : `4135` (herbe médicinale, item 22000) dans la course féminine ou
   `4212` (slimedrop, 22087) dans `bat6`. Ce sont des **butins de combat aléatoires** : trois
   courses aux entrées identiques sauf le sexe donnent l'un ou aucun, jamais tous les deux.
   Le runtime substitue le combat 26 et ne tire pas de butin ; aucun drapeau n'est forcé.
   Prochaine étape : un vrai combat (J4) pour que le butin sorte de sa propre règle.
5. **Valider toute la v0.1** : prologue, quêtes du village, checkpoints strictement identiques
   à l'original aux mêmes points, déplacement à nombre d'images contrôlé, puis parcours
   sur la version GitHub Pages publiée. Aucun de ces critères finaux n'est déclaré acquis.

### Preuves locales conservées, jamais à committer

- `build/orca/pause-2026-11-06/manifest.json` donne les SHA et l'archive des anciens
  worktrees : `field-scratch/`, `flags-scratch/`, `height-build/`. Les copies de projets
  Ghidra et les liens redondants vers la ROM originale ont été exclus ; les preuves ont été
  conservées par liens physiques avant suppression des worktrees.
- `build/orca/walk/` contient la trace, les checkpoints, le rejeu et leurs empreintes.
  Les variantes les plus récentes sont aussi dans l'archive `field-scratch/*_clean.*`.
- Référence non instrumentée : `build/orca/oracle/bat6.ml1` ; session initiale conservée
  dans `build/orca/oracle-session-1711efc/`. Captures de portes et du défaut de légende :
  `build/orca/pages-door-crossing-17bef8c.png`, `build/orca/pages-stale-caption-17bef8c.png`.
- Les journaux instrumentés, états et scripts de patch sont dans l'archive `flags-scratch/`
  (`melonds-trace/`, `melonds-log4/`, `melonds-log9/`, `melonds-log11..13/`, `tmp/`).
  Ils servent à la RE, **pas** de références de parité non modifiées. Une copie instrumentée
  de ROM peut s'y trouver : ne jamais la committer ni la redistribuer.
- Le cœur téléchargé reste sous `tools/build/melondsds_libretro.dll` ; les nouveaux états
  libretro et PNG sont dans l'archive `height-build/melondsds/`. La ROM du joueur n'a pas
  été déplacée ni modifiée.

## Principes

1. **Tranche verticale d'abord.** On construit le chemin minimal « nouvelle partie → se
   déplacer → parler → scènes → combattre » sur une zone, puis on élargit. Un système n'est
   approfondi que lorsqu'une zone jouable en a besoin.
2. **Ne pas redécouvrir ce qui est connu.** Avant d'analyser une fonction ou un format :
   symboles et code de la décompilation USA (adresses converties pour la ROM européenne),
   pages du wiki DQIX, puis seulement Ghidra. Ce qui est repris est cité et revérifié sur la
   ROM.
3. **Traduire le comportement du jeu, remplacer la plateforme.** La logique de jeu est
   traduite fonction par fonction ; les bibliothèques Nintendo (NitroSDK, NitroSystem, Wi-Fi,
   C runtime) et le matériel (VRAM, moteurs 2D/3D, son) ne sont pas traduits mais remplacés
   par `dq9-render`, `dq9-audio`, `dq9-app` et la crate `std`.
4. **Des oracles pour vérifier.** melonDS (stub GDB, sauvegardes d'état), DQIX Enhanced
   (portage PC fidèle à l'image près, GPL : comportement seulement), BattleEmulator et
   Grotto-Searcher (combats, grottes) : même entrée, même résultat.
5. **Les sauvegardes DS comme points de départ.** Charger un `.sav` réel permet de tester
   n'importe quel moment du jeu sans y jouer depuis le début.

## Définition de « 100 % traduit »

Périmètre mesuré sur la décompilation USA (`config/usa/arm9`) : **14 780 fonctions**, 2,9 Mo
de code.

| Catégorie | Contenu | Traitement |
|---|---|---|
| Jeu | logique Level-5 de l'ARM9 principal et des overlays de jeu | **traduite** en Rust, en citant l'adresse d'origine |
| Plateforme | NitroSDK, NitroSystem (G3D, son), C runtime, overlay 31 (Wi-Fi, 1 962 fonctions), multiboot | **remplacée**, non traduite |
| Hors jeu | overlays de débogage (7, 22), anti-piratage, écran d'erreur de lecture (19) | **exclue** |

« 100 % » = toutes les fonctions de la catégorie Jeu sont traduites ou explicitement
remplacées par un équivalent documenté, et les tests différentiels passent.

### Mesure (`cargo dq9re coverage`)

- `re/functions.txt` liste les 14 780 fonctions aux adresses européennes avec leur catégorie.
  Il est généré par `cargo dq9re functions <checkout de dqix-decomp>` (commit noté en tête) ;
  le test `our_symbols_start_listed_functions` vérifie que nos symboles y tombent tous sur un
  début de fonction.
- Une fonction compte comme traduite quand un commentaire d'une crate du portage (toutes sauf
  `dq9re` et `testrom`) la cite dans un marqueur `port:` :
  `//! port: 0x020d91ec, ov_017::021ba810` ou une plage `0x0206974c..=0x02069fd4` (toutes les
  fonctions qui y commencent). Une adresse qui n'est pas un début de fonction est une erreur.
- Classement automatique : overlays 30/31 et fichiers `src/NitroSDK/` de la décompilation,
  C runtime, démarrage et stubs BIOS en Plateforme ; overlays 7, 19, 22, 29 hors jeu ; le
  reste en Jeu, sauf la whitelist de 86 routines ARM9 européennes vérifiées
  individuellement par module Main, adresse, taille et nom source exact
  ([preuves](re/nitro-sdk.md)). **Le NitroSDK de l'ARM9 principal reste partiellement
  identifié** : la part Plateforme grandira à mesure qu'il sera reconnu.
- La CI vérifie les marqueurs à chaque exécution, affiche le résultat dans le résumé du job
  et, sur `main`, réécrit le bloc d'avancement du `README.md`.

### Carte des overlays

Noms internes et rôles d'après [dqix-functions](https://github.com/DQIX/dqix-functions) ;
nombre de fonctions d'après la décompilation USA.

| Overlay | Nom interne | Fonctions | Rôle | Jalon |
|---|---|---:|---|---|
| ARM9 | — | 6 205 | moteur Level-5 + bibliothèques Nintendo (à trier) | selon besoin |
| 17 | gamemain | 1 194 | boucle de jeu, terrain, triggers, interpréteur `.stb` | J1–J3 |
| 1 | event | 515 | scènes, natives des scripts (`ov_001::0215ae1c…`) | J3 |
| 0 | battle | 826 | combat | J4 |
| 24 / 25 / 26 | sub_battle / sub_round / sub_command | 573 / 304 / 24 | combat : action, tour, commandes | J4 |
| 23 | sub_menu | 827 | menus secondaires, fin de combat, contenu téléchargeable | J4–J5 |
| 2 / 4 / 5 / 11 | topmenu / menucallback / equipmenu / menusys | 213 / 532 / 89 / 186 | menus | J5 |
| 3 | shisetsu | 510 | établissements (boutiques, auberge, église…) | J5 |
| 20 / 21 / 9 | title / charamake / charamake2 | 18 / 5 / 43 | titre, création du personnage | J5 |
| 13 | skillup | 40 | points de compétence | J5 |
| 33 / 34 | bgload1 / bgload2 | 5 / 5 | chargement en arrière-plan | J1 |
| 27 | sub_staffroll | 87 | générique de fin | J6 |
| 6 | renkin | 132 | alchimie | J7 |
| 8 / 14 / 12 / 15 | jourecode / subjugation / prof / charaview | 63 / 69 / 71 / 103 | registres, bestiaire, profil, visionneuse | J7 |
| 16 | movieview | 87 | vidéos Mobiclip | J9 |
| 31 / 30 | wifi / sub_libmb | 1 962 / 11 | Wi-Fi Nintendo, téléchargement DS | J8 (remplacés) |
| 7, 22, 19, 29 | debug, sub_debug, readerror, sub_makescn | — | hors jeu | exclus |

Les overlays 10, 28 et 32 (`pitfall`, `sub_hoge`, `sound`) sont à identifier.

## Fait

- [x] Environnement : Rust, CI, outils RE (Ghidra + import scripté, melonDS), `dq9re`.
- [x] Conteneur ROM : en-tête, fichiers, ARM9/overlays (BLZ), NARC, codecs Nintendo.
- [x] Archives GPC2 + compression Level-5 (`gpc2.md`).
- [x] Tables d'enregistrements (`table.md`) ; extraction de tous les textes (`text.md`).
- [x] Pipeline des messages réimplémenté (6 passes, balises, codes `FF01..FF2F`, conditions,
      majuscules) et rendu des dialogues avec les polices du jeu (`font.md`, `dq9re msg`).
- [x] Choix de la réplique d'un PNJ (`dialogue.md`, `dq9re talk`).
- [x] Triggers de scénario : chargement, conditions, pistes, types d'événements, actions
      immédiates et différées sur un état modélisé (`trigger.md`).
- [x] Scripts d'événements `.stb` : format, machine à pile, désassembleur (`stb.md`,
      `dq9re stb`).
- [x] Interpréteur des scripts d'événements dans `dq9-core` : machine à pile traduite de
      `ov_017`, entrées 200/100/300, attentes par signaux, événements typés, `Director`
      branché sur les requêtes des triggers ; les 30 scripts du prologue vont à leur terme
      (`stb.md`, § Interpréteur, `dq9re script`).
- [x] Finitions J3 : identité du héros sauvegardé dans les messages (nom en glyphes et sexe),
      confirmation sonore des choix avec exceptions `NOTSE`, placements masqués à l'heure
      choisie affichables/déplaçables par les natives 574/575 (`dialogue.md`, `sdat.md`, `stb.md`).
- [x] Recensement des ressources externes (`ressources-externes.md`).
- [x] Inventaire statique des 228 natives STB employées : arguments, effets établis,
      preuves par adresses et limites (`re/stb-natives.md`) ; noms descriptifs dans
      `dq9re stb` uniquement pour les comportements démontrés.
- [x] Noms et bornes de `re/functions.txt` appliqués dans Ghidra avant les symboles
      locaux, à l'import comme sur un projet existant (`re/rom-layout.md`).
- [x] Premier lot de **50 routines NitroSDK ARM9** identifié et classé Plateforme,
      non porté ; noms canoniques et preuves individuelles dans
      [`re/nitro-sdk.md`](re/nitro-sdk.md). Le classement global du SDK reste ouvert.
- [x] Second lot de **36 primitives matérielles ARM9** (9 CP15, 27 G3X/affichage) classées
      Plateforme par comportement : Plateforme 2 718, Jeu 12 056 ([preuves](re/nitro-sdk.md)).

## J0 — Accélérateurs

Petits chantiers qui rendent tous les suivants plus rapides.

- [x] **Symboles de la décompilation dans Ghidra** : appliquer les noms et les bornes de
      `re/functions.txt` avant `re/symbols.txt` (nos noms priment) ; exceptions de tailles
      nulles et chevauchement explicites, pas de mode ARM/Thumb inventé.
- [x] **`dq9re functions` / `dq9re coverage`** : liste des fonctions aux adresses
      européennes, classement, marqueurs `port:`, avancement dans le README par la CI.
- [ ] **Classement du NitroSDK** de l'ARM9 principal : poursuivre au-delà des
      [86 routines vérifiées](re/nitro-sdk.md) (Plateforme 2 718), sans attribution par
      préfixe ni voisinage, pour affiner la catégorie Plateforme.
- [ ] **Natives des scripts** : reporter dans `stb.md` les rôles lus par le wiki
      ([Engine-Functions](https://github.com/DQIX/wiki/blob/main/Engine-Functions.md)) en
      vérifiant chacune sur `ov_001` ; priorité aux plus fréquentes (210, 570, 206, 208, 571).
      L'[inventaire statique](re/stb-natives.md) est complet. **Avancement** : les 120 natives
      du prologue d'Angel Falls (étapes 1.1 à 1.3, `cargo dq9re stb-natives --stage 1.1-3`)
      sont identifiées ou listées comme inconnues avec ce qui a été tenté ; unités de temps
      tranchées (fondu en ticks de 60 Hz, mouvements en 30 Hz nominal, `GetTickCount`
      mesuré à 2) ; 570 est `OBJECT3D_FLAG_HIDDEN`. Les légendes 409/411/413, la caméra à trois
      os 552 et la porte 584 sont établies et exécutées (lot « scènes restantes »). Restent le
      bit `0x10` de 571, la transition 547 (J4), l'index du son de porte, 554, 560 et les
      natives hors prologue.
- [x] **Sauvegarde DS** (`dq9-formats::save`, [`re/save.md`](re/save.md)) : conteneur
      (2 emplacements, 3 CRC-32, relecture/réécriture octet pour octet sur une sauvegarde
      réelle), champs typés (groupe, personnages et leurs noms, or, temps de jeu, équipement,
      chapitre/étape/sous-étape, pistes de scénario, drapeaux globaux/d'histoire), les 16
      blocs du sérialiseur (`Block`, pavage exact de la zone de la somme B), rôles des deux
      emplacements (0 normale, 1 suspension, copie miroir à chaque sauvegarde normale) et
      protocole d'écriture à 4 états, 21 fonctions du jeu nommées. `dq9re save`. Le décodage
      interne de quelques blocs (journal de quêtes, bestiaire, sac, cartes au trésor…) est
      repris quand `dq9-core` en aura besoin (liste dans `re/save.md`, § Inconnu).
- [ ] **Oracle** : lecteur d'états melonDS fait et vérifié sur des états réels
      (`dq9_formats::melonds`, `dq9re state`, [`re/melonds-state.md`](re/melonds-state.md) :
      mémoire, code résident et overlays comparés à la ROM ; format d'après ndskit, MIT) ;
      `scripts/melonds-state.ps1` produit un état depuis un `.sav` sans prendre le premier
      plan. **Sauvegarde chargée → RAM** : toutes les copies de `Save_LoadSlot` et les champs
      des membres du groupe (`PartyMember_LoadRecord`) retrouvés octet pour octet
      (`SAVE_COPIES`, `compare_party`, `dq9re state --save`, `re/save.md` § « En RAM »).
      **Position** : objets du groupe et du héros lus en RAM (`GameState+8`, `Object3D+0x44`,
      `dq9re state`), vérifiés en déplaçant le héros. **Différentiel v0.1** :
      `dq9re state --json` lit le bloc d'événements complet (5 pistes, leurs deux jeux de
      drapeaux, drapeaux globaux, quêtes, carte, VBlank) ; `scripts/oracle-compare.ps1` le
      compare au rapport du runtime (`track i: … flags`, `global flags`) ;
      `scripts/melonds-play.ps1` pilote une nouvelle partie réelle (écran titre, création du
      personnage, combat 26, états) sans premier plan. Première mesure réelle au premier
      contrôle de l'Observatoire : carte `4105`, héros à la même position, histoire 1.1.1 et
      `step_flags {1}` identiques ; **17 drapeaux globaux manquaient** au runtime
      (`1450–1454,1461,2721,2751,2774,3191,3775,3955,4046,4212,4283,4410,4414`), `337` commun
      (preuves et limites dans [`re/melonds-state.md`](re/melonds-state.md), § « Oracle
      différentiel »). Depuis, leurs causes sont branchées (§ « Blocages et ordre de reprise »,
      point 4) : seuls `4135`/`4212`, butin de combat aléatoire, restent hors du runtime.
      **Reste** : Angel Falls, quêtes du village et fin du prologue exigent
      le chemin physique réel (les entrées du parcours seront rejouées dans melonDS ; le premier
      PNJ de `X01M05` et ses coordonnées sont relevés dans `re/melonds-state.md`) ; le critère
      J2 « même nombre d'images » demande un pas à pas exact (`pause`/`frame` mesuré à ~0,5
      image par pression, pas encore suffisant).
- [ ] Créer les crates prévues par `architecture.md` au premier besoin : `dq9-render` et
      `dq9-app` faites (J1) ; reste `dq9-audio` ; l'état modélisé des triggers est passé de
      `dq9-formats` à `dq9-core` (`dq9-core/src/trigger.rs` ; `dq9-formats::trigger` ne garde que le parsing).

## J1 — Voir le monde

Afficher une carte et ses personnages dans une fenêtre (natif et wasm).

| Tâche | Réutiliser |
|---|---|
| `BMD0`/`BTX0`/`BCA0` (+ `BTA0`/`BTP0`/`BMA0`) | [apicula](https://github.com/scurest/apicula) (Rust, 0BSD) : reprendre ses lecteurs dans `dq9-formats`, avec mention ; wiki NSBMD/NSBTP/NSBTA |
| rendu wgpu proche de la DS (points fixes, matériaux, toon, brouillard, transparence, tri) | `src/NitroSDK/NSBXX/` de la décompilation (commandes de rendu G3D), GBATEK, rendu de DQIX Enhanced en comparaison visuelle |
| liste des cartes `maplist9.bin`, archives `.ambl`/`.amdj`, objets `.bmdj`, textures | wiki Map-List, Map-Archive, Map-Objects, Map-Textures ; Tagged-Data-Table |
| personnages `.chr` (`.bcfg`, modèles, animations), parties d'équipement | wiki Character-Parts, Motion-Tables |
| application `dq9-app` (winit + wgpu, 60 Hz fixe), caméra libre | — |

Critère : `dq9-app --map M01` affiche le village d'Angel Falls avec ses PNJ animés, en natif
et dans un navigateur.

Avancement :

- [x] Liste des cartes `maplist9.bin` réimplémentée (`dq9_formats::maplist`,
      [`re/maplist.md`](re/maplist.md), `dq9re maps`) : 1 010 enregistrements identiques à la
      RAM d'un état melonDS ; carte courante lue en `g_Zone3D+0`.
- [x] Lecteur G3D porté d'apicula (`nds_rom::g3d`, [`re/g3d.md`](re/g3d.md),
      `dq9re model`) : les 4 328 `BMD0`, 737 `BTX0` et 525 `BTP0` des cartes se lisent,
      22 827 textures décodées, 748 435 triangles assemblés.
- [x] Assemblage des cartes réimplémenté (`dq9_formats::map`,
      [`re/map-archive.md`](re/map-archive.md), `dq9re map`) : fichiers chargés, blocs
      `.bmdj`, parties et instances avec leur transformation ; les 667 `.ambl` et 669 `.amdj`
      s'assemblent, 5 341 ressources toutes résolues, `C01M09` identique à la RAM (melonDS).
- [x] Animations G3D décodées et évaluées image par image (`nds_rom::g3d`,
      [`re/g3d.md`](re/g3d.md), `dq9re model --anim --frame`) d'après la bibliothèque NITRO de
      la décompilation : articulations `BCA0` (poses exactes en virgule fixe, règles d'échelle
      Maya/Softimage), matrices de texture et `BTA0`, couleurs `BMA0`, motifs `BTP0` ; les
      9 887 fichiers d'animation des cartes et des `.chr` s'évaluent sur toutes leurs images.
- [x] Rendu et application (`dq9-render`, `dq9-app`, [`app.md`](app.md)) : `dq9-app --map M01`
      affiche la carte texturée en natif (fenêtre ou `--screenshot` PNG) et dans un navigateur
      (wasm, ROM choisie dans la page) ; passes opaque/translucide, alpha, faces cachées,
      matrices de texture, caméra libre à la projection du jeu (`0x0202df78`), 60 Hz fixe ;
      les 738 jeux d'archives se construisent.
- [x] Personnages réimplémentés au niveau des données (`dq9_formats::cast`, `chr`, `spr`,
      [`re/characters.md`](re/characters.md), `dq9re cast`, `dq9re chr`) : script de
      placement `place.bin` (20 types, chaînes et priorités), distribution `npc.bin`, corps
      (sprite `.spr`, modèle `chara_sub/*.chr`, monstre `enemy.gp2`), mouvement initial,
      tables `.bcfg` ; 508 cartes × 19 chapitres résolues, 2 683 `.chr` et 1 298 `.spr` lus,
      `C01M09` identique à la table des PNJ en RAM (melonDS).
- [x] Intégration des animations et des PNJ dans `dq9-app` : état CPU `AnimatedScene`,
      pas fixes partagés natif/Web, poses G3D, SRT, couleurs/alpha MAT et textures PAT ;
      drapeaux des parties, mouvements initiaux `.chr`, sprites face à la caméra et monstres.
      Captures déterministes `--frame N`, choix de progression `--chapter/--step/--substep`
      (prologue par défaut). Hauteurs de placement conservées sans collision du sol ;
      limites de comportement et de rendu détaillées dans [`app.md`](app.md).
- [x] **Héros animé** (`dq9_formats::hero`, `dq9_render::hero`, `dq9re hero`,
      [`re/characters.md`](re/characters.md) § « Le héros ») : pièces de `chara_pc.gp2`
      résolues depuis les objets portés (visage et cheveux par `itemdt_def`, forme des cheveux
      selon le couvre-chef, bras/chaussures en textures), assemblées sur le gabarit de 14 os,
      animées par `mp0200n`/`mp0200ne` (`stand`, `walk`, `run` ; le jeu joue `stand` au repos et
      `run` en marche), orientées selon le cap, à l'échelle `0x10a × corpulence` (vérifiée sur
      la RAM) ; `dq9-app` l'affiche à la place du repère, avec l'apparence du héros de la
      sauvegarde (`--save`) ou un héros par défaut **[hypothèse]** ; `--screenshot --walk`
      montre la marche. Non fait : armes, boucliers, ailes, teints du `palette.bin`,
      compagnons du groupe.
- [ ] Éclairage de la carte (`.bats`), brouillard et heure du jour appliqués au rendu (fait,
      reste la validation melonDS des zones avancées) :
      parseur `.bats` (`dq9_formats::lighting`, 504 scripts), horloge du jour et choix/mélange
      de l'entrée par phase (`dq9_core::lighting`, d'après `LightingManager`/`GameState`, EU
      vérifié), lumières DS, teinte de base, fond en dégradé, brouillard (équation de melonDS) et
      contours dans `dq9-render` ; `--time night|morning|day|evening`, `--clock`, liste Web.
      Validé à melonDS (état `C01M09`, `scripts/melonds-capture.ps1`) : registres de brouillard
      et valeurs de teinte identiques à la RAM, densité de brouillard à 0,01 près sur cinq
      tables, image à 12,8/255 de moyenne. **Non validé** : lumières DS et ciel en zone avancée
      (aucun état melonDS sur un champ), brouillard sur un matériau de terrain, mode alpha seul,
      indice de remplacement et `BeginFade` ([`re/lighting.md`](re/lighting.md)).

## J2 — Se déplacer

| Tâche | Réutiliser |
|---|---|
| état de jeu (`dq9-core`) initialisé depuis une sauvegarde DS (J0) ou un état par défaut : **fait** pour carte, position, cap, scénario, heure du jour, drapeaux d'événements ; reste groupe, sac, or | — |
| déplacement du héros, collisions, caméra de terrain (rotation L/R) | wiki Map-Collision, Getting-Around ; `ov_017` |
| placement des PNJ par carte et étape (`.npc`, script `place.bin`), sprites `.spr` | wiki Area-Cast, Sprites |
| portes et changements de carte, zones de trigger orientées | wiki Doors, Triggers (« Areas ») |
| heure du jour | wiki Time-Of-Day |

Critère : depuis une sauvegarde réelle, se promener dans tout Angel Falls, entrer dans les
maisons et en ressortir ; position comparée à melonDS sur une même séquence d'entrées.

Avancement : l'heure du jour existe comme horloge de 420 s et phase explicite
(`dq9_core::lighting::DayClock`, port de `GameState::SetDayTimer`/`AdvanceDayTimer`, sans le
branchement de la mise à jour de scène qui la fait courir sur les cartes de type 0 et 7 : l'application
l'entre par option). `dq9-core` porte le déplacement du chef de groupe d'après `ov_017::0218e210`
([`re/movement.md`](re/movement.md)) : pavé et caps, rotation du modèle (`0x324`), vitesse
(`0x189`, accélération `0x28`), requête `.col2` par grille, rayon du sol, sphère contre les murs,
chute, glissement le long des murs, en `fx32` au bit près ; la rotation `L`/`R` de la caméra
(`FieldCamera`) est portée sans sa hauteur ni sa distance. L'application affiche le héros animé (J1) et suit la caméra.
`--save`/le champ Web restaurent la carte, la position et le cap du héros, l'heure et le
scénario du slot normal valide (`dq9_core::state::GameState`, [`re/save.md`](re/save.md)
§ « Position à la reprise » : le groupe est posé au point de départ `0x6e` de la carte ;
égal à melonDS sur la démo et sur la carte 100) ; la sauvegarde de suspension (position
enregistrée) est décodée mais non vérifiée en RAM, le reste de l'état (groupe, sac) et le
cast selon le scénario sauvegardé ne sont pas branchés.
Portes, zones et warps du `.bmbl` : lus en `fx32` comme le jeu (`dq9_formats::map::Regions`,
identiques à la RAM de melonDS dans `C01M09` et `C01`) et testés par `dq9_core::field` : boîte
tournée (sens de la rotation lu sur la ROM), hauteur, cylindres, portes de type 2 qui exigent
une marche contre leur orientation pendant quatre images, aires de la carte et aires ajoutées
par l'action `0x8f`, événements 2, 3, 5, 6, 17 et 20 des triggers (changement de carte rendu à
l'application, autres demandes journalisées jusqu'à J3), marques `0x6c`/`0x88` des portes.
`dq9-app` ne fait que réagir aux événements ; entrer dans les maisons d'Angel Falls et en
sortir fonctionne (`dq9-app --walk`, test `angel_falls_houses_open_and_close`).
Comparaison à melonDS sur l'église de Stornway : sortie (warp `0x72`) et retour (porte de type
2) concordent pour les cartes, les arrivées, les angles et le bord de franchissement, à ± 2
images de pilotage près ([`re/map-archive.md`](re/map-archive.md)). Restent : la tâche de porte
(animation, marche automatique, fondu), les zones des autres types, le
parcours complet d'Angel Falls et la comparaison à nombre d'images égal : les états melonDS
ne sont pas alignés en nombre d'images, le critère différentiel de J2 n'est pas validé.

Déplacement fidèle : porté dans `dq9-core` (`fx`, `movement`, `collision`, `camera`), constantes
comparées à la ROM par `tests/movement_rom.rs` ; vitesse mesurée sous melonDS (une mesure, ± 1 image,
compatible : [`re/movement.md`](re/movement.md)). L'orbite de terrain démarre à la distance 11
et à la hauteur relative 6 du constructeur, suit le héros à mi-hauteur et reprend après les
scènes, y compris les chaînes qui changent de carte (vérifié en natif et dans Chromium :
`M01`, sauvegarde `C01M09`, nouvelle partie jusqu'à `X01M05`). Restent : personnages et objets
de zone qui poussent le héros, aimantation sur les zones orientées, échelles, suivi lissé et
zones de caméra, comparaison position par position à nombre d'images égal.

## J3 — Parler et scènes : **v0.1 jouable**

| Tâche | Réutiliser |
|---|---|
| fenêtre de dialogue à l'écran : police, cadre, nom de l'orateur, défilement, Oui/Non ; balises de mise en page restantes (`<CEN>`, `<TC#>`, `<W=#>`… de `text.md`) | `message.rs`, `font.rs` (fait) ; wiki Text-Markup |
| triggers branchés sur le jeu réel (parler, examiner, entrer dans une zone) | `trigger.rs` (fait) |
| interpréteur `.stb` dans `dq9-core` (attente par image, appels de fonctions) — **fait** (`dq9_core::script`, voir Avancement) | `stb.rs` (fait) ; wiki Event-Scripts |
| natives : personnages (200), caméra (300), messages (400), scène et fondus (500, 100–122) — **jouées à l'écran** pour le prologue (voir Avancement) | wiki Engine-Functions ; `ov_001` |
| deux écrans DS : écran haut (carte) et bas, disposition configurable | — |
| son : musique SDAT, effets et jingles — **fait** (sans comparaison à l'oreille ni à melonDS) : `dq9-audio` (séquenceur, enveloppes DS, ondes ADPCM, PSG, bruit, archives de séquences SSAR, mixeur multi-séquences avec limites et priorités des joueurs du SDAT), musique de la carte lue dans `maplist9.bin` + `mapbgm.bin`, natives 554 et 712 à 738 des scripts (effets des groupes chargés, effets de base, jingles `ME_` avec fondu et reprise de la musique, volume, arrêt, verrou), sons du message (bip de frappe, `<EXC>`, `<QES>`, ouverture et confirmation de choix avec exceptions `NOTSE` ; déplacement du curseur silencieux d'après le code EU), sortie cpal/WebAudio (`--mute`, touche `M`), `dq9re sseq|sdat|se`, `dq9-app --sound-wav` ; reste : sons de portes, de pas et de gestes, effets de combat, flux STRM, réglage de volume du joueur ([`re/sdat.md`](re/sdat.md)) | [SSEQPlayer](https://github.com/kode54/SSEQPlayer), FSS (WTFPL) ; wiki SDAT |

Avancement :

- [ ] **Fenêtre de dialogue** (`dq9_core::talk`, `dq9_render::ui`, `dq9-app/src/talk.rs`,
      [`re/dialogue.md`](re/dialogue.md) § « Fenêtre de message ») : A dans `dq9-app` (natif et
      Web, `Entrée`/`Espace`) parle au PNJ en face du héros (partenaire de `Talk_NearestNpc`,
      déclencheur « parler » de la carte, réplique choisie par `dialogue::select`) ; fenêtre
      du jeu (cadre `win0..7.spr`, police `me`, plaque `//Nom//`, coupure des lignes, pages,
      flèche d'attente, défilement à la 4ᵉ ligne, Oui/Non avec branches `<YES>`/`<NO>` et
      sauts `<JP_x>`/`<LB_x>`), balises de tête (`<CEN>`, `<WIN>`, `<GYOU=>`, `<MOJI=>`,
      `<COLOR=>`, `<RECT=>`) ; les 37 988 répliques anglaises se jouent jusqu'à la fermeture
      (test réel) ; capture `--input` d'une réplique de Hugo, d'Aquila (question) et d'Erinn.
      Le nom et le sexe du héros viennent du slot valide de la sauvegarde, y compris avec
      `--map` ; sans sauvegarde et en nouvelle partie, l'identité par défaut est conservée.
      Substitution et conditions testées ; branche féminine contrôlée à l'écran avec la
      sauvegarde de démonstration.
      **Reste** : valider sur melonDS les constantes de mise en page non lues (marges, plaque,
      fenêtre Oui/Non, fond, flèche), PNJ tournés vers le héros et immobiles, ballons,
      langues autres que l'anglais, `<TC#>` et `<W=#>` (absentes du chemin des conversations).
- [ ] **Triggers branchés** : l'événement 0 « parler » (`Field::talk`) est branché sur la
      conversation ; les demandes `Request::Event` des triggers (actions `0x77`, `0x80`, `0x9b`)
      démarrent l'événement dans l'application, la fin d'un événement tire l'événement 11
      (`Field::event_ended`), la fin d'une conversation l'événement 1 (`Field::talk_ended`), la
      fin d'un combat scripté 15/16 (`Field::battle_ended`) et `0x76` hors conversation ouvre la
      fenêtre (`App::talk_to`) ; l'action `0x78` est le **combat scripté** (`Request::Battle`,
      `trigger.md` § Combats scriptés) ; restent « examiner », les zones d'autres types, le
      système de combat (substitut « gagné » : `scenario::stand_in`) et les demandes `Other`
      (141…).
- [x] **Nouvelle partie : le prologue s'enchaîne** (`dq9-app --new-game`, bouton « Nouvelle
      partie » du Web, `dq9_core::scenario`, [`app.md`](app.md) § « Nouvelle partie »,
      [`re/stb.md`](re/stb.md) § « Enchaînement du prologue ») : état de `GameState::new_game`,
      héros par défaut, première scène `21500` ; `21507 → combat 26 → 21520 → … → 21596` par la
      native 538, le trigger 11, l'action `0x78` et le trigger 15, puis le joueur a la main à
      l'Observatoire (`--new-game --headless --auto`, 210 s de jeu). Le pilote **marche** :
      direction au pavé, `Hero::step`, collisions, portes (la double porte de `X01M07` est
      poussée puis ouverte par la native 584), et atteint l'histoire **1.1.2** à `X01M01`
      (NPC 127, événement 1170) sans téléport ; il s'y arrête, l'accès à la terrasse du warp
      `X01M03` restant inconnu. **Reste** : le combat 26 (J4), la connexion de hauteur de
      l'Observatoire, comparer l'ordre, les durées et les drapeaux à melonDS ; le pilote est
      un outil de test, pas une fonction du jeu.
- [ ] **Interpréteur `.stb`** : `dq9_core::script` traduit `Stb_Run` et ses auxiliaires
      (`ov_017::021d4980…021d6148`, marqueurs `port:`), joue les entrées 200, 100 et 300 d'un
      événement une mise à jour à la fois (`Script::step`) et rend des `Event` typés (fondus,
      messages, acteurs, caméra, son, déclarations de distribution) ; l'application répond par des
      `Signal` (message fermé, acteur au repos, caméra au repos…). Le `Director` démarre l'événement
      d'une requête `Request::Event` des triggers et enchaîne la native 538. Les 30 scripts du
      prologue (étapes 1.1 à 1.3) vont à leur terme sans erreur (`cargo dq9re script 1.1-3`, test
      réel `prologue_scripts_run_to_completion`) ; 8 natives de rôle inconnu ou sans effet
      rendu sont rapportées une fois et ignorées. **Exécution à l'écran** (`dq9_core::cutscene`, `dq9-app/src/event.rs`,
      [`app.md`](app.md) § « Événements », [`re/stb.md`](re/stb.md) § « Exécution à l'écran ») :
      `--stage 1.1` / `--event N` jouent les scènes (fondus, messages dans la fenêtre de
      conversation, acteurs qui bougent, tournent et jouent leurs mouvements, caméra du script
      dont celle portée par les os d'un modèle, déplacements au pas du jeu, lumière et heure,
      brouillard, attachements d'os du halo et des ailes, mouvements des PNJ de la carte, bulles,
      sprites de script, palette des personnages) ; les 30 scripts du prologue vont à leur terme
      (`--events-report 1.1-3` : 28 sans manque visible, 2 partielles — 22 avant les effets de
      scène —, aucune « non jouée »). Les placements masqués à l'heure choisie restent
      construits et les natives 574/575 peuvent les montrer et les déplacer : les scènes
      1330 et 21510 sont désormais complètes ; capture native de 1330 et test réel du
      placement affiché, image de référence `M01` inchangée pixel pour pixel.
      Restent : 547 (transition de combat, J4), le son de la porte 584, les placements de
      collision (574/575 sur les murs/sol), comparer l'ordre, les durées et les poses à
      melonDS, l'appelant de l'entrée 200 et les natives sans rôle
      (`re/stb.md`, § Interpréteur).
- **Deux écrans DS** (`dq9-app/src/screens.rs`, `dq9_render::Panel`) : écran haut = champ 3D avec
  la caméra du jeu (`0x0202df78`) ; écran bas = fond uni, contenu tactile non identifié
  **[inconnu]** ; dispositions `stacked`, `side`, `top` (`--layout`, touche `R`, select Web).
  Partiel : rien n'est encore affiché sur l'écran du bas.

**v0.1** : nouvelle partie avec un héros par défaut (modèle animé fait en J1 ; reste le choix
de son apparence à la création du personnage), prologue d'Angel Falls jouable (scènes,
dialogues, quêtes du village), sans combats. Critère : toute la suite d'événements du
prologue se déroule ; état des drapeaux identique à melonDS aux mêmes points. **État** : le
parcours à pied atteint 1.1.2 (`X01M01`) ; le passage vers le chapitre 2 reste bloqué par
l'accès à la terrasse haute de l'Observatoire. Une marche 3D exhaustive depuis la pose
d'histoire et depuis le départ de la carte plafonne à y 11,25 ; le warp `X01M03` est à y 15,5.
Les zones de type 1 sont **inertes** (aucun lecteur dans le champ) : la connexion manquante
(placement de collision déplacé, plateforme ou changement de carte scénarisé) est **[inconnue]**
et à établir sur melonDS (voir J2).

## J4 — Combattre : **v0.2**

| Tâche | Réutiliser |
|---|---|
| tables : monstres, actions, objets, sorts, niveaux, articles | wiki Monsters, Actions, Items, Spell-Table, Level-Tables, Articles |
| générateurs aléatoires AT/BT/CT (constantes connues) | glossaire dqix-functions, `src/Util/Random.cpp` |
| monstres visibles sur le terrain, apparition et rencontre | wiki Encounters ; scripts DeSmuME (nœuds d'apparition) |
| déroulement : commandes, ordre, IA, dégâts, critiques, tension, états, fin et expérience | `src/Combat/` de la décompilation (formules déjà en C++), wiki Battle-Resolution, Battle-AI, Battle-Commands ; overlays 0, 24, 25, 26 |
| présentation : arène, animations, messages, interface | wiki Battle-Stages, Battle-Presentation, Battle-Text |
| combats scriptés (boss) | wiki Event-Battles |

Critère : résultats identiques à BattleEmulator sur ses combats de référence (même graine,
mêmes entrées). **v0.2** : jusqu'au premier boss du jeu.

## J5 — Progresser : **v0.3**

- Écran titre, création du personnage, chargement/sauvegarde au format DS (J0).
- Menus : objets, équipement (modèles changeants), sorts, statut, stratégie, compétences
  (overlays 2, 4, 5, 11, 13).
- Établissements : boutiques, auberge, église, banque (overlay 3 ; wiki Inns-and-Churches,
  Items `shopdata1.bin`).
- Équipe, recrutement au Quester's Rest, vocations (wiki Party, Vocation-Skill-Trees,
  Skill-Panels).

**v0.3** : on peut jouer, sauvegarder et reprendre, avec une sauvegarde échangeable avec la DS.

## J6 — Histoire complète : **v0.5**

- Toutes les cartes, scènes et natives restantes ; navire, Starflight Express (wiki Ship,
  Starflight-Express, Travel) ; zoom (téléportation).
- Combats de boss et mécaniques spéciales rencontrées ; générique (overlay 27).
- Fermer les **[sens inconnu]** de `trigger.md` et `stb.md` au fil des chapitres.

**v0.5** : le jeu se termine. Critère : scénario parcouru de bout en bout, sauvegardes de
contrôle à chaque chapitre comparées à melonDS.

## J7 — Contenu annexe

- Quêtes, accolades, mini-médailles, bestiaire, registres (overlays 8, 14 ; wiki Quests,
  Accolades, Mini-Medals).
- Alchimie (overlay 6 ; wiki Alchemy, Alchemy-Recipes).
- Grottes et cartes au trésor : générateur traduit depuis `src/Grotto/` de la décompilation,
  vérifié contre Grotto-Searcher ; boss légendaires.
- Récolte, tours de passe-passe, visionneuse de personnages (wiki Gathering, Party-Tricks).

## J8 — Multijoueur et contenu en ligne

Le multijoueur en ligne du projet : ce que la DS faisait en local, sur Internet.

- Coopération jusqu'à 4 joueurs (visites via Pavo) et mode Tag : protocole réseau propre, au
  niveau du jeu (pas d'émulation radio) ; données échangées d'après `ov_017` et la session
  (conditions 0x17/0x51 de `trigger.md`).
- DQVC (calendrier des 52 soldes, fichiers chiffrés RC4) et quêtes/invités téléchargeables,
  servis localement ; formats d'après DQIX Enhanced (faits seulement) et `DQIX/auction`.
- Remplace les overlays 30 et 31.

## J9 — 100 %

- `dq9re coverage` à 100 % pour la catégorie Jeu.
- Vidéos Mobiclip (overlay 16) via FFmpeg ou décodeur propre ; son complet (effets, flux).
- Langues des cinq versions européennes ; versions USA/JPN si la correspondance d'adresses
  est étendue.
- Finitions plateforme : Web, manettes, écran large, mise à l'échelle.

## Approche écartée : recompilation hybride

Exécuter le code ARM d'origine (recompilé ou interprété) et le remplacer progressivement
donnerait un jeu jouable plus tôt, mais : DQIX Enhanced le fait déjà ; il faudrait émuler le
matériel DS (contraire à `architecture.md`) ; la cible Web en souffrirait ; et ses sources
sont en GPL. On garde la réimplémentation et on utilise ces projets comme oracles.

## Risques

| Risque | Parade |
|---|---|
| Rendu 3D éloigné de la DS | comparaison d'images avec melonDS/DQIX Enhanced dès J1 ; sémantique G3D de la décompilation |
| Natives des scripts nombreuses (228) | lectures du wiki, implémentation à la demande par zone jouable |
| Écarts USA/Europe | correspondance d'adresses vérifiée (J0) ; observations du wiki faites sur la ROM européenne |
| Licences | règles de `ressources-externes.md` : intégrer Unlicense/MIT/0BSD/WTFPL, lire seulement GPL et non licencié |
