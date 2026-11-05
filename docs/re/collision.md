# Collision des cartes (`.col2`)

Le lecteur `dq9_formats::map::Collision::parse` décode les octets **décompressés** du maillage,
de l'index de grille et des records de sol des fichiers `.col2` européens. Dans les archives,
les membres `.col2` portent un en-tête de compression Nintendo ; l'appelant les décompresse
avec `nds_rom::compress::decompress` avant de les parser. Le parseur n'effectue aucune E/S.
À ce stade, le rendu et le déplacement ne consomment pas encore ces données.

## Structure observée

Le format n'a pas de signature textuelle. Le premier mot vaut 3 sur les 1 178 fichiers de la
ROM. Les offsets du tableau d'en-tête indiquent les sections ; le lecteur contrôle leurs
bornes et leurs cohérences avant de construire le résultat.

| Décalage | Donnée | Décodage |
|---|---|---|
| `0x00` | version | `u32`, valeur 3 |
| `0x04` | `shift` | `u32`, de 0 à 5 ; échelle probable `2^shift` **[inféré]** |
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

Restent inconnus ou inférés : origine et correspondance spatiale des cellules, attributs de
triangles (sauf leur index de record dans les sept bits de poids fort, lui-même **[inféré]**),
sens exact des deux derniers demi-mots des records et facteur `2^shift` (cohérent avec les
mesures, mais **[inféré]**). Certains petits maillages sont vraisemblablement des volumes
verticaux, pas des surfaces praticables. Le lecteur expose ces données sans leur attribuer de
physique.

## Prochaine étape J2

Assembler les maillages par instance, appliquer leurs transformations et l'échelle confirmée,
puis comparer la hauteur du sol et une trajectoire déterministe avec melonDS. Le déplacement,
les collisions murales, la caméra de terrain et les changements de carte restent à implémenter.
