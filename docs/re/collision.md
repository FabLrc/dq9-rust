# Collision des cartes (`.col2`)

Le lecteur `dq9_formats::map::Collision::parse` décode les octets **décompressés** du maillage,
de l'index de grille et des records de sol des fichiers `.col2` européens. Dans les archives,
les membres `.col2` portent un en-tête de compression Nintendo ; l'appelant les décompresse
avec `nds_rom::compress::decompress` avant de les parser. Le parseur n'effectue aucune E/S.
Le parseur est maintenant consommé par `dq9-core` pour les requêtes de sol et le mouvement
([`movement.md`](movement.md)). La construction reste sans E/S ; les archives et les membres
sont fournis par l'appelant.

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

## Utilisation par J2

`dq9-core::CollisionWorld::build` assemble les collisions de chaque placement visible, applique
la transformation d'instance et l'échelle `2^shift` [inférée]. Les requêtes de sol parcourent
les faces ascendantes directement : l'origine et le placement de la grille ne sont pas établis.
Le déplacement expérimental et ses limites sont décrits dans [`movement.md`](movement.md).

## Correction : mot d'attributs et grille (J2)

Le code `0x0204c040` (voir [`movement.md`](movement.md)) établit l'origine de la grille (minimum
de la boîte de l'en-tête, rangées décalées d'une demi-cellule sur les rangées impaires,
indice `rangée × gridX + colonne + rangée / 2`) et le sens du mot d'attributs : six quartets de
boîte englobante puis `(record << 1) | marque`. Le lecteur `Collision::parse` n'expose pas encore
ces champs.
