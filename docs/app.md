# Application `dq9-app` et rendu `dq9-render`

L'application commune native/Web affiche une carte, ses modèles, textures, animations, PNJ et un
repère de position du joueur. `dq9-core` consomme maintenant les collisions `.col2`
([`re/collision.md`](re/collision.md)) pour un déplacement de terrain expérimental ; les portes,
changements de carte et l'éclairage restent absents.

- `crates/dq9-core` (sans E/S, compile en wasm) : `CollisionWorld` assemble les `.col2` des
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
cargo run -p dq9-app --release -- --map C01M09 --time 1 --ds-aspect
cargo run -p dq9-app --release -- --save rom/dq9.sav   # charge la carte enregistrée, sans position exacte
# Une image hors écran en PNG, sans fenêtre, puis sortie :
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/m01.png
cargo run -p dq9-app --release -- --map M01 --screenshot build/scratch/app/village.png `
    --size 1024x768 --eye=-5,20,40 --target=-20,0,5
cargo run -p dq9-app --release -- --map M01 --frame 60 --screenshot build/scratch/app/m01-f60.png
cargo run -p dq9-app --release -- --map C01M09 --chapter 19 --step 2 --substep 1 --night
```

`--map` : code (`M01`, `C01M09`) ou id (`1100`) de la liste des cartes ; il prévaut sur la
carte lue dans `--save`. En l'absence des deux options, la carte reste `M01`. `--save` lit un
`.sav` local et sélectionne la carte du premier slot normal valide ; aucune donnée n'est envoyée
au réseau. La position exacte du héros n'est pas identifiée dans la sauvegarde normale : il
commence encore au départ provisoire calculé depuis les collisions. `--time` : moment de la
journée 0 à 3 (`LightingManager+0x98`, choix des variantes et des instances visibles) ;
`--ds-aspect` : rendu au format d'un écran DS (256×192), bandes noires autour.

La distribution est celle du prologue **1.1.1 de jour** par défaut. `--chapter`, `--step`,
`--substep` choisissent le point de l'histoire ; `--night` choisit les placements de nuit,
indépendamment de `--time` (variante graphique de la carte). Une sauvegarde ne restaure ici
que la carte : quête, drapeaux et position exacte du héros ne sont pas chargés. `--frame N`
évalue exactement N pas fixes depuis le début des mouvements avant la capture (0 par défaut),
avec la même mise à jour CPU qu'en fenêtre et sur le Web, sans dépendre du temps de chargement
du GPU. Les sprites sont orientés vers la caméra de `--eye`/`--target` avant cette évaluation.

Commandes : glisser avec le bouton gauche pour tourner, molette pour zoomer. En mode orbite,
`ZQSD`/`WASD` (touches physiques) ou flèches déplacent le héros sur le sol ; `Maj` double sa
vitesse provisoire. `Tab` passe en vol libre, où ces touches déplacent la caméra ; `Q`/`E`
(physiques `A`/`E` en AZERTY) déplacent la cible vers le bas/haut. `F` recadre, `R` format DS,
`T` moment suivant, `PageHaut`/`PageBas` carte précédente/suivante, `Échap` quitter.

## Lancer dans un navigateur

```powershell
cargo build -p dq9-app --release --target wasm32-unknown-unknown
# Une fois : la version de wasm-bindgen-cli doit être celle de Cargo.lock.
cargo install wasm-bindgen-cli --version 0.2.130 --locked --root tools/wasm-bindgen
tools/wasm-bindgen/bin/wasm-bindgen --target web --no-typescript `
    --out-dir crates/dq9-app/web/pkg target/wasm32-unknown-unknown/release/dq9-app.wasm
python -m http.server 8000 -d crates/dq9-app/web     # ou tout serveur de fichiers statiques
```

Ouvrir `http://localhost:8000/` (paramètres `map`, `time`, `ds=1`, `chapter`, `step`,
`substep`, `night=1`), puis choisir la ROM et, facultativement, une sauvegarde `.sav` dans la
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
- **Éclairage** : les sommets à normale reçoivent une lumière blanche fixe ; ni lumières de la
  carte (`.bats`), ni teinte du moment de la journée (`LightingManager`), ni table de toon.
  La plupart des modèles de carte portent leur éclairage dans les couleurs de sommet.
- **Non rendu** : brouillard, contours (*edge marking*), anticrénelage, polygones d'ombre
  (mode 3) et fil de fer (alpha 0), panneaux G3D des modèles, ciel (couleur de fond fixe),
  précision de la DS (couleurs 6 bits, profondeur 24 bits ou W).
- **PNJ, limites** : les hauteurs des PNJ sont celles des placements ; le snap du sol et le
  déplacement du héros sont approximatifs (pas d'accélération, rayon, modèle animé ni départ
  depuis une sauvegarde). Pas d'errance, de chemins, de changement de mouvement par les scripts,
  de visiteurs réseau, d'accessoire `s069` ni de teinte additive des palettes (type 22).
  Le mélange de plusieurs poses, les effets/actions `.beff`/`.bact` et les modes de sprite
  autres que debout/assis ne sont pas exécutés. Les couleurs MAT sont évaluées ; les sommets
  à normales restent soumis à la lumière fixe décrite ci-dessus, pas aux lumières DS.

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
- Captures natives d'intégration : `M01 --frame 0` et `--frame 60` donnent des PNG
  différents à caméra identique ; `C01M09 --chapter 19 --step 2 --substep 1 --night`
  montre les sprites de la religieuse et du prêtre face à une caméra `--eye/--target`.
  Ce sont des contrôles fonctionnels, pas une comparaison pixel à pixel avec melonDS.
- Navigateur : bundle wasm reconstruit avec `wasm-bindgen`, ROM choisie localement dans
  la page ; `M01` et `C01M09` affichent la carte et les PNJ, sans erreur JavaScript observée.
  Les captures natives des images 0 et 60 de `M01` diffèrent sur 41 143 pixels à caméra
  identique. Le rendu n'a pas encore été comparé image par image à celui de la DS.
