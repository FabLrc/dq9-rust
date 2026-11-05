# Archives de carte (`.ambl`, `.amdj`) et assemblage d'une carte

Ce que le jeu lit pour afficher une carte et comment il en assemble les pièces, de
`Zone3D::SwitchZone` jusqu'aux instances placées. Implémentation : `dq9_formats::map`
(`MapFiles`, `ZoneInfo`, `BlockList`, `Manifest`, `Map::assemble`) ; `cargo dq9re map M01`
affiche le résultat, `cargo dq9re map --state <état.ml1>` le compare à la RAM. Le contenu des
`.nsbmd`/`.nsbtx`/`.col2` est hors sujet ici (`g3d.md`). Noms des fonctions :
`re/symbols.txt`, section « Map archives ».

Sources : lecture du code ARM9 européen dans Ghidra (adresses EU ci-dessous), décompilation
USA pour quelques fonctions de bibliothèque (`FileIO.cpp`, `ExtendedVectorMath.cpp`,
`AtmosphericEffect.cpp`, en-tête `Zone3D.h`), contrôles sur toute la ROM et sur un état
melonDS. Le wiki DQIX (Map-Archive, Map-Objects, Map-Textures) a servi d'hypothèses de
départ ; les écarts sont listés à la fin.

## Chaîne de chargement

`Zone3D::SwitchZone` (`0x0201384c`) remet `Zone3D` à zéro, note l'id de la carte (`+0`),
l'enregistrement de la liste (`+8`, `maplist.md`) et met en file `data/map/maplist9.bin`.
Chaque image, `Zone3D_StepLoad` (`0x02013e28`) fait avancer les étapes, chacune attendant
son fichier (`BackgroundLoader`) :

| étape | fonction | fichier | effet |
|---|---|---|---|
| 1 | `Zone3D_StepMapList` (`0x02013fc4`) | `maplist9.bin` | réglages de zone (ci-dessous), puis met en file l'`.ambl` |
| 2 | `Zone3D_StepAmbl` (`0x02014118`) | `<code>.ambl` | textures, liste de blocs, `.dat` ; puis éclairage |
| 3 | `Zone3D_StepLighting` (`0x02014c14`) | `ats_<c>.ambl` | `<nom>.bats` → `LightingInfo` (`Zone3D+0x10c`) ; puis `.amdj` |
| 4 | `Zone3D_StepAmdj` (`0x0201470c`) | `<code>.amdj` | blocs `.bmdj`, parties, instances ; puis `.mse` |
| 5 | `0x0201499c` | `<effets>.mse` | `AtmosphericEffectSet::ProcessArchive` |

Si l'`.ambl` ou un `.amdj` ne se charge pas (statut détaillé ≠ 2), l'étape se termine **sans
mettre en file la suivante** : sans `.ambl`, rien d'autre n'est chargé ; sans `.amdj`, pas de
`.mse`. Un éclairage manquant n'arrête rien (l'étape 3 passe à l'`.amdj` dans tous les cas).

Les membres compressés sont lus par `DecompressLZ77FileIntoScratchSpace` /
`IntoAllocatedSpace` (`0x02075618`, `0x02075664`) : taille = 24 bits hauts du premier mot,
corps toujours décodé en LZ10 quel que soit l'octet de type. Les 755 `.bmdj` et 667 `.bmbl`
commencent tous par `0x10` (LZ10) avec une taille non nulle.

## Noms des archives

`Zone3D_QueueAmbl` (`0x0201404c`) et `Zone3D_QueueAmdj` (`0x020145b8`), dossier `data/map` :

| carte | `.ambl` | `.amdj` |
|---|---|---|
| étage de grotte, ids 40001–40096 (`Zone3D_IsGrottoFloor`, `0x0201b5c0`) | `Z0<e>M01.ambl` | `Z0<e>M01.amdj` |
| autre carte de grotte, ids 41101–41505 (`0x0201b5e8`) | `Z0<e>M99.ambl` | `Z0<e>M99.amdj` |
| océan (10000, `O00`), ciel (10100, `O01`) | `<code>.ambl` | `<code>b.amdj` puis `<code>a.amdj` |
| autres | `<code>.ambl` | `<code>.amdj` |

`<e>` = `ActiveGrottoClass::GetActiveGrottoEnviron` ; pour les étages, 0 compte pour 1 et les
valeurs au-delà de 5 pour 5, pas pour les autres cartes de grotte. Pour l'océan et le ciel,
`Zone3D+0x42c` compte les moitiés mises en file : après `b` (compteur 1) l'étape 4 met `a` en
file au lieu de finir.

Le système de fichiers du SDK compare les chemins **sans tenir compte de la casse**
(`FSi_StrNICmp`) : les codes `S09M03` et `M03M05` n'ont que `S09m03.ambl`/`m03m05.ambl`
(`map::rom_file`).

## Réglages de zone (`Zone3D+0xc`, `ZoneInfo`)

Structure de 0x40 octets (`substruct_c_` de `Zone3D.h`), valeurs par défaut posées par
`SwitchZone`.

| champ | source | sens |
|---|---|---|
| `+0xc` (10 o.) | liste des cartes, valeur 11 | nom de l'éclairage (`C01M0300` pour l'église de Stornway) |
| `+0x16` (16 o.) | `.dat`, type `0x6d` | effets atmosphériques : `data/map/<nom>.mse` (`AtmosphericEffectSet::LoadArchive`) |
| `+0x26` (16 o.) | `.dat`, type `0x68` | un code (`C01`, `T00` sur 150 décors de combat) [inconnu] |
| `+0x36` | `.dat`, type `0x65` ; défaut 0x7fff | [inconnu] |
| `+0x3c` | `.dat`, type `0x66` ; défaut 10 | 21 sur presque toutes les cartes [inconnu] |
| `+0x40` | liste, valeur 13 × 4096 réduite à `[0, 2π]` | **[hypothèse]** orientation sur la carte du monde |
| `+0x44`, `+0x48` | liste, valeurs 14 et 15 (`fx32`) | **[hypothèse]** position sur la carte du monde |

- Liste des cartes : `Zone3D_LoadMapList` (`0x0201c734`) relit `maplist9.bin` avec ses
  propres gestionnaires (`g_ZoneMapListHandlers`, `0x020ef2c0`) ; seul le type `0x67`
  (`0x0201c60c`) agit, sur les enregistrements dont la valeur 0 est l'id courant. Une
  valeur 11 absente laisse le nom vide.
- `.dat` : `Zone3D_LoadDat` (`0x0205e114`, `g_DatHandlers` `0x020f051c`, types `0x64`–`0x6e`,
  les autres sans effet). 657 fichiers ; 15 nomment un `.mse` (les 12 `.mse` de `data/map`).

## `.ambl` : textures, liste de blocs, `.dat`

`Zone3D_StepAmbl` monte l'archive et la parcourt deux fois dans l'ordre des membres ; seul ce
qui suit le dernier `.` du nom compte (`strrchr` puis `strcmp`) :

1. chaque `.nsbtx` devient un `Model3D` nommé (`0x0201446c`), **ajouté en tête** de
   `Zone3D+0x418` : la liste est dans l'ordre inverse de l'archive ;
2. `.bmbl` : vide `Zone3D+0x6c` (`0x0201e258`, 0x88 octets) puis la remplit (`0x020143a0`) ;
   `.dat` : réglages ci-dessus ; `.bpos` : complète `Zone3D+0x6c` sans le vider
   (`0x020143e8`).

Toutes les textures de l'archive sont chargées ; **rien ne choisit parmi elles** : le type
`0x6c` des `.bmbl` (nom de texture, ex. `C01M09T1`) n'a pas de gestionnaire.

### Liste de blocs (`.bmbl`/`.bpos`, `BlockList`)

Gestionnaires `g_BlockListHandlers` (`0x020ef388`). Deux types font la liste des blocs :

| type | gestionnaire | valeurs |
|---|---|---|
| `0x64` | `0x0201c7ac` | n : nouvelle liste de n entrées de 0x58 octets, vide (`0x0201e26c`) |
| `0x65` | `0x0201c7d4` | id ; décalage x, y, z (`fx32`, `ToVec3fix`) ; nom ; [nom 2] ; [nom 3] |

Une entrée sans nom est ignorée ; les autres sont ajoutées tant qu'il reste de la place
(`0x0201e314`). Les noms sont copiés dans des champs de 16 octets (`+0x10`, `+0x20`,
`+0x30`). Sur la ROM : 754 entrées, toutes à décalage nul sauf `F07` (0.018, −0.037, 0.02).

Les autres types de la table ne font pas partie de `BlockList` : `0x66`/`0x67` boîtes,
`0x68`/`0x69` et `0x72` régions, `0x6a`/`0x6b` éléments typés, `0x6e` position + angle
(`+0x70`, `+0x7c`), `0x70` couleur BGR555 (`+0x7e`), `0x73`/`0x74` régions typées et leur
action (portes, voir ci-dessous), `0x7b`/`0x7c` rectangles tournés, `0x7d`/`0x7e` grille des
`.bpos` de grotte. Formes restantes [inconnu].

### Portes et changements de carte

Les gestionnaires de régions (`0x0201d530`, dispatch `0x0201d638`, insertion
`0x0201e710`) gardent les zones par type. Le chargeur du héros (`0x02094b5c`) interroge une
fonction de présence avec sa position (`0x02094b9c`). Les enregistrements lus dans les
`.bmbl` se présentent sous deux formes map-changing :

- `0x72` : centre XYZ (0–2), taille XYZ (3–5), angle (6), code destination (8), arrivée XYZ
  (11–13), orientation (14) ;
- `0x73` type 2 : centre XYZ (1–3), taille XYZ (4–6), angle (7), suivi immédiatement du
  `0x74` d'action : code destination (4), arrivée XYZ (7–9), orientation (10).

Le code de destination est résolu par la liste des cartes ; les cibles sans entrée chargée
sont ignorées. Les autres types de région et les actions `0x74` qui ne désignent pas une
carte ne sont pas interprétés comme des portes.

**[Hypothèse]** L'angle est un radian, la taille est complète, et la présence se teste sur un
rectangle XZ tourné, bords inclus, sans contrainte sur Y. C'est le modèle de
`DoorVolume::contains` ; l'orientation de l'arrivée est transmise au héros et à la caméra.
Le wiki DQIX [Map-Textures](https://github.com/DQIX/wiki/blob/main/Map-Textures.md) a servi
de piste de format ; la géométrie n'est pas encore validée contre une capture de melonDS.

`dq9-app` déclenche la transition au pas fixe quand le héros passe de l'extérieur à l'intérieur
d'une zone ; il charge la carte cible et place le héros à l'arrivée. Les 742 jeux d'archives
assemblés dans `maps_assemble` exposent 1 402 portes non dégénérées avec une cible reconnue.
Le sous-système des triggers d'histoire, les changements de carte déclenchés par scripts, et
la validation différentielle de l'entrée restent hors de ce portage.

Smoke Web : la sauvegarde `build/melonds/dq9.sav` charge la carte 109 (`C01M09`) ; maintenir
`S+D` pendant 2,2 s traverse sa porte et le statut passe à `100 C01` (extérieur de Stornway).
La cible est observée ; le même chemin de chargement reçoit la position et l'orientation
d'arrivée. Ce n'est pas une comparaison différentielle avec melonDS.

## Éclairage (`ats_<c>.ambl`)

`Zone3D_QueueLighting` (`0x02014b14`) : sur un étage de grotte (`Zone3D+0x23b8`), le nom
devient `Z0<e>M0100`, `…01`, `…02`, `…03` selon l'étage (`id % 20` : < 5, < 9, < 13, < 17 ;
au-delà, inchangé). Nom vide : on passe à l'`.amdj`. Sinon `data/map/ats_<1er caractère>.ambl`,
où `Zone3D_StepLighting` cherche `<nom>.bats` (`GetFileInNarc`, `0x02075258` : `strcmp`, donc
sensible à la casse). Le format `.bats` (LZ10, table de records, opcodes `0x64`..`0x6a`) est décrit dans `lighting.md`.

Sur la ROM, 663 jeux d'archives chargent un éclairage ; deux noms manquent (`D07M0800.bats`,
`B10M0200.bats`) : la carte se charge sans.

## `.amdj` : blocs

`Zone3D_StepAmdj` monte l'archive sous le nom `ARC`, puis pour chaque membre `.bmdj` et
chaque entrée de la liste de blocs **dont le nom est contenu dans celui du membre**
(`strstr`) charge un bloc (`Zone3D_LoadBlock`, `0x02014910`) : bloc de 0x58 octets (`+0` id
de l'entrée, `+4` manifeste, `+0x48` décalage de l'entrée, `+0x54` suivant), **ajouté en
tête** de `Zone3D+0x41c`. Un `.bmdj` qui contient deux noms est chargé deux fois ; un `.bmdj`
qu'aucun nom ne désigne est ignoré (seul cas : `M12M0001.bmdj`).

Il construit ensuite les blocs depuis la tête de liste (`Zone3D_BuildBlock`, `0x02014a34`) :
tous, sauf pour la seconde moitié de l'océan/du ciel (compteur 2) où seul le premier l'est.
Enfin, si `Zone3D+0x16` n'est pas vide, il met en file le `.mse`.

### Manifeste (`.bmdj`, `Manifest`)

Table d'enregistrements lue par `Manifest_Parse` (`0x0201f050`) avec `g_BmdjHandlers`
(`0x020ef418`) dans une structure de 0x3c octets :

| type | gestionnaire | valeurs | effet |
|---|---|---|---|
| `0x6a` | `0x0201ebd4` | n | tableau de n ressources de 12 octets (`+0`, capacité `+8`) |
| `0x6c` | `0x0201ec10` | id ; nom ; [drapeaux] ; [textures] | ajoute une ressource si place ; devient la ressource courante (première de cet id) |
| `0x72` | `0x0201ece4` | moment ; nom ; [drapeaux] ; [textures] | variante chaînée à la ressource courante (aucune sur la ROM) |
| `0x6d` | `0x0201edd0` | n | tableau de n placements de 32 octets |
| `0x6f` | `0x0201ee00` | 7, 8–10, 11–13 ou 14 valeurs | ajoute un placement si place |
| `0x70` | `0x0201f000` | nom | lien de texture, 8 emplacements (`+0x18`), sans contrôle de débordement |
| `0x6b`, `0x6e` | rien | | |
| `0x71` | pas de gestionnaire | | ignoré (746 fichiers, un flottant) |

**Ressource** (12 octets) : `+0` id (i16), `+2` drapeaux (valeur 2 & `0x3f`), `+3` liens de
texture (valeur 3 : bit i = lien i), `+4` nom source (`C01M0900.imd`), `+8` variantes.
Drapeaux : `0x01` nsbca, `0x02` nsbma, `0x04` nsbta, `0x08` nsbtp, `0x10` objet animé
(`Object3D`, `.bcfg`), `0x20` pas de teinte (`LightingManager::ModelTransformTintBrightnessContrast`
n'est appliqué qu'aux pièces sans ce bit, quand `Save_Warp(...)+0x20 < 0`).

**Placement** (32 octets) :

| octets | valeur | sens |
|---|---|---|
| `+0` u16 | 0 | numéro d'instance |
| `+2` i16 | 1 | id de la ressource ; négatif : l'emplacement numéroté par l'instance |
| `+4` i16 | 5 | instance parente, négatif : aucune |
| `+6` u16 | 13 (sinon `0xf`) | visibilité, bits 4 et 5 inversés (ci-dessous) |
| `+8` 3 × `fx32` | 2, 3, 4 | translation (× 4096, tronquée) |
| `+0x14` 3 × 16 bits | 10–12 (7–9 si 8 à 10 valeurs) | rotation, radians `fx32` gardés sur 16 bits |
| `+0x1a` 3 × 16 bits | 7–9 si plus de 10 valeurs, sinon 1.0 | échelle `fx32` sur 16 bits |
| — | 6 | chaîne lue puis jetée (le nom du parent dans les données) |

Visibilité (drapeaux stockés) : bit 4 → toujours caché ; sinon visible au moment de la
journée `t` (`LightingManager+0x98`, 0 à 3) si le bit `t` est mis. Les valeurs du fichier
ont donc le bit 4 mis pour « selon l'heure » : 63 → `0xf` partout, 49 → `0x1` seulement au
moment 0, 62 → `0xe` aux moments 1–3. **[hypothèse, wiki Map-Archive]** 0 = nuit : les
pièces `…N1` sont visibles au moment 0, les `…L1` aux autres. Un objet d'overlay
(`func_0x0218b5b0()+0x3718`) peut remplacer le moment utilisé [inconnu].

### Parties (`Part`, 0x18 octets par ressource)

`Zone3D_BuildBlock` construit une partie par emplacement de ressource (`capacité` du type
`0x6a`), puis les instances. Un fichier est cherché dans l'archive montée par
`GetFileFromNARCInMemory("ARC:/<nom>")` (`0x020afd1c`), le nom étant celui de la ressource
dont le texte après le dernier `.` est remplacé (`Zone3D_ArcPath`, `0x02014d28`) ; la
recherche passe par le système de fichiers du SDK, donc sans casse (`m08M0600.imd` →
`M08M0600.nsbmd`, `D13M0203.imd` de même).

| ressource | genre (`+0`) | fichiers |
|---|---|---|
| 4ᵉ caractère du nom `A` | 1 collision (`0x02015144`) | `.col2` ; genre 1 même s'il manque |
| drapeau `0x10` | 2 objet (`0x020151dc`) | `.nsbmd`, puis nsbca/nsbma/nsbta/nsbtp/`.bcfg` selon les drapeaux (`g_ObjectAnimFiles`, `0x020e6f58`) |
| autre | 0 modèle (`0x02014d90`) | `.nsbmd`, puis nsbca/nsbma/nsbta/nsbtp selon les drapeaux (`g_ModelAnimFiles`, `0x020e6fb8`) |

- Modèle : d'abord la variante dont le moment vaut `LightingManager+0x98`, sinon la
  ressource. Sans `.nsbmd`, genre `0xffff`. Toutes les textures de `Zone3D+0x418` lui sont
  appliquées. Source de textures des animations : premier bit des liens (`+3`) dont le nom
  de lien est contenu dans le nom d'un `.nsbtx` de la liste. Sans nsbca, le jeu crée les
  matrices d'os et de matériaux du modèle.
- Les animations sont chargées **seulement si leur bit est mis et si le fichier existe** :
  36 bits désignent un fichier absent (ignorés), et des fichiers présents ne sont pas
  chargés faute de bit (`C01M0900.nsbtp`).
- Objet : chaque instance qui le montre reçoit sa copie (`Object3D::ShallowCloneTo`).

### Instances (`Instance`, 0x70 octets par placement)
1. instance i : numéro = placement.instance (`Zone3D_NodeInit`, `0x02013028`) ;
2. parent : placement.parent négatif → frère de l'instance 0 (sauf l'instance 0 elle-même,
   `0x020130f0`) ; sinon première instance de ce numéro (`0x020130c0`) ; un numéro absent
   laisse l'instance hors de l'arbre ;
3. cachée selon la visibilité ci-dessus ;
4. partie : ressource négative → emplacement `instance` ; sinon première ressource de cet id
   (`0x0201f1c0`) ; une partie de genre 0/1/2 sans fichier n'est pas reliée ;
5. transformations (`Zone3D_PlaceNodes`, `0x0201311c`).
ici [inconnu : usage ailleurs].

### Transformations

`Zone3D_PlaceNodeChain` (`0x02013168`) part de l'instance 0 avec la position nulle, l'angle 0
et l'échelle 1, calcule l'instance, ses frères (chaîne `+0x30`) puis descend dans les enfants
(`+0x2c`) avec la transformation du parent :

- position = `MTX_MultVec33(translation, MTX_RotY33(sin, cos de l'angle du parent))` +
  position du parent (rotation autour de y seulement, translation non mise à l'échelle) ;
- angle = `fix32ReduceAngle0To2Pi(angle du parent + rotation y)` (`0x02030f40`) ;
- échelle = échelle × échelle du parent (`fx32`, arrondie).

`fix32sin`/`fix32cos` (`0x02030c78`, `0x02030cac`) lisent la table du SDK
(`g_fixSinCosTable`, `0x020e9460`) à l'indice `((x << 16) / 2π) >> 4` ; la table est
exactement `round(4096 · sin(2πi/4096))` (test sur la ROM), ce qui permet de la calculer.
`FX_Div`, utilisé par la réduction d'angle, est repris du SDK (arrondi `(q + 2¹⁹) >> 20`).
Sur la ROM, toutes les rotations et échelles valent 0 et 1 : seules les translations et les
parents comptent en pratique. Une instance non atteinte garde la position nulle ; un arbre
qui boucle ferait boucler le jeu (erreur ici).

## Vérification

- **ROM** (`maps_assemble`, `crates/dq9-formats/tests/real_rom.rs`) : les 871 cartes livrées
  (id ≠ 0) s'assemblent ; 796 ont leur `.ambl`. Par jeu d'archives distinct (742, dont les 5
  environnements de grotte) : 667 `.ambl` (tous sauf les 14 `ats_*`) et 669 `.amdj` chargés,
  754 blocs, 5 341 ressources, 5 651 placements, 529 liens de texture ; 4 096 modèles,
  112 objets, 1 133 collisions, **toutes résolues dans leur archive** ; 2 125 animations,
  845 sources de textures ; chaque placement mène à une partie ; 1 288 instances ont un
  parent, aucune n'est hors de l'arbre ; 713 cachées au moment 0 ; 15 `.mse`.
- **RAM** : sur `build/melonds/demo.ml1` (carte 109 `C01M09`, moment 0 lu en
  `0x02107930+0x98`), `cargo dq9re map --state` parcourt `g_Zone3D+0x41c` et compare chaque
  bloc : id d'entrée, ressources (champs et noms), liens de texture, 32 octets de chaque
  placement, genre et emplacements d'animation de chaque partie, numéro, cachée, angle,
  position, échelle, partie et parent de chaque instance : **identiques**. Avec `--time 1`
  la comparaison signale les deux instances dont la visibilité change.
- La table de sinus du SDK est comparée entrée par entrée à celle calculée (test ROM).

## Écarts avec le wiki DQIX

- Map-Textures : le `.bmbl` « nomme les textures de la carte » — le jeu n'en lit pas le
  type `0x6c` et charge tous les `.nsbtx` de l'`.ambl`.
- Map-Objects : « 54 `.bmdj` commencent par le mot 16 » — tous les membres sont des flux LZ10
  de taille non nulle, que le jeu décompresse toujours ; la valeur 6 (comptée depuis 1) nomme
  bien une **instance** ; les valeurs 11–13 sont la rotation (lue, seul y sert), la 14 la
  visibilité par moment de la journée ; la 7 est lue puis jetée.
- Le « mot inconnu » des ressources est le masque des liens de texture (type `0x70`).

## Questions ouvertes

- Sens de `Zone3D+0x26`, `+0x36`, `+0x3c` (`.dat`) et des noms 2 et 3 des entrées de blocs.
- Usage du décalage de bloc (`Block+0x48`) ailleurs que dans `0x020177e4`.
- L'objet d'overlay qui peut remplacer le moment de la journée pour la visibilité.
- Les personnages (`.chr`, PNJ) ne font pas partie de cette chaîne : chargés par d'autres
  étapes (`0x02018f30`, `0x020181fc`, `0x02018ab8`, `0x02017a94` après le chargement) [non lu].
- Bit 5 des drapeaux de placement, et le dernier mot de `g_ModelAnimFiles` (0, 0, 1, 1).
