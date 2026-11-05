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
densité packée (6-13, little-endian, 0x20 octets), alpha (14). Le brouillard ne s'applique pas au rendu ici : seul le parseur est porté.

#### Registres de brouillard (ARM9 EU, désassemblage)

Lu dans `build/rom/arm9.bin` (extrait de la ROM du joueur, non committé) et dans Ghidra :

- `func_020c54a4` (`0x020c54b4`) `(enable, type, depthShift, offset)` : si `enable == 0`,
  `DISP3DCNT &= 0xCF7F` (efface l'activation, bit 7). Sinon `FOG_OFFSET = offset` puis
  `DISP3DCNT = DISP3DCNT & 0xC0BF | type << 6 | depthShift << 8 | 0x80`. Pool : `0x0400035C`
  (FOG_OFFSET), `0x04000060` (DISP3DCNT), `0x0000CF7F`.
- `func_020c5574` (`0x020c5584`) `(table)` : appelle `func_020ca50c(table, 0x04000360)`, qui copie
  32 octets (8 mots) : écriture de la table de densité FOG_TABLE (32 entrées).
- Bits de DISP3DCNT : [hypothèse GBATEK] bit 6 = mode couleur (0 alpha seul, 1 couleur + alpha),
  bit 7 = activation, bits 8–11 = décalage de profondeur. Non vérifié à melonDS.
- `FOG_COLOR` (`0x04000358`) : écrit dans les lignes de `ComputeFogInfo` (USA), non relu en EU.
- Le masque `0xCF7F` (désactivation) efface aussi les bits 12–13 : [inconnu], à vérifier.

Validation RAM (savestate melonDS `field.ml1`, `LightingManager` à `0x02107930`, 0x9c octets) :
les valeurs par défaut de `Initialize` (USA) sont présentes à l'identique en EU : offset `0x5800`
à `+0x58`, couleur `0x7FFF` à `+0x5C`, alpha `15` à `+0x60`, `densityTable[i] = 2i` (32 octets) à
`+0x64`. Ces décalages sont [hypothèse] : ils viennent de la valeur, pas d'un type de structure.
Non établi : bits de DISP3DCNT (`GP3D` du savestate, format non documenté dans le dépôt),
donc ni la formule de densité ni l'équation côté GPU. Le brouillard reste non rendu.

### Éclairage avancé (`0x6a`)

Lumière 1 (champs 1-5), lumière 0 (6-10), couleurs (11-17).

Les blocs de base et avancé sont gardés séparément. Le jeu les superpose dans une union
`[inconnu]` : le parseur ne reproduit pas l'aliasing.

## Vérifications

- Test `lighting_scripts_parse` (`crates/dq9-formats/tests/real_rom.rs`) : 504 membres,
  tous décodés par `LightingInfo::parse`.
- Tests synthétiques dans `lighting.rs` : chaque handler, index ignoré, paramètre manquant,
  type invalide, troncature.

## Heure du jour

Indice de l'heure : 0 nuit, 1 matin, 2 jour, 3 soir (wiki Map-Archive). Couleurs en BGR555.
Le gestionnaire d'heure est `LightingManager` `0x02107930`, heure à `+0x98`.
