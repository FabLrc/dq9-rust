# Scripts d'éclairage `.bats`

Source de référence : `crates/dq9-formats/src/lighting.rs`. Sources de décompilation USA
(`tools/dqix-decomp`, commit b5c617a) : `src/Graphics/LightingInfo.cpp`
(`LightingInfo::LoadFromScript` et les handlers de `s_lightingOpcodes`),
`src/Resource/Script.cpp` (table de records), `include/Graphics/LightingInfo.h`.

Adresses EU : `ExecuteLightingScript` `0x0207b938` (taille `0x64`), `LightingInfo::Initialize`
`0x0207b99c`, `Reset` `0x0207b9dc`, `LoadFromScript` `0x0207ba1c`. Appelant : `Zone3D_LoadBats`
`0x02014424`, qui range le résultat dans `Zone3D+0x10c` (cf. `map-archive.md`).

Adresses USA → EU : ARM9 USA `0x0200F3B8..0x0200FB20` → +0x8, `0x0200FB20..0x020EEBD0` → +0x10.

## Conteneur

- Archive `data/map/ats_<c>.ambl` (NARC), membre `<nom>.bats`, compressé en LZ10 (en-tête BIOS
  `0x10`, taille sur 24 bits). Décompressé : 560 à 1072 octets.
- Le corps est une table de records au sens de `table.rs` (`Table::parse`) : en-tête de 16
  octets (nombre de records, décalage du pool, taille du pool, nombre de chaînes), puis les
  records, `0xFF` de bourrage, pool de chaînes.
- Mesure ROM (EU) : 504 membres `.bats` dans les 14 archives `ats_B`..`ats_Z`, tous décodables ;
  6400 records au total, 9 à 16 par script. Pool vide sur les 504 (aucune chaîne).

## Records

Chaque record a un opcode (`kind`) et des champs `INT` (type 1) ou `FLOAT` (type 2). Les
champs de type 0 (chaîne) et 3 sont invalides ici : les scripts lus n'en contiennent pas.

| Opcode | Handler | Fonction USA (`s_lightingOpcodes`, `LightingInfo.cpp`) | Adresse EU | Mesure ROM |
|---|---|---|---|---|
| `0x64` | déclare l'éclairage avancé | `LightingScript_DeclareAdvancedLighting` | `0x0207b2dc` | 236 |
| `0x65` | nop | `LightingScript_Nop` | `0x0207b314` | 0 |
| `0x66` | entrée avancée alternative | `LightingScript_CreateAdvancedLightingEntry_Alternate` | `0x0207b31c` | 0 |
| `0x67` | déclare l'éclairage de base | `LightingScript_DeclareBasicLighting` | `0x0207b4d0` | 268 |
| `0x68` | entrée d'éclairage de base | `LightingScript_CreateBasicLightingEntry` | `0x0207b504` | 715 |
| `0x69` | entrée de brouillard | `LightingScript_CreateFogEntry` | `0x0207b648` | 3529 |
| `0x6a` | entrée avancée | `LightingScript_CreateAdvancedLightingEntry` | `0x0207b78c` | 1652 |

Un opcode inconnu est ignoré.

### Index d'emplacement

Le champ 0 de chaque entrée est l'index d'emplacement, lu dans `0..7` (`SLOTS = 7`). Une valeur
hors de cette plage, négative comprise, est ignorée. Sur la ROM, aucun index n'est hors
bornes. Le code USA indexe sans contrôle pour `0x68` et `0x6a` : comportement non défini, non
reproduit.

### Gradient

`0x64` et `0x67` lisent un décalage de centre de gradient (float, champ 0) si le nombre de
champs est non nul ; sinon il vaut 0.

### Opcode `0x66`

Présent dans la table des handlers USA, absent de la ROM EU (0 occurrence sur 6400 records).
Son layout est reproduit tel que décrit par le code USA, sans vérification sur la ROM.
`[hypothèse]` : le champ 6 est lu puis écarté, comme dans le code USA.

### Éclairage de base (`0x67`, `0x68`)

`0x68` lit la position (champs 1-3), les couleurs de fond et de fond secondaire (4, 5), la
couleur du pot (6), `unk_70` (7), `unk_54` (8), les diffuses sprite et modèle (9, 10) et la
couleur de bord (11) ; la couleur de bord marque l'entrée comme renseignée.
`[hypothèse]` (rapporté par le pair `LightingFogRender`, non vérifié dans les sources lues) : `unk_70` = luminosité, `unk_54` = contraste. Sources : `LightingInfo.cpp` (`0x68`, lignes 101-102), `LightingManager.cpp` (lerp des deux tableaux, l. 216-233). `[inconnu]` : rôle de `positions`.

### Brouillard (`0x69`)

Champs : index (0), activation (1), couleur (2), type (3), profondeur (4), décalage (5), table de
densité packée (6-13, little-endian, 0x20 octets), alpha (14). Application au rendu : § « Rendu et validation à melonDS » plus bas.

#### Registres de brouillard (ARM9 EU, désassemblage)

Lu dans `build/rom/arm9.bin` (extrait de la ROM du joueur, non committé) et dans Ghidra :

- `func_020c54a4` (`0x020c54b4`) `(enable, type, depthShift, offset)` : si `enable == 0`,
  `DISP3DCNT &= 0xCF7F` (efface l'activation, bit 7). Sinon `FOG_OFFSET = offset` puis
  `DISP3DCNT = DISP3DCNT & 0xC0BF | type << 6 | depthShift << 8 | 0x80`. Pool : `0x0400035C`
  (FOG_OFFSET), `0x04000060` (DISP3DCNT), `0x0000CF7F`.
- `func_020c5574` (`0x020c5584`) `(table)` : appelle `func_020ca50c(table, 0x04000360)`, qui copie
  32 octets (8 mots) : écriture de la table de densité FOG_TABLE (32 entrées).
- Bits de DISP3DCNT (GBATEK, confirmés par la source de melonDS, `GPU3D_Soft.cpp`
  `ScanlineFinalPass`) : bit 6 = mode (0 couleur + alpha, **1 alpha seul**), bit 7 = activation,
  bits 8–11 = décalage de profondeur. Le jeu écrit `type << 6` : `type` 0 = couleur et alpha, 1 =
  alpha seul, comme dans `FogInfo` USA.
- `FOG_COLOR` (`0x04000358`) : écrit `color | alpha << 16` par `ProcessZoneChange`
  (`0x020519f4`, +408) et `RecomputeAdvancedLighting` (`0x02051d38`, +1272) ; vérifié par
  xrefs Ghidra EU (écritures à `0x04000358` : ces deux fonctions et `func_020c51dc`).
- Le masque `0xCF7F` (désactivation) efface aussi les bits 12–13 ([inconnu], sans effet connu).
- `DISP3DCNT` dans les états melonDS `field.ml1`, `demo.ml1`, `before.ml1`, `moved.ml1`
  (section `GP3D`, voir § suivant) : `0x1b9` = textures (bit 0), mélange alpha (3),
  anticrénelage (4), **marquage des contours (5)**, brouillard activé (7), décalage 1 ; ni
  toon ni surbrillance (bit 1 à 0) : les données n'emploient pas le toon.

Validation RAM (savestate melonDS `field.ml1`, `LightingManager` à `0x02107930`, 0x9c octets) :
les valeurs par défaut de `Initialize` (USA) sont présentes à l'identique en EU : offset `0x5800`
à `+0x58`, couleur `0x7FFF` à `+0x5C`, alpha `15` à `+0x60`, `densityTable[i] = 2i` (32 octets) à
`+0x64`. Ces décalages sont [hypothèse] : ils viennent de la valeur, pas d'un type de structure.
Le champ `GP3D` du savestate se lit en suivant `GPU3D::DoSavestate` de melonDS (code GPL lu pour
comprendre le format, rien de copié) : `DISP3DCNT` (u32), tables toon (64 octets) et contours
(16 octets), `FOG_COLOR`, `FOG_OFFSET`, `FOG_TABLE` (32 octets). Dans `field.ml1` (`C01M09`) :
`EDGE_COLOR` = `0x0466` ×8, `FOG_COLOR` = `0x001f7fff`, `FOG_OFFSET` = `0x5800`, table nulle,
décalage 1 : exactement l'entrée 0 de `C01M0300.bats` (contour `0x466`, brouillard `offset 22528`,
couleur `0x7fff`, alpha 31, table nulle, décalage 1), brouillard activé même si le champ
« activé » du script est faux (`ProcessZoneChange` passe `true`).

### Éclairage avancé (`0x6a`)

Lumière 1 (champs 1-5), lumière 0 (6-10), couleurs (11-17).

Les blocs de base et avancé sont gardés séparément. Le jeu les superpose dans une union
`[inconnu]` : le parseur ne reproduit pas l'aliasing.

## Vérifications

- Test `lighting_scripts_parse` (`crates/dq9-formats/tests/real_rom.rs`) : 504 membres,
  tous décodés par `LightingInfo::parse`.
- Tests synthétiques dans `lighting.rs` : chaque handler, index ignoré, paramètre manquant,
  type invalide, troncature.

## Heure du jour et choix de l'entrée

Module `dq9_core::lighting`. Sources : `GameTime.cpp`, `LightingManager.cpp`, `AtmosphericEffect.cpp`
(décompilation USA), contrôlées sur le code EU (Ghidra) et la RAM d'un état melonDS.

### L'horloge

- `GameState+0x3cc` (EU `0x020f37a4`) : secondes, flottant ; `+0x3d0` : longueur du jour,
  `420.0` ; `+0x3d4` : `1/60` par tick (RAM : `0x3c888889`) ; `+0x3d8` : en marche ; `+0x3dc` :
  phase. `AdvanceDayTimer` (`0x02010250`) ajoute `ticks * 1/60` si l'horloge court ; `SetDayTimer`
  (`0x02010298`) replie le temps sur `0..420` puis choisit la phase ; `SetTimeOfDay`
  (`0x02010374`) met le temps au début de la phase.
- Seuils lus en RAM EU (`0x020f33bc`) : `420`, `390`, `210`, `180` : nuit `0..180`, matin
  `180..210`, jour `210..390`, soir `390..420`. La table USA est indexée `[2]`, `[3]`, `[4]` pour
  les mêmes valeurs ; l'indexation EU n'est pas relue instruction par instruction **[hypothèse]**,
  mais les valeurs sont celles du wiki Time-Of-Day.
- `ov_017::0218d0ec` (une fois par image de jeu) : appelle `AdvanceDayTimer` seulement sur une
  carte de type `0` ou `7` (champ, océan, ciel), puis recopie `GetDayTimer` et `GetTimeOfDay` dans
  `LightingManager+0x94` et `+0x98` (sauf pendant certains modes : valeurs de `func_02046b24` 3,
  4, 10 et 0x16 **[inconnu]** : le code n'est pas nommé). Ce même code lève le drapeau d'histoire
  `0x1140` quand la phase passe à matin ou nuit.
- Chargement d'une sauvegarde (`func_020aaf84`, `0x020aaf94`+780) : `SetDayTimer(temps lu)`, puis
  recopie dans `+0x94` / `+0x98`, `SetZone`, `RecomputeAdvancedLighting`.
- Dans notre port la phase est une **entrée explicite** (`--time night|morning|day|evening`,
  `--clock <seconde>`, liste déroulante Web, touche `T`) : `DayClock` modélise l'horloge, le
  RTC de la DS n'intervient pas. Le jeu ne lit pas le RTC pour la phase : seule l'horloge de
  `GameState` compte (le wiki Time-Of-Day le confirme : 420 s par jour, nouvelle partie à 210 s).

### Choix de l'entrée d'une carte

- Le script `.bats` est choisi par `Zone3D_QueueLighting` / `Zone3D_StepLighting` (nom de la
  carte, `map-archive.md`) et rangé dans `Zone3D+0x10c` (`LightingInfo`). `maybeMode_` : `1` base,
  `2` avancé, `0` aucun.
- Indice d'emplacement (`LightingManager+0x90`, remplacement ; `0` = non utilisé) : s'il est
  non nul il indexe toutes les tables au lieu de la phase. **[inconnu]** : jamais observé non nul
  (`LightingManager.h` USA) ; reproduit mais non utilisé par l'application.
- **Mode avancé** (champs, champs de bataille ; 236 scripts) : `ProcessZoneChange` complète
  les entrées manquantes (`FillMissingEntries` des blocs avancé et brouillard, `0x020504b4`,
  `0x02050840`), puis `RecomputeAdvancedLighting` (`0x02051d38`, chaque image) appelle
  `GetCurrentAdvancedLightingValues` (`0x02050c30`) : phase courante `i`, suivante `(i+1)%4`,
  `t = (temps − (fin de phase i − 2 s)) / 2 s` borné à `[0, 1]` (fin de phase : 180, 210, 390, 420).
  Couleurs mélangées canal par canal en `char` tronqué (`InterpolateColors`, `0x02051160`), directions
  mélangées linéairement. Le brouillard suit `ComputeFogInfo` (`0x02051254`, même `t`, multiplié par
  l'échelle de lumière) : couleur, alpha, décalage et table mélangés, type et décalage de profondeur
  de la phase courante. Les directions des lumières sont arrondies à 1.19.12 (demi vers l'extérieur),
  normalisées par `VEC_Normalize` (`0x020c2f28`), plafonnées à `0xff7`, puis `SetLightVector`
  garde les bits 3..12 : vecteur signé 1.0.9 (`dq9_core::lighting::light_vector`). Les lumières 2 et
  3 sont noires ; les lumières 0 et 1 ont la couleur interpolée.
- **Mode de base** (villages, bâtiments, grottes ; 268 scripts) : `ProcessZoneChange` (`0x020519f4`) lit
  l'entrée de la **phase** sans mélange. Si le temps dépasse `fin − 4 s`, le gestionnaire passe à la
  phase suivante et **règle l'horloge de `GameState` sur son début** (`SetDayTimer`) :
  `settle_basic_entry`. Il écrit le brouillard de l'entrée (`func_020c54a4(true, type, shift, offset)`,
  `FOG_COLOR`, `func_020c5574(table)`), la couleur de contour, `SetDiffuseAmbientColors(model, 0)` ;
  les lumières restent noires. Les modèles dont les maillages envoient des couleurs
  (`mesh+4 & 2`) sont retouchés une fois au chargement par
  `ModelTransformTintBrightnessContrast` (`0x020516a0`) sauf si la ressource porte le drapeau 0x20 :
  `ColorTransformTintBrightnessContrast` (`0x02050980`) multiplie chaque canal 5 bits par
  `positions[i]` (`func_0207ba28`, EU `0x0207ba38`), ajoute la « luminosité » `unk_70`, puis le
  « contraste » `unk_54³` (ajouté au-dessus de 0,5, retranché en dessous, sans franchir 0,5).
  **Vérifié en RAM** (`field.ml1`, `LightingManager+0x2c..0x3c`) : `0.81`, `1.16`, `2.0`,
  luminosité `0.03`, contraste `0.017576` = `0.26³` ; ce sont les champs `unk_0`, `unk_70` et `unk_54`
  de l'entrée 0 de `C01M0300.bats` (le script de `C01M09`), lus par notre parseur.
- Le fond : `DrawBackgroundGradient` (`0x02052420`) : mode de base, couleurs `background` /
  `background_second` de la phase ; mode avancé, couleurs interpolées. Dégradé vertical dont la
  couleur d'horizon est sur la ligne où tombe un point à 2048 unités dans la direction horizontale
  de la caméra (`MaybeComputeHorizonPosition`, `0x02052360`), plus `gradientCenterOffset_`.
- Le jeu superpose `Fog::OFF` quand `maybeMode_` n'est ni 1 ni 2 (carte sans `.bats`).

### Non modélisé

`BeginFade` (échelle de lumière `lightRGBScale_`, pondère couleurs des lumières et fond ; l'API
`Lighting::select` la prend en paramètre, l'application passe 1.0), indice de remplacement non
nul, mode forcé de `ov_017` (`1 << 9` : phase de bataille lue en `+5`), l'entrée 4 du wagon du
Starflight Express (`zone 6401`), couleurs `ambient` des PNJ (`ApplyAmbientColorToModel` n'est
appliquée qu'aux modèles de terrain d'un monde avancé, cf. `func_02016874`), effets de brillance (table
`SetShininessTable`).

## Rendu et validation à melonDS

Implémentation : `dq9_render::lighting` (`Environment`), `shader.wgsl`, `background.wgsl`,
`edge.wgsl`. `dq9-app::lighting` relie `dq9_core::lighting::Lighting` au rendu.

| Élément | Source | État |
|---|---|---|
| lumières DS (équation GBATEK / melonDS `CalculateLighting`), normales des modèles de carte, `DIF_AMB`, `SPE_EMI`, bits 0-3 de `POLYGON_ATTR` | `LightingManager::SubmitToRenderConfig` `0x02052254` | appliqué par sommet, **non comparé à melonDS** (aucun état melonDS en zone avancée disponible ; en zone de base les lumières sont noires) |
| teinte de base (couleurs de sommets des maillages `mesh+4 & 2`) | `0x02050980`, `0x020516a0` | appliquée, valeurs d'entrée vérifiées en RAM ; image **comparée à melonDS** sur `C01M09` (ci-dessous) |
| diffuse des sprites et des personnages | `spriteDiffuseColor_`, `modelDiffuseColor_` | multiplie leur couleur ; couleur de la carte de base lue de `.bats` |
| fond | `DrawBackgroundGradient` | dégradé appliqué ; **non comparé** (la caméra de `C01M09` ne voit pas le fond) |
| brouillard | `func_020c54a4`, `FOG_COLOR`, `FOG_TABLE`, équation melonDS | **validé à melonDS** sur la densité (ci-dessous), mode alpha seul non validé |
| marquage des contours | `DISP3DCNT` bit 5, `EDGE_COLOR` | appliqué (`edge.wgsl`, règle de melonDS : voisin d'identifiant de polygone différent et plus éloigné), validé à l'œil seulement sur `C01M09` ; sur `M01` il cerne des taches de sol d'identifiant 1 à 6, tracé instable car décalques coplanaires (comportement DS non vérifié). Largeur : un pixel DS (`--ds-aspect`), sinon 1 pixel écran |
| toon, surbrillance, anticrénelage | bits 1 et 4 de `DISP3DCNT` | toon/surbrillance : bit 1 à 0, donc inutile ; anticrénelage non fait |

Équation de brouillard (melonDS `CalculateFogDensity`, vérifiée sur matériel par ses auteurs) :
`z` = profondeur 24 bits du tampon Z (`((z_clip * 0x4000) / w + 0x3fff) * 0x200`) ; si
`z < FOG_OFFSET * 0x200` densité = `table[0]`, sinon `zz = ((z − offset) >> 2) << DISP3DCNT[8..11]`, indice
`zz >> 17` (32 au plus), fraction `zz & 0x1ffff`, interpolation entre les entrées `i−1` et `i` de la
table où l'entrée 0 est doublée et la dernière répétée (copie interne de 34 valeurs), densité plafonnée à
128 (≥ 127 → 128) ; `couleur = (fog * d + pixel * (128 − d)) >> 7` avec le fog 5 bits élargi
à 6 bits (`2c + 1`, `0` reste `0`). Seuls les polygones dont `POLYGON_ATTR` bit 15 est levé sont
embrumés ; la cible de comparaison (C01M09) n'en a que pour les sprites.

### Comparaison à melonDS (état `field.ml1`, `C01M09` de nuit)

`scripts/melonds-capture.ps1` charge un état melonDS dans la fenêtre portable et enregistre l'écran
(`build/scratch/ref-*.png`, 256×192). Notre image : `--map C01M09 --time night --chapter 19
--step 2 --substep 1 --night --eye=0.21,6.87,7.84 --target=0.21,0.87,-1.38 --size 256x192`
(caméra lue en RAM de l'état, `0x027e3950`, la projection du jeu).

- **Image sans brouillard forcé** (équation de teinte et contours) : différence absolue moyenne
  (hors héros et fenêtre de dialogue, 256×105 pixels) **12,8 / 255** par canal (R 12,8, V 13,8, B 11,7) ;
  médiane 7 ; 43 % des pixels à moins de 8. Notre rendu est en moyenne 3 à 4 niveaux plus sombre.
  Écarts connus : pas de filtrage de texture ni d'arrondi à 6 bits comme la DS, héros remplacé
  par un repère, contours et ombres en pixels de 256×192 suréchantillonnés.
- **Brouillard** : l'état est modifié (`FOG_COLOR`, `FOG_OFFSET`, `FOG_TABLE`, décalage de `DISP3DCNT`
  écrits dans la section `GP3D` avant le chargement) et l'image melonDS comparée à la nôtre avec
  `--fog OFFSET,SHIFT,COLOR,D0..D31` (mêmes valeurs). La densité effective (part de la couleur de
  brouillard sur les pixels du PNJ, estimée par moindres carrés avec la couleur sans brouillard) :

| table | offset / décalage / couleur | melonDS | nous |
|---|---|---|---|
| constante 64 | `0x5800` / 1 / bleu | 0,506 | 0,498 |
| `3i` | `0x5a00` / 2 / vert | 0,632 | 0,631 |
| `127 − 4i` | `0x5400` / 0 / jaune | 0,750 | 0,745 |
| `5i` | `0x6000` / 2 / cyan | 0,822 | 0,828 |
| `4i` | `0` / 0 / magenta | 0,878 | 0,891 |

  La formule (profondeur, offset, décalage, interpolation, mélange à 128) concorde à 0,01 près sur
  un point de profondeur (le PNJ) : **pas de validation sur terrain** (aucun matériau de `C01M09`
  n'est brumeux), **ni du mode alpha seul** ni de l'alpha de `FOG_COLOR`.
- Non fait : zone avancée (lumières, dégradé de ciel, fondu de 2 s) comparée à melonDS : il faudrait
  un état melonDS sur un champ ; les états existants sont tous à `C01M09`.
