# Collision des cartes (`.col2`)

Le lecteur `dq9_formats::map::Collision::parse` décode les octets **décompressés** du maillage,
de l'index de grille et des records de sol des fichiers `.col2` européens. Dans les archives,
les membres `.col2` portent un en-tête de compression Nintendo ; l'appelant les décompresse
avec `nds_rom::compress::decompress` avant de les parser. Le parseur n'effectue aucune E/S.
Le parseur est consommé par `dq9-core` pour les requêtes de sol et le mouvement
([`movement.md`](movement.md)). La construction reste sans E/S ; les archives et les membres
sont fournis par l'appelant.

## Structure observée

Le format n'a pas de signature textuelle. Le premier mot vaut 3 sur les 1 178 fichiers de la
ROM. Les offsets du tableau d'en-tête indiquent les sections ; le lecteur contrôle leurs
bornes et leurs cohérences avant de construire le résultat.

| Décalage | Donnée | Décodage |
|---|---|---|
| `0x00` | version | `u32`, valeur 3 |
| `0x04` | `shift` | `u32`, de 0 à 5 ; sommets et bornes valent `entier << shift` en `fx32` |
| `0x08` | limites | six `i16`, min x/y/z puis max x/y/z |
| `0x14` | nombre de triangles | `u32` |
| `0x18` | dimensions de cellule | `u16` taille, `u16` inconnu |
| `0x1c`, `0x20` | dimensions de la grille | `u32` X et Z |
| `0x24`…`0x30` | offsets | triangles, comptes, débuts et indices par cellule |
| `0x34`, `0x38` | records de sol | nombre `u32`, offset `u32` |

Chaque triangle fait 28 octets : trois sommets `i16[3]`, une normale `fx16[3]` signée en
1.3.12, puis un mot d'attributs `u32`. Les coordonnées sont des entiers dans les unités des
limites, pas des valeurs fx32. Les records terminaux font 8 octets ; seuls leurs deux premiers
`u16` ont une interprétation établie. Sur les cartes ordinaires, le premier décode des chiffres
de terrain pour l'identifiant du stage de combat ; sur la carte céleste, les bits 5–9 du
second servent à l'autorisation d'atterrissage.

La grille a `gridX * gridZ + floor(gridZ / 2)` cellules. Le fichier stocke pour chacune un
compte `u8` et un début `u16` dans une liste d'indices `u16`. Le lecteur exige des plages
consécutives, des indices de triangles valides et au plus un `u16` nul d'alignement final. Il
préserve les plages sous forme d'indices contigus (`cells` et `cell_indices`), sans allocation
par cellule.

## Preuves et limites

Les faits du format proviennent de la page [Map-Collision du wiki DQIX](https://github.com/DQIX/wiki/blob/main/Map-Collision.md),
réobservée sur la ROM européenne `YDQP01` : elle rapporte des contrôles exhaustifs sur 1 178
fichiers, 109 122 triangles, normales conformes au produit vectoriel pour chaque triangle non
dégénéré et grilles couvrant les limites. Le code de collision du jeu ne valide pas encore le
lecteur ici.

Le test `collision_meshes_parse` ouvre chaque archive `.amdj` de la ROM, décompresse son
membre `.col2`, puis vérifie par `Collision::parse` les 1 178 maillages (109 122 triangles).
Les tests synthétiques vérifient décodage, sections tronquées et incohérence des plages. La
ROM n'est pas une fixture et n'est pas nécessaire à la CI.

Reste inconnu : le sens exact des deux derniers demi-mots des records ; le facteur `2^shift` est
confirmé par le code (`0x0204bf54` : bornes `<< shift` ; `ov_017::02192b48` : sommets × `1 << shift`).
Certains petits maillages sont vraisemblablement des volumes
verticaux, pas des surfaces praticables. Le lecteur expose ces données sans leur attribuer de
physique.

## Utilisation par J2

`dq9_core::CollisionWorld` garde les maillages `Collision` tels que lus et leurs placements
(`Piece` : maillage, position et angle `fx32` du placement) ; le jeu travaille dans le repère de
chaque pièce, sommets = entiers du fichier `<< shift`. Le déplacement, le rayon du sol et la
poussée des murs sont décrits dans [`movement.md`](movement.md).

## Grille, mot d'attributs et requête (J2, `0x0204c040`)

Le code de requête établit (relu dans `dq9_core::collision::query` et vérifié par des tests
synthétiques) :

- **Objet `.col2` en mémoire** (`0x0204bf54`, 0x4c octets) : `+0` en-tête, `+4` triangles,
  `+8` comptes de cellules (`u8`), `+0xc` débuts (`u16`), `+0x10` indices (`u16`), `+0x14`
  records (version 3), `+0x18` bornes maximales `<< shift`, `+0x24` minimales `<< shift`,
  `+0x48` nombre de cellules `gridX × gridZ + gridZ / 2`.
- **Test de bornes** : la boîte de la requête (en coordonnées de la pièce) doit recouper les
  bornes du maillage (bords inclus), puis elle est décalée de `shift` (décalage arithmétique) ;
  un triangle est retenu si sa boîte la recoupe.
- **Boîte d'un triangle** : le mot d'attributs (`+0x18`) est formé des octets `0x18..0x1a` :
  six quartets d'indices (dans les 9 coordonnées du triangle) du min x/y/z puis du max
  x/y/z ; l'octet `0x1b` est `(indice de record << 1) | marque temporaire`. Cela explique les
  valeurs « permutations de `0x543210` » du wiki.
- **Grille décalée** : au plus 7 cellules, toutes sont balayées ; sinon rangée =
  `⌊FX_Div(zc − minZ, taille) / 4096⌋` (tronqué vers 0), colonne = idem sur `x` (avec
  `+ taille / 2` sur les rangées impaires), `zc` et `xc` étant les centres de la boîte ; la
  cellule centrale a l'indice `rangée × gridX + colonne + rangée / 2` et ses voisines (six
  autour d'une rangée paire, sept selon la position sur une rangée impaire, qui compte
  `gridX + 1` cellules) sont aussi lues. Une valeur légèrement hors grille (moins d'une cellule
  en deçà) tombe, par la troncature, dans la première colonne ou rangée.
- 0x60 triangles au plus par pièce et par requête, 0xc0 candidats examinés.

Le lecteur `Collision::parse` expose déjà ces mots (`Triangle::attributes`).
