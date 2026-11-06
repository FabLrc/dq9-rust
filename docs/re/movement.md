# Déplacement et sol du héros

`dq9-core` reproduit le déplacement du chef de groupe à pied, tel que le code de `ov_017` le
calcule : arithmétique `fx32` (4096 = 1,0) au bit près, un pas par image, sans E/S. Quatre
modules : [`fx`](../../crates/dq9-core/src/fx.rs) (diviseur et racine matériels, vecteurs,
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
(`GameState+0x3c4`, plafonné à 3, 2 au départ) : la logique tourne à 30 Hz nominal, 2 ticks
par mise à jour. L'application avance d'un tick à 60 Hz ; la vitesse en régime est la même
(5,76 unités/s) et la montée en vitesse, par `0x28` par tick, aussi.
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
- **Premier contrôle** à 210,42 s : `X01M05`, aire 4100, 1.1.1, drapeaux globaux
  `{337, 1450, 1451, 1452, 1453, 1454, 1461, 2721, 2751}`, drapeaux d'étape `{1}`, héros
  `(0,660, -0,250, -3,610)` — les six drapeaux de scène et les deux de carte concordent avec
  l'état melonDS `build/orca/oracle/bat6.ml1` ; neuf drapeaux globaux restent inconnus
  (`trigger.md`, § « Reste inconnu »).
- Étapes marchées : conversations 1140 et 1130 à `X01M05` (210–295 s), warp 9 vers `X01M07`
  (312 s), double porte de bibliothèque poussée puis ouverte (deux vantaux, 331–334 s), aire 2
  ajoutée par l'événement 20 (événement 1150), NPC 130 (1600), retour par la porte vers
  `X01M01` (356 s), NPC 127 (1170) à 369,8 s → **histoire 1.1.2**.
- Le pilote s'arrête à 377,8 s sur `X01M01` : ses **sondes dans les huit directions** depuis
  y 8,25 et y 11,25 plafonnent à y 11,25. Ce n'est pas seulement la sonde : une marche
  exhaustive depuis la pose d'histoire (`(2,77 ; 11,25 ; 18,19)`) **et** depuis le point de
  départ de la carte (`(0 ; 8,25 ; 30)`), avec des arêtes validées en 3D (`Hero::step`, montée
  ≤ 1,0, chute ≤ 10), plafonne aussi à y 11,25. Le mur fin `X01A01S1` (bloc 0, placement 12,
  normale `+x`, `x 0,018..0,105`, `z 15,42..18,99`, `y 9..14`) sépare la terrasse du pied de la
  rampe menant au warp `0x72` **type 0** (n° 9) vers `X01M03`. Les zones `0x73` de **type 1**
  sont **inertes** (aucun lecteur dans `Field_CheckRegions`, voir `map-archive.md`) : ce n'est
  ni un monte-charge ni une tâche 8. **[inconnu]** : la connexion manquante (placement de
  collision déplacé, plateforme animée ou changement de carte scénarisé), à établir sur
  melonDS. `NavGrid` collecte désormais tous les sols d'une colonne
  (`CollisionWorld::floors`) et valide chaque arête en 3D (`Hero::step`, distance 3D au
  voisin) : une arête qui ne fait que glisser sous une terrasse est rejetée.
- Limite de la trace : le pilote donne les directions avec la caméra du jeu à l'angle 0 et le
  rejeu force ce même cap ; elle prouve le parcours headless dans notre moteur, pas un rejeu à
  la manette melonDS avec caméra manuelle.
