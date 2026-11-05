# Feuille de route

Objectif : DQ9 entièrement réimplémenté en Rust (natif et Web), avec une **première version
jouable le plus tôt possible**, en s'appuyant au maximum sur le travail public existant
(`ressources-externes.md`). Chaque étape se termine par une doc dans `docs/re/`, une
implémentation testée et un commit.

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
  reste en Jeu, sauf la whitelist de 50 routines ARM9 européennes vérifiées
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
- [x] Recensement des ressources externes (`ressources-externes.md`).
- [x] Inventaire statique des 228 natives STB employées : arguments, effets établis,
      preuves par adresses et limites (`re/stb-natives.md`) ; noms descriptifs dans
      `dq9re stb` uniquement pour les comportements démontrés.
- [x] Noms et bornes de `re/functions.txt` appliqués dans Ghidra avant les symboles
      locaux, à l'import comme sur un projet existant (`re/rom-layout.md`).
- [x] Premier lot de **50 routines NitroSDK ARM9** identifié et classé Plateforme,
      non porté ; noms canoniques et preuves individuelles dans
      [`re/nitro-sdk.md`](re/nitro-sdk.md). Le classement global du SDK reste ouvert.

## J0 — Accélérateurs

Petits chantiers qui rendent tous les suivants plus rapides.

- [x] **Symboles de la décompilation dans Ghidra** : appliquer les noms et les bornes de
      `re/functions.txt` avant `re/symbols.txt` (nos noms priment) ; exceptions de tailles
      nulles et chevauchement explicites, pas de mode ARM/Thumb inventé.
- [x] **`dq9re functions` / `dq9re coverage`** : liste des fonctions aux adresses
      européennes, classement, marqueurs `port:`, avancement dans le README par la CI.
- [ ] **Classement du NitroSDK** de l'ARM9 principal : poursuivre au-delà du
      [premier lot de 50 routines vérifiées](re/nitro-sdk.md), sans attribution par
      préfixe ni voisinage, pour affiner la catégorie Plateforme.
- [ ] **Natives des scripts** : reporter dans `stb.md` les rôles lus par le wiki
      ([Engine-Functions](https://github.com/DQIX/wiki/blob/main/Engine-Functions.md)) en
      vérifiant chacune sur `ov_001` ; priorité aux plus fréquentes (210, 570, 206, 208, 571).
      L'[inventaire statique](re/stb-natives.md) est complet, pas l'identification
      sémantique : restent notamment les bits de 570/571, les déclarations différées,
      les unités temporelles et certaines variantes de messages.
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
      `dq9re state`), vérifiés en déplaçant le héros. **Reste** : comparer les structures que
      `dq9-core` calculera (position après une même suite d'entrées, triggers) à la RAM.
- [ ] Créer les crates prévues par `architecture.md` au premier besoin : `dq9-core`,
      `dq9-render`, `dq9-audio`, `dq9-app` ; l'état modélisé des triggers passe de
      `dq9-formats` à `dq9-core`.

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

## J2 — Se déplacer

| Tâche | Réutiliser |
|---|---|
| état de jeu (`dq9-core`) initialisé depuis une sauvegarde DS (J0) ou un état par défaut | — |
| déplacement du héros, collisions, caméra de terrain (rotation L/R) | wiki Map-Collision, Getting-Around ; `ov_017` |
| placement des PNJ par carte et étape (`.npc`, script `place.bin`), sprites `.spr` | wiki Area-Cast, Sprites |
| portes et changements de carte, zones de trigger orientées | wiki Doors, Triggers (« Areas ») |
| heure du jour | wiki Time-Of-Day |

Critère : depuis une sauvegarde réelle, se promener dans tout Angel Falls, entrer dans les
maisons et en ressortir ; position comparée à melonDS sur une même séquence d'entrées.

## J3 — Parler et scènes : **v0.1 jouable**

| Tâche | Réutiliser |
|---|---|
| fenêtre de dialogue à l'écran : police, cadre, nom de l'orateur, défilement, Oui/Non ; balises de mise en page restantes (`<CEN>`, `<TC#>`, `<W=#>`… de `text.md`) | `message.rs`, `font.rs` (fait) ; wiki Text-Markup |
| triggers branchés sur le jeu réel (parler, examiner, entrer dans une zone) | `trigger.rs` (fait) |
| interpréteur `.stb` dans `dq9-core` (attente par image, appels de fonctions) | `stb.rs` (fait) ; wiki Event-Scripts |
| natives : personnages (200), caméra (300), messages (400), scène et fondus (500, 100–122) | wiki Engine-Functions ; `ov_001` |
| deux écrans DS : écran haut (carte) et bas, disposition configurable | — |
| son minimal : musique SDAT (séquenceur SSEQ + mixage SWAV) | [SSEQPlayer](https://github.com/kode54/SSEQPlayer), FSS (WTFPL) ; wiki SDAT |

**v0.1** : nouvelle partie avec un héros par défaut, prologue d'Angel Falls jouable (scènes,
dialogues, quêtes du village), sans combats. Critère : toute la suite d'événements du
prologue se déroule ; état des drapeaux identique à melonDS aux mêmes points.

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

Le « online » du projet : ce que la DS faisait en local, sur Internet.

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
