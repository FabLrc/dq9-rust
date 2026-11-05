# Fichiers 3D NITRO (`BMD0`, `BTX0` et animations)

Les modèles et textures des cartes sont des fichiers G3D du NITRO-System de Nintendo, rangés
compressés (LZ10) dans les NARC `data/map/<code>.ambl` (textures `.nsbtx`) et
`data/map/<code>.amdj` (modèles `.nsbmd`, animations `.nsbca`/`.nsbta`/`.nsbtp`/`.nsbma`).
Implémentation : `nds_rom::g3d` (sans E/S, wasm) ; `cargo dq9re model <fichier ou NARC>
[--member NOM] --out DIR` écrit chaque texture en PNG et chaque modèle en `.obj` + `.mtl`.

## Sources

- **[apicula](https://github.com/scurest/apicula)** (0BSD) : porté module par module (voir
  l'en-tête de `crates/nds-rom/src/g3d/mod.rs`) : conteneur, dictionnaires, `MDL0`, `TEX0`,
  `PAT0`, commandes de rendu, matrices « pivot », décodage des textures, commandes GPU,
  assemblage des primitives.
- **GBATEK** (« DS 3D Geometry Commands », « DS 3D Texture Formats ») : nombre de paramètres
  de toutes les commandes GPU, formats de texture.
- Noms des champs d'en-tête (`posScale`, `boxPosScale`, `sbcType`…) : structures
  `NNSG3dResMdlInfo`, `NNSG3dResMatData` du SDK NITRO, tels que documentés par apicula et
  sa documentation [nsbmd_docs](https://github.com/scurest/nsbmd_docs).

Rien ici ne vient du code du jeu : ce sont des formats et du matériel Nintendo, que le jeu
charge par la bibliothèque NNS (non marquée `port:`).

## Ce qui est lu

| Élément | Type | Contenu |
|---|---|---|
| Conteneur | `G3dFile` | en-tête NITRO (`BMD0`, `BTX0`, `BCA0`, `BTA0`, `BTP0`, `BMA0`), sections |
| Dictionnaire | `read_dict` | arbre Patricia ignoré, entrées de taille fixe + noms de 16 octets (`Name`) |
| `MDL0` | `Model` | en-tête complet (`up_scale`/`down_scale`, comptes, boîte englobante et `box_pos_scale`), matériaux, maillages, objets, matrices de liaison inverses, commandes de rendu |
| Matériau | `Material` | `DIF_AMB`, `SPE_EMI`, `POLYGON_ATTR` + masque, `TEXIMAGE_PARAM` + masque, base de palette, taille de texture, matrice de texture (échelle, rotation, translation), noms de texture et de palette (dictionnaires d'appariement) |
| Objet | `Object` | translation, rotation 3×3 ou « pivot » compressée, échelle ; `matrix()` = T·R·S |
| Commandes de rendu | `RenderOp` | charger/stocker une matrice, multiplier par un objet, peau (`Skin`), `ScaleUp`/`ScaleDown`, lier un matériau, dessiner un maillage |
| Liste d'affichage | `DisplayList` → `GpuCmd` | toutes les commandes GPU de GBATEK ; positions en virgule fixe exacte (`Fx12`), normales 1.0.9, coordonnées de texture 1.11.4, couleurs RGB555 |
| Géométrie | `Geometry` | exécute les commandes de rendu et les listes d'affichage : triangles indexés en espace modèle (`up_scale` appliqué), normales, couleurs, UV normalisées |
| `TEX0` | `Tex0` | textures (`TextureParams`, données, index des blocs 4×4) et palettes |
| Décodage | `decode_texture` | RGBA8 pour les 7 formats : A3I5, 4, 16, 256 couleurs, 4×4 compressé, A5I3, direct |
| `PAT0` | `Pattern` | noms de textures/palettes et images clés par matériau |
| `JNT0`, `SRT0`, `MAT0` | `AnimSection` | seulement les noms des animations |

Différences avec apicula :

- toutes les valeurs restent en virgule fixe exacte, `cgmath` est remplacé par `Fx12` et
  `Mat4` (f32, convention vecteurs colonnes d'apicula) ;
- une section illisible fait échouer le fichier (apicula l'ignore) ; aucune lecture n'est
  hors bornes ;
- `Material::params()` rend le `TEXIMAGE_PARAM` du matériau (apicula le masque par le masque
  de `POLYGON_ATTR`, ce qui semble une erreur) ;
- les commandes GPU de matrice (`MTX_PUSH/POP/STORE/IDENTITY/LOAD/MULT/TRANS`) sont aussi
  exécutées, et la palette d'une couleur hors palette lit 0 comme chez apicula.

Non porté : lecture des animations `JNT0`/`SRT0`/`MAT0` (courbes, matrices « basis »),
matrice de texture appliquée aux UV, panneaux (billboards, commandes de rendu `0x07`/`0x08`),
éclairage, export COLLADA/glTF, visualiseur.

## `up_scale`

`posScale` (`up_scale`) n'est appliqué que par la commande de rendu `0x0b` (`ScaleUp`), que
le convertisseur émet seulement quand l'échelle diffère de 1 : les modèles à `up_scale`
= 1 n'en ont pas (M08M08F4, S14M01E1). Les sommets de M01M0003 sont stockés à 1/8.

## Contrôles sur la ROM

`crates/nds-rom/tests/real_rom.rs`, `map_g3d_files` (tous les `.nsbmd`, `.nsbtx`, `.nsbtp`
des NARC `data/map/*.ambl`, `*.amdj`, décompressés) :

- 4 328 `BMD0` (un modèle chacun, sans `TEX0` intégré dans aucun), 737 `BTX0`, 525 `BTP0`
  (525 animations de motif) : tout se lit ;
- 36 632 maillages, 31 297 matériaux ; toutes les listes d'affichage s'exécutent :
  1 365 561 sommets, 748 435 triangles ;
- 22 827 textures, 22 822 palettes, toutes décodées (69 462 528 texels) : 15 532 en 16
  couleurs, 3 498 A3I5, 3 445 A5I3, 331 en 256 couleurs, 16 en 4 couleurs, 5 directes,
  aucune 4×4 compressée ;
- palette d'une texture : `<nom>_pl`, sinon celle qu'un matériau ou une animation de
  motif lui associe ; seule `c04m01gds28` (C04M05) n'en a aucune **[hypothèse]** texture
  inutilisée ;
- la boîte d'en-tête (× `box_pos_scale`, à 2 unités de virgule fixe près) contient les
  sommets en pose de repos, sauf pour M10M0600 et S14M01E1 (copié dans 5 cartes), qui la
  dépassent de plus d'une unité **[inconnu]**.

Essai de bout en bout : `cargo dq9re model data/map/M01.amdj --member M01M0003.nsbmd` (village
d'Angel Falls) donne 17 objets, 39 matériaux, 50 maillages, 1 304 sommets, 692 triangles,
boîte x −44,1..0,76, y −2,5..15, z −22,6..36 (unités du monde) ; vue de dessus, l'OBJ montre
les chemins, l'herbe et les toits bleus. Les PNG de `M01M00T1.nsbtx` (`.ambl`) sont des
textures cohérentes (tuiles de toit, herbe, porte).
