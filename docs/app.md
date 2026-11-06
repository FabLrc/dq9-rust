# Application `dq9-app` et rendu `dq9-render`

L'application commune native/Web affiche une carte, ses modèles, textures, animations, PNJ et le
héros animé (pièces de `chara_pc.gp2` assemblées, repos/marche/course selon la vitesse ;
[`re/characters.md`](re/characters.md) § « Le héros »). `dq9-core` consomme les collisions `.col2`
([`re/collision.md`](re/collision.md)) pour un déplacement de terrain expérimental ;
`dq9_core::field` teste les portes, aires et warps des `.bmbl` à chaque pas
([`re/map-archive.md`](re/map-archive.md)) et `dq9-app` charge la carte de destination qu'il
renvoie ; l'éclairage de la carte, le brouillard et l'heure du jour sont appliqués
([`re/lighting.md`](re/lighting.md)).

- `crates/dq9-core` (sans E/S, compile en wasm) : `lighting` (horloge du jour `DayClock`,
  phase `TimeOfDay`, choix et mélange de l'entrée `.bats`, registres de brouillard, vecteurs
  de lumière) ; `CollisionWorld` assemble les `.col2` des
  placements visibles ; `Hero` avance au pas fixe, recale le sol et bloque les faces verticales.
  Vitesse et départ sont provisoires ([`re/movement.md`](re/movement.md)).
- `crates/dq9-render` (sans E/S, compile en wasm) : `Scene::build(map, fichier)` reste
  disponible pour une scène statique ; `AnimatedScene::build(map, id, code, story, fichier)`
  ajoute les animations et les PNJ. `step(camera)` avance d'un pas de 60 Hz ;
  `face_camera` et `upload` préparent le rendu. `Renderer` conserve les ressources GPU ;
  `Camera` est la caméra libre.
- `crates/dq9-app` (winit + wgpu) : seule crate qui touche au système. Lit la ROM (fichier en
  natif, `<input type=file>` sur le Web), ouvre la fenêtre, fait tourner la boucle : mises à
  jour à pas fixes de 60 Hz (accumulateur), un rendu par image affichée.

## Lancer en natif

```powershell
cargo run -p dq9-app --release -- --map M01            # ROM : --rom, DQ9_ROM, ou le seul *.nds de rom/
cargo run -p dq9-app --release -- --map C01M09 --time morning --layout side
cargo run -p dq9-app --release -- --map F01 --clock 389 --screenshot build/scratch/app/f01-dusk.png
cargo run -p dq9-app --release -- --save rom/dq9.sav   # carte, position, cap et heure de la sauvegarde
# Une image hors écran en PNG, sans fenêtre, puis sortie :
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/m01.png
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/village.png `
    --size 1024x768 --eye=-5,20,40 --target=-20,0,5
cargo run -p dq9-app --release -- --map M01 --frame 60 --screenshot build/scratch/app/m01-f60.png
cargo run -p dq9-app --release -- --map C01M09 --chapter 19 --step 2 --substep 1 --night
# Sans fenêtre : marcher selon un script (axes du monde, secondes) et journaliser les cartes
# et positions, portes comprises (entrer dans une maison d'Angel Falls et en sortir) :
cargo run -p dq9-app --release -- --map M01 --start=-19.5,-9 --walk "-z:3,+z:3,idle:0.5"
# Une capture après des entrées : marcher puis parler au PNJ en face (« Conversations ») :
cargo run -p dq9-app --release -- --map M01 --step 2 --start=5.4,2.5 `
    --input "+x:0.05,a,idle:1.2" --eye=4,3,14 --target=6,-2.5,2.5 --layout top `
    --screenshot build/scratch/app/hugo.png
```

Avec `--screenshot`, `--walk` et `--start` pilotent le héros pendant les `--frame N` pas fixes
(le script court depuis le pas 0, puis le héros reste immobile) : la capture montre l'allure
choisie par sa vitesse, par exemple la marche vers l'ouest depuis Angel Falls :

```powershell
cargo run -p dq9-app --release -- --map M01 --walk "-x:5" --start=-1.74,1.22 --frame 20 `
    --eye=-1.5,0,-3 --target=-3,-0.6,1.2 --screenshot build/scratch/app/hero-walk.png
cargo run -p dq9-app --release -- --map M01 --frame 10 `
    --eye=-1.74,0.3,-3.5 --target=-1.74,-0.5,1.22 --screenshot build/scratch/app/hero.png
```

`--walk` joue les mêmes pas fixes qu'en fenêtre (`Hero::step` avec les touches du pavé que
la direction demandée presse, caméra au yaw 0, puis `Field::step` avec la pose du héros) : `+x`, `-x`, `+z`, `-z` (deux axes font une diagonale : `+x-z`) ou `idle`, deux
points et une durée ; `--start x,z` pose le héros au sol de ce point (aussi avec
`--screenshot`, face à +z). Chaque changement de carte, chaque arrivée et chaque tronçon sont
écrits sur la sortie standard.

### Conversations

Dans la fenêtre : `Entrée`, `Espace` ou `Z` (touche A) parle au PNJ en face du héros (le plus
« regardé » à moins de 1,5 unité, ou dans sa zone de conversation : `dq9_core::talk::nearest`),
avance dans le texte (pendant la frappe, affiche jusqu'à la prochaine attente) et ferme à la
fin ; `Retour` ou `X` (touche B) répond Non ; `↑`/`↓` ou `W`/`S` déplacent le curseur des
questions. Le héros ne bouge pas pendant une conversation. La réplique est celle que le jeu
choisit (`dialogue.md`) pour la carte, le chapitre, l'étape, les quêtes et le moment du jour de
l'état de l'application, en anglais. Sur le Web la ROM chargée suffit : mêmes touches.

`--input "SCRIPT"` (avec `--screenshot`) joue des entrées avant la capture, avec les mêmes pas
fixes : les tronçons de `--walk` (`+x:0.5`, `idle:2`) et les boutons `a` (action : parler,
avancer), `b` (annuler), `up`, `down`, chacun d'un pas. Le héros reste à sa dernière position
(le repère orange de la capture) ; un script qui change de carte est une erreur. Exemple : la
capture de Hugo ci-dessus (étape 2 d'Angel Falls, `<N_TURN>//Hugo// …`).

Le dessin du cadre, du nom, du texte et de la flèche vient de `dq9_render::ui` ; les
constantes de mise en page sont celles de `MsgWin_Show` quand elles ont été lues, sinon des
hypothèses (`dialogue.md`, § « Fenêtre de message »). La fenêtre est dessinée sur l'écran du haut,
dans la zone qu'il occupe (`screens.rs`).

`--map` : code (`M01`, `C01M09`) ou id (`1100`) de la liste des cartes ; il prévaut sur la
carte lue dans `--save`. En l'absence des deux options, la carte reste `M01`. `--save` lit un
`.sav` local et sélectionne la carte du premier slot normal valide ; aucune donnée n'est envoyée
au réseau. Le héros commence à la position et au cap que le jeu donne après « Continue » (point
de départ de la carte, `re/save.md` § « Position à la reprise »), posé sur le sol ; l'écran
`--screenshot` imprime cette pose (`hero: …`) et centre la caméra dessus. `--time` : moment de la
journée `night`, `morning`, `day`, `evening` ou 0 à 3 (`LightingManager+0x98` : choix des
variantes, des instances visibles et de l'entrée d'éclairage), par défaut la phase de l'horloge de
la sauvegarde, sinon la nuit ; `--clock S` : seconde du jour de 420 s (nuit 0, matin 180, jour 210,
soir 390) ; sa phase remplace `--time`, et les deux dernières secondes d'une phase se fondent
dans la suivante en zone avancée (champ) ; `--fog OFFSET,SHIFT,COLOR,D0..D31` remplace le
brouillard (comparaisons à melonDS) ;
`--layout stacked|side|top` : disposition des écrans DS (section « Écrans DS »).

La distribution est celle du prologue **1.1.1 de jour** par défaut. `--chapter`, `--step`,
`--substep` choisissent le point de l'histoire ; `--night` choisit les placements de nuit,
indépendamment de `--time` (variante graphique de la carte). Une sauvegarde ne restaure ici
que la carte, la position, le cap et l'heure : quête, drapeaux et groupe ne sont pas chargés
(la distribution suit `--chapter`/`--step`/`--substep`). `--frame N`
évalue exactement N pas fixes depuis le début des mouvements avant la capture (0 par défaut),
avec la même mise à jour CPU qu'en fenêtre et sur le Web, sans dépendre du temps de chargement
du GPU. Les sprites sont orientés vers la caméra de `--eye`/`--target` avant cette évaluation.

Commandes : glisser avec le bouton gauche pour tourner, molette pour zoomer. En mode orbite,
`ZQSD`/`WASD` (touches physiques) ou flèches sont le pavé du jeu : le héros tourne, accélère et
marche à 5,76 unités/s comme `Character_Update`, bloqué par les murs et posé sur le sol de la
carte ([`re/movement.md`](re/movement.md)) ; `Q`/`E` (physiques `A`/`E` en AZERTY) sont les
boutons `L`/`R` du jeu : ils tournent la caméra de ± 45° au plus autour de son yaw d'entrée sur
la carte, ensemble ils la ramènent. `Tab` passe en vol libre, où les touches de déplacement
déplacent la caméra et `Q`/`E` la cible vers le bas/haut ; `Maj` accélère la caméra. `F` recadre, `R` change la disposition des écrans (`stacked`, `side`, `top`),
`T` moment suivant (liste « Moment » sur le Web), `M` coupe/rétablit la musique, `PageHaut`/`PageBas` carte précédente/suivante, `Entrée`/`Espace` parler, `Échap` quitter.

**Musique** : à chaque chargement de carte, `dq9-app` joue la séquence de `bgm.sdat` que le jeu
choisit pour elle (`re/sdat.md` : fente musique de `maplist9.bin`, variante de nuit, surcharges de
`mapbgm.bin`), par `cpal` en natif et par WebAudio sur le Web (le navigateur n'autorise le son
qu'après un geste : la musique démarre au premier clic ou à la première touche). `--mute`
(`?mute=1` sur le Web) n'ouvre aucun périphérique audio ; `dq9re sseq <n> --wav f.wav` rend une
séquence sans fenêtre.

## Événements (scènes `.stb`)

Les scènes du jeu (cinématiques du prologue, dialogues scriptés) sont des scripts `.stb`
([`re/stb.md`](re/stb.md)) que `dq9-app` joue à l'écran :

```powershell
# Le prologue : première scène de l'étape 1.1 (`21500`, Angel Falls, « Opening (Background) »),
# puis, par la native 538, `21507`. Dans la fenêtre ; Entrée/Espace lit les messages.
cargo run -p dq9-app --release -- --stage 1.1
cargo run -p dq9-app --release -- --event 1130          # une scène seule, sur sa carte
# Une capture après N pas de 60 Hz (le script, les fondus, les messages et la caméra rejoués
# depuis le début), avec l'action pressée tous les 100 pas pour lire les messages :
cargo run -p dq9-app --release -- --stage 1.1 --frame 420 --press-every 100 `
    --size 512x384 --screenshot build/scratch/ev/hello.png
# La logique du jeu sans fenêtre (déclencheurs, changements de carte, scènes), journalisée :
cargo run -p dq9-app --release -- --stage 1.1 --headless-steps 60000 --press-every 100
# Ce que joue chaque scène d'une plage d'étapes (tableau ci-dessous) :
cargo run -p dq9-app --release -- --events-report 1.1-3
```

`--stage MAJEUR.MINEUR` choisit le point de l'histoire et la première scène de l'étape dans
`eventlist_lv5.bin` (`dq9_core::script::load::stage_opening`) ; `--event N` une scène du
numéro de `data/event` ou `data/evspt_lv5`. Une scène se joue sur la carte que son
enregistrement de la liste des événements désigne (valeur 4), le héros à la position de
l'enregistrement quand elle en a une : l'application charge cette carte si elle est ailleurs.
Sur le Web, `?event=N`. Dans la fenêtre, un déclencheur qui demande `Request::Event` (actions
`0x77`, `0x80`, `0x9b` : entrer dans une zone, parler) met l'événement en file ; à la fin d'un
événement l'application tire l'événement 11 des triggers (`Field::event_ended`), comme
`EventTask_Update`.

**Architecture.** `dq9_core::cutscene` tient l'état de la scène que le script a produit (acteurs,
caméra, fondus) sans E/S ; `dq9-app/src/event.rs` (`EventPlayer`) fait tourner le `Director`,
applique cet état au rendu et renvoie les `Signal` au script ; `dq9_render::AnimatedScene`
fournit les acteurs (`add_actor`, `play_actor`, `actor_bone`, `play_hero`, `set_npc_angle`).

- **Cadence.** Un pas fixe de 60 Hz est un tick ; le script avance toutes les deux ticks (une
  mise à jour du jeu, [`re/stb-natives.md`](re/stb-natives.md) § « Temps et unités »), la scène à
  chaque tick. Déplacements, rotations, échelle et alpha chronométrés durent `2 × durée` ticks ;
  les fondus comptent des ticks.
- **Modèles.** Avant de jouer une scène, l'application la joue à blanc (toutes les attentes
  finissent aussitôt) pour savoir quels `.chr` chaque personnage charge (natives 200, 228, 229,
  233, 566, 567), les ajoute à la scène et envoie la scène au GPU une fois. Un modèle sans
  polygone (les caméras du prologue, `ev01000cc000.chr`) n'a pas d'instance : il garde son
  squelette et ses mouvements. Mouvements d'un acteur : table `BCFG` de son archive et des
  paquets ajoutés (`LoadModelArchive`), un mouvement sans fichier propre jouant une tranche du
  fichier du paquet de type 0. Le héros joue les mouvements des paquets `evNNNNNp000.chr`.
- **Caméra.** Œil, cible, orbite, champ de vision (demi-angle, 15° par défaut), roulis, secousses,
  suivi d'un acteur et caméra portée par deux os d'un modèle (native 572, utilisée par
  l'ouverture) sont celles du script ; la caméra libre reprend à la fin. Une scène est cadrée en
  4:3 sur l'écran haut (`screens.rs`).
- **Messages.** La fenêtre de conversation (`talk.rs`) affiche le texte de la table de la scène ;
  sans balise de fin elle attend l'action (`Dialogue::for_event`) ; la native 401 la ferme, la
  native 410 en retire le cadre. Le joueur est verrouillé (pas de marche, pas de conversation).
- **Fondus.** Un rectangle noir ou blanc sur tout l'écran 3D, de niveau −16 à +16 ; un moteur
  verrouillé (`112`, `114`, `116`) ignore les fondus simples. L'écran « principal » est pris pour
  l'écran 3D **[hypothèse]** ; le fondu de l'autre moteur et l'échange des écrans sont ignorés.

**Résultat sur le prologue** (`--events-report 1.1-3`, ROM EU, textes anglais ; les 30 scripts
vont à leur terme, aucune scène « non jouée » ; le son appartient à un autre chantier et n'est
compté nulle part). « Complet » : tout ce que le script demande est exécuté, aux natives sans effet
visible établi près (554, 559, 560, 571, 712, 815).

| Scène(s) | Cartes | Résultat | Ce qui manque (visible) |
|---|---|---|---|
| 1130, 1140, 1310, 1350, 1360, 1370, 1390, 1410 | `X01M05`, `M01` | complet | — |
| 1150, 1160, 1600, 1610 | `X01M07` | partiel | mouvements `stand`/`walk`/`talk_loop` des PNJ de la carte (sprites ou modèles) et héros de référence (cast mode 4, personnage 99/139/130 de la table des personnages, non dessinés) ; bulles 541/542/561 |
| 1170, 1320 | `X01M01`, `M01` | partiel | natives 545/546 (`run`/`stand` d'un personnage), 328 (caméra) |
| 1330, 1340, 1380 | `M01M06`, `M01M04`, `M01` | partiel | mouvement 1 d'un acteur déclaré, bulles 541/542/561, `574` (bit des placements de carte) |
| 21500 + 21507 | `M01M12` | partiel | pas de son ni de lumière/heure (588, 589…), sprites 2D (521), 8 attachements d'os (tête, torse : halo, ailes), natives 238 (teinte), 409/411/413 (effets de fenêtre), 541/542/561, 547, 552 (caméra à trois os), 574, 584, mouvement `nozoki_loop` absent des données |
| 21510 | `X01M04` | partiel | 574, attachement d'os |
| 21520 → 21521 → 21522 → 21596 | `F01M01`, `X01`, `X01M01`, `X01M05` | partiel | brouillard (804), lumière/heure, teinte 238, 574, 809, 6 attachements d'os |
| 21591, 21592, 21594, 21597 | `X01M05`, `M01`, `X01M04` | partiel | sprites 2D (21591), 575/574, 581/582, 809, lumière, attachements d'os (21592, 21597) |
| 21593, 21595 | `M01` | partiel | sprites 2D, brouillard, teinte 238, bulles, 559, 581/582, 809, mouvement 1 d'un acteur |

La chaîne 21520 → 21596 (quatre cartes) se joue d'un bout à l'autre par les natives 538 ; ce qui
suit 21507 n'est pas enchaîné automatiquement : le trigger 11 de la carte rend une demande
`Other(120)` (`0x78`, « tâche `+0x3718` », **[sens inconnu]**, `trigger.md`) et la scène 21520
se lance par `--event 21520`.

**Limites et hypothèses.** Voir aussi [`re/stb.md`](re/stb.md), § « Exécution » : liaison par défaut
du membre déclaré `n` au créneau et à l'acteur `n`, taille par défaut des modèles chargés (0x10a
avec le sélecteur 0, 1 avec le sélecteur 1), canaux des acteurs, sens du roulis, unité des
durées de la caméra. Les PNJ de la carte ne changent pas de mouvement ; seuls leur position, leur
direction (les sprites montrent le côté qui regarde la caméra) et leur visibilité suivent le
script ; ils retrouvent leur place à la fin. Les modèles ne sont pas encore comparés
à melonDS.

## Lancer dans un navigateur

```powershell
cargo build -p dq9-app --release --target wasm32-unknown-unknown
# Une fois : la version de wasm-bindgen-cli doit être celle de Cargo.lock.
cargo install wasm-bindgen-cli --version 0.2.130 --locked --root tools/wasm-bindgen
tools/wasm-bindgen/bin/wasm-bindgen --target web --no-typescript `
    --out-dir crates/dq9-app/web/pkg target/wasm32-unknown-unknown/release/dq9-app.wasm
python -m http.server 8000 -d crates/dq9-app/web     # ou tout serveur de fichiers statiques
```

Ouvrir `http://localhost:8000/` (paramètres `map`, `time` (nom ou 0 à 3), `clock`, `layout` (`stacked`, `side` ou `top`), `chapter`, `step`,
`substep`, `night=1`, `mute=1`), puis choisir la ROM et, facultativement, une sauvegarde `.sav` dans la
page. Les deux fichiers sont lus localement dans le navigateur, jamais envoyés ni hébergés ;
`map` explicite prévaut sur la carte de la sauvegarde.
WebGPU quand le navigateur le propose, sinon WebGL 2. `crates/dq9-app/web/pkg/` est généré
(ignoré par git).

## Rendu

Ce que fait `dq9-render`, et ce qui s'écarte du moteur 3D de la DS.

Fidèle à ce que lit le jeu :

- **Instances** : une par placement visible au moment choisi (`Instance::hidden`), des parties
  modèle et objet (`.nsbmd`) ; matrice monde = translation, rotation `y`, échelle de
  `Instance::world` (`0x02013168`). `dq9-core` décompresse les `.col2`, transforme les faces
  visibles et les utilise pour le sol et les murs (voir `re/movement.md`).
- **Textures** : toutes les `.nsbtx` de l'`.ambl` (`Zone3D+0x418`), liées aux matériaux par
  nom de texture et de palette. Quand plusieurs fichiers ont le même nom, le premier de la
  liste (le plus récent) gagne **[hypothèse]**. Une texture qu'aucun fichier ne fournit
  (30 sur la ROM, `Scene::unbound_textures`) laisse le matériau sans texture **[hypothèse]**.
- **Matériaux** : couleurs de sommet, alpha du polygone (`POLYGON_ATTR` bits 16-20), faces
  cachées (bits 6-7, faces avant dans le sens trigonométrique), modes modulation et décal
  (bits 4-5 ; tous les matériaux des cartes sont en modulation), répétition et miroir
  (`TEXIMAGE_PARAM` bits 16-19), matrice de texture du matériau (`Model::tex_matrices`),
  filtrage au plus proche, aucun pixel d'alpha 0.
- **Passes** : polygones opaques puis translucides (alpha < 31 ou texture à alpha partiel,
  A3I5/A5I3) ; les texels d'alpha 31 d'un polygone d'alpha 31 sont dessinés dans la passe
  opaque, comme les pixels opaques de la DS ; les pixels translucides n'écrivent la
  profondeur qu'avec le bit 11 ; test de profondeur « égal » (bit 14) après les autres
  opaques.
- **Caméra** : celle du jeu par défaut (`0x0202df78`) : demi-angle vertical de 15°
  (`0x0202e9b4`, sinus et cosinus pour `NNS_G3dGlbPerspective`), plans à 1 et 400 ; libre en
  orbite ou en vol ; option 256×192.
- **Héros** (`dq9_render::hero`, `AnimatedScene::add_hero`) : corps et jambes sur le gabarit de
  14 os, animés par `mp0200n`/`mp0200ne` (`stand`, `walk`, `run`), visage, cheveux et couvre-chef
  posés sur l'os `head`, bras et chaussures par fichiers de texture, fusionnés en un modèle ;
  orienté selon le cap (`angle_of_yaw`), échelle `0x10a × corpulence` ; l'allure vient de la
  distance parcourue au pas (`Gait::from_speed`). Apparence : le premier personnage de la
  sauvegarde (`--save`), sinon un héros par défaut **[hypothèse]** ; ni armes, ni boucliers, ni
  ailes, ni teints du `palette.bin` ([`re/characters.md`](re/characters.md)).
- **PNJ** : distribution et placements de `cast::load`, sprites `.spr` décodés une fois
  (images détourées), noms `stand_*` choisis par `cast::sprite_stand`, panneaux face à la
  caméra ; variantes assises `sleep_*` avec miroir. Échelles du jeu : sprites `0x8f/4096`
  par pixel, modèles `0x10a/4096` ; positions `fx32/4096`, rotation en radians.
  Archives `.chr`, mouvement de `cast::initial_motion` (dont le paquet supplémentaire du
  type 19), monstres du membre `.mon` de `enemy.gp2` puis premier `.cchr`.
- **Animations** : fichiers explicitement sélectionnés par les drapeaux des parties de
  carte, pose G3D (`JNT0`), matrices de texture (`SRT0`), couleurs/alpha (`MAT0`) et
  changements de texture/palette (`PAT0`). Les modèles simples bouclent sur le nombre
  complet d'images (`Animation3D::AdvanceTimer`), les objets à paquet BCFG utilisent les
  bornes et la vitesse du premier mouvement ; les `.chr` par mouvement utilisent sa vitesse
  et la plage du fichier `[0, images−1[`, pas les anciennes tranches BCFG
  (voir `characters.md`).
  Les listes d'affichage sont décodées une fois ; les poses réécrivent seulement les
  sommets, sans reconstruire les indices ni réallouer les ressources GPU. SRT/PAT seuls ne
  refont pas la géométrie. L'état CPU animé est distinct de l'API de scène statique.

Approximé ou absent :

- **Tri des translucides** : du plus loin au plus proche selon la profondeur du centre de
  chaque maillage × instance ; la DS trie par `Y` à l'écran (ou garde l'ordre d'envoi).
- **Éclairage** : les modèles de carte sont éclairés au sommet par les deux lumières DS du
  `.bats` (équation de GBATEK : émission + par lumière `couleur × (diffuse·max(0, −L·N) + ambiante
  + spéculaire·brillance)`, sur 5 bits ; bits 0-3 de `POLYGON_ATTR`), normales du modèle, ambiante
  de la zone pour le terrain en mode avancé. Les zones de base n'ont pas de lumière : leurs
  couleurs de sommets sont transformées (teinte, luminosité, contraste) ; sprites et personnages
  sont multipliés par la diffuse de la zone ; les personnages gardent une lumière fixe en plus (les
  vraies lumières de la DS passent par leur propre matrice, non portée). Fond : dégradé vertical
  centré sur l'horizon ; brouillard de la DS (offset, décalage, table de 32 densités, couleur) sur
  les matériaux au bit 15 de `POLYGON_ATTR` et les sprites ; marquage des contours. La table de
  brillance (`SetShininessTable`, bit 15 de `SPE_EMI`) n'est pas portée.
- **Non rendu** : anticrénelage, polygones d'ombre (mode 3) et fil de fer (alpha 0), panneaux
  G3D des modèles, mode alpha seul du brouillard et alpha de `FOG_COLOR`, précision de la DS
  (couleurs 6 bits, profondeur W ; la profondeur du brouillard est recalculée sur 24 bits).
- **PNJ, limites** : les PNJ ne s'arrêtent ni ne se tournent vers le héros pendant qu'il leur
  parle (les sprites suivent la caméra ; l'orientation des modèles n'a pas d'API dans
  `AnimatedScene`) ; les hauteurs des PNJ sont celles des placements ; le snap du sol et le
  déplacement du héros sont approximatifs (pas d'accélération ni rayon).
  Les portes, aires et warps suivent `dq9_core::field` (comparés à
  melonDS sur l'église de Stornway) sans l'animation de la porte ; les triggers de zone sont
  évalués ; leurs demandes d'événement sont exécutées (§ Événements), les autres demandes
  seulement journalisées, et les escaliers, chutes et chemins des PNJ ne sont pas exécutés. Les
  visiteurs réseau, l'accessoire `s069` et la teinte additive des palettes (type 22) restent
  absents. Le mélange de plusieurs poses, les effets/actions `.beff`/`.bact` et les modes de
  sprite autres que debout/assis ne sont pas exécutés. Les couleurs MAT sont évaluées ; les
  sommets à normales restent soumis à la lumière fixe décrite ci-dessus, pas aux lumières DS.

## Écrans DS

La fenêtre montre les deux écrans de la DS (`crates/dq9-app/src/screens.rs`) :

- **Haut** : le champ en 3D, avec la caméra et la projection du jeu (`0x0202df78`, demi-angle
  vertical de 15°). L'écran fait 256×192 ; seule la taille du rectangle varie avec la fenêtre.
- **Bas** : fond uni (`dq9_render::Panel`), sans contenu. Le contenu de l'écran tactile n'est
  pas identifié **[inconnu]** ; aucune donnée de la ROM n'y est affichée.
- **Fenêtre de message** : dessinée sur l'écran du haut (`talk.rs`, `dialogue.md`).

Dispositions : `stacked` (défaut, écran du haut au-dessus de celui du bas), `side` (côte à côte),
`top` (écran du haut seul). Chaque écran est mis à l'échelle entière (1, 2, 3…) quand la fenêtre
le permet, sinon à l'échelle fractionnaire ; les écrans sont centrés. Natif : `--layout`, et `R`
passe à la disposition suivante. Web : le select « Écrans » ou `?layout=`. Les anciens
`--ds-aspect` et `ds=` sont supprimés.

## Vérification

- `crates/dq9-render/tests/real_rom.rs` : les 738 jeux d'archives distincts (moment 0,
  environnement de grotte 1) se construisent sans erreur : 3 501 modèles, 3 646 instances,
  18 570 textures décodées, 697 035 triangles ; 28 834 matériaux, dont 48 sans texture et
  201 avec une matrice de texture.
- Captures `--screenshot` de `M01` (Angel Falls : village, cascade, rivière), `C01M09`
  (église de Stornway) et `F01` (plaine d'Angel Falls) : textures à leur place, feuillages et
  fleurs détourés, cascade et brume translucides sans trous.
- Intégration CPU : horloges synthétiques (plage BCFG, vitesse fractionnaire, boucle et
  fichier d'une seule image) ; poses mises en cache comparées à `Geometry::build_posed`
  aux images 0, 4 et 8 de `s000/walk`. Test réel
  `runtime_cast_and_animation_are_deterministic` : mêmes sommets et matériaux après
  60 pas sur `M01`, `C01M09`, `F01` et `M01M08` (ce dernier avec le monstre de la quête 152),
  même topologie, nombre de corps conforme à la distribution.
- Héros : captures `M01` du héros de face au repos et de profil en marche (jambes, bras et
  cheveux animés, cap suivi) ; test réel `hero_model_follows_its_gait` (modèle déterministe, mouvement
  différent par allure).
- Captures natives d'intégration : `M01 --frame 0` et `--frame 60` donnent des PNG
  différents à caméra identique ; `C01M09 --chapter 19 --step 2 --substep 1 --night`
  montre les sprites de la religieuse et du prêtre face à une caméra `--eye/--target`.
  Ce sont des contrôles fonctionnels, pas une comparaison pixel à pixel avec melonDS.
- Éclairage : captures `M01` (jour, nuit, matin, soir : brouillard et ciel visibles, dégradé
  différent par moment) ; comparaison à melonDS sur `C01M09` de nuit et brouillard forcé :
  écart moyen 12,8/255 par canal sans brouillard ; densité de brouillard à 0,01 près sur
  cinq tables (détail et limites : [`re/lighting.md`](re/lighting.md) § « Comparaison à melonDS »,
  `scripts/melonds-capture.ps1`). Lumières DS, ciel et fondu de zone avancée non comparés.
- Navigateur : bundle wasm reconstruit avec `wasm-bindgen`, ROM choisie localement dans
  la page ; `M01` et `C01M09` affichent la carte et les PNJ, sans erreur JavaScript observée.
  Les captures natives des images 0 et 60 de `M01` diffèrent sur 41 143 pixels à caméra
  identique. Le rendu n'a pas encore été comparé image par image à celui de la DS.
