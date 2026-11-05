# Fichiers 3D NITRO (`BMD0`, `BTX0` et animations)

Les modèles et textures des cartes sont des fichiers G3D du NITRO-System de Nintendo, rangés
compressés (LZ10) dans les NARC `data/map/<code>.ambl` (textures `.nsbtx`) et
`data/map/<code>.amdj` (modèles `.nsbmd`, animations `.nsbca`/`.nsbta`/`.nsbtp`/`.nsbma`).
Les archives de personnages `data/**/*.chr` (NARC) rangent les mêmes fichiers **non
compressés** (modèle, mouvements `.nsbca`, `.nsbma`, `.nsbta`, `.nsbtp`).
Implémentation : `nds_rom::g3d` (sans E/S, wasm) ; `cargo dq9re model <fichier ou NARC>
[--member NOM] [--anim MEMBRE --frame N] --out DIR` écrit chaque texture en PNG et chaque
modèle en `.obj` + `.mtl`, posé et animé par `--anim` à l'image `N`.

## Sources

- **[apicula](https://github.com/scurest/apicula)** (0BSD) : porté module par module (voir
  l'en-tête de `crates/nds-rom/src/g3d/mod.rs`) : conteneur, dictionnaires, `MDL0`, `TEX0`,
  `PAT0`, commandes de rendu, matrices « pivot » et « basis », décodage des textures,
  commandes GPU, assemblage des primitives, disposition des pistes `JNT0`.
- **Décompilation USA** (Unlicense), `src/NitroSDK/NSBXX/` : la bibliothèque G3D telle que le
  jeu la lie (`JAC.cpp`, `MAT.cpp`, `MAM.cpp`, `PatternAnimation.cpp`,
  `RenderCommandProcs.cpp`, `RenderCommands.cpp`) : évaluation des courbes, règles d'échelle,
  matrices de texture. Adresses européennes dans `re/symbols.txt` (section « NITRO G3D
  animation library ») et en commentaire du code.
- **Ghidra sur l'ARM9 européen** : `VEC_Normalize` (`0x020c2f28` : diviseur matériel
  `2^56 / |v|²`, racine `sqrt(4 |v|²)`) et `FX_DivAsync`/`FX_GetDivResult` (`0x020c2d00`,
  `0x020c2c6c` : `(a << 32) / b`, arrondi à `2^-12`) ; tables d'indices des matrices pivot
  `0x020e9270` (modèles) et `0x020e9294` (animations), identiques entre elles et à apicula.
- **GBATEK** (« DS 3D Geometry Commands », « DS 3D Texture Formats », « DS 3D Texture
  Coordinates ») : commandes GPU, formats de texture, matrice de texture en mode « source
  coordonnées de texture », diviseur et racine matériels.
- Noms des champs d'en-tête (`posScale`, `boxPosScale`, `sbcType`…) : structures
  `NNSG3dResMdlInfo`, `NNSG3dResMatData` du SDK NITRO, tels que documentés par apicula et
  sa documentation [nsbmd_docs](https://github.com/scurest/nsbmd_docs).

Tout ceci est du format et du code de bibliothèque Nintendo : cité par adresse, jamais
compté comme fonction du jeu portée (pas de marqueur `port:`).

## Ce qui est lu

| Élément | Type | Contenu |
|---|---|---|
| Conteneur | `G3dFile` | en-tête NITRO (`BMD0`, `BTX0`, `BCA0`, `BTA0`, `BTP0`, `BMA0`), sections ; noms des animations (`AnimSection`) |
| Dictionnaire | `read_dict` | arbre Patricia ignoré, entrées de taille fixe + noms de 16 octets (`Name`) |
| `MDL0` | `Model` | en-tête complet (`up_scale`/`down_scale`, règle d'échelle, mode de matrice de texture, comptes, boîte englobante et `box_pos_scale`), matériaux, maillages, objets, matrices de liaison inverses, commandes de rendu, hiérarchie (`object_links`) |
| Matériau | `Material` | `DIF_AMB`, `SPE_EMI`, `POLYGON_ATTR` + masque, `TEXIMAGE_PARAM` + masque, base de palette, taille de texture, `magW`/`magH`, matrice de texture (`tex_srt()`, `tex_matrix()`), noms de texture et de palette |
| Objet | `Object` | translation, rotation 3×3 ou « pivot » compressée, échelle et son inverse ; `matrix()` = T·R·S |
| Pose | `ObjectTransform`, `Model::rest_pose`, `Model::pose_matrices` | transformation exacte par objet puis matrice selon la règle d'échelle du modèle |
| Commandes de rendu | `RenderOp` | charger/stocker une matrice, multiplier par un objet, peau (`Skin`), `ScaleUp`/`ScaleDown`, lier un matériau, dessiner un maillage |
| Liste d'affichage | `DisplayList` → `GpuCmd` | toutes les commandes GPU de GBATEK ; positions en virgule fixe exacte (`Fx12`), normales 1.0.9, coordonnées de texture 1.11.4, couleurs RGB555 |
| Géométrie | `Geometry` | `build` (pose de repos), `build_posed` (matrices d'objets données) : triangles indexés en espace modèle, normales, couleurs, UV normalisées brutes ; `apply_tex_matrices` |
| `TEX0` | `Tex0` | textures (`TextureParams`, données, index des blocs 4×4) et palettes |
| Décodage | `decode_texture` | RGBA8 pour les 7 formats : A3I5, 4, 16, 256 couleurs, 4×4 compressé, A5I3, direct |
| `JNT0` | `JointAnimation` | pistes par objet, `track_transform`, `transforms`, `pose`/`pose_at` |
| `SRT0` | `TexSrtAnimation` | pistes par matériau (échelle, rotation, translation), `srt`, `matrix` |
| `MAT0` | `MaterialAnimation` | pistes par matériau (diffuse, ambiante, spéculaire, émission, alpha), `colors` |
| `PAT0` | `Pattern` | noms de textures/palettes et images clés par matériau, `keyframe`, `at` |
| Matrice de texture | `TexMatrix` | les 6 termes utiles de la matrice GPU, `apply` (texels), `normalized` (UV normalisées) |

Différences avec apicula :

- toutes les valeurs restent en virgule fixe exacte, `cgmath` est remplacé par `Fx12` et
  `Mat4` (f32, convention vecteurs colonnes d'apicula) ; les animations suivent l'arithmétique
  32/64 bits de la bibliothèque (interpolations, normalisation, produits vectoriels) ;
- une section illisible fait échouer le fichier (apicula l'ignore) ; aucune lecture n'est
  hors bornes : les échantillons sont lus à l'évaluation, une lecture hors du fichier rend une
  erreur ;
- `Material::params()` rend le `TEXIMAGE_PARAM` du matériau (apicula le masque par le masque
  de `POLYGON_ATTR`, ce qui semble une erreur) ;
- les commandes GPU de matrice (`MTX_PUSH/POP/STORE/IDENTITY/LOAD/MULT/TRANS`) sont aussi
  exécutées, et la palette d'une couleur hors palette lit 0 comme chez apicula ;
- l'échantillonnage des courbes d'apicula (« I made this up ») est remplacé par celui de la
  bibliothèque ; `SRT0` (incomplet chez apicula) et `MAT0` (absent) sont complets.

Non porté : mélange de plusieurs animations d'articulations sur un même objet
(`ProcessJointAnimationsOnBoneMatrix`, `0x020b3bbc`), animations de visibilité (`V\0AV`,
absentes des fichiers lus ici), panneaux (billboards, commandes de rendu `0x07`/`0x08`),
placage d'environnement (`TEXIMAGE_PARAM` génération 2 et 3), éclairage, export
COLLADA/glTF, choix des animations par le jeu.

## `up_scale`

`posScale` (`up_scale`) n'est appliqué que par la commande de rendu `0x0b` (`ScaleUp`), que
le convertisseur émet seulement quand l'échelle diffère de 1 : les modèles à `up_scale`
= 1 n'en ont pas (M08M08F4, S14M01E1). Les sommets de M01M0003 sont stockés à 1/8.

## Règles d'échelle (`scaling_rule`)

La commande de rendu `0x06` porte `(objet, parent, drapeaux)` ; la bibliothèque calcule la
transformation de l'objet (`RenderCommand_6`, `0x020b4c9c` : animation ou pose de repos) puis
l'envoie selon la règle du modèle (`BoneMatrixScaleCalculationProc_Type0/1/2` et
`BoneMatrixDataSubmissionProc_Type0/1/2`, `0x020b9a3c`..`0x020ba3a0`) :

- 0 (standard) : `T·R·S` ;
- 1 (Maya, compensation d'échelle de segment) : un objet au drapeau 1 annule l'échelle de
  son parent (`T·S(parent⁻¹)·R·S`), le drapeau 2 mémorise l'inverse de sa propre échelle ;
- 2 (Softimage, « classic scale off ») : l'échelle cumulée des ancêtres est retirée puis
  réappliquée dans le repère local, `S(cumul⁻¹)·T(t∘cumul)·R·S(cumul)·S`, en suivant la
  hiérarchie dans l'ordre des commandes ; l'objet 0 démarre avec une échelle cumulée
  identité (`SetUpRenderCommandHandler`, `0x020b4078`).

Le fichier stocke chaque échelle avec son inverse (6 `fx32`). Dans la ROM : 6 113 modèles en
règle 2, 126 en règle 0, aucun en règle 1. La règle 2 explique les deux modèles qui
débordaient de leur boîte d'en-tête en `T·R·S` : M10M0600 et S14M01E1 imbriquent des objets
mis à l'échelle ; avec la règle, tous les modèles tiennent dans leur boîte.

## Animations d'articulations (`JNT0`, bloc `J\0AC`)

En-tête : nombre d'images (`u16`), de pistes (`u16`), drapeaux (`u32` : bit 0 interpoler
entre images, bit 1 la dernière image revient vers la première ; vus : 0, 1, 3), décalages
des matrices pivot et « basis », puis un décalage `u16` par piste. La piste `i` anime
l'objet `i` ; l'octet haut de son mot de drapeaux désigne la piste réellement lue (toujours
`i` dans la ROM). Drapeaux : bit 0 aucune composante (identité) ; translation bits 1-2
(1 : absente, 2 : pose de repos, aucun : animée) et 3-5 constante par axe ; rotation bits 6-7
(absente, repos) et 8 constante ; échelle bits 9-10 et 11-13 constante par axe.

Une valeur animée est une constante (translation `fx32`, échelle `fx32` + inverse, rotation
un indice) ou une courbe `(info, décalage)` : `info` bits 16-28 dernière image échantillonnée
au pas réduit, 29 échantillons 16 bits, 30-31 `log2` du pas (1, 2 ou 4 images). Avec un pas
de 2 ou 4, les images intermédiaires sont la moyenne de deux échantillons ou un mélange 3:1 ;
les images au-delà de la dernière image réduite sont stockées une par une. Un indice de
rotation désigne une matrice pivot (bit 15) ou « basis » (5 `u16` : deux lignes de 13 bits,
la troisième par produit vectoriel). Les rotations mélangées sont renormalisées par
`VEC_Normalize`. Un temps fractionnaire (1.19.12 images) n'est interpolé que si le bit 0 est
mis, sinon il est arrondi à l'image inférieure ; le temps est borné à `[0, images)`
(`JACAnimationProcessingCallback`, `0x020b78cc`).

## Matrices de texture et `SRT0` (bloc `M\0AT`)

Un matériau à `flags` bit 0 porte une échelle, une rotation (`sin`, `cos`) et une translation
(en tailles de texture) ; dans la ROM, ces matériaux (2 297) et seulement eux ont la génération
de coordonnées 1 (« source coordonnées de texture »). La bibliothèque en fait la matrice GPU
selon `tex_mtx_mode` (`MaterialTextureMatrixLoadProc_Type0..3`) ; chaque variante spécialisée
(sans rotation, sans échelle…) est la formule générale avec l'identité, d'où une formule par
mode dans `TexMatrix::new`. Les 6 239 modèles de la ROM utilisent le mode 3 (Softimage XSI :
rotation puis translation puis échelle, centrées sur le coin bas-gauche). `magW`/`magH`
multiplient ensuite la première et la seconde ligne. Coordonnées en texels :
`s' = a s + c t + e/16`, `t' = b s + d t + f/16`.

`SRT0` : nombre d'images, mot d'en-tête (`0x300`, `0x301`, `0x303` ; bits 0-1 comme `JNT0`
**[hypothèse]**, `0x300` **[inconnu]**), puis un dictionnaire de pistes nommées par matériau,
cinq courbes `(info, valeur)` : échelle s, échelle t, rotation, translation s, t. `info` :
bits 0-15 dernière image au pas réduit, 28 échantillons 16 bits, 29 constante (dans
`valeur`), 30-31 pas. L'animation remplace la transformation du matériau et force la
génération 1 (`MATAnimationProcessingCallback`, `0x020b9668`).

## Couleurs (`MAT0`, bloc `M\0AM`)

Nombre d'images, mot d'en-tête (0, 1, 3), pistes par matériau : cinq mots (diffuse,
ambiante, spéculaire, émission, alpha) : bits 0-15 décalage des échantillons ou valeur
constante, 16-28 dernière image au pas réduit, 29 constante, 30-31 pas. Couleurs RGB555 en
`u16`, mélangées canal par canal ; alpha en `u8` (0..31). Le résultat remplace `DIF_AMB`,
`SPE_EMI` (en gardant le bit 15) et l'alpha de `POLYGON_ATTR` (`MaterialColors::apply`).

## Motifs (`PAT0`)

Par piste : nombre d'images clés (`u16`), un `u16` inutilisé, un rapport (images clés par
image, 1.3.12) et le décalage des clés. La recherche part de `rapport × image` puis recule et
avance jusqu'à la dernière clé commençant au plus tard à l'image (`0x020b77a8`) ; nous
bornons ce premier essai au nombre de clés, ce que la bibliothèque ne fait pas. Un indice de
palette `0xFF` garde la palette courante.

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
  sommets en pose de repos de tous les modèles (règle d'échelle appliquée).

`animation_files` (toutes les animations des NARC de cartes et des 2 683 `.chr`) :

- cartes : 299 `.nsbca`, 873 `.nsbta`, 512 `.nsbma`, 525 `.nsbtp` (compressés) ; `.chr` :
  5 347 `.nsbca`, 692 `.nsbta`, 1 594 `.nsbma`, 45 `.nsbtp` (non compressés) ; une animation
  par fichier : 5 646 `JNT0`, 1 565 `SRT0`, 2 106 `MAT0`, 570 `PAT0`, 98 684 pistes ;
- chaque image de chaque piste s'évalue sans erreur, plus la demi-image des animations
  interpolées : 6 732 887 évaluations ;
- la piste `i` lit toujours ses propres données ;
- chaque animation d'articulations pose les modèles de sa propre archive qui ont assez
  d'objets : 2 965 poses (règle 2 avec matrices de texture en mode 3 : 2 955 ; règle 0 : 10),
  toutes construites.

Essais de bout en bout :

- `cargo dq9re model data/map/M01.amdj --member M01M0003.nsbmd` (village d'Angel Falls)
  donne 17 objets, 39 matériaux, 50 maillages, 1 304 sommets, 692 triangles, boîte
  x −44,1..0,76, y −2,5..15, z −22,6..36 (unités du monde) ; vue de dessus, l'OBJ montre les
  chemins, l'herbe et les toits bleus. Les PNG de `M01M00T1.nsbtx` (`.ambl`) sont des
  textures cohérentes (tuiles de toit, herbe, porte).
- `--member M01M0001.nsbmd --anim M01M0001.nsbca --frame N` (la cascade : objets `fall`,
  `nami_*`, `shibuki*`, `spr_*`, 181 images) : l'image 0 rend la pose de repos (boîte
  x −10,46..18,51, z −21,70..−16,05) ; aux images 30 et 60, les gerbes `spr_*` glissent de
  plusieurs unités (jusqu'à 5,9 entre les images 0 et 60) et la boîte s'étend à
  z ..−15,84 puis ..−14,98.
- `data/chara_sub/s000.chr --anim walk.nsbca` (personnage à peau, 15 objets, marche de
  9 images bouclée) : pose de repos bras écartés (x ±9,39) ; images 0 et 4 en foulées
  symétriques (x −7,61..7,66 puis −7,66..7,61), tous les sommets bougent (jusqu'à
  5,9 unités) ; l'image 8 revient sur l'image 0.
