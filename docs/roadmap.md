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
      mesuré à 2) ; 570 est `OBJECT3D_FLAG_HIDDEN`. Restent le bit `0x10` de 571, les bits
      de 409/411/413, 574/575, 541/542/561, 238, 554, 560 et les natives hors prologue.
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
compatible : [`re/movement.md`](re/movement.md)). Restent : personnages et objets de zone qui poussent le
héros, aimantation sur les zones orientées, échelles, caméra complète (hauteur, distance,
suivi), comparaison position par position à nombre d'images égal.

## J3 — Parler et scènes : **v0.1 jouable**

| Tâche | Réutiliser |
|---|---|
| fenêtre de dialogue à l'écran : police, cadre, nom de l'orateur, défilement, Oui/Non ; balises de mise en page restantes (`<CEN>`, `<TC#>`, `<W=#>`… de `text.md`) | `message.rs`, `font.rs` (fait) ; wiki Text-Markup |
| triggers branchés sur le jeu réel (parler, examiner, entrer dans une zone) | `trigger.rs` (fait) |
| interpréteur `.stb` dans `dq9-core` (attente par image, appels de fonctions) — **fait** (`dq9_core::script`, voir Avancement) | `stb.rs` (fait) ; wiki Event-Scripts |
| natives : personnages (200), caméra (300), messages (400), scène et fondus (500, 100–122) — **jouées à l'écran** pour le prologue (voir Avancement) | wiki Engine-Functions ; `ov_001` |
| deux écrans DS : écran haut (carte) et bas, disposition configurable | — |
| son : musique SDAT, effets et jingles — **fait** (sans comparaison à l'oreille ni à melonDS) : `dq9-audio` (séquenceur, enveloppes DS, ondes ADPCM, PSG, bruit, archives de séquences SSAR, mixeur multi-séquences avec limites et priorités des joueurs du SDAT), musique de la carte lue dans `maplist9.bin` + `mapbgm.bin`, natives 554 et 712 à 738 des scripts (effets des groupes chargés, effets de base, jingles `ME_` avec fondu et reprise de la musique, volume, arrêt, verrou), sons du message (bip de frappe, `<EXC>`, `<QES>`, ouverture de choix), sortie cpal/WebAudio (`--mute`, touche `M`), `dq9re sseq|sdat|se`, `dq9-app --sound-wav` ; reste : sons de portes, de pas et de gestes, curseur de choix, effets de combat, flux STRM, réglage de volume du joueur ([`re/sdat.md`](re/sdat.md)) | [SSEQPlayer](https://github.com/kode54/SSEQPlayer), FSS (WTFPL) ; wiki SDAT |

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
      **Reste** : valider sur melonDS les constantes de mise en page non lues (marges, plaque,
      fenêtre Oui/Non, fond, flèche), PNJ tournés vers le héros et immobiles, ballons, nom du
      héros de la sauvegarde, langues autres que l'anglais, `<TC#>` et `<W=#>` (absentes du
      chemin des conversations).
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
      l'Observatoire (`--new-game --headless --auto`, 210 s de jeu) ; avec `--pilot`, le parcours
      automatique traverse les étapes 1.1 à 1.4 jusqu'au chapitre 2 (`M01M10`, 652 s). **Reste** :
      le combat 26 (J4), comparer l'ordre, les durées et les drapeaux à melonDS, un vrai
      pilote (marche, collisions) ; le pilote est un outil de test, pas une fonction du jeu.
- [ ] **Interpréteur `.stb`** : `dq9_core::script` traduit `Stb_Run` et ses auxiliaires
      (`ov_017::021d4980…021d6148`, marqueurs `port:`), joue les entrées 200, 100 et 300 d'un
      événement une mise à jour à la fois (`Script::step`) et rend des `Event` typés (fondus,
      messages, acteurs, caméra, son, déclarations de distribution) ; l'application répond par des
      `Signal` (message fermé, acteur au repos, caméra au repos…). Le `Director` démarre l'événement
      d'une requête `Request::Event` des triggers et enchaîne la native 538. Les 30 scripts du
      prologue (étapes 1.1 à 1.3) vont à leur terme sans erreur (`cargo dq9re script 1.1-3`, test
      réel `prologue_scripts_run_to_completion`) ; 13 natives de rôle inconnu ou sans effet
      rendu sont rapportées une fois et ignorées. **Exécution à l'écran** (`dq9_core::cutscene`, `dq9-app/src/event.rs`,
      [`app.md`](app.md) § « Événements », [`re/stb.md`](re/stb.md) § « Exécution à l'écran ») :
      `--stage 1.1` / `--event N` jouent les scènes (fondus, messages dans la fenêtre de
      conversation, acteurs qui bougent, tournent et jouent leurs mouvements, caméra du script
      dont celle portée par les os d'un modèle, déplacements au pas du jeu, lumière et heure,
      brouillard, attachements d'os du halo et des ailes, mouvements des PNJ de la carte, bulles,
      sprites de script, palette des personnages) ; les 30 scripts du prologue vont à leur terme
      (`--events-report 1.1-3` : 26 sans manque visible, 4 partielles — 22 avant les effets de
      scène —, aucune « non jouée »). Restent : les effets du texte des légendes (409/411/413),
      547, 552, 584, les placements masqués à l'heure choisie que 574 montre, comparer l'ordre,
      les durées et les poses à melonDS, l'appelant de l'entrée 200 et les natives sans rôle
      (`re/stb.md`, § Interpréteur).
- **Deux écrans DS** (`dq9-app/src/screens.rs`, `dq9_render::Panel`) : écran haut = champ 3D avec
  la caméra du jeu (`0x0202df78`) ; écran bas = fond uni, contenu tactile non identifié
  **[inconnu]** ; dispositions `stacked`, `side`, `top` (`--layout`, touche `R`, select Web).
  Partiel : rien n'est encore affiché sur l'écran du bas.

**v0.1** : nouvelle partie avec un héros par défaut (modèle animé fait en J1 ; reste le choix
de son apparence à la création du personnage), prologue d'Angel Falls jouable (scènes,
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
