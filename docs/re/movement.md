# Déplacement et sol du héros

`dq9-core` reproduit le déplacement du chef de groupe à pied, tel que le code de `ov_017` le
calcule : arithmétique `fx32` (4096 = 1,0) au bit près, un pas par mise à jour du jeu, sans
E/S. Quatre modules : [`fx`](../../crates/dq9-core/src/fx.rs) (diviseur et racine matériels,
vecteurs,
angles), [`movement`](../../crates/dq9-core/src/movement.rs) (`Hero`, d-pad, vitesse, glissement),
[`collision`](../../crates/dq9-core/src/collision.rs) (maillages `.col2`, grille, rayon du sol,
sphère contre les murs) et [`camera`](../../crates/dq9-core/src/camera.rs) (rotation `L`/`R`).
La résolution ROM et NARC est fournie par l'appelant. Les ressources de la carte viennent de
[`collision.md`](collision.md) (format) ; les portes, aires et warps sont testés par
`dq9_core::field` ([`map-archive.md`](map-archive.md), « Portes, zones et changements de
carte »), qui reçoit la pose du héros après `Hero::step`.

## Ce qui est porté

Un `Hero::step(monde, touches, yaw_caméra, ticks)` suit `ov_017::0218e210` pour le chef de
groupe, dans l'ordre du jeu :

1. **Mise à jour générique** (`PartyMember_Update` `0x02037d98` → `Character_Update`
   `0x020332bc`) : le modèle tourne vers le cap voulu de `0x324` par tick (arrêt exact sur la
   cible, `0x02033720`) ; la vitesse monte de `0x28` par tick jusqu'à `0x189` (ou redescend
   vers 0) ; **le déplacement du tick utilise la vitesse du tick précédent**, limitée à
   `0x189 × (1 − |écart modèle/cap| / π)`, le long du cap voulu (pas de l'angle du modèle :
   le drapeau `0x100` du héros) ; enfin la gravité retire `+0xac` à `y`
   (`0x0203349c`).
2. **Lecture du pavé** (`Hero_ReadDirectionInput` `0x02038148`), *après* le mouvement : une
   touche pressée ne déplace donc le héros qu'au tick suivant. Cap = `table[direction] + yaw
   de la caméra`, table `0x020e90b8`. Les diagonales ont priorité, puis haut, bas, gauche,
   droite (haut + bas = haut). Aucune touche : état au repos, la vitesse décroît.
3. **Collision avec la carte** (`ov_017::0219344c`, voir plus bas), en deux demi-passes
   lorsque le héros a parcouru plus de `0x333` (0,2 unité) pendant le tick ;
   le jeu y arrive à 3 ticks : `0x189 × 3 = 0x497`, alors que 2 ticks (`0x312`) restent
   sous le seuil.
4. **Glissement le long du mur touché** : si le mur touché (triangle mémorisé en `+0xe4`) et
   le déplacement restent supérieurs à `0x27`, le cap du mur (`FX_Atan2` de sa normale) est
   comparé au cap voulu : à moins de ≈ 145° (`10294,4`) on garde ce cap ; au-delà de 90°
   (`0x1922`) le cap devient celui du mur ± 6772,6 (≈ 95°) ; si le déplacement réel est à
   moins de 643,4 (≈ 9°) de ce cap, le cap voulu devient celui-là (`0x0218e4a0`–`0x0218e5a4`,
   flottants lus sur la ROM).
5. **Bornes du déplacement** : un mouvement inférieur à 5 sur chaque axe est annulé ; la
   distance parcourue après collision ne dépasse pas celle d'avant (`0x0218e5a8`–`0x0218e658`).

`ticks` est le nombre d'images écoulées depuis la dernière mise à jour du jeu
(`GameState+0x3c4`, plafonné à 3, 2 au départ) : la logique du champ tourne à 30 Hz, 2 ticks
par mise à jour. La boucle externe de `dq9-app` reste à 60 Hz (VBlanks) ; à chaque VBlank
`App::run_steps` incrémente `tick` puis ne fait tourner `Hero::step(..., 2)`, `Field::step`,
la démarche et l'instance de rendu que sur les `tick` impairs, la pose étant tenue à l'image
intermédiaire. Audio, portes, messages, VM d'événements, caméra et `runtime.step` restent à
60 Hz. `tick` est la phase VBlank libre du jeu : il n'est remis à 0 que par
`start_new_game`, jamais par un chargement de carte, une sauvegarde ou un retour de
contrôle. La vitesse en régime est la même (5,76 unités/s) et la montée en vitesse, par
`0x28` par tick, aussi.

Cette cadence est vérifiée sur la ROM réelle par
`field_cadence_holds_the_pose_and_matches_the_original_every_vblank` (`app.rs`) : à partir de
la pose `(1,756 ; −3,041)`, angle brut 0, vitesse 0 de `X01M05`, les pas impairs donnent les
angles bruts exacts 0, 1608, 3216, …, 12868 et les `z` fx de la capture originale, les pas
pairs tenant la pose octet pour octet (caméra comprise). Le premier VBlank ne lit que le
pavé, le premier déplacement est au troisième pas de champ (VBlank 5) et le demi-tour se
cale sur la cible au VBlank 19 (revue
`build/v01-20261107/review/cadence-30hz-field-review.md` §7).

**[hypothèse]** `Field::step` (donc `step_doorways`) tourne lui aussi une fois par mise à
jour, comme `Field_CheckRegions` : le compteur d'approche d'une porte, à
`APPROACH_FRAMES = 3`, déclenche alors l'événement 17 au 4e appel, soit 8 VBlanks. La chaîne
d'appel statique (`0x020d6cb4` → `ov_017::0218b688` → `0218cbd4` → `Field_CheckRegions`) ne
donne pas la cadence de la boucle principale ; la mesure melonDS (4 VBlanks ⇒ champ à 60 Hz,
8 ⇒ 30 Hz) reste à faire. Ne pas régater `field.step` seul sans cette mesure.

**`--screenshot`/`--frame`** ([`outils.md`](outils.md)) est une voie d'aperçu hors
`App::run_steps` : elle garde `Hero::step(..., 1)` par image demandée et ne prétend pas
reproduire la cadence. Les chemins de jeu (fenêtre, Web, pilote, rejeu) passent tous par
`run_steps`.

**[hypothèse]** les écarts d'échelonnage (hystérésis du sol de `0x28`, arrondis des demi-passes)
sont négligeables à cette cadence.

### Collision (`CollisionWorld::resolve`, `ov_017::0219344c` / `02192b48`)

- Une *pièce* est un `.col2` placé (position et angle du placement). Le jeu travaille dans le
  repère de la pièce : position − origine de la pièce, tournée de −angle (`MTX_RotY33_`,
  `MTX_MultVec33`), sommets = entiers du fichier `<< shift`.
- **Sélection des triangles** (`0x0204c040`, `Collision::query`) : boîte de la requête
  `[x ± rayon/2, y − 10 … y + hauteur, z ± rayon/2]` (rayon `0x1000`, hauteur `0x1800` du héros,
  `ov_017::0218f064`) ; test contre les bornes du maillage, puis cellules de la grille (voir
  [`collision.md`](collision.md)) et boîte de chaque triangle (six quartets de son mot
  d'attributs) ; 0x60 triangles au plus. La boîte n'est pas tournée avec la pièce (comme dans
  le jeu) : **[hypothèse]** les pièces de sol n'ont pas d'angle.
- **Sol** : rayon vertical de `y + 0x1800` à `y − 0xa000` contre les faces de normale `y >
  0x800` ; la plus haute (hauteur interpolée), puis le point exact sur son plan (`0x02032090`,
  t en 12 bits : quelques unités de bruit sur `y`, comme le jeu) + `0x199`. Le héros prend le
  sol le plus haut des pièces s'il est à moins de `0x800` (0,5) sous lui, ou moins de `0x1000`
  au-dessus ; `y` ne change que si l'écart dépasse `0x28`. Un sol plus de 0,5 sous lui ne le
  porte pas : huit rayons de 0,25 unité autour confirment qu'il n'y a pas de sol proche, le
  héros tombe (`+0xac` croît de `0x51` par image jusqu'à `0x1000` ; ramené à 0 au sol).
  Aucun sol du tout : le héros reste en place (pas de chute dans le vide).
- **Murs** (`0x020315c8`) : sphère de rayon `0x800` centrée à `y + 0x800`, contre les faces de
  normale `y ≤ 0x800` ; première passe : segment du centre le long de −normale (rayon de la
  sphère) ; si il traverse le triangle, le centre est poussé à `rayon` du plan ; seconde passe :
  point le plus proche du triangle (`0x02031d30`) pour les bords et sommets. Un piège de coin
  (normales presque égales, `0x800 < n·n' < 0xfd7`) renvoie la sphère à sa position précédente.
- Les valeurs de `0x021d62d8`, `0x021d62f8` (huit rayons de bord), `0x020319e0`/`e4`, les
  flottants du glissement et la table des caps sont lues par `tests/movement_rom.rs`.

### Caméra de terrain (`dq9_core::camera::FieldCamera`, `0x020a213c`)

`L` (`0x200`) ajoute au yaw, `R` (`0x100`) retire, de `trunc(0,0015 × Δms × 4096)` (202 à 33 ms) ;
le yaw ne peut pas entrer dans la bande `(π/4, 7π/4)` : il s'arrête sur son bord. `L` et `R`
ensemble ramènent le yaw au yaw de base à `0,00225 × Δms × 4096` par mise à jour (arrêt à
`0x28`). L'application (`Q`/`E` en orbite) mesure le yaw depuis celui de l'entrée sur la carte.
L'application initialise l'orbite aux valeurs du constructeur (`0x0202df78`) : distance 11,
œil 6 unités au-dessus de la cible, champ vertical 30°. La cible suit la position du héros
+ la moitié de sa hauteur (0,75), comme `FieldCamera_Update` (`0x020a213c`). Le cadrage des
bornes de toute la carte reste réservé au vol libre (`F`) ; `F` en orbite rétablit le suivi.

Une scène garde les paramètres de la caméra de terrain ; un changement de carte dans la
chaîne remplace ce point de retour par celui de la destination. Le dernier état de scène est
appliqué **avant** la restitution, puis la cible rejoint le héros à sa pose finale : aucune
présentation inactive ne peut réinstaller le dernier gros plan. Le vol libre, lui, retrouve
son point de vue sans suivre le héros. La projection des caméras cinématiques reste séparée.

### Hors périmètre (non porté)

Personnages poussant le chef de groupe (`ov_017::0218df50`, `0218dfd8`), colliders de 8 triangles
des objets de zone (`ov_017::02193290`), cartes `0x1131` et `10000`, record de terrain copié
sur le personnage (`02033f6c`), mode grotte (`+0x23b8`), branche véhicule de `0x02017da0`,
aimantation sur les zones orientées (`0x0201e90c`), stylet, échelles (`0x020385a8`), événements
qui prennent la main, animations (états autres que repos et marche), rayon de mur élargi du drapeau `0x20`.

## Position de départ

Une sauvegarde normale ne stocke pas la position du héros : elle donne la carte (id `11476`,
par ex. 109 = `C01M09`) et le jeu pose le groupe au point de départ de la carte,
l'enregistrement `0x6e` de son `.bmbl` (`Party_Place`, `ov_017::0219c598` ;
[`save.md`](save.md) § « Position à la reprise »). `dq9_core::state::GameState` en déduit la
pose (`hero_pose`) et `dq9-app --save` y place le héros (`Hero::at`), recalé sur le sol trouvé
par `CollisionWorld::ground_height` (`0x02018fcc`, `0x199` au-dessus, comme le jeu). Le jeu
pose le cap voulu et l'angle du modèle sur l'angle de départ (`0x020399c0` → `0x02033874`) et
met la vitesse à 0 : `Hero::place` fait de même. Une carte sans enregistrement, une pose d'une
autre carte ou l'absence de sauvegarde gardent le point provisoire calculé depuis les collisions
(`CollisionWorld::spawn`). Position et cap sont égaux à ceux de melonDS sur la démo (109) et sur
la même sauvegarde repointée sur la carte 100.

## Analyse du code

Obtenue avec `scripts/ghidra-dump.ps1` (décompilation de `ov_017` et de l'ARM9 dans
`build/decomp/`). Adresses européennes ; noms dans `re/symbols.txt`.

### Qui déplace le héros

- `ov_017::0218d8cc` ne déplace pas le héros : elle fabrique `field+0x4438` (direction du pavé
  dans le monde), lu par les échelles et le dialogue.
- Le déplacement du chef de groupe passe par `ov_017::0218e674` (boucle sur les membres) →
  `ov_017::0218e210`.
- `0x020a654c` → `0x020a75fc` (touches + `Character_Update`) agit sur l'objet `GameState+0x32c`,
  **nul** dans l'état `field.ml1` : ce n'est pas le héros à pied (**[inconnu]** : véhicule ?).
- Ordre de `PartyMember_Update` (`0x02037d98`) : `0x02052af8` (mise à jour générique,
  `Character_Update` par une table de sauts à `0x02048144`) puis `Hero_ReadDirectionInput`
  puis `0x020385a8` (échelle).

### Entrée → cap (`0x02038148`)

- Code de direction `+0x250` : Haut+Gauche `0xc`, Haut+Droite `0xd`, Bas+Gauche `0xe`,
  Bas+Droite `0xf`, Haut 8, Bas 9, Gauche 10, Droite 11 (touches `0x40`, `0x80`, `0x20`,
  `0x10`).
- Cap = `table[code-8] + yaw caméra`, réduit à [0, 2π] ; table `0x020e90b8` en radians fx32
  (4096 = 1 rad, π = `0x3244`, 2π = `0x6488`) : Haut π, Bas 0, Gauche `0x4b66` (3π/2), Droite
  `0x1922` (π/2), Haut+Gauche `0x3ed5`, Haut+Droite `0x25b3`, Bas+Gauche `0x57f7`, Bas+Droite
  `0x0c91`. Convention : direction = (sin θ, 0, cos θ) ; la caméra à yaw 0 regarde vers −z.
- Avec touche : état 1 (marche), `Character_SetFacing` (`+0xae` = cap). Sans : état 0.

### `Character_Update` (`0x020332bc`)

Champs : `+0x44` position fx32 ×3, `+0x54` angle du modèle, `+0xac` vitesse de chute,
`+0xae` cap voulu, `+0xb0` vitesse de rotation, `+0xb2` vitesse courante, `+0xb4` vitesse
max, `+0xb6` accélération, `+0xbe` état. Valeurs d'initialisation (`0x02032e68`, lues dans la
ROM ; `ov_017::0218f064` écrit `0x324` et `0x189` pour les membres du groupe) : rotation
**`0x324`** (≈ 0,196 rad / tick), vitesse max **`0x189`** (393/4096 = 0,096 unité/tick =
**5,76 unités/s** à 60 ticks/s), accélération **`0x28`**.

La vitesse est lue **avant** sa mise à jour (`ldrsh r4, [r8, #0xb2]` en `0x0203355c`, puis
écriture sans relecture) ; elle monte de l'accélération sans plafonner (`0x190` après dix
ticks), puis retombe à `0x189` quand l'excédent est inférieur à l'accélération. La
décélération peut passer sous 0 sur un tick (mouvement nul : le déplacement exige une vitesse
> 0). Gravité : `y −= +0xac` ; au-dessous de `−0x32000`, `y` est remis à `+0x32000`.

### Caméra de terrain

Objet caméra : `+4` œil, `+0x10` cible, `+0x70` yaw, `+0x74` dy, `+0x78` distance. Défaut
(`0x0202df78`) : yaw 0, dy `0x6000`, distance `0xb000`. `Camera_SetOrbit` (`0x0202e5e8`) :
œil = cible + RotY(yaw) · (0, dy, √(dist² − dy²)). `FieldCamera_Update` (`0x020a213c`) : la
cible est la position du personnage `+0x21c` + hauteur/2 + décalage `+0x22c`. Constantes
(`0x020a2708`…`0x020a2724`) : `0x020a270c` = 0,00225, `0x020a2714` = 0,0015, `0x020a2710` = 4096,
bande `0xc91`/`0x57f7`, `0x020a2720` = `0xccc`, `0x020a2724` = `−13516`.

### Arithmétique

`FX_Div` = `((a << 32) / b + 0x80000) >> 20` (diviseur matériel, troncature ; diviseur nul : 0),
`VEC_Mag`/`VEC_Distance` = `(⌊√(4 Σ v²)⌋ + 1) >> 1`, `VEC_Normalize` = `⌊√(4 Σ v²)⌋ ×
⌊2⁵⁶ / Σ v²⌋` puis `(· · v >> 32 + 0x1000) >> 13`, produit et scalaire `(a·b + 0x800) >> 12`,
`VEC_DotProduct` et produit vectoriel arrondis une fois sur la somme, `FX_Atan2` par la
table de 129 entrées `0x020ed460` (`round(4096 · atan(i/128))`, vérifiée), sin/cos par
`0x020e9460`. La rotation de pièce (`0x020c3260`/`32f8`) est un sinus/cosinus 64 bits tronqué
à `fx32` après conversion flottante ; le port évalue `sin`/`cos` en `f64` sur l'argument
`0x6487 − angle` (un de moins qu'un tour, comme le jeu) : **[hypothèse]** identique au bit près,
écart possible d'une unité sur les pièces tournées.

## Comparaison melonDS

Les états existants `build/melonds/before.ml1` et `moved.ml1`, produits dans `C01M09`, exposent
le compteur VBlank et la position du héros : (0,210 ; 0,120 ; −1,380), puis (−2,853 ; 0,106 ;
3,854), à 1969 VBlank d'écart ; ils ne sont pas alignés et ne valident pas la vitesse.

**Mesure de vitesse** (`scripts/melonds-state.ps1 -Save build/sav/demo.sav -Settle 8 -Keys
"A:0.2,idle:1.5 ×4, B:0.2,idle:1.5 ×3, Down:0.4"`) : les quatre `A` font défiler le dialogue de
l'église de Stornway jusqu'au menu, les trois `B` le ferment ; le héros, à (0,210 ; −1,380)
face au nord, reçoit `Bas` pendant 0,4 s puis 0,5 s de repos avant l'état. Position lue en RAM
(`GameState+0x3ac` → objet, `+0x44`) : **Δz = 2,361**, cap final 0 (face au sud). Le port, même
position et même cap de départ, mêmes entrées : Δz = 2,095 pour 24 images de touche à 60 Hz
(2,287 pour 26) et 2,168 à 2,360 pour 11 à 13 mises à jour à 30 Hz (`ticks = 2`) : l'écart à la
mesure est de l'ordre de l'image de gigue des messages postés (0,096 unité par image) ; le
modèle provisoire (4 unités/s, sans accélération ni demi-tour) donnait 1,6. Une seconde mesure
(`Bas` 0,9 s) n'a pas bougé le héros (le dialogue n'a pas été fermé à temps) : **une seule
mesure, de ± 1 image** ; la validation image par image reste à faire.

## Reste à faire

Toutes les entrées non portées de « Hors périmètre » ; le suivi lissé et les zones de caméra
(l'orbite par défaut et le suivi direct du héros sont appliqués) ; une comparaison position
par position à nombre d'images égal sous melonDS.

Le pilote juge désormais ses points de passage et son but en **3D** (§ « Retour de 1.2.1 ») ;
la grille ne relie deux cellules voisines que si l'écart d'étage reste sous `STEP_HEIGHT`
(0,75), donc une rampe ou un escalier continus, jamais deux coursives empilées ; l'arête est en
plus validée par `Hero::step`. Une cellule peut porter plusieurs étages. Restent : la
comparaison à melonDS du parcours de retour et la mesure de la cadence de `Field::step` pour
les portes (§ « Ce qui est porté »). La sonde de connectivité du pilote (`edge_pass`) garde
`ticks = 1` : c'est un test de franchissement, pas la cadence du champ.

`cargo dq9re map --reach` ([`map-archive.md`](map-archive.md#voir-la-collision-dq9re-map---image---reach))
rejoue `Hero::step` sur une grille de 0,5 unité et rend les composantes par hauteur visibles :
c'est l'outil à employer avant d'annoncer une route, plutôt qu'un BFS qui ne compare que
`x/z`. Deux précautions : la grille du remplissage est ancrée sur son point de départ, donc un
`--to` très précis peut manquer une cellule déjà atteinte (comparer des points ronds ou
relancer depuis l'autre extrémité) ; et la commande charge la carte **statique**, sans les
demandes `0x95`/`0x96` d'un trigger.

## Parcours joueur réel du prologue (preuve de marche)

Le pilote headless marche (`Hero::step`, collisions, portes, recherche de partenaire) et la
trace de ses entrées est rejouable sans lui :

```powershell
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot `
  --checkpoints build/checkpoints.jsonl --trace-inputs build/inputs.jsonl
cargo run -p dq9-app --release -- --new-game --headless `
  --replay build/inputs.jsonl --checkpoints build/replay.jsonl
```

- **22 670 pas fixes** dans `inputs.jsonl` (pavé et touche action par image) ; le rejeu sans
  pilote donne **les mêmes checkpoints octet pour octet** (sha256
  `3da80263943fe45ab8bde29c4b23657bc2b82102fea2de09d2fb4db59f57e2a8`) jusqu'à l'histoire 1.1.2.
- Le lot tour (main `8d41723`) porte la trace à **26 681 pas fixes** et à l'histoire
  **1.2.1** (fin de 21510 à 391,12 s), enregistrée dans
  `build/v01-20261107/tower/coordinator-fixed-pilot-inputs.jsonl` et ses points
  `…-checkpoints.jsonl` (journal `…stdout.txt`, erreurs `…stderr.txt`). Le pilote y arrive
  sur `4101` à 407,67 s par le warp 0 de `4103`, puis s'arrête à 444,7 s : c'est le retour
  décrit plus bas.
- **Premier contrôle** à 210,42 s : `X01M05`, aire 4100, 1.1.1, drapeaux globaux
  `{337, 1450, 1451, 1452, 1453, 1454, 1461, 2721, 2751}`, drapeaux d'étape `{1}`, héros
  `(0,660, -0,250, -3,610)` — les six drapeaux de scène et les deux de carte concordent avec
  l'état melonDS `build/orca/oracle/bat6.ml1` ; neuf drapeaux globaux restent inconnus
  (`trigger.md`, § « Reste inconnu »).
- Étapes marchées : conversations 1140 et 1130 à `X01M05` (210–295 s), warp 9 vers `X01M07`
  (312 s), double porte de bibliothèque poussée puis ouverte (deux vantaux, 331–334 s), aire 2
  ajoutée par l'événement 20 (événement 1150), NPC 130 (1600), retour par la porte vers
  `X01M01` (356 s), NPC 127 (1170) à 369,8 s → **histoire 1.1.2**.
- Le pilote s'arrêtait à 377,8 s sur `X01M01` : ses **sondes dans les huit directions** depuis
  y 8,25 et y 11,25 plafonnent à y 11,25. Ce n'est pas seulement la sonde : une marche
  exhaustive depuis la pose d'histoire (`(2,77 ; 11,25 ; 18,19)`) **et** depuis le point de
  départ de la carte (`(0 ; 8,25 ; 30)`), avec des arêtes validées en 3D (`Hero::step`, montée
  ≤ 1,0, chute ≤ 10), plafonne aussi à y 11,25. Le mur fin `X01A01S1` (bloc 0, instance 16,
  normale `+x`, `x 0,018..0,105`, `z 15,42..18,99`, `y 9..14`) sépare la terrasse du pied de
  la rampe menant au warp `0x72` **type 0** (n° 9) vers `X01M03`. Les zones `0x73` de
  **type 1** sont **inertes** (aucun lecteur dans `Field_CheckRegions`, voir
  `map-archive.md`) : ce n'est ni un monte-charge ni une tâche 8. La cause n'est pas une
  connexion manquante mais une **barrière scriptée** que le port n'appliquait pas
  (§ suivant). `NavGrid` collecte tous les sols d'une colonne
  (`CollisionWorld::floors`) et valide chaque arête en 3D (`Hero::step`, distance 3D au
  voisin) : une arête qui ne fait que glisser sous une terrasse est rejetée.
- Limite de la trace : le pilote donne les directions avec la caméra du jeu à l'angle 0 et le
  rejeu force ce même cap ; elle prouve le parcours headless dans notre moteur, pas un rejeu à
  la manette melonDS avec caméra manuelle.

### Barrière de la rampe (actions 0x95/0x96, placement 16)

La barrière `X01A01S1` est le **placement 16** du bloc 0 (id d'instance, pas le numéro de
ressource 12). Le déclencheur qui la lève est dans `data/scenario/triggerX01.bin` :

```
$ cargo dq9re triggers X01 --story 1.1
room 4101 1.1..1.1 event 11: if 0x8(1170) then 0x68(set_track_flag,4) \
    0x95(hide_object,0,1048576) 0x84(advance_track,0,1,1,2)
room 4101 1.1..1.1 event 20: if 0x9(4101) 0x23(1) then 0x96(show_object,0,1048576)
room 4101 1.1..1.1 event 20: if 0x9(4101) 0x23(2) then 0x95(hide_object,0,1048576)
```

L'opérande `1048576 = 0x00100000` porte l'id de placement dans sa moitié haute : `16`. La
décompilation européenne (`Trig_RunAction` `0x02061c14`, cas `0x31`/`0x32` = actions
`0x95`/`0x96`) appelle `Zone3D_FindPlacement` (`0x02019518`, entrée de bloc = `arg`, id =
moitié haute) puis `func_02013390(placement, arg, 0|1)`, qui pose ou efface le bit `0x4` de
`placement+2` et pose le bit `0x40` : exactement ce que fait la native 574
(`StbNat_SetPlacementHidden`). La requête de collision lit ce bit : `Zone3D_PieceTriangles`
(`0x02017da0`, `iVar3+0x28` = parent) rend 0 triangles si le nœud de la pièce, ou un de ses
parents, a le bit `0x4` de `+2` (`(*(ushort *)(iVar3 + 2) & 4) == 0` puis la boucle sur les
parents), exactement l'état que notre `piece_hidden` parcourt. L'événement 11 de fin de 1170
cache donc la barrière **juste avant** d'avancer la piste à 1.1.2 ; l'événement 20 de
chargement la montre à la sous-étape 1 et la cache à la sous-étape 2. Le chargement de la
carte est fidèle : sur l'état melonDS `build/melonds-height/probe.ml1`,
`cargo dq9re map X01M01 --state …` compare les 17 instances (placement 16 compris, à
l'origine) et conclut « the loaded blocks match the assembly » ; seul l'effet du trigger
manquait.

Correctif : `CollisionWorld::set_placement_hidden(bloc, id, caché)` rend le placement et son
sous-arbre invisibles aux requêtes (sol, murs, `floors`, `resolve`), avec un compteur
`revision()` ; `set_placement_position` déplace le sous-arbre pour la native 575. L'action
`0x95`/`0x96` produit une demande `Request::SetPlacementHidden` (immédiate, avant les actions
différées) que l'application traite sur la collision et sur les instances dessinées ; les
natives 574/575 suivent le même chemin pour les placements de collision, y compris depuis le
rapport `--events-report`, et le pilote jette sa grille de sol quand `revision()` change ou
quand il entre dans une carte (sa collision neuve repart de la révision 0). Les tests de
collision couvrent cacher/montrer, le bloc qui ne correspond pas, le parent qui cache son
descendant, le déplacement du sous-arbre et la rotation d'un parent déplacé ; les tests sur la
ROM jouent la vraie demande de l'événement 11 puis la marche continue jusqu'à `X01M03`.

Vérification : `fmt`, `clippy --all-targets`, les tests de l'espace de travail sur la vraie ROM
et la compilation wasm sont verts (dont les 28 tests de l'application, exécutés avec la ROM
et une sauvegarde explicites). Preuve de collision, sans téléport ni mur abaissé :

```powershell
# Contrôle NÉGATIF : la carte statique garde S1 (le placement 16 n'est caché que par
# l'événement 11 de fin de 1170, que cette commande ne joue pas) ; « not reached » est
# donc attendu et ne prouve pas un défaut du port.
cargo dq9re map X01M01 --reach=2.77,11.25,18.19 --to=-8.226,13.97,16.379
# 9320 nœuds, y 4.249..11.251, « not reached » — contrôle négatif sur la carte statique

# Depuis l'ouest de S1, sur la rampe, le remplissage 3D atteint le warp et monte à y 23,118
# (toujours la carte statique : S1 est encore là, la rampe est du bon côté).
cargo dq9re map X01M01 --reach=-1.25,12.42,17.5 --to=-8.226,13.97,16.379
# 9542 nœuds, y 4.249..23.118, « reached »

# Contrôle POSITIF du cache : le test d'application joue la vraie demande de l'événement 11
# (placement 16) puis la marche réelle du héros jusqu'à X01M03 :
cargo test -p dq9-app trigger_1170_hides_the_tower_gate_and_the_hero_walks_to_x01m03
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot
# observé : 1170 finit à 334,45 s (histoire 1.1.2) ; la carte 4103 est atteinte à
# 336,40 s par le warp n° 9, puis 4109 -> 4103 -> 4104 ; 21510 (PNJ 199) finit à
# 391,12 s (histoire 1.2.1). Le retour depuis 1.2.1 est décrit ci-dessous.
```

Le contrôle négatif et le test d'application ne mesurent pas la même chose : le premier
charge la carte **statique** (S1 visible, aucune demande `0x95`), le second joue la demande
réelle de l'événement 11 et la marche du héros. Aucune option CLI `--reach` ne cache un
placement : ne pas présenter la carte statique comme l'état du jeu après 1170.

## Retour de 1.2.1 vers le bas de l'Observatoire

Le pilote atteint l'histoire **1.2.1** à 391,12 s (fin de 21510 sur `4104`), revient par
`4103` puis arrive sur `4101` à 407,67 s à `(-3,9 ; 12,47 ; 16,75)`, sur la rampe haute.
Il s'y arrêtait à 444,7 s. La cause est **dans le pilote**, pas dans la collision :

- le plan mène au warp `0x72` n° 8 de `4101` (boîte `(0 ; 5 ; 29)`, 4×4×2 → `y 3..7`) dont le
  sol de station est à `y 4,25`. Le suiveur testait ses points de passage et son but en
  **`x/z` seulement** : arrivé à `(-0,16 ; 8,25 ; 28,97)`, à 0,26 du but en `x/z` mais **4
  unités au-dessus**, il déclarait le but atteint, poussait le cap nul du warp pendant 20
  images puis abandonnait (« no way out at Vec3(0.01, 4.25, 29.17) », stderr de
  `build/v01-20261107/tower/coordinator-fixed-pilot.stderr.txt`) ;
- la connexion existe : le palier `4,25` communique avec la terrasse (`--reach` depuis
  `0,4.25,29` couvre `y 4.249..11.251`), et le warp n° 8 s'atteint depuis le palier en
  traversant la double porte `class 0, number 3` (`4101`, enregistrement `0x74` #14, zone
  `0x73` #13 centrée `(0 ; 5 ; 30,5)`, drapeau `8` = `FLAG_WARP`) ;
- les sorties vers `4105` de `4101` (`0x72` n° 7 en `(0 ; 1 ; 34)`, portes 0/6 et 0/7 en
  `(0 ; 1 ; 35)`) sont **sous** le palier (boîtes `y -1..3` et `-0,5..2,5`) : aucun plan
  n'atteint leurs cellules depuis `4,25`, et ce n'est pas un défaut à corriger en élargissant
  la tolérance de hauteur. Le retour passe par `4107` ;
- le carnet de sorties était indexé par `(carte, destination, histoire)` : **une** sortie
  échouée vers `4107` interdisait aussi les portes vers `4107`, d'où l'oscillation
  `4101 ↔ 4103 ↔ 4109 ↔ 4104` puis « no walkable way back through map 4101 to 4105 ».

Correctifs (`crates/dq9-app/src/app/auto.rs`, code tenu pour les contrôles centraux) :

- `LEVEL_REACH = 0,8` : un point de passage ou un but n'est atteint que si `|Δy|` est
  inférieur, en plus de la distance `x/z`. Une tolérance plus large (1,5 avait été essayé)
  laissait le héros « atteindre » la première marche d'un escalier depuis la rampe au-dessus
  d'elle, puis pousser vers l'est sans jamais descendre ; 0,8 couvre la demi-montée d'une
  arête (≤ 0,625) et la hauteur de station (0,0999) ;
- les cellules bloquées après un coincement sont mémorisées par **nœud** `(cellule, étage)`
  et non par cellule : une coursive au-dessus d'un passage n'est plus interdite avec lui ;
- le carnet de sorties distingue chaque sortie (`class`/`number`, position, destination) :
  une sortie en échec n'interdit plus les autres vers la même carte ;
- un point d'appui « devant la porte » doit être sur l'étage de la porte (`|Δy| < 1,5` par
  rapport au centre du volume) : le palier au-dessus d'une porte du dessous n'est plus
  proposé ;
- une route épuisée sans que le but soit atteint n'est **plus** prise pour une arrivée : le
  pilote replanifie depuis sa position ; si la grille refuse le plan, le but est abandonné.
  Avant, une route vide faisait compter les 20 images de sortie et concluait « no way out »
  alors que le héros était encore quatre unités au-dessus du but ;
- un coincement n'arrête plus la course : le but est abandonné et noté comme essayé (même
  carnet que les sorties), le pilote en choisit un autre, et `Stop::Stuck` n'est rendu que
  lorsqu'il ne reste plus rien à tenter. Un seul replan par fenêtre de 45 images
  (`stall` remis à zéro), pour que les cellules apprises diffèrent au lieu d'épuiser les huit
  replans en huit images ; le replan bloque le **nœud où le héros est coincé**, pas le point
  de passage (déjà validé par la sonde), pour ne pas interdire le but lui-même ;
- une sortie dont la marche a échoué est retenue **hors de l'histoire**
  (`failed_exits` : carte, destination, clé de sortie) : un objectif qui échoue n'est pas
  rejoué indéfiniment après chaque changement de drapeau ; les autres sorties vers la même
  carte restent tentables.

Ces règles sont génériques : aucune exception par carte, par étape ou par porte.

Régression corrigée : le test
`trigger_1170_hides_the_tower_gate_and_the_hero_walks_to_x01m03` part d'un pilote neuf sur
`4101` à 1.1.2 ; la première sortie essayée est le warp n° 8 vers `4107`, dont la salle est
**derrière** la double porte (la zone du warp ne contient que `z 28..30`) et dont le plan de
grille s'avère impraticable. Avec le test d'arrivée en 3D seul, le suiveur suivait ce plan et
se bloquait à `(17,0 ; 6,24 ; 28,73)` ; l'ancien suiveur s'arrêtait plus tôt (arrivée en `x/z`)
et passait au warp n° 9 vers `4103`. Désormais le but est abandonné et la sortie suivante
(`4103`) est prise, ce que le test exige.

Preuve de descente physique, sans `NavGrid`, sans téléport, sans drapeau forcé : le préfixe
du parcours du pilote jusqu'à l'image 24 460 (qui pose le héros sur `4101` à
`(-3,9 ; 12,47 ; 16,75)`) suivi de 450 images de pavé seulement (droite 90, bas+droite 60,
gauche 150, haut 150) traverse la carte par le bas est, le palier et la porte :

```powershell
# entrées : build/v01-20261107/return/replay-descent-4107.jsonl (25 442 images, aucune touche A)
cargo run -p dq9-app --release -- --new-game --headless `
  --replay build/v01-20261107/return/replay-descent-4107.jsonl `
  --checkpoints build/v01-20261107/return/replay-descent-4107-cp.jsonl
# observé : image 24 460, carte 4101, héros (-3,9 ; 12,47 ; 16,75), histoire 1.2.1 ;
# image 25 299, carte 4107, héros (0 ; 0,17 ; 16,9), histoire 1.2.1, cause
# « Doorway { class: 0, number: 3 } ». sha256 c15c6b54… (entrées) et 192881e1… (points).
```

Résultat du pilote corrigé sur l'état 1.2.1, avec le plafond de pas demandé :

```powershell
cargo run -p dq9-app --release -- --map X01M01 --chapter 1 --step 2 --substep 1 `
  --headless --auto --pilot --headless-steps 30000 --mute
# observé : 4101 -> 4107 par « Doorway { class: 0, number: 3 } » à 44,6 s ; PNJ 139 (Apus) ;
# 4107 -> 4105 par le warp 1 à 59,9 s ; puis 4105/4106 et 4105/4107/4101 ; arrêt à 207,8 s
# sur 4101. Aucun téléport.

# Pilote complet depuis la nouvelle partie, mêmes règles, deux minutes de jeu :
cargo run -p dq9-app --release -- --new-game --headless --auto --pilot `
  --checkpoints build/v01-20261107/return/full-return-cp.jsonl `
  --trace-inputs build/v01-20261107/return/full-return-inputs.jsonl
# observé : 407,7 s retour sur 4101 (1.2.1) ; 475,7 s sortie vers 4107, arrivée à 485,8 s
# par la porte 0/3 ; Apus (PNJ 139) ; 4105 à 500,9 s par le warp 1 ; gardien 101 à 514,1 s ;
# 21591 de 519,7 à 526,3 s puis carte 1100 (Angel Falls) par le warp du trigger 11 ;
# 21592, 1380, 1390, 1370 (scènes du village) ; M01M08 à 582,4 s ; arrêt à 665,2 s en
# contrôle libre sur 1108, héros (-2,20 ; 0,10 ; 0,78). Historique 1.2.1, drapeaux de piste
# 0 {4, 7} et {0, 1, 3, 12, 13}. Aucun téléport, aucun drapeau forcé.
```

Le chemin physique correspondant : descendre le flanc est (`8,25 → 6,14 → 4,25`), longer le
`4,25` vers l'ouest jusqu'au palier `(0 ; 4,25 ; 31,1)`, entrer vers le sud dans la double
porte → `4107`. Dans `4107`, Apus (PNJ 139, événement 0 : `0x68(set_track_flag,0)`), puis le
warp `0x72` n° 8 en `(0 ; 0 ; 9,5)` vers `4105`, le gardien (PNJ 101, événement 1 :
`0xb(193) 0x10(0)` → `0x77` 21591), et 21591 → `0xe1` warp vers `1100`.

Limites : le code reste à committer après les contrôles centraux ; la preuve de descente est
un rejeu de nos entrées dans **notre** moteur, pas un rejeu melonDS ; l'ordre et les durées ne
sont pas comparés ; la suite à `1100` (quêtes du village) appartient aux lots village et
scènes.

Les compteurs de pas, temps et positions ci-dessus ont été mesurés avant la cadence de champ
30 Hz de `run_steps`. Depuis, le pilote avance toujours d'un pas fixe de 60 Hz mais la logique
de champ ne consomme le pavé qu'un VBlank sur deux : les trajectoires restent dans le moteur,
mais les numéros d'image et les points de contrôle peuvent se décaler et doivent être
re-mesurés par le lot de vérification, sans changer les entrées du pilote.
