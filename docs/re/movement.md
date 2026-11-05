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
Une sauvegarde sélectionne la carte par son id `11476` (par ex. 109 = `C01M09`), mais son
vecteur sauvegardé n'est pas la position du héros : le départ reste le point provisoire calculé
depuis les collisions, non la position de reprise.

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
