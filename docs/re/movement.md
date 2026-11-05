# Déplacement et sol du héros

`dq9-core` assemble les `.col2` des placements visibles de la carte, les transforme en faces du
monde, puis avance un héros sur un pas fixe de 1/60 s. Le module n'effectue aucune E/S ; la
résolution ROM et NARC est fournie par l'appelant.

## Références observées

- `ov_017::0218d8cc` construit un vecteur horizontal à partir des touches DS (`UP/DOWN` `0x40/0x80`,
  `RIGHT/LEFT` `0x20/0x10`) et de la direction de la caméra, additionne les axes, puis normalise.
  La fonction appelée `func_020968a4` peut transformer le vecteur quand l'acteur porte le drapeau
  `+0x224` ; son effet n'est pas réimplémenté.
- `0x02018fcc` (appel à `func_02018fbc`) cherche le sol dans un intervalle vertical de +1 à −10
  unités autour de la position. Le résultat déplace le Y de `0x199` en fx32, soit environ 0,1 unité.
  L'origine de la grille `.col2` n'étant pas établie, le noyau teste les triangles directement.
- Les sommets `.col2` sont des entiers multipliés par `2^shift`, puis convertis de fx32 (division
  par 4096). `shift` est inféré à partir de mesures et reste documenté comme tel dans
  [`collision.md`](collision.md).

## Modèle courant

`CollisionWorld::build` décompresse chaque membre collision une fois par partie, puis transforme
les faces à chaque placement visible. `ground_height` choisit la face ascendante la plus haute qui
contient exactement le point XZ et respecte la fenêtre verticale du jeu. `Hero::step` normalise
l'entrée, plafonne la diagonale, teste les faces verticales pour bloquer le déplacement et tente un
mouvement par axe en cas de blocage, puis recale la position au sol. L'avatar temporaire rendu par
l'application est un repère orange, pas le modèle animé du protagoniste.

Les portes, aires et warps des `.bmbl` sont testés par `dq9_core::field` (`map-archive.md`,
« Portes, zones et changements de carte ») : présence du meneur à chaque pas, avec hauteur et
rotation lues sur la ROM, comparées à melonDS. Le module ne modifie pas ce déplacement : il
reçoit la pose du héros après `Hero::step`.

La vitesse de 4 unités/s (doublée avec Maj) est provisoire : les routines qui choisissent la vitesse,
l'accélération, le rayon, la rotation du modèle et les transitions d'animation ne sont pas
modélisées. Le test des murs utilise le centre du joueur et un point à 0,75 unité de hauteur, sans
rayon de capsule. Les escaliers spéciaux, chutes et triggers de scénario restent absents.
**Position de départ.** Une sauvegarde normale ne stocke pas la position du héros : elle donne
la carte (id `11476`, par ex. 109 = `C01M09`) et le jeu pose le groupe au point de départ de la
carte, l'enregistrement `0x6e` de son `.bmbl` (`Party_Place`, `ov_017::0219c598` ;
[`save.md`](save.md) § « Position à la reprise »). `dq9_core::state::GameState` en déduit la
pose (`hero_pose`) et `dq9-app --save` y place le héros, recalé sur le sol trouvé par les
collisions (`0x199` au-dessus, comme le jeu) ; le cap initial est celui du jeu, sa convention
par rapport au lacet de `Hero` est **[hypothèse]** (la même que pour les portes). Une carte
sans enregistrement, une pose d'une autre carte ou l'absence de sauvegarde gardent le point
provisoire calculé depuis les collisions. Position et cap sont égaux à ceux de melonDS sur la
démo (109) et sur la même sauvegarde repointée sur la carte 100 ; seule la hauteur diffère
avant recalage.

## Comparaison melonDS

Les états existants `build/melonds/before.ml1` et `moved.ml1`, produits dans `C01M09`, exposent
le compteur VBlank et la position du héros : (0,210 ; 0,120 ; −1,380), puis (−2,853 ; 0,106 ;
3,854). Le journal antérieur associe au second état environ une seconde vers le bas puis 0,6 s vers
la gauche. Les compteurs observés sont 2128 et 4097, soit 1969 VBlank d'écart ; ces captures ne
sont donc pas des points de mesure alignés image par image et ne valident pas la vitesse. L'oracle
peut comparer les champs `position` de `dq9re state`, mais une comparaison différentielle
reproductible exige encore une capture depuis le même état avec un nombre précis d'images et des
entrées horodatées.

Le port ne marque aucune routine ARM9 complète : ce modèle est volontairement partiel et ne doit
pas gonfler `cargo dq9re coverage`.

## Analyse du code de déplacement (J2, en cours)

Obtenue avec `scripts/ghidra-dump.ps1` (décompilation de toutes les fonctions d'`ov_017` et du
module ARM9 dans `build/decomp/`, ~2,5 min). **Rien de cette section n'est encore porté** :
`dq9-core` garde son modèle provisoire et aucune fonction n'est marquée `port:`. Adresses
européennes ; noms dans `re/symbols.txt`.

### Qui déplace le héros

- `0x0218d8cc` ne déplace pas le héros : elle fabrique `field+0x4438` (direction du pavé dans le
  monde, relative à l'axe caméra → héros), lu par les échelles et le dialogue.
- Le déplacement du chef de groupe passe par `ov_017::0218e674` (boucle sur les membres) →
  `ov_017::0218e210` : (1) `PartyMember_Update` `0x02037d98` ; (2) collision
  `ov_017::0219344c` avec la position précédente ; (3) réglages : glissement si le déplacement
  corrigé dépasse 0x27, retour à la position précédente si |Δ| < 5 sur chaque axe, la distance
  parcourue ne peut pas dépasser celle d'avant la collision.
- `0x020a654c` → `0x020a75fc` (touches + `Character_Update`) agit sur l'objet `GameState+0x32c`,
  **nul** dans l'état `field.ml1` : ce n'est pas le héros à pied (**[inconnu]** : véhicule ?).

### Ordre dans `PartyMember_Update` (`0x02037d98`)

`0x02052af8` (mise à jour générique) **d'abord**, puis `Hero_ReadDirectionInput` (`0x02038148`)
puis `0x020385a8` (échelle). L'entrée d'une image ne déplace donc le personnage qu'à l'image
suivante : à reproduire pour l'alignement image par image.

### Entrée → cap (`0x02038148`)

- Code de direction `+0x250` : Haut+Gauche `0xc`, Haut+Droite `0xd`, Bas+Gauche `0xe`,
  Bas+Droite `0xf`, Haut 8, Bas 9, Gauche 10, Droite 11 (touches `0x40`, `0x80`, `0x20`,
  `0x10`). Les diagonales ont priorité.
- Cap = `table[code-8] + yaw caméra`, réduit à [0, 2π) ; table `0x020e90b8` en radians fx32
  (4096 = 1 rad, π = `0x3244`, 2π = `0x6488`) : Haut π, Bas 0, Gauche `0x4b66` (3π/2), Droite
  `0x1922` (π/2), Haut+Gauche `0x3ed5`, Haut+Droite `0x25b3`, Bas+Gauche `0x57f7`, Bas+Droite
  `0x0c91`. Convention : direction = (sin θ, 0, cos θ) ; la caméra à yaw 0 regarde vers −z.
- Sans touche : état 0 (repos). Avec touche : état 1 (marche), `Character_SetFacing`
  (`+0xae` = cap).
- Aimantation sur une zone orientée (`0x0201e90c` dans la liste `zone+0x6c`, écart < `0xc91`,
  aussi vers l'angle +π) : **non portée**, liste de zones propre au travail sur les triggers.
- Glissement au stylet (mode 2) : hors périmètre des entrées explicites.

### `Character_Update` (`0x020332bc`)

Champs : `+0x44` position fx32 ×3, `+0x54` angle du modèle, `+0xac` vitesse de chute,
`+0xae` cap voulu, `+0xb0` vitesse de rotation, `+0xb2` vitesse courante, `+0xb4` vitesse
max, `+0xb6` accélération, `+0xbe` état. Valeurs d'initialisation (`0x02032e68`, lues dans la
ROM) : rotation **`0x324`** (≈ 0,196 rad / image), vitesse max **`0x189`** (393/4096 =
0,0959 unité/image = **5,76 unités/s**), accélération **`0x28`** par image.

1. `0x02033720` : l'angle du modèle tourne vers `+0xae` du pas `+0xb0 × ticks`
   (`fix32SignedAngleDistance`, arrêt exact sur la cible).
2. `0x0203349c` (état 1) : vitesse ← vers max par `+0xb6 × ticks` (arrêt exact) ; hors état 1,
   décélère de la même valeur ; vitesse effective = min(vitesse, max × (0x1000 − |écart angulaire| / π)) ;
   déplacement = `(sin a, 0, cos a) × vitesse × ticks` avec `a` = angle **du modèle** (`+0x54`,
   pas `+0xae`) sauf drapeau `0x100` ; `Vector3fixMultiplyScalar` = `(x·y + 0x800) >> 12`.
3. Gravité : y −= `+0xac` ; `+0xac` croît de `0x51` par image jusqu'à `0x1000` dans
   `0x0219344c`, remis à 0 au contact du sol.

`ticks` = `GameState+0x3c4` (valeur 1 à vitesse normale **[hypothèse]**).

### Collision (`ov_017::0219344c` et `0x0204c040`)

- Requête `.col2` (`0x0204c040`) : position − origine de la pièce, coordonnées `>> shift` ;
  grille **décalée** : rangée = `FX_Div(zc − minZ, taille) / 4096` (troncature), colonne =
  idem avec `+ taille/2` sur les rangées impaires, indice de cellule =
  `rangée × gridX + colonne + rangée / 2` ; cellule centrale et voisines (selon parité),
  toutes les cellules si le maillage en compte ≤ 7. Un triangle est retenu si sa boîte
  englobante recoupe la boîte de requête ; 0x60 triangles au plus.
- **Le mot d'attributs d'un triangle** (`+0x18`) : les octets `0x18..0x1a` sont six quartets
  d'indices (dans les 9 coordonnées du triangle) du min x/y/z et du max x/y/z — la boîte
  englobante précalculée ; l'octet `0x1b` est `(indice de record << 1) | marque temporaire`.
  Cela explique les valeurs « permutations de `0x543210` » du wiki.
- Sol (`0x02018fcc`) : rayon de y+1 à y−10, résultat `+0x199` ; sans ajout de l'origine y de
  la pièce (**[hypothèse]** : y d'origine toujours nul pour les pièces de sol).
- `0x02192b48` : rayon depuis y+0x1800, fenêtre de marche 0x800 (0x1800 en mode forcé),
  poussée contre les murs par `0x020315c8`.
- Arithmétique : `FX_Div` = `((a << 32) / b + 0x80000) >> 20` (diviseur matériel, troncature),
  `VEC_Normalize`/`VEC_Mag` par le diviseur et la racine matériels, `FX_Atan2` par table de
  `0x020c3550`, sin/cos par table `0x020e9460`.

### Caméra de terrain

Objet caméra : `+4` œil, `+0x10` cible, `+0x70` yaw, `+0x74` dy, `+0x78` distance. Défaut
(`0x0202df78`) : yaw 0, dy `0x6000`, distance `0xb000`. `Camera_SetOrbit` (`0x0202e5e8`) :
œil = cible + RotY(yaw) · (0, dy, √(dist² − dy²)). `FieldCamera_Update` (`0x020a213c`) :
L `0x200` ajoute, R `0x100` retire au yaw (pas = deltaTime × constantes de `0x020a2710/14`,
**[inconnu]** valeurs non lues), neutralisé pendant une transition de luminosité ; la cible
est la position du personnage `+0x21c` + hauteur/2 + décalage `+0x22c`.

### Reste à faire (J2 non terminé)

Port Rust de `fx` (mathématique matérielle), `Character_Update`, entrée, requête `.col2`,
sol, murs (`0x020315c8`, `0x02031b94`, `0x02032090`), caméra ; câblage `dq9-app` (L/R) ;
mesure de la vitesse sous melonDS (5,76 unités/s attendues) ; lecture des constantes
`0x020a2710..0x020a2724`, `0x02193274`, tables `0x020c3550` et `0x020e9460`.
