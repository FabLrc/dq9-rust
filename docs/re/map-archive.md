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
`0x68`/`0x69` et `0x72` régions, `0x6a`/`0x6b` éléments typés, `0x6e` **point de départ du
groupe** (position `fx32` en `BlockList+0x70`, angle en `+0x7c` ; `BlockList::start`, lu par
`Party_Place`, `save.md` § « Position à la reprise »), `0x70` couleur BGR555 (`+0x7e`),
`0x73`/`0x74` régions typées et leur
action (portes, voir ci-dessous), `0x7b`/`0x7c` rectangles tournés, `0x7d`/`0x7e` grille des
`.bpos` de grotte. Formes restantes [inconnu].

### Portes, zones et changements de carte

Les enregistrements `0x72`, `0x73` et `0x74` ne servent pas `BlockList` mais la même structure
(`Zone3D+0x6c`, 0x88 octets, vidée par `BlockList_Clear`) : le `.bmbl` y range aussi les
régions que le champ teste à chaque image. Implémentation : `dq9_formats::map::Regions`
(lecture), `dq9_core::field` (tests et effets) ; `cargo dq9re doors <carte>` affiche les
enregistrements, `--state` les compare à la RAM. Noms : `re/symbols.txt`, section « Map
regions ».

| offset | contenu | écriture |
|---|---|---|
| `+0x18` (nombre `+0x1c`, capacité `+0x20`) | **warps** `0x72`, 0x70 octets, dans l'ordre du fichier | `MapRegion_RecWarp` (`0x0201ce90`), `MapRegion_AddWarp` (`0x0201e584`) |
| `+0x24` (nombre `+0x28`, capacité `+0x2c`) | **zones** `0x73`, 0x74 octets, dans l'ordre du fichier | `MapRegion_RecZone` (`0x0201d540`), `MapRegion_AddZone` (`0x0201e720`) |
| `+0x3c + 4 × type` | tête de la liste des zones d'un type, **la plus récente d'abord** (suivant à `+0x70`) | `MapRegion_AddZone` |

Une zone est ajoutée en tête de sa liste : le jeu parcourt donc les zones d'un type de la
dernière définie à la première, et la première qui convient l'emporte
(`Regions::zones_of`).

**Warp `0x72`** (25 valeurs, 978 sur la ROM dont 976 nomment une carte et 975 une carte de la liste). Les flottants sont
convertis en `fx32` par `ToVec3fix` (× 4096, tronqué) :

| valeurs | sens | RAM |
|---|---|---|
| 0–2 | centre | `+0x58` |
| 3–5 | **taille complète** (largeur, hauteur, profondeur) | boîte `+4` (coin haut) et `+0x10` (coin bas) = centre ± taille / 2 ; `+0x54` = (largeur/2)² + (profondeur/2)² |
| 6 | angle de la boîte, radians, **non réduit** | `+0x52` (16 bits) |
| 7 | type : 0 (935) ; 1 (43) met en file la tâche 8 de la file du champ si `0x02039820` le permet **[hypothèse : porte à animation]** | `+0` |
| 8 | destination : code de la liste des cartes (ou entier = id) | `+0x1c` |
| 9, 10 | −1, 0 ou 1 **[inconnu]** | `+0x64`, `+0x68` |
| 11–13 | position d'arrivée dans la carte de destination | `+0x20` |
| 14 | orientation d'arrivée, radians | `+0x50` (16 bits) |
| 15 | entrée (−1 pour 786 des 978, 0 à 4) **[inconnu]** | `+0x6c` |
| 16–24 | trois vecteurs, copies de l'arrivée sur presque toutes les portes **[inconnu]** | `+0x2c`, `+0x38`, `+0x44` |

**Zone `0x73`** (9 valeurs) : type, centre, taille complète, angle de la boîte (radians,
**réduit** à `[0, 2π]`), orientation (radians, non réduite). Types de la ROM : 2 (670, « porte »
à destination ou non), 3 (22, aire de la carte, 15 cartes), 0, 1, 4, 5, 6, 8, 9, 10, 11, 12
(non décodés ici).

| offset | valeur |
|---|---|
| `+4` | type |
| `+8` | centre |
| `+0x14` | taille complète ; `+0x24` = (largeur/2)² + (profondeur/2)² |
| `+0x22` | angle de la boîte (réduit) |
| `+0x20` | orientation : la direction vers laquelle la porte **fait face**, vers l'extérieur |
| `+0x28` | bits d'exécution (voir `Zone_Holds`), nuls au chargement |

**Action `0x74`** (`MapRegion_RecAction`, `0x0201d648`) : complète **la dernière zone lue**
(pointeur global, même après d'autres enregistrements) selon son type :

- type 3 : la valeur 0 est le numéro de l'aire (`+0x2c`) ;
- type 2 : valeur 0 classe (`+0x2c`), 1 numéro (`+0x2d`), 2 drapeaux (12 bits, `+0x2e >> 4`),
  3 quartet bas de `+0x2e` **[inconnu]** ; avec le drapeau `0x8`, 4 destination (`+0x30`),
  5 et 6 (−1) **[inconnu]**, 7–9 position d'arrivée (`+0x38`), 10 orientation (`+0x68`), 11
  (`+0x6a`), 12–20 trois vecteurs **[inconnu]**, et avec `0x20` un flottant (`+0x6c`). Sur la ROM
  429 portes de type 2 ont une destination.
- Drapeaux des portes présents dans les données : `0x2`, `0x8` (destination), `0x10` (test du
  côté, voir plus bas), `0x20`, `0x40` et leurs sommes (0, 2, 8, 10, 16, 18, 24, 26, 32, 40, 42,
  64, 66, 72, 74, 80, 82, 96, 104 ; deux enregistrements courts portent une valeur aberrante et
  sont lus sans destination). Les bits `0x1` et `0x4` ne sont jamais dans les données : ce sont
  des bits d'exécution (`0x4` = « utilisée », posée par l'action `0x6c`, effacée par `0x88`,
  observé en RAM ci-dessous) ; `0x100` est posé par l'action `0x99`.

#### Tests de présence

Toutes les positions sont des `fx32`, celle du meneur est `Object3D+0x44` de l'objet de
`GameState+0x397c` (`GetUnknownGameObject`).

| fonction | adresse | test |
|---|---|---|
| `Region_InBox` | `0x02031128` | boîte `[haut x y z, bas x y z]`, bords **inclus**, **hauteur testée** |
| `Region_InRotatedBox` | `0x020321f0` | sans angle : `Region_InBox`. Avec un angle non nul : refuse si la distance au sol au centre dépasse `r²` ; sinon tourne le point autour du centre de `-angle` |
| `Region_InCylinder` | `0x02032158` | `y` dans `[cy, cy + h]` et distance au sol ≤ largeur |
| `Zone_Holds` | `0x02094bac` | bit 8 de `+0x28` : jamais ; **profondeur < 1 : cylindre** (rayon = largeur, hauteur = hauteur, à partir de `y` du centre) ; sinon boîte tournée avec l'angle `+0x22` ; bits « une seule fois » `1`/`2`/`4` |

La rotation est celle de `RotationMatrixY(fix32ReduceAngle0To2Pi(-angle))` appliquée par
`MTX_MultVec43`, dont les matrices sont lues sur la ROM (`MTX_RotY43_` : `[c 0 −s; 0 1 0; s 0
c]`, vecteur ligne) : **`x' = x cos a − z sin a`, `z' = x sin a + z cos a`**. C'est l'inverse du
modèle de la première version du portage (`x cos + z sin`), qui était une hypothèse ; la
hauteur est testée (elle était ignorée) et le test porte sur la **présence** à chaque image, pas
sur un franchissement extérieur → intérieur.

#### Ce que le champ fait de chaque région

`ov_017::0219755c` (`Field_CheckRegions`), à chaque image, dans cet ordre pour les types lus ici :

1. **Portes de type 2** (`Field_CheckDoorways`, `ov_017::02197a64`). Une porte compte si ses
   bits `0x1` et `0x4` sont nuls, qu'elle tient le meneur (`Zone_Holds`) et qu'il marche ou
   regarde **contre** son orientation : le produit scalaire de `(sin f, cos f)` avec la
   direction de la marche demandée (ou, à l'arrêt, celle du regard `+0x54`) est ≤ 0 ; avec le
   drapeau `0x10` c'est le côté où se tient le meneur par rapport au centre qui décide. Deux
   portes au plus. Un compteur (`+0x42ec`) monte d'une unité par image où la marche est
   droite (produit scalaire en valeur absolue au-delà de `0xccc − 0x170`, environ 45°) et
   retombe à 0 sinon ; **à 3, l'image suivante déclenche** `Field_TouchDoorways`
   (`ov_017::02198f84`) : événement **17** (mot 7 = classe | numéro << 16, mot 10 =
   orientation), puis, sans bit `0x4`, la porte part vers sa destination avec sa propre
   arrivée, sauf si un trigger a posé un transport (`Save_Warp`), qui la remplace.
2. **Aires de la carte** (type 3, `Area_CheckLeader`, `ov_017::0219814c`) : la première aire
   de la liste qui tient le meneur ; changer d'aire déclenche l'événement 5 (aire quittée) puis
   2 (aire atteinte), mot 1 = numéro (`trigger.md`, « Zones »).
3. **Aires ajoutées par les triggers** (action `0x8f`, `ov_017::02198e30`) : même règle.
4. **Warps `0x72`** (`Field_CheckWarps`, `ov_017::0219ca88`) : le meneur dans la boîte
   (`Region_InRotatedBox`) copie les données du warp dans `Save_Warp` (`Field_WarpFromRegion`,
   `ov_017::0219c6f4` : carte, position, orientation, entrée), met en file la tâche de porte
   si le type vaut 1, applique le transport et déclenche l'événement **6** (mot 3 = la carte
   quittée). Le prédicat que le type 1 teste sur le meneur (`0x02039820`) et la tâche de porte
   (animation, marche automatique, fondu) ne sont pas lus.

#### Zones de type 1 et passage de hauteur de l'Observatoire

Les zones `0x73` de **type 1** (43 sur la ROM, trois sur `X01M01`) ne sont lues par aucune
routine de `Field_CheckRegions` : `ov_017::0219755c` appelle `0x02197e2c`, `021975e4` (type 9),
`02197a64` (portes de type 2), `Area_CheckLeader` (type 3), puis `021982ac` (type 4),
`021983e0` (type 5), `021984f4` (type 8), `02198618` (type 10), `021986fc` et `02198aa0`
(grotte). Aucun de ces appels ne prend la liste de type 1 (`func_0201e838(zone3d+0x6c, 1)`).
`MapRegion_RecAction` (`0x0201d648`) traite bien la valeur 0 d'une action `0x74` après une zone
de type 1 (`0x0201d6ec`) : elle écrit la classe 1 dans le quartet bas de `+0x2c` et un
demi-mot dans `+0x2d`, mais rien ne relit ces champs à l'image. **Les zones de type 1 sont donc
inertes à l'exécution** ; elles ne sont ni un monte-charge ni la transition de hauteur
supposée (la tâche 8 du champ vient du **type de warp** `0x72`, valeur 7 = 1, et non d'une
zone). `X01M01` ne contient d'ailleurs aucun warp de type 1.

Le passage vers `X01M03` est le warp `0x72` **n° 9** de `X01M01` : boîte 4 × 4 × 3 centrée en
`(-8,226 ; 15,5 ; 16,379)`, angle `5,759587`, **type 0**, destination `X01M03`, arrivée
`(-9,75 ; 4,2 ; 17,62)` angle `-2,303835`. L'arrivée est le point de départ de `X01M03`
(`-10,810 ; 4,0 ; 16,910`, `X01M03` a sa propre origine en y) : c'est un atterrissage normal,
pas une destination incohérente.

La terrasse haute est reliée au reste de `X01M01` par la **rampe en spirale** de la collision
`X01A0100`. Depuis la pose d'histoire 1.1.2 (`(2,77 ; 11,25 ; 18,19)`), le seul obstacle direct
est le mur fin `X01A01S1` (bloc 0, placement 12 ; deux triangles en `x 0,018..0,105`,
`z 15,42..18,99`, `y 9..14`, normale `+x`), qui ferme le passage ouest à cette hauteur. Le
chemin praticable contourne son extrémité nord (`z ≈ 20`) puis rejoint le pied de la rampe
vers `(-5,5 ; 11,25 ; 22,0)`, monte par les paliers `(-5,5 ; 19)`, `(-4 ; 12,4 ; 18)`,
`(-6 ; 13,2 ; 16,5)` et atteint `(-8,5 ; 14,13 ; 16)` **dans la boîte du warp**. Chaque arête
de ce trajet a été validée par une marche de 24 images du héros avec `Hero::step` et la
collision de la carte (l'`edge_pass` de la grille du pilote).

Le pilote s'arrêtait pourtant à y 11,25 : ses **sondes dans les huit directions** depuis
y 8,25 et y 11,25 ne relient pas la terrasse à la rampe, et il ne demandait pas le warp
comme but. C'est une limite de la grille du pilote (et de son choix de but), pas une
collision manquante ni une mécanique de jeu absente.

#### Arrivée

La carte de destination se charge, le héros est posé à la position d'arrivée et tourné de
l'angle d'arrivée. **Convention des angles** : l'orientation d'un objet est un `fx32` en
radians et sa direction est `(sin a, 0, cos a)` (0 regarde vers +z). La RAM le confirme : le
héros qui descend (+z) a l'angle 0, celui qui va à gauche 19 544 = 4,77 rad (3π/2 ≈ 4,71), un
angle négatif d'arrivée est réduit à `[0, 2π]` (−2 645 devient 23 091). L'application utilise
`yaw = π − angle` (`field::yaw_of_angle`) ; la caméra de l'arrivée suit le héros, ce n'est pas
vérifié dans le jeu.

`dq9-app` appelle `Field::step` à chaque pas fixe avec la pose du héros et fait ce que
renvoient les événements : changement de carte (chargement, pose d'arrivée, événements 20 puis
3 de la carte), demandes de trigger journalisées comme non prises en charge jusqu'à J3. La
vitesse du héros reste celle du portage du déplacement.

#### Vérification

- **Données** (`maps_assemble`, `dq9-formats/tests/real_rom.rs`) : 742 jeux d'archives
  distincts exposent 975 warps à destination connue, 429 portes de type 2 à destination et 22
  aires de type 3 (chiffres du wiki).
- **RAM** : `cargo dq9re doors --state build/melonds/walk_1.5.ml1` (et `demo`, `before`,
  `moved`, tous en `C01M09`) : le warp lu (type, boîte haut/bas, centre, destination,
  arrivée, angles, `r²`) est **identique** aux 0x70 octets de `g_Zone3D+0x6c+0x18`. Dans
  `C01` (état `walk_1.95`) les 15 zones lues sont identiques aux 0x74 octets de la RAM, sauf
  les drapeaux des deux portes de `C01M14` (numéros 22 et 24) : la RAM les a à 12 et 14, la
  lecture à 8 et 10. C'est le bit « utilisée » que pose l'événement 20 de `triggerC01` (action
  `0x6c`, condition « nuit », la sauvegarde est de nuit) ; `dq9_core::field` le reproduit
  (`stornway_at_night_marks_its_two_locked_doorways`).
- **Arrivées** (`dq9-core/tests/field_rom.rs`) : sur 1 426 arrivées (warps et portes), 4
  tombent dans un warp de la carte de destination avec le test lu dans le code, 25 avec la
  rotation inverse : le sens de la rotation est fixé. Sur 100 arrivées qui tombent dans une
  porte de type 2 de la destination, 96 regardent hors de la porte (produit scalaire avec son
  orientation > 0), ce que la règle « marcher ou regarder contre l'orientation » demande.
  La taille est bien la taille complète : en demi-étendues les arrivées d'Angel Falls
  tomberaient dans la porte de retour.
- **melonDS** : voir la section suivante.

#### Comparaison à melonDS

Sauvegarde de démonstration (`build/melonds/dq9.sav`), scénario 19.2.1, nuit, église de
Stornway (`C01M09`, 109). `scripts/melonds-state.ps1 -Keys` pilote le jeu avec des touches
maintenues (durées en temps réel, **± 2 images** : ce ne sont pas des mesures alignées au
nombre d'images, `movement.md`) ; états `.ml1` relus par `cargo dq9re doors --state`. Il
faut d'abord fermer le dialogue d'accueil du prêtre (A, A, puis B pour le menu).

| état | touches | carte | position du meneur | angle |
|---|---|---|---|---|
| départ | — | 109 | (0,210 ; 0,120 ; −1,380) | 12 861 (π) |
| `walk_1.5` | bas 1,5 s | 109 | (0,339 ; 0,106 ; 7,277) | 0 |
| `walk_1.65` | bas 1,65 s | 109 | (0,339 ; 0,106 ; 7,853) | 0 |
| `walk_1.7` | bas 1,7 s | 109 | (0,339 ; 0,106 ; 8,237) | 0 |
| `walk_1.75` | bas 1,75 s | **100** (`C01`) | **(14,850 ; 2,458 ; −10,050)** | 23 091 (5,637 rad) |
| `walk_1.95` | bas 1,95 s | 100 | identique | identique |
| `door_0.25` | puis haut + droite 0,25 s | 100 | (15,321 ; 2,458 ; −10,561), dans les zones 10 et 11 | 10 431 |
| `door_2.0` | puis haut + droite 2 s | **109** | **(0,200 ; 0,100 ; 7,900)** | 12 869 (π) |

- **Sortie de l'église** (warp `0x72` de type 0, boîte centrée en z 9,268 de profondeur 1,5,
  donc de 8,518 à 10,018) : à z = 8,237 la carte n'a pas changé, 0,05 s plus tard elle a changé.
  Le bord prédit (8,518) est dans l'intervalle ; la lecture en demi-étendues (7,77) est exclue.
  L'arrivée est exactement celle de l'enregistrement (14,85 ; −10,05), l'angle −2 645 réduit
  en 23 091, et `Save_Warp` (`GameState+0x3f8`) porte la carte 100, la position (14,85 ; 2,22 ;
  −10,05), l'angle −2 645, les octets `+7` et `+0xc` à 1 : exactement `Field_WarpFromRegion`.
  La hauteur passe de 2,22 à 2,458 : le jeu pose le héros sur le sol (2,358 + 0,1) ; le
  portage trouve 2,459.
- **Retour par la porte de type 2** (volume centré en (15,475 ; 2,5 ; −11,104), 1,4 × 3 ×
  1,5, tourné de 5,72 rad) : 0,25 s après l'arrivée, le héros est en (15,321 ; −10,561), **dans
  les deux zones** selon `dq9_core::field` (la carte n'a pas encore changé : la tâche de porte
  joue son animation), et 2 s plus tard la carte est 109 à l'arrivée exacte de la porte
  (0,2 ; 0,1 ; 7,9), orientation π. Le même parcours simulé par `dq9-app --walk` :
  `cargo run -p dq9-app --release -- --map C01 --start=14.85,-10.05 --walk "+x-z:1.5"` change de
  carte en (15,369 ; −10,569) vers 109 à (0,200 ; 0,100 ; 7,900), `Doorway { number: 20 }`.
- Hors portée : l'instant exact du déclenchement (les états sont pris après le geste), la vitesse
  du héros (3,8 à 5,8 unités/s selon le tronçon, 4 dans le portage), la durée de la tâche de
  porte, la caméra d'arrivée. Aucun de ces états n'est une comparaison image par image : le
  critère différentiel de J2 n'est pas validé.

Smoke natif d'Angel Falls (`dq9-app`, sans fenêtre) :

```text
> cargo run -p dq9-app --release -- --map M01 --start=-19.5,-9 --walk "-z:3,+z:3,idle:0.5"
start: map 1100 M01 hero (-19.500, -0.800, -9.000) yaw 3.142
map change 1100 -> 1102: Doorway { class: 0, number: 9 } with the hero at (-19.500, -0.150, -12.267), arrival Some(Vec3(0.39990234, 0.15991211, 6.0998535)) angle Some(-3.1418457)
arrived: map 1102 M01M02 hero (0.400, 0.160, 6.100) yaw 6.283
map change 1102 -> 1100: Warp { index: 0, kind: 0 } with the hero at (0.400, 0.158, 7.300), arrival Some(Vec3(-19.569824, -0.099853516, -11.799805)) angle Some(0.0)
arrived: map 1100 M01 hero (-19.570, -0.100, -11.800) yaw 3.142
```

La porte de `M01M02` (zone de type 2, z de −14 à −12) s'ouvre à la quatrième image passée à
l'intérieur (z −12,27), la sortie (warp de type 0, z de 7,25 à 8,75) quand le héros y entre
(z 7,30).

(`angel_falls_houses_open_and_close` dans `dq9-core/tests/field_rom.rs` rejoue ce parcours.)

#### Limites et inconnues

- Tâche de porte (animation, marche et fondu), prédicat `0x02039820` du type 1, gardes de
  `Field_CheckRegions` (`+0x424`, tâche modale), états du meneur `+0xbe` (« en marche », ici :
  une direction est demandée) et touches tenues (ici : la direction demandée).
- Zones de types 0, 1, 4 à 12 (`Field_CheckRegions` en lit plusieurs : `021982ac`, `021983e0`,
  `021984f4`, `02198618`, `021986fc`, `02198aa0`), valeurs 9, 10, 15 et 16–24 des warps,
  valeurs 5, 6, 11 et 12–20 des portes, quartet bas de `+0x2e`, entrée du transport.
- Les bits « une seule fois » de `+0x28` (ce qui les pose est inconnu : aucun enregistrement) ;
  les actions `0x6d`, `0x95`, `0x96`… qui touchent les objets de porte ne sont pas modélisées.

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
